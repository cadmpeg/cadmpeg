// SPDX-License-Identifier: Apache-2.0
#![allow(clippy::unwrap_used)]

use std::io::Cursor;

use cadmpeg_ir::codec::{Codec, DecodeOptions};

use crate::container::{self};
use crate::test_support::*;
use crate::CreoCodec;

use super::*;

use crate::feature::{
    FeatureEntityTableEntry, FeatureParameterFrame, FeatureSection3d, FeatureSectionOrientation,
    FeatureSectionPoint, FeatureSectionReferencePlane, FeatureSegment, FeatureSegmentTable,
    FeatureVariableTable,
};
use crate::surface::{PositionalCylinderFrame, SurfaceBodyBoundary, SurfaceParameterRecord};

#[test]
fn normalization_rejects_overflowed_feature_frame_vectors() {
    assert_eq!(normalize([f64::MAX, f64::MAX, 0.0]), None);
    let normalized = normalize([0.0, 3.0, 4.0]).expect("finite vector");
    assert!(normalized[0].abs() < 1e-12);
    assert!((normalized[1] - 0.6).abs() < 1e-12);
    assert!((normalized[2] - 0.8).abs() < 1e-12);
}

fn datum(id: u32, normal: [f64; 3], offset: f64) -> DatumPlane {
    DatumPlane {
        id,
        feature_id: id.saturating_sub(1),
        normal,
        offset,
        corners: [[Some(0.0); 3]; 2],
        offset_in_payload: usize::try_from(id).expect("fixture id fits usize"),
    }
}

fn blank_definition() -> FeatureDefinition {
    FeatureDefinition {
        id: 42,
        owner_feature_id: Some(42),
        body: Vec::new(),
        parameter_frames: Vec::new(),
        outlines: Vec::new(),
        variables: None,
        segments: None,
        trim_entities: None,
        trim_vertices: None,
        order_table: None,
        section_3d: None,
        dimensions: None,
        relations: None,
        saved_section: None,
        offset: 0,
    }
}

#[test]
fn unique_complete_local_system_supplies_section_plane_equation() {
    let mut definition = blank_definition();
    definition.parameter_frames = vec![FeatureParameterFrame {
        kind: FeatureParameterFrameKind::LocalSystem,
        body: Vec::new(),
        decoded_values: Some(vec![
            0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 2.0, 0.0, 0.0, 3.0, 4.0, 5.0,
        ]),
        offset: 1,
    }];

    assert_eq!(
        definition_local_plane_equation(&definition),
        Some(([1.0, 0.0, 0.0], 3.0))
    );

    definition.parameter_frames.push(FeatureParameterFrame {
        kind: FeatureParameterFrameKind::LocalSystem,
        body: Vec::new(),
        decoded_values: None,
        offset: 2,
    });
    assert_eq!(
        definition_local_plane_equation(&definition),
        Some(([1.0, 0.0, 0.0], 3.0))
    );

    definition.parameter_frames.push(FeatureParameterFrame {
        kind: FeatureParameterFrameKind::LocalSystem,
        body: Vec::new(),
        decoded_values: Some(vec![0.0; 12]),
        offset: 3,
    });
    assert_eq!(definition_local_plane_equation(&definition), None);
}

#[test]
fn unresolved_local_system_does_not_hide_a_complete_outline_plane() {
    let unresolved = PlaneLocalSystem {
        surface_id: 7,
        body: Vec::new(),
        slots: vec![None; 12],
        origin: None,
        u_axis: None,
        normal: None,
        classification: crate::surface::LocalSystemClassification::Unclassified,
        row_offset: 10,
        offset: 11,
    };
    let outline = OutlinePlane {
        surface_id: 7,
        origin: [0.0, 0.0, 3.0],
        u_axis: [1.0, 0.0, 0.0],
        normal: [0.0, 0.0, 1.0],
        offset: 12,
    };

    assert_eq!(
        plane_equation(7, &[], &[unresolved], &[outline]),
        Some(([0.0, 0.0, 1.0], 3.0))
    );
}

#[test]
fn plane_namespace_collision_withholds_equation() {
    let model = PlaneLocalSystem {
        surface_id: 7,
        body: Vec::new(),
        slots: vec![None; 12],
        origin: Some([0.0, 2.0, 0.0]),
        u_axis: Some([1.0, 0.0, 0.0]),
        normal: Some([0.0, 1.0, 0.0]),
        classification: crate::surface::LocalSystemClassification::Unclassified,
        row_offset: 10,
        offset: 11,
    };
    assert_eq!(
        plane_equation(7, &[datum(7, [0.0, 1.0, 0.0], 2.0)], &[model], &[]),
        None
    );

    let outline = OutlinePlane {
        surface_id: 7,
        origin: [0.0, 2.0, 0.0],
        u_axis: [1.0, 0.0, 0.0],
        normal: [0.0, 1.0, 0.0],
        offset: 12,
    };
    assert_eq!(
        plane_equation(7, &[datum(7, [0.0, 1.0, 0.0], 2.0)], &[], &[outline]),
        None
    );
}

