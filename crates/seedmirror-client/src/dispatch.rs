use std::{
    collections::{HashMap, VecDeque},
    fs::remove_file,
    path::{Path, PathBuf},
    pin::Pin,
    process::Stdio,
    time::Duration,
};

use anyhow::Context;
use seedmirror_core::message::{ClientMessage, FetchFailure, FileMeta, ServerMessage};
use tokio::{
    io::{AsyncBufReadExt, BufReader},
    net::{
        UnixStream,
        unix::{OwnedReadHalf, OwnedWriteHalf},
    },
    process::{Child, Command},
    time::sleep,
};

use crate::{
    cli::{Args, PathMapping},
    fetch::{Fetch, FetchOutcome, human_bytes, mismatch_reason, prepare_sync, set_syncing_path},
    state::StateBrokerTx,
};

type Task = Pin<Box<dyn Future<Output = anyhow::Result<()>> + Send>>;

pub(crate) fn init_remote_watcher(args: &Args, state_tx: StateBrokerTx) -> anyhow::Result<Task> {
    if args.local_socket_path.try_exists()? {
        remove_file(&args.local_socket_path).with_context(|| {
            format!(
                "failed to remove existing socket: {:?}",
                args.local_socket_path
            )
        })?;
    }

    let mut ssh_child = Command::new("ssh")
        .kill_on_drop(true)
        .arg(&args.ssh_hostname)
        .arg("-nNT")
        .arg("-o")
        .arg("ServerAliveInterval=60")
        .arg("-o")
        .arg("ServerAliveCountMax=3")
        .arg("-L")
        .arg(format!(
            "{}:{}",
            args.local_socket_path.to_string_lossy(),
            args.socket_path.to_string_lossy()
        ))
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()
        .with_context(|| "failed to spawn ssh")?;

    if let Some(stderr) = ssh_child.stderr.take() {
        tokio::spawn(async move {
            let mut lines = BufReader::new(stderr).lines();

            while let Ok(Some(line)) = lines.next_line().await {
                log::error!("ssh stderr: {line}");
            }
        });
    }

    let remote_watcher = new_remote_watcher(args.clone(), state_tx, ssh_child);
    Ok(Box::pin(remote_watcher))
}

/// Read half of the socket to the server.
struct Reader {
    stream: BufReader<OwnedReadHalf>,
    read_buf: Vec<u8>,
}

impl Reader {
    fn new(stream: OwnedReadHalf) -> Self {
        Self {
            stream: BufReader::new(stream),
            read_buf: Vec::new(),
        }
    }

    async fn next(&mut self) -> anyhow::Result<ServerMessage<'_>> {
        ServerMessage::read_from_reader(&mut self.stream, &mut self.read_buf).await
    }
}

/// Write half of the socket to the server.
struct Writer {
    stream: OwnedWriteHalf,
    write_buf: Vec<u8>,
}

impl Writer {
    fn new(stream: OwnedWriteHalf) -> Self {
        Self {
            stream,
            write_buf: Vec::new(),
        }
    }

    async fn send(&mut self, msg: &ClientMessage) -> anyhow::Result<()> {
        msg.write_to_stream(&mut self.stream, &mut self.write_buf)
            .await
    }
}

enum Pending {
    None,
    Listing,
    Fetching(Box<Fetch>),
}

struct State {
    args: Args,
    state_tx: StateBrokerTx,
    writer: Writer,

    /// Paths that still need syncing, latest version per path.
    work: HashMap<PathBuf, FileMeta>,

    /// Path mappings that have yet to be synced.
    mappings: VecDeque<PathMapping>,

    /// The request currently in flight.
    pending: Pending,
}

impl State {
    fn new(args: Args, state_tx: StateBrokerTx, writer: Writer) -> Self {
        let mappings: VecDeque<PathMapping> = if args.no_initial_sync {
            VecDeque::new()
        } else {
            args.path_mappings.iter().cloned().collect()
        };

        if args.no_initial_sync {
            log::info!("initial sync disabled, watching for live updates");
        } else if mappings.is_empty() {
            log::info!("initial sync listing complete");
        } else {
            log::info!("starting initial sync for {} paths", mappings.len());
        }

        Self {
            args,
            state_tx,
            writer,
            work: HashMap::new(),
            mappings,
            pending: Pending::None,
        }
    }

    fn take_fetch(&mut self) -> Option<Fetch> {
        match std::mem::replace(&mut self.pending, Pending::None) {
            Pending::Fetching(fetch) => Some(*fetch),
            other => {
                self.pending = other;
                None
            }
        }
    }

