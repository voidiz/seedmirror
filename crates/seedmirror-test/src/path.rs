use std::{fs, path::Path};

use anyhow::Context;

pub fn assert_dst_contains_src(src: &Path, dst: &Path) -> anyhow::Result<()> {
    for entry_res in fs::read_dir(src)? {
        let entry = entry_res?.path();
        let dst_path = dst.join(
            entry
                .file_name()
                .with_context(|| "invalid final path component".to_string())?,
        );

        if !dst_path.exists() {
            anyhow::bail!("expected {dst_path:?} to exist");
        }

        if entry.is_dir() && dst_path.is_dir() {
            assert_dst_contains_src(&entry, &dst_path)?;
        } else if entry.is_file() && dst_path.is_file() {
            continue;
        } else {
            anyhow::bail!("unexpected type mismatch between src: {entry:?} and dst: {dst_path:?}");
        }
    }

    Ok(())
}

pub fn dir_contains(src: &Path, dst: &Path) -> bool {
    assert_dst_contains_src(src, dst).is_ok()
}
