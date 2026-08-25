<img src="assets/icon/mhr-icon.svg" alt="" width="64" height="64" />

# mhr

`mhr` is a read-only, desktop GitHub-flavoured markdown viewer. Point it at a file and it opens a window that re-renders the moment the file changes on disk. It has no network access, by design, so a document renders the same offline as online. Below: what it does, how it works, how it keeps a document safe to open, how to install it, how to build and run it, what it supports, how to contribute a change, and where the source and further documentation live.

https://github.com/user-attachments/assets/416b2828-7832-4f42-ad6a-3d9670a43118

## What it does

Run `mhr notes.md`, a window opens with the rendered markdown, and every time you (or an editor, or an agent) save that file, the window updates in place without losing your scroll position or open `<details>` elements. There is no editing surface; `mhr` never writes to the watched file.

## How it works

One Rust binary. `comrak` parses markdown to HTML, `notify` watches the file's parent directory, and `wry` and `tao` host a system webview that receives the new HTML through `evaluate_script`. The frontend is a static HTML shell plus about sixty lines of vanilla JavaScript, both compiled into the binary by `rust-embed`. Parsing, syntax highlighting, math conversion, and escaping all happen in Rust. JavaScript only morphs the DOM and draws Mermaid diagrams.

## Safety

A markdown file `mhr` opens can come from an editor, an agent, or a repository you do not control, not only from the person reading it. `mhr` treats every document as untrusted input.

- **No network access.** `index.html` sets `connect-src 'none'` in its Content-Security-Policy, so anything that needs the network fails immediately instead of working locally and leaking data elsewhere.
- **Raw HTML is escaped, never executed.** A document that embeds `<script>`, or any other raw tag, shows it as text on the page rather than running it. `src/render.rs` has tests that check this directly.
- **Read-only.** `mhr` never writes to the file it watches. There is no editing surface.
- **No `unsafe` code in this crate.** `Cargo.toml` forbids the `unsafe` keyword at the compiler level (`[lints.rust] unsafe_code = "forbid"`), not only by convention. This does not extend to dependencies, which are ordinary Rust crates and may use `unsafe` internally.

The design invariants behind each of these, and the tests that guard them, are spelled out in [`AGENTS.md`](AGENTS.md).

## Install

Every package is built for x86_64, which is also called amd64. There is no arm64 build yet. Nobody has asked for one so far, so please open an issue if you need it.

The snap is the recommended install on any distribution that runs snapd. Ubuntu includes snapd by default. The snap bundles its own WebKitGTK and updates itself:

```
sudo snap install markdown-hot-reload
sudo snap alias markdown-hot-reload.mhr mhr
```

The second command creates the `mhr` command. It is needed because the snap installs itself as `markdown-hot-reload.mhr`. A bare `mhr` alias needs approval from the Snap Store, and that request is currently under review. The alias works on your own machine whatever the Store decides.

Each [release](https://github.com/chairulakmal/markdown-hot-reload/releases) also ships a `.deb` for Ubuntu 24.04, Debian 13, and newer, and an `x86_64-unknown-linux-gnu` tarball for every other distribution. Neither one updates itself, because there is no apt repository for `mhr`.

[The install guide](https://mhr.chairulakmal.com/) is the full version: all three paths step by step, how to check a download against its published checksum, and how to remove each one.

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

- Install guide for snap, `.deb`, and tarball: [mhr.chairulakmal.com](https://mhr.chairulakmal.com/)
- Snap Store listing: [snapcraft.io/markdown-hot-reload](https://snapcraft.io/markdown-hot-reload)
- Source: [github.com/chairulakmal/markdown-hot-reload](https://github.com/chairulakmal/markdown-hot-reload)
- Report a problem: [issue tracker](https://github.com/chairulakmal/markdown-hot-reload/issues)
- Contributor notes: [`AGENTS.md`](AGENTS.md)
- Vendored frontend assets: [`docs/vendored-assets.md`](docs/vendored-assets.md)

## License

MIT, see [`LICENSE`](LICENSE).
