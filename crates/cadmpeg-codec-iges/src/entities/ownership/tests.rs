// SPDX-License-Identifier: Apache-2.0
#![allow(clippy::unwrap_used)]

use crate::test_support::test_owned::{owned_test_file, OwnedTestEntity};
use cadmpeg_ir::SourceGeometryRole;

#[test]
fn failed_trim_marks_its_support_before_any_neutral_owner_exists() {
    let support = || OwnedTestEntity {
        entity_type: 128,
        form: 0,
        label: "SUPPORT".into(),
        status: "00000000",
        parameters:
            "128,1,1,1,1,0,0,1,0,0,0,0,1,1,0,0,1,1,1,1,1,1,0,0,0,1,0,0,0,1,0,1,1,0,0,1,0,1;".into(),
    };
    let independent = crate::test_support::decode(owned_test_file(&[support()]));
    assert_eq!(
        independent.ir().model.surfaces[0]
            .source_object
            .as_ref()
            .unwrap()
            .geometry_role,
        Some(SourceGeometryRole::Independent)
    );
    let failed = crate::test_support::decode(owned_test_file(&[
        support(),
        OwnedTestEntity {
            entity_type: 144,
            form: 0,
            label: "TRIM".into(),
            status: "00000000",
            parameters: "144,1,2,0,0;".into(),
        },
    ]));
    assert!(failed.ir().model.faces.is_empty());
    assert_eq!(failed.ir().model.surfaces.len(), 1);
    assert_eq!(
        failed.ir().model.surfaces[0]
            .source_object
            .as_ref()
            .unwrap()
            .geometry_role,
        Some(SourceGeometryRole::Support)
    );
    assert!(!failed.report().losses.is_empty());
}

#[test]
fn logical_group_members_keep_their_independent_geometry_role() {
    let result = crate::test_support::decode(owned_test_file(&[
        OwnedTestEntity {
            entity_type: 110,
            form: 0,
            label: "LINE".into(),
            status: "00000000",
            parameters: "110,0,0,0,1,0,0;".into(),
        },
        OwnedTestEntity {
            entity_type: 402,
            form: 7,
            label: "GROUP".into(),
            status: "00000000",
            parameters: "402,1,1;".into(),
        },
    ]));
    assert_eq!(
        result.ir().model.curves[0]
            .source_object
            .as_ref()
            .unwrap()
            .geometry_role,
        Some(SourceGeometryRole::Independent)
    );
}

#[test]
fn declared_counts_cannot_make_support_scanning_escape_the_primary_span() {
    use crate::parameter::{ParameterRecord, Token, TokenValue};
    let values = [102, i64::MAX, 1, 3];
    let tokens = values
        .into_iter()
        .enumerate()
        .map(|(index, value)| Token {
            value: TokenValue::Integer(value),
            span: index..index + 1,
        })
        .collect();
    let record =
        ParameterRecord::from_test_tokens(5, 1..2, Vec::new(), values.len(), tokens, Vec::new());
    let owner = crate::test_support::directory_target(5, 102);
    let mut targets = Vec::new();
    super::visit_support_fields(&owner, &record, &mut |index| {
        targets.push(record.integer(index).unwrap());
        Ok(())
    })
    .unwrap();
    assert_eq!(targets, [1, 3]);
}

#[test]
fn unreadable_owner_flag_keeps_its_readable_support_pointer() {
    use crate::test_support::test_owned::{owned_test_file, OwnedTestEntity};
    use cadmpeg_ir::codec::{Codec, DecodeOptions};
    for claim in ["1E999", "bad", "3Hbad"] {
        let bytes = owned_test_file(&[
            OwnedTestEntity {
                entity_type: 108,
                form: 0,
                label: "PLANE".into(),
                status: "00000000",
                parameters: "108,0,0,1,0,0,0,0,0,0;".into(),
            },
            OwnedTestEntity {
                entity_type: 144,
                form: 0,
                label: "OWNER".into(),
                status: "00000000",
                parameters: format!("144,1,{claim},0;"),
            },
        ]);
        let result = crate::IgesCodec
            .decode(&mut std::io::Cursor::new(bytes), &DecodeOptions::default())
            .unwrap();
        assert_eq!(result.ir().model.surfaces.len(), 1);
        assert_eq!(
            result.ir().model.surfaces[0]
                .source_object
                .as_ref()
                .unwrap()
                .geometry_role,
            Some(cadmpeg_ir::SourceGeometryRole::Support)
        );
    }
}
