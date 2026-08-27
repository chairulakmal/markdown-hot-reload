//! LaTeX math turned into `MathML` in Rust, with the converter's output checked
//! before any of it reaches the webview.
//!
//! `pulldown-latex` does not escape everything it copies out of the document.
//! `\operatorname{...}` passes its argument through untouched, and a parse error
//! quotes the offending source back inside `<merror>`. Either one lets a
//! document put live markup on the page, so nothing the converter returns is
//! trusted until [`is_trusted`] has checked it against an allowlist of `MathML`
//! elements and attributes. Anything unrecognized discards the whole conversion,
//! and the caller shows the escaped LaTeX instead.
//!
//! The content security policy in `index.html` would also stop these payloads,
//! because `script-src` has no `unsafe-inline`. That is a second line, not this
//! one. The project's promise is that escaping happens in Rust.

use pulldown_latex::config::{DisplayMode, RenderConfig};
use pulldown_latex::{Parser, Storage, push_mathml};

/// Elements the validator accepts, from the `MathML` Core element set.
///
/// Two omissions are deliberate rather than oversights.
///
/// `annotation` and `annotation-xml` are left out because
/// `<annotation-xml encoding="text/html">` is an HTML integration point: the
/// parser reads its children as HTML instead of `MathML`, which is the one place
/// inside a `<math>` subtree where arbitrary markup would be honored. Nothing
/// needs them here, since [`config`] never sets `RenderConfig::annotation`.
///
/// `merror` is left out so that a LaTeX parse error fails the check and falls
/// back like any other untrusted output. That closes the vector where the
/// converter echoes the failing source back verbatim, and it shows the reader
/// their own LaTeX rather than an ASCII-art error box drawn by a crate they have
/// never heard of.
const ELEMENTS: &[&str] = &[
    "math",
    "mfrac",
    "mi",
    "mmultiscripts",
    "mn",
    "mo",
    "mover",
    "mpadded",
    "mphantom",
    "mprescripts",
    "mroot",
    "mrow",
    "ms",
    "mspace",
    "msqrt",
    "mstyle",
    "msub",
    "msubsup",
    "msup",
    "mtable",
    "mtd",
    "mtext",
    "mtr",
    "munder",
    "munderover",
    "semantics",
];

/// Attributes the validator accepts: everything `pulldown-latex` is capable of
/// writing, minus `encoding`, which only ever appears on the annotation elements
/// that [`ELEMENTS`] refuses.
///
/// `style` is here because the converter uses it for real layout work, spacing
/// and color, not only for error borders. Its value is confined by the rules in
/// [`is_trusted`]: it must be double-quoted and can contain no `<` or `>`, so it
/// cannot end the attribute or open a tag. CSS in a style attribute cannot run
/// script in any browser this targets, and a URL inside one would still have to
/// get past `default-src 'self'` and `connect-src 'none'`.
const ATTRIBUTES: &[&str] = &[
    "class",
    "depth",
    "display",
    "displaystyle",
    "height",
    "largeop",
    "linethickness",
    "mathvariant",
    "maxsize",
    "minsize",
    "movablelimits",
    "scriptlevel",
    "stretchy",
    "style",
    "symmetric",
    "width",
    "xmlns",
];

/// Converts one LaTeX span to `MathML`, or returns `None` when the result cannot
/// be trusted, which includes every LaTeX that failed to parse.
pub fn to_mathml(latex: &str, display: bool) -> Option<String> {
    let storage = Storage::new();
    let parser = Parser::new(latex, &storage);

    let mut mathml = String::new();
    push_mathml(&mut mathml, parser, config(display)).ok()?;

    is_trusted(&mathml).then_some(mathml)
}

fn config(display: bool) -> RenderConfig<'static> {
    RenderConfig {
        display_mode: if display {
            DisplayMode::Block
        } else {
            DisplayMode::Inline
        },
        // Annotations would emit the LaTeX source into the tree, and the
        // elements that carry it are refused by ELEMENTS.
        annotation: None,
        ..RenderConfig::default()
    }
}

