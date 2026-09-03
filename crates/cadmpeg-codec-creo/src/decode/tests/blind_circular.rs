// SPDX-License-Identifier: Apache-2.0
//! Tests: blind circular.

use super::parameter_slot;
use crate::decode::analytic::PlaneEquation;
use crate::decode::feature_history::{
    coordinate_pair_proves_torus_radii, differing_positive_lengths,
    five_coordinate_envelope_proves_torus_radii, outline_has_unique_radius_delta,
    paired_five_coordinate_sphere_center, parallel_support_radius, round_constant_radius,
    round_observed_radii, round_placed_cylinder_radii, round_support_radius,
    schema_feature_definition, section_entity_is_generated_profile, slot_fillet_cylinder,
    unique_positive_length,
};
use crate::decode::holes::{
    compact_simple_hole_cylinder_id, extrusion_extent_and_direction,
    single_cap_circular_sweep_geometry, two_cap_circular_sweep_geometry, ExtrusionSpan,
};
use crate::decode::surfaces::{
    reference_cap_bound_round_frame, reference_circle_pair_cylinder_frame,
};
use crate::decode::sweep::{
    agreed_generated_cylinder_extent, blind_extrusion_from_carriers, bounded_cylinder_span,
    directed_blind_extrusion_span, generated_bounded_cylinder_extent, generated_cap_plane_extent,
    generated_rectilinear_plane_extent, ordered_parallel_cap_extent,
    resolved_feature_extrusion_span, unique_available_positional_cylinder_frame_records,
    ExtrusionCarrierSpan,
};
use cadmpeg_ir::document::CadIr;
use cadmpeg_ir::features::{
    ExtrudeExtent, ExtrudeSide, FeatureDefinition as IrFeatureDefinition, Length, RadiusForm,
    RadiusSpec, Termination,
};
use cadmpeg_ir::geometry::{Surface, SurfaceGeometry};
use cadmpeg_ir::ids::SurfaceId;
use cadmpeg_ir::math::{Point3, Vector3};
use std::collections::BTreeSet;

#[test]
fn blind_circular_sweep_requires_materialized_cap_and_cylinder_entries() {
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
    let entries = vec![
        entry(43, 204, None),
        entry(46, 203, None),
        entry(49, 200, Some(4)),
        entry(51, 200, None),
    ];
    let table = crate::feature::FeatureEntityTable {
        feature_id: Some(40),
        table_class_id: 29,
        entry_ids: entries.iter().map(|entry| entry.entity_id).collect(),
        entries,
        surface_ids: vec![46, 51],
        non_surface_entity_ids: vec![43, 49],
        offset: 0,
    };
    let row = |feature_id, id, kind: crate::surface::SurfaceKind| crate::surface::SurfaceRow {
        id,
        type_byte: kind.canonical_type_byte(),
        kind,
        feature_id,
        reversed: false,
        boundary_type: 0,
        next_surface: 0,
        offset: id as usize,
    };
    let mut scan = crate::container::scan_bytes(Vec::new());
    scan.features.entity_tables.push(table);
    scan.surfaces.rows.extend([
        row(40, 46, crate::surface::SurfaceKind::Plane),
        row(40, 51, crate::surface::SurfaceKind::Cylinder),
    ]);
    scan.planes.outlines.push(crate::surface::OutlinePlane {
        surface_id: 46,
        origin: [0.0, 16.0, 0.0],
        normal: [0.0, 1.0, 0.0],
        u_axis: [1.0, 0.0, 0.0],
        offset: 46,
    });
    scan.planes
        .envelopes
        .push(crate::surface::PlaneEnvelopeRecord {
            surface_id: 46,
            body: Vec::new(),
            envelope: crate::surface::PlaneEnvelope::Standard {
                bounds_2d: [[None; 2]; 2],
                corners_3d: [
                    [Some(-4.45), Some(16.0), Some(-4.45)],
                    [Some(4.45), Some(16.0), Some(4.45)],
                ],
            },
            corner_coordinate_equal: [Some(false), Some(true), Some(false)],
            scalar_tokens: Vec::new(),
            row_offset: 0,
            offset: 0,
        });
    scan.features
        .section_transforms
        .push(crate::placement::FeatureSectionTransform {
            definition_id: 40,
            feature_id: Some(40),
            origin: [0.0, 0.0, 0.0],
            u_axis: [1.0, 0.0, 0.0],
            v_axis: [0.0, 0.0, 1.0],
            normal: [0.0, 1.0, 0.0],
            offset: 0,
        });

    assert!(single_cap_circular_sweep_geometry(&scan, 40).is_some());

    let reversed_entries = vec![
        entry(143, 204, None),
        entry(146, 203, None),
        entry(149, 200, Some(4)),
        entry(151, 200, None),
    ];
    scan.features
        .entity_tables
        .push(crate::feature::FeatureEntityTable {
            feature_id: Some(41),
            table_class_id: 29,
            entry_ids: reversed_entries
                .iter()
                .map(|entry| entry.entity_id)
                .collect(),
            entries: reversed_entries,
            surface_ids: vec![143, 151],
            non_surface_entity_ids: vec![146, 149],
            offset: 0,
        });
    scan.surfaces.rows.extend([
        row(41, 143, crate::surface::SurfaceKind::Plane),
        row(41, 151, crate::surface::SurfaceKind::Cylinder),
    ]);
    scan.planes.outlines.push(crate::surface::OutlinePlane {
        surface_id: 143,
        origin: [0.0, 16.0, 0.0],
        normal: [0.0, 1.0, 0.0],
        u_axis: [1.0, 0.0, 0.0],
        offset: 143,
    });
    scan.planes
        .envelopes
        .push(crate::surface::PlaneEnvelopeRecord {
            surface_id: 143,
            body: Vec::new(),
            envelope: crate::surface::PlaneEnvelope::Standard {
                bounds_2d: [[None; 2]; 2],
                corners_3d: [
                    [Some(-4.45), Some(16.0), Some(-4.45)],
                    [Some(4.45), Some(16.0), Some(4.45)],
                ],
            },
            corner_coordinate_equal: [Some(false), Some(true), Some(false)],
            scalar_tokens: Vec::new(),
            row_offset: 0,
            offset: 0,
        });
    scan.features
        .section_transforms
        .push(crate::placement::FeatureSectionTransform {
            definition_id: 41,
            feature_id: Some(41),
            origin: [0.0, 0.0, 0.0],
            u_axis: [1.0, 0.0, 0.0],
            v_axis: [0.0, 0.0, 1.0],
            normal: [0.0, 1.0, 0.0],
            offset: 0,
        });
    assert!(single_cap_circular_sweep_geometry(&scan, 41).is_some());

    assert!(section_entity_is_generated_profile(
        true,
        Some(40),
        4,
        &[crate::surface::SurfaceKind::Cylinder],
        &scan.features.entity_tables,
        &scan.surfaces.rows,
    ));

    scan.features.entity_tables[0]
        .surface_ids
        .retain(|id| *id != 51);
    assert!(single_cap_circular_sweep_geometry(&scan, 40).is_none());
    assert!(!section_entity_is_generated_profile(
        true,
        Some(40),
        4,
        &[crate::surface::SurfaceKind::Cylinder],
        &scan.features.entity_tables,
        &scan.surfaces.rows,
    ));
}

