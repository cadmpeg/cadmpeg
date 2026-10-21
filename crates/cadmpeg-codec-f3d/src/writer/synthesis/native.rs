// SPDX-License-Identifier: Apache-2.0
//! Select typed native content that has no neutral form; regenerate the rest.

use std::collections::BTreeMap;

use cadmpeg_core::CodecError;
use cadmpeg_ir::codec::write::{NativeDisposition, NativeDispositions};
use cadmpeg_ir::document::CadIr;
use cadmpeg_ir::report::loss::LossNote;

use crate::native::F3dNative;

const WRITTEN: &[&str] = &[
    "act_tables",
    "act_types",
    "act_entities",
    "act_guids",
    "act_registry_channels",
    "act_root_components",
    "act_table_references",
    "asm_histories",
    "asm_delta_states",
    "asm_bulletin_boards",
    "asm_entity_changes",
    "construction_recipes",
    "persistent_references",
    "lost_edge_references",
    "design_body_members",
    "design_types",
    "design_entity_headers",
    "design_record_headers",
    "body_native_keys",
    "body_visibilities",
    "edge_continuities",
    "face_sidedness",
    "face_native_keys",
    "design_parameters",
    "design_parameter_scopes",
    "design_parameter_owners",
    "design_parameter_companions",
    "design_dimension_recipe_records",
    "design_sketch_placements",
    "sketch_points",
    "sketch_curve_identities",
    "sketch_relations",
    "sketch_texts",
    "design_configurations",
    "creation_timestamps",
    "edge_ownerships",
    "vertex_ownerships",
    "wire_topologies",
    "transform_hints",
    "tolerant_edge_tails",
    "tolerant_vertex_tails",
    "tolerant_coedge_parameters",
    "persistent_design_links",
    "persistent_subentity_tags",
];

