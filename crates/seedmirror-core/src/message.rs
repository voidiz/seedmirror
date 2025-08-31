use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(tag = "message")]
pub enum Message {
    /// Sent when a client connects.
    Connected {
        /// Full path to the directory being watched.
        root_path: PathBuf,
    },

    /// Sent when a file is updated.
    FileUpdated {
        /// Path relative to the root path being watched.
        path: PathBuf,
    },
}
