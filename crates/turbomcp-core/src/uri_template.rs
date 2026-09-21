//! Matching a URI against an RFC 6570 URI template.
//!
//! `ResourceTemplate.uriTemplate` is specified as "A URI template (according to
//! RFC 6570)". Published RFC 6570 crates only *expand* templates — turning
//! `{table}` plus a value into a URI — and none of them match in reverse, which
//! is the direction a server needs to route `resources/read`. Hence this.
//!
//! # What is supported
//!
//! Simple string expansion (RFC 6570 §3.2.2) — a bare `{var}` — which is what
//! MCP resource templates use in practice.
//!
//! A variable matches a non-empty run of characters. An **interior** variable,
//! one with literal text after it, may not contain `/`: that restriction is
//! what keeps `db://{table}/rows/{id}.json` from claiming
//! `db://totally/unrelated/path.json`. A **trailing** variable takes the whole
//! remainder, `/` included — the spec's own headline example is
//! `file:///{path}` for "Access files in the project directory", and there is
//! no ambiguity about where a variable with nothing after it ends.
//!
//! Matching is leftmost-first with no backtracking. A template whose literal
//! text can also appear inside one of its own variables (`x://{a}bb` against
//! `x://abbb`) will not match; real URI templates do not have that shape.

use alloc::vec::Vec;

/// A template decomposed into the literal text around its variables.
///
/// Built at macro-expansion time by [`Self::parse`] so dispatch does no parsing
/// per request.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UriTemplate<'a> {
    /// Literal chunks, in order, with the variables between them removed.
    literals: Vec<&'a str>,
    /// Whether the template opens with a variable rather than literal text.
    leading_var: bool,
    /// Whether the template closes with a variable rather than literal text.
    trailing_var: bool,
}

impl<'a> UriTemplate<'a> {
    /// Split a template into its literal chunks.
    ///
    /// Anything between `{` and the next `}` is a variable; the text between
    /// expressions is literal. An unclosed `{` makes the remainder literal,
    /// which keeps a malformed template from matching everything.
    #[must_use]
    pub fn parse(template: &'a str) -> Self {
        let mut literals = Vec::new();
        let mut leading_var = false;
        let mut trailing_var = false;
        let mut rest = template;
        let mut first = true;

        while let Some(open) = rest.find('{') {
            let Some(close_offset) = rest[open..].find('}') else {
                // Unclosed brace: the rest is literal.
                break;
            };
            let literal = &rest[..open];
            if first && literal.is_empty() {
                leading_var = true;
            } else if !literal.is_empty() {
                literals.push(literal);
            }
            first = false;
            rest = &rest[open + close_offset + 1..];
            trailing_var = true;
        }

        if !rest.is_empty() {
            literals.push(rest);
            trailing_var = false;
        }

        Self {
            literals,
            leading_var,
            trailing_var,
        }
    }

    /// Whether the template has any variables at all.
    #[must_use]
    pub fn is_concrete(&self) -> bool {
        !self.leading_var && !self.trailing_var && self.literals.len() <= 1
    }

    /// Whether `uri` is an instance of this template.
    #[must_use]
    pub fn matches(&self, uri: &str) -> bool {
        // A template with no variables is just a string.
        if self.is_concrete() {
            return self.literals.first().copied().unwrap_or("") == uri;
        }

        let mut rest = uri;
        let mut literals = self.literals.iter();

        if self.leading_var {
            match literals.next() {
                Some(literal) => {
                    // The gap has to be non-empty, so the literal cannot sit at
                    // the very start.
                    let Some(index) = rest.find(literal).filter(|index| *index > 0) else {
                        return false;
                    };
                    if !is_valid_variable(&rest[..index]) {
                        return false;
                    }
                    rest = &rest[index + literal.len()..];
                }
                // The whole template is one variable, so it takes everything.
                None => return is_valid_trailing_variable(rest),
            }
        } else {
            let Some(literal) = literals.next() else {
                return false;
            };
            let Some(stripped) = rest.strip_prefix(literal) else {
                return false;
            };
            rest = stripped;
        }

        for literal in literals {
            let Some(index) = rest.find(literal).filter(|index| *index > 0) else {
                return false;
            };
            if !is_valid_variable(&rest[..index]) {
                return false;
            }
            rest = &rest[index + literal.len()..];
        }

        if self.trailing_var {
            is_valid_trailing_variable(rest)
        } else {
            rest.is_empty()
        }
    }
}

/// Whether a run of characters can stand in for an interior variable.
///
/// Non-empty and confined to one path segment. Letting an interior variable
/// span `/` is exactly what made `db://{table}/rows/{id}.json` claim
/// `db://totally/unrelated/path.json`.
fn is_valid_variable(value: &str) -> bool {
    !value.contains('/') && is_valid_trailing_variable(value)
}