pub(super) fn reconcile(target: &mut CadIr, losses: &mut Vec<LossNote>) -> Result<(), CodecError> {
    let original = super::super::primitives::f3d_native(target)?.unwrap_or_default();
    validate_input(target, &original)?;
    let sketch_graph = if super::design::sketch::admit(target, &original)? {
        let mut graph = F3dNative::default();
        super::design::sketch::retain_graph(&mut graph, &original);
        Some(graph)
    } else {
        None
    };
    // Parameter values have an independent generated stream. The native-only
    // metadata registry can therefore coexist with neutral parameter edits.
    let histories = original
        .asm_histories
        .iter()
        .filter(|history| {
            !history.binding_budget_exceeded
                && history.states().iter().all(|state| {
                    state.records.is_empty()
                        && state.bulletin_boards.is_empty()
                        && state.owner_ref < 0
                        && state.partner_ref.is_none()
                        && state.transition.as_ref().is_none_or(|transition| {
                            transition.records == Default::default()
                                && transition.topology == Default::default()
                                && transition.record_revisions.is_empty()
                                && transition.rows == Default::default()
                        })
                })
        })
        .take(1)
        .cloned()
        .collect();
    let mut native = F3dNative {
        act_tables: original.act_tables.clone(),
        act_types: original.act_types.clone(),
        act_entities: original.act_entities.clone(),
        act_guids: original.act_guids.clone(),
        act_registry_channels: original.act_registry_channels.clone(),
        act_root_components: original.act_root_components.clone(),
        act_table_references: original.act_table_references.clone(),
        asm_histories: histories,
        construction_recipes: original.construction_recipes.clone(),
        persistent_references: original.persistent_references.clone(),
        lost_edge_references: original.lost_edge_references.clone(),
        design_body_members: original.design_body_members.clone(),
        design_types: original.design_types.clone(),
        design_entity_headers: original.design_entity_headers.clone(),
        design_record_headers: original.design_record_headers.clone(),
        design_parameters: original.design_parameters.clone(),
        body_native_keys: original.body_native_keys,
        body_visibilities: original.body_visibilities,
        edge_continuities: original.edge_continuities,
        face_sidedness: original.face_sidedness,
        face_native_keys: original.face_native_keys,
        creation_timestamps: original.creation_timestamps,
        edge_ownerships: original.edge_ownerships,
        vertex_ownerships: original.vertex_ownerships,
        wire_topologies: original.wire_topologies,
        transform_hints: original.transform_hints,
        tolerant_edge_tails: original.tolerant_edge_tails,
        tolerant_vertex_tails: original.tolerant_vertex_tails,
        tolerant_coedge_parameters: original.tolerant_coedge_parameters,
        persistent_design_links: original.persistent_design_links,
        persistent_subentity_tags: original.persistent_subentity_tags,
        ..F3dNative::default()
    };
    if let Some(graph) = &sketch_graph {
        super::design::sketch::retain_graph(&mut native, graph);
        for curve in &original.sketch_curve_identities {
            if !native
                .sketch_curve_identities
                .iter()
                .any(|written| written.id == curve.id)
            {
                losses.push(crate::loss::F3dLossCode::WriterDesignRecordsOmitted.note(format!(
                    "Sketch curve {} has no stated owner or retained containing membership and is omitted; supported Sketch entities and constraints remain",
                    curve.id,
                )));
            }
        }
        losses.push(crate::loss::F3dLossCode::WriterDesignRecordsOmitted.note(
            "Sketch identity, geometry and parameter bindings are regenerated; dimensional companion payloads and evaluation invalidation are omitted",
        ));
    }
    super::streams::reconcile(&mut native, losses);
    native
        .design_types
        .sort_by(|left, right| left.class_tag.as_str().cmp(right.class_tag.as_str()));
    native.construction_recipes.retain(|recipe| {
        recipe.record_index.is_some()
            && recipe.design.as_ref().is_none_or(|design| {
                design.id.value.len() == 3
                    && design.id.value.bytes().all(|byte| byte.is_ascii_digit())
            })
    });
    let mut recipe_ordinals = std::collections::HashMap::new();
    for recipe in &mut native.construction_recipes {
        let ordinal = recipe_ordinals
            .entry((
                recipe.kind,
                recipe.design.as_ref().map(|design| design.id.value.clone()),
            ))
            .or_insert(0);
        recipe.recipe_index = *ordinal;
        *ordinal += 1;
    }
    let mut omitted_sketch_records = original
        .sketch_points
        .iter()
        .map(|record| record.record_index)
        .chain(
            original
                .sketch_curve_identities
                .iter()
                .map(|record| record.record_index),
        )
        .chain(
            original
                .sketch_relations
                .iter()
                .map(|record| record.record_index),
        )
        .chain(
            original
                .sketch_texts
                .iter()
                .map(|record| record.record_index),
        )
        .collect::<std::collections::BTreeSet<_>>();
    if sketch_graph.is_some() {
        omitted_sketch_records.clear();
    }
    let omitted_curve_indices = original
        .sketch_curve_identities
        .iter()
        .filter(|before| {
            !native
                .sketch_curve_identities
                .iter()
                .any(|after| after.id == before.id)
        })
        .map(|curve| {
            (
                crate::ids::design_segment(&curve.id),
                u64::from(curve.record_index),
            )
        })
        .collect::<std::collections::BTreeSet<_>>();
    native.design_entity_headers.retain(|header| {
        !omitted_curve_indices.contains(&(
            crate::ids::design_segment(&header.id),
            header.entity_id.suffix(),
        )) && header.member_values().next().is_none()
            && !original
                .design_material_assignments
                .iter()
                .any(|assignment| assignment.entity_id == header.entity_id)
            && !header
                .reference_values()
                .any(|index| omitted_sketch_records.contains(index))
    });
    let omitted_header_entities = original
        .design_entity_headers
        .iter()
        .filter(|before| {
            !native
                .design_entity_headers
                .iter()
                .any(|after| after.entity_id == before.entity_id)
        })
        .map(|header| {
            (
                crate::ids::design_segment(&header.id),
                header.entity_id.suffix(),
            )
        })
        .collect::<std::collections::BTreeSet<_>>();
    for record in &mut native.design_types {
        let segment = crate::ids::design_segment(record.id());
        let entities = record
            .entities
            .values()
            .copied()
            .filter(|id| {
                !omitted_header_entities.contains(&(segment, *id))
                    && !omitted_curve_indices.contains(&(segment, *id))
            })
            .collect();
        record.set_entities(crate::records::identity::ReferenceRun::unlocated(entities));
    }
    // Source class tags are local to their original stream. Select the
    // registered owning type in the newly assembled registry instead.
    for header in &mut native.design_entity_headers {
        if let Some(ordinal) = native.design_types.iter().position(|record| {
            record
                .entities
                .values()
                .any(|entity| *entity == header.entity_id.suffix())
        }) {
            let code = ordinal
                .checked_add(256)
                .filter(|code| *code <= 999)
                .ok_or_else(|| {
                    CodecError::InvalidInput(
                        "F3D Design registry exceeds three-digit class tags".into(),
                    )
                })?;
            header.class_tag = code.to_string().try_into().map_err(CodecError::Malformed)?;
        }
    }
    let reachable = native
        .design_entity_headers
        .iter()
        .filter(|header| header.in_sketch_module())
        .flat_map(|header| header.reference_values().copied())
        .collect::<std::collections::BTreeSet<_>>();
    native
        .design_record_headers
        .retain(|header| reachable.contains(&header.record_index));
    if sketch_graph.is_some() {
        let typed_indices = native
            .sketch_points
            .iter()
            .flat_map(|point| [point.record_index, point.paired_reference])
            .chain(
                native
                    .sketch_curve_identities
                    .iter()
                    .map(|curve| curve.record_index),
            )
            .chain(
                native
                    .sketch_relations
                    .iter()
                    .map(|relation| relation.record_index),
            )
            .chain(native.sketch_texts.iter().map(|text| text.record_index))
            .chain(
                native
                    .design_sketch_placements
                    .iter()
                    .map(|placement| placement.record_index),
            )
            .chain(
                native
                    .design_parameter_scopes
                    .iter()
                    .map(|scope| scope.record_index),
            )
            .chain(
                native
                    .design_dimension_recipe_records
                    .iter()
                    .map(|record| record.record_index),
            )
            .chain(
                native
                    .design_parameters
                    .iter()
                    .map(|parameter| parameter.record_index),
            )
            .chain(
                native
                    .design_parameter_owners
                    .iter()
                    .map(|owner| owner.record_index()),
            )
            .chain(
                native
                    .design_parameter_companions
                    .iter()
                    .map(|companion| companion.record_index()),
            )
            .collect::<std::collections::BTreeSet<_>>();
        native
            .design_record_headers
            .retain(|header| !typed_indices.contains(&header.record_index));
    }
    // These native lanes select identities or add native-only display facts.
    // Geometry and visibility values are always authored from the neutral model.
    native.body_native_keys.retain(|record| {
        target
            .model
            .bodies
            .iter()
            .any(|body| body.id == record.body)
    });
    native.body_visibilities.retain(|record| {
        target
            .model
            .bodies
            .iter()
            .any(|body| body.id == record.body)
    });
    native
        .edge_continuities
        .retain(|record| target.model.edges.iter().any(|edge| edge.id == record.edge));
    native
        .face_sidedness
        .retain(|record| target.model.faces.iter().any(|face| face.id == record.face));
    native.creation_timestamps = target
        .model
        .attributes
        .iter()
        .enumerate()
        .filter_map(|(ordinal, attribute)| {
            let unix_microseconds = crate::brep::creation_timestamp_value(attribute)?;
            let existing = native
                .creation_timestamps
                .iter()
                .find(|record| record.target == attribute.target);
            Some(crate::records::recipes::CreationTimestamp {
                id: existing.map_or_else(
                    || format!("f3d:design:creation-timestamp#generated-{ordinal}"),
                    |record| record.id.clone(),
                ),
                target: attribute.target.clone(),
                record_index: existing
                    .map_or(u32::try_from(ordinal).ok()?, |record| record.record_index),
                unix_microseconds,
            })
        })
        .filter(|record| target_exists(target, &record.target))
        .collect();
    native
        .persistent_design_links
        .retain(|record| target_exists(target, &record.target) && record.design_reference < 0);
    native.persistent_subentity_tags.retain(|record| {
        target_exists(target, &record.target)
            && record
                .design_references
                .iter()
                .all(|reference| *reference < 0)
    });
    // Removing references to omitted design records changes group positions,
    // which are layout identities rather than content.
    let mut link_ordinals = BTreeMap::new();
    for record in &mut native.persistent_design_links {
        let ordinal = link_ordinals.entry(record.target.clone()).or_insert(0);
        record.ordinal = *ordinal;
        *ordinal += 1;
    }
    let mut tag_ordinals = BTreeMap::new();
    for record in &mut native.persistent_subentity_tags {
        let ordinal = tag_ordinals.entry(record.target.clone()).or_insert(0);
        record.ordinal = *ordinal;
        *ordinal += 1;
    }
    super::act::reconcile(target, &mut native, losses)?;
    super::body_keys::reconcile(target, &mut native)?;
    super::geometry::reconcile(target, &mut native);
    super::design::regenerate(target, &mut native, losses)?;
    target.model.assembly_joints.clear();
    target.native.0.remove("f3d");
    super::design::with_context(|ctx| {
        native.store(ctx, target.native.namespace_mut("f3d"))?;
        Ok(())
    })
}

