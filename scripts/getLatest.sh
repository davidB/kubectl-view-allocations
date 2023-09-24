#!/bin/bash

# The install script is licensed under the CC-0 1.0 license.

# See https://github.com/davidB/kubectl-view-allocations/blob/master/LICENSE for more details.
#
# To run this script execute:
#   `curl https://raw.githubusercontent.com/davidB/kubectl-view-allocations/master/scripts/getLatest.sh | sh`

GITHUB_REPO="kubectl-view-allocations"
GITHUB_USER="davidB"
EXE_FILENAME="kubectl-view-allocations"

bye() {
  result=$?
  if [ "$result" != "0" ]; then
    echo "Fail to install ${GITHUB_USER}/${GITHUB_REPO}"
  fi
  exit $result
}

fail() {
  echo "$1"
  exit 1
}

find_download_url() {
  local SUFFIX=$1
  URL=$(curl -s https://api.github.com/repos/${GITHUB_USER}/${GITHUB_REPO}/releases/latest |
    grep "browser_download_url.*${SUFFIX}" |
    cut -d : -f 2,3 |
    tr -d \" |
    head -n 1)
  echo "${URL//[[:space:]]/}"
}

find_arch() {
  ARCH=$(uname -m)
  case $ARCH in
  armv5*) ARCH="armv5" ;;
  armv6*) ARCH="armv6" ;;
  armv7*) ARCH="armv7" ;;
  aarch64) ARCH="arm64" ;;
  x86) ARCH="386" ;;
  # x86_64) ARCH="amd64";;
  i686) ARCH="386" ;;
  i386) ARCH="386" ;;
  esac
  echo $ARCH
}

find_os() {
  UNAME=$(uname)
  OS=$(echo "$UNAME" | tr '[:upper:]' '[:lower:]')

  case "$OS" in
  # Minimalist GNU for Windows
  mingw*) OS='windows' ;;
  msys*) OS='windows' ;;
  esac
  echo "$OS"
}

find_suffix() {
  local ARCH=$1
  local OS=$2
  local SUFFIX="$ARCH-$OS.tar.gz"
  case "$SUFFIX" in
  "x86_64-darwin.tar.gz") SUFFIX='x86_64-apple-darwin.tar.gz' ;;
  "arm64-darwin.tar.gz") SUFFIX='aarch64-apple-darwin.tar.gz' ;;
  "x86_64-linux.tar.gz") SUFFIX='x86_64-unknown-linux-musl.tar.gz' ;;
  "arm64-linux.tar.gz") SUFFIX='aarch64-unknown-linux-musl.tar.gz' ;;
    # "x86_64-windows.tar.gz") SUFFIX='x86_64-pc-windows-msvc.zip';;
  esac
  echo "$SUFFIX"
}

download_file() {
  local FILE_URL="$1"
  local FILE_PATH="$2"
  echo "Getting $FILE_URL"
  httpStatusCode=$(curl -s -w '%{http_code}' -L "$FILE_URL" -o "$FILE_PATH")
  if [ "$httpStatusCode" != 200 ]; then
    echo "failed to download '${URL}'"
    fail "Request fail with http status code $httpStatusCode"
  fi
}

find_exec_dest_path() {
  local DEST_DIR="/usr/local/bin"
  if [ ! -w $DEST_DIR ]; then
    DEST_DIR=$(pwd)
  fi
  echo "${DEST_DIR}"
}

install_file() {
  local FILE_PATH=$1
  local EXE_DEST_FILE=$2
  TMP_DIR="/tmp/${GITHUB_USER}_${GITHUB_REPO}"
  mkdir -p "$TMP_DIR" || true
  tar xf "$FILE_PATH" -C "$TMP_DIR"
  if [ -f "$TMP_DIR/${EXE_FILENAME}" ]; then
    cp "$TMP_DIR/${EXE_FILENAME}" "${EXE_DEST_FILE}"
  else
    for dir in "$TMP_DIR"/*/; do
      if [ -f "$dir${EXE_FILENAME}" ]; then
        cp "$dir${EXE_FILENAME}" "${EXE_DEST_FILE}"
        break
      fi
    done
  fi

  chmod +x "${EXE_DEST_FILE}"
  rm -rf "$TMP_DIR"
}

main() {
  EXE_DEST_DIR=$(find_exec_dest_path)
  EXE_DEST_FILE="${EXE_DEST_DIR}/${EXE_FILENAME}"
  ARCH=$(find_arch)
  OS=$(find_os)
  SUFFIX=$(find_suffix "$ARCH" "$OS")
  FILE_URL=$(find_download_url "$SUFFIX")
  FILE_PATH="/tmp/${GITHUB_USER}-${GITHUB_REPO}-latest-${SUFFIX}"
  if [ -z "${FILE_URL}" ]; then
    fail "Did not find a release for your system: $OS $ARCH"
  fi
  download_file "${FILE_URL}" "${FILE_PATH}"
  install_file "${FILE_PATH}" "${EXE_DEST_FILE}"
  rm -Rf "${FILE_PATH}"
  echo "executable installed at ${EXE_DEST_FILE}"
  bye
}

#Stop execution on any error
trap "bye" EXIT
set -e
# set -x
main
