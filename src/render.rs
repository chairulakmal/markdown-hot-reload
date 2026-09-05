use crate::math;
use comrak::adapters::CodefenceRendererAdapter;
use comrak::html::ChildRendering;
use comrak::nodes::{AstNode, NodeValue, Sourcepos};
use comrak::options::Plugins;
use comrak::plugins::syntect::{SyntectAdapter, SyntectAdapterBuilder};
use comrak::{Anchorizer, Arena, Options, create_formatter, parse_document};
use std::collections::HashSet;
use std::fmt;
use std::sync::OnceLock;

/// Shared between `options()`, which prefixes every heading id, and
/// `rewrite_local_anchor_links()`, which has to prefix the same way when it
/// rewrites a link pointing at one, so the two can never drift apart.
const HEADER_ID_PREFIX: &str = "user-content-";

/// Emits mermaid fences as `<pre class="mermaid">`, the shape mermaid.js scans
/// for, instead of letting syntect fail to find a `mermaid` syntax and fall
/// back to plain text with the language marker stripped.
struct Mermaid;

impl CodefenceRendererAdapter for Mermaid {
    fn write(
        &self,
        output: &mut dyn fmt::Write,
        _lang: &str,
        _meta: &str,
        code: &str,
        _sourcepos: Option<Sourcepos>,
    ) -> fmt::Result {
        // Diagram source is untrusted document text and bypasses syntect's own
        // escaping, so it is escaped here before reaching the webview.
        write!(output, "<pre class=\"mermaid\">{}</pre>", escape_html(code))
    }
}

/// The one place plain text gets turned into safe HTML, shared with `main.rs`
/// for its own error notices so escaping logic exists in a single place.
pub fn escape_html(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

/// Highlights to CSS classes rather than inline styles. Passing a theme name
/// here would bake one theme's colors into the HTML, including a
/// `background-color` on the `<pre>` that no stylesheet can override, so a dark
/// page would show a white code block. The classes let `highlight.css` pick the
/// palette from `prefers-color-scheme` instead, and leave the block's own
/// background to `--code-bg` in `github.css`.
fn syntect() -> &'static SyntectAdapter {
    static ADAPTER: OnceLock<SyntectAdapter> = OnceLock::new();
    ADAPTER.get_or_init(|| {
        SyntectAdapterBuilder::new()
            .css_with_class_prefix("hl-")
            .build()
    })
}

fn options() -> Options<'static> {
    let mut o = Options::default();

    o.extension.strikethrough = true;
    o.extension.tagfilter = true;
    o.extension.table = true;
    o.extension.autolink = true;
    o.extension.tasklist = true;
    o.extension.footnotes = true;
    o.extension.description_lists = true;
    o.extension.alerts = true;
    o.extension.superscript = true;
    o.extension.multiline_block_quotes = true;
    o.extension.math_dollars = true;
    o.extension.math_code = true;
    o.extension.header_id_prefix = Some(String::from(HEADER_ID_PREFIX));
    // Without this, the id gets the prefix but the heading's own anchor link
    // does not, so clicking it jumps to a fragment nothing has.
    o.extension.header_id_prefix_in_href = true;
    o.extension.front_matter_delimiter = Some(String::from("---"));

    // Match what GitHub actually renders rather than what the spec alone says.
    o.render.gfm_quirks = true;
    o.render.tasklist_classes = true;

    // Raw HTML passes through comrak so that the GitHub-safe subset (tables,
    // <details>, <kbd>, alignment) renders like it does on github.com. Nothing
    // reaches the webview until `sanitize` has filtered it against an allowlist.
    // The `tagfilter` extension above is the earlier pass that neutralises
    // <script> and friends before the string is even built.
    o.render.r#unsafe = true;

    o
}

