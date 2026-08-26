use rust_embed::Embed;
use std::borrow::Cow;
use std::sync::{Arc, Mutex};
use wry::WebViewId;
use wry::http::{Request, Response, header};

#[derive(Embed)]
#[folder = "assets/"]
struct Asset;

pub const SCHEME: &str = "mhr";

/// Custom-protocol origins differ by platform: `WebKit` (Linux, macOS) serves
/// `<scheme>://<host>/<path>`, while `WebView2` rewrites it to
/// `http://<scheme>.<host>/<path>`.
pub fn index_url() -> String {
    if cfg!(any(target_os = "windows", target_os = "android")) {
        format!("http://{SCHEME}.localhost/index.html")
    } else {
        format!("{SCHEME}://localhost/index.html")
    }
}

/// Side of the pre-rasterized window icon, in pixels.
pub const ICON_SIZE: u32 = 128;

/// The window icon as raw RGBA. `tao` wants pixels rather than an encoded
/// image, and decoding a PNG or an SVG at runtime would mean a dependency, so
/// `icon/window-icon.rgba` is rasterized ahead of time from `icon/mhr-icon.svg`
/// and embedded as pixels. `docs/vendored-assets.md` in the repository records
/// the command that regenerates it; the published crate excludes `docs/`.
pub fn icon_rgba() -> Option<Vec<u8>> {
    let file = Asset::get("icon/window-icon.rgba")?;
    let expected = ICON_SIZE as usize * ICON_SIZE as usize * 4;
    (file.data.len() == expected).then(|| file.data.into_owned())
}

/// Serves the embedded assets. `index.html` carries a `<!--CONTENT-->` marker
/// that is replaced with the current render, so the first paint needs no
/// JavaScript at all; later updates arrive through `evaluate_script`.
pub fn handler(
    body: Arc<Mutex<String>>,
) -> impl Fn(WebViewId, Request<Vec<u8>>) -> Response<Cow<'static, [u8]>> + Send + Sync + 'static {
    move |_id, request| {
        let path = request.uri().path().trim_start_matches('/');
        let path = if path.is_empty() { "index.html" } else { path };

        let Some(file) = Asset::get(path) else {
            return not_found();
        };

        let bytes: Cow<'static, [u8]> = if path == "index.html" {
            let shell = String::from_utf8_lossy(&file.data);
            let current = body.lock().map(|b| b.clone()).unwrap_or_default();
            Cow::Owned(shell.replace("<!--CONTENT-->", &current).into_bytes())
        } else {
            Cow::Owned(file.data.into_owned())
        };

        Response::builder()
            .status(200)
            .header(header::CONTENT_TYPE, mime_for(path))
            .body(bytes)
            .unwrap_or_else(|_| not_found())
    }
}

fn not_found() -> Response<Cow<'static, [u8]>> {
    Response::builder()
        .status(404)
        .body(Cow::Borrowed(&b"not found"[..]))
        .expect("static 404 response is always valid")
}

fn mime_for(path: &str) -> &'static str {
    match path.rsplit_once('.').map(|(_, ext)| ext) {
        Some("html") => "text/html; charset=utf-8",
        Some("js") => "text/javascript; charset=utf-8",
        Some("css") => "text/css; charset=utf-8",
        Some("svg") => "image/svg+xml",
        Some("woff2") => "font/woff2",
        Some("png") => "image/png",
        _ => "application/octet-stream",
    }
}

#[cfg(test)]
mod tests {
    use super::{Asset, SCHEME, handler, index_url, mime_for};
    use std::sync::{Arc, Mutex};
    use wry::http::Request;

    /// Runs one request through the real handler and returns the status and
    /// body, so the tests exercise the splice rather than a copy of it.
    fn get(path: &str, body: &str) -> (u16, String) {
        let serve = handler(Arc::new(Mutex::new(String::from(body))));
        let request = Request::builder()
            .uri(format!("{SCHEME}://localhost/{path}"))
            .body(Vec::new())
            .expect("test request is well formed");

        let response = serve("test-webview", request);
        let status = response.status().as_u16();
        (
            status,
            String::from_utf8_lossy(response.body()).into_owned(),
        )
    }

