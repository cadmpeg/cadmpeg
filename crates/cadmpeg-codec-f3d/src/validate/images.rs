// SPDX-License-Identifier: Apache-2.0
//! Validate Canvas and Decal image frames and their Design object joins.

use std::collections::HashSet;

use super::{design_stream, push_constant_finding, Ctx};
use crate::records::topology::extrude_selection::DesignOperandRole;
use crate::{ids, records};
use cadmpeg_ir::report::check::{Check, Finding};

/// Validate Canvas scope and Design object joins.
pub(super) fn validate_canvas_images(ctx: &Ctx<'_, '_>, findings: &mut Vec<Finding>) {
    let native = ctx.native;
    let mut scope_bindings = HashSet::new();
    let mut geometry_records = HashSet::new();
    let geometry_entities = native
        .design_types
        .iter()
        .filter(|design_type| {
            matches!(
                design_type.module.as_str(),
                records::entity_header::DESIGN_MODULE_BODY
                    | records::entity_header::DESIGN_MODULE_GEOMETRY
            )
        })
        .flat_map(|design_type| {
            let design_segment = ids::design_segment(design_type.id());
            design_type
                .entities
                .values()
                .map(move |suffix| (design_segment, *suffix))
        })
        .collect::<std::collections::HashSet<_>>();
    let component_entities = native
        .design_types
        .iter()
        .filter(|design_type| {
            matches!(
                design_type.module.as_str(),
                records::entity_header::DESIGN_MODULE_FUSION
                    | records::entity_header::DESIGN_MODULE_COMPONENT
            )
        })
        .flat_map(|design_type| {
            let design_segment = ids::design_segment(design_type.id());
            design_type
                .entities
                .values()
                .map(move |suffix| (design_segment, *suffix))
        })
        .collect::<std::collections::HashSet<_>>();
    for image in &native.design_canvas_images {
        let native_stream = design_stream(&image.id);
        let design_segment = ids::design_segment(&image.id);
        let scope = ctx
            .scopes_by_index
            .get(&(native_stream, image.scope_record_index));
        let scope_valid = scope.is_some_and(|scope| {
            scope.kind() == crate::records::feature::scope::DesignFeatureKind::Canvas
        });
        let scope_unique = if scope_valid {
            scope_bindings.insert((native_stream, image.scope_record_index))
        } else {
            false
        };
        let geometry_unique = if scope_unique {
            geometry_records.insert((native_stream, image.geometry().record_index()))
        } else {
            false
        };
        let valid = scope_valid
            && scope_unique
            && geometry_unique
            && scope.is_some_and(|scope| scope.byte_offset() == image.scope_byte_offset())
            && image.plane_entity_suffix.is_none_or(|suffix| {
                geometry_entities.contains(&(design_segment, u64::from(suffix)))
            })
            && component_entities
                .contains(&(design_segment, u64::from(image.component_entity_suffix)));
        if !valid {
            push_constant_finding(
                findings,
                Check::NativeLinks,
                "Fusion Canvas image has an invalid frame or Design object join",
                Some(image.id.clone()),
            );
        }
    }
}

/// Validate Decal native and neutral object joins.
pub(super) fn validate_decal_images(ctx: &Ctx<'_, '_>, findings: &mut Vec<Finding>) {
    const TARGET_ROLE: DesignOperandRole = DesignOperandRole::BODIES_A;
    let mut scope_bindings = HashSet::new();
    let mut asset_records = HashSet::new();
    let fusion_entities = ctx
        .native
        .design_types
        .iter()
        .filter(|design_type| design_type.module == records::entity_header::DESIGN_MODULE_FUSION)
        .flat_map(|design_type| {
            let segment = ids::design_segment(design_type.id());
            design_type
                .entities
                .values()
                .map(move |suffix| (segment, *suffix))
        })
        .collect::<std::collections::HashSet<_>>();
    for image in &ctx.native.design_decal_images {
        let native_stream = design_stream(&image.id);
        let design_segment = ids::design_segment(&image.id);
        let scope = ctx
            .scopes_by_index
            .get(&(native_stream, image.scope_record_index()));
        let group = ctx
            .operand_groups_by_index
            .get(&(native_stream, image.target_group_record_index));
        let neutral_is_valid = !ctx.holds_design_intent() || scope.is_some_and(|scope| {
            if image.placement.is_some() {
                return ctx.ir.model.assets.iter().any(|candidate| {
                    candidate
                        .name
                        .as_ref()
                        .map(cadmpeg_core::text::NonBlankString::as_str)
                        == Some(image.asset.name())
                }) && ctx.ir.model.features.iter().any(|feature| {
                    feature.native_ref.as_deref() == Some(scope.id.as_str())
                        && !matches!(
                            feature.evaluation.definition(),
                            cadmpeg_ir::features::FeatureDefinition::Operation(
                                cadmpeg_ir::features::FeatureOperation::Decal { .. }
                            )
                        )
                });
            }
            let expected_faces = crate::design::decode::decal::DecalTargets {
                groups: &ctx.native.design_construction_operand_groups,
                operands: &ctx.native.design_body_recipe_operands,
            }
            .faces(scope, image.target_group_record_index)
            .unwrap_or_else(|| cadmpeg_ir::features::selections::FaceSelection::Native(scope.id.clone()));
            ctx.ir
                .model
                .features
                .iter()
                .any(|feature| {
                    feature.native_ref.as_deref() == Some(scope.id.as_str()) &&
                    matches!(
                        feature.evaluation.definition(),
                        cadmpeg_ir::features::FeatureDefinition::Operation(cadmpeg_ir::features::FeatureOperation::Decal {
                            asset,
                            faces,
                            mapping: cadmpeg_ir::features::datums::DecalMapping::FitToFaces,
                            opacity: None,
                        }) if faces == &expected_faces
                            && ctx.ir.model.assets.iter().any(|candidate| {
                                candidate.id == *asset
                                    && candidate.name.as_ref().map(cadmpeg_core::text::NonBlankString::as_str) == Some(image.asset.name())
                            })
                    )
                })
        });
        let scope_valid = scope.is_some_and(|scope| {
            scope.kind() == crate::records::feature::scope::DesignFeatureKind::Decal
        });
        let scope_unique = if scope_valid {
            scope_bindings.insert((native_stream, image.scope_record_index()))
        } else {
            false
        };
        let asset_unique = if scope_unique {
            asset_records.insert((native_stream, image.asset.record_index()))
        } else {
            false
        };
        let valid = scope_valid
            && scope_unique
            && asset_unique
            && scope.is_some_and(|scope| scope.byte_offset() == image.scope_byte_offset())
            && fusion_entities.contains(&(design_segment, u64::from(image.asset.entity_suffix())))
            && group.is_some_and(|group| {
                group.scope_record_index == image.scope_record_index()
                    && group.role() == TARGET_ROLE
            })
            && neutral_is_valid;
        if !valid {
            push_constant_finding(
                findings,
                Check::NativeLinks,
                "Fusion Decal image has an invalid frame or Design object join",
                Some(image.id.clone()),
            );
        }
    }
}
