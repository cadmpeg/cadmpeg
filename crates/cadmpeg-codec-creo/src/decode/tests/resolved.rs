// SPDX-License-Identifier: Apache-2.0
//! Synthetic byte-literal tests for the container framing and honest decode.
//!
//! No external CAD file is used; every fixture is a hand-built PSB byte image
//! exercising the `#UGC:2` framing, the `#\n#<name>\n` section-boundary rule, the
//! persistence-layout signals, and the `srf_array`/`crv_array` count headers.
#![allow(clippy::unwrap_used)]

use std::collections::{BTreeMap, BTreeSet};
use std::io::Cursor;

use cadmpeg_ir::codec::{Codec, DecodeOptions};

use cadmpeg_ir::sketches::SketchConstraintDefinition;

use crate::loss::CreoLossCode;
use crate::test_support::*;
use crate::CreoCodec;

#[test]
fn decode_retains_repeated_sketch_snapshots_with_offset_identities() {
    let mut definition =
        b"feat_defs_40\0segtab_ptr\0\xf8\x02\xf7\x01\xfb\xe2schema\xf2\xf7\x01\xe2".to_vec();
    definition.extend_from_slice(&[2, 0, 0, 0, 7, 8, 0xf6, 0, 0, 0xf6, 0xf6, 42, 0xe2, 0xe3]);
    definition.extend_from_slice(&[25, 0, 0, 0, 8, 9, 0xf6, 0, 0, 0xf6, 0xf6, 42, 0xe2, 0xe3]);
    definition.extend_from_slice(
        b"dimtab_ptr\0\xf8\x01\xf7\x58\xfb\xe2\
          \xe0\x01type\0\x02\xe0\x01value\0\xe4\
          \xe0\x01direct\0\x00\xe0\x01aux_value\0\x0f\
          \xe0\x01ext_id\0\x2a\xe0\x00relat_ptr\0",
    );
    let mut payload = definition.clone();
    payload.extend_from_slice(&definition);
    let data = build_prt("c", &[("FeatDefs", payload)]);

    let result = CreoCodec
        .decode(&mut Cursor::new(data), &DecodeOptions::default())
        .expect("decode");
    assert_eq!(result.ir().model.sketches.len(), 2);
    assert_eq!(result.ir().model.features.len(), 2);
    assert!(result
        .ir()
        .model
        .sketches
        .iter()
        .all(|sketch| sketch.id.as_str().starts_with("creo:model:sketch#offset:")));
    for sketch in &result.ir().model.sketches {
        let expected_native_ref =
            sketch
                .id
                .0
                .replacen("creo:model:sketch#", "creo:featdefs:sketch#", 1);
        let identity_scope = sketch
            .id
            .0
            .strip_prefix("creo:model:sketch#")
            .expect("Creo sketch identity");
        assert_eq!(
            sketch.native_ref.as_deref(),
            Some(expected_native_ref.as_str())
        );
        assert_eq!(
            result
                .ir()
                .model
                .sketch_entities
                .iter()
                .filter(|entity| entity.sketch == sketch.id)
                .count(),
            2
        );
        assert!(result
            .ir()
            .model
            .sketch_entities
            .iter()
            .filter(|entity| entity.sketch == sketch.id)
            .all(|entity| entity.id().0.contains(&format!("#{identity_scope}:"))));
        let parameters = result
            .ir()
            .model
            .parameters
            .iter()
            .filter(|parameter| parameter.native_ref.as_deref() == sketch.native_ref.as_deref())
            .collect::<Vec<_>>();
        assert_eq!(parameters.len(), 1);
        assert_eq!(
            parameters[0].owner,
            Some(cadmpeg_ir::features::FeatureId(format!(
                "creo:model:sketch_feature#{identity_scope}"
            )))
        );
        assert!(parameters[0]
            .id
            .as_str()
            .contains(&format!("#{identity_scope}:")));
        let constraints = result
            .ir()
            .model
            .sketch_constraints
            .iter()
            .filter(|constraint| constraint.sketch == sketch.id)
            .collect::<Vec<_>>();
        assert_eq!(constraints.len(), 2);
        let reference_verhor = constraints
            .iter()
            .find(|constraint| {
                matches!(
                    &constraint.definition,
                    SketchConstraintDefinition::Native { .. }
                )
            })
            .expect("reference-line verhor");
        assert!(reference_verhor.id.as_str().starts_with(&format!(
            "creo:featdefs:sketch_constraint#{identity_scope}:verhor:reference_line:offset:"
        )));
        let SketchConstraintDefinition::Native {
            native_properties,
            operands,
            ..
        } = &reference_verhor.definition
        else {
            panic!("reference-line verhor must remain native");
        };
        assert_eq!(native_properties["verhor"], "0");
        assert_eq!(operands[0].object_index, 42);
        assert_eq!(
            operands[0].native_ref.as_deref(),
            sketch.native_ref.as_deref()
        );
    }
    assert_eq!(
        result
            .ir()
            .model
            .sketch_entities
            .iter()
            .map(|entity| entity.id())
            .collect::<BTreeSet<_>>()
            .len(),
        4
    );
    let coverage = result.report();
    assert_eq!(
        coverage.coverage_count(crate::coverage::DECODED_FEATURE_SEGMENT_ROW_COUNT),
        4
    );
    assert_eq!(
        coverage.coverage_count(crate::coverage::RESOLVED_FEATURE_SEGMENT_GEOMETRY_COUNT),
        0
    );
    assert_eq!(
        coverage.coverage_count(crate::coverage::UNRESOLVED_FEATURE_SEGMENT_GEOMETRY_COUNT),
        4
    );
    assert_eq!(
        coverage.coverage_count(crate::coverage::MISSING_FEATURE_SEGMENT_ROW_COUNT),
        0
    );
    assert!(result.report().losses.iter().any(|loss| {
        loss.code == CreoLossCode::SectionSegmentGeometryUnresolved.kind()
            && loss.code.category() == cadmpeg_ir::LossCategory::Geometry
            && loss.severity == cadmpeg_ir::Severity::Warning
            && loss.message.contains(
                "4 decoded section segment(s) retain source-native geometry because their exact \
                 neutral construction remains unresolved",
            )
    }));
    let validation = cadmpeg_ir::validate_neutral(result.ir(), result.report().losses.clone());
    assert!(validation.is_ok(), "{validation:#?}");
}

