// SPDX-License-Identifier: Apache-2.0
#![allow(clippy::unwrap_used)]

use std::io::Cursor;

use cadmpeg_ir::codec::{Codec, DecodeOptions};

use crate::loss::IgesLossCode;
use crate::test_support::*;
use crate::IgesCodec;

#[test]
fn associativity_definition_ignores_unrelated_directory_fields() {
    let result = IgesCodec
        .decode(
            &mut Cursor::new(owned_test_file_with_directory_fields(
                &[OwnedTestEntity {
                    entity_type: 302,
                    form: 5001,
                    label: "DEFIN".into(),
                    status: "00000200",
                    parameters: "302,1,1,1,1,1;".into(),
                }],
                &[(1, 9)],
                &[(1, 7)],
                &[(1, 4)],
                &[(1, 3)],
                &[(1, 8)],
            )),
            &DecodeOptions::default(),
        )
        .unwrap();

    assert!(!result.report().losses.iter().any(|loss| {
        loss.code == IgesLossCode::EntityNotProjected.kind()
            && loss.message.contains("associativity definition")
    }));
}

#[test]
fn units_data_ignores_unrelated_directory_fields() {
    let result = IgesCodec
        .decode(
            &mut Cursor::new(owned_test_file_with_directory_fields(
                &[OwnedTestEntity {
                    entity_type: 316,
                    form: 0,
                    label: "UNITS".into(),
                    status: "00000200",
                    parameters: "316,1,6HLENGTH,2HKN,1852;".into(),
                }],
                &[(1, 9)],
                &[(1, 7)],
                &[(1, 4)],
                &[(1, 3)],
                &[(1, 8)],
            )),
            &DecodeOptions::default(),
        )
        .unwrap();

    assert!(!result.report().losses.iter().any(|loss| {
        loss.code == IgesLossCode::EntityNotProjected.kind() && loss.message.contains("units")
    }));
}