#[test]
fn resolves_perpendicular_datum_frame() {
    let definition = FeatureDefinition {
        id: 42,
        owner_feature_id: Some(42),
        body: Vec::new(),
        parameter_frames: Vec::new(),
        outlines: Vec::new(),
        variables: None,
        segments: None,
        trim_entities: None,
        trim_vertices: None,
        order_table: None,
        section_3d: Some(FeatureSection3d {
            sketch_plane_entity_id: Some(2),
            sketch_plane_flip: Some(BinaryFlag::Clear),
            reference_plane_entity_ids: vec![3, 4],
            reference_plane_rows: Vec::new(),
            reference_plane_datum_geometry_id: Some(4),
            orientation: FeatureSectionOrientation::default(),
            dimension_ids: Vec::new(),
            offset: 100,
        }),
        dimensions: None,
        relations: None,
        saved_section: None,
        offset: 90,
    };
    assert_eq!(
        resolve(
            &[definition],
            &PlacementSources {
                datums: &[
                    datum(2, [1.0, 0.0, 0.0], 2.0),
                    datum(3, [1.0, 0.0, 0.0], 1.0),
                    datum(4, [0.0, 0.0, 1.0], 3.0),
                ],
                surface_rows: &[],
                model_planes: &[],
                outline_planes: &[],
                plane_envelopes: &[],
                surface_parameters: &[],
                geometry_tables: &[],
                affected_ids: &[],
            },
            &[],
        ),
        vec![FeatureSectionTransform {
            definition_id: 42,
            feature_id: Some(42),
            origin: [2.0, 0.0, 3.0],
            u_axis: [0.0, 1.0, 0.0],
            v_axis: [0.0, 0.0, 1.0],
            normal: [1.0, 0.0, 0.0],
            offset: 100,
        }]
    );
}

#[test]
fn resolves_reference_flip_from_selected_positional_row() {
    let mut definition = blank_definition();
    definition.section_3d = Some(FeatureSection3d {
        sketch_plane_entity_id: Some(2),
        sketch_plane_flip: Some(BinaryFlag::Clear),
        reference_plane_entity_ids: vec![3, 4],
        reference_plane_rows: vec![
            FeatureSectionReferencePlane {
                plane_entity_id: 3,
                reference_type: Some(5),
                external_reference_id: None,
                segment_id: Some(3),
                sub_index: None,
                reference_flip: Some(BinaryFlag::Clear),
            },
            FeatureSectionReferencePlane {
                plane_entity_id: 4,
                reference_type: Some(5),
                external_reference_id: None,
                segment_id: Some(4),
                sub_index: None,
                reference_flip: Some(BinaryFlag::Set),
            },
        ],
        reference_plane_datum_geometry_id: None,
        orientation: FeatureSectionOrientation::default(),
        dimension_ids: Vec::new(),
        offset: 100,
    });

    let transforms = resolve(
        &[definition],
        &PlacementSources {
            datums: &[
                datum(2, [1.0, 0.0, 0.0], 2.0),
                datum(3, [1.0, 0.0, 0.0], 1.0),
                datum(4, [0.0, 0.0, 1.0], 3.0),
            ],
            surface_rows: &[],
            model_planes: &[],
            outline_planes: &[],
            plane_envelopes: &[],
            surface_parameters: &[],
            geometry_tables: &[],
            affected_ids: &[],
        },
        &[],
    );

    assert_eq!(transforms.len(), 1);
    assert_eq!(transforms[0].origin, [2.0, 0.0, 3.0]);
    assert_eq!(transforms[0].u_axis, [0.0, -1.0, 0.0]);
    assert_eq!(transforms[0].v_axis, [0.0, 0.0, -1.0]);
    assert_eq!(transforms[0].normal, [1.0, 0.0, 0.0]);
}

#[test]
fn resolves_section_from_complete_local_frame_when_references_are_unresolved() {
    let mut definition = blank_definition();
    definition.parameter_frames = vec![FeatureParameterFrame {
        kind: FeatureParameterFrameKind::LocalSystem,
        body: Vec::new(),
        decoded_values: Some(vec![
            0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, -3.0, -4.0, 0.0,
        ]),
        offset: 1,
    }];
    definition.section_3d = Some(FeatureSection3d {
        sketch_plane_entity_id: Some(348),
        sketch_plane_flip: Some(BinaryFlag::Clear),
        reference_plane_entity_ids: vec![2, 274],
        reference_plane_rows: Vec::new(),
        reference_plane_datum_geometry_id: None,
        orientation: FeatureSectionOrientation::default(),
        dimension_ids: Vec::new(),
        offset: 100,
    });

    assert_eq!(
        resolve(
            &[definition],
            &PlacementSources {
                datums: &[],
                surface_rows: &[],
                model_planes: &[],
                outline_planes: &[],
                plane_envelopes: &[],
                surface_parameters: &[],
                geometry_tables: &[],
                affected_ids: &[],
            },
            &[],
        ),
        vec![FeatureSectionTransform {
            definition_id: 42,
            feature_id: Some(42),
            origin: [-3.0, -4.0, 0.0],
            u_axis: [0.0, 1.0, 0.0],
            v_axis: [0.0, 0.0, 1.0],
            normal: [1.0, 0.0, 0.0],
            offset: 100,
        }]
    );
}