#[test]
fn decode_reports_missing_declared_section_segment_rows() {
    let mut payload =
        b"feat_defs_40\0segtab_ptr\0\xf8\x02\xf7\x01\xfb\xe2schema\xf2\xf7\x01\xe2".to_vec();
    payload.extend_from_slice(&[2, 0, 0, 0, 7, 8, 0xf6, 0, 0, 0xf6, 0xf6, 42, 0xe2, 0xe3]);
    payload.extend_from_slice(b"dimtab_ptr\0");
    let data = build_prt("c", &[("FeatDefs", payload)]);

    let result = CreoCodec
        .decode(&mut Cursor::new(data), &DecodeOptions::default())
        .expect("decode incomplete segment table");
    let coverage = result.report();

    assert_eq!(
        coverage.coverage_count(crate::coverage::DECODED_FEATURE_SEGMENT_ROW_COUNT),
        1
    );
    assert_eq!(
        coverage.coverage_count(crate::coverage::DECODED_FEATURE_LINE_SEGMENT_COUNT),
        1
    );
    assert_eq!(
        coverage.coverage_count(crate::coverage::RESOLVED_FEATURE_LINE_SEGMENT_GEOMETRY_COUNT),
        0
    );
    assert_eq!(
        coverage.coverage_count(crate::coverage::UNRESOLVED_FEATURE_LINE_SEGMENT_GEOMETRY_COUNT),
        1
    );
    assert_eq!(
        coverage.coverage_count(crate::coverage::MISSING_FEATURE_SEGMENT_ROW_COUNT),
        1
    );
    assert!(result.report().losses.iter().any(|loss| {
        loss.code == CreoLossCode::SectionSegmentMissing.kind()
            && loss.code.category() == cadmpeg_ir::LossCategory::DesignIntent
            && loss.severity == cadmpeg_ir::Severity::Warning
            && loss.message.contains(
                "1 declared section segment row(s) did not decode and remain unavailable to the \
                 defining sketch",
            )
    }));
}

