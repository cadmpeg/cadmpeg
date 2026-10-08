// SPDX-License-Identifier: Apache-2.0
//! Tests: blind circular.

use super::parameter_slot;
use crate::decode::analytic::equations::PlaneEquation;
use crate::decode::feature_history::draft::schema_feature_definition;
use crate::decode::feature_history::link::section_entity_is_generated_profile;
use crate::decode::feature_history::round::{
    coordinate_pair_proves_torus_radii, differing_positive_lengths,
    five_coordinate_envelope_proves_torus_radii, outline_has_unique_radius_delta,
    paired_five_coordinate_sphere_center, parallel_support_radius, round_constant_radius,
    round_observed_radii, round_placed_cylinder_radii, round_support_radius, slot_fillet_cylinder,
    unique_positive_length,
};

use crate::decode::holes::sweep::{
    compact_simple_hole_cylinder_id, extrusion_extent_and_direction,
    single_cap_circular_sweep_geometry, two_cap_circular_sweep_geometry,
};
use crate::decode::surfaces::cylinders::{
    reference_cap_bound_round_frame, reference_circle_pair_cylinder_frame,
};

use crate::feature::schema::SchemaClass;
use cadmpeg_ir::document::CadIr;
use cadmpeg_ir::features::edge_treatments::RadiusSpec;
use cadmpeg_ir::features::FeatureDefinition as IrFeatureDefinition;
use cadmpeg_ir::features::FeatureOperation as IrFeatureOperation;
use cadmpeg_ir::features::{ExtrudeExtent, ExtrudeSide, LinearTermination};
use cadmpeg_ir::geometry::{SolvedSurfaceGeometry, Surface, SurfaceGeometry};
use cadmpeg_ir::ids::SurfaceId;
use cadmpeg_ir::math::{Point3, Vector3};
use cadmpeg_ir::scalar::PositiveLength;

const EPS_RADIUS_EQUIVALENCE: f64 = 1e-12;

fn service_round_support_radius(
    scan: &crate::container::ContainerScan<'_>,
    ir: &CadIr,
    source_carriers: &crate::decode::source_carriers::SourceUnitCarriers,
    feature_id: u32,
) -> Option<f64> {
    crate::decode::with_test_decode_ctx(|ctx| {
        round_support_radius(ctx, scan, ir, source_carriers, feature_id)
    })
    .expect("service round support admitted")
}

fn service_single_cap_circular_sweep_geometry<'a>(
    scan: &'a crate::container::ContainerScan<'_>,
    feature_id: u32,
) -> Option<crate::decode::holes::sweep::CircularSweepGeometry<'a>> {
    crate::decode::with_test_decode_ctx(|ctx| {
        single_cap_circular_sweep_geometry(ctx, scan, feature_id)
    })
    .expect("service resources")
}

fn service_two_cap_circular_sweep_geometry<'a>(
    scan: &'a crate::container::ContainerScan<'_>,
    feature_id: u32,
) -> Option<crate::decode::holes::sweep::CircularSweepGeometry<'a>> {
    crate::decode::with_test_decode_ctx(|ctx| {
        two_cap_circular_sweep_geometry(ctx, scan, feature_id)
    })
    .expect("service resources")
}

