#!/usr/bin/env bash
# Clean-room install test for the mhr .deb package: downloads a release
# asset (or takes a local file), verifies its checksum, installs it via
# apt in a throwaway ubuntu:24.04 container, and confirms the GUI actually
# renders and hot-reloads under Xvfb rather than just checking the binary
# runs `--version`.
set -euo pipefail

REPO="chairulakmal/markdown-hot-reload"
TAG=""
DEB_PATH=""
KEEP=0
OUT_DIR=""

usage() {
  cat <<'EOF'
Usage: test-deb.sh [--tag <release-tag>] [--deb <local-file.deb>] [--keep] [--out-dir <dir>]

  --tag      Release tag to test (default: latest release on GitHub)
  --deb      Test a local .deb instead of downloading one from GitHub
  --keep     Leave the test container running afterwards, for debugging
  --out-dir  Where to write downloaded files and screenshots (default: mktemp)
EOF
}

while [[ $# -gt 0 ]]; do
  case "$1" in
    --tag) TAG="$2"; shift 2 ;;
    --deb) DEB_PATH="$(realpath "$2")"; shift 2 ;;
    --keep) KEEP=1; shift ;;
    --out-dir) OUT_DIR="$2"; shift 2 ;;
    -h|--help) usage; exit 0 ;;
    *) echo "unknown argument: $1" >&2; usage; exit 1 ;;
  esac
done

for bin in docker gh; do
  command -v "$bin" >/dev/null || { echo "missing required tool: $bin" >&2; exit 1; }
done
docker ps >/dev/null 2>&1 || { echo "docker daemon not reachable" >&2; exit 1; }

OUT_DIR="${OUT_DIR:-$(mktemp -d)}"
mkdir -p "$OUT_DIR"
CONTAINER="mhr-deb-test-$$"

cleanup() {
  if [[ "$KEEP" -eq 0 ]]; then
    docker rm -f "$CONTAINER" >/dev/null 2>&1 || true
  else
    echo "==> Container '$CONTAINER' left running for inspection: docker exec -it $CONTAINER bash"
  fi
}
trap cleanup EXIT

if [[ -z "$DEB_PATH" ]]; then
  echo "==> Fetching release assets (${TAG:-latest}) from $REPO"
  if [[ -n "$TAG" ]]; then
    gh release download "$TAG" --repo "$REPO" -p "*.deb" -p "SHA256SUMS" --clobber -D "$OUT_DIR"
  else
    gh release download --repo "$REPO" -p "*.deb" -p "SHA256SUMS" --clobber -D "$OUT_DIR"
  fi
  DEB_PATH=$(find "$OUT_DIR" -maxdepth 1 -name "*.deb" | head -1)
  [[ -n "$DEB_PATH" ]] || { echo "no .deb asset found on the release" >&2; exit 1; }
  if [[ -f "$OUT_DIR/SHA256SUMS" ]]; then
    echo "==> Verifying checksum"
    (cd "$OUT_DIR" && grep "$(basename "$DEB_PATH")" SHA256SUMS | sha256sum -c -)
  else
    echo "WARNING: no SHA256SUMS asset on this release, skipping checksum verification" >&2
  fi
fi

DEB_NAME=$(basename "$DEB_PATH")
echo "==> Testing $DEB_NAME"
dpkg-deb -I "$DEB_PATH"

echo "==> Starting clean ubuntu:24.04 container"
docker run -d --name "$CONTAINER" ubuntu:24.04 sleep infinity >/dev/null
docker cp "$DEB_PATH" "$CONTAINER:/root/$DEB_NAME"

echo "==> Installing via apt (resolves the WebKitGTK/GTK dependency chain)"
docker exec "$CONTAINER" bash -lc "apt-get update -qq && apt-get install -y -qq /root/$DEB_NAME"

echo "==> Checking installed binary"
docker exec "$CONTAINER" bash -lc "mhr --version && mhr --help"

echo "==> Installing Xvfb and screenshot tooling"
docker exec "$CONTAINER" bash -lc "DEBIAN_FRONTEND=noninteractive apt-get install -y -qq xvfb x11-utils imagemagick"

REPO_ROOT=$(git rev-parse --show-toplevel 2>/dev/null || echo "")
FIXTURE="$REPO_ROOT/fixtures/kitchen-sink.md"
if [[ -n "$REPO_ROOT" && -f "$FIXTURE" ]]; then
  echo "==> Using fixtures/kitchen-sink.md as the render fixture"
  docker cp "$FIXTURE" "$CONTAINER:/root/test.md"
else
  docker exec "$CONTAINER" bash -lc "printf '# Hello\n\nTesting **mhr** clean install.\n' > /root/test.md"
fi

echo "==> Launching mhr under Xvfb"
docker exec "$CONTAINER" bash -lc "
  xvfb-run -a --server-args='-screen 0 1280x1000x24' mhr /root/test.md > /root/mhr.log 2>&1 &
  disown
  sleep 4
"

MHR_PID=$(docker exec "$CONTAINER" bash -lc "pgrep -x mhr" || true)
if [[ -z "$MHR_PID" ]]; then
  echo "FAIL: mhr did not stay running. Log:" >&2
  docker exec "$CONTAINER" bash -lc "cat /root/mhr.log" >&2 || true
  exit 1
fi
echo "==> mhr running as pid $MHR_PID"

XAUTH=$(docker exec "$CONTAINER" bash -lc "find /tmp -iname Xauthority | head -1")
docker exec "$CONTAINER" bash -lc "DISPLAY=:99 XAUTHORITY=$XAUTH import -display :99 -window root /root/before.png"

echo "==> Editing the watched file on disk to check hot reload"
docker exec "$CONTAINER" bash -lc "printf '\n## Reload check\n\nIf you can read this, the watcher reloaded after the file changed on disk.\n' >> /root/test.md"
sleep 2
docker exec "$CONTAINER" bash -lc "DISPLAY=:99 XAUTHORITY=$XAUTH import -display :99 -window root /root/after.png"

docker cp "$CONTAINER:/root/before.png" "$OUT_DIR/before.png"
docker cp "$CONTAINER:/root/after.png" "$OUT_DIR/after.png"
docker exec "$CONTAINER" bash -lc "cat /root/mhr.log" > "$OUT_DIR/mhr.log" || true

echo "==> Done."
echo "==> Screenshots: $OUT_DIR/before.png $OUT_DIR/after.png"
echo "==> Log: $OUT_DIR/mhr.log"
echo "==> Read both screenshots to confirm rendering and that the reload picked up the appended section."