#[test]
fn decode_counts_resolved_section_segment_geometry() {
    let mut payload =
        b"feat_defs_40\0var_arr\0\xf8\x04\xf7\x01\xfb\xe2schema\xf1\xf7\x01\xe2".to_vec();
    payload.extend_from_slice(&[1, 7, 0xe4, 0x0f, 1, 0, 3, 0xe2]);
    payload.extend_from_slice(&[2, 7, 0x46, 0x08, 0, 0, 0, 0, 0, 0, 0x0f, 1, 0, 4, 0xe2]);
    payload.extend_from_slice(&[1, 8, 0x46, 0x08, 0, 0, 0, 0, 0, 0, 0x0f, 1, 0, 5, 0xe2]);
    payload.extend_from_slice(&[2, 8, 0xe4, 0x0f, 1, 0, 6, 0xe2]);
    payload.extend_from_slice(b"segtab_ptr\0\xf8\x01\xf7\x01\xfb\xe2schema\xf2\xf7\x01\xe2");
    payload.extend_from_slice(&[2, 0, 0, 0, 7, 8, 0xf6, 0, 0, 0xf6, 0xf6, 42, 0xe2, 0xe3]);
    payload.extend_from_slice(b"dimtab_ptr\0");
    let data = build_prt("c", &[("FeatDefs", payload)]);

    let result = CreoCodec
        .decode(&mut Cursor::new(data), &DecodeOptions::default())
        .expect("decode resolved segment");
    let coverage = result.report();

    assert_eq!(
        coverage.coverage_count(crate::coverage::DECODED_FEATURE_SEGMENT_ROW_COUNT),
        1
    );
    assert_eq!(
        coverage.coverage_count(crate::coverage::RESOLVED_FEATURE_SEGMENT_GEOMETRY_COUNT),
        1
    );
    assert_eq!(
        coverage.coverage_count(crate::coverage::UNRESOLVED_FEATURE_SEGMENT_GEOMETRY_COUNT),
        0
    );
    assert_eq!(
        coverage.coverage_count(crate::coverage::DECODED_FEATURE_LINE_SEGMENT_COUNT),
        1
    );
    assert_eq!(
        coverage.coverage_count(crate::coverage::RESOLVED_FEATURE_LINE_SEGMENT_GEOMETRY_COUNT),
        1
    );
    assert_eq!(
        coverage.coverage_count(crate::coverage::UNRESOLVED_FEATURE_LINE_SEGMENT_GEOMETRY_COUNT),
        0
    );
    assert_eq!(
        coverage.coverage_count(crate::coverage::MISSING_FEATURE_SEGMENT_ROW_COUNT),
        0
    );
    assert_eq!(
        coverage.coverage_count(crate::coverage::DECODED_FEATURE_SOLVER_VARIABLE_COUNT),
        4
    );
    assert_eq!(
        coverage.coverage_count(crate::coverage::MISSING_FEATURE_SOLVER_VARIABLE_COUNT),
        0
    );
    assert!(!result.report().losses.iter().any(|loss| {
        loss.message
            .contains("decoded section segment(s) retain source-native geometry")
    }));
}

