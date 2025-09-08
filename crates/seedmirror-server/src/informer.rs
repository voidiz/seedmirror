use std::{
    collections::HashMap,
    path::{self},
    time::Duration,
};

use anyhow::Context;
use notify::Event;
use seedmirror_core::message::Message;
use tokio::{sync::broadcast, task::JoinHandle, time::sleep};

use crate::watcher::NotifyEventReceiver;

struct NotifyHandler {
    /// Channel for incoming filesystem events.
    notify_rx: NotifyEventReceiver,

    /// Broadcast channel used to inform clients of updated files.
    server_msg_tx: broadcast::Sender<Message>,

    /// Ongoing event handlers.
    events: HashMap<Message, JoinHandle<()>>,
}

impl NotifyHandler {
    fn new(notify_rx: NotifyEventReceiver, server_msg_tx: broadcast::Sender<Message>) -> Self {
        Self {
            notify_rx,
            server_msg_tx,
            events: HashMap::new(),
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
                    self.events.remove(&msg);
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
                notify::EventKind::Modify(_) => {
                    let msg = Message::FileUpdated {
                        path: absolute_path,
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

        let msg_tx = self.server_msg_tx.clone();
        self.events.insert(
            // FIXME: While we might not send any of the expensive messages here (with Vecs etc.),
            // this is potentially expensive and should be replaced with something easier to hash.
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
    rx: NotifyEventReceiver,
    server_msg_tx: broadcast::Sender<Message>,
) {
    let state = NotifyHandler::new(rx, server_msg_tx);
    if let Err(e) = state.handle().await {
        log::error!("error in filesystem event handler: {e:#}");
    }
}
