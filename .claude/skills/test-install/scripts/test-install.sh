#!/usr/bin/env bash
# Clean-room test for the mhr distribution channels that CI's plain
# `cargo build` never exercises: the .deb (apt dependency resolution) and
# `cargo install` (the file set Cargo.toml's `exclude` actually ships).
# Both paths converge on the same check: confirm the GUI opens and
# hot-reloads under Xvfb, not just that the binary runs `--version`.
set -euo pipefail

REPO="chairulakmal/markdown-hot-reload"
TAG=""
DEB_PATH=""
CARGO_INSTALL_PATH=""
CARGO_INSTALL_VERSION=""
KEEP=0
OUT_DIR=""

usage() {
  cat <<'EOF'
Usage:
  test-install.sh [--tag <release-tag>] [--deb <local-file.deb>] [--keep] [--out-dir <dir>]
  test-install.sh --cargo-install-path <worktree-dir> [--keep] [--out-dir <dir>]
  test-install.sh --cargo-install-version <X.Y.Z> [--keep] [--out-dir <dir>]

Deb mode (default):
  --tag                     Release tag to test (default: latest release on GitHub)
  --deb                     Test a local .deb instead of downloading one from GitHub

Cargo install mode:
  --cargo-install-path      Test `cargo install` against a local worktree/checkout,
                             packaged exactly as `cargo publish` would ship it (runs
                             `cargo package` first, so Cargo.toml's `exclude` list is
                             actually exercised). Use this before `cargo publish`,
                             since a crates.io version can be yanked but never
                             replaced.
  --cargo-install-version   Test `cargo install mhr@<version>` from crates.io.
                             Use this after publishing, as a spot check.

Common:
  --keep     Leave the test container running afterwards, for debugging
  --out-dir  Where to write downloaded files and screenshots (default: mktemp)
EOF
}

while [[ $# -gt 0 ]]; do
  case "$1" in
    --tag) TAG="$2"; shift 2 ;;
    --deb) DEB_PATH="$(realpath "$2")"; shift 2 ;;
    --cargo-install-path) CARGO_INSTALL_PATH="$(realpath "$2")"; shift 2 ;;
    --cargo-install-version) CARGO_INSTALL_VERSION="$2"; shift 2 ;;
    --keep) KEEP=1; shift ;;
    --out-dir) OUT_DIR="$2"; shift 2 ;;
    -h|--help) usage; exit 0 ;;
    *) echo "unknown argument: $1" >&2; usage; exit 1 ;;
  esac
done

CARGO_MODE=0
if [[ -n "$CARGO_INSTALL_PATH" || -n "$CARGO_INSTALL_VERSION" ]]; then
  CARGO_MODE=1
fi
if [[ -n "$CARGO_INSTALL_PATH" && -n "$CARGO_INSTALL_VERSION" ]]; then
  echo "--cargo-install-path and --cargo-install-version are mutually exclusive" >&2
  exit 1
fi
if [[ "$CARGO_MODE" -eq 1 && ( -n "$DEB_PATH" || -n "$TAG" ) ]]; then
  echo "--cargo-install-* and --tag/--deb are mutually exclusive" >&2
  exit 1
fi

command -v docker >/dev/null || { echo "missing required tool: docker" >&2; exit 1; }
if [[ "$CARGO_MODE" -eq 0 && -z "$DEB_PATH" ]]; then
  command -v gh >/dev/null || { echo "missing required tool: gh" >&2; exit 1; }
fi
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

echo "==> Starting clean ubuntu:24.04 container"
docker run -d --name "$CONTAINER" ubuntu:24.04 sleep infinity >/dev/null

if [[ "$CARGO_MODE" -eq 1 ]]; then
  echo "==> Installing Rust toolchain and WebKitGTK build dependencies"
  docker exec "$CONTAINER" bash -lc "
    apt-get update -qq &&
    DEBIAN_FRONTEND=noninteractive apt-get install -y -qq curl ca-certificates tar build-essential pkg-config libwebkit2gtk-4.1-dev libgtk-3-dev &&
    curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y --default-toolchain stable -q
  "

  if [[ -n "$CARGO_INSTALL_PATH" ]]; then
    # `git worktree add` leaves a `.git` file pointing at an absolute host
    # path, which resolves to nothing inside the container. `git archive`
    # sidesteps that by extracting the exact tracked-file set at that
    # commit with no `.git` at all; with no VCS present, `cargo package`
    # falls back to "everything on disk minus Cargo.toml's exclude", which
    # composes with the archive's file set into the same result cargo's
    # own git integration would have produced.
    echo "==> Archiving worktree $CARGO_INSTALL_PATH into the container"
    git -C "$CARGO_INSTALL_PATH" archive --format=tar HEAD |
      docker exec -i "$CONTAINER" bash -c "mkdir -p /root/src && tar -x -C /root/src"

    echo "==> Packaging (applies Cargo.toml's exclude list, same as cargo publish) and verifying the build"
    docker exec "$CONTAINER" bash -lc "cd /root/src && cargo package --locked"
    PKG_DIR=$(docker exec "$CONTAINER" bash -lc "ls -d /root/src/target/package/*/" | head -1)
    echo "==> Installing from the packaged tree, not the worktree, so the exclude list is actually exercised"
    docker exec "$CONTAINER" bash -lc "cargo install --locked --path '$PKG_DIR'"
  else
    echo "==> Installing mhr@$CARGO_INSTALL_VERSION from crates.io"
    docker exec "$CONTAINER" bash -lc "cargo install --locked mhr@$CARGO_INSTALL_VERSION"
  fi
  docker exec "$CONTAINER" bash -lc "ln -sf \$HOME/.cargo/bin/mhr /usr/local/bin/mhr"
else
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

  docker cp "$DEB_PATH" "$CONTAINER:/root/$DEB_NAME"
  echo "==> Installing via apt (resolves the WebKitGTK/GTK dependency chain)"
  docker exec "$CONTAINER" bash -lc "apt-get update -qq && apt-get install -y -qq /root/$DEB_NAME"
fi

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
# Prepended, not appended. mhr keeps the scroll position across a reload, so a
# marker added to the end of a long fixture renders below the fold and the two
# screenshots come back looking the same whether the watcher fired or not. They
# can still differ byte for byte, so comparing the files does not help either.
# The rewrite truncates the same inode instead of renaming over it, which keeps
# the in-place save this check has always exercised; the rename path has its own
# test in src/watch.rs.
docker exec "$CONTAINER" bash -lc "
  { printf '# Reload check\n\nIf you can read this, the watcher reloaded after the file changed on disk.\n\n'; cat /root/test.md; } > /root/reloaded.md
  cat /root/reloaded.md > /root/test.md
  rm -f /root/reloaded.md
"
sleep 2
docker exec "$CONTAINER" bash -lc "DISPLAY=:99 XAUTHORITY=$XAUTH import -display :99 -window root /root/after.png"

docker cp "$CONTAINER:/root/before.png" "$OUT_DIR/before.png"
docker cp "$CONTAINER:/root/after.png" "$OUT_DIR/after.png"
docker exec "$CONTAINER" bash -lc "cat /root/mhr.log" > "$OUT_DIR/mhr.log" || true

echo "==> Done."
echo "==> Screenshots: $OUT_DIR/before.png $OUT_DIR/after.png"
echo "==> Log: $OUT_DIR/mhr.log"
echo "==> Read both screenshots to confirm rendering, and that after.png opens with the Reload check heading."
