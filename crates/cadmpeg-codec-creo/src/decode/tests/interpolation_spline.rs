// SPDX-License-Identifier: Apache-2.0
//! Tests: interpolation spline.

use crate::decode::analytic::equations::PlaneEquation;
use crate::decode::feature_history::{
    class_942_boundary_surface_entity_graph, draft_neutral_plane_selection,
    feature_allows_linear_extrusion, feature_is_sheet_extrusion, feature_surface_transitions,
    filled_surface_feature_definition, named_feature_definition, new_sheet_output_surface_id,
    numbered_feature_name_has_family, preceding_features_establish_body,
    reference_named_feature_definition, schema_feature_definition,
    section_sweep_allows_linear_extrusion, section_sweep_boolean_operation,
    surface_transition_dependencies, sweep_output_kind, thicken_plane_offset,
};
use crate::decode::holes::{extrusion_extent_and_direction, ExtrusionSpan};
use crate::decode::sketch_transfer::recipe::{
    current_additive_feature_recipe, current_feature_recipe, current_feature_recipe_parent,
};
use crate::decode::sweep::profiles::ProfileEntity;
use crate::decode::sweep::{
    arcs_intersect, extrusion_brep_side_surface, extrusion_cap_pcurve, extrusion_side_uvs,
    line_arc_intersect, ordered_extrusion_profiles, profile_segments_intersect,
    profile_strictly_contains, resolved_sketch_profiles, ExtrusionProfile,
};
use crate::decode::uniqueness::unique_feature_profile_definition;
use crate::feature::schema::SchemaClass;
use cadmpeg_ir::document::CadIr;
use cadmpeg_ir::features::{
    AngularTermination, BooleanOp, ChamferSpec, EdgeSelection, ExtrudeDirection, ExtrudeExtent,
    ExtrudeSide, FaceSelection, Feature, FeatureDefinition as IrFeatureDefinition,
    FeatureId as IrFeatureId, LinearTermination, PathRef, ProfileRef, SurfaceBoundary, ThickenSide,
    UnresolvedFamily,
};
use cadmpeg_ir::geometry::{PcurveGeometry, Surface, SurfaceGeometry};
use cadmpeg_ir::ids::{BodyId, SurfaceId};
use cadmpeg_ir::math::{Point2, Point3, Vector3};
use cadmpeg_ir::scalar::{Angle, Length};
use cadmpeg_ir::sketches::{
    Sketch, SketchEntity, SketchEntityId, SketchEntityUse, SketchGeometry,
    SketchGeometryDefinition, SketchId,
};
use cadmpeg_ir::topology::BodyKind;
use std::collections::BTreeMap;

const EPS_FULL_TURN: f64 = 1e-12;

#[test]
// These checked constructors must accept the explicit test fixtures.
#[allow(clippy::unwrap_used)]
fn interpolation_spline_remains_a_closed_extrusion_profile() {
    let sketch_id = SketchId::mint("creo:model:sketch#spline".to_string()).unwrap();
    let spline_id = SketchEntityId::mint("creo:model:sketch_entity#spline".to_string()).unwrap();
    let first_line_id =
        SketchEntityId::mint("creo:model:sketch_entity#first-line".to_string()).unwrap();
    let second_line_id =
        SketchEntityId::mint("creo:model:sketch_entity#second-line".to_string()).unwrap();
    let spline = SketchGeometry::nurbs(
        cadmpeg_ir::geometry::PcurveNurbs::new(
            3,
            vec![2.0, 2.0, 2.0, 2.0, 5.0, 5.0, 5.0, 5.0],
            vec![
                Point2::new(1.0, 0.0),
                Point2::new(1.0, 0.552_284_749_8),
                Point2::new(0.552_284_749_8, 1.0),
                Point2::new(0.0, 1.0),
            ],
            Some(vec![1.0, 0.75, 0.75, 1.0]),
            false,
        )
        .unwrap(),
    );
    let first_line = SketchGeometry::try_from(SketchGeometryDefinition::Line {
        start: Point2::new(0.0, 1.0),
        end: Point2::new(0.0, 0.0),
    })
    .unwrap();
    let second_line = SketchGeometry::try_from(SketchGeometryDefinition::Line {
        start: Point2::new(0.0, 0.0),
        end: Point2::new(1.0, 0.0),
    })
    .unwrap();
    let mut ir = CadIr::empty();
    ir.model.sketches.push(Sketch {
        id: sketch_id.clone(),
        name: None,
        configuration: None,
        visible: None,
        placement: cadmpeg_ir::sketches::SketchPlacement::Unresolved,
        profiles: cadmpeg_ir::sketches::SketchProfiles::try_from(vec![vec![
            SketchEntityUse {
                entity: spline_id.clone(),
                reversed: false,
            },
            SketchEntityUse {
                entity: first_line_id.clone(),
                reversed: false,
            },
            SketchEntityUse {
                entity: second_line_id.clone(),
                reversed: false,
            },
        ]])
        .unwrap(),
        native_ref: None,
    });
    for (id, geometry) in [
        (spline_id, spline.clone()),
        (first_line_id, first_line.clone()),
        (second_line_id, second_line.clone()),
    ] {
        ir.model
            .sketch_entities
            .push(SketchEntity::new(id, sketch_id.clone(), geometry));
    }

    let profiles = resolved_sketch_profiles(&ir, &sketch_id, 1).expect("spline profile");
    assert_eq!(profiles[0][0].start(), [1.0, 0.0]);
    assert_eq!(profiles[0][0].end(), [0.0, 1.0]);
    let ordered = ordered_extrusion_profiles(profiles.clone()).expect("closed spline");
    let area = ordered[0].area();
    assert_eq!(
        ordered
            .iter()
            .map(|profile| profile.entities().clone())
            .collect::<Vec<_>>(),
        profiles
    );
    assert!(area > 0.0);
    assert!(profile_strictly_contains(&profiles[0], [0.2, 0.2]));
    assert!(!profile_strictly_contains(&profiles[0], [2.0, 2.0]));
    let diagonal = ProfileEntity::new(
        SketchGeometry::nurbs(
            cadmpeg_ir::geometry::PcurveNurbs::new(
                1,
                vec![0.0, 0.0, 1.0, 1.0],
                vec![Point2::new(0.0, 0.0), Point2::new(1.0, 1.0)],
                None,
                false,
            )
            .unwrap(),
        ),
        false,
    )
    .expect("valid profile entity");
    let crossing_line = ProfileEntity::new(
        SketchGeometry::try_from(SketchGeometryDefinition::Line {
            start: Point2::new(0.0, 1.0),
            end: Point2::new(1.0, 0.0),
        })
        .unwrap(),
        false,
    )
    .expect("valid profile entity");
    assert!(profile_segments_intersect(
        &diagonal,
        &crossing_line,
        1.0e-9
    ));

    for reversed in [false, true] {
        let start = if reversed { [0.0, 1.0] } else { [1.0, 0.0] };
        let end = if reversed { [1.0, 0.0] } else { [0.0, 1.0] };
        let pcurve = extrusion_cap_pcurve(&spline, reversed, start, end).unwrap();
        let PcurveGeometry::Nurbs { nurbs } = &pcurve else {
            panic!("spline cap pcurve is not NURBS");
        };
        assert_eq!(nurbs.weights(), Some(&[1.0, 0.75, 0.75, 1.0][..]));
        let first = cadmpeg_ir::eval::pcurve_uv(&pcurve, 2.0).expect("spline start");
        let last = cadmpeg_ir::eval::pcurve_uv(&pcurve, 5.0).expect("spline end");
        assert!((first.u - start[0]).abs() < 1.0e-12);
        assert!((first.v - start[1]).abs() < 1.0e-12);
        assert!((last.u - end[0]).abs() < 1.0e-12);
        assert!((last.v - end[1]).abs() < 1.0e-12);
        assert_eq!(
            extrusion_side_uvs(
                &spline,
                reversed,
                start,
                end,
                ExtrusionSpan {
                    lower: -2.0,
                    upper: 3.0,
                },
            ),
            [
                [[2.0, 0.0], [5.0, 0.0]],
                [[5.0, 0.0], [5.0, 1.0]],
                [[2.0, 1.0], [5.0, 1.0]],
                [[2.0, 0.0], [2.0, 1.0]],
            ]
        );
    }

    let transform = crate::placement::FeatureSectionTransform::new(
        1,
        Some(1),
        [10.0, 20.0, 30.0],
        [1.0, 0.0, 0.0],
        [0.0, 1.0, 0.0],
        0,
    )
    .expect("valid section frame");
    let side = extrusion_brep_side_surface(
        &transform,
        &spline,
        false,
        [1.0, 0.0],
        [0.0, 1.0],
        ExtrusionSpan {
            lower: -2.0,
            upper: 3.0,
        },
    )
    .expect("spline side surface");
    let SurfaceGeometry::Nurbs(side) = side else {
        panic!("spline side surface is not NURBS");
    };
    assert_eq!((side.u_degree(), side.v_degree()), (3, 1));
    assert_eq!(side.u_knots(), [2.0, 2.0, 2.0, 2.0, 5.0, 5.0, 5.0, 5.0]);
    assert_eq!(side.v_knots(), [0.0, 0.0, 1.0, 1.0]);
    assert_eq!(
        side.poles().nth(0).copied().unwrap(),
        Point3::new(11.0, 20.0, 28.0)
    );
    assert_eq!(
        side.poles().nth(1).copied().unwrap(),
        Point3::new(11.0, 20.0, 33.0)
    );
    assert_eq!(
        side.poles().nth(6).copied().unwrap(),
        Point3::new(10.0, 21.0, 28.0)
    );
    assert_eq!(
        side.poles().nth(7).copied().unwrap(),
        Point3::new(10.0, 21.0, 33.0)
    );
    assert_eq!(
        side.pole_weights()
            .map(std::iter::Iterator::collect::<Vec<_>>),
        Some([1.0, 1.0, 0.75, 0.75, 0.75, 0.75, 1.0, 1.0].to_vec())
    );
}