#[test]
fn decode_reports_missing_declared_solver_variable_rows() {
    let payload = b"feat_defs_40\0var_arr\0\xf8\x02\xf7\x01\xfb\xe2\
        \xe0\x05type\0\x01\xe0\x08key\0\x07\xe0\x02value\0\xe4\
        \xe0\x02guess\0\x0f\xe0\x06known\0\x01\
        \xe0\x0chomogeneity\0\x02\xe0\x08uvar_id\0\x03\xf1\xf7\x01\xe2"
        .to_vec();
    let data = build_prt("c", &[("FeatDefs", payload)]);

    let result = CreoCodec
        .decode(&mut Cursor::new(data), &DecodeOptions::default())
        .expect("decode incomplete variable table");
    let coverage = result.report();

    assert_eq!(
        coverage.coverage_count(crate::coverage::DECODED_FEATURE_SOLVER_VARIABLE_COUNT),
        1
    );
    assert_eq!(
        coverage.coverage_count(crate::coverage::MISSING_FEATURE_SOLVER_VARIABLE_COUNT),
        1
    );
    assert!(result.report().losses.iter().any(|loss| {
        loss.code == CreoLossCode::SectionSolverVariableMissing.kind()
            && loss.code.category() == cadmpeg_ir::LossCategory::DesignIntent
            && loss.severity == cadmpeg_ir::Severity::Warning
            && loss.message.contains(
                "1 declared section solver variable row(s) did not decode; stored and \
                 equation-derived coordinates are withheld",
            )
    }));
}

#[test]
fn incomplete_section_tables_keep_saved_endpoint_witnesses() {
    let definition = crate::feature::FeatureDefinition {
        id: 7,
        owner_feature_id: Some(8),
        body: Vec::new(),
        parameter_frames: Vec::new(),
        outlines: Vec::new(),
        variables: Some(crate::feature::FeatureVariableTable {
            declared_count: 1,
            entity_ref: None,
            rows: Vec::new(),
            points: Vec::new(),
            offset: 0,
        }),
        segments: Some(crate::feature::FeatureSegmentTable {
            declared_count: 2,
            has_elided_prototype: false,
            entity_ref: None,
            rows: vec![crate::feature::FeatureSegment {
                kind: crate::feature::FeatureSegmentKind::Line,
                directions: [None; 3],
                point_ids: [21, 22],
                center_id: None,
                arc_orientation: None,
                vertical_horizontal: None,
                radius_ref: None,
                radius2_ref: None,
                external_id: 3,
                body: Vec::new(),
                offset: 0,
            }],
            circle_rows: Vec::new(),
            point_rows: Vec::new(),
            centered_line_rows: Vec::new(),
            reference_line_rows: Vec::new(),
            bounded_curve_rows: Vec::new(),
            conic_rows: Vec::new(),
            opaque_rows: Vec::new(),
            offset: 0,
        }),
        trim_entities: None,
        trim_vertices: None,
        order_table: Some(crate::feature::FeatureOrderTable {
            declared_count: 1,
            has_prototype: false,
            entity_ref: None,
            rows: vec![crate::feature::FeatureOrderRow {
                external_id: 3,
                internal_id: 3,
                bitmask: 0,
                offset: 0,
            }],
            offset: 0,
        }),
        section_3d: None,
        dimensions: None,
        relations: None,
        saved_section: Some(crate::feature::FeatureSavedSection {
            entities: vec![crate::feature::FeatureSavedEntity::Line(
                crate::feature::FeatureSavedLine {
                    entity_id: 3,
                    references: Vec::new(),
                    attributes: Vec::new(),
                    endpoints: [
                        [Some(2.0), Some(3.0), Some(0.0)],
                        [Some(5.0), Some(7.0), Some(0.0)],
                    ],
                    body: Vec::new(),
                    offset: 0,
                },
            )],
            offset: 0,
        }),
        offset: 0,
    };

    assert_eq!(
        crate::decode::resolved_section_points(&definition),
        BTreeMap::from([(21, [2.0, 3.0]), (22, [5.0, 7.0])])
    );
}

