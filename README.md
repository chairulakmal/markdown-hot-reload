<img src="https://raw.githubusercontent.com/chairulakmal/markdown-hot-reload/main/assets/icon/mhr-icon.svg" alt="" width="64" height="64" />

# Markdown Hot Reload

[![crates.io](https://img.shields.io/crates/v/mhr.svg)](https://crates.io/crates/mhr)
[![Snap Store](https://img.shields.io/badge/snap-markdown--hot--reload-orange.svg)](https://snapcraft.io/markdown-hot-reload)
[![License: MIT](https://img.shields.io/badge/license-MIT-blue.svg)](https://github.com/chairulakmal/markdown-hot-reload/blob/main/LICENSE)

`mhr` is a desktop viewer for markdown you did not write: a plan an agent just produced, a README from a repository you cloned an hour ago, a file your editor is rewriting while you read it. It renders GitHub-flavored markdown in a native window and re-renders the moment the file changes on disk. There is no server and no port, so nothing else on the machine can reach your document. It has no network access at all, and it never writes to the file it opens. Below: what it does, how it works, how it treats every document as untrusted input, how to install it, how to build and run it, what it supports, which platforms it targets, how to contribute a change, and where the source and further documentation live.

[![A window showing rendered markdown beside the editor writing it](https://raw.githubusercontent.com/chairulakmal/markdown-hot-reload/main/docs/demo-poster.png)](https://github.com/user-attachments/assets/416b2828-7832-4f42-ad6a-3d9670a43118)

Click the picture to play a 15-second demo.

## What it does

Run `mhr notes.md`. A window opens with the rendered markdown, and every time you (or an editor, or an agent) save that file, the window updates in place without losing your scroll position or open `<details>` elements. There is no editing surface; `mhr` never writes to the watched file.

## How it works

One Rust binary. `comrak` parses markdown to HTML, `notify` watches the file's parent directory, and `wry` and `tao` host a system webview that receives the new HTML through `evaluate_script`. The frontend is a static HTML shell plus about sixty lines of vanilla JavaScript, both compiled into the binary by `rust-embed`. Parsing, syntax highlighting, math conversion, and escaping all happen in Rust. JavaScript only morphs the DOM and draws Mermaid diagrams.

## Safety

Treating every document as untrusted input is a design constraint here, not advice to the reader. Five guarantees follow from it, and the code enforces each one.

- **No server and no port.** The native window is the whole app. Almost every other markdown previewer renders to a localhost port and opens a browser tab, which means any other process on the machine can read the document. `mhr` opens no socket.
- **No network access.** `index.html` sets `connect-src 'none'` in its Content-Security-Policy, so anything that needs the network fails immediately instead of working locally and leaking data elsewhere.
- **Raw HTML is escaped, never executed.** A document that embeds `<script>`, or any other raw tag, shows it as text on the page rather than running it. `src/render.rs` has tests that check this directly.
- **Read-only.** `mhr` never writes to the file it watches. There is no editing surface.
- **No `unsafe` code in this crate.** `Cargo.toml` forbids the `unsafe` keyword at the compiler level (`[lints.rust] unsafe_code = "forbid"`), not only by convention. This does not extend to dependencies, which are ordinary Rust crates and may use `unsafe` internally.

The design invariants behind each of these, and the tests that guard them, are detailed in [`AGENTS.md`](AGENTS.md).

## Install

Four ways to install, covered below in this order. `cargo install mhr` builds from source and works wherever the Rust toolchain does. The snap is the better choice on a Linux desktop, because it bundles its own WebKitGTK and updates itself. A `.deb` and a tarball are attached to every release for anyone who wants neither.

```
cargo install mhr
```

This compiles the crate on your machine, so it needs Rust 1.88 or newer, which [rustup](https://rustup.rs) installs. On Linux it also needs three system packages, because the webview is WebKitGTK. On Ubuntu and Debian:

```
sudo apt install libwebkit2gtk-4.1-dev libgtk-3-dev pkg-config
```

Install those first. Without them the build fails at the linker, and the error does not say which package is missing. Linux is the only platform with a released build today, so a `cargo install` on macOS or Windows is untested rather than supported. See Platforms below.

Every prebuilt package below is built for x86_64, which is also called amd64. There is no arm64 build yet. Nobody has asked for one so far, so please open an issue if you need it.

The snap is the recommended install on any distribution that runs snapd. Ubuntu includes snapd by default:

```
sudo snap install markdown-hot-reload
sudo snap alias markdown-hot-reload.mhr mhr
```

The second command creates the `mhr` command. It is needed because the snap installs itself as `markdown-hot-reload.mhr`. A bare `mhr` alias needs approval from the Snap Store, and that request is under review. The alias works on your own machine whatever the Store decides.

Running the snap from a terminal can print lines like `Could not open /sys/class/dmi/id/chassis_type` or `This call is not available inside the sandbox`. These come from GTK probing hardware and desktop details that Snap's confinement blocks on purpose. They are harmless: the window still opens and renders the file normally, so it is safe to ignore them.

Each [release](https://github.com/chairulakmal/markdown-hot-reload/releases) also ships a `.deb` for Ubuntu 24.04, Debian 13, and newer, and an `x86_64-unknown-linux-gnu` tarball for every other distribution. Neither one updates itself, because there is no apt repository for `mhr`.

[The install guide](https://mhr.chairulakmal.com/) is the full version: all four paths step by step, how to check a download against its published checksum, and how to update or remove each one.

## Building and running

There is no npm, no bundler, and no separate frontend build step. The only toolchain is Cargo, plus the same prerequisites a `cargo install` needs: Rust 1.88 or newer and, on Linux, the three WebKitGTK packages listed under Install above. An older compiler is refused with a clear message rather than failing midway through the build.

Clone the repository, then build and run:

```
cargo build --release
./target/release/mhr path/to/file.md
```

To work on `mhr` itself, run it against the fixture document and edit that file from a second terminal:

```
cargo run -- fixtures/kitchen-sink.md
```

Every save re-renders the window, so a change is visible immediately. `fixtures/kitchen-sink.md` exercises every markdown feature the app supports, making it the fastest way to confirm a rendering change did not break something else.

`cargo test` runs the render, math, CLI, asset, and watcher tests, and `cargo clippy --all-targets` should report no warnings. [`AGENTS.md`](AGENTS.md) lists the full command set CI runs.

## What it supports

- GitHub Flavored Markdown: tables, task lists, strikethrough, autolinks, footnotes, alerts, description lists, superscript, multiline block quotes
- Syntax-highlighted fenced code blocks
- Inline and display math (`$...$`, `$$...$$`, and GitHub's `` $`...`$ ``), converted to MathML offline. LaTeX that does not parse shows its own source rather than a broken render
- Mermaid diagrams, loaded on demand so documents without one pay no cost
- Front matter, parsed and stripped from the rendered output rather than shown as a stray paragraph

Images are the one gap. `mhr` reads only the file you point it at, so a local image beside the document, `![](diagram.png)`, does not display, and a remote image is blocked by the lack of network access. An image embedded as a `data:` URI does display.

Raw HTML in a document is always escaped, never executed. Documents come from agents and editors, not a trusted author, so `mhr` treats them as untrusted input.

## Platforms

Linux is the primary target. macOS is next. Windows is nice-to-have.

## Contributing

`main` is a protected branch. A change reaches it through a pull request (PR): create a branch, open a PR, wait for the `ci` check to pass, then merge. Nobody can push directly to `main`, force-push it, or delete it, including the maintainer.

Two things are required before a merge:

- **Signed commits.** An unsigned commit is rejected. GitHub explains the setup in [Managing commit signature verification](https://docs.github.com/en/authentication/managing-commit-signature-verification).
- **A passing `ci` check.** It runs `cargo fmt --all -- --check`, `cargo clippy --locked --all-targets`, `cargo deny check`, `cargo test --locked`, and `cargo build --locked --release` on Ubuntu 24.04. Run those locally first before opening a PR.

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
