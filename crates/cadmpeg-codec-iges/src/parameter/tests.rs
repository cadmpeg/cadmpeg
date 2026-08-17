// SPDX-License-Identifier: Apache-2.0
#![allow(clippy::unwrap_used)]
#![allow(unused_imports)]

use std::collections::BTreeMap;
use std::fmt::Write as _;
use std::io::{self, Cursor, Read, Seek, SeekFrom};

use cadmpeg_core::decode::DecodeMode;
use cadmpeg_core::decode::ResourceDimension;
use cadmpeg_core::CodecError;
use cadmpeg_ir::codec::{Codec, CodecBackend, Confidence, DecodeOptions, EncodeInput, Encoder};
use cadmpeg_ir::geometry::{
    Curve, CurveGeometry, NurbsCurve, NurbsSurface, Pcurve, PcurveGeometry, Surface,
    SurfaceGeometry,
};
use cadmpeg_ir::ids::{
    BodyId, CoedgeId, CurveId, EdgeId, FaceId, LoopId, PcurveId, PointId, RegionId, ShellId,
    SurfaceId, VertexId,
};
use cadmpeg_ir::math::{Point2, Point3, Vector3};
use cadmpeg_ir::report::WritePath;
use cadmpeg_ir::topology::{
    Body, BodyKind, Coedge, Edge, Face, Loop, LoopBoundaryRole, Point, Region, Sense, Shell, Vertex,
};
use cadmpeg_ir::units::Units;
use cadmpeg_ir::CadIr;

use super::{
    analyze_trailing_pointer_groups, trailing_pointer_groups, ParameterRecord, Token, TokenValue,
};
use crate::directory::{DirectoryEntry, Status};
use crate::test_support::*;
use crate::{IgesCodec, IgesEncoder, IgesVersion, IgesWriteOptions};

fn directory_target(sequence: u32, entity_type: i64) -> DirectoryEntry {
    DirectoryEntry {
        source_offset: 0,
        sequence,
        entity_type,
        parameter_start: 1,
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
        parameter_line_count: 1,
        form: 0,
        reserved: [[b' '; 8]; 2],
        label: [b' '; 8],
        subscript: 0,
    }
}

#[test]
fn blank_parameter_field_is_an_omitted_value() {
    let result = IgesCodec
        .decode(
            &mut Cursor::new(owned_test_file(&[OwnedTestEntity {
                entity_type: 116,
                form: 0,
                label: "BLANK".into(),
                status: "00010000",
                parameters: "116,1,2,3,   ;".into(),
            }])),
            &DecodeOptions::default(),
        )
        .unwrap();

    assert_eq!(result.ir().model.points.len(), 1);
    let validation = cadmpeg_ir::validate_neutral(result.ir(), result.report().losses.clone());
    assert!(validation.is_ok(), "{validation:#?}");
}

#[test]
fn even_parameter_back_pointer_is_rejected_without_guessing_its_owner() {
    let mut bytes = owned_test_file(&[
        OwnedTestEntity {
            entity_type: 116,
            form: 0,
            label: "FIRST".into(),
            status: "00010000",
            parameters: "116,1,2,3,0;comment".into(),
        },
        OwnedTestEntity {
            entity_type: 116,
            form: 0,
            label: "SECOND".into(),
            status: "00010000",
            parameters: "116,4,5,6,0;".into(),
        },
    ]);
    let marker = bytes
        .windows(8)
        .position(|window| window == b"P      2")
        .expect("second Parameter Data card");
    let card_start = marker - 72;
    bytes[card_start + 64..card_start + 72].copy_from_slice(b"       2");

    let error = IgesCodec
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .unwrap_err();
    assert!(error.to_string().contains(
        "Parameter Data card P2 back-pointer 2 is not an owning odd Directory Entry sequence"
    ));
}

