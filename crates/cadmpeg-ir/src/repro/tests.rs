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

#[test]
fn compact_projection_writes_the_bytes_serde_json_writes() {
    let value = spellings();
    let expected = serde_json::to_vec(&value).unwrap();
    assert_eq!(compact_json(&value, usize::MAX).unwrap(), expected);
}

#[test]
fn compact_projection_refuses_one_byte_past_its_limit() {
    let value = spellings();
    let length = serde_json::to_vec(&value).unwrap().len();
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
    let first = Signature::new(&finding, crate::codec_id!("cadir"), Stage::Check);
    finding.entity = Some("synthetic:other:edge#999".into());
    finding.message = "edge synthetic:other:edge#999 misses by 3e-3".into();
    let second = Signature::new(&finding, crate::codec_id!("cadir"), Stage::Check);
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
    let signature = Signature::new(&finding, crate::codec_id!("cadir"), Stage::Check);
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
    let signature = Signature::new(&finding, crate::codec_id!("f3d"), Stage::Check);
    assert_eq!(
        signature.message,
        "offsets {number}..{number} differ by {number}"
    );
    assert_eq!(
        signature
            .numbers
            .iter()
            .map(|number| number.value.get())
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
    let context = AttributeJson::Object(BTreeMap::from([(
        "tool".to_owned(),
        AttributeJson::String("cli".into()),
    )]));
    let metadata = BundleMetadata {
        signature: Signature::new(&finding, crate::codec_id!("cadir"), Stage::Check),
        roots: Vec::new(),
        context,
        manifest: Vec::new(),
        incomplete_reason: None,
    };
    let wire = serde_json::to_string(&metadata).expect("metadata writes");
    let read: BundleMetadata = serde_json::from_str(&wire).expect("distinct keys read");
    assert_eq!(read, metadata);
    let tool = r#"{"tool":{"kind":"string","value":"cli"}}"#;
    for (stated, repeated) in [
        (
            tool,
            r#"{"tool":{"kind":"string","value":"cli"},"tool":{"kind":"string","value":"gui"}}"#,
        ),
        (
            tool,
            r#"{"tool":{"kind":"object","value":{"name":{"kind":"null"},"name":{"kind":"null"}}}}"#,
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

/// `Signature.check` is an open name, `codec` a codec id, and
/// `incomplete_reason` a `NonBlankString` (§2.7).
#[test]
fn bundle_metadata_reads_an_open_check_name_and_refuses_a_blank_reason() {
    let finding = Finding {
        check: Check::NativeLinks,
        severity: Severity::Error,
        entity: None,
        related: Vec::new(),
        message: "unresolved link".into(),
    };
    let metadata = BundleMetadata {
        signature: Signature::new(&finding, crate::codec_id!("cadir"), Stage::Check),
        roots: Vec::new(),
        context: AttributeJson::Object(BTreeMap::new()),
        manifest: Vec::new(),
        incomplete_reason: NonBlankString::new("bounded cut"),
    };
    let wire = serde_json::to_value(&metadata).unwrap();
    assert_eq!(wire["signature"]["check"], "native_links");
    let mut open = wire.clone();
    open["signature"]["check"] = "a_check_no_release_names_yet".into();
    let read: BundleMetadata = serde_json::from_value(open).unwrap();
    assert_eq!(
        read.signature.check.as_str(),
        "a_check_no_release_names_yet"
    );
    assert_eq!(read.signature.hash, metadata.signature.hash);
    for (field, value) in [("incomplete_reason", " "), ("codec", "Not A Codec")] {
        let mut refused = wire.clone();
        if field == "codec" {
            refused["signature"][field] = value.into();
        } else {
            refused[field] = value.into();
        }
        assert!(
            serde_json::from_value::<BundleMetadata>(refused).is_err(),
            "{field}"
        );
    }
}
