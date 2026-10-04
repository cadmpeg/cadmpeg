// SPDX-License-Identifier: Apache-2.0
//! Feature-history caller tests.

use super::admitted;
use crate::decode::feature_history::draft::{
    feature_allows_linear_extrusion, feature_is_sheet_extrusion,
    numbered_feature_name_has_family, preceding_features_establish_body, schema_feature_definition,
};
use crate::decode::feature_history::named::reference_named_feature_definition;
use crate::decode::feature_history::outputs::{new_sheet_output_surface_id, sweep_output_kind};
use crate::feature::schema::SchemaClass;
use cadmpeg_ir::document::CadIr;
use cadmpeg_ir::features::{
    edge_treatments::ChamferSpec, BooleanOp, EdgeSelection, ExtrudeExtent, ExtrudeSide,
    FaceSelection, Feature, FeatureDefinition as IrFeatureDefinition, FeatureId as IrFeatureId,
    FeatureOperation as IrFeatureOperation, LinearTermination, PlanarProfileRef, ProfileRef,
    UnresolvedFamily,
};
use cadmpeg_ir::ids::BodyId;
use cadmpeg_ir::topology::BodyKind;
use std::collections::BTreeMap;

#[test]
fn unresolved_display_state_family_blocks_schema_sweep_fallback() {
    let mut scan = crate::test_support::empty_container_scan();
    scan.features
        .operations
        .push(crate::feature::operations::FeatureOperation {
            feature_id: 917,
            kind: crate::feature::operations::OperationKind::Native,
            name: crate::feature::operations::OperationName::Derived,
            recipe: crate::feature::operations::RecipeResolution::None,
            display_state_conflict: true,
            depdb: Some(crate::feature::operations::DepdbPrefix {
                schema: crate::feature::schema::SchemaClass::Protrusion,
                parent: 0,
            }),
            offset: 0,
            state_offset: 0,
        });

    assert!(!admitted(|ctx| feature_allows_linear_extrusion(ctx, &scan, 917)));
    scan.features.operations[0].kind = crate::feature::operations::OperationKind::Extrude;
    assert!(admitted(|ctx| feature_allows_linear_extrusion(ctx, &scan, 917)));
}

