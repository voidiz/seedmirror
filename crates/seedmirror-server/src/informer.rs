use std::{
    collections::HashMap,
    path::{self, Path, PathBuf},
    time::UNIX_EPOCH,
};

use anyhow::Context;
use notify::Event;
use seedmirror_core::message::FileMeta;
use tokio::{sync::broadcast, task::JoinHandle, time::sleep};

use crate::{cli::Args, watcher::NotifyEventReceiver};

#[derive(Clone, Debug)]
pub(crate) enum BroadcastMessage {
    FileUpdated { meta: FileMeta },
}

struct NotifyHandler {
    args: Args,

    /// Channel for incoming filesystem events.
    notify_rx: NotifyEventReceiver,

    /// Broadcast channel used to inform clients of updated files.
    server_msg_tx: broadcast::Sender<BroadcastMessage>,

    /// Ongoing event handlers for file updates.
    event_handlers: HashMap<PathBuf, JoinHandle<()>>,
}

impl NotifyHandler {
    fn new(
        args: Args,
        notify_rx: NotifyEventReceiver,
        server_msg_tx: broadcast::Sender<BroadcastMessage>,
    ) -> Self {
        Self {
            args,
            notify_rx,
            server_msg_tx,
            event_handlers: HashMap::new(),
        }
    }

    async fn handle(mut self) -> anyhow::Result<()> {
        log::debug!("started notify handler");
        let mut msg_rx = self.server_msg_tx.subscribe();

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
                    // Clean up the event handler when the message has been sent
                    let BroadcastMessage::FileUpdated { meta } = msg;
                    self.event_handlers.remove(&meta.path);
                }
            }
        }
    }

    fn process_event(&mut self, event: &Event) -> anyhow::Result<()> {
        log::debug!("received filesystem event: {event:?}");

        for path in &event.paths {
            let absolute_path = path::absolute(path)
                .with_context(|| format!("failed to resolve path: {path:?}"))?;

            #[allow(clippy::single_match)]
            match event.kind {
                notify::EventKind::Create(_) | notify::EventKind::Modify(_) => {
                    let meta = build_file_meta(&absolute_path).with_context(|| {
                        format!("failed to stat updated path: {absolute_path:?}")
                    })?;

                    let msg = BroadcastMessage::FileUpdated { meta };
                    self.queue_notify_message(&absolute_path, msg);
                }
                notify::EventKind::Remove(_) => {
                    self.abort_event_handler(&absolute_path);
                }
                _ => (),
            };
        }

        Ok(())
    }

    fn queue_notify_message(&mut self, path: &Path, msg: BroadcastMessage) {
        self.abort_event_handler(path);

        let sync_delay = self.args.sync_delay;
        let msg_tx = self.server_msg_tx.clone();
        self.event_handlers.insert(
            path.to_path_buf(),
            tokio::spawn(async move {
                sleep(sync_delay).await;

                // Spawn a separate task so it can't be canceled. Currently there aren't any yield
                // points, so it isn't strictly necessary right now.
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

    fn abort_event_handler(&mut self, path: &Path) {
        if let Some(handle) = self.event_handlers.remove(path) {
            handle.abort();
        }
    }
}

fn build_file_meta(absolute_path: &Path) -> anyhow::Result<FileMeta> {
    let md = std::fs::metadata(absolute_path)
        .with_context(|| format!("failed to stat path: {absolute_path:?}"))?;

    let mut path = absolute_path.to_path_buf();

    // Push an empty component to the path to add a trailing slash. This is
    // important for rsync to treat it as a directory so that
    // `rsync <src dir> <dst dir>`
    // synchronizes the state of `<src dir>` with `<dst dir>` instead of placing
    // `<src dir>` inside `<dst dir>`.
    // TODO: remove once rsync is replaced by the fetch protocol.
    if md.is_dir() {
        path.push("");
    }

    let mtime_nanos = md
        .modified()
        .with_context(|| format!("failed to read mtime: {absolute_path:?}"))?
        .duration_since(UNIX_EPOCH)
        .map_err(|e| anyhow::anyhow!("mtime before epoch: {e:?}"))?
        .as_nanos() as u64;

    Ok(FileMeta {
        path,
        size: md.len(),
        mtime_nanos,
        is_dir: md.is_dir(),
    })
}

pub(crate) async fn notify_handler(
    args: Args,
    rx: NotifyEventReceiver,
    server_msg_tx: broadcast::Sender<BroadcastMessage>,
) {
    let state = NotifyHandler::new(args, rx, server_msg_tx);
    if let Err(e) = state.handle().await {
        log::error!("error in filesystem event handler: {e:#}");
    }
}
