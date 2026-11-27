// SPDX-License-Identifier: Apache-2.0
//! Edge, edge-treatment vertex, and edge identity operands.

use super::extrude_selections::parse_edge_identity_member;
use super::recipe_operands::{parse_edge_operand, parse_vertex_recipe};
use crate::container::ContainerScan;
use crate::design::decode::dimension_frames::bind_recipe_reference_candidates;
use crate::design::decode::text::design_record_id;
use crate::design::{design_feature_family, DesignFeatureFamily};
use crate::ids::native_stream;
use crate::records::decal::DesignRecordHeader;
use crate::records::feature::scope::DesignParameterScope;
use crate::records::feature::surface_ops::DesignSurfaceOffsetSupport;
use crate::records::feature::work_geometry::DesignEdgeTreatmentVertexOperand;
use crate::records::recipes::ConstructionRecipe;
use crate::records::sketch_links::PersistentSubentityTag;
use crate::records::topology::construction::DesignConstructionOperandGroup;
use crate::records::topology::edge_identity::{DesignEdgeIdentityOperand, DesignEdgeOperand};
use crate::records::topology::extrude_selection::DesignOperandRole;
use cadmpeg_core::container::ContainerRole;
use cadmpeg_core::decode::DecodeContext;
use cadmpeg_core::CodecError;
use std::collections::{HashMap, HashSet};

/// Decode edge-recipe operand frames named by edge-selecting feature scopes.
pub(crate) fn decode_edge_operands(
    ctx: &DecodeContext<'_>,
    scan: &ContainerScan,
    scopes: &[DesignParameterScope],
    groups: &[DesignConstructionOperandGroup],
    headers: &[DesignRecordHeader],
    recipes: &[ConstructionRecipe],
) -> Result<Vec<DesignEdgeOperand>, CodecError> {
    let record_headers = headers;
    let mut headers = HashMap::new();
    for header in record_headers {
        let Some(stream) = native_stream(&header.id) else {
            continue;
        };
        let key = (stream, header.record_index);
        headers.insert(key, header);
    }
    let mut terminal_group_members = HashSet::new();
    for group in groups {
        let Some(stream) = native_stream(&group.id) else {
            continue;
        };
        let Some(member) = group.members().last() else {
            continue;
        };
        let key = (stream, group.scope_record_index, member.value);
        terminal_group_members.insert(key);
    }
    let mut stream_offsets: HashMap<&str, Vec<u64>> = HashMap::new();
    for header in record_headers {
        if let Some(stream) = native_stream(&header.id) {
            stream_offsets
                .entry(stream)
                .or_default()
                .push(header.byte_offset);
        }
    }
    for offsets in stream_offsets.values_mut() {
        offsets.sort_unstable();
    }
    let mut out = Vec::new();
    for scope in scopes
        .iter()
        .filter(|scope| has_edge_recipe_operands(&scope.kind()))
    {
        let mut member_indices = HashSet::new();
        for group in groups.iter().filter(|group| {
            native_stream(&group.id) == native_stream(&scope.id)
                && group.scope_record_index == scope.record_index
        }) {
            for member in group.members() {
                member_indices.insert(member.value);
            }
        }
        if let Some(operation) = scope.surface_extend_operation() {
            for &index in &operation.edge_record_indices {
                member_indices.insert(index);
            }
        }
        if let Some(operation) = scope.surface_offset_operation() {
            if let DesignSurfaceOffsetSupport::BoundaryCarrier {
                edge_record_indices,
                ..
            } = &operation.support
            {
                for &index in edge_record_indices {
                    member_indices.insert(index);
                }
            }
        }
        if let Some(construction) = scope.work_point_construction() {
            for input in construction.rule.inputs() {
                member_indices.insert(input.record_index());
            }
        }
        let Some(stream) = native_stream(&scope.id) else {
            continue;
        };
        let Some(entry) = scan.design_stream_entry_for_scope(ContainerRole::Bulkstream, stream)
        else {
            continue;
        };
        let bytes = scan.entry_bytes(&entry.name)?;
        let records = scan.design_record_offsets(&entry.name)?;
        for (ordinal, record_index) in scope.reference_members().values().copied().enumerate() {
            if !member_indices.contains(&record_index) {
                continue;
            }
            let Ok(ordinal) = u32::try_from(ordinal) else {
                continue;
            };
            let Some(header) = headers.get(&(stream, record_index)) else {
                continue;
            };
            let terminal_group_member =
                terminal_group_members.contains(&(stream, scope.record_index, header.record_index));
            let stream_end = u64::try_from(bytes.len()).map_err(|_| {
                CodecError::malformed("Fusion Design BulkStream exceeds the addressable range")
            })?;
            let terminal_group_limit = terminal_group_member.then(|| {
                stream_offsets
                    .get(stream)
                    .and_then(|offsets| {
                        let at = offsets.partition_point(|offset| *offset <= header.byte_offset);
                        offsets.get(at).copied()
                    })
                    .unwrap_or(stream_end)
            });
            let Some(operand) = parse_edge_operand(
                ctx,
                bytes,
                &records,
                scope,
                (ordinal, header),
                recipes,
                terminal_group_limit,
            ) else {
                continue;
            };
            let operand = operand?;

            out.push(operand);
        }
    }
    out.sort_by(|a, b| a.id.cmp(&b.id));
    Ok(out)
}

