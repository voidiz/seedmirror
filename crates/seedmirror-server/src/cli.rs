use std::{path::PathBuf, time::Duration};

use clap::Parser;

#[derive(Parser, Debug, Clone)]
#[command(version, about)]
pub(crate) struct Args {
    // TODO: More platform-agnostic tempdir
    /// Path to unix domain socket used to communicate with server.
    #[arg(short, long, default_value_os_t = PathBuf::from("/tmp/seedmirror-server.sock"))]
    pub socket_path: PathBuf,

    /// Delay in milliseconds before file modifications are reported to the client.
    #[arg(long, default_value = "10000", value_parser = Self::parse_millis)]
    pub sync_delay: Duration,
}

impl Args {
    fn parse_millis(s: &str) -> clap::error::Result<Duration, String> {
        s.parse::<u64>()
            .map(Duration::from_millis)
            .map_err(|e| format!("invalid duration '{}': {}", s, e))
    }
}
