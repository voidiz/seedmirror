use std::{
    io::ErrorKind,
    path::{Path, PathBuf},
    time::{Instant, UNIX_EPOCH},
};

use anyhow::Context;
use seedmirror_core::message::FileMeta;
use tokio::io::AsyncWriteExt;

use crate::{
    cli::PathMapping,
    file::{cleanup_partial, finalize_tmp, tmp_path},
    state::{StateBrokerMessage, StateBrokerTx},
};

pub(crate) struct Fetch {
    /// Remote information about the file being fetched.
    meta: FileMeta,

    /// Path to local file where the file is being written to.
    local: PathBuf,

    /// File handle of the local file being written to.
    file: Option<tokio::fs::File>,

    /// Size received in file header, more recent than `meta.path`.
    size: u64,

    /// Modified time received in file header, more recent than `meta.mtime_nanos`.
    mtime_nanos: u64,

    /// Amount of bytes written so far.
    written: u64,

    /// Timestamp at which the transfer started. Set when we receive the file header from the
    /// server.
    started: Option<Instant>,

    /// File transfer percentage (0-100).
    last_percentage: Option<u8>,
}

pub(crate) enum FetchOutcome {
    /// File fully transferred.
    Done { size: u64, mtime_nanos: u64 },

    /// File changed on the server mid-transfer.
    Stale,
}

impl Fetch {
    pub(crate) fn new(meta: FileMeta, local: PathBuf) -> Self {
        Self {
            meta,
            local,
            file: None,
            size: 0,
            mtime_nanos: 0,
            written: 0,
            started: None,
            last_percentage: None,
        }
    }

    pub(crate) fn meta(&self) -> &FileMeta {
        &self.meta
    }

    pub(crate) fn local(&self) -> &Path {
        &self.local
    }

    pub(crate) async fn on_header(&mut self, size: u64, mtime_nanos: u64) -> anyhow::Result<()> {
        if self.file.is_some() {
            anyhow::bail!("redundant header during fetch of {:?}", self.meta.path);
        }

        if let Some(parent) = self.local.parent()
            && !parent.as_os_str().is_empty()
        {
            tokio::fs::create_dir_all(parent)
                .await
                .with_context(|| format!("failed to create parent dir {parent:?}"))?;
        }

        let tmp = tmp_path(&self.local);
        self.file = Some(
            tokio::fs::File::create(&tmp)
                .await
                .with_context(|| format!("failed to create temp file {tmp:?}"))?,
        );

        self.size = size;
        self.mtime_nanos = mtime_nanos;
        self.started = Some(Instant::now());

        Ok(())
    }

    pub(crate) async fn on_chunk(
        &mut self,
        data: &[u8],
        progress: &StateBrokerTx,
    ) -> anyhow::Result<()> {
        let write_result = {
            let Some(file) = self.file.as_mut() else {
                anyhow::bail!("received file chunk before header for {:?}", self.meta.path);
            };

            file.write_all(data).await
        };

        if let Err(e) = write_result {
            log::error!("failed to write temp file {:?}: {e:#}", self.local);
            cleanup_partial_logged(&self.local).await;
            return Err(e.into());
        }

        self.written += data.len() as u64;
        report_progress(
            progress,
            self.written,
            self.size,
            self.started.unwrap_or_else(Instant::now),
            &mut self.last_percentage,
        );

        Ok(())
    }

    pub(crate) async fn on_eof(mut self, progress: &StateBrokerTx) -> anyhow::Result<FetchOutcome> {
        let Some(file) = self.file.take() else {
            cleanup_partial_logged(&self.local).await;
            anyhow::bail!("received file EOF before header for {:?}", self.meta.path);
        };

        if self.written != self.size {
            drop(file);
            cleanup_partial_logged(&self.local).await;
            anyhow::bail!(
                "fetch for {:?} size mismatch: got {} of {} bytes",
                self.meta.path,
                self.written,
                self.size
            );
        }

        if let Err(e) = finalize_tmp(file, &self.local, self.mtime_nanos).await {
            cleanup_partial_logged(&self.local).await;
            return Err(e).with_context(|| format!("failed to install {:?}", self.local));
        }

        report_progress(
            progress,
            self.size,
            self.size,
            self.started.unwrap_or_else(Instant::now),
            &mut self.last_percentage,
        );

        Ok(FetchOutcome::Done {
            size: self.size,
            mtime_nanos: self.mtime_nanos,
        })
    }

    /// Discard the local partially transferred file.
    pub(crate) async fn discard(mut self) {
        drop(self.file.take());
        cleanup_partial_logged(&self.local).await;
    }
}