#[test]
fn two_cap_circular_sweep_joins_materialized_caps_and_one_cylinder() {
    let mut scan = crate::container::scan_bytes(Vec::new());
    let row = |id, kind: crate::surface::SurfaceKind| crate::surface::SurfaceRow {
        id,
        type_byte: kind.canonical_type_byte(),
        kind,
        feature_id: 825,
        reversed: false,
        boundary_type: 0,
        next_surface: 0,
        offset: id as usize,
    };
    scan.surfaces.rows.extend([
        row(828, crate::surface::SurfaceKind::Plane),
        row(831, crate::surface::SurfaceKind::Plane),
        row(836, crate::surface::SurfaceKind::Cylinder),
    ]);
    scan.planes
        .positional_frames
        .push(crate::surface::OutlinePlane {
            surface_id: 828,
            origin: [0.0, 4.0, 0.0],
            normal: [0.0, 1.0, 0.0],
            u_axis: [1.0, 0.0, 0.0],
            offset: 828,
        });
    scan.planes.outlines.push(crate::surface::OutlinePlane {
        surface_id: 831,
        origin: [0.0, -4.0, 0.0],
        normal: [0.0, 1.0, 0.0],
        u_axis: [1.0, 0.0, 0.0],
        offset: 831,
    });
    scan.planes
        .envelopes
        .push(crate::surface::PlaneEnvelopeRecord {
            surface_id: 831,
            body: Vec::new(),
            envelope: crate::surface::PlaneEnvelope::Standard {
                bounds_2d: [[None; 2]; 2],
                corners_3d: [
                    [Some(-13.25), Some(-4.0), Some(-0.75)],
                    [Some(-11.75), Some(-4.0), Some(0.75)],
                ],
            },
            corner_coordinate_equal: [Some(false), Some(true), Some(false)],
            scalar_tokens: Vec::new(),
            row_offset: 0,
            offset: 0,
        });
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
    let entries = vec![
        entry(828, 204, None),
        entry(831, 203, None),
        entry(834, 200, Some(22)),
        entry(836, 200, None),
    ];
    scan.features
        .entity_tables
        .push(crate::feature::FeatureEntityTable {
            feature_id: Some(825),
            table_class_id: 29,
            entry_ids: entries.iter().map(|entry| entry.entity_id).collect(),
            entries,
            surface_ids: vec![828, 831, 836],
            non_surface_entity_ids: vec![834],
            offset: 0,
        });

    let sweep = two_cap_circular_sweep_geometry(&scan, 825).expect("two-cap sweep");
    assert_eq!(sweep.cylinder_ids, vec![836]);
    assert_eq!(sweep.direction, [0.0, -1.0, 0.0]);
    assert_eq!(
        sweep.extent,
        ExtrudeExtent::OneSided {
            side: ExtrudeSide {
                termination: Termination::Blind {
                    length: Length(8.0),
                },
                draft: None,
                offset: None,
            },
        }
    );
    assert!(matches!(
        sweep.geometry,
        SurfaceGeometry::Cylinder { origin, axis, radius, .. }
            if origin == Point3::new(-12.5, -4.0, 0.0)
                && axis == Vector3::new(0.0, -1.0, 0.0)
                && radius == 0.75
    ));

    scan.features.entity_tables[0]
        .surface_ids
        .retain(|id| *id != 831);
    assert!(two_cap_circular_sweep_geometry(&scan, 825).is_none());
}

#[test]
fn compact_hole_materialized_core_establishes_the_simple_form() {
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
    let mut table = crate::feature::FeatureEntityTable {
        feature_id: Some(107),
        table_class_id: 29,
        entry_ids: vec![109, 112, 115, 117],
        entries: vec![
            entry(109, 204, None),
            entry(112, 203, None),
            entry(115, 200, Some(0)),
            entry(117, 200, None),
        ],
        surface_ids: vec![117],
        non_surface_entity_ids: Vec::new(),
        offset: 0,
    };
    let row = crate::surface::SurfaceRow {
        id: 117,
        type_byte: 0x24,
        kind: crate::surface::SurfaceKind::Cylinder,
        feature_id: 107,
        reversed: true,
        boundary_type: 0,
        next_surface: 0,
        offset: 0,
    };

    assert_eq!(
        compact_simple_hole_cylinder_id(
            107,
            std::slice::from_ref(&table),
            std::slice::from_ref(&row),
        ),
        Some(117)
    );
    let mut exact_class_203_plane = table.clone();
    exact_class_203_plane.surface_ids.push(112);
    let topology_plane = crate::surface::SurfaceRow {
        id: 112,
        type_byte: 0x22,
        kind: crate::surface::SurfaceKind::Plane,
        feature_id: 107,
        reversed: false,
        boundary_type: 0,
        next_surface: 0,
        offset: 0,
    };
    assert_eq!(
        compact_simple_hole_cylinder_id(
            107,
            std::slice::from_ref(&exact_class_203_plane),
            &[topology_plane, row.clone()],
        ),
        Some(117)
    );
    table.entries[2].source_entity_id = None;
    assert!(compact_simple_hole_cylinder_id(
        107,
        std::slice::from_ref(&table),
        std::slice::from_ref(&row),
    )
    .is_none());
    table.entries[2].source_entity_id = Some(0);
    table.table_class_id = 28;
    assert!(compact_simple_hole_cylinder_id(
        107,
        std::slice::from_ref(&table),
        std::slice::from_ref(&row),
    )
    .is_none());
    table.table_class_id = 29;
    table.entries[3].class_id = 201;
    assert!(compact_simple_hole_cylinder_id(
        107,
        std::slice::from_ref(&table),
        std::slice::from_ref(&row),
    )
    .is_none());
    table.entries[3].class_id = 200;
    table.surface_ids.push(109);
    assert!(compact_simple_hole_cylinder_id(
        107,
        std::slice::from_ref(&table),
        std::slice::from_ref(&row),
    )
    .is_none());

    let mut extended = crate::feature::FeatureEntityTable {
        feature_id: Some(107),
        table_class_id: 29,
        entry_ids: vec![109, 112, 120, 121, 115, 117],
        entries: vec![
            entry(109, 204, None),
            entry(112, 203, None),
            entry(120, 204, None),
            entry(121, 203, None),
            entry(115, 200, Some(0)),
            entry(117, 200, None),
        ],
        surface_ids: vec![109, 117],
        non_surface_entity_ids: vec![112, 120, 121, 115],
        offset: 0,
    };
    for (index, entry) in extended.entries.iter_mut().enumerate() {
        entry.offset = index;
        entry.end_offset = index + 1;
    }
    let plane = crate::surface::SurfaceRow {
        id: 109,
        type_byte: 0x22,
        kind: crate::surface::SurfaceKind::Plane,
        feature_id: 107,
        reversed: false,
        boundary_type: 0,
        next_surface: 0,
        offset: 0,
    };
    let rows = [plane.clone(), row.clone()];
    assert_eq!(
        compact_simple_hole_cylinder_id(107, std::slice::from_ref(&extended), &rows),
        Some(117)
    );
    let mut class_203_plane = extended.clone();
    class_203_plane.surface_ids[0] = 112;
    class_203_plane
        .non_surface_entity_ids
        .retain(|id| *id != 112);
    class_203_plane.non_surface_entity_ids.push(109);
    let mut second_topology_plane = plane;
    second_topology_plane.id = 112;
    let second_topology_rows = [second_topology_plane, row];
    assert_eq!(
        compact_simple_hole_cylinder_id(
            107,
            std::slice::from_ref(&class_203_plane),
            &second_topology_rows,
        ),
        Some(117)
    );
    extended.surface_ids.push(120);
    assert!(compact_simple_hole_cylinder_id(107, std::slice::from_ref(&extended), &rows).is_none());
}

#[test]
fn torus_outline_identifies_exactly_one_prototype_radius_delta() {
    let outline = |values| crate::surface::TorusOutlineFrame {
        values,
        selector: 0,
        offset: 0,
    };
    assert!(outline_has_unique_radius_delta(
        outline([-192.5, -5.0, -40.0, -167.5, -3.0, 52.5]),
        2.0
    ));
    assert!(!outline_has_unique_radius_delta(
        outline([-2.0, -2.0, 0.0, 0.0, 0.0, 8.0]),
        2.0
    ));
    assert!(!outline_has_unique_radius_delta(
        outline([-2.0, 0.0, 0.0, 2.0, 0.0, 8.0]),
        2.0
    ));
    let five_coordinate =
        |values| crate::surface::Type26FiveCoordinateEnvelope { values, offset: 0 };
    assert!(five_coordinate_envelope_proves_torus_radii(
        five_coordinate([-2.65, -15.0, -2.65, 2.65, -17.65]),
        0.0,
        2.65
    ));
    assert!(!five_coordinate_envelope_proves_torus_radii(
        five_coordinate([-2.65, -15.0, -2.5, 2.65, -17.65]),
        0.0,
        2.65
    ));
    assert!(five_coordinate_envelope_proves_torus_radii(
        five_coordinate([-4.95, 17.24, -4.95, 4.95, 16.74]),
        4.45,
        0.5
    ));
    assert!(coordinate_pair_proves_torus_radii(
        [-4.95, 17.24],
        [16.74, 4.95],
        4.45,
        0.5
    ));
    assert_eq!(
        paired_five_coordinate_sphere_center(
            [
                five_coordinate([-2.65, -15.0, -2.65, 2.65, -17.65]),
                five_coordinate([-2.65, -12.35, -2.65, 2.65, -15.0]),
            ],
            2.65,
        ),
        Some([0.0, 0.0, -15.0])
    );
    assert!(paired_five_coordinate_sphere_center(
        [
            five_coordinate([-2.65, -15.0, -2.65, 2.65, -17.65]),
            five_coordinate([-2.65, -12.0, -2.65, 2.65, -15.0]),
        ],
        2.65,
    )
    .is_none());
}

