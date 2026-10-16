//! Rustdoc cleanup for the generated file.
//!
//! The schema's descriptions are written for the TypeScript source and the spec
//! site, and typify copies them into doc comments verbatim. Two constructs do
//! not survive the trip:
//!
//! - TSDoc `{@link Target | label}`. Upstream's `schema.json` generator also
//!   drops the ` | `, so `2026-07-28` reads `{@link CallToolRequesttools/call}`.
//!   The target is recovered as the longest definition name the text starts
//!   with, and the link becomes an intra-doc link when that definition was
//!   generated, so rustdoc checks it.
//! - Site-relative markdown links (`](/specification/…)`), which resolve on
//!   modelcontextprotocol.io and nowhere else.
//! - Bare URLs, which markdown renders as text; rustdoc wants `<…>`.

use crate::names::{generated_types, rust_name};

const SITE: &str = "https://modelcontextprotocol.io";

/// Rewrite both constructs in `src`, the generated file. `defs` are the schema
/// definition names typify was given.
pub fn scrub(src: &str, defs: &[String]) -> String {
    let types = generated_types(src);
    autolinks(&absolute_links(&tsdoc_links(&line_docs(src), defs, &types)))
}

/// Turn prettyplease's `/** … */` doc blocks into `///` lines.
///
/// prettyplease writes a multi-line doc as a block comment whose continuation
/// lines carry the item's indentation, so on a field every paragraph after the
/// first started with four spaces and rustdoc rendered it as a code block.
fn line_docs(src: &str) -> String {
    let mut out = String::with_capacity(src.len());
    let mut lines = src.lines();
    while let Some(line) = lines.next() {
        let indent = &line[..line.len() - line.trim_start().len()];
        let Some(first) = line.trim_start().strip_prefix("/**") else {
            out.push_str(line);
            out.push('\n');
            continue;
        };
        let mut text = first;
        loop {
            let (body, last) = match text.strip_suffix("*/") {
                Some(body) => (body, true),
                None => (text, false),
            };
            let body = body.trim_end();
            out.push_str(indent);
            out.push_str("///");
            if !body.is_empty() {
                out.push(' ');
                out.push_str(body);
            }
            out.push('\n');
            if last {
                break;
            }
            let Some(next) = lines.next() else { break };
            text = next.strip_prefix(indent).unwrap_or(next);
        }
    }
    out
}

fn tsdoc_links(src: &str, defs: &[String], types: &[&str]) -> String {
    const OPEN: &str = "{@link ";
    let mut out = String::with_capacity(src.len());
    let mut rest = src;
    while let Some(start) = rest.find(OPEN) {
        let Some(len) = rest[start..].find('}') else {
            break;
        };
        out.push_str(&rest[..start]);
        out.push_str(&render(&rest[start + OPEN.len()..start + len], defs, types));
        rest = &rest[start + len + 1..];
    }
    out.push_str(rest);
    out
}

/// One `{@link …}` body as rustdoc markdown.
fn render(body: &str, defs: &[String], types: &[&str]) -> String {
    let body = body.trim();
    let (target, label) = match body.split_once('|') {
        Some((target, label)) => (target.trim(), Some(label.trim())),
        None if body.starts_with("http") => return body.to_owned(),
        None => split_glued(body, defs),
    };
    // `ClientCapabilities.sampling.tools` links to `ClientCapabilities`.
    let def = target.split('.').next().unwrap_or(target);
    let text = code(label.unwrap_or(target));
    match rust_name(def, types) {
        // rustdoc resolves a label that is already the path.
        Some(ty) if label.is_none() && target == ty => format!("[{text}]"),
        Some(ty) => format!("[{text}]({ty})"),
        None => text,
    }
}

/// Split `CallToolRequesttools/call` into its target and its label.
fn split_glued<'a>(body: &'a str, defs: &[String]) -> (&'a str, Option<&'a str>) {
    let Some(def) = defs
        .iter()
        .filter(|def| body.starts_with(def.as_str()))
        .max_by_key(|def| def.len())
    else {
        return (body, None);
    };
    let rest = &body[def.len()..];
    if rest.is_empty() || rest.starts_with('.') {
        (body, None)
    } else {
        (&body[..def.len()], Some(rest))
    }
}

/// Code-format a bare name or method; leave prose and pre-formatted text alone.
fn code(text: &str) -> String {
    if text.contains('`') || text.contains(char::is_whitespace) {
        text.to_owned()
    } else {
        format!("`{text}`")
    }
}