    fn pop_work(&mut self) -> Option<FileMeta> {
        let path = self.work.keys().next()?.clone();
        self.work.remove(&path)
    }

    /// Handle queued sync jobs, either based on incoming `FileUpdate`s in watched directories from
    /// the server, or initial sync.
    async fn next(&mut self) -> anyhow::Result<()> {
        // Don't do anything if we're in the middle of a request
        if !matches!(self.pending, Pending::None) {
            return Ok(());
        }

        loop {
            if let Some(meta) = self.pop_work() {
                if self.begin_sync(meta).await? {
                    return Ok(());
                }

                continue;
            }

            if let Some(mapping) = self.mappings.pop_front() {
                self.pending = Pending::Listing;
                self.writer
                    .send(&ClientMessage::ListDir {
                        path: mapping.remote,
                    })
                    .await?;

                return Ok(());
            }

            return Ok(());
        }
    }

    /// Sync remote `meta` with local fs.
    async fn begin_sync(&mut self, meta: FileMeta) -> anyhow::Result<bool> {
        match prepare_sync(&meta, &self.args.path_mappings, self.args.dry_run).await? {
            Some(local) => self.begin_fetch(meta, local).await,
            None => Ok(false),
        }
    }

    async fn begin_fetch(&mut self, meta: FileMeta, local: PathBuf) -> anyhow::Result<bool> {
        let tolerance_nanos = self.args.modify_window.saturating_mul(1_000_000_000);
        let Some(reason) = mismatch_reason(&local, &meta, tolerance_nanos)? else {
            self.work.remove(&meta.path);
            return Ok(false);
        };

        if self.args.dry_run {
            log::info!(
                "would sync remote {:?} to local {local:?} (size: {}, reason: {reason})",
                meta.path,
                human_bytes(meta.size)
            );

            self.work.remove(&meta.path);
            return Ok(false);
        }

        log::info!(
            "syncing remote {:?} to local {local:?} (size: {}, reason: {reason})",
            meta.path,
            human_bytes(meta.size)
        );

        set_syncing_path(&self.state_tx, &meta, &local);

        let path = meta.path.clone();
        self.pending = Pending::Fetching(Box::new(Fetch::new(meta, local)));
        self.writer.send(&ClientMessage::FetchFile { path }).await?;

        Ok(true)
    }

    async fn finish_fetch(
        &mut self,
        path: PathBuf,
        local: PathBuf,
        outcome: FetchOutcome,
    ) -> anyhow::Result<()> {
        // Check if there's a newer version of the file that came in after we started the transfer
        let newer = match outcome {
            FetchOutcome::Done { size, mtime_nanos } => match self.work.remove(&path) {
                Some(newer) if newer.size != size || newer.mtime_nanos != mtime_nanos => {
                    Some(newer)
                }
                _ => None,
            },
            FetchOutcome::Stale => self.work.remove(&path),
        };

        self.pending = Pending::None;

        // Refetch if there was a newer version
        if let Some(newer) = newer {
            self.begin_fetch(newer, local).await?;
        }

        Ok(())
    }

    async fn dispatch(&mut self, msg: ServerMessage<'_>) -> anyhow::Result<()> {
        match msg {
            ServerMessage::FileUpdated { meta } => self.on_file_updated(meta),
            ServerMessage::ListDirResponse { entries } => self.on_listing(entries),
            ServerMessage::ListDirFailed { reason } => self.on_list_failed(reason),
            ServerMessage::FileHeader { size, mtime_nanos } => {
                self.on_header(size, mtime_nanos).await
            }
            ServerMessage::FileChunk { data } => self.on_chunk(data).await,
            ServerMessage::FileEof => self.on_eof().await,
            ServerMessage::FetchFailed { kind, reason } => self.on_fetch_failed(kind, reason).await,
            _ => anyhow::bail!("unexpected message after handshake"),
        }
    }

    fn on_file_updated(&mut self, meta: FileMeta) -> anyhow::Result<()> {
        log::debug!("queued update for {:?} ({} bytes)", meta.path, meta.size);
        self.work.insert(meta.path.clone(), meta);
        Ok(())
    }

