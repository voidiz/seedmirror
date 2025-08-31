use clap::Parser;

#[derive(Parser, Debug)]
pub(crate) struct Cli {
    /// rsync command used to synchronize files. Will be combined with `ssh_cmd` to rsync over ssh.
    #[arg(short, long, default_value_t = Self::default_rsync_cmd())]
    rsync_cmd: String,

    /// Set the ssh command excluding the hostname (which is derived from `ssh_hostname`).
    #[arg(short, long, default_value_t = Self::default_ssh_cmd())]
    ssh_cmd: String,

    /// Set the hostname to ssh to. Will be combined with `ssh_cmd`.
    ssh_hostname: String,
}

impl Cli {
    fn default_rsync_cmd() -> String {
        "rsync -avhzP --info=progress2".to_string()
    }

    fn default_ssh_cmd() -> String {
        "ssh".to_string()
    }
}
