// SPDX-License-Identifier: Apache-2.0

use super::super::section_axis_line_carrier;
use super::section_axis_reference_line_geometry;
use crate::decode::feature_history::dependencies::{
    agreed_feature_replay_edge_ids, agreed_feature_replay_geometry_ids,
};
use crate::decode::feature_history::selections::agreed_feature_geometry_ids;
use crate::decode::sketch::coordinates::resolved_section_coordinates;
use crate::decode::sketch_transfer::recipe::{
    current_feature_operation, current_feature_recipe, current_feature_recipe_parent,
    resolved_feature_schema_class_from_classes, row_feature_schema_classes,
    unique_feature_revolution_extent,
};
use crate::decode::uniqueness::{
    unique_feature_section_transform, unique_owned_feature_definition,
};
use crate::feature::rows::agreed_feature_affected_ids;
use cadmpeg_ir::sketches::{SketchGeometry, SketchGeometryDefinition};
use std::collections::{BTreeMap, BTreeSet};

#[test]
fn section_axis_line_carrier_uses_equal_decoded_ordinates() {
    crate::decode::with_test_decode_ctx(|ctx| {
        let segment = crate::feature::definitions::FeatureSegment {
            kind: crate::feature::definitions::FeatureSegmentKind::Line([7, 9]),
            directions: [Some(0), None, Some(0)],
            center_id: None,
            arc_orientation: None,
            vertical_horizontal: None,
            radius_ref: None,
            radius2_ref: None,
            external_id: 12,
            body: Vec::new(),
            offset: 40,
        };
        let definition = crate::feature::definitions::FeatureDefinition {
            identity: crate::feature::definitions::DefinitionIdentity::Parsed {
                schema_id: std::num::NonZeroU32::new(5),
                owner_feature_id: Some(6),
            },
            body: Vec::new(),
            parameter_frames: Vec::new(),
            outlines: Vec::new(),
            variables: Some(crate::feature::definitions::test_support::with_points(
                crate::feature::definitions::FeatureVariableTable {
                    declared_count: 0,
                    entity_ref: None,
                    rows: Vec::new(),
                    offset: 0,
                },
                vec![
                    crate::feature::definitions::FeatureSectionPoint {
                        point_id: 7,
                        u: Some(2.0),
                        v: None,
                    },
                    crate::feature::definitions::FeatureSectionPoint {
                        point_id: 9,
                        u: Some(2.0),
                        v: Some(8.0),
                    },
                ],
            )),
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
        assert_eq!(
            section_axis_line_carrier(&definition, &segment),
            Some(
                SketchGeometry::try_from(SketchGeometryDefinition::ReferenceLine {
                    origin: cadmpeg_ir::math::Point2::new(2.0, 0.0),
                    direction: cadmpeg_ir::math::Point2::new(0.0, 1.0),
                })
                .expect("valid test fixture")
            )
        );
        assert_eq!(
            section_axis_reference_line_geometry(
                &definition,
                &crate::decode::with_test_decode_ctx(|ctx| resolved_section_coordinates(
                    ctx,
                    &definition
                ))
                .expect("test section solve"),
                &segment,
            ),
            Some(
                SketchGeometry::try_from(SketchGeometryDefinition::ReferenceLine {
                    origin: cadmpeg_ir::math::Point2::new(2.0, 0.0),
                    direction: cadmpeg_ir::math::Point2::new(0.0, 1.0),
                })
                .expect("valid test fixture")
            )
        );
        assert_eq!(
            section_axis_reference_line_geometry(
                &definition,
                &BTreeMap::from([(7, [Some(2.0), None]), (9, [None, Some(8.0)]),]),
                &segment,
            ),
            None
        );
        let mut selector_segment = segment.clone();
        selector_segment.directions = [None; 3];
        selector_segment.vertical_horizontal = Some(0);
        let mut selector_definition = definition.clone();
        selector_definition.segments = Some(crate::feature::definitions::FeatureSegmentTable {
            declared_count: 1,
            has_elided_prototype: false,
            entity_ref: None,
            rows: (vec![selector_segment.clone()])
                .into_iter()
                .map(crate::feature::segment_rows::SegmentRow::Ordinary)
                .collect(),
            offset: 0,
        });
        assert_eq!(
            section_axis_reference_line_geometry(
                &selector_definition,
                &BTreeMap::from([(7, [Some(2.0), None]), (9, [Some(2.0), Some(8.0)])]),
                &selector_segment,
            ),
            Some(
                SketchGeometry::try_from(SketchGeometryDefinition::ReferenceLine {
                    origin: cadmpeg_ir::math::Point2::new(2.0, 0.0),
                    direction: cadmpeg_ir::math::Point2::new(0.0, 1.0),
                })
                .expect("valid test fixture")
            )
        );
        assert_eq!(
            unique_owned_feature_definition(ctx, std::slice::from_ref(&definition), 6)
                .expect("admitted unique lookup")
                .map(|matched| matched.identity.id()),
            Some(5)
        );
        assert!(
            unique_owned_feature_definition(ctx, &[definition.clone(), definition.clone()], 6)
                .expect("admitted unique lookup")
                .is_none()
        );
        let operation = crate::feature::operations::FeatureOperation {
            feature_id: 6,
            kind: crate::feature::operations::OperationKind::Extrude,
            name: crate::feature::operations::OperationName::Stored {
                bytes: b"Extrude id 6".to_vec(),
                keyword: crate::feature::operations::IdKeyword::Id,
                prefix: None,
            },
            recipe: crate::feature::operations::RecipeResolution::Resolved(
                crate::feature::operations::FeatureRecipe::ProtrudeExtrude,
            ),
            display_state_conflict: false,
            depdb: Some(crate::feature::operations::DepdbPrefix {
                schema: crate::feature::schema::SchemaClass::Protrusion,
                parent: 0,
            }),
            offset: 10,
            state_offset: 10,
        };
        assert_eq!(
            current_feature_operation(std::slice::from_ref(&operation), 6)
                .and_then(crate::feature::operations::FeatureOperation::root_schema_class),
            Some(crate::feature::schema::SchemaClass::Protrusion)
        );
        assert!(current_feature_operation(&[operation.clone(), operation.clone()], 6).is_none());
        assert_eq!(
            current_feature_recipe(std::slice::from_ref(&operation), 6),
            Some(crate::feature::operations::FeatureRecipe::ProtrudeExtrude)
        );
        let mut conflicting_recipe = operation.clone();
        conflicting_recipe.recipe = crate::feature::operations::RecipeResolution::Resolved(
            crate::feature::operations::FeatureRecipe::ProtrudeRevolve,
        );
        assert_eq!(
            current_feature_recipe(&[operation.clone(), conflicting_recipe], 6),
            None
        );
        let mut parented_operation = operation.clone();
        parented_operation.depdb = Some(crate::feature::operations::DepdbPrefix {
            schema: crate::feature::schema::SchemaClass::Protrusion,
            parent: 5,
        });
        assert_eq!(
            current_feature_recipe_parent(std::slice::from_ref(&parented_operation), 6),
            Some(5)
        );
        let mut conflicting_parent = parented_operation.clone();
        conflicting_parent.depdb = Some(crate::feature::operations::DepdbPrefix {
            schema: crate::feature::schema::SchemaClass::Protrusion,
            parent: 4,
        });
        assert_eq!(
            current_feature_recipe_parent(&[parented_operation, conflicting_parent], 6),
            None
        );
        let row = |schema_class, offset| crate::feature::rows::FeatureRow {
            feature_id: 6,
            root_schema_class: Some(crate::feature::schema::SchemaClass::from(schema_class)),
            stream_offset: 0,
            body: vec![0; 2].try_into().expect("row body"),
            body_offset: offset + 1,
            offset,
        };
        let checked_classes = |rows: &[crate::feature::rows::FeatureRow]| {
            crate::decode::with_test_decode_ctx(|ctx| row_feature_schema_classes(ctx, rows, 6))
                .expect("schema classes fit service limits")
        };
        let resolve_classes =
            |operations: &[crate::feature::operations::FeatureOperation],
             rows: &[crate::feature::rows::FeatureRow]| {
                let classes = checked_classes(rows);
                crate::decode::with_test_decode_ctx(|ctx| {
                    resolved_feature_schema_class_from_classes(
                        operations,
                        6,
                        |visit_class| {
                            for class in ctx.admit_iter(&classes, "test feature schema classes")? {
                                if matches!(visit_class(*class)?, std::ops::ControlFlow::Break(()))
                                {
                                    break;
                                }
                            }
                            Ok(())
                        },
                        || Ok(false),
                    )
                })
                .expect("feature schema class rows fit service limits")
            };
        assert_eq!(
            resolve_classes(&[], &[row(917, 20), row(917, 30)]),
            Some(crate::feature::schema::SchemaClass::Protrusion)
        );
        assert_eq!(resolve_classes(&[], &[row(913, 20), row(914, 30)]), None);
        assert_eq!(
            resolve_classes(
                std::slice::from_ref(&operation),
                &[row(913, 20), row(914, 30)],
            ),
            Some(crate::feature::schema::SchemaClass::Protrusion)
        );
        assert_eq!(
            resolve_classes(
                std::slice::from_ref(&operation),
                &[row(913, 20), row(913, 30)],
            ),
            Some(crate::feature::schema::SchemaClass::Protrusion)
        );
        assert_eq!(
            checked_classes(&[row(913, 20), row(914, 30)]),
            BTreeSet::from([
                crate::feature::schema::SchemaClass::Round,
                crate::feature::schema::SchemaClass::Chamfer
            ])
        );
        let extent = |feature_id, offset| crate::feature::rows::FeatureRevolutionExtent {
            feature_id,
            offset,
        };
        let extent_feature_id = |records: &[crate::feature::rows::FeatureRevolutionExtent],
                                 feature_id| {
            crate::decode::with_test_decode_ctx(|ctx| {
                unique_feature_revolution_extent(ctx, records, feature_id)
                    .map(|extent| extent.map(|record| record.feature_id))
            })
            .expect("revolution extent rows fit service limits")
        };
        assert_eq!(
            extent_feature_id(&[extent(6, 40), extent(6, 50)], 6),
            Some(6)
        );
        assert_eq!(extent_feature_id(&[extent(7, 40)], 6), None);
        let transform = crate::placement::FeatureSectionTransform::new(
            5,
            Some(6),
            [0.0; 3],
            [1.0, 0.0, 0.0],
            [0.0, 1.0, 0.0],
            40,
        )
        .expect("valid section frame");
        assert_eq!(
            unique_feature_section_transform(ctx, std::slice::from_ref(&transform), 5, 40)
                .expect("admitted unique lookup")
                .map(|placed| placed.offset),
            Some(40)
        );
        assert!(unique_feature_section_transform(
            ctx,
            &[transform.clone(), transform.clone()],
            5,
            40
        )
        .expect("admitted unique lookup")
        .is_none());
        let repeated_schema = crate::placement::FeatureSectionTransform::new(
            transform.definition_id,
            Some(7),
            transform.origin(),
            transform.u_axis(),
            transform.v_axis(),
            50,
        )
        .expect("valid section frame");
        assert_eq!(
            unique_feature_section_transform(ctx, &[transform.clone(), repeated_schema], 5, 40)
                .expect("admitted unique lookup")
                .map(|placed| placed.offset),
            Some(40)
        );
        let competing_definition = crate::placement::FeatureSectionTransform::new(
            7,
            transform.feature_id,
            transform.origin(),
            transform.u_axis(),
            transform.v_axis(),
            50,
        )
        .expect("valid section frame");
        assert!(
            unique_feature_section_transform(ctx, &[transform, competing_definition], 5, 40)
                .expect("admitted unique lookup")
                .is_none()
        );
        let affected = |ids: &[u32], offset| crate::feature::rows::FeatureAffectedIds {
            feature_id: 6,
            kind: crate::feature::rows::AffectedIdKind::Edges,
            ids: ids.to_vec(),
            offset,
        };
        assert_eq!(
            agreed_feature_affected_ids(ctx,
                &[affected(&[7, 8], 60), affected(&[7, 8], 70)],
                6,
                crate::feature::rows::AffectedIdKind::Edges,
            ).expect("affected ID agreement admission"),
            Some(&[7, 8][..])
        );
        assert_eq!(
            agreed_feature_affected_ids(ctx,
                &[affected(&[7, 8], 60), affected(&[8, 7], 70)],
                6,
                crate::feature::rows::AffectedIdKind::Edges,
            ).expect("affected ID agreement admission"),
            None
        );
        let replay = |geometry_ids: &[u32], edge_ids: &[u32], offset| {
            crate::feature::rows::FeatureReplayAffectedIds {
                feature_id: 6,
                geometry_ids: geometry_ids.to_vec(),
                edge_ids: edge_ids.to_vec(),
                geometry_extent: crate::feature::rows::ReplayExtentSource::Explicit,
                edge_extent: crate::feature::rows::ReplayExtentSource::Inherited,
                offset,
            }
        };
        let geometry = |ids: &[u32], offset| crate::feature::rows::FeatureAffectedIds {
            feature_id: 6,
            kind: crate::feature::rows::AffectedIdKind::Geometry,
            ids: ids.to_vec(),
            offset,
        };
        let replay_geometry = replay(&[9], &[7], 80);
        assert_eq!(
            agreed_feature_geometry_ids(ctx, &[], std::slice::from_ref(&replay_geometry), 6)
                .expect("admitted feature geometry ID agreement"),
            Some(&[9][..])
        );
        let named_empty = geometry(&[], 60);
        assert_eq!(
            agreed_feature_geometry_ids(
                ctx,
                std::slice::from_ref(&named_empty),
                std::slice::from_ref(&replay_geometry),
                6,
            )
            .expect("admitted feature geometry ID agreement"),
            Some(&[][..])
        );
        let conflicting_named = [geometry(&[7], 60), geometry(&[8], 70)];
        assert_eq!(
            agreed_feature_geometry_ids(
                ctx,
                &conflicting_named,
                std::slice::from_ref(&replay_geometry),
                6,
            )
            .expect("admitted feature geometry ID agreement"),
            None
        );
        assert_eq!(
            agreed_feature_replay_geometry_ids(
                ctx,
                &[replay(&[1, 2], &[7], 80), replay(&[1, 2], &[7], 90)],
                6,
            )
            .expect("admitted replay geometry ID agreement"),
            Some(&[1, 2][..])
        );
        assert_eq!(
            agreed_feature_replay_edge_ids(
                ctx,
                &[replay(&[1], &[7], 80), replay(&[1], &[], 90)],
                6,
            )
            .expect("admitted replay edge ID agreement"),
            None
        );
    });
}
