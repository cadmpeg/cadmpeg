// SPDX-License-Identifier: Apache-2.0

use crate::decode::feature_history::section_profile_ref;
use crate::decode::holes::placement::{CapOutline, HoleCylinder};
use crate::decode::holes::{
    circular_sweep_cylinder_from_cap_outlines, circular_sweep_feature_definition,
    cylinder_from_single_cap_outline, hole_cylinder_from_cap_outlines, hole_extent_and_direction,
    hole_placement, CircularSweepGeometry,
};
use crate::decode::sketch_transfer::skamp_constraints::sketch_constraint_loci_compatible;
use crate::decode::sweep::{
    circular_section_profile_from_cylinder, connected_sketch_profile_vertices,
};
use cadmpeg_ir::document::CadIr;
use cadmpeg_ir::features::{
    BooleanOp, ExtrudeExtent, ExtrudeSide, FeatureDefinition as IrFeatureDefinition,
    LinearTermination, PlanarProfileRef, ProfileRef,
};
use cadmpeg_ir::math::{Point2, Point3, Vector3};
use cadmpeg_ir::scalar::Length;
use cadmpeg_ir::sketches::{
    Sketch, SketchConstraintDefinitionInput, SketchEntity, SketchEntityId, SketchEntityUse,
    SketchGeometry, SketchGeometryDefinition, SketchId, SketchLocus,
};
use std::collections::BTreeMap;

#[test]
fn circular_sweep_projects_profile_direction_and_extent() {
    let rows = [12, 13]
        .map(|id| super::class_911_surface_row(6, id, crate::surface::SurfaceKind::Cylinder));
    let sweep = CircularSweepGeometry {
        cylinder_rows: rows.iter().collect(),
        section_definition_id: None,
        direction: [0.0, 0.0, -1.0],
        extent: ExtrudeExtent::OneSided {
            side: ExtrudeSide {
                termination: LinearTermination::Blind {
                    length: cadmpeg_ir::scalar::NonZeroLength::new(6.5)
                        .expect("nonzero length fixture"),
                },
                draft: None,
            },
        },
        geometry: HoleCylinder {
            origin: Point3::new(2.0, 3.0, 4.0),
            axis: Vector3::new(0.0, 0.0, -1.0),
            ref_direction: Vector3::new(1.0, 0.0, 0.0),
            radius: 1.5,
        },
    };

    assert_eq!(
        circular_sweep_feature_definition(
            ProfileRef::Planar(PlanarProfileRef::Sketch(
                SketchId::mint("creo:model:sketch#917".to_string()).expect("valid test fixture")
            )),
            &sweep,
            BooleanOp::Join,
            Some(true),
        ),
        IrFeatureDefinition::Extrude {
            profile: ProfileRef::Planar(PlanarProfileRef::Sketch(
                SketchId::mint("creo:model:sketch#917".to_string()).expect("valid test fixture")
            )),
            direction: cadmpeg_ir::features::ExtrudeDirection::Explicit {
                vector: cadmpeg_ir::features::FeatureDirection3::new(Vector3::new(0.0, 0.0, -1.0))
                    .expect("valid direction fixture"),
                source: None,
            },
            extent: ExtrudeExtent::OneSided {
                side: ExtrudeSide {
                    termination: LinearTermination::Blind {
                        length: cadmpeg_ir::scalar::NonZeroLength::new(6.5)
                            .expect("nonzero length fixture"),
                    },
                    draft: None,
                },
            },
            op: BooleanOp::Join,
            start: cadmpeg_ir::features::ExtrudeStart::ProfilePlane {},
            solid: Some(true),
            face_maker: None,
            inner_wire_taper: None,
            length_along_profile_normal: None,
            allow_multi_profile_faces: None,
        }
    );
}

