//! Decoders for wire fields a receiver must read more carefully than serde's
//! defaults do. The generated types point at these by `deserialize_with`
//! (turbomcp-codegen's `lenient` pass), so each holds on every type that
//! carries the field.
//!
//! Numbers are read through [`serde_json::Number`], never a bare `f64`/`u64`:
//! with `serde_json/arbitrary_precision` enabled anywhere in a build (cargo
//! unifies features, so a downstream binary can switch it on), serde replays a
//! decimal it buffered for an untagged enum or a `flatten` as serde_json's
//! private number map. `Number` reads that map; a primitive rejects it, and an
//! annotation `priority: 0.5` failed the whole content block.

use serde::de::Error as _;
use serde::{Deserialize, Deserializer};
use serde_json::{Number, Value};

/// A JSON number as `f64`.
///
/// # Errors
/// A value that is not a number, or a number beyond `f64` (which serde_json
/// rejects without the feature, too).
pub(crate) fn f64<'de, D: Deserializer<'de>>(deserializer: D) -> Result<f64, D::Error> {
    to_f64(Number::deserialize(deserializer)?)
}

/// An optional JSON number as `f64`; `null` is absent.
///
/// # Errors
/// As [`f64`].
pub(crate) fn option_f64<'de, D: Deserializer<'de>>(
    deserializer: D,
) -> Result<Option<f64>, D::Error> {
    Option::<Number>::deserialize(deserializer)?
        .map(to_f64)
        .transpose()
}

fn to_f64<E: serde::de::Error>(number: Number) -> Result<f64, E> {
    number
        .as_f64()
        .ok_or_else(|| E::custom(format_args!("number out of range: {number}")))
}

/// Any JSON value, with a present `null` kept as `Some(Value::Null)`.
///
/// `Option<Value>` alone reads `null` as `None`, so a tool whose structured
/// result is `null` (an `outputSchema` of `["object", "null"]`) arrived as "no
/// structured content", and a proxy re-sent it without the field. Absent is
/// still `None`, through the field's `default`.
///
/// # Errors
/// Input that is not JSON.
#[allow(clippy::unnecessary_wraps)] // the `deserialize_with` signature
pub(crate) fn present<'de, D: Deserializer<'de>>(
    deserializer: D,
) -> Result<Option<Value>, D::Error> {
    Value::deserialize(deserializer).map(Some)
}

/// `ttlMs`: "A client SHOULD treat a negative `ttlMs` as `0`" (caching), and
/// a JSON number is the same number however it is spelled, so `5000.0` is
/// `5000`. As a plain `u64` either one failed the whole result, so a list
/// call failed over an advisory cache hint.
///
/// # Errors
/// A value that is not a number.
pub(crate) fn ttl_ms<'de, D: Deserializer<'de>>(deserializer: D) -> Result<u64, D::Error> {
    let number = Number::deserialize(deserializer)?;
    if let Some(ms) = number.as_u64() {
        return Ok(ms);
    }
    if number.as_i64().is_some() {
        return Ok(0); // negative
    }
    let ms = number
        .as_f64()
        .ok_or_else(|| D::Error::custom(format_args!("ttlMs out of range: {number}")))?;
    if ms.is_nan() || ms <= 0.0 {
        return Ok(0);
    }
    // Saturating: a float past `u64::MAX` is "forever" either way.
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    Ok(ms as u64)
}

#[cfg(test)]
mod tests {
    use serde::Deserialize;
    use serde_json::{Value, json};

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

    #[derive(Deserialize)]
    struct Floats {
        #[serde(deserialize_with = "super::f64")]
        progress: f64,
        #[serde(default, deserialize_with = "super::option_f64")]
        total: Option<f64>,
    }

    #[test]
    fn floats_read_any_number_and_absent_or_null_is_none() {
        let f: Floats = serde_json::from_str(r#"{"progress": 3, "total": 0.5}"#).unwrap();
        assert_eq!((f.progress, f.total), (3.0, Some(0.5)));
        let f: Floats = serde_json::from_str(r#"{"progress": 1.5, "total": null}"#).unwrap();
        assert_eq!(f.total, None);
        let f: Floats = serde_json::from_str(r#"{"progress": 1.5}"#).unwrap();
        assert_eq!(f.total, None);
        assert!(serde_json::from_str::<Floats>(r#"{"progress": "1"}"#).is_err());
    }

    #[derive(Deserialize)]
    struct Any {
        #[serde(default, deserialize_with = "super::present")]
        data: Option<Value>,
    }

    #[test]
    fn present_null_is_not_absent() {
        let any = |s| serde_json::from_value::<Any>(s).unwrap().data;
        assert_eq!(any(json!({ "data": null })), Some(Value::Null));
        assert_eq!(any(json!({})), None);
        assert_eq!(any(json!({ "data": [1] })), Some(json!([1])));
    }
}
