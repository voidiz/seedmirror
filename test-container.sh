#!/usr/bin/env bash

set -euo pipefail

IMAGE_NAME="seedmirror-test"
CONTAINER_NAME="seedmirror-test"

echo "Building Docker image ${IMAGE_NAME}..."
docker build -t "${IMAGE_NAME}" ./docker/seedmirror-test

if [[ $(docker ps -q -f name=^/${CONTAINER_NAME}$) ]]; then
    echo "Attaching to existing container '${CONTAINER_NAME}'..."
    docker exec -it "${CONTAINER_NAME}" /bin/bash
    exit 0
fi

echo "Starting new container '${CONTAINER_NAME}'..."
docker run -it \
  --rm \
  --name "${CONTAINER_NAME}" \
  -v "$(pwd):/workspace" \
  -p 2222:22 \
  "${IMAGE_NAME}"
