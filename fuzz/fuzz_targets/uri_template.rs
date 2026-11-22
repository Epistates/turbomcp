#![no_main]
//! Resource URI-template matching: a template (from a `#[resource("...")]`,
//! trusted, or from a gateway's upstream, not) against a URI from
//! `resources/read` (untrusted). Neither compiling nor matching may panic or
//! hang; a malformed template, or one over the matcher's limits, yields
//! `None`.
//!
//! The first byte picks the half. Most inputs are a URI matched against a
//! fixed set of templates covering every operator, which exercises the
//! matcher itself (captures, percent-decoding, the traversal checks) at
//! matching speed. The rest are a template of their own and a URI, split on
//! the first NUL, as a gateway compiles each new upstream template. Each such
//! template is new to the cache, so those inputs run at regex-compile speed.

use libfuzzer_sys::fuzz_target;
use turbomcp_server::__macro_support::match_uri_template;

const TEMPLATES: &[&str] = &[
    "file://{+path}",
    "docs://{name}",
    "repo://{owner}/{repo}/issues/{number}{?state,labels}",
    "x://pre/{id:3}",
    "x://a{#frag}",
    "x://h{.ext}",
    "x://p{/seg}{/list*}",
    "x://m{;k,v}",
    "x://q{?q*}{&page}",
];

fuzz_target!(|data: &[u8]| {
    let Some((&half, rest)) = data.split_first() else {
        return;
    };
    let Ok(s) = std::str::from_utf8(rest) else {
        return;
    };
    if half < 0xc0 {
        for template in TEMPLATES {
            let _ = match_uri_template(template, s);
        }
    } else {
        // Fall back to matching the whole string against itself.
        let (template, uri) = s.split_once('\0').unwrap_or((s, s));
        let _ = match_uri_template(template, uri);
    }
});
