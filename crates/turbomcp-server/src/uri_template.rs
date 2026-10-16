//! Matching concrete URIs against RFC 6570 URI templates.
//!
//! RFC 6570 defines expansion, not matching, and no maintained Rust crate
//! matches beyond Level 2, so — like the reference SDKs — a template compiles to
//! an anchored regular expression. Every operator is modeled:
//!
//! | Expression | Matches            | Required |
//! |------------|--------------------|----------|
//! | `{var}`    | one path segment   | yes      |
//! | `{+var}`   | anything, across `/` | yes    |
//! | `{#var}`   | `#` + fragment     | no       |
//! | `{.var}`   | `.` + label        | no       |
//! | `{/var}`   | `/` + segment      | no       |
//! | `{;var}`   | `;var=value`       | no       |
//! | `{?var}`, `{&var}` | `?var=value` / `&var=value` | no |
//!
//! Captures are percent-decoded: a client expanding `notes://{title}` with
//! `"Q3 plan"` sends `notes://Q3%20plan`, and the handler should see the title.
//! Decoding never widens what a variable binds, though. One that matches a
//! single segment (`{var}`, `{/var}`, `{.var}`) does not match a value that
//! decodes to a `/` or `\`, so `docs://{name}` can't be handed
//! `..%2F..%2Fetc%2Fpasswd`; and no variable binds a `.` or `..` path segment,
//! which a normalized URI (RFC 3986 §6.2.2.3) never contains and a traversal
//! always does. Such a URI is simply not a match: refused as not found.
//! An optional variable the URI leaves out is simply absent from the result.
//! Prefix modifiers (`{var:3}`) match like the bare variable; an exploded
//! (`{var*}`) list comes back as the expanded text after its leading operator.

use std::sync::{Arc, LazyLock};

use percent_encoding::percent_decode_str;
use regex::Regex;

/// Why a string is not a URI template this matcher can use.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct UriTemplateError(String);

impl core::fmt::Display for UriTemplateError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(f, "invalid URI template: {}", self.0)
    }
}

impl std::error::Error for UriTemplateError {}

/// How one captured value is recovered from its match.
#[derive(Clone, Debug)]
struct Capture {
    name: String,
    group: usize,
    kind: Kind,
    /// The template promises one segment (`{var}`, `{/var}`, `{.var}`), so
    /// the decoded value may not contain a path separator.
    segment: bool,
}

/// What surrounds the value inside the captured text.
#[derive(Clone, Copy, Debug)]
enum Kind {
    /// The capture is the value.
    Value,
    /// An exploded list, captured with its leading operator character.
    Exploded,
    /// `;name` or `;name=value`, captured whole so a bare `;name` still counts.
    Parameter,
}

/// Check `template` against the RFC 6570 grammar this matcher accepts: the
/// one grammar, shared by [`UriTemplate::parse`] (which calls it first) and
/// by `#[resource]`, which evaluates it at compile time so a template the
/// matcher could never read fails the build instead of being listed forever.
///
/// # Errors
/// What the template breaks, as a fixed message.
pub const fn validate(template: &str) -> Result<(), &'static str> {
    let b = template.as_bytes();
    let mut i = 0;
    while i < b.len() {
        match b[i] {
            b'}' => return Err("unmatched `}`"),
            b'{' => {
                let mut close = i + 1;
                while close < b.len() && b[close] != b'}' {
                    if b[close] == b'{' {
                        return Err("`{` inside an expression");
                    }
                    close += 1;
                }
                if close == b.len() {
                    return Err("unclosed `{`");
                }
                if let Err(e) = validate_expression(b, i + 1, close) {
                    return Err(e);
                }
                i = close + 1;
            }
            _ => i += 1,
        }
    }
    Ok(())
}

