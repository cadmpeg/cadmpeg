// SPDX-License-Identifier: Apache-2.0
//! Tests: numbered intersect.

use super::parameter_slot;
use crate::decode::analytic::{ConeEquation, PlaneEquation};
use crate::decode::build::has_transferred_geometry;
use crate::decode::feature_history::{
    add_surface_prototype_feature_dependencies, chamfer_constant_distance,
    equal_distance_chamfer_setback, feature_edge_selection, feature_entity_dependencies,
    feature_generated_dependencies, feature_output_surface_dependencies, feature_result_edge_ids,
    feature_result_surface_ids, feature_result_topology, generated_curve_edge_refs,
    generated_surface_face_refs, geometry_generator_features, knit_class_100_operand_entity_ids,
    knit_operand_surface_ids, model_feature_ids, native_feature_dependency_ids,
    profile_segment_ids, reconciled_dependencies, surface_intersect_feature_definition,
    surface_merge_entity_dependencies, surface_merge_quilt_ids, GeometryGeneratorFeature,
};
use crate::decode::holes::{
    cylinder_from_complementary_outline_bounds, extrusion_extent_and_direction, hole_placement,
};
use crate::decode::sketch::{
    normalized, section_linear_distance_coordinate, solve_section_coordinate_equations,
    solve_unsigned_dimension_coordinates, SectionCoordinateEquation,
};
use crate::decode::surfaces::fc05_model_frame;
use crate::decode::sweep::{
    extruded_section_line, feature_outline_planes, feature_plane_equations,
    placed_tabulated_cylinder_directrix, revolution_boundary_pcurve, revolved_section_circle,
    revolved_section_surface, signed_unit_chart,
};
use cadmpeg_ir::document::CadIr;
use cadmpeg_ir::features::{
    Angle, BodySelection, EdgeSelection, ExtrudeExtent, ExtrudeSide, FaceSelection, Feature,
    FeatureDefinition as IrFeatureDefinition, FeatureId as IrFeatureId, GeneratedEdgeRef,
    GeneratedFaceRef, Length, RadiusSpec, RevolutionAxis, Termination,
};
use cadmpeg_ir::geometry::{
    Curve, CurveGeometry, ProceduralSurface, ProceduralSurfaceDefinition, Surface, SurfaceGeometry,
};
use cadmpeg_ir::ids::{CurveId, EdgeId, ProceduralSurfaceId, SurfaceId};
use cadmpeg_ir::math::{Point2, Point3, Vector3};
use cadmpeg_ir::sketches::{SketchEntityId, SketchEntityUse, SketchGeometry};
use cadmpeg_ir::units::Units;
use std::collections::{BTreeMap, BTreeSet};

#[test]
fn numbered_intersect_name_identifies_section_shape_feature() {
    let table = || crate::feature::FeatureEntityTable {
        feature_id: Some(50),
        table_class_id: 29,
        entry_ids: vec![61, 75],
        entries: Vec::new(),
        surface_ids: vec![61, 75],
        non_surface_entity_ids: Vec::new(),
        offset: 0,
    };
    let surface = |id, feature_id| crate::surface::SurfaceRow {
        id,
        type_byte: crate::surface::SurfaceKind::Plane.canonical_type_byte(),
        kind: crate::surface::SurfaceKind::Plane,
        feature_id,
        reversed: false,
        boundary_type: 0,
        next_surface: 0,
        offset: 0,
    };
    let valid_scan = || {
        let mut scan = crate::container::scan_bytes(Vec::new());
        scan.features.entity_tables.push(table());
        scan.surfaces
            .rows
            .extend([surface(61, 50), surface(75, 50)]);
        scan
    };

    let mut scan = crate::container::scan_bytes(Vec::new());
    assert_eq!(
        surface_intersect_feature_definition(&scan, 50, "Intersect 1"),
        None
    );
    scan = valid_scan();
    assert_eq!(
        surface_intersect_feature_definition(&scan, 50, "Intersect 1"),
        Some(IrFeatureDefinition::SectionShape {
            first: BodySelection::Unresolved,
            second: BodySelection::Unresolved,
            approximate: None,
        })
    );
    scan.surfaces.rows.pop();
    assert_eq!(
        surface_intersect_feature_definition(&scan, 50, "Intersect 1"),
        None
    );

    let mut duplicate_surface_row = valid_scan();
    duplicate_surface_row.surfaces.rows.push(surface(61, 50));
    assert_eq!(
        surface_intersect_feature_definition(&duplicate_surface_row, 50, "Intersect 1"),
        None
    );

    let mut foreign_surface = valid_scan();
    foreign_surface.surfaces.rows[1].feature_id = 51;
    assert_eq!(
        surface_intersect_feature_definition(&foreign_surface, 50, "Intersect 1"),
        None
    );

    let mut duplicate_surface_id = valid_scan();
    duplicate_surface_id.features.entity_tables[0].surface_ids = vec![61, 61];
    assert_eq!(
        surface_intersect_feature_definition(&duplicate_surface_id, 50, "Intersect 1"),
        None
    );

    let mut multiple_materialized_tables = valid_scan();
    multiple_materialized_tables
        .features
        .entity_tables
        .push(table());
    assert_eq!(
        surface_intersect_feature_definition(&multiple_materialized_tables, 50, "Intersect 1"),
        None
    );

    assert_eq!(
        surface_intersect_feature_definition(&scan, 50, "Intersect"),
        None
    );
    assert_eq!(
        surface_intersect_feature_definition(&scan, 50, "Intersect copy"),
        None
    );
}

#[test]
fn equal_distance_chamfer_setback_uses_nearest_forward_parallel_support() {
    let cone = |origin, axis| ConeEquation {
        origin,
        axis,
        ref_direction: [0.0, 0.0, 1.0],
        radius: 0.0,
        ratio: 1.0,
        half_angle: std::f64::consts::FRAC_PI_4,
    };
    let cones = [
        cone([10.5, 0.0, 0.0], [-1.0, 0.0, 0.0]),
        cone([-10.5, 0.0, 0.0], [1.0, 0.0, 0.0]),
    ];
    let supports = [
        PlaneEquation {
            origin: [10.0, 0.0, 0.0],
            normal: [1.0, 0.0, 0.0],
        },
        PlaneEquation {
            origin: [-10.0, 0.0, 0.0],
            normal: [1.0, 0.0, 0.0],
        },
        PlaneEquation {
            origin: [0.0, 2.0, 0.0],
            normal: [0.0, 1.0, 0.0],
        },
    ];

    assert_eq!(equal_distance_chamfer_setback(&cones, &supports), Some(0.5));

    let mut non_equal = cones;
    non_equal[1].origin[0] = -10.25;
    assert_eq!(equal_distance_chamfer_setback(&non_equal, &supports), None);
}

