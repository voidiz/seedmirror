use std::{
    fs::remove_file,
    path::{Path, PathBuf},
    pin::Pin,
    process::Stdio,
    time::Duration,
};

use anyhow::Context;
use seedmirror_core::message::Message;
use tokio::{io::BufReader, net::UnixStream, process::Command, time::sleep};

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

    let ssh_cmd = format!(
        "{} -L {}:{} {}",
        args.ssh_cmd,
        args.local_socket_path.to_string_lossy(),
        args.socket_path.to_string_lossy(),
        args.ssh_hostname
    );

    let _ = Command::new("sh")
        .arg("-c")
        .arg(ssh_cmd)
        .stdin(Stdio::null())
        .spawn()
        .with_context(|| format!("failed to spawn ssh, command `{}`", args.ssh_cmd))?;

    let remote_watcher = new_remote_watcher(args.clone(), workqueue);
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
                self.workqueue
                    .push("__full_sync".to_string(), full_sync(self.args.clone()))
                    .await?;
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

async fn new_remote_watcher(args: Args, workqueue: Workqueue) -> anyhow::Result<()> {
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
        let rsync_cmd = construct_rsync_cmd(&args, remote_path, local_path);
        let dry_run_rsync_cmd = format!("{rsync_cmd} -n");
        let dry_run_output = run_with_output(&dry_run_rsync_cmd).await?;
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
        run_with_streaming_output(&rsync_cmd, |line| {
            let remote_file_path = remote_path.join(&line);
            let local_file_path = local_path.join(&line);
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
    let rsync_cmd = construct_rsync_cmd(&args, &remote_file_path, &local_file_path);

    log::info!(r#"syncing remote {remote_file_path:?} to local {local_file_path:?}"#);
    if !args.dry_run {
        let _ = run_with_output(&rsync_cmd).await?;
    }

    Ok(())
}

fn construct_rsync_cmd(args: &Args, remote_path: &Path, local_path: &Path) -> String {
    let ssh_hostname = &args.ssh_hostname;
    let rsync_base_cmd = &args.rsync_cmd;
    format!("{rsync_base_cmd} {ssh_hostname}:{remote_path:?} {local_path:?}")
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
