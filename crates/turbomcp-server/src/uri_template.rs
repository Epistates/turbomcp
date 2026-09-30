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

/// A parsed, compiled URI template.
#[derive(Clone, Debug)]
pub struct UriTemplate {
    source: String,
    regex: Regex,
    captures: Vec<Capture>,
}

impl UriTemplate {
    /// Parse `template`, rejecting what RFC 6570 does not allow.
    pub fn parse(template: &str) -> Result<Self, UriTemplateError> {
        let mut pattern = String::from("^");
        let mut captures = Vec::new();
        let mut group = 0usize;
        let mut rest = template;
        while !rest.is_empty() {
            let open = rest.find('{').unwrap_or(rest.len());
            let literal = &rest[..open];
            if literal.contains('}') {
                return Err(UriTemplateError(format!("unmatched `}}` in `{template}`")));
            }
            pattern.push_str(&regex::escape(literal));
            rest = &rest[open..];
            if rest.is_empty() {
                break;
            }
            let close = rest
                .find('}')
                .ok_or_else(|| UriTemplateError(format!("unclosed `{{` in `{template}`")))?;
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
    let (op, list) = match body.chars().next() {
        Some(c @ ('+' | '#' | '.' | '/' | ';' | '?' | '&')) => (Some(c), &body[1..]),
        Some(c @ ('=' | ',' | '!' | '@' | '|')) => {
            return Err(UriTemplateError(format!(
                "operator `{c}` is reserved for future extensions"
            )));
        }
        _ => (None, body),
    };
    if list.is_empty() {
        return Err(UriTemplateError("empty expression `{}`".into()));
    }
    let specs: Vec<VarSpec> = list
        .split(',')
        .map(VarSpec::parse)
        .collect::<Result<_, _>>()?;
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
        });
    }
    Ok(())
}

struct VarSpec {
    name: String,
    explode: bool,
}

impl VarSpec {
    /// `varname [ "*" | ":" max-length ]`, where a varname is `varchar` runs
    /// joined by single dots and a varchar is ALPHA / DIGIT / `_` / pct-encoded.
    fn parse(spec: &str) -> Result<Self, UriTemplateError> {
        let (name, explode) = match spec.strip_suffix('*') {
            Some(name) => (name, true),
            None => match spec.split_once(':') {
                Some((name, max)) => {
                    let valid = !max.is_empty()
                        && max.len() <= 4
                        && max.bytes().all(|b| b.is_ascii_digit())
                        && !max.starts_with('0');
                    if !valid {
                        return Err(UriTemplateError(format!(
                            "prefix length in `{spec}` must be 1-9999"
                        )));
                    }
                    (name, false)
                }
                None => (spec, false),
            },
        };
        let valid_name = !name.is_empty()
            && !name.starts_with('.')
            && !name.ends_with('.')
            && !name.contains("..")
            && is_varname(name);
        if !valid_name {
            return Err(UriTemplateError(format!("invalid variable name `{name}`")));
        }
        Ok(Self {
            name: name.to_owned(),
            explode,
        })
    }
}

fn is_varname(name: &str) -> bool {
    let bytes = name.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'%' => {
                let hex = bytes.get(i + 1..i + 3);
                if !hex.is_some_and(|h| h.iter().all(u8::is_ascii_hexdigit)) {
                    return false;
                }
                i += 3;
            }
            b if b.is_ascii_alphanumeric() || b == b'_' || b == b'.' => i += 1,
            _ => return false,
        }
    }
    true
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
