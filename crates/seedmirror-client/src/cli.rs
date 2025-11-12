use std::path::PathBuf;

use clap::Parser;

#[derive(Clone, Parser, Debug)]
pub(crate) struct Args {
    /// Set the hostname to ssh to. Will be combined with `ssh-cmd`.
    #[arg(long)]
    pub(crate) ssh_hostname: String,

    /// Absolute paths to sync. Specify multiple times to sync multiple paths.
    ///
    /// For example:
    /// /home/my_server/files/:/home/my_computer/files/
    ///
    /// Remote paths are relative to the home directory of the server and local paths are relative
    /// to the working directory of the client process.
    #[arg(
        short = 'p',
        long = "path-mapping",
        value_name= "<REMOTE SOURCE PATH>:<LOCAL DESTINATION PATH>",
        value_parser = Self::parse_path_mapping,
        action = clap::ArgAction::Append
    )]
    pub path_mappings: Vec<(PathBuf, PathBuf)>,

    /// Perform full sync of remote directory upon connecting.
    #[arg(long, default_value_t = true)]
    pub initial_sync: bool,

    /// Preview all file changes through logs. No actual syncing of files (or full sync) will be
    /// done.
    #[arg(long, default_value_t = false)]
    pub dry_run: bool,

    /// rsync command used to synchronize files. Will be combined with `ssh-hostname` to rsync over ssh.
    #[arg(long, default_value_t = Self::default_rsync_cmd())]
    pub(crate) rsync_cmd: String,

    /// Set the ssh command for forwarding the unix domain socket from the server. Should not
    /// include the hostname since it is derived from `ssh-hostname`.
    #[arg(long, default_value_t = Self::default_ssh_cmd())]
    pub(crate) ssh_cmd: String,

    /// Path to unix domain socket to forward from server.
    #[arg(long, default_value_os_t = PathBuf::from("/tmp/seedmirror-server.sock"))]
    pub socket_path: PathBuf,

    /// Local path to forward unix domain socket to.
    #[arg(long, default_value_os_t = PathBuf::from("/tmp/forwarded-seedmirror-server.sock"))]
    pub local_socket_path: PathBuf,
}

impl Args {
    fn parse_path_mapping(s: &str) -> clap::error::Result<(PathBuf, PathBuf), String> {
        let parts: Vec<_> = s.split(':').collect();
        if parts.len() != 2 {
            return Err("expected <remote source path>:<local destination path>".into());
        }

        let remote_path = Self::parse_absolute_path(parts[0])?;
        let local_path = Self::parse_absolute_path(parts[1])?;

        Ok((remote_path, local_path))
    }

    fn default_rsync_cmd() -> String {
        // mkpath automatically creates destination path
        r#"rsync -ahz --partial --mkpath --out-format="%n""#.to_string()
    }

    fn default_ssh_cmd() -> String {
        "ssh -nNT".to_string()
    }

    fn parse_absolute_path(s: &str) -> clap::error::Result<PathBuf, String> {
        let path = PathBuf::from(s);
        if !path.is_absolute() {
            return Err(format!("expected {path:?} to be an absolute path"));
        }

        Ok(path)
    }
}
