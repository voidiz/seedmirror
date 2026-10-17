use std::{
    io::ErrorKind,
    path::{Path, PathBuf},
    time::{Duration, UNIX_EPOCH},
};

use anyhow::Context;
use seedmirror_core::message::FileMeta;

use crate::cli::PathMapping;

/// Generates a temp file path next to the destination.
pub(crate) fn tmp_path(local: &Path) -> PathBuf {
    let mut tmp = local.as_os_str().to_owned();
    tmp.push(".seedmirror-partial");
    PathBuf::from(tmp)
}

/// Clean up a partially transferred file.
pub(crate) async fn cleanup_partial(local: &Path) -> anyhow::Result<()> {
    match tokio::fs::remove_file(tmp_path(local)).await {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == ErrorKind::NotFound => Ok(()),
        Err(e) => Err(e).with_context(|| format!("failed to remove partial file for {local:?}")),
    }
}

/// Rename and set metadata on partially transferred file. Called when the file transfer is
/// completed.
pub(crate) async fn finalize_tmp(
    tmp_file: tokio::fs::File,
    local: &Path,
    mtime_nanos: u64,
) -> anyhow::Result<()> {
    let tmp = tmp_path(local);
    let mtime = UNIX_EPOCH + Duration::from_nanos(mtime_nanos);

    tmp_file
        .sync_all()
        .await
        .with_context(|| format!("failed to sync temp file {tmp:?}"))?;

    drop(tmp_file);

    let file = std::fs::File::options()
        .write(true)
        .open(&tmp)
        .with_context(|| format!("failed to reopen temp file {tmp:?}"))?;

    file.set_modified(mtime)
        .with_context(|| format!("failed to set mtime on {tmp:?}"))?;

    file.sync_all()
        .with_context(|| format!("failed to sync temp file {tmp:?}"))?;

    drop(file);

    tokio::fs::rename(&tmp, local)
        .await
        .with_context(|| format!("failed to move {tmp:?} to {local:?}"))?;

    Ok(())
}

pub(crate) async fn create_dir(local: &Path, dry_run: bool) -> anyhow::Result<Option<PathBuf>> {
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

pub(crate) fn remove_local(local: &Path, is_dir: bool) -> anyhow::Result<()> {
    if is_dir {
        std::fs::remove_dir_all(local).with_context(|| format!("failed to remove dir {local:?}"))
    } else {
        std::fs::remove_file(local).with_context(|| format!("failed to remove file {local:?}"))
    }
}

pub(crate) fn local_mtime_nanos(md: &std::fs::Metadata) -> anyhow::Result<u64> {
    Ok(md
        .modified()
        .with_context(|| "failed to read local mtime")?
        .duration_since(UNIX_EPOCH)
        .map_err(|e| anyhow::anyhow!("local mtime before epoch: {e:?}"))?
        .as_nanos() as u64)
}

pub(crate) fn resolve_local_path(
    meta: &FileMeta,
    mappings: &[PathMapping],
) -> anyhow::Result<PathBuf> {
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