#[test]
fn blind_circular_sweep_requires_materialized_cap_and_cylinder_entries() {
    let entry =
        |entity_id, class_id, source_entity_id| crate::feature::entity::FeatureEntityTableEntry {
            payload: crate::feature::entity::entry_payload(class_id, source_entity_id, None, None),

            entity_id,
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
    let table = crate::feature::entity::FeatureEntityTable::new(
        40,
        29,
        entries,
        &std::collections::BTreeSet::new(),
        0,
    )
    .with_surface_ids([46, 51]);
    let row = |feature_id, id, kind: crate::surface::SurfaceKind| crate::surface::SurfaceRow {
        id,
        kind,
        feature_id,
        reversed: false,
        boundary_type: crate::surface::BoundaryType::Code00,
        next_surface: 0,
        offset: usize::try_from(id).expect("fixture index fits usize"),
    };
    let mut scan = crate::test_support::empty_container_scan();
    scan.features.entity_tables.push(table);
    scan.surfaces.rows.extend([
        row(40, 46, crate::surface::SurfaceKind::Plane),
        row(40, 51, crate::surface::SurfaceKind::Cylinder),
    ]);
    scan.planes.outlines.push(crate::surface::OutlinePlane {
        surface_id: 46,
        origin: [0.0, 16.0, 0.0],
        normal: cadmpeg_ir::units::UnitVector3::Y_AXIS,
        u_axis: cadmpeg_ir::units::UnitVector3::X_AXIS,
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
    scan.features.section_transforms.push(
        crate::placement::FeatureSectionTransform::new(
            40,
            Some(40),
            [0.0, 0.0, 0.0],
            [1.0, 0.0, 0.0],
            [0.0, 0.0, -1.0],
            0,
        )
        .expect("valid section frame"),
    );

    assert!(service_single_cap_circular_sweep_geometry(&scan, 40).is_some());

    let reversed_entries = vec![
        entry(143, 204, None),
        entry(146, 203, None),
        entry(149, 200, Some(4)),
        entry(151, 200, None),
    ];
    scan.features.entity_tables.push(
        crate::feature::entity::FeatureEntityTable::new(
            41,
            29,
            reversed_entries,
            &std::collections::BTreeSet::new(),
            0,
        )
        .with_surface_ids([143, 151]),
    );
    scan.surfaces.rows.extend([
        row(41, 143, crate::surface::SurfaceKind::Plane),
        row(41, 151, crate::surface::SurfaceKind::Cylinder),
    ]);
    scan.planes.outlines.push(crate::surface::OutlinePlane {
        surface_id: 143,
        origin: [0.0, 16.0, 0.0],
        normal: cadmpeg_ir::units::UnitVector3::Y_AXIS,
        u_axis: cadmpeg_ir::units::UnitVector3::X_AXIS,
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
    scan.features.section_transforms.push(
        crate::placement::FeatureSectionTransform::new(
            41,
            Some(41),
            [0.0, 0.0, 0.0],
            [1.0, 0.0, 0.0],
            [0.0, 0.0, -1.0],
            0,
        )
        .expect("valid section frame"),
    );
    assert!(service_single_cap_circular_sweep_geometry(&scan, 41).is_some());

    assert!(
        crate::decode::with_test_decode_ctx(|ctx| section_entity_is_generated_profile(
            ctx,
            true,
            Some(40),
            4,
            &[crate::surface::SurfaceKind::Cylinder],
            &scan.features.entity_tables,
            &scan.surfaces.rows,
        ))
        .expect("service profile admits generated profile scan")
    );

    scan.features.entity_tables[0].unmark_surface_id(51);
    assert!(service_single_cap_circular_sweep_geometry(&scan, 40).is_none());
    assert!(
        !crate::decode::with_test_decode_ctx(|ctx| section_entity_is_generated_profile(
            ctx,
            true,
            Some(40),
            4,
            &[crate::surface::SurfaceKind::Cylinder],
            &scan.features.entity_tables,
            &scan.surfaces.rows,
        ))
        .expect("service profile admits generated profile scan")
    );
}

#[test]
fn two_cap_circular_sweep_joins_materialized_caps_and_one_cylinder() {
    let mut scan = crate::test_support::empty_container_scan();
    let row = |id, kind: crate::surface::SurfaceKind| crate::surface::SurfaceRow {
        id,
        kind,
        feature_id: 825,
        reversed: false,
        boundary_type: crate::surface::BoundaryType::Code00,
        next_surface: 0,
        offset: usize::try_from(id).expect("fixture index fits usize"),
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
            normal: cadmpeg_ir::units::UnitVector3::Y_AXIS,
            u_axis: cadmpeg_ir::units::UnitVector3::X_AXIS,
            offset: 828,
        });
    scan.planes.outlines.push(crate::surface::OutlinePlane {
        surface_id: 831,
        origin: [0.0, -4.0, 0.0],
        normal: cadmpeg_ir::units::UnitVector3::Y_AXIS,
        u_axis: cadmpeg_ir::units::UnitVector3::X_AXIS,
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
    let entry =
        |entity_id, class_id, source_entity_id| crate::feature::entity::FeatureEntityTableEntry {
            payload: crate::feature::entity::entry_payload(class_id, source_entity_id, None, None),

            entity_id,
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
    scan.features.entity_tables.push(
        crate::feature::entity::FeatureEntityTable::new(
            825,
            29,
            entries,
            &std::collections::BTreeSet::new(),
            0,
        )
        .with_surface_ids([828, 831, 836]),
    );

    let sweep = service_two_cap_circular_sweep_geometry(&scan, 825).expect("two-cap sweep");
    assert_eq!(
        sweep
            .cylinder_rows
            .iter()
            .map(|row| row.id)
            .collect::<Vec<_>>(),
        vec![836]
    );
    assert_eq!(sweep.direction, [0.0, -1.0, 0.0]);
    assert_eq!(
        sweep.extent,
        ExtrudeExtent::OneSided {
            side: ExtrudeSide {
                termination: LinearTermination::Blind {
                    length: cadmpeg_ir::scalar::NonZeroLength::new(8.0)
                        .expect("nonzero length fixture"),
                },
                draft: None,
            },
        }
    );
    let cylinder_surface = sweep.geometry;
    assert!(
        *cylinder_surface.origin() == Point3::new(-12.5, -4.0, 0.0)
            && *cylinder_surface.frame().axis().as_raw() == Vector3::new(0.0, -1.0, 0.0)
            && cylinder_surface.radius().get() == 0.75
    );

    scan.features.entity_tables[0].unmark_surface_id(831);
    assert!(service_two_cap_circular_sweep_geometry(&scan, 825).is_none());
}

#[test]
fn compact_hole_materialized_core_establishes_the_simple_form() {
    let entry =
        |entity_id, class_id, source_entity_id| crate::feature::entity::FeatureEntityTableEntry {
            payload: crate::feature::entity::entry_payload(class_id, source_entity_id, None, None),

            entity_id,
            prefixed: false,
            offset: 0,
            end_offset: 0,
        };
    let mut table = crate::feature::entity::FeatureEntityTable::new(
        107,
        29,
        vec![
            entry(109, 204, None),
            entry(112, 203, None),
            entry(115, 200, Some(0)),
            entry(117, 200, None),
        ],
        &std::collections::BTreeSet::new(),
        0,
    )
    .with_surface_ids([117]);
    let row = crate::surface::SurfaceRow {
        id: 117,
        kind: crate::surface::SurfaceKind::Cylinder,
        feature_id: 107,
        reversed: true,
        boundary_type: crate::surface::BoundaryType::Code00,
        next_surface: 0,
        offset: 0,
    };

    assert_eq!(
        crate::decode::with_test_decode_ctx(|ctx| compact_simple_hole_cylinder_id(
            ctx,
            107,
            std::slice::from_ref(&table),
            std::slice::from_ref(&row),
        ))
        .expect("admitted surface roster"),
        Some(117)
    );
    let mut exact_class_203_plane = table.clone();
    exact_class_203_plane.mark_surface_ids([112, 117]);
    let topology_plane = crate::surface::SurfaceRow {
        id: 112,
        kind: crate::surface::SurfaceKind::Plane,
        feature_id: 107,
        reversed: false,
        boundary_type: crate::surface::BoundaryType::Code00,
        next_surface: 0,
        offset: 0,
    };
    assert_eq!(
        crate::decode::with_test_decode_ctx(|ctx| compact_simple_hole_cylinder_id(
            ctx,
            107,
            std::slice::from_ref(&exact_class_203_plane),
            &[topology_plane, row.clone()],
        ))
        .expect("admitted surface roster"),
        Some(117)
    );
    table.entries[2].payload = crate::feature::entity::EntryPayload::Source { entity: None };
    assert!(
        crate::decode::with_test_decode_ctx(|ctx| compact_simple_hole_cylinder_id(
            ctx,
            107,
            std::slice::from_ref(&table),
            std::slice::from_ref(&row),
        ))
        .expect("admitted surface roster")
        .is_none()
    );
    table.entries[2].payload = crate::feature::entity::EntryPayload::Source { entity: Some(0) };
    table.table_class_id = 28;
    assert!(
        crate::decode::with_test_decode_ctx(|ctx| compact_simple_hole_cylinder_id(
            ctx,
            107,
            std::slice::from_ref(&table),
            std::slice::from_ref(&row),
        ))
        .expect("admitted surface roster")
        .is_none()
    );
    table.table_class_id = 29;
    table.entries[3].payload = crate::feature::entity::EntryPayload::Plain {
        class: crate::feature::entity::PlainClass::new(201).expect("201 is not the source class"),
    };
    assert!(
        crate::decode::with_test_decode_ctx(|ctx| compact_simple_hole_cylinder_id(
            ctx,
            107,
            std::slice::from_ref(&table),
            std::slice::from_ref(&row),
        ))
        .expect("admitted surface roster")
        .is_none()
    );
    table.entries[3].payload = crate::feature::entity::EntryPayload::Source { entity: None };
    table.mark_surface_ids([109, 117]);
    assert!(
        crate::decode::with_test_decode_ctx(|ctx| compact_simple_hole_cylinder_id(
            ctx,
            107,
            std::slice::from_ref(&table),
            std::slice::from_ref(&row),
        ))
        .expect("admitted surface roster")
        .is_none()
    );

    let mut extended = crate::feature::entity::FeatureEntityTable::new(
        107,
        29,
        vec![
            entry(109, 204, None),
            entry(112, 203, None),
            entry(120, 204, None),
            entry(121, 203, None),
            entry(115, 200, Some(0)),
            entry(117, 200, None),
        ],
        &std::collections::BTreeSet::new(),
        0,
    )
    .with_surface_ids([109, 117]);
    for (index, entry) in extended.entries.iter_mut().enumerate() {
        entry.offset = index;
        entry.end_offset = index + 1;
    }
    let plane = crate::surface::SurfaceRow {
        id: 109,
        kind: crate::surface::SurfaceKind::Plane,
        feature_id: 107,
        reversed: false,
        boundary_type: crate::surface::BoundaryType::Code00,
        next_surface: 0,
        offset: 0,
    };
    let rows = [plane.clone(), row.clone()];
    assert_eq!(
        crate::decode::with_test_decode_ctx(|ctx| compact_simple_hole_cylinder_id(
            ctx,
            107,
            std::slice::from_ref(&extended),
            &rows
        ))
        .expect("admitted surface roster"),
        Some(117)
    );
    let mut class_203_plane = extended.clone();
    class_203_plane.mark_surface_ids([112, 117]);
    let mut second_topology_plane = plane;
    second_topology_plane.id = 112;
    let second_topology_rows = [second_topology_plane, row];
    assert_eq!(
        crate::decode::with_test_decode_ctx(|ctx| compact_simple_hole_cylinder_id(
            ctx,
            107,
            std::slice::from_ref(&class_203_plane),
            &second_topology_rows,
        ))
        .expect("admitted surface roster"),
        Some(117)
    );
    extended.mark_surface_ids([109, 117, 120]);
    assert!(
        crate::decode::with_test_decode_ctx(|ctx| compact_simple_hole_cylinder_id(
            ctx,
            107,
            std::slice::from_ref(&extended),
            &rows
        ))
        .expect("admitted surface roster")
        .is_none()
    );
}

#[test]
fn torus_outline_identifies_exactly_one_prototype_radius_delta() {
    let outline = |values| crate::surface::TorusOutlineFrame {
        values,
        selector: 0,
        offset: 0,
    };
    assert!(crate::decode::with_test_decode_ctx(|ctx| {
        outline_has_unique_radius_delta(
            ctx,
            outline([-192.5, -5.0, -40.0, -167.5, -3.0, 52.5]),
            2.0,
        )
    })
    .expect("admitted torus outline comparison"));
    assert!(!crate::decode::with_test_decode_ctx(|ctx| {
        outline_has_unique_radius_delta(ctx, outline([-2.0, -2.0, 0.0, 0.0, 0.0, 8.0]), 2.0)
    })
    .expect("admitted torus outline comparison"));
    assert!(!crate::decode::with_test_decode_ctx(|ctx| {
        outline_has_unique_radius_delta(ctx, outline([-2.0, 0.0, 0.0, 2.0, 0.0, 8.0]), 2.0)
    })
    .expect("admitted torus outline comparison"));
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
        crate::decode::with_test_decode_ctx(|ctx| paired_five_coordinate_sphere_center(
            ctx,
            [
                five_coordinate([-2.65, -15.0, -2.65, 2.65, -17.65]),
                five_coordinate([-2.65, -12.35, -2.65, 2.65, -15.0]),
            ],
            2.65,
        ))
        .expect("service profile admits paired sphere coordinates"),
        Some([0.0, 0.0, -15.0])
    );
    assert!(
        crate::decode::with_test_decode_ctx(|ctx| paired_five_coordinate_sphere_center(
            ctx,
            [
                five_coordinate([-2.65, -15.0, -2.65, 2.65, -17.65]),
                five_coordinate([-2.65, -12.0, -2.65, 2.65, -15.0]),
            ],
            2.65,
        ))
        .expect("service profile admits paired sphere coordinates")
        .is_none()
    );
}

#[test]
fn unique_parallel_round_supports_define_constant_radius() {
    let plane = |origin, normal| PlaneEquation { origin, normal };
    assert_eq!(
        crate::decode::with_test_decode_ctx(|ctx| {
            unique_positive_length(ctx, &[0.5, 0.5 + EPS_RADIUS_EQUIVALENCE])
        })
        .expect("service profile admits positive length samples")
        .map(cadmpeg_ir::scalar::PositiveLength::get),
        Some(0.5)
    );
    assert_eq!(
        crate::decode::with_test_decode_ctx(|ctx| unique_positive_length(ctx, &[0.5, 0.6]))
            .expect("service profile admits positive length samples"),
        None
    );
    assert_eq!(
        crate::decode::with_test_decode_ctx(|ctx| unique_positive_length(ctx, &[0.0]))
            .expect("service profile admits positive length samples"),
        None
    );
    assert!(!crate::decode::with_test_decode_ctx(|ctx| {
        differing_positive_lengths(ctx, &[15.0, 15.0 + EPS_RADIUS_EQUIVALENCE])
    })
    .expect("service profile admits positive length samples"));
    assert!(crate::decode::with_test_decode_ctx(|ctx| {
        differing_positive_lengths(ctx, &[15.0, 7.0, 15.0])
    })
    .expect("service profile admits positive length samples"));
    assert!(!crate::decode::with_test_decode_ctx(|ctx| {
        differing_positive_lengths(ctx, &[0.0, 1.0])
    })
    .expect("service profile admits positive length samples"));
    assert_eq!(
        crate::decode::with_test_decode_ctx(|ctx| parallel_support_radius(
            ctx,
            &[
                plane([-8.0, 0.0, 0.0], [1.0, 0.0, 0.0]),
                plane([0.0, 0.0, -6.1], [0.0, 0.0, 1.0]),
                plane([-9.0, 0.0, 0.0], [1.0, 0.0, 0.0]),
            ],
            |plane| Ok(Some(*plane))
        ))
        .expect("service profile admits round support plane comparisons"),
        Some(0.5)
    );
    assert_eq!(
        crate::decode::with_test_decode_ctx(|ctx| parallel_support_radius(
            ctx,
            &[
                plane([-8.0, 0.0, 0.0], [1.0, 0.0, 0.0]),
                plane([-9.0, 0.0, 0.0], [1.0, 0.0, 0.0]),
                plane([0.0, 0.0, -6.0], [0.0, 0.0, 1.0]),
                plane([0.0, 0.0, -8.0], [0.0, 0.0, 1.0]),
            ],
            |plane| Ok(Some(*plane))
        ))
        .expect("service profile admits round support plane comparisons"),
        None
    );
    assert_eq!(
        crate::decode::with_test_decode_ctx(|ctx| parallel_support_radius(
            ctx,
            &[
                plane([-8.0, 0.0, 0.0], [1.0, 0.0, 0.0]),
                plane([-9.0, 0.0, 0.0], [1.0, 0.0, 0.0]),
                plane([0.0, 0.0, -6.0], [0.0, 0.0, 1.0]),
                plane([0.0, 0.0, -7.0], [0.0, 0.0, 1.0]),
            ],
            |plane| Ok(Some(*plane))
        ))
        .expect("service profile admits round support plane comparisons"),
        Some(0.5)
    );
    let cylinder = crate::decode::with_test_decode_ctx(|ctx| {
        slot_fillet_cylinder(
            ctx,
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
    })
    .expect("service profile admits slot midplanes")
    .expect("fully constrained slot fillet");
    assert_eq!(cylinder.origin, [-8.5, -2.0, -6.5]);
    assert_eq!(cylinder.axis, [0.0, 1.0, 0.0]);
    assert_eq!(cylinder.radius, 0.5);
    assert!(
        crate::decode::with_test_decode_ctx(|ctx| slot_fillet_cylinder(
            ctx,
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
        ))
        .expect("service profile admits slot midplanes")
        .is_none()
    );
}

#[test]
fn round_support_planes_define_radius_without_generated_surface_rows() {
    let mut scan = crate::test_support::empty_container_scan();
    scan.features
        .affected_ids
        .push(crate::feature::rows::FeatureAffectedIds {
            feature_id: 913,
            kind: crate::feature::rows::AffectedIdKind::Geometry,
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
            id: SurfaceId::mint(format!("creo:visibgeom:surface#{id}")).expect("identity grammar"),
            geometry: SurfaceGeometry::Solved(SolvedSurfaceGeometry::Plane(
                cadmpeg_ir::geometry::analytic::PlaneSurface::try_new(
                    Point3::from(origin),
                    Vector3::from(normal),
                    Vector3::new(0.0, 0.0, 1.0),
                )
                .expect("valid PlaneSurface fixture"),
            )),
            source_object: None,
        });
    }

    assert_eq!(
        crate::decode::with_test_decode_ctx(|ctx| round_constant_radius(
            ctx,
            &scan,
            &ir,
            &crate::decode::source_carriers::SourceUnitCarriers::default(),
            913
        ))
        .expect("round constant radius"),
        Some(0.5)
    );
}

#[test]
fn mixed_round_families_reconcile_placed_cylinders_and_prototype_tori() {
    let mut scan = crate::test_support::empty_container_scan();
    scan.framing.layout = crate::container::Layout::Nd;
    scan.framing.sections.push(
        crate::container::Section::scan_for_test(
            "VisibGeom".to_string(),
            0,
            1_000,
            None,
            &[0u8; 1_000],
        )
        .expect("section extent")
        .section,
    );
    scan.surfaces.rows.extend([
        crate::surface::SurfaceRow {
            id: 11,
            kind: crate::surface::SurfaceKind::Cylinder,
            feature_id: 913,
            reversed: false,
            boundary_type: crate::surface::BoundaryType::Code00,
            next_surface: 0,
            offset: 100,
        },
        crate::surface::SurfaceRow {
            id: 12,
            kind: crate::surface::SurfaceKind::TorusOrSphere,
            feature_id: 913,
            reversed: false,
            boundary_type: crate::surface::BoundaryType::Code00,
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
            scalar_tokens: replay_frame.slots.clone(),
            opaque_spans: Vec::new(),
            scalar_frames: vec![replay_frame.clone()],
            carrier: crate::surface::SurfaceParameterCarrier::Unresolved(
                crate::surface::SurfaceKind::TorusOrSphere,
            ),
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
            family: crate::surface::SurfacePrototypeFamily::Torus(
                crate::surface::TorusLabel::Torus,
            ),
            parameters: vec![scalar("radius1", 10.0), scalar("radius2", 0.5)],
            offset: 150,
        });

    let mut ir = CadIr::empty();
    ir.model.surfaces.push(Surface {
        id: SurfaceId::mint("creo:visibgeom:surface#11".to_string()).expect("identity grammar"),
        geometry: SurfaceGeometry::Solved(SolvedSurfaceGeometry::Cylinder(
            cadmpeg_ir::geometry::analytic::CylinderSurface::try_new(
                Point3::new(0.0, 0.0, 0.0),
                Vector3::new(0.0, 0.0, 1.0),
                Vector3::new(1.0, 0.0, 0.0),
                0.5,
            )
            .expect("valid CylinderSurface fixture"),
        )),
        source_object: None,
    });
    scan.features
        .affected_ids
        .push(crate::feature::rows::FeatureAffectedIds {
            feature_id: 913,
            kind: crate::feature::rows::AffectedIdKind::Geometry,
            ids: vec![1, 2, 3, 4],
            offset: 0,
        });
    for (id, x) in [(3, -9.0), (4, -8.0)] {
        ir.model.surfaces.push(Surface {
            id: SurfaceId::mint(format!("creo:visibgeom:surface#{id}")).expect("identity grammar"),
            geometry: SurfaceGeometry::Solved(SolvedSurfaceGeometry::Plane(
                cadmpeg_ir::geometry::analytic::PlaneSurface::try_new(
                    Point3::new(x, 0.0, 0.0),
                    Vector3::new(1.0, 0.0, 0.0),
                    Vector3::new(0.0, 1.0, 0.0),
                )
                .expect("valid PlaneSurface fixture"),
            )),
            source_object: None,
        });
    }

    assert_eq!(
        crate::decode::with_test_decode_ctx(|ctx| round_constant_radius(
            ctx,
            &scan,
            &ir,
            &crate::decode::source_carriers::SourceUnitCarriers::default(),
            913
        ))
        .expect("round constant radius"),
        Some(0.5)
    );

    if let Some(Surface {
        geometry: SurfaceGeometry::Solved(SolvedSurfaceGeometry::Cylinder(cylinder_surface)),
        ..
    }) = ir.model.surfaces.first_mut()
    {
        let origin = cylinder_surface.origin();
        let axis = cylinder_surface.frame().axis().as_raw();
        let ref_direction = cylinder_surface.frame().reference().as_raw();

        let radius = 0.75;
        *cylinder_surface = cadmpeg_ir::geometry::analytic::CylinderSurface::try_new(
            *origin,
            *axis,
            *ref_direction,
            radius,
        )
        .expect("valid CylinderSurface fixture");
    }
    assert_eq!(
        crate::decode::with_test_decode_ctx(|ctx| round_constant_radius(
            ctx,
            &scan,
            &ir,
            &crate::decode::source_carriers::SourceUnitCarriers::default(),
            913
        ))
        .expect("round constant radius"),
        None
    );
}

#[test]
fn placed_cylinder_samples_identify_variable_radius_with_unresolved_siblings() {
    let mut scan = crate::test_support::empty_container_scan();
    for (id, kind) in [
        (11, crate::surface::SurfaceKind::Cylinder),
        (12, crate::surface::SurfaceKind::TorusOrSphere),
        (13, crate::surface::SurfaceKind::Cylinder),
    ] {
        scan.surfaces.rows.push(crate::surface::SurfaceRow {
            id,
            kind,
            feature_id: 5,
            reversed: false,
            boundary_type: crate::surface::BoundaryType::Code00,
            next_surface: 0,
            offset: usize::try_from(id).expect("fixture index fits usize"),
        });
    }
    let mut ir = CadIr::empty();
    for (id, radius) in [(11, 15.0), (13, 1.0)] {
        ir.model.surfaces.push(Surface {
            id: SurfaceId::mint(format!("creo:visibgeom:surface#{id}")).expect("identity grammar"),
            geometry: SurfaceGeometry::Solved(SolvedSurfaceGeometry::Cylinder(
                cadmpeg_ir::geometry::analytic::CylinderSurface::try_new(
                    Point3::new(0.0, 0.0, 0.0),
                    Vector3::new(0.0, 0.0, 1.0),
                    Vector3::new(1.0, 0.0, 0.0),
                    radius,
                )
                .expect("valid CylinderSurface fixture"),
            )),
            source_object: None,
        });
    }

    assert!(matches!(
        crate::decode::with_test_decode_ctx(|ctx| schema_feature_definition(ctx, &scan, &ir, &crate::decode::source_carriers::SourceUnitCarriers::default(), 5, Some(SchemaClass::Round), "Round")).expect("valid test fixture"),
        IrFeatureDefinition::Operation(IrFeatureOperation::Fillet {
            ref groups,
        }) if matches!(
            groups.as_slice(),
            [cadmpeg_ir::features::edge_treatments::FilletGroup {
                radius: RadiusSpec::Unresolved { form: Some(cadmpeg_ir::features::edge_treatments::RadiusForm::Variable) },
                ..
            }]
        )
    ));
}

#[test]
fn unequal_round_samples_are_not_hidden_by_support_radius() {
    let mut scan = crate::test_support::empty_container_scan();
    for (id, parameter) in [(11, Some(15.0)), (12, Some(1.0)), (13, None)] {
        scan.surfaces.rows.push(crate::surface::SurfaceRow {
            id,
            kind: crate::surface::SurfaceKind::Cylinder,
            feature_id: 5,
            reversed: false,
            boundary_type: crate::surface::BoundaryType::Code00,
            next_surface: 0,
            offset: usize::try_from(id).expect("fixture index fits usize"),
        });
        if let Some(radius) = parameter {
            let first = crate::surface::SurfaceParameterScalar {
                value: Some(1.0),
                raw: vec![0],
                offset: 1,
            };
            let second = crate::surface::SurfaceParameterScalar {
                value: Some(1.0 + 2.0 * radius),
                raw: vec![0],
                offset: 3,
            };
            let extent = [
                crate::surface::SurfaceParameterScalar {
                    value: Some(0.0),
                    raw: vec![0],
                    offset: 4,
                },
                crate::surface::SurfaceParameterScalar {
                    value: Some(0.0),
                    raw: vec![0],
                    offset: 5,
                },
                crate::surface::SurfaceParameterScalar {
                    value: Some(0.0),
                    raw: vec![0],
                    offset: 6,
                },
                crate::surface::SurfaceParameterScalar {
                    value: Some(2.0 * radius),
                    raw: vec![0],
                    offset: 7,
                },
                crate::surface::SurfaceParameterScalar {
                    value: Some(0.0),
                    raw: vec![0],
                    offset: 8,
                },
                crate::surface::SurfaceParameterScalar {
                    value: Some(0.0),
                    raw: vec![0],
                    offset: 9,
                },
            ];
            scan.surfaces
                .parameters
                .push(crate::surface::SurfaceParameterRecord {
                    surface_id: id,
                    body: vec![0x11, 0x00, 0x11, 0, 0, 0, 0, 0, 0, 0],
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
                    carrier: crate::surface::SurfaceParameterCarrier::Unresolved(
                        crate::surface::SurfaceKind::Cylinder,
                    ),
                    boundary: crate::surface::SurfaceBodyBoundary::CompoundClose,
                    offset: usize::try_from(id).expect("fixture index fits usize"),
                    body_offset: usize::try_from(id).expect("fixture index fits usize") + 1,
                });
        }
    }
    scan.features
        .affected_ids
        .push(crate::feature::rows::FeatureAffectedIds {
            feature_id: 5,
            kind: crate::feature::rows::AffectedIdKind::Geometry,
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
            id: SurfaceId::mint(format!("creo:visibgeom:surface#{id}")).expect("identity grammar"),
            geometry: SurfaceGeometry::Solved(SolvedSurfaceGeometry::Plane(
                cadmpeg_ir::geometry::analytic::PlaneSurface::try_new(
                    Point3::from(origin),
                    Vector3::from(normal),
                    Vector3::new(0.0, 0.0, 1.0),
                )
                .expect("valid PlaneSurface fixture"),
            )),
            source_object: None,
        });
    }

    assert_eq!(
        crate::decode::with_test_decode_ctx(|ctx| round_observed_radii(ctx, &scan, 5))
            .expect("service profile admits observed radii"),
        [15.0, 1.0]
    );
    assert_eq!(
        service_round_support_radius(
            &scan,
            &ir,
            &crate::decode::source_carriers::SourceUnitCarriers::default(),
            5
        ),
        Some(0.5)
    );
    assert_eq!(
        crate::decode::with_test_decode_ctx(|ctx| round_constant_radius(
            ctx,
            &scan,
            &ir,
            &crate::decode::source_carriers::SourceUnitCarriers::default(),
            5
        ))
        .expect("round constant radius"),
        None
    );
    assert!(matches!(
        crate::decode::with_test_decode_ctx(|ctx| schema_feature_definition(ctx, &scan, &ir, &crate::decode::source_carriers::SourceUnitCarriers::default(), 5, Some(SchemaClass::Round), "Round")).expect("valid test fixture"),
        IrFeatureDefinition::Operation(IrFeatureOperation::Fillet {
            groups,
        }) if matches!(
            groups.as_slice(),
            [cadmpeg_ir::features::edge_treatments::FilletGroup {
                radius: RadiusSpec::Unresolved { form: Some(cadmpeg_ir::features::edge_treatments::RadiusForm::Variable) },
                ..
            }]
        )
    ));
}

#[test]
fn unequal_placed_round_cylinders_are_not_hidden_by_support_radius() {
    let mut scan = crate::test_support::empty_container_scan();
    for id in [11, 12] {
        scan.surfaces.rows.push(crate::surface::SurfaceRow {
            id,
            kind: crate::surface::SurfaceKind::Cylinder,
            feature_id: 5,
            reversed: false,
            boundary_type: crate::surface::BoundaryType::Code00,
            next_surface: 0,
            offset: usize::try_from(id).expect("fixture index fits usize"),
        });
    }
    scan.features
        .affected_ids
        .push(crate::feature::rows::FeatureAffectedIds {
            feature_id: 5,
            kind: crate::feature::rows::AffectedIdKind::Geometry,
            ids: vec![1, 2, 3, 4],
            offset: 0,
        });

    let mut ir = CadIr::empty();
    for (id, radius) in [(11, 15.0), (12, 1.0)] {
        ir.model.surfaces.push(Surface {
            id: SurfaceId::mint(format!("creo:visibgeom:surface#{id}")).expect("identity grammar"),
            geometry: SurfaceGeometry::Solved(SolvedSurfaceGeometry::Cylinder(
                cadmpeg_ir::geometry::analytic::CylinderSurface::try_new(
                    Point3::new(0.0, 0.0, 0.0),
                    Vector3::new(0.0, 0.0, 1.0),
                    Vector3::new(1.0, 0.0, 0.0),
                    radius,
                )
                .expect("valid CylinderSurface fixture"),
            )),
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
            id: SurfaceId::mint(format!("creo:visibgeom:surface#{id}")).expect("identity grammar"),
            geometry: SurfaceGeometry::Solved(SolvedSurfaceGeometry::Plane(
                cadmpeg_ir::geometry::analytic::PlaneSurface::try_new(
                    Point3::from(origin),
                    Vector3::from(normal),
                    Vector3::new(0.0, 0.0, 1.0),
                )
                .expect("valid PlaneSurface fixture"),
            )),
            source_object: None,
        });
    }

    assert_eq!(
        crate::decode::with_test_decode_ctx(|ctx| round_placed_cylinder_radii(
            ctx,
            &scan,
            &ir,
            &crate::decode::source_carriers::SourceUnitCarriers::default(),
            5
        ))
        .expect("service profile admits placed radii"),
        [15.0, 1.0]
    );
    assert_eq!(
        service_round_support_radius(
            &scan,
            &ir,
            &crate::decode::source_carriers::SourceUnitCarriers::default(),
            5
        ),
        Some(0.5)
    );
    assert_eq!(
        crate::decode::with_test_decode_ctx(|ctx| round_constant_radius(
            ctx,
            &scan,
            &ir,
            &crate::decode::source_carriers::SourceUnitCarriers::default(),
            5
        ))
        .expect("round constant radius"),
        None
    );
    assert!(matches!(
        crate::decode::with_test_decode_ctx(|ctx| schema_feature_definition(ctx, &scan, &ir, &crate::decode::source_carriers::SourceUnitCarriers::default(), 5, Some(SchemaClass::Round), "Round")).expect("valid test fixture"),
        IrFeatureDefinition::Operation(IrFeatureOperation::Fillet {
            groups,
        }) if matches!(
            groups.as_slice(),
            [cadmpeg_ir::features::edge_treatments::FilletGroup {
                radius: RadiusSpec::Unresolved { form: Some(cadmpeg_ir::features::edge_treatments::RadiusForm::Variable) },
                ..
            }]
        )
    ));
}

#[test]
fn unequal_mixed_round_cylinders_are_not_hidden_by_unresolved_torus() {
    let mut scan = crate::test_support::empty_container_scan();
    for (id, kind) in [
        (11, crate::surface::SurfaceKind::Cylinder),
        (12, crate::surface::SurfaceKind::TorusOrSphere),
        (13, crate::surface::SurfaceKind::Cylinder),
    ] {
        scan.surfaces.rows.push(crate::surface::SurfaceRow {
            id,
            kind,
            feature_id: 5,
            reversed: false,
            boundary_type: crate::surface::BoundaryType::Code00,
            next_surface: 0,
            offset: usize::try_from(id).expect("fixture index fits usize"),
        });
    }
    scan.features
        .affected_ids
        .push(crate::feature::rows::FeatureAffectedIds {
            feature_id: 5,
            kind: crate::feature::rows::AffectedIdKind::Geometry,
            ids: vec![1, 2, 3, 4],
            offset: 0,
        });

    let mut ir = CadIr::empty();
    for (id, radius) in [(11, 15.0), (13, 1.0)] {
        ir.model.surfaces.push(Surface {
            id: SurfaceId::mint(format!("creo:visibgeom:surface#{id}")).expect("identity grammar"),
            geometry: SurfaceGeometry::Solved(SolvedSurfaceGeometry::Cylinder(
                cadmpeg_ir::geometry::analytic::CylinderSurface::try_new(
                    Point3::new(0.0, 0.0, 0.0),
                    Vector3::new(0.0, 0.0, 1.0),
                    Vector3::new(1.0, 0.0, 0.0),
                    radius,
                )
                .expect("valid CylinderSurface fixture"),
            )),
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
            id: SurfaceId::mint(format!("creo:visibgeom:surface#{id}")).expect("identity grammar"),
            geometry: SurfaceGeometry::Solved(SolvedSurfaceGeometry::Plane(
                cadmpeg_ir::geometry::analytic::PlaneSurface::try_new(
                    Point3::from(origin),
                    Vector3::from(normal),
                    Vector3::new(0.0, 0.0, 1.0),
                )
                .expect("valid PlaneSurface fixture"),
            )),
            source_object: None,
        });
    }

    assert_eq!(
        crate::decode::with_test_decode_ctx(|ctx| round_placed_cylinder_radii(
            ctx,
            &scan,
            &ir,
            &crate::decode::source_carriers::SourceUnitCarriers::default(),
            5
        ))
        .expect("service profile admits placed radii"),
        [15.0, 1.0]
    );
    assert_eq!(
        service_round_support_radius(
            &scan,
            &ir,
            &crate::decode::source_carriers::SourceUnitCarriers::default(),
            5
        ),
        Some(0.5)
    );
    assert_eq!(
        crate::decode::with_test_decode_ctx(|ctx| round_constant_radius(
            ctx,
            &scan,
            &ir,
            &crate::decode::source_carriers::SourceUnitCarriers::default(),
            5
        ))
        .expect("round constant radius"),
        None
    );
    assert!(matches!(
        crate::decode::with_test_decode_ctx(|ctx| schema_feature_definition(ctx, &scan, &ir, &crate::decode::source_carriers::SourceUnitCarriers::default(), 5, Some(SchemaClass::Round), "Round")).expect("valid test fixture"),
        IrFeatureDefinition::Operation(IrFeatureOperation::Fillet {
            groups,
        }) if matches!(
            groups.as_slice(),
            [cadmpeg_ir::features::edge_treatments::FilletGroup {
                radius: RadiusSpec::Unresolved { form: Some(cadmpeg_ir::features::edge_treatments::RadiusForm::Variable) },
                ..
            }]
        )
    ));
}

#[test]
fn opposite_reference_caps_select_one_round_envelope_axis() {
    let circle = |entity_id, axis, start: [f64; 3], end: [f64; 3]| {
        let mut center = start;
        let radial_lane = (0..3)
            .find(|lane| start[*lane] != end[*lane])
            .expect("distinct cap endpoints");
        center[radial_lane] = end[radial_lane];
        crate::reference::ReferenceCircle::try_new(
            entity_id,
            crate::reference::ReferenceCircleCenter::Stored(
                cadmpeg_ir::features::FinitePoint3::new(center.into())
                    .expect("finite circle center"),
            ),
            cadmpeg_ir::scalar::PositiveLength::new(2.0).expect("positive radius"),
            cadmpeg_ir::units::UnitVector3::new(cadmpeg_ir::math::Vector3::from(axis))
                .expect("unit axis"),
            [
                cadmpeg_ir::features::FinitePoint3::new(start.into()).expect("finite start"),
                cadmpeg_ir::features::FinitePoint3::new(end.into()).expect("finite end"),
            ],
            0,
        )
        .expect("checked reference geometry")
    };
    let envelope = crate::surface::Type24RoundEnvelope {
        diameter: 2.0,
        extent_endpoints: [[3.5, 8.0, -6.0], [5.5, 10.0, -4.0]],
    };
    let first = circle(367, [0.0, 0.0, 1.0], [3.5, 8.0, -6.0], [5.5, 10.0, -6.0]);
    let second = circle(368, [0.0, 0.0, -1.0], [5.5, 10.0, -4.0], [3.5, 8.0, -4.0]);
    let frame =
        reference_cap_bound_round_frame(envelope, &[&first, &second]).expect("opposite Z caps");
    assert_eq!(frame.frame().origin(), [4.5, 9.0, -6.0]);
    assert_eq!(frame.frame().axis(), [0.0, 0.0, 1.0]);
    assert_eq!(frame.frame().ref_direction(), [1.0, 0.0, 0.0]);
    assert_eq!(frame.radius().get(), 1.0);
    assert_eq!(frame.length().map(PositiveLength::get), Some(2.0));
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
    let circle = |entity_id, center: [f64; 3], axis, start: [f64; 3]| {
        crate::reference::ReferenceCircle::try_new(
            entity_id,
            crate::reference::ReferenceCircleCenter::Stored(
                cadmpeg_ir::features::FinitePoint3::new(center.into()).expect("finite center"),
            ),
            cadmpeg_ir::scalar::PositiveLength::new(2.0).expect("positive radius"),
            cadmpeg_ir::units::UnitVector3::new(cadmpeg_ir::math::Vector3::from(axis))
                .expect("unit axis"),
            [
                cadmpeg_ir::features::FinitePoint3::new(start.into()).expect("finite start"),
                cadmpeg_ir::features::FinitePoint3::new(
                    std::array::from_fn::<_, 3, _>(|lane| 2.0 * center[lane] - start[lane]).into(),
                )
                .expect("on-circle end"),
            ],
            0,
        )
        .expect("checked reference geometry")
    };
    let first = circle(41, [3.0, 5.0, -2.0], [0.0, 0.0, 1.0], [3.0, 7.0, -2.0]);
    let second = circle(42, [3.0, 5.0, 4.0], [0.0, 0.0, -1.0], [1.0, 5.0, 4.0]);

    assert_eq!(
        reference_circle_pair_cylinder_frame(&[&first, &second]),
        Some(
            crate::surface::PositionalCylinderFrame::new(
                first.center().get().into(),
                [0.0, 0.0, 1.0],
                [0.0, 1.0, 0.0],
                2.0,
                Some(6.0)
            )
            .expect("valid positional cylinder frame")
        )
    );
    assert!(reference_circle_pair_cylinder_frame(&[&first]).is_none());

    let unequal_radius = crate::reference::ReferenceCircle::try_new(
        second.entity_id,
        crate::reference::ReferenceCircleCenter::Stored(second.center()),
        cadmpeg_ir::scalar::PositiveLength::new(1.0).expect("positive radius"),
        second.axis(),
        [
            cadmpeg_ir::features::FinitePoint3::new([2.0, 5.0, 4.0].into()).expect("start"),
            cadmpeg_ir::features::FinitePoint3::new([4.0, 5.0, 4.0].into()).expect("end"),
        ],
        second.offset,
    )
    .expect("valid unequal-radius circle");
    assert!(reference_circle_pair_cylinder_frame(&[&first, &unequal_radius]).is_none());

    let displaced = circle(43, [3.5, 5.0, 4.0], [0.0, 0.0, 1.0], [3.5, 7.0, 4.0]);
    assert!(reference_circle_pair_cylinder_frame(&[&first, &displaced]).is_none());

    let derived_center = crate::reference::ReferenceCircle::try_new(
        second.entity_id,
        crate::reference::ReferenceCircleCenter::Diameter,
        second.radius(),
        cadmpeg_ir::units::UnitVector3::Z_AXIS,
        [second.start(), second.end()],
        second.offset,
    )
    .expect("valid diameter-derived center");
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
                    termination: LinearTermination::Blind {
                        length: cadmpeg_ir::scalar::NonZeroLength::new(3.0)
                            .expect("nonzero length fixture"),
                    },
                    draft: None,
                },
                second: ExtrudeSide {
                    termination: LinearTermination::Blind {
                        length: cadmpeg_ir::scalar::NonZeroLength::new(2.0)
                            .expect("nonzero length fixture"),
                    },
                    draft: None,
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
                    termination: LinearTermination::Blind {
                        length: cadmpeg_ir::scalar::NonZeroLength::new(48.0)
                            .expect("nonzero length fixture"),
                    },
                    draft: None,
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
                    termination: LinearTermination::Blind {
                        length: cadmpeg_ir::scalar::NonZeroLength::new(48.0)
                            .expect("nonzero length fixture"),
                    },
                    draft: None,
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
                    termination: LinearTermination::Blind {
                        length: cadmpeg_ir::scalar::NonZeroLength::new(38.0)
                            .expect("nonzero length fixture"),
                    },
                    draft: None,
                },
            },
            [-0.0, 1.0, -0.0],
        ))
    );
}

mod generated_cylinders;
