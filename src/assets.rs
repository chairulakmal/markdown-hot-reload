//! The embedded frontend, and the custom protocol that serves it.
//!
//! Every file under `assets/` is compiled into the binary, so the webview
//! never reads the disk or the network for a stylesheet, a font or a script.
//! `handler` splices the current render into the shell so the first paint
//! needs no JavaScript, and the two URL predicates decide which navigations
//! stay inside the window.

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

/// Whether `url` stays on the app's own origin, on either origin spelling.
///
/// `link.rs` uses this to tell the app's own pages apart from a document link
/// worth handing to the desktop. It is not the navigation test: everything the
/// app serves shares this origin, so a URL can pass here and still address
/// something that is not the document. `is_shell_url` is what navigation asks.
/// A host that merely begins with the app host, such as
/// `mhr://localhost.example.com`, is not on the origin and is refused.
pub fn is_app_url(url: &str) -> bool {
    path_on_origin(url).is_some()
}

/// Whether `url` addresses the shell document itself, with any fragment or
/// query.
///
/// `main.rs` hands this to `WebViewBuilder::with_navigation_handler`, which
/// cancels every navigation it rejects. A rendered document is untrusted input
/// and may contain links, and two kinds of link have to be refused for the
/// same reason: the window has no address bar and no back button, so any
/// navigation that leaves the document is a dead end.
///
/// An off-origin link is the obvious one. It would load a remote page the
/// policy in `index.html` no longer covers, so `connect-src 'none'` and the
/// offline guarantee stop holding.
///
/// A relative link is the quiet one. `[other](./other.md)` resolves against
/// the shell and lands on this origin, so an origin test alone accepts it; the
/// webview then navigates, `handler` finds nothing embedded under that path,
/// and the window is left showing `not found` with no way back to the
/// document. The same holds for a link that happens to name an embedded asset,
/// which would replace the document with a stylesheet.
///
/// So only the shell passes. The empty path is the origin root, which `handler`
/// also answers with the shell. A same-page fragment jump, which is how a table
/// of contents works, keeps the path and is allowed.
pub fn is_shell_url(url: &str) -> bool {
    let Some(path) = path_on_origin(url) else {
        return false;
    };
    let path = path
        .split_once(['#', '?'])
        .map_or(path, |(before, _)| before);
    matches!(path, "" | "/" | "/index.html")
}

