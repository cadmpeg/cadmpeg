// SPDX-License-Identifier: Apache-2.0
//! Tests: zero orientation.

use super::with_decode_ctx;
use crate::decode::analytic::{
    ordered_face_loops, ordered_planar_face_loops, point_on_carrier, solve_carriers,
    CarrierEquation, ConeEquation, PlaneEquation, SphereEquation, TorusEquation,
};
use crate::decode::build::has_transferred_geometry;
use crate::decode::feature_history::{
    full_turn_revolution_carrier_axis, named_feature_definition,
    named_or_referenced_feature_definition, resolved_revolution_axis, revolution_axis_for_transfer,
    schema_feature_definition,
};
use crate::decode::sketch::{
    intersect_incident_section_carriers, section_arc_geometry, trim_segment_id,
    SectionIntersectionCarrier,
};
use crate::decode::sketch_transfer::{
    materialized_saved_section_external_ids, resolved_profile_chains,
};
use crate::decode::surfaces::{
    axis_containing_plane_torus_circle_candidates, coaxial_cone_torus_circle_candidates,
    coaxial_cones_section_candidates, cubic_extrusion_plane_generator_curve,
    cubic_unit_interval_roots, nurbs_plane_boundary_curve, resolve_curve_candidates,
    select_unique_curve_candidate, shared_extrusion_generator_curve,
};
use crate::decode::sweep::{
    bspline_basis, bspline_basis_derivative, interpolation_spline_surface, placed_section_nurbs,
    revolution_face_sense, revolution_profile_boundary_pcurve, revolved_brep_surface,
    revolved_nurbs_surface, saved_spline_nurbs, saved_spline_sketch_geometry,
};
use crate::topology::HalfEdgeId;
use cadmpeg_ir::document::CadIr;
use cadmpeg_ir::features::{
    Angle, BooleanOp, FeatureDefinition as IrFeatureDefinition, Length, RevolutionAxis,
    RevolveExtent, Termination,
};
use cadmpeg_ir::geometry::{CurveGeometry, NurbsCurve, NurbsSurface, Surface, SurfaceGeometry};
use cadmpeg_ir::ids::{BodyId, PointId, SurfaceId};
use cadmpeg_ir::math::{Point2, Point3, Vector3};
use cadmpeg_ir::sketches::{SketchGeometry, SketchId};
use cadmpeg_ir::topology::{Body, BodyKind, Point};
use cadmpeg_ir::units::Units;
use std::collections::{BTreeMap, BTreeSet};

#[test]
fn zero_orientation_arc_runs_clockwise_from_first_endpoint() {
    let segment = crate::feature::FeatureSegment {
        kind: crate::feature::FeatureSegmentKind::Arc,
        directions: [None; 3],
        point_ids: [1, 2],
        center_id: Some(3),
        arc_orientation: Some(0),
        vertical_horizontal: None,
        radius_ref: Some(4),
        radius2_ref: None,
        external_id: 12,
        body: Vec::new(),
        offset: 40,
    };
    let points = BTreeMap::from([(1, [0.0, -2.0]), (2, [0.0, 2.0]), (3, [0.0, 0.0])]);
    let Some(SketchGeometry::Arc {
        center,
        radius,
        start_angle,
        end_angle,
    }) = section_arc_geometry(&points, &segment)
    else {
        panic!("complete arc");
    };
    assert_eq!(center, cadmpeg_ir::math::Point2::new(0.0, 0.0));
    assert_eq!(radius, Length(2.0));
    assert!((start_angle.0 - std::f64::consts::FRAC_PI_2).abs() < 1e-12);
    assert!((end_angle.0 - 3.0 * std::f64::consts::FRAC_PI_2).abs() < 1e-12);
}

#[test]
fn profile_chain_follows_trim_vertex_incidence() {
    let definition = crate::feature::FeatureDefinition {
        id: 40,
        owner_feature_id: Some(40),
        body: Vec::new(),
        parameter_frames: Vec::new(),
        outlines: Vec::new(),
        variables: None,
        segments: None,
        trim_entities: Some(crate::feature::FeatureTrimEntityTable {
            declared_count: None,
            entity_ref: None,
            entry_ref: None,
            buckets: Vec::new(),
            rows: [(10, [1, 2]), (11, [3, 2]), (12, [3, 4]), (13, [4, 1])]
                .into_iter()
                .map(
                    |(external_id, vertices)| crate::feature::FeatureTrimEntity {
                        external_id,
                        mode: None,
                        vertices,
                        center_vertex: None,
                        kind: crate::feature::TrimEntityKind::Line,
                        offset: external_id as usize,
                    },
                )
                .collect(),
            solved_external_ids: vec![10, 11, 12, 13],
            offset: 5,
        }),
        trim_vertices: None,
        order_table: None,
        section_3d: None,
        dimensions: None,
        relations: None,
        saved_section: None,
        offset: 1,
    };
    let profiles = resolved_profile_chains(
        &definition,
        &SketchId("creo:model:sketch#40".to_string()),
        &BTreeSet::from([10_u32, 11_u32, 12_u32, 13_u32]),
    );
    assert_eq!(profiles.len(), 1);
    assert_eq!(profiles[0].len(), 4);
    assert_eq!(profiles[0][0].entity.0, "creo:featdefs:sketch_entity#40:10");
    assert!(!profiles[0][0].reversed);
    assert!(profiles[0][1].reversed);

    let mut incomplete = definition.clone();
    let table = incomplete.trim_entities.as_mut().expect("trim table");
    table.declared_count = Some(1);
    table.buckets.push(crate::feature::FeatureTrimBucket {
        index: 0,
        declared_entry_count: 4,
        decoded_entry_count: 3,
        offset: 5,
    });
    assert!(resolved_profile_chains(
        &incomplete,
        &SketchId("creo:model:sketch#40".to_string()),
        &BTreeSet::from([10_u32, 11_u32, 12_u32, 13_u32]),
    )
    .is_empty());
    assert_eq!(
        trim_segment_id(
            &incomplete,
            &incomplete.trim_entities.as_ref().expect("trim table").rows[0],
        ),
        None
    );

    assert!(resolved_profile_chains(
        &definition,
        &SketchId("creo:model:sketch#40".to_string()),
        &BTreeSet::from([10_u32, 11_u32, 12_u32]),
    )
    .is_empty());

    let mut incomplete_trim_graph = definition.clone();
    incomplete_trim_graph.segments = Some(crate::feature::FeatureSegmentTable {
        declared_count: 4,
        has_elided_prototype: false,
        entity_ref: None,
        rows: [(10, [1, 2]), (11, [2, 3]), (12, [3, 4]), (13, [4, 1])]
            .into_iter()
            .map(|(external_id, point_ids)| crate::feature::FeatureSegment {
                kind: crate::feature::FeatureSegmentKind::Line,
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
            })
            .collect(),
        circle_rows: Vec::new(),
        point_rows: Vec::new(),
        centered_line_rows: Vec::new(),
        reference_line_rows: Vec::new(),
        bounded_curve_rows: Vec::new(),
        conic_rows: Vec::new(),
        opaque_rows: Vec::new(),
        offset: 2,
    });
    incomplete_trim_graph
        .trim_entities
        .as_mut()
        .expect("trim table")
        .rows
        .retain(|row| row.external_id != 13);
    let profiles = resolved_profile_chains(
        &incomplete_trim_graph,
        &SketchId("creo:model:sketch#40".to_string()),
        &BTreeSet::from([10_u32, 11_u32, 12_u32, 13_u32]),
    );
    assert_eq!(profiles.len(), 1);
    assert_eq!(profiles[0].len(), 4);

    let mut arcs = definition.clone();
    arcs.trim_entities = Some(crate::feature::FeatureTrimEntityTable {
        declared_count: None,
        entity_ref: None,
        entry_ref: None,
        buckets: Vec::new(),
        rows: [(10, [1, 2]), (11, [2, 1])]
            .into_iter()
            .map(
                |(external_id, vertices)| crate::feature::FeatureTrimEntity {
                    external_id,
                    mode: None,
                    vertices,
                    center_vertex: Some(3),
                    kind: crate::feature::TrimEntityKind::Arc,
                    offset: external_id as usize,
                },
            )
            .collect(),
        solved_external_ids: vec![10, 11],
        offset: 5,
    });
    arcs.segments = Some(crate::feature::FeatureSegmentTable {
        declared_count: 2,
        has_elided_prototype: false,
        entity_ref: None,
        rows: [10, 11]
            .into_iter()
            .map(|external_id| crate::feature::FeatureSegment {
                kind: crate::feature::FeatureSegmentKind::Arc,
                directions: [None; 3],
                point_ids: [1, 2],
                center_id: Some(3),
                arc_orientation: Some(0),
                vertical_horizontal: None,
                radius_ref: None,
                radius2_ref: None,
                external_id,
                body: Vec::new(),
                offset: external_id as usize,
            })
            .collect(),
        circle_rows: Vec::new(),
        point_rows: Vec::new(),
        centered_line_rows: Vec::new(),
        reference_line_rows: Vec::new(),
        bounded_curve_rows: Vec::new(),
        conic_rows: Vec::new(),
        opaque_rows: Vec::new(),
        offset: 4,
    });
    let arc_profile = resolved_profile_chains(
        &arcs,
        &SketchId("creo:model:sketch#40".to_string()),
        &BTreeSet::from([10, 11]),
    );
    assert_eq!(arc_profile.len(), 1);
    assert!(arc_profile[0].iter().all(|entity| entity.reversed));

    let mut segment_graph = definition;
    segment_graph.trim_entities = None;
    segment_graph.segments = Some(crate::feature::FeatureSegmentTable {
        declared_count: 5,
        has_elided_prototype: false,
        entity_ref: None,
        rows: [
            (10, [1, 2]),
            (11, [3, 2]),
            (12, [3, 4]),
            (13, [4, 1]),
            (20, [8, 9]),
        ]
        .into_iter()
        .map(|(external_id, point_ids)| crate::feature::FeatureSegment {
            kind: crate::feature::FeatureSegmentKind::Line,
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
        })
        .collect(),
        circle_rows: Vec::new(),
        point_rows: Vec::new(),
        centered_line_rows: Vec::new(),
        reference_line_rows: Vec::new(),
        bounded_curve_rows: Vec::new(),
        conic_rows: Vec::new(),
        opaque_rows: Vec::new(),
        offset: 4,
    });
    let segment_profile = resolved_profile_chains(
        &segment_graph,
        &SketchId("creo:model:sketch#40".to_string()),
        &BTreeSet::from([10, 11, 12, 13, 20]),
    );
    assert_eq!(segment_profile.len(), 1);
    assert_eq!(segment_profile[0].len(), 4);
    assert!(!segment_profile[0][0].reversed);
    assert!(segment_profile[0][1].reversed);
}

