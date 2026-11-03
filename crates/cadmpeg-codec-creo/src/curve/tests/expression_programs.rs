// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeMap;

const ONE_COMMENT: &[u8] = b"\xe0\x00entity(crv_fr_eqn)\0\xe3\xe0\x01id\0\x07\
    \xe0\x0aexpression\0\xf8\x01/*x*/\0";
const WITH_LOCAL_SYSTEM: &[u8] = b"\xe0\x00entity(crv_fr_eqn)\0\xe3\xe0\x01id\0\x07\
    \xe0\x02local_sys\0\xf9\x04\x03\xe4\x0f\x0f\x0f\x0f\x0f\x18\xe5\x0f\x0f\x0f\
    \xe0\x0aexpression\0\xf8\x01/*x*/\0";

fn expression_lines(source: &[&str]) -> Vec<super::super::CurveExpressionLine> {
    source
        .iter()
        .enumerate()
        .map(|(offset, text)| super::super::CurveExpressionLine {
            text: (*text).to_owned(),
            offset,
        })
        .collect()
}

#[test]
fn expression_assignment_parses_a_line_with_dependencies() {
    let line = expression_lines(&["a=b+c"]);
    assert!(super::super::expression_assignment(&line[0]).is_some());
}

#[test]
fn solve_program_collects_one_block() {
    let lines = expression_lines(&["SOLVE", "x=1", "FOR x"]);
    assert_eq!(
        super::super::curve_expression_solve_program(&lines)
            .blocks
            .len(),
        1
    );
}

#[test]
fn solve_program_indexes_executable_solve_lines() {
    let lines = expression_lines(&["SOLVE", "x=1", "y=2", "FOR x"]);
    let program = super::super::curve_expression_solve_program(&lines);
    assert_eq!(
        program
            .executable_line_indices
            .into_iter()
            .collect::<Vec<_>>(),
        [2]
    );
}

#[test]
fn expression_program_evaluates_one_assignment() {
    let lines = expression_lines(&["a=1"]);
    let evaluation = crate::decode::with_service_context(|ctx| {
        super::super::evaluate_expression_program_details(
            ctx,
            &lines,
            None,
            &super::super::ExternalRelationSymbols::default(),
        )
    })
    .expect("evaluate expression program details");
    assert_eq!(evaluation.assignments.len(), 1);
}

#[test]
fn solve_unknowns_parse_names_and_reject_case_insensitive_duplicates() {
    let service = super::super::curve_expression_solve_unknowns("x, Y")
        .expect("compute the curve expression solve unknowns");
    assert_eq!(
        service
            .iter()
            .map(|unknown| unknown.name.as_str())
            .collect::<Vec<_>>(),
        ["x", "Y"]
    );
    assert!(super::super::curve_expression_solve_unknowns("x, X").is_none());
}

#[test]
fn curve_expression_record_with_comment_parses() {
    assert_eq!(super::super::expression_records(ONE_COMMENT).len(), 1);
}

#[test]
fn curve_expression_record_with_local_system_parses() {
    assert_eq!(super::super::expression_records(WITH_LOCAL_SYSTEM).len(), 1);
}

#[test]
fn rank_two_curve_frame_is_atomic_and_stops_before_the_next_named_field() {
    let body = [0x18, 0xe4, 0x0f, 0xe4, 0x18, 0xe5, 0x0f, 0x18, 0xe6];
    let expected = [0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 0.0];
    for label in ["entity(crv_fr_eqn)", "backup_ents(crv_fr_eqn)"] {
        let mut payload = b"\xe0\x00".to_vec();
        payload.extend(label.as_bytes());
        payload.extend(b"\0\xe3\xe0\x01id\0\x07\xe0\x02local_sys\0\xf9\x04\x03");
        payload.extend(body);
        payload.extend(b"\xe0\x01csys_type\0\x02\xe0\x0aexpression\0\xf8\x01/*x*/\0");
        let records = super::super::expression_records(&payload);
        assert_eq!(records.len(), 1);
        let frame = records[0].local_system.as_ref().expect("owned frame");
        assert_eq!((frame.dimensions, frame.count), (4, 3));
        assert_eq!(frame.body, body);
        assert_eq!(
            frame.explicit_slots.as_ref().map(|slots| slots.get()),
            Some(expected)
        );
    }
}

