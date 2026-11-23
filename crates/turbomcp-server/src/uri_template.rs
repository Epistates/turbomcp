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
//!
//! A template has at most [`UriTemplate::MAX_LEN`] bytes and
//! [`UriTemplate::MAX_VARIABLES`] variables. What a compiled template holds
//! grows with its text and, once a URI matches it, with its variables: one
//! with 800 variables holds about 248 MB (regex 1.13). A server's own
//! templates sit far inside both limits. A gateway's come from its upstreams,
//! and the limits are what make those safe to compile and cache.

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

/// Why [`validate`] refuses a template. The text is built in a `const`, so
/// the build error `#[resource]` reports can carry a limit and the count
/// that broke it.
#[derive(Clone, Copy)]
pub struct Refusal {
    text: [u8; 96],
    len: usize,
}

impl Refusal {
    const fn new(text: &str) -> Self {
        Self {
            text: [0; 96],
            len: 0,
        }
        .push(text)
    }

    /// `self` followed by `text` (ASCII), cut at capacity.
    const fn push(mut self, text: &str) -> Self {
        let b = text.as_bytes();
        let mut i = 0;
        while i < b.len() && self.len < self.text.len() {
            self.text[self.len] = b[i];
            self.len += 1;
            i += 1;
        }
        self
    }

    /// `self` followed by `n` in decimal.
    const fn number(mut self, mut n: usize) -> Self {
        let mut digits = [0u8; 20];
        let mut count = 0;
        loop {
            digits[count] = b'0' + (n % 10) as u8;
            count += 1;
            n /= 10;
            if n == 0 {
                break;
            }
        }
        while count > 0 && self.len < self.text.len() {
            count -= 1;
            self.text[self.len] = digits[count];
            self.len += 1;
        }
        self
    }

    /// The reason, as text.
    #[must_use]
    pub const fn as_str(&self) -> &str {
        match core::str::from_utf8(self.text.split_at(self.len).0) {
            Ok(text) => text,
            Err(_) => "invalid URI template",
        }
    }
}

impl core::fmt::Display for Refusal {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str(self.as_str())
    }
}

impl core::fmt::Debug for Refusal {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_tuple("Refusal").field(&self.as_str()).finish()
    }
}

/// Check `template` against the RFC 6570 grammar this matcher accepts, and
/// against its limits: the one check, shared by [`UriTemplate::parse`]
/// (which calls it first) and by `#[resource]`, which evaluates it at
/// compile time so a template the matcher could never read fails the build
/// instead of being listed forever.
///
/// # Errors
/// What the template breaks.
pub const fn validate(template: &str) -> Result<(), Refusal> {
    let b = template.as_bytes();
    if b.len() > UriTemplate::MAX_LEN {
        return Err(Refusal::new("a URI template can be at most ")
            .number(UriTemplate::MAX_LEN)
            .push(" bytes; this one is ")
            .number(b.len()));
    }
    let mut variables = 0;
    let mut i = 0;
    while i < b.len() {
        match b[i] {
            b'}' => return Err(Refusal::new("unmatched `}`")),
            b'{' => {
                let mut close = i + 1;
                while close < b.len() && b[close] != b'}' {
                    if b[close] == b'{' {
                        return Err(Refusal::new("`{` inside an expression"));
                    }
                    close += 1;
                }
                if close == b.len() {
                    return Err(Refusal::new("unclosed `{`"));
                }
                match validate_expression(b, i + 1, close) {
                    Ok(specs) => variables += specs,
                    Err(e) => return Err(Refusal::new(e)),
                }
                i = close + 1;
            }
            _ => i += 1,
        }
    }
    if variables > UriTemplate::MAX_VARIABLES {
        return Err(Refusal::new("a URI template can have at most ")
            .number(UriTemplate::MAX_VARIABLES)
            .push(" variables; this one has ")
            .number(variables));
    }
    Ok(())
}

