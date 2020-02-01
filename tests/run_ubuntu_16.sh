#!/bin/bash

DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" >/dev/null 2>&1 && pwd)"

docker build -t ubuntu_curl:16.04 -f "${DIR}/ubuntu_16.dockerfile" "${DIR}"
docker run --rm \
  -v ${DIR}/..:/prj \
  -v $HOME/.kube:/root/.kube \
  -w /tmp \
  ubuntu_curl:16.04 \
  /prj/tests/test_first_run.sh

# docker run --rm -it \
#   -v ${DIR}/..:/prj \
#   -v $HOME/.kube:/root/.kube \
#   -w /tmp \
#   ubuntu:16.04 \
#   bash