/// Whether every tag in `html` is an allowlisted element carrying only
/// allowlisted attributes, and every tag is closed in order.
///
/// The scan follows the HTML tokenizer on where markup begins, because the
/// browser will: `<` opens a tag only when a letter, `/`, `!` or `?` follows it,
/// and is literal text otherwise. That last case is not a corner case here. A
/// perfectly ordinary `$a < b$` converts to `<mo><</mo>`, so a validator that
/// treated every `<` as a tag would reject one of the most common expressions
/// anyone writes.
///
/// Everything it cannot read confidently is rejected, since the cost of a false
/// rejection is showing the reader their LaTeX unrendered.
fn is_trusted(html: &str) -> bool {
    let bytes = html.as_bytes();
    let mut open: Vec<&str> = Vec::new();
    let mut i = 0;

    while i < bytes.len() {
        if bytes[i] != b'<' {
            i += 1;
            continue;
        }

        match bytes.get(i + 1) {
            // A trailing `<`, a comment, or a bogus comment. None of these are
            // things the converter emits, so none of them are worth parsing.
            None | Some(b'!' | b'?') => return false,

            Some(b'/') => {
                let Some((name, rest)) = tag_name(&html[i + 2..]) else {
                    return false;
                };
                // End tags may legally carry attributes, which the parser then
                // discards. Refusing them outright keeps this scan honest about
                // what it has actually inspected.
                if !rest.starts_with('>') || open.pop() != Some(name) {
                    return false;
                }
                i = html.len() - rest.len() + 1;
            }

            Some(c) if c.is_ascii_alphabetic() => {
                let Some((name, rest)) = tag_name(&html[i + 1..]) else {
                    return false;
                };
                if !ELEMENTS.contains(&name) {
                    return false;
                }
                let Some((self_closing, rest)) = attributes(rest) else {
                    return false;
                };
                if !self_closing {
                    open.push(name);
                }
                i = html.len() - rest.len();
            }

            // Literal text, such as the `<` in `<mo><</mo>`.
            _ => i += 1,
        }
    }

    open.is_empty()
}

/// Splits a leading tag name off `s`, returning it with the rest of the input.
fn tag_name(s: &str) -> Option<(&str, &str)> {
    let end = s
        .find(|c: char| !c.is_ascii_alphanumeric() && c != '-')
        .unwrap_or(s.len());
    (end > 0).then(|| s.split_at(end))
}

/// Consumes a start tag's attributes up to and including its `>`, returning
/// whether the tag closed itself and the rest of the input.
///
/// `MathML` is foreign content, where a browser honors `/>`, so `<mspace />`
/// opens nothing and must not be pushed onto the stack of open tags.
fn attributes(mut s: &str) -> Option<(bool, &str)> {
    loop {
        s = s.trim_start_matches([' ', '\t', '\n', '\r']);

        if let Some(rest) = s.strip_prefix("/>") {
            return Some((true, rest));
        }
        if let Some(rest) = s.strip_prefix('>') {
            return Some((false, rest));
        }

        let (name, rest) = tag_name(s)?;
        if !ATTRIBUTES.contains(&name) {
            return None;
        }

        // Bare attributes and single quotes are both legal HTML and neither is
        // something the converter writes, so requiring `="..."` means the value
        // below is delimited by exactly one character and nothing else.
        let rest = rest.trim_start_matches([' ', '\t', '\n', '\r']);
        let rest = rest.strip_prefix("=\"")?;
        let (value, rest) = rest.split_once('"')?;
        if value.contains(['<', '>']) {
            return None;
        }
        s = rest;
    }
}

#[cfg(test)]
mod tests {
    use super::{is_trusted, to_mathml};
    use proptest::prelude::*;

    proptest! {
        /// `is_trusted` is a hand-rolled scanner that slices `html` on byte
        /// offsets it computes itself; the thing worth fuzzing is that no
        /// input, valid `MathML` or not, ever panics it, since a panic here
        /// would take the whole render down instead of falling back to the
        /// escaped LaTeX the way a rejection does.
        #[test]
        fn is_trusted_never_panics(s in "\\PC*") {
            let _ = is_trusted(&s);
        }

        /// `script` is not in `ELEMENTS`, so wrapping arbitrary text around a
        /// `<script>` tag must be refused regardless of what surrounds it.
        /// This is the general form `rejects_unknown_elements_and_attributes`
        /// below checks with one fixed payload.
        #[test]
        fn never_trusts_a_wrapped_script_tag(before in "\\PC*", after in "\\PC*") {
            let html = format!("<math>{before}<script>alert(1)</script>{after}</math>");
            prop_assert!(!is_trusted(&html), "{html}");
        }
    }