/// The path of `url` on either spelling of the app origin, fragment and query
/// still attached, or `None` if `url` is not on the origin at all.
fn path_on_origin(url: &str) -> Option<&str> {
    let on_origin = |origin: &str| {
        url.strip_prefix(origin)
            .filter(|rest| rest.is_empty() || rest.starts_with('/'))
    };
    on_origin(&format!("{SCHEME}://localhost"))
        .or_else(|| on_origin(&format!("http://{SCHEME}.localhost")))
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
/// JavaScript at all; later updates arrive through `evaluate_script`. A
/// `<!--VERSION-->` marker in the help overlay is replaced the same way, so the
/// version the overlay reports comes from the crate and a release bump cannot
/// leave it stale. Every other asset skips this branch, so both markers are
/// inert everywhere else.
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
            // Version first, then the render. In the other order the document
            // would be scanned for the version marker too, and a document is
            // untrusted input. `sanitize` strips comments, so nothing can carry
            // the marker this far today; the ordering keeps that a second line
            // of defence rather than the only one.
            Cow::Owned(
                shell
                    .replace("<!--VERSION-->", env!("CARGO_PKG_VERSION"))
                    .replace("<!--CONTENT-->", &current)
                    .into_bytes(),
            )
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

/// Reads one embedded file as text. Test-only, so that `main.rs` can pair
/// `INIT_SCRIPT` against the `app.js` it has to agree with without `Asset`
/// itself becoming part of the crate's surface.
#[cfg(test)]
pub fn embedded_text(path: &str) -> Option<String> {
    Asset::get(path).map(|f| String::from_utf8_lossy(&f.data).into_owned())
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
    use super::{Asset, SCHEME, handler, index_url, is_app_url, is_shell_url, mime_for};
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

    /// The generated and vendored stylesheets, the fonts in a subdirectory, and
    /// the diagram bundle `app.js` loads lazily by name. A path mistake here
    /// breaks silently rather than loudly: math would still render, just with
    /// the wrong font and no environment alignment, code would still render,
    /// just with no colors, and a diagram would fail only in a document that
    /// has one.
    #[test]
    fn embeds_the_vendored_stylesheets_and_fonts() {
        for path in [
            "latex.css",
            "highlight-light.css",
            "highlight-dark.css",
            "mermaid.min.js",
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

    /// The help overlay reports the running version, and it comes from the
    /// crate rather than a literal in the shell, so a release bump cannot leave
    /// the About block claiming an older one.
    #[test]
    fn splices_the_crate_version_into_the_shell() {
        let (status, body) = get("index.html", "<p>hello</p>");
        assert_eq!(status, 200);
        assert!(!body.contains("<!--VERSION-->"), "marker survived: {body}");
        assert!(
            body.contains(env!("CARGO_PKG_VERSION")),
            "shell does not name the crate version: {body}"
        );
    }

    /// The render is untrusted input, so the version marker is replaced before
    /// the document is spliced in and not after. In the other order a document
    /// carrying the marker would have it substituted too, which is the app
    /// putting its own text where a document's text should be.
    #[test]
    fn replaces_the_version_marker_before_splicing_the_document() {
        let (_, body) = get("index.html", "<p><!--VERSION--></p>");
        assert_eq!(
            body.matches(env!("CARGO_PKG_VERSION")).count(),
            1,
            "the version was substituted inside the document as well: {body}"
        );
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

    /// Offline is enforced by the policy, not merely intended, so the two
    /// directives that enforce it are pinned here. A stray `'unsafe-inline'`
    /// in `script-src`, or a `connect-src` that permits anything, would leave
    /// the app rendering identically on this machine and differently on one
    /// with no network, which is the failure the invariant exists to prevent.
    #[test]
    fn the_policy_forbids_network_access_and_inline_script() {
        let csp = shell();
        assert!(csp.contains("connect-src 'none'"), "{csp}");
        assert!(csp.contains("object-src 'none'"), "{csp}");
        assert!(csp.contains("base-uri 'none'"), "{csp}");
        assert!(csp.contains("form-action 'none'"), "{csp}");

        let script_src = directive(&csp, "script-src");
        assert!(
            !script_src.contains("unsafe-inline") && !script_src.contains("unsafe-eval"),
            "script-src relaxed: {script_src}"
        );
        for directive_name in ["default-src", "script-src", "img-src", "font-src"] {
            let value = directive(&csp, directive_name);
            assert!(
                !value.contains("https:") && !value.contains("http:")
                    || value.contains("http://mhr.localhost"),
                "{directive_name} reaches off-origin: {value}"
            );
        }
    }

    /// Every relative asset `index.html` names has to be embedded under that
    /// exact path. A rename or a typo fails silently at runtime: the request
    /// 404s, the page still loads, and the app comes up with no syntax colors,
    /// no diagram morphing or no key handling, depending on which line was
    /// wrong. Scanning the shell rather than listing the paths keeps this
    /// honest as files are added.
    #[test]
    fn embeds_every_asset_the_shell_names() {
        let shell = shell();
        let scripts = attribute_values(&shell, "src");
        for name in ["app.js", "chrome.js"] {
            assert!(
                scripts.iter().any(|src| src == name),
                "the shell stopped loading {name}: {scripts:?}"
            );
        }

        let mut named = scripts;
        named.extend(attribute_values(&shell, "href"));
        for path in named {
            // An overlay's About block links the repository, which is off-origin
            // and served by nobody here; the navigation handler sends it to the
            // system browser instead.
            if path.contains("://") || path.starts_with('#') || path.starts_with("mailto:") {
                continue;
            }
            assert!(
                Asset::get(&path).is_some(),
                "index.html names {path}, which is not embedded"
            );
        }
    }

    /// The theme override forces a syntax palette by rewriting the `media`
    /// attribute on these two links. That only works if the palette lives on
    /// the attribute and not in a query inside the file: a `@media` block in
    /// either sheet would keep tracking the OS whatever the attribute says, and
    /// a forced-dark page would show light code. So the split has to stay split
    /// and each link has to keep its `media`.
    #[test]
    fn each_highlight_sheet_is_media_gated_and_carries_no_media_query() {
        // The shell is pretty-printed with one attribute per line, so collapse
        // runs of whitespace before matching the link.
        let shell = shell();
        let collapsed = shell.split_whitespace().collect::<Vec<_>>().join(" ");
        for (sheet, scheme) in [
            ("highlight-light.css", "(prefers-color-scheme: light)"),
            ("highlight-dark.css", "(prefers-color-scheme: dark)"),
        ] {
            assert!(
                collapsed.contains(&format!("href=\"{sheet}\" media=\"{scheme}\"")),
                "the {sheet} link is not gated by {scheme}: {collapsed}"
            );

            let css = String::from_utf8_lossy(
                &Asset::get(sheet)
                    .unwrap_or_else(|| panic!("{sheet} is embedded"))
                    .data,
            )
            .into_owned();
            assert!(
                !css.contains("@media"),
                "{sheet} still wraps its rules in a media query, so the override cannot force it"
            );
        }
    }

    /// github.css carries the palette three ways: light at `:root`, dark for
    /// "OS dark and not forced light", dark for "forced dark". Flattened back
    /// to a plain light/dark pair, the in-app override silently stops working,
    /// because nothing on the page could then force the palette the OS did not
    /// pick. This pins the two selectors that make the third state exist.
    #[test]
    fn github_css_keeps_the_three_state_theme_ladder() {
        let css = String::from_utf8_lossy(
            &Asset::get("github.css")
                .expect("github.css is embedded")
                .data,
        )
        .into_owned();
        assert!(
            css.contains(":root[data-theme=\"dark\"]"),
            "github.css lost the forced-dark selector"
        );
        assert!(
            css.contains(":root:not([data-theme=\"light\"])"),
            "github.css lost the guard that lets forced-light win under an OS dark preference"
        );
    }

    /// A variable added to one palette block and missed in another falls back
    /// to the light value, so it shows only under one theme and one override,
    /// the combination a sweep is most likely to skip.
    #[test]
    fn every_palette_block_defines_the_same_variables() {
        let light = palette(PALETTES[0]).into_keys().collect::<Vec<_>>();
        assert!(!light.is_empty(), "found no variables under `:root`");
        for selector in &PALETTES[1..] {
            assert_eq!(
                palette(selector).into_keys().collect::<Vec<_>>(),
                light,
                "`{selector}` differs from `:root`"
            );
        }
    }

    /// Every palette block has to pass two contrast models. WCAG 2's ratio is
    /// the one audits check, so it is the floor. APCA is the perceptual model
    /// behind the WCAG 3 drafts, and it catches what WCAG 2 misses: GitHub's
    /// own dark muted text passes WCAG 2 at 6.5:1 and scores Lc 46.
    ///
    /// The Lc targets are APCA's published levels. 90 is what it prefers for
    /// 16px body text and 75 its minimum, which secondary text and links are
    /// held to because they are read at body size. 60 is for bold text such as
    /// alert titles. 30 is for a solid mark that carries meaning, the quote bar
    /// and the checkbox outline, and 15 is where a thin line stops being
    /// visible at all.
    #[test]
    fn palette_meets_the_contrast_targets() {
        // Foreground, background, minimum APCA Lc, minimum WCAG 2 ratio.
        let pairs = [
            ("--fg", "--bg", 90.0, 7.0),
            ("--fg", "--code-bg", 90.0, 7.0),
            ("--fg-muted", "--bg", 75.0, 4.5),
            ("--fg-muted", "--code-bg", 75.0, 4.5),
            ("--link", "--bg", 75.0, 4.5),
            // Inline `code` sets a background and no colour, so a link
            // wrapping one renders link text on --code-bg at 0.875em.
            ("--link", "--code-bg", 75.0, 4.5),
            ("--accent-note", "--bg", 60.0, 4.5),
            ("--accent-tip", "--bg", 60.0, 4.5),
            ("--accent-important", "--bg", 60.0, 4.5),
            ("--accent-warning", "--bg", 60.0, 4.5),
            ("--accent-caution", "--bg", 60.0, 4.5),
            ("--focus", "--bg", 0.0, 3.0),
            ("--border-emphasis", "--bg", 30.0, 3.0),
            ("--border", "--bg", 15.0, 1.0),
            ("--border-subtle", "--bg", 15.0, 1.0),
        ];
        for selector in PALETTES {
            let colours = palette(selector);
            let colour = |name: &str| {
                colours
                    .get(name)
                    .unwrap_or_else(|| panic!("`{selector}` has no {name}"))
                    .clone()
            };
            for (fg, bg, lc, ratio) in pairs {
                let (fg_value, bg_value) = (colour(fg), colour(bg));
                let got_lc = apca_lc(&fg_value, &bg_value);
                let got_ratio = wcag_ratio(&fg_value, &bg_value);
                assert!(
                    got_lc >= lc && got_ratio >= ratio,
                    "{fg} {fg_value} on {bg} {bg_value} under `{selector}`: \
                     Lc {got_lc:.1} (needs {lc}), {got_ratio:.2}:1 (needs {ratio}:1)"
                );
            }
        }
    }

    /// Pins both formulas to published results, so a slip in either one
    /// cannot quietly loosen the palette test above. The Lc values are the
    /// reference pairs from the APCA-W3 documentation.
    #[test]
    fn contrast_math_matches_the_reference_values() {
        let close = |got: f64, want: f64| (got - want).abs() < 0.01;
        assert!(close(apca_lc("#888888", "#ffffff"), 63.06));
        assert!(close(apca_lc("#ffffff", "#888888"), 68.54));
        assert!(close(apca_lc("#000000", "#aaaaaa"), 58.15));
        assert!(close(apca_lc("#aaaaaa", "#000000"), 56.24));
        assert!(close(wcag_ratio("#000000", "#ffffff"), 21.0));
        assert!(close(wcag_ratio("#777777", "#ffffff"), 4.48));
    }

    /// The three palette blocks in github.css: light, dark by OS preference,
    /// and dark forced from the app.
    const PALETTES: [&str; 3] = [
        ":root",
        ":root:not([data-theme=\"light\"])",
        ":root[data-theme=\"dark\"]",
    ];

    /// The custom properties one palette block defines, name to value.
    fn palette(selector: &str) -> std::collections::BTreeMap<String, String> {
        let css = String::from_utf8_lossy(
            &Asset::get("github.css")
                .expect("github.css is embedded")
                .data,
        )
        .into_owned();
        let start = css
            .find(&format!("{selector} {{"))
            .unwrap_or_else(|| panic!("github.css has no `{selector}` block"));
        let block = &css[start..];
        let block = &block[..block.find('}').expect("the block is closed")];
        block
            .lines()
            .filter_map(|line| line.trim().strip_suffix(';')?.split_once(':'))
            .filter(|(name, _)| name.starts_with("--"))
            .map(|(name, value)| (name.to_string(), value.trim().to_string()))
            .collect()
    }

    fn rgb(colour: &str) -> [f64; 3] {
        let hex = colour
            .strip_prefix('#')
            .filter(|hex| hex.len() == 6)
            .unwrap_or_else(|| panic!("{colour} is not #rrggbb, the only form this test reads"));
        let channel = |i: usize| {
            f64::from(u8::from_str_radix(&hex[i..i + 2], 16).expect("hex digits")) / 255.0
        };
        [channel(0), channel(2), channel(4)]
    }

    /// The WCAG 2 contrast ratio, which ignores which colour is the text.
    fn wcag_ratio(a: &str, b: &str) -> f64 {
        let luminance = |c: [f64; 3]| {
            let linear = |v: f64| {
                if v <= 0.040_45 {
                    v / 12.92
                } else {
                    ((v + 0.055) / 1.055).powf(2.4)
                }
            };
            0.2126 * linear(c[0]) + 0.7152 * linear(c[1]) + 0.0722 * linear(c[2])
        };
        let (x, y) = (luminance(rgb(a)), luminance(rgb(b)));
        (x.max(y) + 0.05) / (x.min(y) + 0.05)
    }

    /// APCA lightness contrast as a magnitude, whichever colour is lighter.
    /// Constants are APCA-W3 0.0.98G-4g, from `src/apca-w3.js` in
    /// github.com/Myndex/apca-w3.
    fn apca_lc(text: &str, background: &str) -> f64 {
        let luminance = |c: [f64; 3]| {
            let y = 0.212_672_9 * c[0].powf(2.4)
                + 0.715_152_2 * c[1].powf(2.4)
                + 0.072_175 * c[2].powf(2.4);
            if y > 0.022 {
                y
            } else {
                y + (0.022 - y).powf(1.414)
            }
        };
        let (fg, bg) = (luminance(rgb(text)), luminance(rgb(background)));
        if (bg - fg).abs() < 0.0005 {
            return 0.0;
        }
        let sapc = if bg > fg {
            (bg.powf(0.56) - fg.powf(0.57)) * 1.14
        } else {
            (bg.powf(0.65) - fg.powf(0.62)) * 1.14
        };
        if sapc.abs() < 0.1 {
            0.0
        } else {
            (sapc.abs() - 0.027) * 100.0
        }
    }

    /// Every value of one double-quoted attribute in `html`, in document order.
    /// The match starts at an attribute boundary, so asking for `src` does not
    /// also collect the value of a `data-src`.
    fn attribute_values(html: &str, attribute: &str) -> Vec<String> {
        let needle = format!("{attribute}=\"");
        html.match_indices(&needle)
            .filter(|(i, _)| html[..*i].ends_with(char::is_whitespace))
            .filter_map(|(i, m)| {
                let rest = &html[i + m.len()..];
                rest.find('"').map(|end| rest[..end].to_string())
            })
            .collect()
    }

    fn shell() -> String {
        String::from_utf8_lossy(&Asset::get("index.html").expect("shell is embedded").data)
            .into_owned()
    }

    /// Pulls one directive's value out of the policy so a test can assert about
    /// `script-src` without matching text that belongs to `style-src`, which
    /// legitimately carries `'unsafe-inline'`.
    fn directive<'a>(csp: &'a str, name: &str) -> &'a str {
        let start = csp
            .find(&format!("{name} "))
            .unwrap_or_else(|| panic!("policy has no {name} directive"));
        let rest = &csp[start..];
        &rest[..rest.find(';').unwrap_or(rest.len())]
    }

    /// `link.rs` asks this to keep the app's own pages out of the outbound
    /// route, so both origin spellings pass and every external scheme, plus a
    /// look-alike host that only shares a prefix, does not.
    #[test]
    fn recognizes_the_app_origin_and_refuses_everything_else() {
        assert!(is_app_url("mhr://localhost/index.html"));
        assert!(is_app_url("mhr://localhost/index.html#user-content-h"));
        assert!(is_app_url("mhr://localhost/app.js"));
        assert!(is_app_url("mhr://localhost"));
        assert!(is_app_url("http://mhr.localhost/index.html"));

        assert!(!is_app_url("https://example.com/"));
        assert!(!is_app_url("http://example.com/"));
        assert!(!is_app_url("mailto:someone@example.com"));
        assert!(!is_app_url("file:///etc/passwd"));
        assert!(!is_app_url("data:text/html,<script>alert(1)</script>"));

        assert!(!is_app_url("mhr://localhost.example.com/"));
        assert!(!is_app_url("http://mhr.localhost.example.com/"));
    }

    /// The navigation handler in `main.rs` cancels anything this rejects. Being
    /// on the app's origin is not enough, because a relative link in a document
    /// resolves onto that origin: `[other](./other.md)` becomes
    /// `mhr://localhost/other.md`, which the asset handler answers with 404,
    /// leaving a window that has no address bar and no back button showing
    /// `not found`. That is the bug this test pins. An embedded asset is
    /// refused for the same reason, since navigating to one would replace the
    /// document with a stylesheet. Only the shell, with any fragment, passes.
    #[test]
    fn allows_navigation_to_the_shell_and_nothing_else_on_the_origin() {
        for url in [
            "mhr://localhost/index.html",
            "mhr://localhost/index.html#user-content-h",
            "mhr://localhost/index.html?v=1",
            "mhr://localhost/",
            "mhr://localhost",
            "http://mhr.localhost/index.html",
            "http://mhr.localhost/",
        ] {
            assert!(is_shell_url(url), "{url}");
        }

        for url in [
            // The relative document links that used to strand the window.
            "mhr://localhost/other.md",
            "mhr://localhost/docs/notes.md",
            "http://mhr.localhost/other.md",
            // Embedded assets, which navigation has no business reaching even
            // though the handler serves them happily to the page.
            "mhr://localhost/app.js",
            "mhr://localhost/github.css",
            // A path that merely starts like the shell's.
            "mhr://localhost/index.html.md",
            // Off-origin, already refused by the origin test.
            "https://example.com/",
            "mhr://localhost.example.com/index.html",
        ] {
            assert!(!is_shell_url(url), "{url}");
        }
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

        let csp = shell();
        assert!(csp.contains(&format!("{SCHEME}:")), "CSP misses the scheme");
        assert!(
            csp.contains(&format!("http://{SCHEME}.localhost")),
            "CSP misses the WebView2 origin"
        );
    }
}
