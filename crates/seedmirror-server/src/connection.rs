use std::path::PathBuf;

use anyhow::Context;
use notify::{RecursiveMode, Watcher};
use seedmirror_core::message::Message;
use tokio::{
    fs::remove_file,
    io::BufReader,
    net::{UnixListener, UnixStream},
    sync::broadcast,
    task::JoinSet,
};

use crate::{informer, watcher};

pub(crate) async fn connection_manager(socket_path: PathBuf) {
    if let Err(e) = connection_manager_inner(socket_path).await {
        log::error!("error starting connection manager: {e:#}");
    }
}

async fn connection_manager_inner(socket_path: PathBuf) -> anyhow::Result<()> {
    if socket_path.try_exists()? {
        remove_file(&socket_path)
            .await
            .with_context(|| format!("failed to remove existing socket: {socket_path:?}"))?;
    }

    let listener = UnixListener::bind(&socket_path)
        .with_context(|| format!("failed to listen to socket at {socket_path:?}"))?;

    loop {
        match listener.accept().await {
            Ok((stream, _addr)) => {
                tokio::spawn(connection_handler(stream));
            }
            Err(e) => {
                log::error!("failed to accept incoming connection: {e:#}");
            }
        }
    }
}

async fn connection_handler(stream: UnixStream) {
    if let Err(e) = connection_handler_inner(stream).await {
        log::error!("connection handler failed: {e:#}");
    }
}

async fn connection_handler_inner(mut stream: UnixStream) -> anyhow::Result<()> {
    log::info!("established socket connection with client");

    let (server_msg_tx, mut server_msg_rx) = broadcast::channel::<Message>(100);

    // Watcher will be shut down on drop
    let (mut watcher, notify_rx) = watcher::create_watcher().await?;

    let mut set = JoinSet::new();
    set.spawn(informer::notify_handler(notify_rx, server_msg_tx));

    loop {
        tokio::select! {
            res = server_msg_rx.recv() => {
                match handle_server_msg(res, &mut stream).await {
                    Ok(true) => break,
                    Ok(false) => (),
                    Err(e) => anyhow::bail!(e),
                };
            }
            Ok(_) = stream.readable() => {
                match handle_client_msg(&mut watcher, &mut stream).await {
                    Ok(true) => break,
                    Ok(false) => (),
                    Err(e) => anyhow::bail!(e),
                };
            }
        }
    }

    log::info!("connection broken, terminating it...");
    Ok(())
}

/// Returns true if the connection should be terminated.
async fn handle_server_msg(
    res: Result<Message, broadcast::error::RecvError>,
    stream: &mut UnixStream,
) -> anyhow::Result<bool> {
    match res {
        Ok(msg) => {
            if msg.write_to_stream(stream).await? {
                return Ok(true);
            }
        }
        Err(broadcast::error::RecvError::Lagged(skipped)) => {
            log::warn!("receiving too many filesystem events, skipping {skipped} event(s)");
        }
        Err(e) => anyhow::bail!("recv on filesystem event broadcast channel failed: {e:#}"),
    }

    Ok(false)
}

/// Returns true if the connection should be terminated.
async fn handle_client_msg(
    watcher: &mut impl Watcher,
    stream: &mut UnixStream,
) -> anyhow::Result<bool> {
    let (read_stream, mut write_stream) = tokio::io::split(&mut *stream);

    let mut reader = BufReader::new(read_stream);
    let msg = match Message::read_from_reader(&mut reader).await {
        Ok(msg) => msg,
        Err(e) => {
            log::debug!(
                "error when reading message from client (probably due to terminated connection): {e:#}"
            );

            return Ok(true);
        }
    };

    #[allow(clippy::single_match)]
    match msg {
        // TODO: Exchange version information to ensure client and server match
        Message::ConnectionRequest { watched_paths } => {
            for path in watched_paths {
                watcher.watch(&path, RecursiveMode::Recursive)?;
            }

            Message::Connected
                .write_to_stream(&mut write_stream)
                .await?;
        }
        _ => (),
    }

    Ok(false)
}