/// One expression's body, `b[start..end]`: an optional operator, then
/// comma-separated variable specs, counted.
const fn validate_expression(b: &[u8], start: usize, end: usize) -> Result<usize, &'static str> {
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
    let mut specs = 0;
    while i <= end {
        if i == end || b[i] == b',' {
            if let Err(e) = validate_varspec(b, spec, i) {
                return Err(e);
            }
            specs += 1;
            spec = i + 1;
        }
        i += 1;
    }
    Ok(specs)
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
    /// The longest template the matcher compiles, in bytes. A compiled
    /// template holds about 340 bytes for each byte of its text.
    pub const MAX_LEN: usize = 2048;

    /// The most variables a template can have. Once a URI matches, a
    /// template holds up to about 16 KiB more for each, and the growth
    /// steepens past a few hundred.
    pub const MAX_VARIABLES: usize = 32;

    /// Parse `template`, rejecting what RFC 6570 does not allow and what is
    /// over the matcher's limits.
    ///
    /// # Errors
    /// The [`UriTemplateError`] saying what the template breaks.
    pub fn parse(template: &str) -> Result<Self, UriTemplateError> {
        validate(template)
            .map_err(|e| UriTemplateError(format!("{e} in `{}`", quoted(template))))?;
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

    /// `template` parsed, from a cache the whole process shares: each
    /// template compiles once, however often it's matched. The cache is
    /// bounded by an estimate of the memory its templates hold, so templates
    /// from a peer (a gateway's upstreams) can use it too.
    ///
    /// # Errors
    /// As [`parse`](Self::parse); a refusal is cached as well.
    pub fn cached(template: &str) -> Result<Arc<Self>, UriTemplateError> {
        COMPILED.get_with_by_ref(template, || Self::parse(template).map(Arc::new))
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

/// `template` as an error quotes it: whole, or its first 120 bytes and `…`.
fn quoted(template: &str) -> String {
    turbomcp_core::budget::shortened(template, 120).into_owned()
}

/// What the cache of compiled templates may hold, in estimated bytes.
const CACHE_BUDGET: u64 = 64 * 1024 * 1024;

/// Templates are compiled once. A server lists the same handful every time,
/// so a cache keyed by the template text stops every read from recompiling
/// every template it tries. It's bounded by [`weight`], an estimate of
/// memory, rather than by a count that a few large templates could fill with
/// gigabytes.
static COMPILED: LazyLock<moka::sync::Cache<String, Result<Arc<UriTemplate>, UriTemplateError>>> =
    LazyLock::new(|| {
        moka::sync::Cache::builder()
            .max_capacity(CACHE_BUDGET)
            .weigher(|template: &String, compiled| weight(template, compiled))
            .build()
    });

/// Roughly what a cached template holds, in bytes, measured with regex 1.13:
/// a fixed cost, about 340 bytes for each byte of the template, and up to
/// about 16 KiB for each variable once a URI has matched. A refusal holds
/// only its text.
fn weight(template: &str, compiled: &Result<Arc<UriTemplate>, UriTemplateError>) -> u32 {
    let len = u32::try_from(template.len()).unwrap_or(u32::MAX);
    match compiled {
        Ok(compiled) => {
            let variables = u32::try_from(compiled.captures.len()).unwrap_or(u32::MAX);
            16_384u32
                .saturating_add(len.saturating_mul(384))
                .saturating_add(variables.saturating_mul(16_384))
        }
        Err(_) => len.saturating_add(1024),
    }
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
        let a = UriTemplate::cached("cache://{v}").unwrap();
        let b = UriTemplate::cached("cache://{v}").unwrap();
        assert!(Arc::ptr_eq(&a, &b));
    }

    /// `n` variables, `{v0}-{v1}-…`, after `x://`.
    fn with_variables(n: usize) -> String {
        let vars: Vec<String> = (0..n).map(|i| format!("{{v{i}}}")).collect();
        format!("x://{}", vars.join("-"))
    }

    /// A template with 800 variables would hold about 248 MB once a URI
    /// matched it. The limits refuse it, by its length here, and say so with
    /// the numbers.
    #[test]
    fn the_limits_refuse_a_template_too_costly_to_match() {
        let huge = with_variables(800);
        let e = UriTemplate::parse(&huge).unwrap_err().to_string();
        assert!(
            e.contains(&format!("at most 2048 bytes; this one is {}", huge.len())),
            "{e}"
        );
        assert!(e.ends_with("…`"), "the error quotes only the start: {e}");

        let max = with_variables(UriTemplate::MAX_VARIABLES);
        assert!(UriTemplate::parse(&max).is_ok());
        let over = with_variables(UriTemplate::MAX_VARIABLES + 1);
        assert_eq!(
            validate(&over).unwrap_err().as_str(),
            "a URI template can have at most 32 variables; this one has 33"
        );
        // Variables in one expression count one by one.
        let list: Vec<String> = (0..33).map(|i| format!("q{i}")).collect();
        assert!(validate(&format!("x://s{{?{}}}", list.join(","))).is_err());

        let long = format!("x://{}{{v}}", "a".repeat(UriTemplate::MAX_LEN));
        assert!(UriTemplate::parse(&long).is_err());
    }

    /// The cache's estimate of what it holds stays within its budget however
    /// many large templates pass through it.
    #[test]
    fn the_cache_stays_within_its_memory_budget() {
        let pad = "p".repeat(UriTemplate::MAX_LEN - with_variables(32).len() - 8);
        let heaviest = weight(
            &format!("{}{pad}", with_variables(32)),
            &UriTemplate::parse(&format!("{}{pad}", with_variables(32))).map(Arc::new),
        );
        let templates = usize::try_from(CACHE_BUDGET / u64::from(heaviest)).unwrap() * 2;
        for i in 0..templates {
            let template = format!("{}{pad}{i:08}", with_variables(32));
            assert!(UriTemplate::cached(&template).is_ok(), "{i}");
        }
        COMPILED.run_pending_tasks();
        assert!(
            COMPILED.weighted_size() <= CACHE_BUDGET,
            "{} > {CACHE_BUDGET}",
            COMPILED.weighted_size()
        );
    }
}