    #[test]
    fn converts_inline_math() {
        let mathml = to_mathml("E = mc^2", false).expect("valid LaTeX converts");
        assert!(mathml.starts_with("<math display=\"inline\""), "{mathml}");
        assert!(
            mathml.contains("<msup><mi>c</mi><mn>2</mn></msup>"),
            "{mathml}"
        );
    }

    #[test]
    fn converts_display_math() {
        let mathml = to_mathml(r"\frac{1}{2}", true).expect("valid LaTeX converts");
        assert!(mathml.starts_with("<math display=\"block\""), "{mathml}");
        assert!(mathml.contains("<mfrac>"), "{mathml}");
    }

    /// `pulldown-latex` emits the argument of `\operatorname` without escaping
    /// it, so a document can close the `<math>` element and open its own tag.
    /// Upstream may fix this; the fallback is what must not regress.
    #[test]
    fn refuses_markup_smuggled_through_operatorname() {
        let payload = r"\operatorname{</math><script>alert(1)</script>}";
        assert!(to_mathml(payload, false).is_none(), "{payload}");
    }

    /// A parse error puts the failing source back on the page verbatim inside
    /// `<merror>`, which is why `merror` is not an allowlisted element.
    #[test]
    fn refuses_markup_echoed_back_by_a_parse_error() {
        let payload = r"\begin{x}</math><img src=q onerror=alert(1)>";
        assert!(to_mathml(payload, false).is_none(), "{payload}");
    }

    /// Every failed parse takes the fallback path, so the reader sees their own
    /// LaTeX rather than the converter's error box.
    #[test]
    fn refuses_any_latex_that_failed_to_parse() {
        assert!(to_mathml(r"\thiscommanddoesnotexist", false).is_none());
        assert!(to_mathml(r"\frac{1", false).is_none());
    }

    /// The converter writes a bare `<` for a less-than operator, so a validator
    /// that read every `<` as a tag would reject ordinary arithmetic.
    #[test]
    fn accepts_a_bare_less_than_in_text_content() {
        let mathml = to_mathml("a < b", false).expect("`a < b` is ordinary LaTeX");
        assert!(mathml.contains("<mo><</mo>"), "{mathml}");
    }

    #[test]
    fn accepts_self_closing_tags_without_unbalancing_the_stack() {
        assert!(is_trusted(r#"<math><mspace width="1em" /></math>"#));
    }

    #[test]
    fn rejects_unknown_elements_and_attributes() {
        assert!(!is_trusted("<math><script>alert(1)</script></math>"));
        assert!(!is_trusted(r#"<math><mi onclick="alert(1)">x</mi></math>"#));
        assert!(!is_trusted(
            "<math><annotation-xml encoding=\"text/html\"></annotation-xml></math>"
        ));
    }

    /// An unquoted value can contain a `>` that ends the tag early, which would
    /// leave the scan reading markup as text.
    #[test]
    fn rejects_attribute_values_that_are_not_double_quoted() {
        assert!(!is_trusted("<math><mi class=a>x</mi></math>"));
        assert!(!is_trusted("<math><mi class='a'>x</mi></math>"));
        assert!(!is_trusted("<math><mi class>x</mi></math>"));
    }

    #[test]
    fn rejects_tags_that_are_not_closed_in_order() {
        assert!(!is_trusted("<math><mi>x</math></mi>"));
        assert!(!is_trusted("<math><mi>x</mi>"));
        assert!(!is_trusted("<mi>x</mi></math>"));
    }

    #[test]
    fn rejects_comments_and_truncated_tags() {
        assert!(!is_trusted("<math><!-- hi --></math>"));
        assert!(!is_trusted("<math><mi>x</mi></math><"));
        assert!(!is_trusted("<math><mi"));
    }
}