#[test]
fn zero_parameter_back_pointer_is_not_bound_to_the_first_directory_entry() {
    let mut bytes = owned_test_file(&[OwnedTestEntity {
        entity_type: 116,
        form: 0,
        label: "POINT".into(),
        status: "00010000",
        parameters: "116,1,2,3,0;".into(),
    }]);
    let marker = bytes
        .windows(8)
        .position(|window| window == b"P      1")
        .expect("Parameter Data card");
    let card_start = marker - 72;
    bytes[card_start + 64..card_start + 72].copy_from_slice(b"       0");

    let error = IgesCodec
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .unwrap_err();
    assert!(error.to_string().contains(
        "Parameter Data card P1 back-pointer 0 is not an owning odd Directory Entry sequence"
    ));
}

#[test]
fn parameter_card_count_must_equal_the_owned_contiguous_range() {
    let entity = OwnedTestEntity {
        entity_type: 116,
        form: 0,
        label: "POINT".into(),
        status: "00010000",
        parameters: format!("116,1,2,3,0;{}", "comment".repeat(12)),
    };
    let canonical = owned_test_file(&[entity]);

    for (declared, expected) in [
        (1, "declares 1 Parameter Data cards but owns 2"),
        (3, "declares 3 Parameter Data cards but owns 2"),
    ] {
        let mut bytes = canonical.clone();
        let marker = bytes
            .windows(8)
            .position(|window| window == b"D      2")
            .expect("second Directory Entry card");
        let card_start = marker - 72;
        bytes[card_start + 24..card_start + 32]
            .copy_from_slice(format!("{declared:>8}").as_bytes());

        let error = IgesCodec
            .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
            .unwrap_err();
        assert!(error.to_string().contains(expected), "{error}");
    }
}

#[test]
fn parameter_back_pointers_must_match_declared_ranges() {
    let mut bytes = owned_test_file(&[
        OwnedTestEntity {
            entity_type: 116,
            form: 0,
            label: "FIRST".into(),
            status: "00010000",
            parameters: "116,1,2,3,0;".into(),
        },
        OwnedTestEntity {
            entity_type: 116,
            form: 0,
            label: "SECOND".into(),
            status: "00010000",
            parameters: "116,4,5,6,0;".into(),
        },
    ]);
    let marker = bytes
        .windows(8)
        .position(|window| window == b"P      2")
        .expect("second Parameter Data card");
    let card_start = marker - 72;
    bytes[card_start + 64..card_start + 72].copy_from_slice(b"       1");

    let error = IgesCodec
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .unwrap_err();
    assert!(matches!(error, CodecError::Malformed(_)));
}

#[test]
fn parameter_card_count_includes_comment_card_payload() {
    let comment = "comment".repeat(12);
    let parameters = format!("116,1,2,3,0;{comment}");
    let result = IgesCodec
        .decode(
            &mut Cursor::new(owned_test_file(&[OwnedTestEntity {
                entity_type: 116,
                form: 0,
                label: "POINT".into(),
                status: "00010000",
                parameters,
            }])),
            &DecodeOptions::default(),
        )
        .unwrap();

    let entity = &result.ir().native.namespace("iges").unwrap().arenas["entities"][0];
    let fields = entity.fields();
    assert_eq!(fields["parameter_line_count"], 2);
    let retained_comment = fields["comment"].as_array().unwrap();
    assert_eq!(retained_comment.len(), 128 - "116,1,2,3,0;".len());
    let prefix = retained_comment
        .iter()
        .take(comment.len())
        .map(|value| value.as_u64().unwrap().try_into().unwrap())
        .collect::<Vec<u8>>();
    assert_eq!(prefix, comment.as_bytes());
}

#[test]
fn trailing_pointer_boundary_search_stays_linear_for_ambiguous_suffixes() {
    let token_count: usize = 4096;
    let mut tokens = (0..token_count)
        .map(|_| Token {
            value: TokenValue::Integer(0),
            span: 0..0,
        })
        .collect::<Vec<_>>();
    for index in (1..token_count.saturating_sub(2)).step_by(2) {
        tokens[index].value = TokenValue::Integer(0);
        tokens[index + 1].value = TokenValue::Integer((token_count - index - 3) as i64);
    }
    let record = ParameterRecord {
        directory_sequence: 1,
        line_range: 1..2,
        bytes: Vec::new(),
        tokens,
        parameter_end: token_count,
        comment: Vec::new(),
    };

    assert!(trailing_pointer_groups(&record, &BTreeMap::new()).is_none());
}