#[test]
fn unique_parallel_round_supports_define_constant_radius() {
    assert_eq!(unique_positive_length(&[0.5, 0.5 + 1.0e-12]), Some(0.5));
    assert_eq!(unique_positive_length(&[0.5, 0.6]), None);
    assert_eq!(unique_positive_length(&[0.0]), None);
    assert!(!differing_positive_lengths(&[15.0, 15.0 + 1.0e-12]));
    assert!(differing_positive_lengths(&[15.0, 7.0, 15.0]));
    assert!(!differing_positive_lengths(&[0.0, 1.0]));
    assert_eq!(
        parallel_support_radius([
            ([-8.0, 0.0, 0.0], [1.0, 0.0, 0.0]),
            ([0.0, 0.0, -6.1], [0.0, 0.0, 1.0]),
            ([-9.0, 0.0, 0.0], [1.0, 0.0, 0.0]),
        ]),
        Some(0.5)
    );
    assert_eq!(
        parallel_support_radius([
            ([-8.0, 0.0, 0.0], [1.0, 0.0, 0.0]),
            ([-9.0, 0.0, 0.0], [1.0, 0.0, 0.0]),
            ([0.0, 0.0, -6.0], [0.0, 0.0, 1.0]),
            ([0.0, 0.0, -8.0], [0.0, 0.0, 1.0]),
        ]),
        None
    );
    assert_eq!(
        parallel_support_radius([
            ([-8.0, 0.0, 0.0], [1.0, 0.0, 0.0]),
            ([-9.0, 0.0, 0.0], [1.0, 0.0, 0.0]),
            ([0.0, 0.0, -6.0], [0.0, 0.0, 1.0]),
            ([0.0, 0.0, -7.0], [0.0, 0.0, 1.0]),
        ]),
        Some(0.5)
    );
    let cylinder = slot_fillet_cylinder(
        [
            PlaneEquation {
                origin: [0.0, -2.0, 0.0],
                normal: [0.0, 1.0, 0.0],
            },
            PlaneEquation {
                origin: [0.0, 3.0, 0.0],
                normal: [0.0, 1.0, 0.0],
            },
        ],
        &[
            PlaneEquation {
                origin: [-9.0, 0.0, 0.0],
                normal: [1.0, 0.0, 0.0],
            },
            PlaneEquation {
                origin: [-8.0, 0.0, 0.0],
                normal: [1.0, 0.0, 0.0],
            },
            PlaneEquation {
                origin: [0.0, 0.0, -7.0],
                normal: [0.0, 0.0, 1.0],
            },
            PlaneEquation {
                origin: [0.0, 0.0, -6.0],
                normal: [0.0, 0.0, 1.0],
            },
        ],
    )
    .expect("fully constrained slot fillet");
    assert_eq!(cylinder.origin, [-8.5, -2.0, -6.5]);
    assert_eq!(cylinder.axis, [0.0, 1.0, 0.0]);
    assert_eq!(cylinder.radius, 0.5);
    assert!(slot_fillet_cylinder(
        [
            PlaneEquation {
                origin: [0.0, -2.0, 0.0],
                normal: [0.0, 1.0, 0.0],
            },
            PlaneEquation {
                origin: [0.0, 3.0, 0.0],
                normal: [0.0, 1.0, 0.0],
            },
        ],
        &[
            PlaneEquation {
                origin: [-9.0, 0.0, 0.0],
                normal: [1.0, 0.0, 0.0],
            },
            PlaneEquation {
                origin: [-8.0, 0.0, 0.0],
                normal: [1.0, 0.0, 0.0],
            },
        ],
    )
    .is_none());
}

#[test]
fn round_support_planes_define_radius_without_generated_surface_rows() {
    let mut scan = crate::container::scan_bytes(Vec::new());
    scan.features
        .affected_ids
        .push(crate::feature::FeatureAffectedIds {
            feature_id: 913,
            kind: crate::feature::AffectedIdKind::Geometry,
            ids: vec![1, 2, 3, 4],
            offset: 0,
        });
    let mut ir = CadIr::empty();
    for (id, origin, normal) in [
        (1, [0.0, 0.0, 0.0], [0.0, 1.0, 0.0]),
        (2, [0.0, 5.0, 0.0], [0.0, 1.0, 0.0]),
        (3, [-9.0, 0.0, 0.0], [1.0, 0.0, 0.0]),
        (4, [-8.0, 0.0, 0.0], [1.0, 0.0, 0.0]),
    ] {
        ir.model.surfaces.push(Surface {
            id: SurfaceId(format!("creo:visibgeom:surface#{id}")),
            geometry: SurfaceGeometry::Plane {
                origin: Point3::new(origin[0], origin[1], origin[2]),
                normal: Vector3::new(normal[0], normal[1], normal[2]),
                u_axis: Vector3::new(0.0, 0.0, 1.0),
            },
            source_object: None,
        });
    }

    assert_eq!(round_constant_radius(&scan, &ir, 913), Some(0.5));
}

#[test]
fn mixed_round_families_reconcile_placed_cylinders_and_prototype_tori() {
    let mut scan = crate::container::scan_bytes(Vec::new());
    scan.framing.layout = crate::container::Layout::Nd;
    scan.framing.sections.push(crate::container::Section {
        name: "VisibGeom".to_string(),
        raw_name: "VisibGeom".to_string(),
        offset: 0,
        length: 1_000,
        expanded_length: None,
        role: crate::container::role::GEOMETRY,
    });
    scan.surfaces.rows.extend([
        crate::surface::SurfaceRow {
            id: 11,
            type_byte: 0x24,
            kind: crate::surface::SurfaceKind::Cylinder,
            feature_id: 913,
            reversed: false,
            boundary_type: 0,
            next_surface: 0,
            offset: 100,
        },
        crate::surface::SurfaceRow {
            id: 12,
            type_byte: 0x26,
            kind: crate::surface::SurfaceKind::TorusOrSphere,
            feature_id: 913,
            reversed: false,
            boundary_type: 0,
            next_surface: 0,
            offset: 200,
        },
    ]);
    let replay_frame = crate::surface::SurfaceParameterScalarFrame {
        offset: 0,
        slots: vec![parameter_slot(0.5)],
    };
    scan.surfaces
        .parameters
        .push(crate::surface::SurfaceParameterRecord {
            surface_id: 12,
            body: vec![0],
            scalar_values: vec![0.5],
            scalar_tokens: replay_frame.slots.clone(),
            opaque_spans: Vec::new(),
            scalar_frames: vec![replay_frame.clone()],
            terminal_scalar_frame: Some(replay_frame),
            tabulated_cylinder_frame: None,
            positional_cylinder_frame: None,
            split_cylinder_outline_bounds: None,
            positional_cone_frame: None,
            positional_torus_frame: None,
            boundary: crate::surface::SurfaceBodyBoundary::CompoundClose,
            offset: 200,
            body_offset: 201,
        });
    let scalar = |name: &str, value: f64| crate::surface::SurfaceNamedParameter {
        name: name.to_string(),
        value: crate::surface::SurfaceNamedValue::ScalarSequence(vec![value]),
        body: Vec::new(),
        offset: 150,
        value_offset: 150,
    };
    scan.surfaces
        .prototype_records
        .push(crate::surface::SurfacePrototypeRecord {
            declared_family: "torus".to_string(),
            family: crate::surface::SurfacePrototypeFamily::Torus,
            parameters: vec![scalar("radius1", 10.0), scalar("radius2", 0.5)],
            offset: 150,
        });

    let mut ir = CadIr::empty();
    ir.model.surfaces.push(Surface {
        id: SurfaceId("creo:visibgeom:surface#11".to_string()),
        geometry: SurfaceGeometry::Cylinder {
            origin: Point3::new(0.0, 0.0, 0.0),
            axis: Vector3::new(0.0, 0.0, 1.0),
            ref_direction: Vector3::new(1.0, 0.0, 0.0),
            radius: 0.5,
        },
        source_object: None,
    });
    scan.features
        .affected_ids
        .push(crate::feature::FeatureAffectedIds {
            feature_id: 913,
            kind: crate::feature::AffectedIdKind::Geometry,
            ids: vec![1, 2, 3, 4],
            offset: 0,
        });
    for (id, x) in [(3, -9.0), (4, -8.0)] {
        ir.model.surfaces.push(Surface {
            id: SurfaceId(format!("creo:visibgeom:surface#{id}")),
            geometry: SurfaceGeometry::Plane {
                origin: Point3::new(x, 0.0, 0.0),
                normal: Vector3::new(1.0, 0.0, 0.0),
                u_axis: Vector3::new(0.0, 1.0, 0.0),
            },
            source_object: None,
        });
    }

    assert_eq!(round_constant_radius(&scan, &ir, 913), Some(0.5));

    if let Some(Surface {
        geometry: SurfaceGeometry::Cylinder { radius, .. },
        ..
    }) = ir.model.surfaces.first_mut()
    {
        *radius = 0.75;
    }
    assert_eq!(round_constant_radius(&scan, &ir, 913), None);
}

