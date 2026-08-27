# Agent notes

Working notes for agents on `mhr`, a read-only GitHub-flavored markdown viewer that re-renders when the file changes on disk. The rule that overrides every other consideration: **no npm, no `package.json`, no frontend build step, and it must render identically with networking disabled.** What follows, in order: what the app is, the invariants, the commands, how changes land on `main`, traps already paid for, where vendored assets are documented, and how the app is distributed.

## What it is

One Rust binary. `comrak` renders the markdown, `notify` watches the file's parent directory, and `wry` and `tao` host a system webview that receives the HTML through `evaluate_script`. The frontend is a static HTML shell plus a small amount of vanilla JavaScript, compiled in by `rust-embed`. [`README.md`](README.md) covers the architecture and supported feature set in full.

Linux is the primary target, macOS is next, Windows is nice-to-have.

## Invariants

Breaking any of these is a design change, not a refactor. Raise it rather than doing it quietly.

- **No npm.** No `package.json`, no bundler, no `node_modules`. Frontend dependencies are vendored files in `assets/`.
- **Offline is enforced, not intended.** `index.html` sets `connect-src 'none'`, and `img-src` allows only `'self'` and `data:`, so anything needing the network fails loudly instead of working locally and failing elsewhere.
- **Read-only.** No editing surface, no writing to the watched file, ever.
- **`render.r#unsafe` stays false.** Documents come from agents and editors, so raw HTML is escaped rather than executed. `src/render.rs` tests assert this; do not weaken them without an explicit decision.
- **No `unsafe` in this crate.** `Cargo.toml` sets `[lints.rust] unsafe_code = "forbid"`, enforced by the compiler, not review. This is separate from comrak's `render.r#unsafe` above, which is about HTML in documents. If a platform binding ever seems to need `unsafe`, that is a discussion, not a lint to relax.
- **Parsing, highlighting, and escaping happen in Rust.** JavaScript only morphs the DOM and draws diagrams.

## Commands

```
cargo fmt --all -- --check           # CI fails on a formatting diff, so check before pushing
cargo clippy --locked --all-targets  # lint levels come from [lints] in Cargo.toml, not from flags here
cargo deny check                     # license and advisory policy for the dependency tree, from deny.toml
cargo test --locked                  # render and its escaping guarantees, math validation, CLI parsing, assets, the watcher
cargo build --locked --release
```

These five are what the `ci` job runs, in that order. Run them before opening a pull request; a failure in any of them blocks the merge.

```
./target/release/mhr fixtures/kitchen-sink.md
```

The GUI smoke check, run by hand after a build. It is not one of the CI five.

`--locked` is part of the command, not decoration. Without it, cargo quietly rewrites `Cargo.lock` when it drifts from `Cargo.toml`, and the run passes against a dependency graph nobody committed.

The snap build needs LXD, which `snapcraft pack` sets up on first run. CI builds it on every pull request and every push to `main` that touches more than prose, so there is rarely a reason to run it by hand. The build takes 7 to 9 minutes; the `timeout-minutes: 45` on that job is a ceiling for a stuck build, not an estimate.

Property tests run under proptest with a fresh random seed each run, so a failure that appears once may not reappear. When one fails, proptest prints the shrunk counterexample and writes an RNG seed to `proptest-regressions/`, which is gitignored: the seed replays a stream, not a value, so it only reproduces the failure while the generator is unchanged, and a CI runner is discarded before anyone could commit it anyway. **Turn the shrunk counterexample into an ordinary `#[test]` with that literal input.** That is what survives a change to the strategy, and it is the record that belongs in the repository. `PROPTEST_CASES=20000 cargo test --locked` runs the whole suite at eighty times the default case count in about six seconds, which is worth doing before touching `render.rs` or `math.rs`.

A property over rendered HTML must assert on tag openings, not on substrings that merely look like markup. Body text carries unescaped quotes and equals signs, so a document can legitimately render the literal text `href="javascript:` inside a `<code>` block. Two versions of `to_html_never_emits_a_tag_it_does_not_generate` asserted on attribute shapes and failed on innocent documents before this was narrowed to `<` openings, which only a real tag can produce.

`fixtures/kitchen-sink.md` exercises every supported GFM feature. Edit it from a second terminal to test reload.

The GUI cannot be verified from a headless tool call. Add a test to `src/render.rs` for anything checkable from HTML output, and hand the run command to the maintainer for anything visual.

## How changes land

`main` is governed by a repository ruleset named `protect main and default branches` with an empty bypass list, so the maintainer takes the same route as everyone else: branch, pull request, passing CI, merge. [`README.md`](README.md) has the contributor-facing version. The rules below constrain `.github/workflows/ci.yml`, so read them before editing that file.

