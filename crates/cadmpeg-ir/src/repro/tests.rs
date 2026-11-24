// SPDX-License-Identifier: Apache-2.0
use super::*;
use crate::report::{check::Check, Severity};

#[derive(Serialize)]
struct Spellings {
    text: String,
    floats: Vec<f64>,
    single: f32,
    nested: Vec<Vec<u8>>,
    empty: BTreeMap<String, u8>,
    keyed: BTreeMap<String, Option<bool>>,
    unit: (),
}

fn spellings() -> Spellings {
    Spellings {
        text: "a \"quoted\" \\ text\nwith  spaces\t".into(),
        floats: vec![f64::NAN, f64::INFINITY, -0.0, 1.5e300, 0.1],
        single: 1.1,
        nested: vec![Vec::new(), vec![1, 2]],
        empty: BTreeMap::new(),
        keyed: BTreeMap::from([("k e y".into(), None), ("x".into(), Some(true))]),
        unit: (),
    }
}

/// The bytes `serde_json` writes for `value`, with negative zero written as
/// zero (§2.5).
fn one_zero_bytes(value: &Spellings) -> Vec<u8> {
    serde_json::to_string(value)
        .unwrap()
        .replace("-0.0", "0.0")
        .into_bytes()
}

#[test]
fn compact_projection_writes_the_bytes_serde_json_writes() {
    let value = spellings();
    let expected = one_zero_bytes(&value);
    assert_ne!(expected, serde_json::to_vec(&value).unwrap());
    assert_eq!(compact_json(&value, usize::MAX).unwrap(), expected);
}

#[test]
fn compact_projection_refuses_one_byte_past_its_limit() {
    let value = spellings();
    let length = one_zero_bytes(&value).len();
    assert_eq!(compact_json(&value, length).unwrap().len(), length);
    let error = compact_json(&value, length - 1).unwrap_err();
    assert!(
        matches!(&error, CodecError::Malformed(message) if message == "fragment byte limit exceeded"),
        "{error}"
    );
}

#[test]
fn repro_signature_ignores_identity_numbers_and_value_for_hash() {
    let mut finding = Finding {
        check: Check::GeometricConsistency,
        severity: Severity::Error,
        entity: Some("synthetic:scope:edge#123".into()),
        related: Vec::new(),
        message: "edge synthetic:scope:edge#123 misses by 1.5e-3".into(),
    };
    let first = Signature::new(&finding, "cadir", Stage::Check);
    finding.entity = Some("synthetic:other:edge#999".into());
    finding.message = "edge synthetic:other:edge#999 misses by 3e-3".into();
    let second = Signature::new(&finding, "cadir", Stage::Check);
    assert_eq!(first.hash, "9655c8ece9402271");
    assert_eq!(first.hash, second.hash);
    assert_eq!(first.numbers.len(), 1);
    assert!(!first.reproduces(&finding));
    assert_eq!(first.hash.len(), 16);
    assert!(first
        .hash
        .bytes()
        .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte)));
}

#[test]
fn repro_signature_strips_quoted_names_and_paths() {
    let finding = Finding {
        check: Check::GeometricConsistency,
        severity: Severity::Error,
        entity: Some("synthetic:scope:edge#1".into()),
        related: Vec::new(),
        message: "carrier `PLANTED_NAME` from /PLANTED_PATH/file.cad misses by 2.0".into(),
    };
    let signature = Signature::new(&finding, "cadir", Stage::Check);
    assert!(!signature.message.contains("PLANTED"));
    assert_eq!(signature.numbers.len(), 1);
}

#[test]
fn repro_signature_normalizes_hex_ranges_and_decimal_exponents() {
    let finding = Finding {
        check: Check::NativeLinks,
        severity: Severity::Error,
        entity: None,
        related: Vec::new(),
        message: "offsets 0x10..0x20 differ by -1.5e+2".into(),
    };
    let signature = Signature::new(&finding, "f3d", Stage::Check);
    assert_eq!(
        signature.message,
        "offsets {number}..{number} differ by {number}"
    );
    assert_eq!(
        signature
            .numbers
            .iter()
            .map(|number| number.value)
            .collect::<Vec<_>>(),
        vec![16.0, 32.0, -150.0]
    );
}

#[test]
fn bundle_metadata_context_refuses_a_key_stated_twice() {
    let finding = Finding {
        check: Check::NativeLinks,
        severity: Severity::Error,
        entity: None,
        related: Vec::new(),
        message: "unresolved link".into(),
    };
    let mut context = Map::new();
    context.insert("tool".into(), Value::String("cli".into()));
    let metadata = BundleMetadata {
        signature: Signature::new(&finding, "cadir", Stage::Check),
        roots: Vec::new(),
        context,
        manifest: Vec::new(),
        incomplete_reason: None,
    };
    let wire = serde_json::to_string(&metadata).expect("metadata writes");
    let read: BundleMetadata = serde_json::from_str(&wire).expect("distinct keys read");
    assert_eq!(read, metadata);
    for (stated, repeated) in [
        (
            r#""context":{"tool":"cli"}"#,
            r#""context":{"tool":"cli","tool":"gui"}"#,
        ),
        (
            r#""context":{"tool":"cli"}"#,
            r#""context":{"tool":{"name":"a","name":"b"}}"#,
        ),
    ] {
        let tampered = wire.replace(stated, repeated);
        assert_ne!(tampered, wire);
        let error = serde_json::from_str::<BundleMetadata>(&tampered)
            .expect_err("a repeated context key is refused")
            .to_string();
        assert!(error.contains("duplicate key"), "{error}");
    }
}
