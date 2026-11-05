// SPDX-License-Identifier: Apache-2.0
//! Select typed native content that has no neutral form; regenerate the rest.

use std::collections::BTreeMap;

use cadmpeg_core::CodecError;
use cadmpeg_ir::codec::write::{NativeDisposition, NativeDispositions};
use cadmpeg_ir::document::CadIr;
use cadmpeg_ir::report::loss::LossNote;

use crate::{loss::F3dLossCode, native::F3dNative};

fn sketch_owner_matches(
    target: &CadIr,
    placements: &[crate::records::sketch_placement::DesignSketchPlacement],
    entity: &cadmpeg_ir::sketches::SketchEntity,
    owner: u32,
    record_id: &str,
) -> bool {
    let Some(scope) = crate::ids::native_stream(record_id) else {
        return false;
    };
    target
        .model
        .sketches
        .iter()
        .find(|sketch| sketch.id == entity.sketch)
        .and_then(|sketch| sketch.native_ref.as_deref())
        .and_then(|id| placements.iter().find(|placement| placement.id == id))
        .is_some_and(|placement| {
            placement.entity_id.suffix() == u64::from(owner)
                && crate::ids::native_stream(&placement.id) == Some(scope)
        })
}

const WRITTEN: &[&str] = &[
    "act_tables",
    "act_types",
    "act_entities",
    "act_guids",
    "act_registry_channels",
    "act_component_links",
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
    "mesh_surface_sentinels",
    "design_parameters",
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
    "sketch_points",
    "sketch_curve_identities",
    "sketch_curve_links",
    "sketch_relations",
    "sketch_texts",
];

/// Rebuild ASM history chain links from the retained state order. A
/// coherent chain is left untouched; an edited chain is relinked instead of
/// refused. Node identities, owners and payloads are content and are kept.
fn relink_history_chains(histories: &mut [crate::history_records::AsmHistory]) {
    for history in histories {
        if history.states().is_empty() || crate::history::graph_is_coherent(history) {
            continue;
        }
        let node_indices: Vec<i64> = history
            .states()
            .iter()
            .map(|state| state.node_index)
            .collect();
        let mut previous: Option<i64> = None;
        for (position, state) in history.states_mut().iter_mut().enumerate() {
            state.previous_ref = previous;
            state.next_ref = node_indices.get(position + 1).copied();
            previous = Some(state.node_index);
        }
    }
}