#[test]
fn incomplete_or_wrong_extent_curve_frames_preserve_the_owned_body() {
    let body = [0x18, 0xe4, 0x0f, 0xe4, 0x18, 0xe5, 0x0f, 0x18, 0xe6];
    let mut extended = body.to_vec();
    extended.push(0x18);
    for (dimensions, count, candidate) in [
        (4, 3, body[..8].to_vec()),
        (4, 3, extended),
        (3, 4, body.to_vec()),
    ] {
        let mut payload =
            b"\xe0\x00entity(crv_fr_eqn)\0\xe3\xe0\x01id\0\x07\xe0\x02local_sys\0\xf9".to_vec();
        payload.extend([dimensions, count]);
        payload.extend(&candidate);
        payload.extend(b"\xe0\x01csys_type\0\x02\xe0\x0aexpression\0\xf8\x01/*x*/\0");
        let records = super::super::expression_records(&payload);
        let frame = records[0].local_system.as_ref().expect("preserved frame");
        assert_eq!(
            (frame.dimensions, frame.count),
            (u32::from(dimensions), u32::from(count))
        );
        assert_eq!(frame.body, candidate);
        assert!(frame.explicit_slots.is_none());
    }
}

#[test]
fn curve_parameter_scalar_cache_alone_yields_no_records() {
    let payload = [0x46, 0x08, 0, 0, 0, 0, 0, 0];
    assert!(crate::decode::with_service_context(|ctx| {
        super::super::parameter_records_with_face_ids(ctx, &payload, None)
    })
    .expect("compute the parameter records with face IDs")
    .is_empty());
}

#[test]
fn depdb_curve_scalar_cache_alone_yields_no_rows() {
    let payload = b"crv_array\0\xf2\xf8\x02crv_id\0\x06type\0\x08feat_id\0\x04topol_ref_data\0\x46\x08\0\0\0\0\0\0";
    assert!(crate::decode::with_service_context(|ctx| {
        super::super::depdb_cross_section_rows(ctx, payload)
    })
    .expect("compute the DEPDB cross section rows")
    .is_empty());
}

#[test]
fn curve_parameter_records_parse_one_row() {
    let payload = b"topol_ref_data\0\x07\x08\x04\x01\xf6\xff\x0a\x0b\x07\x07\0\0\xe3\xe1\xe3";
    let records = crate::decode::with_service_context(|ctx| {
        super::super::parameter_records_with_face_ids(ctx, payload, None)
    })
    .expect("row is admitted");
    assert_eq!(records.len(), 1);
}

#[test]
fn expression_conditional_if_endif_is_valid() {
    let lines = [
        super::super::CurveExpressionLine {
            text: "if 1".to_string(),
            offset: 0,
        },
        super::super::CurveExpressionLine {
            text: "endif".to_string(),
            offset: 5,
        },
    ];
    assert!(super::super::expression_program_control_is_valid(&lines));
}

#[test]
fn nested_conditional_frame_ends_with_its_parent_activation() {
    let frame = || super::super::ConditionalFrame {
        parent: super::super::CurveExpressionActivation::Active,
        condition: Some(true),
    };
    let mut stack = super::super::ConditionalStack::default();
    stack.push(frame());
    stack.push(frame());
    assert_eq!(stack.end(), super::super::CurveExpressionActivation::Active);
}

#[test]
fn depdb_segment_four_field_tail_is_incomplete() {
    let segment = [7, 8, 4, 1, 0xf6, 0, 9, 10, 0];
    assert!(crate::decode::with_service_context(|ctx| {
        super::super::parse_depdb_curve_segment(ctx, &segment, 0)
    })
    .expect("candidate scan succeeds")
    .is_none());
}

#[test]
fn depdb_segment_unique_candidates_parse_one_row() {
    let segment = [7, 8, 4, 1, 0xf6, 0, 8, 0, 9, 10, 0];
    let row = crate::decode::with_service_context(|ctx| {
        super::super::parse_depdb_curve_segment(ctx, &segment, 0)
    })
    .expect("candidate scan succeeds")
    .expect("one prefix and one suffix");
    assert_eq!(row.id, 7);
    assert_eq!(row.suffix.next_curve, 9);
}

