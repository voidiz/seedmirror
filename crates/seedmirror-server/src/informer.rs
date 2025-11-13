use std::{
    collections::HashMap,
    path::{self, Path, PathBuf},
};

use anyhow::Context;
use notify::Event;
use seedmirror_core::message::Message;
use tokio::{sync::broadcast, task::JoinHandle, time::sleep};

use crate::{cli::Args, watcher::NotifyEventReceiver};

struct NotifyHandler {
    args: Args,

    /// Channel for incoming filesystem events.
    notify_rx: NotifyEventReceiver,

    /// Broadcast channel used to inform clients of updated files.
    server_msg_tx: broadcast::Sender<Message>,

    /// Ongoing event handlers for file updates.
    event_handlers: HashMap<PathBuf, JoinHandle<()>>,
}

impl NotifyHandler {
    fn new(
        args: Args,
        notify_rx: NotifyEventReceiver,
        server_msg_tx: broadcast::Sender<Message>,
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
                    if let Message::FileUpdated { path } = msg {
                        self.event_handlers.remove(&path);
                    }
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
                    let mut path = absolute_path.clone();

                    // Push an empty component to the path to add a trailing slash. This is
                    // important for rsync to treat it as a directory so that
                    // `rsync <src dir> <dst dir>`
                    // synchronizes the state of `<src dir>` with `<dst dir>` instead of placing
                    // `<src dir>` inside `<dst dir>`.
                    if path.is_dir() {
                        path.push("");
                    }

                    let msg = Message::FileUpdated { path };
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

    fn queue_notify_message(&mut self, path: &Path, msg: Message) {
        self.abort_event_handler(path);

        let sync_delay = self.args.sync_delay.clone();
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

pub(crate) async fn notify_handler(
    args: Args,
    rx: NotifyEventReceiver,
    server_msg_tx: broadcast::Sender<Message>,
) {
    let state = NotifyHandler::new(args, rx, server_msg_tx);
    if let Err(e) = state.handle().await {
        log::error!("error in filesystem event handler: {e:#}");
    }
}
