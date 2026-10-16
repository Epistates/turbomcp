//! The official schema as a test oracle, shared by the schema tests.
//!
//! The schemas (and the `2026-07-28` examples) are vendored from the same
//! upstream tag as the generated types by `just codegen`, and drift-checked
//! with them.

#![allow(dead_code)] // each test file uses its own subset

use std::cell::RefCell;
use std::collections::HashMap;
use std::path::PathBuf;
use std::rc::Rc;

use serde_json::{Map, Value};

pub fn fixtures(rev: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(format!("tests/fixtures/schema/{rev}"))
}

pub struct Oracle {
    pub rev: &'static str,
    pub root: Value,
    pub defs_key: &'static str,
    validators: RefCell<HashMap<String, Rc<jsonschema::Validator>>>,
}

impl Oracle {
    pub fn load(rev: &'static str) -> Self {
        let path = fixtures(rev).join("schema.json");
        let raw = std::fs::read_to_string(&path)
            .unwrap_or_else(|e| panic!("{}: {e} (run `just codegen`)", path.display()));
        let root: Value = serde_json::from_str(&raw).expect("schema.json is JSON");
        let defs_key = if root.get("$defs").is_some() {
            "$defs"
        } else {
            "definitions"
        };
        Self {
            rev,
            root,
            defs_key,
            validators: RefCell::default(),
        }
    }

    /// Every definition, in schema order.
    pub fn defs(&self) -> &Map<String, Value> {
        self.root[self.defs_key]
            .as_object()
            .expect("the schema has definitions")
    }

    pub fn def(&self, name: &str) -> &Value {
        self.defs()
            .get(name)
            .unwrap_or_else(|| panic!("{}: no definition named {name}", self.rev))
    }

    /// The ways `instance` violates the named definition (empty: it conforms).
    pub fn violations(&self, def: &str, instance: &Value) -> Vec<String> {
        let validator = self
            .validators
            .borrow_mut()
            .entry(def.to_owned())
            .or_insert_with(|| {
                self.def(def);
                let mut schema = self.root.clone();
                schema["$ref"] = Value::String(format!("#/{}/{def}", self.defs_key));
                Rc::new(
                    jsonschema::validator_for(&schema)
                        .unwrap_or_else(|e| panic!("{}: compiling {def}: {e}", self.rev)),
                )
            })
            .clone();
        validator
            .iter_errors(instance)
            .map(|e| format!("{} at {}", e, e.instance_path()))
            .collect()
    }

    /// Panic unless `instance` conforms to the named definition.
    pub fn validate(&self, def: &str, instance: &Value) {
        let errors = self.violations(def, instance);
        assert!(
            errors.is_empty(),
            "{} {def} violates the schema:\n  {}\ninstance: {instance:#}",
            self.rev,
            errors.join("\n  ")
        );
    }
}

/// What changed between the JSON that was sent and the JSON a typed decode
/// re-serialized, as `-path` (dropped), `+path = value` (added) and
/// `~path: before -> after` lines. A test asserts this list, so every
/// difference is named rather than normalized away.
pub fn changes(sent: &Value, back: &Value) -> Vec<String> {
    let mut out = Vec::new();
    walk(sent, back, String::new(), &mut out);
    return out;

    fn walk(sent: &Value, back: &Value, path: String, out: &mut Vec<String>) {
        match (sent, back) {
            (Value::Object(a), Value::Object(b)) => {
                for (key, value) in a {
                    let at = format!("{path}/{key}");
                    match b.get(key) {
                        Some(other) => walk(value, other, at, out),
                        None => out.push(format!("-{at}")),
                    }
                }
                for (key, value) in b {
                    if !a.contains_key(key) {
                        out.push(format!("+{path}/{key} = {value}"));
                    }
                }
            }
            (Value::Array(a), Value::Array(b)) if a.len() == b.len() => {
                for (i, (x, y)) in a.iter().zip(b).enumerate() {
                    walk(x, y, format!("{path}/{i}"), out);
                }
            }
            _ if sent == back => {}
            _ => out.push(format!("~{path}: {sent} -> {back}")),
        }
    }
}
