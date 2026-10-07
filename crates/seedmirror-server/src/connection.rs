use anyhow::Context;
use notify::{RecursiveMode, Watcher};
use seedmirror_core::message::{ClientMessage, FetchFailure, FileMeta, ServerMessage};
use std::{path::Path, time::UNIX_EPOCH};
use tokio::{
    fs::remove_file,
    io::{AsyncReadExt, BufReader},
    net::{UnixListener, UnixStream},
    sync::broadcast,
    task::JoinSet,
};

use crate::{
    cli::Args,
    informer::{self, BroadcastMessage},
    watcher,
};

/// File content bytes per `FileChunk`.
// TODO: Make it variable depending on file size?
const FETCH_CHUNK_BYTES: usize = 64 * 1024;

/// How often to check whether a file has updated during transfer.
const FETCH_STAT_EVERY_CHUNKS: usize = 16;

enum FetchOutcome {
    /// File fully transferred.
    Complete,

    /// File changed mid-transfer.
    Stale,
}

/// Returns file handle and metadata for file at `path`.
async fn prepare_fetch(path: &Path) -> anyhow::Result<(tokio::fs::File, u64, u64)> {
    let meta = tokio::fs::metadata(path)
        .await
        .with_context(|| format!("failed to stat {path:?}"))?;

    if meta.is_dir() {
        anyhow::bail!("fetch for {path:?} refused: is a directory");
    }

    let mtime_nanos = mtime_nanos_of(&meta, path)?;
    let file = tokio::fs::File::open(path)
        .await
        .with_context(|| format!("failed to open {path:?}"))?;

    Ok((file, meta.len(), mtime_nanos))
}

async fn stream_chunks(
    path: &Path,
    size: u64,
    mtime_nanos: u64,
    file: &mut tokio::fs::File,
    write_stream: &mut (impl tokio::io::AsyncWriteExt + Unpin),
    write_buf: &mut Vec<u8>,
) -> anyhow::Result<FetchOutcome> {
    let mut chunk_buf = vec![0u8; FETCH_CHUNK_BYTES];
    let mut chunks_since_check = 0usize;

    loop {
        let chunk_size = file
            .read(&mut chunk_buf)
            .await
            .with_context(|| format!("failed to read {path:?}"))?;

        if chunk_size == 0 {
            break;
        }

        let chunk = ServerMessage::FileChunk {
            data: &chunk_buf[..chunk_size],
        };

        chunk.write_to_stream(&mut *write_stream, write_buf).await?;

        chunks_since_check += 1;
        if chunks_since_check < FETCH_STAT_EVERY_CHUNKS {
            continue;
        }

        chunks_since_check = 0;

        // Check whether file has updated
        let cur = tokio::fs::metadata(path)
            .await
            .with_context(|| format!("fetch for {path:?} failed: file vanished"))?;

        let cur_mtime = mtime_nanos_of(&cur, path)?;
        if cur.len() != size || cur_mtime != mtime_nanos {
            return Ok(FetchOutcome::Stale);
        }
    }

    let cur = tokio::fs::metadata(path)
        .await
        .with_context(|| format!("fetch for {path:?} failed: file vanished before Eof"))?;

    let cur_mtime = mtime_nanos_of(&cur, path)?;
    if cur.len() != size || cur_mtime != mtime_nanos {
        return Ok(FetchOutcome::Stale);
    }

    Ok(FetchOutcome::Complete)
}

pub(crate) async fn connection_manager(args: Args) -> anyhow::Result<()> {
    if let Err(e) = connection_manager_inner(args).await {
        anyhow::bail!("error starting connection manager: {e:#}");
    }

    Ok(())
}

async fn connection_manager_inner(args: Args) -> anyhow::Result<()> {
    let socket_path = &args.socket_path;

    if socket_path.try_exists()? {
        remove_file(&socket_path)
            .await
            .with_context(|| format!("failed to remove existing socket: {socket_path:?}"))?;
    }

    let listener = UnixListener::bind(socket_path)
        .with_context(|| format!("failed to listen to socket at {socket_path:?}"))?;

    loop {
        match listener.accept().await {
            Ok((stream, _addr)) => {
                tokio::spawn(connection_handler(args.clone(), stream));
            }
            Err(e) => {
                log::error!("failed to accept incoming connection: {e:#}");
            }
        }
    }
}