/// Whether a run of characters can stand in for a trailing variable.
///
/// Non-empty, and `/` is allowed: with nothing after it, a trailing variable
/// unambiguously takes the remainder. `file:///{path}` is the spec's own
/// example and means a whole path.
///
/// The `..`, `%` and NUL rejections are a path-traversal guard: a handler
/// receives the raw URI and commonly uses the variable as a path component, so
/// refusing to route is a safer default than serving something unintended. It
/// matches the guard the WASM server already applied.
fn is_valid_trailing_variable(value: &str) -> bool {
    !value.is_empty() && !value.contains("..") && !value.contains('%') && !value.contains('\0')
}

/// Whether `uri` is an instance of `template`.
///
/// Convenience for callers that hold the template as a string. Parsing is
/// cheap, but a hot dispatch path should keep a [`UriTemplate`] instead.
#[must_use]
pub fn matches(template: &str, uri: &str) -> bool {
    UriTemplate::parse(template).matches(uri)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_concrete_template_matches_only_itself() {
        assert!(matches("config://app", "config://app"));
        assert!(!matches("config://app", "config://app/extra"));
        assert!(!matches("config://app", "config://ap"));
    }

    /// The defect: everything between the first `{` and the last `}` used to be
    /// ignored, so these all reached the handler for
    /// `db://{table}/rows/{id}.json`.
    #[test]
    fn a_uri_the_template_cannot_produce_does_not_match() {
        let template = "db://{table}/rows/{id}.json";
        assert!(matches(template, "db://users/rows/7.json"));

        assert!(!matches(template, "db://totally/unrelated/path.json"));
        assert!(!matches(template, "db://x.json"));
        assert!(!matches(template, "db://.json"));
        assert!(!matches(template, "db://users/rows/.json"));
    }

    /// Two templates sharing a scheme and an extension used to be
    /// indistinguishable, so whichever was declared first took both.
    #[test]
    fn templates_sharing_a_prefix_and_suffix_stay_distinct() {
        let rows = "db://{table}/rows/{id}.json";
        let meta = "db://{table}/meta.json";

        assert!(matches(rows, "db://users/rows/7.json"));
        assert!(!matches(meta, "db://users/rows/7.json"));

        assert!(matches(meta, "db://users/meta.json"));
        assert!(!matches(rows, "db://users/meta.json"));
    }

    /// An interior variable is confined to one path segment — allowing it to
    /// span them is what let a template claim URIs it could not produce.
    #[test]
    fn an_interior_variable_does_not_span_path_segments() {
        assert!(matches("file:///{name}.txt", "file:///notes.txt"));
        assert!(!matches("file:///{name}.txt", "file:///deep/notes.txt"));
    }

    /// A trailing variable takes the remainder. `file:///{path}` is the spec's
    /// own example, for "Access files in the project directory" — reading that
    /// as one segment would reject the headline case.
    #[test]
    fn a_trailing_variable_spans_path_segments() {
        assert!(matches("file:///{path}", "file:///notes.txt"));
        assert!(matches("file:///{path}", "file:///deep/nested/notes.txt"));
        assert!(matches(
            "apple-doc://{topic}",
            "apple-doc://swift/StringProtocol"
        ));

        // Still not a wildcard: the scheme is literal, and the variable is
        // non-empty.
        assert!(!matches("file:///{path}", "other:///notes.txt"));
        assert!(!matches("file:///{path}", "file:///"));
    }

    #[test]
    fn a_variable_must_be_non_empty() {
        assert!(!matches("file:///{name}.txt", "file:///.txt"));
        assert!(!matches("{whole}", ""));
        assert!(matches("{whole}", "anything"));
    }

    #[test]
    fn traversal_shapes_are_refused() {
        for hostile in [
            "file:///../secret.txt",
            "file:///%2e%2e.txt",
            "file:///a%00b.txt",
        ] {
            assert!(
                !matches("file:///{name}.txt", hostile),
                "{hostile} should not route"
            );
        }
    }

    #[test]
    fn a_leading_variable_is_matched() {
        assert!(matches("{host}/status", "example/status"));
        assert!(!matches("{host}/status", "/status"));
        assert!(!matches("{host}/status", "example/other"));
    }

    #[test]
    fn a_trailing_variable_is_matched() {
        assert!(matches("db://{table}", "db://users"));
        assert!(!matches("db://{table}", "db://"));
        assert!(!matches("db://{table}", "other://users"));
    }

    /// A malformed template must not become a wildcard.
    #[test]
    fn an_unclosed_brace_is_treated_as_literal() {
        assert!(matches("db://{table", "db://{table"));
        assert!(!matches("db://{table", "db://anything"));
    }

    #[test]
    fn is_concrete_distinguishes_templates_from_plain_uris() {
        assert!(UriTemplate::parse("config://app").is_concrete());
        assert!(!UriTemplate::parse("db://{table}").is_concrete());
        assert!(!UriTemplate::parse("{whole}").is_concrete());
    }
}