    /// The generated and vendored stylesheets, and the fonts in a subdirectory.
    /// A path mistake here breaks silently rather than loudly: math would still
    /// render, just with the wrong font and no environment alignment, and code
    /// would still render, just with no colors.
    #[test]
    fn embeds_the_vendored_stylesheets_and_fonts() {
        for path in [
            "latex.css",
            "highlight.css",
            "font/latinmodern-math.woff2",
            "font/lmroman12-regular.woff2",
            "font/lmroman12-bold.woff2",
            "font/lmroman12-italic.woff2",
        ] {
            assert!(Asset::get(path).is_some(), "{path} is not embedded");
        }
    }

    /// The four brand SVGs and the pre-rasterized window icon. The favicon
    /// links in `index.html` name two of these by path, and a rename would
    /// leave the page silently falling back to a blank icon.
    #[test]
    fn embeds_the_icon_set() {
        for path in [
            "icon/mhr-icon.svg",
            "icon/mhr-icon-mono.svg",
            "icon/mhr-icon-on-dark.svg",
            "icon/mhr-icon-16.svg",
            "icon/window-icon.rgba",
        ] {
            assert!(Asset::get(path).is_some(), "{path} is not embedded");
        }
    }

    /// `Icon::from_rgba` rejects a buffer whose length is not `w * h * 4`, and
    /// a regenerated blob at the wrong size would only show up as a missing
    /// icon at runtime. Checked here so the size mismatch fails the build.
    #[test]
    fn rasterized_icon_is_the_size_the_window_asks_for() {
        let rgba = super::icon_rgba().expect("window icon is embedded");
        assert_eq!(
            rgba.len(),
            super::ICON_SIZE as usize * super::ICON_SIZE as usize * 4
        );
    }

    /// The first paint carries the current render in the served HTML, so a
    /// window that opens before any JavaScript runs still shows the document.
    #[test]
    fn splices_the_current_render_into_the_shell() {
        let (status, body) = get("index.html", "<p>hello</p>");
        assert_eq!(status, 200);
        assert!(body.contains("<p>hello</p>"), "{body}");
        assert!(!body.contains("<!--CONTENT-->"), "marker survived: {body}");
    }

    /// A custom protocol request for the origin root arrives with an empty
    /// path, which has to mean the shell rather than a 404.
    #[test]
    fn serves_the_shell_for_an_empty_path() {
        let (status, body) = get("", "<p>hello</p>");
        assert_eq!(status, 200);
        assert!(body.contains("<p>hello</p>"), "{body}");
    }

    #[test]
    fn splices_nothing_into_assets_that_are_not_the_shell() {
        let (status, body) = get("app.js", "<p>hello</p>");
        assert_eq!(status, 200);
        assert!(
            !body.contains("hello"),
            "render leaked into a script: {body}"
        );
    }

    #[test]
    fn refuses_a_path_that_is_not_embedded() {
        assert_eq!(get("../Cargo.toml", "").0, 404);
        assert_eq!(get("nothing-here.css", "").0, 404);
    }

    #[test]
    fn labels_every_embedded_asset_with_a_type_a_browser_accepts() {
        assert_eq!(mime_for("index.html"), "text/html; charset=utf-8");
        assert_eq!(mime_for("app.js"), "text/javascript; charset=utf-8");
        assert_eq!(mime_for("github.css"), "text/css; charset=utf-8");
        assert_eq!(mime_for("font/lmroman12-regular.woff2"), "font/woff2");
        // An unknown extension, and a name with no extension at all, both have
        // to land somewhere rather than panicking on the rsplit.
        assert_eq!(mime_for("notes.bin"), "application/octet-stream");
        assert_eq!(mime_for("LICENSE"), "application/octet-stream");
    }

    /// `WebView2` rewrites a custom scheme to `http://<scheme>.localhost`, so
    /// the URL the webview is pointed at has to match the origin it will end up
    /// with, and the CSP in `index.html` has to allow both spellings.
    #[test]
    fn addresses_the_shell_the_way_the_platform_will_serve_it() {
        let url = index_url();
        assert!(url.ends_with("/index.html"), "{url}");

        if cfg!(any(target_os = "windows", target_os = "android")) {
            assert_eq!(url, format!("http://{SCHEME}.localhost/index.html"));
        } else {
            assert_eq!(url, format!("{SCHEME}://localhost/index.html"));
        }

        let csp =
            String::from_utf8_lossy(&Asset::get("index.html").expect("shell").data).into_owned();
        assert!(csp.contains(&format!("{SCHEME}:")), "CSP misses the scheme");
        assert!(
            csp.contains(&format!("http://{SCHEME}.localhost")),
            "CSP misses the WebView2 origin"
        );
    }
}
