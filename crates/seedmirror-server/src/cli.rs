use std::path::PathBuf;

use clap::Parser;

#[derive(Parser, Debug)]
#[command(version, about)]
pub(crate) struct Args {
    // TODO: More platform-agnostic tempdir
    /// Path to unix domain socket used to communicate with server.
    #[arg(short, long, default_value_os_t = PathBuf::from("/tmp/seedmirror-server.sock"))]
    pub socket_path: PathBuf,

    /// Root path to watch.
    #[arg(short, long)]
    pub root_path: PathBuf,
}
