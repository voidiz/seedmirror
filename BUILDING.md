# building seedmirror

## binary

### requirements

- cargo

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
