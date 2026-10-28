// SPDX-License-Identifier: Apache-2.0

use super::{validate, Entry, Value};
use cadmpeg_core::decode::View;

type TestResult = Result<(), Box<dyn std::error::Error>>;

#[test]
fn tagged_entries_exhaust_their_variable_width_table() -> TestResult {
    let mut bytes = Vec::new();
    for tag in [1_u8, 2, 3] {
        bytes.extend_from_slice(&7_u16.to_le_bytes());
        bytes.push(tag);
        bytes.extend_from_slice(if tag == 3 { &[0; 8] } else { &[0; 16] });
    }
    validate(View::over_retained(&bytes), 3)?;
    let mut view = View::over_retained(&bytes);
    for tag in [1, 2, 3] {
        let entry = Entry::read(&mut view)?;
        assert_eq!(entry.key, 7);
        assert_eq!(entry.encoded_len(), if tag == 3 { 11 } else { 19 });
        assert!(matches!(
            (tag, &entry.value),
            (1, Value::Identifier1(_)) | (2, Value::Identifier2(_)) | (3, Value::Pair3(_))
        ));
        let wire = serde_json::to_value(&entry)?;
        assert_eq!(serde_json::from_value::<Entry>(wire)?, entry);
    }
    assert!(view.is_empty());
    Ok(())
}

#[test]
fn table_rejects_wrong_counts_and_trailing_bytes() {
    assert!(validate(View::over_retained(&[]), 0).is_ok());
    let mut bytes = [0_u8; 19];
    bytes[2] = 2;
    for end in 0..bytes.len() {
        assert!(validate(View::over_retained(&bytes[..end]), 1).is_err());
    }
    assert!(validate(View::over_retained(&bytes), 0).is_err());
    assert!(validate(View::over_retained(&bytes), 2).is_err());
    let mut trailing = [0_u8; 20];
    trailing[2] = 2;
    assert!(validate(View::over_retained(&trailing), 1).is_err());
}

#[test]
fn fixed_entry_length_retains_untagged_tables() {
    for tag in [0_u8, 3, 4, u8::MAX] {
        let mut bytes = [0_u8; 38];
        bytes[2] = tag;
        bytes[21] = tag;
        assert!(validate(View::over_retained(&bytes), 2).is_ok());
        assert!(super::is_fixed_entry_form(View::over_retained(&bytes), 2));
        for end in [11, 19, 37] {
            assert!(validate(View::over_retained(&bytes[..end]), 2).is_err());
        }
    }
    let mut tagged = [0_u8; 11];
    tagged[2] = 3;
    assert!(validate(View::over_retained(&tagged), 1).is_ok());
    assert!(!super::is_fixed_entry_form(View::over_retained(&tagged), 1));
}

#[test]
fn native_entries_reject_unknown_forms_and_wrong_identifier_widths() {
    for form in ["identifier1", "identifier2"] {
        for value in ["", "00", "zz", "0000000000000000000000000000000000ff"] {
            let wire = serde_json::json!({"key": 7, "value": {"form": form, "value": value}});
            assert!(serde_json::from_value::<Entry>(wire).is_err());
        }
    }
    for wire in [
        serde_json::json!({"key": 7, "value": {"form": "unknown", "value": [0, 0]}}),
        serde_json::json!({"key": 7, "value": {"form": "pair3", "value": [0]}}),
    ] {
        assert!(serde_json::from_value::<Entry>(wire).is_err());
    }
}
