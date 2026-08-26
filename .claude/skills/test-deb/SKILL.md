---
name: test-deb
description: Clean-room install test for the mhr .deb package. Downloads a GitHub release's .deb (or takes a local one), verifies its SHA256 checksum, installs it via apt in a throwaway ubuntu:24.04 Docker container, and confirms the GUI actually renders and hot-reloads under Xvfb. Use when asked to test a deb, verify a release package, or do a clean install check before or after publishing a release.
---

# Test the mhr .deb package

This skill runs `scripts/test-deb.sh`, which does a full clean-room verification of the `mhr` Debian package: not just "does `dpkg -i` succeed" but "does the installed binary actually open a window and render markdown."

## Running it

```
.claude/skills/test-deb/scripts/test-deb.sh                       # tests the latest GitHub release
.claude/skills/test-deb/scripts/test-deb.sh --tag v0.1.0          # tests a specific tag
.claude/skills/test-deb/scripts/test-deb.sh --deb path/to/mhr.deb # tests a local build, skips download
.claude/skills/test-deb/scripts/test-deb.sh --keep                # leaves the container running for debugging
```

Requires `docker` (daemon reachable) and an authenticated `gh` CLI. Run it from the repo root so it can pick up `fixtures/kitchen-sink.md` as the render fixture; without a repo root it falls back to a minimal inline markdown snippet.

## What it checks, and why each step is there

1. **Downloads the `.deb` and `SHA256SUMS`** from the release (or uses `--deb`), and verifies the checksum before touching anything. A corrupt or tampered artifact should fail here, not three steps later as a confusing apt error.
2. **Prints the package control fields** (`dpkg-deb -I`) so the dependency list and version are visible in the run output.
3. **Installs via `apt-get install ./mhr*.deb` in a freshly started `ubuntu:24.04` container**, never `dpkg -i`. `dpkg -i` does not resolve dependencies, so it would silently pass on a broken `Depends:` line that `apt` would catch. The container is one-shot: no image is reused between runs, so nothing left over from a previous test can mask a real regression.
4. **Runs `mhr --version` and `mhr --help`** to confirm the binary is on `PATH` and executes at all.
5. **Launches `mhr` against the fixture under `xvfb-run`**, then checks the process is still alive a few seconds later. A webview that fails to construct (see the `WebViewBuilder::build` trap in the repo's `CLAUDE.md`) exits immediately instead of hanging, so a dead process here is a real finding, not flakiness.
6. **Screenshots the root window with ImageMagick's `import`**, before and after appending a section to the watched file on disk. The two screenshots are the actual evidence: read both with the Read tool (they render as images) to confirm the markdown rendered correctly (headings, code fences, syntax highlighting) and that the appended section shows up in the second one, proving the `notify` watcher fired end to end, not just that the process didn't crash.

## After it runs

Read `$OUT_DIR/before.png` and `$OUT_DIR/after.png` yourself and describe what they show; do not report success on process-alive alone. If anything looks wrong, `$OUT_DIR/mhr.log` has the process's stderr/stdout, and `--keep` leaves the container up for `docker exec -it <name> bash` to dig further.

This complements, not replaces, `cargo test --locked` and a real run on the maintainer's machine for anything that needs an actual GPU or platform-specific input handling: Xvfb gives software rendering only, and this only ever runs the Linux `.deb` path.
