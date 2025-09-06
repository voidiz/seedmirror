use std::path::PathBuf;

use clap::Parser;

#[derive(Parser, Debug)]
pub(crate) struct Args {
    /// rsync command used to synchronize files. Will be combined with `ssh_cmd` to rsync over ssh.
    #[arg(long, default_value_t = Self::default_rsync_cmd())]
    pub(crate) rsync_cmd: String,

    /// Set the ssh command excluding the hostname (which is derived from `ssh_hostname`).
    #[arg(long, default_value_t = Self::default_ssh_cmd())]
    pub(crate) ssh_cmd: String,

    /// Set the hostname to ssh to. Will be combined with `ssh_cmd`.
    #[arg(long)]
    pub(crate) ssh_hostname: String,

    /// Path to unix domain socket to forward from server.
    #[arg(long, default_value_os_t = PathBuf::from("/tmp/seedmirror-server.sock"))]
    pub socket_path: PathBuf,

    /// Local path to forward unix domain socket to.
    #[arg(long, default_value_os_t = PathBuf::from("/tmp/forwarded-seedmirror-server.sock"))]
    pub local_socket_path: PathBuf,
}

impl Args {
    fn default_rsync_cmd() -> String {
        "rsync -avhzP --info=progress2".to_string()
    }

    fn default_ssh_cmd() -> String {
        "ssh -nNT".to_string()
    }
}