- **The required check is named `ci`, the id of the job in `ci.yml`.** Rename the job or add a `name:` key to it and the ruleset waits for a check that no longer reports, and every pull request stops merging.
- **The `snap` job must never become a required check.** The workflow's concurrency rule cancels it when the branch is pushed again, and a prose-only pull request skips it altogether. A cancelled check is not a passing one, and a skipped one never reports, so requiring it would block merges at random.
- **Every commit must be signed**, including one an agent creates for the author. If a push is rejected with no clear reason, check `git log --show-signature -1` first.
- **Keep `user.email` set to an address registered on the account.** The ruleset requires an approving review for any change GitHub cannot attribute to a user account, and nobody can approve their own pull request, so a stray address makes a pull request unmergeable.
- **Batch same-day docs commits into one pull request rather than opening one per commit.** If a second docs-only change comes up while an earlier docs pull request from the same day is still open, add it to that branch instead of opening a new one.

The `changes` job follows from the first two bullets. It compares changed files against a list of prose files and decides whether `snap` runs, so a documentation-only pull request skips the snap build. It never gates `ci`, because a required check that does not report leaves the pull request unmergeable. Top-level files are named one by one rather than matched with `*.md`, since `fixtures/kitchen-sink.md` is input to the render and CLI tests and a change to it must still run them. `docs/*` and `.claude/*` are directory wildcards instead, safe because nothing under either ever affects the build. A new top-level prose file must be added to the list by name, or editing it triggers the snap build.

## Traps already checked for