#[test]
fn resolves_generated_section_from_declared_cap_pair() {
    let definition = FeatureDefinition {
        id: 917,
        owner_feature_id: Some(40),
        body: Vec::new(),
        parameter_frames: Vec::new(),
        outlines: Vec::new(),
        variables: None,
        segments: None,
        trim_entities: None,
        trim_vertices: None,
        order_table: None,
        section_3d: Some(FeatureSection3d {
            sketch_plane_entity_id: Some(42),
            sketch_plane_flip: None,
            reference_plane_entity_ids: vec![191],
            reference_plane_rows: Vec::new(),
            reference_plane_datum_geometry_id: Some(2),
            orientation: FeatureSectionOrientation::default(),
            dimension_ids: Vec::new(),
            offset: 100,
        }),
        dimensions: None,
        relations: None,
        saved_section: None,
        offset: 90,
    };
    let rows = [43, 92].map(|id| SurfaceRow {
        id,
        type_byte: 0x22,
        kind: SurfaceKind::Plane,
        feature_id: 40,
        reversed: false,
        boundary_type: 0,
        next_surface: 0,
        offset: usize::try_from(id).expect("fixture id fits usize"),
    });
    let outlines = [
        OutlinePlane {
            surface_id: 43,
            origin: [0.0, 0.0, 0.0],
            normal: [0.0, 1.0, 0.0],
            u_axis: [1.0, 0.0, 0.0],
            offset: 43,
        },
        OutlinePlane {
            surface_id: 92,
            origin: [0.0, 38.0, 0.0],
            normal: [0.0, 1.0, 0.0],
            u_axis: [1.0, 0.0, 0.0],
            offset: 92,
        },
    ];
    let geometry_tables = [FeatureGeometryTable {
        feature_id: 40,
        kind: FeatureGeometryTableKind::DatumIds,
        count: 1,
        entity_class: 1,
        entry_ids: Some(vec![42]),
        offset: 80,
    }];
    let entries = [(43, 204), (92, 203)].map(|(entity_id, class_id)| {
        crate::feature::FeatureEntityTableEntry {
            entity_id,
            class_id,
            source_entity_id: None,
            related_entity_id: None,
            related_entity_state: None,
            prefixed: false,
            offset: usize::try_from(entity_id).expect("fixture id fits usize"),
            end_offset: usize::try_from(entity_id + 1).expect("fixture id fits usize"),
        }
    });
    let entity_tables = [
        FeatureEntityTable {
            feature_id: Some(40),
            table_class_id: 80,
            entry_ids: vec![700],
            entries: vec![crate::feature::FeatureEntityTableEntry {
                entity_id: 700,
                class_id: 7,
                source_entity_id: None,
                related_entity_id: None,
                related_entity_state: None,
                prefixed: false,
                offset: 60,
                end_offset: 61,
            }],
            surface_ids: Vec::new(),
            non_surface_entity_ids: vec![700],
            offset: 50,
        },
        FeatureEntityTable {
            feature_id: Some(40),
            table_class_id: 80,
            entry_ids: vec![43, 92],
            entries: entries.to_vec(),
            surface_ids: vec![43, 92],
            non_surface_entity_ids: Vec::new(),
            offset: 70,
        },
    ];

    assert_eq!(
        resolve(
            &[definition],
            &PlacementSources {
                datums: &[
                    datum(2, [1.0, 0.0, 0.0], 0.0),
                    datum(191, [1.0, 0.0, 0.0], 8.0),
                ],
                surface_rows: &rows,
                model_planes: &[],
                outline_planes: &outlines,
                plane_envelopes: &[],
                surface_parameters: &[],
                geometry_tables: &geometry_tables,
                affected_ids: &[],
            },
            &entity_tables,
        ),
        vec![FeatureSectionTransform {
            definition_id: 917,
            feature_id: Some(40),
            origin: [0.0, 0.0, 0.0],
            u_axis: [0.0, 0.0, 1.0],
            v_axis: [1.0, 0.0, 0.0],
            normal: [0.0, 1.0, 0.0],
            offset: 100,
        }]
    );
}

