use std::{fs, time::Duration};

use seedmirror_test::{
    harness::{Entry, Fixture},
    path::assert_dst_contains_src,
    process::ProcessGuard,
};

#[test]
fn test_full_sync() -> anyhow::Result<()> {
    let fx = Fixture::new("sync_test")
        .source("subdir/initial.txt", Entry::empty())
        .target("existing_file.txt", Entry::empty())
        .build()?;

    fx.build("release")?;

    let _server = ProcessGuard::spawn(&mut fx.server_cmd("release"))?;
    let _client = ProcessGuard::spawn(&mut fx.client_cmd("release"))?;

    fx.wait_for_initial_sync()?;
    assert_dst_contains_src(&fx.src, &fx.dst)?;

    // Check that existing files aren't removed
    fx.assert_target_preserved()?;

    // A file created after the initial sync is picked up by the watcher
    let new_file = "new_file.txt";
    fs::write(fx.src.join(new_file), b"")?;
    fx.wait_for(Duration::from_secs(10), || fx.dst.join(new_file).exists())?;

    assert_dst_contains_src(&fx.src, &fx.dst)?;
    Ok(())
}
