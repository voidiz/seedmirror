/// Message protocol used to communicate between seedmirror-client and seedmirror-server. Includes
/// sending file chunks. The protocol is serial, so there are no IDs. For example, if the client
/// sends `FetchFile`, all file-related messages from the server will be in response to that
/// request.
use anyhow::Context;
use serde::{Deserialize, Serialize};
use std::{fmt::Debug, path::PathBuf};
use tokio::io::{AsyncReadExt, AsyncWriteExt};

/// 1 MiB
const MAX_MESSAGE_BYTES: usize = 1024 * 1024;

#[derive(Serialize, Deserialize, Clone, Debug)]
pub enum ClientMessage {
    /// Sent by the client upon connecting to the server.
    ConnectionRequest {
        /// List of paths to watch.
        watched_paths: Vec<PathBuf>,
    },

    /// Sent by the client to recursively walk `path` on the server. If `path` is a file rather than
    /// a directory, the response contains just that file's entry.
    ListDir { path: PathBuf },

    /// Sent by the client to fetch a specific file. This generally happens when the client has done
    /// a `ListDir` and it identifies mismatches in `mtime` + `size`.
    FetchFile { path: PathBuf },
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub enum FetchFailure {
    /// The file changed on the server mid-transfer. This is generally followed by a new
    /// `FileUpdated` from the server.
    Stale,

    /// Any other error.
    Unavailable,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub enum ServerMessage<'a> {
    /// Sent by the server to acknowledge a `ConnectionRequest`.
    Connected,

    /// Sent by the server when a `ConnectionRequest` fails.
    ConnectionFailed { reason: String },

    /// Sent by the server when a file in a monitored directory is updated.
    FileUpdated { meta: FileMeta },

    /// Sent by the server in response to `ListDir`.
    ListDirResponse { entries: Vec<FileMeta> },

    /// Sent by the server when a `ListDir` request fails.
    ListDirFailed { reason: String },

    /// Sent by the server when a `FetchFile` request fails before or during
    /// transfer.
    FetchFailed {
        /// Why the transfer was aborted.
        kind: FetchFailure,
        reason: String,
    },

    /// Sent by the server in response to `FetchFile`.
    FileHeader {
        /// Size of the file, used to determine the chunk size
        size: u64,

        /// Modified time, unix timestamp, nanoseconds. Shared between client and server when a file
        /// is fully transferred to detect outdated files.
        mtime_nanos: u64,
    },

    /// Sent by the server following `FileHeader`. All following `FileChunk`s must belong to the file
    /// corresponding to the preceding `FileHeader`.
    FileChunk { data: &'a [u8] },

    /// Sent by the server following the last `FileChunk`. If this message is not received at the
    /// end of the file, it is safe for the client to delete the partial file. Conversely, when the
    /// message is received, the file transfer should be seen as completed.
    FileEof,
}

/// Shared length-prefixed write logic. `buf` is used to avoid allocating a buffer for every `msg`.
async fn write_inner<T>(
    msg: &T,
    mut stream: impl AsyncWriteExt + Unpin,
    buf: &mut Vec<u8>,
) -> anyhow::Result<()>
where
    T: Serialize + Debug,
{
    buf.clear();
    postcard::to_io(msg, &mut *buf).with_context(|| "failed to serialize message")?;
    if buf.len() > MAX_MESSAGE_BYTES {
        anyhow::bail!(
            "message too large: {} bytes (max {MAX_MESSAGE_BYTES})",
            buf.len()
        );
    }

    async {
        stream.write_all(&(buf.len() as u32).to_le_bytes()).await?;
        stream.write_all(buf).await?;
        stream.flush().await?;
        Ok::<_, std::io::Error>(())
    }
    .await
    .map_err(|e| anyhow::anyhow!(e).context("failed writing to socket"))?;

    Ok(())
}

/// Deserialize length-prefixed `T` from `reader`, reusing `buf`.
async fn read_inner<'a, T, R>(reader: &mut R, buf: &'a mut Vec<u8>) -> anyhow::Result<T>
where
    T: Deserialize<'a>,
    R: AsyncReadExt + Unpin,
{
    let mut len_bytes = [0u8; 4];
    reader
        .read_exact(&mut len_bytes)
        .await
        .with_context(|| "failed to read message length (was the connection closed?)")?;
    let content_length = u32::from_le_bytes(len_bytes) as usize;
    if content_length > MAX_MESSAGE_BYTES {
        anyhow::bail!("message too large: {content_length} bytes (max {MAX_MESSAGE_BYTES})");
    }

    buf.clear();
    buf.resize(content_length, 0);
    reader
        .read_exact(buf)
        .await
        .with_context(|| "failed to read message body")?;

    postcard::from_bytes(buf).with_context(|| "failed to deserialize message")
}

impl ClientMessage {
    pub async fn write_to_stream(
        &self,
        stream: impl AsyncWriteExt + Unpin,
        buf: &mut Vec<u8>,
    ) -> anyhow::Result<()> {
        write_inner(self, stream, buf).await
    }

    pub async fn read_from_reader<R>(reader: &mut R, buf: &mut Vec<u8>) -> anyhow::Result<Self>
    where
        R: AsyncReadExt + Unpin,
    {
        let msg: Self = read_inner(reader, buf).await?;
        log::debug!("received client message: {msg:?}");
        Ok(msg)
    }
}

impl<'a> ServerMessage<'a> {
    pub async fn write_to_stream(
        &self,
        stream: impl AsyncWriteExt + Unpin,
        buf: &mut Vec<u8>,
    ) -> anyhow::Result<()> {
        write_inner(self, stream, buf).await
    }

    pub async fn read_from_reader<R>(
        reader: &mut R,
        buf: &'a mut Vec<u8>,
    ) -> anyhow::Result<ServerMessage<'a>>
    where
        R: AsyncReadExt + Unpin,
    {
        let msg: ServerMessage<'a> = read_inner(reader, buf).await?;

        // Skip chunk payloads to reduce noise
        if !matches!(msg, ServerMessage::FileChunk { .. }) {
            log::debug!("received server message: {msg:?}");
        }

        Ok(msg)
    }
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct FileMeta {
    pub path: PathBuf,
    pub size: u64,
    pub mtime_nanos: u64,
    pub is_dir: bool,
}