async fn cleanup_partial_logged(local: &Path) {
    if let Err(e) = cleanup_partial(local).await {
        log::warn!("{e:#}");
    }
}

/// Determine what needs to be done to sync remote `meta` to the local fs based on `mappings`.
/// Returns the local path to fetch, or `None` when there is nothing to transfer.
pub(crate) async fn prepare_sync(
    meta: &FileMeta,
    mappings: &[PathMapping],
    dry_run: bool,
) -> anyhow::Result<Option<PathBuf>> {
    let local_path = resolve_local_path(meta, mappings)?;
    let local_is_dir = match std::fs::symlink_metadata(&local_path) {
        Ok(md) => Some(md.is_dir() && !md.file_type().is_symlink()),
        Err(e) if e.kind() == ErrorKind::NotFound => None,
        Err(e) => anyhow::bail!("failed to stat local {local_path:?}: {e:#}"),
    };

    match (local_is_dir, meta.is_dir) {
        // Remote dir already exists locally
        (Some(true), true) => Ok(None),

        // Remote file exists locally as a file, need to check whether the contents match
        (Some(false), false) => Ok(Some(local_path)),

        // Remote file missing locally
        (None, false) => Ok(Some(local_path)),

        // Remote dir missing locally
        (None, true) => create_dir(&local_path, dry_run).await,

        // Remote is file but local is dir
        (Some(true), false) => {
            log_replace(dry_run, &local_path, true, false);
            if dry_run {
                return Ok(None);
            }

            remove_local(&local_path, true)?;
            Ok(Some(local_path))
        }

        // Remote is dir but local is file
        (Some(false), true) => {
            log_replace(dry_run, &local_path, false, true);
            if dry_run {
                return Ok(None);
            }
            remove_local(&local_path, false)?;
            create_dir(&local_path, dry_run).await
        }
    }
}

fn log_replace(dry_run: bool, local: &Path, local_is_dir: bool, remote_is_dir: bool) {
    let verb = if dry_run {
        "would replace"
    } else {
        "replacing"
    };
    let local_kind = if local_is_dir { "dir" } else { "file" };
    let remote_kind = if remote_is_dir { "dir" } else { "file" };
    log::info!("{verb} mismatched {local:?} (local is {local_kind}, remote is {remote_kind})");
}

fn resolve_local_path(meta: &FileMeta, mappings: &[PathMapping]) -> anyhow::Result<PathBuf> {
    let mapping = best_prefix_match(&meta.path, mappings)
        .cloned()
        .with_context(|| format!("received file meta for unwatched path: {:?}", meta.path))?;

    let relative = meta.path.strip_prefix(&mapping.strip).with_context(|| {
        format!(
            "failed to relativize {:?} under {:?}",
            meta.path, mapping.strip
        )
    })?;

    Ok(mapping.local.join(relative))
}

async fn create_dir(local: &Path, dry_run: bool) -> anyhow::Result<Option<PathBuf>> {
    if dry_run {
        log::info!("would create dir {local:?}");
        return Ok(None);
    }

    log::info!("creating dir {local:?}");
    tokio::fs::create_dir_all(local)
        .await
        .with_context(|| format!("failed to create dir {local:?}"))?;

    Ok(None)
}

fn remove_local(local: &Path, is_dir: bool) -> anyhow::Result<()> {
    if is_dir {
        std::fs::remove_dir_all(local).with_context(|| format!("failed to remove dir {local:?}"))
    } else {
        std::fs::remove_file(local).with_context(|| format!("failed to remove file {local:?}"))
    }
}

/// Why local file state does not match remote `meta`
pub(crate) enum MismatchReason {
    /// No local file exists
    Missing,

    /// Local and remote differ in file vs dir
    TypeMismatch {
        local_is_dir: bool,
        remote_is_dir: bool,
    },

    /// Same mtime but different sizes
    Size { local: u64, remote: u64 },

    /// Same size but different mtimes
    Mtime { local: u64, remote: u64 },
}

impl std::fmt::Display for MismatchReason {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Missing => write!(f, "local missing"),
            Self::TypeMismatch {
                local_is_dir,
                remote_is_dir,
            } => write!(
                f,
                "type mismatch (local is {}, remote is {})",
                if *local_is_dir { "dir" } else { "file" },
                if *remote_is_dir { "dir" } else { "file" },
            ),
            Self::Size { local, remote } => write!(
                f,
                "size mismatch (local {} vs remote {})",
                human_bytes(*local),
                human_bytes(*remote)
            ),
            Self::Mtime { local, remote } => {
                write!(f, "mtime mismatch (local {local} vs remote {remote})")
            }
        }
    }
}