pub(super) fn reconcile(
    target: &mut CadIr,
    mut original: F3dNative,
    losses: &mut Vec<LossNote>,
) -> Result<(), CodecError> {
    // W2: chain links are source layout, not content. Rebuild them from the
    // retained state order instead of refusing an edited chain.
    relink_history_chains(&mut original.asm_histories);
    validate_input(target, &original)?;
    // Parameter values have an independent generated stream. The native-only
    // metadata registry can therefore coexist with neutral parameter edits.
    let histories = original
        .asm_histories
        .iter()
        .filter(|history| {
            let nodes = history
                .states()
                .iter()
                .map(|state| state.node_index)
                .collect::<std::collections::BTreeSet<_>>();
            !history.binding_budget_exceeded
                && history.states().iter().all(|state| {
                    (state.owner_ref < 0 || nodes.contains(&state.owner_ref))
                        && state.partner_ref.is_none()
                        && (!state.bulletin_boards.is_empty()
                            || state.transition.as_ref().is_none_or(|transition| {
                                transition.records
                                == crate::history_records::AsmHistoricalEntityDelta::default()
                                && transition.topology
                                    == crate::history_records::AsmHistoricalTopologyDelta::default()
                                && transition.record_revisions.is_empty()
                                && transition.rows
                                    == crate::history_records::AsmHistoricalRowsDelta::default()
                            }))
                })
        })
        .take(1)
        .cloned()
        .map(|mut history| {
            for state in history.states_mut() {
                state.records.clear();
                state.bulletin_boards.clear();
                state.transition = None;
            }
            history
        })
        .collect();
    let mut native = F3dNative {
        act_tables: original.act_tables.clone(),
        act_types: original.act_types.clone(),
        act_entities: original.act_entities.clone(),
        act_guids: original.act_guids.clone(),
        act_registry_channels: original.act_registry_channels.clone(),
        act_component_links: original.act_component_links.clone(),
        act_table_references: original.act_table_references.clone(),
        asm_histories: histories,
        construction_recipes: original.construction_recipes.clone(),
        persistent_references: original.persistent_references.clone(),
        lost_edge_references: original.lost_edge_references.clone(),
        design_body_members: original.design_body_members.clone(),
        design_types: original.design_types.clone(),
        design_entity_headers: original.design_entity_headers.clone(),
        design_record_headers: original.design_record_headers.clone(),
        design_material_assignments: original.design_material_assignments.clone(),
        design_body_bindings: original.design_body_bindings.clone(),
        design_parameters: original.design_parameters.clone(),
        design_configurations: original.design_configurations.clone(),
        body_native_keys: original.body_native_keys,
        body_visibilities: original.body_visibilities,
        edge_continuities: original.edge_continuities,
        face_sidedness: original.face_sidedness,
        face_native_keys: original.face_native_keys,
        mesh_surface_sentinels: original.mesh_surface_sentinels,
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
        sketch_points: original.sketch_points.clone(),
        sketch_curve_identities: original.sketch_curve_identities.clone(),
        sketch_curve_links: original.sketch_curve_links.clone(),
        sketch_relations: original.sketch_relations.clone(),
        sketch_texts: original.sketch_texts.clone(),
        ..F3dNative::default()
    };
    native.mesh_surface_sentinels.retain(|sentinel| {
        target.model.surfaces.iter().any(|surface| {
            super::super::generate::preconditions::mesh_sentinel_matches(surface, sentinel)
        })
    });
    super::streams::reconcile(&mut native, losses);
    // Native namespace storage orders every arena by record id, so the
    // generated registry follows id order rather than the decoded file
    // order. Sort by the same key here, and every dynamic class tag the
    // writer remaps below names the position the type table actually holds.
    native
        .design_types
        .sort_by(|left, right| left.id().cmp(right.id()));
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
    // The sort above reorders the type table the retained dynamic class tags
    // name. Point every retained sketch record at the position of the type
    // that registers it; a record no remaining type registers is dropped
    // under W1 and reported through the native dispositions.
    macro_rules! remap_sketch_tags {
        ($records:expr) => {{
            let mut kept = Vec::new();
            for mut record in std::mem::take($records) {
                let tag = native
                    .design_types
                    .iter()
                    .position(|design_type| {
                        design_type
                            .entities
                            .values()
                            .any(|entity| *entity == u64::from(record.record_index))
                    })
                    .and_then(|ordinal| ordinal.checked_add(256))
                    .filter(|code| *code <= 999);
                match tag {
                    Some(code) => {
                        record.class_tag =
                            code.to_string().try_into().map_err(CodecError::Malformed)?;
                        kept.push(record);
                    }
                    None => {}
                }
            }
            *$records = kept;
        }};
    }
    remap_sketch_tags!(&mut native.sketch_points);
    remap_sketch_tags!(&mut native.sketch_curve_identities);
    remap_sketch_tags!(&mut native.sketch_relations);
    remap_sketch_tags!(&mut native.sketch_texts);
    for reference in &mut native.lost_edge_references {
        for (index, tag) in [
            (reference.record_index, &mut reference.class_tag),
            (reference.next_record_index, &mut reference.next_class_tag),
        ] {
            if let Some(code) = native
                .design_types
                .iter()
                .position(|kind| {
                    kind.entities
                        .values()
                        .any(|entity| *entity == u64::from(index))
                })
                .and_then(|ordinal| ordinal.checked_add(256))
                .filter(|code| *code <= 999)
            {
                *tag = code.to_string().try_into().map_err(CodecError::Malformed)?;
            }
        }
    }
    // Record headers annotate indexed slots in the generated registry.
    {
        let headers = std::mem::take(&mut native.design_record_headers);
        let mut kept = Vec::with_capacity(headers.len());
        for mut header in headers {
            let tag = native
                .design_types
                .iter()
                .position(|design_type| {
                    design_type
                        .entities
                        .values()
                        .any(|entity| *entity == u64::from(header.record_index))
                })
                .and_then(|ordinal| ordinal.checked_add(256))
                .filter(|code| *code <= 999);
            match tag {
                Some(code) => {
                    header.class_tag =
                        code.to_string().try_into().map_err(CodecError::Malformed)?;
                    kept.push(header);
                }
                None => losses.push(F3dLossCode::WriterNativeRecordsOmitted.note(format!(
                    "Design record header {} has no registered class and is omitted",
                    header.id
                ))),
            }
        }
        native.design_record_headers = kept;
    }
    // W1: a sketch point or curve without an owning sketch has no writable
    // record form. Drop it here so the graph validator never sees it; the
    // native dispositions report the drop. A dangling owner stays a
    // precondition failure for the validator to refuse.
    native
        .sketch_points
        .retain(|record| record.owner_reference.is_some());
    native.sketch_curve_identities.retain_mut(|record| {
        let Some(owner) = record.owner_reference else {
            return false;
        };
        let mut projected = target
            .model
            .sketch_entities
            .iter()
            .filter(|entity| entity.native_ref.as_deref() == Some(record.id.as_str()));
        let Some(entity) = projected.next() else {
            return true;
        };
        let was_nurbs = matches!(
            record.geometry,
            Some(crate::records::sketch_geometry::SketchCurveGeometry::Nurbs { .. })
        );
        let is_nurbs = matches!(
            entity.geometry.definition(),
            cadmpeg_ir::sketches::SketchGeometryDefinition::Nurbs { .. }
        );
        if !was_nurbs && !is_nurbs {
            return true;
        }
        if projected.next().is_some()
            || entity.geometry_ref.is_some()
            || !entity.endpoint_refs.is_empty()
            || !sketch_owner_matches(
                target,
                &original.design_sketch_placements,
                entity,
                owner,
                &record.id,
            )
        {
            return false;
        }
        let Some(authored) = super::super::generate::sketches::neutral_nurbs(entity) else {
            return false;
        };
        let Some(crate::records::sketch_geometry::SketchCurveGeometry::Nurbs {
            carrier_reference: None,
            subtype_class_tag,
            geometry,
            ..
        }) = &mut record.geometry
        else {
            return false;
        };
        let Some(subtype) = native
            .design_types
            .iter()
            .position(|kind| {
                kind.type_guid
                    .as_str()
                    .eq_ignore_ascii_case("EE518F9E-9CC3-4424-9CAB-2B92426E5312")
                    && kind.version == 4
                    && kind.module == "Geometry"
            })
            .and_then(|ordinal| ordinal.checked_add(256))
            .filter(|tag| *tag <= 999)
        else {
            return false;
        };
        let Ok(tag) = subtype.to_string().try_into() else {
            return false;
        };
        let Ok(updated) = crate::records::sketch_geometry::SketchNurbsGeometry::from_parts(
            authored.degree(),
            geometry.fit_tolerance().get(),
            8,
            authored.knots_copy(),
            authored.poles().clone(),
        ) else {
            return false;
        };
        *subtype_class_tag = tag;
        *geometry = updated;
        true
    });
    native.sketch_texts.retain_mut(|text| {
        let mut projected = target
            .model
            .sketch_entities
            .iter()
            .filter(|entity| entity.native_ref.as_deref() == Some(text.id.as_str()));
        if let Some(entity) = projected.next() {
            let owner_matches = sketch_owner_matches(
                target,
                &original.design_sketch_placements,
                entity,
                text.owner_reference,
                &text.id,
            );
            if projected.next().is_some()
                || entity.geometry_ref.is_some()
                || !entity.endpoint_refs.is_empty()
                || !owner_matches
            {
                return false;
            }
            let Some(authored) = super::super::generate::sketches::neutral_text(
                entity,
                text.record_index,
                text.owner_reference,
            ) else {
                return false;
            };
            text.text = authored.text;
            text.font_family = authored.font_family;
            text.font_weight = authored.font_weight;
            text.height = authored.height;
            text.layout = authored.layout;
        }
        super::super::generate::records::sketch_text_supported(text)
            && text
                .class_tag
                .dynamic_ordinal()
                .and_then(|ordinal| native.design_types.get(ordinal))
                .is_some_and(|kind| {
                    kind.type_guid
                        .as_str()
                        .eq_ignore_ascii_case("E0618268-3A06-450E-9E94-7CF4C2E66802")
                        && kind.version == text.class_version
                        && kind
                            .entities
                            .values()
                            .any(|id| *id == u64::from(text.record_index))
                })
    });
    // W1: a sketch-curve link the generator cannot consume has no writable
    // carrier. Only a coedge-owned link resolving to a carried coedge is
    // regenerated; anything else is dropped here and reported through the
    // native dispositions.
    native.sketch_curve_links.retain(|record| {
        record.target_coedge.is_some()
            && crate::brep::link_neutral_id(
                &target.model.bodies,
                &target.model.faces,
                &target.model.edges,
                &target.model.vertices,
                &target.model.coedges,
                &record.target,
                record.target_coedge,
            )
            .is_some_and(|id| {
                target
                    .model
                    .coedges
                    .iter()
                    .any(|coedge| coedge.id.as_str() == id)
            })
    });
    // W1: a header is omitted only with a sketch record that is actually
    // dropped. Retained sketch records keep their headers.
    let retained_sketch_records = native
        .sketch_points
        .iter()
        .map(|record| record.record_index)
        .chain(
            native
                .sketch_curve_identities
                .iter()
                .map(|record| record.record_index),
        )
        .chain(
            native
                .sketch_relations
                .iter()
                .map(|record| record.record_index),
        )
        .chain(native.sketch_texts.iter().map(|record| record.record_index))
        .collect::<std::collections::BTreeSet<_>>();
    let omitted_sketch_records = original
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
        .filter(|index| !retained_sketch_records.contains(index))
        .collect::<std::collections::BTreeSet<_>>();
    for header in &mut native.design_entity_headers {
        if let Some(references) = header.sketch_references_mut() {
            references
                .references
                .retain(|reference| !omitted_sketch_records.contains(&reference.value));
            if references
                .record_reference
                .is_some_and(|index| omitted_sketch_records.contains(&index))
            {
                references.record_reference = None;
                references.record_reference_offset = None;
            }
        }
    }
    native.design_entity_headers.retain(|header| {
        header.member_values().next().is_none()
            && !original
                .design_material_assignments
                .iter()
                .any(|assignment| assignment.entity_id == header.entity_id)
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
        .map(|header| header.entity_id.suffix())
        .collect::<std::collections::BTreeSet<_>>();
    // Design entity ids share one space across headers and sketch records.
    // Only remove an omitted entity no kept record references: a kept
    // header's suffix, a retained sketch record's index, or a sketch
    // companion's paired reference all select primary records in the written
    // bulkstream. Record headers carry no primary record, so their indices
    // never retain a registration.
    let needed_entities = native
        .design_entity_headers
        .iter()
        .map(|header| header.entity_id.suffix())
        .chain(
            retained_sketch_records
                .iter()
                .map(|index| u64::from(*index)),
        )
        .chain(
            native
                .sketch_points
                .iter()
                .map(|point| u64::from(point.paired_reference)),
        )
        .collect::<std::collections::BTreeSet<_>>();
    for record in &mut native.design_types {
        let entities = record
            .entities
            .values()
            .copied()
            .filter(|id| !omitted_header_entities.contains(id) || needed_entities.contains(id))
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
    // These native lanes select identities or add native-only display facts.
    // Geometry and visibility values are always authored from the neutral model.
    super::design::reconcile_body_tags(target, &mut native);
    super::body_keys::reconcile(target, &mut native)?;
    super::materials::author_neutral(target, &mut native, losses)?;
    native.design_material_assignments.retain_mut(|assignment| {
        let Some(binding) = target.model.appearance_bindings.iter().find(|binding| {
            binding.source_entity_id.as_deref() == Some(assignment.entity_id.as_str())
        }) else {
            return false;
        };
        let cadmpeg_ir::appearance::AppearanceTarget::Body(body_id) = &binding.target else {
            return false;
        };
        let Some(key) = target
            .model
            .bodies
            .iter()
            .find(|body| &body.id == body_id)
            .and_then(|body| body.native_ref.as_deref())
            .and_then(|id| native.body_native_keys.iter().find(|key| key.id() == id))
            .and_then(|key| key.asm_body_key)
        else {
            return false;
        };
        assignment.asm_body_key = key;
        true
    });
    native.body_native_keys.retain(|record| {
        target
            .model
            .bodies
            .iter()
            .any(|body| body.native_ref.as_deref() == Some(record.id().as_str()))
    });
    native.body_visibilities.retain(|record| {
        target
            .model
            .bodies
            .iter()
            .any(|body| body.native_ref.as_deref() == Some(record.body.as_str()))
    });
    native.edge_continuities.retain(|record| {
        target
            .model
            .edges
            .iter()
            .any(|edge| edge.native_ref.as_deref() == Some(record.owner.as_str()))
    });
    native.face_sidedness.retain(|record| {
        target.model.faces.iter().any(|face| {
            record.owns_native_ref(face.native_ref.as_deref(), &native.face_native_keys)
        })
    });
    let retained_timestamps = std::mem::take(&mut native.creation_timestamps);
    native.creation_timestamps = target
        .model
        .attributes
        .iter()
        .enumerate()
        .filter_map(|(ordinal, attribute)| {
            let unix_microseconds = crate::brep::creation_timestamp_value(attribute)?;
            let (row, target_coedge) = crate::brep::link_target_for(
                &target.model.bodies,
                &target.model.faces,
                &target.model.edges,
                &target.model.vertices,
                &target.model.coedges,
                &attribute.target,
            )?;
            let row = cadmpeg_asm::brep::records::identity::NativeRowId::from(row);
            let existing = retained_timestamps
                .iter()
                .find(|record| record.target == row && record.target_coedge == target_coedge);
            Some(crate::records::recipes::CreationTimestamp {
                id: existing.map_or_else(
                    || format!("f3d:design:creation-timestamp#generated-{ordinal}"),
                    |record| record.id.clone(),
                ),
                target: row,
                target_coedge,
                record_index: existing
                    .map_or(u32::try_from(ordinal).ok()?, |record| record.record_index),
                unix_microseconds,
            })
        })
        .collect();
    // A retained record shadowed by a neutral-derived timestamp is a copy
    // of document content; an exact duplicate of another retained record
    // is corrupt input for the generator to refuse. Shadow against the
    // derived prefix only.
    let derived = native.creation_timestamps.len();
    // W1: re-emit each retained timestamp whose target resolves in the
    // written model. A timestamp whose target does not resolve is dropped;
    // the export wrapper charges the `export:native.records-omitted`
    // cascade loss naming it.
    for record in retained_timestamps {
        let shadowed = native.creation_timestamps[..derived].iter().any(|derived| {
            derived.target == record.target && derived.target_coedge == record.target_coedge
        });
        if shadowed {
            continue;
        }
        let resolves = crate::brep::link_neutral_id(
            &target.model.bodies,
            &target.model.faces,
            &target.model.edges,
            &target.model.vertices,
            &target.model.coedges,
            &record.target,
            record.target_coedge,
        )
        .is_some();
        if resolves {
            native.creation_timestamps.push(record);
        }
    }
    native.persistent_design_links.retain(|record| {
        crate::brep::link_neutral_id(
            &target.model.bodies,
            &target.model.faces,
            &target.model.edges,
            &target.model.vertices,
            &target.model.coedges,
            &record.target,
            record.target_coedge,
        )
        .is_some()
    });
    native.persistent_subentity_tags.retain(|record| {
        crate::brep::link_neutral_id(
            &target.model.bodies,
            &target.model.faces,
            &target.model.edges,
            &target.model.vertices,
            &target.model.coedges,
            &record.target,
            record.target_coedge,
        )
        .is_some()
    });
    // Removing references to omitted design records changes group positions,
    // which are layout identities rather than content.
    let mut link_ordinals = BTreeMap::new();
    for record in &mut native.persistent_design_links {
        let ordinal = link_ordinals
            .entry((record.target.clone(), record.target_coedge))
            .or_insert(0);
        record.ordinal = *ordinal;
        *ordinal += 1;
    }
    let mut tag_ordinals = BTreeMap::new();
    for record in &mut native.persistent_subentity_tags {
        let ordinal = tag_ordinals
            .entry((record.target.clone(), record.target_coedge))
            .or_insert(0);
        record.ordinal = *ordinal;
        *ordinal += 1;
    }
    super::act::reconcile(target, &mut native, losses)?;
    super::geometry::reconcile(target, &mut native);
    super::design::regenerate(target, &mut native, losses)?;
    target.model.assembly_joints.clear();
    target.native.0.remove("f3d");
    super::design::with_context(|ctx| {
        native.store(ctx, target.native.namespace_mut("f3d"))?;
        Ok(())
    })
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

/// Terminal §5.0 reduction: shed every native record the Design generator
/// reads, keeping BREP-anchored carriers and feature-owned neutral parameters.
/// Every omission is reported through the export dispositions.
pub(super) fn shed_design(target: &mut CadIr) -> Result<(), CodecError> {
    let native = super::super::primitives::f3d_native(target)?.unwrap_or_default();
    let kept = F3dNative {
        body_native_keys: native.body_native_keys,
        body_visibilities: native.body_visibilities,
        edge_continuities: native.edge_continuities,
        edge_ownerships: native.edge_ownerships,
        face_native_keys: native.face_native_keys,
        face_sidedness: native.face_sidedness,
        vertex_ownerships: native.vertex_ownerships,
        wire_topologies: native.wire_topologies,
        transform_hints: native.transform_hints,
        tolerant_edge_tails: native.tolerant_edge_tails,
        tolerant_vertex_tails: native.tolerant_vertex_tails,
        tolerant_coedge_parameters: native.tolerant_coedge_parameters,
        creation_timestamps: native.creation_timestamps,
        persistent_design_links: native.persistent_design_links,
        persistent_subentity_tags: native.persistent_subentity_tags,
        persistent_references: native.persistent_references,
        lost_edge_references: native.lost_edge_references,
        ..F3dNative::default()
    };
    target
        .model
        .parameters
        .retain(|parameter| parameter.owner.is_some());
    target.model.configurations.clear();
    target.model.assembly_joints.clear();
    target.native.0.remove("f3d");
    super::design::with_context(|ctx| {
        kept.store(ctx, target.native.namespace_mut("f3d"))?;
        Ok(())
    })
}

pub(super) fn prune(target: &mut CadIr) -> Result<(), CodecError> {
    let mut native = super::super::primitives::f3d_native(target)?.unwrap_or_default();
    native.mesh_surface_sentinels.retain(|sentinel| {
        target.model.surfaces.iter().any(|surface| {
            super::super::generate::preconditions::mesh_sentinel_matches(surface, sentinel)
        })
    });
    native.body_native_keys.retain(|record| {
        target
            .model
            .bodies
            .iter()
            .any(|body| body.native_ref.as_deref() == Some(record.id().as_str()))
    });
    native.body_visibilities.retain(|record| {
        target
            .model
            .bodies
            .iter()
            .any(|body| body.native_ref.as_deref() == Some(record.body.as_str()))
    });
    native.edge_continuities.retain(|record| {
        target
            .model
            .edges
            .iter()
            .any(|edge| edge.native_ref.as_deref() == Some(record.owner.as_str()))
    });
    native.face_sidedness.retain(|record| {
        target.model.faces.iter().any(|face| {
            record.owns_native_ref(face.native_ref.as_deref(), &native.face_native_keys)
        })
    });
    let retained_timestamps = std::mem::take(&mut native.creation_timestamps);
    native.creation_timestamps = target
        .model
        .attributes
        .iter()
        .enumerate()
        .filter_map(|(ordinal, attribute)| {
            let unix_microseconds = crate::brep::creation_timestamp_value(attribute)?;
            let (row, target_coedge) = crate::brep::link_target_for(
                &target.model.bodies,
                &target.model.faces,
                &target.model.edges,
                &target.model.vertices,
                &target.model.coedges,
                &attribute.target,
            )?;
            let row = cadmpeg_asm::brep::records::identity::NativeRowId::from(row);
            let existing = retained_timestamps
                .iter()
                .find(|record| record.target == row && record.target_coedge == target_coedge);
            Some(crate::records::recipes::CreationTimestamp {
                id: existing.map_or_else(
                    || format!("f3d:design:creation-timestamp#generated-{ordinal}"),
                    |record| record.id.clone(),
                ),
                target: row,
                target_coedge,
                record_index: existing
                    .map_or(u32::try_from(ordinal).ok()?, |record| record.record_index),
                unix_microseconds,
            })
        })
        .collect();
    // A retained record shadowed by a neutral-derived timestamp is a copy
    // of document content; an exact duplicate of another retained record
    // is corrupt input for the generator to refuse. Shadow against the
    // derived prefix only.
    let derived = native.creation_timestamps.len();
    // W1: re-emit each retained timestamp whose target resolves in the
    // written model. A timestamp whose target does not resolve is dropped;
    // the export wrapper charges the `export:native.records-omitted`
    // cascade loss naming it.
    for record in retained_timestamps {
        let shadowed = native.creation_timestamps[..derived].iter().any(|derived| {
            derived.target == record.target && derived.target_coedge == record.target_coedge
        });
        if shadowed {
            continue;
        }
        let resolves = crate::brep::link_neutral_id(
            &target.model.bodies,
            &target.model.faces,
            &target.model.edges,
            &target.model.vertices,
            &target.model.coedges,
            &record.target,
            record.target_coedge,
        )
        .is_some();
        if resolves {
            native.creation_timestamps.push(record);
        }
    }
    native.persistent_design_links.retain(|record| {
        crate::brep::link_neutral_id(
            &target.model.bodies,
            &target.model.faces,
            &target.model.edges,
            &target.model.vertices,
            &target.model.coedges,
            &record.target,
            record.target_coedge,
        )
        .is_some()
    });
    native.persistent_subentity_tags.retain(|record| {
        crate::brep::link_neutral_id(
            &target.model.bodies,
            &target.model.faces,
            &target.model.edges,
            &target.model.vertices,
            &target.model.coedges,
            &record.target,
            record.target_coedge,
        )
        .is_some()
    });
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
    invalidated_timestamps: &std::collections::BTreeSet<String>,
    regenerated_parameters: &std::collections::BTreeSet<String>,
) -> NativeDispositions {
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
                "act_tables"
                    | "act_types"
                    | "act_entities"
                    | "act_guids"
                    | "act_registry_channels"
                    | "act_component_links"
                    | "act_table_references"
            ) && after.is_some_and(|records| !records.is_empty())
            {
                result.set("f3d", arena, NativeDisposition::Regenerated);
                continue;
            }
            for record in before {
                let regenerated_document_parameter = arena.as_str() == "design_parameters"
                    && original.model.parameters.iter().any(|parameter| {
                        parameter.owner.is_none()
                            && parameter.native_ref.as_deref() == Some(record.id())
                            && original
                                .model
                                .parameters
                                .iter()
                                .filter(|candidate| {
                                    candidate.owner.is_none()
                                        && candidate.name == parameter.name
                                        && candidate.expression == parameter.expression
                                        && candidate.value == parameter.value
                                        && candidate.driving == parameter.driving
                                })
                                .count()
                                == 1
                            && written
                                .model
                                .parameters
                                .iter()
                                .filter(|candidate| {
                                    candidate.owner.is_none()
                                        && candidate.name == parameter.name
                                        && candidate.expression == parameter.expression
                                        && candidate.value == parameter.value
                                        && candidate.driving == parameter.driving
                                })
                                .count()
                                == 1
                    });
                if arena.as_str() == "design_parameters"
                    && (regenerated_parameters.contains(record.id())
                        || regenerated_document_parameter)
                {
                    result.set_record("f3d", arena, record.id(), NativeDisposition::Regenerated);
                    continue;
                }
                if arena.as_str() == "creation_timestamps"
                    && invalidated_timestamps.contains(record.id())
                {
                    result.set_record("f3d", arena, record.id(), NativeDisposition::Dropped);
                    continue;
                }
                if let Some(candidate) = after.and_then(|records| {
                    records
                        .iter()
                        .find(|candidate| candidate.id() == record.id())
                }) {
                    let disposition = if arena.as_str() == "mesh_surface_sentinels" {
                        NativeDisposition::Regenerated
                    } else if candidate == record {
                        NativeDisposition::Written
                    } else {
                        NativeDisposition::Regenerated
                    };
                    result.set_record("f3d", arena, record.id(), disposition);
                } else {
                    // A record with no carried copy is an explicit drop, not
                    // the arena default: the arena declaration below can
                    // state Written while this record stays Dropped.
                    result.set_record("f3d", arena, record.id(), NativeDisposition::Dropped);
                }
            }
            // W3: the arena declaration states what the writer did with the
            // arena. Any regenerated record regenerates the arena; a carried
            // record writes it; an arena with no carried record is dropped.
            // The per-record exceptions and the omission losses name the
            // dropped records.
            let mut written = false;
            let mut regenerated = false;
            for record in before {
                match result.record_disposition("f3d", arena, record.id()) {
                    NativeDisposition::Written => written = true,
                    NativeDisposition::Regenerated => regenerated = true,
                    NativeDisposition::Dropped => {}
                }
            }
            if regenerated || written {
                if let Some(space) = result.namespaces.get_mut("f3d") {
                    if let Some(entry) = space.arenas.get_mut(arena) {
                        entry.disposition = if regenerated {
                            NativeDisposition::Regenerated
                        } else {
                            NativeDisposition::Written
                        };
                    }
                }
            }
        }
    }
    result
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