#[test]
fn resolves_oblique_reference_from_an_earlier_extruded_line() {
    let source = FeatureDefinition {
        id: 917,
        owner_feature_id: Some(40),
        body: Vec::new(),
        parameter_frames: Vec::new(),
        outlines: Vec::new(),
        variables: Some(FeatureVariableTable {
            declared_count: 0,
            entity_ref: None,
            rows: Vec::new(),
            points: vec![
                FeatureSectionPoint {
                    point_id: 8,
                    u: Some(0.0),
                    v: Some(0.0),
                },
                FeatureSectionPoint {
                    point_id: 9,
                    u: Some(1.0),
                    v: Some(0.0),
                },
            ],
            offset: 10,
        }),
        segments: Some(FeatureSegmentTable {
            declared_count: 1,
            has_elided_prototype: false,
            entity_ref: None,
            rows: vec![FeatureSegment {
                kind: FeatureSegmentKind::Line,
                directions: [None; 3],
                point_ids: [8, 9],
                center_id: None,
                arc_orientation: None,
                vertical_horizontal: None,
                radius_ref: None,
                radius2_ref: None,
                external_id: 43,
                body: Vec::new(),
                offset: 20,
            }],
            circle_rows: Vec::new(),
            point_rows: Vec::new(),
            centered_line_rows: Vec::new(),
            reference_line_rows: Vec::new(),
            bounded_curve_rows: Vec::new(),
            conic_rows: Vec::new(),
            opaque_rows: Vec::new(),
            offset: 20,
        }),
        trim_entities: None,
        trim_vertices: None,
        order_table: None,
        section_3d: Some(FeatureSection3d {
            sketch_plane_entity_id: Some(2),
            sketch_plane_flip: None,
            reference_plane_entity_ids: vec![4],
            reference_plane_rows: Vec::new(),
            reference_plane_datum_geometry_id: Some(4),
            orientation: FeatureSectionOrientation::default(),
            dimension_ids: Vec::new(),
            offset: 30,
        }),
        dimensions: None,
        relations: None,
        saved_section: None,
        offset: 5,
    };
    let dependent = FeatureDefinition {
        id: 579,
        owner_feature_id: Some(579),
        body: Vec::new(),
        parameter_frames: Vec::new(),
        outlines: Vec::new(),
        variables: None,
        segments: None,
        trim_entities: None,
        trim_vertices: None,
        order_table: None,
        section_3d: Some(FeatureSection3d {
            sketch_plane_entity_id: Some(799),
            sketch_plane_flip: None,
            reference_plane_entity_ids: vec![43],
            reference_plane_rows: Vec::new(),
            reference_plane_datum_geometry_id: None,
            orientation: FeatureSectionOrientation::default(),
            dimension_ids: Vec::new(),
            offset: 40,
        }),
        dimensions: None,
        relations: None,
        saved_section: None,
        offset: 35,
    };
    let generated_plane = SurfaceRow {
        id: 43,
        type_byte: 0x22,
        kind: SurfaceKind::Plane,
        feature_id: 40,
        reversed: false,
        boundary_type: 1,
        next_surface: 0,
        offset: 50,
    };

    let transforms = resolve(
        &[source.clone(), dependent.clone()],
        &PlacementSources {
            datums: &[
                datum(2, [1.0, 0.0, 0.0], 0.0),
                datum(4, [0.0, 0.0, 1.0], 0.0),
                datum(799, [0.0, 1.0, 0.0], 1.0),
            ],
            surface_rows: std::slice::from_ref(&generated_plane),
            model_planes: &[],
            outline_planes: &[],
            plane_envelopes: &[],
            surface_parameters: &[],
            geometry_tables: &[],
            affected_ids: &[],
        },
        &[],
    );

    assert_eq!(transforms.len(), 2);
    assert_eq!(transforms[1].definition_id, 579);
    assert_eq!(transforms[1].feature_id, Some(579));
    assert_eq!(transforms[1].origin, [0.0, 1.0, 0.0]);
    assert_eq!(transforms[1].u_axis, [1.0, 0.0, 0.0]);
    assert_eq!(transforms[1].v_axis, [0.0, 0.0, -1.0]);
    assert_eq!(transforms[1].normal, [0.0, 1.0, 0.0]);

    let duplicate_plane = SurfaceRow {
        offset: 51,
        ..generated_plane
    };
    let ambiguous = resolve(
        &[source, dependent],
        &PlacementSources {
            datums: &[
                datum(2, [1.0, 0.0, 0.0], 0.0),
                datum(4, [0.0, 0.0, 1.0], 0.0),
                datum(799, [0.0, 1.0, 0.0], 1.0),
            ],
            surface_rows: &[generated_plane, duplicate_plane],
            model_planes: &[],
            outline_planes: &[],
            plane_envelopes: &[],
            surface_parameters: &[],
            geometry_tables: &[],
            affected_ids: &[],
        },
        &[],
    );
    assert_eq!(ambiguous.len(), 1);
    assert_eq!(ambiguous[0].definition_id, 917);
}

#[test]
fn resolves_orientation_from_an_outline_plane_carrier() {
    let definition = FeatureDefinition {
        id: 42,
        owner_feature_id: Some(42),
        body: Vec::new(),
        parameter_frames: Vec::new(),
        outlines: Vec::new(),
        variables: None,
        segments: None,
        trim_entities: None,
        trim_vertices: None,
        order_table: None,
        section_3d: Some(FeatureSection3d {
            sketch_plane_entity_id: Some(2),
            sketch_plane_flip: Some(BinaryFlag::Clear),
            reference_plane_entity_ids: vec![4],
            reference_plane_rows: Vec::new(),
            reference_plane_datum_geometry_id: Some(4),
            orientation: FeatureSectionOrientation::default(),
            dimension_ids: Vec::new(),
            offset: 100,
        }),
        dimensions: None,
        relations: None,
        saved_section: None,
        offset: 90,
    };
    let reference = OutlinePlane {
        surface_id: 4,
        origin: [0.0, 0.0, 3.0],
        normal: [0.0, 0.0, 1.0],
        u_axis: [1.0, 0.0, 0.0],
        offset: 70,
    };

    let transforms = resolve(
        &[definition],
        &PlacementSources {
            datums: &[datum(2, [1.0, 0.0, 0.0], 2.0)],
            surface_rows: &[],
            model_planes: &[],
            outline_planes: &[reference],
            plane_envelopes: &[],
            surface_parameters: &[],
            geometry_tables: &[],
            affected_ids: &[],
        },
        &[],
    );
    assert_eq!(transforms.len(), 1);
    assert_eq!(transforms[0].origin, [2.0, 0.0, 3.0]);
    assert_eq!(transforms[0].u_axis, [0.0, 1.0, 0.0]);
    assert_eq!(transforms[0].v_axis, [0.0, 0.0, 1.0]);
}