#[test]
fn multi_incident_trim_vertex_requires_one_agreeing_pairwise_intersection() {
    let line = |start: [f64; 2], end: [f64; 2]| SectionIntersectionCarrier {
        geometry: SketchGeometry::Line {
            start: cadmpeg_ir::math::Point2::new(start[0], start[1]),
            end: cadmpeg_ir::math::Point2::new(end[0], end[1]),
        },
    };
    let concurrent = [
        line([-1.0, 0.0], [1.0, 0.0]),
        line([0.0, -1.0], [0.0, 1.0]),
        line([-1.0, -1.0], [1.0, 1.0]),
    ];
    assert_eq!(
        intersect_incident_section_carriers(&concurrent),
        Some([0.0, 0.0])
    );

    let inconsistent = [
        line([-1.0, 0.0], [1.0, 0.0]),
        line([0.0, -1.0], [0.0, 1.0]),
        line([-1.0, 2.0], [2.0, -1.0]),
    ];
    assert_eq!(intersect_incident_section_carriers(&inconsistent), None);
}

#[test]
fn revolution_axis_uses_the_unique_complete_section_centerline() {
    let definition = crate::feature::FeatureDefinition {
        id: 40,
        owner_feature_id: Some(40),
        body: Vec::new(),
        parameter_frames: Vec::new(),
        outlines: Vec::new(),
        variables: Some(crate::feature::FeatureVariableTable {
            declared_count: 0,
            entity_ref: None,
            rows: Vec::new(),
            points: vec![
                crate::feature::FeatureSectionPoint {
                    point_id: 1,
                    u: Some(0.0),
                    v: Some(-2.0),
                },
                crate::feature::FeatureSectionPoint {
                    point_id: 2,
                    u: Some(0.0),
                    v: Some(3.0),
                },
            ],
            offset: 1,
        }),
        segments: Some(crate::feature::FeatureSegmentTable {
            declared_count: 1,
            has_elided_prototype: false,
            entity_ref: None,
            rows: vec![crate::feature::FeatureSegment {
                kind: crate::feature::FeatureSegmentKind::Line,
                directions: [None; 3],
                point_ids: [1, 2],
                center_id: None,
                arc_orientation: None,
                vertical_horizontal: Some(0),
                radius_ref: None,
                radius2_ref: None,
                external_id: 1,
                body: Vec::new(),
                offset: 2,
            }],
            circle_rows: Vec::new(),
            point_rows: Vec::new(),
            centered_line_rows: Vec::new(),
            reference_line_rows: Vec::new(),
            bounded_curve_rows: Vec::new(),
            conic_rows: Vec::new(),
            opaque_rows: Vec::new(),
            offset: 2,
        }),
        trim_entities: None,
        trim_vertices: None,
        order_table: None,
        section_3d: None,
        dimensions: None,
        relations: None,
        saved_section: None,
        offset: 1,
    };
    let transform = crate::placement::FeatureSectionTransform {
        definition_id: 40,
        feature_id: Some(40),
        origin: [5.0, 7.0, 11.0],
        u_axis: [1.0, 0.0, 0.0],
        v_axis: [0.0, 0.0, 1.0],
        normal: [0.0, -1.0, 0.0],
        offset: 3,
    };

    let axis = resolved_revolution_axis(&definition, &transform).expect("axis");
    assert_eq!(axis.origin, Point3::new(5.0, 7.0, 9.0));
    assert_eq!(axis.direction, Vector3::new(0.0, 0.0, 1.0));
}

