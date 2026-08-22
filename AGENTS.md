# Agent notes

Working notes for agents on `mhr`, a read-only GitHub-flavoured markdown viewer that re-renders when the file changes on disk. The rule that overrides every other consideration: **this project has no npm, no `package.json`, and no build step for the frontend, and it must render identically with networking disabled.** Below: what the app is, the invariants, the commands, how changes land on `main`, the traps already paid for, how the vendored assets are refreshed, and where the app is distributed.

## What it is

A single Rust binary. `wry` and `tao` open a system webview, `comrak` turns markdown into HTML, `notify` watches the file, and the result is pushed into the webview with `evaluate_script`. The frontend is a static HTML shell plus about sixty lines of vanilla JavaScript, all compiled into the binary with `rust-embed`.

Linux is the primary target, macOS is next, Windows is nice-to-have.

## Invariants

Breaking any of these is a design change, not a refactor. Raise it rather than doing it quietly.

- **No npm.** No `package.json`, no bundler, no `node_modules`. Frontend dependencies are vendored files in `assets/`.
- **Offline is enforced, not intended.** `index.html` sets `connect-src 'none'` and `img-src 'self'`. Anything needing the network fails loudly instead of working on your machine and failing on a plane.
- **Read-only.** No editing surface, no writing to the watched file, ever.
- **`render.unsafe_` stays false.** Documents come from agents and editors, so raw HTML is escaped rather than executed. `src/render.rs` has tests asserting this; do not weaken them without an explicit decision.
- **No `unsafe` in this crate.** `Cargo.toml` sets `[lints.rust] unsafe_code = "forbid"`, so this is enforced by the compiler rather than by review. Note this is a separate thing from comrak's `render.unsafe_` above, which is about HTML in documents. If a platform binding ever seems to need `unsafe`, that is a discussion, not a lint to relax.
- **Parsing, highlighting, and escaping happen in Rust.** JavaScript only morphs the DOM and draws diagrams.

## Commands

```
cargo fmt --all -- --check           # CI fails on a formatting diff, so check before pushing
cargo clippy --locked --all-targets  # lint levels come from [lints] in Cargo.toml, not from flags here
cargo deny check                     # licence and advisory policy for the dependency tree, from deny.toml
cargo test --locked                  # render pipeline and its escaping guarantees, math validation, CLI parsing, the watcher
cargo build --locked --release
./target/release/mhr fixtures/kitchen-sink.md
```

Those five are what the `ci` job runs, in that order. Run them before opening a pull request, because a failure in any of them blocks the merge.

`--locked` is part of the command, not decoration. Without it, cargo quietly rewrites `Cargo.lock` when it drifts from `Cargo.toml`, and the run then passes against a dependency graph nobody committed.

Building the snap locally needs LXD, which `snapcraft pack` sets up on first run. CI builds it on every push to `main`, so there is rarely a reason to run it by hand.

`fixtures/kitchen-sink.md` exercises every supported GFM feature. Edit it from a second terminal to test reload.

The GUI cannot be verified from a headless tool call. Add a test to `src/render.rs` for anything checkable from HTML output, and hand the run command to akmal for anything visual.

## How changes land

`main` is governed by a repository ruleset named `protect main and default branches`. Its bypass list is empty, so the maintainer takes the same route as everyone else: create a branch, open a pull request, wait for CI, merge. Direct pushes to `main`, force-pushes, and branch deletion are all refused.

Three parts of that ruleset constrain `.github/workflows/ci.yml`. Read them before editing that file.

- **The required check is named `ci`, which is the id of the job in `ci.yml`.** Renaming the job, or adding a `name:` key to it, renames the check. The ruleset then waits for a check that no longer reports, and every pull request stops merging.
- **The `snap` job must never become a required check.** It carries `if: github.event_name == 'push'`, so it does not run on a pull request. A required check that never runs stays *expected* instead of passing, and blocks the merge forever.
- **Every commit must be signed.** The ruleset rejects an unsigned commit when it is pushed, including a commit an agent creates for the author. If a push is rejected with no clear reason, check `git log --show-signature -1` first.

One more rule affects commit metadata. The ruleset requires an approving review for any change GitHub cannot attribute to a user account, and nobody can approve their own pull request. Keep `user.email` set to an address that is registered on the account, or such a pull request cannot be merged at all.

## Traps already checked for