#[test]
fn resolves_generated_sketch_datum_from_unique_parent_relation() {
    let definition = FeatureDefinition {
        id: 80,
        owner_feature_id: Some(40),
        body: Vec::new(),
        parameter_frames: Vec::new(),
        outlines: Vec::new(),
        variables: None,
        segments: None,
        trim_entities: None,
        trim_vertices: None,
        order_table: None,
        section_3d: Some(FeatureSection3d {
            sketch_plane_entity_id: Some(42),
            sketch_plane_flip: None,
            reference_plane_entity_ids: vec![90],
            reference_plane_rows: Vec::new(),
            reference_plane_datum_geometry_id: Some(2),
            orientation: FeatureSectionOrientation {
                section_flip: Some(BinaryFlag::Set),
                reference_type: Some(5),
                segment_id: None,
                reference_flip: Some(BinaryFlag::Clear),
            },
            dimension_ids: Vec::new(),
            offset: 100,
        }),
        dimensions: None,
        relations: None,
        saved_section: None,
        offset: 90,
    };
    let geometry_table = FeatureGeometryTable {
        feature_id: 40,
        kind: FeatureGeometryTableKind::DatumIds,
        count: 1,
        entity_class: 87,
        entry_ids: Some(vec![42]),
        offset: 20,
    };
    let parents = FeatureAffectedIds {
        feature_id: 11,
        kind: AffectedIdKind::Parents,
        ids: vec![1, 3],
        offset: 40,
    };
    let transforms = resolve(
        &[definition],
        &PlacementSources {
            datums: &[
                datum(2, [1.0, 0.0, 0.0], 0.0),
                datum(4, [0.0, 1.0, 0.0], 0.0),
            ],
            surface_rows: &[],
            model_planes: &[],
            outline_planes: &[],
            plane_envelopes: &[],
            surface_parameters: &[],
            geometry_tables: &[geometry_table],
            affected_ids: &[parents],
        },
        &[],
    );
    assert_eq!(transforms.len(), 1);
    assert_eq!(transforms[0].normal, [0.0, -1.0, 0.0]);
    assert_eq!(transforms[0].u_axis, [0.0, 0.0, -1.0]);
    assert_eq!(transforms[0].v_axis, [1.0, 0.0, 0.0]);
}

#[test]
fn resolves_generated_plane_from_contextually_unambiguous_envelope_axis() {
    let definition = FeatureDefinition {
        id: 80,
        owner_feature_id: Some(40),
        body: Vec::new(),
        parameter_frames: Vec::new(),
        outlines: Vec::new(),
        variables: None,
        segments: None,
        trim_entities: None,
        trim_vertices: None,
        order_table: None,
        section_3d: Some(FeatureSection3d {
            sketch_plane_entity_id: Some(42),
            sketch_plane_flip: None,
            reference_plane_entity_ids: vec![90],
            reference_plane_rows: Vec::new(),
            reference_plane_datum_geometry_id: Some(2),
            orientation: FeatureSectionOrientation::default(),
            dimension_ids: Vec::new(),
            offset: 100,
        }),
        dimensions: None,
        relations: None,
        saved_section: None,
        offset: 90,
    };
    let geometry_table = FeatureGeometryTable {
        feature_id: 40,
        kind: FeatureGeometryTableKind::DatumIds,
        count: 1,
        entity_class: 87,
        entry_ids: Some(vec![42]),
        offset: 20,
    };
    let parents = FeatureAffectedIds {
        feature_id: 40,
        kind: AffectedIdKind::Parents,
        ids: vec![1, 3],
        offset: 40,
    };
    let row = SurfaceRow {
        id: 7,
        type_byte: 0x22,
        kind: SurfaceKind::Plane,
        feature_id: 3,
        reversed: false,
        boundary_type: 1,
        next_surface: 0,
        offset: 50,
    };
    let envelope = PlaneEnvelopeRecord {
        surface_id: 7,
        body: Vec::new(),
        envelope: PlaneEnvelope::Standard {
            bounds_2d: [[Some(0.0); 2]; 2],
            corners_3d: [
                [Some(0.0), Some(-1.0), Some(3.0)],
                [Some(0.0), Some(1.0), Some(3.0)],
            ],
        },
        corner_coordinate_equal: [Some(true), Some(false), Some(true)],
        scalar_tokens: Vec::new(),
        row_offset: 50,
        offset: 60,
    };

    let transforms = resolve(
        &[definition],
        &PlacementSources {
            datums: &[datum(2, [1.0, 0.0, 0.0], 0.0)],
            surface_rows: &[row],
            model_planes: &[],
            outline_planes: &[],
            plane_envelopes: &[envelope],
            surface_parameters: &[],
            geometry_tables: &[geometry_table],
            affected_ids: &[parents],
        },
        &[],
    );
    assert_eq!(transforms.len(), 1);
    assert_eq!(transforms[0].origin, [0.0, 0.0, 3.0]);
    assert_eq!(transforms[0].normal, [0.0, 0.0, 1.0]);
}

