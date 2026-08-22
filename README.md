# mhr

`mhr` is a read-only GitHub-flavoured markdown viewer for the desktop: point it at a file and it opens a window that re-renders the moment the file changes on disk. The thing worth knowing before anything else is that it has no network access at all, by design, so a rendered document behaves the same on a plane as it does at a desk. Below: what it does, how to build and run it, what it supports, how to contribute a change, and where the rest of the project's documentation lives.

https://github.com/user-attachments/assets/416b2828-7832-4f42-ad6a-3d9670a43118

## What it does

You run `mhr notes.md`, a window opens showing the rendered markdown, and every time you (or an editor, or an agent) save that file, the window updates in place without losing your scroll position or open `<details>` elements. There is no editing surface. `mhr` never writes to the file it watches.

## Building and running

There is no npm, no bundler, and no separate frontend build step. The only toolchain is Cargo.

```
cargo build --release
./target/release/mhr path/to/file.md
```

`cargo test` runs the render pipeline tests, and `cargo clippy --all-targets` should stay clean.

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
- **A passing `ci` check.** It runs `cargo fmt --check`, `cargo clippy`, `cargo deny check`, `cargo test`, and a `cargo release` build on Ubuntu 24.04. Run those locally first before creating a PR.

Working notes for contributors, including the decisions taken, the traps already checked for, and how the vendored frontend assets get refreshed, live in [`AGENTS.md`](AGENTS.md).

## License

MIT, see [`LICENSE`](LICENSE).