#[test]
fn depdb_segment_distinct_prefixes_remain_ambiguous() {
    let segment = [7, 8, 4, 1, 0xf6, 8, 8, 4, 1, 0xf6, 0, 9, 10, 0];
    assert!(crate::decode::with_service_context(|ctx| {
        super::super::parse_depdb_curve_segment(ctx, &segment, 0)
    })
    .expect("ambiguous segment is parsed")
    .is_none());
}

#[test]
fn depdb_six_field_segment_distinct_prefixes_remain_ambiguous() {
    let segment = [7, 8, 4, 1, 0xf6, 8, 8, 4, 1, 0xf6, 0, 8, 0, 9, 10, 0];
    assert!(crate::decode::with_service_context(|ctx| {
        super::super::parse_depdb_curve_segment(ctx, &segment, 0)
    })
    .expect("ambiguous segment is parsed")
    .is_none());
}

#[test]
fn curve_prototypes_parse_one_row() {
    let payload = b"crv_array\0crv_id\0\x07type\0\x08feat_id\0\x04";
    assert_eq!(super::super::prototypes(payload).len(), 1);
}

#[test]
fn solve_synchronization_copies_assignment_and_solve_values() {
    use super::super::{
        CurveExpressionActivation, CurveExpressionAssignment, CurveExpressionSolveBlock,
        CurveExpressionTarget, CurveExpressionValue, SolveUnknown,
    };
    let assignment = CurveExpressionAssignment {
        target: CurveExpressionTarget::Parameter {
            name: "a".into(),
            declared_unit: None,
        },
        expression: "\"ab\"".into(),
        dependencies: Vec::new(),
        value: Some(CurveExpressionValue::String("ab".into())),
        activation: CurveExpressionActivation::Active,
        offset: 1,
    };
    for (assignment_value, solution) in [
        (Some(CurveExpressionValue::String("ab".into())), None),
        (None, Some(CurveExpressionValue::String("ab".into()))),
    ] {
        let mut evaluated = assignment.clone();
        evaluated.value = assignment_value;
        let solutions = solution.into_iter().map(|value| (0, vec![value])).collect();
        let mut blocks = vec![CurveExpressionSolveBlock {
            equations: Vec::new(),
            assignments: vec![assignment.clone()],
            unknowns: vec![SolveUnknown {
                name: "x".into(),
                solution: None,
            }],
            offset: 0,
            for_offset: 2,
            work_caps: std::collections::BTreeSet::new(),
        }];
        super::super::synchronize_solve_blocks(
            &mut blocks,
            &[evaluated.clone()],
            &solutions,
            &BTreeMap::new(),
        );
        assert_eq!(blocks[0].assignments[0], evaluated);
        assert_eq!(
            blocks[0].unknowns[0].solution.as_ref(),
            solutions.get(&0).and_then(|values| values.first())
        );
    }
}

#[test]
fn expression_helix_without_helix_outputs_is_none() {
    let record = super::super::expression_records(
        b"\xe0\x00entity(crv_fr_eqn)\0\xe3\xe0\x01id\0\x07\xe0\x0aexpression\0\xf8\x01a=1\0",
    )
    .pop()
    .expect("record");
    assert!(
        crate::decode::with_service_context(|ctx| super::super::expression_helix(ctx, &record))
            .expect("service")
            .is_none()
    );
}

#[test]
fn affine_equation_merge_subtracts_coefficients() {
    use crate::curve::{RelationDimension, SimultaneousAffineValue};
    let left = SimultaneousAffineValue::constant(2.0, RelationDimension::default());
    let right = SimultaneousAffineValue {
        dimension: RelationDimension::default(),
        constant: 1.0,
        coefficients: BTreeMap::from([("y".to_owned(), 1.0)]),
    };
    let difference = left
        .combine(right, true)
        .expect("subtract the affine expressions");
    assert_eq!(difference.constant, 1.0);
    assert_eq!(
        difference.coefficients,
        BTreeMap::from([("y".to_owned(), -1.0)])
    );
}
