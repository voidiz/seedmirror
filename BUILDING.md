# building seedmirror

## binary

### requirements

- rust 1.89.0+ (stable)

### building

```bash
cargo build --release
```

## docker (seedmirror-client)

### requirements

- docker

### building

```bash
docker build -f docker/seedmirror-client/Dockerfile . --tag seedmirror-client
```