/// One expression's body, `b[start..end]`: an optional operator, then
/// comma-separated variable specs.
const fn validate_expression(b: &[u8], start: usize, end: usize) -> Result<(), &'static str> {
    let mut i = start;
    if i < end {
        match b[i] {
            b'+' | b'#' | b'.' | b'/' | b';' | b'?' | b'&' => i += 1,
            b'=' | b',' | b'!' | b'@' | b'|' => {
                return Err("the operator is reserved for future extensions");
            }
            _ => {}
        }
    }
    if i == end {
        return Err("empty expression");
    }
    let mut spec = i;
    while i <= end {
        if i == end || b[i] == b',' {
            if let Err(e) = validate_varspec(b, spec, i) {
                return Err(e);
            }
            spec = i + 1;
        }
        i += 1;
    }
    Ok(())
}

/// `varname [ "*" | ":" max-length ]`, where a varname is varchar runs
/// joined by single dots and a varchar is ALPHA / DIGIT / `_` / pct-encoded.
const fn validate_varspec(b: &[u8], start: usize, end: usize) -> Result<(), &'static str> {
    let mut name_end = end;
    if end > start && b[end - 1] == b'*' {
        name_end = end - 1;
    } else {
        let mut colon = start;
        while colon < end && b[colon] != b':' {
            colon += 1;
        }
        if colon < end {
            let digits = end - colon - 1;
            if digits == 0 || digits > 4 || b[colon + 1] == b'0' {
                return Err("a prefix length must be 1-9999");
            }
            let mut d = colon + 1;
            while d < end {
                if !b[d].is_ascii_digit() {
                    return Err("a prefix length must be 1-9999");
                }
                d += 1;
            }
            name_end = colon;
        }
    }
    if name_end == start {
        return Err("empty variable name");
    }
    if b[start] == b'.' || b[name_end - 1] == b'.' {
        return Err("a variable name can't start or end with `.`");
    }
    let mut i = start;
    while i < name_end {
        match b[i] {
            b'%' => {
                if i + 2 >= name_end
                    || !b[i + 1].is_ascii_hexdigit()
                    || !b[i + 2].is_ascii_hexdigit()
                {
                    return Err("a `%` in a variable name must start a pct-encoding");
                }
                i += 3;
            }
            b'.' => {
                if b[i + 1] == b'.' {
                    return Err("a variable name can't contain `..`");
                }
                i += 1;
            }
            c if c.is_ascii_alphanumeric() || c == b'_' => i += 1,
            _ => return Err("a variable name is letters, digits, `_`, `.` and pct-encodings"),
        }
    }
    Ok(())
}

/// A parsed, compiled URI template.
#[derive(Clone, Debug)]
pub struct UriTemplate {
    source: String,
    regex: Regex,
    captures: Vec<Capture>,
}

impl UriTemplate {
    /// Parse `template`, rejecting what RFC 6570 does not allow.
    ///
    /// # Errors
    /// The [`UriTemplateError`] saying what RFC 6570 does not allow.
    pub fn parse(template: &str) -> Result<Self, UriTemplateError> {
        validate(template).map_err(|e| UriTemplateError(format!("{e} in `{template}`")))?;
        let mut pattern = String::from("^");
        let mut captures = Vec::new();
        let mut group = 0usize;
        let mut rest = template;
        while !rest.is_empty() {
            let open = rest.find('{').unwrap_or(rest.len());
            let literal = &rest[..open];
            pattern.push_str(&regex::escape(literal));
            rest = &rest[open..];
            if rest.is_empty() {
                break;
            }
            let close = rest.find('}').unwrap_or(rest.len());
            compile_expression(&rest[1..close], &mut pattern, &mut captures, &mut group)?;
            rest = &rest[close + 1..];
        }
        pattern.push('$');
        let regex = Regex::new(&pattern).map_err(|e| UriTemplateError(e.to_string()))?;
        Ok(Self {
            source: template.to_owned(),
            regex,
            captures,
        })
    }

