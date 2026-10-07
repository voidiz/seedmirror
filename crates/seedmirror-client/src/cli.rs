use std::path::PathBuf;

use clap::Parser;

#[derive(Clone, Parser, Debug)]
pub(crate) struct Args {
    /// Set the hostname to ssh to.
    #[arg(long)]
    pub ssh_hostname: String,

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
    pub path_mappings: Vec<PathMapping>,

    /// Disable the full sync of every mapped path upon connecting.
    #[arg(long)]
    pub no_initial_sync: bool,

    /// Preview all file changes through logs. No actual syncing of files (or full sync) will be
    /// done.
    #[arg(long, default_value_t = false)]
    pub dry_run: bool,

    /// Control the tolerance window of the modified time (mtime) check when syncing a file. If the
    /// mtime of the local file and remote file differ by less than this value and all other
    /// heuristics (file size) are the same, the file will not be synced.
    #[arg(long, default_value_t = 1, value_name = "SECONDS")]
    pub modify_window: u64,

    /// Path to unix domain socket to forward from server.
    #[arg(long, default_value_os_t = PathBuf::from("/tmp/seedmirror-server.sock"))]
    pub socket_path: PathBuf,

    /// Local path to forward unix domain socket to.
    #[arg(long, default_value_os_t = PathBuf::from("/tmp/forwarded-seedmirror-server.sock"))]
    pub local_socket_path: PathBuf,

    /// Whether the GUI HTTP server is enabled.
    #[cfg(feature = "gui")]
    #[arg(long, default_value_t = false)]
    pub gui: bool,

    /// Address to bind the GUI HTTP server to.
    #[cfg(feature = "gui")]
    #[arg(long, default_value_t = "0.0.0.0:8080".to_string())]
    pub http_addr: String,
}

#[derive(Clone, Debug)]
pub(crate) struct PathMapping {
    /// Remote root to watch and list (trailing slash normalized away).
    pub remote: PathBuf,

    /// Local destination root.
    pub local: PathBuf,

    /// The base path used to calculate the local path from the remote path.
    /// It is `remote` for `src/`, the parent of `remote` for `src`.
    pub strip: PathBuf,
}

impl Args {
    fn parse_path_mapping(s: &str) -> clap::error::Result<PathMapping, String> {
        let parts = s.split(':').collect::<Vec<_>>();
        let [remote_str, local_str] = parts[..] else {
            return Err("expected <remote source path>:<local destination path>".into());
        };

        let remote_path = Self::parse_absolute_path(remote_str)?;
        let local_path = Self::parse_absolute_path(local_str)?;

        let strip = if remote_str.ends_with('/') {
            remote_path.clone()
        } else {
            // Unreachable for `/`. Since it ends with `/` it falls under the branch above.
            // `parent()` is `None` only for root.
            remote_path
                .parent()
                .map(|p| p.to_path_buf())
                .ok_or_else(|| format!("cannot place filesystem root into target: {s}"))?
        };

        Ok(PathMapping {
            remote: remote_path,
            local: local_path,
            strip,
        })
    }

    fn parse_absolute_path(s: &str) -> clap::error::Result<PathBuf, String> {
        let path = PathBuf::from(s);
        if !path.is_absolute() {
            return Err(format!("expected {path:?} to be an absolute path"));
        }

        Ok(path)
    }
}
