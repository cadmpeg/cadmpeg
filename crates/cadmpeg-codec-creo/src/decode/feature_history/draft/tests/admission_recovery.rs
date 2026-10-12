// SPDX-License-Identifier: Apache-2.0

use super::super::{admitted_hole_placements, class_942_boundary_surface_entity_graph, numbered_feature_name_has_family, thicken_feature_definition};
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
use cadmpeg_core::CodecError;

#[test]
fn empty_hole_placements_keep_zero_work() {
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = 0;
    policy.limits.max_retained_bytes = 0;
    policy.limits.max_materialized_bytes = 0;
    policy.limits.max_collection_items = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("root");
    assert!(admitted_hole_placements(&ctx, [None, None, None])
        .expect("absent placements need no work or storage")
        .is_empty());
}

#[test]
fn numbered_feature_fast_recovery_keeps_zero_work() {
    for name in ["", "Thicken 3", "Extrude", "Extrude "] {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_work_units = 0;
        policy.limits.max_retained_bytes = 0;
        policy.limits.max_materialized_bytes = 0;
        policy.limits.max_collection_items = 0;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("root");
        assert!(!numbered_feature_name_has_family(&ctx, name, "Extrude")
            .expect("fixed prefix and absent ordinal are free"));
        }
}

#[test]
fn boundary_surface_table_recovery_visits_only_present_prefix_rows() {
    use crate::feature::entity::FeatureEntityTable;
    let surface = crate::surface::SurfaceRow {
        id: 145,
        kind: crate::surface::SurfaceKind::Extrusion(crate::surface::ExtrusionVariant::Linear),
        feature_id: 144,
        reversed: false,
        boundary_type: crate::surface::BoundaryType::Code00,
        next_surface: 0,
        offset: 0,
    };
    let table = |feature_id, class| {
        FeatureEntityTable::new(
            feature_id,
            class,
            Vec::new(),
            &std::collections::BTreeSet::new(),
            0,
        )
    };
    let mut duplicate_prefix = vec![table(144, 29), table(144, 29)];
    for _ in 0..512 {
        duplicate_prefix.push(table(99, 29));
    }
    // One surface row precedes the table scan. The duplicate slot stops after
    // its two present rows; no trailing row or exhausted iterator is visited.
    for (tables, table_visits) in [
        (Vec::new(), 0),
        (vec![table(99, 29)], 1),
        (duplicate_prefix, 2),
    ] {
        let need = 1 + table_visits;
        crate::test_support::assert_refusal_order(
            ResourceDimension::WorkUnits,
            &if table_visits == 0 {
                vec!["creo boundary surface generated rows"]
            } else {
                vec![
                    "creo boundary surface generated rows",
                    "creo boundary surface entity tables",
                ]
            },
            |cap| {
                let arena = DecodeArena::new();
                let mut policy = DecodePolicy::service();
                policy.limits.max_work_units = cap;
                policy.limits.max_retained_bytes = 0;
                policy.limits.max_materialized_bytes = 0;
                policy.limits.max_collection_items = 0;
                let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("root");
                let run = || {
                    class_942_boundary_surface_entity_graph(
                        &ctx,
                        144,
                        &tables,
                        std::slice::from_ref(&surface),
                    )
                };
                let result = run();
                if cap == need {
                    assert!(
                        !result.expect("exact present-row work keeps incomplete graph recovery")
                    );
                    let original = ctx
                        .charge_work_limit(1, "after boundary surface recovery")
                        .expect_err("all admitted row work consumed");
                    assert_eq!(
                        (original.dimension, original.used, original.additional),
                        (ResourceDimension::WorkUnits, need, 1)
                    );
                } else {
                    let original = ctx.resource_refusal().expect("present row work refusal");
                    assert!(
                        matches!(result, Err(CodecError::ResourceLimit(actual)) if actual == original)
                    );
                    let operation = if cap == 0 {
                        "creo boundary surface generated rows"
                    } else {
                        "creo boundary surface entity tables"
                    };
                    assert_eq!(
                        (
                            original.dimension,
                            original.used,
                            original.additional,
                            original.operation
                        ),
                        (ResourceDimension::WorkUnits, cap, 1, operation)
                    );
                }

                if cap < need {
                    Err(ctx
                        .resource_refusal()
                        .expect("present visit refusal")
                        .into())
                } else {
                    Ok::<_, CodecError>(())
                }
            },
        );
    }
}