#[test]
fn circular_sweep_cylinder_recovers_its_section_profile() {
    let transform = crate::placement::FeatureSectionTransform::new(
        917,
        Some(40),
        [1.0, 2.0, 3.0],
        [0.0, 0.0, -1.0],
        [1.0, 0.0, 0.0],
        20,
    )
    .expect("valid section frame");
    let cylinder = HoleCylinder {
        origin: Point3::new(5.0, -14.0, 1.0),
        axis: Vector3::new(0.0, 1.0, 0.0),
        ref_direction: Vector3::new(1.0, 0.0, 0.0),
        radius: 4.5,
    };

    assert_eq!(
        circular_section_profile_from_cylinder(&transform, &cylinder),
        Some(([2.0, 4.0], 4.5))
    );
    let mut off_axis = cylinder;
    off_axis.axis = Vector3::new(1.0, 0.0, 0.0);
    off_axis.ref_direction = Vector3::new(0.0, 0.0, 1.0);
    assert_eq!(
        circular_section_profile_from_cylinder(&transform, &off_axis),
        None
    );
}

#[test]
fn typed_center_locus_requires_a_circular_geometry_family() {
    let entity = SketchEntityId::mint("creo:test:entity#1").expect("valid test fixture");
    let definition = SketchConstraintDefinitionInput::CoincidentLoci {
        loci: vec![SketchLocus::Center(entity.clone())],
    };
    let unresolved = BTreeMap::from([(
        entity.clone(),
        SketchGeometry::native(
            cadmpeg_ir::products::NonEmptyString::new("solver_only_section_entity")
                .expect("nonempty source identity"),
        ),
    )]);
    assert!(!sketch_constraint_loci_compatible(&definition, &unresolved));

    let native_arc = BTreeMap::from([(
        entity.clone(),
        SketchGeometry::native(
            cadmpeg_ir::products::NonEmptyString::new("arc").expect("nonempty source identity"),
        ),
    )]);
    assert!(sketch_constraint_loci_compatible(&definition, &native_arc));

    let native_line = BTreeMap::from([(
        entity.clone(),
        SketchGeometry::native(
            cadmpeg_ir::products::NonEmptyString::new("line").expect("nonempty source identity"),
        ),
    )]);
    assert!(!sketch_constraint_loci_compatible(
        &definition,
        &native_line
    ));

    let resolved = BTreeMap::from([(
        entity,
        SketchGeometry::try_from(SketchGeometryDefinition::Circle {
            center: Point2::new(0.0, 0.0),
            radius: Length::new(1.0).expect("finite length fixture"),
        })
        .expect("valid test fixture"),
    )]);
    assert!(sketch_constraint_loci_compatible(&definition, &resolved));
}

#[test]
fn section_profile_prefers_a_resolved_sketch_chain() {
    let mut ir = CadIr::empty();
    ir.model.sketches.push(Sketch {
        id: SketchId::mint("creo:model:sketch#offset:40".to_string()).expect("valid test fixture"),
        name: None,
        configuration: None,
        visible: None,
        placement: cadmpeg_ir::sketches::SketchPlacement::try_resolved(
            Point3::new(0.0, 0.0, 0.0),
            Vector3::new(0.0, 0.0, 1.0),
            Vector3::new(1.0, 0.0, 0.0),
        )
        .expect("valid test fixture"),
        profiles: cadmpeg_ir::sketches::SketchProfiles::default(),
        native_ref: Some("creo:featdefs:sketch#offset:40".to_string()),
    });
    assert_eq!(
        section_profile_ref(&ir, "creo:featdefs:sketch#offset:40".to_string()),
        ProfileRef::Planar(PlanarProfileRef::Native(
            "creo:featdefs:sketch#offset:40".to_string()
        ))
    );

    ir.model.sketches[0].profiles.push_single(SketchEntityUse {
        entity: SketchEntityId::mint("creo:featdefs:sketch_entity#offset:40:4".to_string())
            .expect("valid test fixture"),
        reversed: false,
    });
    assert_eq!(
        section_profile_ref(&ir, "creo:featdefs:sketch#offset:40".to_string()),
        ProfileRef::Planar(PlanarProfileRef::Sketch(
            SketchId::mint("creo:model:sketch#offset:40".to_string()).expect("valid test fixture")
        ))
    );
    assert_eq!(
        section_profile_ref(&ir, "creo:featdefs:sketch#918".to_string()),
        ProfileRef::Planar(PlanarProfileRef::Native(
            "creo:featdefs:sketch#918".to_string()
        ))
    );
}

