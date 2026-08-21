// SPDX-License-Identifier: Apache-2.0

#![allow(clippy::unwrap_used)]

use std::io::Cursor;

use cadmpeg_ir::codec::{Codec, DecodeOptions};

use crate::loss::IgesLossCode;
use crate::test_support::*;
use crate::IgesCodec;

use super::Status;

#[test]
fn subordinate_switch_dependency_bits_follow_the_four_defined_values() {
    for (subordinate, physical, logical) in [
        (0, false, false),
        (1, true, false),
        (2, false, true),
        (3, true, true),
    ] {
        let status = Status {
            blank: 0,
            subordinate,
            use_flag: 0,
            hierarchy: 0,
        };
        assert_eq!(status.is_physically_dependent(), physical);
        assert_eq!(status.is_logically_dependent(), logical);
    }
}

#[test]
fn blank_directory_status_defaults_to_zero_fields() {
    let result = IgesCodec
        .decode(
            &mut Cursor::new(owned_test_file(&[OwnedTestEntity {
                entity_type: 116,
                form: 0,
                label: "BLANK".into(),
                status: "        ",
                parameters: "116,1,2,3,0;".into(),
            }])),
            &DecodeOptions::default(),
        )
        .unwrap();

    assert_eq!(result.ir().model.points.len(), 1);
    assert!(
        result.report().losses.is_empty(),
        "{:#?}",
        result.report().losses
    );
    let validation = cadmpeg_ir::validate_neutral(result.ir(), Vec::new());
    assert!(validation.is_ok(), "{validation:#?}");
}

#[test]
fn eight_digit_directory_status_supplies_four_two_digit_fields() {
    let result = IgesCodec
        .decode(
            &mut Cursor::new(owned_test_file(&[OwnedTestEntity {
                entity_type: 116,
                form: 0,
                label: "STATUS".into(),
                status: "01020304",
                parameters: "116,1,2,3,0;".into(),
            }])),
            &DecodeOptions::default(),
        )
        .unwrap();
    let entity = &result.ir().native.namespace("iges").unwrap().arenas["entities"][0];

    assert_eq!(entity.fields()["blank_status"], 1);
    assert_eq!(entity.fields()["subordinate_status"], 2);
    assert_eq!(entity.fields()["use_flag"], 3);
    assert_eq!(entity.fields()["hierarchy_status"], 4);
}

#[test]
fn a_nonblank_space_in_the_status_number_quarantines_the_record() {
    for status in ["     201", "0000 201", "0000020 "] {
        let result = IgesCodec
            .decode(
                &mut Cursor::new(owned_test_file(&[OwnedTestEntity {
                    entity_type: 116,
                    form: 0,
                    label: "STATUS".into(),
                    status,
                    parameters: "116,1,2,3,0;".into(),
                }])),
                &DecodeOptions::default(),
            )
            .unwrap();

        let native = result.ir().native.namespace("iges").unwrap();
        let quarantined = &native.arenas["quarantined_directory_records"];
        let losses = &result.report().losses;
        assert!(native.arenas["entities"].is_empty(), "{status}");
        assert_eq!(quarantined.len(), 1, "{status}");
        assert_eq!(quarantined[0].fields()["defect"], "status-number-invalid");
        assert_eq!(losses.len(), 1, "{status}: {losses:#?}");
        assert_eq!(
            losses[0].code,
            IgesLossCode::DirectoryRecordQuarantined.kind()
        );
    }
}

#[test]
fn inspect_reports_directory_entity_and_form_census() {
    let bytes = point_file();

    let summary = IgesCodec
        .inspect(
            &mut Cursor::new(bytes),
            &cadmpeg_core::decode::InspectOptions::default(),
        )
        .unwrap();

    assert!(summary.notes.contains(&"entities=1".into()));
    assert!(summary.notes.contains(&"entity.116.form.0=1".into()));
    assert!(summary.notes.contains(&"parameter_records=1".into()));
    assert!(summary.notes.contains(&"parameter_tokens=4".into()));
}

#[test]
fn decode_treats_subordinate_switch_three_as_physically_dependent() {
    let result = IgesCodec
        .decode(
            &mut Cursor::new(direction_file()),
            &DecodeOptions::default(),
        )
        .unwrap();

    assert!(!result.report().geometry_transferred);
    assert_eq!(result.report().losses.len(), 1);
    let loss = &result.report().losses[0];
    assert_eq!(loss.code, IgesLossCode::EntityRetainedUnprojected.kind());
    assert_eq!(
        loss.provenance
            .as_ref()
            .and_then(|provenance| provenance.tag.as_deref()),
        Some("directory_entry:D1")
    );
    let native = result.ir().native.namespace("iges").unwrap();
    assert_eq!(native.arenas["directions"].len(), 1);
    let direction_fields = native.arenas["directions"][0].fields();
    let components = direction_fields["components"].as_array().unwrap();
    assert_eq!(components[0], 2.0);
    assert_eq!(components[1], -3.0);
    assert_eq!(components[2], 4.0);
    assert_eq!(
        native.arenas["directions"][0].fields()["physically_dependent"],
        true
    );
    let validation = cadmpeg_ir::validate_neutral(result.ir(), Vec::new());
    assert!(validation.is_ok(), "{:#?}", validation.findings);
}