#[test]
fn placed_cylinder_samples_identify_variable_radius_with_unresolved_siblings() {
    let mut scan = crate::container::scan_bytes(Vec::new());
    for (id, kind) in [
        (11, crate::surface::SurfaceKind::Cylinder),
        (12, crate::surface::SurfaceKind::TorusOrSphere),
        (13, crate::surface::SurfaceKind::Cylinder),
    ] {
        scan.surfaces.rows.push(crate::surface::SurfaceRow {
            id,
            type_byte: 0,
            kind,
            feature_id: 5,
            reversed: false,
            boundary_type: 0,
            next_surface: 0,
            offset: id as usize,
        });
    }
    let mut ir = CadIr::empty();
    for (id, radius) in [(11, 15.0), (13, 1.0)] {
        ir.model.surfaces.push(Surface {
            id: SurfaceId(format!("creo:visibgeom:surface#{id}")),
            geometry: SurfaceGeometry::Cylinder {
                origin: Point3::new(0.0, 0.0, 0.0),
                axis: Vector3::new(0.0, 0.0, 1.0),
                ref_direction: Vector3::new(1.0, 0.0, 0.0),
                radius,
            },
            source_object: None,
        });
    }

    assert!(matches!(
        schema_feature_definition(&scan, &ir, 5, 913, "Round"),
        IrFeatureDefinition::Fillet {
            ref groups,
        } if matches!(
            groups.as_slice(),
            [cadmpeg_ir::features::FilletGroup {
                radius: RadiusSpec::Unresolved {
                    form: Some(RadiusForm::Variable),
                },
                ..
            }]
        )
    ));
}

#[test]
fn unequal_round_samples_are_not_hidden_by_support_radius() {
    let mut scan = crate::container::scan_bytes(Vec::new());
    for (id, parameter) in [(11, Some(15.0)), (12, Some(1.0)), (13, None)] {
        scan.surfaces.rows.push(crate::surface::SurfaceRow {
            id,
            type_byte: 0x24,
            kind: crate::surface::SurfaceKind::Cylinder,
            feature_id: 5,
            reversed: false,
            boundary_type: 0,
            next_surface: 0,
            offset: id as usize,
        });
        if let Some(radius) = parameter {
            let first = crate::surface::SurfaceParameterScalar {
                value: Some(1.0),
                raw: Vec::new(),
                offset: 1,
                length: 1,
            };
            let second = crate::surface::SurfaceParameterScalar {
                value: Some(1.0 + 2.0 * radius),
                raw: Vec::new(),
                offset: 3,
                length: 1,
            };
            let extent = [
                crate::surface::SurfaceParameterScalar {
                    value: Some(0.0),
                    raw: Vec::new(),
                    offset: 4,
                    length: 1,
                },
                crate::surface::SurfaceParameterScalar {
                    value: Some(0.0),
                    raw: Vec::new(),
                    offset: 5,
                    length: 1,
                },
                crate::surface::SurfaceParameterScalar {
                    value: Some(0.0),
                    raw: Vec::new(),
                    offset: 6,
                    length: 1,
                },
                crate::surface::SurfaceParameterScalar {
                    value: Some(2.0 * radius),
                    raw: Vec::new(),
                    offset: 7,
                    length: 1,
                },
                crate::surface::SurfaceParameterScalar {
                    value: Some(0.0),
                    raw: Vec::new(),
                    offset: 8,
                    length: 1,
                },
                crate::surface::SurfaceParameterScalar {
                    value: Some(0.0),
                    raw: Vec::new(),
                    offset: 9,
                    length: 1,
                },
            ];
            scan.surfaces
                .parameters
                .push(crate::surface::SurfaceParameterRecord {
                    surface_id: id,
                    body: vec![0x11, 0x00, 0x11, 0, 0, 0, 0, 0, 0, 0],
                    scalar_values: Vec::new(),
                    scalar_tokens: Vec::new(),
                    opaque_spans: Vec::new(),
                    scalar_frames: vec![
                        crate::surface::SurfaceParameterScalarFrame {
                            offset: 1,
                            slots: vec![first],
                        },
                        crate::surface::SurfaceParameterScalarFrame {
                            offset: 3,
                            slots: std::iter::once(second).chain(extent).collect(),
                        },
                    ],
                    terminal_scalar_frame: None,
                    tabulated_cylinder_frame: None,
                    positional_cylinder_frame: None,
                    split_cylinder_outline_bounds: None,
                    positional_cone_frame: None,
                    positional_torus_frame: None,
                    boundary: crate::surface::SurfaceBodyBoundary::CompoundClose,
                    offset: id as usize,
                    body_offset: id as usize + 1,
                });
        }
    }
    scan.features
        .affected_ids
        .push(crate::feature::FeatureAffectedIds {
            feature_id: 5,
            kind: crate::feature::AffectedIdKind::Geometry,
            ids: vec![1, 2, 3, 4],
            offset: 0,
        });

    let mut ir = CadIr::empty();
    for (id, origin, normal) in [
        (1, [0.0, 0.0, 0.0], [0.0, 1.0, 0.0]),
        (2, [0.0, 5.0, 0.0], [0.0, 1.0, 0.0]),
        (3, [-9.0, 0.0, 0.0], [1.0, 0.0, 0.0]),
        (4, [-8.0, 0.0, 0.0], [1.0, 0.0, 0.0]),
    ] {
        ir.model.surfaces.push(Surface {
            id: SurfaceId(format!("creo:visibgeom:surface#{id}")),
            geometry: SurfaceGeometry::Plane {
                origin: Point3::new(origin[0], origin[1], origin[2]),
                normal: Vector3::new(normal[0], normal[1], normal[2]),
                u_axis: Vector3::new(0.0, 0.0, 1.0),
            },
            source_object: None,
        });
    }

    assert_eq!(round_observed_radii(&scan, 5), [15.0, 1.0]);
    assert_eq!(round_support_radius(&scan, &ir, 5), Some(0.5));
    assert_eq!(round_constant_radius(&scan, &ir, 5), None);
    assert!(matches!(
        schema_feature_definition(&scan, &ir, 5, 913, "Round"),
        IrFeatureDefinition::Fillet {
            groups,
        } if matches!(
            groups.as_slice(),
            [cadmpeg_ir::features::FilletGroup {
                radius: RadiusSpec::Unresolved {
                    form: Some(RadiusForm::Variable),
                },
                ..
            }]
        )
    ));
}

#[test]
fn unequal_placed_round_cylinders_are_not_hidden_by_support_radius() {
    let mut scan = crate::container::scan_bytes(Vec::new());
    for id in [11, 12] {
        scan.surfaces.rows.push(crate::surface::SurfaceRow {
            id,
            type_byte: 0x24,
            kind: crate::surface::SurfaceKind::Cylinder,
            feature_id: 5,
            reversed: false,
            boundary_type: 0,
            next_surface: 0,
            offset: id as usize,
        });
    }
    scan.features
        .affected_ids
        .push(crate::feature::FeatureAffectedIds {
            feature_id: 5,
            kind: crate::feature::AffectedIdKind::Geometry,
            ids: vec![1, 2, 3, 4],
            offset: 0,
        });

    let mut ir = CadIr::empty();
    for (id, radius) in [(11, 15.0), (12, 1.0)] {
        ir.model.surfaces.push(Surface {
            id: SurfaceId(format!("creo:visibgeom:surface#{id}")),
            geometry: SurfaceGeometry::Cylinder {
                origin: Point3::new(0.0, 0.0, 0.0),
                axis: Vector3::new(0.0, 0.0, 1.0),
                ref_direction: Vector3::new(1.0, 0.0, 0.0),
                radius,
            },
            source_object: None,
        });
    }
    for (id, origin, normal) in [
        (1, [0.0, 0.0, 0.0], [0.0, 1.0, 0.0]),
        (2, [0.0, 5.0, 0.0], [0.0, 1.0, 0.0]),
        (3, [-9.0, 0.0, 0.0], [1.0, 0.0, 0.0]),
        (4, [-8.0, 0.0, 0.0], [1.0, 0.0, 0.0]),
    ] {
        ir.model.surfaces.push(Surface {
            id: SurfaceId(format!("creo:visibgeom:surface#{id}")),
            geometry: SurfaceGeometry::Plane {
                origin: Point3::new(origin[0], origin[1], origin[2]),
                normal: Vector3::new(normal[0], normal[1], normal[2]),
                u_axis: Vector3::new(0.0, 0.0, 1.0),
            },
            source_object: None,
        });
    }

    assert_eq!(round_placed_cylinder_radii(&scan, &ir, 5), [15.0, 1.0]);
    assert_eq!(round_support_radius(&scan, &ir, 5), Some(0.5));
    assert_eq!(round_constant_radius(&scan, &ir, 5), None);
    assert!(matches!(
        schema_feature_definition(&scan, &ir, 5, 913, "Round"),
        IrFeatureDefinition::Fillet {
            groups,
        } if matches!(
            groups.as_slice(),
            [cadmpeg_ir::features::FilletGroup {
                radius: RadiusSpec::Unresolved {
                    form: Some(RadiusForm::Variable),
                },
                ..
            }]
        )
    ));
}