- **`WebViewBuilder::build(&window)` fails at runtime on Linux** with "the window handle kind is not supported". WebKitGTK attaches to a GTK container, so Linux and the BSDs need `build_gtk(window.default_vbox())`. It compiles either way, and macOS and Windows take the other branch, so this path is never exercised outside Linux.
- **Watch the parent directory, never the file.** Editors save by writing a temp file and renaming over the original, which orphans an inotify watch on the original inode and silently stops all reloads.
- **One rename does not test the rename trap.** A single save still produces an event with the watch held on the file, because the rename happened to the watched inode; it is the *second* save that is silent. `watch::tests::redraws_on_every_save_that_arrives_by_rename` therefore renames twice, and a one-rename version of it passes against the broken code. Same shape of mistake applies to any test written for this module: reproduce the bug and watch the test fail before trusting it.
- **Passing a theme name to syntect bakes it into the HTML.** `SyntectAdapter::new(Some(theme))` writes inline `style` attributes on every span *and* a `background-color` on the `<pre>` itself, which no stylesheet can override, so a dark page gets a white code block. `SyntectAdapterBuilder::new().css_with_class_prefix("hl-")` emits classes instead and leaves the block's background to `--code-bg`. `src/render.rs` has a test asserting no `style=` reaches the page.
- **comrak puts the codefence language on the `<code>` tag, not `<pre>`.** Use `plugins.render.codefence_renderers`, which dispatches by language and gives full control of the output, rather than branching inside a `SyntaxHighlighterAdapter`.
- **comrak does not emit MathML.** It renders a math node as `<span data-math-style>` with the LaTeX left raw, and it has no plugin hook for math the way it has for code fences. `src/render.rs` overrides `NodeValue::Math` through `create_formatter!` instead, which hands over the literal before comrak escapes it, so `src/math.rs` never has to unescape HTML to find out what the author wrote.
- **`pulldown-latex` does not escape everything it echoes.** `\operatorname{...}` passes its argument through untouched, and a parse error quotes the failing source back inside `<merror>`. Both put document-controlled markup on the page. `src/math.rs` therefore validates the converter's output against an element and attribute allowlist and discards the whole conversion on anything unrecognised. Do not replace that check with a trust in the crate; `src/math.rs` has tests carrying both payloads.
- **A valid `$a < b$` converts to `<mo><</mo>`, with the `<` unescaped.** It works because the HTML tokenizer only starts a tag when a letter, `/`, `!` or `?` follows the `<`. Any validator over this output has to follow the same rule or it rejects ordinary arithmetic.
- **Custom protocol origins differ by platform.** WebKit serves `mhr://localhost/`, WebView2 rewrites to `http://mhr.localhost/`. The CSP and `assets::index_url` both account for this.
- **`rust-toolchain.toml` does not pin the compiler cargo actually runs.** One was tried here and removed. The file only tells rustup which toolchain to select, and cargo resolves `rustc` from `PATH`, so a system rustc that sits ahead of the rustup shim is unaffected by it. That is a machine-level problem with a machine-level fix, not something the repo can carry. What the repo can do it already does: `rust-version = "1.88"` in `Cargo.toml` makes cargo refuse an older compiler with a clear message, and the `rust-deps` part in `snap/snapcraft.yaml` installs a current toolchain so the snap build does not fall back to the older system rustc on core24.
- **The snap build installs its own rustup, and the part doing it has to be named `rust-deps`.** snapcraft 9.0.0 installed rustup for the rust plugin and 9.0.1 stopped, which breaks any `rust-channel` setting with `Environment validation failed for part 'mhr': 'rustup' not found` ([snapcraft#6330](https://github.com/canonical/snapcraft/issues/6330), open). The plugin looks for a dependency named exactly `rust-deps`, and finding it, checks PATH for `cargo` and `rustc` rather than `rustup`. So the name is load-bearing, `rust-channel` must then be `none` (the plugin errors if a channel is also set), and the toolchain has to be symlinked into `/usr/local/bin`, because `$HOME/.cargo/bin` is not on PATH when the next part builds. Revisit this once the upstream bug closes; until then, do not "simplify" it back to a bare `rust-channel: stable`.

## Vendored assets

Compiled into the binary from `assets/`, compressed by `rust-embed`. To refresh, download the file, replace it in place, and rebuild.

| File | Version | Source |
| --- | --- | --- |
| `idiomorph.min.js` | 0.7.3 | `cdn.jsdelivr.net/npm/idiomorph@0.7.3/dist/idiomorph.min.js` |
| `mermaid.min.js` | 11.x | `cdn.jsdelivr.net/npm/mermaid@11/dist/mermaid.min.js` |
| `highlight.css` | generated | syntect 5.3.0, see below |
| `latex.css` | 0.8.0 | `github.com/carloskiki/pulldown-latex` release `0.8.0`, `styles.css` |
| `font/*.woff2` | 0.8.0 | `github.com/carloskiki/pulldown-latex` release `0.8.0`, `font/` |

`highlight.css` is generated rather than downloaded. It is the two syntax-highlighting palettes, `InspiredGitHub` for a light page and `base16-ocean.dark` for a dark one, produced by syntect's `css_for_theme_with_class_style` with `ClassStyle::SpacedPrefixed { prefix: "hl-" }`. Each palette sits inside its own `prefers-color-scheme` media query, because the two themes do not emit the same selector set: the light theme has rules the dark one has no answer for, and some of them are specific enough to win an override. Regenerate it with a throwaway crate depending on `syntect` at the version in `Cargo.lock`; the file's own header comment records the exact call. The dead `.hl-code` rule is dropped on the way out, since the `<pre>` never carries that class and the rule's `background-color` would fight `--code-bg`.

Mermaid is 3.5 MB raw and dominates the binary, so it is loaded only when a document actually contains a diagram.

`latex.css` and the four Latin Modern fonts come from the same `pulldown-latex` release as the crate version in `Cargo.toml`, and must be refreshed together with it: the stylesheet targets classes the crate emits (`menv-align`, `menv-cases` and friends), so a version mismatch silently mis-aligns matrices and align environments rather than failing. The fonts are 528 KB and are vendored rather than left to the system on purpose, for the same reason the app has no network access: a machine with no math font installed should not render math differently from one that has.

## Distribution

CI builds the snap on every push to `main` and uploads it as a workflow artifact, so a build break is caught at the commit that causes it rather than at release time. Nothing is published automatically: the artifact is there to install with `snap install --dangerous` and test.

Snap Store and GitHub Releases are the only planned distribution channels. Flathub was considered and dropped, not deferred: this is a hobby project with one maintainer, and a second packaging manifest and Store listing costs more upkeep than the extra reach is worth. Don't propose adding it back without the author raising it first.

