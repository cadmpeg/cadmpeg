// SPDX-License-Identifier: Apache-2.0
//! Source-backed views for assertions over transient scanner record types.
//!
//! These values are test views only. The CADIR native wire remains free of
//! offsets and source encodings; missing facts are never fabricated.

use cadmpeg_ir::{CadIr, NativeNamespace, NativeRecord, SourceFidelity};
use serde_json::Value;

pub(crate) fn native_namespace(ir: &CadIr, fidelity: &SourceFidelity) -> NativeNamespace {
    let mut namespace = ir
        .native
        .namespace("catia")
        .expect("codec namespace")
        .clone();
    for (arena, records) in namespace.arenas_mut() {
        for record in records {
            let path = format!("native.catia.{arena}.{}", record.id());
            let mut value = Value::Object(record.fields().expect("typed native fields"));
            restore(&mut value, &path, fidelity);
            let prefix = format!("{path}.");
            for (name, encoded) in &fidelity.metadata {
                if let Some(suffix) = name.strip_prefix(&prefix) {
                    insert(
                        &mut value,
                        suffix,
                        serde_json::from_str(encoded).expect("source layout JSON"),
                    );
                }
            }
            *record = NativeRecord::new(
                cadmpeg_ir::ids::Identity::new(record.id()).expect("native identity"),
                value.as_object().expect("native object").clone(),
            )
            .expect("source-backed record view");
        }
    }
    namespace
}

fn restore(value: &mut Value, path: &str, fidelity: &SourceFidelity) {
    if let Some(id) = value.get("record_id").and_then(Value::as_str).filter(|_| {
        value.get("byte_len").is_some_and(Value::is_number)
            && value.get("sha256").is_some_and(Value::is_string)
    }) {
        let record = fidelity
            .retained_record(id)
            .expect("source evidence record");
        assert_eq!(record.stream(), path, "source evidence owner");
        let bytes = record.data().expect("exact retained source bytes");
        assert_eq!(cadmpeg_ir::hash::sha256_hex(bytes), record.sha256());
        assert_eq!(
            cadmpeg_core::decode::u64_from_index(bytes.len()),
            record.byte_len()
        );
        *value = Value::String(bytes.iter().map(|byte| format!("{byte:02x}")).collect());
        return;
    }
    match value {
        Value::Object(fields) => {
            // Binary64's semantic value is exact; the scanner's transient
            // bits wrapper is reconstructed only inside this test view.
            if let Some(Value::Object(scalar)) = fields.get_mut("Binary64") {
                scalar.remove("non_finite");
                if let Some(number) = scalar.remove("value").and_then(|value| value.as_f64()) {
                    scalar.insert("bits".into(), Value::from(number.to_bits()));
                }
            }
            for (key, child) in fields {
                restore(child, &format!("{path}.{key}"), fidelity);
            }
        }
        Value::Array(values) => {
            for (ordinal, child) in values.iter_mut().enumerate() {
                restore(child, &format!("{path}[{ordinal}]"), fidelity);
            }
        }
        _ => {}
    }
}

fn insert(target: &mut Value, path: &str, value: Value) {
    let end = path.find(['.', '[']).unwrap_or(path.len());
    let key = &path[..end];
    let mut target = target
        .as_object_mut()
        .expect("layout object")
        .entry(key.to_owned())
        .or_insert(Value::Null);
    let mut suffix = &path[end..];
    while let Some(index) = suffix.strip_prefix('[') {
        let close = index.find(']').expect("layout array index terminator");
        let ordinal: usize = index[..close].parse().expect("layout array index");
        target = target
            .as_array_mut()
            .expect("source layout array")
            .get_mut(ordinal)
            .expect("existing source layout array entry");
        suffix = &index[close + 1..];
    }
    if suffix.is_empty() {
        *target = value;
    } else {
        insert(
            target,
            suffix.strip_prefix('.').expect("layout child separator"),
            value,
        );
    }
}