#[test]
fn unequal_mixed_round_cylinders_are_not_hidden_by_unresolved_torus() {
    let mut scan = crate::container::scan_bytes(Vec::new());
    for (id, kind) in [
        (11, crate::surface::SurfaceKind::Cylinder),
        (12, crate::surface::SurfaceKind::TorusOrSphere),
        (13, crate::surface::SurfaceKind::Cylinder),
    ] {
        scan.surfaces.rows.push(crate::surface::SurfaceRow {
            id,
            type_byte: if kind == crate::surface::SurfaceKind::TorusOrSphere {
                0x26
            } else {
                0x24
            },
            kind,
            feature_id: 5,
            reversed: false,
            boundary_type: 0,
            next_surface: 0,
            offset: id as usize,
        });
    }
    scan.features
        .affected_ids
        .push(crate::feature::FeatureAffectedIds {
            feature_id: 5,
            kind: crate::feature::AffectedIdKind::Geometry,
            ids: vec![1, 2, 3, 4],
            offset: 0,
        });

    let mut ir = CadIr::empty();
    for (id, radius) in [(11, 15.0), (13, 1.0)] {
        ir.model.surfaces.push(Surface {
            id: SurfaceId(format!("creo:visibgeom:surface#{id}")),
            geometry: SurfaceGeometry::Cylinder {
                origin: Point3::new(0.0, 0.0, 0.0),
                axis: Vector3::new(0.0, 0.0, 1.0),
                ref_direction: Vector3::new(1.0, 0.0, 0.0),
                radius,
            },
            source_object: None,
        });
    }
    for (id, origin, normal) in [
        (1, [0.0, 0.0, 0.0], [0.0, 1.0, 0.0]),
        (2, [0.0, 5.0, 0.0], [0.0, 1.0, 0.0]),
        (3, [-9.0, 0.0, 0.0], [1.0, 0.0, 0.0]),
        (4, [-8.0, 0.0, 0.0], [1.0, 0.0, 0.0]),
    ] {
        ir.model.surfaces.push(Surface {
            id: SurfaceId(format!("creo:visibgeom:surface#{id}")),
            geometry: SurfaceGeometry::Plane {
                origin: Point3::new(origin[0], origin[1], origin[2]),
                normal: Vector3::new(normal[0], normal[1], normal[2]),
                u_axis: Vector3::new(0.0, 0.0, 1.0),
            },
            source_object: None,
        });
    }

    assert_eq!(round_placed_cylinder_radii(&scan, &ir, 5), [15.0, 1.0]);
    assert_eq!(round_support_radius(&scan, &ir, 5), Some(0.5));
    assert_eq!(round_constant_radius(&scan, &ir, 5), None);
    assert!(matches!(
        schema_feature_definition(&scan, &ir, 5, 913, "Round"),
        IrFeatureDefinition::Fillet {
            groups,
        } if matches!(
            groups.as_slice(),
            [cadmpeg_ir::features::FilletGroup {
                radius: RadiusSpec::Unresolved {
                    form: Some(RadiusForm::Variable),
                },
                ..
            }]
        )
    ));
}

#[test]
fn opposite_reference_caps_select_one_round_envelope_axis() {
    let circle = |entity_id, axis, start, end| crate::reference::ReferenceCircle {
        entity_id,
        center: [0.0; 3],
        center_stored: true,
        radius: 2.0,
        axis,
        start,
        end,
        offset: 0,
    };
    let envelope = crate::surface::Type24RoundEnvelope {
        diameter: 2.0,
        extent_endpoints: [[3.5, 8.0, -6.0], [5.5, 10.0, -4.0]],
    };
    let first = circle(367, [0.0, 0.0, 1.0], [3.5, 8.0, -6.0], [5.5, 10.0, -6.0]);
    let second = circle(368, [0.0, 0.0, -1.0], [5.5, 10.0, -4.0], [3.5, 8.0, -4.0]);
    let frame =
        reference_cap_bound_round_frame(envelope, &[&first, &second]).expect("opposite Z caps");
    assert_eq!(frame.origin, [4.5, 9.0, -6.0]);
    assert_eq!(frame.axis, [0.0, 0.0, 1.0]);
    assert_eq!(frame.ref_direction, [1.0, 0.0, 0.0]);
    assert_eq!(frame.radius, 1.0);
    assert_eq!(frame.length, Some(2.0));
    assert!(reference_cap_bound_round_frame(envelope, &[&first]).is_none());

    let x_first = circle(371, [1.0, 0.0, 0.0], [3.5, 8.0, -6.0], [3.5, 10.0, -4.0]);
    let x_second = circle(372, [-1.0, 0.0, 0.0], [5.5, 10.0, -4.0], [5.5, 8.0, -6.0]);
    assert!(
        reference_cap_bound_round_frame(envelope, &[&first, &second, &x_first, &x_second])
            .is_none()
    );

    let crossed_first = circle(369, [0.0, 0.0, -1.0], [5.5, 8.0, -6.0], [3.5, 10.0, -6.0]);
    let crossed_second = circle(370, [0.0, 0.0, 1.0], [3.5, 10.0, -4.0], [5.5, 8.0, -4.0]);
    assert_eq!(
        reference_cap_bound_round_frame(envelope, &[&crossed_first, &crossed_second]),
        Some(frame)
    );
    assert!(reference_cap_bound_round_frame(envelope, &[&first, &crossed_second]).is_none());
}

#[test]
fn coaxial_reference_circles_define_a_cylinder_frame() {
    let circle = |entity_id, center, axis, start| crate::reference::ReferenceCircle {
        entity_id,
        center,
        center_stored: true,
        radius: 2.0,
        axis,
        start,
        end: [0.0, 0.0, 0.0],
        offset: 0,
    };
    let first = circle(41, [3.0, 5.0, -2.0], [0.0, 0.0, 1.0], [3.0, 7.0, -2.0]);
    let second = circle(42, [3.0, 5.0, 4.0], [0.0, 0.0, -1.0], [1.0, 5.0, 4.0]);

    assert_eq!(
        reference_circle_pair_cylinder_frame(&[&first, &second]),
        Some(crate::surface::PositionalCylinderFrame {
            origin: first.center,
            axis: [0.0, 0.0, 1.0],
            ref_direction: [0.0, 1.0, 0.0],
            radius: 2.0,
            length: Some(6.0),
        })
    );
    assert!(reference_circle_pair_cylinder_frame(&[&first]).is_none());

    let mut unequal_radius = second.clone();
    unequal_radius.radius = 1.0;
    assert!(reference_circle_pair_cylinder_frame(&[&first, &unequal_radius]).is_none());

    let displaced = circle(43, [3.5, 5.0, 4.0], [0.0, 0.0, 1.0], [3.5, 7.0, 4.0]);
    assert!(reference_circle_pair_cylinder_frame(&[&first, &displaced]).is_none());

    let mut derived_center = second;
    derived_center.center_stored = false;
    assert!(reference_circle_pair_cylinder_frame(&[&first, &derived_center]).is_none());
}