#[test]
fn extrusion_profiles_require_one_oppositely_oriented_hole() {
    let rectangle = |minimum: [f64; 2], maximum: [f64; 2], clockwise: bool| {
        let mut points = [
            minimum,
            [maximum[0], minimum[1]],
            maximum,
            [minimum[0], maximum[1]],
        ];
        if clockwise {
            points.reverse();
        }
        (0..4)
            .map(|index| {
                let start = points[index];
                let end = points[(index + 1) % 4];
                ProfileEntity::new(
                    SketchGeometry::try_from(SketchGeometryDefinition::Line {
                        start: Point2::new(start[0], start[1]),
                        end: Point2::new(end[0], end[1]),
                    })
                    .expect("valid test fixture"),
                    false,
                )
                .expect("valid profile entity")
            })
            .collect::<ExtrusionProfile>()
    };
    let outer = rectangle([-2.0, -2.0], [2.0, 2.0], false);
    let hole = rectangle([-1.0, -1.0], [1.0, 1.0], true);
    let profiles = ordered_extrusion_profiles(vec![hole.clone(), outer.clone()])
        .expect("strict outer and hole");
    let outer_area = profiles[0].area();
    assert_eq!(profiles[0].entities(), &outer);
    assert!(outer_area > 0.0);
    assert!(profiles[1].area() < 0.0);

    assert!(ordered_extrusion_profiles(vec![
        rectangle([-2.0, -2.0], [2.0, 2.0], false),
        rectangle([-1.0, -1.0], [1.0, 1.0], false),
    ])
    .is_none());
    assert!(ordered_extrusion_profiles(vec![
        rectangle([-2.0, -2.0], [2.0, 2.0], false),
        rectangle([1.0, -1.0], [3.0, 1.0], true),
    ])
    .is_none());

    let circular_hole = [
        (std::f64::consts::PI, 0.0, [-0.5, 0.0], [0.5, 0.0]),
        (
            std::f64::consts::TAU,
            std::f64::consts::PI,
            [0.5, 0.0],
            [-0.5, 0.0],
        ),
    ]
    .into_iter()
    .map(|(end_angle, start_angle, _, _)| {
        ProfileEntity::new(
            SketchGeometry::try_from(SketchGeometryDefinition::Arc {
                center: Point2::new(0.0, 0.0),
                radius: Length::new(0.5).expect("finite length fixture"),
                start_angle: Angle::new(start_angle).expect("finite angle fixture"),
                end_angle: Angle::new(end_angle).expect("finite angle fixture"),
            })
            .expect("valid test fixture"),
            true,
        )
        .expect("valid profile entity")
    })
    .collect::<ExtrusionProfile>();
    let profiles = ordered_extrusion_profiles(vec![
        circular_hole,
        rectangle([-2.0, -2.0], [2.0, 2.0], false),
    ])
    .expect("arc-bounded hole");
    assert!(matches!(
        profiles[1].entities()[0].geometry(),
        crate::decode::sweep::profiles::ProfileGeometry::Arc { .. }
    ));
}

#[test]
fn extrusion_profile_intersections_include_analytic_tangency() {
    let full_upper_circle = ([0.0, 0.0], 1.0, 0.0, std::f64::consts::PI);
    assert!(line_arc_intersect(
        [[-2.0, 1.0], [2.0, 1.0]],
        full_upper_circle,
        1.0e-9,
    ));
    assert!(!line_arc_intersect(
        [[-2.0, 1.1], [2.0, 1.1]],
        full_upper_circle,
        1.0e-9,
    ));
    assert!(arcs_intersect(
        full_upper_circle,
        ([2.0, 0.0], 1.0, std::f64::consts::PI, std::f64::consts::PI),
        1.0e-9,
    ));
    assert!(!arcs_intersect(
        full_upper_circle,
        ([3.0, 0.0], 1.0, std::f64::consts::PI, std::f64::consts::PI),
        1.0e-9,
    ));
}

#[test]
fn equal_opposite_cap_planes_define_symmetric_extent() {
    let extent = extrusion_extent_and_direction(
        [0.0, 0.0, 0.0],
        [0.0, -1.0, 0.0],
        [
            ([0.0, 4.0, 0.0], [0.0, 1.0, 0.0]),
            ([0.0, -4.0, 0.0], [0.0, 1.0, 0.0]),
            ([3.0, 0.0, 0.0], [1.0, 0.0, 0.0]),
        ],
    );

    assert_eq!(
        extent,
        Some((
            ExtrudeExtent::Symmetric {
                side: ExtrudeSide {
                    termination: LinearTermination::Blind {
                        length: cadmpeg_ir::scalar::NonZeroLength::new(8.0)
                            .expect("nonzero length fixture")
                    },
                    draft: None,
                }
            },
            [0.0, -1.0, 0.0]
        ))
    );
}