#[test]
fn resolves_section_frame_from_two_generated_arc_cylinders() {
    let segment = |external_id, center_id| FeatureSegment {
        kind: FeatureSegmentKind::Arc,
        directions: [None; 3],
        point_ids: [0; 2],
        center_id: Some(center_id),
        arc_orientation: None,
        vertical_horizontal: None,
        radius_ref: None,
        radius2_ref: None,
        external_id,
        body: Vec::new(),
        offset: external_id as usize,
    };
    let definition = FeatureDefinition {
        id: 917,
        owner_feature_id: Some(40),
        body: Vec::new(),
        parameter_frames: Vec::new(),
        outlines: Vec::new(),
        variables: Some(FeatureVariableTable {
            declared_count: 0,
            entity_ref: None,
            rows: Vec::new(),
            points: vec![
                FeatureSectionPoint {
                    point_id: 1,
                    u: Some(-12.5),
                    v: Some(0.0),
                },
                FeatureSectionPoint {
                    point_id: 2,
                    u: Some(12.5),
                    v: Some(0.0),
                },
            ],
            offset: 100,
        }),
        segments: Some(FeatureSegmentTable {
            declared_count: 2,
            has_elided_prototype: false,
            entity_ref: None,
            rows: vec![segment(252, 1), segment(255, 2)],
            circle_rows: Vec::new(),
            point_rows: Vec::new(),
            centered_line_rows: Vec::new(),
            reference_line_rows: Vec::new(),
            bounded_curve_rows: Vec::new(),
            conic_rows: Vec::new(),
            opaque_rows: Vec::new(),
            offset: 110,
        }),
        trim_entities: None,
        trim_vertices: None,
        order_table: None,
        section_3d: None,
        dimensions: None,
        relations: None,
        saved_section: None,
        offset: 90,
    };
    let rows = [
        SurfaceRow {
            id: 819,
            type_byte: 0x24,
            kind: SurfaceKind::Cylinder,
            feature_id: 40,
            reversed: false,
            boundary_type: 0,
            next_surface: 0,
            offset: 200,
        },
        SurfaceRow {
            id: 822,
            type_byte: 0x24,
            kind: SurfaceKind::Cylinder,
            feature_id: 40,
            reversed: false,
            boundary_type: 0,
            next_surface: 0,
            offset: 220,
        },
    ];
    let parameters = |surface_id, origin, offset| SurfaceParameterRecord {
        surface_id,
        body: Vec::new(),
        scalar_values: Vec::new(),
        scalar_tokens: Vec::new(),
        opaque_spans: Vec::new(),
        scalar_frames: Vec::new(),
        terminal_scalar_frame: None,
        tabulated_cylinder_frame: None,
        positional_cylinder_frame: Some(PositionalCylinderFrame {
            origin,
            axis: [0.0, 1.0, 0.0],
            ref_direction: [1.0, 0.0, 0.0],
            radius: 0.75,
            length: Some(34.0),
        }),
        split_cylinder_outline_bounds: None,
        positional_cone_frame: None,
        positional_torus_frame: None,
        boundary: SurfaceBodyBoundary::CompoundClose,
        offset,
        body_offset: offset + 1,
    };
    let parameters = [
        parameters(819, [-12.5, 4.0, 0.0], 200),
        parameters(822, [12.5, 4.0, 0.0], 220),
    ];
    let entry = |entity_id, source_entity_id, offset| FeatureEntityTableEntry {
        entity_id,
        class_id: 200,
        source_entity_id: Some(source_entity_id),
        related_entity_id: None,
        related_entity_state: None,
        prefixed: false,
        offset,
        end_offset: offset + 1,
    };
    let tables = [FeatureEntityTable {
        feature_id: Some(40),
        table_class_id: 2,
        entry_ids: vec![819, 822],
        entries: vec![entry(819, 252, 300), entry(822, 255, 310)],
        surface_ids: vec![819, 822],
        non_surface_entity_ids: Vec::new(),
        offset: 290,
    }];
    let sources = PlacementSources {
        datums: &[],
        surface_rows: &rows,
        model_planes: &[],
        outline_planes: &[],
        plane_envelopes: &[],
        surface_parameters: &parameters,
        geometry_tables: &[],
        affected_ids: &[],
    };

    assert_eq!(
        generated_cylinder_section_transform(&definition, &sources, &tables),
        Some(FeatureSectionTransform {
            definition_id: 917,
            feature_id: Some(40),
            origin: [0.0, 4.0, 0.0],
            u_axis: [1.0, 0.0, 0.0],
            v_axis: [0.0, 0.0, -1.0],
            normal: [0.0, 1.0, 0.0],
            offset: 200,
        })
    );

    let mut far_divergent = parameters.clone();
    for record in &mut far_divergent {
        record
            .positional_cylinder_frame
            .as_mut()
            .expect("cylinder frame")
            .origin[0] += 1.0e12;
    }
    far_divergent[1]
        .positional_cylinder_frame
        .as_mut()
        .expect("second cylinder frame")
        .axis = [0.1, 0.99_f64.sqrt(), 0.0];
    let divergent_sources = PlacementSources {
        surface_parameters: &far_divergent,
        ..sources
    };
    assert!(
        generated_cylinder_section_transform(&definition, &divergent_sources, &tables).is_none()
    );
    let mut wrong_class = tables.clone();
    wrong_class[0].entries[0].class_id = 201;
    assert!(generated_cylinder_section_transform(&definition, &sources, &wrong_class).is_none());
    let mut non_surface = tables;
    non_surface[0].surface_ids.pop();
    assert!(generated_cylinder_section_transform(&definition, &sources, &non_surface).is_none());
}