#[test]
fn asymmetric_cap_planes_define_two_sided_extent() {
    assert_eq!(
        extrusion_extent_and_direction(
            [0.0; 3],
            [0.0, 0.0, 1.0],
            [
                ([0.0, 0.0, -2.0], [0.0, 0.0, 1.0]),
                ([0.0, 0.0, 3.0], [0.0, 0.0, 1.0]),
            ],
        ),
        Some((
            ExtrudeExtent::TwoSided {
                first: ExtrudeSide {
                    termination: Termination::Blind {
                        length: Length(3.0),
                    },
                    draft: None,
                    offset: None,
                },
                second: ExtrudeSide {
                    termination: Termination::Blind {
                        length: Length(2.0),
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
fn one_negative_cap_offset_reverses_blind_direction() {
    assert_eq!(
        extrusion_extent_and_direction(
            [0.0; 3],
            [0.0, -1.0, 0.0],
            [([0.0, 48.0, 0.0], [0.0, 1.0, 0.0])],
        ),
        Some((
            ExtrudeExtent::OneSided {
                side: ExtrudeSide {
                    termination: Termination::Blind {
                        length: Length(48.0),
                    },
                    draft: None,
                    offset: None,
                },
            },
            [-0.0, 1.0, -0.0],
        ))
    );
}

#[test]
fn zero_offset_support_plane_does_not_obscure_blind_cap() {
    assert_eq!(
        extrusion_extent_and_direction(
            [0.0; 3],
            [0.0, 1.0, 0.0],
            [
                ([20.0, 0.0, 6.0], [0.0, 1.0, 0.0]),
                ([0.0, 48.0, 0.0], [0.0, 1.0, 0.0]),
            ],
        ),
        Some((
            ExtrudeExtent::OneSided {
                side: ExtrudeSide {
                    termination: Termination::Blind {
                        length: Length(48.0),
                    },
                    draft: None,
                    offset: None,
                },
            },
            [0.0, 1.0, 0.0],
        ))
    );
}

#[test]
fn interior_axis_normal_planes_do_not_shorten_blind_extent() {
    assert_eq!(
        extrusion_extent_and_direction(
            [0.0; 3],
            [0.0, -1.0, 0.0],
            [
                ([0.0, 38.0, 0.0], [0.0, 1.0, 0.0]),
                ([3.0, 2.5, 7.0], [0.0, -1.0, 0.0]),
                ([-4.0, 5.75, 1.0], [0.0, 1.0, 0.0]),
            ],
        ),
        Some((
            ExtrudeExtent::OneSided {
                side: ExtrudeSide {
                    termination: Termination::Blind {
                        length: Length(38.0),
                    },
                    draft: None,
                    offset: None,
                },
            },
            [-0.0, 1.0, -0.0],
        ))
    );
}

#[test]
fn agreeing_generated_cylinders_define_blind_extrusion_extent() {
    let transform = crate::placement::FeatureSectionTransform {
        definition_id: 917,
        feature_id: Some(40),
        origin: [0.0, 4.0, 0.0],
        u_axis: [1.0, 0.0, 0.0],
        v_axis: [0.0, 0.0, -1.0],
        normal: [0.0, 1.0, 0.0],
        offset: 100,
    };
    let frame = |origin| crate::surface::PositionalCylinderFrame {
        origin,
        axis: [0.0, 1.0, 0.0],
        ref_direction: [1.0, 0.0, 0.0],
        radius: 0.75,
        length: Some(34.0),
    };
    let frames = [frame([-12.5, 4.0, 0.0]), frame([12.5, 4.0, 0.0])];
    assert_eq!(
        agreed_generated_cylinder_extent(&transform, &frames),
        Some((
            ExtrudeExtent::OneSided {
                side: ExtrudeSide {
                    termination: Termination::Blind {
                        length: Length(34.0)
                    },
                    draft: None,
                    offset: None,
                }
            },
            [0.0, 1.0, 0.0]
        ))
    );
    assert_eq!(
        directed_blind_extrusion_span(transform.normal, [0.0, 1.0, 0.0], 34.0),
        Some(ExtrusionSpan {
            lower: 0.0,
            upper: 34.0,
        })
    );
    assert_eq!(
        directed_blind_extrusion_span(transform.normal, [0.0, -1.0, 0.0], 34.0),
        Some(ExtrusionSpan {
            lower: -34.0,
            upper: 0.0,
        })
    );
    assert!(directed_blind_extrusion_span(transform.normal, [1.0, 0.0, 0.0], 34.0).is_none());

    let mut inconsistent = frames;
    inconsistent[1].length = Some(33.0);
    assert!(agreed_generated_cylinder_extent(&transform, &inconsistent).is_none());
    inconsistent = frames;
    inconsistent[1].origin[1] = 5.0;
    assert!(agreed_generated_cylinder_extent(&transform, &inconsistent).is_none());

    let diagonal = 0.5_f64.sqrt();
    let diagonal_transform = crate::placement::FeatureSectionTransform {
        normal: [diagonal, diagonal, 0.0],
        ..transform
    };
    let perpendicular = [crate::surface::PositionalCylinderFrame {
        origin: diagonal_transform.origin,
        axis: [diagonal, -diagonal, 0.0],
        ..frames[0]
    }];
    assert!(agreed_generated_cylinder_extent(&diagonal_transform, &perpendicular).is_none());
}

#[test]
fn generated_cylinder_extent_uses_unique_available_parameter_frames() {
    let frame = crate::surface::PositionalCylinderFrame {
        origin: [1.0, 2.0, 3.0],
        axis: [0.0, 0.0, 1.0],
        ref_direction: [1.0, 0.0, 0.0],
        radius: 4.0,
        length: Some(5.0),
    };
    let parameter =
        |surface_id, positional_cylinder_frame| crate::surface::SurfaceParameterRecord {
            surface_id,
            body: Vec::new(),
            scalar_values: Vec::new(),
            scalar_tokens: Vec::new(),
            opaque_spans: Vec::new(),
            scalar_frames: Vec::new(),
            terminal_scalar_frame: None,
            tabulated_cylinder_frame: None,
            positional_cylinder_frame,
            split_cylinder_outline_bounds: None,
            positional_cone_frame: None,
            positional_torus_frame: None,
            boundary: crate::surface::SurfaceBodyBoundary::CompoundClose,
            offset: 0,
            body_offset: 0,
        };
    let surface_ids = BTreeSet::from([1, 2, 3]);
    let parameters = [parameter(1, Some(frame)), parameter(2, None)];
    assert_eq!(
        unique_available_positional_cylinder_frame_records(&surface_ids, &parameters),
        Some(vec![(1, frame)])
    );

    let duplicates = [parameter(1, Some(frame)), parameter(1, Some(frame))];
    assert!(
        unique_available_positional_cylinder_frame_records(&surface_ids, &duplicates).is_none()
    );
}

#[test]
fn bounded_generated_cylinders_define_a_blind_extrusion() {
    let row = |id, kind: crate::surface::SurfaceKind| crate::surface::SurfaceRow {
        id,
        type_byte: kind.canonical_type_byte(),
        kind,
        feature_id: 7,
        reversed: false,
        boundary_type: 0,
        next_surface: 0,
        offset: id as usize,
    };
    let mut scan = crate::container::scan_bytes(Vec::new());
    scan.surfaces.rows.extend([
        row(31, crate::surface::SurfaceKind::Plane),
        row(32, crate::surface::SurfaceKind::Plane),
        row(33, crate::surface::SurfaceKind::Cylinder),
    ]);
    let parameter = crate::surface::SurfaceParameterRecord {
        surface_id: 33,
        body: Vec::new(),
        scalar_values: Vec::new(),
        scalar_tokens: Vec::new(),
        opaque_spans: Vec::new(),
        scalar_frames: Vec::new(),
        terminal_scalar_frame: None,
        tabulated_cylinder_frame: None,
        positional_cylinder_frame: Some(crate::surface::PositionalCylinderFrame {
            origin: [2.0, 4.0, 0.0],
            axis: [0.0, -1.0, 0.0],
            ref_direction: [1.0, 0.0, 0.0],
            radius: 1.0,
            length: Some(8.0),
        }),
        split_cylinder_outline_bounds: None,
        positional_cone_frame: None,
        positional_torus_frame: None,
        boundary: crate::surface::SurfaceBodyBoundary::CompoundClose,
        offset: 33,
        body_offset: 34,
    };
    scan.surfaces.parameters.push(parameter);
    let plane = |id, y, normal| Surface {
        id: SurfaceId(format!("creo:visibgeom:surface#{id}")),
        geometry: SurfaceGeometry::Plane {
            origin: Point3::new(0.0, y, 0.0),
            normal,
            u_axis: Vector3::new(1.0, 0.0, 0.0),
        },
        source_object: None,
    };
    let mut ir = CadIr::empty();
    ir.model.surfaces.extend([
        plane(31, 4.0, Vector3::new(0.0, 1.0, 0.0)),
        plane(32, -4.0, Vector3::new(0.0, -1.0, 0.0)),
        Surface {
            id: SurfaceId("creo:visibgeom:surface#33".to_string()),
            geometry: SurfaceGeometry::Cylinder {
                origin: Point3::new(2.0, 4.0, 0.0),
                axis: Vector3::new(0.0, -1.0, 0.0),
                ref_direction: Vector3::new(1.0, 0.0, 0.0),
                radius: 1.0,
            },
            source_object: None,
        },
    ]);

    let expected = Some((
        ExtrudeExtent::OneSided {
            side: ExtrudeSide {
                termination: Termination::Blind {
                    length: Length(8.0),
                },
                draft: None,
                offset: None,
            },
        },
        [0.0, -1.0, 0.0],
    ));
    assert_eq!(
        generated_bounded_cylinder_extent(&scan, &ir, 7, None),
        expected
    );

    scan.planes.outlines.push(crate::surface::OutlinePlane {
        surface_id: 31,
        origin: [0.0, 5.0, 0.0],
        normal: [0.0, 1.0, 0.0],
        u_axis: [1.0, 0.0, 0.0],
        offset: 31,
    });
    let conflicting_extent = generated_bounded_cylinder_extent(&scan, &ir, 7, None);
    assert!(conflicting_extent.is_none());
    scan.planes.outlines[0].origin[1] = 4.0;
    assert_eq!(
        generated_bounded_cylinder_extent(&scan, &ir, 7, None),
        expected
    );

    scan.surfaces
        .rows
        .push(row(34, crate::surface::SurfaceKind::Plane));
    ir.model.surfaces.push(Surface {
        id: SurfaceId("creo:visibgeom:surface#34".to_string()),
        geometry: SurfaceGeometry::Unknown { record: None },
        source_object: None,
    });
    assert_eq!(
        generated_bounded_cylinder_extent(&scan, &ir, 7, None),
        expected
    );

    scan.surfaces
        .rows
        .push(row(35, crate::surface::SurfaceKind::Cylinder));
    ir.model.surfaces.push(Surface {
        id: SurfaceId("creo:visibgeom:surface#35".to_string()),
        geometry: SurfaceGeometry::Unknown { record: None },
        source_object: None,
    });
    assert_eq!(
        generated_bounded_cylinder_extent(&scan, &ir, 7, None),
        expected
    );
    scan.surfaces.rows.truncate(3);
    ir.model.surfaces.truncate(3);

    let mut untransferred_caps = ir.clone();
    untransferred_caps
        .model
        .surfaces
        .retain(|surface| surface.id == SurfaceId("creo:visibgeom:surface#33".to_string()));
    assert_eq!(
        generated_bounded_cylinder_extent(&scan, &untransferred_caps, 7, None),
        generated_bounded_cylinder_extent(&scan, &ir, 7, None)
    );

    scan.surfaces.parameters[0]
        .positional_cylinder_frame
        .as_mut()
        .expect("cylinder frame")
        .length = None;
    assert_eq!(
        generated_bounded_cylinder_extent(&scan, &ir, 7, None),
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
            [0.0, -1.0, 0.0],
        ))
    );
    assert!(generated_bounded_cylinder_extent(&scan, &untransferred_caps, 7, None).is_none());
    let lengthless = scan.surfaces.parameters[0]
        .positional_cylinder_frame
        .expect("cylinder frame");
    assert!(bounded_cylinder_span(
        lengthless,
        &[
            ([0.0, -4.0, 0.0], [0.0, 1.0, 0.0]),
            ([0.0, -6.0, 0.0], [0.0, 1.0, 0.0]),
        ],
    )
    .is_none());
    let invalid_length = crate::surface::PositionalCylinderFrame {
        length: Some(0.0),
        ..lengthless
    };
    assert!(
        bounded_cylinder_span(invalid_length, &[([0.0, -4.0, 0.0], [0.0, 1.0, 0.0])]).is_none()
    );
    scan.surfaces.parameters[0]
        .positional_cylinder_frame
        .as_mut()
        .expect("cylinder frame")
        .length = Some(8.0);

    let transform = crate::placement::FeatureSectionTransform {
        definition_id: 7,
        feature_id: Some(7),
        origin: [0.0, 4.0, 0.0],
        u_axis: [1.0, 0.0, 0.0],
        v_axis: [0.0, 0.0, 1.0],
        normal: [0.0, -1.0, 0.0],
        offset: 0,
    };
    assert_eq!(
        generated_bounded_cylinder_extent(&scan, &untransferred_caps, 7, Some(&transform)),
        generated_bounded_cylinder_extent(&scan, &ir, 7, None)
    );
    let definition = crate::feature::FeatureDefinition {
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
    let surface_rows = std::mem::take(&mut scan.surfaces.rows);
    scan.surfaces.rows = surface_rows
        .iter()
        .filter(|row| row.id == 33)
        .cloned()
        .collect();
    let model_surfaces = std::mem::take(&mut ir.model.surfaces);
    ir.model.surfaces = model_surfaces
        .iter()
        .filter(|surface| surface.id == SurfaceId::from("creo:visibgeom:surface#33"))
        .cloned()
        .collect();
    assert_eq!(
        resolved_feature_extrusion_span(&scan, &ir, &definition, &transform),
        Some(ExtrusionSpan {
            lower: 0.0,
            upper: 8.0,
        })
    );
    scan.surfaces.rows = surface_rows;
    ir.model.surfaces = model_surfaces;
    let displaced = crate::placement::FeatureSectionTransform {
        origin: [0.0, 3.0, 0.0],
        ..transform.clone()
    };
    assert!(
        generated_bounded_cylinder_extent(&scan, &untransferred_caps, 7, Some(&displaced))
            .is_none()
    );
    let perpendicular = crate::placement::FeatureSectionTransform {
        normal: [1.0, 0.0, 0.0],
        ..transform
    };
    assert!(
        generated_bounded_cylinder_extent(&scan, &untransferred_caps, 7, Some(&perpendicular))
            .is_none()
    );

    let mut oblique = ir.clone();
    let SurfaceGeometry::Plane { normal, .. } = &mut oblique.model.surfaces[0].geometry else {
        panic!("plane");
    };
    *normal = Vector3::new(0.0, 1.0, 1.0);
    assert!(generated_bounded_cylinder_extent(&scan, &oblique, 7, None).is_none());

    scan.surfaces.parameters[0]
        .positional_cylinder_frame
        .as_mut()
        .expect("cylinder frame")
        .length = Some(7.0);
    assert!(generated_bounded_cylinder_extent(&scan, &ir, 7, None).is_none());
    scan.surfaces.parameters[0]
        .positional_cylinder_frame
        .as_mut()
        .expect("cylinder frame")
        .length = Some(8.0);

    scan.surfaces.rows.push(scan.surfaces.rows[0].clone());
    assert!(generated_bounded_cylinder_extent(&scan, &ir, 7, None).is_none());
    scan.surfaces.rows.pop();

    scan.surfaces
        .parameters
        .push(scan.surfaces.parameters[0].clone());
    assert!(generated_bounded_cylinder_extent(&scan, &ir, 7, None).is_none());
    scan.surfaces.parameters.pop();

    let mut missing_transfer = ir.clone();
    missing_transfer.model.surfaces.pop();
    assert!(generated_bounded_cylinder_extent(&scan, &missing_transfer, 7, None).is_none());
}

#[test]
fn terminal_plane_orients_oppositely_parameterized_extrusion_carriers() {
    let carriers = [
        ExtrusionCarrierSpan {
            starts: vec![[0.0, 5.5, 0.0]],
            vector: [0.0, 2.0, 0.0],
        },
        ExtrusionCarrierSpan {
            starts: vec![[4.0, 7.5, 0.0]],
            vector: [0.0, -2.0, 0.0],
        },
    ];
    let terminal_plane = [([0.0, 7.5, 0.0], [0.0, 1.0, 0.0])];
    assert_eq!(
        blind_extrusion_from_carriers(&carriers, &terminal_plane, None),
        Some((
            ExtrudeExtent::OneSided {
                side: ExtrudeSide {
                    termination: Termination::Blind {
                        length: Length(2.0),
                    },
                    draft: None,
                    offset: None,
                },
            },
            [0.0, 1.0, 0.0],
        ))
    );

    let reversed = [
        ExtrusionCarrierSpan {
            starts: vec![[4.0, 7.5, 0.0]],
            vector: [0.0, -2.0, 0.0],
        },
        ExtrusionCarrierSpan {
            starts: vec![[0.0, 5.5, 0.0]],
            vector: [0.0, 2.0, 0.0],
        },
    ];
    assert_eq!(
        blind_extrusion_from_carriers(&reversed, &terminal_plane, None),
        blind_extrusion_from_carriers(&carriers, &terminal_plane, None)
    );
    assert!(blind_extrusion_from_carriers(&carriers, &[], None).is_none());
}

#[test]
fn ordered_parallel_caps_define_blind_direction_and_depth() {
    let start = PlaneEquation {
        origin: [2.0, 7.0, 3.0],
        normal: [0.0, 0.0, -2.0],
    };
    let end = PlaneEquation {
        origin: [-4.0, 11.0, 13.0],
        normal: [0.0, 0.0, 5.0],
    };

    assert_eq!(
        ordered_parallel_cap_extent(start, end),
        Some((
            ExtrudeExtent::OneSided {
                side: ExtrudeSide {
                    termination: Termination::Blind {
                        length: Length(10.0),
                    },
                    draft: None,
                    offset: None,
                },
            },
            [0.0, 0.0, 1.0],
        ))
    );
    assert_eq!(
        ordered_parallel_cap_extent(end, start),
        Some((
            ExtrudeExtent::OneSided {
                side: ExtrudeSide {
                    termination: Termination::Blind {
                        length: Length(10.0),
                    },
                    draft: None,
                    offset: None,
                },
            },
            [0.0, 0.0, -1.0],
        ))
    );

    let tilted = PlaneEquation {
        origin: end.origin,
        normal: [0.0, 1.0, 1.0],
    };
    assert!(ordered_parallel_cap_extent(start, tilted).is_none());
    assert!(ordered_parallel_cap_extent(start, start).is_none());
}

#[test]
fn generated_table_cap_classes_bind_the_ordered_cap_planes() {
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
        feature_id: Some(7),
        table_class_id: 29,
        entry_ids: vec![31, 32, 33],
        entries: vec![
            entry(31, 204, None),
            entry(32, 203, None),
            entry(33, 200, Some(11)),
        ],
        surface_ids: vec![31, 32, 33],
        non_surface_entity_ids: Vec::new(),
        offset: 0,
    };
    let row = |id| crate::surface::SurfaceRow {
        id,
        type_byte: crate::surface::SurfaceKind::Plane.canonical_type_byte(),
        kind: crate::surface::SurfaceKind::Plane,
        feature_id: 7,
        reversed: id == 31,
        boundary_type: 0,
        next_surface: 0,
        offset: id as usize,
    };
    let plane = |id, z| Surface {
        id: SurfaceId(format!("creo:visibgeom:surface#{id}")),
        geometry: SurfaceGeometry::Plane {
            origin: Point3::new(4.0, -2.0, z),
            normal: Vector3::new(0.0, 0.0, 1.0),
            u_axis: Vector3::new(1.0, 0.0, 0.0),
        },
        source_object: None,
    };
    let mut scan = crate::container::scan_bytes(Vec::new());
    scan.features.entity_tables.push(table.clone());
    scan.surfaces.rows.extend([row(31), row(32), row(33)]);
    let mut ir = CadIr::empty();
    ir.model.surfaces.extend([plane(31, 2.0), plane(32, 8.0)]);

    assert_eq!(
        generated_cap_plane_extent(&scan, &ir, 7),
        Some((
            ExtrudeExtent::OneSided {
                side: ExtrudeSide {
                    termination: Termination::Blind {
                        length: Length(6.0),
                    },
                    draft: None,
                    offset: None,
                },
            },
            [0.0, 0.0, 1.0],
        ))
    );

    scan.features.entity_tables[0].entries[2].source_entity_id = None;
    assert!(generated_cap_plane_extent(&scan, &ir, 7).is_none());
    scan.features.entity_tables[0] = table.clone();
    scan.features.entity_tables.push(table);
    assert!(generated_cap_plane_extent(&scan, &ir, 7).is_none());
}

#[test]
fn rectilinear_generated_planes_define_one_axial_extrusion_family() {
    let row = |id, reversed| crate::surface::SurfaceRow {
        id,
        type_byte: crate::surface::SurfaceKind::Plane.canonical_type_byte(),
        kind: crate::surface::SurfaceKind::Plane,
        feature_id: 7,
        reversed,
        boundary_type: 0,
        next_surface: 0,
        offset: id as usize,
    };
    let mut scan = crate::container::scan_bytes(Vec::new());
    scan.surfaces.rows.extend([
        row(37, false),
        row(31, false),
        row(32, true),
        row(33, true),
        row(34, true),
        row(36, false),
        row(35, true),
    ]);
    let plane = |id, origin, normal| Surface {
        id: SurfaceId(format!("creo:visibgeom:surface#{id}")),
        geometry: SurfaceGeometry::Plane {
            origin,
            normal,
            u_axis: Vector3::new(0.0, 0.0, 1.0),
        },
        source_object: None,
    };
    let mut ir = CadIr::empty();
    ir.model.surfaces.extend([
        Surface {
            id: SurfaceId("creo:visibgeom:surface#37".to_string()),
            geometry: SurfaceGeometry::Unknown { record: None },
            source_object: None,
        },
        plane(31, Point3::new(0.0, 6.0, 0.0), Vector3::new(0.0, 1.0, 0.0)),
        plane(32, Point3::new(0.0, 48.0, 0.0), Vector3::new(0.0, 1.0, 0.0)),
        plane(33, Point3::new(4.0, 48.0, 0.0), Vector3::new(1.0, 0.0, 0.0)),
        plane(
            34,
            Point3::new(-4.0, 48.0, 0.0),
            Vector3::new(1.0, 0.0, 0.0),
        ),
        plane(36, Point3::new(0.0, 30.0, 0.0), Vector3::new(0.0, 1.0, 0.0)),
        plane(35, Point3::new(0.0, 48.0, 0.0), Vector3::new(0.0, 1.0, 0.0)),
    ]);
    let mut section = crate::feature::FeatureSection3d {
        sketch_plane_entity_id: Some(30),
        sketch_plane_flip: Some(crate::feature::BinaryFlag::Clear),
        reference_plane_entity_ids: vec![29],
        reference_plane_rows: Vec::new(),
        reference_plane_datum_geometry_id: None,
        orientation: crate::feature::FeatureSectionOrientation {
            section_flip: Some(crate::feature::BinaryFlag::Set),
            ..Default::default()
        },
        dimension_ids: Vec::new(),
        offset: 0,
    };

    assert_eq!(
        generated_rectilinear_plane_extent(&scan, &ir, 7, Some(&section)),
        Some((
            ExtrudeExtent::OneSided {
                side: ExtrudeSide {
                    termination: Termination::Blind {
                        length: Length(42.0),
                    },
                    draft: None,
                    offset: None,
                },
            },
            [0.0, -1.0, 0.0],
        ))
    );
    section.sketch_plane_flip = Some(crate::feature::BinaryFlag::Set);
    assert_eq!(
        generated_rectilinear_plane_extent(&scan, &ir, 7, Some(&section)),
        Some((
            ExtrudeExtent::OneSided {
                side: ExtrudeSide {
                    termination: Termination::Blind {
                        length: Length(42.0),
                    },
                    draft: None,
                    offset: None,
                },
            },
            [0.0, 1.0, 0.0],
        ))
    );
    section.orientation.section_flip = Some(crate::feature::BinaryFlag::Clear);
    assert_eq!(
        generated_rectilinear_plane_extent(&scan, &ir, 7, Some(&section)),
        Some((
            ExtrudeExtent::OneSided {
                side: ExtrudeSide {
                    termination: Termination::Blind {
                        length: Length(42.0),
                    },
                    draft: None,
                    offset: None,
                },
            },
            [0.0, -1.0, 0.0],
        ))
    );
    assert!(generated_rectilinear_plane_extent(&scan, &ir, 7, None).is_none());
    let mut incomplete_section = section.clone();
    incomplete_section.sketch_plane_entity_id = None;
    assert!(generated_rectilinear_plane_extent(&scan, &ir, 7, Some(&incomplete_section)).is_none());

    scan.surfaces.rows[3].reversed = false;
    assert!(generated_rectilinear_plane_extent(&scan, &ir, 7, Some(&section)).is_none());
    scan.surfaces.rows[3].reversed = true;
    ir.model.surfaces.pop();
    assert!(generated_rectilinear_plane_extent(&scan, &ir, 7, Some(&section)).is_none());
}