/// Returns `None` when `local` matches `remote_meta`, otherwise the reason they differ.
pub(crate) fn mismatch_reason(
    local: &Path,
    remote_meta: &FileMeta,
    tolerance_nanos: u64,
) -> anyhow::Result<Option<MismatchReason>> {
    let local_meta = match std::fs::metadata(local) {
        Ok(md) => md,
        Err(e) if e.kind() == ErrorKind::NotFound => return Ok(Some(MismatchReason::Missing)),
        Err(e) => anyhow::bail!("failed to stat local {local:?}: {e:#}"),
    };

    if local_meta.is_dir() || remote_meta.is_dir {
        let local_is_dir = local_meta.is_dir();
        if local_is_dir == remote_meta.is_dir {
            return Ok(None);
        }

        return Ok(Some(MismatchReason::TypeMismatch {
            local_is_dir,
            remote_is_dir: remote_meta.is_dir,
        }));
    }

    if local_meta.len() != remote_meta.size {
        return Ok(Some(MismatchReason::Size {
            local: local_meta.len(),
            remote: remote_meta.size,
        }));
    }

    let local_mtime = local_mtime_nanos(&local_meta)?;
    if local_mtime.abs_diff(remote_meta.mtime_nanos) > tolerance_nanos {
        return Ok(Some(MismatchReason::Mtime {
            local: local_mtime,
            remote: remote_meta.mtime_nanos,
        }));
    }

    Ok(None)
}

pub(crate) fn set_syncing_path(state_tx: &StateBrokerTx, meta: &FileMeta, local: &Path) {
    if let Err(e) = state_tx.try_send(StateBrokerMessage::SetSyncingPath {
        remote_file_path: meta.path.to_string_lossy().into_owned(),
        local_file_path: local.to_string_lossy().into_owned(),
    }) {
        log::error!("failed to send sync state update: {e:?}");
    }
}

fn local_mtime_nanos(md: &std::fs::Metadata) -> anyhow::Result<u64> {
    Ok(md
        .modified()
        .with_context(|| "failed to read local mtime")?
        .duration_since(UNIX_EPOCH)
        .map_err(|e| anyhow::anyhow!("local mtime before epoch: {e:?}"))?
        .as_nanos() as u64)
}

fn report_progress(
    state_tx: &StateBrokerTx,
    written: u64,
    total: u64,
    started: Instant,
    last_percentage: &mut Option<u8>,
) {
    let percentage = written
        .saturating_mul(100)
        .checked_div(total)
        .map(|p| p.min(100) as u8)
        .unwrap_or(100);

    if *last_percentage == Some(percentage) {
        return;
    }

    *last_percentage = Some(percentage);

    let elapsed = started.elapsed().as_secs_f64().max(0.001);
    let speed = written as f64 / elapsed;
    let remaining = if written >= total {
        0
    } else {
        ((total - written) as f64 / speed.max(1.0)) as u64
    };

    let _ = state_tx.try_send(StateBrokerMessage::SetSyncProgress {
        transferred: human_bytes(written),
        progress: percentage,
        transfer_speed: format!("{}/s", human_bytes(speed as u64)),
        remaining: fmt_hms(remaining),
    });
}

pub(crate) fn human_bytes(n: u64) -> String {
    const UNITS: &[&str] = &["B", "KiB", "MiB", "GiB", "TiB"];

    let mut v = n as f64;
    let mut unit = 0;
    while v >= 1024.0 && unit + 1 < UNITS.len() {
        v /= 1024.0;
        unit += 1;
    }

    if unit == 0 {
        format!("{n}B")
    } else {
        format!("{v:.1}{}", UNITS[unit])
    }
}

fn fmt_hms(total_secs: u64) -> String {
    format!(
        "{}:{:02}:{:02}",
        total_secs / 3600,
        (total_secs / 60) % 60,
        total_secs % 60
    )
}

/// Returns the mapping whose remote root best matches `remote_file_path`,
/// i.e. the longest-prefix (most shared parent directories) match.
fn best_prefix_match<'a>(
    remote_file_path: &'a Path,
    mappings: &'a [PathMapping],
) -> Option<&'a PathMapping> {
    mappings
        .iter()
        .filter(|mapping| remote_file_path.starts_with(&mapping.remote))
        .max_by_key(|mapping| mapping.remote.components().count())
}