#[test]
fn signed_distance_with_spanning_line_rejects_conflicting_fixed_coordinate() {
    let line = |point_ids| crate::feature::FeatureSegment {
        kind: crate::feature::FeatureSegmentKind::Line,
        directions: [None; 3],
        point_ids,
        center_id: None,
        arc_orientation: None,
        vertical_horizontal: Some(0),
        radius_ref: None,
        radius2_ref: None,
        external_id: 10,
        body: Vec::new(),
        offset: 0,
    };
    let definition = crate::feature::FeatureDefinition {
        id: 40,
        owner_feature_id: None,
        body: Vec::new(),
        parameter_frames: Vec::new(),
        outlines: Vec::new(),
        variables: None,
        segments: Some(crate::feature::FeatureSegmentTable {
            declared_count: 1,
            has_elided_prototype: false,
            entity_ref: None,
            rows: vec![line([1, 2])],
            circle_rows: Vec::new(),
            point_rows: Vec::new(),
            centered_line_rows: Vec::new(),
            reference_line_rows: Vec::new(),
            bounded_curve_rows: Vec::new(),
            conic_rows: Vec::new(),
            opaque_rows: Vec::new(),
            offset: 0,
        }),
        trim_entities: None,
        trim_vertices: None,
        order_table: None,
        section_3d: None,
        dimensions: None,
        relations: None,
        saved_section: None,
        offset: 0,
    };
    let segments = definition
        .segments
        .as_ref()
        .expect("segments")
        .rows
        .iter()
        .collect::<Vec<_>>();
    let valid = BTreeMap::from([(1, [Some(2.0), Some(0.0)]), (2, [Some(2.0), Some(3.0)])]);
    assert_eq!(
        crate::decode::sketch::section_linear_distance_coordinate(
            &definition,
            &segments,
            1,
            2,
            &valid,
            &[],
            &BTreeSet::new(),
        ),
        Some(1)
    );
    let conflicting = BTreeMap::from([(1, [Some(2.0), Some(0.0)]), (2, [Some(4.0), Some(3.0)])]);
    assert_eq!(
        crate::decode::sketch::section_linear_distance_coordinate(
            &definition,
            &segments,
            1,
            2,
            &conflicting,
            &[],
            &BTreeSet::new(),
        ),
        None
    );
    assert_eq!(
        crate::decode::sketch::section_linear_distance_coordinate(
            &definition,
            &segments,
            1,
            2,
            &BTreeMap::new(),
            &[(1, [2.0, 0.0]), (2, [4.0, 3.0])],
            &BTreeSet::new(),
        ),
        None
    );
}

