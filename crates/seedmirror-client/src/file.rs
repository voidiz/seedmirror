use std::{
    io::ErrorKind,
    path::{Path, PathBuf},
    time::{Duration, UNIX_EPOCH},
};

use anyhow::Context;

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