/// Filters the rendered document against an allowlist before it reaches the
/// webview. This is the whole reason `render.r#unsafe` can be true: a document
/// may contain any HTML, and only the elements, attributes and URL schemes
/// named here survive. GitHub's own sanitizer permits close to this set.
///
/// ammonia runs over the entire document, not only the document-authored
/// spans, so the allowlist has to be a superset of what comrak and the math
/// converter emit as well. A tag one of them generates that is missing here
/// would be stripped from the output.
fn sanitize(html: &str) -> String {
    ammonia::Builder::default()
        // `section` wraps the footnote list comrak emits; `input` is the
        // task-list checkbox. Neither is in ammonia's default set.
        .add_tags(["input", "section"])
        .add_tags(math::ELEMENTS.iter().copied())
        .add_generic_attributes(["class", "id", "align"])
        // Every MathML attribute the converter can write, except `style`. The
        // page carries no inline styles by design (see the syntect note in
        // `AGENTS.md`); dropping it here costs at most some spacing on an
        // exotic expression, which `math::is_trusted` had already accepted.
        .add_generic_attributes(math::ATTRIBUTES.iter().copied().filter(|a| *a != "style"))
        // A task-list checkbox is the only `<input>` this viewer has any reason
        // to draw, and it is never interactive. Naming `type` in
        // `add_tag_attributes` would allow any value, so the value is pinned
        // instead and `disabled` is forced on every one: a read-only window
        // must not grow a text box because a document asked for one.
        .add_tag_attributes("input", ["checked"])
        .add_tag_attribute_values("input", "type", ["checkbox"])
        .set_tag_attribute_value("input", "disabled", "")
        // github.com honours `open`, so a document that ships an expanded
        // disclosure block renders expanded here too.
        .add_tag_attributes("details", ["open"])
        .add_tag_attributes(
            "a",
            [
                "aria-label",
                "data-heading-content",
                "data-footnote-ref",
                "data-footnote-backref",
                "data-footnote-backref-idx",
            ],
        )
        .add_tag_attributes("section", ["data-footnotes"])
        // A URL on the page may not point anywhere that fetches or executes.
        // `connect-src 'none'` in the CSP is the backstop; this is the fence.
        // `data:` is here only so an embedded image survives, and
        // `is_embedded_image` is what keeps the rest of that scheme out.
        .url_schemes(HashSet::from(["http", "https", "mailto", "data"]))
        .attribute_filter(|_element, attribute, value| {
            // Leading whitespace is stripped before the comparison because a
            // browser strips it before resolving the URL, so ` data:...` is the
            // same URL to the webview and has to be the same URL here.
            let url = value.trim_start();
            let is_data = url
                .get(..5)
                .is_some_and(|s| s.eq_ignore_ascii_case("data:"));
            if matches!(attribute, "src" | "href") && is_data && !is_embedded_image(url) {
                return None;
            }
            Some(value.into())
        })
        .clean(html)
        .to_string()
}

/// Whether a `data:` URL carries a raster image and nothing else.
///
/// An embedded image is the only image an offline viewer can show, so the
/// scheme cannot simply be banned. It also cannot be trusted: `data:text/html`
/// and `data:image/svg+xml` both run script in a webview. This is the rule
/// comrak applied itself while `render.r#unsafe` was false, tightened by
/// requiring the media type to end where a real data URL ends, at `;` or `,`.
fn is_embedded_image(url: &str) -> bool {
    const TYPES: [&str; 4] = ["png", "gif", "jpeg", "webp"];

    if !url
        .get(..11)
        .is_some_and(|prefix| prefix.eq_ignore_ascii_case("data:image/"))
    {
        return false;
    }
    let rest = &url[11..];

    TYPES.iter().any(|media| {
        rest.get(..media.len())
            .is_some_and(|found| found.eq_ignore_ascii_case(media))
            && matches!(rest.as_bytes().get(media.len()), Some(b';' | b','))
    })
}