#[test]
fn cap_proof_classifies_section_sweeps_without_overriding_revolves() {
    use crate::feature::FeatureRecipeKind::{Extrude, Revolve};

    assert!(section_sweep_allows_linear_extrusion(
        Some(SchemaClass::Cut),
        None
    ));
    assert!(section_sweep_allows_linear_extrusion(
        Some(SchemaClass::Protrusion),
        None
    ));
    assert!(section_sweep_allows_linear_extrusion(
        Some(SchemaClass::Protrusion),
        Some(Extrude)
    ));
    assert!(section_sweep_allows_linear_extrusion(None, Some(Extrude)));
    assert!(!section_sweep_allows_linear_extrusion(
        Some(SchemaClass::Protrusion),
        Some(Revolve)
    ));
    assert!(!section_sweep_allows_linear_extrusion(
        Some(SchemaClass::DatumPlane),
        None
    ));
}

#[test]
fn unresolved_display_state_family_blocks_schema_sweep_fallback() {
    let mut scan = crate::container::scan_bytes(Vec::new());
    scan.features
        .operations
        .push(crate::feature::FeatureOperation {
            feature_id: 917,
            kind: crate::feature::OperationKind::Native,
            name: crate::feature::operations::OperationName::Derived,
            recipe: crate::feature::RecipeResolution::None,
            display_state_conflict: true,
            depdb: Some(crate::feature::operations::DepdbPrefix {
                schema: crate::feature::schema::SchemaClass::Protrusion,
                parent: 0,
            }),
            offset: 0,
            state_offset: 0,
        });

    assert!(!feature_allows_linear_extrusion(&scan, 917));
    scan.features.operations[0].kind = crate::feature::OperationKind::Extrude;
    assert!(feature_allows_linear_extrusion(&scan, 917));
}

#[test]
fn class_942_linear_sweep_requires_a_numbered_extrude_reference() {
    let mut scan = crate::container::scan_bytes(Vec::new());
    scan.features
        .operations
        .push(crate::feature::FeatureOperation {
            feature_id: 942,
            kind: crate::feature::OperationKind::Stored("Surface".to_string()),
            name: crate::feature::operations::OperationName::Stored {
                bytes: b"Surface id 942".to_vec(),
                keyword: crate::feature::operations::IdKeyword::Id,
                prefix: None,
            },
            recipe: crate::feature::RecipeResolution::None,
            display_state_conflict: false,
            depdb: Some(crate::feature::operations::DepdbPrefix {
                schema: crate::feature::schema::SchemaClass::Surface,
                parent: 0,
            }),
            offset: 0,
            state_offset: 0,
        });
    scan.features
        .reference_names
        .push(crate::feature::FeatureReferenceName {
            feature_id: 942,
            name_bytes: b"Extrude 1".to_vec(),
            own_reference_id: 1,
            reference_type: 0,
            offset: 0,
        });

    assert!(feature_is_sheet_extrusion(&scan, 942));
    assert!(feature_allows_linear_extrusion(&scan, 942));
    assert_eq!(
        sweep_output_kind(&scan, &CadIr::empty(), "extrusion", 942),
        Some(BodyKind::Sheet)
    );
    assert!(matches!(
        schema_feature_definition(
            &scan,
            &CadIr::empty(),
            942,
            Some(SchemaClass::Surface),
            "Surface"
        )
        .expect("valid test fixture"),
        IrFeatureDefinition::Extrude {
            profile: ProfileRef::Unresolved(_),
            op: BooleanOp::NewBody,
            solid: Some(false),
            ..
        }
    ));

    scan.features.reference_names[0].name_bytes = b"Boundary Blend 1".to_vec();
    assert!(!feature_is_sheet_extrusion(&scan, 942));
    assert!(!feature_allows_linear_extrusion(&scan, 942));
    assert_eq!(
        sweep_output_kind(&scan, &CadIr::empty(), "extrusion", 942),
        None
    );
    assert!(matches!(
        schema_feature_definition(
            &scan,
            &CadIr::empty(),
            942,
            Some(SchemaClass::Surface),
            "Surface"
        )
        .expect("valid test fixture"),
        IrFeatureDefinition::Unresolved {
            family: UnresolvedFamily::BoundarySurface
        }
    ));
}

#[test]
fn class_942_schema_state_precedes_surface_body_tree_fallback() {
    let mut scan = crate::container::scan_bytes(Vec::new());
    scan.features
        .operations
        .push(crate::feature::FeatureOperation {
            feature_id: 942,
            kind: crate::feature::OperationKind::Stored("Surface".to_string()),
            name: crate::feature::operations::OperationName::Stored {
                bytes: b"Surface id 942".to_vec(),
                keyword: crate::feature::operations::IdKeyword::Id,
                prefix: None,
            },
            recipe: crate::feature::RecipeResolution::None,
            display_state_conflict: false,
            depdb: Some(crate::feature::operations::DepdbPrefix {
                schema: crate::feature::schema::SchemaClass::Surface,
                parent: 0,
            }),
            offset: 0,
            state_offset: 0,
        });

    assert!(matches!(
        schema_feature_definition(&scan, &CadIr::empty(), 942, Some(SchemaClass::Surface), "Surface").expect("valid test fixture"),
        IrFeatureDefinition::Native { kind, .. } if kind.as_str() == "Surface"
    ));
}

