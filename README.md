<img src="https://raw.githubusercontent.com/chairulakmal/markdown-hot-reload/main/assets/icon/mhr-icon.svg" alt="" width="64" height="64" />

# Markdown Hot Reload

[![crates.io](https://img.shields.io/crates/v/mhr.svg)](https://crates.io/crates/mhr)
[![Snap Store](https://snapcraft.io/markdown-hot-reload/badge.svg)](https://snapcraft.io/markdown-hot-reload)

`mhr` is a markdown viewer with hot reload. Point it at a file on Linux and a native window renders it as GitHub-flavored markdown, then re-renders the moment the file changes on disk. It opens no port, reaches no network, and never writes to the file, so markdown you did not write is safe to open: a plan an agent produced, or a README from a repository you recently cloned. This README covers what the app does, how it works, its safety guarantees, how to install it, how to build and run it, which markdown features it supports, target platforms, and how to contribute.

[![A window showing rendered markdown beside the editor writing it](https://raw.githubusercontent.com/chairulakmal/markdown-hot-reload/main/docs/demo-poster.png)](https://github.com/user-attachments/assets/416b2828-7832-4f42-ad6a-3d9670a43118)

Click the picture to play a 15-second demo.

## What it does

Run `mhr notes.md` and a window opens with the file rendered. Every save updates it in place, whether you saved it, your editor did, or an agent did. Your scroll position and your open `<details>` sections survive the reload. There is no editing surface.

While the window is open, the terminal that started `mhr` is blocked. Add `&` to the end of the command to get your prompt back:

```
mhr TODO.md &
```

## How it works

One Rust binary. `comrak` parses the markdown to HTML. `notify` watches the file's parent directory for changes. `wry` and `tao` host a system webview, which receives the new HTML through `evaluate_script`. The frontend is a static HTML file plus a small amount of plain JavaScript, both compiled into the binary by `rust-embed`. All parsing, syntax highlighting, math conversion, escaping, and HTML sanitization happen in Rust. The JavaScript updates the DOM and draws Mermaid diagrams. It never parses markdown, never unescapes HTML, and never uses the network.

## Safety

`mhr` treats every document as untrusted input, and enforces that in code rather than asking the reader to be careful. Five guarantees follow.

- **No server and no port.** The native window is the whole app. Most other markdown viewers render to a localhost port and open a browser tab, so any other process on the machine can read the document. `mhr` opens no socket.
- **No network access.** `index.html` sets `connect-src 'none'` in its Content-Security-Policy. Anything that needs the network fails immediately, instead of working on your machine and leaking data on someone else's.
- **Raw HTML is filtered to a safe subset.** A document can use the same HTML that GitHub allows, such as tables, `<details>`, and `<kbd>`, and it renders. Anything that could run, such as `<script>`, `<style>`, an event handler attribute, or a `javascript:` URL, is removed or shown as inert text. The rendered HTML passes through an allowlist sanitizer before it reaches the window. `src/render.rs` has tests that check this.
- **Read-only.** `mhr` never writes to the file it watches. There is no editing surface.
- **No `unsafe` code in this crate.** `Cargo.toml` forbids the `unsafe` keyword at the compiler level (`[lints.rust] unsafe_code = "forbid"`). Dependencies are ordinary Rust crates and may use `unsafe` internally.

For the design invariants behind each guarantee, and the tests that protect them, check [`AGENTS.md`](AGENTS.md).

## Install

`mhr` has four install methods. `cargo install mhr` builds it from source. This is the most portable choice, and the only path that could run beyond Linux, though only Linux is tested. On a Linux desktop, the snap is usually better: one command, and it updates itself. Every release also attaches a `.deb` and a tarball for anyone who wants neither.

[The install guide](https://mhr.chairulakmal.com/) has the full version: every path step by step, how to verify a download against its published checksum, and how to update or remove each one.

Every prebuilt package is built for x86_64, also called amd64. There is no arm64 build yet. Open an issue if you need one.

The snap and the `.deb` add `mhr` to your file manager's "Open With" menu for markdown (.md) files. The tarball and `cargo install` install only the binary, with no menu entry.

### cargo install

```
cargo install mhr
```

This builds the crate on your machine, so it needs Rust 1.88 or newer, which [rustup](https://rustup.rs) installs. On macOS and Windows that is the only prerequisite, because the webview is part of the operating system. On Linux the webview uses WebKitGTK, so it also needs three system packages. On Ubuntu and Debian:

```
sudo apt install libwebkit2gtk-4.1-dev libgtk-3-dev pkg-config
```

Install these first. Without them, the build fails at the linker, with an error that does not name the missing package. Only Linux has a tested build, so a `cargo install` on macOS or Windows may work but is unverified. See Platforms below.

### snap

The snap is the recommended install on any distribution that runs snapd. Ubuntu includes snapd by default.

```
sudo snap install markdown-hot-reload
```

The snap's own command is `markdown-hot-reload.mhr`. The Snap Store also grants `mhr` as a short name for it, so `mhr` is what you type. An install made before the Store granted that name may not have it until snapd updates the snap. To add it now, run `sudo snap alias markdown-hot-reload.mhr mhr`.

Running the snap from a terminal can print lines such as `Could not open /sys/class/dmi/id/chassis_type` or `This call is not available inside the sandbox`. GTK prints these when Snap's confinement blocks it from probing hardware and desktop details. They are harmless. The window still opens and renders the file, so you can ignore them.

### .deb and tarball

Every [release](https://github.com/chairulakmal/markdown-hot-reload/releases) also ships a `.deb` for Ubuntu 24.04, Debian 13, and newer, plus an `x86_64-unknown-linux-gnu` tarball for other distributions. Neither one updates itself, because `mhr` has no apt repository.

## Building and running

There is no npm, no bundler, and no separate frontend build step. The only toolchain is Cargo, plus the prerequisites `cargo install` needs: Rust 1.88 or newer and, on Linux, the three WebKitGTK packages from the Install section. An older compiler is refused with a clear message.

Clone the repository, then build and run:

```
cargo build --release
./target/release/mhr path/to/file.md
```

To work on `mhr` itself, run it against the fixture document and edit that file from a second terminal:

```
cargo run -- fixtures/kitchen-sink.md
```

Every save re-renders the window. `fixtures/kitchen-sink.md` uses every markdown feature the app supports, so it is the fastest way to check that a rendering change did not break something else.

`cargo test --locked` runs the render, math, CLI, asset, and watcher tests. `cargo clippy --locked --all-targets` should report no warnings. The Contributing section lists the full command set that CI runs, and [`AGENTS.md`](AGENTS.md) explains why `--locked` is required.

## What it supports

- GitHub Flavored Markdown: tables, task lists, strikethrough, autolinks, footnotes, alerts, description lists, superscript, multiline block quotes
- Syntax-highlighted fenced code blocks
- Inline and display math (`$...$`, `$$...$$`, and GitHub's `` $`...`$ ``), converted to MathML offline. LaTeX that does not parse shows its own source instead of a broken render
- Mermaid diagrams, loaded on demand, so a document without a diagram loads nothing extra
- Front matter, parsed and removed from the output instead of shown as a stray paragraph
- A safe subset of raw HTML, the same tags GitHub allows, such as `<details>`, `<kbd>`, `<sub>`, and hand-written tables. Scripts, styles, event handlers, inline `style` attributes, and non-web URL schemes are removed

Images are the one gap. `mhr` reads only the single file you name. A local image next to the document (`![](diagram.png)`) does not display. A remote image is blocked, because there is no network access. An image embedded as a `data:` URI does display, as long as it is a PNG, GIF, JPEG, or WebP. An embedded SVG does not, because an SVG can carry a script.

## Platforms

Linux is the only platform `mhr` is built and tested on. Every release ships Linux packages only. `cargo install mhr` may compile on macOS or Windows, since the webview library supports both, but nothing there is tested. macOS is the next target. Windows is nice-to-have.

## Contributing

`main` is a protected branch. A change reaches it through a pull request (PR): create a branch, open a PR, wait for the `ci` check to pass, then merge. Nobody can push directly to `main`, force-push it, or delete it, including the maintainer.

Two things are required before a merge:

- **Signed commits.** An unsigned commit is rejected. GitHub explains the setup in [Managing commit signature verification](https://docs.github.com/en/authentication/managing-commit-signature-verification).
- **A passing `ci` check.** It runs `cargo fmt --all -- --check`, `cargo clippy --locked --all-targets`, `cargo deny check`, `cargo test --locked`, and `cargo build --locked --release` on Ubuntu 24.04. Run those locally first.

Contributor notes, including design invariants, decisions taken, and traps already checked for, live in [`AGENTS.md`](AGENTS.md). How the vendored frontend assets are refreshed is in [`docs/vendored-assets.md`](docs/vendored-assets.md).

## Links

- Install guide for cargo, the snap, the `.deb`, and the tarball: [mhr.chairulakmal.com](https://mhr.chairulakmal.com/)
- Crate: [crates.io/crates/mhr](https://crates.io/crates/mhr)
- Snap Store listing: [snapcraft.io/markdown-hot-reload](https://snapcraft.io/markdown-hot-reload)
- Source: [github.com/chairulakmal/markdown-hot-reload](https://github.com/chairulakmal/markdown-hot-reload)
- Report a problem: [issue tracker](https://github.com/chairulakmal/markdown-hot-reload/issues)
- Contributor notes: [`AGENTS.md`](AGENTS.md)
- Vendored frontend assets: [`docs/vendored-assets.md`](docs/vendored-assets.md)

## License

MIT, see [`LICENSE`](LICENSE).