#[test]
fn resolves_section_frame_from_complete_generated_planar_prism() {
    let line = |external_id, point_ids| FeatureSegment {
        kind: FeatureSegmentKind::Line,
        directions: [None; 3],
        point_ids,
        center_id: None,
        arc_orientation: None,
        vertical_horizontal: None,
        radius_ref: None,
        radius2_ref: None,
        external_id,
        body: Vec::new(),
        offset: external_id as usize,
    };
    let definition = FeatureDefinition {
        id: 917,
        owner_feature_id: Some(10),
        body: Vec::new(),
        parameter_frames: Vec::new(),
        outlines: Vec::new(),
        variables: Some(FeatureVariableTable {
            declared_count: 0,
            entity_ref: None,
            rows: Vec::new(),
            points: vec![
                FeatureSectionPoint {
                    point_id: 1,
                    u: Some(-20.0),
                    v: Some(-6.0),
                },
                FeatureSectionPoint {
                    point_id: 2,
                    u: Some(20.0),
                    v: Some(-6.0),
                },
                FeatureSectionPoint {
                    point_id: 3,
                    u: Some(20.0),
                    v: Some(6.0),
                },
                FeatureSectionPoint {
                    point_id: 4,
                    u: Some(-20.0),
                    v: Some(6.0),
                },
            ],
            offset: 100,
        }),
        segments: Some(FeatureSegmentTable {
            declared_count: 4,
            has_elided_prototype: false,
            entity_ref: None,
            rows: vec![
                line(4, [1, 2]),
                line(5, [2, 3]),
                line(6, [3, 4]),
                line(7, [4, 1]),
            ],
            circle_rows: Vec::new(),
            point_rows: Vec::new(),
            centered_line_rows: Vec::new(),
            reference_line_rows: Vec::new(),
            bounded_curve_rows: Vec::new(),
            conic_rows: Vec::new(),
            opaque_rows: Vec::new(),
            offset: 110,
        }),
        trim_entities: None,
        trim_vertices: None,
        order_table: None,
        section_3d: None,
        dimensions: None,
        relations: None,
        saved_section: None,
        offset: 90,
    };
    let outline = |surface_id, origin, normal| OutlinePlane {
        surface_id,
        origin,
        normal,
        u_axis: if normal[0] == 1.0 {
            [0.0, 1.0, 0.0]
        } else {
            [1.0, 0.0, 0.0]
        },
        offset: surface_id as usize,
    };
    let outlines = [
        outline(13, [0.0, 0.0, 0.0], [0.0, 1.0, 0.0]),
        outline(18, [0.0, 48.0, 0.0], [0.0, 1.0, 0.0]),
        outline(23, [0.0, 0.0, 6.0], [0.0, 0.0, 1.0]),
        outline(25, [20.0, 0.0, 0.0], [1.0, 0.0, 0.0]),
        outline(27, [0.0, 0.0, -6.0], [0.0, 0.0, 1.0]),
        outline(29, [-20.0, 0.0, 0.0], [1.0, 0.0, 0.0]),
    ];
    let entry = |entity_id, class_id, source_entity_id| FeatureEntityTableEntry {
        entity_id,
        class_id,
        source_entity_id,
        related_entity_id: None,
        related_entity_state: None,
        prefixed: false,
        offset: entity_id as usize,
        end_offset: entity_id as usize + 1,
    };
    let tables = [FeatureEntityTable {
        feature_id: Some(10),
        table_class_id: 79,
        entry_ids: vec![13, 18, 23, 25, 27, 29],
        entries: vec![
            entry(13, 204, None),
            entry(18, 203, None),
            entry(23, 200, Some(4)),
            entry(25, 200, Some(5)),
            entry(27, 200, Some(6)),
            entry(29, 200, Some(7)),
        ],
        surface_ids: vec![13, 18, 23, 25, 27, 29],
        non_surface_entity_ids: Vec::new(),
        offset: 200,
    }];
    let sources = PlacementSources {
        datums: &[],
        surface_rows: &[],
        model_planes: &[],
        outline_planes: &outlines,
        plane_envelopes: &[],
        surface_parameters: &[],
        geometry_tables: &[],
        affected_ids: &[],
    };

    assert_eq!(
        generated_planar_section_transform(&definition, &sources, &tables),
        Some(FeatureSectionTransform {
            definition_id: 917,
            feature_id: Some(10),
            origin: [0.0, 0.0, 0.0],
            u_axis: [1.0, 0.0, 0.0],
            v_axis: [0.0, 0.0, -1.0],
            normal: [-0.0, 1.0, 0.0],
            offset: 200,
        })
    );
}