/// Decode vertex-recipe members retained inside edge-treatment groups.
pub(crate) fn decode_edge_treatment_vertex_operands(
    scan: &ContainerScan,
    scopes: &[DesignParameterScope],
    groups: &[DesignConstructionOperandGroup],
    headers: &[DesignRecordHeader],
    recipes: &[ConstructionRecipe],
) -> Result<Vec<DesignEdgeTreatmentVertexOperand>, CodecError> {
    let mut header_index = HashMap::new();
    for header in headers {
        let Some(stream) = native_stream(&header.id) else {
            continue;
        };
        let key = (stream, header.record_index);
        header_index.insert(key, header);
    }
    let mut out = Vec::new();
    for scope in scopes
        .iter()
        .filter(|scope| has_edge_recipe_operands(&scope.kind()))
    {
        let Some(stream) = native_stream(&scope.id) else {
            continue;
        };
        let Some(entry) = scan.design_stream_entry_for_scope(ContainerRole::Bulkstream, stream)
        else {
            continue;
        };
        let bytes = scan.entry_bytes(&entry.name)?;
        let records = scan.design_record_offsets(&entry.name)?;
        for (scope_reference_ordinal, record_index) in
            scope.reference_members().values().copied().enumerate()
        {
            let mut matches = groups
                .iter()
                .filter(|group| {
                    native_stream(&group.id) == Some(stream)
                        && group.scope_record_index == scope.record_index
                })
                .filter_map(|group| {
                    let mut ordinals = group
                        .members()
                        .iter()
                        .map(|member| &member.value)
                        .enumerate()
                        .filter(|(_, member)| **member == record_index);
                    let (ordinal, _) = ordinals.next()?;
                    ordinals.next().is_none().then_some((group, ordinal))
                });
            let Some((group, group_member_ordinal)) = matches.next() else {
                continue;
            };
            if matches.next().is_some() {
                continue;
            }
            let Some(header) = header_index.get(&(stream, record_index)) else {
                continue;
            };
            let Some(recipe) = parse_vertex_recipe(bytes, &records, stream, header, recipes) else {
                continue;
            };
            let (Ok(scope_reference_ordinal), Ok(group_member_ordinal)) = (
                u32::try_from(scope_reference_ordinal),
                u32::try_from(group_member_ordinal),
            ) else {
                continue;
            };

            out.push(DesignEdgeTreatmentVertexOperand {
                id: design_record_id(
                    stream,
                    ":edge-treatment-vertex-operand#",
                    header.byte_offset,
                ),
                scope_record_index: scope.record_index,
                scope_reference_ordinal,
                group_record_index: group.record_index,
                group_member_ordinal,
                recipe,
            });
        }
    }
    out.sort_by(|left, right| left.id.cmp(&right.id));
    Ok(out)
}

/// Bind persistent subentity candidates carried by decoded vertex recipes.
pub(crate) fn bind_vertex_recipe_candidates(
    scopes: &mut [DesignParameterScope],
    tags: &[PersistentSubentityTag],
) {
    for scope in scopes {
        let scope_id = scope.id.clone();
        if let Some(construction) = scope.work_plane_construction_mut() {
            for reference in construction.recipe_references_mut() {
                bind_recipe_reference_candidates(reference, tags, Some(&scope_id));
            }
        }
        let Some(construction) = scope.work_point_construction_mut() else {
            continue;
        };
        for recipe in construction.rule.vertex_recipes_mut() {
            for reference in &mut recipe.recipe_references {
                bind_recipe_reference_candidates(reference, tags, Some(&scope_id));
            }
        }
    }
}

/// Bind active fallback candidates for edge-treatment corner recipes.
pub(crate) fn bind_edge_treatment_vertex_candidates(
    operands: &mut [DesignEdgeTreatmentVertexOperand],
    tags: &[PersistentSubentityTag],
) {
    for operand in operands {
        for reference in &mut operand.recipe.recipe_references {
            bind_recipe_reference_candidates(reference, tags, Some(&operand.id));
        }
    }
}

