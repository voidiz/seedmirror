# seedmirror

## requirements

- openssh 6.7+ (for unix domain socket forwarding support)
- rsync

## running (examples)

To synchronize all files in `media/` on the server to `/mnt/storage/media/` on the client:

```bash
# On the server
seedmirror_server --root-path media/

# On the client
seedmirror_client --ssh-hostname myserver --destination-path /mnt/storage/media/
```
