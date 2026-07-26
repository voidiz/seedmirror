#[derive(Debug)]
pub(crate) enum StatusMessage {
    // Sent every time the sync logic starts working on a new file
    SyncingPath {
        remote_file_path: String,
        local_file_path: String,
    },

    SyncProgress {
        // Transferred so far (bytes, with unit prefix)
        transferred: String,

        // Progress, percentage
        progress: String,

        // bytes/s, with unit prefix
        transfer_speed: String,

        // Estimated remining time (h:mm:ss)
        remaining: String,
    },
}