#[test]
fn resolved_thicken_source_has_one_present_row_refusal_boundary() {
    let scan = super::thicken_scan();
    let mut ir = cadmpeg_ir::document::CadIr::empty();
    ir.model.faces.push(super::resolved_hole_face());
    let source_refusals = std::cell::Cell::new(0);
    let definition =
        crate::test_support::assert_work_boundaries(&["creo thicken model face lookup"], |ctx| {
            let result = thicken_feature_definition(ctx, &scan, &ir, 17);
            if ctx
                .resource_refusal()
                .is_some_and(|refusal| refusal.operation == "creo thicken source face IDs")
            {
                source_refusals.set(source_refusals.get() + 1);
            }
            result
        });
    // The boundary walker visits each preceding admission once, then checks
    // the exact model-face allowance. There is one source face, not an end row.
    assert_eq!(source_refusals.get(), 1);
    assert!(
        matches!(definition, cadmpeg_ir::features::FeatureDefinition::Operation(
        cadmpeg_ir::features::FeatureOperation::Thicken {
            faces: cadmpeg_ir::features::FaceSelection::Resolved { faces, native }, ..
        }) if faces == vec![super::resolved_hole_face().id]
            && native == "creo:allfeatur:thicken_source_surfaces#17:11")
    );
}

#[test]
fn rejected_section_sketch_identity_uses_only_live_scratch() {
    let mut scan = crate::test_support::empty_container_scan();
    scan.features.rows.push(crate::feature::rows::FeatureRow {
        feature_id: 2,
        root_schema_class: Some(crate::feature::schema::SchemaClass::Section),
        stream_offset: 0,
        body: vec![0; 64].try_into().expect("row body"),
        body_offset: 0,
        offset: 0,
    });
    scan.features.definitions.push(crate::feature::definitions::FeatureDefinition {
        identity: crate::feature::definitions::DefinitionIdentity::Parsed {
            schema_id: std::num::NonZeroU32::new(7),
            owner_feature_id: Some(2),
        },
        body: Vec::new(),
        parameter_frames: Vec::new(),
        outlines: Vec::new(),
        variables: None,
        segments: None,
        trim_entities: None,
        trim_vertices: None,
        order_table: None,
        section_3d: Some(crate::feature::definitions::FeatureSection3d {
            sketch_plane_entity_id: None,
            sketch_plane_flip: None,
            reference_planes: crate::feature::definitions::ReferencePlanes::Named(Vec::new()),
            reference_plane_datum_geometry_id: None,
            orientation: crate::feature::definitions::FeatureSectionOrientation::default(),
            dimension_ids: Vec::new(),
            offset: 20,
        }),
        dimensions: None,
        relations: None,
        saved_section: None,
        offset: 10,
    });
    scan.features.section_transforms.push(
        crate::placement::FeatureSectionTransform::new(
            7, Some(2), [0.0; 3], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0], 20,
        ).expect("valid section frame"),
    );
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_retained_bytes = 0;
    policy.limits.max_materialized_bytes = cadmpeg_core::decode::u64_from_index("creo:model:sketch#7".len());
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("root");
    for _ in 0..3 {
        let definition = super::super::schema_feature_definition(
            &ctx, &scan, &cadmpeg_ir::document::CadIr::empty(),
            &crate::decode::source_carriers::SourceUnitCarriers::default(),
            2, Some(crate::feature::schema::SchemaClass::Section), "Section",
        ).expect("missing model sketch retains no lookup identity");
        assert!(matches!(definition, cadmpeg_ir::features::FeatureDefinition::Operation(
            cadmpeg_ir::features::FeatureOperation::Sketch {
                sketch: cadmpeg_ir::features::SketchFeatureBinding::Planar(None),
            }
        )));
    }
}
