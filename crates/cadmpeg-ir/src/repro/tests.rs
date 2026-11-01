// SPDX-License-Identifier: Apache-2.0
use super::*;
use crate::report::{check::Check, Severity};

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
