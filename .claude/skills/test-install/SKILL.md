---
name: test-install
description: Clean-room test for how mhr actually gets installed. Deb mode downloads a GitHub release's .deb (or takes a local one), verifies its SHA256 checksum, and installs it via apt. Cargo mode packages a local worktree the way `cargo publish` would (or installs a published version from crates.io) and installs it with `cargo install`. Either mode runs in a throwaway ubuntu:24.04 Docker container and confirms the GUI actually renders and hot-reloads under Xvfb. Use when asked to test a deb, test cargo install, verify a release package, or do a clean install check before or after publishing a release.
---

# Test how mhr gets installed

This skill runs `scripts/test-install.sh`, which does a full clean-room verification of an `mhr` distribution channel: not just "does the install command succeed" but "does the installed binary actually open a window and render markdown."

It has two modes, for the two ways someone actually gets `mhr` onto their machine without a GUI-having package manager already resolving everything for them: the `.deb` (apt) and `cargo install` (crates.io, or a local worktree before it's published).

## Running it

```
# deb mode (default)
.claude/skills/test-install/scripts/test-install.sh                       # tests the latest GitHub release
.claude/skills/test-install/scripts/test-install.sh --tag v0.1.0          # tests a specific tag
.claude/skills/test-install/scripts/test-install.sh --deb path/to/mhr.deb # tests a local build, skips download

# cargo install mode
.claude/skills/test-install/scripts/test-install.sh --cargo-install-path <worktree-dir>    # before publishing
.claude/skills/test-install/scripts/test-install.sh --cargo-install-version 0.1.2          # after publishing

# either mode
.claude/skills/test-install/scripts/test-install.sh --keep                # leaves the container running for debugging
```

Requires `docker` (daemon reachable). Deb mode without `--deb` also needs an authenticated `gh` CLI. Run it from the repo root so it can pick up `fixtures/kitchen-sink.md` as the render fixture; without a repo root it falls back to a minimal inline markdown snippet.

## What it checks, and why each step is there

**Deb mode**

1. **Downloads the `.deb` and `SHA256SUMS`** from the release (or uses `--deb`), and verifies the checksum before touching anything. A corrupt or tampered artifact should fail here, not three steps later as a confusing apt error.
2. **Prints the package control fields** (`dpkg-deb -I`) so the dependency list and version are visible in the run output.
3. **Installs via `apt-get install ./mhr*.deb`**, never `dpkg -i`. `dpkg -i` does not resolve dependencies, so it would silently pass on a broken `Depends:` line that `apt` would catch.

**Cargo install mode**

1. **Installs the Rust toolchain and the three WebKitGTK build packages** the `README.md` install section documents (`libwebkit2gtk-4.1-dev libgtk-3-dev pkg-config`), since `cargo install` compiles from source and CI's `cargo build` never runs against a fresh machine that lacks them.
2. With `--cargo-install-path`, **archives the worktree with `git archive` rather than `docker cp`-ing it directly**, because a `git worktree add` checkout's `.git` is a file pointing at an absolute path on the host, which resolves to nothing inside the container. Archiving also sidesteps needing a real VCS check inside the container: with no `.git` present, `cargo package` falls back to "everything on disk minus Cargo.toml's `exclude`", which composes with the archive's already-tracked-only file set into the same result cargo's own git integration would produce.
3. **Runs `cargo package --locked`**, which applies `Cargo.toml`'s `exclude` list and builds the result to verify it compiles, then **installs from the packaged directory** (`target/package/mhr-<version>/`), not the worktree. Installing from the worktree directly would skip `exclude` entirely and could pass even if a file the binary needs at runtime never makes it into the published crate.
4. With `--cargo-install-version`, **installs straight from crates.io** as a post-publish spot check of the exact bits a user would get.

**Both modes converge here**

5. **Runs `mhr --version` and `mhr --help`** to confirm the binary is on `PATH` and executes at all.
6. **Launches `mhr` against the fixture under `xvfb-run`**, then checks the process is still alive a few seconds later. A webview that fails to construct (see the `WebViewBuilder::build` trap in the repo's `CLAUDE.md`) exits immediately instead of hanging, so a dead process here is a real finding, not flakiness.
7. **Screenshots the root window with ImageMagick's `import`**, before and after appending a section to the watched file on disk. The two screenshots are the actual evidence: read both with the Read tool (they render as images) to confirm the markdown rendered correctly (headings, code fences, syntax highlighting) and that the appended section shows up in the second one, proving the `notify` watcher fired end to end, not just that the process didn't crash.

## After it runs

Read `$OUT_DIR/before.png` and `$OUT_DIR/after.png` yourself and describe what they show; do not report success on process-alive alone. If anything looks wrong, `$OUT_DIR/mhr.log` has the process's stderr/stdout, and `--keep` leaves the container up for `docker exec -it <name> bash` to dig further.

This complements, not replaces, `cargo test --locked` and a real run on the maintainer's machine for anything that needs an actual GPU or platform-specific input handling: Xvfb gives software rendering only, and this only ever runs the Linux path for either package format. The snap has its own verification, on a real desktop, documented in `docs/releasing.md`.
