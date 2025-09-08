use serde::{Deserialize, Serialize};
use std::{io::ErrorKind, path::PathBuf};
use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt};

#[derive(Serialize, Deserialize, Clone, Debug, Eq, PartialEq, Hash)]
#[serde(tag = "message")]
pub enum Message {
    ConnectionRequest {
        /// List of paths to watch.
        watched_paths: Vec<PathBuf>,
    },

    /// Sent by the server to acknowledge a `ConnectionRequest`.
    Connected,

    /// Sent when a file is updated.
    FileUpdated {
        /// Full (absolute) updated path.
        path: PathBuf,
    },
}

impl Message {
    /// Returns true if the connection is broken and should be terminated.
    pub async fn write_to_stream(
        &self,
        mut stream: impl AsyncWriteExt + Unpin,
    ) -> anyhow::Result<bool> {
        let json = format!("{}\n", serde_json::to_string_pretty(self)?);
        let content_length = json.len();
        let payload = format!("{content_length}\n{json}");
        let write_result = stream.write_all(payload.as_bytes()).await;

        if let Err(e) = write_result {
            match e.kind() {
                ErrorKind::BrokenPipe => {
                    return Ok(true);
                }
                _ => {
                    return Err(anyhow::anyhow!(e).context("failed writing to socket"));
                }
            }
        }

        Ok(false)
    }

    pub async fn read_from_reader<R>(reader: &mut R) -> anyhow::Result<Self>
    where
        R: AsyncReadExt + AsyncBufReadExt + Unpin,
    {
        let mut line = String::new();
        reader.read_line(&mut line).await?;

        let content_length: usize = line.trim().parse()?;
        let mut msg_bytes = vec![0; content_length];
        reader.read_exact(&mut msg_bytes).await?;

        log::debug!("received message: {}", String::from_utf8_lossy(&msg_bytes));
        let msg: Message = serde_json::from_slice(&msg_bytes)?;

        Ok(msg)
    }
}
