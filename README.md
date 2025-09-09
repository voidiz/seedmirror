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
seedmirror_server

# On the client
seedmirror_client --ssh-hostname myserver \
    -p /home/server/media/music:/mnt/storage/music/ \
    -p /home/server/media/videos:/mnt/storage/videos/
```

## docker

### seedmirror-client

To build the image for `seedmirror-client` (from the root of the repo):

```bash
docker build -f docker/seedmirror-client/Dockerfile . --tag seedmirror-client:0.1.0
```

## caveats

- When creating path mappings, no local destination path should be within another local destination path. Violating this might cause unexpected behavior since the destination path is determined by comparing the incoming file with the remote source path of each path mapping and choosing the one with the longest matching prefix.

  For example, the following is discouraged:

  - `/remote/media/:/local/media/`
  - `/remote/media/pictures/:/local/media/pictures/`

  But the following is okay:

  - `/remote/media/videos/:/local/media/videos/`
  - `/remote/media/pictures/:/local/media/pictures/`