#[test]
fn counted_lists_can_use_a_defaulted_final_item_without_crossing_a_suffix() {
    let partial_final_item = ParameterRecord {
        directory_sequence: 1,
        line_range: 1..2,
        bytes: Vec::new(),
        tokens: std::iter::once(0)
            .chain(std::iter::once(1))
            .chain((0..19).map(|_| 0))
            .map(|value| Token {
                value: TokenValue::Integer(value),
                span: 0..0,
            })
            .collect(),
        parameter_end: 21,
        comment: Vec::new(),
    };
    assert_eq!(
        partial_final_item.count_with_stride_before_default_tail(1, 20, 21),
        Some(1)
    );

    let mut suffixed_tokens = vec![0, 2];
    suffixed_tokens.extend(std::iter::repeat_n(0, 20));
    suffixed_tokens.extend([1, 9]);
    let suffixed_parameter_end = suffixed_tokens.len();
    let suffixed = ParameterRecord {
        directory_sequence: 1,
        line_range: 1..2,
        bytes: Vec::new(),
        tokens: suffixed_tokens
            .into_iter()
            .map(|value| Token {
                value: TokenValue::Integer(value),
                span: 0..0,
            })
            .collect(),
        parameter_end: suffixed_parameter_end,
        comment: Vec::new(),
    };
    assert_eq!(
        suffixed.count_with_stride_before_default_tail(1, 20, 22),
        None
    );
}

#[test]
fn field_defaults_do_not_cross_the_selected_parameter_boundary() {
    let record = ParameterRecord {
        directory_sequence: 1,
        line_range: 1..2,
        bytes: Vec::new(),
        tokens: [106, 1, 2, 1, 9, 0]
            .into_iter()
            .map(|value| Token {
                value: TokenValue::Integer(value),
                span: 0..0,
            })
            .collect(),
        parameter_end: 4,
        comment: Vec::new(),
    };

    assert_eq!(record.integer_or(3, 7), Some(1));
    assert_eq!(record.integer_or(4, 7), None);
    assert_eq!(record.number_or(4, 7.0), None);
    assert_eq!(record.string_or_empty(4), None);
    assert_eq!(record.integer_or(99, 7), Some(7));
}

#[test]
fn unique_invalid_trailing_pointer_group_remains_visible() {
    let record = ParameterRecord {
        directory_sequence: 1,
        line_range: 1..2,
        bytes: Vec::new(),
        tokens: [116, 1, 99, 0]
            .into_iter()
            .map(|value| Token {
                value: TokenValue::Integer(value),
                span: 0..0,
            })
            .collect(),
        parameter_end: 4,
        comment: Vec::new(),
    };

    let analysis = analyze_trailing_pointer_groups(&record, &BTreeMap::new());
    assert_eq!(analysis.candidate_count, 1);
    let groups = analysis.groups.expect("unique structural group");
    assert!(!groups.fully_valid);
    assert_eq!(groups.association_pointers[0].raw_pointer, 99);
    assert!(groups.associations.is_empty());
}

#[test]
fn unique_valid_trailing_pointer_group_boundary_wins() {
    let first_association = directory_target(1, 402);
    let second_association = directory_target(3, 402);
    let directory = BTreeMap::from([(1, &first_association), (3, &second_association)]);
    let record = ParameterRecord {
        directory_sequence: 1,
        line_range: 1..2,
        bytes: Vec::new(),
        tokens: [116, 0, 0, 2, 3, 1, 0]
            .into_iter()
            .map(|value| Token {
                value: TokenValue::Integer(value),
                span: 0..0,
            })
            .collect(),
        parameter_end: 7,
        comment: Vec::new(),
    };

    let analysis = analyze_trailing_pointer_groups(&record, &directory);
    assert_eq!(analysis.candidate_count, 1);
    assert_eq!(analysis.valid_candidate_count, 1);
    let groups = analysis.groups.expect("unique valid group");
    assert_eq!(groups.token_start, 3);
    assert_eq!(groups.associations, vec![3, 1]);
    assert!(groups.properties.is_empty());
    assert_eq!(groups.association_pointers.len(), 2);
    assert_eq!(trailing_pointer_groups(&record, &directory), Some(groups));
}