#[test]
fn class_942_linear_sweep_requires_a_numbered_extrude_reference() {
    let mut scan = crate::test_support::empty_container_scan();
    scan.features
        .operations
        .push(crate::feature::operations::FeatureOperation {
            feature_id: 942,
            kind: crate::feature::operations::OperationKind::Stored("Surface".to_string()),
            name: crate::feature::operations::OperationName::Stored {
                bytes: b"Surface id 942".to_vec(),
                keyword: crate::feature::operations::IdKeyword::Id,
                prefix: None,
            },
            recipe: crate::feature::operations::RecipeResolution::None,
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
        .push(crate::feature::operations::FeatureReferenceName {
            feature_id: 942,
            name_bytes: b"Extrude 1".to_vec(),
            own_reference_id: 1,
            reference_type: 0,
            offset: 0,
        });

    assert!(admitted(|ctx| feature_is_sheet_extrusion(ctx, &scan, 942)));
    assert!(admitted(|ctx| feature_allows_linear_extrusion(ctx, &scan, 942)));
    assert_eq!(
        admitted(|ctx| sweep_output_kind(ctx, &scan, &CadIr::empty(), "extrusion", 942)),
        Some(BodyKind::Sheet)
    );
    assert!(matches!(
        crate::decode::with_test_decode_ctx(|ctx| schema_feature_definition(
            ctx,
            &scan,
            &CadIr::empty(),
            &crate::decode::source_carriers::SourceUnitCarriers::default(),
            942,
            Some(SchemaClass::Surface),
            "Surface"
        ))
        .expect("valid test fixture"),
        IrFeatureDefinition::Operation(IrFeatureOperation::Extrude {
            profile: ProfileRef::Planar(PlanarProfileRef::Unresolved(_)),
            op: BooleanOp::NewBody,
            solid: Some(false),
            ..
        })
    ));

    scan.features.reference_names[0].name_bytes = b"Boundary Blend 1".to_vec();
    assert!(!admitted(|ctx| feature_is_sheet_extrusion(ctx, &scan, 942)));
    assert!(!admitted(|ctx| feature_allows_linear_extrusion(ctx, &scan, 942)));
    assert_eq!(
        admitted(|ctx| sweep_output_kind(ctx, &scan, &CadIr::empty(), "extrusion", 942)),
        None
    );
    assert!(matches!(
        crate::decode::with_test_decode_ctx(|ctx| schema_feature_definition(
            ctx,
            &scan,
            &CadIr::empty(),
            &crate::decode::source_carriers::SourceUnitCarriers::default(),
            942,
            Some(SchemaClass::Surface),
            "Surface"
        ))
        .expect("valid test fixture"),
        IrFeatureDefinition::Operation(IrFeatureOperation::Unresolved {
            family: UnresolvedFamily::BoundarySurface
        })
    ));
}

#[test]
fn numbered_reference_name_selects_only_its_exact_feature_family() {
    assert!(admitted(|ctx| numbered_feature_name_has_family(ctx, "Thicken 1", "Thicken")));
    assert!(admitted(|ctx| numbered_feature_name_has_family(ctx, "Thicken 12", "Thicken")));
    assert!(!admitted(|ctx| numbered_feature_name_has_family(ctx, "Thicken", "Thicken")));
    assert!(!admitted(|ctx| numbered_feature_name_has_family(ctx, "Thicken A", "Thicken")));
    assert!(!admitted(|ctx| numbered_feature_name_has_family(ctx, "GThicken 1", "Thicken")));
    assert!(matches!(
        admitted(|ctx| reference_named_feature_definition(ctx, "Boundary Blend 1")),
        Some(IrFeatureDefinition::Operation(
            IrFeatureOperation::Unresolved {
                family: UnresolvedFamily::BoundarySurface
            }
        ))
    ));
    assert!(matches!(
        admitted(|ctx| reference_named_feature_definition(ctx, "Thicken 1")),
        Some(IrFeatureDefinition::Operation(
            IrFeatureOperation::Thicken {
                faces: FaceSelection::Unresolved,
                thickness: None,
                side: None,
            }
        ))
    ));
    assert!(admitted(|ctx| reference_named_feature_definition(ctx, "Fill 1")).is_none());
    assert!(matches!(
        admitted(|ctx| reference_named_feature_definition(ctx, "Merge 2")),
        Some(IrFeatureDefinition::Operation(
            IrFeatureOperation::KnitSurface {
                faces: FaceSelection::Unresolved,
                merge_entities: Some(true),
                create_solid: Some(false),
                gap_tolerance: None,
            }
        ))
    ));
    assert!(admitted(|ctx| reference_named_feature_definition(ctx, "Extrude 2")).is_none());
}

#[test]
fn new_sheet_output_requires_an_owned_output_surface() {
    let entry =
        |entity_id, class_id, source_entity_id| crate::feature::entity::FeatureEntityTableEntry {
            payload: crate::feature::entity::entry_payload(class_id, source_entity_id, None, None),

            entity_id,
            prefixed: true,
            offset: 0,
            end_offset: 0,
        };
    let table = |table_class_id, entries: Vec<crate::feature::entity::FeatureEntityTableEntry>| {
        crate::feature::entity::FeatureEntityTable::new(
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
        admitted(|ctx| new_sheet_output_surface_id(ctx, 144, &tables, std::slice::from_ref(&surface))),
        Some(145)
    );

    let mut prior_surface = surface;
    prior_surface.feature_id = 97;
    assert_eq!(
        admitted(|ctx| new_sheet_output_surface_id(ctx, 144, &tables, &[prior_surface])),
        None
    );
}

#[test]
fn only_body_evidence_or_a_new_body_sweep_establishes_prior_material() {
    let feature = |definition, outputs: Vec<cadmpeg_ir::ids::BodyId>| Feature {
        id: IrFeatureId::mint("creo:model:feature#1".to_string()).expect("identity grammar"),
        ordinal: 0,
        name: None,
        suppressed: Some(false),
        dependencies: cadmpeg_ir::features::DistinctMembers::default(),
        source_properties: BTreeMap::new(),
        source_tag: None,
        source_text: None,
        source_content: cadmpeg_ir::features::FeatureContent::default(),

        evaluation: cadmpeg_ir::features::FeatureEvaluation::new(
            definition,
            cadmpeg_ir::features::DistinctMembers::try_from(
                outputs,
                &cadmpeg_test_support::service_decode_context(),
            )
            .expect("distinct output fixture"),
        ),
        native_ref: None,
    };
    let mut ir = CadIr::empty();
    ir.model.features.push(feature(
        IrFeatureDefinition::Operation(IrFeatureOperation::Chamfer {
            groups: cadmpeg_ir::features::NonEmptyMembers::one(
                cadmpeg_ir::features::edge_treatments::ChamferGroup {
                    edges: EdgeSelection::Unresolved,
                    spec: ChamferSpec::Unresolved { form: None },
                },
            ),
            flip_direction: false,
        }),
        Vec::new(),
    ));
    assert!(!admitted(|ctx| preceding_features_establish_body(ctx, &ir)));

    ir.model.features[0].evaluation.set_outputs(
        cadmpeg_ir::features::DistinctMembers::try_from(
            vec![BodyId::mint("creo:model:body#1".to_string()).expect("identity grammar")],
            &cadmpeg_test_support::service_decode_context(),
        )
        .expect("distinct output fixture"),
    );
    assert!(admitted(|ctx| preceding_features_establish_body(ctx, &ir)));

    ir.model.features[0] = feature(
        IrFeatureDefinition::Operation(IrFeatureOperation::Extrude {
            profile: ProfileRef::Planar(PlanarProfileRef::Native("creo:section#1".to_string())),
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
        }),
        Vec::new(),
    );
    assert!(admitted(|ctx| preceding_features_establish_body(ctx, &ir)));
    ir.model.features[0].suppressed = Some(true);
    assert!(!admitted(|ctx| preceding_features_establish_body(ctx, &ir)));
    ir.model.features[0].suppressed = Some(false);
    ir.model.features[0].evaluation.edit(|definition, _| {
        let IrFeatureDefinition::Operation(IrFeatureOperation::Extrude { op, .. }) = definition
        else {
            unreachable!();
        };
        *op = BooleanOp::Join;
    });
    assert!(!admitted(|ctx| preceding_features_establish_body(ctx, &ir)));
}
