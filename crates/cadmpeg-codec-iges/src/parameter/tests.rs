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
fn type102_entity_table_boundary_follows_declared_child_count() {
    let association = directory_target(1, 402);
    let first_child = directory_target(3, 110);
    let second_child = directory_target(5, 110);
    let composite = directory_target(7, 102);
    let directory = BTreeMap::from([
        (1, &association),
        (3, &first_child),
        (5, &second_child),
        (7, &composite),
    ]);
    let record = ParameterRecord {
        directory_sequence: 7,
        line_range: 1..2,
        bytes: Vec::new(),
        tokens: [102, 2, 3, 5, 1, 1, 0]
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
    let groups = analysis.groups.expect("Type 102 table boundary");
    assert_eq!(groups.token_start, 4);
    assert_eq!(groups.associations, vec![1]);
    assert!(groups.properties.is_empty());
}

#[test]
fn type102_malformed_count_does_not_enable_generic_recovery() {
    let association = directory_target(1, 402);
    let composite = directory_target(3, 102);
    let directory = BTreeMap::from([(1, &association), (3, &composite)]);
    let record = ParameterRecord {
        directory_sequence: 3,
        line_range: 1..2,
        bytes: Vec::new(),
        tokens: vec![
            Token {
                value: TokenValue::Integer(102),
                span: 0..0,
            },
            Token {
                value: TokenValue::Omitted,
                span: 0..0,
            },
            Token {
                value: TokenValue::Integer(1),
                span: 0..0,
            },
            Token {
                value: TokenValue::Integer(1),
                span: 0..0,
            },
            Token {
                value: TokenValue::Integer(1),
                span: 0..0,
            },
            Token {
                value: TokenValue::Integer(1),
                span: 0..0,
            },
            Token {
                value: TokenValue::Integer(0),
                span: 0..0,
            },
        ],
        parameter_end: 7,
        comment: Vec::new(),
    };

    let analysis = analyze_trailing_pointer_groups(&record, &directory);
    assert_eq!(analysis.candidate_count, 0);
    assert_eq!(analysis.valid_candidate_count, 0);
    assert!(analysis.groups.is_none());
}

#[test]
fn type402_group_entity_table_boundary_precedes_valid_generic_alternatives() {
    for form in [1_i64, 7, 14, 15] {
        let mut group = directory_target(1, 402);
        group.form = form;
        let member = directory_target(3, 116);
        let second_member = directory_target(5, 402);
        let trailing_association = directory_target(7, 402);
        let directory = BTreeMap::from([
            (1, &group),
            (3, &member),
            (5, &second_member),
            (7, &trailing_association),
        ]);
        let record = ParameterRecord {
            directory_sequence: 1,
            line_range: 1..2,
            bytes: Vec::new(),
            tokens: [402, 2, 3, 5, 1, 7, 0]
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
        assert_eq!(analysis.candidate_count, 1, "Form {form}");
        assert_eq!(analysis.valid_candidate_count, 1, "Form {form}");
        let groups = analysis.groups.expect("Type 402 table boundary");
        assert_eq!(groups.token_start, 4, "Form {form}");
        assert_eq!(groups.associations, vec![7], "Form {form}");
        assert!(groups.properties.is_empty(), "Form {form}");
    }
}

#[test]
fn type402_malformed_member_count_does_not_enable_generic_recovery() {
    let mut group = directory_target(1, 402);
    group.form = 7;
    let member = directory_target(3, 116);
    let second_member = directory_target(5, 402);
    let trailing_association = directory_target(7, 402);
    let directory = BTreeMap::from([
        (1, &group),
        (3, &member),
        (5, &second_member),
        (7, &trailing_association),
    ]);
    let record = ParameterRecord {
        directory_sequence: 1,
        line_range: 1..2,
        bytes: Vec::new(),
        tokens: vec![
            Token {
                value: TokenValue::Integer(402),
                span: 0..0,
            },
            Token {
                value: TokenValue::Omitted,
                span: 0..0,
            },
            Token {
                value: TokenValue::Integer(3),
                span: 0..0,
            },
            Token {
                value: TokenValue::Integer(5),
                span: 0..0,
            },
            Token {
                value: TokenValue::Integer(1),
                span: 0..0,
            },
            Token {
                value: TokenValue::Integer(7),
                span: 0..0,
            },
            Token {
                value: TokenValue::Integer(0),
                span: 0..0,
            },
        ],
        parameter_end: 7,
        comment: Vec::new(),
    };

    let analysis = analyze_trailing_pointer_groups(&record, &directory);
    assert_eq!(analysis.candidate_count, 0);
    assert_eq!(analysis.valid_candidate_count, 0);
    assert!(analysis.groups.is_none());
}

#[test]
fn type126_entity_table_boundary_uses_k_and_degree() {
    for (form, k, degree) in [(0_i64, 0_i64, 0_i64), (0, 1, 1), (3, 2, 1), (5, 3, 2)] {
        let association = directory_target(1, 402);
        let mut curve = directory_target(3, 126);
        curve.form = form;
        let directory = BTreeMap::from([(1, &association), (3, &curve)]);
        let expected_start =
            18 + usize::try_from(k).unwrap() * 5 + usize::try_from(degree).unwrap();
        let mut values = vec![0_i64; expected_start + 3];
        values[0] = 126;
        values[1] = k;
        values[2] = degree;
        values[expected_start] = 1;
        values[expected_start + 1] = 1;
        values[expected_start + 2] = 0;
        let record = ParameterRecord {
            directory_sequence: 3,
            line_range: 1..2,
            bytes: Vec::new(),
            tokens: values
                .into_iter()
                .map(|value| Token {
                    value: TokenValue::Integer(value),
                    span: 0..0,
                })
                .collect(),
            parameter_end: expected_start + 3,
            comment: Vec::new(),
        };

        let analysis = analyze_trailing_pointer_groups(&record, &directory);
        assert_eq!(analysis.candidate_count, 1, "Form {form}");
        assert_eq!(analysis.valid_candidate_count, 1, "Form {form}");
        let groups = analysis.groups.expect("Type 126 table boundary");
        assert_eq!(groups.token_start, expected_start, "Form {form}");
        assert_eq!(groups.associations, vec![1], "Form {form}");
        assert!(groups.properties.is_empty(), "Form {form}");
    }
}

#[test]
fn type126_entity_table_boundary_precedes_valid_generic_alternative() {
    let target_1 = directory_target(1, 402);
    let target_7 = directory_target(7, 402);
    let target_21 = directory_target(21, 402);
    let target_23 = directory_target(23, 402);
    let curve = directory_target(25, 126);
    let directory = BTreeMap::from([
        (1, &target_1),
        (7, &target_7),
        (21, &target_21),
        (23, &target_23),
        (25, &curve),
    ]);
    let record = ParameterRecord {
        directory_sequence: 25,
        line_range: 1..2,
        bytes: Vec::new(),
        tokens: [
            126, 1, 1, 0, 0, 1, 0, 18, 21, 23, 23, 21, 21, 21, 21, 21, 23, 21, 21, 21, 23, 1, 1, 1,
            1, 7, 0,
        ]
        .into_iter()
        .map(|value| Token {
            value: TokenValue::Integer(value),
            span: 0..0,
        })
        .collect(),
        parameter_end: 27,
        comment: Vec::new(),
    };

    let analysis = analyze_trailing_pointer_groups(&record, &directory);
    assert_eq!(analysis.candidate_count, 1);
    assert_eq!(analysis.valid_candidate_count, 1);
    let groups = analysis.groups.expect("Type 126 table boundary");
    assert_eq!(groups.token_start, 24);
    assert_eq!(groups.associations, vec![7]);
    assert!(groups.properties.is_empty());
}

#[test]
fn type126_malformed_k_or_degree_does_not_enable_generic_recovery() {
    let target_1 = directory_target(1, 402);
    let target_7 = directory_target(7, 402);
    let target_21 = directory_target(21, 402);
    let target_23 = directory_target(23, 402);
    let mut curve = directory_target(25, 126);
    curve.form = 0;
    let directory = BTreeMap::from([
        (1, &target_1),
        (7, &target_7),
        (21, &target_21),
        (23, &target_23),
        (25, &curve),
    ]);
    for (k, degree) in [(0_i64, 1_i64), (-1, 0), (1, -1)] {
        let mut values = [
            126, 1, 1, 0, 0, 1, 0, 18, 21, 23, 23, 21, 21, 21, 21, 21, 23, 21, 21, 21, 23, 1, 1, 1,
            1, 7, 0,
        ];
        values[1] = k;
        values[2] = degree;
        let record = ParameterRecord {
            directory_sequence: 25,
            line_range: 1..2,
            bytes: Vec::new(),
            tokens: values
                .into_iter()
                .map(|value| Token {
                    value: TokenValue::Integer(value),
                    span: 0..0,
                })
                .collect(),
            parameter_end: 27,
            comment: Vec::new(),
        };

        let analysis = analyze_trailing_pointer_groups(&record, &directory);
        assert_eq!(analysis.candidate_count, 0, "K={k}, M={degree}");
        assert_eq!(analysis.valid_candidate_count, 0, "K={k}, M={degree}");
        assert!(analysis.groups.is_none(), "K={k}, M={degree}");
    }
}

#[test]
fn type112_entity_table_boundary_uses_segment_count() {
    for (segment_count, expected_start) in [(1_i64, 31_usize), (2, 44)] {
        let association = directory_target(1, 402);
        let spline = directory_target(3, 112);
        let directory = BTreeMap::from([(1, &association), (3, &spline)]);
        let mut values = vec![0_i64; expected_start + 3];
        values[0] = 112;
        values[1] = 3;
        values[2] = 0;
        values[3] = 3;
        values[4] = segment_count;
        values[expected_start] = 1;
        values[expected_start + 1] = 1;
        values[expected_start + 2] = 0;
        let record = ParameterRecord {
            directory_sequence: 3,
            line_range: 1..2,
            bytes: Vec::new(),
            tokens: values
                .into_iter()
                .map(|value| Token {
                    value: TokenValue::Integer(value),
                    span: 0..0,
                })
                .collect(),
            parameter_end: expected_start + 3,
            comment: Vec::new(),
        };

        let analysis = analyze_trailing_pointer_groups(&record, &directory);
        assert_eq!(analysis.candidate_count, 1);
        assert_eq!(analysis.valid_candidate_count, 1);
        let groups = analysis.groups.expect("Type 112 table boundary");
        assert_eq!(groups.token_start, expected_start);
        assert_eq!(groups.associations, vec![1]);
        assert!(groups.properties.is_empty());
    }
}

#[test]
fn type112_entity_table_boundary_precedes_valid_generic_alternatives() {
    let target_1 = directory_target(1, 402);
    let target_7 = directory_target(7, 402);
    let target_15 = directory_target(15, 402);
    let target_17 = directory_target(17, 402);
    let target_39 = directory_target(39, 402);
    let spline = directory_target(41, 112);
    let directory = BTreeMap::from([
        (1, &target_1),
        (7, &target_7),
        (15, &target_15),
        (17, &target_17),
        (39, &target_39),
        (41, &spline),
    ]);
    let record = ParameterRecord {
        directory_sequence: 41,
        line_range: 1..2,
        bytes: Vec::new(),
        tokens: [
            112, 3, 0, 3, 1, 1, 3, 25, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 39, 17, 7, 1, 15, 17, 7, 1,
            15, 17, 7, 1, 1, 1, 0,
        ]
        .into_iter()
        .map(|value| Token {
            value: TokenValue::Integer(value),
            span: 0..0,
        })
        .collect(),
        parameter_end: 34,
        comment: Vec::new(),
    };

    let analysis = analyze_trailing_pointer_groups(&record, &directory);
    assert_eq!(analysis.candidate_count, 1);
    assert_eq!(analysis.valid_candidate_count, 1);
    let groups = analysis.groups.expect("Type 112 table boundary");
    assert_eq!(groups.token_start, 31);
    assert_eq!(groups.associations, vec![1]);
    assert!(groups.properties.is_empty());
}

#[test]
fn type112_malformed_segment_count_does_not_enable_generic_recovery() {
    let target_1 = directory_target(1, 402);
    let target_7 = directory_target(7, 402);
    let target_15 = directory_target(15, 402);
    let target_17 = directory_target(17, 402);
    let target_39 = directory_target(39, 402);
    let spline = directory_target(41, 112);
    let directory = BTreeMap::from([
        (1, &target_1),
        (7, &target_7),
        (15, &target_15),
        (17, &target_17),
        (39, &target_39),
        (41, &spline),
    ]);
    for segment_count in [0_i64, -1] {
        let mut values = [
            112, 3, 0, 3, 1, 1, 3, 25, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 39, 17, 7, 1, 15, 17, 7, 1,
            15, 17, 7, 1, 1, 1, 0,
        ];
        values[4] = segment_count;
        let record = ParameterRecord {
            directory_sequence: 41,
            line_range: 1..2,
            bytes: Vec::new(),
            tokens: values
                .into_iter()
                .map(|value| Token {
                    value: TokenValue::Integer(value),
                    span: 0..0,
                })
                .collect(),
            parameter_end: 34,
            comment: Vec::new(),
        };

        let analysis = analyze_trailing_pointer_groups(&record, &directory);
        assert_eq!(analysis.candidate_count, 0, "N={segment_count}");
        assert_eq!(analysis.valid_candidate_count, 0, "N={segment_count}");
        assert!(analysis.groups.is_none(), "N={segment_count}");
    }
}

#[test]
fn type106_entity_table_boundary_uses_interpretation_width() {
    let cases = [
        (1_i64, 1_i64, 2_usize, 2_usize),
        (2, 2, 2, 3),
        (3, 3, 2, 6),
        (11, 1, 2, 2),
        (12, 2, 2, 3),
        (13, 3, 2, 6),
        (20, 1, 2, 2),
        (21, 1, 2, 2),
        (31, 1, 2, 2),
        (32, 1, 2, 2),
        (33, 1, 2, 2),
        (34, 1, 2, 2),
        (35, 1, 2, 2),
        (36, 1, 2, 2),
        (37, 1, 2, 2),
        (38, 1, 2, 2),
        (40, 1, 3, 2),
        (63, 1, 2, 2),
    ];

    for (form, interpretation, tuple_count, tuple_width) in cases {
        let mut values = vec![106, interpretation, tuple_count as i64];
        if interpretation == 1 {
            values.push(0);
        }
        values.extend(std::iter::repeat_n(0, tuple_count * tuple_width));
        values.extend([1, 1, 0]);
        let expected_start = match interpretation {
            1 => 4 + tuple_count * 2,
            2 => 3 + tuple_count * 3,
            3 => 3 + tuple_count * 6,
            _ => unreachable!(),
        };
        let association = directory_target(1, 402);
        let mut copious = directory_target(3, 106);
        copious.form = form;
        let directory = BTreeMap::from([(1, &association), (3, &copious)]);
        let record = ParameterRecord {
            directory_sequence: 3,
            line_range: 1..2,
            bytes: Vec::new(),
            tokens: values
                .into_iter()
                .map(|value| Token {
                    value: TokenValue::Integer(value),
                    span: 0..0,
                })
                .collect(),
            parameter_end: expected_start + 3,
            comment: Vec::new(),
        };

        let analysis = analyze_trailing_pointer_groups(&record, &directory);
        assert_eq!(analysis.candidate_count, 1);
        assert_eq!(analysis.valid_candidate_count, 1);
        let groups = analysis.groups.expect("Type 106 table boundary");
        assert_eq!(groups.token_start, expected_start);
        assert_eq!(groups.associations, vec![1]);
        assert!(groups.properties.is_empty());
    }
}

#[test]
fn type106_form_interpretation_mismatch_does_not_enable_generic_recovery() {
    let association = directory_target(1, 402);
    let mut copious = directory_target(3, 106);
    copious.form = 11;
    let directory = BTreeMap::from([(1, &association), (3, &copious)]);
    let record = ParameterRecord {
        directory_sequence: 3,
        line_range: 1..2,
        bytes: Vec::new(),
        tokens: [106, 2, 1, 1, 1, 0]
            .into_iter()
            .map(|value| Token {
                value: TokenValue::Integer(value),
                span: 0..0,
            })
            .collect(),
        parameter_end: 6,
        comment: Vec::new(),
    };

    let analysis = analyze_trailing_pointer_groups(&record, &directory);
    assert_eq!(analysis.candidate_count, 0);
    assert_eq!(analysis.valid_candidate_count, 0);
    assert!(analysis.groups.is_none());
}

#[test]
fn type106_form63_rejects_nonplanar_interpretation_for_boundary_recovery() {
    let association = directory_target(1, 402);
    let mut copious = directory_target(3, 106);
    copious.form = 63;
    let directory = BTreeMap::from([(1, &association), (3, &copious)]);
    let record = ParameterRecord {
        directory_sequence: 3,
        line_range: 1..2,
        bytes: Vec::new(),
        tokens: [106, 2, 2, 0, 0, 0, 0, 0, 0, 1, 1, 0]
            .into_iter()
            .map(|value| Token {
                value: TokenValue::Integer(value),
                span: 0..0,
            })
            .collect(),
        parameter_end: 12,
        comment: Vec::new(),
    };

    let analysis = analyze_trailing_pointer_groups(&record, &directory);
    assert_eq!(analysis.candidate_count, 0);
    assert_eq!(analysis.valid_candidate_count, 0);
    assert!(analysis.groups.is_none());
}

#[test]
fn type116_entity_table_boundary_keeps_defaulted_display_pointer_slot() {
    let association = directory_target(1, 402);
    let point = directory_target(3, 116);
    let directory = BTreeMap::from([(1, &association), (3, &point)]);
    let token_values = [
        vec![116, 1, 2, 3, 0, 1, 1, 0]
            .into_iter()
            .map(TokenValue::Integer)
            .collect::<Vec<_>>(),
        vec![
            TokenValue::Integer(116),
            TokenValue::Integer(1),
            TokenValue::Integer(2),
            TokenValue::Integer(3),
            TokenValue::Omitted,
            TokenValue::Integer(1),
            TokenValue::Integer(1),
            TokenValue::Integer(0),
        ],
    ];

    for values in token_values {
        let record = ParameterRecord {
            directory_sequence: 3,
            line_range: 1..2,
            bytes: Vec::new(),
            tokens: values
                .into_iter()
                .map(|value| Token { value, span: 0..0 })
                .collect(),
            parameter_end: 8,
            comment: Vec::new(),
        };

        let analysis = analyze_trailing_pointer_groups(&record, &directory);
        assert_eq!(analysis.candidate_count, 1);
        assert_eq!(analysis.valid_candidate_count, 1);
        let groups = analysis.groups.expect("Type 116 table boundary");
        assert_eq!(groups.token_start, 5);
        assert_eq!(groups.associations, vec![1]);
        assert!(groups.properties.is_empty());
    }
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
fn decode_uses_type116_boundary_without_assigning_malformed_groups() {
    let result = IgesCodec
        .decode(
            &mut Cursor::new(owned_test_file(&[
                OwnedTestEntity {
                    entity_type: 116,
                    form: 0,
                    label: "BAD".into(),
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
    assert!(!result
        .report()
        .losses
        .iter()
        .any(|loss| loss.code == crate::loss::IgesLossCode::ParameterBoundaryAmbiguous.kind()));

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
fn decode_uses_type116_entity_boundary_for_explicit_and_omitted_display_pointer() {
    for parameters in ["116,1,2,3,0,1,1,0;", "116,1,2,3,,1,1,0;"] {
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
                        entity_type: 116,
                        form: 0,
                        label: "POINT".into(),
                        status: "00000000",
                        parameters: parameters.into(),
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
}

#[test]
fn decode_uses_type102_entity_boundary_for_form7_association() {
    let result = IgesCodec
        .decode(
            &mut Cursor::new(owned_test_file(&[
                OwnedTestEntity {
                    entity_type: 402,
                    form: 7,
                    label: "GROUP".into(),
                    status: "00000000",
                    parameters: "402,1,7;".into(),
                },
                OwnedTestEntity {
                    entity_type: 110,
                    form: 0,
                    label: "LINEA".into(),
                    status: "00010000",
                    parameters: "110,0,0,0,1,0,0;".into(),
                },
                OwnedTestEntity {
                    entity_type: 110,
                    form: 0,
                    label: "LINEB".into(),
                    status: "00010000",
                    parameters: "110,1,0,0,2,0,0;".into(),
                },
                OwnedTestEntity {
                    entity_type: 102,
                    form: 0,
                    label: "COMPOS".into(),
                    status: "00000000",
                    parameters: "102,2,3,5,1,1,0;".into(),
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
        .find(|record| record.id() == "iges:entity:directory#7")
        .unwrap();
    assert_eq!(
        source.fields()["association_links"].as_array().unwrap(),
        &[serde_json::json!("iges:entity:directory#1")]
    );
}

#[test]
fn decode_uses_type106_entity_boundary_for_form7_association() {
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
                    entity_type: 106,
                    form: 11,
                    label: "PATH".into(),
                    status: "00000000",
                    parameters: "106,1,2,0,0,0,1,0,1,1,0;".into(),
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

#[test]
fn decode_uses_type402_entity_boundary_for_group_forms() {
    for form in [1_i64, 7, 14, 15] {
        let result = IgesCodec
            .decode(
                &mut Cursor::new(owned_test_file(&[
                    OwnedTestEntity {
                        entity_type: 402,
                        form,
                        label: "GROUP".into(),
                        status: "00000000",
                        parameters: "402,2,3,5,1,7,0;".into(),
                    },
                    OwnedTestEntity {
                        entity_type: 116,
                        form: 0,
                        label: "MEMBER1".into(),
                        status: "00000000",
                        parameters: "116,0,0,0,0,1,1,0;".into(),
                    },
                    OwnedTestEntity {
                        entity_type: 402,
                        form: 7,
                        label: "MEMBER2".into(),
                        status: "00000000",
                        parameters: "402,1,3,1,1,0;".into(),
                    },
                    OwnedTestEntity {
                        entity_type: 402,
                        form: 7,
                        label: "ASSOC".into(),
                        status: "00000000",
                        parameters: "402,1,5;".into(),
                    },
                ])),
                &DecodeOptions::default(),
            )
            .unwrap();

        assert!(
            result.report().losses.is_empty(),
            "Form {form}: {:#?}",
            result.report().losses
        );
        let native = result.ir().native.namespace("iges").unwrap();
        let source = native.arenas["entities"]
            .iter()
            .find(|record| record.id() == "iges:entity:directory#1")
            .unwrap();
        assert_eq!(
            source.fields()["association_links"].as_array().unwrap(),
            &[serde_json::json!("iges:entity:directory#7")]
        );
        let group = native.arenas["groups"]
            .iter()
            .find(|record| record.fields()["source_entity"] == "iges:entity:directory#1")
            .unwrap();
        assert_eq!(group.fields()["declared_member_count"], 2);
        assert_eq!(
            group.fields()["members"].as_array().unwrap(),
            &[
                serde_json::json!("iges:entity:directory#3"),
                serde_json::json!("iges:entity:directory#5"),
            ]
        );
        assert_eq!(group.fields()["ordered"], matches!(form, 14 | 15));
        assert_eq!(
            group.fields()["back_pointers_required"],
            matches!(form, 1 | 14)
        );
    }
}

#[test]
fn decode_uses_type126_entity_boundary_for_form7_association() {
    for form in 0_i64..=5 {
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
                        entity_type: 126,
                        form,
                        label: "NURBS".into(),
                        status: "00010000",
                        parameters: "126,1,1,1,0,1,0,0,0,1,1,1,1,0,0,0,2,0,0,0,1,0,0,1,1,1,0;"
                            .into(),
                    },
                ])),
                &DecodeOptions::default(),
            )
            .unwrap();

        assert!(result.report().losses.is_empty(), "Form {form}");
        assert_eq!(result.ir().model.curves.len(), 1, "Form {form}");
        let source = result.ir().native.namespace("iges").unwrap().arenas["entities"]
            .iter()
            .find(|record| record.id() == "iges:entity:directory#3")
            .unwrap();
        assert_eq!(
            source.fields()["association_links"].as_array().unwrap(),
            &[serde_json::json!("iges:entity:directory#1")],
            "Form {form}"
        );
    }
}

#[test]
fn decode_uses_type112_entity_boundary_for_form7_association() {
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
                    entity_type: 112,
                    form: 0,
                    label: "SPLINE".into(),
                    status: "00010000",
                    parameters:
                        "112,3,0,3,1,1,3,25,1,1,1,1,1,1,1,1,1,1,1,39,17,7,1,15,17,7,1,15,17,7,1,1,1,0;"
                            .into(),
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
    assert_eq!(result.ir().model.curves.len(), 1);
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
fn type114_entity_table_boundary_uses_segment_dimensions() {
    for ((u_segments, v_segments), expected_start) in [((1_i64, 1_i64), 201_usize), ((2, 1), 298)] {
        let association = directory_target(1, 212);
        let surface = directory_target(3, 114);
        let directory = BTreeMap::from([(1, &association), (3, &surface)]);
        let mut values = vec![0_i64; expected_start + 3];
        values[0] = 114;
        values[1] = 3;
        values[2] = 1;
        values[3] = u_segments;
        values[4] = v_segments;
        values[expected_start] = 1;
        values[expected_start + 1] = 1;
        values[expected_start + 2] = 0;
        let record = ParameterRecord {
            directory_sequence: 3,
            line_range: 1..2,
            bytes: Vec::new(),
            tokens: values
                .into_iter()
                .map(|value| Token {
                    value: TokenValue::Integer(value),
                    span: 0..0,
                })
                .collect(),
            parameter_end: expected_start + 3,
            comment: Vec::new(),
        };

        let analysis = analyze_trailing_pointer_groups(&record, &directory);
        assert_eq!(analysis.candidate_count, 1);
        assert_eq!(analysis.valid_candidate_count, 1);
        let groups = analysis.groups.expect("Type 114 table boundary");
        assert_eq!(groups.token_start, expected_start);
        assert_eq!(groups.associations, vec![1]);
        assert!(groups.properties.is_empty());
    }
}

#[test]
fn type114_entity_table_boundary_precedes_valid_generic_alternative() {
    let association = directory_target(1, 212);
    let surface = directory_target(3, 114);
    let directory = BTreeMap::from([(1, &association), (3, &surface)]);
    let mut values = vec![1_i64; 204];
    values[0] = 114;
    values[1] = 3;
    values[2] = 1;
    values[3] = 1;
    values[4] = 1;
    values[5] = 0;
    values[6] = 1;
    values[7] = 0;
    values[8] = 1;
    values[9] = 193;
    values[203] = 0;
    let record = ParameterRecord {
        directory_sequence: 3,
        line_range: 1..2,
        bytes: Vec::new(),
        tokens: values
            .into_iter()
            .map(|value| Token {
                value: TokenValue::Integer(value),
                span: 0..0,
            })
            .collect(),
        parameter_end: 204,
        comment: Vec::new(),
    };

    let analysis = analyze_trailing_pointer_groups(&record, &directory);
    assert_eq!(analysis.candidate_count, 1);
    assert_eq!(analysis.valid_candidate_count, 1);
    let groups = analysis.groups.expect("Type 114 table boundary");
    assert_eq!(groups.token_start, 201);
    assert_eq!(groups.associations, vec![1]);
    assert!(groups.properties.is_empty());
}

#[test]
fn type114_malformed_segment_dimensions_do_not_enable_generic_recovery() {
    let association = directory_target(1, 212);
    let surface = directory_target(3, 114);
    let directory = BTreeMap::from([(1, &association), (3, &surface)]);
    for (u_segments, v_segments) in [(0_i64, 1_i64), (-1, 1), (1, 0)] {
        let mut values = vec![1_i64; 204];
        values[0] = 114;
        values[1] = 3;
        values[2] = 1;
        values[3] = u_segments;
        values[4] = v_segments;
        values[5] = 0;
        values[6] = 1;
        values[7] = 0;
        values[8] = 1;
        values[9] = 193;
        values[203] = 0;
        let record = ParameterRecord {
            directory_sequence: 3,
            line_range: 1..2,
            bytes: Vec::new(),
            tokens: values
                .into_iter()
                .map(|value| Token {
                    value: TokenValue::Integer(value),
                    span: 0..0,
                })
                .collect(),
            parameter_end: 204,
            comment: Vec::new(),
        };

        let analysis = analyze_trailing_pointer_groups(&record, &directory);
        assert_eq!(
            analysis.candidate_count, 0,
            "M={u_segments}, N={v_segments}"
        );
        assert_eq!(
            analysis.valid_candidate_count, 0,
            "M={u_segments}, N={v_segments}"
        );
        assert!(analysis.groups.is_none(), "M={u_segments}, N={v_segments}");
    }
}

#[test]
fn decode_uses_type114_entity_boundary_for_form7_association() {
    let mut values = vec!["114", "3", "1", "1", "1", "0", "1", "0", "1", "193"]
        .into_iter()
        .map(str::to_owned)
        .collect::<Vec<_>>();
    values.extend((0..191).map(|_| "1".to_owned()));
    values.extend(["1", "1", "0"].map(str::to_owned));
    let parameters = format!("{};", values.join(","));

    let result = IgesCodec
        .decode(
            &mut Cursor::new(owned_test_file(&[
                OwnedTestEntity {
                    entity_type: 212,
                    form: 0,
                    label: "TARGET".into(),
                    status: "00010100",
                    parameters: "212,1,1,1,1,1,1.5707963267948966,0,0,0,0,0,0,1HA;".into(),
                },
                OwnedTestEntity {
                    entity_type: 114,
                    form: 0,
                    label: "SPLSURF".into(),
                    status: "00000000",
                    parameters,
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
    assert_eq!(result.ir().model.surfaces.len(), 1);
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
fn type128_entity_table_boundary_uses_surface_indices() {
    for ((k1, k2, m1, m2), expected_start) in
        [((1_i64, 1_i64, 1_i64, 1_i64), 38_usize), ((2, 1, 1, 0), 46)]
    {
        let association = directory_target(1, 212);
        let mut surface = directory_target(3, 128);
        surface.form = 9;
        let directory = BTreeMap::from([(1, &association), (3, &surface)]);
        let mut values = vec![0_i64; expected_start + 3];
        values[0] = 128;
        values[1] = k1;
        values[2] = k2;
        values[3] = m1;
        values[4] = m2;
        values[expected_start] = 1;
        values[expected_start + 1] = 1;
        values[expected_start + 2] = 0;
        let record = ParameterRecord {
            directory_sequence: 3,
            line_range: 1..2,
            bytes: Vec::new(),
            tokens: values
                .into_iter()
                .map(|value| Token {
                    value: TokenValue::Integer(value),
                    span: 0..0,
                })
                .collect(),
            parameter_end: expected_start + 3,
            comment: Vec::new(),
        };

        let analysis = analyze_trailing_pointer_groups(&record, &directory);
        assert_eq!(analysis.candidate_count, 1);
        assert_eq!(analysis.valid_candidate_count, 1);
        let groups = analysis.groups.expect("Type 128 table boundary");
        assert_eq!(groups.token_start, expected_start);
        assert_eq!(groups.associations, vec![1]);
        assert!(groups.properties.is_empty());
    }
}

#[test]
fn type128_entity_table_boundary_precedes_valid_generic_alternative() {
    let target_1 = directory_target(1, 212);
    let target_3 = directory_target(3, 212);
    let mut surface = directory_target(5, 128);
    surface.form = 0;
    let directory = BTreeMap::from([(1, &target_1), (3, &target_3), (5, &surface)]);
    let values = [
        128, 1, 1, 1, 1, 1, 1, 0, 0, 0, 0, 1, 3, 4, 0, 1, 3, 4, 21, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1,
        1, 1, 1, 1, 1, 1, 3, 1, 3, 1, 1, 0,
    ];
    let record = ParameterRecord {
        directory_sequence: 5,
        line_range: 1..2,
        bytes: Vec::new(),
        tokens: values
            .into_iter()
            .map(|value| Token {
                value: TokenValue::Integer(value),
                span: 0..0,
            })
            .collect(),
        parameter_end: 41,
        comment: Vec::new(),
    };

    let analysis = analyze_trailing_pointer_groups(&record, &directory);
    assert_eq!(analysis.candidate_count, 1);
    assert_eq!(analysis.valid_candidate_count, 1);
    let groups = analysis.groups.expect("Type 128 table boundary");
    assert_eq!(groups.token_start, 38);
    assert_eq!(groups.associations, vec![1]);
    assert!(groups.properties.is_empty());
}

#[test]
fn type128_malformed_indices_do_not_enable_generic_recovery() {
    let target_1 = directory_target(1, 212);
    let target_3 = directory_target(3, 212);
    let mut surface = directory_target(5, 128);
    surface.form = 0;
    let directory = BTreeMap::from([(1, &target_1), (3, &target_3), (5, &surface)]);
    for (k1, k2, m1, m2) in [(0_i64, 1_i64, 1_i64, 1_i64), (-1, 1, 0, 0), (1, 0, 2, 0)] {
        let mut values = [
            128, 1, 1, 1, 1, 1, 1, 0, 0, 0, 0, 1, 3, 4, 0, 1, 3, 4, 21, 1, 1, 1, 1, 1, 1, 1, 1, 1,
            1, 1, 1, 1, 1, 1, 3, 1, 3, 1, 1, 0,
        ];
        values[1] = k1;
        values[2] = k2;
        values[3] = m1;
        values[4] = m2;
        let record = ParameterRecord {
            directory_sequence: 5,
            line_range: 1..2,
            bytes: Vec::new(),
            tokens: values
                .into_iter()
                .map(|value| Token {
                    value: TokenValue::Integer(value),
                    span: 0..0,
                })
                .collect(),
            parameter_end: 41,
            comment: Vec::new(),
        };

        let analysis = analyze_trailing_pointer_groups(&record, &directory);
        assert_eq!(
            analysis.candidate_count, 0,
            "K1={k1}, K2={k2}, M1={m1}, M2={m2}"
        );
        assert_eq!(
            analysis.valid_candidate_count, 0,
            "K1={k1}, K2={k2}, M1={m1}, M2={m2}"
        );
        assert!(
            analysis.groups.is_none(),
            "K1={k1}, K2={k2}, M1={m1}, M2={m2}"
        );
    }
}

#[test]
fn decode_uses_type128_entity_boundary_for_form7_association() {
    let values = [
        "128", "1", "1", "1", "1", "1", "1", "0", "0", "0", "0", "1", "3", "4", "0", "1", "3", "4",
        "21", "1", "1", "1", "1", "1", "1", "1", "1", "1", "1", "1", "1", "1", "1", "1", "1", "3",
        "1", "3", "1", "1", "0",
    ]
    .join(",");
    let result = IgesCodec
        .decode(
            &mut Cursor::new(owned_test_file(&[
                OwnedTestEntity {
                    entity_type: 212,
                    form: 0,
                    label: "TARGET1".into(),
                    status: "00010100",
                    parameters: "212,1,1,1,1,1,1.5707963267948966,0,0,0,0,0,0,1HA;".into(),
                },
                OwnedTestEntity {
                    entity_type: 212,
                    form: 0,
                    label: "TARGET3".into(),
                    status: "00010100",
                    parameters: "212,1,1,1,1,1,1.5707963267948966,0,0,0,0,0,0,1HA;".into(),
                },
                OwnedTestEntity {
                    entity_type: 128,
                    form: 0,
                    label: "NURBS".into(),
                    status: "00000000",
                    parameters: format!("{values};"),
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
    assert_eq!(result.ir().model.surfaces.len(), 1);
    let source = result.ir().native.namespace("iges").unwrap().arenas["entities"]
        .iter()
        .find(|record| record.id() == "iges:entity:directory#5")
        .unwrap();
    assert_eq!(
        source.fields()["association_links"].as_array().unwrap(),
        &[serde_json::json!("iges:entity:directory#1")]
    );
}

#[test]
fn type144_entity_table_boundary_uses_inner_boundary_count() {
    for (inner_count, expected_start) in [(0_i64, 5_usize), (1, 6)] {
        let association = directory_target(1, 212);
        let surface = directory_target(3, 144);
        let directory = BTreeMap::from([(1, &association), (3, &surface)]);
        let mut values = vec![0_i64; expected_start + 3];
        values[0] = 144;
        values[1] = 1;
        values[2] = i64::from(inner_count > 0);
        values[3] = inner_count;
        values[expected_start] = 1;
        values[expected_start + 1] = 1;
        values[expected_start + 2] = 0;
        let record = ParameterRecord {
            directory_sequence: 3,
            line_range: 1..2,
            bytes: Vec::new(),
            tokens: values
                .into_iter()
                .map(|value| Token {
                    value: TokenValue::Integer(value),
                    span: 0..0,
                })
                .collect(),
            parameter_end: expected_start + 3,
            comment: Vec::new(),
        };

        let analysis = analyze_trailing_pointer_groups(&record, &directory);
        assert_eq!(analysis.candidate_count, 1);
        assert_eq!(analysis.valid_candidate_count, 1);
        let groups = analysis.groups.expect("Type 144 table boundary");
        assert_eq!(groups.token_start, expected_start);
        assert_eq!(groups.associations, vec![1]);
        assert!(groups.properties.is_empty());
    }
}

#[test]
fn type144_entity_table_boundary_precedes_valid_generic_alternative() {
    let target_1 = directory_target(1, 212);
    let target_3 = directory_target(3, 212);
    let source = directory_target(5, 144);
    let directory = BTreeMap::from([(1, &target_1), (3, &target_3), (5, &source)]);
    let values = [144, 1, 1, 1, 3, 2, 1, 3, 0];
    let record = ParameterRecord {
        directory_sequence: 5,
        line_range: 1..2,
        bytes: Vec::new(),
        tokens: values
            .into_iter()
            .map(|value| Token {
                value: TokenValue::Integer(value),
                span: 0..0,
            })
            .collect(),
        parameter_end: values.len(),
        comment: Vec::new(),
    };

    let analysis = analyze_trailing_pointer_groups(&record, &directory);
    assert_eq!(analysis.candidate_count, 1);
    assert_eq!(analysis.valid_candidate_count, 1);
    let groups = analysis.groups.expect("Type 144 table boundary");
    assert_eq!(groups.token_start, 6);
    assert_eq!(groups.associations, vec![3]);
    assert!(groups.properties.is_empty());
}

#[test]
fn type144_malformed_inner_counts_do_not_enable_generic_recovery() {
    let association = directory_target(1, 212);
    let source = directory_target(3, 144);
    let directory = BTreeMap::from([(1, &association), (3, &source)]);
    for values in [
        vec![144, 1, 0, -1, 0, 1, 1, 0],
        vec![144, 1, 0, 100, 0, 1, 1, 0],
        vec![144, 1, 0],
    ] {
        let parameter_end = values.len();
        let record = ParameterRecord {
            directory_sequence: 3,
            line_range: 1..2,
            bytes: Vec::new(),
            tokens: values
                .into_iter()
                .map(|value| Token {
                    value: TokenValue::Integer(value),
                    span: 0..0,
                })
                .collect(),
            parameter_end,
            comment: Vec::new(),
        };

        let analysis = analyze_trailing_pointer_groups(&record, &directory);
        assert_eq!(analysis.candidate_count, 0);
        assert_eq!(analysis.valid_candidate_count, 0);
        assert!(analysis.groups.is_none());
    }
}

#[test]
fn decode_uses_type144_entity_boundary_for_form0_association() {
    let result = IgesCodec
        .decode(
            &mut Cursor::new(owned_test_file(&[
                OwnedTestEntity {
                    entity_type: 108,
                    form: 0,
                    label: "PLANE".into(),
                    status: "00010000",
                    parameters: "108,0,0,1,0,0,0,0,0,0;".into(),
                },
                OwnedTestEntity {
                    entity_type: 106,
                    form: 63,
                    label: "OUTMODEL".into(),
                    status: "00010000",
                    parameters: "106,1,5,0,0,0,1,0,1,1,0,1,0,0;".into(),
                },
                OwnedTestEntity {
                    entity_type: 106,
                    form: 63,
                    label: "OUTPCURV".into(),
                    status: "00010500",
                    parameters: "106,1,5,0,0,0,1,0,1,1,0,1,0,0;".into(),
                },
                OwnedTestEntity {
                    entity_type: 142,
                    form: 0,
                    label: "OUTBOUND".into(),
                    status: "00010000",
                    parameters: "142,0,1,5,3,3;".into(),
                },
                OwnedTestEntity {
                    entity_type: 106,
                    form: 63,
                    label: "INMODEL".into(),
                    status: "00010000",
                    parameters: "106,1,5,0,0.25,0.25,0.75,0.25,0.75,0.75,0.25,0.75,0.25,0.25;"
                        .into(),
                },
                OwnedTestEntity {
                    entity_type: 106,
                    form: 63,
                    label: "INPCURV".into(),
                    status: "00010500",
                    parameters: "106,1,5,0,0.25,0.25,0.75,0.25,0.75,0.75,0.25,0.75,0.25,0.25;"
                        .into(),
                },
                OwnedTestEntity {
                    entity_type: 142,
                    form: 0,
                    label: "INBOUND".into(),
                    status: "00010000",
                    parameters: "142,0,1,11,9,3;".into(),
                },
                OwnedTestEntity {
                    entity_type: 144,
                    form: 0,
                    label: "TRIMMED".into(),
                    status: "00000000",
                    parameters: "144,1,1,1,7,13,1,17,0;".into(),
                },
                OwnedTestEntity {
                    entity_type: 212,
                    form: 0,
                    label: "TARGET1".into(),
                    status: "00010100",
                    parameters: "212,1,1,1,1,1,1.5707963267948966,0,0,0,0,0,0,1HA;".into(),
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
    assert!(
        result.report().losses.is_empty(),
        "{:#?}",
        result.report().losses
    );
    assert_eq!(result.ir().model.faces.len(), 1);
    let source = result.ir().native.namespace("iges").unwrap().arenas["entities"]
        .iter()
        .find(|record| record.id() == "iges:entity:directory#15")
        .unwrap();
    assert_eq!(
        source.fields()["association_links"].as_array().unwrap(),
        &[serde_json::json!("iges:entity:directory#17")]
    );
}
