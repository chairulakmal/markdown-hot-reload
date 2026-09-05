mod assets;
mod cli;
mod link;
mod math;
mod render;
mod watch;

use anyhow::{Context, Result};
use std::path::Path;
use std::sync::{Arc, Mutex};
use tao::dpi::LogicalSize;
use tao::event::{Event, WindowEvent};
use tao::event_loop::{ControlFlow, EventLoopBuilder};
use tao::window::{Icon, WindowBuilder};
use wry::{NewWindowResponse, WebView, WebViewBuilder};

#[derive(Debug)]
pub enum UserEvent {
    Changed,
    Vanished,
}

/// Buffers renders that arrive before app.js has run, so a save during page
/// load is not dropped. app.js replaces `__render` and drains the queue.
const INIT_SCRIPT: &str = "window.__q=[];window.__render=h=>window.__q.push(h);";

fn main() -> Result<()> {
    let path = match cli::parse(std::env::args_os().skip(1))? {
        cli::Request::Help => {
            println!("{}", cli::USAGE);
            return Ok(());
        }
        cli::Request::Version => {
            println!("{} {}", env!("CARGO_PKG_NAME"), env!("CARGO_PKG_VERSION"));
            return Ok(());
        }
        cli::Request::Open(arg) => cli::open(&arg)?,
    };

    let body = Arc::new(Mutex::new(read_and_render(&path)));

    // No app id is set here, on purpose. GTK claims one as a session-bus name,
    // which strict snap confinement refuses, and tao turns that refusal into a
    // panic before the window opens. Without an id, GTK falls back to the
    // program name, which is `mhr` in every package, and that is what
    // `StartupWMClass` in the two desktop files matches. See AGENTS.md.
    let event_loop = EventLoopBuilder::<UserEvent>::with_user_event().build();
    let proxy = event_loop.create_proxy();

    let window = WindowBuilder::new()
        .with_title(title(&path))
        .with_window_icon(window_icon())
        .with_inner_size(LogicalSize::new(920.0, 1000.0))
        .build(&event_loop)?;

    let builder = WebViewBuilder::new()
        .with_initialization_script(INIT_SCRIPT)
        .with_custom_protocol(assets::SCHEME.to_string(), assets::handler(body.clone()))
        // A document is untrusted input and may carry links. Without this, one
        // click loads a remote page into a window with no address bar and no
        // way back, on a page the CSP no longer covers. Only the app's own
        // shell, fragment jumps included, is allowed to load. An outbound link
        // is handed to the desktop browser instead of being swallowed, because
        // a viewer that answers a deliberate click with nothing at all leaves
        // the person no way to reach the page and no way to see where it went.
        .with_navigation_handler(|url| {
            if assets::is_app_url(&url) {
                return true;
            }
            link::open(&url);
            false
        })
        // Nothing in a document can ask for a window, since `target` is not in
        // the sanitizer's allowlist for `a`. A modifier-click can still arrive
        // here, so the URL takes the same route rather than vanishing, and the
        // app still opens no second window of its own.
        .with_new_window_req_handler(|url, _features| {
            link::open(&url);
            NewWindowResponse::Deny
        })
        .with_url(assets::index_url());

    // WebKitGTK attaches to a GTK container, not to a raw window handle, so the
    // portable `build(&window)` path is only correct on macOS and Windows.
    #[cfg(not(any(
        target_os = "linux",
        target_os = "dragonfly",
        target_os = "freebsd",
        target_os = "netbsd",
        target_os = "openbsd"
    )))]
    let webview = builder.build(&window)?;
    #[cfg(any(
        target_os = "linux",
        target_os = "dragonfly",
        target_os = "freebsd",
        target_os = "netbsd",
        target_os = "openbsd"
    ))]
    let webview = {
        use tao::platform::unix::WindowExtUnix;
        use wry::WebViewBuilderExtUnix;

        let vbox = window
            .default_vbox()
            .context("tao window exposed no GTK vbox to attach the webview to")?;
        builder.build_gtk(vbox)?
    };

    // The watcher thread only knows how to say "something happened"; turning
    // that into a redraw is the event loop's job, below.
    let _debouncer = watch::spawn(path.clone(), move |event| {
        let _ = proxy.send_event(event);
    })?;

    event_loop.run(move |event, _, control_flow| {
        *control_flow = ControlFlow::Wait;

        match event {
            Event::UserEvent(UserEvent::Changed) => {
                let html = read_and_render(&path);
                if let Ok(mut slot) = body.lock() {
                    slot.clone_from(&html);
                }
                push(&webview, &html);
            }
            Event::UserEvent(UserEvent::Vanished) => {
                push(&webview, NOTICE_VANISHED);
            }
            Event::WindowEvent {
                event: WindowEvent::CloseRequested,
                ..
            } => *control_flow = ControlFlow::Exit,
            _ => {}
        }
    });
}

