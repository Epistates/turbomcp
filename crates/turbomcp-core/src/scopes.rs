//! Scope hierarchies: when a granted OAuth scope covers a required one.
//!
//! "Servers MUST account for scope hierarchies, where a broader scope implies
//! narrower ones, when deciding whether a token is sufficient for an
//! operation" (2026-07-28 authorization). What a scope implies is the
//! authorization server's to define, so a [`ScopeHierarchy`] is declared
//! where the server's authenticator is configured, and travels with each
//! request it authorizes as a [`ScopeRules`] fact in the request's
//! [`Extensions`](crate::Extensions). Every scope check then goes through
//! [`RequestContext::has_scopes`](crate::RequestContext::has_scopes), which
//! consults it.
//!
//! With none declared, a scope covers exactly itself.

use alloc::collections::BTreeMap;
use alloc::string::String;
use alloc::sync::Arc;
use alloc::vec::Vec;
use core::fmt;

/// When a granted scope covers a required one. Every scope covers itself
/// whatever the hierarchy says; [`implies`](Self::implies) is asked only
/// about distinct scopes.
pub trait ScopeHierarchy: Send + Sync + fmt::Debug {
    /// Whether holding `granted` is enough for an operation requiring
    /// `required`.
    fn implies(&self, granted: &str, required: &str) -> bool;
}

/// The hierarchy written into scope names: `files` covers `files:read`,
/// which covers `files:read:public`, with `:` (or another separator) between
/// levels. A scope covers the ones under it, never the reverse, and `files`
/// does not cover `filesystem`.
#[derive(Clone, Copy, Debug)]
pub struct PrefixHierarchy {
    separator: char,
}

impl PrefixHierarchy {
    /// Levels separated by `separator`.
    #[must_use]
    pub const fn new(separator: char) -> Self {
        Self { separator }
    }
}

impl Default for PrefixHierarchy {
    /// Levels separated by `:`.
    fn default() -> Self {
        Self::new(':')
    }
}

impl ScopeHierarchy for PrefixHierarchy {
    fn implies(&self, granted: &str, required: &str) -> bool {
        required
            .strip_prefix(granted)
            .is_some_and(|rest| rest.starts_with(self.separator))
    }
}

/// A hierarchy spelled out scope by scope: `admin` implies `files:write` and
/// `users:read`, say. Implication is transitive, so `admin` implying `write`
/// and `write` implying `read` makes `admin` cover `read`.
#[derive(Clone, Debug, Default)]
pub struct ImpliedScopes {
    implied: BTreeMap<String, Vec<String>>,
}

impl ImpliedScopes {
    /// No implications yet.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Holding `scope` also grants each of `implied`.
    #[must_use]
    pub fn implies<I, S>(mut self, scope: impl Into<String>, implied: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        self.implied
            .entry(scope.into())
            .or_default()
            .extend(implied.into_iter().map(Into::into));
        self
    }
}

impl ScopeHierarchy for ImpliedScopes {
    fn implies(&self, granted: &str, required: &str) -> bool {
        // Depth-first over the implication graph; `seen` makes a cycle in the
        // table terminate rather than recurse forever.
        let mut stack: Vec<&str> = alloc::vec![granted];
        let mut seen: Vec<&str> = Vec::new();
        while let Some(scope) = stack.pop() {
            if seen.contains(&scope) {
                continue;
            }
            seen.push(scope);
            for next in self.implied.get(scope).into_iter().flatten() {
                if next == required {
                    return true;
                }
                stack.push(next);
            }
        }
        false
    }
}

/// The scope hierarchy in force for a request, as a fact in its
/// [`Extensions`](crate::Extensions). Attached by the transport from the
/// authenticator that authorized the request.
#[derive(Clone, Debug)]
pub struct ScopeRules(pub Arc<dyn ScopeHierarchy>);

/// The scopes a token's claims grant: `scope` and `scp`, each a
/// space-delimited string or an array of strings (Microsoft Entra ID issues
/// `scp` as a string). `claim` looks a claim up by name. The one reading of
/// "granted" everything shares, so a resource server's baseline gate and a
/// tool's declared scopes can't disagree about a token.
pub fn granted_by<'a>(claim: impl Fn(&str) -> Option<&'a serde_json::Value>) -> Vec<String> {
    let mut out = Vec::new();
    for name in ["scope", "scp"] {
        match claim(name) {
            Some(serde_json::Value::String(s)) => {
                out.extend(s.split_whitespace().map(String::from))
            }
            Some(serde_json::Value::Array(arr)) => {
                out.extend(arr.iter().filter_map(|v| v.as_str().map(String::from)));
            }
            _ => {}
        }
    }
    out
}

impl ScopeRules {
    /// Whether `granted` covers every scope in `required` under these rules.
    #[must_use]
    pub fn satisfies(&self, granted: &[String], required: &[&str]) -> bool {
        satisfies(Some(self.0.as_ref()), granted, required)
    }
}

/// Whether `granted` covers every scope in `required` under `hierarchy`
/// (exact matching when there is none). An empty requirement is satisfied.
#[must_use]
pub fn satisfies(
    hierarchy: Option<&dyn ScopeHierarchy>,
    granted: &[String],
    required: &[&str],
) -> bool {
    required.iter().all(|required| {
        granted.iter().any(|granted| {
            granted == required || hierarchy.is_some_and(|h| h.implies(granted, required))
        })
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloc::borrow::ToOwned;

    fn granted(scopes: &[&str]) -> Vec<String> {
        scopes.iter().map(|s| (*s).to_owned()).collect()
    }

    #[test]
    fn without_a_hierarchy_a_scope_covers_only_itself() {
        assert!(satisfies(None, &granted(&["files"]), &["files"]));
        assert!(!satisfies(None, &granted(&["files"]), &["files:read"]));
        assert!(satisfies(None, &granted(&[]), &[]));
    }

    #[test]
    fn a_prefix_covers_the_levels_under_it_only() {
        let h = PrefixHierarchy::default();
        let g = granted(&["files"]);
        assert!(satisfies(
            Some(&h),
            &g,
            &["files:read", "files:read:public"]
        ));
        assert!(!satisfies(Some(&h), &g, &["filesystem"]));
        assert!(!satisfies(Some(&h), &granted(&["files:read"]), &["files"]));
    }

    #[test]
    fn implications_are_transitive_and_cycles_end() {
        let h = ImpliedScopes::new()
            .implies("admin", ["write"])
            .implies("write", ["read"])
            .implies("read", ["admin"]);
        assert!(satisfies(Some(&h), &granted(&["admin"]), &["read"]));
        assert!(!satisfies(Some(&h), &granted(&["admin"]), &["billing"]));
        let rules = ScopeRules(Arc::new(h));
        assert!(rules.satisfies(&granted(&["write"]), &["read", "write"]));
        assert!(!rules.satisfies(&[], &["read"]));
    }
}