// comrak has no plugin hook for math the way it has for code fences, and it
// renders a math node as `<span data-math-style>` with the LaTeX left raw.
// Overriding the node in the formatter is what gives access to the literal
// before comrak escapes it, so the conversion never has to unescape HTML to
// find out what the author wrote.
create_formatter!(MathFormatter, {
    NodeValue::Math(ref nm) => |context, entering| {
        if entering {
            write_math(context, nm.literal.as_str(), nm.display_math)?;
        }
        return Ok(ChildRendering::Skip);
    },
});

/// Writes one math span, falling back to the escaped LaTeX whenever the
/// conversion in `math` refuses its own output. A document that cannot be
/// rendered shows its source, never a half-trusted render.
fn write_math(output: &mut dyn fmt::Write, latex: &str, display: bool) -> fmt::Result {
    match math::to_mathml(latex, display) {
        Some(mathml) if display => {
            // A <div> here would end the enclosing <p>, because comrak puts
            // display math inside one, so the scroll container is a span that
            // CSS makes a block. Same treatment `pre.mermaid` gets.
            write!(output, "<span class=\"mhr-math-display\">{mathml}</span>")
        }
        Some(mathml) => output.write_str(&mathml),
        None => write!(
            output,
            "<code class=\"mhr-math-raw\">{}</code>",
            escape_html(latex)
        ),
    }
}

/// comrak's `header_id_prefix_in_href` only rewrites the small anchor icon it
/// inserts next to each heading; a link written anywhere else in the
/// document, which is how every hand-authored table of contents and every
/// GitHub-rendered one works, still targets the bare, unprefixed slug and
/// never matches. GitHub's own rendering resolves those links too, so this
/// replicates that here rather than in `assets/app.js`: parsing stays in
/// Rust, and JavaScript never touches document text. Only links whose fragment
/// matches a real heading are touched, so footnote references and other
/// hash links, none of which comrak prefixes, are left alone.
fn rewrite_local_anchor_links<'a>(root: &'a AstNode<'a>) {
    let mut anchorizer = Anchorizer::new();
    let heading_ids: HashSet<String> = root
        .descendants()
        .filter(|node| matches!(node.data.borrow().value, NodeValue::Heading(_)))
        .map(|node| anchorizer.anchorize(&node.collect_text()))
        .collect();

    for node in root.descendants() {
        if let NodeValue::Link(ref mut link) = node.data.borrow_mut().value
            && let Some(fragment) = link.url.strip_prefix('#')
            && heading_ids.contains(fragment)
        {
            link.url = format!("#{HEADER_ID_PREFIX}{fragment}");
        }
    }
}

pub fn to_html(markdown: &str) -> String {
    let mut plugins = Plugins::default();
    plugins.render.codefence_syntax_highlighter = Some(syntect());
    plugins
        .render
        .codefence_renderers
        .insert(String::from("mermaid"), &Mermaid);

    let options = options();
    let arena = Arena::new();
    let root = parse_document(&arena, markdown, &options);
    rewrite_local_anchor_links(root);

    let mut html = String::new();
    match MathFormatter::format_document_with_plugins(root, &options, &mut html, &plugins) {
        Ok(()) => sanitize(&html),
        // Writing into a String cannot fail, so this arm exists only because
        // the formatter is generic over Write.
        Err(_) => String::from("<p data-mhr-notice>Render failed.</p>"),
    }
}

#[cfg(test)]
mod tests {
    use super::{escape_html, to_html};
    use proptest::prelude::*;

