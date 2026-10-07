use std::{
    fs,
    path::Path,
    process::Command,
    thread,
    time::{Duration, Instant},
};

use anyhow::Context;
use seedmirror_test::{
    harness::{Entry, Fixture},
    process::ProcessGuard,
};

const BIG_FILE_BYTES: u64 = 1 << 30;

#[test]
#[ignore = "requires perf and perf_event access"]
fn profile_full_sync() -> anyhow::Result<()> {
    let fx = Fixture::new("profiling")
        .source("subdir/initial.txt", Entry::empty())
        .source("subdir/test/big.bin", Entry::zeros(BIG_FILE_BYTES))
        .build()?;

    fx.build("profiling")?;

    let out_dir = fx.workspace_dir.join("target/profiling");
    let server_data = out_dir.join("seedmirror-server.perf.data");
    let client_data = out_dir.join("seedmirror-client.perf.data");
    let _ = fs::remove_file(&server_data);
    let _ = fs::remove_file(&client_data);

    let server = ProcessGuard::spawn(&mut fx.server_cmd("profiling"))?;
    let perf_server = spawn_perf(server.id(), &server_data)?;

    let client = ProcessGuard::spawn(&mut fx.client_cmd("profiling"))?;
    let perf_client = spawn_perf(client.id(), &client_data)?;

    fx.wait_for_initial_sync()?;

    // Stop the profilers before their targets exit so the captures flush
    drop(perf_client);
    drop(perf_server);
    drop(client);
    drop(server);

    for (name, path) in [("server", &server_data), ("client", &client_data)] {
        anyhow::ensure!(
            wait_for_file(path, Duration::from_secs(10)),
            "perf did not write the {name} profile to {path:?}"
        );

        println!("{name} profile ready: {path:?}");
    }

    Ok(())
}

fn spawn_perf(pid: u32, out: &Path) -> anyhow::Result<ProcessGuard> {
    let mut cmd = Command::new("perf");
    cmd.args(["record", "-F", "99", "-g", "--call-graph", "dwarf", "-o"])
        .arg(out)
        .arg("-p")
        .arg(pid.to_string());

    ProcessGuard::spawn(&mut cmd).with_context(|| "failed to spawn perf (is it in PATH?)")
}

fn wait_for_file(path: &Path, timeout: Duration) -> bool {
    let start = Instant::now();
    while start.elapsed() < timeout {
        if fs::metadata(path).map(|m| m.len() > 0).unwrap_or(false) {
            return true;
        }

        thread::sleep(Duration::from_millis(100));
    }

    false
}