fn target_exists(ir: &CadIr, target: &cadmpeg_ir::attributes::AttributeTarget) -> bool {
    use cadmpeg_ir::attributes::AttributeTarget;
    match target {
        AttributeTarget::Body(id) => ir.model.bodies.iter().any(|record| &record.id == id),
        AttributeTarget::Face(id) => ir.model.faces.iter().any(|record| &record.id == id),
        AttributeTarget::Edge(id) => ir.model.edges.iter().any(|record| &record.id == id),
        AttributeTarget::Coedge(id) => ir.model.coedges.iter().any(|record| &record.id == id),
        AttributeTarget::Vertex(id) => ir.model.vertices.iter().any(|record| &record.id == id),
        _ => false,
    }
}

/// Rejoin independently admitted body candidates using one output key space.
pub(super) fn repair_keys(target: &mut CadIr) -> Result<(), CodecError> {
    let mut native = super::super::primitives::f3d_native(target)?.unwrap_or_default();
    super::body_keys::reconcile(target, &mut native)?;
    super::design::with_context(|ctx| {
        native.store(ctx, target.native.namespace_mut("f3d"))?;
        Ok(())
    })
}

pub(super) fn prune(target: &mut CadIr) -> Result<(), CodecError> {
    let mut native = super::super::primitives::f3d_native(target)?.unwrap_or_default();
    native.body_native_keys.retain(|record| {
        target
            .model
            .bodies
            .iter()
            .any(|body| body.id == record.body)
    });
    native.body_visibilities.retain(|record| {
        target
            .model
            .bodies
            .iter()
            .any(|body| body.id == record.body)
    });
    native
        .edge_continuities
        .retain(|record| target.model.edges.iter().any(|edge| edge.id == record.edge));
    native
        .face_sidedness
        .retain(|record| target.model.faces.iter().any(|face| face.id == record.face));
    native.creation_timestamps = target
        .model
        .attributes
        .iter()
        .enumerate()
        .filter_map(|(ordinal, attribute)| {
            let unix_microseconds = crate::brep::creation_timestamp_value(attribute)?;
            let existing = native
                .creation_timestamps
                .iter()
                .find(|record| record.target == attribute.target);
            Some(crate::records::recipes::CreationTimestamp {
                id: existing.map_or_else(
                    || format!("f3d:design:creation-timestamp#generated-{ordinal}"),
                    |record| record.id.clone(),
                ),
                target: attribute.target.clone(),
                record_index: existing
                    .map_or(u32::try_from(ordinal).ok()?, |record| record.record_index),
                unix_microseconds,
            })
        })
        .filter(|record| target_exists(target, &record.target))
        .collect();
    native
        .persistent_design_links
        .retain(|record| target_exists(target, &record.target));
    native
        .persistent_subentity_tags
        .retain(|record| target_exists(target, &record.target));
    super::geometry::reconcile(target, &mut native);
    target.native.0.remove("f3d");
    super::design::with_context(|ctx| {
        native.store(ctx, target.native.namespace_mut("f3d"))?;
        Ok(())
    })
}

