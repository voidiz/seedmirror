use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(tag = "message")]
pub enum Message {
    FileUpdated {
        /// Path relative to the root path being watched.
        path: PathBuf,
    },
}