#[test]
fn type123_entity_table_boundary_precedes_a_valid_generic_alternative() {
    let mut association = directory_target(1, 402);
    association.form = 7;
    let direction = directory_target(3, 123);
    let directory = BTreeMap::from([(1, &association), (3, &direction)]);
    let record = ParameterRecord {
        directory_sequence: 3,
        line_range: 1..2,
        bytes: Vec::new(),
        tokens: [123, 0, 0, 2, 1, 1, 0]
            .into_iter()
            .map(|value| Token {
                value: TokenValue::Integer(value),
                span: 0..0,
            })
            .collect(),
        parameter_end: 7,
        comment: Vec::new(),
    };

    let analysis = analyze_trailing_pointer_groups(&record, &directory);
    assert_eq!(analysis.candidate_count, 1);
    assert_eq!(analysis.valid_candidate_count, 1);
    let groups = analysis.groups.expect("Type 123 table boundary");
    assert_eq!(groups.token_start, 4);
    assert_eq!(groups.associations, vec![1]);
    assert!(groups.properties.is_empty());
}

#[test]
fn type110_entity_table_boundary_precedes_valid_generic_alternatives() {
    let first_association = directory_target(1, 402);
    let second_association = directory_target(3, 402);
    let line = directory_target(5, 110);
    let directory = BTreeMap::from([
        (1, &first_association),
        (3, &second_association),
        (5, &line),
    ]);
    let record = ParameterRecord {
        directory_sequence: 5,
        line_range: 1..2,
        bytes: Vec::new(),
        tokens: [110, 7, 3, 3, 1, 3, 3, 1, 3, 0]
            .into_iter()
            .map(|value| Token {
                value: TokenValue::Integer(value),
                span: 0..0,
            })
            .collect(),
        parameter_end: 10,
        comment: Vec::new(),
    };

    let analysis = analyze_trailing_pointer_groups(&record, &directory);
    assert_eq!(analysis.candidate_count, 1);
    assert_eq!(analysis.valid_candidate_count, 1);
    let groups = analysis.groups.expect("Type 110 table boundary");
    assert_eq!(groups.token_start, 7);
    assert_eq!(groups.associations, vec![3]);
    assert!(groups.properties.is_empty());
}

#[test]
fn multiple_valid_trailing_pointer_group_boundaries_are_ambiguous() {
    let association = directory_target(1, 402);
    let directory = BTreeMap::from([(1, &association)]);
    let record = ParameterRecord {
        directory_sequence: 1,
        line_range: 1..2,
        bytes: Vec::new(),
        tokens: [116, 0, 0, 2, 1, 1, 0]
            .into_iter()
            .map(|value| Token {
                value: TokenValue::Integer(value),
                span: 0..0,
            })
            .collect(),
        parameter_end: 7,
        comment: Vec::new(),
    };

    let analysis = analyze_trailing_pointer_groups(&record, &directory);
    assert_eq!(analysis.candidate_count, 2);
    assert_eq!(analysis.valid_candidate_count, 2);
    assert!(analysis.groups.is_none());
    assert!(trailing_pointer_groups(&record, &directory).is_none());
}