pub(super) fn coverage(
    original: &CadIr,
    written: &CadIr,
    _losses: &mut Vec<LossNote>,
) -> Result<NativeDispositions, CodecError> {
    let mut result = NativeDispositions::for_namespace(
        "f3d",
        crate::native::F3D_ARENA_NAMES,
        NativeDisposition::Dropped,
    );
    if let Some(namespace) = original.native.namespace("f3d") {
        for (arena, before) in namespace.arenas() {
            if !WRITTEN.contains(&arena.as_str()) {
                continue;
            }
            let after = written
                .native
                .namespace("f3d")
                .and_then(|namespace| namespace.arenas().get(arena));
            if matches!(
                arena.as_str(),
                "design_parameters"
                    | "design_configurations"
                    | "act_tables"
                    | "act_types"
                    | "act_entities"
                    | "act_guids"
                    | "act_registry_channels"
                    | "act_root_components"
                    | "act_table_references"
            ) && after.is_some_and(|records| !records.is_empty())
            {
                result.set("f3d", arena, NativeDisposition::Regenerated);
                continue;
            }
            for record in before {
                if let Some(candidate) = after.and_then(|records| {
                    records
                        .iter()
                        .find(|candidate| candidate.id() == record.id())
                }) {
                    let disposition = if candidate == record {
                        NativeDisposition::Written
                    } else {
                        NativeDisposition::Regenerated
                    };
                    result.set_record("f3d", arena, record.id(), disposition);
                }
            }
        }
    }
    Ok(result)
}

