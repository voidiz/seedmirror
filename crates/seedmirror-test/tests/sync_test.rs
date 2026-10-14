use std::{fs, time::Duration};

use seedmirror_test::{
    harness::{Entry, Fixture},
    path::assert_dst_contains_src,
};

#[test]
fn test_full_sync() -> anyhow::Result<()> {
    let fx = Fixture::new("sync_test")
        .source("subdir/initial.txt", Entry::empty())
        .target("existing_file.txt", Entry::empty())
        .build()?;

    fx.build("release")?;

    let (_server, _client) = fx.spawn_and_sync("release")?;

    assert_dst_contains_src(&fx.src, &fx.dst)?;

    // Check that existing files aren't removed
    fx.assert_target_preserved()?;

    // A file created after the initial sync is picked up by the watcher
    let new_file = "new_file.txt";
    fs::write(fx.src.join(new_file), b"")?;
    fx.wait_for(Duration::from_secs(2), || fx.dst.join(new_file).exists())?;

    assert_dst_contains_src(&fx.src, &fx.dst)?;
    Ok(())
}

/// inotify will not report all files recursively when moving a directory or creating a directory
/// hard link. Test that we traverse the directory ourselves.
#[test]
fn test_moved_in_dir_syncs_contents() -> anyhow::Result<()> {
    let fx = Fixture::new("moved_dir_test")
        .source("initial.txt", Entry::empty())
        .build()?;
    fx.build("release")?;

    let (_server, _client) = fx.spawn_and_sync("release")?;

    let staging = fx.tmp.path().join("staging").join("files");
    fs::create_dir_all(&staging)?;
    fs::write(staging.join("notes.txt"), b"contents")?;

    let dest = fx.src.join("files");
    fs::rename(&staging, &dest)?;

    fx.wait_for(Duration::from_secs(2), || {
        fx.dst.join("files/notes.txt").exists()
    })?;

    assert_dst_contains_src(&fx.src, &fx.dst)?;
    Ok(())
}