#[test]
fn decode_reports_ambiguous_boundary_without_assigning_groups() {
    let result = IgesCodec
        .decode(
            &mut Cursor::new(owned_test_file(&[
                OwnedTestEntity {
                    entity_type: 116,
                    form: 0,
                    label: "AMBIG".into(),
                    status: "00000000",
                    parameters: "116,4,3,3,3,3,0;".into(),
                },
                OwnedTestEntity {
                    entity_type: 402,
                    form: 0,
                    label: "ASSOC".into(),
                    status: "00000000",
                    parameters: "402;".into(),
                },
            ])),
            &DecodeOptions::default(),
        )
        .unwrap();

    assert_eq!(result.ir().model.points.len(), 1);
    let ambiguity_losses = result
        .report()
        .losses
        .iter()
        .filter(|loss| loss.code == crate::loss::IgesLossCode::ParameterBoundaryAmbiguous.kind())
        .collect::<Vec<_>>();
    assert_eq!(ambiguity_losses.len(), 1);
    assert!(ambiguity_losses[0].message.contains("2 equally valid"));
    assert_eq!(
        ambiguity_losses[0]
            .provenance
            .as_ref()
            .and_then(|provenance| provenance.tag.as_deref()),
        Some("directory_entry:D1")
    );

    let source = result.ir().native.namespace("iges").unwrap().arenas["entities"]
        .iter()
        .find(|record| record.id() == "iges:entity:directory#1")
        .unwrap();
    assert!(source.fields()["association_links"]
        .as_array()
        .unwrap()
        .is_empty());
    assert!(source.fields()["property_links"]
        .as_array()
        .unwrap()
        .is_empty());
    assert_eq!(source.fields()["parameters"].as_array().unwrap().len(), 7);
}

#[test]
fn decode_uses_type123_entity_boundary_for_form7_association() {
    let result = IgesCodec
        .decode(
            &mut Cursor::new(owned_test_file(&[
                OwnedTestEntity {
                    entity_type: 402,
                    form: 7,
                    label: "GROUP".into(),
                    status: "00000000",
                    parameters: "402,1,3;".into(),
                },
                OwnedTestEntity {
                    entity_type: 123,
                    form: 0,
                    label: "DIRECT".into(),
                    status: "00010000",
                    parameters: "123,0,0,2,1,1,0;".into(),
                },
            ])),
            &DecodeOptions::default(),
        )
        .unwrap();

    assert!(!result
        .report()
        .losses
        .iter()
        .any(|loss| loss.code == crate::loss::IgesLossCode::ParameterBoundaryAmbiguous.kind()));
    let source = result.ir().native.namespace("iges").unwrap().arenas["entities"]
        .iter()
        .find(|record| record.id() == "iges:entity:directory#3")
        .unwrap();
    assert_eq!(
        source.fields()["association_links"].as_array().unwrap(),
        &[serde_json::json!("iges:entity:directory#1")]
    );
}

#[test]
fn decode_uses_type110_entity_boundary_for_form7_association() {
    let result = IgesCodec
        .decode(
            &mut Cursor::new(owned_test_file(&[
                OwnedTestEntity {
                    entity_type: 402,
                    form: 7,
                    label: "GROUPA".into(),
                    status: "00000000",
                    parameters: "402,1,5;".into(),
                },
                OwnedTestEntity {
                    entity_type: 402,
                    form: 7,
                    label: "GROUPB".into(),
                    status: "00000000",
                    parameters: "402,1,5;".into(),
                },
                OwnedTestEntity {
                    entity_type: 110,
                    form: 0,
                    label: "LINE".into(),
                    status: "00010000",
                    parameters: "110,7,3,3,1,3,3,1,3,0;".into(),
                },
            ])),
            &DecodeOptions::default(),
        )
        .unwrap();

    assert!(!result
        .report()
        .losses
        .iter()
        .any(|loss| loss.code == crate::loss::IgesLossCode::ParameterBoundaryAmbiguous.kind()));
    let source = result.ir().native.namespace("iges").unwrap().arenas["entities"]
        .iter()
        .find(|record| record.id() == "iges:entity:directory#5")
        .unwrap();
    assert_eq!(
        source.fields()["association_links"].as_array().unwrap(),
        &[serde_json::json!("iges:entity:directory#3")]
    );
}