#[test]
fn class_942_sheet_extrusion_uses_linear_cap_extent_evaluation() {
    let mut scan = crate::container::scan_bytes(Vec::new());
    scan.features
        .operations
        .push(crate::feature::FeatureOperation {
            feature_id: 942,
            kind: crate::feature::OperationKind::Stored("Surface".to_string()),
            name: crate::feature::operations::OperationName::Stored {
                bytes: b"Surface id 942".to_vec(),
                keyword: crate::feature::operations::IdKeyword::Id,
                prefix: None,
            },
            recipe: crate::feature::RecipeResolution::None,
            display_state_conflict: false,
            depdb: Some(crate::feature::operations::DepdbPrefix {
                schema: crate::feature::schema::SchemaClass::Surface,
                parent: 0,
            }),
            offset: 0,
            state_offset: 0,
        });
    scan.features
        .reference_names
        .push(crate::feature::FeatureReferenceName {
            feature_id: 942,
            name_bytes: b"Extrude 1".to_vec(),
            own_reference_id: 1,
            reference_type: 0,
            offset: 0,
        });
    scan.features.section_transforms.push(
        crate::placement::FeatureSectionTransform::new(
            1,
            Some(942),
            [0.0, 0.0, 0.0],
            [1.0, 0.0, 0.0],
            [0.0, 1.0, 0.0],
            0,
        )
        .expect("valid section frame"),
    );
    let entry = |entity_id, class_id, source_entity_id| crate::feature::FeatureEntityTableEntry {
        payload: crate::feature::entry_payload(class_id, source_entity_id, None, None),

        entity_id,
        prefixed: false,
        offset: 0,
        end_offset: 0,
    };
    scan.features.entity_tables.push(
        crate::feature::FeatureEntityTable::new(
            942,
            29,
            vec![
                entry(31, 204, None),
                entry(32, 203, None),
                entry(33, 200, Some(11)),
            ],
            &std::collections::BTreeSet::new(),
            0,
        )
        .with_surface_ids([31, 32, 33]),
    );
    let row = |id| crate::surface::SurfaceRow {
        id,
        kind: crate::surface::SurfaceKind::Plane,
        feature_id: 942,
        reversed: false,
        boundary_type: crate::surface::BoundaryType::Code00,
        next_surface: 0,
        offset: id as usize,
    };
    scan.surfaces.rows.extend([row(31), row(32), row(33)]);
    let plane = |id, z| Surface {
        id: SurfaceId::mint(format!("creo:visibgeom:surface#{id}")).expect("identity grammar"),
        geometry: SurfaceGeometry::Plane(
            cadmpeg_ir::geometry::PlaneSurface::try_new(
                Point3::new(0.0, 0.0, z),
                Vector3::new(0.0, 0.0, 1.0),
                Vector3::new(1.0, 0.0, 0.0),
            )
            .expect("valid PlaneSurface fixture"),
        ),
        source_object: None,
    };
    let mut ir = CadIr::empty();
    ir.model.surfaces.extend([plane(31, 2.0), plane(32, 8.0)]);

    assert!(matches!(
        schema_feature_definition(&scan, &ir, 942, Some(SchemaClass::Surface), "Surface").expect("valid test fixture"),
        IrFeatureDefinition::Extrude {
            direction: cadmpeg_ir::features::ExtrudeDirection::Explicit {
                vector: direction,
                ..
            },
            extent: ExtrudeExtent::OneSided {
                side: ExtrudeSide {
                    termination: LinearTermination::Blind {
                        length: actual_length,
                    },
                    ..
                }
            },
            op: BooleanOp::NewBody,
            solid: Some(false),
            ..
        } if (direction == Vector3::new(0.0, 0.0, 1.0)) && actual_length.get() == 6.0
    ));
}

#[test]
fn numbered_reference_name_selects_only_its_exact_feature_family() {
    assert!(numbered_feature_name_has_family("Thicken 1", "Thicken"));
    assert!(numbered_feature_name_has_family("Thicken 12", "Thicken"));
    assert!(!numbered_feature_name_has_family("Thicken", "Thicken"));
    assert!(!numbered_feature_name_has_family("Thicken A", "Thicken"));
    assert!(!numbered_feature_name_has_family("GThicken 1", "Thicken"));
    assert!(matches!(
        reference_named_feature_definition("Boundary Blend 1"),
        Some(IrFeatureDefinition::Unresolved {
            family: UnresolvedFamily::BoundarySurface
        })
    ));
    assert!(matches!(
        reference_named_feature_definition("Thicken 1"),
        Some(IrFeatureDefinition::Thicken {
            faces: FaceSelection::Unresolved,
            thickness: None,
            side: None,
        })
    ));
    assert!(reference_named_feature_definition("Fill 1").is_none());
    assert!(matches!(
        reference_named_feature_definition("Merge 2"),
        Some(IrFeatureDefinition::KnitSurface {
            faces: FaceSelection::Unresolved,
            merge_entities: Some(true),
            create_solid: Some(false),
            gap_tolerance: None,
        })
    ));
    assert!(reference_named_feature_definition("Extrude 2").is_none());
}

#[test]
fn feature_surface_transitions_require_complete_unique_predecessor_chains() {
    let entry = |entity_id, class_id, related_entity_id| crate::feature::FeatureEntityTableEntry {
        payload: crate::feature::entry_payload(
            class_id,
            None,
            related_entity_id,
            related_entity_id.map(|_| 0),
        ),

        entity_id,
        prefixed: true,
        offset: entity_id as usize,
        end_offset: entity_id as usize,
    };
    let table = crate::feature::FeatureEntityTable::new(
        17,
        80,
        vec![
            entry(101, 214, Some(11)),
            entry(201, 210, Some(101)),
            entry(102, 214, Some(12)),
            entry(202, 210, Some(102)),
        ],
        &std::collections::BTreeSet::new(),
        0,
    )
    .with_surface_ids([201, 202]);
    let row = |id, feature_id| crate::surface::SurfaceRow {
        id,
        kind: crate::surface::SurfaceKind::Plane,
        feature_id,
        reversed: false,
        boundary_type: crate::surface::BoundaryType::Code00,
        next_surface: 0,
        offset: id as usize,
    };
    let rows = vec![row(11, 3), row(12, 4), row(201, 17), row(202, 17)];

    assert_eq!(
        feature_surface_transitions(17, std::slice::from_ref(&table), &rows),
        Some(vec![(11, 201), (12, 202)])
    );

    let mut partial = table.clone();
    partial.entries.pop();
    assert_eq!(feature_surface_transitions(17, &[partial], &rows), None);

    let mut conflicting = table.clone();
    conflicting.entries[3].payload = crate::feature::EntryPayload::Related {
        class: crate::feature::RelatedClass::Class210,
        entity: 101,
        state: crate::feature::RelatedState::Zero,
    };
    assert_eq!(feature_surface_transitions(17, &[conflicting], &rows), None);
    let mut wrong_predecessor_class = table.clone();
    wrong_predecessor_class.entries[0].payload = crate::feature::EntryPayload::Related {
        class: crate::feature::RelatedClass::Class219,
        entity: 11,
        state: crate::feature::RelatedState::Zero,
    };
    wrong_predecessor_class
        .entries
        .push(entry(999, 214, Some(888)));
    assert_eq!(
        feature_surface_transitions(17, &[wrong_predecessor_class], &rows),
        None
    );
    assert_eq!(
        surface_transition_dependencies(17, std::slice::from_ref(&table), &rows),
        [3, 4]
    );
}

#[test]
fn draft_neutral_plane_requires_one_owned_class_209_plane() {
    let entry = |entity_id, class_id| crate::feature::FeatureEntityTableEntry {
        payload: crate::feature::entry_payload(class_id, None, None, None),

        entity_id,
        prefixed: true,
        offset: entity_id as usize,
        end_offset: entity_id as usize,
    };
    let table = |entries: Vec<crate::feature::FeatureEntityTableEntry>, surface_ids| {
        crate::feature::FeatureEntityTable::new(
            225,
            29,
            entries,
            &std::collections::BTreeSet::new(),
            0,
        )
        .with_surface_ids(surface_ids)
    };
    let row = |id, kind: crate::surface::SurfaceKind, feature_id| crate::surface::SurfaceRow {
        id,
        kind,
        feature_id,
        reversed: false,
        boundary_type: crate::surface::BoundaryType::Code00,
        next_surface: 0,
        offset: id as usize,
    };
    let mut scan = crate::container::scan_bytes(Vec::new());
    scan.features
        .entity_tables
        .push(table(vec![entry(226, 209)], vec![226]));
    scan.surfaces
        .rows
        .push(row(226, crate::surface::SurfaceKind::Plane, 225));
    assert_eq!(
        draft_neutral_plane_selection(&scan, 225),
        FaceSelection::Native("creo:visibgeom:surface#226".to_string())
    );

    scan.features.entity_tables[0].mark_surface_ids([]);
    assert_eq!(
        draft_neutral_plane_selection(&scan, 225),
        FaceSelection::Unresolved
    );
    scan.features.entity_tables[0].mark_surface_ids([226]);
    scan.features
        .entity_tables
        .push(table(vec![entry(227, 209)], vec![227]));
    scan.surfaces
        .rows
        .push(row(227, crate::surface::SurfaceKind::Plane, 225));
    assert_eq!(
        draft_neutral_plane_selection(&scan, 225),
        FaceSelection::Unresolved
    );
}