#[test]
fn connected_profile_vertices_include_open_chain_terminals() {
    let sketch_id =
        SketchId::mint("creo:model:sketch#917".to_string()).expect("valid test fixture");
    let entity_id = |external_id| {
        SketchEntityId::mint(format!("creo:featdefs:sketch_entity#917:{external_id}"))
            .expect("valid test fixture")
    };
    let mut ir = CadIr::empty();
    ir.model.sketches.push(Sketch {
        id: sketch_id.clone(),
        name: None,
        configuration: None,
        visible: None,
        placement: cadmpeg_ir::sketches::SketchPlacement::Unresolved,
        profiles: cadmpeg_ir::sketches::SketchProfiles::try_from(vec![vec![
            SketchEntityUse {
                entity: entity_id(1),
                reversed: false,
            },
            SketchEntityUse {
                entity: entity_id(2),
                reversed: true,
            },
        ]])
        .expect("valid test fixture"),
        native_ref: None,
    });
    ir.model.sketch_entities.extend([
        SketchEntity::new(
            entity_id(1),
            sketch_id.clone(),
            SketchGeometry::try_from(SketchGeometryDefinition::Line {
                start: Point2::new(0.0, 0.0),
                end: Point2::new(1.0, 0.0),
            })
            .expect("valid test fixture"),
        ),
        SketchEntity::new(
            entity_id(2),
            sketch_id.clone(),
            SketchGeometry::try_from(SketchGeometryDefinition::Line {
                start: Point2::new(1.0, 1.0),
                end: Point2::new(1.0, 0.0),
            })
            .expect("valid test fixture"),
        ),
    ]);

    assert_eq!(
        connected_sketch_profile_vertices(&ir, &sketch_id),
        vec![(0, vec![[0.0, 0.0], [1.0, 0.0], [1.0, 1.0]])]
    );

    ir.model.sketch_entities[1]
        .geometry
        .edit(|definition| {
            if let SketchGeometryDefinition::Line { start, .. } = definition {
                *start = Point2::new(0.0, 0.0);
            } else {
                unreachable!();
            }
        })
        .expect("valid test fixture");
    assert_eq!(
        connected_sketch_profile_vertices(&ir, &sketch_id),
        vec![(0, vec![[0.0, 0.0], [1.0, 0.0]])]
    );

    ir.model.sketch_entities[1]
        .geometry
        .edit(|definition| {
            if let SketchGeometryDefinition::Line { end, .. } = definition {
                *end = Point2::new(2.0, 0.0);
            } else {
                unreachable!();
            }
        })
        .expect("valid test fixture");
    assert!(connected_sketch_profile_vertices(&ir, &sketch_id).is_empty());
}