#[test]
fn full_turn_revolution_uses_the_unique_generated_carrier_axis() {
    let mut scan = crate::container::scan_bytes(Vec::new());
    for (id, kind) in [
        (31, crate::surface::SurfaceKind::Cylinder),
        (32, crate::surface::SurfaceKind::Cone),
        (33, crate::surface::SurfaceKind::TorusOrSphere),
    ] {
        scan.surfaces.rows.push(crate::surface::SurfaceRow {
            id,
            type_byte: 0,
            kind,
            feature_id: 7,
            reversed: false,
            boundary_type: 0,
            next_surface: 0,
            offset: id as usize,
        });
    }
    let mut ir = CadIr::empty(Units::default());
    ir.model.surfaces.extend([
        Surface {
            id: SurfaceId("creo:visibgeom:surface#31".to_string()),
            geometry: SurfaceGeometry::Cylinder {
                origin: Point3::new(2.0, 3.0, 0.0),
                axis: Vector3::new(0.0, -1.0, 0.0),
                ref_direction: Vector3::new(1.0, 0.0, 0.0),
                radius: 1.0,
            },
            source_object: None,
        },
        Surface {
            id: SurfaceId("creo:visibgeom:surface#32".to_string()),
            geometry: SurfaceGeometry::Cone {
                origin: Point3::new(2.0, -5.0, 0.0),
                axis: Vector3::new(0.0, 1.0, 0.0),
                ref_direction: Vector3::new(1.0, 0.0, 0.0),
                radius: 0.0,
                ratio: 1.0,
                half_angle: 0.5,
            },
            source_object: None,
        },
        Surface {
            id: SurfaceId("creo:visibgeom:surface#33".to_string()),
            geometry: SurfaceGeometry::Sphere {
                center: Point3::new(2.0, 8.0, 0.0),
                axis: Vector3::new(0.0, 0.0, 1.0),
                ref_direction: Vector3::new(1.0, 0.0, 0.0),
                radius: 2.0,
            },
            source_object: None,
        },
    ]);
    let full_turn = RevolveExtent::OneSided {
        termination: Termination::Angle {
            angle: Angle(std::f64::consts::TAU),
        },
    };

    assert_eq!(
        full_turn_revolution_carrier_axis(&scan, &ir, 7, Some(&full_turn)),
        Some(RevolutionAxis {
            origin: Point3::new(2.0, 0.0, 0.0),
            direction: Vector3::new(0.0, 1.0, 0.0),
        })
    );
    let carrier_only_definition = crate::feature::FeatureDefinition {
        id: 7,
        owner_feature_id: Some(7),
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
    };
    let transform = crate::placement::FeatureSectionTransform {
        definition_id: 7,
        feature_id: Some(7),
        origin: [0.0, 0.0, 0.0],
        u_axis: [1.0, 0.0, 0.0],
        v_axis: [0.0, 1.0, 0.0],
        normal: [0.0, 0.0, 1.0],
        offset: 0,
    };
    assert_eq!(
        revolution_axis_for_transfer(
            &scan,
            &ir,
            7,
            &carrier_only_definition,
            &transform,
            Some(&full_turn),
        ),
        Some(RevolutionAxis {
            origin: Point3::new(2.0, 0.0, 0.0),
            direction: Vector3::new(0.0, 1.0, 0.0),
        })
    );
    let partial = RevolveExtent::OneSided {
        termination: Termination::Angle { angle: Angle(1.0) },
    };
    assert!(full_turn_revolution_carrier_axis(&scan, &ir, 7, Some(&partial)).is_none());
    if let SurfaceGeometry::Cone { origin, .. } = &mut ir.model.surfaces[1].geometry {
        origin.x = 3.0;
    }
    assert!(full_turn_revolution_carrier_axis(&scan, &ir, 7, Some(&full_turn)).is_none());
    if let SurfaceGeometry::Cone { origin, .. } = &mut ir.model.surfaces[1].geometry {
        origin.x = 2.0;
    }
    let SurfaceGeometry::Sphere { center, .. } = &mut ir.model.surfaces[2].geometry else {
        unreachable!();
    };
    center.z = 1.0;
    assert!(full_turn_revolution_carrier_axis(&scan, &ir, 7, Some(&full_turn)).is_none());
}