/// `data-mhr-notice` rather than a class: `render::sanitize` strips an unknown
/// data-* attribute from a document, so nothing a document contains can be
/// styled as one of the app's own notices.
const NOTICE_VANISHED: &str =
    "<p data-mhr-notice>File is gone. Still watching, will redraw if it comes back.</p>";

fn push(webview: &WebView, html: &str) {
    let json = serde_json::to_string(html).unwrap_or_else(|_| String::from("\"\""));
    let _ = webview.evaluate_script(&format!("window.__render({json})"));
}

fn read_and_render(path: &Path) -> String {
    match std::fs::read_to_string(path) {
        Ok(markdown) => render::to_html(&markdown),
        Err(e) => format!(
            "<p data-mhr-notice>Cannot read {}: {}</p>",
            render::escape_html(&path.display().to_string()),
            render::escape_html(&e.to_string())
        ),
    }
}

/// A missing or malformed icon is not worth failing a launch over: the window
/// opens with the desktop's default mark instead. Wayland ignores this
/// regardless and takes the icon from the `.desktop` file, which the shell
/// finds by matching the window class against `StartupWMClass`.
fn window_icon() -> Option<Icon> {
    Icon::from_rgba(assets::icon_rgba()?, assets::ICON_SIZE, assets::ICON_SIZE).ok()
}

fn title(path: &Path) -> String {
    let name = path.file_name().map_or_else(
        || path.display().to_string(),
        |n| n.to_string_lossy().into_owned(),
    );
    format!("{name} · mhr")
}

#[cfg(test)]
mod tests {
    use super::{INIT_SCRIPT, NOTICE_VANISHED, read_and_render, title};
    use std::path::Path;

    /// `INIT_SCRIPT` and `app.js` are two files in two languages that have to
    /// agree on two names. Rust queues renders into `window.__q` behind a
    /// placeholder `window.__render`; `app.js` replaces the placeholder and
    /// drains the queue. Rename either name on one side and reloading stops
    /// working with no error anywhere: the placeholder keeps swallowing every
    /// render into an array nothing reads. Nothing else in the suite pairs
    /// them, because one is a Rust string and the other an embedded asset.
    #[test]
    fn the_reload_handshake_uses_the_same_names_on_both_sides() {
        let app_js = crate::assets::embedded_text("app.js").expect("app.js is embedded");

        assert_eq!(
            shared_globals(INIT_SCRIPT),
            shared_globals(&app_js),
            "the two sides of the reload handshake name different globals"
        );
        assert_eq!(
            shared_globals(INIT_SCRIPT),
            ["q", "render"].map(String::from).into_iter().collect(),
            "the handshake changed shape; check that push() still agrees"
        );
    }

    /// Every `window.__name` identifier in `source`, whole rather than by
    /// prefix. A `contains("window.__q")` check would be satisfied by
    /// `window.__queue`, so a rename on one side only would pass it.
    fn shared_globals(source: &str) -> std::collections::BTreeSet<String> {
        source
            .match_indices("window.__")
            .map(|(i, m)| {
                source[i + m.len()..]
                    .chars()
                    .take_while(|c| c.is_ascii_alphanumeric() || *c == '_')
                    .collect()
            })
            .collect()
    }

    /// The title bar is the only place the filename is shown, so a path with
    /// no file name still has to produce something rather than an empty title.
    #[test]
    fn titles_the_window_after_the_file() {
        assert_eq!(title(Path::new("/docs/notes.md")), "notes.md · mhr");
        assert_eq!(title(Path::new("/")), "/ · mhr");
    }

    /// A read failure reaches the webview as HTML, so the path and the
    /// operating system's message both have to be escaped on the way in. The
    /// path is attacker-controlled in the sense that matters here: it is
    /// whatever was typed at the shell, and it is spliced into a document.
    #[test]
    fn escapes_the_path_and_the_error_in_a_read_failure_notice() {
        let html = read_and_render(Path::new("<script>alert(1)</script>.md"));
        assert!(html.contains("data-mhr-notice"), "{html}");
        assert!(!html.contains("<script"), "{html}");
        assert!(html.contains("&lt;script&gt;"), "{html}");
    }

    #[test]
    fn renders_a_file_it_can_read() {
        let html = read_and_render(Path::new("fixtures/kitchen-sink.md"));
        assert!(html.contains("<table>"), "{html}");
        assert!(
            !html.contains("data-mhr-notice"),
            "read reported a failure: {html}"
        );
    }

    /// Both notices are spliced into the page as HTML, so they have to be
    /// valid on their own rather than relying on the caller to wrap them.
    #[test]
    fn the_vanished_notice_is_self_contained_markup() {
        assert!(NOTICE_VANISHED.starts_with("<p data-mhr-notice>"));
        assert!(NOTICE_VANISHED.ends_with("</p>"));
    }
}
