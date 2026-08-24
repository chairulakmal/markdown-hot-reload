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
/// here would bake one theme's colours into the HTML, including a
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

    // render.unsafe_ stays false: documents are written by agents and editors,
    // so raw HTML is escaped rather than executed. Revisit with an ammonia
    // allowlist if <details> and aligned images become worth the risk.

    o
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
/// Rust, and JavaScript only morphs the DOM. Only links whose fragment
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
        Ok(()) => html,
        // Writing into a String cannot fail, so this arm exists only because
        // the formatter is generic over Write.
        Err(_) => String::from("<p class=\"mhr-notice\">Render failed.</p>"),
    }
}

#[cfg(test)]
mod tests {
    use super::to_html;

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
    /// theme's colours, and the `background-color` syntect puts on the `<pre>`
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
    #[test]
    fn escapes_raw_html_rather_than_executing_it() {
        let html = to_html("<script>alert(1)</script>\n\n<img src=x onerror=alert(1)>");
        assert!(!html.contains("<script"), "{html}");
        assert!(!html.contains("onerror"), "{html}");
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
        ];
        for needle in expected {
            assert!(html.contains(needle), "fixture missing {needle:?}\n{html}");
        }
    }
}