#[test]
fn draft_neutral_plane_rejects_foreign_or_non_plane_surface_rows() {
    let table = crate::feature::FeatureEntityTable::new(
        225,
        64,
        vec![crate::feature::FeatureEntityTableEntry {
            entity_id: 226,
            payload: crate::feature::entry_payload(209, None, None, None),
            prefixed: true,
            offset: 0,
            end_offset: 0,
        }],
        &std::collections::BTreeSet::new(),
        0,
    )
    .with_surface_ids([226]);
    for (kind, owner) in [
        (crate::surface::SurfaceKind::Cylinder, 225),
        (crate::surface::SurfaceKind::Plane, 224),
    ] {
        let mut scan = crate::container::scan_bytes(Vec::new());
        scan.features.entity_tables.push(table.clone());
        scan.surfaces.rows.push(crate::surface::SurfaceRow {
            id: 226,
            kind,
            feature_id: owner,
            reversed: false,
            boundary_type: crate::surface::BoundaryType::Code00,
            next_surface: 0,
            offset: 0,
        });
        assert_eq!(
            draft_neutral_plane_selection(&scan, 225),
            FaceSelection::Unresolved
        );
    }
}

#[test]
fn thicken_plane_offsets_require_parallel_agreeing_oriented_distances() {
    let plane = |origin, normal| PlaneEquation { origin, normal };
    let mut planes = BTreeMap::from([
        (11, plane([0.0, 2.0, 0.0], [0.0, -1.0, 0.0])),
        (12, plane([4.0, 0.0, 0.0], [1.0, 0.0, 0.0])),
        (201, plane([0.0, -3.0, 0.0], [0.0, 1.0, 0.0])),
        (202, plane([-1.0, 0.0, 0.0], [1.0, 0.0, 0.0])),
    ]);
    let transitions = [(11, 201), (12, 202), (13, 203)];
    let row = |id, reversed| crate::surface::SurfaceRow {
        id,
        kind: crate::surface::SurfaceKind::Plane,
        feature_id: if id >= 200 { 17 } else { 3 },
        reversed,
        boundary_type: crate::surface::BoundaryType::Code00,
        next_surface: 0,
        offset: id as usize,
    };
    let mut rows = vec![
        row(11, true),
        row(12, false),
        row(201, false),
        row(202, true),
    ];

    assert_eq!(
        thicken_plane_offset(&transitions, &planes, &rows),
        Some((5.0, ThickenSide::Reverse))
    );

    planes.get_mut(&201).expect("plane").origin[1] = 7.0;
    planes.get_mut(&202).expect("plane").origin[0] = 9.0;
    assert_eq!(
        thicken_plane_offset(&transitions, &planes, &rows),
        Some((5.0, ThickenSide::Forward))
    );

    planes.get_mut(&202).expect("plane").origin[0] = -2.0;
    assert_eq!(thicken_plane_offset(&transitions, &planes, &rows), None);

    planes.get_mut(&202).expect("plane").origin[0] = -1.0;
    planes.get_mut(&202).expect("plane").normal = [0.0, 1.0, 0.0];
    assert_eq!(thicken_plane_offset(&transitions, &planes, &rows), None);

    planes.get_mut(&202).expect("plane").normal = [1.0, 0.0, 0.0];
    rows[3].reversed = false;
    assert_eq!(thicken_plane_offset(&transitions, &planes, &rows), None);
}