#[test]
fn signed_distance_without_a_spanning_line_requires_equal_endpoint_coordinate() {
    let line = |external_id, point_ids| crate::feature::FeatureSegment {
        kind: crate::feature::FeatureSegmentKind::Line,
        directions: [None; 3],
        point_ids,
        center_id: None,
        arc_orientation: None,
        vertical_horizontal: Some(0),
        radius_ref: None,
        radius2_ref: None,
        external_id,
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
            declared_count: 2,
            has_elided_prototype: false,
            entity_ref: None,
            rows: vec![line(10, [1, 3]), line(11, [2, 4])],
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
    let coordinates = BTreeMap::from([(1, [Some(0.0), Some(1.0)]), (2, [Some(2.0), Some(3.0)])]);

    assert_eq!(
        section_linear_distance_coordinate(
            &definition,
            &segments,
            1,
            2,
            &coordinates,
            &[],
            &BTreeSet::new(),
        ),
        None
    );

    let mut endpoint_carriers = definition.clone();
    let endpoint_segments = endpoint_carriers.segments.as_mut().expect("segments");
    endpoint_segments.rows.clear();
    endpoint_segments
        .reference_line_rows
        .push(crate::feature::FeatureReferenceLineSegment {
            directions: [None; 3],
            point_ids: [Some(1), Some(3)],
            vertical_horizontal: None,
            external_id: 20,
            offset: 0,
        });
    endpoint_segments
        .bounded_curve_rows
        .push(crate::feature::FeatureBoundedCurveSegment {
            directions: [None; 3],
            point_ids: [2, 4],
            center_id: None,
            arc_orientation: None,
            vertical_horizontal: None,
            radius_ref: None,
            radius2_ref: None,
            external_id: 21,
            offset: 0,
        });
    assert_eq!(
        section_linear_distance_coordinate(
            &endpoint_carriers,
            &[],
            1,
            2,
            &BTreeMap::from([(1, [Some(0.0), Some(1.0)]), (2, [Some(2.0), Some(1.0)])]),
            &[],
            &BTreeSet::new(),
        ),
        Some(0)
    );

    let mut centered_endpoint_carrier = definition;
    let centered_segments = centered_endpoint_carrier
        .segments
        .as_mut()
        .expect("segments");
    centered_segments.rows.clear();
    centered_segments
        .centered_line_rows
        .push(crate::feature::FeatureCenteredLineSegment {
            center_id: 2,
            external_id: 22,
            offset: 0,
        });
    assert_eq!(
        section_linear_distance_coordinate(
            &centered_endpoint_carrier,
            &[],
            0,
            1,
            &BTreeMap::from([(0, [Some(0.0), Some(1.0)]), (1, [Some(2.0), Some(1.0)])]),
            &[],
            &BTreeSet::new(),
        ),
        Some(0)
    );
}

#[test]
fn chamfer_requires_every_affected_support_plane_to_be_placed() {
    let mut scan = crate::container::scan_bytes(Vec::new());
    scan.surfaces.rows.push(crate::surface::SurfaceRow {
        id: 10,
        type_byte: crate::surface::SurfaceKind::Cone.canonical_type_byte(),
        kind: crate::surface::SurfaceKind::Cone,
        feature_id: 914,
        reversed: false,
        boundary_type: 0,
        next_surface: 0,
        offset: 10,
    });
    scan.surfaces
        .parameters
        .push(crate::surface::SurfaceParameterRecord {
            surface_id: 10,
            body: Vec::new(),
            scalar_values: Vec::new(),
            scalar_tokens: Vec::new(),
            opaque_spans: Vec::new(),
            scalar_frames: Vec::new(),
            terminal_scalar_frame: None,
            tabulated_cylinder_frame: None,
            positional_cylinder_frame: None,
            split_cylinder_outline_bounds: None,
            positional_cone_frame: Some(crate::surface::PositionalConeFrame {
                apex: [0.5, 0.0, 0.0],
                axis: [-1.0, 0.0, 0.0],
                ref_direction: [0.0, 1.0, 0.0],
                half_angle: std::f64::consts::FRAC_PI_4,
            }),
            positional_torus_frame: None,
            boundary: crate::surface::SurfaceBodyBoundary::CompoundClose,
            offset: 10,
            body_offset: 11,
        });
    scan.surfaces.rows.push(crate::surface::SurfaceRow {
        id: 31,
        type_byte: crate::surface::SurfaceKind::Plane.canonical_type_byte(),
        kind: crate::surface::SurfaceKind::Plane,
        feature_id: 3,
        reversed: false,
        boundary_type: 0,
        next_surface: 0,
        offset: 31,
    });
    scan.surfaces.rows.push(crate::surface::SurfaceRow {
        id: 98,
        type_byte: crate::surface::SurfaceKind::Cylinder.canonical_type_byte(),
        kind: crate::surface::SurfaceKind::Cylinder,
        feature_id: 3,
        reversed: false,
        boundary_type: 0,
        next_surface: 0,
        offset: 98,
    });
    scan.planes
        .positional_frames
        .push(crate::surface::OutlinePlane {
            surface_id: 31,
            origin: [0.0, 0.0, 0.0],
            normal: [1.0, 0.0, 0.0],
            u_axis: [0.0, 1.0, 0.0],
            offset: 31,
        });
    scan.features
        .affected_ids
        .push(crate::feature::FeatureAffectedIds {
            feature_id: 914,
            kind: crate::feature::AffectedIdKind::Geometry,
            ids: vec![31],
            offset: 0,
        });

    assert_eq!(chamfer_constant_distance(&scan, 914), Some(0.5));
    scan.features.affected_ids[0].ids.extend([98, 99]);
    assert_eq!(chamfer_constant_distance(&scan, 914), Some(0.5));

    scan.features.affected_ids[0].ids.push(32);
    scan.surfaces.rows.push(crate::surface::SurfaceRow {
        id: 32,
        type_byte: crate::surface::SurfaceKind::Plane.canonical_type_byte(),
        kind: crate::surface::SurfaceKind::Plane,
        feature_id: 3,
        reversed: false,
        boundary_type: 0,
        next_surface: 0,
        offset: 32,
    });
    assert_eq!(chamfer_constant_distance(&scan, 914), None);
}

#[test]
fn linear_plane_extent_requires_complete_generated_plane_evidence() {
    let row = |id| crate::surface::SurfaceRow {
        id,
        type_byte: crate::surface::SurfaceKind::Plane.canonical_type_byte(),
        kind: crate::surface::SurfaceKind::Plane,
        feature_id: 917,
        reversed: false,
        boundary_type: 0,
        next_surface: 0,
        offset: id as usize,
    };
    let plane = |id, z| crate::surface::OutlinePlane {
        surface_id: id,
        origin: [0.0, 0.0, z],
        normal: [0.0, 0.0, 1.0],
        u_axis: [1.0, 0.0, 0.0],
        offset: id as usize,
    };
    let mut scan = crate::container::scan_bytes(Vec::new());
    scan.surfaces.rows.extend([row(31), row(32)]);
    scan.planes.outlines.push(plane(31, 2.0));

    assert!(feature_plane_equations(&scan, 917).is_none());

    scan.planes.outlines.push(plane(32, 8.0));
    assert_eq!(
        feature_plane_equations(&scan, 917).and_then(|planes| {
            extrusion_extent_and_direction([0.0, 0.0, 0.0], [0.0, 0.0, 1.0], planes)
        }),
        Some((
            ExtrudeExtent::OneSided {
                side: ExtrudeSide {
                    termination: Termination::Blind {
                        length: Length(8.0),
                    },
                    draft: None,
                    offset: None,
                },
            },
            [0.0, 0.0, 1.0],
        ))
    );
}

#[test]
fn hole_outline_placement_requires_complete_feature_plane_evidence() {
    let row = |id| crate::surface::SurfaceRow {
        id,
        type_byte: crate::surface::SurfaceKind::Plane.canonical_type_byte(),
        kind: crate::surface::SurfaceKind::Plane,
        feature_id: 911,
        reversed: false,
        boundary_type: 0,
        next_surface: 0,
        offset: id as usize,
    };
    let plane = |id, z| crate::surface::OutlinePlane {
        surface_id: id,
        origin: [0.0, 0.0, z],
        normal: [0.0, 0.0, 1.0],
        u_axis: [1.0, 0.0, 0.0],
        offset: id as usize,
    };
    let mut scan = crate::container::scan_bytes(Vec::new());
    scan.surfaces.rows.extend([row(31), row(32), row(33)]);
    scan.planes
        .outlines
        .extend([plane(31, 0.0), plane(32, 5.0)]);

    assert!(feature_outline_planes(&scan, 911).is_none());

    scan.planes.outlines.push(plane(33, 10.0));
    assert_eq!(
        feature_outline_planes(&scan, 911).map(|planes| planes.len()),
        Some(3)
    );
    assert!(hole_placement(feature_outline_planes(&scan, 911).expect("complete planes")).is_none());

    scan.planes.outlines.push(plane(33, 10.0));
    assert!(feature_outline_planes(&scan, 911).is_none());
}

#[test]
fn hole_outline_placement_preserves_stored_plane_order() {
    let row = |id| crate::surface::SurfaceRow {
        id,
        type_byte: crate::surface::SurfaceKind::Plane.canonical_type_byte(),
        kind: crate::surface::SurfaceKind::Plane,
        feature_id: 911,
        reversed: false,
        boundary_type: 0,
        next_surface: 0,
        offset: id as usize,
    };
    let plane = |id, z| crate::surface::OutlinePlane {
        surface_id: id,
        origin: [0.0, 0.0, z],
        normal: [0.0, 0.0, 1.0],
        u_axis: [1.0, 0.0, 0.0],
        offset: id as usize,
    };
    let mut scan = crate::container::scan_bytes(Vec::new());
    scan.surfaces.rows.extend([row(902), row(701)]);
    scan.planes
        .outlines
        .extend([plane(902, 0.0), plane(701, 6.5)]);

    assert_eq!(
        feature_outline_planes(&scan, 911),
        Some(vec![
            (902, [0.0, 0.0, 0.0], [0.0, 0.0, 1.0]),
            (701, [0.0, 0.0, 6.5], [0.0, 0.0, 1.0]),
        ])
    );
    assert_eq!(
        hole_placement(feature_outline_planes(&scan, 911).expect("complete planes")),
        Some((
            902,
            [0.0, 0.0, 1.0],
            Termination::Blind {
                length: Length(6.5),
            },
        ))
    );
}

#[test]
fn surface_prototype_dependencies_point_from_consumers_to_unique_producers() {
    let mut dependencies = BTreeMap::new();
    add_surface_prototype_feature_dependencies(&mut dependencies, 40, &[0, 40, 286, 286, 1111]);
    add_surface_prototype_feature_dependencies(&mut dependencies, 41, &[286]);

    assert_eq!(
        dependencies,
        BTreeMap::from([(286, vec![40, 41]), (1111, vec![40])])
    );
}

#[test]
fn section_coordinate_system_solves_coupled_equations_and_withholds_derivations_on_conflict() {
    let mut sum = SectionCoordinateEquation::default();
    sum.add_point(1, 0, 1.0);
    sum.add_point(2, 0, 1.0);
    sum.rhs = 10.0;
    let mut difference = SectionCoordinateEquation::default();
    difference.add_point(1, 0, 1.0);
    difference.add_point(2, 0, -1.0);
    difference.rhs = 2.0;
    assert_eq!(
        solve_section_coordinate_equations(
            &[
                sum,
                difference,
                SectionCoordinateEquation::point_value(1, 1, 3.0),
                SectionCoordinateEquation::point_value(2, 1, 4.0),
            ],
            &BTreeMap::new(),
        ),
        BTreeMap::from([(1, [Some(6.0), Some(3.0)]), (2, [Some(4.0), Some(4.0)]),])
    );

    let stored = BTreeMap::from([((1, 0), 1.0), ((1, 1), 3.0)]);
    assert_eq!(
        solve_section_coordinate_equations(
            &[
                SectionCoordinateEquation::point_value(1, 0, 1.0),
                SectionCoordinateEquation::point_value(1, 0, 2.0),
                SectionCoordinateEquation::point_value(1, 1, 3.0),
            ],
            &stored,
        ),
        BTreeMap::from([(1, [Some(1.0), Some(3.0)])])
    );
    let stored = BTreeMap::from([((1, 0), 1.0), ((1, 1), 3.0), ((2, 0), 2.0), ((2, 1), 4.0)]);
    assert_eq!(
        solve_section_coordinate_equations(
            &[
                SectionCoordinateEquation::point_value(1, 0, 1.0),
                SectionCoordinateEquation::point_value(1, 1, 3.0),
                SectionCoordinateEquation::point_value(2, 0, 2.0),
                SectionCoordinateEquation::point_value(2, 1, 4.0),
                SectionCoordinateEquation::point_difference(1, 3, 0, 0.0),
                SectionCoordinateEquation::point_difference(2, 3, 0, 0.0),
                SectionCoordinateEquation::point_value(3, 1, 5.0),
            ],
            &stored,
        ),
        BTreeMap::from([
            (1, [Some(1.0), Some(3.0)]),
            (2, [Some(2.0), Some(4.0)]),
            (3, [None, Some(5.0)]),
        ])
    );
    assert_eq!(
        solve_section_coordinate_equations(
            &[
                SectionCoordinateEquation::point_value(3, 0, 1.0e12),
                SectionCoordinateEquation::point_value(3, 1, -1.0e12),
            ],
            &BTreeMap::new(),
        ),
        BTreeMap::from([(3, [Some(1.0e12), Some(-1.0e12)])])
    );
    assert_eq!(
        solve_section_coordinate_equations(
            &[SectionCoordinateEquation::point_value(4, 0, 7.0)],
            &BTreeMap::new(),
        ),
        BTreeMap::from([(4, [Some(7.0), None])])
    );
}

#[test]
fn unsigned_dimension_signs_are_reconciled_only_when_unique() {
    let equations = [
        SectionCoordinateEquation::point_value(1, 0, 0.0),
        SectionCoordinateEquation::point_value(2, 0, 10.0),
    ];
    let stored = BTreeMap::from([((1, 0), 0.0), ((2, 0), 10.0)]);
    assert_eq!(
        solve_unsigned_dimension_coordinates(
            &equations,
            &stored,
            &[(1, 3, 0, 3.0), (3, 2, 0, 7.0)],
        ),
        BTreeMap::from([((3, 0), 3.0)])
    );
    assert_eq!(
        solve_unsigned_dimension_coordinates(
            &[SectionCoordinateEquation::point_value(1, 0, 0.0)],
            &BTreeMap::from([((1, 0), 0.0)]),
            &[(1, 2, 0, 3.0)],
        ),
        BTreeMap::new()
    );
}

#[test]
fn normalization_rejects_overflowed_finite_vectors() {
    assert_eq!(normalized([f64::MAX, f64::MAX, 0.0]), None);
    assert_eq!(normalized([3.0, 4.0, 0.0]), Some([0.6, 0.8, 0.0]));
}

#[test]
fn dependency_reconciliation_preserves_typed_history_edges() {
    let owner = IrFeatureId("creo:model:feature#40".to_string());
    let sketch = IrFeatureId("creo:model:sketch_feature#917".to_string());
    let parent = IrFeatureId("creo:model:feature#3".to_string());
    let missing = IrFeatureId("creo:model:feature#999".to_string());
    let emitted = [owner.clone(), sketch.clone(), parent.clone()]
        .into_iter()
        .collect();

    assert_eq!(
        reconciled_dependencies(
            &owner,
            &[sketch.clone(), missing],
            [parent.clone(), sketch.clone(), owner.clone()],
            &emitted,
        ),
        vec![sketch, parent]
    );
}

#[test]
fn class_100_entity_reference_depends_on_its_unique_generator() {
    let entry = |entity_id, class_id, source_entity_id| crate::feature::FeatureEntityTableEntry {
        entity_id,
        class_id,
        source_entity_id,
        related_entity_id: None,
        related_entity_state: None,
        prefixed: true,
        offset: 0,
        end_offset: 0,
    };
    let table = |feature_id: u32,
                 table_class_id: u32,
                 entries: Vec<crate::feature::FeatureEntityTableEntry>| {
        crate::feature::FeatureEntityTable {
            feature_id: Some(feature_id),
            table_class_id,
            entry_ids: entries.iter().map(|entry| entry.entity_id).collect(),
            entries,
            surface_ids: Vec::new(),
            non_surface_entity_ids: Vec::new(),
            offset: 0,
        }
    };
    let producer = table(175, 67, vec![entry(192, 200, Some(175))]);
    let consumer = table(416, 100, vec![entry(192, 98, None)]);

    assert_eq!(
        feature_entity_dependencies(&[producer.clone(), consumer.clone()], 416),
        [175]
    );
    let duplicate_owned_producer = table(
        175,
        67,
        vec![entry(192, 200, Some(175)), entry(192, 200, Some(175))],
    );
    assert_eq!(
        feature_entity_dependencies(&[duplicate_owned_producer.clone(), consumer.clone()], 416),
        [175]
    );
    assert_eq!(
        knit_class_100_operand_entity_ids(416, &[duplicate_owned_producer, consumer.clone()]),
        None
    );
    assert_eq!(
        knit_class_100_operand_entity_ids(416, &[producer.clone(), consumer.clone()]),
        Some(vec![192])
    );
    let source_missing_entry = table(175, 67, vec![entry(192, 200, None)]);
    assert_eq!(
        knit_class_100_operand_entity_ids(416, &[source_missing_entry.clone(), consumer.clone()]),
        None
    );
    assert_eq!(
        feature_entity_dependencies(&[source_missing_entry, consumer.clone()], 416),
        [175]
    );
    assert_eq!(
        knit_class_100_operand_entity_ids(416, &[consumer.clone(), producer.clone()]),
        None
    );
    assert_eq!(
        feature_entity_dependencies(&[consumer.clone(), producer.clone()], 416),
        [175]
    );
    assert_eq!(
        native_feature_dependency_ids(
            &[],
            &[],
            &[producer.clone(), consumer.clone()],
            &[],
            &[],
            416,
            &[40, 40],
        ),
        [40, 175]
    );
    let conflicting = table(312, 67, vec![entry(192, 200, Some(312))]);
    assert!(feature_entity_dependencies(
        &[producer.clone(), conflicting.clone(), consumer.clone()],
        416
    )
    .is_empty());
    assert_eq!(
        knit_class_100_operand_entity_ids(
            416,
            &[producer.clone(), conflicting.clone(), consumer.clone()]
        ),
        None
    );
    assert!(native_feature_dependency_ids(
        &[],
        &[],
        &[producer.clone(), conflicting, consumer],
        &[],
        &[],
        416,
        &[],
    )
    .is_empty());
    let second_producer = table(176, 67, vec![entry(193, 200, Some(176))]);
    let mixed_consumer = table(
        419,
        100,
        vec![
            entry(192, 98, None),
            entry(193, 98, None),
            entry(194, 98, None),
        ],
    );
    assert_eq!(
        feature_entity_dependencies(
            &[producer.clone(), second_producer, mixed_consumer.clone()],
            419,
        ),
        [175, 176]
    );
    assert_eq!(
        knit_class_100_operand_entity_ids(419, &[producer.clone(), mixed_consumer]),
        None
    );
    let missing = table(417, 100, vec![entry(193, 98, None)]);
    assert_eq!(knit_class_100_operand_entity_ids(417, &[missing]), None);
    let duplicate = table(418, 100, vec![entry(192, 98, None), entry(192, 98, None)]);
    assert_eq!(
        knit_class_100_operand_entity_ids(418, &[producer.clone(), duplicate]),
        None
    );
    let self_reference = table(175, 100, vec![entry(192, 98, None)]);
    assert_eq!(
        knit_class_100_operand_entity_ids(175, &[producer, self_reference]),
        None
    );
}

#[test]
fn owned_output_entity_depends_on_its_prior_surface_target() {
    let entry = |entity_id, class_id, source_entity_id| crate::feature::FeatureEntityTableEntry {
        entity_id,
        class_id,
        source_entity_id,
        related_entity_id: None,
        related_entity_state: None,
        prefixed: true,
        offset: 0,
        end_offset: 0,
    };
    let table = |table_class_id, entries: Vec<crate::feature::FeatureEntityTableEntry>| {
        crate::feature::FeatureEntityTable {
            feature_id: Some(2976),
            table_class_id,
            entry_ids: entries.iter().map(|entry| entry.entity_id).collect(),
            entries,
            surface_ids: Vec::new(),
            non_surface_entity_ids: Vec::new(),
            offset: 0,
        }
    };
    let tables = vec![
        table(67, vec![entry(2997, 200, Some(2976))]),
        table(100, vec![entry(2997, 98, None)]),
    ];
    let surface = crate::surface::SurfaceRow {
        id: 98,
        type_byte: 0x2a,
        kind: crate::surface::SurfaceKind::Extrusion,
        feature_id: 97,
        reversed: false,
        boundary_type: 0,
        next_surface: 0,
        offset: 0,
    };

    assert_eq!(
        feature_output_surface_dependencies(&tables, std::slice::from_ref(&surface), 2976),
        [97]
    );

    let mut current_surface = surface;
    current_surface.feature_id = 2976;
    assert!(feature_output_surface_dependencies(&tables, &[current_surface], 2976).is_empty());
}

#[test]
fn surface_merge_quilt_roster_links_every_unique_generator() {
    let entry = |entity_id, source_entity_id, offset| crate::feature::FeatureEntityTableEntry {
        entity_id,
        class_id: 200,
        source_entity_id: Some(source_entity_id),
        related_entity_id: None,
        related_entity_state: None,
        prefixed: true,
        offset,
        end_offset: offset + 1,
    };
    let producer = |feature_id, entity_id, offset| crate::feature::FeatureEntityTable {
        feature_id: Some(feature_id),
        table_class_id: 67,
        entry_ids: vec![entity_id],
        entries: vec![entry(entity_id, feature_id, offset + 1)],
        surface_ids: Vec::new(),
        non_surface_entity_ids: vec![entity_id],
        offset,
    };
    let replay = crate::feature::FeatureSurfaceMergeAffectedIds {
        feature_id: 416,
        geometry_ids: vec![98, 145, 157, 184, 321],
        edge_ids: vec![241],
        quilt_ids: vec![103, 192, 329],
        geometry_extent: crate::feature::ReplayExtentSource::Explicit,
        edge_extent: crate::feature::ReplayExtentSource::Explicit,
        quilt_extent: crate::feature::ReplayExtentSource::Inherited,
        offset: 100,
    };
    let tables = [
        producer(97, 103, 10),
        producer(175, 192, 20),
        producer(312, 329, 30),
    ];

    assert_eq!(
        surface_merge_entity_dependencies(&[], std::slice::from_ref(&replay), &tables, 416),
        [97, 175, 312]
    );
    assert_eq!(
        surface_merge_quilt_ids(&[], std::slice::from_ref(&replay), 416),
        Some([103, 192, 329].as_slice())
    );
    let wrong_class = crate::feature::FeatureEntityTable {
        feature_id: Some(175),
        table_class_id: 67,
        entry_ids: vec![192],
        entries: vec![crate::feature::FeatureEntityTableEntry {
            entity_id: 192,
            class_id: 201,
            source_entity_id: Some(175),
            related_entity_id: None,
            related_entity_state: None,
            prefixed: true,
            offset: 20,
            end_offset: 0,
        }],
        surface_ids: Vec::new(),
        non_surface_entity_ids: vec![192],
        offset: 20,
    };
    assert_eq!(
        surface_merge_entity_dependencies(
            &[],
            std::slice::from_ref(&replay),
            &[producer(97, 103, 10), wrong_class, producer(312, 329, 30)],
            416,
        ),
        [97, 312]
    );

    let future = producer(400, 777, 200);
    let future_replay = crate::feature::FeatureSurfaceMergeAffectedIds {
        feature_id: 417,
        geometry_ids: Vec::new(),
        edge_ids: Vec::new(),
        quilt_ids: vec![777],
        geometry_extent: crate::feature::ReplayExtentSource::Explicit,
        edge_extent: crate::feature::ReplayExtentSource::Explicit,
        quilt_extent: crate::feature::ReplayExtentSource::Explicit,
        offset: 100,
    };
    assert!(surface_merge_entity_dependencies(
        &[],
        std::slice::from_ref(&future_replay),
        std::slice::from_ref(&future),
        417,
    )
    .is_empty());
}

#[test]
fn generated_surface_faces_require_unique_rows_and_materialized_producers() {
    let row = |id, feature_id| crate::surface::SurfaceRow {
        id,
        type_byte: 0x22,
        kind: crate::surface::SurfaceKind::Plane,
        feature_id,
        reversed: false,
        boundary_type: 0,
        next_surface: 0,
        offset: 0,
    };
    let rows = [row(98, 97), row(145, 144)];
    let producers = BTreeSet::from([
        IrFeatureId("creo:model:feature#97".to_string()),
        IrFeatureId("creo:model:feature#144".to_string()),
    ]);
    let result_surface_ids = BTreeMap::from([(97, vec![98]), (144, vec![145])]);

    assert_eq!(
        generated_surface_face_refs(&[98, 145], &rows, &result_surface_ids, &producers),
        Some(vec![
            GeneratedFaceRef {
                feature: IrFeatureId("creo:model:feature#97".to_string()),
                local_id: "surface#98".to_string(),
            },
            GeneratedFaceRef {
                feature: IrFeatureId("creo:model:feature#144".to_string()),
                local_id: "surface#145".to_string(),
            },
        ])
    );
    assert_eq!(
        generated_surface_face_refs(
            &[98],
            &[row(98, 97), row(98, 97)],
            &result_surface_ids,
            &producers,
        ),
        None
    );
    assert_eq!(
        generated_surface_face_refs(&[98], &rows, &result_surface_ids, &BTreeSet::new()),
        None
    );
}

#[test]
fn feature_result_faces_require_unique_owned_materialized_table_surfaces() {
    let row = |id, feature_id| crate::surface::SurfaceRow {
        id,
        type_byte: 0x22,
        kind: crate::surface::SurfaceKind::Plane,
        feature_id,
        reversed: false,
        boundary_type: 0,
        next_surface: 0,
        offset: 0,
    };
    let entry = |entity_id, class_id, source_entity_id| crate::feature::FeatureEntityTableEntry {
        entity_id,
        class_id,
        source_entity_id,
        related_entity_id: None,
        related_entity_state: None,
        prefixed: false,
        offset: 0,
        end_offset: 0,
    };
    let table = crate::feature::FeatureEntityTable {
        feature_id: Some(97),
        table_class_id: 29,
        entry_ids: vec![98, 145],
        entries: vec![entry(98, 200, Some(1)), entry(145, 203, None)],
        surface_ids: vec![98, 145],
        non_surface_entity_ids: Vec::new(),
        offset: 0,
    };
    let rows = [row(98, 97), row(145, 97)];
    let curve_rows = [crate::curve::CurveTopologyRow {
        id: 77,
        type_byte: 8,
        feature_id: 97,
        directions: [1, 0xf6],
        faces: [98, 145],
        next_edges: [77, 77],
        offset: 0,
    }];
    assert_eq!(feature_result_edge_ids(&curve_rows, 97), Some(vec![77]));
    let duplicate_curve_rows = [
        curve_rows[0].clone(),
        crate::curve::CurveTopologyRow {
            offset: 1,
            ..curve_rows[0].clone()
        },
    ];
    assert!(feature_result_edge_ids(&duplicate_curve_rows, 97).is_none());
    assert_eq!(
        feature_result_surface_ids(std::slice::from_ref(&table), &rows, 97),
        Some(vec![98, 145])
    );
    assert_eq!(
        feature_result_topology(std::slice::from_ref(&table), &rows, &curve_rows, 97)
            .expect("complete result topology")
            .faces,
        vec!["surface#98", "surface#145"]
    );
    assert_eq!(
        feature_result_topology(std::slice::from_ref(&table), &rows, &curve_rows, 97)
            .expect("complete result topology")
            .edges,
        vec!["curve#77"]
    );

    let mut duplicate = table.clone();
    duplicate.entry_ids.push(98);
    duplicate.entries.push(entry(98, 204, None));
    duplicate.surface_ids.push(98);
    assert!(feature_result_surface_ids(&[duplicate], &rows, 97).is_none());

    let mut missing = table;
    missing.entry_ids[1] = 146;
    missing.entries[1] = entry(146, 203, None);
    missing.surface_ids[1] = 146;
    assert!(feature_result_surface_ids(&[missing], &rows, 97).is_none());

    let foreign = crate::feature::FeatureEntityTable {
        feature_id: Some(97),
        table_class_id: 29,
        entry_ids: vec![145],
        entries: vec![entry(145, 203, None)],
        surface_ids: vec![145],
        non_surface_entity_ids: Vec::new(),
        offset: 0,
    };
    assert!(feature_result_surface_ids(&[foreign], &[row(145, 144)], 97).is_none());
}

#[test]
fn generated_face_dependencies_follow_the_producer_feature() {
    let producer = IrFeatureId("creo:model:feature#97".to_string());
    let definition = IrFeatureDefinition::Thicken {
        faces: FaceSelection::Generated {
            faces: vec![GeneratedFaceRef {
                feature: producer.clone(),
                local_id: "surface#98".to_string(),
            }],
            native: "creo:allfeatur:thicken#9".to_string(),
        },
        thickness: None,
        side: None,
    };
    assert_eq!(feature_generated_dependencies(&definition), vec![producer]);
}

#[test]
fn generated_edge_dependencies_follow_the_producer_feature() {
    let producer = IrFeatureId("creo:model:feature#97".to_string());
    let generated_edges = EdgeSelection::Generated {
        edges: vec![GeneratedEdgeRef {
            feature: producer.clone(),
            local_id: "curve#77".to_string(),
        }],
        native: "creo:allfeatur:fillet#9".to_string(),
    };
    let fillet = IrFeatureDefinition::Fillet {
        groups: vec![cadmpeg_ir::features::FilletGroup {
            edges: generated_edges.clone(),
            radius: RadiusSpec::Unresolved { form: None },
            tangency_weight: None,
        }],
    };
    assert_eq!(
        feature_generated_dependencies(&fillet),
        vec![producer.clone()]
    );

    let chamfer = IrFeatureDefinition::Chamfer {
        groups: vec![cadmpeg_ir::features::ChamferGroup {
            edges: generated_edges,
            spec: cadmpeg_ir::features::ChamferSpec::Unresolved { form: None },
        }],
        flip_direction: false,
    };
    assert_eq!(feature_generated_dependencies(&chamfer), vec![producer]);
}

#[test]
fn surface_merge_quilts_resolve_through_unique_generated_surface_outputs() {
    let entry =
        |entity_id, class_id, source_entity_id, offset| crate::feature::FeatureEntityTableEntry {
            entity_id,
            class_id,
            source_entity_id,
            related_entity_id: None,
            related_entity_state: None,
            prefixed: true,
            offset,
            end_offset: offset + 1,
        };
    let table = |feature_id: u32,
                 table_class_id: u32,
                 entries: Vec<crate::feature::FeatureEntityTableEntry>,
                 offset: usize| {
        crate::feature::FeatureEntityTable {
            feature_id: Some(feature_id),
            table_class_id,
            entry_ids: entries.iter().map(|entry| entry.entity_id).collect(),
            entries,
            surface_ids: Vec::new(),
            non_surface_entity_ids: Vec::new(),
            offset,
        }
    };
    let row = |id, feature_id| crate::surface::SurfaceRow {
        id,
        type_byte: 0x22,
        kind: crate::surface::SurfaceKind::Plane,
        feature_id,
        reversed: false,
        boundary_type: 0,
        next_surface: 0,
        offset: 0,
    };
    let replay = |feature_id, quilt_ids, offset| crate::feature::FeatureSurfaceMergeAffectedIds {
        feature_id,
        geometry_ids: Vec::new(),
        edge_ids: Vec::new(),
        quilt_ids,
        geometry_extent: crate::feature::ReplayExtentSource::Explicit,
        edge_extent: crate::feature::ReplayExtentSource::Explicit,
        quilt_extent: crate::feature::ReplayExtentSource::Explicit,
        offset,
    };
    let mut scan = crate::container::scan_bytes(Vec::new());
    scan.features.entity_tables = vec![
        table(97, 67, vec![entry(103, 200, Some(97), 11)], 10),
        table(97, 100, vec![entry(103, 98, None, 21)], 20),
        table(144, 67, vec![entry(150, 200, Some(144), 31)], 30),
        table(144, 100, vec![entry(150, 145, None, 41)], 40),
    ];
    scan.surfaces.rows = vec![row(98, 97), row(145, 144)];
    scan.features
        .surface_merge_replay_affected_ids
        .push(replay(416, vec![103, 150], 100));

    assert_eq!(
        knit_operand_surface_ids(&scan, 416, &[103, 150]),
        Some(vec![98, 145])
    );

    scan.features
        .entity_tables
        .push(table(312, 67, vec![entry(103, 200, Some(312), 51)], 50));
    assert_eq!(knit_operand_surface_ids(&scan, 416, &[103, 150]), None);

    scan.features
        .entity_tables
        .push(table(400, 67, vec![entry(777, 200, Some(400), 201)], 200));
    scan.features
        .surface_merge_replay_affected_ids
        .push(replay(417, vec![777], 100));
    assert_eq!(knit_operand_surface_ids(&scan, 417, &[777]), None);
}

#[test]
fn generated_curve_edges_require_unique_rows_and_materialized_producers() {
    let row = |id, feature_id, offset| crate::curve::CurveTopologyRow {
        id,
        type_byte: 8,
        feature_id,
        directions: [1, 0xf6],
        faces: [10, 11],
        next_edges: [id, id],
        offset,
    };
    let rows = [row(45, 12, 100), row(46, 18, 200)];
    let producers = BTreeSet::from([
        IrFeatureId("creo:model:feature#12".to_string()),
        IrFeatureId("creo:model:feature#18".to_string()),
    ]);
    let result_edge_ids = BTreeMap::from([(12, vec![45]), (18, vec![46])]);

    assert_eq!(
        generated_curve_edge_refs(&[45, 46], &rows, &producers, &result_edge_ids),
        Some(vec![
            GeneratedEdgeRef {
                feature: IrFeatureId("creo:model:feature#12".to_string()),
                local_id: "curve#45".to_string(),
            },
            GeneratedEdgeRef {
                feature: IrFeatureId("creo:model:feature#18".to_string()),
                local_id: "curve#46".to_string(),
            },
        ])
    );
    assert_eq!(
        generated_curve_edge_refs(
            &[45],
            &[row(45, 12, 100), row(45, 12, 300)],
            &producers,
            &result_edge_ids,
        ),
        None
    );
    assert_eq!(
        generated_curve_edge_refs(&[45], &rows, &BTreeSet::new(), &result_edge_ids),
        None
    );
    assert_eq!(
        generated_curve_edge_refs(&[45], &rows, &producers, &BTreeMap::new()),
        None
    );
}

#[test]
fn mixed_current_and_generated_edges_remain_native() {
    let mut scan = crate::container::scan_bytes(Vec::new());
    scan.features
        .affected_ids
        .push(crate::feature::FeatureAffectedIds {
            feature_id: 10,
            kind: crate::feature::AffectedIdKind::Edges,
            ids: vec![45, 46],
            offset: 0,
        });
    scan.curves.topology_rows.extend([
        crate::curve::CurveTopologyRow {
            id: 45,
            type_byte: 8,
            feature_id: 97,
            directions: [1, 0xf6],
            faces: [1, 2],
            next_edges: [45, 45],
            offset: 0,
        },
        crate::curve::CurveTopologyRow {
            id: 46,
            type_byte: 8,
            feature_id: 97,
            directions: [1, 0xf6],
            faces: [1, 2],
            next_edges: [46, 46],
            offset: 1,
        },
    ]);
    let mut ir = CadIr::empty(cadmpeg_ir::units::Units::default());
    ir.model.features.push(Feature {
        id: IrFeatureId("creo:model:feature#97".to_string()),
        ordinal: 0,
        name: None,
        suppressed: None,
        parent: None,
        dependencies: Vec::new(),
        source_properties: std::collections::BTreeMap::new(),
        source_tag: None,
        source_text: None,
        source_content: Vec::new(),
        outputs: Vec::new(),
        definition: IrFeatureDefinition::Native {
            kind: "producer".to_string(),
            parameters: std::collections::BTreeMap::new(),
            properties: std::collections::BTreeMap::new(),
        },
        native_ref: None,
    });
    ir.model.edges.push(cadmpeg_ir::topology::Edge {
        id: EdgeId("creo:visibgeom:edge#45".to_string()),
        curve: None,
        start: cadmpeg_ir::ids::VertexId("test:start".to_string()),
        end: cadmpeg_ir::ids::VertexId("test:end".to_string()),
        param_range: None,
        tolerance: None,
    });

    assert_eq!(
        feature_edge_selection(&scan, &ir, 10),
        Some(EdgeSelection::Native(
            "creo:allfeatur:edgs_affected#10:45,46".to_string()
        ))
    );
}

#[test]
fn agreed_empty_edge_selection_is_resolved() {
    let mut scan = crate::container::scan_bytes(Vec::new());
    scan.features.affected_ids.extend([
        crate::feature::FeatureAffectedIds {
            feature_id: 10,
            kind: crate::feature::AffectedIdKind::Edges,
            ids: Vec::new(),
            offset: 0,
        },
        crate::feature::FeatureAffectedIds {
            feature_id: 10,
            kind: crate::feature::AffectedIdKind::Edges,
            ids: Vec::new(),
            offset: 1,
        },
    ]);

    assert_eq!(
        feature_edge_selection(&scan, &CadIr::empty(Units::default()), 10),
        Some(EdgeSelection::Resolved {
            edges: Vec::new(),
            native: "creo:allfeatur:edgs_affected#10:".to_string(),
        })
    );

    let mut replay_scan = crate::container::scan_bytes(Vec::new());
    replay_scan
        .features
        .replay_affected_ids
        .push(crate::feature::FeatureReplayAffectedIds {
            feature_id: 10,
            geometry_ids: vec![1, 2, 3],
            edge_ids: Vec::new(),
            geometry_extent: crate::feature::ReplayExtentSource::Explicit,
            edge_extent: crate::feature::ReplayExtentSource::Explicit,
            offset: 0,
        });
    assert_eq!(
        feature_edge_selection(&replay_scan, &CadIr::empty(Units::default()), 10),
        Some(EdgeSelection::Resolved {
            edges: Vec::new(),
            native: "creo:allfeatur:replay_edgs_affected#10:".to_string(),
        })
    );
}

#[test]
fn conflicting_empty_and_nonempty_edge_selections_remain_unresolved() {
    let mut scan = crate::container::scan_bytes(Vec::new());
    scan.features.affected_ids.extend([
        crate::feature::FeatureAffectedIds {
            feature_id: 10,
            kind: crate::feature::AffectedIdKind::Edges,
            ids: Vec::new(),
            offset: 0,
        },
        crate::feature::FeatureAffectedIds {
            feature_id: 10,
            kind: crate::feature::AffectedIdKind::Edges,
            ids: vec![45],
            offset: 1,
        },
    ]);

    assert_eq!(
        feature_edge_selection(&scan, &CadIr::empty(Units::default()), 10),
        None
    );
}

#[test]
fn geometry_generator_features_join_surface_and_curve_evidence() {
    let mut scan = crate::container::scan_bytes(Vec::new());
    scan.surfaces.rows.push(crate::surface::SurfaceRow {
        id: 61,
        type_byte: 0x22,
        kind: crate::surface::SurfaceKind::Plane,
        feature_id: 50,
        reversed: false,
        boundary_type: 0,
        next_surface: 0,
        offset: 200,
    });
    scan.curves
        .topology_rows
        .push(crate::curve::CurveTopologyRow {
            id: 59,
            type_byte: 8,
            feature_id: 50,
            directions: [1, 0xf6],
            faces: [61, 62],
            next_edges: [59, 59],
            offset: 100,
        });

    assert_eq!(
        geometry_generator_features(&scan),
        [GeometryGeneratorFeature {
            feature_id: 50,
            offset: 100,
            surface_ids: vec![61],
            curve_ids: vec![59],
        }]
    );
}

#[test]
fn model_feature_ids_include_row_backed_generated_producers() {
    let mut scan = crate::container::scan_bytes(Vec::new());
    scan.features.rows.push(crate::feature::FeatureRow {
        feature_id: 50,
        header: [0xeb, 0x04],
        root_schema_class: Some(913),
        stream_offset: 0,
        body: Vec::new(),
        body_offset: 1,
        offset: 0,
    });
    scan.surfaces.rows.push(crate::surface::SurfaceRow {
        id: 61,
        type_byte: crate::surface::SurfaceKind::Plane.canonical_type_byte(),
        kind: crate::surface::SurfaceKind::Plane,
        feature_id: 50,
        reversed: false,
        boundary_type: 0,
        next_surface: 0,
        offset: 200,
    });
    scan.curves
        .topology_rows
        .push(crate::curve::CurveTopologyRow {
            id: 59,
            type_byte: 8,
            feature_id: 50,
            directions: [1, 0xf6],
            faces: [61, 62],
            next_edges: [59, 59],
            offset: 100,
        });

    let available_features = model_feature_ids(&scan);
    assert_eq!(
        available_features,
        BTreeSet::from([IrFeatureId("creo:model:feature#50".to_string())])
    );
    assert_eq!(
        generated_surface_face_refs(
            &[61],
            &scan.surfaces.rows,
            &BTreeMap::from([(50, vec![61])]),
            &available_features,
        ),
        Some(vec![GeneratedFaceRef {
            feature: IrFeatureId("creo:model:feature#50".to_string()),
            local_id: "surface#61".to_string(),
        }])
    );
    assert_eq!(
        generated_curve_edge_refs(
            &[59],
            &scan.curves.topology_rows,
            &available_features,
            &BTreeMap::from([(50, vec![59])]),
        ),
        Some(vec![GeneratedEdgeRef {
            feature: IrFeatureId("creo:model:feature#50".to_string()),
            local_id: "curve#59".to_string(),
        }])
    );
    scan.features
        .affected_ids
        .push(crate::feature::FeatureAffectedIds {
            feature_id: 10,
            kind: crate::feature::AffectedIdKind::Edges,
            ids: vec![59],
            offset: 0,
        });
    assert_eq!(
        feature_edge_selection(&scan, &CadIr::empty(Units::default()), 10),
        Some(EdgeSelection::Generated {
            edges: vec![GeneratedEdgeRef {
                feature: IrFeatureId("creo:model:feature#50".to_string()),
                local_id: "curve#59".to_string(),
            }],
            native: "creo:allfeatur:edgs_affected#10:59".to_string(),
        })
    );
}

#[test]
fn closed_fallback_profile_selects_revolution_segments() {
    let segment = |external_id| crate::feature::FeatureSegment {
        kind: crate::feature::FeatureSegmentKind::Line,
        directions: [None; 3],
        point_ids: [1, 2],
        center_id: None,
        arc_orientation: None,
        vertical_horizontal: None,
        radius_ref: None,
        radius2_ref: None,
        external_id,
        body: Vec::new(),
        offset: 0,
    };
    let segments = [segment(9), segment(10), segment(11)];
    let profiles = vec![vec![
        SketchEntityUse {
            entity: SketchEntityId("creo:featdefs:sketch_entity#2:9".to_string()),
            reversed: false,
        },
        SketchEntityUse {
            entity: SketchEntityId("creo:featdefs:sketch_entity#2:11".to_string()),
            reversed: true,
        },
    ]];

    assert_eq!(
        profile_segment_ids(2, &segments, &profiles),
        BTreeSet::from([9, 11])
    );
}

#[test]
fn complementary_split_outlines_establish_a_cylinder_carrier() {
    let bounds = [
        [[-0.3125, 1.3125], [0.3125, 1.625]],
        [[-0.3125, 1.625], [0.3125, 1.9375]],
    ];
    let plane = SurfaceGeometry::Plane {
        origin: Point3::new(0.0, 0.0, -1.0),
        normal: Vector3::new(0.0, 0.0, 1.0),
        u_axis: Vector3::new(1.0, 0.0, 0.0),
    };
    assert_eq!(
        cylinder_from_complementary_outline_bounds(&plane, bounds),
        Some(SurfaceGeometry::Cylinder {
            origin: Point3::new(0.0, 1.625, -1.0),
            axis: Vector3::new(0.0, 0.0, 1.0),
            ref_direction: Vector3::new(1.0, 0.0, 0.0),
            radius: 0.3125,
        })
    );
}

#[test]
fn split_outline_carrier_requires_complementary_square_bounds() {
    let plane = SurfaceGeometry::Plane {
        origin: Point3::new(0.0, 0.0, 0.0),
        normal: Vector3::new(0.0, 0.0, 1.0),
        u_axis: Vector3::new(1.0, 0.0, 0.0),
    };
    assert!(cylinder_from_complementary_outline_bounds(
        &plane,
        [[[-1.0, 0.0], [1.0, 0.5]], [[-1.0, 0.6], [1.0, 1.0]]],
    )
    .is_none());
    assert!(cylinder_from_complementary_outline_bounds(
        &plane,
        [[[-1.0, 0.0], [1.0, 0.5]], [[-1.0, 0.5], [1.0, 3.0]]],
    )
    .is_none());
}

#[test]
fn tabulated_cylinder_frame_places_a_unique_cubic_chart() {
    let mut replay = crate::surface::TabulatedCylinderCurveReplay {
        body: Vec::new(),
        surface_id: 7,
        curve_id: 9,
        curve_type: 0x13,
        flip: 1,
        tangent_condition: 0,
        degree: 3,
        parameter_body: vec![],
        control_point_ids: [1, 2, 3, 4],
        successor_reference: 5,
        control_point_bodies: std::array::from_fn(|_| vec![]),
        control_points: [
            Some([1.0, 2.0]),
            Some([2.0, 2.5]),
            Some([3.0, 3.5]),
            Some([4.0, 4.0]),
        ],
        terminal_reference: 6,
        offset: 0,
        surface_row_offset: 0,
    };
    let parameters = crate::surface::SurfaceParameterRecord {
        surface_id: 7,
        body: vec![],
        scalar_values: vec![],
        scalar_tokens: vec![],
        opaque_spans: vec![crate::surface::SurfaceParameterOpaqueSpan {
            raw: vec![0x00, 0x0c, 0x9a],
            offset: 3,
            length: 3,
        }],
        scalar_frames: vec![
            crate::surface::SurfaceParameterScalarFrame {
                offset: 0,
                slots: [0.0, 0.0, 1.0].into_iter().map(parameter_slot).collect(),
            },
            crate::surface::SurfaceParameterScalarFrame {
                offset: 6,
                slots: [13.0, 22.0, 5.0, 10.0, 20.0, 10.0]
                    .into_iter()
                    .map(parameter_slot)
                    .collect(),
            },
        ],
        terminal_scalar_frame: None,
        tabulated_cylinder_frame: None,
        positional_cylinder_frame: None,
        split_cylinder_outline_bounds: None,
        positional_cone_frame: None,
        positional_torus_frame: None,
        boundary: crate::surface::SurfaceBodyBoundary::CompoundClose,
        offset: 0,
        body_offset: 0,
    };

    let (curve, sweep) =
        placed_tabulated_cylinder_directrix(&replay, &parameters, None).expect("placement");
    assert_eq!(curve.control_points[0], Point3::new(-13.0, -20.0, 5.0));
    assert_eq!(curve.control_points[3], Point3::new(-10.0, -22.0, 5.0));
    assert_eq!(sweep, [0.0, 0.0, 5.0]);

    let mut broad_signed_frame = parameters;
    broad_signed_frame.scalar_frames.truncate(1);
    broad_signed_frame.tabulated_cylinder_frame = Some(crate::surface::TabulatedCylinderFrame {
        values: [1.0, 2.0, 5.0, 4.0, 4.0, 10.0],
        prefixes: [0xa2, 0x42, 0x88, 0xa3, 0x18, 0x8a],
    });
    let (curve, sweep) = placed_tabulated_cylinder_directrix(&replay, &broad_signed_frame, None)
        .expect("broad signed-DICT placement");
    assert_eq!(curve.control_points[0], Point3::new(1.0, 2.0, 5.0));
    assert_eq!(curve.control_points[3], Point3::new(4.0, 4.0, 5.0));
    assert_eq!(sweep, [0.0, 0.0, 5.0]);

    broad_signed_frame.scalar_frames.clear();
    let (curve, sweep) = placed_tabulated_cylinder_directrix(&replay, &broad_signed_frame, None)
        .expect("complete frame supplies its signed sweep");
    assert_eq!(curve.control_points[0], Point3::new(1.0, 2.0, 5.0));
    assert_eq!(curve.control_points[3], Point3::new(4.0, 4.0, 5.0));
    assert_eq!(sweep, [0.0, 0.0, 5.0]);

    broad_signed_frame.tabulated_cylinder_frame = Some(crate::surface::TabulatedCylinderFrame {
        values: [1.0, 1.0, 2.0, 4.0, 4.0, 4.0],
        prefixes: [0xa2, 0x42, 0x88, 0xa3, 0x18, 0x8a],
    });
    assert!(placed_tabulated_cylinder_directrix(&replay, &broad_signed_frame, None).is_none());

    broad_signed_frame.tabulated_cylinder_frame = Some(crate::surface::TabulatedCylinderFrame {
        values: [29.0, 5.0, 2.0, -26.0, 10.0, 4.0],
        prefixes: [0x4a, 0x46, 0x2f, 0x46, 0x46, 0x2e],
    });
    replay.control_points[1] = Some([10.0, -5.0]);
    assert!(
        placed_tabulated_cylinder_directrix(&replay, &broad_signed_frame, None).is_none(),
        "the offset layout requires its prototype chart origin"
    );
    let (curve, sweep) =
        placed_tabulated_cylinder_directrix(&replay, &broad_signed_frame, Some([-30.0, 0.0, 0.0]))
            .expect("independently signed offset placement");
    assert_eq!(curve.control_points[0], Point3::new(-29.0, 5.0, 2.0));
    assert_eq!(curve.control_points[1], Point3::new(-20.0, 5.0, -5.0));
    assert_eq!(curve.control_points[3], Point3::new(-26.0, 5.0, 4.0));
    assert_eq!(sweep, [0.0, 5.0, 0.0]);

    broad_signed_frame.tabulated_cylinder_frame = Some(crate::surface::TabulatedCylinderFrame {
        values: [1.0, 2.0, 5.0, 4.0, 4.0, 10.0],
        prefixes: [0xdd, 0xa1, 0x9e, 0xd8, 0xa2, 0x9e],
    });
    replay.control_points[1] = Some([2.0, 2.5]);
    let (curve, sweep) = placed_tabulated_cylinder_directrix(&replay, &broad_signed_frame, None)
        .expect("scalar encodings do not change the coordinate chart");
    assert_eq!(curve.control_points[0], Point3::new(1.0, 2.0, 5.0));
    assert_eq!(curve.control_points[3], Point3::new(4.0, 4.0, 5.0));
    assert_eq!(sweep, [0.0, 0.0, 5.0]);

    broad_signed_frame.tabulated_cylinder_frame = Some(crate::surface::TabulatedCylinderFrame {
        values: [1.0, 1.0, 2.0, 4.0, 4.0, 4.0],
        prefixes: [0xdd, 0xa1, 0x9e, 0xd8, 0xa2, 0x9e],
    });
    assert!(placed_tabulated_cylinder_directrix(&replay, &broad_signed_frame, None).is_none());

    replay.control_points = [
        Some([1.0, 2.0]),
        Some([2.0, 2.5]),
        Some([3.0, 3.5]),
        Some([4.0, 4.0]),
    ];
    broad_signed_frame.tabulated_cylinder_frame = Some(crate::surface::TabulatedCylinderFrame {
        values: [-11.25, 2.0, 5.0, -8.25, 4.0, 10.0],
        prefixes: [0x46, 0x46, 0x2f, 0x46, 0x46, 0x2e],
    });
    let (curve, sweep) =
        placed_tabulated_cylinder_directrix(&replay, &broad_signed_frame, Some([-12.25, 0.0, 0.0]))
            .expect("prototype chart origin supplies an arbitrary intercept");
    assert_eq!(curve.control_points[0], Point3::new(-11.25, 2.0, 5.0));
    assert_eq!(curve.control_points[3], Point3::new(-8.25, 4.0, 5.0));
    assert_eq!(sweep, [0.0, 0.0, 5.0]);
    assert!(placed_tabulated_cylinder_directrix(&replay, &broad_signed_frame, None).is_none());
}

#[test]
fn tabulated_cylinder_offset_chart_resolves_signed_unit_axes() {
    assert_eq!(
        signed_unit_chart(
            [33.480_874_469_5, 34.047_445_706_6],
            [3.480_874_469_5, 4.047_445_706_6],
            30.0,
        ),
        Some((1.0, -30.0))
    );
    assert_eq!(
        signed_unit_chart(
            [0.576_336_341_1, 0.746_308_064_9],
            [-0.746_308_064_9, -0.576_336_341_1],
            0.0,
        ),
        Some((-1.0, 0.0))
    );
    assert_eq!(
        signed_unit_chart(
            [21.592_186_587_7, 21.604_574_667_3],
            [8.407_813_412_3, -8.395_425_332_7],
            30.0,
        ),
        Some((1.0, -30.0))
    );
    assert_eq!(signed_unit_chart([1.0, 2.0], [4.0, 5.0], 30.0), None);
}

#[test]
fn zero_offset_2d_tabulated_frame_retains_the_stored_span() {
    let replay = crate::surface::TabulatedCylinderCurveReplay {
        body: Vec::new(),
        surface_id: 815,
        curve_id: 1,
        curve_type: 0x13,
        flip: 1,
        tangent_condition: 0,
        degree: 3,
        parameter_body: Vec::new(),
        control_point_ids: [1, 2, 3, 4],
        successor_reference: 0,
        control_point_bodies: std::array::from_fn(|_| Vec::new()),
        control_points: [
            Some([2.603_530_729_189_511_6, -6.634_758_301_120_719]),
            Some([2.486_761_892_214_414, -6.583_162_851_673_087]),
            Some([2.403_937_662_020_322, -6.519_347_555_976_829]),
            Some([2.355_057_866_495_792, -6.440_596_814_034_794]),
        ],
        terminal_reference: 0,
        offset: 0,
        surface_row_offset: 0,
    };
    let body = vec![
        0x18, 0xe4, 0x0f, 0x00, 0x0c, 0x9a, 0x8d, 0xd7, 0x28, 0x94, 0x26, 0x4b, 0xb2, 0x2d, 0x19,
        0xc3, 0x2b, 0xcf, 0xac, 0x01, 0x44, 0x9e, 0x1e, 0xb8, 0x51, 0xeb, 0x85, 0x1f, 0x8f, 0xd4,
        0x07, 0xeb, 0x3f, 0xff, 0xf8, 0x2d, 0x1a, 0x89, 0xfe, 0x14, 0x80, 0xb6, 0x48, 0x9e, 0x85,
        0x1e, 0xb8, 0x51, 0xeb, 0x85,
    ];
    let tabulated_cylinder_frame = crate::surface::decode_tabulated_cylinder_frame(
        &body,
        &crate::scalar::ScalarCache::default(),
    )
    .map(|(frame, _)| frame);
    let parameters = crate::surface::SurfaceParameterRecord {
        surface_id: 815,
        body,
        scalar_values: Vec::new(),
        scalar_tokens: Vec::new(),
        opaque_spans: vec![crate::surface::SurfaceParameterOpaqueSpan {
            raw: vec![0, 0x0c, 0x9a],
            offset: 3,
            length: 3,
        }],
        scalar_frames: vec![crate::surface::SurfaceParameterScalarFrame {
            offset: 0,
            slots: vec![
                parameter_slot(0.0),
                parameter_slot(1.0),
                parameter_slot(0.0),
            ],
        }],
        terminal_scalar_frame: None,
        tabulated_cylinder_frame,
        positional_cylinder_frame: None,
        split_cylinder_outline_bounds: None,
        positional_cone_frame: None,
        positional_torus_frame: None,
        boundary: crate::surface::SurfaceBodyBoundary::CompoundClose,
        offset: 0,
        body_offset: 0,
    };
    let (curve, sweep) = placed_tabulated_cylinder_directrix(&replay, &parameters, None)
        .expect("zero-offset directrix placement");
    assert_eq!(
        curve.control_points[0],
        Point3::new(-2.603_530_729_189_511_6, 6.634_758_301_120_719, 4.78)
    );
    assert_eq!(
        curve.control_points[3],
        Point3::new(-2.355_057_866_495_792, 6.440_596_814_034_794, 4.78)
    );
    assert_eq!(sweep, [0.0, 0.0, 0.099_999_999_999_999_64]);
}

#[test]
fn geometry_signal_excludes_opaque_carriers() {
    let mut ir = CadIr::empty(Units::default());
    let surface_id = SurfaceId("surface".to_string());
    ir.model.surfaces.push(Surface {
        id: surface_id.clone(),
        geometry: SurfaceGeometry::Unknown { record: None },
        source_object: None,
    });
    ir.model.curves.push(Curve {
        id: CurveId("curve".to_string()),
        geometry: CurveGeometry::Unknown { record: None },
        source_object: None,
    });

    assert!(!has_transferred_geometry(&ir));

    ir.model.procedural_surfaces.push(ProceduralSurface {
        id: ProceduralSurfaceId("procedural".to_string()),
        surface: surface_id,
        definition: ProceduralSurfaceDefinition::Exact {
            parameters: cadmpeg_ir::geometry::SplineSurfaceParameters::OrderedRanges {
                ranges: [[0.0, 1.0], [0.0, 1.0]],
            },
            extension: 0,
            revision_form: None,
        },
        cache_fit_tolerance: None,
        record_bounds: None,
    });

    assert!(has_transferred_geometry(&ir));
}

#[test]
fn fc05_row_frame_maps_cyclically_onto_each_model_axis() {
    let center = [11.0, 13.0];
    let reference = [0.6, 0.8];
    assert_eq!(
        fc05_model_frame(0, 17.0, center, reference, -1.0),
        ([17.0, 13.0, 11.0], [-1.0, 0.0, 0.0], [0.0, 0.8, 0.6])
    );
    assert_eq!(
        fc05_model_frame(1, 17.0, center, reference, -1.0),
        ([11.0, 17.0, 13.0], [0.0, -1.0, 0.0], [0.6, 0.0, 0.8])
    );
    assert_eq!(
        fc05_model_frame(2, 17.0, center, reference, -1.0),
        ([13.0, 11.0, 17.0], [0.0, 0.0, -1.0], [0.8, 0.6, 0.0])
    );
}

#[test]
fn full_turn_section_carriers_classify_analytic_revolution_surfaces() {
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
    let line = |start: [f64; 2], end: [f64; 2]| SketchGeometry::Line {
        start: cadmpeg_ir::math::Point2::new(start[0], start[1]),
        end: cadmpeg_ir::math::Point2::new(end[0], end[1]),
    };

    assert!(matches!(
        revolved_section_circle(&transform, [2.0, 3.0], axis),
        Some(CurveGeometry::Circle {
            center,
            axis,
            ref_direction,
            radius,
        }) if center == Point3::new(0.0, 3.0, 0.0)
            && axis == Vector3::new(0.0, 1.0, 0.0)
            && ref_direction == Vector3::new(1.0, 0.0, 0.0)
            && radius == 2.0
    ));
    assert!(revolved_section_circle(&transform, [0.0, 3.0], axis).is_none());
    assert!(matches!(
        extruded_section_line(&transform, [2.0, 3.0]),
        Some(CurveGeometry::Line { origin, direction })
            if origin == Point3::new(2.0, 3.0, 0.0)
                && direction == Vector3::new(0.0, 0.0, 1.0)
    ));

    assert!(matches!(
        revolved_section_surface(&transform, &line([2.0, 0.0], [2.0, 4.0]), axis),
        Some(SurfaceGeometry::Cylinder { radius, .. }) if radius == 2.0
    ));
    assert!(matches!(
        revolved_section_surface(&transform, &line([0.0, 3.0], [4.0, 3.0]), axis),
        Some(SurfaceGeometry::Plane { origin, .. }) if origin.y == 3.0
    ));
    assert!(matches!(
        revolved_section_surface(&transform, &line([2.0, 0.0], [4.0, 2.0]), axis),
        Some(SurfaceGeometry::Cone { radius, half_angle, .. })
            if radius == 2.0 && (half_angle - std::f64::consts::FRAC_PI_4).abs() < 1e-12
    ));
    assert!(matches!(
        revolved_section_surface(&transform, &line([4.0, 0.0], [2.0, 2.0]), axis),
        Some(SurfaceGeometry::Cone { axis, radius, half_angle, .. })
            if axis.y == -1.0
                && radius == 4.0
                && (half_angle - std::f64::consts::FRAC_PI_4).abs() < 1e-12
    ));
    let centered_arc = SketchGeometry::Arc {
        center: cadmpeg_ir::math::Point2::new(0.0, 3.0),
        radius: Length(2.0),
        start_angle: Angle(0.0),
        end_angle: Angle(std::f64::consts::PI),
    };
    assert!(matches!(
        revolved_section_surface(&transform, &centered_arc, axis),
        Some(SurfaceGeometry::Sphere { radius, .. }) if radius == 2.0
    ));
    let offset_arc = SketchGeometry::Arc {
        center: cadmpeg_ir::math::Point2::new(5.0, 3.0),
        radius: Length(2.0),
        start_angle: Angle(0.0),
        end_angle: Angle(std::f64::consts::PI),
    };
    assert!(matches!(
        revolved_section_surface(&transform, &offset_arc, axis),
        Some(SurfaceGeometry::Torus { major_radius, minor_radius, .. })
            if major_radius == 5.0 && minor_radius == 2.0
    ));
    let offset_circle = SketchGeometry::Circle {
        center: Point2::new(5.0, 3.0),
        radius: Length(2.0),
    };
    assert!(matches!(
        revolved_section_surface(&transform, &offset_circle, axis),
        Some(SurfaceGeometry::Torus { major_radius, minor_radius, .. })
            if major_radius == 5.0 && minor_radius == 2.0
    ));
}

#[test]
fn spindle_torus_boundary_pcurve_retains_the_signed_ring_branch() {
    let surface = SurfaceGeometry::Torus {
        center: Point3::new(0.0, 0.0, 0.0),
        axis: Vector3::new(0.0, 0.0, 1.0),
        ref_direction: Vector3::new(1.0, 0.0, 0.0),
        major_radius: 2.0,
        minor_radius: 5.0,
    };
    let axis = RevolutionAxis {
        origin: Point3::new(0.0, 0.0, 0.0),
        direction: Vector3::new(0.0, 0.0, 1.0),
    };
    let pcurve =
        revolution_boundary_pcurve(&surface, [-3.0, 0.0, 0.0], axis).expect("spindle boundary");
    for parameter in [0.0, 0.25, 0.5, 0.75, 1.0] {
        let uv = cadmpeg_ir::eval::pcurve_uv(&pcurve, parameter).expect("pcurve point");
        let point = cadmpeg_ir::eval::surface_point(&surface, uv.u, uv.v).expect("surface point");
        assert!((point.x.hypot(point.y) - 3.0).abs() < 1e-12);
        assert!(point.z.abs() < 1e-12);
    }
}