    /// The template as written.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.source
    }

    /// The variable names, in template order.
    pub fn variables(&self) -> impl Iterator<Item = &str> {
        self.captures.iter().map(|c| c.name.as_str())
    }

    /// Match `uri`, returning each variable it binds with its decoded value.
    /// A value that does not decode to UTF-8 is not a match.
    #[must_use]
    pub fn matches(&self, uri: &str) -> Option<Vec<(String, String)>> {
        let caps = self.regex.captures(uri)?;
        let mut out = Vec::with_capacity(self.captures.len());
        for c in &self.captures {
            let Some(m) = caps.get(c.group) else {
                continue;
            };
            let raw = m.as_str();
            let raw = match c.kind {
                Kind::Value => raw,
                Kind::Exploded => raw.get(1..).unwrap_or_default(),
                Kind::Parameter => {
                    let rest = raw.get(1 + c.name.len()..).unwrap_or_default();
                    rest.strip_prefix('=').unwrap_or(rest)
                }
            };
            let value = percent_decode_str(raw).decode_utf8().ok()?;
            if (c.segment && value.contains(['/', '\\']))
                || value
                    .split(['/', '\\'])
                    .any(|seg| seg == "." || seg == "..")
            {
                return None;
            }
            out.push((c.name.clone(), value.into_owned()));
        }
        Some(out)
    }
}

/// One `{…}` expression: an optional operator and a comma-separated list of
/// variable specs.
fn compile_expression(
    body: &str,
    pattern: &mut String,
    captures: &mut Vec<Capture>,
    group: &mut usize,
) -> Result<(), UriTemplateError> {
    // `validate` has checked the grammar; this only builds.
    let (op, list) = match body.chars().next() {
        Some(c @ ('+' | '#' | '.' | '/' | ';' | '?' | '&')) => (Some(c), &body[1..]),
        _ => (None, body),
    };
    let specs: Vec<VarSpec> = list.split(',').map(VarSpec::parse).collect();
    let many = specs.len() > 1;
    for (i, spec) in specs.iter().enumerate() {
        *group += 1;
        let fragment = match op {
            // Required, like the matcher has always treated them; a list of
            // them is comma-separated.
            None => {
                let sep = if i == 0 { "" } else { "," };
                let class = if many { "[^/?#,]+" } else { "[^/?#]+" };
                format!("{sep}({class})")
            }
            Some('+') => {
                let sep = if i == 0 { "" } else { "," };
                let class = if many { "[^,]+" } else { ".+" };
                format!("{sep}({class})")
            }
            Some('#') => {
                let lead = if i == 0 { "#" } else { "," };
                format!("(?:{lead}({}))?", if many { "[^,]*" } else { ".*" })
            }
            Some(op @ ('.' | '/')) => {
                let lit = regex::escape(&op.to_string());
                let class = if op == '.' { "[^/?#.]*" } else { "[^/?#]*" };
                if spec.explode {
                    format!("((?:{lit}{class})+)?")
                } else {
                    format!("(?:{lit}({class}))?")
                }
            }
            Some(';') => {
                let name = regex::escape(&spec.name);
                format!("(;{name}(?:=[^;/?#]*)?)?")
            }
            Some('?' | '&') => {
                if spec.explode {
                    "((?:[?&][^&#]*)+)?".to_owned()
                } else {
                    let name = regex::escape(&spec.name);
                    format!("(?:[?&]{name}=([^&#]*))?")
                }
            }
            Some(_) => unreachable!("operator set checked above"),
        };
        pattern.push_str(&fragment);
        captures.push(Capture {
            name: spec.name.clone(),
            group: *group,
            kind: match op {
                Some(';') => Kind::Parameter,
                Some('.' | '/' | '?' | '&') if spec.explode => Kind::Exploded,
                _ => Kind::Value,
            },
            segment: matches!(op, None | Some('.' | '/')) && !spec.explode,
        });
    }
    Ok(())
}

struct VarSpec {
    name: String,
    explode: bool,
}

impl VarSpec {
    /// A spec [`validate`] accepted: `varname`, `varname*` or
    /// `varname:max`.
    fn parse(spec: &str) -> Self {
        let (name, explode) = match spec.strip_suffix('*') {
            Some(name) => (name, true),
            None => (spec.split_once(':').map_or(spec, |(name, _)| name), false),
        };
        Self {
            name: name.to_owned(),
            explode,
        }
    }
}

