# Releasing

This document is the procedure for cutting an `mhr` release. The point that matters most: the snap is tested on a real desktop before the git tag is created, not after, because a snap can pass every automated check and still fail to open a window. Below: the channel and tag ladder, the order of operations from version bump to Snap Store promotion, why that order is a real fix for a real failure, and the shortcut allowed for a patch release.

- [Channel and tag ladder](#channel-and-tag-ladder)
- [Order of operations](#order-of-operations)
- [Why the snap is tested before the tag exists](#why-the-snap-is-tested-before-the-tag-exists)
- [Patch releases](#patch-releases)

## Channel and tag ladder

| Stage | Meaning | Snap channel | Git tag |
| --- | --- | --- | --- |
| alpha | Feature-incomplete or freshly landed, bugs expected | `edge` | `vX.Y.Z-alpha` |
| beta | Feature-complete, hunting bugs rather than missing features | `beta` (or `candidate`, for something this small) | `vX.Y.Z-beta` |
| stable | Safe to hand to someone with zero context and no warning | `stable` | `vX.Y.Z`, no suffix |

Keep the tag suffix and the snap channel in lockstep. The value of this ladder on a one-maintainer project is that a reader can predict one from the other without asking: a `-beta` tag sitting on `stable`, or a bare tag sitting on `edge`, breaks that.

`grade: stable` in `snap/snapcraft.yaml` is a hard requirement for the `candidate` and `stable` channels, not just a label; the Snap Store backend refuses a `devel`-graded snap on either one. With the grade already set to `stable`, promoting a channel is a plain `snapcraft release` call.

## Order of operations

**Prepare**

1. Bump the version in `Cargo.toml`, `Cargo.lock` and `snap/snapcraft.yaml` together, on a branch. The `release` workflow's tag-check step fails closed if any of the three disagrees with the tag.
2. Open a pull request, let CI pass, merge. Note the merge commit and its CI run.

**Get a snap onto `edge`**

3. Check the Store queue is empty first, with `snapcraft revisions <snap>`. A revision held for manual review stops every later upload from finishing, so clear it before uploading rather than after.
4. Download the merge commit's snap artifact: `gh run download <run-id> -R chairulakmal/markdown-hot-reload -n markdown-hot-reload-snap`. Do not build it locally; the artifact is what CI actually produced, and that is the thing about to be tested and released.
5. Verify the artifact before uploading it, by unpacking it with `unsquashfs` and checking `meta/`: `version` and `grade` in `snap.yaml`, `confinement: strict`, no unexpected `slots:` block, and `Exec` plus `StartupWMClass` set correctly in `meta/gui/*.desktop`. This check is cheap, and it catches a stale or wrong build before it reaches the Store, where revisions are permanent.
6. `snapcraft upload --release=edge <file>.snap`. This step is deliberately manual: CI proves the snap builds, and a human decides when the Store's users see it. Do not wire `snapcraft upload` into CI.

**Test on a real desktop, one package at a time**

The snap and the deb install the same two desktop entries, and they are indistinguishable to the shell, so testing both packages installed at once proves nothing about either one.

7. Remove the deb if it is installed, with `sudo apt remove mhr`. Install or refresh the snap from `edge`. Coming off a sideloaded (`--dangerous`) install needs `sudo snap refresh --edge --amend <snap>`, because that kind of install carries no assertion the Store can match against a normal refresh.
8. Run `hash -r`, then open a file. A shell caches the path of a command it has already run, and installing or replacing a package does not clear that cache, so confirm which binary is actually running with `pgrep -af bin/mhr` before trusting anything on screen.
9. Look at three things: the window opens at all, the taskbar shows `mhr`'s own icon, and right-clicking a markdown file then choosing "Open With" launches it. This has to be a human looking at a screen; on Wayland, GNOME refuses to let any tool read back which window the shell matched to which icon.
10. Repeat for the deb, with the snap held out of the way: `sudo snap disable <snap>`, install the deb from `target/debian/`, `hash -r`, and check the same three things. Then `sudo snap enable <snap>`.

**Publish**

11. Tag `vX.Y.Z` (or `vX.Y.Z-alpha` / `-beta`), signed, and push it. This triggers the `release` workflow, which drafts a GitHub Release with the deb, the tarball and `SHA256SUMS` attached, and marks it a prerelease automatically for a hyphenated tag.
12. Review the draft, replace the generated notes with real ones, publish.
13. Promote the revision already sitting on `edge`, rather than uploading again: `snapcraft release <snap> <revision> stable`. This way the bits verified in step 9 are exactly the bits that get released.

## Why the snap is tested before the tag exists

A tag is a promise, and it should not be made until the thing it names has been seen working. Two failures shaped this project's checklist into that order.

**A green `snap` build job says the snap builds, not that it runs.** A confinement bug once passed CI, unpacked correctly, and carried the right version and grade, and still aborted before opening a window, because strict confinement refused a session-bus name the toolkit asked for. Nothing in the repository could have caught it: the deb, `cargo run`, `cargo test` and even `snapcraft expand-extensions` all run unconfined, so this whole class of bug stays invisible until the snap is actually installed on a desktop with a real session. So step 9 is not optional and not automatable.

**The deb and the snap need re-verifying together, whenever a change touches a string they both rely on.** `StartupWMClass`, the icon name and the binary name are matched by the desktop shell, not by the compiler, so changing any of them in one package silently invalidates the last verification of the other. Treat a change to a shared string as a reason to run steps 9 and 10 both, even if only one package's files changed.

## Patch releases

For a patch whose only user-visible change is a bug fix, going straight from alpha to stable is proportionate; reserve beta and `candidate` for a release that changes visible behaviour enough to be worth field-testing first.

A patch may also sit on `edge` untagged while it is being verified. The lockstep rule above is about published state, and a strict reading of it would mean tagging `vX.Y.Z-alpha`, bumping the version in three files for that tag, then bumping them again for the real one, which is churn with no reader benefit. So for a patch: upload the CI-built snap to `edge`, run the checks that need running, then tag and promote that same revision. `edge` is the channel that carries unfinished work by definition. Do not stretch this shortcut to a release that changes visible behaviour.
