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
    analyze_trailing_pointer_groups, groups_for_candidate, structural_pointer_group_candidates,
    trailing_pointer_groups, ParameterRecord, Token, TokenValue,
};
use crate::directory::{DirectoryEntry, Status};
use crate::test_support::*;
use crate::{IgesCodec, IgesEncoder, IgesVersion, IgesWriteOptions};

impl From<i64> for TokenValue {
    fn from(value: i64) -> Self {
        Self::Integer(value)
    }
}

impl From<f64> for TokenValue {
    fn from(value: f64) -> Self {
        Self::Real(value)
    }
}

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

fn integer_parameter_record(sequence: u32, values: &[i64]) -> ParameterRecord {
    ParameterRecord {
        directory_sequence: sequence,
        line_range: 1..2,
        bytes: Vec::new(),
        tokens: values
            .iter()
            .copied()
            .map(|value| Token {
                value: TokenValue::Integer(value),
                span: 0..0,
            })
            .collect(),
        parameter_end: values.len(),
        comment: Vec::new(),
    }
}

fn token_parameter_record(sequence: u32, values: Vec<TokenValue>) -> ParameterRecord {
    let parameter_end = values.len();
    ParameterRecord {
        directory_sequence: sequence,
        line_range: 1..2,
        bytes: Vec::new(),
        tokens: values
            .into_iter()
            .map(|value| Token { value, span: 0..0 })
            .collect(),
        parameter_end,
        comment: Vec::new(),
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
fn type402_single_parent_boundary_follows_child_count() {
    for (values, expected_start) in [
        (vec![402, 1, 1, 3, 5, 1, 9, 0], 5),
        (vec![402, 1, 2, 3, 5, 7, 1, 9, 0], 6),
    ] {
        let mut source = directory_target(1, 402);
        source.form = 9;
        let parent = directory_target(3, 212);
        let child = directory_target(5, 212);
        let second_child = directory_target(7, 212);
        let trailing_association = directory_target(9, 212);
        let directory = BTreeMap::from([
            (1, &source),
            (3, &parent),
            (5, &child),
            (7, &second_child),
            (9, &trailing_association),
        ]);
        let parameter_end = values.len();
        let record = ParameterRecord {
            directory_sequence: 1,
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
        assert_eq!(analysis.candidate_count, 1);
        assert_eq!(analysis.valid_candidate_count, 1);
        let groups = analysis.groups.expect("Type 402 Form 9 table boundary");
        assert_eq!(groups.token_start, expected_start);
        assert_eq!(groups.associations, vec![9]);
        assert!(groups.properties.is_empty());
    }
}

#[test]
fn type402_single_parent_boundary_precedes_valid_generic_alternative() {
    let mut source = directory_target(5, 402);
    source.form = 9;
    let parent = directory_target(1, 212);
    let child = directory_target(3, 212);
    let directory = BTreeMap::from([(1, &parent), (3, &child), (5, &source)]);
    let values = [402, 1, 2, 1, 1, 2, 1, 3, 0];
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
    let groups = analysis.groups.expect("Type 402 Form 9 table boundary");
    assert_eq!(groups.token_start, 6);
    assert_eq!(groups.associations, vec![3]);
    assert!(groups.properties.is_empty());
}

#[test]
fn type402_single_parent_malformed_counts_do_not_enable_generic_recovery() {
    let mut source = directory_target(5, 402);
    source.form = 9;
    let target = directory_target(1, 212);
    let directory = BTreeMap::from([(1, &target), (5, &source)]);
    let cases = [
        vec![402, 0, 1, 1, 1, 1, 1, 0],
        vec![402, 1, 0, 1, 1, 1, 1, 0],
        vec![402, 1, -1, 1, 1, 1, 1, 0],
        vec![402, 1, 100, 1, 1, 1, 1, 0],
        vec![402, 1, 1],
    ];

    for values in cases {
        let parameter_end = values.len();
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
fn type230_entity_table_boundary_follows_island_count() {
    for (island_count, expected_start) in [(0_i64, 9), (1, 10), (2, 11)] {
        let association = directory_target(1, 212);
        let source = directory_target(3, 230);
        let directory = BTreeMap::from([(1, &association), (3, &source)]);
        let mut values = vec![0_i64; expected_start + 3];
        values[0] = 230;
        values[1] = 1;
        values[2] = 2;
        values[8] = island_count;
        for index in 0..usize::try_from(island_count).unwrap() {
            values[9 + index] = 1;
        }
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
        assert_eq!(analysis.candidate_count, 1, "N={island_count}");
        assert_eq!(analysis.valid_candidate_count, 1, "N={island_count}");
        let groups = analysis.groups.expect("Type 230 table boundary");
        assert_eq!(groups.token_start, expected_start, "N={island_count}");
        assert_eq!(groups.associations, vec![1], "N={island_count}");
        assert!(groups.properties.is_empty(), "N={island_count}");
    }
}

#[test]
fn type230_entity_table_boundary_precedes_valid_generic_alternative() {
    let target_1 = directory_target(1, 212);
    let target_3 = directory_target(3, 212);
    let source = directory_target(5, 230);
    let directory = BTreeMap::from([(1, &target_1), (3, &target_3), (5, &source)]);
    let values = [230, 7, 2, 0, 0, 0, 1, 0, 1, 2, 1, 3, 0];
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
    let groups = analysis.groups.expect("Type 230 table boundary");
    assert_eq!(groups.token_start, 10);
    assert_eq!(groups.associations, vec![3]);
    assert!(groups.properties.is_empty());
}

#[test]
fn type230_malformed_island_counts_do_not_enable_generic_recovery() {
    let target_1 = directory_target(1, 212);
    let target_5 = directory_target(5, 212);
    let source = directory_target(3, 230);
    let directory = BTreeMap::from([(1, &target_1), (3, &source), (5, &target_5)]);
    let cases = [
        vec![230, 1, 2, 0, 0, 0, 1, 0, -1, 1, 5, 0],
        vec![230, 1, 2, 0, 0, 0, 1, 0, 100, 1, 5, 0],
        vec![230, 1, 2],
        vec![230, 1, 2, 0, 0, 0, 1, 0, 2, 1],
    ];

    for values in cases {
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
fn type132_fixed_primary_boundary_follows_fourteen_fields() {
    let association = directory_target(3, 212);
    let mut source = directory_target(7, 132);
    source.form = 0;
    let directory = BTreeMap::from([(3, &association), (7, &source)]);
    let record = token_parameter_record(
        7,
        vec![
            132.into(),
            1.0.into(),
            2.0.into(),
            3.0.into(),
            0.into(),
            101.into(),
            1.into(),
            TokenValue::String(b"C1".to_vec()),
            0.into(),
            TokenValue::String(b"PORT".to_vec()),
            0.into(),
            42.into(),
            1.into(),
            0.into(),
            0.into(),
            1.into(),
            3.into(),
            0.into(),
        ],
    );

    let analysis = analyze_trailing_pointer_groups(&record, &directory);
    assert_eq!(analysis.candidate_count, 1);
    assert_eq!(analysis.valid_candidate_count, 1);
    let groups = analysis.groups.expect("Type 132 table boundary");
    assert_eq!(groups.token_start, 15);
    assert_eq!(groups.associations, vec![3]);
    assert!(groups.properties.is_empty());
}

#[test]
fn type132_table_boundary_precedes_valid_generic_alternative() {
    let association_1 = directory_target(1, 212);
    let association_3 = directory_target(3, 212);
    let property_5 = directory_target(5, 406);
    let property_7 = directory_target(7, 406);
    let mut source = directory_target(9, 132);
    source.form = 0;
    let directory = BTreeMap::from([
        (1, &association_1),
        (3, &association_3),
        (5, &property_5),
        (7, &property_7),
        (9, &source),
    ]);
    let record = token_parameter_record(
        9,
        vec![
            132.into(),
            1.0.into(),
            2.0.into(),
            3.0.into(),
            0.into(),
            101.into(),
            1.into(),
            TokenValue::String(b"C1".to_vec()),
            0.into(),
            TokenValue::String(b"PORT".to_vec()),
            0.into(),
            42.into(),
            1.into(),
            0.into(),
            2.into(),
            1.into(),
            3.into(),
            2.into(),
            5.into(),
            7.into(),
        ],
    );

    let generic = structural_pointer_group_candidates(&record);
    assert_eq!(
        generic
            .iter()
            .map(|candidate| candidate.token_start)
            .collect::<Vec<_>>(),
        vec![14, 15]
    );
    let analysis = analyze_trailing_pointer_groups(&record, &directory);
    assert_eq!(analysis.candidate_count, 1);
    assert_eq!(analysis.valid_candidate_count, 1);
    let groups = analysis.groups.expect("Type 132 table boundary");
    assert_eq!(groups.token_start, 15);
    assert_eq!(groups.associations, vec![3]);
    assert_eq!(groups.properties, vec![5, 7]);
}

#[test]
fn type132_complete_wrong_fields_keep_boundary_and_truncated_spans_do_not_recover() {
    let association = directory_target(3, 212);
    let mut source = directory_target(7, 132);
    source.form = 0;
    let directory = BTreeMap::from([(3, &association), (7, &source)]);
    let wrong_fields = token_parameter_record(
        7,
        vec![
            132.into(),
            TokenValue::String(b"bad".to_vec()),
            2.into(),
            3.into(),
            0.into(),
            999.into(),
            1.into(),
            TokenValue::String(b"C1".to_vec()),
            0.into(),
            TokenValue::String(b"PORT".to_vec()),
            0.into(),
            42.into(),
            1.into(),
            0.into(),
            0.into(),
            1.into(),
            3.into(),
            0.into(),
        ],
    );
    let analysis = analyze_trailing_pointer_groups(&wrong_fields, &directory);
    assert_eq!(analysis.candidate_count, 1);
    assert_eq!(analysis.valid_candidate_count, 1);
    assert_eq!(
        analysis
            .groups
            .expect("Type 132 table boundary")
            .token_start,
        15
    );

    for values in [
        vec![132, 1, 2, 3, 0, 101, 1, 0, 0, 0, 0, 42, 1, 0],
        vec![132, 1, 2, 3, 0, 101, 1, 0, 0, 0, 0, 42, 1, 0, 0, 1, 3],
    ] {
        let analysis =
            analyze_trailing_pointer_groups(&integer_parameter_record(7, &values), &directory);
        assert_eq!(analysis.candidate_count, 0, "values={values:?}");
        assert_eq!(analysis.valid_candidate_count, 0, "values={values:?}");
        assert!(analysis.groups.is_none(), "values={values:?}");
    }
}

#[test]
fn type320_entity_table_boundary_follows_member_and_connect_counts() {
    for (member_count, connect_count, expected_start) in
        [(0_i64, 0_i64, 8), (1, 0, 9), (0, 1, 9), (2, 1, 11)]
    {
        let association = directory_target(1, 212);
        let member = directory_target(3, 132);
        let connect_point = directory_target(5, 132);
        let mut source = directory_target(7, 320);
        source.form = 0;
        let directory = BTreeMap::from([
            (1, &association),
            (3, &member),
            (5, &connect_point),
            (7, &source),
        ]);
        let member_count = usize::try_from(member_count).unwrap();
        let connect_count = usize::try_from(connect_count).unwrap();
        let mut values = vec![0_i64; expected_start + 3];
        values[0] = 320;
        values[3] = i64::try_from(member_count).unwrap();
        for index in 0..member_count {
            values[4 + index] = 3;
        }
        values[4 + member_count] = 0;
        values[5 + member_count] = 0;
        values[6 + member_count] = 0;
        values[7 + member_count] = i64::try_from(connect_count).unwrap();
        for index in 0..connect_count {
            values[8 + member_count + index] = 5;
        }
        values[expected_start] = 1;
        values[expected_start + 1] = 1;
        values[expected_start + 2] = 0;
        let parameter_end = values.len();
        let record = ParameterRecord {
            directory_sequence: 7,
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
        assert_eq!(
            analysis.candidate_count, 1,
            "NA={member_count}, NC={connect_count}"
        );
        assert_eq!(
            analysis.valid_candidate_count, 1,
            "NA={member_count}, NC={connect_count}"
        );
        let groups = analysis.groups.expect("Type 320 table boundary");
        assert_eq!(groups.token_start, expected_start);
        assert_eq!(groups.associations, vec![1]);
        assert!(groups.properties.is_empty());
    }
}

#[test]
fn type320_entity_table_boundary_precedes_valid_generic_alternative() {
    let target_1 = directory_target(1, 212);
    let target_3 = directory_target(3, 212);
    let source = directory_target(5, 320);
    let directory = BTreeMap::from([(1, &target_1), (3, &target_3), (5, &source)]);
    let values = [320, 0, 0, 1, 1, 0, 0, 0, 1, 2, 1, 3, 0];
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
    let groups = analysis.groups.expect("Type 320 table boundary");
    assert_eq!(groups.token_start, 10);
    assert_eq!(groups.associations, vec![3]);
    assert!(groups.properties.is_empty());
}

#[test]
fn type320_malformed_counts_do_not_enable_generic_recovery() {
    let target_1 = directory_target(1, 212);
    let target_5 = directory_target(5, 212);
    let source = directory_target(3, 320);
    let directory = BTreeMap::from([(1, &target_1), (3, &source), (5, &target_5)]);
    let cases = [
        vec![320, 0, 0, -1, 1, 0, 0, 1, 5, 1, 5, 0],
        vec![320, 0, 0, 100, 1, 0, 0, 1, 5, 1, 5, 0],
        vec![320, 0, 0, 1, 1, 0, 0],
        vec![320, 0, 0, 1, 1, 0, 0, -1, 1, 5, 0],
        vec![320, 0, 0, 1, 1, 0, 0, 2, 5],
    ];

    for values in cases {
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
fn type180_forms_share_postorder_boundary() {
    for form in [0_i64, 1] {
        let mut source = directory_target(1, 180);
        source.form = form;
        let operand = directory_target(3, if form == 1 { 186 } else { 158 });
        let association = directory_target(7, 212);
        let directory = BTreeMap::from([(1, &source), (3, &operand), (7, &association)]);
        let record = integer_parameter_record(1, &[180, 3, -3, -5, 1, 1, 7, 0]);

        let analysis = analyze_trailing_pointer_groups(&record, &directory);
        assert_eq!(analysis.candidate_count, 1, "Form {form}");
        assert_eq!(analysis.valid_candidate_count, 1, "Form {form}");
        let groups = analysis.groups.expect("Type 180 table boundary");
        assert_eq!(groups.token_start, 5, "Form {form}");
        assert_eq!(groups.associations, vec![7], "Form {form}");
        assert!(groups.properties.is_empty(), "Form {form}");
    }
}

#[test]
fn type180_table_boundary_precedes_generic_candidate() {
    let source = directory_target(1, 180);
    let association = directory_target(7, 212);
    let directory = BTreeMap::from([(1, &source), (7, &association)]);
    let record = integer_parameter_record(1, &[180, 5, -3, -5, 1, -3, 1, 7, 0]);

    let generic = structural_pointer_group_candidates(&record);
    assert_eq!(generic.len(), 1);
    assert_eq!(generic[0].token_start, 6);

    let analysis = analyze_trailing_pointer_groups(&record, &directory);
    assert_eq!(analysis.candidate_count, 0);
    assert_eq!(analysis.valid_candidate_count, 0);
    assert!(analysis.groups.is_none());
}

#[test]
fn type180_malformed_length_or_terms_do_not_enable_generic_recovery() {
    let source = directory_target(1, 180);
    let association = directory_target(7, 212);
    let directory = BTreeMap::from([(1, &source), (7, &association)]);
    for values in [
        vec![180, 2, -3, -5, 1, 7, 0],
        vec![180, 0, -3, -5, 1, 7, 0],
        vec![180, -1, -3, -5, 1, 7, 0],
        vec![180, 5, -3, -5, 1],
    ] {
        let record = integer_parameter_record(1, &values);
        let analysis = analyze_trailing_pointer_groups(&record, &directory);
        assert_eq!(analysis.candidate_count, 0, "values={values:?}");
        assert_eq!(analysis.valid_candidate_count, 0, "values={values:?}");
        assert!(analysis.groups.is_none(), "values={values:?}");
    }

    let mut record = integer_parameter_record(1, &[180, 5, -3, -5, 1, 7, 0]);
    record.tokens[1].value = TokenValue::Real(5.0);
    let analysis = analyze_trailing_pointer_groups(&record, &directory);
    assert_eq!(analysis.candidate_count, 0);
    assert_eq!(analysis.valid_candidate_count, 0);
    assert!(analysis.groups.is_none());
}

#[test]
fn type202_form0_boundary_follows_eight_primary_fields() {
    let association = directory_target(1, 212);
    let property = directory_target(5, 406);
    let mut source = directory_target(9, 202);
    source.form = 0;
    let directory = BTreeMap::from([(1, &association), (5, &property), (9, &source)]);
    let record = integer_parameter_record(9, &[202, 1, 0, 0, 0, 0, 2, 3, 5, 1, 1, 1, 5]);

    let analysis = analyze_trailing_pointer_groups(&record, &directory);
    assert_eq!(analysis.candidate_count, 1);
    assert_eq!(analysis.valid_candidate_count, 1);
    let groups = analysis.groups.expect("Type 202 table boundary");
    assert_eq!(groups.token_start, 9);
    assert_eq!(groups.associations, vec![1]);
    assert_eq!(groups.properties, vec![5]);
}

#[test]
fn type202_form0_boundary_precedes_generic_candidate() {
    let association = directory_target(1, 212);
    let property = directory_target(5, 406);
    let mut source = directory_target(9, 202);
    source.form = 0;
    let directory = BTreeMap::from([(1, &association), (5, &property), (9, &source)]);
    let record = integer_parameter_record(9, &[202, 1, 0, 0, 0, 0, 2, 3, 2, 1, 1, 1, 5]);

    let generic = structural_pointer_group_candidates(&record);
    let mut valid_starts = Vec::new();
    for candidate in generic {
        if groups_for_candidate(&record, &directory, candidate)
            .expect("generic Type 202 candidate")
            .fully_valid
        {
            valid_starts.push(candidate.token_start);
        }
    }
    assert_eq!(valid_starts, vec![8, 9]);

    let analysis = analyze_trailing_pointer_groups(&record, &directory);
    assert_eq!(analysis.candidate_count, 1);
    assert_eq!(analysis.valid_candidate_count, 1);
    assert_eq!(
        analysis
            .groups
            .expect("Type 202 table boundary")
            .token_start,
        9
    );
}

#[test]
fn type202_complete_wrong_fields_keep_boundary_and_truncated_spans_do_not_recover() {
    let association = directory_target(1, 212);
    let property = directory_target(5, 406);
    let source = directory_target(9, 202);
    let directory = BTreeMap::from([(1, &association), (5, &property), (9, &source)]);
    let wrong_fields = token_parameter_record(
        9,
        vec![
            202.into(),
            1.into(),
            0.into(),
            0.into(),
            TokenValue::String(b"bad-x".to_vec()),
            TokenValue::Omitted,
            2.into(),
            TokenValue::String(b"bad-leader".to_vec()),
            5.into(),
            1.into(),
            1.into(),
            1.into(),
            5.into(),
        ],
    );
    let analysis = analyze_trailing_pointer_groups(&wrong_fields, &directory);
    assert_eq!(analysis.candidate_count, 1);
    assert_eq!(analysis.valid_candidate_count, 1);
    assert_eq!(analysis.groups.expect("Type 202 boundary").token_start, 9);

    for values in [
        vec![202, 1, 0, 0, 0, 0, 2, 3],
        vec![202, 1, 0, 0, 0, 0, 2, 3, 5, 1, 1],
    ] {
        let analysis =
            analyze_trailing_pointer_groups(&integer_parameter_record(9, &values), &directory);
        assert_eq!(analysis.candidate_count, 0, "values={values:?}");
        assert_eq!(analysis.valid_candidate_count, 0, "values={values:?}");
        assert!(analysis.groups.is_none(), "values={values:?}");
    }
}

#[test]
fn type104_forms_share_eleven_field_boundary() {
    let association = directory_target(1, 212);
    let property = directory_target(5, 406);
    for form in 0..=3 {
        let mut source = directory_target(9, 104);
        source.form = form;
        let directory = BTreeMap::from([(1, &association), (5, &property), (9, &source)]);
        let record =
            integer_parameter_record(9, &[104, 1, 0, 1, 0, 0, -1, 0, 2, 0, 0, 1, 1, 1, 1, 5]);

        let analysis = analyze_trailing_pointer_groups(&record, &directory);
        assert_eq!(analysis.candidate_count, 1, "form {form}");
        assert_eq!(analysis.valid_candidate_count, 1, "form {form}");
        let groups = analysis.groups.expect("Type 104 table boundary");
        assert_eq!(groups.token_start, 12, "form {form}");
        assert_eq!(groups.associations, vec![1], "form {form}");
        assert_eq!(groups.properties, vec![5], "form {form}");
    }
}

#[test]
fn type104_table_boundary_precedes_valid_generic_alternative() {
    let association = directory_target(1, 212);
    let property = directory_target(5, 406);
    let mut source = directory_target(9, 104);
    source.form = 1;
    let directory = BTreeMap::from([(1, &association), (5, &property), (9, &source)]);
    let record = integer_parameter_record(9, &[104, 1, 0, 1, 0, 0, -1, 0, 2, 0, 3, 1, 1, 1, 1, 5]);

    let generic = structural_pointer_group_candidates(&record);
    let mut valid_starts = Vec::new();
    for candidate in generic {
        if groups_for_candidate(&record, &directory, candidate)
            .expect("generic Type 104 candidate")
            .fully_valid
        {
            valid_starts.push(candidate.token_start);
        }
    }
    assert_eq!(valid_starts, vec![10, 12]);

    let analysis = analyze_trailing_pointer_groups(&record, &directory);
    assert_eq!(analysis.candidate_count, 1);
    assert_eq!(analysis.valid_candidate_count, 1);
    assert_eq!(
        analysis
            .groups
            .expect("Type 104 table boundary")
            .token_start,
        12
    );
}

#[test]
fn type104_complete_wrong_fields_keep_boundary_and_truncated_spans_do_not_recover() {
    let association = directory_target(1, 212);
    let property = directory_target(5, 406);
    let source = directory_target(9, 104);
    let directory = BTreeMap::from([(1, &association), (5, &property), (9, &source)]);
    let wrong_fields = token_parameter_record(
        9,
        vec![
            104.into(),
            1.into(),
            0.into(),
            1.into(),
            0.into(),
            0.into(),
            (-1).into(),
            0.into(),
            2.into(),
            0.into(),
            TokenValue::String(b"bad-x".to_vec()),
            TokenValue::String(b"bad-y".to_vec()),
            1.into(),
            1.into(),
            1.into(),
            5.into(),
        ],
    );
    let analysis = analyze_trailing_pointer_groups(&wrong_fields, &directory);
    assert_eq!(analysis.candidate_count, 1);
    assert_eq!(analysis.valid_candidate_count, 1);
    assert_eq!(analysis.groups.expect("Type 104 boundary").token_start, 12);

    for values in [
        vec![104, 1, 0, 1, 0, 0, -1, 0, 2, 0],
        vec![104, 1, 0, 1, 0, 0, -1, 0, 2, 0, 0, 1, 1, 1, 1],
    ] {
        let analysis =
            analyze_trailing_pointer_groups(&integer_parameter_record(9, &values), &directory);
        assert_eq!(analysis.candidate_count, 0, "values={values:?}");
        assert_eq!(analysis.valid_candidate_count, 0, "values={values:?}");
        assert!(analysis.groups.is_none(), "values={values:?}");
    }
}

#[test]
fn type108_forms_share_nine_field_boundary() {
    let association = directory_target(1, 212);
    let boundary = directory_target(7, 100);
    let property = directory_target(5, 406);
    for form in [-1, 0, 1] {
        let mut source = directory_target(9, 108);
        source.form = form;
        let pointer = if form == 0 { 0 } else { 7 };
        let directory = BTreeMap::from([
            (1, &association),
            (5, &property),
            (7, &boundary),
            (9, &source),
        ]);
        let record =
            integer_parameter_record(9, &[108, 0, 0, 1, 2, pointer, 0, 0, 0, 1, 1, 1, 1, 5]);

        let analysis = analyze_trailing_pointer_groups(&record, &directory);
        assert_eq!(analysis.candidate_count, 1, "form {form}");
        assert_eq!(analysis.valid_candidate_count, 1, "form {form}");
        let groups = analysis.groups.expect("Type 108 table boundary");
        assert_eq!(groups.token_start, 10, "form {form}");
        assert_eq!(groups.associations, vec![1], "form {form}");
        assert_eq!(groups.properties, vec![5], "form {form}");
    }
}

#[test]
fn type108_table_boundary_precedes_valid_generic_alternative() {
    let association = directory_target(1, 212);
    let boundary = directory_target(7, 100);
    let property = directory_target(5, 406);
    let mut source = directory_target(9, 108);
    source.form = 1;
    let directory = BTreeMap::from([
        (1, &association),
        (5, &property),
        (7, &boundary),
        (9, &source),
    ]);
    let record = integer_parameter_record(9, &[108, 0, 0, 1, 2, 7, 0, 0, 3, 1, 1, 1, 1, 5]);

    let generic = structural_pointer_group_candidates(&record);
    let mut valid_starts = Vec::new();
    for candidate in generic {
        if groups_for_candidate(&record, &directory, candidate)
            .expect("generic Type 108 candidate")
            .fully_valid
        {
            valid_starts.push(candidate.token_start);
        }
    }
    assert_eq!(valid_starts, vec![8, 10]);

    let analysis = analyze_trailing_pointer_groups(&record, &directory);
    assert_eq!(analysis.candidate_count, 1);
    assert_eq!(analysis.valid_candidate_count, 1);
    assert_eq!(
        analysis
            .groups
            .expect("Type 108 table boundary")
            .token_start,
        10
    );
}

#[test]
fn type108_complete_wrong_fields_keep_boundary_and_truncated_spans_do_not_recover() {
    let association = directory_target(1, 212);
    let boundary = directory_target(7, 100);
    let property = directory_target(5, 406);
    let source = directory_target(9, 108);
    let directory = BTreeMap::from([
        (1, &association),
        (5, &property),
        (7, &boundary),
        (9, &source),
    ]);
    let wrong_fields = token_parameter_record(
        9,
        vec![
            108.into(),
            0.into(),
            0.into(),
            1.into(),
            2.into(),
            7.into(),
            0.into(),
            0.into(),
            TokenValue::String(b"bad-z".to_vec()),
            TokenValue::String(b"bad-size".to_vec()),
            1.into(),
            1.into(),
            1.into(),
            5.into(),
        ],
    );
    let analysis = analyze_trailing_pointer_groups(&wrong_fields, &directory);
    assert_eq!(analysis.candidate_count, 1);
    assert_eq!(analysis.valid_candidate_count, 1);
    assert_eq!(analysis.groups.expect("Type 108 boundary").token_start, 10);

    for values in [
        vec![108, 0, 0, 1, 2, 7, 0, 0, 0],
        vec![108, 0, 0, 1, 2, 7, 0, 0, 0, 1, 1, 1],
    ] {
        let analysis =
            analyze_trailing_pointer_groups(&integer_parameter_record(9, &values), &directory);
        assert_eq!(analysis.candidate_count, 0, "values={values:?}");
        assert_eq!(analysis.valid_candidate_count, 0, "values={values:?}");
        assert!(analysis.groups.is_none(), "values={values:?}");
    }
}

#[test]
fn type214_forms_share_count_driven_boundary() {
    let association = directory_target(3, 212);
    let n1 = vec![
        TokenValue::Integer(214),
        TokenValue::Integer(1),
        TokenValue::Integer(2),
        TokenValue::Integer(1),
        TokenValue::Integer(0),
        TokenValue::Integer(0),
        TokenValue::Integer(6),
        TokenValue::Integer(3),
        TokenValue::Integer(3),
        TokenValue::Integer(3),
        TokenValue::Integer(3),
        TokenValue::Integer(3),
        TokenValue::Integer(3),
        TokenValue::Integer(0),
    ];
    let n2 = vec![
        TokenValue::Integer(214),
        TokenValue::Integer(2),
        TokenValue::Integer(2),
        TokenValue::Integer(1),
        TokenValue::Integer(0),
        TokenValue::Integer(0),
        TokenValue::Integer(6),
        TokenValue::Integer(3),
        TokenValue::Integer(3),
        TokenValue::Integer(3),
        TokenValue::Integer(3),
        TokenValue::Integer(3),
        TokenValue::Integer(3),
        TokenValue::Integer(3),
        TokenValue::Integer(3),
        TokenValue::Integer(0),
    ];
    let mut wrong_field = n1.clone();
    wrong_field[7] = TokenValue::String(b"1HX".to_vec());

    for form in 1_i64..=12 {
        let mut source = directory_target(1, 214);
        source.form = form;
        let directory = BTreeMap::from([(1, &source), (3, &association)]);
        for (values, expected_start) in [
            (n1.clone(), 9_usize),
            (n2.clone(), 11_usize),
            (wrong_field.clone(), 9_usize),
        ] {
            let analysis =
                analyze_trailing_pointer_groups(&token_parameter_record(1, values), &directory);
            assert_eq!(analysis.candidate_count, 1, "Form {form}");
            assert_eq!(analysis.valid_candidate_count, 1, "Form {form}");
            let groups = analysis.groups.expect("Type 214 table boundary");
            assert_eq!(groups.token_start, expected_start, "Form {form}");
            assert_eq!(groups.associations, vec![3, 3, 3], "Form {form}");
            assert!(groups.properties.is_empty(), "Form {form}");
        }
    }
}

#[test]
fn type214_table_boundary_precedes_generic_candidate() {
    let mut source = directory_target(1, 214);
    source.form = 1;
    let association = directory_target(3, 212);
    let directory = BTreeMap::from([(1, &source), (3, &association)]);
    let record = token_parameter_record(
        1,
        vec![
            TokenValue::Integer(214),
            TokenValue::Integer(1),
            TokenValue::Integer(2),
            TokenValue::Integer(1),
            TokenValue::Integer(0),
            TokenValue::Integer(0),
            TokenValue::Integer(6),
            TokenValue::Integer(3),
            TokenValue::Integer(3),
            TokenValue::Integer(3),
            TokenValue::Integer(3),
            TokenValue::Integer(3),
            TokenValue::Integer(3),
            TokenValue::Integer(0),
        ],
    );
    let generic = structural_pointer_group_candidates(&record);
    assert_eq!(
        generic
            .iter()
            .map(|candidate| candidate.token_start)
            .collect::<Vec<_>>(),
        vec![6, 9]
    );

    let analysis = analyze_trailing_pointer_groups(&record, &directory);
    assert_eq!(analysis.candidate_count, 1);
    assert_eq!(analysis.valid_candidate_count, 1);
    let groups = analysis.groups.expect("Type 214 table boundary");
    assert_eq!(groups.token_start, 9);
    assert_eq!(groups.associations, vec![3, 3, 3]);
    assert!(groups.properties.is_empty());
}

#[test]
fn type214_malformed_count_or_span_does_not_enable_generic_recovery() {
    let mut source = directory_target(1, 214);
    source.form = 1;
    let association = directory_target(3, 212);
    let directory = BTreeMap::from([(1, &source), (3, &association)]);
    let n1 = vec![
        TokenValue::Integer(214),
        TokenValue::Integer(1),
        TokenValue::Integer(2),
        TokenValue::Integer(1),
        TokenValue::Integer(0),
        TokenValue::Integer(0),
        TokenValue::Integer(6),
        TokenValue::Integer(3),
        TokenValue::Integer(3),
        TokenValue::Integer(3),
        TokenValue::Integer(3),
        TokenValue::Integer(3),
        TokenValue::Integer(3),
        TokenValue::Integer(0),
    ];
    let mut wrong_type = n1.clone();
    wrong_type[1] = TokenValue::Real(1.0);
    let mut omitted = n1.clone();
    omitted[1] = TokenValue::Omitted;
    let mut zero = n1.clone();
    zero[1] = TokenValue::Integer(0);
    let mut negative = n1.clone();
    negative[1] = TokenValue::Integer(-1);
    let mut overflowing = n1.clone();
    overflowing[1] = TokenValue::Integer(i64::MAX);
    let mut truncated = n1;
    truncated.truncate(9);

    for values in [wrong_type, omitted, zero, negative, overflowing, truncated] {
        let analysis =
            analyze_trailing_pointer_groups(&token_parameter_record(1, values), &directory);
        assert_eq!(analysis.candidate_count, 0);
        assert_eq!(analysis.valid_candidate_count, 0);
        assert!(analysis.groups.is_none());
    }
}

#[test]
fn type218_forms_share_fixed_primary_boundary() {
    let association = directory_target(3, 212);
    for (form, values, expected_start) in [
        (
            0_i64,
            vec![
                TokenValue::Integer(218),
                TokenValue::Integer(3),
                TokenValue::Integer(5),
                TokenValue::Integer(1),
                TokenValue::Integer(3),
                TokenValue::Integer(0),
            ],
            3_usize,
        ),
        (
            1_i64,
            vec![
                TokenValue::Integer(218),
                TokenValue::Integer(3),
                TokenValue::Integer(5),
                TokenValue::Integer(7),
                TokenValue::Integer(1),
                TokenValue::Integer(3),
                TokenValue::Integer(0),
            ],
            4,
        ),
    ] {
        let mut source = directory_target(1, 218);
        source.form = form;
        let directory = BTreeMap::from([(1, &source), (3, &association)]);
        let analysis =
            analyze_trailing_pointer_groups(&token_parameter_record(1, values), &directory);
        assert_eq!(analysis.candidate_count, 1, "Form {form}");
        assert_eq!(analysis.valid_candidate_count, 1, "Form {form}");
        let groups = analysis.groups.expect("Type 218 table boundary");
        assert_eq!(groups.token_start, expected_start, "Form {form}");
        assert_eq!(groups.associations, vec![3], "Form {form}");
        assert!(groups.properties.is_empty(), "Form {form}");
    }

    for (form, values, expected_start) in [
        (
            0_i64,
            vec![
                TokenValue::Integer(218),
                TokenValue::Integer(3),
                TokenValue::Real(5.0),
                TokenValue::Integer(1),
                TokenValue::Integer(3),
                TokenValue::Integer(0),
            ],
            3_usize,
        ),
        (
            0,
            vec![
                TokenValue::Integer(218),
                TokenValue::Integer(3),
                TokenValue::Omitted,
                TokenValue::Integer(1),
                TokenValue::Integer(3),
                TokenValue::Integer(0),
            ],
            3,
        ),
        (
            1,
            vec![
                TokenValue::Integer(218),
                TokenValue::Integer(3),
                TokenValue::Integer(5),
                TokenValue::Real(7.0),
                TokenValue::Integer(1),
                TokenValue::Integer(3),
                TokenValue::Integer(0),
            ],
            4,
        ),
        (
            1,
            vec![
                TokenValue::Integer(218),
                TokenValue::Integer(3),
                TokenValue::Integer(5),
                TokenValue::Omitted,
                TokenValue::Integer(1),
                TokenValue::Integer(3),
                TokenValue::Integer(0),
            ],
            4,
        ),
    ] {
        let mut source = directory_target(1, 218);
        source.form = form;
        let directory = BTreeMap::from([(1, &source), (3, &association)]);
        let analysis =
            analyze_trailing_pointer_groups(&token_parameter_record(1, values), &directory);
        assert_eq!(analysis.candidate_count, 1, "Form {form}");
        assert_eq!(analysis.valid_candidate_count, 1, "Form {form}");
        let groups = analysis
            .groups
            .expect("Type 218 boundary with invalid field");
        assert_eq!(groups.token_start, expected_start, "Form {form}");
        assert_eq!(groups.associations, vec![3], "Form {form}");
    }
}

#[test]
fn type218_table_boundary_precedes_generic_candidates() {
    let association = directory_target(3, 212);
    for (form, values, expected_start, alternative_start) in [
        (
            0_i64,
            vec![218, 3, 5, 6, 3, 3, 3, 3, 3, 3, 0],
            3_usize,
            6_usize,
        ),
        (1, vec![218, 3, 5, 7, 6, 3, 3, 3, 3, 3, 3, 0], 4, 7),
    ] {
        let mut source = directory_target(1, 218);
        source.form = form;
        let directory = BTreeMap::from([(1, &source), (3, &association)]);
        let record = integer_parameter_record(1, &values);
        let generic = structural_pointer_group_candidates(&record);
        assert!(generic
            .iter()
            .any(|candidate| candidate.token_start == expected_start));
        assert!(generic
            .iter()
            .any(|candidate| candidate.token_start == alternative_start));

        let analysis = analyze_trailing_pointer_groups(&record, &directory);
        assert_eq!(analysis.candidate_count, 1, "Form {form}");
        assert_eq!(analysis.valid_candidate_count, 1, "Form {form}");
        let groups = analysis.groups.expect("Type 218 table boundary");
        assert_eq!(groups.token_start, expected_start, "Form {form}");
        assert_eq!(groups.associations, vec![3; 6], "Form {form}");
        assert!(groups.properties.is_empty(), "Form {form}");
    }
}

#[test]
fn type218_truncated_primary_or_group_does_not_enable_generic_recovery() {
    let association = directory_target(3, 212);
    for (form, values) in [
        (0_i64, vec![218, 3]),
        (0, vec![218, 3, 5, 1, 3]),
        (1, vec![218, 3, 5]),
        (1, vec![218, 3, 5, 7, 1, 3]),
    ] {
        let mut source = directory_target(1, 218);
        source.form = form;
        let directory = BTreeMap::from([(1, &source), (3, &association)]);
        let analysis =
            analyze_trailing_pointer_groups(&integer_parameter_record(1, &values), &directory);
        assert_eq!(
            analysis.candidate_count, 0,
            "Form {form}, values={values:?}"
        );
        assert_eq!(
            analysis.valid_candidate_count, 0,
            "Form {form}, values={values:?}"
        );
        assert!(analysis.groups.is_none(), "Form {form}, values={values:?}");
    }
}

#[test]
fn type406_form1_entity_table_boundary_follows_level_list() {
    let mut source = directory_target(1, 406);
    source.form = 1;
    let association = directory_target(3, 212);
    let directory = BTreeMap::from([(1, &source), (3, &association)]);
    for values in [
        vec![
            TokenValue::Integer(406),
            TokenValue::Integer(1),
            TokenValue::Integer(5),
            TokenValue::Integer(1),
            TokenValue::Integer(3),
            TokenValue::Integer(0),
        ],
        vec![
            TokenValue::Integer(406),
            TokenValue::Integer(1),
            TokenValue::String(b"5".to_vec()),
            TokenValue::Integer(1),
            TokenValue::Integer(3),
            TokenValue::Integer(0),
        ],
    ] {
        let analysis =
            analyze_trailing_pointer_groups(&token_parameter_record(1, values), &directory);
        assert_eq!(analysis.candidate_count, 1);
        assert_eq!(analysis.valid_candidate_count, 1);
        let groups = analysis.groups.expect("Type 406 Form 1 table boundary");
        assert_eq!(groups.token_start, 3);
        assert_eq!(groups.associations, vec![3]);
        assert!(groups.properties.is_empty());
    }
}

#[test]
fn type406_form1_table_boundary_precedes_generic_candidate() {
    let mut source = directory_target(1, 406);
    source.form = 1;
    let association = directory_target(3, 212);
    let property_a = directory_target(5, 406);
    let property_b = directory_target(7, 406);
    let directory = BTreeMap::from([
        (1, &source),
        (3, &association),
        (5, &property_a),
        (7, &property_b),
    ]);
    let values = vec![
        TokenValue::Integer(406),
        TokenValue::Integer(1),
        TokenValue::Integer(1),
        TokenValue::Integer(1),
        TokenValue::Integer(3),
        TokenValue::Integer(2),
        TokenValue::Integer(5),
        TokenValue::Integer(7),
    ];
    let record = token_parameter_record(1, values);
    let generic = structural_pointer_group_candidates(&record);
    assert!(generic.iter().any(|candidate| candidate.token_start == 2));

    let analysis = analyze_trailing_pointer_groups(&record, &directory);
    assert_eq!(analysis.candidate_count, 1);
    assert_eq!(analysis.valid_candidate_count, 1);
    let groups = analysis.groups.expect("Type 406 Form 1 table boundary");
    assert_eq!(groups.token_start, 3);
    assert_eq!(groups.associations, vec![3]);
    assert_eq!(groups.properties, vec![5, 7]);
}

#[test]
fn type406_form1_malformed_np_or_span_does_not_enable_generic_recovery() {
    let mut source = directory_target(1, 406);
    source.form = 1;
    let association = directory_target(3, 212);
    let directory = BTreeMap::from([(1, &source), (3, &association)]);
    for values in [
        vec![
            TokenValue::Integer(406),
            TokenValue::Real(1.0),
            TokenValue::Integer(5),
            TokenValue::Integer(1),
            TokenValue::Integer(3),
            TokenValue::Integer(0),
        ],
        vec![
            TokenValue::Integer(406),
            TokenValue::Omitted,
            TokenValue::Integer(5),
            TokenValue::Integer(1),
            TokenValue::Integer(3),
            TokenValue::Integer(0),
        ],
        vec![
            TokenValue::Integer(406),
            TokenValue::Integer(0),
            TokenValue::Integer(5),
            TokenValue::Integer(1),
            TokenValue::Integer(3),
            TokenValue::Integer(0),
        ],
        vec![
            TokenValue::Integer(406),
            TokenValue::Integer(5),
            TokenValue::Integer(5),
            TokenValue::Integer(1),
            TokenValue::Integer(3),
            TokenValue::Integer(0),
        ],
        vec![
            TokenValue::Integer(406),
            TokenValue::Integer(4),
            TokenValue::Integer(5),
            TokenValue::Integer(6),
        ],
    ] {
        let record = token_parameter_record(1, values);
        let generic_count = structural_pointer_group_candidates(&record).len();
        let analysis = analyze_trailing_pointer_groups(&record, &directory);
        assert_eq!(analysis.candidate_count, 0, "generic_count={generic_count}");
        assert_eq!(analysis.valid_candidate_count, 0);
        assert!(analysis.groups.is_none());
    }
}

#[test]
fn type406_drawing_properties_share_fixed_primary_boundary() {
    let association = directory_target(3, 212);
    for (form, values) in [
        (
            16_i64,
            vec![
                TokenValue::Integer(406),
                TokenValue::Integer(2),
                TokenValue::Integer(10),
                TokenValue::Integer(20),
                TokenValue::Integer(1),
                TokenValue::Integer(3),
                TokenValue::Integer(0),
            ],
        ),
        (
            17_i64,
            vec![
                TokenValue::Integer(406),
                TokenValue::Integer(2),
                TokenValue::Integer(2),
                TokenValue::String(b"MM".to_vec()),
                TokenValue::Integer(1),
                TokenValue::Integer(3),
                TokenValue::Integer(0),
            ],
        ),
        (
            16,
            vec![
                TokenValue::Integer(406),
                TokenValue::Integer(2),
                TokenValue::String(b"X".to_vec()),
                TokenValue::Integer(20),
                TokenValue::Integer(1),
                TokenValue::Integer(3),
                TokenValue::Integer(0),
            ],
        ),
        (
            17,
            vec![
                TokenValue::Integer(406),
                TokenValue::Integer(2),
                TokenValue::Integer(2),
                TokenValue::Integer(1),
                TokenValue::Integer(1),
                TokenValue::Integer(3),
                TokenValue::Integer(0),
            ],
        ),
    ] {
        let mut source = directory_target(1, 406);
        source.form = form;
        let directory = BTreeMap::from([(1, &source), (3, &association)]);
        let analysis =
            analyze_trailing_pointer_groups(&token_parameter_record(1, values), &directory);
        assert_eq!(analysis.candidate_count, 1, "Form {form}");
        assert_eq!(analysis.valid_candidate_count, 1, "Form {form}");
        let groups = analysis
            .groups
            .expect("Type 406 drawing property table boundary");
        assert_eq!(groups.token_start, 4, "Form {form}");
        assert_eq!(groups.associations, vec![3], "Form {form}");
        assert!(groups.properties.is_empty(), "Form {form}");
    }
}

#[test]
fn type406_drawing_property_boundary_precedes_generic_candidate() {
    let association = directory_target(3, 212);
    let property_a = directory_target(5, 406);
    let property_b = directory_target(7, 406);
    for (form, values) in [
        (
            16_i64,
            vec![
                TokenValue::Integer(406),
                TokenValue::Integer(2),
                TokenValue::Integer(10),
                TokenValue::Integer(1),
                TokenValue::Integer(3),
                TokenValue::Integer(2),
                TokenValue::Integer(5),
                TokenValue::Integer(7),
            ],
        ),
        (
            17_i64,
            vec![
                TokenValue::Integer(406),
                TokenValue::Integer(2),
                TokenValue::Integer(2),
                TokenValue::Integer(1),
                TokenValue::Integer(3),
                TokenValue::Integer(2),
                TokenValue::Integer(5),
                TokenValue::Integer(7),
            ],
        ),
    ] {
        let mut source = directory_target(1, 406);
        source.form = form;
        let directory = BTreeMap::from([
            (1, &source),
            (3, &association),
            (5, &property_a),
            (7, &property_b),
        ]);
        let record = token_parameter_record(1, values);
        assert!(
            structural_pointer_group_candidates(&record)
                .iter()
                .any(|candidate| candidate.token_start == 3),
            "Form {form} generic candidate"
        );
        let analysis = analyze_trailing_pointer_groups(&record, &directory);
        assert_eq!(analysis.candidate_count, 0, "Form {form}");
        assert_eq!(analysis.valid_candidate_count, 0, "Form {form}");
        assert!(analysis.groups.is_none(), "Form {form}");
    }
}

#[test]
fn type406_drawing_property_malformed_np_or_span_does_not_enable_generic_recovery() {
    let association = directory_target(3, 212);
    let mut source = directory_target(1, 406);
    for (form, cases) in [
        (
            16_i64,
            vec![
                vec![
                    TokenValue::Integer(406),
                    TokenValue::Real(2.0),
                    TokenValue::Integer(10),
                    TokenValue::Integer(20),
                    TokenValue::Integer(1),
                    TokenValue::Integer(3),
                    TokenValue::Integer(0),
                ],
                vec![
                    TokenValue::Integer(406),
                    TokenValue::Omitted,
                    TokenValue::Integer(10),
                    TokenValue::Integer(20),
                    TokenValue::Integer(1),
                    TokenValue::Integer(3),
                    TokenValue::Integer(0),
                ],
                vec![
                    TokenValue::Integer(406),
                    TokenValue::Integer(0),
                    TokenValue::Integer(10),
                    TokenValue::Integer(20),
                    TokenValue::Integer(1),
                    TokenValue::Integer(3),
                    TokenValue::Integer(0),
                ],
                vec![
                    TokenValue::Integer(406),
                    TokenValue::Integer(2),
                    TokenValue::Integer(10),
                ],
            ],
        ),
        (
            17_i64,
            vec![
                vec![
                    TokenValue::Integer(406),
                    TokenValue::Real(2.0),
                    TokenValue::Integer(2),
                    TokenValue::String(b"MM".to_vec()),
                    TokenValue::Integer(1),
                    TokenValue::Integer(3),
                    TokenValue::Integer(0),
                ],
                vec![
                    TokenValue::Integer(406),
                    TokenValue::Omitted,
                    TokenValue::Integer(2),
                    TokenValue::String(b"MM".to_vec()),
                    TokenValue::Integer(1),
                    TokenValue::Integer(3),
                    TokenValue::Integer(0),
                ],
                vec![
                    TokenValue::Integer(406),
                    TokenValue::Integer(0),
                    TokenValue::Integer(2),
                    TokenValue::String(b"MM".to_vec()),
                    TokenValue::Integer(1),
                    TokenValue::Integer(3),
                    TokenValue::Integer(0),
                ],
                vec![
                    TokenValue::Integer(406),
                    TokenValue::Integer(2),
                    TokenValue::Integer(2),
                ],
            ],
        ),
    ] {
        source.form = form;
        let directory = BTreeMap::from([(1, &source), (3, &association)]);
        for values in cases {
            let record = token_parameter_record(1, values);
            let analysis = analyze_trailing_pointer_groups(&record, &directory);
            assert_eq!(analysis.candidate_count, 0, "Form {form}");
            assert_eq!(analysis.valid_candidate_count, 0, "Form {form}");
            assert!(analysis.groups.is_none(), "Form {form}");
        }
    }
}

#[test]
fn type406_form6_entity_table_boundary_follows_fixed_values() {
    let association = directory_target(3, 212);
    let mut source = directory_target(1, 406);
    source.form = 6;
    let directory = BTreeMap::from([(1, &source), (3, &association)]);
    for values in [
        vec![
            TokenValue::Integer(406),
            TokenValue::Integer(5),
            TokenValue::Real(1.0),
            TokenValue::Real(2.0),
            TokenValue::Integer(1),
            TokenValue::Integer(2),
            TokenValue::Integer(8),
            TokenValue::Integer(1),
            TokenValue::Integer(3),
            TokenValue::Integer(0),
        ],
        vec![
            TokenValue::Integer(406),
            TokenValue::Integer(4),
            TokenValue::Integer(1),
            TokenValue::Real(2.0),
            TokenValue::Integer(2),
            TokenValue::Integer(2),
            TokenValue::Integer(8),
            TokenValue::Integer(1),
            TokenValue::Integer(3),
            TokenValue::Integer(0),
        ],
        vec![
            TokenValue::Integer(406),
            TokenValue::Omitted,
            TokenValue::Real(1.0),
            TokenValue::Real(2.0),
            TokenValue::Integer(1),
            TokenValue::Integer(2),
            TokenValue::Integer(8),
            TokenValue::Integer(1),
            TokenValue::Integer(3),
            TokenValue::Integer(0),
        ],
        vec![
            TokenValue::Integer(406),
            TokenValue::Integer(5),
            TokenValue::Real(1.0),
            TokenValue::Real(2.0),
            TokenValue::Integer(2),
            TokenValue::Integer(2),
            TokenValue::Integer(8),
            TokenValue::Integer(1),
            TokenValue::Integer(3),
            TokenValue::Integer(0),
        ],
    ] {
        let analysis =
            analyze_trailing_pointer_groups(&token_parameter_record(1, values), &directory);
        assert_eq!(analysis.candidate_count, 1);
        assert_eq!(analysis.valid_candidate_count, 1);
        let groups = analysis.groups.expect("Type 406 Form 6 table boundary");
        assert_eq!(groups.token_start, 7);
        assert_eq!(groups.associations, vec![3]);
        assert!(groups.properties.is_empty());
    }
}

#[test]
fn type406_form6_table_boundary_precedes_generic_candidate() {
    let mut source = directory_target(1, 406);
    source.form = 6;
    let association = directory_target(3, 212);
    let directory = BTreeMap::from([(1, &source), (3, &association)]);
    let values = vec![406, 5, 1, 2, 1, 2, 8, 6, 3, 3, 3, 3, 3, 3, 0];
    let record = integer_parameter_record(1, &values);
    let generic = structural_pointer_group_candidates(&record);
    assert!(generic.iter().any(|candidate| candidate.token_start == 7));
    assert!(generic.iter().any(|candidate| candidate.token_start == 10));

    let analysis = analyze_trailing_pointer_groups(&record, &directory);
    assert_eq!(analysis.candidate_count, 1);
    assert_eq!(analysis.valid_candidate_count, 1);
    let groups = analysis.groups.expect("Type 406 Form 6 table boundary");
    assert_eq!(groups.token_start, 7);
    assert_eq!(groups.associations, vec![3; 6]);
    assert!(groups.properties.is_empty());
}

#[test]
fn type406_form6_truncated_primary_or_group_does_not_enable_generic_recovery() {
    let mut source = directory_target(1, 406);
    source.form = 6;
    let association = directory_target(3, 212);
    let directory = BTreeMap::from([(1, &source), (3, &association)]);
    for values in [vec![406, 5, 1, 2, 1, 2], vec![406, 5, 1, 2, 1, 2, 8, 1, 3]] {
        let analysis =
            analyze_trailing_pointer_groups(&integer_parameter_record(1, &values), &directory);
        assert_eq!(analysis.candidate_count, 0, "values={values:?}");
        assert_eq!(analysis.valid_candidate_count, 0, "values={values:?}");
        assert!(analysis.groups.is_none(), "values={values:?}");
    }
}

#[test]
fn type406_form19_entity_table_boundary_follows_fixed_values() {
    let mut source = directory_target(1, 406);
    source.form = 19;
    let association = directory_target(3, 212);
    let directory = BTreeMap::from([(1, &source), (3, &association)]);
    for values in [
        vec![
            406.into(),
            1.into(),
            12.into(),
            1.into(),
            3.into(),
            0.into(),
        ],
        vec![406.into(), 1.into(), 0.into(), 1.into(), 3.into(), 0.into()],
        vec![
            406.into(),
            1.into(),
            TokenValue::Real(12.0),
            1.into(),
            3.into(),
            0.into(),
        ],
        vec![
            406.into(),
            2.into(),
            12.into(),
            1.into(),
            3.into(),
            0.into(),
        ],
        vec![
            406.into(),
            TokenValue::Omitted,
            12.into(),
            1.into(),
            3.into(),
            0.into(),
        ],
    ] {
        let analysis =
            analyze_trailing_pointer_groups(&token_parameter_record(1, values), &directory);
        assert_eq!(analysis.candidate_count, 1);
        assert_eq!(analysis.valid_candidate_count, 1);
        let groups = analysis.groups.expect("Type 406 Form 19 table boundary");
        assert_eq!(groups.token_start, 3);
        assert_eq!(groups.associations, vec![3]);
        assert!(groups.properties.is_empty());
    }
}

#[test]
fn type406_form19_table_boundary_precedes_generic_candidate() {
    let mut source = directory_target(1, 406);
    source.form = 19;
    let association = directory_target(3, 212);
    let directory = BTreeMap::from([(1, &source), (3, &association)]);
    let record = integer_parameter_record(1, &[406, 1, 12, 6, 3, 3, 3, 3, 3, 3, 0]);
    let generic = structural_pointer_group_candidates(&record);
    assert!(generic.iter().any(|candidate| candidate.token_start == 3));
    assert!(generic.iter().any(|candidate| candidate.token_start == 6));

    let analysis = analyze_trailing_pointer_groups(&record, &directory);
    assert_eq!(analysis.candidate_count, 1);
    assert_eq!(analysis.valid_candidate_count, 1);
    let groups = analysis.groups.expect("Type 406 Form 19 table boundary");
    assert_eq!(groups.token_start, 3);
    assert_eq!(groups.associations, vec![3; 6]);
    assert!(groups.properties.is_empty());
}

#[test]
fn type406_form19_malformed_np_or_span_does_not_enable_generic_recovery() {
    let mut source = directory_target(1, 406);
    source.form = 19;
    let association = directory_target(3, 212);
    let directory = BTreeMap::from([(1, &source), (3, &association)]);
    for values in [
        vec![406.into(), 1.into(), 12.into()],
        vec![406.into(), 1.into(), 12.into(), 1.into(), 3.into()],
    ] {
        let analysis =
            analyze_trailing_pointer_groups(&token_parameter_record(1, values), &directory);
        assert_eq!(analysis.candidate_count, 0);
        assert_eq!(analysis.valid_candidate_count, 0);
        assert!(analysis.groups.is_none());
    }
}

#[test]
fn type406_fixed_property_forms_follow_table_boundaries() {
    let association = directory_target(3, 212);
    for (form, boundary, cases) in [
        (
            18_i64,
            3,
            vec![
                vec![
                    406.into(),
                    1.into(),
                    25.0.into(),
                    1.into(),
                    3.into(),
                    0.into(),
                ],
                vec![
                    406.into(),
                    2.into(),
                    25.0.into(),
                    1.into(),
                    3.into(),
                    0.into(),
                ],
                vec![
                    406.into(),
                    TokenValue::Omitted,
                    25.0.into(),
                    1.into(),
                    3.into(),
                    0.into(),
                ],
                vec![
                    406.into(),
                    1.into(),
                    TokenValue::Omitted,
                    1.into(),
                    3.into(),
                    0.into(),
                ],
            ],
        ),
        (
            20,
            3,
            vec![
                vec![406.into(), 1.into(), 1.into(), 1.into(), 3.into(), 0.into()],
                vec![406.into(), 2.into(), 1.into(), 1.into(), 3.into(), 0.into()],
                vec![
                    406.into(),
                    TokenValue::Omitted,
                    1.into(),
                    1.into(),
                    3.into(),
                    0.into(),
                ],
                vec![
                    406.into(),
                    1.into(),
                    TokenValue::Omitted,
                    1.into(),
                    3.into(),
                    0.into(),
                ],
            ],
        ),
        (
            21,
            3,
            vec![
                vec![406.into(), 1.into(), 0.into(), 1.into(), 3.into(), 0.into()],
                vec![406.into(), 2.into(), 0.into(), 1.into(), 3.into(), 0.into()],
                vec![
                    406.into(),
                    TokenValue::Omitted,
                    0.into(),
                    1.into(),
                    3.into(),
                    0.into(),
                ],
                vec![
                    406.into(),
                    1.into(),
                    TokenValue::Omitted,
                    1.into(),
                    3.into(),
                    0.into(),
                ],
            ],
        ),
        (
            22,
            11,
            vec![
                vec![
                    406.into(),
                    9.into(),
                    1.into(),
                    1.into(),
                    1.into(),
                    10.0.into(),
                    20.0.into(),
                    1.5.into(),
                    2.5.into(),
                    3.into(),
                    4.into(),
                    1.into(),
                    3.into(),
                    0.into(),
                ],
                vec![
                    406.into(),
                    8.into(),
                    1.into(),
                    1.into(),
                    1.into(),
                    10.0.into(),
                    20.0.into(),
                    1.5.into(),
                    2.5.into(),
                    3.into(),
                    4.into(),
                    1.into(),
                    3.into(),
                    0.into(),
                ],
                vec![
                    406.into(),
                    TokenValue::Omitted,
                    1.into(),
                    1.into(),
                    1.into(),
                    10.0.into(),
                    20.0.into(),
                    1.5.into(),
                    2.5.into(),
                    3.into(),
                    4.into(),
                    1.into(),
                    3.into(),
                    0.into(),
                ],
                vec![
                    406.into(),
                    9.into(),
                    1.into(),
                    TokenValue::Omitted,
                    1.into(),
                    10.0.into(),
                    20.0.into(),
                    1.5.into(),
                    2.5.into(),
                    3.into(),
                    4.into(),
                    1.into(),
                    3.into(),
                    0.into(),
                ],
            ],
        ),
        (
            23,
            4,
            vec![
                vec![
                    406.into(),
                    2.into(),
                    3.into(),
                    TokenValue::String(b"DIPS".to_vec()),
                    1.into(),
                    3.into(),
                    0.into(),
                ],
                vec![
                    406.into(),
                    1.into(),
                    3.into(),
                    TokenValue::String(b"DIPS".to_vec()),
                    1.into(),
                    3.into(),
                    0.into(),
                ],
                vec![
                    406.into(),
                    TokenValue::Omitted,
                    3.into(),
                    TokenValue::String(b"DIPS".to_vec()),
                    1.into(),
                    3.into(),
                    0.into(),
                ],
                vec![
                    406.into(),
                    2.into(),
                    3.into(),
                    TokenValue::Omitted,
                    1.into(),
                    3.into(),
                    0.into(),
                ],
            ],
        ),
    ] {
        let mut source = directory_target(1, 406);
        source.form = form;
        let directory = BTreeMap::from([(1, &source), (3, &association)]);
        for values in cases {
            let analysis =
                analyze_trailing_pointer_groups(&token_parameter_record(1, values), &directory);
            assert_eq!(analysis.candidate_count, 1, "Form {form}");
            assert_eq!(analysis.valid_candidate_count, 1, "Form {form}");
            let groups = analysis.groups.expect("fixed property table boundary");
            assert_eq!(groups.token_start, boundary, "Form {form}");
            assert_eq!(groups.associations, vec![3]);
            assert!(groups.properties.is_empty(), "Form {form}");
        }
    }
}

#[test]
fn type406_fixed_property_table_precedes_generic_candidates() {
    let association = directory_target(3, 212);
    for (form, boundary, values, alternate) in [
        (18_i64, 3, vec![406, 1, 25, 6, 3, 3, 3, 3, 3, 3, 0], 6),
        (20, 3, vec![406, 1, 1, 6, 3, 3, 3, 3, 3, 3, 0], 6),
        (21, 3, vec![406, 1, 0, 6, 3, 3, 3, 3, 3, 3, 0], 6),
        (
            22,
            11,
            vec![406, 9, 1, 1, 1, 10, 20, 1, 2, 3, 4, 6, 3, 3, 3, 3, 3, 3, 0],
            14,
        ),
        (23, 4, vec![406, 2, 3, 4, 6, 3, 3, 3, 3, 3, 3, 0], 7),
    ] {
        let mut source = directory_target(1, 406);
        source.form = form;
        let directory = BTreeMap::from([(1, &source), (3, &association)]);
        let record = integer_parameter_record(1, &values);
        let generic = structural_pointer_group_candidates(&record);
        assert!(
            generic
                .iter()
                .any(|candidate| candidate.token_start == boundary),
            "Form {form} fixed candidate"
        );
        assert!(
            generic
                .iter()
                .any(|candidate| candidate.token_start == alternate),
            "Form {form} generic candidate"
        );
        let analysis = analyze_trailing_pointer_groups(&record, &directory);
        assert_eq!(analysis.candidate_count, 1, "Form {form}");
        assert_eq!(analysis.valid_candidate_count, 1, "Form {form}");
        let groups = analysis.groups.expect("fixed property table boundary");
        assert_eq!(groups.token_start, boundary, "Form {form}");
        assert_eq!(groups.associations, vec![3; 6], "Form {form}");
        assert!(groups.properties.is_empty(), "Form {form}");
    }
}

#[test]
fn type406_fixed_property_truncation_suppresses_generic_recovery() {
    let association = directory_target(3, 212);
    for (form, cases) in [
        (
            18_i64,
            vec![
                vec![406.into(), 1.into(), 25.0.into()],
                vec![406.into(), 1.into(), 25.0.into(), 1.into(), 3.into()],
            ],
        ),
        (
            20,
            vec![
                vec![406.into(), 1.into(), 1.into()],
                vec![406.into(), 1.into(), 1.into(), 1.into(), 3.into()],
            ],
        ),
        (
            21,
            vec![
                vec![406.into(), 1.into(), 0.into()],
                vec![406.into(), 1.into(), 0.into(), 1.into(), 3.into()],
            ],
        ),
        (
            22,
            vec![
                vec![
                    406.into(),
                    9.into(),
                    1.into(),
                    1.into(),
                    1.into(),
                    10.0.into(),
                    20.0.into(),
                    1.5.into(),
                    2.5.into(),
                    3.into(),
                ],
                vec![
                    406.into(),
                    9.into(),
                    1.into(),
                    1.into(),
                    1.into(),
                    10.0.into(),
                    20.0.into(),
                    1.5.into(),
                    2.5.into(),
                    3.into(),
                    4.into(),
                    1.into(),
                    3.into(),
                ],
            ],
        ),
        (
            23,
            vec![
                vec![406.into(), 2.into(), 3.into()],
                vec![
                    406.into(),
                    2.into(),
                    3.into(),
                    TokenValue::String(b"DIPS".to_vec()),
                    1.into(),
                    3.into(),
                ],
            ],
        ),
    ] {
        let mut source = directory_target(1, 406);
        source.form = form;
        let directory = BTreeMap::from([(1, &source), (3, &association)]);
        for values in cases {
            let analysis =
                analyze_trailing_pointer_groups(&token_parameter_record(1, values), &directory);
            assert_eq!(analysis.candidate_count, 0, "Form {form}");
            assert_eq!(analysis.valid_candidate_count, 0, "Form {form}");
            assert!(analysis.groups.is_none(), "Form {form}");
        }
    }
}

#[test]
fn type406_form32_entity_table_boundary_follows_fixed_values() {
    let association = directory_target(3, 212);
    let mut source = directory_target(1, 406);
    source.form = 32;
    let directory = BTreeMap::from([(1, &source), (3, &association)]);
    for values in [
        vec![
            TokenValue::Integer(406),
            TokenValue::Integer(3),
            TokenValue::String(b"JANE".to_vec()),
            TokenValue::String(b"ENG".to_vec()),
            TokenValue::String(b"20260714.123456".to_vec()),
            TokenValue::Integer(1),
            TokenValue::Integer(3),
            TokenValue::Integer(0),
        ],
        vec![
            TokenValue::Integer(406),
            TokenValue::Integer(3),
            TokenValue::Integer(1),
            TokenValue::String(b"ENG".to_vec()),
            TokenValue::String(b"20260714.123456".to_vec()),
            TokenValue::Integer(1),
            TokenValue::Integer(3),
            TokenValue::Integer(0),
        ],
        vec![
            TokenValue::Integer(406),
            TokenValue::Integer(3),
            TokenValue::String(b"JANE".to_vec()),
            TokenValue::Integer(1),
            TokenValue::String(b"20260714.123456".to_vec()),
            TokenValue::Integer(1),
            TokenValue::Integer(3),
            TokenValue::Integer(0),
        ],
        vec![
            TokenValue::Integer(406),
            TokenValue::Integer(3),
            TokenValue::String(b"JANE".to_vec()),
            TokenValue::String(b"ENG".to_vec()),
            TokenValue::Integer(4),
            TokenValue::Integer(1),
            TokenValue::Integer(3),
            TokenValue::Integer(0),
        ],
    ] {
        let analysis =
            analyze_trailing_pointer_groups(&token_parameter_record(1, values), &directory);
        assert_eq!(analysis.candidate_count, 1);
        assert_eq!(analysis.valid_candidate_count, 1);
        let groups = analysis.groups.expect("Type 406 Form 32 table boundary");
        assert_eq!(groups.token_start, 5);
        assert_eq!(groups.associations, vec![3]);
        assert!(groups.properties.is_empty());
    }
}

#[test]
fn type406_form32_table_boundary_precedes_generic_candidate() {
    let association = directory_target(3, 212);
    let mut source = directory_target(1, 406);
    source.form = 32;
    let directory = BTreeMap::from([(1, &source), (3, &association)]);
    let record = token_parameter_record(
        1,
        vec![
            TokenValue::Integer(406),
            TokenValue::Integer(3),
            TokenValue::String(b"JANE".to_vec()),
            TokenValue::String(b"ENG".to_vec()),
            TokenValue::Integer(4),
            TokenValue::Integer(3),
            TokenValue::Integer(3),
            TokenValue::Integer(3),
            TokenValue::Integer(3),
            TokenValue::Integer(0),
        ],
    );
    let generic = structural_pointer_group_candidates(&record);
    assert!(generic.iter().any(|candidate| candidate.token_start == 4));
    assert!(generic.iter().any(|candidate| candidate.token_start == 5));

    let analysis = analyze_trailing_pointer_groups(&record, &directory);
    assert_eq!(analysis.candidate_count, 1);
    assert_eq!(analysis.valid_candidate_count, 1);
    let groups = analysis.groups.expect("Type 406 Form 32 table boundary");
    assert_eq!(groups.token_start, 5);
    assert_eq!(groups.associations, vec![3, 3, 3]);
    assert!(groups.properties.is_empty());
}

#[test]
fn type406_form32_malformed_np_or_span_does_not_enable_generic_recovery() {
    let association = directory_target(3, 212);
    let mut source = directory_target(1, 406);
    source.form = 32;
    let directory = BTreeMap::from([(1, &source), (3, &association)]);
    for values in [
        vec![
            TokenValue::Integer(406),
            TokenValue::Real(3.0),
            TokenValue::String(b"JANE".to_vec()),
            TokenValue::String(b"ENG".to_vec()),
            TokenValue::String(b"20260714.123456".to_vec()),
            TokenValue::Integer(1),
            TokenValue::Integer(3),
            TokenValue::Integer(0),
        ],
        vec![
            TokenValue::Integer(406),
            TokenValue::Omitted,
            TokenValue::String(b"JANE".to_vec()),
            TokenValue::String(b"ENG".to_vec()),
            TokenValue::String(b"20260714.123456".to_vec()),
            TokenValue::Integer(1),
            TokenValue::Integer(3),
            TokenValue::Integer(0),
        ],
        vec![
            TokenValue::Integer(406),
            TokenValue::Integer(0),
            TokenValue::String(b"JANE".to_vec()),
            TokenValue::String(b"ENG".to_vec()),
            TokenValue::String(b"20260714.123456".to_vec()),
            TokenValue::Integer(1),
            TokenValue::Integer(3),
            TokenValue::Integer(0),
        ],
        vec![
            TokenValue::Integer(406),
            TokenValue::Integer(2),
            TokenValue::String(b"JANE".to_vec()),
            TokenValue::String(b"ENG".to_vec()),
            TokenValue::String(b"20260714.123456".to_vec()),
            TokenValue::Integer(1),
            TokenValue::Integer(3),
            TokenValue::Integer(0),
        ],
        vec![
            TokenValue::Integer(406),
            TokenValue::Integer(3),
            TokenValue::String(b"JANE".to_vec()),
            TokenValue::String(b"ENG".to_vec()),
        ],
    ] {
        let record = token_parameter_record(1, values);
        let generic_count = structural_pointer_group_candidates(&record).len();
        let analysis = analyze_trailing_pointer_groups(&record, &directory);
        assert_eq!(analysis.candidate_count, 0, "generic_count={generic_count}");
        assert_eq!(analysis.valid_candidate_count, 0);
        assert!(analysis.groups.is_none());
    }
}

#[test]
fn type406_form33_entity_table_boundary_follows_fixed_values() {
    let association = directory_target(3, 212);
    let mut source = directory_target(1, 406);
    source.form = 33;
    let directory = BTreeMap::from([(1, &source), (3, &association)]);
    for values in [
        vec![
            TokenValue::Integer(406),
            TokenValue::Integer(2),
            TokenValue::Integer(2),
            TokenValue::String(b"C".to_vec()),
            TokenValue::Integer(1),
            TokenValue::Integer(3),
            TokenValue::Integer(0),
        ],
        vec![
            TokenValue::Integer(406),
            TokenValue::Integer(2),
            TokenValue::String(b"NO".to_vec()),
            TokenValue::String(b"C".to_vec()),
            TokenValue::Integer(1),
            TokenValue::Integer(3),
            TokenValue::Integer(0),
        ],
        vec![
            TokenValue::Integer(406),
            TokenValue::Integer(2),
            TokenValue::Integer(2),
            TokenValue::Integer(1),
            TokenValue::Integer(1),
            TokenValue::Integer(3),
            TokenValue::Integer(0),
        ],
        vec![
            TokenValue::Integer(406),
            TokenValue::Integer(2),
            TokenValue::Integer(2),
            TokenValue::Omitted,
            TokenValue::Integer(1),
            TokenValue::Integer(3),
            TokenValue::Integer(0),
        ],
    ] {
        let analysis =
            analyze_trailing_pointer_groups(&token_parameter_record(1, values), &directory);
        assert_eq!(analysis.candidate_count, 1);
        assert_eq!(analysis.valid_candidate_count, 1);
        let groups = analysis.groups.expect("Type 406 Form 33 table boundary");
        assert_eq!(groups.token_start, 4);
        assert_eq!(groups.associations, vec![3]);
        assert!(groups.properties.is_empty());
    }
}

#[test]
fn type406_form33_table_boundary_precedes_generic_candidate() {
    let association = directory_target(3, 212);
    let mut source = directory_target(1, 406);
    source.form = 33;
    let directory = BTreeMap::from([(1, &source), (3, &association)]);
    let record = token_parameter_record(
        1,
        vec![
            TokenValue::Integer(406),
            TokenValue::Integer(2),
            TokenValue::Integer(2),
            TokenValue::String(b"C".to_vec()),
            TokenValue::Integer(5),
            TokenValue::Integer(3),
            TokenValue::Integer(3),
            TokenValue::Integer(3),
            TokenValue::Integer(3),
            TokenValue::Integer(3),
            TokenValue::Integer(0),
        ],
    );
    let generic = structural_pointer_group_candidates(&record);
    assert!(generic.iter().any(|candidate| candidate.token_start == 4));
    assert!(generic.iter().any(|candidate| candidate.token_start == 6));

    let analysis = analyze_trailing_pointer_groups(&record, &directory);
    assert_eq!(analysis.candidate_count, 1);
    assert_eq!(analysis.valid_candidate_count, 1);
    let groups = analysis.groups.expect("Type 406 Form 33 table boundary");
    assert_eq!(groups.token_start, 4);
    assert_eq!(groups.associations, vec![3; 5]);
    assert!(groups.properties.is_empty());
}

#[test]
fn type406_form33_malformed_np_or_span_does_not_enable_generic_recovery() {
    let association = directory_target(3, 212);
    let mut source = directory_target(1, 406);
    source.form = 33;
    let directory = BTreeMap::from([(1, &source), (3, &association)]);
    for values in [
        vec![
            TokenValue::Integer(406),
            TokenValue::Real(2.0),
            TokenValue::Integer(2),
            TokenValue::String(b"C".to_vec()),
            TokenValue::Integer(1),
            TokenValue::Integer(3),
            TokenValue::Integer(0),
        ],
        vec![
            TokenValue::Integer(406),
            TokenValue::Omitted,
            TokenValue::Integer(2),
            TokenValue::String(b"C".to_vec()),
            TokenValue::Integer(1),
            TokenValue::Integer(3),
            TokenValue::Integer(0),
        ],
        vec![
            TokenValue::Integer(406),
            TokenValue::Integer(0),
            TokenValue::Integer(2),
            TokenValue::String(b"C".to_vec()),
            TokenValue::Integer(1),
            TokenValue::Integer(3),
            TokenValue::Integer(0),
        ],
        vec![
            TokenValue::Integer(406),
            TokenValue::Integer(-1),
            TokenValue::Integer(2),
            TokenValue::String(b"C".to_vec()),
            TokenValue::Integer(1),
            TokenValue::Integer(3),
            TokenValue::Integer(0),
        ],
        vec![
            TokenValue::Integer(406),
            TokenValue::Integer(3),
            TokenValue::Integer(2),
            TokenValue::String(b"C".to_vec()),
            TokenValue::Integer(1),
            TokenValue::Integer(3),
            TokenValue::Integer(0),
        ],
        vec![
            TokenValue::Integer(406),
            TokenValue::Integer(2),
            TokenValue::Integer(2),
        ],
        vec![
            TokenValue::Integer(406),
            TokenValue::Integer(2),
            TokenValue::Integer(2),
            TokenValue::String(b"C".to_vec()),
            TokenValue::Integer(1),
            TokenValue::Integer(3),
        ],
    ] {
        let record = token_parameter_record(1, values);
        let generic_count = structural_pointer_group_candidates(&record).len();
        let analysis = analyze_trailing_pointer_groups(&record, &directory);
        assert_eq!(analysis.candidate_count, 0, "generic_count={generic_count}");
        assert_eq!(analysis.valid_candidate_count, 0);
        assert!(analysis.groups.is_none());
    }
}

#[test]
fn type406_form2_entity_table_boundary_follows_fixed_values() {
    let mut source = directory_target(1, 406);
    source.form = 2;
    let association = directory_target(3, 212);
    let directory = BTreeMap::from([(1, &source), (3, &association)]);
    let record = integer_parameter_record(1, &[406, 3, 0, 1, 2, 1, 3, 0]);

    let analysis = analyze_trailing_pointer_groups(&record, &directory);
    assert_eq!(analysis.candidate_count, 1);
    assert_eq!(analysis.valid_candidate_count, 1);
    let groups = analysis.groups.expect("Type 406 Form 2 table boundary");
    assert_eq!(groups.token_start, 5);
    assert_eq!(groups.associations, vec![3]);
    assert!(groups.properties.is_empty());
}

#[test]
fn type406_form2_table_boundary_precedes_generic_candidate() {
    let mut source = directory_target(1, 406);
    source.form = 2;
    let association = directory_target(3, 212);
    let directory = BTreeMap::from([(1, &source), (3, &association)]);
    let record = integer_parameter_record(1, &[406, 3, 0, 1, 1, 3, 0]);

    let generic = structural_pointer_group_candidates(&record);
    assert_eq!(generic.len(), 1);
    assert_eq!(generic[0].token_start, 4);

    let analysis = analyze_trailing_pointer_groups(&record, &directory);
    assert_eq!(analysis.candidate_count, 0);
    assert_eq!(analysis.valid_candidate_count, 0);
    assert!(analysis.groups.is_none());
}

#[test]
fn type406_form2_malformed_np_or_span_does_not_enable_generic_recovery() {
    let mut source = directory_target(1, 406);
    source.form = 2;
    let association = directory_target(3, 212);
    let directory = BTreeMap::from([(1, &source), (3, &association)]);
    for values in [
        vec![406, 2, 0, 1, 1, 3, 0],
        vec![406, 4, 0, 1, 2, 3, 0],
        vec![406, 3, 0, 1],
    ] {
        let record = integer_parameter_record(1, &values);
        let analysis = analyze_trailing_pointer_groups(&record, &directory);
        assert_eq!(analysis.candidate_count, 0, "values={values:?}");
        assert_eq!(analysis.valid_candidate_count, 0, "values={values:?}");
        assert!(analysis.groups.is_none(), "values={values:?}");
    }

    let mut record = integer_parameter_record(1, &[406, 3, 0, 1, 2, 1, 3, 0]);
    record.tokens[1].value = TokenValue::Real(3.0);
    let analysis = analyze_trailing_pointer_groups(&record, &directory);
    assert_eq!(analysis.candidate_count, 0);
    assert_eq!(analysis.valid_candidate_count, 0);
    assert!(analysis.groups.is_none());
}

#[test]
fn type406_form3_entity_table_boundary_follows_fixed_values() {
    let mut source = directory_target(1, 406);
    source.form = 3;
    let association = directory_target(3, 212);
    let directory = BTreeMap::from([(1, &source), (3, &association)]);
    let record = token_parameter_record(
        1,
        vec![
            TokenValue::Integer(406),
            TokenValue::Integer(2),
            TokenValue::Integer(17),
            TokenValue::String(b"POWER".to_vec()),
            TokenValue::Integer(1),
            TokenValue::Integer(3),
            TokenValue::Integer(0),
        ],
    );

    let analysis = analyze_trailing_pointer_groups(&record, &directory);
    assert_eq!(analysis.candidate_count, 1);
    assert_eq!(analysis.valid_candidate_count, 1);
    let groups = analysis.groups.expect("Type 406 Form 3 table boundary");
    assert_eq!(groups.token_start, 4);
    assert_eq!(groups.associations, vec![3]);
    assert!(groups.properties.is_empty());
}

#[test]
fn type406_form3_table_boundary_precedes_generic_candidate() {
    let mut source = directory_target(1, 406);
    source.form = 3;
    let association = directory_target(3, 212);
    let directory = BTreeMap::from([(1, &source), (3, &association)]);
    let record = integer_parameter_record(1, &[406, 2, 1, 3, 0]);

    let generic = structural_pointer_group_candidates(&record);
    assert!(generic.iter().any(|candidate| candidate.token_start == 2));

    let analysis = analyze_trailing_pointer_groups(&record, &directory);
    assert_eq!(analysis.candidate_count, 0);
    assert_eq!(analysis.valid_candidate_count, 0);
    assert!(analysis.groups.is_none());
}

#[test]
fn type406_form3_malformed_np_or_span_does_not_enable_generic_recovery() {
    let mut source = directory_target(1, 406);
    source.form = 3;
    let association = directory_target(3, 212);
    let directory = BTreeMap::from([(1, &source), (3, &association)]);
    for values in [
        vec![
            TokenValue::Integer(406),
            TokenValue::Integer(1),
            TokenValue::Integer(17),
            TokenValue::String(b"POWER".to_vec()),
            TokenValue::Integer(1),
            TokenValue::Integer(3),
            TokenValue::Integer(0),
        ],
        vec![
            TokenValue::Integer(406),
            TokenValue::Integer(2),
            TokenValue::Integer(17),
        ],
        vec![
            TokenValue::Integer(406),
            TokenValue::Integer(2),
            TokenValue::Integer(17),
            TokenValue::Integer(1),
            TokenValue::Integer(3),
            TokenValue::Integer(0),
        ],
    ] {
        let generic_count =
            structural_pointer_group_candidates(&token_parameter_record(1, values.clone())).len();
        let record = token_parameter_record(1, values);
        let analysis = analyze_trailing_pointer_groups(&record, &directory);
        assert_eq!(analysis.candidate_count, 0, "generic_count={generic_count}");
        assert_eq!(analysis.valid_candidate_count, 0);
        assert!(analysis.groups.is_none());
    }
}

#[test]
fn type406_form8_entity_table_boundary_follows_fixed_values() {
    let mut source = directory_target(1, 406);
    source.form = 8;
    let association = directory_target(3, 212);
    let directory = BTreeMap::from([(1, &source), (3, &association)]);
    for pin_number in [TokenValue::String(b"PA7".to_vec()), TokenValue::Integer(17)] {
        let record = token_parameter_record(
            1,
            vec![
                TokenValue::Integer(406),
                TokenValue::Integer(1),
                pin_number,
                TokenValue::Integer(1),
                TokenValue::Integer(3),
                TokenValue::Integer(0),
            ],
        );

        let analysis = analyze_trailing_pointer_groups(&record, &directory);
        assert_eq!(analysis.candidate_count, 1);
        assert_eq!(analysis.valid_candidate_count, 1);
        let groups = analysis.groups.expect("Type 406 Form 8 table boundary");
        assert_eq!(groups.token_start, 3);
        assert_eq!(groups.associations, vec![3]);
        assert!(groups.properties.is_empty());
    }
}

#[test]
fn type406_form8_table_boundary_precedes_generic_candidate() {
    let mut source = directory_target(1, 406);
    source.form = 8;
    let association = directory_target(3, 212);
    let directory = BTreeMap::from([(1, &source), (3, &association)]);
    let record = integer_parameter_record(1, &[406, 1, 1, 3, 0]);

    let generic = structural_pointer_group_candidates(&record);
    assert!(generic.iter().any(|candidate| candidate.token_start == 2));

    let analysis = analyze_trailing_pointer_groups(&record, &directory);
    assert_eq!(analysis.candidate_count, 0);
    assert_eq!(analysis.valid_candidate_count, 0);
    assert!(analysis.groups.is_none());
}

#[test]
fn type406_form8_malformed_np_or_span_does_not_enable_generic_recovery() {
    let mut source = directory_target(1, 406);
    source.form = 8;
    let association = directory_target(3, 212);
    let directory = BTreeMap::from([(1, &source), (3, &association)]);
    for values in [
        vec![
            TokenValue::Integer(406),
            TokenValue::Integer(2),
            TokenValue::String(b"PA7".to_vec()),
            TokenValue::Integer(1),
            TokenValue::Integer(3),
            TokenValue::Integer(0),
        ],
        vec![TokenValue::Integer(406), TokenValue::Integer(1)],
    ] {
        let generic_count =
            structural_pointer_group_candidates(&token_parameter_record(1, values.clone())).len();
        let record = token_parameter_record(1, values);
        let analysis = analyze_trailing_pointer_groups(&record, &directory);
        assert_eq!(analysis.candidate_count, 0, "generic_count={generic_count}");
        assert_eq!(analysis.valid_candidate_count, 0);
        assert!(analysis.groups.is_none());
    }
}

#[test]
fn type406_form9_entity_table_boundary_follows_fixed_values() {
    let mut source = directory_target(1, 406);
    source.form = 9;
    let association = directory_target(3, 212);
    let directory = BTreeMap::from([(1, &source), (3, &association)]);
    for first_number in [
        TokenValue::String(b"GENERIC".to_vec()),
        TokenValue::Integer(1),
    ] {
        let record = token_parameter_record(
            1,
            vec![
                TokenValue::Integer(406),
                TokenValue::Integer(4),
                first_number,
                TokenValue::String(b"MIL123".to_vec()),
                TokenValue::String(b"VEND42".to_vec()),
                TokenValue::String(b"INT99".to_vec()),
                TokenValue::Integer(1),
                TokenValue::Integer(3),
                TokenValue::Integer(0),
            ],
        );

        let analysis = analyze_trailing_pointer_groups(&record, &directory);
        assert_eq!(analysis.candidate_count, 1);
        assert_eq!(analysis.valid_candidate_count, 1);
        let groups = analysis.groups.expect("Type 406 Form 9 table boundary");
        assert_eq!(groups.token_start, 6);
        assert_eq!(groups.associations, vec![3]);
        assert!(groups.properties.is_empty());
    }
}

#[test]
fn type406_form9_table_boundary_precedes_generic_candidate() {
    let mut source = directory_target(1, 406);
    source.form = 9;
    let association = directory_target(3, 212);
    let directory = BTreeMap::from([(1, &source), (3, &association)]);
    let record = token_parameter_record(
        1,
        vec![
            TokenValue::Integer(406),
            TokenValue::Integer(4),
            TokenValue::String(b"GENERIC".to_vec()),
            TokenValue::String(b"MIL123".to_vec()),
            TokenValue::String(b"VEND42".to_vec()),
            TokenValue::Integer(1),
            TokenValue::Integer(3),
            TokenValue::Integer(0),
        ],
    );

    let generic = structural_pointer_group_candidates(&record);
    assert!(generic.iter().any(|candidate| candidate.token_start == 5));

    let analysis = analyze_trailing_pointer_groups(&record, &directory);
    assert_eq!(analysis.candidate_count, 0);
    assert_eq!(analysis.valid_candidate_count, 0);
    assert!(analysis.groups.is_none());
}

#[test]
fn type406_form9_malformed_np_or_span_does_not_enable_generic_recovery() {
    let mut source = directory_target(1, 406);
    source.form = 9;
    let association = directory_target(3, 212);
    let directory = BTreeMap::from([(1, &source), (3, &association)]);
    for values in [
        vec![
            TokenValue::Integer(406),
            TokenValue::Integer(3),
            TokenValue::String(b"GENERIC".to_vec()),
            TokenValue::String(b"MIL123".to_vec()),
            TokenValue::String(b"VEND42".to_vec()),
            TokenValue::Integer(1),
            TokenValue::Integer(3),
            TokenValue::Integer(0),
        ],
        vec![
            TokenValue::Integer(406),
            TokenValue::Integer(4),
            TokenValue::String(b"GENERIC".to_vec()),
            TokenValue::String(b"MIL123".to_vec()),
            TokenValue::String(b"VEND42".to_vec()),
        ],
    ] {
        let generic_count =
            structural_pointer_group_candidates(&token_parameter_record(1, values.clone())).len();
        let record = token_parameter_record(1, values);
        let analysis = analyze_trailing_pointer_groups(&record, &directory);
        assert_eq!(analysis.candidate_count, 0, "generic_count={generic_count}");
        assert_eq!(analysis.valid_candidate_count, 0);
        assert!(analysis.groups.is_none());
    }
}

#[test]
fn type406_form10_entity_table_boundary_follows_fixed_values() {
    let mut source = directory_target(1, 406);
    source.form = 10;
    let association = directory_target(3, 212);
    let directory = BTreeMap::from([(1, &source), (3, &association)]);
    for first_value in [
        TokenValue::Integer(1),
        TokenValue::Integer(2),
        TokenValue::String(b"1".to_vec()),
    ] {
        let record = token_parameter_record(
            1,
            vec![
                TokenValue::Integer(406),
                TokenValue::Integer(6),
                first_value,
                TokenValue::Integer(0),
                TokenValue::Integer(1),
                TokenValue::Integer(0),
                TokenValue::Integer(1),
                TokenValue::Integer(0),
                TokenValue::Integer(1),
                TokenValue::Integer(3),
                TokenValue::Integer(0),
            ],
        );

        let analysis = analyze_trailing_pointer_groups(&record, &directory);
        assert_eq!(analysis.candidate_count, 1);
        assert_eq!(analysis.valid_candidate_count, 1);
        let groups = analysis.groups.expect("Type 406 Form 10 table boundary");
        assert_eq!(groups.token_start, 8);
        assert_eq!(groups.associations, vec![3]);
        assert!(groups.properties.is_empty());
    }
}

#[test]
fn type406_form10_table_boundary_precedes_generic_candidate() {
    let mut source = directory_target(1, 406);
    source.form = 10;
    let association = directory_target(3, 212);
    let directory = BTreeMap::from([(1, &source), (3, &association)]);
    let record = token_parameter_record(
        1,
        vec![
            TokenValue::Integer(406),
            TokenValue::Integer(6),
            TokenValue::Integer(1),
            TokenValue::Integer(0),
            TokenValue::Integer(1),
            TokenValue::Integer(0),
            TokenValue::Integer(1),
            TokenValue::Integer(1),
            TokenValue::Integer(3),
            TokenValue::Integer(0),
        ],
    );

    let generic = structural_pointer_group_candidates(&record);
    assert!(generic.iter().any(|candidate| candidate.token_start == 7));

    let analysis = analyze_trailing_pointer_groups(&record, &directory);
    assert_eq!(analysis.candidate_count, 0);
    assert_eq!(analysis.valid_candidate_count, 0);
    assert!(analysis.groups.is_none());
}

#[test]
fn type406_form10_malformed_np_or_span_does_not_enable_generic_recovery() {
    let mut source = directory_target(1, 406);
    source.form = 10;
    let association = directory_target(3, 212);
    let directory = BTreeMap::from([(1, &source), (3, &association)]);
    for values in [
        vec![
            TokenValue::Integer(406),
            TokenValue::Integer(5),
            TokenValue::Integer(1),
            TokenValue::Integer(0),
            TokenValue::Integer(1),
            TokenValue::Integer(0),
            TokenValue::Integer(1),
            TokenValue::Integer(1),
            TokenValue::Integer(3),
            TokenValue::Integer(0),
        ],
        vec![
            TokenValue::Integer(406),
            TokenValue::Integer(6),
            TokenValue::Integer(1),
            TokenValue::Integer(0),
            TokenValue::Integer(1),
            TokenValue::Integer(0),
            TokenValue::Integer(1),
        ],
    ] {
        let generic_count =
            structural_pointer_group_candidates(&token_parameter_record(1, values.clone())).len();
        let record = token_parameter_record(1, values);
        let analysis = analyze_trailing_pointer_groups(&record, &directory);
        assert_eq!(analysis.candidate_count, 0, "generic_count={generic_count}");
        assert_eq!(analysis.valid_candidate_count, 0);
        assert!(analysis.groups.is_none());
    }
}

#[test]
fn type406_form13_entity_table_boundary_follows_conditional_values() {
    let mut source = directory_target(1, 406);
    source.form = 13;
    let association = directory_target(3, 212);
    let directory = BTreeMap::from([(1, &source), (3, &association)]);
    for (np, values, expected_start) in [
        (
            2,
            vec![
                TokenValue::Integer(406),
                TokenValue::Integer(2),
                TokenValue::Real(2.5),
                TokenValue::String(b"AWG".to_vec()),
                TokenValue::Integer(1),
                TokenValue::Integer(3),
                TokenValue::Integer(0),
            ],
            4,
        ),
        (
            3,
            vec![
                TokenValue::Integer(406),
                TokenValue::Integer(3),
                TokenValue::Real(2.5),
                TokenValue::String(b"AWG".to_vec()),
                TokenValue::String(b"ANSI123".to_vec()),
                TokenValue::Integer(1),
                TokenValue::Integer(3),
                TokenValue::Integer(0),
            ],
            5,
        ),
        (
            2,
            vec![
                TokenValue::Integer(406),
                TokenValue::Integer(2),
                TokenValue::String(b"2HNO".to_vec()),
                TokenValue::String(b"AWG".to_vec()),
                TokenValue::Integer(1),
                TokenValue::Integer(3),
                TokenValue::Integer(0),
            ],
            4,
        ),
    ] {
        assert_eq!(values[1], TokenValue::Integer(np));
        let analysis =
            analyze_trailing_pointer_groups(&token_parameter_record(1, values), &directory);
        assert_eq!(analysis.candidate_count, 1);
        assert_eq!(analysis.valid_candidate_count, 1);
        let groups = analysis.groups.expect("Type 406 Form 13 table boundary");
        assert_eq!(groups.token_start, expected_start);
        assert_eq!(groups.associations, vec![3]);
        assert!(groups.properties.is_empty());
    }
}

#[test]
fn type406_form13_table_boundary_precedes_generic_candidate() {
    let mut source = directory_target(1, 406);
    source.form = 13;
    let association = directory_target(3, 212);
    let directory = BTreeMap::from([(1, &source), (3, &association)]);
    for values in [
        vec![
            TokenValue::Integer(406),
            TokenValue::Integer(2),
            TokenValue::Real(2.5),
            TokenValue::Integer(1),
            TokenValue::Integer(3),
            TokenValue::Integer(0),
        ],
        vec![
            TokenValue::Integer(406),
            TokenValue::Integer(3),
            TokenValue::Real(2.5),
            TokenValue::String(b"AWG".to_vec()),
            TokenValue::Integer(1),
            TokenValue::Integer(3),
            TokenValue::Integer(0),
        ],
    ] {
        let generic =
            structural_pointer_group_candidates(&token_parameter_record(1, values.clone()));
        let expected_generic_start = if values[1] == TokenValue::Integer(2) {
            3
        } else {
            4
        };
        assert!(generic
            .iter()
            .any(|candidate| candidate.token_start == expected_generic_start));

        let analysis =
            analyze_trailing_pointer_groups(&token_parameter_record(1, values), &directory);
        assert_eq!(analysis.candidate_count, 0);
        assert_eq!(analysis.valid_candidate_count, 0);
        assert!(analysis.groups.is_none());
    }
}

#[test]
fn type406_form13_malformed_np_or_span_does_not_enable_generic_recovery() {
    let mut source = directory_target(1, 406);
    source.form = 13;
    let association = directory_target(3, 212);
    let directory = BTreeMap::from([(1, &source), (3, &association)]);
    for values in [
        vec![
            TokenValue::Integer(406),
            TokenValue::Integer(4),
            TokenValue::Real(2.5),
            TokenValue::String(b"AWG".to_vec()),
            TokenValue::Integer(1),
            TokenValue::Integer(3),
            TokenValue::Integer(0),
        ],
        vec![
            TokenValue::Integer(406),
            TokenValue::Integer(3),
            TokenValue::Real(2.5),
            TokenValue::String(b"AWG".to_vec()),
        ],
    ] {
        let generic_count =
            structural_pointer_group_candidates(&token_parameter_record(1, values.clone())).len();
        let analysis =
            analyze_trailing_pointer_groups(&token_parameter_record(1, values), &directory);
        assert_eq!(analysis.candidate_count, 0, "generic_count={generic_count}");
        assert_eq!(analysis.valid_candidate_count, 0);
        assert!(analysis.groups.is_none());
    }
}

#[test]
fn type406_form14_entity_table_boundary_follows_string_list() {
    let mut source = directory_target(1, 406);
    source.form = 14;
    let association = directory_target(3, 212);
    let directory = BTreeMap::from([(1, &source), (3, &association)]);
    for (values, expected_start) in [
        (
            vec![
                TokenValue::Integer(406),
                TokenValue::Integer(1),
                TokenValue::String(b"FLOW".to_vec()),
                TokenValue::Integer(1),
                TokenValue::Integer(3),
                TokenValue::Integer(0),
            ],
            3,
        ),
        (
            vec![
                TokenValue::Integer(406),
                TokenValue::Integer(2),
                TokenValue::String(b"FLOW".to_vec()),
                TokenValue::String(b"MOD".to_vec()),
                TokenValue::Integer(1),
                TokenValue::Integer(3),
                TokenValue::Integer(0),
            ],
            4,
        ),
        (
            vec![
                TokenValue::Integer(406),
                TokenValue::Integer(2),
                TokenValue::String(b"FLOW".to_vec()),
                TokenValue::Omitted,
                TokenValue::Integer(1),
                TokenValue::Integer(3),
                TokenValue::Integer(0),
            ],
            4,
        ),
        (
            vec![
                TokenValue::Integer(406),
                TokenValue::Integer(1),
                TokenValue::Integer(7),
                TokenValue::Integer(1),
                TokenValue::Integer(3),
                TokenValue::Integer(0),
            ],
            3,
        ),
    ] {
        let analysis =
            analyze_trailing_pointer_groups(&token_parameter_record(1, values), &directory);
        assert_eq!(analysis.candidate_count, 1);
        assert_eq!(analysis.valid_candidate_count, 1);
        let groups = analysis.groups.expect("Type 406 Form 14 table boundary");
        assert_eq!(groups.token_start, expected_start);
        assert_eq!(groups.associations, vec![3]);
        assert!(groups.properties.is_empty());
    }
}

#[test]
fn type406_form14_table_boundary_precedes_generic_candidate() {
    let mut source = directory_target(1, 406);
    source.form = 14;
    let association = directory_target(3, 212);
    let directory = BTreeMap::from([(1, &source), (3, &association)]);
    let values = vec![
        TokenValue::Integer(406),
        TokenValue::Integer(2),
        TokenValue::String(b"FLOW".to_vec()),
        TokenValue::String(b"MOD".to_vec()),
        TokenValue::Integer(5),
        TokenValue::Integer(3),
        TokenValue::Integer(3),
        TokenValue::Integer(3),
        TokenValue::Integer(3),
        TokenValue::Integer(3),
        TokenValue::Integer(0),
    ];
    let record = token_parameter_record(1, values);
    let generic = structural_pointer_group_candidates(&record);
    assert!(generic.iter().any(|candidate| candidate.token_start == 4));
    assert!(generic.iter().any(|candidate| candidate.token_start == 6));

    let analysis = analyze_trailing_pointer_groups(&record, &directory);
    assert_eq!(analysis.candidate_count, 1);
    assert_eq!(analysis.valid_candidate_count, 1);
    let groups = analysis.groups.expect("Type 406 Form 14 table boundary");
    assert_eq!(groups.token_start, 4);
    assert_eq!(groups.associations, vec![3; 5]);
    assert!(groups.properties.is_empty());
}

#[test]
fn type406_form14_malformed_count_or_span_does_not_enable_generic_recovery() {
    let mut source = directory_target(1, 406);
    source.form = 14;
    let association = directory_target(3, 212);
    let directory = BTreeMap::from([(1, &source), (3, &association)]);
    for values in [
        vec![
            TokenValue::Integer(406),
            TokenValue::Real(1.0),
            TokenValue::String(b"FLOW".to_vec()),
            TokenValue::Integer(1),
            TokenValue::Integer(3),
            TokenValue::Integer(0),
        ],
        vec![
            TokenValue::Integer(406),
            TokenValue::Omitted,
            TokenValue::String(b"FLOW".to_vec()),
            TokenValue::Integer(1),
            TokenValue::Integer(3),
            TokenValue::Integer(0),
        ],
        vec![
            TokenValue::Integer(406),
            TokenValue::Integer(0),
            TokenValue::String(b"FLOW".to_vec()),
            TokenValue::Integer(1),
            TokenValue::Integer(3),
            TokenValue::Integer(0),
        ],
        vec![
            TokenValue::Integer(406),
            TokenValue::Integer(-1),
            TokenValue::String(b"FLOW".to_vec()),
            TokenValue::Integer(1),
            TokenValue::Integer(3),
            TokenValue::Integer(0),
        ],
        vec![
            TokenValue::Integer(406),
            TokenValue::Integer(3),
            TokenValue::String(b"FLOW".to_vec()),
            TokenValue::String(b"MOD".to_vec()),
            TokenValue::Integer(1),
            TokenValue::Integer(3),
            TokenValue::Integer(0),
        ],
        vec![
            TokenValue::Integer(406),
            TokenValue::Integer(2),
            TokenValue::String(b"FLOW".to_vec()),
        ],
        vec![
            TokenValue::Integer(406),
            TokenValue::Integer(1),
            TokenValue::String(b"FLOW".to_vec()),
            TokenValue::Integer(1),
            TokenValue::Integer(3),
        ],
    ] {
        let record = token_parameter_record(1, values);
        let generic_count = structural_pointer_group_candidates(&record).len();
        let analysis = analyze_trailing_pointer_groups(&record, &directory);
        assert_eq!(analysis.candidate_count, 0, "generic_count={generic_count}");
        assert_eq!(analysis.valid_candidate_count, 0);
        assert!(analysis.groups.is_none());
    }
}

#[test]
fn type406_form15_entity_table_boundary_follows_fixed_values() {
    let mut source = directory_target(1, 406);
    source.form = 15;
    let association = directory_target(3, 212);
    let directory = BTreeMap::from([(1, &source), (3, &association)]);
    for name in [
        TokenValue::String(b"USERNM".to_vec()),
        TokenValue::Integer(1),
    ] {
        let record = token_parameter_record(
            1,
            vec![
                TokenValue::Integer(406),
                TokenValue::Integer(1),
                name,
                TokenValue::Integer(1),
                TokenValue::Integer(3),
                TokenValue::Integer(0),
            ],
        );

        let analysis = analyze_trailing_pointer_groups(&record, &directory);
        assert_eq!(analysis.candidate_count, 1);
        assert_eq!(analysis.valid_candidate_count, 1);
        let groups = analysis.groups.expect("Type 406 Form 15 table boundary");
        assert_eq!(groups.token_start, 3);
        assert_eq!(groups.associations, vec![3]);
        assert!(groups.properties.is_empty());
    }
}

#[test]
fn type406_form15_table_boundary_precedes_generic_candidate() {
    let mut source = directory_target(1, 406);
    source.form = 15;
    let association = directory_target(3, 212);
    let directory = BTreeMap::from([(1, &source), (3, &association)]);
    let values = vec![
        TokenValue::Integer(406),
        TokenValue::Integer(1),
        TokenValue::Integer(1),
        TokenValue::Integer(3),
        TokenValue::Integer(0),
    ];
    let generic = structural_pointer_group_candidates(&token_parameter_record(1, values.clone()));
    assert!(generic.iter().any(|candidate| candidate.token_start == 2));

    let analysis = analyze_trailing_pointer_groups(&token_parameter_record(1, values), &directory);
    assert_eq!(analysis.candidate_count, 0);
    assert_eq!(analysis.valid_candidate_count, 0);
    assert!(analysis.groups.is_none());
}

#[test]
fn type406_form15_malformed_np_or_span_does_not_enable_generic_recovery() {
    let mut source = directory_target(1, 406);
    source.form = 15;
    let association = directory_target(3, 212);
    let directory = BTreeMap::from([(1, &source), (3, &association)]);
    for values in [
        vec![
            TokenValue::Integer(406),
            TokenValue::Integer(2),
            TokenValue::String(b"USERNM".to_vec()),
            TokenValue::Integer(1),
            TokenValue::Integer(3),
            TokenValue::Integer(0),
        ],
        vec![
            TokenValue::Integer(406),
            TokenValue::Integer(1),
            TokenValue::String(b"USERNM".to_vec()),
        ],
        vec![TokenValue::Integer(406), TokenValue::Integer(1)],
    ] {
        let generic_count =
            structural_pointer_group_candidates(&token_parameter_record(1, values.clone())).len();
        let analysis =
            analyze_trailing_pointer_groups(&token_parameter_record(1, values), &directory);
        assert_eq!(analysis.candidate_count, 0, "generic_count={generic_count}");
        assert_eq!(analysis.valid_candidate_count, 0);
        assert!(analysis.groups.is_none());
    }
}

#[test]
fn type406_form24_entity_table_boundary_follows_definition_lists() {
    let mut source = directory_target(1, 406);
    source.form = 24;
    let association = directory_target(3, 212);
    let directory = BTreeMap::from([(1, &source), (3, &association)]);
    for (values, expected_start) in [
        (
            vec![
                TokenValue::Integer(406),
                TokenValue::Integer(5),
                TokenValue::Integer(1),
                TokenValue::Integer(1),
                TokenValue::String(b"TOP1".to_vec()),
                TokenValue::Integer(1),
                TokenValue::String(b"SIGNAL_T".to_vec()),
                TokenValue::Integer(1),
                TokenValue::Integer(3),
                TokenValue::Integer(0),
            ],
            7,
        ),
        (
            vec![
                TokenValue::Integer(406),
                TokenValue::Integer(9),
                TokenValue::Integer(2),
                TokenValue::Integer(10),
                TokenValue::String(b"TOP1".to_vec()),
                TokenValue::Integer(1),
                TokenValue::String(b"SIGNAL_T".to_vec()),
                TokenValue::Integer(20),
                TokenValue::String(b"CORE".to_vec()),
                TokenValue::Integer(0),
                TokenValue::String(b"UNDEFINED".to_vec()),
                TokenValue::Integer(1),
                TokenValue::Integer(3),
                TokenValue::Integer(0),
            ],
            11,
        ),
        (
            vec![
                TokenValue::Integer(406),
                TokenValue::Integer(5),
                TokenValue::Integer(1),
                TokenValue::Integer(1),
                TokenValue::String(b"TOP1".to_vec()),
                TokenValue::Integer(1),
                TokenValue::Integer(1),
                TokenValue::Integer(1),
                TokenValue::Integer(3),
                TokenValue::Integer(0),
            ],
            7,
        ),
    ] {
        let analysis =
            analyze_trailing_pointer_groups(&token_parameter_record(1, values), &directory);
        assert_eq!(analysis.candidate_count, 1);
        assert_eq!(analysis.valid_candidate_count, 1);
        let groups = analysis.groups.expect("Type 406 Form 24 table boundary");
        assert_eq!(groups.token_start, expected_start);
        assert_eq!(groups.associations, vec![3]);
        assert!(groups.properties.is_empty());
    }
}

#[test]
fn type406_form24_table_boundary_precedes_generic_candidate() {
    let mut source = directory_target(1, 406);
    source.form = 24;
    let association = directory_target(3, 212);
    let directory = BTreeMap::from([(1, &source), (3, &association)]);
    let values = vec![
        TokenValue::Integer(406),
        TokenValue::Integer(5),
        TokenValue::Integer(1),
        TokenValue::Integer(1),
        TokenValue::String(b"TOP1".to_vec()),
        TokenValue::Integer(1),
        TokenValue::Integer(1),
        TokenValue::Integer(3),
        TokenValue::Integer(0),
    ];
    let generic = structural_pointer_group_candidates(&token_parameter_record(1, values.clone()));
    assert!(generic.iter().any(|candidate| candidate.token_start == 6));

    let analysis = analyze_trailing_pointer_groups(&token_parameter_record(1, values), &directory);
    assert_eq!(analysis.candidate_count, 0);
    assert_eq!(analysis.valid_candidate_count, 0);
    assert!(analysis.groups.is_none());
}

#[test]
fn type406_form24_malformed_np_or_span_does_not_enable_generic_recovery() {
    let mut source = directory_target(1, 406);
    source.form = 24;
    let association = directory_target(3, 212);
    let directory = BTreeMap::from([(1, &source), (3, &association)]);
    for values in [
        vec![
            TokenValue::Integer(406),
            TokenValue::Integer(6),
            TokenValue::Integer(1),
            TokenValue::Integer(1),
            TokenValue::String(b"TOP1".to_vec()),
            TokenValue::Integer(1),
            TokenValue::String(b"SIGNAL_T".to_vec()),
            TokenValue::Integer(1),
            TokenValue::Integer(3),
            TokenValue::Integer(0),
        ],
        vec![
            TokenValue::Integer(406),
            TokenValue::Integer(5),
            TokenValue::Integer(0),
            TokenValue::Integer(1),
            TokenValue::Integer(3),
            TokenValue::Integer(0),
        ],
        vec![
            TokenValue::Integer(406),
            TokenValue::Integer(5),
            TokenValue::Integer(1),
            TokenValue::Integer(1),
            TokenValue::String(b"TOP1".to_vec()),
            TokenValue::Integer(1),
        ],
    ] {
        let generic_count =
            structural_pointer_group_candidates(&token_parameter_record(1, values.clone())).len();
        let analysis =
            analyze_trailing_pointer_groups(&token_parameter_record(1, values), &directory);
        assert_eq!(analysis.candidate_count, 0, "generic_count={generic_count}");
        assert_eq!(analysis.valid_candidate_count, 0);
        assert!(analysis.groups.is_none());
    }
}

#[test]
fn type406_form25_entity_table_boundary_follows_level_lists() {
    let mut source = directory_target(1, 406);
    source.form = 25;
    let association = directory_target(3, 212);
    let directory = BTreeMap::from([(1, &source), (3, &association)]);
    for (values, expected_start) in [
        (
            vec![
                TokenValue::Integer(406),
                TokenValue::Integer(3),
                TokenValue::String(b"BOARD".to_vec()),
                TokenValue::Integer(1),
                TokenValue::Integer(10),
                TokenValue::Integer(1),
                TokenValue::Integer(3),
                TokenValue::Integer(0),
            ],
            5,
        ),
        (
            vec![
                TokenValue::Integer(406),
                TokenValue::Integer(5),
                TokenValue::String(b"BOARD".to_vec()),
                TokenValue::Integer(3),
                TokenValue::Integer(10),
                TokenValue::Integer(20),
                TokenValue::Integer(30),
                TokenValue::Integer(1),
                TokenValue::Integer(3),
                TokenValue::Integer(0),
            ],
            7,
        ),
        (
            vec![
                TokenValue::Integer(406),
                TokenValue::Integer(3),
                TokenValue::String(b"BOARD".to_vec()),
                TokenValue::Integer(1),
                TokenValue::String(b"1HX".to_vec()),
                TokenValue::Integer(1),
                TokenValue::Integer(3),
                TokenValue::Integer(0),
            ],
            5,
        ),
    ] {
        let analysis =
            analyze_trailing_pointer_groups(&token_parameter_record(1, values), &directory);
        assert_eq!(analysis.candidate_count, 1);
        assert_eq!(analysis.valid_candidate_count, 1);
        let groups = analysis.groups.expect("Type 406 Form 25 table boundary");
        assert_eq!(groups.token_start, expected_start);
        assert_eq!(groups.associations, vec![3]);
        assert!(groups.properties.is_empty());
    }
}

#[test]
fn type406_form25_table_boundary_precedes_generic_candidate() {
    let mut source = directory_target(1, 406);
    source.form = 25;
    let association = directory_target(3, 212);
    let directory = BTreeMap::from([(1, &source), (3, &association)]);
    let values = vec![
        TokenValue::Integer(406),
        TokenValue::Integer(3),
        TokenValue::String(b"BOARD".to_vec()),
        TokenValue::Integer(1),
        TokenValue::Integer(1),
        TokenValue::Integer(3),
        TokenValue::Integer(0),
    ];
    let generic = structural_pointer_group_candidates(&token_parameter_record(1, values.clone()));
    assert!(generic.iter().any(|candidate| candidate.token_start == 4));

    let analysis = analyze_trailing_pointer_groups(&token_parameter_record(1, values), &directory);
    assert_eq!(analysis.candidate_count, 0);
    assert_eq!(analysis.valid_candidate_count, 0);
    assert!(analysis.groups.is_none());
}

#[test]
fn type406_form25_malformed_np_or_span_does_not_enable_generic_recovery() {
    let mut source = directory_target(1, 406);
    source.form = 25;
    let association = directory_target(3, 212);
    let directory = BTreeMap::from([(1, &source), (3, &association)]);
    for values in [
        vec![
            TokenValue::Integer(406),
            TokenValue::Integer(4),
            TokenValue::String(b"BOARD".to_vec()),
            TokenValue::Integer(1),
            TokenValue::Integer(10),
            TokenValue::Integer(1),
            TokenValue::Integer(3),
            TokenValue::Integer(0),
        ],
        vec![
            TokenValue::Integer(406),
            TokenValue::Integer(2),
            TokenValue::String(b"BOARD".to_vec()),
            TokenValue::Integer(0),
            TokenValue::Integer(1),
            TokenValue::Integer(3),
            TokenValue::Integer(0),
        ],
        vec![
            TokenValue::Integer(406),
            TokenValue::Integer(3),
            TokenValue::String(b"BOARD".to_vec()),
            TokenValue::Integer(1),
        ],
        vec![
            TokenValue::Integer(406),
            TokenValue::Integer(3),
            TokenValue::String(b"BOARD".to_vec()),
            TokenValue::Integer(1),
            TokenValue::Integer(10),
        ],
    ] {
        let generic_count =
            structural_pointer_group_candidates(&token_parameter_record(1, values.clone())).len();
        let analysis =
            analyze_trailing_pointer_groups(&token_parameter_record(1, values), &directory);
        assert_eq!(analysis.candidate_count, 0, "generic_count={generic_count}");
        assert_eq!(analysis.valid_candidate_count, 0);
        assert!(analysis.groups.is_none());
    }
}

#[test]
fn type406_form26_entity_table_boundary_follows_fixed_values() {
    let mut source = directory_target(1, 406);
    source.form = 26;
    let association = directory_target(3, 212);
    let directory = BTreeMap::from([(1, &source), (3, &association)]);
    for values in [
        vec![
            TokenValue::Integer(406),
            TokenValue::Integer(3),
            TokenValue::Real(0.8),
            TokenValue::Real(0.7),
            TokenValue::Integer(5),
            TokenValue::Integer(1),
            TokenValue::Integer(3),
            TokenValue::Integer(0),
        ],
        vec![
            TokenValue::Integer(406),
            TokenValue::Integer(3),
            TokenValue::String(b"BAD".to_vec()),
            TokenValue::Real(0.7),
            TokenValue::Integer(6),
            TokenValue::Integer(1),
            TokenValue::Integer(3),
            TokenValue::Integer(0),
        ],
    ] {
        let analysis =
            analyze_trailing_pointer_groups(&token_parameter_record(1, values), &directory);
        assert_eq!(analysis.candidate_count, 1);
        assert_eq!(analysis.valid_candidate_count, 1);
        let groups = analysis.groups.expect("Type 406 Form 26 table boundary");
        assert_eq!(groups.token_start, 5);
        assert_eq!(groups.associations, vec![3]);
        assert!(groups.properties.is_empty());
    }
}

#[test]
fn type406_form26_table_boundary_precedes_generic_candidate() {
    let mut source = directory_target(1, 406);
    source.form = 26;
    let association = directory_target(3, 212);
    let directory = BTreeMap::from([(1, &source), (3, &association)]);
    let values = vec![
        TokenValue::Integer(406),
        TokenValue::Integer(3),
        TokenValue::Real(0.8),
        TokenValue::Real(0.7),
        TokenValue::Integer(1),
        TokenValue::Integer(3),
        TokenValue::Integer(0),
    ];
    let generic = structural_pointer_group_candidates(&token_parameter_record(1, values.clone()));
    assert!(generic.iter().any(|candidate| candidate.token_start == 4));

    let analysis = analyze_trailing_pointer_groups(&token_parameter_record(1, values), &directory);
    assert_eq!(analysis.candidate_count, 0);
    assert_eq!(analysis.valid_candidate_count, 0);
    assert!(analysis.groups.is_none());
}

#[test]
fn type406_form26_malformed_np_or_span_does_not_enable_generic_recovery() {
    let mut source = directory_target(1, 406);
    source.form = 26;
    let association = directory_target(3, 212);
    let directory = BTreeMap::from([(1, &source), (3, &association)]);
    for values in [
        vec![
            TokenValue::Integer(406),
            TokenValue::Integer(4),
            TokenValue::Real(0.8),
            TokenValue::Real(0.7),
            TokenValue::Integer(5),
            TokenValue::Integer(1),
            TokenValue::Integer(3),
            TokenValue::Integer(0),
        ],
        vec![
            TokenValue::Integer(406),
            TokenValue::Omitted,
            TokenValue::Real(0.8),
            TokenValue::Real(0.7),
            TokenValue::Integer(5),
            TokenValue::Integer(1),
            TokenValue::Integer(3),
            TokenValue::Integer(0),
        ],
        vec![
            TokenValue::Integer(406),
            TokenValue::Integer(3),
            TokenValue::Real(0.8),
            TokenValue::Real(0.7),
        ],
    ] {
        let generic_count =
            structural_pointer_group_candidates(&token_parameter_record(1, values.clone())).len();
        let analysis =
            analyze_trailing_pointer_groups(&token_parameter_record(1, values), &directory);
        assert_eq!(analysis.candidate_count, 0, "generic_count={generic_count}");
        assert_eq!(analysis.valid_candidate_count, 0);
        assert!(analysis.groups.is_none());
    }
}

#[test]
fn type406_form28_entity_table_boundary_follows_fixed_values() {
    let mut source = directory_target(1, 406);
    source.form = 28;
    let association = directory_target(3, 212);
    let directory = BTreeMap::from([(1, &source), (3, &association)]);
    for values in [
        vec![
            TokenValue::Integer(406),
            TokenValue::Integer(6),
            TokenValue::Integer(0),
            TokenValue::Integer(2),
            TokenValue::Integer(1),
            TokenValue::String(b"MM".to_vec()),
            TokenValue::Integer(0),
            TokenValue::Integer(3),
            TokenValue::Integer(1),
            TokenValue::Integer(3),
            TokenValue::Integer(0),
        ],
        vec![
            TokenValue::Integer(406),
            TokenValue::Integer(6),
            TokenValue::Integer(0),
            TokenValue::Integer(2),
            TokenValue::Omitted,
            TokenValue::String(b"MM".to_vec()),
            TokenValue::Integer(0),
            TokenValue::Integer(3),
            TokenValue::Integer(1),
            TokenValue::Integer(3),
            TokenValue::Integer(0),
        ],
        vec![
            TokenValue::Integer(406),
            TokenValue::Integer(6),
            TokenValue::Integer(9),
            TokenValue::Integer(2),
            TokenValue::Integer(1),
            TokenValue::String(b"MM".to_vec()),
            TokenValue::Integer(2),
            TokenValue::Integer(3),
            TokenValue::Integer(1),
            TokenValue::Integer(3),
            TokenValue::Integer(0),
        ],
    ] {
        let analysis =
            analyze_trailing_pointer_groups(&token_parameter_record(1, values), &directory);
        assert_eq!(analysis.candidate_count, 1);
        assert_eq!(analysis.valid_candidate_count, 1);
        let groups = analysis.groups.expect("Type 406 Form 28 table boundary");
        assert_eq!(groups.token_start, 8);
        assert_eq!(groups.associations, vec![3]);
        assert!(groups.properties.is_empty());
    }
}

#[test]
fn type406_form28_table_boundary_precedes_generic_candidate() {
    let mut source = directory_target(1, 406);
    source.form = 28;
    let association = directory_target(3, 212);
    let directory = BTreeMap::from([(1, &source), (3, &association)]);
    let values = vec![
        TokenValue::Integer(406),
        TokenValue::Integer(6),
        TokenValue::Integer(0),
        TokenValue::Integer(2),
        TokenValue::Integer(1),
        TokenValue::String(b"MM".to_vec()),
        TokenValue::Integer(0),
        TokenValue::Integer(1),
        TokenValue::Integer(3),
        TokenValue::Integer(0),
    ];
    let generic = structural_pointer_group_candidates(&token_parameter_record(1, values.clone()));
    assert!(generic.iter().any(|candidate| candidate.token_start == 7));

    let analysis = analyze_trailing_pointer_groups(&token_parameter_record(1, values), &directory);
    assert_eq!(analysis.candidate_count, 0);
    assert_eq!(analysis.valid_candidate_count, 0);
    assert!(analysis.groups.is_none());
}

#[test]
fn type406_form28_malformed_np_or_span_does_not_enable_generic_recovery() {
    let mut source = directory_target(1, 406);
    source.form = 28;
    let association = directory_target(3, 212);
    let directory = BTreeMap::from([(1, &source), (3, &association)]);
    for values in [
        vec![
            TokenValue::Integer(406),
            TokenValue::Integer(7),
            TokenValue::Integer(0),
            TokenValue::Integer(2),
            TokenValue::Integer(1),
            TokenValue::String(b"MM".to_vec()),
            TokenValue::Integer(0),
            TokenValue::Integer(3),
            TokenValue::Integer(1),
            TokenValue::Integer(3),
            TokenValue::Integer(0),
        ],
        vec![
            TokenValue::Integer(406),
            TokenValue::Omitted,
            TokenValue::Integer(0),
            TokenValue::Integer(2),
            TokenValue::Integer(1),
            TokenValue::String(b"MM".to_vec()),
            TokenValue::Integer(0),
            TokenValue::Integer(3),
            TokenValue::Integer(1),
            TokenValue::Integer(3),
            TokenValue::Integer(0),
        ],
        vec![
            TokenValue::Integer(406),
            TokenValue::Integer(6),
            TokenValue::Integer(0),
            TokenValue::Integer(2),
            TokenValue::Integer(1),
            TokenValue::String(b"MM".to_vec()),
            TokenValue::Integer(0),
        ],
    ] {
        let generic_count =
            structural_pointer_group_candidates(&token_parameter_record(1, values.clone())).len();
        let analysis =
            analyze_trailing_pointer_groups(&token_parameter_record(1, values), &directory);
        assert_eq!(analysis.candidate_count, 0, "generic_count={generic_count}");
        assert_eq!(analysis.valid_candidate_count, 0);
        assert!(analysis.groups.is_none());
    }
}

#[test]
fn type406_form29_entity_table_boundary_follows_fixed_values() {
    let mut source = directory_target(1, 406);
    source.form = 29;
    let association = directory_target(3, 212);
    let directory = BTreeMap::from([(1, &source), (3, &association)]);
    for values in [
        vec![
            TokenValue::Integer(406),
            TokenValue::Integer(8),
            TokenValue::Integer(0),
            TokenValue::Integer(2),
            TokenValue::Integer(2),
            TokenValue::Real(0.1),
            TokenValue::Real(-0.1),
            TokenValue::Integer(0),
            TokenValue::Integer(0),
            TokenValue::Integer(3),
            TokenValue::Integer(1),
            TokenValue::Integer(3),
            TokenValue::Integer(0),
        ],
        vec![
            TokenValue::Integer(406),
            TokenValue::Integer(8),
            TokenValue::Integer(0),
            TokenValue::Integer(2),
            TokenValue::Omitted,
            TokenValue::Real(0.1),
            TokenValue::Real(-0.1),
            TokenValue::Integer(0),
            TokenValue::Integer(0),
            TokenValue::Integer(3),
            TokenValue::Integer(1),
            TokenValue::Integer(3),
            TokenValue::Integer(0),
        ],
        vec![
            TokenValue::Integer(406),
            TokenValue::Integer(8),
            TokenValue::Integer(0),
            TokenValue::String(b"2".to_vec()),
            TokenValue::Integer(2),
            TokenValue::Real(0.1),
            TokenValue::Real(-0.1),
            TokenValue::Integer(0),
            TokenValue::Integer(0),
            TokenValue::Integer(3),
            TokenValue::Integer(1),
            TokenValue::Integer(3),
            TokenValue::Integer(0),
        ],
    ] {
        let analysis =
            analyze_trailing_pointer_groups(&token_parameter_record(1, values), &directory);
        assert_eq!(analysis.candidate_count, 1);
        assert_eq!(analysis.valid_candidate_count, 1);
        let groups = analysis.groups.expect("Type 406 Form 29 table boundary");
        assert_eq!(groups.token_start, 10);
        assert_eq!(groups.associations, vec![3]);
        assert!(groups.properties.is_empty());
    }
}

#[test]
fn type406_form29_table_boundary_precedes_generic_candidate() {
    let mut source = directory_target(1, 406);
    source.form = 29;
    let association = directory_target(3, 212);
    let directory = BTreeMap::from([(1, &source), (3, &association)]);
    let values = vec![
        TokenValue::Integer(406),
        TokenValue::Integer(8),
        TokenValue::Integer(0),
        TokenValue::Integer(2),
        TokenValue::Integer(2),
        TokenValue::Real(0.1),
        TokenValue::Real(-0.1),
        TokenValue::Integer(0),
        TokenValue::Integer(0),
        TokenValue::Integer(3),
        TokenValue::Integer(1),
        TokenValue::Integer(3),
        TokenValue::Integer(0),
    ];
    let record = token_parameter_record(1, values);
    let generic = structural_pointer_group_candidates(&record);
    assert!(generic.iter().any(|candidate| candidate.token_start == 8));

    let analysis = analyze_trailing_pointer_groups(&record, &directory);
    assert_eq!(analysis.candidate_count, 1);
    assert_eq!(analysis.valid_candidate_count, 1);
    let groups = analysis.groups.expect("Type 406 Form 29 table boundary");
    assert_eq!(groups.token_start, 10);
    assert_eq!(groups.associations, vec![3]);
    assert!(groups.properties.is_empty());
}

#[test]
fn type406_form29_malformed_np_or_span_does_not_enable_generic_recovery() {
    let mut source = directory_target(1, 406);
    source.form = 29;
    let association = directory_target(3, 212);
    let directory = BTreeMap::from([(1, &source), (3, &association)]);
    for values in [
        vec![
            TokenValue::Integer(406),
            TokenValue::Integer(7),
            TokenValue::Integer(0),
            TokenValue::Integer(2),
            TokenValue::Integer(2),
            TokenValue::Real(0.1),
            TokenValue::Real(-0.1),
            TokenValue::Integer(0),
            TokenValue::Integer(0),
            TokenValue::Integer(3),
            TokenValue::Integer(1),
            TokenValue::Integer(3),
            TokenValue::Integer(0),
        ],
        vec![
            TokenValue::Integer(406),
            TokenValue::Omitted,
            TokenValue::Integer(0),
            TokenValue::Integer(2),
            TokenValue::Integer(2),
            TokenValue::Real(0.1),
            TokenValue::Real(-0.1),
            TokenValue::Integer(0),
            TokenValue::Integer(0),
            TokenValue::Integer(3),
            TokenValue::Integer(1),
            TokenValue::Integer(3),
            TokenValue::Integer(0),
        ],
        vec![
            TokenValue::Integer(406),
            TokenValue::Integer(8),
            TokenValue::Integer(0),
            TokenValue::Integer(2),
            TokenValue::Integer(2),
            TokenValue::Real(0.1),
            TokenValue::Real(-0.1),
            TokenValue::Integer(0),
        ],
        vec![
            TokenValue::Integer(406),
            TokenValue::Integer(8),
            TokenValue::Integer(0),
            TokenValue::Integer(2),
            TokenValue::Integer(2),
            TokenValue::Real(0.1),
            TokenValue::Real(-0.1),
            TokenValue::Integer(0),
            TokenValue::Integer(0),
        ],
    ] {
        let record = token_parameter_record(1, values);
        let generic_count = structural_pointer_group_candidates(&record).len();
        let analysis = analyze_trailing_pointer_groups(&record, &directory);
        assert_eq!(analysis.candidate_count, 0, "generic_count={generic_count}");
        assert_eq!(analysis.valid_candidate_count, 0);
        assert!(analysis.groups.is_none());
    }
}

#[test]
fn type406_form31_entity_table_boundary_follows_fixed_corners() {
    let mut source = directory_target(1, 406);
    source.form = 31;
    let association = directory_target(3, 212);
    let directory = BTreeMap::from([(1, &source), (3, &association)]);
    for values in [
        vec![
            TokenValue::Integer(406),
            TokenValue::Integer(8),
            TokenValue::Integer(0),
            TokenValue::Integer(0),
            TokenValue::Integer(2),
            TokenValue::Integer(0),
            TokenValue::Integer(2),
            TokenValue::Integer(1),
            TokenValue::Integer(0),
            TokenValue::Integer(1),
            TokenValue::Integer(1),
            TokenValue::Integer(3),
            TokenValue::Integer(0),
        ],
        vec![
            TokenValue::Integer(406),
            TokenValue::Integer(8),
            TokenValue::Integer(0),
            TokenValue::Integer(0),
            TokenValue::String(b"2".to_vec()),
            TokenValue::Integer(0),
            TokenValue::Integer(2),
            TokenValue::Integer(1),
            TokenValue::Integer(0),
            TokenValue::Integer(1),
            TokenValue::Integer(1),
            TokenValue::Integer(3),
            TokenValue::Integer(0),
        ],
    ] {
        let analysis =
            analyze_trailing_pointer_groups(&token_parameter_record(1, values), &directory);
        assert_eq!(analysis.candidate_count, 1);
        assert_eq!(analysis.valid_candidate_count, 1);
        let groups = analysis.groups.expect("Type 406 Form 31 table boundary");
        assert_eq!(groups.token_start, 10);
        assert_eq!(groups.associations, vec![3]);
        assert!(groups.properties.is_empty());
    }
}

#[test]
fn type406_form31_table_boundary_precedes_generic_candidate() {
    let mut source = directory_target(1, 406);
    source.form = 31;
    let association = directory_target(3, 212);
    let directory = BTreeMap::from([(1, &source), (3, &association)]);
    let values = vec![
        TokenValue::Integer(406),
        TokenValue::Integer(8),
        TokenValue::Integer(0),
        TokenValue::Integer(0),
        TokenValue::Integer(2),
        TokenValue::Integer(0),
        TokenValue::Integer(2),
        TokenValue::Integer(1),
        TokenValue::Integer(0),
        TokenValue::Integer(3),
        TokenValue::Integer(1),
        TokenValue::Integer(3),
        TokenValue::Integer(0),
    ];
    let record = token_parameter_record(1, values);
    let generic = structural_pointer_group_candidates(&record);
    assert!(generic.iter().any(|candidate| candidate.token_start == 8));

    let analysis = analyze_trailing_pointer_groups(&record, &directory);
    assert_eq!(analysis.candidate_count, 1);
    assert_eq!(analysis.valid_candidate_count, 1);
    let groups = analysis.groups.expect("Type 406 Form 31 table boundary");
    assert_eq!(groups.token_start, 10);
    assert_eq!(groups.associations, vec![3]);
    assert!(groups.properties.is_empty());
}

#[test]
fn type406_form31_malformed_np_or_span_does_not_enable_generic_recovery() {
    let mut source = directory_target(1, 406);
    source.form = 31;
    let association = directory_target(3, 212);
    let directory = BTreeMap::from([(1, &source), (3, &association)]);
    for values in [
        vec![
            TokenValue::Integer(406),
            TokenValue::Integer(7),
            TokenValue::Integer(0),
            TokenValue::Integer(0),
            TokenValue::Integer(2),
            TokenValue::Integer(0),
            TokenValue::Integer(2),
            TokenValue::Integer(1),
            TokenValue::Integer(0),
            TokenValue::Integer(1),
            TokenValue::Integer(1),
            TokenValue::Integer(3),
            TokenValue::Integer(0),
        ],
        vec![
            TokenValue::Integer(406),
            TokenValue::Omitted,
            TokenValue::Integer(0),
            TokenValue::Integer(0),
            TokenValue::Integer(2),
            TokenValue::Integer(0),
            TokenValue::Integer(2),
            TokenValue::Integer(1),
            TokenValue::Integer(0),
            TokenValue::Integer(1),
            TokenValue::Integer(1),
            TokenValue::Integer(3),
            TokenValue::Integer(0),
        ],
        vec![
            TokenValue::Integer(406),
            TokenValue::Integer(8),
            TokenValue::Integer(0),
            TokenValue::Integer(0),
            TokenValue::Integer(2),
            TokenValue::Integer(0),
            TokenValue::Integer(2),
            TokenValue::Integer(1),
        ],
        vec![
            TokenValue::Integer(406),
            TokenValue::Integer(8),
            TokenValue::Integer(0),
            TokenValue::Integer(0),
            TokenValue::Integer(2),
            TokenValue::Integer(0),
            TokenValue::Integer(2),
            TokenValue::Integer(1),
            TokenValue::Integer(0),
        ],
    ] {
        let record = token_parameter_record(1, values);
        let generic_count = structural_pointer_group_candidates(&record).len();
        let analysis = analyze_trailing_pointer_groups(&record, &directory);
        assert_eq!(analysis.candidate_count, 0, "generic_count={generic_count}");
        assert_eq!(analysis.valid_candidate_count, 0);
        assert!(analysis.groups.is_none());
    }
}

#[test]
fn type406_form36_entity_table_boundary_follows_np_arity() {
    let association = directory_target(3, 212);
    let property = directory_target(5, 316);
    let mut source = directory_target(1, 406);
    source.form = 36;
    let directory = BTreeMap::from([(1, &source), (3, &association), (5, &property)]);
    for (values, expected_start) in [
        (
            vec![
                TokenValue::Integer(406),
                TokenValue::Integer(1),
                TokenValue::Integer(1),
                TokenValue::Integer(1),
                TokenValue::Integer(3),
                TokenValue::Integer(1),
                TokenValue::Integer(5),
            ],
            3,
        ),
        (
            vec![
                TokenValue::Integer(406),
                TokenValue::Integer(2),
                TokenValue::Integer(2),
                TokenValue::Integer(1),
                TokenValue::Integer(1),
                TokenValue::Integer(3),
                TokenValue::Integer(1),
                TokenValue::Integer(5),
            ],
            4,
        ),
        (
            vec![
                TokenValue::Integer(406),
                TokenValue::Integer(2),
                TokenValue::Integer(2),
                TokenValue::String(b"1".to_vec()),
                TokenValue::Integer(1),
                TokenValue::Integer(3),
                TokenValue::Integer(1),
                TokenValue::Integer(5),
            ],
            4,
        ),
    ] {
        let analysis =
            analyze_trailing_pointer_groups(&token_parameter_record(1, values), &directory);
        assert_eq!(analysis.candidate_count, 1);
        assert_eq!(analysis.valid_candidate_count, 1);
        let groups = analysis.groups.expect("Type 406 Form 36 table boundary");
        assert_eq!(groups.token_start, expected_start);
        assert_eq!(groups.associations, vec![3]);
        assert_eq!(groups.properties, vec![5]);
    }
}

#[test]
fn type406_form36_table_boundary_precedes_generic_candidate() {
    let association = directory_target(3, 212);
    let property = directory_target(5, 316);
    let mut source = directory_target(1, 406);
    source.form = 36;
    let directory = BTreeMap::from([(1, &source), (3, &association), (5, &property)]);
    let values = vec![
        TokenValue::Integer(406),
        TokenValue::Integer(1),
        TokenValue::Integer(1),
        TokenValue::Integer(2),
        TokenValue::Integer(3),
        TokenValue::Integer(3),
        TokenValue::Integer(1),
        TokenValue::Integer(5),
    ];
    let record = token_parameter_record(1, values);
    let generic = structural_pointer_group_candidates(&record);
    assert!(generic.iter().any(|candidate| candidate.token_start == 2));

    let analysis = analyze_trailing_pointer_groups(&record, &directory);
    assert_eq!(analysis.candidate_count, 1);
    assert_eq!(analysis.valid_candidate_count, 1);
    let groups = analysis.groups.expect("Type 406 Form 36 table boundary");
    assert_eq!(groups.token_start, 3);
    assert_eq!(groups.associations, vec![3, 3]);
    assert_eq!(groups.properties, vec![5]);
}

#[test]
fn type406_form36_malformed_np_or_span_does_not_enable_generic_recovery() {
    let association = directory_target(3, 212);
    let property = directory_target(5, 316);
    let mut source = directory_target(1, 406);
    source.form = 36;
    let directory = BTreeMap::from([(1, &source), (3, &association), (5, &property)]);
    let cases = [
        vec![
            TokenValue::Integer(406),
            TokenValue::Integer(0),
            TokenValue::Integer(1),
            TokenValue::Integer(1),
            TokenValue::Integer(3),
            TokenValue::Integer(1),
            TokenValue::Integer(5),
        ],
        vec![
            TokenValue::Integer(406),
            TokenValue::Integer(3),
            TokenValue::Integer(1),
            TokenValue::Integer(1),
            TokenValue::Integer(3),
            TokenValue::Integer(1),
            TokenValue::Integer(5),
        ],
        vec![
            TokenValue::Integer(406),
            TokenValue::Omitted,
            TokenValue::Integer(1),
            TokenValue::Integer(1),
            TokenValue::Integer(3),
            TokenValue::Integer(1),
            TokenValue::Integer(5),
        ],
        vec![
            TokenValue::Integer(406),
            TokenValue::Integer(1),
            TokenValue::Integer(1),
        ],
        vec![
            TokenValue::Integer(406),
            TokenValue::Integer(2),
            TokenValue::Integer(1),
        ],
    ];
    for values in cases {
        let record = token_parameter_record(1, values);
        let generic_count = structural_pointer_group_candidates(&record).len();
        let analysis = analyze_trailing_pointer_groups(&record, &directory);
        assert_eq!(analysis.candidate_count, 0, "generic_count={generic_count}");
        assert_eq!(analysis.valid_candidate_count, 0);
        assert!(analysis.groups.is_none());
    }
}

#[test]
fn type184_entity_table_boundary_follows_item_and_transform_lists() {
    for (form, item_count, expected_start) in [(0_i64, 1_i64, 4), (0, 2, 6), (1, 3, 8)] {
        let association = directory_target(1, 212);
        let mut source = directory_target(3, 184);
        source.form = form;
        let directory = BTreeMap::from([(1, &association), (3, &source)]);
        let item_count = usize::try_from(item_count).unwrap();
        let mut values = vec![0_i64; expected_start + 3];
        values[0] = 184;
        values[1] = i64::try_from(item_count).unwrap();
        for index in 0..item_count {
            values[2 + index] = 1;
            values[2 + item_count + index] = 0;
        }
        values[expected_start] = 1;
        values[expected_start + 1] = 1;
        values[expected_start + 2] = 0;
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
        assert_eq!(analysis.candidate_count, 1, "form={form}, N={item_count}");
        assert_eq!(
            analysis.valid_candidate_count, 1,
            "form={form}, N={item_count}"
        );
        let groups = analysis.groups.expect("Type 184 table boundary");
        assert_eq!(groups.token_start, expected_start);
        assert_eq!(groups.associations, vec![1]);
        assert!(groups.properties.is_empty());
    }
}

#[test]
fn type184_entity_table_boundary_precedes_valid_generic_alternative() {
    let target_1 = directory_target(1, 212);
    let target_3 = directory_target(3, 212);
    let target_7 = directory_target(7, 212);
    let source = directory_target(5, 184);
    let directory = BTreeMap::from([(1, &target_1), (3, &target_3), (5, &source), (7, &target_7)]);
    let values = [184, 2, 1, 3, 0, 2, 1, 7, 0];
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
    let groups = analysis.groups.expect("Type 184 table boundary");
    assert_eq!(groups.token_start, 6);
    assert_eq!(groups.associations, vec![7]);
    assert!(groups.properties.is_empty());
}

#[test]
fn type184_malformed_counts_do_not_enable_generic_recovery() {
    let target_1 = directory_target(1, 212);
    let target_5 = directory_target(5, 212);
    let source = directory_target(3, 184);
    let directory = BTreeMap::from([(1, &target_1), (3, &source), (5, &target_5)]);
    let cases = [
        vec![184, 0, 1, 5, 1, 5, 0],
        vec![184, -1, 1, 5, 1, 5, 0],
        vec![184, 100, 1, 5, 1, 5, 0],
        vec![184],
        vec![184, 2, 1, 5, 0],
    ];

    for values in cases {
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
fn type412_entity_table_boundary_follows_do_dont_list() {
    for (list_count, expected_start) in [(0_i64, 13_usize), (1, 14), (2, 15)] {
        let association = directory_target(1, 212);
        let source = directory_target(3, 412);
        let directory = BTreeMap::from([(1, &association), (3, &source)]);
        let list_count = usize::try_from(list_count).unwrap();
        let mut values = vec![0_i64; expected_start + 3];
        values[0] = 412;
        values[1] = 1;
        values[2] = 1;
        values[6] = 2;
        values[7] = 2;
        values[8] = 1;
        values[9] = 1;
        values[11] = i64::try_from(list_count).unwrap();
        values[12] = 0;
        for index in 0..list_count {
            values[13 + index] = i64::try_from(index + 1).unwrap();
        }
        values[expected_start] = 1;
        values[expected_start + 1] = 1;
        values[expected_start + 2] = 0;
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
        assert_eq!(analysis.candidate_count, 1, "LC={list_count}");
        assert_eq!(analysis.valid_candidate_count, 1, "LC={list_count}");
        let groups = analysis.groups.expect("Type 412 table boundary");
        assert_eq!(groups.token_start, expected_start);
        assert_eq!(groups.associations, vec![1]);
        assert!(groups.properties.is_empty());
    }
}

#[test]
fn type412_entity_table_boundary_precedes_valid_generic_alternative() {
    let association = directory_target(1, 212);
    let source = directory_target(3, 412);
    let directory = BTreeMap::from([(1, &association), (3, &source)]);
    let values = [412, 1, 1, 0, 0, 0, 2, 2, 1, 1, 0, 1, 0, 2, 1, 1, 0];
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
        parameter_end: values.len(),
        comment: Vec::new(),
    };

    let analysis = analyze_trailing_pointer_groups(&record, &directory);
    assert_eq!(analysis.candidate_count, 1);
    assert_eq!(analysis.valid_candidate_count, 1);
    let groups = analysis.groups.expect("Type 412 table boundary");
    assert_eq!(groups.token_start, 14);
    assert_eq!(groups.associations, vec![1]);
    assert!(groups.properties.is_empty());
}

#[test]
fn type412_malformed_counts_do_not_enable_generic_recovery() {
    let target_1 = directory_target(1, 212);
    let target_5 = directory_target(5, 212);
    let source = directory_target(3, 412);
    let directory = BTreeMap::from([(1, &target_1), (3, &source), (5, &target_5)]);
    let cases = [
        vec![412, 1, 1, 0, 0, 0, 2, 2, 1, 1, 0, -1, 0, 1, 5, 0],
        vec![412, 1, 1, 0, 0, 0, 2, 2, 1, 1, 0, 100, 0, 1, 5, 0],
        vec![412, 1, 1, 0, 0, 0, 2, 2, 1, 1, 0],
        vec![412, 1, 1, 0, 0, 0, 2, 2, 1, 1, 0, 2, 0, 1],
    ];

    for values in cases {
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

    let mut values = (0..16)
        .map(|_| Token {
            value: TokenValue::Integer(0),
            span: 0..0,
        })
        .collect::<Vec<_>>();
    values[0].value = TokenValue::Integer(412);
    values[1].value = TokenValue::Integer(1);
    values[2].value = TokenValue::Integer(1);
    values[6].value = TokenValue::Integer(2);
    values[7].value = TokenValue::Integer(2);
    values[8].value = TokenValue::Integer(1);
    values[9].value = TokenValue::Integer(1);
    values[11].value = TokenValue::String(b"1".to_vec());
    values[13].value = TokenValue::Integer(1);
    values[14].value = TokenValue::Integer(1);
    values[15].value = TokenValue::Integer(5);
    let record = ParameterRecord {
        directory_sequence: 3,
        line_range: 1..2,
        bytes: Vec::new(),
        parameter_end: values.len(),
        tokens: values,
        comment: Vec::new(),
    };
    let analysis = analyze_trailing_pointer_groups(&record, &directory);
    assert_eq!(analysis.candidate_count, 0);
    assert_eq!(analysis.valid_candidate_count, 0);
    assert!(analysis.groups.is_none());
}

#[test]
fn type414_entity_table_boundary_follows_do_dont_list() {
    for (list_count, expected_start) in [(0_i64, 11_usize), (1, 12), (2, 13)] {
        let association = directory_target(1, 212);
        let source = directory_target(3, 414);
        let directory = BTreeMap::from([(1, &association), (3, &source)]);
        let list_count = usize::try_from(list_count).unwrap();
        let mut values = vec![0_i64; expected_start + 3];
        values[0] = 414;
        values[1] = 1;
        values[2] = 4;
        values[6] = 8;
        values[7] = 1;
        values[8] = 1;
        values[9] = i64::try_from(list_count).unwrap();
        values[10] = 0;
        for index in 0..list_count {
            values[11 + index] = i64::try_from(index + 1).unwrap();
        }
        values[expected_start] = 1;
        values[expected_start + 1] = 1;
        values[expected_start + 2] = 0;
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
        assert_eq!(analysis.candidate_count, 1, "LC={list_count}");
        assert_eq!(analysis.valid_candidate_count, 1, "LC={list_count}");
        let groups = analysis.groups.expect("Type 414 table boundary");
        assert_eq!(groups.token_start, expected_start);
        assert_eq!(groups.associations, vec![1]);
        assert!(groups.properties.is_empty());
    }
}

#[test]
fn type414_entity_table_boundary_precedes_valid_generic_alternative() {
    let association = directory_target(1, 212);
    let source = directory_target(3, 414);
    let directory = BTreeMap::from([(1, &association), (3, &source)]);
    let values = [414, 1, 4, 0, 0, 0, 8, 1, 1, 1, 0, 2, 1, 1, 0];
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
        parameter_end: values.len(),
        comment: Vec::new(),
    };

    let analysis = analyze_trailing_pointer_groups(&record, &directory);
    assert_eq!(analysis.candidate_count, 1);
    assert_eq!(analysis.valid_candidate_count, 1);
    let groups = analysis.groups.expect("Type 414 table boundary");
    assert_eq!(groups.token_start, 12);
    assert_eq!(groups.associations, vec![1]);
    assert!(groups.properties.is_empty());
}

#[test]
fn type414_malformed_counts_do_not_enable_generic_recovery() {
    let target_1 = directory_target(1, 212);
    let target_5 = directory_target(5, 212);
    let source = directory_target(3, 414);
    let directory = BTreeMap::from([(1, &target_1), (3, &source), (5, &target_5)]);
    let cases = [
        vec![414, 1, 4, 0, 0, 0, 8, 1, 1, -1, 0, 1, 5, 0],
        vec![414, 1, 4, 0, 0, 0, 8, 1, 1, 100, 0, 1, 5, 0],
        vec![414, 1, 4, 0, 0, 0, 8, 1, 1],
        vec![414, 1, 4, 0, 0, 0, 8, 1, 1, 2, 0, 1],
    ];

    for values in cases {
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

    let mut values = (0..14)
        .map(|_| Token {
            value: TokenValue::Integer(0),
            span: 0..0,
        })
        .collect::<Vec<_>>();
    values[0].value = TokenValue::Integer(414);
    values[1].value = TokenValue::Integer(1);
    values[2].value = TokenValue::Integer(4);
    values[6].value = TokenValue::Integer(8);
    values[7].value = TokenValue::Integer(1);
    values[8].value = TokenValue::Integer(1);
    values[9].value = TokenValue::String(b"1".to_vec());
    values[11].value = TokenValue::Integer(1);
    values[12].value = TokenValue::Integer(1);
    values[13].value = TokenValue::Integer(5);
    let record = ParameterRecord {
        directory_sequence: 3,
        line_range: 1..2,
        bytes: Vec::new(),
        parameter_end: values.len(),
        tokens: values,
        comment: Vec::new(),
    };
    let analysis = analyze_trailing_pointer_groups(&record, &directory);
    assert_eq!(analysis.candidate_count, 0);
    assert_eq!(analysis.valid_candidate_count, 0);
    assert!(analysis.groups.is_none());
}

#[test]
fn type402_form5_entity_table_boundary_follows_label_placements() {
    for (placement_count, expected_start) in [(1_i64, 9_usize), (2, 16)] {
        let association = directory_target(1, 212);
        let mut source = directory_target(3, 402);
        source.form = 5;
        let directory = BTreeMap::from([(1, &association), (3, &source)]);
        let placement_count = usize::try_from(placement_count).unwrap();
        let mut values = vec![0_i64; expected_start + 3];
        values[0] = 402;
        values[1] = i64::try_from(placement_count).unwrap();
        for index in 0..placement_count {
            let start = 2 + index * 7;
            values[start] = 1;
            values[start + 4] = 1;
            values[start + 6] = 1;
        }
        values[expected_start] = 1;
        values[expected_start + 1] = 1;
        values[expected_start + 2] = 0;
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
        assert_eq!(analysis.candidate_count, 1, "N={placement_count}");
        assert_eq!(analysis.valid_candidate_count, 1, "N={placement_count}");
        let groups = analysis.groups.expect("Type 402 Form 5 table boundary");
        assert_eq!(groups.token_start, expected_start);
        assert_eq!(groups.associations, vec![1]);
        assert!(groups.properties.is_empty());
    }
}

#[test]
fn type402_form5_entity_table_boundary_precedes_valid_generic_alternative() {
    let mut generic_target = directory_target(1, 402);
    generic_target.form = 7;
    let association = directory_target(9, 212);
    let mut source = directory_target(3, 402);
    source.form = 5;
    let directory = BTreeMap::from([(1, &generic_target), (3, &source), (9, &association)]);
    let values = [402, 1, 5, 0, 0, 0, 7, 0, 2, 1, 9, 0];
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
        parameter_end: values.len(),
        comment: Vec::new(),
    };

    let analysis = analyze_trailing_pointer_groups(&record, &directory);
    assert_eq!(analysis.candidate_count, 1);
    assert_eq!(analysis.valid_candidate_count, 1);
    let groups = analysis.groups.expect("Type 402 Form 5 table boundary");
    assert_eq!(groups.token_start, 9);
    assert_eq!(groups.associations, vec![9]);
    assert!(groups.properties.is_empty());
}

#[test]
fn type402_form5_malformed_counts_do_not_enable_generic_recovery() {
    let target = directory_target(1, 212);
    let mut source = directory_target(3, 402);
    source.form = 5;
    let directory = BTreeMap::from([(1, &target), (3, &source)]);
    let cases = [
        vec![402, 0, 1, 0, 0, 0, 5, 0, 7, 0, 1, 1, 0],
        vec![402, -1, 1, 0, 0, 0, 5, 0, 7, 0, 1, 1, 0],
        vec![402, 1000, 1, 0, 0, 0, 5, 0, 7, 0, 1, 1, 0],
        vec![402],
        vec![402, 2, 1, 0, 0, 0, 5, 0, 7, 1, 1, 0],
    ];

    for values in cases {
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

    let mut values = (0..13)
        .map(|_| Token {
            value: TokenValue::Integer(0),
            span: 0..0,
        })
        .collect::<Vec<_>>();
    values[0].value = TokenValue::Integer(402);
    values[1].value = TokenValue::String(b"1".to_vec());
    values[2].value = TokenValue::Integer(1);
    values[6].value = TokenValue::Integer(5);
    values[8].value = TokenValue::Integer(7);
    values[10].value = TokenValue::Integer(1);
    values[11].value = TokenValue::Integer(1);
    let record = ParameterRecord {
        directory_sequence: 3,
        line_range: 1..2,
        bytes: Vec::new(),
        parameter_end: values.len(),
        tokens: values,
        comment: Vec::new(),
    };
    let analysis = analyze_trailing_pointer_groups(&record, &directory);
    assert_eq!(analysis.candidate_count, 0);
    assert_eq!(analysis.valid_candidate_count, 0);
    assert!(analysis.groups.is_none());
}

#[test]
fn type406_form34_and_form35_entity_table_boundary_follows_text_score_ranges() {
    let association = directory_target(1, 212);
    let cases = [
        (34, vec![406, 4, 1, 1, 2, 4, 1, 1, 0], 6),
        (34, vec![406, 7, 2, 1, 2, 4, 2, 1, 3, 1, 1, 0], 9),
        (35, vec![406, 4, 1, 1, 2, 4, 1, 1, 0], 6),
        (35, vec![406, 7, 2, 1, 2, 4, 2, 1, 3, 1, 1, 0], 9),
    ];
    for (form, values, expected_start) in cases {
        let mut source = directory_target(3, 406);
        source.form = form;
        let directory = BTreeMap::from([(1, &association), (3, &source)]);
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
        assert_eq!(analysis.candidate_count, 1, "Form {form}");
        assert_eq!(analysis.valid_candidate_count, 1, "Form {form}");
        let groups = analysis.groups.expect("text-score table boundary");
        assert_eq!(groups.token_start, expected_start, "Form {form}");
        assert_eq!(groups.associations, vec![1], "Form {form}");
        assert!(groups.properties.is_empty(), "Form {form}");
    }

    let mut source = directory_target(3, 406);
    source.form = 34;
    let directory = BTreeMap::from([(1, &association), (3, &source)]);
    let values = [406, 4, 1, 1, 1, 2, 1, 1, 0];
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
        parameter_end: values.len(),
        comment: Vec::new(),
    };
    let analysis = analyze_trailing_pointer_groups(&record, &directory);
    assert_eq!(analysis.candidate_count, 1);
    assert_eq!(analysis.valid_candidate_count, 1);
    let groups = analysis.groups.expect("Form 34 table boundary");
    assert_eq!(groups.token_start, 6);
    assert_eq!(groups.associations, vec![1]);
}

#[test]
fn type406_form34_and_form35_malformed_counts_do_not_enable_generic_recovery() {
    let association = directory_target(1, 212);
    let mut source = directory_target(3, 406);
    source.form = 34;
    let directory = BTreeMap::from([(1, &association), (3, &source)]);
    let cases = [
        vec![406, 1, 0, 1, 1, 0],
        vec![406, 1, -1, 1, 1, 0],
        vec![406, 5, 1, 1, 1, 1, 1, 0],
        vec![406, 4, 1, 1, 1],
        vec![406],
    ];
    for values in cases {
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

    let record = ParameterRecord {
        directory_sequence: 3,
        line_range: 1..2,
        bytes: Vec::new(),
        tokens: vec![
            Token {
                value: TokenValue::Integer(406),
                span: 0..0,
            },
            Token {
                value: TokenValue::Integer(4),
                span: 0..0,
            },
            Token {
                value: TokenValue::String(b"1".to_vec()),
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
        parameter_end: 6,
        comment: Vec::new(),
    };
    let analysis = analyze_trailing_pointer_groups(&record, &directory);
    assert_eq!(analysis.candidate_count, 0);
    assert_eq!(analysis.valid_candidate_count, 0);
    assert!(analysis.groups.is_none());

    let mut source = directory_target(3, 406);
    source.form = 35;
    let directory = BTreeMap::from([(1, &association), (3, &source)]);
    let values = [
        Token {
            value: TokenValue::Integer(406),
            span: 0..0,
        },
        Token {
            value: TokenValue::Integer(4),
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
            value: TokenValue::String(b"1".to_vec()),
            span: 0..0,
        },
        Token {
            value: TokenValue::Integer(2),
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
    ];
    let record = ParameterRecord {
        directory_sequence: 3,
        line_range: 1..2,
        bytes: Vec::new(),
        parameter_end: values.len(),
        tokens: values.into_iter().collect(),
        comment: Vec::new(),
    };
    let analysis = analyze_trailing_pointer_groups(&record, &directory);
    assert_eq!(analysis.candidate_count, 1);
    assert_eq!(analysis.valid_candidate_count, 1);
    assert_eq!(
        analysis.groups.expect("Form 35 table boundary").token_start,
        6
    );
}

#[test]
fn type406_form30_entity_table_boundary_follows_fixed_np_and_note_count() {
    let association = directory_target(1, 212);
    let units = directory_target(5, 316);
    let mut source = directory_target(3, 406);
    source.form = 30;
    let directory = BTreeMap::from([(1, &association), (3, &source), (5, &units)]);
    let make_record = |values: Vec<TokenValue>| {
        let tokens = values
            .into_iter()
            .map(|value| Token { value, span: 0..0 })
            .collect::<Vec<_>>();
        let parameter_end = tokens.len();
        ParameterRecord {
            directory_sequence: 3,
            line_range: 1..2,
            bytes: Vec::new(),
            tokens,
            parameter_end,
            comment: Vec::new(),
        }
    };
    let cases = [
        (
            vec![406, 14, 0, 1, 1, 3, 0, 0, 1, 0, 0, 0, 12, 0, 0, 1, 5],
            14,
            Vec::new(),
            vec![5],
        ),
        (
            vec![
                406, 14, 0, 1, 1, 3, 0, 0, 1, 0, 0, 0, 12, 1, 1, 1, 1, 1, 1, 1, 5,
            ],
            17,
            vec![1],
            vec![5],
        ),
    ];
    for (values, expected_start, associations, properties) in cases {
        let record = make_record(
            values
                .into_iter()
                .map(TokenValue::Integer)
                .collect::<Vec<_>>(),
        );
        let analysis = analyze_trailing_pointer_groups(&record, &directory);
        assert_eq!(analysis.candidate_count, 1);
        assert_eq!(analysis.valid_candidate_count, 1);
        let groups = analysis.groups.expect("Form 30 table boundary");
        assert_eq!(groups.token_start, expected_start);
        assert_eq!(groups.associations, associations);
        assert_eq!(groups.properties, properties);
    }
}

#[test]
fn type406_form30_complete_counted_span_keeps_boundary_with_wrong_note_type() {
    let association = directory_target(1, 212);
    let units = directory_target(5, 316);
    let mut source = directory_target(3, 406);
    source.form = 30;
    let directory = BTreeMap::from([(1, &association), (3, &source), (5, &units)]);
    let values = vec![
        TokenValue::Integer(406),
        TokenValue::Integer(14),
        TokenValue::Integer(0),
        TokenValue::Integer(1),
        TokenValue::Integer(1),
        TokenValue::Integer(3),
        TokenValue::Integer(0),
        TokenValue::Integer(0),
        TokenValue::Integer(1),
        TokenValue::Integer(0),
        TokenValue::Integer(0),
        TokenValue::Integer(0),
        TokenValue::Integer(12),
        TokenValue::Integer(1),
        TokenValue::String(b"bad".to_vec()),
        TokenValue::Integer(1),
        TokenValue::Integer(1),
        TokenValue::Integer(1),
        TokenValue::Integer(1),
        TokenValue::Integer(1),
        TokenValue::Integer(5),
    ];
    let tokens = values
        .into_iter()
        .map(|value| Token { value, span: 0..0 })
        .collect::<Vec<_>>();
    let record = ParameterRecord {
        directory_sequence: 3,
        line_range: 1..2,
        bytes: Vec::new(),
        parameter_end: tokens.len(),
        tokens,
        comment: Vec::new(),
    };

    let analysis = analyze_trailing_pointer_groups(&record, &directory);
    assert_eq!(analysis.candidate_count, 1);
    assert_eq!(analysis.valid_candidate_count, 1);
    assert_eq!(
        analysis.groups.expect("Form 30 table boundary").token_start,
        17
    );
}

#[test]
fn type406_form30_malformed_np_or_note_count_does_not_enable_generic_recovery() {
    let association = directory_target(1, 212);
    let units = directory_target(5, 316);
    let mut source = directory_target(3, 406);
    source.form = 30;
    let directory = BTreeMap::from([(1, &association), (3, &source), (5, &units)]);
    let integers = |values: &[i64]| {
        values
            .iter()
            .copied()
            .map(TokenValue::Integer)
            .collect::<Vec<_>>()
    };
    let cases = vec![
        integers(&[406, 15, 0, 1, 1, 3, 0, 0, 1, 0, 0, 0, 12, 0, 0, 1, 5]),
        integers(&[406, 14, 0, 1, 1, 3, 0, 0, 1, 0, 0, 0, 12, -1, 0, 1, 5]),
        integers(&[
            406, 15, 0, 1, 1, 3, 0, 0, 1, 0, 0, 0, 12, 1, 1, 1, 1, 1, 3, 1, 5,
        ]),
        vec![
            integers(&[406, 14, 0, 1, 1, 3, 0, 0, 1, 0, 0, 0, 12]),
            vec![TokenValue::String(b"1".to_vec())],
            integers(&[0, 1, 5]),
        ]
        .into_iter()
        .flatten()
        .collect(),
        integers(&[406, 14, 0, 1, 1, 3, 0, 0, 1, 0, 0, 0, 12, 1, 1, 1]),
        integers(&[406, 14, 0, 1, 1, 3, 0, 0, 1, 0, 0, 0, 12]),
    ];
    for values in cases {
        let tokens = values
            .into_iter()
            .map(|value| Token { value, span: 0..0 })
            .collect::<Vec<_>>();
        let record = ParameterRecord {
            directory_sequence: 3,
            line_range: 1..2,
            bytes: Vec::new(),
            parameter_end: tokens.len(),
            tokens,
            comment: Vec::new(),
        };
        let analysis = analyze_trailing_pointer_groups(&record, &directory);
        assert_eq!(analysis.candidate_count, 0);
        assert_eq!(analysis.valid_candidate_count, 0);
        assert!(analysis.groups.is_none());
    }
}

#[test]
fn type406_form11_entity_table_boundary_follows_nested_value_counts() {
    let association = directory_target(1, 212);
    let units = directory_target(3, 316);
    let mut source = directory_target(5, 406);
    source.form = 11;
    let directory = BTreeMap::from([(1, &association), (3, &units), (5, &source)]);
    let cases = [
        (
            vec![
                TokenValue::Integer(406),
                TokenValue::Integer(5),
                TokenValue::Integer(5),
                TokenValue::Integer(2),
                TokenValue::Integer(0),
                TokenValue::Integer(33),
                TokenValue::Integer(46),
                TokenValue::Integer(1),
                TokenValue::Integer(1),
                TokenValue::Integer(1),
                TokenValue::Integer(3),
            ],
            7,
        ),
        (
            vec![
                TokenValue::Integer(406),
                TokenValue::Integer(18),
                TokenValue::Integer(5),
                TokenValue::Integer(1),
                TokenValue::Integer(2),
                TokenValue::Integer(1),
                TokenValue::Integer(2),
                TokenValue::Integer(2),
                TokenValue::Integer(3),
                TokenValue::Integer(10),
                TokenValue::Integer(20),
                TokenValue::Integer(100),
                TokenValue::Integer(200),
                TokenValue::Integer(300),
                TokenValue::Integer(2),
                TokenValue::Integer(1),
                TokenValue::Integer(2),
                TokenValue::Integer(3),
                TokenValue::Integer(4),
                TokenValue::Integer(2),
                TokenValue::Integer(1),
                TokenValue::Integer(1),
                TokenValue::Integer(1),
                TokenValue::Integer(3),
            ],
            20,
        ),
    ];
    for (values, expected_start) in cases {
        let tokens = values
            .into_iter()
            .map(|value| Token { value, span: 0..0 })
            .collect::<Vec<_>>();
        let record = ParameterRecord {
            directory_sequence: 5,
            line_range: 1..2,
            bytes: Vec::new(),
            parameter_end: tokens.len(),
            tokens,
            comment: Vec::new(),
        };
        if expected_start == 20 {
            let generic_valid_candidate_count = structural_pointer_group_candidates(&record)
                .iter()
                .filter_map(|candidate| groups_for_candidate(&record, &directory, *candidate))
                .filter(|groups| groups.fully_valid)
                .count();
            assert_eq!(generic_valid_candidate_count, 2);
        }
        let analysis = analyze_trailing_pointer_groups(&record, &directory);
        assert_eq!(analysis.candidate_count, 1);
        assert_eq!(analysis.valid_candidate_count, 1);
        let groups = analysis.groups.expect("Form 11 table boundary");
        assert_eq!(groups.token_start, expected_start);
        assert_eq!(groups.associations, vec![1]);
        assert_eq!(groups.properties, vec![3]);
    }
}

#[test]
fn type406_form11_complete_nested_span_keeps_boundary_with_invalid_value() {
    let association = directory_target(1, 212);
    let units = directory_target(3, 316);
    let mut source = directory_target(5, 406);
    source.form = 11;
    let directory = BTreeMap::from([(1, &association), (3, &units), (5, &source)]);
    let values = vec![
        TokenValue::Integer(406),
        TokenValue::Integer(4),
        TokenValue::Integer(5),
        TokenValue::Integer(1),
        TokenValue::Integer(0),
        TokenValue::String(b"bad".to_vec()),
        TokenValue::Integer(1),
        TokenValue::Integer(1),
        TokenValue::Integer(1),
        TokenValue::Integer(3),
    ];
    let tokens = values
        .into_iter()
        .map(|value| Token { value, span: 0..0 })
        .collect::<Vec<_>>();
    let record = ParameterRecord {
        directory_sequence: 5,
        line_range: 1..2,
        bytes: Vec::new(),
        parameter_end: tokens.len(),
        tokens,
        comment: Vec::new(),
    };

    let analysis = analyze_trailing_pointer_groups(&record, &directory);
    assert_eq!(analysis.candidate_count, 1);
    assert_eq!(analysis.valid_candidate_count, 1);
    assert_eq!(
        analysis.groups.expect("Form 11 table boundary").token_start,
        6
    );
}

#[test]
fn type406_form11_malformed_nested_counts_do_not_enable_generic_recovery() {
    let association = directory_target(1, 212);
    let units = directory_target(3, 316);
    let mut source = directory_target(5, 406);
    source.form = 11;
    let directory = BTreeMap::from([(1, &association), (3, &units), (5, &source)]);
    let cases = vec![
        vec![
            TokenValue::Integer(406),
            TokenValue::Integer(6),
            TokenValue::Integer(5),
            TokenValue::Integer(2),
            TokenValue::Integer(0),
            TokenValue::Integer(33),
            TokenValue::Integer(46),
            TokenValue::Integer(1),
            TokenValue::Integer(1),
            TokenValue::Integer(1),
            TokenValue::Integer(3),
        ],
        vec![
            TokenValue::Integer(406),
            TokenValue::Integer(3),
            TokenValue::Integer(5),
            TokenValue::Integer(0),
            TokenValue::Integer(0),
            TokenValue::Integer(1),
            TokenValue::Integer(1),
            TokenValue::Integer(1),
            TokenValue::Integer(3),
        ],
        vec![
            TokenValue::Integer(406),
            TokenValue::Integer(3),
            TokenValue::Integer(5),
            TokenValue::Integer(-1),
            TokenValue::Integer(0),
            TokenValue::Integer(1),
            TokenValue::Integer(1),
            TokenValue::Integer(1),
            TokenValue::Integer(3),
        ],
        vec![
            TokenValue::Integer(406),
            TokenValue::Integer(7),
            TokenValue::Integer(5),
            TokenValue::Integer(1),
            TokenValue::String(b"1".to_vec()),
            TokenValue::Integer(1),
            TokenValue::Integer(33),
            TokenValue::Integer(46),
            TokenValue::Integer(1),
            TokenValue::Integer(1),
            TokenValue::Integer(1),
            TokenValue::Integer(3),
        ],
        vec![
            TokenValue::Integer(406),
            TokenValue::Integer(5),
            TokenValue::Integer(5),
            TokenValue::Integer(1),
            TokenValue::Integer(1),
            TokenValue::Integer(1),
            TokenValue::Integer(0),
            TokenValue::Integer(1),
            TokenValue::Integer(1),
            TokenValue::Integer(1),
            TokenValue::Integer(3),
        ],
        vec![
            TokenValue::Integer(406),
            TokenValue::Integer(5),
            TokenValue::Integer(5),
            TokenValue::Integer(1),
            TokenValue::Integer(1),
            TokenValue::Integer(1),
            TokenValue::Integer(-1),
            TokenValue::Integer(1),
            TokenValue::Integer(1),
            TokenValue::Integer(1),
            TokenValue::Integer(3),
        ],
        vec![
            TokenValue::Integer(406),
            TokenValue::Integer(5),
            TokenValue::Integer(5),
            TokenValue::Integer(1),
            TokenValue::Integer(1),
            TokenValue::Integer(1),
            TokenValue::String(b"2".to_vec()),
            TokenValue::Integer(1),
            TokenValue::Integer(1),
            TokenValue::Integer(1),
            TokenValue::Integer(3),
        ],
        vec![
            TokenValue::Integer(406),
            TokenValue::Integer(9),
            TokenValue::Integer(5),
            TokenValue::Integer(1),
            TokenValue::Integer(1),
            TokenValue::Integer(1),
            TokenValue::Integer(2),
            TokenValue::Integer(10),
            TokenValue::Integer(1),
        ],
        vec![
            TokenValue::Integer(406),
            TokenValue::Integer(5),
            TokenValue::Integer(5),
            TokenValue::Integer(2),
            TokenValue::Integer(0),
            TokenValue::Integer(33),
        ],
        vec![
            TokenValue::Integer(406),
            TokenValue::Integer(5),
            TokenValue::Integer(5),
            TokenValue::Integer(1),
            TokenValue::Integer(1),
            TokenValue::Integer(1),
            TokenValue::Integer(i64::MAX),
        ],
    ];
    for values in cases {
        let tokens = values
            .into_iter()
            .map(|value| Token { value, span: 0..0 })
            .collect::<Vec<_>>();
        let record = ParameterRecord {
            directory_sequence: 5,
            line_range: 1..2,
            bytes: Vec::new(),
            parameter_end: tokens.len(),
            tokens,
            comment: Vec::new(),
        };
        let analysis = analyze_trailing_pointer_groups(&record, &directory);
        assert_eq!(analysis.candidate_count, 0);
        assert_eq!(analysis.valid_candidate_count, 0);
        assert!(analysis.groups.is_none());
    }
}

#[test]
fn type406_form12_entity_table_boundary_follows_name_count() {
    let association = directory_target(1, 212);
    let units = directory_target(3, 316);
    let mut source = directory_target(5, 406);
    source.form = 12;
    let directory = BTreeMap::from([(1, &association), (3, &units), (5, &source)]);
    let cases = [
        (
            vec![
                TokenValue::Integer(406),
                TokenValue::Integer(1),
                TokenValue::String(b"BASE.IGS".to_vec()),
                TokenValue::Integer(1),
                TokenValue::Integer(1),
                TokenValue::Integer(1),
                TokenValue::Integer(3),
            ],
            3,
        ),
        (
            vec![
                TokenValue::Integer(406),
                TokenValue::Integer(2),
                TokenValue::String(b"BASE.IGS".to_vec()),
                TokenValue::String(b"DETAIL.IGS".to_vec()),
                TokenValue::Integer(1),
                TokenValue::Integer(1),
                TokenValue::Integer(1),
                TokenValue::Integer(3),
            ],
            4,
        ),
    ];
    for (values, expected_start) in cases {
        let tokens = values
            .into_iter()
            .map(|value| Token { value, span: 0..0 })
            .collect::<Vec<_>>();
        let record = ParameterRecord {
            directory_sequence: 5,
            line_range: 1..2,
            bytes: Vec::new(),
            parameter_end: tokens.len(),
            tokens,
            comment: Vec::new(),
        };
        let analysis = analyze_trailing_pointer_groups(&record, &directory);
        assert_eq!(analysis.candidate_count, 1);
        assert_eq!(analysis.valid_candidate_count, 1);
        let groups = analysis.groups.expect("Form 12 table boundary");
        assert_eq!(groups.token_start, expected_start);
        assert_eq!(groups.associations, vec![1]);
        assert_eq!(groups.properties, vec![3]);
    }
}

#[test]
fn type406_form12_table_boundary_beats_generic_alternatives() {
    let association = directory_target(1, 212);
    let units = directory_target(3, 316);
    let mut source = directory_target(5, 406);
    source.form = 12;
    let directory = BTreeMap::from([(1, &association), (3, &units), (5, &source)]);
    let tokens = [
        TokenValue::Integer(406),
        TokenValue::Integer(2),
        TokenValue::String(b"BASE.IGS".to_vec()),
        TokenValue::Integer(2),
        TokenValue::Integer(1),
        TokenValue::Integer(1),
        TokenValue::Integer(0),
    ]
    .into_iter()
    .map(|value| Token { value, span: 0..0 })
    .collect::<Vec<_>>();
    let record = ParameterRecord {
        directory_sequence: 5,
        line_range: 1..2,
        bytes: Vec::new(),
        parameter_end: tokens.len(),
        tokens,
        comment: Vec::new(),
    };
    let generic_valid_candidate_count = structural_pointer_group_candidates(&record)
        .iter()
        .filter_map(|candidate| groups_for_candidate(&record, &directory, *candidate))
        .filter(|groups| groups.fully_valid)
        .count();
    assert_eq!(generic_valid_candidate_count, 2);

    let analysis = analyze_trailing_pointer_groups(&record, &directory);
    assert_eq!(analysis.candidate_count, 1);
    assert_eq!(analysis.valid_candidate_count, 1);
    let groups = analysis.groups.expect("Form 12 table boundary");
    assert_eq!(groups.token_start, 4);
    assert_eq!(groups.associations, vec![1]);
    assert!(groups.properties.is_empty());
}

#[test]
fn type406_form12_malformed_count_or_name_list_do_not_enable_generic_recovery() {
    let association = directory_target(1, 212);
    let units = directory_target(3, 316);
    let mut source = directory_target(5, 406);
    source.form = 12;
    let directory = BTreeMap::from([(1, &association), (3, &units), (5, &source)]);
    let cases = [
        vec![TokenValue::Integer(406)],
        vec![
            TokenValue::Integer(406),
            TokenValue::Integer(0),
            TokenValue::Integer(1),
            TokenValue::Integer(1),
            TokenValue::Integer(0),
        ],
        vec![
            TokenValue::Integer(406),
            TokenValue::Integer(-1),
            TokenValue::Integer(1),
            TokenValue::Integer(1),
            TokenValue::Integer(0),
        ],
        vec![
            TokenValue::Integer(406),
            TokenValue::String(b"1".to_vec()),
            TokenValue::Integer(1),
            TokenValue::Integer(1),
            TokenValue::Integer(0),
        ],
        vec![
            TokenValue::Integer(406),
            TokenValue::Integer(2),
            TokenValue::String(b"BASE.IGS".to_vec()),
        ],
        vec![
            TokenValue::Integer(406),
            TokenValue::Integer(1),
            TokenValue::String(b"BASE.IGS".to_vec()),
            TokenValue::String(b"EXTRA.IGS".to_vec()),
            TokenValue::Integer(1),
            TokenValue::Integer(1),
            TokenValue::Integer(0),
        ],
        vec![
            TokenValue::Integer(406),
            TokenValue::Integer(i64::MAX),
            TokenValue::Integer(1),
            TokenValue::Integer(1),
            TokenValue::Integer(0),
        ],
    ];
    for values in cases {
        let tokens = values
            .into_iter()
            .map(|value| Token { value, span: 0..0 })
            .collect::<Vec<_>>();
        let record = ParameterRecord {
            directory_sequence: 5,
            line_range: 1..2,
            bytes: Vec::new(),
            parameter_end: tokens.len(),
            tokens,
            comment: Vec::new(),
        };
        let analysis = analyze_trailing_pointer_groups(&record, &directory);
        assert_eq!(analysis.candidate_count, 0);
        assert_eq!(analysis.valid_candidate_count, 0);
        assert!(analysis.groups.is_none());
    }
}

#[test]
fn type406_form27_entity_table_boundary_follows_np_and_value_pair_count() {
    let association = directory_target(1, 212);
    let units = directory_target(3, 316);
    let mut source = directory_target(5, 406);
    source.form = 27;
    let directory = BTreeMap::from([(1, &association), (3, &units), (5, &source)]);
    let cases = [
        (
            vec![
                TokenValue::Integer(406),
                TokenValue::Integer(4),
                TokenValue::String(b"PROPTEST".to_vec()),
                TokenValue::Integer(1),
                TokenValue::Integer(1),
                TokenValue::Integer(17),
                TokenValue::Integer(1),
                TokenValue::Integer(1),
                TokenValue::Integer(1),
                TokenValue::Integer(3),
            ],
            6,
        ),
        (
            vec![
                TokenValue::Integer(406),
                TokenValue::Integer(6),
                TokenValue::String(b"PROPTEST".to_vec()),
                TokenValue::Integer(2),
                TokenValue::Integer(1),
                TokenValue::Integer(17),
                TokenValue::Integer(3),
                TokenValue::String(b"HELLO".to_vec()),
                TokenValue::Integer(1),
                TokenValue::Integer(1),
                TokenValue::Integer(1),
                TokenValue::Integer(3),
            ],
            8,
        ),
    ];
    for (values, expected_start) in cases {
        let tokens = values
            .into_iter()
            .map(|value| Token { value, span: 0..0 })
            .collect::<Vec<_>>();
        let record = ParameterRecord {
            directory_sequence: 5,
            line_range: 1..2,
            bytes: Vec::new(),
            parameter_end: tokens.len(),
            tokens,
            comment: Vec::new(),
        };
        let analysis = analyze_trailing_pointer_groups(&record, &directory);
        assert_eq!(analysis.candidate_count, 1);
        assert_eq!(analysis.valid_candidate_count, 1);
        let groups = analysis.groups.expect("Form 27 table boundary");
        assert_eq!(groups.token_start, expected_start);
        assert_eq!(groups.associations, vec![1]);
        assert_eq!(groups.properties, vec![3]);
    }
}

#[test]
fn type406_form27_complete_counted_span_keeps_boundary_with_invalid_value_type() {
    let association = directory_target(1, 212);
    let units = directory_target(3, 316);
    let mut source = directory_target(5, 406);
    source.form = 27;
    let directory = BTreeMap::from([(1, &association), (3, &units), (5, &source)]);
    let values = vec![
        TokenValue::Integer(406),
        TokenValue::Integer(4),
        TokenValue::String(b"PROPTEST".to_vec()),
        TokenValue::Integer(1),
        TokenValue::Integer(7),
        TokenValue::Integer(17),
        TokenValue::Integer(1),
        TokenValue::Integer(1),
        TokenValue::Integer(1),
        TokenValue::Integer(3),
    ];
    let tokens = values
        .into_iter()
        .map(|value| Token { value, span: 0..0 })
        .collect::<Vec<_>>();
    let record = ParameterRecord {
        directory_sequence: 5,
        line_range: 1..2,
        bytes: Vec::new(),
        parameter_end: tokens.len(),
        tokens,
        comment: Vec::new(),
    };

    let analysis = analyze_trailing_pointer_groups(&record, &directory);
    assert_eq!(analysis.candidate_count, 1);
    assert_eq!(analysis.valid_candidate_count, 1);
    assert_eq!(
        analysis.groups.expect("Form 27 table boundary").token_start,
        6
    );
}

#[test]
fn type406_form27_malformed_np_or_value_count_does_not_enable_generic_recovery() {
    let association = directory_target(1, 212);
    let units = directory_target(3, 316);
    let mut source = directory_target(5, 406);
    source.form = 27;
    let directory = BTreeMap::from([(1, &association), (3, &units), (5, &source)]);
    let cases = vec![
        vec![
            TokenValue::Integer(406),
            TokenValue::Integer(5),
            TokenValue::String(b"PROPTEST".to_vec()),
            TokenValue::Integer(1),
            TokenValue::Integer(1),
            TokenValue::Integer(17),
            TokenValue::Integer(1),
            TokenValue::Integer(1),
            TokenValue::Integer(1),
            TokenValue::Integer(3),
        ],
        vec![
            TokenValue::Integer(406),
            TokenValue::Integer(2),
            TokenValue::String(b"PROPTEST".to_vec()),
            TokenValue::Integer(0),
            TokenValue::Integer(1),
            TokenValue::Integer(1),
            TokenValue::Integer(1),
            TokenValue::Integer(3),
        ],
        vec![
            TokenValue::Integer(406),
            TokenValue::Integer(2),
            TokenValue::String(b"PROPTEST".to_vec()),
            TokenValue::Integer(-1),
            TokenValue::Integer(1),
            TokenValue::Integer(1),
            TokenValue::Integer(1),
            TokenValue::Integer(3),
        ],
        vec![
            TokenValue::Integer(406),
            TokenValue::Integer(2),
            TokenValue::String(b"PROPTEST".to_vec()),
            TokenValue::String(b"1".to_vec()),
            TokenValue::Integer(1),
            TokenValue::Integer(17),
            TokenValue::Integer(1),
            TokenValue::Integer(1),
            TokenValue::Integer(1),
            TokenValue::Integer(3),
        ],
        vec![
            TokenValue::Integer(406),
            TokenValue::Integer(4),
            TokenValue::String(b"PROPTEST".to_vec()),
            TokenValue::Integer(1),
            TokenValue::Integer(1),
        ],
    ];
    for values in cases {
        let tokens = values
            .into_iter()
            .map(|value| Token { value, span: 0..0 })
            .collect::<Vec<_>>();
        let record = ParameterRecord {
            directory_sequence: 5,
            line_range: 1..2,
            bytes: Vec::new(),
            parameter_end: tokens.len(),
            tokens,
            comment: Vec::new(),
        };
        let analysis = analyze_trailing_pointer_groups(&record, &directory);
        assert_eq!(analysis.candidate_count, 0);
        assert_eq!(analysis.valid_candidate_count, 0);
        assert!(analysis.groups.is_none());
    }
}

#[test]
fn type402_form6_entity_table_boundary_follows_view_list() {
    for (visible_count, expected_start) in [(0_i64, 4_usize), (1, 5), (2, 6)] {
        let association = directory_target(1, 212);
        let view = directory_target(3, 410);
        let mut source = directory_target(5, 402);
        source.form = 6;
        let visible_1 = directory_target(7, 212);
        let visible_2 = directory_target(9, 212);
        let directory = BTreeMap::from([
            (1, &association),
            (3, &view),
            (5, &source),
            (7, &visible_1),
            (9, &visible_2),
        ]);
        let visible_count = usize::try_from(visible_count).unwrap();
        let mut values = vec![0_i64; expected_start + 3];
        values[0] = 402;
        values[1] = 1;
        values[2] = i64::try_from(visible_count).unwrap();
        values[3] = 3;
        for (offset, sequence) in [7_i64, 9].into_iter().take(visible_count).enumerate() {
            values[4 + offset] = sequence;
        }
        values[expected_start] = 1;
        values[expected_start + 1] = 1;
        values[expected_start + 2] = 0;
        let parameter_end = values.len();
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
            parameter_end,
            comment: Vec::new(),
        };

        let analysis = analyze_trailing_pointer_groups(&record, &directory);
        assert_eq!(analysis.candidate_count, 1, "N1={visible_count}");
        assert_eq!(analysis.valid_candidate_count, 1, "N1={visible_count}");
        let groups = analysis.groups.expect("Type 402 Form 6 table boundary");
        assert_eq!(groups.token_start, expected_start);
        assert_eq!(groups.associations, vec![1]);
        assert!(groups.properties.is_empty());
    }
}

#[test]
fn type402_form6_entity_table_boundary_precedes_valid_generic_alternative() {
    let association = directory_target(1, 212);
    let view = directory_target(3, 410);
    let mut source = directory_target(5, 402);
    source.form = 6;
    let directory = BTreeMap::from([(1, &association), (3, &view), (5, &source)]);
    let values = [402, 1, 1, 3, 2, 1, 1, 0];
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
    let groups = analysis.groups.expect("Type 402 Form 6 table boundary");
    assert_eq!(groups.token_start, 5);
    assert_eq!(groups.associations, vec![1]);
    assert!(groups.properties.is_empty());
}

#[test]
fn type402_form6_malformed_fields_do_not_enable_generic_recovery() {
    let association = directory_target(1, 212);
    let view = directory_target(3, 410);
    let mut source = directory_target(5, 402);
    source.form = 6;
    let visible = directory_target(7, 212);
    let directory = BTreeMap::from([(1, &association), (3, &view), (5, &source), (7, &visible)]);
    let cases = [
        vec![402, 0, 1, 3, 7, 1, 1, 0],
        vec![402, 1, -1, 3, 7, 1, 1, 0],
        vec![402, 1, 1000, 3, 7, 1, 1, 0],
        vec![402, 1],
        vec![402, 1, 2, 3, 7],
    ];

    for values in cases {
        let parameter_end = values.len();
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
            parameter_end,
            comment: Vec::new(),
        };

        let analysis = analyze_trailing_pointer_groups(&record, &directory);
        assert_eq!(analysis.candidate_count, 0);
        assert_eq!(analysis.valid_candidate_count, 0);
        assert!(analysis.groups.is_none());
    }

    let mut values = (0..8)
        .map(|_| Token {
            value: TokenValue::Integer(0),
            span: 0..0,
        })
        .collect::<Vec<_>>();
    values[0].value = TokenValue::Integer(402);
    values[1].value = TokenValue::Integer(1);
    values[2].value = TokenValue::String(b"1".to_vec());
    values[3].value = TokenValue::Integer(3);
    values[4].value = TokenValue::Integer(7);
    values[5].value = TokenValue::Integer(1);
    values[6].value = TokenValue::Integer(1);
    let record = ParameterRecord {
        directory_sequence: 5,
        line_range: 1..2,
        bytes: Vec::new(),
        tokens: values,
        parameter_end: 8,
        comment: Vec::new(),
    };
    let analysis = analyze_trailing_pointer_groups(&record, &directory);
    assert_eq!(analysis.candidate_count, 0);
    assert_eq!(analysis.valid_candidate_count, 0);
    assert!(analysis.groups.is_none());
}

#[test]
fn type402_form12_entity_table_boundary_follows_entry_pairs() {
    for (entry_count, expected_start) in [(1_usize, 4_usize), (2, 6)] {
        let internal_1 = directory_target(1, 116);
        let internal_2 = directory_target(5, 110);
        let association = directory_target(7, 212);
        let mut source = directory_target(3, 402);
        source.form = 12;
        let directory = BTreeMap::from([
            (1, &internal_1),
            (3, &source),
            (5, &internal_2),
            (7, &association),
        ]);
        let mut tokens = (0..expected_start + 3)
            .map(|_| Token {
                value: TokenValue::Integer(0),
                span: 0..0,
            })
            .collect::<Vec<_>>();
        tokens[0].value = TokenValue::Integer(402);
        tokens[1].value = TokenValue::Integer(i64::try_from(entry_count).unwrap());
        for (offset, sequence) in [1_i64, 5].into_iter().take(entry_count).enumerate() {
            let start = 2 + offset * 2;
            tokens[start].value = TokenValue::String(format!("REF{}", offset + 1).into_bytes());
            tokens[start + 1].value = TokenValue::Integer(sequence);
        }
        tokens[expected_start].value = TokenValue::Integer(1);
        tokens[expected_start + 1].value = TokenValue::Integer(7);
        tokens[expected_start + 2].value = TokenValue::Integer(0);
        let parameter_end = tokens.len();
        let record = ParameterRecord {
            directory_sequence: 3,
            line_range: 1..2,
            bytes: Vec::new(),
            tokens,
            parameter_end,
            comment: Vec::new(),
        };

        let analysis = analyze_trailing_pointer_groups(&record, &directory);
        assert_eq!(analysis.candidate_count, 1, "N={entry_count}");
        assert_eq!(analysis.valid_candidate_count, 1, "N={entry_count}");
        let groups = analysis.groups.expect("Type 402 Form 12 table boundary");
        assert_eq!(groups.token_start, expected_start);
        assert_eq!(groups.associations, vec![7]);
        assert!(groups.properties.is_empty());
    }
}

#[test]
fn type402_form12_malformed_counts_or_pairs_do_not_enable_generic_recovery() {
    let target = directory_target(1, 116);
    let association = directory_target(7, 212);
    let mut source = directory_target(3, 402);
    source.form = 12;
    let directory = BTreeMap::from([(1, &target), (3, &source), (7, &association)]);
    let cases = vec![
        vec![
            TokenValue::Integer(402),
            TokenValue::Integer(0),
            TokenValue::String(b"REF".to_vec()),
            TokenValue::Integer(1),
            TokenValue::Integer(1),
            TokenValue::Integer(7),
            TokenValue::Integer(0),
        ],
        vec![
            TokenValue::Integer(402),
            TokenValue::Integer(-1),
            TokenValue::String(b"REF".to_vec()),
            TokenValue::Integer(1),
            TokenValue::Integer(1),
            TokenValue::Integer(7),
            TokenValue::Integer(0),
        ],
        vec![
            TokenValue::Integer(402),
            TokenValue::Integer(i64::MAX),
            TokenValue::String(b"REF".to_vec()),
            TokenValue::Integer(1),
            TokenValue::Integer(1),
            TokenValue::Integer(7),
            TokenValue::Integer(0),
        ],
        vec![TokenValue::Integer(402)],
        vec![
            TokenValue::Integer(402),
            TokenValue::Integer(2),
            TokenValue::String(b"REF".to_vec()),
            TokenValue::Integer(1),
        ],
        vec![
            TokenValue::Integer(402),
            TokenValue::String(b"1".to_vec()),
            TokenValue::String(b"REF".to_vec()),
            TokenValue::Integer(1),
            TokenValue::Integer(1),
            TokenValue::Integer(7),
            TokenValue::Integer(0),
        ],
    ];

    for values in cases {
        let tokens = values
            .into_iter()
            .map(|value| Token { value, span: 0..0 })
            .collect::<Vec<_>>();
        let parameter_end = tokens.len();
        let record = ParameterRecord {
            directory_sequence: 3,
            line_range: 1..2,
            bytes: Vec::new(),
            tokens,
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
fn type402_form13_entity_table_boundary_follows_geometry_list() {
    for (geometry_count, expected_start) in [(1_usize, 5_usize), (2, 6)] {
        let dimension = directory_target(1, 216);
        let geometry_1 = directory_target(5, 116);
        let geometry_2 = directory_target(7, 110);
        let association = directory_target(9, 212);
        let mut source = directory_target(3, 402);
        source.form = 13;
        let directory = BTreeMap::from([
            (1, &dimension),
            (3, &source),
            (5, &geometry_1),
            (7, &geometry_2),
            (9, &association),
        ]);
        let mut tokens = (0..expected_start + 3)
            .map(|_| Token {
                value: TokenValue::Integer(0),
                span: 0..0,
            })
            .collect::<Vec<_>>();
        tokens[0].value = TokenValue::Integer(402);
        tokens[1].value = TokenValue::Integer(1);
        tokens[2].value = TokenValue::Integer(i64::try_from(geometry_count).unwrap());
        tokens[3].value = TokenValue::Integer(1);
        tokens[4].value = TokenValue::Integer(5);
        if geometry_count == 2 {
            tokens[5].value = TokenValue::Integer(7);
        }
        tokens[expected_start].value = TokenValue::Integer(1);
        tokens[expected_start + 1].value = TokenValue::Integer(9);
        tokens[expected_start + 2].value = TokenValue::Integer(0);
        let parameter_end = tokens.len();
        let record = ParameterRecord {
            directory_sequence: 3,
            line_range: 1..2,
            bytes: Vec::new(),
            tokens,
            parameter_end,
            comment: Vec::new(),
        };

        let analysis = analyze_trailing_pointer_groups(&record, &directory);
        assert_eq!(analysis.candidate_count, 1, "NG={geometry_count}");
        assert_eq!(analysis.valid_candidate_count, 1, "NG={geometry_count}");
        let groups = analysis.groups.expect("Type 402 Form 13 table boundary");
        assert_eq!(groups.token_start, expected_start);
        assert_eq!(groups.associations, vec![9]);
        assert!(groups.properties.is_empty());
    }
}

#[test]
fn type402_form13_malformed_fields_do_not_enable_generic_recovery() {
    let dimension = directory_target(1, 216);
    let geometry = directory_target(5, 116);
    let association = directory_target(9, 212);
    let mut source = directory_target(3, 402);
    source.form = 13;
    let directory = BTreeMap::from([
        (1, &dimension),
        (3, &source),
        (5, &geometry),
        (9, &association),
    ]);
    let cases = vec![
        vec![
            TokenValue::Integer(402),
            TokenValue::Integer(1),
            TokenValue::Integer(0),
            TokenValue::Integer(1),
            TokenValue::Integer(1),
            TokenValue::Integer(9),
            TokenValue::Integer(0),
        ],
        vec![
            TokenValue::Integer(402),
            TokenValue::Integer(1),
            TokenValue::Integer(-1),
            TokenValue::Integer(1),
            TokenValue::Integer(1),
            TokenValue::Integer(9),
            TokenValue::Integer(0),
        ],
        vec![
            TokenValue::Integer(402),
            TokenValue::Integer(1),
            TokenValue::Integer(i64::MAX),
            TokenValue::Integer(1),
            TokenValue::Integer(1),
            TokenValue::Integer(9),
            TokenValue::Integer(0),
        ],
        vec![
            TokenValue::Integer(402),
            TokenValue::Integer(1),
            TokenValue::String(b"1".to_vec()),
            TokenValue::Integer(1),
            TokenValue::Integer(1),
            TokenValue::Integer(9),
            TokenValue::Integer(0),
        ],
        vec![TokenValue::Integer(402), TokenValue::Integer(1)],
        vec![
            TokenValue::Integer(402),
            TokenValue::Integer(1),
            TokenValue::Integer(2),
            TokenValue::Integer(1),
            TokenValue::Integer(5),
        ],
        vec![
            TokenValue::Integer(402),
            TokenValue::Integer(2),
            TokenValue::Integer(1),
            TokenValue::Integer(1),
            TokenValue::Integer(5),
            TokenValue::Integer(1),
            TokenValue::Integer(9),
            TokenValue::Integer(0),
        ],
    ];

    for values in cases {
        let tokens = values
            .into_iter()
            .map(|value| Token { value, span: 0..0 })
            .collect::<Vec<_>>();
        let parameter_end = tokens.len();
        let record = ParameterRecord {
            directory_sequence: 3,
            line_range: 1..2,
            bytes: Vec::new(),
            tokens,
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
fn type408_fixed_primary_boundary_follows_translation_and_scale() {
    let association = directory_target(3, 212);
    let source = directory_target(7, 408);
    let definition = directory_target(9, 308);
    let directory = BTreeMap::from([(3, &association), (7, &source), (9, &definition)]);
    let record = token_parameter_record(
        7,
        vec![
            408.into(),
            9.into(),
            1.into(),
            2.into(),
            3.into(),
            TokenValue::Real(0.5),
            1.into(),
            3.into(),
            0.into(),
        ],
    );

    let analysis = analyze_trailing_pointer_groups(&record, &directory);
    assert_eq!(analysis.candidate_count, 1);
    assert_eq!(analysis.valid_candidate_count, 1);
    let groups = analysis.groups.expect("Type 408 table boundary");
    assert_eq!(groups.token_start, 6);
    assert_eq!(groups.associations, vec![3]);
    assert!(groups.properties.is_empty());
}

#[test]
fn type408_fixed_primary_boundary_precedes_valid_generic_alternative() {
    let association_1 = directory_target(1, 212);
    let association_3 = directory_target(3, 212);
    let property = directory_target(5, 406);
    let source = directory_target(7, 408);
    let definition = directory_target(9, 308);
    let directory = BTreeMap::from([
        (1, &association_1),
        (3, &association_3),
        (5, &property),
        (7, &source),
        (9, &definition),
    ]);
    let record = integer_parameter_record(7, &[408, 9, 1, 2, 3, 2, 1, 3, 6, 5, 5, 5, 5, 5, 5]);

    let analysis = analyze_trailing_pointer_groups(&record, &directory);
    assert_eq!(analysis.candidate_count, 1);
    assert_eq!(analysis.valid_candidate_count, 1);
    let groups = analysis.groups.expect("Type 408 table boundary");
    assert_eq!(groups.token_start, 6);
    assert_eq!(groups.associations, vec![3]);
    assert_eq!(groups.properties, vec![5; 6]);
}

#[test]
fn type408_complete_wrong_fields_keep_boundary_and_malformed_spans_do_not_recover() {
    let association_1 = directory_target(1, 212);
    let association_3 = directory_target(3, 212);
    let source = directory_target(7, 408);
    let definition = directory_target(9, 308);
    let directory = BTreeMap::from([
        (1, &association_1),
        (3, &association_3),
        (7, &source),
        (9, &definition),
    ]);
    let wrong_fields = token_parameter_record(
        7,
        vec![
            408.into(),
            TokenValue::String(b"bad".to_vec()),
            TokenValue::Real(2.0),
            TokenValue::String(b"bad".to_vec()),
            1.into(),
            TokenValue::Omitted,
            1.into(),
            3.into(),
            0.into(),
        ],
    );
    let analysis = analyze_trailing_pointer_groups(&wrong_fields, &directory);
    assert_eq!(analysis.candidate_count, 1);
    assert_eq!(analysis.valid_candidate_count, 1);
    assert_eq!(
        analysis
            .groups
            .expect("Type 408 table boundary")
            .token_start,
        6
    );

    for values in [vec![408, 9, 1, 2, 3], vec![408, 9, 1, 2, 3, 1, 1, 3]] {
        let analysis =
            analyze_trailing_pointer_groups(&integer_parameter_record(7, &values), &directory);
        assert_eq!(analysis.candidate_count, 0);
        assert_eq!(analysis.valid_candidate_count, 0);
        assert!(analysis.groups.is_none());
    }
}

#[test]
fn type402_form19_entity_table_boundary_follows_segment_blocks() {
    for (block_count, expected_start) in [(1_i64, 8_usize), (2, 14)] {
        let association = directory_target(3, 212);
        let mut source = directory_target(11, 402);
        source.form = 19;
        let directory = BTreeMap::from([(3, &association), (11, &source)]);
        let mut values = vec![0_i64; expected_start + 3];
        values[0] = 402;
        values[1] = block_count;
        values[expected_start] = 1;
        values[expected_start + 1] = 3;
        values[expected_start + 2] = 0;

        let analysis =
            analyze_trailing_pointer_groups(&integer_parameter_record(11, &values), &directory);
        assert_eq!(analysis.candidate_count, 1, "block_count={block_count}");
        assert_eq!(
            analysis.valid_candidate_count, 1,
            "block_count={block_count}"
        );
        let groups = analysis.groups.expect("Type 402 Form 19 table boundary");
        assert_eq!(groups.token_start, expected_start);
        assert_eq!(groups.associations, vec![3]);
        assert!(groups.properties.is_empty());
    }
}

#[test]
fn type402_form19_entity_table_boundary_precedes_valid_generic_alternative() {
    let association_1 = directory_target(1, 212);
    let association_3 = directory_target(3, 212);
    let property_5 = directory_target(5, 406);
    let property_7 = directory_target(7, 406);
    let mut source = directory_target(11, 402);
    source.form = 19;
    let directory = BTreeMap::from([
        (1, &association_1),
        (3, &association_3),
        (5, &property_5),
        (7, &property_7),
        (11, &source),
    ]);
    let record = integer_parameter_record(11, &[402, 1, 9, 0, 0, 0, 0, 2, 1, 3, 2, 5, 7]);

    let analysis = analyze_trailing_pointer_groups(&record, &directory);
    assert_eq!(analysis.candidate_count, 1);
    assert_eq!(analysis.valid_candidate_count, 1);
    let groups = analysis.groups.expect("Type 402 Form 19 table boundary");
    assert_eq!(groups.token_start, 8);
    assert_eq!(groups.associations, vec![3]);
    assert_eq!(groups.properties, vec![5, 7]);
}

#[test]
fn type402_form19_malformed_count_or_span_does_not_enable_generic_recovery() {
    let association = directory_target(3, 212);
    let mut source = directory_target(11, 402);
    source.form = 19;
    let directory = BTreeMap::from([(3, &association), (11, &source)]);

    let wrong_fields = token_parameter_record(
        11,
        vec![
            402.into(),
            1.into(),
            TokenValue::String(b"bad".to_vec()),
            TokenValue::Real(0.5),
            0.into(),
            TokenValue::Omitted,
            TokenValue::Omitted,
            2.into(),
            1.into(),
            3.into(),
            0.into(),
        ],
    );
    let analysis = analyze_trailing_pointer_groups(&wrong_fields, &directory);
    assert_eq!(analysis.candidate_count, 1);
    assert_eq!(analysis.valid_candidate_count, 1);
    let groups = analysis.groups.expect("Type 402 Form 19 table boundary");
    assert_eq!(groups.token_start, 8);
    assert_eq!(groups.associations, vec![3]);
    assert!(groups.properties.is_empty());

    let malformed = vec![
        token_parameter_record(
            11,
            vec![
                402.into(),
                0.into(),
                9.into(),
                0.into(),
                0.into(),
                0.into(),
                0.into(),
                0.into(),
                1.into(),
                3.into(),
                0.into(),
            ],
        ),
        token_parameter_record(
            11,
            vec![
                402.into(),
                (-1_i64).into(),
                9.into(),
                0.into(),
                0.into(),
                0.into(),
                0.into(),
                0.into(),
                1.into(),
                3.into(),
                0.into(),
            ],
        ),
        token_parameter_record(
            11,
            vec![
                402.into(),
                i64::MAX.into(),
                9.into(),
                0.into(),
                0.into(),
                0.into(),
                0.into(),
                0.into(),
                1.into(),
                3.into(),
                0.into(),
            ],
        ),
        token_parameter_record(
            11,
            vec![
                402.into(),
                TokenValue::Real(1.0),
                9.into(),
                0.into(),
                0.into(),
                0.into(),
                0.into(),
                0.into(),
                1.into(),
                3.into(),
                0.into(),
            ],
        ),
        token_parameter_record(
            11,
            vec![
                402.into(),
                1.into(),
                9.into(),
                0.into(),
                0.into(),
                0.into(),
                0.into(),
            ],
        ),
        token_parameter_record(
            11,
            vec![
                402.into(),
                1.into(),
                9.into(),
                0.into(),
                0.into(),
                0.into(),
                0.into(),
                0.into(),
                1.into(),
                3.into(),
            ],
        ),
    ];
    for record in malformed {
        let analysis = analyze_trailing_pointer_groups(&record, &directory);
        assert_eq!(analysis.candidate_count, 0);
        assert_eq!(analysis.valid_candidate_count, 0);
        assert!(analysis.groups.is_none());
    }
}

#[test]
fn type402_form18_entity_table_boundary_follows_all_class_lists() {
    for (counts, expected_start) in [
        (vec![0_i64; 6], 10_usize),
        (vec![1, 1, 1, 1, 1, 1], 16),
        (vec![2, 1, 1, 1, 1, 1], 17),
    ] {
        let association = directory_target(1, 212);
        let mut source = directory_target(3, 402);
        source.form = 18;
        let directory = BTreeMap::from([(1, &association), (3, &source)]);
        let mut values = vec![0_i64; expected_start + 3];
        values[0] = 402;
        values[1] = 2;
        values[2..8].copy_from_slice(&counts);
        values[8] = 1;
        values[9] = 2;
        values[expected_start] = 1;
        values[expected_start + 1] = 1;
        values[expected_start + 2] = 0;
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
        assert_eq!(analysis.candidate_count, 1, "counts={counts:?}");
        assert_eq!(analysis.valid_candidate_count, 1, "counts={counts:?}");
        let groups = analysis.groups.expect("Type 402 Form 18 table boundary");
        assert_eq!(groups.token_start, expected_start);
        assert_eq!(groups.associations, vec![1]);
        assert!(groups.properties.is_empty());
    }
}

#[test]
fn type402_form18_malformed_fields_do_not_enable_generic_recovery() {
    let association = directory_target(1, 212);
    let mut source = directory_target(3, 402);
    source.form = 18;
    let directory = BTreeMap::from([(1, &association), (3, &source)]);
    let cases = vec![
        vec![
            TokenValue::Integer(402),
            TokenValue::Integer(1),
            TokenValue::Integer(0),
            TokenValue::Integer(0),
            TokenValue::Integer(0),
            TokenValue::Integer(0),
            TokenValue::Integer(0),
            TokenValue::Integer(0),
            TokenValue::Integer(1),
            TokenValue::Integer(2),
            TokenValue::Integer(1),
            TokenValue::Integer(1),
            TokenValue::Integer(0),
        ],
        vec![
            TokenValue::Integer(402),
            TokenValue::Integer(2),
            TokenValue::Integer(-1),
            TokenValue::Integer(0),
            TokenValue::Integer(0),
            TokenValue::Integer(0),
            TokenValue::Integer(0),
            TokenValue::Integer(0),
            TokenValue::Integer(1),
            TokenValue::Integer(2),
            TokenValue::Integer(1),
            TokenValue::Integer(1),
            TokenValue::Integer(0),
        ],
        vec![
            TokenValue::Integer(402),
            TokenValue::Integer(2),
            TokenValue::Integer(i64::MAX),
            TokenValue::Integer(0),
            TokenValue::Integer(0),
            TokenValue::Integer(0),
            TokenValue::Integer(0),
            TokenValue::Integer(0),
            TokenValue::Integer(1),
            TokenValue::Integer(2),
            TokenValue::Integer(1),
            TokenValue::Integer(1),
            TokenValue::Integer(0),
        ],
        vec![
            TokenValue::Integer(402),
            TokenValue::Integer(2),
            TokenValue::String(b"1".to_vec()),
            TokenValue::Integer(0),
            TokenValue::Integer(0),
            TokenValue::Integer(0),
            TokenValue::Integer(0),
            TokenValue::Integer(0),
            TokenValue::Integer(1),
            TokenValue::Integer(2),
            TokenValue::Integer(1),
            TokenValue::Integer(1),
            TokenValue::Integer(0),
        ],
        vec![TokenValue::Integer(402), TokenValue::Integer(2)],
        vec![
            TokenValue::Integer(402),
            TokenValue::Integer(2),
            TokenValue::Integer(2),
            TokenValue::Integer(0),
            TokenValue::Integer(0),
            TokenValue::Integer(0),
            TokenValue::Integer(0),
            TokenValue::Integer(0),
            TokenValue::Integer(1),
            TokenValue::Integer(2),
            TokenValue::Integer(1),
            TokenValue::Integer(1),
            TokenValue::Integer(0),
        ],
    ];

    for values in cases {
        let tokens = values
            .into_iter()
            .map(|value| Token { value, span: 0..0 })
            .collect::<Vec<_>>();
        let parameter_end = tokens.len();
        let record = ParameterRecord {
            directory_sequence: 3,
            line_range: 1..2,
            bytes: Vec::new(),
            tokens,
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
fn type402_form20_entity_table_boundary_follows_all_class_lists() {
    for (counts, expected_start) in [
        (vec![0_i64; 6], 9_usize),
        (vec![1, 1, 1, 1, 1, 1], 15),
        (vec![2, 1, 1, 1, 1, 1], 16),
    ] {
        let association = directory_target(1, 212);
        let mut source = directory_target(3, 402);
        source.form = 20;
        let directory = BTreeMap::from([(1, &association), (3, &source)]);
        let mut values = vec![0_i64; expected_start + 3];
        values[0] = 402;
        values[1] = 1;
        values[2..8].copy_from_slice(&counts);
        values[8] = 1;
        values[expected_start] = 1;
        values[expected_start + 1] = 1;
        values[expected_start + 2] = 0;
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
        assert_eq!(analysis.candidate_count, 1, "counts={counts:?}");
        assert_eq!(analysis.valid_candidate_count, 1, "counts={counts:?}");
        let groups = analysis.groups.expect("Type 402 Form 20 table boundary");
        assert_eq!(groups.token_start, expected_start);
        assert_eq!(groups.associations, vec![1]);
        assert!(groups.properties.is_empty());
    }
}

#[test]
fn type402_form20_entity_table_boundary_beats_target_valid_generic_alternative() {
    let target_1 = directory_target(1, 212);
    let target_3 = directory_target(3, 212);
    let mut source = directory_target(5, 402);
    source.form = 20;
    let directory = BTreeMap::from([(1, &target_1), (3, &target_3), (5, &source)]);
    let values = [402, 1, 0, 0, 0, 0, 0, 1, 1, 2, 1, 3, 0];
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
    let groups = analysis.groups.expect("Type 402 Form 20 table boundary");
    assert_eq!(groups.token_start, 10);
    assert_eq!(groups.associations, vec![3]);
    assert!(groups.properties.is_empty());
}

#[test]
fn type402_form20_malformed_fields_do_not_enable_generic_recovery() {
    let association = directory_target(1, 212);
    let mut source = directory_target(3, 402);
    source.form = 20;
    let directory = BTreeMap::from([(1, &association), (3, &source)]);
    let cases = vec![
        vec![
            TokenValue::Integer(402),
            TokenValue::Integer(2),
            TokenValue::Integer(0),
            TokenValue::Integer(0),
            TokenValue::Integer(0),
            TokenValue::Integer(0),
            TokenValue::Integer(0),
            TokenValue::Integer(0),
            TokenValue::Integer(1),
            TokenValue::Integer(1),
            TokenValue::Integer(1),
            TokenValue::Integer(0),
        ],
        vec![
            TokenValue::Integer(402),
            TokenValue::Integer(1),
            TokenValue::Integer(-1),
            TokenValue::Integer(0),
            TokenValue::Integer(0),
            TokenValue::Integer(0),
            TokenValue::Integer(0),
            TokenValue::Integer(0),
            TokenValue::Integer(1),
            TokenValue::Integer(1),
            TokenValue::Integer(1),
            TokenValue::Integer(0),
        ],
        vec![
            TokenValue::Integer(402),
            TokenValue::Integer(1),
            TokenValue::Integer(i64::MAX),
            TokenValue::Integer(0),
            TokenValue::Integer(0),
            TokenValue::Integer(0),
            TokenValue::Integer(0),
            TokenValue::Integer(0),
            TokenValue::Integer(1),
            TokenValue::Integer(1),
            TokenValue::Integer(1),
            TokenValue::Integer(0),
        ],
        vec![
            TokenValue::Integer(402),
            TokenValue::Integer(1),
            TokenValue::String(b"1".to_vec()),
            TokenValue::Integer(0),
            TokenValue::Integer(0),
            TokenValue::Integer(0),
            TokenValue::Integer(0),
            TokenValue::Integer(0),
            TokenValue::Integer(1),
            TokenValue::Integer(1),
            TokenValue::Integer(1),
            TokenValue::Integer(0),
        ],
        vec![TokenValue::Integer(402), TokenValue::Integer(1)],
        vec![
            TokenValue::Integer(402),
            TokenValue::Integer(1),
            TokenValue::Integer(1),
            TokenValue::Integer(1),
            TokenValue::Integer(1),
            TokenValue::Integer(1),
            TokenValue::Integer(1),
            TokenValue::Integer(1),
            TokenValue::Integer(1),
        ],
    ];

    for values in cases {
        let tokens = values
            .into_iter()
            .map(|value| Token { value, span: 0..0 })
            .collect::<Vec<_>>();
        let parameter_end = tokens.len();
        let record = ParameterRecord {
            directory_sequence: 3,
            line_range: 1..2,
            bytes: Vec::new(),
            tokens,
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

#[test]
fn type143_entity_table_boundary_uses_boundary_count() {
    for (boundary_count, expected_start) in [(0_i64, 4_usize), (1, 5), (2, 6)] {
        let association = directory_target(1, 212);
        let source = directory_target(3, 143);
        let directory = BTreeMap::from([(1, &association), (3, &source)]);
        let mut values = vec![0_i64; expected_start + 3];
        values[0] = 143;
        values[1] = 1;
        values[2] = 1;
        values[3] = boundary_count;
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
        let groups = analysis.groups.expect("Type 143 table boundary");
        assert_eq!(groups.token_start, expected_start);
        assert_eq!(groups.associations, vec![1]);
        assert!(groups.properties.is_empty());
    }
}

#[test]
fn type143_entity_table_boundary_precedes_valid_generic_alternative() {
    let target_1 = directory_target(1, 212);
    let target_3 = directory_target(3, 212);
    let source = directory_target(5, 143);
    let directory = BTreeMap::from([(1, &target_1), (3, &target_3), (5, &source)]);
    let values = [143, 1, 1, 1, 2, 1, 3, 0];
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
    let groups = analysis.groups.expect("Type 143 table boundary");
    assert_eq!(groups.token_start, 5);
    assert_eq!(groups.associations, vec![3]);
    assert!(groups.properties.is_empty());
}

#[test]
fn type143_malformed_boundary_counts_do_not_enable_generic_recovery() {
    let association = directory_target(1, 212);
    let source = directory_target(3, 143);
    let directory = BTreeMap::from([(1, &association), (3, &source)]);
    for values in [
        vec![143, 1, 1, -1, 0, 1, 1, 0],
        vec![143, 1, 1, 100, 0, 1, 1, 0],
        vec![143, 1, 1],
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
fn type141_entity_table_boundary_uses_nested_curve_counts() {
    for (counts, expected_start) in [
        (vec![0_i64], 8_usize),
        (vec![2], 10),
        (vec![0, 0], 11),
        (vec![1, 2], 14),
    ] {
        let association = directory_target(1, 212);
        let source = directory_target(3, 141);
        let directory = BTreeMap::from([(1, &association), (3, &source)]);
        let mut values = vec![0_i64; expected_start + 3];
        values[0] = 141;
        values[1] = i64::from(counts.iter().any(|count| *count > 0));
        values[2] = 1;
        values[3] = 1;
        values[4] = i64::try_from(counts.len()).expect("test count fits");
        let mut index = 5;
        for count in counts {
            values[index] = 1;
            values[index + 1] = 1;
            values[index + 2] = count;
            for pcurve_index in 0..usize::try_from(count).expect("test count is nonnegative") {
                values[index + 3 + pcurve_index] = 1;
            }
            index += 3 + usize::try_from(count).expect("test count is nonnegative");
        }
        values[expected_start] = 1;
        values[expected_start + 1] = 1;
        values[expected_start + 2] = 0;
        assert_eq!(index, expected_start);
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
        let groups = analysis.groups.expect("Type 141 table boundary");
        assert_eq!(groups.token_start, expected_start);
        assert_eq!(groups.associations, vec![1]);
        assert!(groups.properties.is_empty());
    }
}

#[test]
fn type141_entity_table_boundary_precedes_valid_generic_alternative() {
    let target_1 = directory_target(1, 212);
    let target_3 = directory_target(3, 212);
    let source = directory_target(5, 141);
    let directory = BTreeMap::from([(1, &target_1), (3, &target_3), (5, &source)]);
    let values = [141, 1, 1, 1, 1, 1, 3, 2, 1, 2, 1, 3, 0];
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
    let groups = analysis.groups.expect("Type 141 table boundary");
    assert_eq!(groups.token_start, 10);
    assert_eq!(groups.associations, vec![3]);
    assert!(groups.properties.is_empty());
}

#[test]
fn type141_malformed_boundary_counts_do_not_enable_generic_recovery() {
    let association = directory_target(1, 212);
    let source = directory_target(3, 141);
    let directory = BTreeMap::from([(1, &association), (3, &source)]);
    for values in [
        vec![141, 1, 1, 0, 0, 1, 1, 0],
        vec![141, 1, 1, -1, 0, 1, 1, 0],
        vec![141, 1, 1, 100, 0, 1, 1, 0],
        vec![141, 1, 1, 1, 1, 1, 1, 1, 1, 0],
        vec![141, 1, 1, 1, 1, 1, 1, -1, 1, 1, 0],
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
fn type410_entity_table_boundaries_follow_view_fields() {
    let cases = [
        (0_i64, vec![410, 1, 1, 0, 0, 0, 0, 0, 0, 1, 3, 0], 9_usize),
        (
            1_i64,
            vec![
                410, 2, 1, 0, 0, 1, 0, 0, 0, 0, 0, 10, 0, 1, 0, 5, -2, 2, -1, 1, 3, -5, 5, 1, 3, 0,
            ],
            23,
        ),
    ];

    for (form, values, expected_start) in cases {
        let association = directory_target(3, 212);
        let mut source = directory_target(9, 410);
        source.form = form;
        let directory = BTreeMap::from([(3, &association), (9, &source)]);
        let record = integer_parameter_record(9, &values);

        let analysis = analyze_trailing_pointer_groups(&record, &directory);
        assert_eq!(analysis.candidate_count, 1);
        assert_eq!(analysis.valid_candidate_count, 1);
        let groups = analysis.groups.expect("Type 410 table boundary");
        assert_eq!(groups.token_start, expected_start);
        assert_eq!(groups.associations, vec![3]);
        assert!(groups.properties.is_empty());
    }
}

#[test]
fn type410_entity_table_boundary_precedes_valid_generic_alternative() {
    let association_1 = directory_target(1, 212);
    let association_3 = directory_target(3, 212);
    let property_5 = directory_target(5, 406);
    let property_7 = directory_target(7, 406);

    for (form, values, expected_start) in [
        (
            0_i64,
            vec![410, 1, 1, 0, 0, 0, 0, 0, 2, 1, 3, 2, 5, 7],
            9_usize,
        ),
        (
            1_i64,
            vec![
                410, 2, 1, 0, 0, 1, 0, 0, 0, 0, 0, 10, 0, 1, 0, 5, -2, 2, -1, 1, 3, -5, 2, 1, 3, 2,
                5, 7,
            ],
            23,
        ),
    ] {
        let mut source = directory_target(9, 410);
        source.form = form;
        let directory = BTreeMap::from([
            (1, &association_1),
            (3, &association_3),
            (5, &property_5),
            (7, &property_7),
            (9, &source),
        ]);
        let record = integer_parameter_record(9, &values);

        let analysis = analyze_trailing_pointer_groups(&record, &directory);
        assert_eq!(analysis.candidate_count, 1);
        assert_eq!(analysis.valid_candidate_count, 1);
        let groups = analysis.groups.expect("Type 410 table boundary");
        assert_eq!(groups.token_start, expected_start);
        assert_eq!(groups.associations, vec![3]);
        assert_eq!(groups.properties, vec![5, 7]);
    }
}

#[test]
fn type410_complete_wrong_fields_keep_boundary_and_truncated_spans_do_not_recover() {
    let association = directory_target(3, 212);
    let mut source = directory_target(9, 410);
    let directory = BTreeMap::from([(3, &association), (9, &source)]);

    let wrong_form0 = token_parameter_record(
        9,
        vec![
            410.into(),
            TokenValue::String(b"bad".to_vec()),
            1.into(),
            0.into(),
            0.into(),
            0.into(),
            0.into(),
            0.into(),
            0.into(),
            1.into(),
            3.into(),
            0.into(),
        ],
    );
    let analysis = analyze_trailing_pointer_groups(&wrong_form0, &directory);
    assert_eq!(analysis.candidate_count, 1);
    assert_eq!(analysis.valid_candidate_count, 1);
    assert_eq!(
        analysis
            .groups
            .expect("Type 410 Form 0 boundary")
            .token_start,
        9
    );

    for values in [
        vec![410, 1, 1, 0, 0, 0, 0, 0],
        vec![410, 1, 1, 0, 0, 0, 0, 0, 0, 1, 3],
    ] {
        let analysis =
            analyze_trailing_pointer_groups(&integer_parameter_record(9, &values), &directory);
        assert_eq!(analysis.candidate_count, 0);
        assert_eq!(analysis.valid_candidate_count, 0);
        assert!(analysis.groups.is_none());
    }

    source.form = 1;
    let directory = BTreeMap::from([(3, &association), (9, &source)]);
    let wrong_form1 = token_parameter_record(
        9,
        vec![
            410.into(),
            TokenValue::String(b"bad".to_vec()),
            TokenValue::Real(1.5),
            0.into(),
            0.into(),
            1.into(),
            0.into(),
            0.into(),
            0.into(),
            0.into(),
            0.into(),
            10.into(),
            0.into(),
            1.into(),
            0.into(),
            5.into(),
            TokenValue::Real(-2.0),
            TokenValue::Real(2.0),
            TokenValue::Real(-1.0),
            TokenValue::Real(1.0),
            3.into(),
            TokenValue::Real(-5.0),
            TokenValue::Real(5.0),
            1.into(),
            3.into(),
            0.into(),
        ],
    );
    let analysis = analyze_trailing_pointer_groups(&wrong_form1, &directory);
    assert_eq!(analysis.candidate_count, 1);
    assert_eq!(analysis.valid_candidate_count, 1);
    assert_eq!(
        analysis
            .groups
            .expect("Type 410 Form 1 boundary")
            .token_start,
        23
    );

    for values in [
        vec![
            410, 2, 1, 0, 0, 1, 0, 0, 0, 0, 0, 10, 0, 1, 0, 5, -2, 2, -1, 1, 3, -5,
        ],
        vec![
            410, 2, 1, 0, 0, 1, 0, 0, 0, 0, 0, 10, 0, 1, 0, 5, -2, 2, -1, 1, 3, -5, 5, 1, 3,
        ],
    ] {
        let analysis =
            analyze_trailing_pointer_groups(&integer_parameter_record(9, &values), &directory);
        assert_eq!(analysis.candidate_count, 0);
        assert_eq!(analysis.valid_candidate_count, 0);
        assert!(analysis.groups.is_none());
    }
}

#[test]
fn type416_entity_table_boundaries_follow_external_reference_fields() {
    let association = directory_target(3, 212);
    let mut source = directory_target(9, 416);
    for (form, values, expected_start) in [
        (
            0_i64,
            vec![
                TokenValue::Integer(416),
                TokenValue::String(b"FILE01".to_vec()),
                TokenValue::String(b"ONE".to_vec()),
                1.into(),
                3.into(),
                0.into(),
            ],
            3_usize,
        ),
        (
            1_i64,
            vec![
                TokenValue::Integer(416),
                TokenValue::String(b"FILE01".to_vec()),
                1.into(),
                3.into(),
                0.into(),
            ],
            2,
        ),
        (
            2_i64,
            vec![
                TokenValue::Integer(416),
                TokenValue::String(b"FILE01".to_vec()),
                TokenValue::String(b"LOG".to_vec()),
                1.into(),
                3.into(),
                0.into(),
            ],
            3,
        ),
        (
            3_i64,
            vec![
                TokenValue::Integer(416),
                TokenValue::String(b"NAT".to_vec()),
                1.into(),
                3.into(),
                0.into(),
            ],
            2,
        ),
        (
            4_i64,
            vec![
                TokenValue::Integer(416),
                TokenValue::String(b"LIBRARY".to_vec()),
                TokenValue::String(b"NAT".to_vec()),
                1.into(),
                3.into(),
                0.into(),
            ],
            3,
        ),
    ] {
        source.form = form;
        let directory = BTreeMap::from([(3, &association), (9, &source)]);
        let record = token_parameter_record(9, values);
        let analysis = analyze_trailing_pointer_groups(&record, &directory);
        assert_eq!(analysis.candidate_count, 1, "Form {form}");
        assert_eq!(analysis.valid_candidate_count, 1, "Form {form}");
        let groups = analysis.groups.expect("Type 416 table boundary");
        assert_eq!(groups.token_start, expected_start, "Form {form}");
        assert_eq!(groups.associations, vec![3], "Form {form}");
        assert!(groups.properties.is_empty(), "Form {form}");
    }
}

#[test]
fn type416_entity_table_boundary_precedes_valid_generic_alternative() {
    let association_1 = directory_target(1, 212);
    let association_3 = directory_target(3, 212);
    let property_5 = directory_target(5, 406);
    let property_7 = directory_target(7, 406);
    let mut source = directory_target(9, 416);

    for (form, values, expected_start) in [
        (
            0_i64,
            vec![
                TokenValue::Integer(416),
                TokenValue::String(b"FILE01".to_vec()),
                2.into(),
                1.into(),
                3.into(),
                2.into(),
                5.into(),
                7.into(),
            ],
            3_usize,
        ),
        (
            1_i64,
            vec![
                416.into(),
                2.into(),
                1.into(),
                3.into(),
                2.into(),
                5.into(),
                7.into(),
            ],
            2,
        ),
        (
            2_i64,
            vec![
                TokenValue::Integer(416),
                TokenValue::String(b"FILE01".to_vec()),
                2.into(),
                1.into(),
                3.into(),
                2.into(),
                5.into(),
                7.into(),
            ],
            3,
        ),
        (
            3_i64,
            vec![
                416.into(),
                2.into(),
                1.into(),
                3.into(),
                2.into(),
                5.into(),
                7.into(),
            ],
            2,
        ),
        (
            4_i64,
            vec![
                TokenValue::Integer(416),
                TokenValue::String(b"LIBRARY".to_vec()),
                2.into(),
                1.into(),
                3.into(),
                2.into(),
                5.into(),
                7.into(),
            ],
            3,
        ),
    ] {
        source.form = form;
        let directory = BTreeMap::from([
            (1, &association_1),
            (3, &association_3),
            (5, &property_5),
            (7, &property_7),
            (9, &source),
        ]);
        let record = token_parameter_record(9, values);
        let generic = structural_pointer_group_candidates(&record);
        assert!(generic
            .iter()
            .any(|candidate| candidate.token_start != expected_start));
        let analysis = analyze_trailing_pointer_groups(&record, &directory);
        assert_eq!(analysis.candidate_count, 1, "Form {form}");
        assert_eq!(analysis.valid_candidate_count, 1, "Form {form}");
        let groups = analysis.groups.expect("Type 416 table boundary");
        assert_eq!(groups.token_start, expected_start, "Form {form}");
        assert_eq!(groups.associations, vec![3], "Form {form}");
        assert_eq!(groups.properties, vec![5, 7], "Form {form}");
    }
}

#[test]
fn type416_complete_wrong_fields_keep_boundary_and_truncated_spans_do_not_recover() {
    let association = directory_target(3, 212);
    let mut source = directory_target(9, 416);

    for (form, wrong_values, expected_start, truncated_primary, truncated_group) in [
        (
            0_i64,
            vec![
                416.into(),
                TokenValue::String(b"BAD".to_vec()),
                TokenValue::String(b"NAME".to_vec()),
                1.into(),
                3.into(),
                0.into(),
            ],
            3_usize,
            vec![416.into(), TokenValue::String(b"FILE01".to_vec())],
            vec![
                416.into(),
                TokenValue::String(b"FILE01".to_vec()),
                TokenValue::String(b"NAME".to_vec()),
                1.into(),
                3.into(),
            ],
        ),
        (
            1_i64,
            vec![
                416.into(),
                TokenValue::String(b"BAD".to_vec()),
                1.into(),
                3.into(),
                0.into(),
            ],
            2,
            vec![416.into()],
            vec![
                416.into(),
                TokenValue::String(b"FILE01".to_vec()),
                1.into(),
                3.into(),
            ],
        ),
        (
            2_i64,
            vec![
                416.into(),
                TokenValue::String(b"BAD".to_vec()),
                TokenValue::String(b"LOG".to_vec()),
                1.into(),
                3.into(),
                0.into(),
            ],
            3,
            vec![416.into(), TokenValue::String(b"FILE01".to_vec())],
            vec![
                416.into(),
                TokenValue::String(b"FILE01".to_vec()),
                TokenValue::String(b"LOG".to_vec()),
                1.into(),
                3.into(),
            ],
        ),
        (
            3_i64,
            vec![
                416.into(),
                TokenValue::String(b"BAD".to_vec()),
                1.into(),
                3.into(),
                0.into(),
            ],
            2,
            vec![416.into()],
            vec![
                416.into(),
                TokenValue::String(b"FILE01".to_vec()),
                1.into(),
                3.into(),
            ],
        ),
        (
            4_i64,
            vec![
                416.into(),
                TokenValue::String(b"LIB".to_vec()),
                TokenValue::String(b"BAD".to_vec()),
                1.into(),
                3.into(),
                0.into(),
            ],
            3,
            vec![416.into(), TokenValue::String(b"LIBRARY".to_vec())],
            vec![
                416.into(),
                TokenValue::String(b"LIBRARY".to_vec()),
                TokenValue::String(b"NAT".to_vec()),
                1.into(),
                3.into(),
            ],
        ),
    ] {
        source.form = form;
        let directory = BTreeMap::from([(3, &association), (9, &source)]);
        let wrong = token_parameter_record(9, wrong_values);
        let analysis = analyze_trailing_pointer_groups(&wrong, &directory);
        assert_eq!(analysis.candidate_count, 1, "Form {form}");
        assert_eq!(analysis.valid_candidate_count, 1, "Form {form}");
        assert_eq!(
            analysis
                .groups
                .expect("Type 416 complete boundary")
                .token_start,
            expected_start,
            "Form {form}"
        );

        for values in [truncated_primary, truncated_group] {
            let analysis =
                analyze_trailing_pointer_groups(&token_parameter_record(9, values), &directory);
            assert_eq!(analysis.candidate_count, 0, "Form {form}");
            assert_eq!(analysis.valid_candidate_count, 0, "Form {form}");
            assert!(analysis.groups.is_none(), "Form {form}");
        }
    }
}

#[test]
fn type420_entity_table_boundary_follows_connect_point_count() {
    let association = directory_target(1, 212);
    let source = directory_target(9, 420);
    let directory = BTreeMap::from([(1, &association), (9, &source)]);

    for (connect_count, expected_start) in [(0_i64, 12_usize), (1, 13), (2, 14)] {
        let mut values = vec![0_i64; expected_start + 3];
        values[0] = 420;
        values[11] = connect_count;
        values[expected_start] = 1;
        values[expected_start + 1] = 1;
        values[expected_start + 2] = 0;
        let analysis =
            analyze_trailing_pointer_groups(&integer_parameter_record(9, &values), &directory);
        assert_eq!(analysis.candidate_count, 1, "NC={connect_count}");
        assert_eq!(analysis.valid_candidate_count, 1, "NC={connect_count}");
        let groups = analysis.groups.expect("Type 420 table boundary");
        assert_eq!(groups.token_start, expected_start, "NC={connect_count}");
        assert_eq!(groups.associations, vec![1], "NC={connect_count}");
        assert!(groups.properties.is_empty(), "NC={connect_count}");
    }
}

#[test]
fn type420_entity_table_boundary_precedes_valid_generic_alternative() {
    let association_1 = directory_target(1, 212);
    let association_3 = directory_target(3, 212);
    let property_5 = directory_target(5, 406);
    let property_7 = directory_target(7, 406);
    let source = directory_target(9, 420);
    let directory = BTreeMap::from([
        (1, &association_1),
        (3, &association_3),
        (5, &property_5),
        (7, &property_7),
        (9, &source),
    ]);
    let values = [420, 1, 0, 0, 0, 1, 0, 0, 0, 1, 0, 2, 0, 2, 1, 3, 2, 5, 7];
    let record = integer_parameter_record(9, &values);
    assert!(structural_pointer_group_candidates(&record)
        .iter()
        .any(|candidate| candidate.token_start == 13));

    let analysis = analyze_trailing_pointer_groups(&record, &directory);
    assert_eq!(analysis.candidate_count, 1);
    assert_eq!(analysis.valid_candidate_count, 1);
    let groups = analysis.groups.expect("Type 420 table boundary");
    assert_eq!(groups.token_start, 14);
    assert_eq!(groups.associations, vec![3]);
    assert_eq!(groups.properties, vec![5, 7]);
}

#[test]
fn type420_complete_wrong_fields_keep_boundary_and_malformed_spans_do_not_recover() {
    let association = directory_target(1, 212);
    let property_5 = directory_target(5, 406);
    let property_7 = directory_target(7, 406);
    let source = directory_target(9, 420);
    let directory = BTreeMap::from([
        (1, &association),
        (5, &property_5),
        (7, &property_7),
        (9, &source),
    ]);
    let wrong_fields = token_parameter_record(
        9,
        vec![
            420.into(),
            1.into(),
            0.into(),
            0.into(),
            0.into(),
            TokenValue::String(b"BAD".to_vec()),
            0.into(),
            0.into(),
            0.into(),
            TokenValue::String(b"R".to_vec()),
            0.into(),
            0.into(),
            1.into(),
            1.into(),
            2.into(),
            5.into(),
            7.into(),
        ],
    );
    let analysis = analyze_trailing_pointer_groups(&wrong_fields, &directory);
    assert_eq!(analysis.candidate_count, 1);
    assert_eq!(analysis.valid_candidate_count, 1);
    assert_eq!(
        analysis
            .groups
            .expect("Type 420 complete primary boundary")
            .token_start,
        12
    );

    let malformed = [
        integer_parameter_record(9, &[420, 1, 0, 0, 0, 1, 0, 0, 0, 1, 0, -1, 1, 1, 2, 5, 7]),
        token_parameter_record(
            9,
            vec![
                420.into(),
                1.into(),
                0.into(),
                0.into(),
                0.into(),
                1.into(),
                0.into(),
                0.into(),
                0.into(),
                1.into(),
                0.into(),
                TokenValue::String(b"BAD".to_vec()),
                1.into(),
                1.into(),
                2.into(),
                5.into(),
                7.into(),
            ],
        ),
        token_parameter_record(
            9,
            vec![
                420.into(),
                1.into(),
                0.into(),
                0.into(),
                0.into(),
                1.into(),
                TokenValue::Omitted,
                TokenValue::Omitted,
                TokenValue::Omitted,
                TokenValue::String(b"R".to_vec()),
                0.into(),
            ],
        ),
        integer_parameter_record(9, &[420, 1, 0, 0, 0, 1, 0, 0, 0, 1, 0, 1]),
        integer_parameter_record(9, &[420, 1, 0, 0, 0, 1, 0, 0, 0, 1, 0, 0, 1, 1, 2, 5]),
    ];
    for record in malformed {
        let analysis = analyze_trailing_pointer_groups(&record, &directory);
        assert_eq!(analysis.candidate_count, 0);
        assert_eq!(analysis.valid_candidate_count, 0);
        assert!(analysis.groups.is_none());
    }
}

#[test]
fn type430_entity_table_boundary_follows_solid_pointer_for_both_forms() {
    let association = directory_target(1, 212);
    let property = directory_target(7, 406);

    for (form, target_type) in [(0_i64, 158_i64), (1, 186)] {
        let target = directory_target(5, target_type);
        let mut source = directory_target(9, 430);
        source.form = form;
        let directory = BTreeMap::from([
            (1, &association),
            (5, &target),
            (7, &property),
            (9, &source),
        ]);
        let analysis = analyze_trailing_pointer_groups(
            &integer_parameter_record(9, &[430, 5, 1, 1, 1, 7]),
            &directory,
        );
        assert_eq!(analysis.candidate_count, 1, "Form {form}");
        assert_eq!(analysis.valid_candidate_count, 1, "Form {form}");
        let groups = analysis.groups.expect("Type 430 table boundary");
        assert_eq!(groups.token_start, 2, "Form {form}");
        assert_eq!(groups.associations, vec![1], "Form {form}");
        assert_eq!(groups.properties, vec![7], "Form {form}");
    }
}

#[test]
fn type430_entity_table_boundary_suppresses_generic_recovery_for_malformed_span() {
    let association_1 = directory_target(1, 212);
    let association_3 = directory_target(3, 212);
    let target = directory_target(5, 158);
    let property = directory_target(7, 406);
    let source = directory_target(9, 430);
    let directory = BTreeMap::from([
        (1, &association_1),
        (3, &association_3),
        (5, &target),
        (7, &property),
        (9, &source),
    ]);
    let record = integer_parameter_record(9, &[430, 5, 3, 1, 3, 1, 1, 1, 7]);
    let generic = structural_pointer_group_candidates(&record);
    let generic_candidate = generic
        .iter()
        .find(|candidate| candidate.token_start == 1)
        .copied()
        .expect("generic recovery candidate");
    assert!(
        groups_for_candidate(&record, &directory, generic_candidate)
            .expect("generic candidate groups")
            .fully_valid
    );

    let analysis = analyze_trailing_pointer_groups(&record, &directory);
    assert_eq!(analysis.candidate_count, 0);
    assert_eq!(analysis.valid_candidate_count, 0);
    assert!(analysis.groups.is_none());
}

#[test]
fn type430_complete_wrong_fields_keep_boundary_and_malformed_spans_do_not_recover() {
    let association = directory_target(1, 212);
    let property = directory_target(7, 406);
    let target = directory_target(5, 158);
    let source = directory_target(9, 430);
    let directory = BTreeMap::from([
        (1, &association),
        (5, &target),
        (7, &property),
        (9, &source),
    ]);
    let wrong_fields = token_parameter_record(
        9,
        vec![
            430.into(),
            TokenValue::String(b"BAD".to_vec()),
            1.into(),
            1.into(),
            1.into(),
            7.into(),
        ],
    );
    let omitted_pointer = token_parameter_record(
        9,
        vec![
            430.into(),
            TokenValue::Omitted,
            1.into(),
            1.into(),
            1.into(),
            7.into(),
        ],
    );
    for record in [wrong_fields, omitted_pointer] {
        let analysis = analyze_trailing_pointer_groups(&record, &directory);
        assert_eq!(analysis.candidate_count, 1);
        assert_eq!(analysis.valid_candidate_count, 1);
        assert_eq!(
            analysis
                .groups
                .expect("Type 430 complete boundary")
                .token_start,
            2
        );
    }

    for record in [
        integer_parameter_record(9, &[430]),
        integer_parameter_record(9, &[430, 5, 1, 1]),
    ] {
        let analysis = analyze_trailing_pointer_groups(&record, &directory);
        assert_eq!(analysis.candidate_count, 0);
        assert_eq!(analysis.valid_candidate_count, 0);
        assert!(analysis.groups.is_none());
    }
}