#[test]
fn named_revolve_transfers_profile_axis() {
    let definition = crate::feature::FeatureDefinition {
        id: 822,
        owner_feature_id: Some(822),
        body: Vec::new(),
        parameter_frames: Vec::new(),
        outlines: Vec::new(),
        variables: Some(crate::feature::FeatureVariableTable {
            declared_count: 4,
            entity_ref: None,
            rows: [(1, 1, 0.0), (2, 1, 0.0), (1, 2, 0.0), (2, 2, 10.0)]
                .into_iter()
                .map(
                    |(variable_type, key, value)| crate::feature::FeatureVariableRow {
                        variable_type,
                        key,
                        value: Some(value),
                        value_body: Vec::new(),
                        guess: Some(value),
                        guess_body: Vec::new(),
                        guess_dimension_driven: false,
                        known: Some(0),
                        homogeneity: Some(1),
                        uvar_id: None,
                        dimension_driven: false,
                        offset: 0,
                    },
                )
                .collect(),
            points: Vec::new(),
            offset: 0,
        }),
        segments: Some(crate::feature::FeatureSegmentTable {
            declared_count: 1,
            has_elided_prototype: false,
            entity_ref: None,
            rows: vec![crate::feature::FeatureSegment {
                kind: crate::feature::FeatureSegmentKind::Line,
                directions: [None; 3],
                point_ids: [1, 2],
                center_id: None,
                arc_orientation: None,
                vertical_horizontal: None,
                radius_ref: None,
                radius2_ref: None,
                external_id: 1,
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
        order_table: None,
        section_3d: Some(crate::feature::FeatureSection3d {
            sketch_plane_entity_id: None,
            sketch_plane_flip: None,
            reference_plane_entity_ids: Vec::new(),
            reference_plane_rows: Vec::new(),
            reference_plane_datum_geometry_id: None,
            orientation: crate::feature::FeatureSectionOrientation::default(),
            dimension_ids: Vec::new(),
            offset: 90,
        }),
        dimensions: None,
        relations: None,
        saved_section: None,
        offset: 80,
    };
    let transform = crate::placement::FeatureSectionTransform {
        definition_id: 822,
        feature_id: Some(822),
        origin: [0.0; 3],
        u_axis: [1.0, 0.0, 0.0],
        v_axis: [0.0, 1.0, 0.0],
        normal: [0.0, 0.0, 1.0],
        offset: 90,
    };
    let mut scan = crate::container::scan_bytes(Vec::new());
    scan.features.definitions.push(definition);
    scan.features.section_transforms.push(transform);
    scan.features
        .revolution_extents
        .push(crate::feature::FeatureRevolutionExtent {
            feature_id: 822,
            kind: crate::feature::FeatureRevolutionExtentKind::FullTurn,
            offset: 1,
        });
    let mut ir = CadIr::empty(Units::default());
    ir.model.bodies.push(Body {
        id: BodyId("creo:feature:revolution#822:body".to_string()),
        kind: BodyKind::Solid,
        regions: Vec::new(),
        transform: None,
        name: None,
        color: None,
        visible: None,
    });

    let Some(cadmpeg_ir::features::FeatureDefinition::Revolve {
        construction:
            cadmpeg_ir::features::RevolutionConstruction {
                axis: Some(axis),
                solid: Some(true),
                ..
            },
        op: BooleanOp::NewBody,
    }) = named_feature_definition(&scan, &ir, 822, "Revolve")
    else {
        panic!("named revolve axis");
    };
    assert_eq!(axis.origin, Point3::new(0.0, 0.0, 0.0));
    assert_eq!(axis.direction, Vector3::new(0.0, 1.0, 0.0));
}

#[test]
fn named_extrude_with_evaluated_body_is_new_body() {
    let scan = crate::container::scan_bytes(Vec::new());
    let mut ir = CadIr::empty(Units::default());
    ir.model.bodies.push(Body {
        id: BodyId("creo:feature:extrusion#822:body".to_string()),
        kind: BodyKind::Solid,
        regions: Vec::new(),
        transform: None,
        name: None,
        color: None,
        visible: None,
    });

    let Some(cadmpeg_ir::features::FeatureDefinition::Extrude { op, solid, .. }) =
        named_feature_definition(&scan, &ir, 822, "Extrude")
    else {
        panic!("named extrude definition");
    };
    assert_eq!(op, BooleanOp::NewBody);
    assert_eq!(solid, Some(true));
}

#[test]
fn schema_numbered_extrude_with_evaluated_body_is_new_body() {
    let scan = crate::container::scan_bytes(Vec::new());
    let mut ir = CadIr::empty(Units::default());
    ir.model.bodies.push(Body {
        id: BodyId("creo:feature:extrusion#822:body".to_string()),
        kind: BodyKind::Solid,
        regions: Vec::new(),
        transform: None,
        name: None,
        color: None,
        visible: None,
    });

    let IrFeatureDefinition::Extrude { op, solid, .. } =
        schema_feature_definition(&scan, &ir, 822, 0, "Extrude 822")
    else {
        panic!("schema numbered extrude definition");
    };
    assert_eq!(op, BooleanOp::NewBody);
    assert_eq!(solid, Some(true));
}

#[test]
fn conflicting_section_sweep_names_remain_unresolved() {
    let mut scan = crate::container::scan_bytes(Vec::new());
    scan.features
        .operations
        .push(crate::feature::FeatureOperation {
            feature_id: 822,
            kind: "Extrude".to_string(),
            display_name_stored: true,
            stored_name: Some("Extrude id 822".to_string()),
            stored_name_bytes: Some(b"Extrude id 822".to_vec()),
            identifier_keyword: Some("id".to_string()),
            stored_name_prefix: None,
            recipe: None,
            recipe_conflict: true,
            display_state_conflict: false,
            root_schema_class: None,
            parent_feature_id: None,
            offset: 0,
            state_offset: 0,
        });
    scan.features
        .reference_names
        .push(crate::feature::FeatureReferenceName {
            feature_id: 822,
            name: "Revolve 822".to_string(),
            name_bytes: b"Revolve 822".to_vec(),
            own_reference_id: 1,
            reference_type: 0,
            offset: 0,
        });
    let ir = CadIr::empty(Units::default());

    for kind in [
        "Protrusion",
        "Cut",
        "Extrude",
        "Extrude 822",
        "Revolve",
        "Revolve 822",
    ] {
        assert!(
            named_feature_definition(&scan, &ir, 822, kind).is_none(),
            "conflicting section-sweep name projected: {kind}"
        );
    }
    assert!(named_or_referenced_feature_definition(&scan, &ir, 822, "Native Feature").is_none());
}

#[test]
fn saved_spline_collocation_interpolates_points_and_endpoint_derivatives() {
    let spline = crate::feature::FeatureSavedSpline {
        entity_id: Some(7),
        declared_point_count: Some(3),
        interpolation_points: vec![[0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [2.0, 0.0, 0.0]],
        interpolation_points_body: Vec::new(),
        endpoint_tangents: Some([[1.0, 0.0, 0.0], [1.0, 0.0, 0.0]]),
        endpoint_tangents_body: None,
        parameters: Some(vec![0.0, 1.0, 2.0]),
        parameters_body: None,
        offset: 10,
    };
    let nurbs = saved_spline_nurbs(&spline).expect("clamped interpolation spline");
    for (parameter, expected) in [(0.0, 0.0), (1.0, 1.0), (2.0, 2.0)] {
        let point = nurbs.control_points.iter().enumerate().fold(
            [0.0; 3],
            |mut point, (index, control)| {
                let basis = bspline_basis(
                    index,
                    nurbs.degree as usize,
                    parameter,
                    &nurbs.knots,
                    nurbs.control_points.len(),
                );
                point[0] += basis * control.x;
                point[1] += basis * control.y;
                point[2] += basis * control.z;
                point
            },
        );
        assert!((point[0] - expected).abs() < 1e-12);
        assert!(point[1].abs() < 1e-12 && point[2].abs() < 1e-12);
    }
    for parameter in [0.0, 2.0] {
        let derivative = nurbs.control_points.iter().enumerate().fold(
            [0.0; 3],
            |mut derivative, (index, control)| {
                let basis = bspline_basis_derivative(
                    index,
                    nurbs.degree as usize,
                    parameter,
                    &nurbs.knots,
                    nurbs.control_points.len(),
                );
                derivative[0] += basis * control.x;
                derivative[1] += basis * control.y;
                derivative[2] += basis * control.z;
                derivative
            },
        );
        assert!((derivative[0] - 1.0).abs() < 1e-12);
        assert!(derivative[1].abs() < 1e-12 && derivative[2].abs() < 1e-12);
    }
    assert!(matches!(
        saved_spline_sketch_geometry(&spline),
        Some(SketchGeometry::Nurbs { degree: 3, .. })
    ));
    let definition = crate::feature::FeatureDefinition {
        id: 917,
        owner_feature_id: Some(40),
        body: Vec::new(),
        parameter_frames: Vec::new(),
        outlines: Vec::new(),
        variables: None,
        segments: Some(crate::feature::FeatureSegmentTable {
            declared_count: 1,
            has_elided_prototype: false,
            entity_ref: None,
            rows: Vec::new(),
            circle_rows: Vec::new(),
            point_rows: Vec::new(),
            centered_line_rows: Vec::new(),
            reference_line_rows: Vec::new(),
            bounded_curve_rows: Vec::new(),
            conic_rows: Vec::new(),
            opaque_rows: vec![crate::feature::FeatureOpaqueSegment {
                kind: 25,
                directions: [None; 3],
                point_ids: [Some(1), Some(2)],
                center_id: None,
                arc_orientation: None,
                vertical_horizontal: None,
                radius_ref: None,
                radius2_ref: None,
                external_id: 42,
                body: Vec::new(),
                offset: 20,
            }],
            offset: 20,
        }),
        trim_entities: None,
        trim_vertices: None,
        order_table: Some(crate::feature::FeatureOrderTable {
            declared_count: 1,
            has_prototype: false,
            entity_ref: None,
            rows: vec![crate::feature::FeatureOrderRow {
                external_id: 42,
                internal_id: 7,
                bitmask: 0,
                offset: 30,
            }],
            offset: 30,
        }),
        section_3d: None,
        dimensions: None,
        relations: None,
        saved_section: Some(crate::feature::FeatureSavedSection {
            entities: vec![crate::feature::FeatureSavedEntity::Spline(spline.clone())],
            offset: 40,
        }),
        offset: 1,
    };
    assert_eq!(
        materialized_saved_section_external_ids(&definition),
        BTreeSet::from([42])
    );

    let mut incomplete = spline;
    incomplete.declared_point_count = Some(4);
    assert!(saved_spline_nurbs(&incomplete).is_none());
    assert!(saved_spline_sketch_geometry(&incomplete).is_none());

    let mut duplicate_saved_id = definition.clone();
    duplicate_saved_id
        .saved_section
        .as_mut()
        .expect("saved section")
        .entities
        .push(crate::feature::FeatureSavedEntity::Spline(incomplete));
    assert!(materialized_saved_section_external_ids(&duplicate_saved_id).is_empty());

    let mut ambiguous_external_id = definition;
    let duplicate_opaque = ambiguous_external_id
        .segments
        .as_ref()
        .expect("segments")
        .opaque_rows[0]
        .clone();
    ambiguous_external_id
        .segments
        .as_mut()
        .expect("segments")
        .opaque_rows
        .push(duplicate_opaque);
    ambiguous_external_id
        .segments
        .as_mut()
        .expect("segments")
        .declared_count = 2;
    assert!(materialized_saved_section_external_ids(&ambiguous_external_id).is_empty());

    let mut incomplete_segment_table = ambiguous_external_id;
    incomplete_segment_table
        .segments
        .as_mut()
        .expect("segments")
        .opaque_rows
        .pop();
    assert_eq!(
        materialized_saved_section_external_ids(&incomplete_segment_table),
        BTreeSet::from([42])
    );
}

#[test]
fn tensor_product_collocation_preserves_position_and_derivative_order() {
    let points = [
        [0.0, 0.0, 0.0],
        [0.0, 1.0, 2.0],
        [1.0, 0.0, 1.0],
        [1.0, 1.0, 3.0],
    ];
    let du = [1.0, 0.0, 1.0];
    let dv = [0.0, 1.0, 2.0];
    let zero = [0.0; 3];
    let nurbs = interpolation_spline_surface(
        &points,
        &[0.0, 1.0],
        &[0.0, 1.0],
        &[du, du, du, du],
        &[dv, dv, dv, dv],
        &[zero, zero, zero, zero],
    )
    .expect("bicubic tensor-product surface");

    assert_eq!((nurbs.u_count, nurbs.v_count), (4, 4));
    assert_eq!(nurbs.u_knots, [0.0, 0.0, 0.0, 0.0, 1.0, 1.0, 1.0, 1.0]);
    assert_eq!(nurbs.v_knots, nurbs.u_knots);
    for u in 0..4 {
        for v in 0..4 {
            let point = &nurbs.control_points[u * 4 + v];
            let expected_u = u as f64 / 3.0;
            let expected_v = v as f64 / 3.0;
            assert!((point.x - expected_u).abs() < 1e-12);
            assert!((point.y - expected_v).abs() < 1e-12);
            assert!((point.z - expected_u - 2.0 * expected_v).abs() < 1e-12);
        }
    }
}

#[test]
fn nonplanar_saved_spline_places_as_model_curve() {
    let transform = crate::placement::FeatureSectionTransform {
        definition_id: 917,
        feature_id: Some(40),
        origin: [10.0, 20.0, 30.0],
        u_axis: [1.0, 0.0, 0.0],
        v_axis: [0.0, 0.0, 1.0],
        normal: [0.0, -1.0, 0.0],
        offset: 5,
    };
    let local = NurbsCurve {
        degree: 1,
        knots: vec![0.0, 0.0, 1.0, 1.0],
        control_points: vec![Point3::new(1.0, 2.0, 3.0), Point3::new(4.0, 5.0, 6.0)],
        weights: None,
        periodic: false,
    };

    let placed = placed_section_nurbs(&transform, &local);

    assert_eq!(placed.control_points[0], Point3::new(11.0, 17.0, 32.0));
    assert_eq!(placed.control_points[1], Point3::new(14.0, 14.0, 35.0));
}

#[test]
fn transferred_geometry_is_derived_from_ir_arenas() {
    let mut ir = CadIr::empty(Units::default());
    assert!(!has_transferred_geometry(&ir));

    ir.model.points.push(Point {
        id: PointId("point".to_string()),
        position: Point3::new(1.0, 2.0, 3.0),
        source_object: None,
    });
    assert!(has_transferred_geometry(&ir));
}

#[test]
fn full_revolution_uses_exact_quadratic_circle_poles() {
    let directrix = NurbsCurve {
        degree: 1,
        knots: vec![0.0, 0.0, 1.0, 1.0],
        control_points: vec![Point3::new(2.0, 0.0, 0.0), Point3::new(2.0, 0.0, 1.0)],
        weights: None,
        periodic: false,
    };
    let surface = revolved_nurbs_surface(
        &directrix,
        RevolutionAxis {
            origin: Point3::new(0.0, 0.0, 0.0),
            direction: Vector3::new(0.0, 0.0, 1.0),
        },
    )
    .expect("revolution surface");

    assert_eq!((surface.u_count, surface.v_count), (2, 9));
    assert_eq!(surface.control_points[0], Point3::new(2.0, 0.0, 0.0));
    assert_eq!(surface.control_points[1], Point3::new(2.0, 2.0, 0.0));
    assert_eq!(surface.control_points[2], Point3::new(0.0, 2.0, 0.0));
    assert_eq!(surface.control_points[8], surface.control_points[0]);
    assert_eq!(
        surface.weights.as_ref().expect("rational weights")[1],
        std::f64::consts::FRAC_1_SQRT_2
    );
}

#[test]
fn revolved_spline_profile_preserves_intrinsic_surface_domain_and_boundary_sense() {
    let transform = crate::placement::FeatureSectionTransform {
        definition_id: 1,
        feature_id: Some(2),
        origin: [0.0, 0.0, 0.0],
        u_axis: [1.0, 0.0, 0.0],
        v_axis: [0.0, 1.0, 0.0],
        normal: [0.0, 0.0, 1.0],
        offset: 0,
    };
    let axis = RevolutionAxis {
        origin: Point3::new(0.0, 0.0, 0.0),
        direction: Vector3::new(0.0, 1.0, 0.0),
    };
    let spline = SketchGeometry::Nurbs {
        degree: 2,
        knots: vec![2.0, 2.0, 2.0, 3.0, 5.0, 5.0, 5.0],
        control_points: vec![
            Point2::new(2.0, 0.0),
            Point2::new(3.0, 0.75),
            Point2::new(3.0, 1.25),
            Point2::new(2.0, 2.0),
        ],
        weights: Some(vec![1.0, 0.75, 0.75, 1.0]),
        periodic: false,
    };
    let segment = (spline.clone(), false, [2.0, 0.0], [2.0, 2.0]);
    let surface =
        revolved_brep_surface(&transform, &spline, false, axis).expect("revolved spline surface");
    let SurfaceGeometry::Nurbs(surface) = &surface else {
        panic!("spline revolution must retain a NURBS surface");
    };

    assert_eq!((surface.u_degree, surface.v_degree), (2, 2));
    assert_eq!((surface.u_count, surface.v_count), (4, 9));
    assert_eq!(surface.u_knots, [2.0, 2.0, 2.0, 3.0, 5.0, 5.0, 5.0]);
    assert_eq!(surface.control_points[0], Point3::new(2.0, 0.0, 0.0));
    assert_eq!(surface.control_points[1], Point3::new(2.0, 0.0, -2.0));
    assert_eq!(surface.control_points[9], Point3::new(3.0, 0.75, 0.0));
    assert_eq!(
        surface.weights.as_ref().expect("rational surface weights")[10],
        0.75 * std::f64::consts::FRAC_1_SQRT_2
    );

    let start_pcurve = revolution_profile_boundary_pcurve(
        &transform,
        &segment,
        &SurfaceGeometry::Nurbs(surface.clone()),
        axis,
        segment.2,
        true,
    )
    .expect("start boundary pcurve");
    let end_pcurve = revolution_profile_boundary_pcurve(
        &transform,
        &segment,
        &SurfaceGeometry::Nurbs(surface.clone()),
        axis,
        segment.3,
        false,
    )
    .expect("end boundary pcurve");
    for (pcurve, expected_u) in [(start_pcurve, 2.0), (end_pcurve, 5.0)] {
        assert_eq!(
            cadmpeg_ir::eval::pcurve_uv(&pcurve, 0.0).expect("pcurve start"),
            cadmpeg_ir::math::Point2::new(expected_u, 0.0)
        );
        assert_eq!(
            cadmpeg_ir::eval::pcurve_uv(&pcurve, 1.0).expect("pcurve end"),
            cadmpeg_ir::math::Point2::new(expected_u, std::f64::consts::TAU)
        );
    }

    let forward_sense = revolution_face_sense(
        &transform,
        &segment,
        &SurfaceGeometry::Nurbs(surface.clone()),
        axis,
        1.0,
    )
    .expect("forward face sense");
    let reverse_sense = revolution_face_sense(
        &transform,
        &segment,
        &SurfaceGeometry::Nurbs(surface.clone()),
        axis,
        -1.0,
    )
    .expect("reverse face sense");
    assert_ne!(forward_sense, reverse_sense);

    let reversed = revolved_brep_surface(&transform, &spline, true, axis)
        .expect("reversed revolved spline surface");
    let SurfaceGeometry::Nurbs(reversed) = reversed else {
        panic!("reversed spline revolution must retain a NURBS surface");
    };
    assert_eq!(reversed.u_knots, [2.0, 2.0, 2.0, 4.0, 5.0, 5.0, 5.0]);
    assert_eq!(reversed.control_points[0], Point3::new(2.0, 2.0, 0.0));
}

#[test]
fn planar_loop_containment_selects_one_outer_boundary() {
    let make_loop = |face_id: u32, first_curve: u32| crate::topology::Loop {
        face_id,
        half_edges: (0_u32..4)
            .map(|index| HalfEdgeId {
                curve_id: first_curve + index,
                side: 0,
            })
            .collect(),
    };
    let outer = make_loop(9, 1);
    let inner = make_loop(9, 5);
    let incidences = (1..=8)
        .map(|vertex| crate::topology::HalfEdgeVertexIncidence {
            half_edge: HalfEdgeId {
                curve_id: vertex,
                side: 0,
            },
            start_vertex_id: vertex,
            end_vertex_id: Some(if vertex % 4 == 0 {
                vertex - 3
            } else {
                vertex + 1
            }),
        })
        .collect::<Vec<_>>();
    let incidence = incidences
        .iter()
        .map(|binding| (binding.half_edge, binding))
        .collect::<BTreeMap<_, _>>();
    let points = BTreeMap::from([
        (1, [-2.0, -2.0, 0.0]),
        (2, [2.0, -2.0, 0.0]),
        (3, [2.0, 2.0, 0.0]),
        (4, [-2.0, 2.0, 0.0]),
        (5, [-1.0, -1.0, 0.0]),
        (6, [1.0, -1.0, 0.0]),
        (7, [1.0, 1.0, 0.0]),
        (8, [-1.0, 1.0, 0.0]),
    ]);
    let plane = PlaneEquation {
        origin: [0.0; 3],
        normal: [0.0, 0.0, 1.0],
    };

    let ordered = ordered_planar_face_loops(vec![&inner, &outer], plane, &incidence, &points)
        .expect("unique outer loop");
    assert_eq!(ordered[0].half_edges[0].curve_id, 1);
    assert_eq!(ordered[1].half_edges[0].curve_id, 5);

    let disjoint_points = points
        .into_iter()
        .map(|(id, mut point)| {
            if id >= 5 {
                point[0] += 10.0;
            }
            (id, point)
        })
        .collect::<BTreeMap<_, _>>();
    assert!(
        ordered_planar_face_loops(vec![&outer, &inner], plane, &incidence, &disjoint_points,)
            .is_none()
    );
    assert_eq!(
        ordered_face_loops(vec![&outer], None, &incidence, &disjoint_points),
        Some(vec![&outer])
    );
    assert!(
        ordered_face_loops(vec![&outer, &inner], None, &incidence, &disjoint_points,).is_none()
    );
}

#[test]
fn planar_loop_containment_derives_plane_from_solved_boundary_vertices() {
    let make_loop = |first_curve: u32| crate::topology::Loop {
        face_id: 9,
        half_edges: (0_u32..4)
            .map(|index| HalfEdgeId {
                curve_id: first_curve + index,
                side: 0,
            })
            .collect(),
    };
    let outer = make_loop(1);
    let inner = make_loop(5);
    let incidences = (1..=8)
        .map(|vertex| crate::topology::HalfEdgeVertexIncidence {
            half_edge: HalfEdgeId {
                curve_id: vertex,
                side: 0,
            },
            start_vertex_id: vertex,
            end_vertex_id: Some(if vertex % 4 == 0 {
                vertex - 3
            } else {
                vertex + 1
            }),
        })
        .collect::<Vec<_>>();
    let incidence = incidences
        .iter()
        .map(|binding| (binding.half_edge, binding))
        .collect::<BTreeMap<_, _>>();
    let points = BTreeMap::from([
        (1, [-2.0, -2.0, 4.0]),
        (2, [2.0, -2.0, 4.0]),
        (3, [2.0, 2.0, 4.0]),
        (4, [-2.0, 2.0, 4.0]),
        (5, [-1.0, -1.0, 4.0]),
        (6, [1.0, -1.0, 4.0]),
        (7, [1.0, 1.0, 4.0]),
        (8, [-1.0, 1.0, 4.0]),
    ]);

    let ordered = ordered_face_loops(vec![&inner, &outer], None, &incidence, &points)
        .expect("boundary vertices prove a unique plane");
    assert_eq!(ordered[0].half_edges[0].curve_id, 1);
    assert_eq!(ordered[1].half_edges[0].curve_id, 5);

    let non_planar = points
        .into_iter()
        .map(|(id, mut point)| {
            if id == 8 {
                point[2] += 1.0;
            }
            (id, point)
        })
        .collect::<BTreeMap<_, _>>();
    assert!(ordered_face_loops(vec![&outer, &inner], None, &incidence, &non_planar).is_none());
}

#[test]
fn extrusion_nurbs_boundary_requires_one_plane_supported_control_edge() {
    let surface = NurbsSurface {
        u_degree: 3,
        v_degree: 1,
        u_knots: vec![0.0, 0.0, 0.0, 0.0, 1.0, 1.0, 1.0, 1.0],
        v_knots: vec![0.0, 0.0, 1.0, 1.0],
        u_count: 4,
        v_count: 2,
        control_points: (0..4)
            .flat_map(|u| {
                [
                    Point3::new(f64::from(u), 0.0, f64::from(u * u)),
                    Point3::new(f64::from(u), 1.0, f64::from(u * u)),
                ]
            })
            .collect(),
        weights: Some(vec![1.0, 1.0, 2.0, 2.0, 3.0, 3.0, 4.0, 4.0]),
        u_periodic: false,
        v_periodic: false,
    };
    let boundary = nurbs_plane_boundary_curve(
        &surface,
        PlaneEquation {
            origin: [0.0, 1.0, 0.0],
            normal: [0.0, 1.0, 0.0],
        },
    )
    .expect("v1 boundary");
    let CurveGeometry::Nurbs(boundary) = boundary else {
        panic!("extrusion boundary must retain its NURBS parameterization");
    };
    assert_eq!(boundary.degree, 3);
    assert_eq!(boundary.knots, surface.u_knots);
    assert_eq!(
        boundary.control_points,
        vec![
            Point3::new(0.0, 1.0, 0.0),
            Point3::new(1.0, 1.0, 1.0),
            Point3::new(2.0, 1.0, 4.0),
            Point3::new(3.0, 1.0, 9.0),
        ]
    );
    assert_eq!(boundary.weights, Some(vec![1.0, 2.0, 3.0, 4.0]));

    let generator = nurbs_plane_boundary_curve(
        &surface,
        PlaneEquation {
            origin: [3.0, 0.0, 0.0],
            normal: [1.0, 0.0, 0.0],
        },
    )
    .expect("u1 boundary");
    let CurveGeometry::Nurbs(generator) = generator else {
        panic!("extrusion generator must retain its NURBS parameterization");
    };
    assert_eq!(generator.degree, 1);
    assert_eq!(generator.knots, surface.v_knots);
    assert_eq!(
        generator.control_points,
        vec![Point3::new(3.0, 0.0, 9.0), Point3::new(3.0, 1.0, 9.0)]
    );
    assert_eq!(generator.weights, Some(vec![4.0, 4.0]));

    assert!(nurbs_plane_boundary_curve(
        &surface,
        PlaneEquation {
            origin: [0.0, 0.5, 0.0],
            normal: [0.0, 1.0, 0.0],
        },
    )
    .is_none());
    let mut coplanar = surface.clone();
    for point in &mut coplanar.control_points {
        point.z = 0.0;
    }
    assert!(nurbs_plane_boundary_curve(
        &coplanar,
        PlaneEquation {
            origin: [0.0, 0.0, 0.0],
            normal: [0.0, 0.0, 1.0],
        },
    )
    .is_none());
    coplanar.control_points = surface.control_points;
    coplanar.weights.as_mut().expect("weights")[0] = 0.0;
    assert!(nurbs_plane_boundary_curve(
        &coplanar,
        PlaneEquation {
            origin: [0.0, 1.0, 0.0],
            normal: [0.0, 1.0, 0.0],
        },
    )
    .is_none());
}

#[test]
fn shared_extrusion_generator_requires_equivalent_boundaries_and_separated_nets() {
    let first = NurbsSurface {
        u_degree: 1,
        v_degree: 1,
        u_knots: vec![0.0, 0.0, 1.0, 1.0],
        v_knots: vec![0.0, 0.0, 1.0, 1.0],
        u_count: 2,
        v_count: 2,
        control_points: vec![
            Point3::new(-1.0, 0.0, 0.0),
            Point3::new(-1.0, 0.0, 1.0),
            Point3::new(0.0, 0.0, 0.0),
            Point3::new(0.0, 0.0, 1.0),
        ],
        weights: Some(vec![2.0, 2.0, 3.0, 4.0]),
        u_periodic: false,
        v_periodic: false,
    };
    let second = NurbsSurface {
        u_degree: 1,
        v_degree: 1,
        u_knots: vec![0.0, 0.0, 1.0, 1.0],
        v_knots: vec![4.0, 4.0, 8.0, 8.0],
        u_count: 2,
        v_count: 2,
        control_points: vec![
            Point3::new(0.0, 0.0, 0.0),
            Point3::new(0.0, 0.0, 1.0),
            Point3::new(0.0, 1.0, 0.0),
            Point3::new(0.0, 1.0, 1.0),
        ],
        weights: Some(vec![6.0, 8.0, 8.0, 8.0]),
        u_periodic: false,
        v_periodic: false,
    };
    let shared =
        shared_extrusion_generator_curve(&first, &second).expect("shared generator boundary");
    let CurveGeometry::Nurbs(shared) = shared else {
        panic!("shared extrusion generator must retain its NURBS representation");
    };
    assert_eq!(shared.degree, 1);
    assert_eq!(shared.knots, first.v_knots);
    assert_eq!(
        shared.control_points,
        vec![Point3::new(0.0, 0.0, 0.0), Point3::new(0.0, 0.0, 1.0)]
    );
    assert_eq!(shared.weights, Some(vec![3.0, 4.0]));

    let mut reversed = second.clone();
    reversed.control_points.swap(0, 1);
    reversed.control_points.swap(2, 3);
    reversed.weights.as_mut().expect("weights").swap(0, 1);
    reversed.weights.as_mut().expect("weights").swap(2, 3);
    assert!(shared_extrusion_generator_curve(&first, &reversed).is_some());

    let mut same_side = second.clone();
    same_side.control_points[2] = Point3::new(-2.0, 0.0, 0.0);
    same_side.control_points[3] = Point3::new(-2.0, 0.0, 1.0);
    assert!(shared_extrusion_generator_curve(&first, &same_side).is_none());

    let mut periodic_transverse = second.clone();
    periodic_transverse.u_periodic = true;
    assert!(shared_extrusion_generator_curve(&first, &periodic_transverse).is_none());

    let mut different_boundary = second;
    different_boundary.control_points[1].x = 0.1;
    assert!(shared_extrusion_generator_curve(&first, &different_boundary).is_none());
}

#[test]
fn cubic_extrusion_plane_generator_requires_one_directrix_root() {
    let surface = NurbsSurface {
        u_degree: 3,
        v_degree: 1,
        u_knots: vec![0.0, 0.0, 0.0, 0.0, 1.0, 1.0, 1.0, 1.0],
        v_knots: vec![0.0, 0.0, 1.0, 1.0],
        u_count: 4,
        v_count: 2,
        control_points: [-1.0, -0.5, 0.5, 1.0]
            .into_iter()
            .flat_map(|x| [Point3::new(x, 0.0, 0.0), Point3::new(x, 0.0, 2.0)])
            .collect(),
        weights: Some(vec![1.0, 1.0, 2.0, 2.0, 3.0, 3.0, 4.0, 4.0]),
        u_periodic: false,
        v_periodic: false,
    };
    let generator = with_decode_ctx(|ctx| {
        cubic_extrusion_plane_generator_curve(
            ctx,
            &surface,
            PlaneEquation {
                origin: [0.0, 0.0, 0.0],
                normal: [1.0, 0.0, 0.0],
            },
        )
    })
    .expect("resource limits")
    .expect("unique directrix-plane root");
    let CurveGeometry::Nurbs(generator) = generator else {
        panic!("plane section generator must retain its NURBS representation");
    };
    assert_eq!(generator.degree, 1);
    assert_eq!(generator.knots, surface.v_knots);
    assert_eq!(generator.control_points.len(), 2);
    assert!(generator
        .control_points
        .iter()
        .all(|point| point.x.abs() <= 1e-8));
    assert_eq!(generator.control_points[0].z, 0.0);
    assert_eq!(generator.control_points[1].z, 2.0);
    let weights = generator.weights.expect("rational generator");
    assert_eq!(weights.len(), 2);
    assert!((weights[0] - weights[1]).abs() <= 1e-12);

    assert!(with_decode_ctx(|ctx| cubic_extrusion_plane_generator_curve(
        ctx,
        &surface,
        PlaneEquation {
            origin: [2.0, 0.0, 0.0],
            normal: [1.0, 0.0, 0.0],
        },
    ))
    .expect("resource limits")
    .is_none());
    assert!(with_decode_ctx(|ctx| cubic_extrusion_plane_generator_curve(
        ctx,
        &surface,
        PlaneEquation {
            origin: [0.0, 0.0, 1.0],
            normal: [0.0, 0.0, 1.0],
        },
    ))
    .expect("resource limits")
    .is_none());
    assert_eq!(
        cubic_unit_interval_roots(1.0, -1.5, 0.66, -0.08, 1e-12).len(),
        3
    );
    assert_eq!(
        cubic_unit_interval_roots(1.0, -1.8, 1.05, -0.2, 1e-12).len(),
        2
    );
}

#[test]
fn carrier_solver_accepts_two_carrier_tangent_vertices() {
    let plane = CarrierEquation::Plane(PlaneEquation {
        origin: [0.0, 0.0, 2.0],
        normal: [0.0, 0.0, 1.0],
    });
    let sphere = CarrierEquation::Sphere(SphereEquation {
        center: [0.0, 0.0, 0.0],
        ref_direction: [1.0, 0.0, 0.0],
        radius: 2.0,
    });
    assert_eq!(solve_carriers(&[plane, sphere]), Some([0.0, 0.0, 2.0]));

    let second_sphere = CarrierEquation::Sphere(SphereEquation {
        center: [5.0, 0.0, 0.0],
        ref_direction: [0.0, 1.0, 0.0],
        radius: 3.0,
    });
    assert_eq!(
        solve_carriers(&[sphere, second_sphere]),
        Some([2.0, 0.0, 0.0])
    );

    let secant = CarrierEquation::Sphere(SphereEquation {
        center: [3.0, 0.0, 0.0],
        ref_direction: [0.0, 1.0, 0.0],
        radius: 2.0,
    });
    assert_eq!(solve_carriers(&[sphere, secant]), None);
}

#[test]
fn coaxial_cone_torus_components_support_edges_and_vertices() {
    let cone = CarrierEquation::Cone(ConeEquation {
        origin: [0.0, 0.0, 0.0],
        axis: [0.0, 0.0, 1.0],
        ref_direction: [1.0, 0.0, 0.0],
        radius: 2.0,
        ratio: 1.0,
        half_angle: std::f64::consts::FRAC_PI_4,
    });
    let secant_torus = CarrierEquation::Torus(TorusEquation {
        center: [0.0, 0.0, 0.0],
        axis: [0.0, 0.0, 1.0],
        ref_direction: [1.0, 0.0, 0.0],
        major_radius: 3.0,
        minor_radius: 2.0,
    });
    let candidates = coaxial_cone_torus_circle_candidates(cone, secant_torus);
    assert_eq!(candidates.len(), 2);
    assert!(resolve_curve_candidates(
        coaxial_cone_torus_circle_candidates(cone, secant_torus),
        None,
    )
    .is_none());
    let upper_parameter = f64::midpoint(1.0, 7.0_f64.sqrt());
    let upper_radius = 2.0 + upper_parameter;
    assert!(matches!(
        select_unique_curve_candidate(
            candidates,
            [
                [upper_radius, 0.0, upper_parameter],
                [0.0, upper_radius, upper_parameter],
            ],
        ),
        Some((CurveGeometry::Circle { center, radius, .. }, "coaxial_cone_torus_circle"))
            if (center.z - upper_parameter).abs() < 1e-12
                && (radius - upper_radius).abs() < 1e-12
    ));
    let tangent_plane = CarrierEquation::Plane(PlaneEquation {
        origin: [3.0 + 7.0_f64.sqrt(), 0.0, 0.0],
        normal: [1.0, 0.0, 1.0],
    });
    let vertex = solve_carriers(&[cone, secant_torus, tangent_plane])
        .expect("unique cone-torus circle tangent");
    assert!((vertex[0] - upper_radius).abs() < 1e-12);
    assert!(vertex[1].abs() < 1e-12);
    assert!((vertex[2] - upper_parameter).abs() < 1e-12);

    let tangent_torus = CarrierEquation::Torus(TorusEquation {
        center: [0.0, 0.0, 0.0],
        axis: [0.0, 0.0, 1.0],
        ref_direction: [1.0, 0.0, 0.0],
        major_radius: 5.0,
        minor_radius: 3.0 / 2.0_f64.sqrt(),
    });
    let tangent_candidates = coaxial_cone_torus_circle_candidates(cone, tangent_torus);
    assert!(matches!(
        tangent_candidates.as_slice(),
        [(CurveGeometry::Circle { center, radius, .. }, "coaxial_cone_torus_circle")]
            if (center.z - 1.5).abs() < 1e-12 && (radius - 3.5).abs() < 1e-12
    ));
    assert!(matches!(
        resolve_curve_candidates(tangent_candidates, None),
        Some((CurveGeometry::Circle { center, radius, .. }, "coaxial_cone_torus_circle"))
            if (center.z - 1.5).abs() < 1e-12 && (radius - 3.5).abs() < 1e-12
    ));
    assert!(resolve_curve_candidates(
        coaxial_cone_torus_circle_candidates(cone, tangent_torus),
        Some([[0.0, 0.0, 0.0], [1.0, 0.0, 0.0]]),
    )
    .is_none());
    let shifted_torus = CarrierEquation::Torus(TorusEquation {
        center: [1.0, 0.0, 0.0],
        axis: [0.0, 0.0, 1.0],
        ref_direction: [1.0, 0.0, 0.0],
        major_radius: 3.0,
        minor_radius: 2.0,
    });
    assert!(coaxial_cone_torus_circle_candidates(cone, shifted_torus).is_empty());
}

#[test]
fn axis_containing_plane_torus_components_support_edges_and_vertices() {
    let plane = CarrierEquation::Plane(PlaneEquation {
        origin: [0.0, 0.0, 0.0],
        normal: [0.0, 1.0, 0.0],
    });
    let torus = CarrierEquation::Torus(TorusEquation {
        center: [0.0, 0.0, 0.0],
        axis: [0.0, 0.0, 1.0],
        ref_direction: [1.0, 0.0, 0.0],
        major_radius: 3.0,
        minor_radius: 1.0,
    });
    let candidates = axis_containing_plane_torus_circle_candidates(plane, torus);
    assert_eq!(candidates.len(), 2);
    assert!(resolve_curve_candidates(candidates.clone(), None).is_none());
    assert!(matches!(
        select_unique_curve_candidate(candidates, [[4.0, 0.0, 0.0], [3.0, 0.0, 1.0]]),
        Some((CurveGeometry::Circle { center, radius, .. }, "axis_containing_plane_torus_meridian_circle"))
            if (center.x - 3.0).abs() < 1e-12
                && center.y.abs() < 1e-12
                && center.z.abs() < 1e-12
                && (radius - 1.0).abs() < 1e-12
    ));

    let tangent_plane = CarrierEquation::Plane(PlaneEquation {
        origin: [4.0, 0.0, 0.0],
        normal: [1.0, 0.0, 0.0],
    });
    assert_eq!(
        solve_carriers(&[plane, torus, tangent_plane]),
        Some([4.0, 0.0, 0.0])
    );

    let offset_plane = CarrierEquation::Plane(PlaneEquation {
        origin: [0.0, 0.5, 0.0],
        normal: [0.0, 1.0, 0.0],
    });
    assert!(axis_containing_plane_torus_circle_candidates(offset_plane, torus).is_empty());
}

#[test]
fn coaxial_cone_components_respect_axis_orientation_and_coincidence() {
    let first = CarrierEquation::Cone(ConeEquation {
        origin: [0.0, 0.0, 0.0],
        axis: [0.0, 0.0, 1.0],
        ref_direction: [1.0, 0.0, 0.0],
        radius: 2.0,
        ratio: 1.0,
        half_angle: std::f64::consts::FRAC_PI_4,
    });
    let second = CarrierEquation::Cone(ConeEquation {
        origin: [0.0, 0.0, 0.0],
        axis: [0.0, 0.0, 1.0],
        ref_direction: [1.0, 0.0, 0.0],
        radius: 4.0,
        ratio: 1.0,
        half_angle: 0.5_f64.atan(),
    });
    let candidates = coaxial_cones_section_candidates(first, second);
    assert_eq!(candidates.len(), 2);
    assert!(matches!(
        select_unique_curve_candidate(candidates, [[6.0, 0.0, 4.0], [0.0, 6.0, 4.0]]),
        Some((CurveGeometry::Circle { center, radius, .. }, "coaxial_cones_circle"))
            if (center.z - 4.0).abs() < 1e-12 && (radius - 6.0).abs() < 1e-12
    ));
    let tangent_plane = CarrierEquation::Plane(PlaneEquation {
        origin: [10.0, 0.0, 0.0],
        normal: [1.0, 0.0, 1.0],
    });
    let vertex = solve_carriers(&[first, second, tangent_plane])
        .expect("unique coaxial-cone circle tangent");
    assert!((vertex[0] - 6.0).abs() < 1e-12);
    assert!(vertex[1].abs() < 1e-12);
    assert!((vertex[2] - 4.0).abs() < 1e-12);

    let reversed = CarrierEquation::Cone(ConeEquation {
        origin: [0.0, 0.0, 0.0],
        axis: [0.0, 0.0, -1.0],
        ref_direction: [1.0, 0.0, 0.0],
        radius: 4.0,
        ratio: 1.0,
        half_angle: 0.5_f64.atan(),
    });
    let reversed_candidates = coaxial_cones_section_candidates(first, reversed);
    assert_eq!(reversed_candidates.len(), 2);
    assert!(reversed_candidates.iter().any(|(geometry, _)| matches!(
        geometry,
        CurveGeometry::Circle { center, radius, .. }
            if (center.z - 4.0 / 3.0).abs() < 1e-12
                && (radius - 10.0 / 3.0).abs() < 1e-12
    )));
    assert!(coaxial_cones_section_candidates(first, first).is_empty());
    let shifted = CarrierEquation::Cone(ConeEquation {
        origin: [1.0, 0.0, 0.0],
        axis: [0.0, 0.0, 1.0],
        ref_direction: [1.0, 0.0, 0.0],
        radius: 4.0,
        ratio: 1.0,
        half_angle: 0.5_f64.atan(),
    });
    assert!(coaxial_cones_section_candidates(first, shifted).is_empty());

    let CarrierEquation::Cone(mut elliptical_first_equation) = first else {
        unreachable!();
    };
    elliptical_first_equation.ratio = 0.5;
    let elliptical_first = CarrierEquation::Cone(elliptical_first_equation);
    let CarrierEquation::Cone(mut elliptical_second_equation) = second else {
        unreachable!();
    };
    elliptical_second_equation.ratio = 0.5;
    let elliptical_second = CarrierEquation::Cone(elliptical_second_equation);
    let candidates = coaxial_cones_section_candidates(elliptical_first, elliptical_second);
    assert_eq!(candidates.len(), 2);
    let selected = select_unique_curve_candidate(candidates, [[6.0, 0.0, 4.0], [0.0, 3.0, 4.0]])
        .expect("selected coaxial elliptical-cone section");
    assert!(matches!(
        &selected,
        (
            CurveGeometry::Ellipse {
                center,
                major_radius,
                minor_radius,
                ..
            },
            "coaxial_cones_ellipse"
        ) if (center.z - 4.0).abs() < 1e-12
            && (major_radius - 6.0).abs() < 1e-12
            && (minor_radius - 3.0).abs() < 1e-12
    ));
    for parameter in [-1.0, 0.0, 1.0] {
        let point = cadmpeg_ir::eval::curve_point(&selected.0, parameter)
            .expect("coaxial cone ellipse point");
        let point = [point.x, point.y, point.z];
        assert!(point_on_carrier(point, elliptical_first));
        assert!(point_on_carrier(point, elliptical_second));
    }
    elliptical_second_equation.ref_direction = [0.0, 1.0, 0.0];
    let incompatible_frame = CarrierEquation::Cone(elliptical_second_equation);
    assert!(coaxial_cones_section_candidates(elliptical_first, incompatible_frame).is_empty());

    elliptical_second_equation.ratio = 2.0;
    elliptical_second_equation.half_angle = 0.25_f64.atan();
    let reciprocal_swapped = CarrierEquation::Cone(elliptical_second_equation);
    let candidates = coaxial_cones_section_candidates(elliptical_first, reciprocal_swapped);
    assert_eq!(candidates.len(), 2);
    let selected = select_unique_curve_candidate(candidates, [[14.0, 0.0, 12.0], [0.0, 7.0, 12.0]])
        .expect("selected reciprocal-frame cone section");
    assert!(matches!(
        &selected,
        (
            CurveGeometry::Ellipse {
                center,
                major_radius,
                minor_radius,
                ..
            },
            "coaxial_cones_ellipse"
        ) if (center.z - 12.0).abs() < 1e-12
            && (major_radius - 14.0).abs() < 1e-12
            && (minor_radius - 7.0).abs() < 1e-12
    ));
    for parameter in [-1.0, 0.0, 1.0] {
        let point = cadmpeg_ir::eval::curve_point(&selected.0, parameter)
            .expect("reciprocal-frame section point");
        let point = [point.x, point.y, point.z];
        assert!(point_on_carrier(point, elliptical_first));
        assert!(point_on_carrier(point, reciprocal_swapped));
    }
}
