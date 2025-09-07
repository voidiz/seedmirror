use std::{
    collections::HashMap,
    io::ErrorKind,
    path::{self, PathBuf},
    time::Duration,
};

use anyhow::Context;
use notify::Event;
use seedmirror_core::message::Message;
use tokio::{
    fs::remove_file,
    io::AsyncWriteExt,
    net::{UnixListener, UnixStream},
    sync::broadcast,
    task::JoinHandle,
    time::sleep,
};

use crate::watcher::NotifyEventReceiver;

struct NotifyHandler {
    /// Root path being watched.
    root_path: PathBuf,

    /// Channel for incoming filesystem events.
    notify_rx: NotifyEventReceiver,

    /// Broadcast channel used to inform clients of updated files.
    msg_tx: broadcast::Sender<Message>,

    /// Ongoing event handlers.
    events: HashMap<Message, JoinHandle<()>>,
}

impl NotifyHandler {
    fn new(
        root_path: PathBuf,
        notify_rx: NotifyEventReceiver,
        msg_tx: broadcast::Sender<Message>,
    ) -> Self {
        Self {
            root_path,
            notify_rx,
            msg_tx,
            events: HashMap::new(),
        }
    }

    async fn handle(mut self) -> anyhow::Result<()> {
        let mut msg_rx = self.msg_tx.subscribe();

        loop {
            tokio::select! {
                Some(res) = self.notify_rx.recv() => {
                    match res {
                        Ok(event) => {
                            self.process_event(&event)
                                .with_context(|| format!("failed to process filesystem event: {event:?}"))?;
                        },
                        Err(e) => {
                            anyhow::bail!(e);
                        }
                    }
                },
                Ok(msg) = msg_rx.recv() => {
                    self.events.remove(&msg);
                }
            }
        }
    }

    fn process_event(&mut self, event: &Event) -> anyhow::Result<()> {
        log::debug!("received filesystem event: {event:?}");

        let absolute_root = path::absolute(&self.root_path)
            .with_context(|| format!("failed to resolve root path: {:?}", self.root_path))?;

        for path in &event.paths {
            let absolute_path = path::absolute(path)
                .with_context(|| format!("failed to resolve path: {path:?}"))?;
            let relative_to_root = absolute_path.strip_prefix(&absolute_root)?;

            #[allow(clippy::single_match)]
            match event.kind {
                notify::EventKind::Modify(_) => {
                    let msg = Message::FileUpdated {
                        path: relative_to_root.to_owned(),
                    };
                    self.queue_message(msg);
                }
                _ => (),
            };
        }

        Ok(())
    }

    fn queue_message(&mut self, msg: Message) {
        if let Some(handle) = self.events.remove(&msg) {
            handle.abort();
        }

        let msg_tx = self.msg_tx.clone();
        self.events.insert(
            msg.clone(),
            tokio::spawn(async move {
                sleep(Duration::from_secs(10)).await;

                // Spawn a separate task so it can't be canceled
                tokio::spawn(async move {
                    let inner = || -> anyhow::Result<()> {
                        log::info!("broadcasting message: {msg:?}");
                        msg_tx.send(msg.clone())?;
                        Ok(())
                    };

                    if let Err(e) = inner() {
                        log::error!("failed to send message: {e:#}");
                    }
                });
            }),
        );
    }
}

pub(crate) async fn notify_handler(
    root_path: PathBuf,
    rx: NotifyEventReceiver,
    tx: broadcast::Sender<Message>,
) {
    let state = NotifyHandler::new(root_path, rx, tx);
    if let Err(e) = state.handle().await {
        log::error!("error in filesystem event handler: {e:#}");
    }
}

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
