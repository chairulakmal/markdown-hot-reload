<img src="assets/icon/mhr-icon.svg" alt="" width="64" height="64" />

# mhr

`mhr` is a read-only GitHub-flavoured markdown viewer for the desktop: point it at a file and it opens a window that re-renders the moment the file changes on disk. The thing worth knowing before anything else is that it has no network access at all, by design, so a rendered document behaves the same on a plane as it does at a desk. Below: what it does, how it works, how to install it, how to build and run it, what it supports, how to contribute a change, and where the source and the rest of the project's documentation live.

https://github.com/user-attachments/assets/416b2828-7832-4f42-ad6a-3d9670a43118

## What it does

You run `mhr notes.md`, a window opens showing the rendered markdown, and every time you (or an editor, or an agent) save that file, the window updates in place without losing your scroll position or open `<details>` elements. There is no editing surface. `mhr` never writes to the file it watches.

## How it works

One Rust binary. `comrak` parses the markdown and renders it to HTML, `notify` watches the file's parent directory, and `wry` and `tao` open a system webview that receives the new HTML through `evaluate_script`. The frontend is a static HTML shell plus about sixty lines of vanilla JavaScript, both compiled into the binary by `rust-embed`. Parsing, syntax highlighting, math conversion, and escaping all happen in Rust. The JavaScript only morphs the DOM and draws Mermaid diagrams.

## Install

The snap is the recommended install on any distribution that runs snapd. It carries its own copy of WebKitGTK, so nothing else has to be installed first:

```
sudo snap install markdown-hot-reload
```

The command it installs is `markdown-hot-reload.mhr` until the Snap Store grants the shorter `mhr` alias.

On Debian and Ubuntu, a `.deb` is attached to each [release](https://github.com/chairulakmal/markdown-hot-reload/releases). It uses the WebKitGTK your system already has instead of bundling a second copy, which keeps it small:

```
sudo apt install ./mhr_0.1.0-1_amd64.deb
```

That package needs Ubuntu 24.04 (Noble Numbat) or Debian 13 (trixie) or newer. It links against the `t64` builds of GTK and GLib, and earlier releases do not have them.

On any other distribution, download the `x86_64-unknown-linux-gnu` tarball from the same release page and copy the binary into a directory on your `PATH`. Install WebKitGTK 4.1 from your own package manager first. Search for `webkit2gtk`, because the exact package name differs between distributions. Every release also ships a `SHA256SUMS` file for checking the download.

## Building and running

There is no npm, no bundler, and no separate frontend build step. The only toolchain is Cargo. You need Rust 1.88 or newer, which [rustup](https://rustup.rs) installs. An older compiler is refused with a clear message instead of failing part way through the build.

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

Every save re-renders the window, so a change is visible immediately. `fixtures/kitchen-sink.md` uses every markdown feature the app supports, which makes it the fastest way to confirm that a rendering change did not break something else.

`cargo test` runs the render pipeline tests, and `cargo clippy --all-targets` should stay clean. [`AGENTS.md`](AGENTS.md) lists the full command set that CI runs.

## What it supports

- GitHub Flavored Markdown: tables, task lists, strikethrough, autolinks, footnotes, alerts, description lists, superscript, multiline block quotes
- Syntax-highlighted fenced code blocks
- Inline and display math (`$...$`, `$$...$$`, and GitHub's `` $`...`$ ``), converted to MathML offline. LaTeX that does not parse shows its own source rather than a broken render
- Mermaid diagrams, loaded on demand so documents without one pay no cost
- Front matter, parsed and stripped from the rendered output rather than shown as a stray paragraph

Images are the one gap. `mhr` reads only the file you point it at, so an image beside the document, `![](diagram.png)`, does not display, and a remote image is blocked because the app has no network access. An image embedded in the document as a `data:` URI does display.

Raw HTML in a document is always escaped, never executed. Documents come from agents and editors, not from a trusted author, so `mhr` treats them as untrusted input.

## Platforms

Linux is the primary target. macOS is next. Windows is nice-to-have.

## Contributing

`main` is a protected branch. A change reaches it through a pull request (PR): create a branch, open a PR, wait for the `ci` check to pass, then merge. Nobody can push directly to `main`, force-push it, or delete it, and this includes the maintainer.

Two things are required before a merge:

- **Signed commits.** An unsigned commit is rejected. GitHub explains the setup in [Managing commit signature verification](https://docs.github.com/en/authentication/managing-commit-signature-verification).
- **A passing `ci` check.** It runs `cargo fmt --all -- --check`, `cargo clippy --locked --all-targets`, `cargo deny check`, `cargo test --locked`, and `cargo build --locked --release` on Ubuntu 24.04. Run those locally first before creating a PR.

Working notes for contributors, including the design invariants, the decisions taken, and the traps already checked for, live in [`AGENTS.md`](AGENTS.md). How the vendored frontend assets get refreshed is in [`docs/vendored-assets.md`](docs/vendored-assets.md).

## Links

- Source: [github.com/chairulakmal/markdown-hot-reload](https://github.com/chairulakmal/markdown-hot-reload)
- Report a problem: [issue tracker](https://github.com/chairulakmal/markdown-hot-reload/issues)
- Contributor notes: [`AGENTS.md`](AGENTS.md)
- Vendored frontend assets: [`docs/vendored-assets.md`](docs/vendored-assets.md)

## License

MIT, see [`LICENSE`](LICENSE).