/// `](/specification/2025-06-18/basic/index#meta)` →
/// `](https://modelcontextprotocol.io/specification/2025-06-18/basic#meta)`.
/// The site serves a section's index page at the section path, not at
/// `/index`.
fn absolute_links(src: &str) -> String {
    const OPEN: &str = "](/";
    let mut out = String::with_capacity(src.len());
    let mut rest = src;
    while let Some(start) = rest.find(OPEN) {
        let Some(len) = rest[start..].find(')') else {
            break;
        };
        let path = &rest[start + 2..start + len];
        let (page, anchor) = path.split_at(path.find('#').unwrap_or(path.len()));
        let page = page.strip_suffix("/index").unwrap_or(page);
        out.push_str(&rest[..start]);
        out.push_str(&format!("]({SITE}{page}{anchor})"));
        rest = &rest[start + len + 1..];
    }
    out.push_str(rest);
    out
}

/// Wrap each bare URL in a `///` line in `<…>`.
fn autolinks(src: &str) -> String {
    let mut out = String::with_capacity(src.len());
    for line in src.lines() {
        if line.trim_start().starts_with("///") {
            let words: Vec<String> = line.split(' ').map(autolink).collect();
            out.push_str(&words.join(" "));
        } else {
            out.push_str(line);
        }
        out.push('\n');
    }
    out
}

fn autolink(word: &str) -> String {
    if !(word.starts_with("https://") || word.starts_with("http://")) {
        return word.to_owned();
    }
    let url = word.trim_end_matches(['.', ',', ';', ':', ')']);
    format!("<{url}>{}", &word[url.len()..])
}

#[cfg(test)]
mod tests {
    use super::*;

    fn defs() -> Vec<String> {
        [
            "CallToolRequest",
            "CallToolRequestParams",
            "ClientCapabilities",
            "MetaObject",
            "Tool",
            "ToolUseContent",
            "JSONRPCMessage",
            "ServerResult",
        ]
        .map(String::from)
        .to_vec()
    }

    const TYPES: &str = "pub struct CallToolRequest {\npub struct ClientCapabilities {\npub struct MetaObject(\npub enum Tool {\npub struct ToolUseContent {\npub enum JsonrpcMessage {\n";

    fn scrubbed(doc: &str) -> String {
        scrub(&format!("{TYPES}/// {doc}\n"), &defs())
            .lines()
            .last()
            .unwrap()
            .trim_start_matches("/// ")
            .to_owned()
    }

    #[test]
    fn glued_labels_are_split_off_the_longest_definition_name() {
        assert_eq!(
            scrubbed("see {@link CallToolRequesttools/call}."),
            "see [`tools/call`](CallToolRequest)."
        );
        assert_eq!(scrubbed("{@link ToolUseContent}"), "[`ToolUseContent`]");
        assert_eq!(
            scrubbed("{@link MetaObject`_meta` key naming rules}"),
            "[`_meta` key naming rules](MetaObject)"
        );
    }

    #[test]
    fn a_property_path_links_to_its_type() {
        assert_eq!(
            scrubbed("{@link ClientCapabilities.sampling.tools}"),
            "[`ClientCapabilities.sampling.tools`](ClientCapabilities)"
        );
    }

    #[test]
    fn links_use_the_generated_name_and_degrade_to_code_without_one() {
        assert_eq!(
            scrubbed("{@link JSONRPCMessage}"),
            "[`JSONRPCMessage`](JsonrpcMessage)"
        );
        // A definition the generator dropped is not a link rustdoc would reject.
        assert_eq!(scrubbed("{@link ServerResult}"), "`ServerResult`");
        assert_eq!(scrubbed("{@link Tool | the tool}"), "[the tool](Tool)");
    }

    #[test]
    fn block_docs_become_line_docs_at_the_item_indentation() {
        let src = "pub struct A {\n    /**First line.\n\n    Second paragraph,\n      indented list item.*/\n    pub a: u8,\n    /**One line.*/\n    pub b: u8,\n}\n";
        assert_eq!(
            line_docs(src),
            "pub struct A {\n    /// First line.\n    ///\n    /// Second paragraph,\n    ///   indented list item.\n    pub a: u8,\n    /// One line.\n    pub b: u8,\n}\n"
        );
    }

    #[test]
    fn bare_urls_become_autolinks() {
        assert_eq!(
            scrubbed("See https://example.com/a#b. Or <https://x.y> or [z](https://z.z)."),
            "See <https://example.com/a#b>. Or <https://x.y> or [z](https://z.z)."
        );
    }

    #[test]
    fn site_relative_links_become_absolute() {
        assert_eq!(
            scrubbed("[General fields](/specification/2025-06-18/basic/index#meta) and [x](/a/b)"),
            "[General fields](https://modelcontextprotocol.io/specification/2025-06-18/basic#meta) and [x](https://modelcontextprotocol.io/a/b)"
        );
    }
}