async fn connection_handler(args: Args, stream: UnixStream) {
    if let Err(e) = connection_handler_inner(args, stream).await {
        if is_peer_disconnected(&e) {
            log::info!("client disconnected: {e:#}");
        } else {
            log::error!("connection handler failed: {e:#}");
        }
    }
}

fn is_peer_disconnected(e: &anyhow::Error) -> bool {
    e.chain().any(|cause| {
        cause.downcast_ref::<std::io::Error>().is_some_and(|io| {
            matches!(
                io.kind(),
                std::io::ErrorKind::BrokenPipe
                    | std::io::ErrorKind::ConnectionReset
                    | std::io::ErrorKind::UnexpectedEof
            )
        })
    })
}

async fn connection_handler_inner(args: Args, mut stream: UnixStream) -> anyhow::Result<()> {
    log::info!("established socket connection with client");

    // TODO: Figure out how to handle a big queue of fs events
    let (server_msg_tx, mut server_msg_rx) = broadcast::channel::<BroadcastMessage>(65536);

    // Watcher will be shut down on drop
    let (mut watcher, notify_rx) = watcher::create_watcher().await?;

    let mut set = JoinSet::new();
    set.spawn(informer::notify_handler(args, notify_rx, server_msg_tx));

    let mut read_buf = Vec::new();
    let mut write_buf = Vec::new();

    loop {
        tokio::select! {
            res = server_msg_rx.recv() => {
                handle_server_msg(res, &mut stream, &mut write_buf).await?;
            }
            Ok(_) = stream.readable() => {
                handle_client_msg(&mut watcher, &mut stream, &mut read_buf, &mut write_buf).await?;
            }
        }
    }
}

async fn handle_server_msg(
    res: Result<BroadcastMessage, broadcast::error::RecvError>,
    stream: &mut UnixStream,
    write_buf: &mut Vec<u8>,
) -> anyhow::Result<()> {
    match res {
        Ok(BroadcastMessage::FileUpdated { path }) => {
            let meta = match build_file_meta(&path).await {
                Ok(meta) => meta,
                Err(e) => {
                    log::info!(
                        "skipping FileUpdated for {path:?}, file most likely removed: {e:#}"
                    );

                    return Ok(());
                }
            };

            let msg = ServerMessage::FileUpdated { meta };
            msg.write_to_stream(stream, write_buf).await?;
        }
        Err(broadcast::error::RecvError::Lagged(skipped)) => {
            log::warn!("receiving too many filesystem events, skipping {skipped} event(s)");
        }
        Err(e) => anyhow::bail!("recv on filesystem event broadcast channel failed: {e:#}"),
    }

    Ok(())
}

async fn handle_client_msg(
    watcher: &mut impl Watcher,
    stream: &mut UnixStream,
    read_buf: &mut Vec<u8>,
    write_buf: &mut Vec<u8>,
) -> anyhow::Result<()> {
    let (read_stream, mut write_stream) = tokio::io::split(&mut *stream);

    let mut reader = BufReader::new(read_stream);
    let msg = ClientMessage::read_from_reader(&mut reader, read_buf).await?;

    match msg {
        // TODO: Exchange version information to ensure client and server match
        ClientMessage::ConnectionRequest { watched_paths } => {
            for path in watched_paths {
                let watch_res = watcher
                    .watch(&path, RecursiveMode::Recursive)
                    .with_context(|| format!("failed to watch path `{}`", path.to_string_lossy()));

                if let Err(e) = watch_res {
                    ServerMessage::ConnectionFailed {
                        reason: format!("{e:#}"),
                    }
                    .write_to_stream(&mut write_stream, write_buf)
                    .await?;

                    anyhow::bail!(e);
                }
            }

            ServerMessage::Connected
                .write_to_stream(&mut write_stream, write_buf)
                .await?;
        }
        ClientMessage::ListDir { path } => match list_dir(&path).await {
            Ok(entries) => {
                let resp = ServerMessage::ListDirResponse { entries };
                resp.write_to_stream(&mut write_stream, write_buf).await?;
            }
            Err(e) => {
                let reason = format!("failed to list dir {path:?}: {e:#}");
                log::warn!("{reason}");

                let msg = ServerMessage::ListDirFailed { reason };
                msg.write_to_stream(&mut write_stream, write_buf).await?;
            }
        },
        ClientMessage::FetchFile { path } => {
            serve_fetch(&path, &mut write_stream, write_buf).await?;
        }
    }

    Ok(())
}

