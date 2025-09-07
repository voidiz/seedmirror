use std::{
    io::ErrorKind,
    path::{self, PathBuf},
};

use anyhow::Context;
use seedmirror_core::message::Message;
use tokio::{
    fs::remove_file,
    io::AsyncWriteExt,
    net::{UnixListener, UnixStream},
    sync::broadcast,
};

pub(crate) struct ConnectionManager {
    rx: broadcast::Receiver<Message>,
}

impl ConnectionManager {
    pub(crate) fn new() -> (Self, broadcast::Sender<Message>) {
        let (tx, rx) = broadcast::channel::<Message>(100);
        (Self { rx }, tx)
    }

    pub(crate) async fn start(self, root_path: PathBuf, socket_path: PathBuf) {
        if let Err(e) = self.start_inner(root_path, socket_path).await {
            log::error!("error starting connection manager: {e:#}");
        }
    }

    async fn start_inner(self, root_path: PathBuf, socket_path: PathBuf) -> anyhow::Result<()> {
        if socket_path.try_exists()? {
            remove_file(&socket_path)
                .await
                .with_context(|| format!("failed to remove existing socket: {socket_path:?}"))?;
        }

        let listener = UnixListener::bind(&socket_path)
            .with_context(|| format!("failed to listen to socket at {socket_path:?}"))?;

        let absolute_root_path = path::absolute(&root_path)
            .with_context(|| format!("failed to resolve root path: {root_path:?}"))?;

        loop {
            // TODO: Exchange version information to ensure client and server match
            match listener.accept().await {
                Ok((stream, _addr)) => {
                    tokio::spawn(connection_handler(
                        absolute_root_path.clone(),
                        self.rx.resubscribe(),
                        stream,
                    ));
                }
                Err(e) => {
                    log::error!("failed to accept incoming connection: {e:#}");
                }
            }
        }
    }
}

async fn connection_handler(
    root_path: PathBuf,
    rx: broadcast::Receiver<Message>,
    stream: UnixStream,
) {
    if let Err(e) = connection_handler_inner(root_path, rx, stream).await {
        log::error!("connection handler failed: {e:#}");
    }
}

async fn connection_handler_inner(
    root_path: PathBuf,
    mut rx: broadcast::Receiver<Message>,
    mut stream: UnixStream,
) -> anyhow::Result<()> {
    log::info!("established connection with client");
    write_message(&mut stream, Message::Connected { root_path }).await?;
    loop {
        match rx.recv().await {
            Ok(msg) => {
                if write_message(&mut stream, msg).await? {
                    return Ok(());
                }
            }
            Err(broadcast::error::RecvError::Lagged(skipped)) => {
                log::warn!("receiving too many filesystem events, skipping {skipped} event(s)");
            }
            Err(e) => anyhow::bail!("recv on filesystem event broadcast channel failed: {e:#}"),
        }
    }
}

/// Returns true if the connection is broken and should be terminated.
async fn write_message(stream: &mut UnixStream, msg: Message) -> anyhow::Result<bool> {
    let json = format!("{}\n", serde_json::to_string_pretty(&msg)?);
    let content_length = json.len();
    let payload = format!("{content_length}\n{json}");
    let write_result = stream.write_all(payload.as_bytes()).await;

    if let Err(e) = write_result {
        match e.kind() {
            ErrorKind::BrokenPipe => {
                log::info!("connection to client broken, terminating it...");
                return Ok(true);
            }
            _ => {
                return Err(anyhow::anyhow!(e).context("failed writing to socket"));
            }
        }
    }

    Ok(false)
}