/// (C) checks concern relationships inside the typed native document. A
/// generation limitation is handled later by omission; contradictory graph
/// identities are invalid before any neutral/native reconciliation.
fn validate_input(target: &CadIr, native: &F3dNative) -> Result<(), CodecError> {
    use super::super::generate::preconditions::validate_source_less_history_graph;
    let mut link_groups = BTreeMap::<_, Vec<u32>>::new();
    for link in &native.persistent_design_links {
        link_groups
            .entry(&link.target)
            .or_default()
            .push(link.ordinal);
    }
    let mut tag_groups = BTreeMap::<_, Vec<u32>>::new();
    for tag in &native.persistent_subentity_tags {
        tag_groups.entry(&tag.target).or_default().push(tag.ordinal);
    }
    for ordinals in link_groups.values_mut().chain(tag_groups.values_mut()) {
        ordinals.sort_unstable();
        if ordinals
            .iter()
            .enumerate()
            .any(|(i, ordinal)| usize::try_from(*ordinal) != Ok(i))
        {
            return Err(CodecError::InvalidInput(
                "F3D persistent design links and subentity tags require contiguous ordinals".into(),
            ));
        }
    }
    super::streams::validate(native)?;
    match validate_source_less_history_graph(target, native) {
        Err(error) if super::declines(&error) => Ok(()),
        other => other,
    }
}
