//! Decoders for wire fields the spec tells a receiver to tolerate rather than
//! reject. The generated types point at these by `deserialize_with` (see
//! turbomcp-codegen's `lenient_fields`), so the tolerance holds on every
//! result that carries the field.

use core::fmt;

use serde::Deserializer;
use serde::de::{self, Visitor};

/// `ttlMs`: "A client SHOULD treat a negative `ttlMs` as `0`" (caching), and
/// a JSON number is the same number however it is spelled, so `5000.0` is
/// `5000`. As a plain `u64` either one failed the whole result, so a list
/// call failed over an advisory cache hint.
///
/// # Errors
/// A value that is not a number.
pub(crate) fn ttl_ms<'de, D: Deserializer<'de>>(deserializer: D) -> Result<u64, D::Error> {
    struct TtlMs;

    impl Visitor<'_> for TtlMs {
        type Value = u64;

        fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
            f.write_str("a number of milliseconds")
        }

        fn visit_u64<E: de::Error>(self, v: u64) -> Result<u64, E> {
            Ok(v)
        }

        fn visit_i64<E: de::Error>(self, v: i64) -> Result<u64, E> {
            Ok(u64::try_from(v).unwrap_or(0))
        }

        fn visit_f64<E: de::Error>(self, v: f64) -> Result<u64, E> {
            if v.is_nan() || v <= 0.0 {
                return Ok(0);
            }
            // Saturating: a float past `u64::MAX` is "forever" either way.
            #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
            Ok(v as u64)
        }
    }

    deserializer.deserialize_any(TtlMs)
}

#[cfg(test)]
mod tests {
    use serde::Deserialize;

    #[derive(Deserialize)]
    struct Hinted {
        #[serde(rename = "ttlMs", default, deserialize_with = "super::ttl_ms")]
        ttl_ms: u64,
    }

    fn ttl(json: &str) -> u64 {
        serde_json::from_str::<Hinted>(json).unwrap().ttl_ms
    }

    #[test]
    fn negative_and_float_spellings_are_tolerated() {
        assert_eq!(ttl(r#"{"ttlMs": 5000}"#), 5000);
        assert_eq!(ttl(r#"{"ttlMs": 5000.0}"#), 5000);
        assert_eq!(ttl(r#"{"ttlMs": -1}"#), 0);
        assert_eq!(ttl(r#"{"ttlMs": -0.5}"#), 0);
        assert_eq!(ttl(r#"{}"#), 0);
        assert!(serde_json::from_str::<Hinted>(r#"{"ttlMs": "soon"}"#).is_err());
    }
}
