# seedmirror

seedmirror is a utility to monitor remote filesystem changes and automatically synchronize them to a local directory using rsync over ssh.

## running (binary)

- openssh 6.7+ (for unix domain socket forwarding support, client and server)
- rsync 3.2.3+ (for --mkpath flag, client only)

Download the latest release [here](https://github.com/voidiz/seedmirror/releases) or [build](BUILDING.md) the binaries yourself.

Example: To synchronize all files in

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

## running seedmirror-client (docker)

Pull the latest image from [here](https://github.com/voidiz/seedmirror/pkgs/container/seedmirror-client) or build it as described [here](BUILDING.md).

To run the same (non-Docker) example described above:

```bash
docker run \
  --rm \
  -e PUID=$(id -u) \
  -e PGID=$(id -g) \
  -v "$HOME/.ssh:/config/.ssh" \
  -v "/mnt/storage:/storage" \
  ghcr.io/voidiz/seedmirror-client \
  --ssh-hostname myserver \
  -p /home/server/media/music/:/storage/music/ \
  -p /home/server/media/videos/:/storage/videos/
```

Note that your ssh directory must be mounted at `/config/.ssh`. Furthermore, the values set for `$PUID` and `$PGID` should match the user ID and group ID of all mounted directories, respectively.

## configuration

### flags

See `seedmirror-client --help` or `seedmirror-server --help`.

### logging

The info log level is set by default for both the server and the client. It can be modified by changing the `RUST_LOG` environment variable as described [here](https://docs.rs/env_logger/latest/env_logger/#enabling-logging).

## caveats

- When creating path mappings, no local destination path should be within another local destination path. Violating this might cause unexpected behavior since the destination path is determined by comparing the incoming file with the remote source path of each path mapping and choosing the one with the longest matching prefix.

  For example, the following is discouraged:
  - `/remote/media/:/local/media/`
  - `/remote/media/pictures/:/local/media/pictures/`

  But the following is okay:
  - `/remote/media/videos/:/local/media/videos/`
  - `/remote/media/pictures/:/local/media/pictures/`
