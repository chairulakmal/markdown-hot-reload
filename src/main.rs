mod assets;
mod cli;
mod math;
mod render;
mod watch;

use anyhow::{Context, Result};
use std::path::Path;
use std::sync::{Arc, Mutex};
use tao::dpi::LogicalSize;
use tao::event::{Event, WindowEvent};
use tao::event_loop::{ControlFlow, EventLoopBuilder};
#[cfg(any(
    target_os = "linux",
    target_os = "dragonfly",
    target_os = "freebsd",
    target_os = "netbsd",
    target_os = "openbsd"
))]
use tao::platform::unix::EventLoopBuilderExtUnix;
use tao::window::{Icon, WindowBuilder};
use wry::{WebView, WebViewBuilder};

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

    let mut event_loop_builder = EventLoopBuilder::<UserEvent>::with_user_event();
    // Without an explicit app id, GTK falls back to the invoking process's
    // prgname, which is not guaranteed to be "mhr" under every launcher, so
    // the desktop shell has nothing reliable to match against the installed
    // `.desktop` file's `StartupWMClass` and shows a generic icon instead.
    #[cfg(any(
        target_os = "linux",
        target_os = "dragonfly",
        target_os = "freebsd",
        target_os = "netbsd",
        target_os = "openbsd"
    ))]
    event_loop_builder.with_app_id("mhr");
    let event_loop = event_loop_builder.build();
    let proxy = event_loop.create_proxy();

    let window = WindowBuilder::new()
        .with_title(title(&path))
        .with_window_icon(window_icon())
        .with_inner_size(LogicalSize::new(920.0, 1000.0))
        .build(&event_loop)?;

    let builder = WebViewBuilder::new()
        .with_initialization_script(INIT_SCRIPT)
        .with_custom_protocol(assets::SCHEME.to_string(), assets::handler(body.clone()))
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

const NOTICE_VANISHED: &str =
    "<p class=\"mhr-notice\">File is gone. Still watching, will redraw if it comes back.</p>";

fn push(webview: &WebView, html: &str) {
    let json = serde_json::to_string(html).unwrap_or_else(|_| String::from("\"\""));
    let _ = webview.evaluate_script(&format!("window.__render({json})"));
}

fn read_and_render(path: &Path) -> String {
    match std::fs::read_to_string(path) {
        Ok(markdown) => render::to_html(&markdown),
        Err(e) => format!(
            "<p class=\"mhr-notice\">Cannot read {}: {}</p>",
            render::escape_html(&path.display().to_string()),
            render::escape_html(&e.to_string())
        ),
    }
}

/// A missing or malformed icon is not worth failing a launch over: the window
/// opens with the desktop's default mark instead. Wayland ignores this
/// regardless and takes the icon from the `.desktop` file, matched by app id.
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
