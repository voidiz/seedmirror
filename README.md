# seedmirror

## requirements

- openssh 6.7+ (for unix domain socket forwarding support)
- rsync 3.2.3+ (for --mkpath flag)

## running (example)

To synchronize all files in

- `/home/server/media/music/` on the server to `/mnt/storage/music/` on the client
- `/home/server/media/videos/` on the server to `/mnt/storage/videos/` on the client

```bash
# On the server
seedmirror-server

# On the client
seedmirror-client --ssh-hostname myserver \
    -p /home/server/media/music/:/mnt/storage/music/ \
    -p /home/server/media/videos/:/mnt/storage/videos/
```

### logging

The info log level is set by default for both the server and the client. It can be modified by changing the `RUST_LOG` environment variable as described [here](https://docs.rs/env_logger/latest/env_logger/#enabling-logging).

## docker

### seedmirror-client

To build the image for `seedmirror-client` (from the root of the repo):

```bash
docker build -f docker/seedmirror-client/Dockerfile . --tag seedmirror-client:0.1.0
```

To run the same (non-Docker) example described above:

```bash
docker run \
  --rm \
  -e PUID=$(id -u) \
  -e PGID=$(id -g) \
  -v "$HOME/.ssh:/config/.ssh" \
  -v "/mnt/storage:/storage" \
  seedmirror-client:0.1.0 \
  --ssh-hostname myserver \
  -p /home/server/media/music/:/storage/music/ \
  -p /home/server/media/videos/:/storage/videos/
```

Note that your ssh directory must be mounted at `/config/.ssh`. Furthermore, the values set for `$PUID` and `$PGID` should match the user ID and group ID of all mounted directories, respectively.

## caveats

- When creating path mappings, no local destination path should be within another local destination path. Violating this might cause unexpected behavior since the destination path is determined by comparing the incoming file with the remote source path of each path mapping and choosing the one with the longest matching prefix.

  For example, the following is discouraged:
  - `/remote/media/:/local/media/`
  - `/remote/media/pictures/:/local/media/pictures/`

  But the following is okay:
  - `/remote/media/videos/:/local/media/videos/`
  - `/remote/media/pictures/:/local/media/pictures/`
