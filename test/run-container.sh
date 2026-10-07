#!/usr/bin/env bash

set -euo pipefail

IMAGE_NAME="seedmirror-test"
CONTAINER_NAME="seedmirror-test"

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"

if [[ $(docker ps -q -f name=^/${CONTAINER_NAME}$) ]]; then
    echo "Attaching to existing container '${CONTAINER_NAME}'..."
    docker exec -it "${CONTAINER_NAME}" /bin/bash
    exit 0
fi

echo "Starting new container '${CONTAINER_NAME}'..."
docker run -it \
  --rm \
  --name "${CONTAINER_NAME}" \
  -v "${REPO_ROOT}:/workspace" \
  -p 8080:8080 \
  --cap-add=PERFMON \
  --cap-add=SYS_PTRACE \
  "${IMAGE_NAME}"
