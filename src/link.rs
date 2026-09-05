//! Where a link in a rendered document goes.
//!
//! The window has no address bar and no back button, so a document link can
//! never load in place: `main.rs` cancels the navigation and calls `open`
//! here, which hands the URL to the desktop instead. That keeps the offline
//! guarantee intact, because the app still opens no socket of its own and the
//! page never leaves the origin the policy in `index.html` covers. The request
//! is made by the browser, once, because a person asked for it.

use std::process::{Command, Stdio};

use crate::assets;

/// Whether `url` is a document link worth handing to the desktop.
///
/// The app's own shell is excluded first. `WebView2` serves it from
/// `http://mhr.localhost/`, so a scheme test on its own would send the viewer
/// its own page to open in a browser.
///
/// Only the schemes a person can act on are accepted, and the list is
/// deliberately narrower than the sanitizer's. `data:` is allowed on the page
/// so an embedded image survives, but a `data:` URL opened in a browser
/// becomes a document in a real origin that the policy here no longer covers,
/// and `data:text/html` runs script. It stays out.
pub fn is_external(url: &str) -> bool {
    if assets::is_app_url(url) {
        return false;
    }

    ["http://", "https://", "mailto:"].iter().any(|scheme| {
        url.get(..scheme.len())
            .is_some_and(|head| head.eq_ignore_ascii_case(scheme))
    })
}

/// Hands `url` to the desktop's URL handler, which is what honours the
/// person's chosen browser and mail client. A URL this app has no business
/// opening is dropped.
///
/// The scheme check is repeated here rather than left to the caller: this
/// function is the one place a document-controlled string reaches a process
/// argument, so the fence belongs at the boundary it protects.
pub fn open(url: &str) {
    if !is_external(url) {
        return;
    }

    let (program, args): (&str, &[&str]) = if cfg!(target_os = "macos") {
        ("open", &[])
    } else if cfg!(target_os = "windows") {
        // Not `cmd /c start`, which parses `&` in a query string as a command
        // separator. This handler takes the URL as one argument and does not.
        ("rundll32.exe", &["url.dll,FileProtocolHandler"])
    } else {
        ("xdg-open", &[])
    };

    // The URL is one argument and no shell sees it, so a query string cannot
    // become anything but a query string.
    let spawned = Command::new(program)
        .args(args)
        .arg(url)
        // Otherwise the helper, and then the browser, writes into whatever
        // terminal launched `mhr`.
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn();

    match spawned {
        // The helper exits as soon as it has passed the URL on, well before the
        // browser finishes starting, so this wait is short. It still happens on
        // its own thread, because nothing on the event loop should wait on a
        // process; without the wait the exited helper stays a zombie for the
        // life of the window.
        Ok(mut child) => {
            std::thread::spawn(move || {
                let _ = child.wait();
            });
        }
        Err(e) => eprintln!("mhr: could not open {url}: {e}"),
    }
}

#[cfg(test)]
mod tests {
    use super::is_external;

    #[test]
    fn accepts_the_schemes_a_person_can_act_on() {
        for url in [
            "http://example.com/",
            "https://example.com/a?b=c&d=e",
            "mailto:someone@example.com",
            // A scheme is case-insensitive to the webview, so it has to be
            // case-insensitive here too.
            "HTTPS://example.com/",
            "MailTo:someone@example.com",
        ] {
            assert!(is_external(url), "{url}");
        }
    }

    #[test]
    fn refuses_the_apps_own_shell_on_both_origin_spellings() {
        for url in [
            "mhr://localhost/index.html",
            "mhr://localhost/#user-content-heading",
            "http://mhr.localhost/index.html",
            "http://mhr.localhost/app.js",
        ] {
            assert!(!is_external(url), "{url}");
        }
    }

    /// A host that merely begins with the app's host is not the app, so it
    /// takes the ordinary outbound route rather than being mistaken for the
    /// shell and dropped.
    #[test]
    fn accepts_a_host_that_only_starts_like_the_app_host() {
        assert!(is_external("http://mhr.localhost.example.com/"));
    }

    #[test]
    fn refuses_every_other_scheme() {
        for url in [
            "data:text/html,<script>alert(1)</script>",
            "data:image/png;base64,iVBORw0KGgo=",
            "javascript:alert(1)",
            "file:///etc/shadow",
            "about:blank",
            "",
            "./neighbour.md",
        ] {
            assert!(!is_external(url), "{url}");
        }
    }
}