- **`WebViewBuilder::build(&window)` fails at runtime on Linux** with "the window handle kind is not supported". WebKitGTK attaches to a GTK container, so Linux and the BSDs need `build_gtk(window.default_vbox())`. It compiles either way, and only Linux takes this branch, so nothing catches it before runtime.
- **Watch the parent directory, never the file.** Editors save by writing a temp file and renaming over the original, which orphans an inotify watch on the original inode and silently stops all reloads.
- **One rename does not test the rename trap.** With the watch on the file, a single save still fires, because the rename happened to the watched inode; the *second* save is the silent one. `watch::tests::redraws_on_every_save_that_arrives_by_rename` therefore renames twice, and a one-rename version passes against the broken code. For any test in this module, reproduce the bug and watch the test fail before trusting it.
- **Passing a theme name to syntect bakes it into the HTML.** `SyntectAdapter::new(Some(theme))` writes inline `style` attributes on every span *and* a `background-color` on the `<pre>`, which no stylesheet can override, so a dark page gets a white code block. `SyntectAdapterBuilder::new().css_with_class_prefix("hl-")` emits classes and leaves the background to `--code-bg`. `src/render.rs` asserts no `style=` reaches the page.
- **comrak puts the codefence language on the `<code>` tag, not `<pre>`.** Use `plugins.render.codefence_renderers`, which dispatches by language, rather than branching inside a `SyntaxHighlighterAdapter`.
- **comrak does not emit MathML.** It renders a math node as `<span data-math-style>` with the LaTeX left raw, and has no plugin hook for math, unlike code fences. `src/render.rs` overrides `NodeValue::Math` through `create_formatter!`, which hands over the literal before comrak escapes it, so `src/math.rs` never has to unescape HTML to recover what the author wrote.
- **`pulldown-latex` does not escape everything it echoes.** `\operatorname{...}` passes its argument through untouched, and a parse error quotes the failing source back inside `<merror>`. Both put document-controlled markup on the page. `src/math.rs` validates the converter's output against an element and attribute allowlist and discards the whole conversion on anything unrecognized. Do not replace that check with trust in the crate; `src/math.rs` has tests carrying both payloads.
- **A valid `$a < b$` converts to `<mo><</mo>`, with the `<` unescaped.** The HTML tokenizer only starts a tag when a letter, `/`, `!` or `?` follows the `<`. Any validator over this output must follow the same rule or it rejects ordinary arithmetic.
- **Custom protocol origins differ by platform.** WebKit serves `mhr://localhost/`, WebView2 rewrites to `http://mhr.localhost/`. The CSP and `assets::index_url` both account for this.
- **`rust-toolchain.toml` does not pin the compiler cargo actually runs.** One was tried here and removed. It only tells rustup which toolchain to select, while cargo resolves `rustc` from `PATH`, so a system rustc ahead of the rustup shim still wins. The fix is machine-level: remove the distribution's `rustc` package, or put the shim directory ahead of `/usr/bin`, then confirm `which -a rustc` prints one path. Ubuntu 24.04 packages 1.75, below this crate's minimum, and `apt` can reinstall it whenever another package build-depends on system Rust. The repo's own guards: `rust-version = "1.88"` in `Cargo.toml` makes cargo refuse an older compiler with a clear message, and the `rust-deps` part in `snap/snapcraft.yaml` installs a current toolchain so the snap build does not fall back to the system rustc on core24.
- **The snap build installs its own rustup, and the part doing it has to be named `rust-deps`.** snapcraft 9.0.0 installed rustup for the rust plugin and 9.0.1 stopped, breaking any `rust-channel` setting with `Environment validation failed for part 'mhr': 'rustup' not found` ([snapcraft#6330](https://github.com/canonical/snapcraft/issues/6330), open). The plugin looks for a dependency named exactly `rust-deps`; finding it, it checks PATH for `cargo` and `rustc` rather than `rustup`. So the name is load-bearing, `rust-channel` must be `none` (the plugin errors if a channel is also set), and the toolchain has to be symlinked into `/usr/local/bin`, because `$HOME/.cargo/bin` is not on PATH when the next part builds. Revisit once the upstream bug closes; until then, do not "simplify" it back to a bare `rust-channel: stable`.
- **Do not set a GTK app id. It is a session-bus name, and a confined snap may not own one.** `with_app_id` reaches `gtk::Application::new`, and tao then calls `register`, which asks the session bus for that name. The AppArmor profile snapd generates permits a snap to own nothing but `org.kde.StatusNotifierItem-*`, so the request comes back `AccessDenied`, and tao's `.expect("Failed to initialize gtk backend!")` aborts the process before a window opens. Renaming the id does not help, because no name is allowed by default. Declaring a `dbus` slot does grant it, but that interface carries `deny-connection` in its base declaration, so the Store holds every upload for manual review: tried 2026-08-25, revision 4 was held, and the slot was removed again. The app id was for `StartupWMClass` matching, and GTK's fallback covers that: with no id it uses the program name, `mhr` in both packages, which is what both desktop files now name. Only confinement triggers the crash, so the deb, `cargo run` and the whole test suite miss it.
- **snapd validates `Exec=` in `snap/gui/*.desktop` and silently drops the line when it does not name the app's wrapper.** The wrapper here is `markdown-hot-reload.mhr`, snap name plus app name, so the `Exec=mhr %f` that works in the deb is rejected in the snap. snapd does not error or fall back; it installs the entry to `/var/lib/snapd/desktop/applications/` with no `Exec` key at all. GIO still loads such an entry, so `StartupWMClass` matching and the taskbar icon keep working, and it stays registered as a `text/markdown` handler, but it has no command: "Open With" launches nothing. That combination is why it survived review twice. The bare `mhr` alias does not help even once the Store grants it, because snapd matches the wrapper name and an alias is a separate symlink. Verified 2026-08-25 against installed revision 3.

## Vendored assets

Frontend dependencies are vendored files in `assets/`, compiled into the binary by `rust-embed`. The inventory, the refresh procedure, the icon design rules, and how generated files are regenerated are in [`docs/vendored-assets.md`](docs/vendored-assets.md). Read it before touching anything in `assets/`.

Two pairings there fail silently rather than loudly, which is why they are named here too. `latex.css` and the four fonts must come from the same `pulldown-latex` release as the crate version in `Cargo.toml`, or math mis-aligns instead of erroring. `icon/window-icon.rgba` must agree with `assets::ICON_SIZE`, which a test checks.

## Distribution

CI builds the snap on every pull request and every push to `main` that touches more than prose, and uploads it as a workflow artifact, so a packaging break is caught before release rather than at it. It also catches breaks no commit here causes: the snapcraft 9.0.1 regression above arrived on its own, because the build action installs snapcraft from `latest/stable`. Nothing is published automatically; the artifact is there to install with `snap install --dangerous` and test.

Cutting an actual release, version bump through tag through Snap Store promotion, is [`docs/releasing.md`](docs/releasing.md). Read it before tagging; it exists because a snap can pass every check above and still fail to open a window, and the order it lays out is how that gets caught before a tag makes any promise about it.

The Snap Store, GitHub Releases and crates.io are the only planned channels. Flathub was considered and dropped, not deferred: one maintainer, and a second packaging manifest and Store listing costs more upkeep than the extra reach is worth. Do not propose adding it back without the author raising it first.

An apt repository was considered and dropped for the same reason. Hosting one is free, since `Packages` and a signed `Release` are static files GitHub Pages can serve, but it would commit signed `.deb` binaries into this repository's history forever, put a GPG signing key in the Actions secrets, and ask a user for three setup commands where the snap needs one. Automatic updates are the only problem it would solve, and the snap already solves that. So the `.deb` and the tarball stay manual installs, and the install guide says so in both sections. Same rule as Flathub: do not propose it without the author raising it first.