/// Whether a feature family owns edge-recipe operands directly or through a
/// counted construction-operand group.
pub(crate) fn has_edge_recipe_operands(
    kind: &crate::records::feature::scope::DesignFeatureKind,
) -> bool {
    use crate::records::feature::scope::DesignFeatureKind as Kind;
    matches!(
        design_feature_family(kind),
        Some(
            DesignFeatureFamily::Fillet
                | DesignFeatureFamily::Chamfer
                | DesignFeatureFamily::Revolve
                | DesignFeatureFamily::Loft
                | DesignFeatureFamily::Sweep
                | DesignFeatureFamily::Pipe
                | DesignFeatureFamily::SurfacePatch
                | DesignFeatureFamily::SurfaceExtend
                | DesignFeatureFamily::SurfaceOffset
                | DesignFeatureFamily::SurfaceRuled
        )
    ) || matches!(
        kind,
        Kind::EdgeFlange | Kind::Hem | Kind::WorkPoint | Kind::SurfaceUntrim
    )
}

pub(crate) fn is_surface_untrim_face_group(
    scope: &DesignParameterScope,
    group: &DesignConstructionOperandGroup,
) -> bool {
    scope.kind() == crate::records::feature::scope::DesignFeatureKind::SurfaceUntrim
        && group.role() == DesignOperandRole::ROLE_0X9
}

/// Indexed-record distance from an edge-recipe primary record to its terminal
/// record for the owning consumer.
pub(crate) fn edge_recipe_terminal_delta(
    kind: &crate::records::feature::scope::DesignFeatureKind,
) -> u32 {
    use crate::records::feature::scope::DesignFeatureKind as Kind;
    match design_feature_family(kind) {
        Some(DesignFeatureFamily::Sweep) => 7,
        _ if matches!(kind, Kind::WorkPoint) => 5,
        _ => 4,
    }
}

/// Decode persistent selection identities named by Fillet and Chamfer groups.
pub(crate) fn decode_edge_identity_operands(
    scan: &ContainerScan,
    scopes: &[DesignParameterScope],
    groups: &[DesignConstructionOperandGroup],
    headers: &[DesignRecordHeader],
) -> Result<Vec<DesignEdgeIdentityOperand>, CodecError> {
    let mut header_index = HashMap::new();
    for header in headers {
        let Some(stream) = native_stream(&header.id) else {
            continue;
        };
        let key = (stream, header.record_index);
        header_index.insert(key, header);
    }
    let mut out = Vec::new();
    for group in groups {
        let Some(stream) = native_stream(&group.id) else {
            continue;
        };
        let Some(scope) = scopes.iter().find(|scope| {
            native_stream(&scope.id) == Some(stream)
                && scope.record_index == group.scope_record_index
                && matches!(
                    design_feature_family(&scope.kind()),
                    Some(DesignFeatureFamily::Fillet | DesignFeatureFamily::Chamfer)
                )
        }) else {
            continue;
        };
        let Some(entry) = scan.design_stream_entry_for_scope(ContainerRole::Bulkstream, stream)
        else {
            continue;
        };
        let bytes = scan.entry_bytes(&entry.name)?;
        for (ordinal, record_index) in group
            .members()
            .iter()
            .map(|member| member.value)
            .enumerate()
        {
            let Some(header) = header_index.get(&(stream, record_index)) else {
                continue;
            };
            let Ok(start) = usize::try_from(header.byte_offset) else {
                continue;
            };
            let Some(parsed) = parse_edge_identity_member(bytes, start) else {
                continue;
            };
            let Ok(group_member_ordinal) = u32::try_from(ordinal) else {
                continue;
            };
            let (Ok(asset_id), Ok(context_id)) = (
                crate::records::mesh::DesignRelaxedGuidText::try_from(parsed.asset_id),
                crate::records::mesh::DesignRelaxedGuidText::try_from(parsed.context_id),
            ) else {
                continue;
            };

            out.push(
                DesignEdgeIdentityOperand::try_new(
                    crate::records::topology::edge_identity::DesignEdgeIdentityOperandDraft {
                        id: design_record_id(
                            &entry.name,
                            ":design-edge-identity-operand#",
                            header.byte_offset,
                        ),
                        scope_record_index: scope.record_index,
                        group_record_index: group.record_index,
                        group_member_ordinal,
                        record_index,
                        byte_offset: header.byte_offset,
                        class_tag: header.class_tag.clone(),
                        layout: parsed.layout,
                        local_id: parsed.local_id,
                        asset_id,
                        asset_id_offset: parsed.asset_id_offset,
                        context_id,
                        context_id_offset: parsed.context_id_offset,
                        historical: None,
                        treatment_radius_candidates: Vec::new(),
                        transition_edge_candidates: Vec::new(),
                        resolved_edge_slots: Vec::new(),
                        resolved_edge_slot: None,
                        resolution_identity_id: None,
                    },
                )
                .map_err(CodecError::Malformed)?,
            );
        }
    }
    out.sort_by(|a, b| a.id.cmp(&b.id));
    Ok(out)
}
