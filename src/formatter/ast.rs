//! Shared PostgreSQL AST operations; independent of token layout policy.

use serde_json::Value;

/// Remove only reviewed PostgreSQL source metadata from equivalence trees.
pub(super) fn strip_locations(value: &mut Value) {
    match value {
        Value::Object(fields) => {
            for name in [
                "location",
                "stmt_location",
                "stmt_len",
                "arg_location",
                "payload_location",
                "conninfo_location",
            ] {
                fields.remove(name);
            }
            for child in fields.values_mut() {
                strip_locations(child);
            }
        }
        Value::Array(items) => items.iter_mut().for_each(strip_locations),
        _ => {}
    }
}
