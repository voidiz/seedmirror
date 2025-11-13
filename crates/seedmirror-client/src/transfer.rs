use std::{
    fs::remove_file,
    path::{Path, PathBuf},
    pin::Pin,
    process::Stdio,
    time::Duration,
};

use anyhow::Context;
use seedmirror_core::message::Message;
use tokio::{
    io::BufReader,
    net::UnixStream,
    process::{Child, Command},
    time::sleep,
};

use crate::{
    cli::Args,
    command::{run_with_output, run_with_streaming_output},
    workqueue::Workqueue,
};

type Task = Pin<Box<dyn Future<Output = anyhow::Result<()>> + Send>>;

pub(crate) fn init_remote_watcher(args: &Args, workqueue: Workqueue) -> anyhow::Result<Task> {
    if args.local_socket_path.try_exists()? {
        remove_file(&args.local_socket_path).with_context(|| {
            format!(
                "failed to remove existing socket: {:?}",
                args.local_socket_path
            )
        })?;
    }

    let ssh_child = Command::new("ssh")
        .kill_on_drop(true)
        .arg(&args.ssh_hostname)
        .arg("-nNT")
        .arg("-L")
        .arg(format!(
            "{}:{}",
            args.local_socket_path.to_string_lossy(),
            args.socket_path.to_string_lossy()
        ))
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .spawn()
        .with_context(|| "failed to spawn ssh")?;

    let remote_watcher = new_remote_watcher(args.clone(), workqueue, ssh_child);
    Ok(Box::pin(remote_watcher))
}

struct RemoteWatcher {
    /// Program arguments.
    args: Args,

    /// Queue for sync tasks.
    workqueue: Workqueue,
}

impl RemoteWatcher {
    pub(crate) fn new(args: Args, workqueue: Workqueue) -> Self {
        Self { args, workqueue }
    }

    async fn handle_message(&mut self, msg: Message) -> anyhow::Result<()> {
        match msg {
            Message::Connected => {
                log::debug!("received `Connected` answer from server ",);
                if self.args.initial_sync {
                    self.workqueue
                        .push("__full_sync".to_string(), full_sync(self.args.clone()))
                        .await?;
                }
            }
            Message::FileUpdated { path } => {
                let id = path.to_string_lossy().into_owned();
                self.workqueue
                    .push(id, sync_file(self.args.clone(), path))
                    .await?;
            }
            _ => (),
        };

        Ok(())
    }
}

async fn new_remote_watcher(
    args: Args,
    workqueue: Workqueue,
    // Only kept around so that the process is killed when the program stops and the value is
    // dropped.
    _ssh_child: Child,
) -> anyhow::Result<()> {
    let local_socket_path = &args.local_socket_path;
    log::info!("waiting for {local_socket_path:?} to be created");
    wait_for_file(local_socket_path).await;

    log::info!("connecting to {local_socket_path:?}");
    let mut stream = UnixStream::connect(&local_socket_path)
        .await
        .with_context(|| format!("failed to connect to socket at {local_socket_path:?}"))?;
    log::info!("connected to {local_socket_path:?}");

    let req = Message::ConnectionRequest {
        watched_paths: args
            .path_mappings
            .iter()
            .map(|(remote, _local)| remote.clone())
            .collect(),
    };
    req.write_to_stream(&mut stream).await?;

    let mut watcher = RemoteWatcher::new(args, workqueue);
    let mut reader = BufReader::new(stream);

    loop {
        let msg = Message::read_from_reader(&mut reader).await?;
        watcher.handle_message(msg).await?;
    }
}

async fn wait_for_file(path: &Path) {
    // TODO: Use file watcher at some point
    while !path.exists() {
        sleep(Duration::from_millis(100)).await;
    }
}

async fn full_sync(args: Args) -> anyhow::Result<()> {
    log::info!("performing full sync...");

    for (remote_path, local_path) in &args.path_mappings {
        let (rsync_dry_run_cmd, rsync_dry_run_args) =
            construct_rsync_cmd(&args, remote_path, local_path, true);
        let dry_run_output = run_with_output(rsync_dry_run_cmd, rsync_dry_run_args).await?;

        let fs_entries = dry_run_output.lines().collect::<Vec<_>>();
        let fs_entries_amount = fs_entries.len();
        if fs_entries_amount == 0 {
            log::info!("no difference between remote {remote_path:?} and local {local_path:?}");
            continue;
        }

        let diff_msg = format!(
            "found difference between remote {remote_path:?} and local {local_path:?}. syncing {fs_entries_amount} filesystem entries"
        );
        if args.dry_run {
            log::info!("{diff_msg}: {fs_entries:?}");
            continue;
        }

        log::info!("{diff_msg}");

        let (rsync_cmd, rsync_args) = construct_rsync_cmd(&args, remote_path, local_path, false);
        run_with_streaming_output(rsync_cmd, rsync_args, |line| {
            let line_trimmed = line.trim_matches('"');
            let remote_file_path = remote_path.join(line_trimmed);
            let local_file_path = local_path.join(line_trimmed);
            log::info!(r#"syncing remote {remote_file_path:?} to local {local_file_path:?}"#);
        })
        .await?;
    }

    log::info!("full sync done");
    Ok(())
}

async fn sync_file(args: Args, remote_file_path: PathBuf) -> anyhow::Result<()> {
    let (remote_path, local_path) = best_prefix_match(&remote_file_path, &args.path_mappings).ok_or(anyhow::anyhow!(
        "found no watched remote path that matches the incoming remote file: {remote_file_path:?}"
    ))?;

    let relative_path = remote_file_path.strip_prefix(remote_path)?;
    let local_file_path = local_path.join(relative_path);
    let (rsync_cmd, rsync_args) =
        construct_rsync_cmd(&args, &remote_file_path, &local_file_path, false);

    log::info!(r#"syncing remote {remote_file_path:?} to local {local_file_path:?}"#);
    if !args.dry_run {
        let _ = run_with_output(rsync_cmd, rsync_args).await?;
    }

    Ok(())
}

fn construct_rsync_cmd<'a>(
    args: &'a Args,
    remote_path: &'a Path,
    local_path: &'a Path,
    dry_run: bool,
) -> (&'a str, Vec<String>) {
    let ssh_hostname = &args.ssh_hostname;
    let mut args = vec![
        "-ahz".to_string(),
        "--partial".to_string(),
        "--mkpath".to_string(), // automatically create destination path
        r#"--out-format="%n""#.to_string(),
        format!("{}:{}", ssh_hostname, remote_path.to_string_lossy()),
        local_path.to_string_lossy().to_string(),
    ];

    if dry_run {
        args.push("-n".to_string());
    }

    ("rsync", args)
}

/// Returns the mapping that best matches `remote_file_path` based on the remote path with the
/// longest prefix (amount of shared parent directories).
fn best_prefix_match<'a>(
    remote_file_path: &'a Path,
    mappings: &'a [(PathBuf, PathBuf)],
) -> Option<&'a (PathBuf, PathBuf)> {
    mappings
        .iter()
        .filter(|(remote, _local)| remote_file_path.starts_with(remote))
        .max_by_key(|(remote, _local)| remote.components().count())
}