#[test]
fn feature_profile_definition_uses_unique_transform_or_unique_owner() {
    let definition = crate::feature::FeatureDefinition {
        identity: crate::feature::definitions::DefinitionIdentity::Parsed {
            schema_id: std::num::NonZeroU32::new(822),
            owner_feature_id: Some(822),
        },
        body: Vec::new(),
        parameter_frames: Vec::new(),
        outlines: Vec::new(),
        variables: None,
        segments: None,
        trim_entities: None,
        trim_vertices: None,
        order_table: None,
        section_3d: Some(crate::feature::FeatureSection3d {
            sketch_plane_entity_id: None,
            sketch_plane_flip: None,
            reference_planes: crate::feature::definitions::ReferencePlanes::Named(Vec::new()),
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
    let transform = crate::placement::FeatureSectionTransform::new(
        822,
        Some(822),
        [0.0; 3],
        [1.0, 0.0, 0.0],
        [0.0, 1.0, 0.0],
        90,
    )
    .expect("valid section frame");

    let mismatched_transform = crate::placement::FeatureSectionTransform::new(
        transform.definition_id,
        Some(900),
        transform.origin(),
        transform.u_axis(),
        transform.v_axis(),
        transform.offset,
    )
    .expect("valid section frame");
    assert_eq!(
        unique_feature_profile_definition(
            std::slice::from_ref(&definition),
            std::slice::from_ref(&transform),
            822,
        )
        .map(|definition| definition.identity.id()),
        Some(822)
    );
    assert_eq!(
        unique_feature_profile_definition(std::slice::from_ref(&definition), &[], 822)
            .map(|definition| definition.identity.id()),
        Some(822)
    );
    assert!(
        unique_feature_profile_definition(&[definition.clone(), definition.clone()], &[], 822,)
            .is_none()
    );
    assert!(unique_feature_profile_definition(
        std::slice::from_ref(&definition),
        &[transform.clone(), transform.clone()],
        822,
    )
    .is_none());
    assert_eq!(
        unique_feature_profile_definition(
            std::slice::from_ref(&definition),
            std::slice::from_ref(&mismatched_transform),
            822,
        )
        .map(|definition| definition.identity.id()),
        Some(822)
    );

    let mismatched_section_transform = crate::placement::FeatureSectionTransform::new(
        900,
        Some(822),
        transform.origin(),
        transform.u_axis(),
        transform.v_axis(),
        transform.offset,
    )
    .expect("valid section frame");
    assert!(unique_feature_profile_definition(
        std::slice::from_ref(&definition),
        std::slice::from_ref(&mismatched_section_transform),
        822,
    )
    .is_none());

    let mut scan = crate::container::scan_bytes(Vec::new());
    scan.features.definitions.push(definition);
    let mut ir = CadIr::empty();
    for kind in ["Revolve", "Revolve 2"] {
        assert!(matches!(
            named_feature_definition(&scan, &ir, 822, kind),
            Some(IrFeatureDefinition::Revolve {
                ref construction,
                op: BooleanOp::Unresolved,
            }) if matches!(construction.profile().map(AsRef::as_ref), Some(ProfileRef::Native(profile))
                if profile == "creo:featdefs:sketch#822")
                && construction.axis().is_none()
                && construction.extent().is_none()
        ));
    }

    scan.features
        .revolution_extents
        .push(crate::feature::FeatureRevolutionExtent {
            feature_id: 822,
            offset: 1,
        });
    assert!(matches!(
        named_feature_definition(&scan, &ir, 822, "Revolve"),
        Some(IrFeatureDefinition::Revolve {
            ref construction,
            ..
        }) if matches!(construction.extent(), Some(cadmpeg_ir::features::RevolveExtent::OneSided {
                    termination: AngularTermination::Angle { angle: value },
                }) if (value.get() - std::f64::consts::TAU).abs() < EPS_FULL_TURN)
    ));

    let sketch = SketchId::mint("creo:model:sketch#822".to_string()).expect("valid test fixture");
    ir.model.sketches.push(Sketch {
        id: sketch.clone(),
        name: None,
        configuration: None,
        visible: None,
        placement: cadmpeg_ir::sketches::SketchPlacement::Unresolved,
        profiles: cadmpeg_ir::sketches::SketchProfiles::default(),
        native_ref: Some("creo:featdefs:sketch#822".to_string()),
    });
    assert!(matches!(
        filled_surface_feature_definition(&scan, &ir, 822),
        IrFeatureDefinition::FilledSurface {
            boundary: SurfaceBoundary::Path(PathRef::Sketch(boundary)),
            ..
        } if boundary == sketch
    ));

    scan.features
        .definitions
        .push(scan.features.definitions[0].clone());
    assert!(matches!(
        filled_surface_feature_definition(&scan, &ir, 822),
        IrFeatureDefinition::FilledSurface {
            boundary: SurfaceBoundary::Edges(EdgeSelection::Unresolved),
            ..
        }
    ));
}

#[test]
fn named_linear_sweep_reuses_materialized_cap_extent() {
    let entry = |entity_id, class_id, source_entity_id| crate::feature::FeatureEntityTableEntry {
        payload: crate::feature::entry_payload(class_id, source_entity_id, None, None),

        entity_id,
        prefixed: false,
        offset: 0,
        end_offset: 0,
    };
    let entries = vec![
        entry(31, 204, None),
        entry(32, 203, None),
        entry(33, 200, Some(11)),
    ];
    let mut scan = crate::container::scan_bytes(Vec::new());
    scan.features.entity_tables.push(
        crate::feature::FeatureEntityTable::new(
            7,
            29,
            entries,
            &std::collections::BTreeSet::new(),
            0,
        )
        .with_surface_ids([31, 32]),
    );
    let row = |id| crate::surface::SurfaceRow {
        id,
        kind: crate::surface::SurfaceKind::Plane,
        feature_id: 7,
        reversed: false,
        boundary_type: crate::surface::BoundaryType::Code00,
        next_surface: 0,
        offset: 0,
    };
    scan.surfaces.rows.extend([row(31), row(32)]);
    let plane = |id, z| Surface {
        id: SurfaceId::mint(format!("creo:visibgeom:surface#{id}")).expect("identity grammar"),
        geometry: SurfaceGeometry::Plane(
            cadmpeg_ir::geometry::PlaneSurface::try_new(
                Point3::new(0.0, 0.0, z),
                Vector3::new(0.0, 0.0, 1.0),
                Vector3::new(1.0, 0.0, 0.0),
            )
            .expect("valid PlaneSurface fixture"),
        ),
        source_object: None,
    };
    let mut ir = CadIr::empty();
    ir.model.surfaces.extend([plane(31, 2.0), plane(32, 8.0)]);

    let IrFeatureDefinition::Extrude {
        direction: ExtrudeDirection::Explicit {
            vector: direction, ..
        },
        extent:
            ExtrudeExtent::OneSided {
                side:
                    ExtrudeSide {
                        termination: LinearTermination::Blind { length },
                        ..
                    },
            },
        ..
    } = named_feature_definition(&scan, &ir, 7, "Protrusion").expect("named sweep")
    else {
        panic!("named sweep did not resolve the cap extent");
    };
    assert_eq!(direction, Vector3::new(0.0, 0.0, 1.0));
    assert_eq!(
        length,
        cadmpeg_ir::scalar::NonZeroLength::new(6.0).expect("nonzero length fixture")
    );
}

#[test]
fn boundary_surface_entity_graph_requires_the_complete_generated_chain() {
    let entry = |entity_id, class_id, source_entity_id| crate::feature::FeatureEntityTableEntry {
        payload: crate::feature::entry_payload(class_id, source_entity_id, None, None),

        entity_id,
        prefixed: true,
        offset: 0,
        end_offset: 0,
    };
    let table = |table_class_id, entries: Vec<crate::feature::FeatureEntityTableEntry>| {
        crate::feature::FeatureEntityTable::new(
            144,
            table_class_id,
            entries,
            &std::collections::BTreeSet::new(),
            0,
        )
        .with_surface_ids((table_class_id == 29).then_some(145))
    };
    let tables = vec![
        table(29, vec![entry(145, 200, Some(0))]),
        table(
            94,
            vec![
                entry(146, 221, None),
                entry(147, 222, None),
                entry(148, 220, None),
                entry(149, 220, None),
            ],
        ),
        table(67, vec![entry(150, 200, Some(144))]),
        table(100, vec![entry(150, 145, None)]),
    ];
    let surface = crate::surface::SurfaceRow {
        id: 145,
        kind: crate::surface::SurfaceKind::Extrusion(crate::surface::ExtrusionVariant::Linear),
        feature_id: 144,
        reversed: false,
        boundary_type: crate::surface::BoundaryType::Code00,
        next_surface: 0,
        offset: 0,
    };

    assert!(class_942_boundary_surface_entity_graph(
        144,
        &tables,
        std::slice::from_ref(&surface),
    ));

    let mut incomplete = tables.clone();
    incomplete[1].entries.pop();
    assert!(!class_942_boundary_surface_entity_graph(
        144,
        &incomplete,
        &[surface],
    ));
}

#[test]
fn new_sheet_output_requires_an_owned_output_surface() {
    let entry = |entity_id, class_id, source_entity_id| crate::feature::FeatureEntityTableEntry {
        payload: crate::feature::entry_payload(class_id, source_entity_id, None, None),

        entity_id,
        prefixed: true,
        offset: 0,
        end_offset: 0,
    };
    let table = |table_class_id, entries: Vec<crate::feature::FeatureEntityTableEntry>| {
        crate::feature::FeatureEntityTable::new(
            144,
            table_class_id,
            entries,
            &std::collections::BTreeSet::new(),
            0,
        )
        .with_surface_ids((table_class_id == 29).then_some(145))
    };
    let tables = vec![
        table(29, vec![entry(145, 200, Some(12))]),
        table(67, vec![entry(150, 200, Some(144))]),
        table(100, vec![entry(150, 145, None)]),
    ];
    let surface = crate::surface::SurfaceRow {
        id: 145,
        kind: crate::surface::SurfaceKind::Extrusion(crate::surface::ExtrusionVariant::Linear),
        feature_id: 144,
        reversed: false,
        boundary_type: crate::surface::BoundaryType::Code00,
        next_surface: 0,
        offset: 0,
    };

    assert_eq!(
        new_sheet_output_surface_id(144, &tables, std::slice::from_ref(&surface)),
        Some(145)
    );

    let mut prior_surface = surface;
    prior_surface.feature_id = 97;
    assert_eq!(
        new_sheet_output_surface_id(144, &tables, &[prior_surface]),
        None
    );
}

#[test]
fn stored_section_sweep_family_defines_boolean_operation() {
    use crate::feature::FeatureRecipeEffect::{Cut, Protrude};

    assert_eq!(
        section_sweep_boolean_operation(Some(Protrude), "Körper", false, true),
        BooleanOp::Join
    );
    assert_eq!(
        section_sweep_boolean_operation(Some(Cut), "Ausschnitt", false, false),
        BooleanOp::Cut
    );
    assert_eq!(
        section_sweep_boolean_operation(Some(Protrude), "Protrusion", true, false),
        BooleanOp::NewBody
    );
    assert_eq!(
        section_sweep_boolean_operation(Some(Protrude), "Protrusion", true, true),
        BooleanOp::Join
    );
    assert_eq!(
        section_sweep_boolean_operation(Some(Cut), "Cut", true, true),
        BooleanOp::Cut
    );
    assert_eq!(
        section_sweep_boolean_operation(Some(Protrude), "Körper", false, false),
        BooleanOp::NewBody
    );
    assert_eq!(
        section_sweep_boolean_operation(None, "Protrusion", false, false),
        BooleanOp::NewBody
    );
    assert_eq!(
        section_sweep_boolean_operation(None, "Protrusion", false, true),
        BooleanOp::Join
    );
    assert_eq!(
        section_sweep_boolean_operation(None, "Körper", false, true),
        BooleanOp::Unresolved
    );
    assert_eq!(
        section_sweep_boolean_operation(None, "Körper", true, false),
        BooleanOp::NewBody
    );
}

#[test]
fn datum_feature_uses_its_unique_transferred_plane_carrier() {
    let mut scan = crate::container::scan_bytes(Vec::new());
    scan.surfaces.rows.push(crate::surface::SurfaceRow {
        id: 6,
        kind: crate::surface::SurfaceKind::Plane,
        feature_id: 5,
        reversed: false,
        boundary_type: crate::surface::BoundaryType::Code01,
        next_surface: 0,
        offset: 0,
    });
    let mut ir = CadIr::empty();
    ir.model.surfaces.push(Surface {
        id: SurfaceId::mint("creo:visibgeom:surface#6".to_string()).expect("identity grammar"),
        geometry: SurfaceGeometry::Plane(
            cadmpeg_ir::geometry::PlaneSurface::try_new(
                Point3::new(0.0, 1.0, 0.0),
                Vector3::new(0.0, 1.0, 0.0),
                Vector3::new(0.0, 0.0, 1.0),
            )
            .expect("valid PlaneSurface fixture"),
        ),
        source_object: None,
    });

    assert_eq!(
        schema_feature_definition(&scan, &ir, 5, Some(SchemaClass::DatumPlane), "Datum Plane")
            .expect("valid test fixture"),
        IrFeatureDefinition::DatumPlane {
            frame: cadmpeg_ir::features::FeatureDatumPlaneFrame::new(
                Point3::new(0.0, 1.0, 0.0),
                Vector3::new(0.0, 1.0, 0.0),
                Vector3::new(0.0, 0.0, 1.0)
            )
            .expect("valid test fixture"),
        }
    );
    assert_eq!(
        schema_feature_definition(&scan, &ir, 5, None, "Native Feature")
            .expect("valid test fixture"),
        IrFeatureDefinition::DatumPlane {
            frame: cadmpeg_ir::features::FeatureDatumPlaneFrame::new(
                Point3::new(0.0, 1.0, 0.0),
                Vector3::new(0.0, 1.0, 0.0),
                Vector3::new(0.0, 0.0, 1.0)
            )
            .expect("valid test fixture"),
        }
    );

    scan.surfaces.rows.push(crate::surface::SurfaceRow {
        id: 7,
        kind: crate::surface::SurfaceKind::Plane,
        feature_id: 5,
        reversed: false,
        boundary_type: crate::surface::BoundaryType::Code01,
        next_surface: 0,
        offset: 1,
    });
    assert_eq!(
        schema_feature_definition(&scan, &ir, 5, Some(SchemaClass::DatumPlane), "Datum Plane")
            .expect("valid test fixture"),
        IrFeatureDefinition::Unresolved {
            family: UnresolvedFamily::DatumPlane
        }
    );
    assert!(matches!(
        schema_feature_definition(&scan, &ir, 5, None, "Native Feature")
            .expect("valid test fixture"),
        IrFeatureDefinition::Native { .. }
    ));
}

#[test]
fn datum_feature_preserves_its_unique_transferred_plane_chart() {
    let mut scan = crate::container::scan_bytes(Vec::new());
    scan.surfaces.rows.push(crate::surface::SurfaceRow {
        id: 6,
        kind: crate::surface::SurfaceKind::Plane,
        feature_id: 5,
        reversed: false,
        boundary_type: crate::surface::BoundaryType::Code01,
        next_surface: 0,
        offset: 0,
    });
    scan.planes.outlines.push(crate::surface::OutlinePlane {
        surface_id: 6,
        origin: [0.0, 1.0, 0.0],
        normal: [0.0, 1.0, 0.0],
        u_axis: [0.0, 0.0, 1.0],
        offset: 1,
    });

    assert_eq!(
        schema_feature_definition(
            &scan,
            &CadIr::empty(),
            5,
            Some(SchemaClass::DatumPlane),
            "Datum Plane",
        )
        .expect("valid test fixture"),
        IrFeatureDefinition::DatumPlane {
            frame: cadmpeg_ir::features::FeatureDatumPlaneFrame::new(
                Point3::new(0.0, 1.0, 0.0),
                Vector3::new(0.0, 1.0, 0.0),
                Vector3::new(0.0, 0.0, 1.0)
            )
            .expect("valid test fixture"),
        }
    );
}

#[test]
fn datum_feature_uses_its_unique_complete_local_system() {
    let mut scan = crate::container::scan_bytes(Vec::new());
    scan.features
        .definitions
        .push(crate::feature::FeatureDefinition {
            identity: crate::feature::definitions::DefinitionIdentity::Parsed {
                schema_id: std::num::NonZeroU32::new(5),
                owner_feature_id: Some(5),
            },
            body: Vec::new(),
            parameter_frames: vec![
                crate::feature::FeatureParameterFrame {
                    kind: crate::feature::FeatureParameterFrameKind::LocalSystem,
                    body: Vec::new(),
                    decoded_values: Some([
                        1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 2.0, 3.0, 4.0, 5.0,
                    ]),
                    offset: 1,
                },
                crate::feature::FeatureParameterFrame {
                    kind: crate::feature::FeatureParameterFrameKind::LocalSystem,
                    body: vec![0xff],
                    decoded_values: None,
                    offset: 2,
                },
            ],
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
        });

    assert_eq!(
        schema_feature_definition(
            &scan,
            &CadIr::empty(),
            5,
            Some(SchemaClass::DatumPlane),
            "Datum Plane"
        )
        .expect("valid test fixture"),
        IrFeatureDefinition::DatumPlane {
            frame: cadmpeg_ir::features::FeatureDatumPlaneFrame::new(
                Point3::new(3.0, 4.0, 5.0),
                Vector3::new(0.0, 0.0, 1.0),
                Vector3::new(1.0, 0.0, 0.0)
            )
            .expect("valid test fixture"),
        }
    );
}

#[test]
fn coordinate_system_feature_uses_its_unique_complete_local_system() {
    let mut scan = crate::container::scan_bytes(Vec::new());
    scan.features
        .definitions
        .push(crate::feature::FeatureDefinition {
            identity: crate::feature::definitions::DefinitionIdentity::Parsed {
                schema_id: std::num::NonZeroU32::new(7),
                owner_feature_id: Some(7),
            },
            body: Vec::new(),
            parameter_frames: vec![
                crate::feature::FeatureParameterFrame {
                    kind: crate::feature::FeatureParameterFrameKind::LocalSystem,
                    body: Vec::new(),
                    decoded_values: Some([
                        0.0, 2.0, 0.0, -3.0, 0.0, 0.0, 0.0, 0.0, 4.0, 5.0, 6.0, 7.0,
                    ]),
                    offset: 1,
                },
                crate::feature::FeatureParameterFrame {
                    kind: crate::feature::FeatureParameterFrameKind::LocalSystem,
                    body: vec![0xff],
                    decoded_values: None,
                    offset: 2,
                },
            ],
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
        });

    assert_eq!(
        schema_feature_definition(
            &scan,
            &CadIr::empty(),
            7,
            Some(SchemaClass::CoordinateSystem),
            "PRT_CSYS_DEF"
        )
        .expect("valid test fixture"),
        IrFeatureDefinition::DatumCoordinateSystem {
            frame: cadmpeg_ir::features::FeatureCoordinateFrame::new(
                Point3::new(5.0, 6.0, 7.0),
                Vector3::new(0.0, 1.0, 0.0),
                Vector3::new(-1.0, 0.0, 0.0),
                Vector3::new(0.0, 0.0, 1.0)
            )
            .expect("valid test fixture")
        }
    );
}

#[test]
fn coordinate_system_feature_rejects_a_reflected_local_system() {
    let mut scan = crate::container::scan_bytes(Vec::new());
    scan.features
        .definitions
        .push(crate::feature::FeatureDefinition {
            identity: crate::feature::definitions::DefinitionIdentity::Parsed {
                schema_id: std::num::NonZeroU32::new(7),
                owner_feature_id: Some(7),
            },
            body: Vec::new(),
            parameter_frames: vec![crate::feature::FeatureParameterFrame {
                kind: crate::feature::FeatureParameterFrameKind::LocalSystem,
                body: Vec::new(),
                decoded_values: Some([1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, -1.0, 5.0, 6.0, 7.0]),
                offset: 1,
            }],
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
        });

    assert_eq!(
        schema_feature_definition(
            &scan,
            &CadIr::empty(),
            7,
            Some(SchemaClass::CoordinateSystem),
            "PRT_CSYS_DEF"
        )
        .expect("valid test fixture"),
        IrFeatureDefinition::Unresolved {
            family: UnresolvedFamily::DatumCoordinateSystem
        }
    );
}

#[test]
fn only_body_evidence_or_a_new_body_sweep_establishes_prior_material() {
    let feature = |definition, outputs| Feature {
        id: IrFeatureId::mint("creo:model:feature#1".to_string()).expect("identity grammar"),
        ordinal: 0,
        name: None,
        suppressed: Some(false),
        dependencies: cadmpeg_ir::features::DistinctMembers::default(),
        source_properties: BTreeMap::new(),
        source_tag: None,
        source_text: None,
        source_content: cadmpeg_ir::features::FeatureContent::default(),

        evaluation: cadmpeg_ir::features::FeatureEvaluation::new(definition, outputs),
        native_ref: None,
    };
    let mut ir = CadIr::empty();
    ir.model.features.push(feature(
        IrFeatureDefinition::Chamfer {
            groups: cadmpeg_ir::features::NonEmptyMembers::one(
                cadmpeg_ir::features::ChamferGroup {
                    edges: EdgeSelection::Unresolved,
                    spec: ChamferSpec::Unresolved,
                },
            ),
            flip_direction: false,
        },
        Vec::new(),
    ));
    assert!(!preceding_features_establish_body(&ir));

    ir.model.features[0]
        .evaluation
        .set_outputs(vec![
            BodyId::mint("creo:model:body#1".to_string()).expect("identity grammar")
        ]);
    assert!(preceding_features_establish_body(&ir));

    ir.model.features[0] = feature(
        IrFeatureDefinition::Extrude {
            profile: ProfileRef::Native("creo:section#1".to_string()),
            direction: cadmpeg_ir::features::ExtrudeDirection::ProfileNormal {},
            extent: ExtrudeExtent::OneSided {
                side: ExtrudeSide {
                    termination: LinearTermination::Blind {
                        length: cadmpeg_ir::scalar::NonZeroLength::new(1.0)
                            .expect("nonzero length fixture"),
                    },
                    draft: None,
                },
            },
            op: BooleanOp::NewBody,
            start: cadmpeg_ir::features::ExtrudeStart::ProfilePlane {},
            solid: Some(true),
            face_maker: None,
            inner_wire_taper: None,
            length_along_profile_normal: None,
            allow_multi_profile_faces: None,
        },
        Vec::new(),
    );
    assert!(preceding_features_establish_body(&ir));
    ir.model.features[0].suppressed = Some(true);
    assert!(!preceding_features_establish_body(&ir));
    ir.model.features[0].suppressed = Some(false);
    ir.model.features[0].evaluation.edit(|definition, _| {
        let IrFeatureDefinition::Extrude { op, .. } = definition else {
            unreachable!();
        };
        *op = BooleanOp::Join;
    });
    assert!(!preceding_features_establish_body(&ir));
}

#[test]
fn current_feature_state_controls_recipe_and_parent_projection() {
    let operation = |recipe, parent_feature_id, offset| crate::feature::FeatureOperation {
        feature_id: 6,
        kind: crate::feature::OperationKind::Stored("Sweep".to_string()),
        name: crate::feature::operations::OperationName::Derived,
        recipe: crate::feature::RecipeResolution::Resolved(recipe),
        display_state_conflict: false,
        depdb: Some(crate::feature::operations::DepdbPrefix {
            schema: crate::feature::schema::SchemaClass::Protrusion,
            parent: parent_feature_id,
        }),
        offset,
        state_offset: offset,
    };
    let historical = operation(crate::feature::FeatureRecipe::ProtrudeExtrude, 4, 10);
    let current = operation(crate::feature::FeatureRecipe::ProtrudeRevolve, 5, 20);
    let states = [historical, current.clone()];
    assert_ne!(states[0].recipe.resolved(), states[1].recipe.resolved());
    assert_ne!(states[0].parent_feature_id(), states[1].parent_feature_id());
    assert_eq!(
        current_feature_recipe(std::slice::from_ref(&current), 6),
        Some(crate::feature::FeatureRecipe::ProtrudeRevolve)
    );
    assert_eq!(
        current_feature_recipe_parent(std::slice::from_ref(&current), 6),
        Some(5)
    );
    assert_eq!(
        current_additive_feature_recipe(std::slice::from_ref(&current), 6),
        Some(crate::feature::FeatureRecipeKind::Revolve)
    );
    let mut cut = current;
    cut.recipe =
        crate::feature::RecipeResolution::Resolved(crate::feature::FeatureRecipe::CutRevolve);
    assert_eq!(
        current_additive_feature_recipe(std::slice::from_ref(&cut), 6),
        None
    );
}
