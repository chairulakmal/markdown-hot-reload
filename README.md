<img src="assets/icon/mhr-icon.svg" alt="" width="64" height="64" />

# mhr

`mhr` is a read-only, desktop GitHub-flavoured markdown viewer. Point it at a file and it opens a window that re-renders the moment the file changes on disk. It has no network access, by design, so a document renders the same offline as online. Below: what it does, how it works, how to install it, how to build and run it, what it supports, how to contribute a change, and where the source and further documentation live.

https://github.com/user-attachments/assets/416b2828-7832-4f42-ad6a-3d9670a43118

## What it does

Run `mhr notes.md`, a window opens with the rendered markdown, and every time you (or an editor, or an agent) save that file, the window updates in place without losing your scroll position or open `<details>` elements. There is no editing surface; `mhr` never writes to the watched file.

## How it works

One Rust binary. `comrak` parses markdown to HTML, `notify` watches the file's parent directory, and `wry` and `tao` host a system webview that receives the new HTML through `evaluate_script`. The frontend is a static HTML shell plus about sixty lines of vanilla JavaScript, both compiled into the binary by `rust-embed`. Parsing, syntax highlighting, math conversion, and escaping all happen in Rust. JavaScript only morphs the DOM and draws Mermaid diagrams.

## Install

Every package below is built for x86_64, which is also called amd64. There is no arm64 build yet. Nobody has asked for one so far, so please open an issue if you need it.

The snap is the recommended install on any distribution that runs snapd, and it bundles its own WebKitGTK:

```
sudo snap install markdown-hot-reload
```

The snap is the only install where the command is not `mhr` at first. It arrives as `markdown-hot-reload.mhr`, because a bare `mhr` alias needs approval from the Snap Store, and that request only goes in once this build reaches the stable channel. You do not have to wait for it. This command creates `mhr` on your own machine, and it keeps working whatever the Store decides:

```
sudo snap alias markdown-hot-reload.mhr mhr
```

On Debian and Ubuntu, each [release](https://github.com/chairulakmal/markdown-hot-reload/releases) ships a `.deb` that uses your system's WebKitGTK rather than a second copy:

```
sudo apt install ./mhr_0.1.0-1_amd64.deb
```

The `.deb` installs the command as `mhr` straight away. There is no alias step.

This package needs Ubuntu 24.04 (Noble Numbat), Debian 13 (trixie), or newer. It links against the `t64` builds of GTK and GLib, which earlier releases do not have.

On any other distribution, download the `x86_64-unknown-linux-gnu` tarball from the release page, copy the binary into a directory on your `PATH`, and install WebKitGTK 4.1 from your package manager. The package name varies, so search for `webkit2gtk`. The binary in the tarball is already named `mhr`. Each release also ships a `SHA256SUMS` file for checking the download.

## Building and running

There is no npm, no bundler, and no separate frontend build step. The only toolchain is Cargo, and you need Rust 1.88 or newer, which [rustup](https://rustup.rs) installs. An older compiler is refused with a clear message rather than failing part way through the build.

On Linux, the webview is WebKitGTK, so three system packages are also required. On Ubuntu and Debian:

```
sudo apt install libwebkit2gtk-4.1-dev libgtk-3-dev pkg-config
```

Then build and run:

```
cargo build --release
./target/release/mhr path/to/file.md
```

To work on `mhr` itself, run it against the fixture document and edit that file from a second terminal:

```
cargo run -- fixtures/kitchen-sink.md
```

Every save re-renders the window, so a change is visible immediately. `fixtures/kitchen-sink.md` exercises every markdown feature the app supports, making it the fastest way to confirm a rendering change did not break something else.

`cargo test` runs the render pipeline tests, and `cargo clippy --all-targets` should stay clean. [`AGENTS.md`](AGENTS.md) lists the full command set CI runs.

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

- Source: [github.com/chairulakmal/markdown-hot-reload](https://github.com/chairulakmal/markdown-hot-reload)
- Report a problem: [issue tracker](https://github.com/chairulakmal/markdown-hot-reload/issues)
- Contributor notes: [`AGENTS.md`](AGENTS.md)
- Vendored frontend assets: [`docs/vendored-assets.md`](docs/vendored-assets.md)

## License

MIT, see [`LICENSE`](LICENSE).
