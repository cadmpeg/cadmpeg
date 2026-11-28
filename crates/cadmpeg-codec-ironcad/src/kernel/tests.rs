// SPDX-License-Identifier: Apache-2.0

use cadmpeg_core::decode::View;

use super::parasolid_schema;
use crate::test_support::parasolid_prologue;

#[test]
fn parasolid_schema_follows_the_description() {
    let mut bytes = parasolid_prologue(
        ": TRANSMIT FILE created by modeller version 3401176",
        "SCH_3401176_34101_13006",
    );
    bytes.extend_from_slice(b"\xe7\x00\x00\x00");
    let schema = parasolid_schema(View::over_retained(&bytes)).unwrap();
    assert_eq!(schema.value(), "SCH_3401176_34101_13006");
}

#[test]
fn parasolid_prologue_without_the_lead_byte_is_rejected() {
    let mut bytes = parasolid_prologue("d", "SCH_1_13006");
    bytes[0] = b'P';
    assert!(parasolid_schema(View::over_retained(&bytes)).is_err());
}

#[test]
fn parasolid_schema_field_must_be_one_token() {
    let bytes = parasolid_prologue("d", "SCH_1_13006 x");
    assert!(parasolid_schema(View::over_retained(&bytes)).is_err());
    let mut bytes = parasolid_prologue("d", "SCH_1_13006");
    bytes.truncate(bytes.len() - 1);
    assert!(parasolid_schema(View::over_retained(&bytes)).is_err());
}