#[test]
fn resolved_section_points_propagate_orientation_and_explicit_signed_dimensions() {
    let definition = crate::feature::FeatureDefinition {
        id: 40,
        owner_feature_id: None,
        body: Vec::new(),
        parameter_frames: Vec::new(),
        outlines: Vec::new(),
        variables: Some(crate::feature::FeatureVariableTable {
            declared_count: 1,
            entity_ref: None,
            rows: vec![crate::feature::FeatureVariableRow {
                variable_type: 3,
                key: 6,
                value: None,
                value_body: Vec::new(),
                guess: None,
                guess_body: Vec::new(),
                guess_dimension_driven: false,
                known: None,
                homogeneity: None,
                uvar_id: None,
                dimension_driven: true,
                offset: 0,
            }],
            points: vec![
                crate::feature::FeatureSectionPoint {
                    point_id: 1,
                    u: Some(2.0),
                    v: Some(3.0),
                },
                crate::feature::FeatureSectionPoint {
                    point_id: 2,
                    u: None,
                    v: None,
                },
                crate::feature::FeatureSectionPoint {
                    point_id: 3,
                    u: Some(7.0),
                    v: Some(11.0),
                },
                crate::feature::FeatureSectionPoint {
                    point_id: 4,
                    u: Some(5.0),
                    v: Some(20.0),
                },
                crate::feature::FeatureSectionPoint {
                    point_id: 5,
                    u: None,
                    v: None,
                },
                crate::feature::FeatureSectionPoint {
                    point_id: 6,
                    u: Some(20.0),
                    v: Some(30.0),
                },
                crate::feature::FeatureSectionPoint {
                    point_id: 7,
                    u: None,
                    v: None,
                },
                crate::feature::FeatureSectionPoint {
                    point_id: 8,
                    u: None,
                    v: None,
                },
                crate::feature::FeatureSectionPoint {
                    point_id: 9,
                    u: Some(20.0),
                    v: Some(40.0),
                },
            ],
            offset: 0,
        }),
        segments: Some(crate::feature::FeatureSegmentTable {
            declared_count: 5,
            has_elided_prototype: false,
            entity_ref: None,
            rows: vec![
                crate::feature::FeatureSegment {
                    kind: crate::feature::FeatureSegmentKind::Line,
                    directions: [None; 3],
                    point_ids: [1, 2],
                    center_id: None,
                    arc_orientation: None,
                    vertical_horizontal: Some(1),
                    radius_ref: None,
                    radius2_ref: None,
                    external_id: 1,
                    body: Vec::new(),
                    offset: 0,
                },
                crate::feature::FeatureSegment {
                    kind: crate::feature::FeatureSegmentKind::Line,
                    directions: [None; 3],
                    point_ids: [6, 7],
                    center_id: None,
                    arc_orientation: None,
                    vertical_horizontal: Some(1),
                    radius_ref: None,
                    radius2_ref: None,
                    external_id: 4,
                    body: Vec::new(),
                    offset: 0,
                },
                crate::feature::FeatureSegment {
                    kind: crate::feature::FeatureSegmentKind::Line,
                    directions: [Some(1), None, None],
                    point_ids: [8, 9],
                    center_id: None,
                    arc_orientation: None,
                    vertical_horizontal: Some(1),
                    radius_ref: None,
                    radius2_ref: None,
                    external_id: 5,
                    body: Vec::new(),
                    offset: 0,
                },
                crate::feature::FeatureSegment {
                    kind: crate::feature::FeatureSegmentKind::Line,
                    directions: [None; 3],
                    point_ids: [4, 5],
                    center_id: None,
                    arc_orientation: None,
                    vertical_horizontal: Some(1),
                    radius_ref: None,
                    radius2_ref: None,
                    external_id: 3,
                    body: Vec::new(),
                    offset: 0,
                },
                crate::feature::FeatureSegment {
                    kind: crate::feature::FeatureSegmentKind::Line,
                    directions: [None; 3],
                    point_ids: [2, 3],
                    center_id: None,
                    arc_orientation: None,
                    vertical_horizontal: Some(0),
                    radius_ref: None,
                    radius2_ref: None,
                    external_id: 2,
                    body: Vec::new(),
                    offset: 0,
                },
            ],
            circle_rows: Vec::new(),
            point_rows: Vec::new(),
            centered_line_rows: Vec::new(),
            reference_line_rows: Vec::new(),
            bounded_curve_rows: Vec::new(),
            conic_rows: Vec::new(),
            opaque_rows: Vec::new(),
            offset: 0,
        }),
        trim_entities: None,
        trim_vertices: None,
        order_table: None,
        section_3d: None,
        dimensions: Some(crate::feature::FeatureDimensionTable {
            declared_count: 2,
            entity_ref: None,
            rows: vec![
                crate::feature::FeatureDimension {
                    dimension_type: 2,
                    value: Some(12.0),
                    value_body: Vec::new(),
                    unresolved_value_token: None,
                    value_unit: crate::feature::DimensionUnit::Millimeters,
                    direction_byte: 0,
                    auxiliary_value: Some(0.0),
                    auxiliary_body: Vec::new(),
                    external_id: 1,
                    references: None,
                    offset: 0,
                },
                crate::feature::FeatureDimension {
                    dimension_type: 3,
                    value: Some(4.0),
                    value_body: Vec::new(),
                    unresolved_value_token: None,
                    value_unit: crate::feature::DimensionUnit::Millimeters,
                    direction_byte: 0,
                    auxiliary_value: Some(0.0),
                    auxiliary_body: Vec::new(),
                    external_id: 2,
                    references: None,
                    offset: 0,
                },
            ],
            offset: 0,
        }),
        relations: Some(crate::feature::FeatureRelationTable {
            declared_count: 6,
            entity_ref: None,
            rows: vec![
                crate::feature::FeatureRelation {
                    relation_id: 1,
                    used: 1,
                    operands: Vec::new(),
                    operand_vectors: Some([
                        [Some(4), Some(5), None, Some(1)],
                        [Some(1), Some(1), Some(0), Some(1)],
                        [Some(15), Some(16), Some(15), Some(1)],
                    ]),
                    sign: 1,
                    dimension_id: 0,
                    relation_type: 0,
                    body: Vec::new(),
                    offset: 0,
                },
                crate::feature::FeatureRelation {
                    relation_id: 3,
                    used: 1,
                    operands: Vec::new(),
                    operand_vectors: Some([
                        [Some(6), Some(7), None, Some(1)],
                        [Some(1), Some(1), Some(0), Some(1)],
                        [Some(15), Some(16), Some(15), Some(1)],
                    ]),
                    sign: 0xf6,
                    dimension_id: 0,
                    relation_type: 0,
                    body: Vec::new(),
                    offset: 0,
                },
                crate::feature::FeatureRelation {
                    relation_id: 4,
                    used: 1,
                    operands: Vec::new(),
                    operand_vectors: Some([
                        [Some(8), Some(9), None, Some(1)],
                        [Some(1), Some(1), Some(0), Some(1)],
                        [Some(15), Some(16), Some(15), Some(1)],
                    ]),
                    sign: 0,
                    dimension_id: 0,
                    relation_type: 0,
                    body: Vec::new(),
                    offset: 0,
                },
                crate::feature::FeatureRelation {
                    relation_id: 2,
                    used: 0,
                    operands: Vec::new(),
                    operand_vectors: Some([
                        [Some(6), Some(0), Some(0), Some(0)],
                        [Some(0); 4],
                        [Some(15), Some(0), Some(0), Some(0)],
                    ]),
                    sign: 1,
                    dimension_id: 1,
                    relation_type: 14,
                    body: Vec::new(),
                    offset: 0,
                },
            ],
            skamps: Vec::new(),
            skamp_header: None,
            triples: Vec::new(),
            triples_header: None,
            offset: 0,
        }),
        saved_section: None,
        offset: 0,
    };

    assert_eq!(
        crate::decode::resolved_section_points(&definition).get(&2),
        Some(&[7.0, 3.0])
    );
    assert_eq!(
        crate::decode::resolved_section_points(&definition).get(&5),
        Some(&[17.0, 20.0])
    );
    assert_eq!(
        crate::decode::resolved_section_radii(&definition).get(&6),
        Some(&4.0)
    );
    assert_eq!(
        crate::decode::resolved_section_points(&definition).get(&7),
        Some(&[8.0, 30.0])
    );
    assert_eq!(
        crate::decode::resolved_section_coordinates(&definition).get(&8),
        Some(&[None, Some(40.0)])
    );

    let mut saved_endpoint_definition = definition.clone();
    saved_endpoint_definition
        .variables
        .as_mut()
        .expect("variables")
        .points
        .extend([
            crate::feature::FeatureSectionPoint {
                point_id: 10,
                u: None,
                v: None,
            },
            crate::feature::FeatureSectionPoint {
                point_id: 11,
                u: None,
                v: Some(3.0),
            },
        ]);
    let segments = saved_endpoint_definition
        .segments
        .as_mut()
        .expect("segments");
    segments.declared_count = 7;
    segments.rows.extend([
        crate::feature::FeatureSegment {
            kind: crate::feature::FeatureSegmentKind::Line,
            directions: [None; 3],
            point_ids: [10, 12],
            center_id: None,
            arc_orientation: None,
            vertical_horizontal: None,
            radius_ref: None,
            radius2_ref: None,
            external_id: 10,
            body: Vec::new(),
            offset: 0,
        },
        crate::feature::FeatureSegment {
            kind: crate::feature::FeatureSegmentKind::Line,
            directions: [None; 3],
            point_ids: [13, 11],
            center_id: None,
            arc_orientation: None,
            vertical_horizontal: None,
            radius_ref: None,
            radius2_ref: None,
            external_id: 11,
            body: Vec::new(),
            offset: 0,
        },
    ]);
    let dimensions = saved_endpoint_definition
        .dimensions
        .as_mut()
        .expect("dimensions");
    dimensions.declared_count = 3;
    dimensions.rows.push(crate::feature::FeatureDimension {
        dimension_type: 2,
        value: Some(15.0),
        value_body: Vec::new(),
        unresolved_value_token: None,
        value_unit: crate::feature::DimensionUnit::Millimeters,
        direction_byte: 0,
        auxiliary_value: Some(0.0),
        auxiliary_body: Vec::new(),
        external_id: 3,
        references: None,
        offset: 0,
    });
    let relations = saved_endpoint_definition
        .relations
        .as_mut()
        .expect("relations");
    relations.declared_count = 7;
    relations.rows.push(crate::feature::FeatureRelation {
        relation_id: 5,
        used: 1,
        operands: Vec::new(),
        operand_vectors: Some([
            [Some(10), Some(11), None, Some(1)],
            [Some(1), Some(1), Some(0), Some(1)],
            [Some(15), Some(16), Some(15), Some(1)],
        ]),
        sign: 1,
        dimension_id: 2,
        relation_type: 0,
        body: Vec::new(),
        offset: 0,
    });
    saved_endpoint_definition.order_table = Some(crate::feature::FeatureOrderTable {
        declared_count: 1,
        has_prototype: false,
        entity_ref: None,
        rows: vec![crate::feature::FeatureOrderRow {
            external_id: 10,
            internal_id: 10,
            bitmask: 0,
            offset: 0,
        }],
        offset: 0,
    });
    saved_endpoint_definition.saved_section = Some(crate::feature::FeatureSavedSection {
        entities: vec![crate::feature::FeatureSavedEntity::Line(
            crate::feature::FeatureSavedLine {
                entity_id: 10,
                references: Vec::new(),
                attributes: Vec::new(),
                endpoints: [
                    [Some(2.0), Some(3.0), Some(0.0)],
                    [Some(0.0), Some(0.0), Some(0.0)],
                ],
                body: Vec::new(),
                offset: 0,
            },
        )],
        offset: 0,
    });
    assert_eq!(
        crate::decode::resolved_section_points(&saved_endpoint_definition).get(&11),
        Some(&[17.0, 3.0])
    );

    let mut incomplete_variables = definition.clone();
    incomplete_variables
        .variables
        .as_mut()
        .expect("variables")
        .declared_count = 2;
    assert!(crate::decode::resolved_section_points(&incomplete_variables).is_empty());

    let mut incomplete_dimensions = definition.clone();
    incomplete_dimensions
        .dimensions
        .as_mut()
        .expect("dimensions")
        .declared_count = 3;
    assert_eq!(
        crate::decode::resolved_section_coordinates(&incomplete_dimensions).get(&5),
        Some(&[None, Some(20.0)])
    );

    let mut incomplete_segments = definition;
    incomplete_segments
        .segments
        .as_mut()
        .expect("segments")
        .declared_count = 6;
    assert!(!crate::decode::resolved_section_points(&incomplete_segments).contains_key(&2));
}