    /// Markdown built from fragments that mean something to the parser, rather
    /// than arbitrary Unicode.
    ///
    /// This distinction decides whether the property below tests anything at
    /// all. A generator over `\PC*` never produces the substring `<script`, so
    /// it passes 20,000 cases against a build with raw HTML rendering turned
    /// on: the assertion holds only because the input never asks the question.
    /// Drawing from a vocabulary of delimiters, tag starts and event-handler
    /// attributes puts the dangerous shapes in front of comrak in every case,
    /// and shrinks to a readable counterexample when one fails.
    fn markdown_ish() -> impl Strategy<Value = String> {
        let token = prop::sample::select(vec![
            "<script",
            ">",
            "</script>",
            "<img src=x onerror=alert(1)>",
            "<",
            "javascript:",
            "`",
            "```",
            "~~~",
            "$",
            "$$",
            "\\operatorname{",
            "}",
            "|",
            "---",
            "\n",
            "\n\n",
            "> ",
            "- [ ] ",
            "[a](",
            ")",
            "#",
            "&",
            "\"",
            "'",
            "\\",
            "mermaid",
            "html",
            "text",
            "![a](",
            "<iframe",
            "<object",
            "data:text/html,",
            "vbscript:",
        ]);
        prop::collection::vec(token, 0..24).prop_map(|parts| parts.concat())
    }

    proptest! {
        /// Escaping has to be reversible in exactly one pass, which is a
        /// stronger claim than "no raw angle bracket survives" and the reason
        /// this is a round trip rather than a substring check.
        ///
        /// Both claims reject a missing `replace`. Only this one rejects the
        /// ordering bug: escape `<` before `&` and every `<` comes out as
        /// `&amp;lt;`, so the reader sees the literal text `&lt;` on the page.
        /// That mutation passes the substring form, since no raw `<` survives
        /// it either.
        #[test]
        fn escape_html_round_trips_in_one_pass(s in markdown_ish()) {
            let escaped = escape_html(&s);
            prop_assert!(!escaped.contains('<'), "{escaped}");
            prop_assert!(!escaped.contains('>'), "{escaped}");
            prop_assert_eq!(unescape_once(&escaped), s);
        }

        /// `render.r#unsafe` is true, so a dangerous tag does reach the output
        /// of comrak; `sanitize` is then the pass that has to remove it. This
        /// is the general form of `neutralizes_dangerous_raw_html` below, and
        /// it is worth having only because `markdown_ish` actually feeds comrak
        /// the payloads: it fails without `sanitize`, which the
        /// arbitrary-Unicode version it replaced did not.
        ///
        /// The assertion is on tag openings only, and that narrowness is
        /// deliberate. A `<` reaches the output only from a tag this renderer
        /// emitted, because every other path escapes it, so `<script` in the
        /// output is proof of a real tag. Nothing else about the output is
        /// safe to match by substring: body text carries unescaped quotes and
        /// equals signs, so a document is free to render the literal text
        /// `href="javascript:` inside a `<code>` block, where it is inert.
        /// Two earlier versions of this test asserted on those shapes and
        /// failed on innocent documents. URL filtering is checked instead by
        /// `strips_link_and_image_urls_in_scripting_schemes` below, which can
        /// look at a known attribute because it controls the input.
        #[test]
        fn to_html_never_emits_a_tag_it_does_not_generate(s in markdown_ish()) {
            let html = to_html(&s).to_lowercase();
            for tag in ["<script", "<iframe", "<object", "<embed", "<style", "<form", "<base"] {
                prop_assert!(!html.contains(tag), "{tag} reached the page: {html}");
            }
        }
    }

    /// Decodes the three entities [`escape_html`] writes, scanning left to
    /// right so each one is decoded exactly once. Decoding by three successive
    /// `replace` calls would undo double-escaping instead of revealing it.
    fn unescape_once(s: &str) -> String {
        let mut out = String::with_capacity(s.len());
        let mut rest = s;
        while let Some(i) = rest.find('&') {
            out.push_str(&rest[..i]);
            rest = &rest[i..];
            let (entity, decoded) = if rest.starts_with("&amp;") {
                ("&amp;", '&')
            } else if rest.starts_with("&lt;") {
                ("&lt;", '<')
            } else if rest.starts_with("&gt;") {
                ("&gt;", '>')
            } else {
                out.push('&');
                rest = &rest[1..];
                continue;
            };
            out.push(decoded);
            rest = &rest[entity.len()..];
        }
        out.push_str(rest);
        out
    }

