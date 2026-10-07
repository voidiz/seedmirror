# Show all recipes
default:
    @just --list

# Build the client and server (release)
build:
    cargo build --release

# Build for x86_64-unknown-linux-musl (static)
build-musl:
    cargo build --release --target x86_64-unknown-linux-musl

# Build with symbols for profiling (binaries in target/profiling/)
build-profiling:
    cargo build --profile profiling

# Run the functional sync test
test:
    cargo test -p seedmirror-test

# Build the test container image
build-test-image:
    docker build -t seedmirror-test ./docker/seedmirror-test

# Build the client release image
build-client-image tag="latest":
    docker build -f docker/seedmirror-client/Dockerfile -t seedmirror-client:{{tag}} .

# Enter (or create) the test container. Build the image first
test-container:
    ./test/run-container.sh

# Run the ignored profiling test. Run this inside the test container
profile-test:
    cargo test -p seedmirror-test --test profile_test -- --ignored --nocapture

# Serve the server perf profile in the browser via samply
view-server port="3000":
    samply import target/profiling/seedmirror-server.perf.data \
        --symbol-dir target/profiling --port {{port}} --no-open

# Serve the client perf profile in the browser via samply
view-client port="3001":
    samply import target/profiling/seedmirror-client.perf.data \
        --symbol-dir target/profiling --port {{port}} --no-open
