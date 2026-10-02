// SPDX-License-Identifier: Apache-2.0

use std::collections::{BTreeMap};
use cadmpeg_ir::math::{Point2};
use cadmpeg_ir::sketches::{SketchConstraintDefinitionInput, SketchCoordinateAxis, SketchEntityId, SketchGeometry, SketchId, SketchLocus, SketchGeometryDefinition};
use cadmpeg_ir::scalar::{Angle, Length};
use super::super::{declared_solver_rows, section_skamp_constraints};
use super::fixtures::{base_definition};
use crate::decode::sketch_transfer::skamp_constraints::{section_skamp_constraints_for_geometry};

#[test]
fn section_solver_incidence_requires_available_geometry_and_active_semantics() {
    let definition = base_definition();
    let point_entity = crate::feature::definitions::FeatureSkampItem {
        entity_id: 14,
        sense: 0,
    };
    let mut point_pair = definition.clone();
    let point_pair_relations = point_pair.relations.as_mut().expect("relations");
    *declared_solver_rows(&mut point_pair_relations.skamps) = vec![crate::feature::definitions::FeatureSkamp {
        id: 19,
        kind: 3,
        flags: 0,
        status: 34,
        items: vec![
            crate::feature::definitions::FeatureSkampItem {
                entity_id: 14,
                sense: 0,
            },
            crate::feature::definitions::FeatureSkampItem {
                entity_id: 99,
                sense: 0,
            },
        ],
        offset: 84,
    }];
    point_pair_relations
        .skamps
        .as_mut()
        .and_then(|table| table.header_mut())
        .expect("skamp header")
        .declared_count = 1;
    let sketch = SketchId::mint("creo:model:sketch#917").expect("valid test fixture");
    let point_pair_geometry = BTreeMap::from([
        (
            SketchEntityId::mint("creo:featdefs:sketch_entity#917:14".to_string()).expect("valid test fixture"),
            SketchGeometry::native(cadmpeg_core::text::NonBlankString::new("point").expect("nonempty source identity")),
        ),
        (
            SketchEntityId::mint("creo:featdefs:sketch_entity#917:99".to_string()).expect("valid test fixture"),
            SketchGeometry::native(cadmpeg_core::text::NonBlankString::new("point").expect("nonempty source identity")),
        ),
    ]);
    assert_eq!(*(crate::decode::with_test_decode_ctx(|ctx| section_skamp_constraints_for_geometry(ctx, &point_pair, &sketch, Some(&point_pair_geometry))).expect("test section solve")[0]
            .0
            .definition).kind(),
        SketchConstraintDefinitionInput::CoincidentLoci {
            loci: vec![
                SketchLocus::Entity(SketchEntityId::mint(
                    "creo:featdefs:sketch_entity#917:14".to_string()
                ).expect("valid test fixture")),
                SketchLocus::Entity(SketchEntityId::mint(
                    "creo:featdefs:sketch_entity#917:99".to_string()
                ).expect("valid test fixture")),
            ],
        }
    );
    let missing_point_geometry = BTreeMap::from([(
        SketchEntityId::mint("creo:featdefs:sketch_entity#917:14".to_string()).expect("valid test fixture"),
        SketchGeometry::native(cadmpeg_core::text::NonBlankString::new("point").expect("nonempty source identity")),
    )]);
    assert!(matches!(
        crate::decode::with_test_decode_ctx(|ctx| section_skamp_constraints_for_geometry(ctx, &point_pair, &sketch, Some(&missing_point_geometry))).expect("test section solve")
            [0]
        .0
        .definition.kind(),
        SketchConstraintDefinitionInput::Native { .. }
    ));
    let mut point_coincidence_definition = definition.clone();
    let point_coincidence_relations = point_coincidence_definition
        .relations
        .as_mut()
        .expect("relations");
    *declared_solver_rows(&mut point_coincidence_relations.skamps) = vec![crate::feature::definitions::FeatureSkamp {
        id: 0,
        kind: 0,
        flags: 0,
        status: 1,
        items: vec![
            point_entity.clone(),
            crate::feature::definitions::FeatureSkampItem {
                entity_id: 13,
                sense: 4,
            },
        ],
        offset: 83,
    }];
    point_coincidence_relations
        .skamps
        .as_mut()
        .and_then(|table| table.header_mut())
        .expect("skamp header")
        .declared_count = 1;
    assert_eq!(*(section_skamp_constraints(
            &point_coincidence_definition,
            &SketchId::mint("creo:model:sketch#917").expect("valid test fixture")
        )[0]
        .0
        .definition).kind(),
        SketchConstraintDefinitionInput::CoincidentLoci {
            loci: vec![
                SketchLocus::Entity(SketchEntityId::mint(
                    "creo:featdefs:sketch_entity#917:14".to_string()
                ).expect("valid test fixture")),
                SketchLocus::Center(SketchEntityId::mint(
                    "creo:featdefs:sketch_entity#917:13".to_string()
                ).expect("valid test fixture")),
            ],
        }
    );
    let point_coincidence_relations = point_coincidence_definition
        .relations
        .as_mut()
        .expect("relations");
    point_coincidence_relations.skamps.as_mut().expect("skamp table").rows_mut()[0].kind = 3;
    point_coincidence_relations.skamps.as_mut().expect("skamp table").rows_mut()[0].items = vec![
        crate::feature::definitions::FeatureSkampItem {
            entity_id: 12,
            sense: 0,
        },
        point_entity,
    ];
    assert_eq!(*(section_skamp_constraints(
            &point_coincidence_definition,
            &SketchId::mint("creo:model:sketch#917").expect("valid test fixture")
        )[0]
        .0
        .definition).kind(),
        SketchConstraintDefinitionInput::PointOnObject {
            point: SketchLocus::Entity(SketchEntityId::mint(
                "creo:featdefs:sketch_entity#917:14".to_string()
            ).expect("valid test fixture")),
            entity: SketchEntityId::mint("creo:featdefs:sketch_entity#917:12".to_string()).expect("valid test fixture"),
        }
    );
    let point_coincidence_relations = point_coincidence_definition
        .relations
        .as_mut()
        .expect("relations");
    point_coincidence_relations.skamps.as_mut().expect("skamp table").rows_mut()[0].items = vec![
        crate::feature::definitions::FeatureSkampItem {
            entity_id: 13,
            sense: 0,
        },
        crate::feature::definitions::FeatureSkampItem {
            entity_id: 14,
            sense: 0,
        },
    ];
    assert_eq!(*(section_skamp_constraints(
            &point_coincidence_definition,
            &SketchId::mint("creo:model:sketch#917").expect("valid test fixture")
        )[0]
        .0
        .definition).kind(),
        SketchConstraintDefinitionInput::PointOnObject {
            point: SketchLocus::Entity(SketchEntityId::mint(
                "creo:featdefs:sketch_entity#917:14".to_string()
            ).expect("valid test fixture")),
            entity: SketchEntityId::mint("creo:featdefs:sketch_entity#917:13".to_string()).expect("valid test fixture"),
        }
    );
    let native_endpoint = SketchEntityId::mint("creo:featdefs:sketch_entity#917:99".to_string()).expect("valid test fixture");
    let point_coincidence_relations = point_coincidence_definition
        .relations
        .as_mut()
        .expect("relations");
    point_coincidence_relations.skamps.as_mut().expect("skamp table").rows_mut()[0].kind = 0;
    point_coincidence_relations.skamps.as_mut().expect("skamp table").rows_mut()[0].items = vec![
        crate::feature::definitions::FeatureSkampItem {
            entity_id: 12,
            sense: 3,
        },
        crate::feature::definitions::FeatureSkampItem {
            entity_id: 99,
            sense: 2,
        },
    ];
    let incidence_geometry = BTreeMap::from([
        (
            SketchEntityId::mint("creo:featdefs:sketch_entity#917:12".to_string()).expect("valid test fixture"),
            SketchGeometry::try_from(SketchGeometryDefinition::Line {
                start: Point2::new(0.0, 0.0),
                end: Point2::new(1.0, 0.0),
            }).expect("valid test fixture"),
        ),
        (
            native_endpoint.clone(),
            SketchGeometry::native(cadmpeg_core::text::NonBlankString::new("solver_only_section_entity").expect("nonempty source identity")),
        ),
        (
            SketchEntityId::mint("creo:featdefs:sketch_entity#917:13".to_string()).expect("valid test fixture"),
            SketchGeometry::try_from(SketchGeometryDefinition::Arc {
                center: Point2::new(0.0, 0.0),
                radius: Length::new(1.0).expect("finite length fixture"),
                start_angle: Angle::new(0.0).expect("finite angle fixture"),
                end_angle: Angle::new(std::f64::consts::FRAC_PI_2).expect("finite angle fixture"),
            }).expect("valid test fixture"),
        ),
    ]);
    assert!(matches!(
        crate::decode::with_test_decode_ctx(|ctx| section_skamp_constraints_for_geometry(ctx,
            &point_coincidence_definition,
            &SketchId::mint("creo:model:sketch#917").expect("valid test fixture"),
            Some(&incidence_geometry),
        )).expect("test section solve")[0]
        .0
        .definition.kind(),
        SketchConstraintDefinitionInput::Native { .. }
    ));
    let point_coincidence_relations = point_coincidence_definition
        .relations
        .as_mut()
        .expect("relations");
    point_coincidence_relations.skamps.as_mut().expect("skamp table").rows_mut()[0].kind = 3;
    point_coincidence_relations.skamps.as_mut().expect("skamp table").rows_mut()[0].items[0] = crate::feature::definitions::FeatureSkampItem {
        entity_id: 13,
        sense: 0,
    };
    assert!(matches!(
        crate::decode::with_test_decode_ctx(|ctx| section_skamp_constraints_for_geometry(ctx,
            &point_coincidence_definition,
            &SketchId::mint("creo:model:sketch#917").expect("valid test fixture"),
            Some(&incidence_geometry),
        )).expect("test section solve")[0]
        .0
        .definition.kind(),
        SketchConstraintDefinitionInput::Native { .. }
    ));
    point_coincidence_definition
        .relations
        .as_mut()
        .expect("relations")
        .skamps.as_mut().expect("skamp table").rows_mut()[0]
        .items[1]
        .sense = 4;
    assert!(matches!(
        crate::decode::with_test_decode_ctx(|ctx| section_skamp_constraints_for_geometry(ctx,
            &point_coincidence_definition,
            &SketchId::mint("creo:model:sketch#917").expect("valid test fixture"),
            Some(&incidence_geometry),
        )).expect("test section solve")[0]
        .0
        .definition.kind(),
        SketchConstraintDefinitionInput::Native { .. }
    ));
    let point_coincidence_relations = point_coincidence_definition
        .relations
        .as_mut()
        .expect("relations");
    point_coincidence_relations.skamps.as_mut().expect("skamp table").rows_mut()[0].kind = 17;
    point_coincidence_relations.skamps.as_mut().expect("skamp table").rows_mut()[0].flags = 1;
    point_coincidence_relations.skamps.as_mut().expect("skamp table").rows_mut()[0].status = 0;
    point_coincidence_relations.skamps.as_mut().expect("skamp table").rows_mut()[0].items = vec![
        crate::feature::definitions::FeatureSkampItem {
            entity_id: 13,
            sense: 2,
        },
        crate::feature::definitions::FeatureSkampItem {
            entity_id: 13,
            sense: 4,
        },
    ];
    let mut unresolved_arc_geometry = incidence_geometry.clone();
    unresolved_arc_geometry.insert(
        SketchEntityId::mint("creo:featdefs:sketch_entity#917:13".to_string()).expect("valid test fixture"),
        SketchGeometry::native(cadmpeg_core::text::NonBlankString::new("arc").expect("nonempty source identity")),
    );
    let inactive = crate::decode::with_test_decode_ctx(|ctx| section_skamp_constraints_for_geometry(ctx,
        &point_coincidence_definition,
        &SketchId::mint("creo:model:sketch#917").expect("valid test fixture"),
        Some(&unresolved_arc_geometry),
    )).expect("test section solve");
    assert_eq!(*(inactive[0].0.definition).kind(),
        SketchConstraintDefinitionInput::SameCoordinate { relation: cadmpeg_ir::sketches::SketchSameCoordinate::try_new(SketchLocus::End(SketchEntityId::mint(
                "creo:featdefs:sketch_entity#917:13".to_string()
            ).expect("valid test fixture")),SketchLocus::Center(SketchEntityId::mint(
                "creo:featdefs:sketch_entity#917:13".to_string()
            ).expect("valid test fixture")),SketchCoordinateAxis::U).expect("valid test fixture") }
    );
    assert_eq!(inactive[0].0.active, Some(false));
    point_coincidence_definition
        .relations
        .as_mut()
        .expect("relations")
        .skamps.as_mut().expect("skamp table").rows_mut()[0]
        .items = vec![
        crate::feature::definitions::FeatureSkampItem {
            entity_id: 99,
            sense: 3,
        },
        crate::feature::definitions::FeatureSkampItem {
            entity_id: 13,
            sense: 3,
        },
    ];
    unresolved_arc_geometry.insert(
        SketchEntityId::mint("creo:featdefs:sketch_entity#917:99".to_string()).expect("valid test fixture"),
        SketchGeometry::native(cadmpeg_core::text::NonBlankString::new("solver_only_section_entity").expect("nonempty source identity")),
    );
    assert_eq!(*(crate::decode::with_test_decode_ctx(|ctx| section_skamp_constraints_for_geometry(ctx,
            &point_coincidence_definition,
            &SketchId::mint("creo:model:sketch#917").expect("valid test fixture"),
            Some(&unresolved_arc_geometry),
        )).expect("test section solve")[0]
        .0
        .definition).kind(),
        SketchConstraintDefinitionInput::SameCoordinate { relation: cadmpeg_ir::sketches::SketchSameCoordinate::try_new(SketchLocus::End(SketchEntityId::mint(
                "creo:featdefs:sketch_entity#917:99".to_string()
            ).expect("valid test fixture")),SketchLocus::Start(SketchEntityId::mint(
                "creo:featdefs:sketch_entity#917:13".to_string()
            ).expect("valid test fixture")),SketchCoordinateAxis::U).expect("valid test fixture") }
    );
    point_coincidence_definition
        .relations
        .as_mut()
        .expect("relations")
        .skamps.as_mut().expect("skamp table").rows_mut()[0]
        .items = vec![
        crate::feature::definitions::FeatureSkampItem {
            entity_id: 13,
            sense: 2,
        },
        crate::feature::definitions::FeatureSkampItem {
            entity_id: 13,
            sense: 4,
        },
    ];
    point_coincidence_definition
        .relations
        .as_mut()
        .expect("relations")
        .skamps.as_mut().expect("skamp table").rows_mut()[0]
        .kind = 15;
    let inactive_type_fifteen = crate::decode::with_test_decode_ctx(|ctx| section_skamp_constraints_for_geometry(ctx,
        &point_coincidence_definition,
        &SketchId::mint("creo:model:sketch#917").expect("valid test fixture"),
        Some(&unresolved_arc_geometry),
    )).expect("test section solve");
    assert_eq!(
        inactive_type_fifteen[0].0.definition,
        inactive[0].0.definition
    );
    point_coincidence_definition
        .relations
        .as_mut()
        .expect("relations")
        .skamps.as_mut().expect("skamp table").rows_mut()[0]
        .flags = 3;
    assert!(matches!(
        crate::decode::with_test_decode_ctx(|ctx| section_skamp_constraints_for_geometry(ctx,
            &point_coincidence_definition,
            &SketchId::mint("creo:model:sketch#917").expect("valid test fixture"),
            Some(&unresolved_arc_geometry),
        )).expect("test section solve")[0]
        .0
        .definition.kind(),
        SketchConstraintDefinitionInput::Native { .. }
    ));
    let type_fifteen = &mut point_coincidence_definition
        .relations
        .as_mut()
        .expect("relations")
        .skamps.as_mut().expect("skamp table").rows_mut()[0];
    type_fifteen.kind = 17;
    type_fifteen.flags = 1;
    let mut inactive_tangent_definition = point_coincidence_definition.clone();
    let inactive_tangent = &mut inactive_tangent_definition
        .relations
        .as_mut()
        .expect("relations")
        .skamps.as_mut().expect("skamp table").rows_mut()[0];
    inactive_tangent.kind = 4;
    inactive_tangent.items = vec![
        crate::feature::definitions::FeatureSkampItem {
            entity_id: 13,
            sense: 2,
        },
        crate::feature::definitions::FeatureSkampItem {
            entity_id: 99,
            sense: 2,
        },
    ];
    assert_eq!(*(crate::decode::with_test_decode_ctx(|ctx| section_skamp_constraints_for_geometry(ctx,
            &inactive_tangent_definition,
            &SketchId::mint("creo:model:sketch#917").expect("valid test fixture"),
            Some(&unresolved_arc_geometry),
        )).expect("test section solve")[0]
        .0
        .definition).kind(),
        SketchConstraintDefinitionInput::TangentLoci {
            first: SketchLocus::End(SketchEntityId::mint(
                "creo:featdefs:sketch_entity#917:13".to_string()
            ).expect("valid test fixture")),
            second: SketchLocus::Start(SketchEntityId::mint(
                "creo:featdefs:sketch_entity#917:99".to_string()
            ).expect("valid test fixture")),
        }
    );
    inactive_tangent_definition
        .relations
        .as_mut()
        .expect("relations")
        .skamps.as_mut().expect("skamp table").rows_mut()[0]
        .status = 1;
    assert!(matches!(
        crate::decode::with_test_decode_ctx(|ctx| section_skamp_constraints_for_geometry(ctx,
            &inactive_tangent_definition,
            &SketchId::mint("creo:model:sketch#917").expect("valid test fixture"),
            Some(&unresolved_arc_geometry),
        )).expect("test section solve")[0]
        .0
        .definition.kind(),
        SketchConstraintDefinitionInput::Native { .. }
    ));
    let mut inactive_point_on_curve_definition = point_coincidence_definition.clone();
    let inactive_point_on_curve = &mut inactive_point_on_curve_definition
        .relations
        .as_mut()
        .expect("relations")
        .skamps.as_mut().expect("skamp table").rows_mut()[0];
    inactive_point_on_curve.kind = 3;
    inactive_point_on_curve.items = vec![
        crate::feature::definitions::FeatureSkampItem {
            entity_id: 13,
            sense: 0,
        },
        crate::feature::definitions::FeatureSkampItem {
            entity_id: 99,
            sense: 4,
        },
    ];
    let unresolved_curve_geometry = BTreeMap::from([
        (
            SketchEntityId::mint("creo:featdefs:sketch_entity#917:13".to_string()).expect("valid test fixture"),
            SketchGeometry::native(cadmpeg_core::text::NonBlankString::new("line").expect("nonempty source identity")),
        ),
        (
            SketchEntityId::mint("creo:featdefs:sketch_entity#917:99".to_string()).expect("valid test fixture"),
            SketchGeometry::native(cadmpeg_core::text::NonBlankString::new("circle").expect("nonempty source identity")),
        ),
    ]);
    assert_eq!(*(crate::decode::with_test_decode_ctx(|ctx| section_skamp_constraints_for_geometry(ctx,
            &inactive_point_on_curve_definition,
            &SketchId::mint("creo:model:sketch#917").expect("valid test fixture"),
            Some(&unresolved_curve_geometry),
        )).expect("test section solve")[0]
        .0
        .definition).kind(),
        SketchConstraintDefinitionInput::PointOnObject {
            point: SketchLocus::Center(SketchEntityId::mint(
                "creo:featdefs:sketch_entity#917:99".to_string()
            ).expect("valid test fixture")),
            entity: SketchEntityId::mint("creo:featdefs:sketch_entity#917:13".to_string()).expect("valid test fixture"),
        }
    );
    inactive_point_on_curve_definition
        .relations
        .as_mut()
        .expect("relations")
        .skamps.as_mut().expect("skamp table").rows_mut()[0]
        .status = 1;
    assert!(matches!(
        crate::decode::with_test_decode_ctx(|ctx| section_skamp_constraints_for_geometry(ctx,
            &inactive_point_on_curve_definition,
            &SketchId::mint("creo:model:sketch#917").expect("valid test fixture"),
            Some(&unresolved_curve_geometry),
        )).expect("test section solve")[0]
        .0
        .definition.kind(),
        SketchConstraintDefinitionInput::PointOnObject {
            point: SketchLocus::Center(_),
            ..
        }
    ));
    let mut inactive_point_symmetry_definition = point_coincidence_definition.clone();
    let inactive_point_symmetry = &mut inactive_point_symmetry_definition
        .relations
        .as_mut()
        .expect("relations")
        .skamps.as_mut().expect("skamp table").rows_mut()[0];
    inactive_point_symmetry.kind = 14;
    inactive_point_symmetry.items = vec![
        crate::feature::definitions::FeatureSkampItem {
            entity_id: 97,
            sense: 0,
        },
        crate::feature::definitions::FeatureSkampItem {
            entity_id: 98,
            sense: 4,
        },
        crate::feature::definitions::FeatureSkampItem {
            entity_id: 99,
            sense: 4,
        },
    ];
    let unresolved_point_symmetry_geometry = BTreeMap::from([
        (
            SketchEntityId::mint("creo:featdefs:sketch_entity#917:97".to_string()).expect("valid test fixture"),
            SketchGeometry::native(cadmpeg_core::text::NonBlankString::new("point").expect("nonempty source identity")),
        ),
        (
            SketchEntityId::mint("creo:featdefs:sketch_entity#917:98".to_string()).expect("valid test fixture"),
            SketchGeometry::native(cadmpeg_core::text::NonBlankString::new("circle").expect("nonempty source identity")),
        ),
        (
            SketchEntityId::mint("creo:featdefs:sketch_entity#917:99".to_string()).expect("valid test fixture"),
            SketchGeometry::native(cadmpeg_core::text::NonBlankString::new("circle").expect("nonempty source identity")),
        ),
    ]);
    assert_eq!(*(crate::decode::with_test_decode_ctx(|ctx| section_skamp_constraints_for_geometry(ctx,
            &inactive_point_symmetry_definition,
            &SketchId::mint("creo:model:sketch#917").expect("valid test fixture"),
            Some(&unresolved_point_symmetry_geometry),
        )).expect("test section solve")[0]
        .0
        .definition).kind(),
        SketchConstraintDefinitionInput::PointSymmetric {
            first: SketchLocus::Center(SketchEntityId::mint(
                "creo:featdefs:sketch_entity#917:98".to_string()
            ).expect("valid test fixture")),
            second: SketchLocus::Center(SketchEntityId::mint(
                "creo:featdefs:sketch_entity#917:99".to_string()
            ).expect("valid test fixture")),
            center: SketchLocus::Entity(SketchEntityId::mint(
                "creo:featdefs:sketch_entity#917:97".to_string()
            ).expect("valid test fixture")),
        }
    );
    inactive_point_symmetry_definition
        .relations
        .as_mut()
        .expect("relations")
        .skamps.as_mut().expect("skamp table").rows_mut()[0]
        .status = 1;
    assert!(matches!(
        crate::decode::with_test_decode_ctx(|ctx| section_skamp_constraints_for_geometry(ctx,
            &inactive_point_symmetry_definition,
            &SketchId::mint("creo:model:sketch#917").expect("valid test fixture"),
            Some(&unresolved_point_symmetry_geometry),
        )).expect("test section solve")[0]
        .0
        .definition.kind(),
        SketchConstraintDefinitionInput::Native { .. }
    ));
    point_coincidence_definition
        .relations
        .as_mut()
        .expect("relations")
        .skamps.as_mut().expect("skamp table").rows_mut()[0]
        .status = 1;
    let active_native_arc = crate::decode::with_test_decode_ctx(|ctx| section_skamp_constraints_for_geometry(ctx,
        &point_coincidence_definition,
        &SketchId::mint("creo:model:sketch#917").expect("valid test fixture"),
        Some(&unresolved_arc_geometry),
    )).expect("test section solve");
    assert!(matches!(
        active_native_arc[0].0.definition.kind(),
        SketchConstraintDefinitionInput::SameCoordinate { .. }
    ));
    let point_coincidence_relations = point_coincidence_definition
        .relations
        .as_mut()
        .expect("relations");
    point_coincidence_relations.skamps.as_mut().expect("skamp table").rows_mut()[0].kind = 3;
    point_coincidence_relations.skamps.as_mut().expect("skamp table").rows_mut()[0].flags = 0;
    point_coincidence_relations.skamps.as_mut().expect("skamp table").rows_mut()[0].items = vec![
        crate::feature::definitions::FeatureSkampItem {
            entity_id: 13,
            sense: 0,
        },
        crate::feature::definitions::FeatureSkampItem {
            entity_id: 99,
            sense: 2,
        },
    ];
    declared_solver_rows(&mut point_coincidence_relations.skamps).insert(
        0,
        crate::feature::definitions::FeatureSkamp {
            id: 1,
            kind: 1,
            flags: 0,
            status: 1,
            items: vec![crate::feature::definitions::FeatureSkampItem {
                entity_id: 99,
                sense: 0,
            }],
            offset: 84,
        },
    );
    point_coincidence_relations
        .skamps
        .as_mut()
        .and_then(|table| table.header_mut())
        .expect("skamp header")
        .declared_count = 2;
    point_coincidence_relations.skamps.as_mut().expect("skamp table").rows_mut()[1].status = 0;
    assert_eq!(*(crate::decode::with_test_decode_ctx(|ctx| section_skamp_constraints_for_geometry(ctx,
            &point_coincidence_definition,
            &SketchId::mint("creo:model:sketch#917").expect("valid test fixture"),
            Some(&incidence_geometry),
        )).expect("test section solve")[0]
        .0
        .definition).kind(),
        SketchConstraintDefinitionInput::Horizontal {
            entity: native_endpoint,
        }
    );
    let point_coincidence_relations = point_coincidence_definition
        .relations
        .as_mut()
        .expect("relations");
    point_coincidence_relations.skamps.as_mut().expect("skamp table").rows_mut()[1].kind = 35;
    point_coincidence_relations.skamps.as_mut().expect("skamp table").rows_mut()[1].items = vec![
        crate::feature::definitions::FeatureSkampItem {
            entity_id: 99,
            sense: 0,
        },
        crate::feature::definitions::FeatureSkampItem {
            entity_id: 12,
            sense: 2,
        },
    ];
    assert!(matches!(
        crate::decode::with_test_decode_ctx(|ctx| section_skamp_constraints_for_geometry(ctx,
            &point_coincidence_definition,
            &SketchId::mint("creo:model:sketch#917").expect("valid test fixture"),
            Some(&incidence_geometry),
        )).expect("test section solve")[0]
        .0
        .definition.kind(),
        SketchConstraintDefinitionInput::Horizontal { .. }
    ));
    assert!(matches!(
        crate::decode::with_test_decode_ctx(|ctx| section_skamp_constraints_for_geometry(ctx,
            &point_coincidence_definition,
            &SketchId::mint("creo:model:sketch#917").expect("valid test fixture"),
            Some(&incidence_geometry),
        )).expect("test section solve")[1]
        .0
        .definition.kind(),
        SketchConstraintDefinitionInput::Midpoint { .. }
    ));
    point_coincidence_definition
        .relations
        .as_mut()
        .expect("relations")
        .skamps.as_mut().expect("skamp table").rows_mut()[1]
        .items[1]
        .sense = 0;
    assert!(matches!(
        crate::decode::with_test_decode_ctx(|ctx| section_skamp_constraints_for_geometry(ctx,
            &point_coincidence_definition,
            &SketchId::mint("creo:model:sketch#917").expect("valid test fixture"),
            Some(&incidence_geometry),
        )).expect("test section solve")[0]
        .0
        .definition.kind(),
        SketchConstraintDefinitionInput::Horizontal { .. }
    ));
}