#[test]
fn scan_decodes_featdefs_gsec3d_placement_references() {
    let payload = b"feat_defs_40\0\xe0\x00gsec3d_ptr\0\
        plane_id\0\x83\x01plane_flip\0\x01\
        \xe0\x00ref_planes\0\xf8\x02\xf7\x05\xf7\x81\x00\xfb\xe2\
        \xe0\x01plane_id\0\x09\
        \xe0\x01flip\0\x01\xe0\x01ref_type\0\x02\
        \xe0\x01seg_id\0\x81\x2c\xe0\x01flip_flag\0\x00\
        dim_id_tab\0\xf3\xf8\x02\x07\x81\x01"
        .to_vec();
    let data = build_prt("c", &[("FeatDefs", payload)]);
    let scan = container::scan_bytes(data.clone());

    let section = scan.features.definitions[0]
        .section_3d
        .as_ref()
        .expect("gsec3d");
    assert_eq!(section.sketch_plane_entity_id, Some(769));
    assert_eq!(
        section.sketch_plane_flip,
        Some(crate::feature::BinaryFlag::Set)
    );
    assert_eq!(section.reference_plane_entity_ids, vec![5, 256]);
    assert_eq!(section.reference_plane_datum_geometry_id, Some(9));
    assert_eq!(
        section.orientation.section_flip,
        Some(crate::feature::BinaryFlag::Set)
    );
    assert_eq!(section.orientation.reference_type, Some(2));
    assert_eq!(section.orientation.segment_id, Some(300));
    assert_eq!(
        section.orientation.reference_flip,
        Some(crate::feature::BinaryFlag::Clear)
    );
    assert_eq!(section.dimension_ids, vec![7, 257]);

    let result = CreoCodec
        .decode(&mut Cursor::new(data), &DecodeOptions::default())
        .expect("decode");
    let sketches = &result.ir().native.namespace("creo").unwrap().arenas["sketches"];
    assert_eq!(sketches.len(), 1);
    assert_eq!(sketches[0].fields()["source_section"], "FeatDefs");
    let placement = &sketches[0].fields()["section_3d"];
    assert_eq!(placement["sketch_plane_entity_id"], 769);
    assert_eq!(placement["sketch_plane_flip"], true);
    assert_eq!(placement["reference_plane_entity_ids"][0], 5);
    assert_eq!(placement["reference_plane_entity_ids"][1], 256);
    assert_eq!(placement["reference_plane_datum_geometry_id"], 9);
    assert_eq!(placement["orientation"]["section_flip"], true);
    assert_eq!(placement["orientation"]["reference_type"], 2);
    assert_eq!(placement["orientation"]["segment_id"], 300);
    assert_eq!(placement["orientation"]["reference_flip"], false);
    assert_eq!(placement["dimension_ids"][0], 7);
    assert_eq!(placement["dimension_ids"][1], 257);
}

#[test]
fn named_gsec3d_fields_stop_at_the_next_record() {
    let mut payload = b"feat_defs_40\0\xe0\x00gsec3d_ptr\0".to_vec();
    payload
        .extend_from_slice(b"\xe0\x00gsec3d_ptr\0plane_id\0\x83\x01\xe0\x00p_saved_result\0\xe3");
    let scan = container::scan_bytes(build_prt("c", &[("FeatDefs", payload)]));

    let section = scan.features.definitions[0]
        .section_3d
        .as_ref()
        .expect("first gsec3d");
    assert_eq!(section.sketch_plane_entity_id, None);
}

#[test]
fn named_gsec3d_fields_extend_to_the_placement_close() {
    let mut payload =
        b"feat_defs_40\0\xe0\x00gsec3d_ptr\0\xe0\x00ref_planes\0\xf8\x01\xf7\x05\xfb\xe2".to_vec();
    payload.resize(payload.len() + 300, 0);
    payload
        .extend_from_slice(b"\xe0\x01plane_id\0\x09plane_id\0\x83\x01\xe0\x00p_saved_result\0\xe3");
    let scan = container::scan_bytes(build_prt("c", &[("FeatDefs", payload)]));

    let section = scan.features.definitions[0]
        .section_3d
        .as_ref()
        .expect("gsec3d");
    assert_eq!(section.sketch_plane_entity_id, Some(769));
    assert_eq!(section.reference_plane_entity_ids, vec![5]);
    assert_eq!(section.reference_plane_datum_geometry_id, Some(9));
}