/// Templates are compiled once. A server lists the same handful every time,
/// so a bounded cache keyed by the template text stops every read from
/// recompiling every template it tries.
static COMPILED: LazyLock<moka::sync::Cache<String, Arc<Result<UriTemplate, UriTemplateError>>>> =
    LazyLock::new(|| moka::sync::Cache::new(1024));

/// The compiled form of `template`, from the cache.
pub(crate) fn compiled(template: &str) -> Arc<Result<UriTemplate, UriTemplateError>> {
    COMPILED.get_with_by_ref(template, || Arc::new(UriTemplate::parse(template)))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The grammar runs at compile time, which is what `#[resource]` relies
    /// on.
    const _: () = assert!(validate("file://{+path}{?q,lang}").is_ok());

    #[test]
    fn the_grammar_refuses_what_could_never_match() {
        for bad in [
            "file://{path:0}",
            "file://{path:abc}",
            "file://{path:12345}",
            "file://{}",
            "file://{a..b}",
            "file://{.a}x{a.}",
            "file://{a%2}",
            "file://{a b}",
            "file://{=a}",
            "file://{a",
            "file://a}",
            "file://{a{b}}",
        ] {
            assert!(validate(bad).is_err(), "{bad}");
            assert!(UriTemplate::parse(bad).is_err(), "{bad}");
        }
        for good in [
            "file://{path:3}",
            "file://{path:9999}",
            "file://{a%41b}",
            "file://{a.b}",
            "file://{list*}",
            "file://{/a,b}{?q}",
            "plain://no-variables",
        ] {
            assert!(validate(good).is_ok(), "{good}");
            assert!(UriTemplate::parse(good).is_ok(), "{good}");
        }
    }

    fn m(template: &str, uri: &str) -> Option<Vec<(String, String)>> {
        UriTemplate::parse(template).unwrap().matches(uri)
    }

    fn pairs(v: &[(&str, &str)]) -> Vec<(String, String)> {
        v.iter()
            .map(|(k, v)| ((*k).to_owned(), (*v).to_owned()))
            .collect()
    }

    #[test]
    fn simple_and_reserved_variables() {
        assert_eq!(
            m("file://{name}", "file://notes"),
            Some(pairs(&[("name", "notes")]))
        );
        assert!(
            m("file://{name}", "file://a/b").is_none(),
            "one segment only"
        );
        assert_eq!(
            m("file://{+path}", "file:///etc/hosts"),
            Some(pairs(&[("path", "/etc/hosts")]))
        );
        assert_eq!(
            m("db://{table}/{id}", "db://users/42"),
            Some(pairs(&[("table", "users"), ("id", "42")]))
        );
        assert_eq!(
            m("map://{x},{y}", "map://1,2"),
            Some(pairs(&[("x", "1"), ("y", "2")]))
        );
        assert_eq!(
            m("map://{x,y}", "map://1,2"),
            Some(pairs(&[("x", "1"), ("y", "2")]))
        );
    }

    /// Decoding can't smuggle a separator into a one-segment variable, and no
    /// variable binds a dot segment, encoded or not.
    #[test]
    fn decoding_never_widens_a_capture() {
        assert!(m("docs://{name}", "docs://..%2F..%2Fetc%2Fpasswd").is_none());
        assert!(m("docs://{name}", "docs://a%2Fb").is_none());
        assert!(m("docs://{name}", "docs://a%5Cb").is_none());
        assert!(m("docs://{name}", "docs://%2E%2E").is_none());
        assert!(m("docs://{name}", "docs://..").is_none());
        assert!(m("file://{+path}", "file:///srv/../etc/passwd").is_none());
        assert!(m("file://{+path}", "file:///srv/%2E%2E/etc/passwd").is_none());
        assert!(m("db{/table}", "db/..").is_none());
        // A dot inside a segment is just a dot.
        assert_eq!(
            m("docs://{name}", "docs://v1..2.txt"),
            Some(pairs(&[("name", "v1..2.txt")]))
        );
        assert_eq!(
            m("file://{+path}", "file:///srv/a.b/c"),
            Some(pairs(&[("path", "/srv/a.b/c")]))
        );
    }

    #[test]
    fn literals_are_literal() {
        assert!(m("file://{name}", "http://notes").is_none());
        assert_eq!(m("x.y://{v}", "x.y://z"), Some(pairs(&[("v", "z")])));
        assert!(m("x.y://{v}", "xqy://z").is_none(), "`.` is not any-char");
    }

    #[test]
    fn captures_are_percent_decoded() {
        assert_eq!(
            m("notes://{title}", "notes://Q3%20plan"),
            Some(pairs(&[("title", "Q3 plan")]))
        );
        assert_eq!(
            m("notes://{title}", "notes://caf%C3%A9"),
            Some(pairs(&[("title", "café")]))
        );
        assert!(m("notes://{title}", "notes://%FF").is_none(), "not UTF-8");
    }

    #[test]
    fn query_variables_are_optional_and_named() {
        assert_eq!(m("search://items{?q}", "search://items"), Some(Vec::new()));
        assert_eq!(
            m("search://items{?q}", "search://items?q=red%20shoes"),
            Some(pairs(&[("q", "red shoes")]))
        );
        assert_eq!(
            m("search://items{?q,limit}", "search://items?q=a&limit=5"),
            Some(pairs(&[("q", "a"), ("limit", "5")]))
        );
        assert_eq!(
            m("search://items{?q,limit}", "search://items?limit=5"),
            Some(pairs(&[("limit", "5")]))
        );
        assert_eq!(
            m("search://items{?q}{&page}", "search://items?q=a&page=2"),
            Some(pairs(&[("q", "a"), ("page", "2")]))
        );
        assert!(m("search://items{?q}", "search://items?other=1").is_none());
    }

    #[test]
    fn path_label_fragment_and_parameter_operators() {
        assert_eq!(
            m("fs://root{/dir}", "fs://root/docs"),
            Some(pairs(&[("dir", "docs")]))
        );
        assert_eq!(m("fs://root{/dir}", "fs://root"), Some(Vec::new()));
        assert_eq!(
            m("fs://root{/path*}", "fs://root/a/b/c"),
            Some(pairs(&[("path", "a/b/c")]))
        );
        assert_eq!(
            m("host://www{.domain}", "host://www.example"),
            Some(pairs(&[("domain", "example")]))
        );
        assert_eq!(
            m("doc://page{#section}", "doc://page#intro"),
            Some(pairs(&[("section", "intro")]))
        );
        assert_eq!(m("doc://page{#section}", "doc://page"), Some(Vec::new()));
        assert_eq!(m("m://x{;v}", "m://x;v=3"), Some(pairs(&[("v", "3")])));
        assert_eq!(m("m://x{;v}", "m://x;v"), Some(pairs(&[("v", "")])));
    }

    #[test]
    fn prefix_modifiers_match_like_the_bare_variable() {
        assert_eq!(m("k://{key:3}", "k://abc"), Some(pairs(&[("key", "abc")])));
    }

    #[test]
    fn malformed_templates_are_refused() {
        for bad in [
            "x://{",
            "x://}",
            "x://{}",
            "x://{a b}",
            "x://{=a}",
            "x://{a:0}",
            "x://{a:10000}",
            "x://{.a.}",
            "x://{a..b}",
            "x://{%zz}",
        ] {
            assert!(UriTemplate::parse(bad).is_err(), "{bad}");
        }
        assert!(UriTemplate::parse("x://{a.b}").is_ok());
        assert!(UriTemplate::parse("x://{a%20b}").is_ok());
    }

    #[test]
    fn the_cache_hands_back_one_compilation() {
        let a = compiled("cache://{v}");
        let b = compiled("cache://{v}");
        assert!(Arc::ptr_eq(&a, &b));
    }
}
