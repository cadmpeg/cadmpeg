// SPDX-License-Identifier: Apache-2.0
#![allow(clippy::unwrap_used)]

use std::io::Cursor;

use cadmpeg_ir::codec::{Codec, DecodeOptions};

use crate::directory::{DirectoryEntry, Status};
use crate::global::Dialect;
use crate::test_support::*;
use crate::IgesCodec;

use super::super::{
    flow_associativity_directory_valid, flow_connection_target_valid,
    flow_continuation_target_valid, flow_display_target_valid, type402_structure_valid,
};

#[test]
fn v4_flow_associativity_requires_entity_use_flag_three() {
    let entry = |form, use_flag, structure| DirectoryEntry {
        source_offset: 0,
        sequence: 1,
        entity_type: 402,
        parameter_start: 0,
        structure,
        line_font: 0,
        level: 0,
        view: 0,
        transform: 0,
        label_display: 0,
        status: Status {
            blank: 0,
            subordinate: 0,
            use_flag,
            hierarchy: 0,
        },
        line_weight: 0,
        color: 0,
        parameter_line_count: 0,
        form,
        reserved: [[b' '; 8]; 2],
        label: [b' '; 8],
        subscript: 0,
    };

    assert!(flow_associativity_directory_valid(
        &entry(18, 3, 0),
        Dialect::V4_0
    ));
    assert!(flow_associativity_directory_valid(
        &entry(18, 3, 99),
        Dialect::V4_0
    ));
    for use_flag in [0, 1, 2, 4, 5] {
        assert!(
            !flow_associativity_directory_valid(&entry(18, use_flag, 0), Dialect::V4_0),
            "{use_flag}"
        );
    }
    assert!(!flow_associativity_directory_valid(
        &entry(20, 3, 0),
        Dialect::V4_0
    ));
    assert!(flow_associativity_directory_valid(
        &entry(18, 2, 99),
        Dialect::V5_0
    ));
    assert!(flow_associativity_directory_valid(
        &entry(20, 2, 99),
        Dialect::V5_0
    ));
}

#[test]
fn decode_v4_flow_uses_the_table_use_flag_and_ignores_structure() {
    const GLOBAL_V4: &[u8] = b"1H,,1H;,7Hproduct,8Hpart.igs,7Hcadmpeg,3H0.1,32,38,6,308,15,7Hproduct,1.0,2,2HMM,1,1.0,13H260714.000000,0.001,1000.0,6Hauthor,3Horg,6,0;";
    let decode = |status: &'static str| {
        IgesCodec
            .decode(
                &mut Cursor::new(owned_test_file_with_global_and_directory_fields(
                    &[OwnedTestEntity {
                        entity_type: 402,
                        form: 18,
                        label: "FLOW".into(),
                        status,
                        parameters: "402,2,0,0,0,0,0,0,0,0;".into(),
                    }],
                    GLOBAL_V4,
                    &[],
                    &[],
                    &[],
                    &[],
                    &[(1, 99)],
                )),
                &DecodeOptions::default(),
            )
            .unwrap()
    };
    let valid = decode("00000300");
    assert!(!valid.report().losses.iter().any(|loss| {
        loss.message
            .contains("flow class counts, flags, typed links")
    }));
    let invalid = decode("00000200");
    assert!(invalid.report().losses.iter().any(|loss| {
        loss.message
            .contains("flow class counts, flags, typed links")
    }));
}

#[test]
fn v4_type402_structure_is_ignored_for_each_predefined_associativity_path() {
    let entry = |form, structure| DirectoryEntry {
        source_offset: 0,
        sequence: 1,
        entity_type: 402,
        parameter_start: 0,
        structure,
        line_font: 0,
        level: 0,
        view: 0,
        transform: 0,
        label_display: 0,
        status: Status {
            blank: 0,
            subordinate: 0,
            use_flag: 2,
            hierarchy: 0,
        },
        line_weight: 0,
        color: 0,
        parameter_line_count: 0,
        form,
        reserved: [[b' '; 8]; 2],
        label: [b' '; 8],
        subscript: 0,
    };

    for form in [2, 5, 6, 8, 9, 10, 11, 12, 13, 16] {
        assert!(
            type402_structure_valid(&entry(form, 99), Dialect::V4_0),
            "V4 form {form}"
        );
        assert!(
            !type402_structure_valid(&entry(form, 99), Dialect::V5_0),
            "V5 form {form}"
        );
        assert!(type402_structure_valid(&entry(form, 0), Dialect::V5_0));
    }
}

#[test]
fn v4_flow_uses_only_the_v4_target_classes() {
    let target = |entity_type, form| DirectoryEntry {
        source_offset: 0,
        sequence: 3,
        entity_type,
        parameter_start: 0,
        structure: 0,
        line_font: 0,
        level: 0,
        view: 0,
        transform: 0,
        label_display: 0,
        status: Status {
            blank: 0,
            subordinate: 0,
            use_flag: 0,
            hierarchy: 0,
        },
        line_weight: 0,
        color: 0,
        parameter_line_count: 0,
        form,
        reserved: [[b' '; 8]; 2],
        label: [b' '; 8],
        subscript: 0,
    };

    let connect_point = target(132, 0);
    let group = target(402, 1);
    let template = target(312, 0);
    let note = target(212, 0);
    let flow = target(402, 18);
    let old_connect_node = target(402, 11);

    assert!(flow_connection_target_valid(
        &connect_point,
        18,
        Dialect::V4_0
    ));
    assert!(!flow_connection_target_valid(&group, 18, Dialect::V4_0));
    assert!(flow_connection_target_valid(&group, 18, Dialect::V5_3));

    assert!(flow_display_target_valid(&template, 18, Dialect::V4_0));
    assert!(!flow_display_target_valid(&note, 18, Dialect::V4_0));
    assert!(flow_display_target_valid(&note, 18, Dialect::V5_3));

    assert!(flow_continuation_target_valid(&flow, 18, Dialect::V4_0));
    assert!(!flow_continuation_target_valid(
        &old_connect_node,
        18,
        Dialect::V4_0
    ));
    assert!(flow_continuation_target_valid(
        &old_connect_node,
        18,
        Dialect::V5_3
    ));
}

#[test]
fn decode_v4_type402_form2_ignores_nonzero_structure() {
    const GLOBAL_V4: &[u8] = b"1H,,1H;,7Hproduct,8Hpart.igs,7Hcadmpeg,3H0.1,32,38,6,308,15,0H,1.0,2,2HMM,1,1.0,13H260714.000000,0.001,1000.0,6Hauthor,3Horg,6,0;";
    let result = IgesCodec
        .decode(
            &mut Cursor::new(owned_test_file_with_global_and_directory_fields(
                &[OwnedTestEntity {
                    entity_type: 402,
                    form: 2,
                    label: "EXTLOGIC".into(),
                    status: "00000200",
                    parameters: "402,1,4HNAME,1;".into(),
                }],
                GLOBAL_V4,
                &[],
                &[],
                &[],
                &[],
                &[(1, 99)],
            )),
            &DecodeOptions::default(),
        )
        .unwrap();

    assert!(result.report().losses.iter().all(|loss| {
        loss.code != crate::loss::IgesLossCode::EntityNotProjected.kind()
            || !loss.message.contains("IGES entity type 402 form 2")
    }));
}
