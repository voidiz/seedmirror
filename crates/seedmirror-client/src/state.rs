use std::num::NonZeroUsize;

use lru::LruCache;
use tokio::sync::{broadcast, mpsc, oneshot};

use crate::task::Task;

/// Max amount of files whose sync status will be stored in the state LRU cache.
const MAX_SYNC_STATUS_FILES: usize = 50;

#[derive(Debug)]
pub(crate) enum StateBrokerMessage {
    // Sent every time the sync logic starts working on a new file
    SetSyncingPath {
        remote_file_path: String,
        local_file_path: String,
    },

    SetSyncProgress {
        /// Transferred so far (bytes, with unit prefix)
        transferred: String,

        /// Progress, percentage
        progress: u8,

        /// bytes/s, with unit prefix
        transfer_speed: String,

        /// Estimated remining time (h:mm:ss)
        /// Set to time taken when progress has reached 100%
        remaining: String,
    },

    GetAllSyncProgress(oneshot::Sender<Vec<FileSyncProgress>>),
}

#[derive(Debug)]
struct State {
    pub last_syncing_path: Option<SyncingPath>,
    pub path_status: LruCache<SyncingPath, SyncProgress>,
}

impl State {
    fn new() -> Self {
        State {
            last_syncing_path: None,
            path_status: LruCache::new(
                NonZeroUsize::new(MAX_SYNC_STATUS_FILES).expect("is non-negative"),
            ),
        }
    }
}

#[derive(Eq, Hash, PartialEq, Clone, Debug)]
pub(crate) struct SyncingPath {
    remote_file_path: String,
    local_file_path: String,
}

#[derive(Debug, Clone)]
pub(crate) struct SyncProgress {
    pub transferred: String,
    pub progress: u8,
    pub transfer_speed: String,
    pub remaining: String,
}

#[derive(Debug, Clone, serde::Serialize)]
pub(crate) struct FileSyncProgress {
    pub remote_file_path: String,
    pub local_file_path: String,

    pub transferred: String,
    pub progress: u8,
    pub transfer_speed: String,
    pub remaining: String,
}

pub(crate) type StateBrokerTx = mpsc::Sender<StateBrokerMessage>;
pub(crate) type ProgressBroadcaster = broadcast::Sender<FileSyncProgress>;
type StateBrokerRx = mpsc::Receiver<StateBrokerMessage>;

pub(crate) fn init_state_broker() -> (Task, StateBrokerTx, ProgressBroadcaster) {
    let (broker_tx, broker_rx) = mpsc::channel::<StateBrokerMessage>(128);
    let (bcast_tx, _) = broadcast::channel::<FileSyncProgress>(128);

    (
        Box::pin(state_broker(broker_rx, bcast_tx.clone())),
        broker_tx,
        bcast_tx,
    )
}

async fn state_broker(mut rx: StateBrokerRx, bcast_tx: ProgressBroadcaster) -> anyhow::Result<()> {
    let mut state = State::new();

    while let Some(msg) = rx.recv().await {
        match msg {
            StateBrokerMessage::SetSyncingPath {
                remote_file_path,
                local_file_path,
            } => {
                state.last_syncing_path = Some(SyncingPath {
                    remote_file_path,
                    local_file_path,
                });
            }
            StateBrokerMessage::SetSyncProgress {
                transferred,
                progress,
                mut transfer_speed,
                mut remaining,
            } => {
                if let Some(key) = state.last_syncing_path.clone() {
                    if progress == 100 {
                        transfer_speed = "-".to_string();
                        remaining = "0:00:00".to_string();
                    }

                    state.path_status.put(
                        key.clone(),
                        SyncProgress {
                            transferred: transferred.clone(),
                            progress,
                            transfer_speed: transfer_speed.clone(),
                            remaining: remaining.clone(),
                        },
                    );

                    // There might be no listeners (websockets), so errors are ignored.
                    let _ = bcast_tx.send(FileSyncProgress {
                        remote_file_path: key.remote_file_path,
                        local_file_path: key.local_file_path,
                        transferred,
                        progress,
                        transfer_speed,
                        remaining,
                    });
                };
            }
            StateBrokerMessage::GetAllSyncProgress(sender) => {
                let payload = state
                    .path_status
                    .clone()
                    .into_iter()
                    .map(|(k, v)| FileSyncProgress {
                        remote_file_path: k.remote_file_path,
                        local_file_path: k.local_file_path,
                        transferred: v.transferred,
                        progress: v.progress,
                        transfer_speed: v.transfer_speed,
                        remaining: v.remaining,
                    })
                    .collect::<Vec<_>>();

                if sender.send(payload).is_err() {
                    log::error!("failed to send response to `GetAllSyncProgress`");
                }
            }
        }
    }

    Ok(())
}