    fn on_listing(&mut self, entries: Vec<FileMeta>) -> anyhow::Result<()> {
        match &self.pending {
            Pending::Listing => {}
            _ => anyhow::bail!("received ListDirResponse with no active listing"),
        }

        self.pending = Pending::None;

        let num_entries = entries.len();
        for entry in entries {
            self.work.insert(entry.path.clone(), entry);
        }

        let total = self.args.path_mappings.len();
        let done = total - self.mappings.len();
        log::info!("listing {done}/{total} returned {num_entries} entries");

        if self.mappings.is_empty() {
            log::info!("initial sync listing complete");
        }

        Ok(())
    }

    fn on_list_failed(&mut self, reason: String) -> anyhow::Result<()> {
        match &self.pending {
            Pending::Listing => {}
            _ => anyhow::bail!("received ListDirFailed with no active listing"),
        }

        anyhow::bail!("list dir failed: {reason}");
    }

    async fn on_header(&mut self, size: u64, mtime_nanos: u64) -> anyhow::Result<()> {
        let Pending::Fetching(fetch) = &mut self.pending else {
            anyhow::bail!("received FileHeader with no active fetch");
        };

        fetch.on_header(size, mtime_nanos).await
    }

    async fn on_chunk(&mut self, data: &[u8]) -> anyhow::Result<()> {
        let Pending::Fetching(fetch) = &mut self.pending else {
            anyhow::bail!("received FileChunk with no active fetch");
        };

        fetch.on_chunk(data, &self.state_tx).await
    }

    async fn on_eof(&mut self) -> anyhow::Result<()> {
        let Some(fetch) = self.take_fetch() else {
            anyhow::bail!("received FileEof with no active fetch");
        };

        let path = fetch.meta().path.clone();
        let local = fetch.local().to_path_buf();
        let outcome = fetch.on_eof(&self.state_tx).await?;
        self.finish_fetch(path, local, outcome).await
    }

    async fn on_fetch_failed(&mut self, kind: FetchFailure, reason: String) -> anyhow::Result<()> {
        let Some(fetch) = self.take_fetch() else {
            anyhow::bail!("received FetchFailed with no active fetch");
        };

        let path = fetch.meta().path.clone();
        let local = fetch.local().to_path_buf();
        fetch.discard().await;

        match kind {
            FetchFailure::Stale => {
                log::info!("server aborted fetch for {path:?}: {reason}");
                self.finish_fetch(path, local, FetchOutcome::Stale).await
            }
            FetchFailure::Unavailable => {
                anyhow::bail!("server could not serve {path:?}: {reason}");
            }
        }
    }
}

async fn run(reader: &mut Reader, state: &mut State) -> anyhow::Result<()> {
    loop {
        state.next().await?;

        let msg = reader.next().await?;
        state.dispatch(msg).await?;
    }
}

async fn handshake(args: &Args, reader: &mut Reader, writer: &mut Writer) -> anyhow::Result<()> {
    writer
        .send(&ClientMessage::ConnectionRequest {
            watched_paths: args
                .path_mappings
                .iter()
                .map(|mapping| mapping.remote.clone())
                .collect(),
        })
        .await?;

    match reader.next().await? {
        ServerMessage::Connected => {
            log::debug!("received `Connected` answer from server");
            Ok(())
        }
        ServerMessage::ConnectionFailed { reason } => {
            anyhow::bail!("connection failed: {reason}");
        }
        _ => anyhow::bail!("unexpected message during handshake"),
    }
}

async fn new_remote_watcher(
    args: Args,
    state_tx: StateBrokerTx,
    mut ssh_child: Child,
) -> anyhow::Result<()> {
    let local_socket_path = &args.local_socket_path;

    tokio::select! {
        _ = wait_for_file(local_socket_path) => {},
        // File will never exist if the ssh process fails and exits.
        status = ssh_child.wait() => {
            anyhow::bail!("ssh exited: {:?}", status?)
        }
    }

    log::info!("connecting to {local_socket_path:?}");
    let stream = UnixStream::connect(&local_socket_path)
        .await
        .with_context(|| format!("failed to connect to socket at {local_socket_path:?}"))?;
    log::info!("connected to {local_socket_path:?}");

    let (read_half, write_half) = stream.into_split();
    let mut reader = Reader::new(read_half);
    let mut writer = Writer::new(write_half);
    handshake(&args, &mut reader, &mut writer).await?;

    let mut state = State::new(args, state_tx, writer);
    run(&mut reader, &mut state).await
}

async fn wait_for_file(path: &Path) {
    // TODO: Use file watcher at some point
    while !path.exists() {
        log::info!("waiting for {path:?} to be created");
        sleep(Duration::from_millis(100)).await;
    }
}