/// Streams `FileHeader` + `FileChunk`s + `FileEof` for `path`.
async fn serve_fetch(
    path: &Path,
    write_stream: &mut (impl tokio::io::AsyncWriteExt + Unpin),
    write_buf: &mut Vec<u8>,
) -> anyhow::Result<()> {
    let (mut file, size, mtime_nanos) = match prepare_fetch(path).await {
        Ok(v) => v,
        Err(e) => {
            return fail_fetch(
                FetchFailure::Unavailable,
                format!("{e:#}, skipping fetch without Eof"),
                write_stream,
                write_buf,
            )
            .await;
        }
    };

    let header = ServerMessage::FileHeader { size, mtime_nanos };

    header
        .write_to_stream(&mut *write_stream, write_buf)
        .await?;

    match stream_chunks(path, size, mtime_nanos, &mut file, write_stream, write_buf).await {
        Ok(FetchOutcome::Complete) => {}
        Ok(FetchOutcome::Stale) => {
            return fail_fetch(
                FetchFailure::Stale,
                format!("file changed mid-transfer for {path:?}, discarding partial"),
                write_stream,
                write_buf,
            )
            .await;
        }
        Err(e) => {
            return fail_fetch(
                FetchFailure::Unavailable,
                format!("{e:#}, aborting fetch without Eof"),
                write_stream,
                write_buf,
            )
            .await;
        }
    }

    ServerMessage::FileEof
        .write_to_stream(&mut *write_stream, write_buf)
        .await?;

    log::debug!("fetched {path:?}: {size} bytes");
    Ok(())
}

async fn fail_fetch(
    kind: FetchFailure,
    reason: String,
    write_stream: &mut (impl tokio::io::AsyncWriteExt + Unpin),
    write_buf: &mut Vec<u8>,
) -> anyhow::Result<()> {
    log::warn!("{reason}");

    let msg = ServerMessage::FetchFailed { kind, reason };
    msg.write_to_stream(&mut *write_stream, write_buf).await?;
    Ok(())
}

/// Recursively walks `path`, returning a `FileMeta` per entry (including `path` itself).
async fn list_dir(path: &Path) -> anyhow::Result<Vec<FileMeta>> {
    tokio::fs::metadata(path)
        .await
        .with_context(|| format!("failed to stat list root {path:?}"))?;

    // spawn_blocking since walk_dir is blocking
    let owned = path.to_path_buf();
    tokio::task::spawn_blocking(move || walk_dir(&owned))
        .await
        .with_context(|| format!("blocking walk task failed for {path:?}"))?
}

fn walk_dir(path: &Path) -> anyhow::Result<Vec<FileMeta>> {
    let mut entries = Vec::new();

    for entry in walkdir::WalkDir::new(path).follow_links(false) {
        let entry = entry.with_context(|| format!("failed to walk under {path:?}"))?;

        // TODO: Support symlinks?
        if entry.file_type().is_symlink() {
            log::warn!("skipping symlink {:?}: not supported", entry.path());
            continue;
        }

        let meta = entry
            .metadata()
            .with_context(|| format!("failed to stat {:?}", entry.path()))?;

        let mtime_nanos = mtime_nanos_of(&meta, entry.path())?;

        entries.push(FileMeta {
            path: entry.path().to_path_buf(),
            size: meta.len(),
            mtime_nanos,
            is_dir: meta.is_dir(),
        });
    }

    Ok(entries)
}

async fn build_file_meta(path: &Path) -> anyhow::Result<FileMeta> {
    let md = tokio::fs::metadata(path)
        .await
        .with_context(|| format!("failed to stat path: {path:?}"))?;

    let mtime_nanos = mtime_nanos_of(&md, path)?;

    Ok(FileMeta {
        path: path.to_path_buf(),
        size: md.len(),
        mtime_nanos,
        is_dir: md.is_dir(),
    })
}

fn mtime_nanos_of(meta: &std::fs::Metadata, path: &Path) -> anyhow::Result<u64> {
    Ok(meta
        .modified()
        .with_context(|| format!("failed to read mtime: {path:?}"))?
        .duration_since(UNIX_EPOCH)
        .map_err(|e| anyhow::anyhow!("mtime before epoch for {path:?}: {e:?}"))?
        .as_nanos() as u64)
}