    #[test]
    fn renders_gfm_tables() {
        let html = to_html("| a | b |\n| --- | --- |\n| 1 | 2 |");
        assert!(html.contains("<table>"), "{html}");
    }

    #[test]
    fn renders_gfm_alerts() {
        let html = to_html("> [!WARNING]\n> careful");
        assert!(html.contains("markdown-alert-warning"), "{html}");
    }

    #[test]
    fn renders_task_lists() {
        let html = to_html("- [x] done\n- [ ] todo");
        assert!(html.contains("type=\"checkbox\""), "{html}");
    }

    #[test]
    fn renders_strikethrough_and_autolinks() {
        let html = to_html("~~gone~~ and https://example.com");
        assert!(html.contains("<del>"), "{html}");
        assert!(html.contains("href=\"https://example.com\""), "{html}");
    }

    #[test]
    fn renders_footnotes() {
        let html = to_html("text[^1]\n\n[^1]: note");
        assert!(html.contains("footnotes"), "{html}");
    }

    #[test]
    fn highlights_fenced_code() {
        let html = to_html("```rust\nfn main() {}\n```");
        assert!(html.contains("<span"), "syntect produced no spans: {html}");
    }

    /// Highlighting has to arrive as classes. An inline `style` would carry one
    /// theme's colors, and the `background-color` syntect puts on the `<pre>`
    /// in theme mode cannot be overridden from a stylesheet, so a dark page
    /// would show a white code block.
    #[test]
    fn highlights_with_classes_rather_than_inline_styles() {
        let html = to_html("```rust\nfn main() {}\n```");
        assert!(
            !html.contains("style="),
            "inline style reached the page: {html}"
        );
        assert!(html.contains("class=\"hl-"), "no prefixed classes: {html}");
    }

    /// Math is converted in Rust, so what reaches the webview is `MathML` and
    /// never the LaTeX source with a marker attribute on it.
    #[test]
    fn renders_math_as_mathml() {
        let inline = to_html("$E = mc^2$");
        assert!(inline.contains("<math display=\"inline\">"), "{inline}");
        assert!(!inline.contains("data-math-style"), "{inline}");

        let display = to_html("$$E = mc^2$$");
        assert!(display.contains("<math display=\"block\">"), "{display}");
        assert!(display.contains("mhr-math-display"), "{display}");
    }

    /// GitHub's other inline math syntax goes through the same path, so it must
    /// not come out as a plain code span.
    #[test]
    fn renders_code_span_math_as_mathml() {
        let html = to_html("$`E = mc^2`$");
        assert!(html.contains("<math display=\"inline\">"), "{html}");
    }

    /// LaTeX that does not parse shows its own source instead of a render
    /// nobody validated, and that source is escaped on the way out.
    #[test]
    fn falls_back_to_escaped_source_when_latex_does_not_convert() {
        let html = to_html("$\\operatorname{</math><script>alert(1)</script>}$");
        assert!(html.contains("mhr-math-raw"), "{html}");
        assert!(!html.contains("<script"), "{html}");
        assert!(!html.contains("<math"), "{html}");
    }

    #[test]
    fn passes_mermaid_through_unhighlighted() {
        let html = to_html("```mermaid\ngraph LR\n  A --> B\n```");
        assert!(html.contains("<pre class=\"mermaid\">"), "{html}");
        assert!(html.contains("graph LR"), "{html}");
        assert!(
            !html.contains("<code"),
            "mermaid fence was wrapped in code: {html}"
        );
        assert!(
            !html.contains("</code>"),
            "unbalanced closing code tag: {html}"
        );
    }