#[test]
fn ordered_hole_cap_planes_define_blind_direction_and_depth() {
    assert_eq!(
        hole_extent_and_direction([
            ([2.0, -21.0, -0.75], [1.0, 0.0, 0.0]),
            ([5.0, -22.5, 0.75], [-1.0, 0.0, 0.0]),
        ]),
        Some((
            [1.0, 0.0, 0.0],
            LinearTermination::Blind {
                length: cadmpeg_ir::scalar::NonZeroLength::new(3.0)
                    .expect("nonzero length fixture"),
            },
        ))
    );
    assert_eq!(
        hole_extent_and_direction([
            ([0.0, 0.5, 0.0], [0.0, 1.0, 0.0]),
            ([0.0, -0.5, 0.0], [0.0, 1.0, 0.0]),
        ]),
        Some((
            [-0.0, -1.0, -0.0],
            LinearTermination::Blind {
                length: cadmpeg_ir::scalar::NonZeroLength::new(1.0)
                    .expect("nonzero length fixture"),
            },
        ))
    );
    assert_eq!(
        hole_extent_and_direction([
            ([0.0; 3], [1.0, 0.0, 0.0]),
            ([1.0, 0.0, 0.0], [0.0, 1.0, 0.0]),
        ]),
        None
    );

    assert_eq!(
        hole_placement([
            (902, [0.0, 0.0, 0.85], [0.0, 0.0, 1.0]),
            (905, [0.0, 0.0, 7.35], [0.0, 0.0, -1.0]),
        ]),
        Some((
            902,
            [0.0, 0.0, 1.0],
            LinearTermination::Blind {
                length: cadmpeg_ir::scalar::NonZeroLength::new(6.5)
                    .expect("nonzero length fixture"),
            },
        ))
    );
    assert_eq!(
        hole_placement([
            (902, [0.0; 3], [0.0, 0.0, 1.0]),
            (905, [0.0, 0.0, 1.0], [0.0, 0.0, -1.0]),
            (908, [0.0, 0.0, 2.0], [0.0, 0.0, -1.0]),
        ]),
        None
    );
    assert!(matches!(
        hole_cylinder_from_cap_outlines([
            CapOutline { surface_id: 902, origin: [0.0, 0.0, 0.85], normal: [0.0, 0.0, 1.0], corners: [[-1.5, 17.5, 0.85], [1.5, 20.5, 0.85]] },
            CapOutline { surface_id: 905, origin: [0.0, 0.0, 7.35], normal: [0.0, 0.0, -1.0], corners: [[-1.5, 17.5, 7.35], [1.5, 20.5, 7.35]] },
        ]),
        Some(HoleCylinder { origin, axis, radius, .. })
            if origin == Point3::new(0.0, 19.0, 0.85)
                && axis == Vector3::new(0.0, 0.0, 1.0)
                && radius == 1.5
    ));
    assert!(hole_cylinder_from_cap_outlines([
        CapOutline {
            surface_id: 902,
            origin: [0.0, 0.0, 0.0],
            normal: [0.0, 0.0, 1.0],
            corners: [[-1.0, -2.0, 0.0], [1.0, 2.0, 0.0]]
        },
        CapOutline {
            surface_id: 905,
            origin: [0.0, 0.0, 1.0],
            normal: [0.0, 0.0, -1.0],
            corners: [[-1.0, -2.0, 1.0], [1.0, 2.0, 1.0]]
        },
    ])
    .is_none());
    assert!(matches!(
        circular_sweep_cylinder_from_cap_outlines([
            (828, [0.0, 4.0, 0.0], [0.0, 1.0, 0.0]),
            (831, [0.0, -4.0, 0.0], [0.0, 1.0, 0.0]),
        ], [
            CapOutline { surface_id: 828, origin: [0.0, 4.0, 0.0], normal: [0.0, 1.0, 0.0], corners: [[-13.25, 4.0, -0.75], [-11.75, 4.0, 0.75]] },
        ]),
        Some(HoleCylinder { origin, axis, radius, .. })
            if origin == Point3::new(-12.5, 4.0, 0.0)
                && axis == Vector3::new(0.0, -1.0, 0.0)
                && radius == 0.75
    ));
    assert!(matches!(
        cylinder_from_single_cap_outline(CapOutline { surface_id: 46, origin: [0.0, 16.0, 0.0], normal: [0.0, 1.0, 0.0], corners: [[-4.45, 16.0, -4.45], [4.45, 16.0, 4.45]] }),
        Some(HoleCylinder { origin, axis, radius, .. })
            if origin == Point3::new(0.0, 16.0, 0.0)
                && axis == Vector3::new(0.0, 1.0, 0.0)
                && radius == 4.45
    ));
}