    /// The whole safety model of a viewer for agent-written files rests on this.
    /// `render.r#unsafe` is true, so a dangerous tag reaches `sanitize`, which
    /// must remove it: `<script>` is escaped to inert text by the tag filter,
    /// and an inline event handler is dropped from an otherwise allowed tag.
    #[test]
    fn neutralizes_dangerous_raw_html() {
        let html = to_html("<script>alert(1)</script>\n\n<img src=x onerror=alert(1)>");
        assert!(!html.contains("<script"), "{html}");
        assert!(!html.contains("onerror"), "{html}");

        for markdown in [
            "<iframe src=x></iframe>",
            "<object data=x></object>",
            "<style>*{}</style>",
        ] {
            let html = to_html(markdown);
            assert!(!html.contains("<iframe"), "{markdown}: {html}");
            assert!(!html.contains("<object"), "{markdown}: {html}");
            assert!(!html.contains("<style"), "{markdown}: {html}");
        }
    }

    /// The GitHub-safe subset of raw HTML renders instead of being dropped,
    /// which is what makes this a GitHub-flavored viewer rather than a
    /// plain-CommonMark one. Block and inline both pass through.
    #[test]
    fn renders_the_github_safe_html_subset() {
        let block = to_html("<details><summary>more</summary>hidden</details>");
        assert!(block.contains("<details>"), "{block}");
        assert!(block.contains("<summary>more</summary>"), "{block}");

        let table = to_html("<table><tr><td>cell</td></tr></table>");
        assert!(
            table.contains("<table>") && table.contains("<td>cell</td>"),
            "{table}"
        );

        let inline = to_html("press <kbd>Esc</kbd> now");
        assert!(inline.contains("press <kbd>Esc</kbd> now"), "{inline}");
    }

    /// An inline `style` attribute never reaches the page. It is the one
    /// document-controlled channel that a class allowlist does not close, and
    /// `highlight.css` relies on nothing carrying its own colors.
    #[test]
    fn drops_style_attributes_from_raw_html() {
        let html = to_html(r#"<p style="color:red">text</p>"#);
        assert!(!html.contains("style="), "{html}");
    }

    /// A URL in a scripting scheme is dropped rather than carried into the
    /// attribute. `render.r#unsafe` is true, so comrak no longer blanks these
    /// itself; `sanitize` keeps only `http`, `https`, `mailto` and an embedded
    /// image, and removes the whole attribute when the scheme is anything else.
    ///
    /// Each needle is matched against a lowercased render. Comparing against
    /// the raw output would make the mixed-case case vacuous: a surviving
    /// `JaVaScRiPt:` shares no substring with a lowercase needle, so the
    /// assertion would pass whether or not the scheme check is case-blind,
    /// which is the one thing that case exists to prove.
    #[test]
    fn strips_link_and_image_urls_in_scripting_schemes() {
        for (markdown, scheme) in [
            ("[a](javascript:alert(1))", "javascript:"),
            ("![a](javascript:alert(1))", "javascript:"),
            ("[a](JaVaScRiPt:alert(1))", "javascript:"),
            ("[a](vbscript:alert(1))", "vbscript:"),
            ("[a](data:text/html,<script>alert(1)</script>)", "data:"),
            ("![a](DATA:TEXT/HTML,x)", "data:"),
        ] {
            let html = to_html(markdown).to_ascii_lowercase();
            assert!(!html.contains(scheme), "{markdown} kept its URL: {html}");
        }
    }

    /// An embedded raster image is the only image an offline viewer can show,
    /// so `data:` cannot be banned outright the way the schemes above are.
    /// `index.html` allows `data:` in `img-src` for exactly these.
    #[test]
    fn keeps_images_embedded_as_data_uris() {
        for markdown in [
            "![a](data:image/png;base64,iVBORw0KGgo=)",
            "![a](data:image/gif;base64,R0lGOD==)",
            "![a](data:image/jpeg;base64,/9j/4AAQ)",
            "![a](data:image/webp;base64,UklGRg==)",
            "![a](DATA:IMAGE/PNG;base64,iVBORw0KGgo=)",
        ] {
            let html = to_html(markdown).to_ascii_lowercase();
            assert!(
                html.contains("src=\"data:image/"),
                "{markdown} lost its src: {html}"
            );
        }
    }

    /// The media type has to end where a real data URL ends. Without this,
    /// `data:image/png` is a prefix of anything, and `data:image/pngx,...`
    /// would ride in on a check that only looked at the front of the string.
    #[test]
    fn rejects_a_data_uri_that_only_looks_like_an_image() {
        for markdown in [
            "![a](data:image/pngx,<script>alert(1)</script>)",
            "![a](data:image/png-html,x)",
            "![a](data:image/svg+xml;base64,PHN2Zz4=)",
            "![a](data:image/,x)",
        ] {
            let html = to_html(markdown);
            assert!(!html.contains("data:"), "{markdown} kept its URL: {html}");
        }
    }

    /// The viewer is read-only, so no document may put an interactive control
    /// on the page. comrak's task-list checkbox is the only `<input>` that
    /// belongs there, and it arrives already disabled; a raw one does not.
    #[test]
    fn never_renders_an_interactive_input() {
        let text = to_html("<form action=x><input type=text></form>");
        assert!(!text.contains("type=\"text\""), "{text}");

        let checkbox = to_html("<input type=checkbox>");
        assert!(checkbox.contains("disabled"), "{checkbox}");

        let generated = to_html("- [x] done\n- [ ] todo");
        assert!(generated.contains("<input"), "{generated}");
        assert!(generated.contains("disabled"), "{generated}");
        assert!(generated.contains("checked"), "{generated}");
    }

    /// The app's own notices are marked with `data-mhr-notice`, which is not in
    /// the allowlist, so a document cannot dress its own text up as one. This
    /// is why the notices in `main.rs` and in `to_html` stopped using a class:
    /// `class` is allowed on everything, and forging one was free.
    #[test]
    fn a_document_cannot_forge_an_app_notice() {
        let html = to_html("<p data-mhr-notice>Cannot read /etc/passwd: denied</p>");
        assert!(!html.contains("data-mhr-notice"), "{html}");
    }

    /// github.com honours `open` on a disclosure block, so a document that
    /// ships one expanded has to render expanded rather than silently closed.
    #[test]
    fn keeps_an_open_disclosure_block_open() {
        let html = to_html("<details open><summary>more</summary>shown</details>");
        assert!(html.contains("<details open"), "{html}");
    }

    /// A mermaid fence bypasses syntect, so it must still be escaped on the way out.
    #[test]
    fn escapes_markup_inside_mermaid_fences() {
        let html = to_html("```mermaid\n<script>alert(1)</script>\n```");
        assert!(!html.contains("<script"), "{html}");
    }

    #[test]
    fn renders_description_lists() {
        let html = to_html("Term\n\n: Definition");
        assert!(
            html.contains("<dl>") && html.contains("<dt>") && html.contains("<dd>"),
            "{html}"
        );
    }

    #[test]
    fn renders_superscript() {
        let html = to_html("x^2^");
        assert!(html.contains("<sup>2</sup>"), "{html}");
    }

    #[test]
    fn renders_multiline_block_quotes() {
        let html = to_html(">>>\nquoted\n>>>");
        assert!(html.contains("<blockquote>"), "{html}");
    }

    /// Front matter is metadata, not content, so it must disappear from the
    /// render rather than showing up as a stray paragraph or literal `---`.
    #[test]
    fn strips_front_matter_from_output() {
        let html = to_html("---\ntitle: hi\n---\n\nbody");
        assert!(!html.contains("title: hi"), "{html}");
        assert!(html.contains("<p>body</p>"), "{html}");
    }

    /// A document that opens with a horizontal rule, rather than real front
    /// matter, must not have its rule swallowed by the front-matter scanner.
    #[test]
    fn does_not_mistake_a_leading_rule_for_front_matter() {
        let html = to_html("---\n\nbelow the rule");
        assert!(html.contains("<hr"), "{html}");
    }

    /// GitHub prefixes both the heading id and its self-link href with
    /// `user-content-`; a mismatch leaves the link-to-heading icon pointing at
    /// a fragment nothing on the page has.
    #[test]
    fn heading_anchor_href_matches_its_prefixed_id() {
        let html = to_html("# Hello World");
        assert!(html.contains(r#"id="user-content-hello-world""#), "{html}");
        assert!(
            html.contains(r##"href="#user-content-hello-world""##),
            "{html}"
        );
    }

    /// A hand-written table of contents, the common case, must resolve to the
    /// same prefixed id the heading actually got, not just the heading's own
    /// generated anchor icon.
    #[test]
    fn rewrites_local_links_to_match_prefixed_heading_ids() {
        let html = to_html("- [Hello](#hello)\n\n# Hello");
        assert!(html.contains(r##"href="#user-content-hello""##), "{html}");
    }

    /// Footnote references are never prefixed by comrak, so a link that
    /// happens to share their shape must not be rewritten into a dead one.
    #[test]
    fn leaves_hash_links_alone_when_no_heading_matches() {
        let html = to_html("[note](#fn1)\n\n# Something Else");
        assert!(html.contains(r##"href="#fn1""##), "{html}");
    }

    /// The fixture's own header claims it exercises every supported GFM
    /// feature; this keeps that claim honest as extensions are added.
    #[test]
    fn kitchen_sink_fixture_hits_every_advertised_feature() {
        let markdown = std::fs::read_to_string("fixtures/kitchen-sink.md")
            .expect("fixtures/kitchen-sink.md ships with the repo");
        let html = to_html(&markdown);

        let expected = [
            "<dl>",
            "<dt>",
            "<dd>",
            "<sup>2</sup>",
            "<blockquote>",
            "<table>",
            "markdown-alert",
            "type=\"checkbox\"",
            "<del>",
            "footnotes",
            "<span",
            "<math display=\"inline\">",
            "<math display=\"block\">",
            // `align` is the one environment with equation numbers, and the
            // rule that positions them lives in `github.css`, not in the
            // vendored `latex.css`.
            "menv-with-eqn",
            "mhr-math-raw",
            "<pre class=\"mermaid\">",
            // Raw HTML: the safe subset renders, block and inline. The
            // `align` attribute on the raw table's last column survives the
            // sanitizer; no pipe table in the fixture sets one.
            "<details>",
            "<details open",
            "<kbd>Ctrl</kbd>",
            r#"align="right""#,
            // The one image an offline viewer can show.
            r#"src="data:image/png;base64"#,
            // `mailto:` has to survive the sanitizer, because `link::open`
            // hands one to the desktop and can only do that if it reaches the
            // page in the first place.
            r#"href="mailto:"#,
        ];
        for needle in expected {
            assert!(html.contains(needle), "fixture missing {needle:?}\n{html}");
        }

        // The same section carries a <script> tag and an inline event handler.
        // The tag is escaped to inert text, the handler is dropped entirely.
        assert!(
            !html.contains("<script"),
            "live script tag reached the page\n{html}"
        );
        assert!(
            !html.contains("onerror"),
            "event handler reached the page\n{html}"
        );

        // The fixture asks for a text box and an SVG data URI on purpose.
        // Neither may reach the page: one is an editing surface, the other
        // runs script in a webview.
        assert!(
            !html.contains("type=\"text\""),
            "raw input stayed editable\n{html}"
        );
        // Matched as an attribute, not as a substring: the same section
        // explains the rule in prose, so `data:image/svg+xml` legitimately
        // appears on the page inside a <code> span.
        assert!(
            !html.contains("src=\"data:image/svg"),
            "scripting data URI reached the page\n{html}"
        );
    }
}
