// SPDX-License-Identifier: Apache-2.0
//! Validation of act, configuration, design-intent, timeline, mesh, and body records.

use super::parameter_scopes::act_link_is_document_root;
use super::{
    design_stream, design_stream_contains_entry, native_edge_recipe_faces, push_constant_finding,
    Ctx,
};
use crate::{ids, records};
use cadmpeg_core::decode::id_from_index;
use cadmpeg_core::CodecError;
use cadmpeg_ir::report::check::{Check, Finding};
use cadmpeg_ir::report::Severity;
use std::collections::{BTreeMap, HashMap, HashSet};

/// Validate ACT record identity, table/group joins, ordered registries, and the
/// stored document-root discriminator.
pub(super) fn validate_act(
    ctx: &Ctx<'_, '_>,
    findings: &mut Vec<Finding>,
) -> Result<(), CodecError> {
    let native = ctx.native;
    let mut record_indices = HashSet::new();
    for entity in &native.act_entities {
        let stream = entity.stream();

        let unique_index = record_indices.insert((stream, entity.record_index()));
        if !unique_index {
            push_constant_finding(findings, Check::NativeLinks, "Fusion ACT entity has an invalid identity, table membership, or change-group frame", Some(entity.id().to_owned()));
        }
    }

    let mut guid_ordinals = std::collections::BTreeMap::<&str, (HashSet<u32>, &str)>::new();
    let mut guid_offsets = HashSet::new();
    for guid in &native.act_guids {
        let stream = guid.stream();

        guid_ordinals
            .entry(stream)
            .or_insert((HashSet::new(), guid.id().as_str()));
        let unique_ordinal = guid_ordinals
            .get_mut(stream)
            .ok_or_else(|| CodecError::malformed("F3D ACT GUID stream index missing"))?
            .0
            .insert(guid.ordinal);
        let unique_offset = guid_offsets.insert((stream, guid.byte_offset()));
        let valid = unique_offset && unique_ordinal;
        if !valid {
            push_constant_finding(
                findings,
                Check::NativeLinks,
                "Fusion ACT GUID-pool entry has an invalid identity, ordinal, offset, or GUID",
                Some(guid.id().to_owned()),
            );
        }
    }

    let mut table_reference_ordinals =
        std::collections::BTreeMap::<&str, (HashSet<u32>, &str)>::new();
    let mut table_reference_offsets = HashSet::new();
    for reference in &native.act_table_references {
        let stream = reference.stream();

        table_reference_ordinals
            .entry(stream)
            .or_insert((HashSet::new(), reference.id().as_str()));
        let unique_ordinal = table_reference_ordinals
            .get_mut(stream)
            .ok_or_else(|| CodecError::malformed("F3D ACT table stream index missing"))?
            .0
            .insert(reference.ordinal);
        let unique_offset = table_reference_offsets.insert((stream, reference.byte_offset()));
        let valid = unique_ordinal && unique_offset;
        if !valid {
            push_constant_finding(
                findings,
                Check::NativeLinks,
                "Fusion ACT table reference has an invalid identity, ordinal, or offset",
                Some(reference.id().to_owned()),
            );
        }
    }

    let mut registry_ordinals = std::collections::BTreeMap::<&str, (HashSet<u32>, &str)>::new();
    let mut registry_offsets = HashSet::new();
    let mut registry_names = HashSet::new();
    for channel in &native.act_registry_channels {
        let stream = channel.stream();

        registry_ordinals
            .entry(stream)
            .or_insert((HashSet::new(), channel.id().as_str()));
        let unique_ordinal = registry_ordinals
            .get_mut(stream)
            .ok_or_else(|| CodecError::malformed("F3D ACT registry stream index missing"))?
            .0
            .insert(channel.ordinal);
        let unique_offset = registry_offsets.insert((stream, channel.byte_offset()));
        let unique_name = registry_names.insert((stream, channel.name()));
        let valid = unique_offset && unique_name && unique_ordinal;
        if !valid {
            push_constant_finding(findings, Check::NativeLinks, "Fusion ACT channel-registry entry has an invalid identity, ordinal, offset, name, or GUID", Some(channel.id().to_owned()));
        }
    }

    let mut root_counts = BTreeMap::<&str, (&str, usize)>::new();
    for root in &native.act_component_links {
        let stream = root.stream();

        root_counts
            .entry(stream)
            .or_insert((root.id().as_str(), 0))
            .1 += usize::from(act_link_is_document_root(
            native,
            stream,
            root.references.tracked_entity,
        ));
        let unique_record_index = record_indices.insert((stream, root.record_index));
        if !unique_record_index {
            push_constant_finding(
                findings,
                Check::NativeLinks,
                "Fusion ACT component link repeats an ACT record index",
                Some(root.id().to_owned()),
            );
        }
    }

    if ctx.ir.fragment {
        findings.push(Finding {
 rule: cadmpeg_ir::rule_name!("f3d/design_records.skipped-f3d-act-document-root-f3d"),
            check: Check::FragmentSkipped, severity: Severity::Info, entity: None,
            related: Vec::new(),
            message: "skipped f3d.act_document_root and f3d.act_ordinal_coverage: complete ACT stream required".into(),
        });
        return Ok(());
    }
    for (witness, count) in root_counts.into_values() {
        if count != 1 {
            push_constant_finding(
                findings,
                Check::NativeLinks,
                "Fusion ACT stream does not have exactly one document-root component link",
                Some(witness.to_owned()),
            );
        }
    }
    for (ordinals, witness, family) in guid_ordinals
        .into_values()
        .map(|(ordinals, witness)| (ordinals, witness, "GUID pool"))
        .chain(
            table_reference_ordinals
                .into_values()
                .map(|(ordinals, witness)| (ordinals, witness, "table reference")),
        )
        .chain(
            registry_ordinals
                .into_values()
                .map(|(ordinals, witness)| (ordinals, witness, "channel registry")),
        )
    {
        let contiguous = id_from_index(ordinals.len()).is_some_and(|length| {
            ordinals
                .iter()
                .max()
                .and_then(|maximum| maximum.checked_add(1))
                == Some(length)
        });
        if !contiguous {
            let message = match family {
                "GUID pool" => "Fusion ACT GUID pool ordinals are not contiguous from zero",
                "table reference" => {
                    "Fusion ACT table reference ordinals are not contiguous from zero"
                }
                _ => "Fusion ACT channel registry ordinals are not contiguous from zero",
            };
            push_constant_finding(
                findings,
                Check::NativeLinks,
                message,
                Some(witness.to_owned()),
            );
        }
    }
    Ok(())
}

/// Validate entry names and authored table order in each member document.
pub(super) fn validate_configurations(ctx: &Ctx<'_, '_>, findings: &mut Vec<Finding>) {
    let mut entry_names = HashSet::new();
    let mut authored_tables = HashSet::new();
    for configuration in &ctx.native.design_configurations {
        let document = configuration.document_scope();
        let name = configuration.entry_name().as_str();
        if !entry_names.insert((document, name)) {
            push_constant_finding(
                findings,
                Check::NativeLinks,
                "Fusion Design configuration entry name is duplicated",
                Some(configuration.id()),
            );
        }
        if configuration
            .tables()
            .iter()
            .any(|table| !table.rows.is_empty())
            && !authored_tables.insert(document)
        {
            push_constant_finding(
                findings,
                Check::NativeLinks,
                "Fusion Design configurations have no single authored table order",
                Some(configuration.id()),
            );
        }
    }
}

/// Report a contained design-intent failure beside features or sketches of the
/// same document. A failure record turns off the checks that resolve design
/// intent, so it must not sit beside the records those checks read. An F3Z
/// member's failure is scoped to its occurrence; the other documents of the
/// archive keep their design records.
pub(super) fn validate_design_intent_failures(ctx: &Ctx, findings: &mut Vec<Finding>) {
    for failure in &ctx.native.design_intent_failures {
        let beside_design = ctx
            .ir
            .model
            .features
            .iter()
            .map(|feature| feature.id.as_str())
            .chain(
                ctx.ir
                    .model
                    .sketches
                    .iter()
                    .map(|sketch| sketch.id.as_str()),
            )
            .any(|id| crate::ids::same_native_occurrence(id, &failure.id));
        if beside_design {
            push_constant_finding(
                findings,
                Check::NativeLinks,
                "Fusion design-intent failure record accompanies decoded design records",
                Some(failure.id.clone()),
            );
        }
    }
}

/// Validate authored Design timeline order and its exact type and scope joins.
/// Scope history order is checked only in a document that holds the ASM
/// histories qualifying each history state.
pub(super) fn validate_feature_timelines(
    ctx: &Ctx,
    findings: &mut Vec<Finding>,
) -> Result<(), CodecError> {
    let native = ctx.native;
    let mut type_ordinals = HashMap::<&str, u32>::new();
    let mut timeline_ordinals = HashMap::<&str, u32>::new();
    let mut entity_type_counts = HashMap::<(&str, u64), usize>::new();
    let mut expected = BTreeMap::<(&str, u64), (String, u32, bool, &str)>::new();
    let mut design_types = native.design_types.iter().collect::<Vec<_>>();
    design_types.sort_by(|left, right| {
        (
            ids::native_stream(left.id()).unwrap_or_default(),
            left.byte_offset,
        )
            .cmp(&(
                ids::native_stream(right.id()).unwrap_or_default(),
                right.byte_offset,
            ))
    });
    for design_type in design_types {
        let Some(meta_stream) = ids::native_stream(design_type.id()) else {
            continue;
        };
        let Some(segment) = ids::design_segment(design_type.id()) else {
            continue;
        };

        let type_ordinal = type_ordinals.entry(meta_stream).or_default();
        let class_tag = type_ordinal.checked_add(256).map(|tag| tag.to_string());
        *type_ordinal = type_ordinal.checked_add(1).ok_or_else(|| {
            CodecError::Malformed("F3D feature timeline type ordinal overflows".into())
        })?;
        for entity_id in design_type.entities.values() {
            *entity_type_counts.entry((segment, *entity_id)).or_default() += 1;
        }
        if !design_type
            .type_guid
            .as_str()
            .eq_ignore_ascii_case(crate::design::decode::meta::FEATURE_TIMELINE_TYPE_GUID)
        {
            continue;
        }

        let source_ordinal = timeline_ordinals.entry(segment).or_default();
        for entity_id in design_type.entities.values() {
            let valid_type =
                crate::design::decode::meta::is_supported_feature_timeline_type(design_type)
                    && class_tag.as_ref().is_some_and(|tag| {
                        records::references::DesignClassTag::try_from(tag.clone()).is_ok()
                    });
            let Some(class_tag) = class_tag.clone() else {
                continue;
            };

            if expected
                .insert(
                    (segment, *entity_id),
                    (
                        class_tag,
                        *source_ordinal,
                        valid_type,
                        design_type.id().as_str(),
                    ),
                )
                .is_some()
            {
                push_constant_finding(
                    findings,
                    Check::NativeLinks,
                    "Fusion Design feature-timeline type repeats an entity identity",
                    Some(design_type.id().to_owned()),
                );
            }
            *source_ordinal = source_ordinal.checked_add(1).ok_or_else(|| {
                CodecError::Malformed("F3D feature timeline source ordinal overflows".into())
            })?;
        }
    }

    let mut actual = native.design_feature_timelines.iter().collect::<Vec<_>>();
    actual.sort_by(|left, right| {
        (left.segment(), left.source_ordinal).cmp(&(right.segment(), right.source_ordinal))
    });
    let mut actual_records = HashSet::<(&str, u64)>::new();
    let mut item_records = HashSet::<(&str, u64)>::new();
    for timeline in actual {
        let segment = timeline.segment();
        let expected_type = expected.get(&(segment, timeline.record_index.get()));
        let unique_record = actual_records.insert((segment, timeline.record_index.get()));
        let record_valid =
            expected_type.is_some_and(|(class_tag, source_ordinal, valid_type, _)| {
                *valid_type
                    && timeline.class_tag.as_str() == class_tag
                    && timeline.source_ordinal == *source_ordinal
            }) && entity_type_counts.get(&(segment, timeline.record_index.get())) == Some(&1)
                && entity_type_counts.get(&(segment, timeline.context_record_index.get()))
                    == Some(&1)
                && unique_record;
        let mut items_valid = true;
        for item in timeline.frame().items().iter().map(|item| item.value) {
            items_valid &= entity_type_counts.get(&(segment, item)) == Some(&1)
                && item_records.insert((segment, item));
        }
        if !record_valid || !items_valid {
            push_constant_finding(
                findings,
                Check::NativeLinks,
                "Fusion Design feature timeline has an invalid typed frame",
                Some(timeline.id().to_owned()),
            );
        }
    }
    for ((segment, entity_id), (_, _, _, type_id)) in &expected {
        if !actual_records.contains(&(*segment, *entity_id)) {
            push_constant_finding(
                findings,
                Check::NativeLinks,
                "Fusion Design feature-timeline type has no decoded record",
                Some((*type_id).to_owned()),
            );
        }
    }

    let mut scope_positions = HashMap::<&str, u64>::new();
    match crate::design::feature_project::scope_order::authored_scope_ordinals_per_stream(
        &native.design_parameter_scopes,
        &native.design_feature_timelines,
    ) {
        Ok(authored) => {
            for scope in &native.design_parameter_scopes {
                let stream = design_stream(&scope.id);
                let Some(position) = authored.get(&(stream, scope.record_index)) else {
                    continue;
                };

                scope_positions.insert(scope.id.as_str(), *position);
            }
        }
        Err(error @ CodecError::ResourceLimit(_)) => return Err(error),
        Err(_) => {
            let entity = native
                .design_parameter_scopes
                .first()
                .map(|scope| scope.id.clone());
            push_constant_finding(
                findings,
                Check::NativeLinks,
                "Fusion Design scopes have no complete authored order",
                entity,
            );
        }
    }

    if !ctx.holds_design_intent() {
        return Ok(());
    }
    let scope_history = crate::design::feature_project::scope_order::ScopeHistoryGraph::new(
        &native.design_parameter_scopes,
        &native.design_body_bindings,
        &native.design_body_recipe_operands,
        native_edge_recipe_faces(native),
        &native.design_component_naming_spaces,
        &native.asm_histories,
    );
    for scope in &native.design_parameter_scopes {
        let Some(position) = scope_positions.get(scope.id.as_str()).copied() else {
            continue;
        };
        match scope_history.predecessor(scope, |candidate| {
            scope_positions.contains_key(candidate.id.as_str())
        }) {
            Ok(crate::design::feature_project::scope_order::ScopeHistoryPredecessor::Scope(
                predecessor,
            )) => {
                if scope_positions
                    .get(predecessor.id.as_str())
                    .is_some_and(|predecessor| *predecessor >= position)
                {
                    push_constant_finding(
                        findings,
                        Check::NativeLinks,
                        "Fusion Design history edge runs forward in its feature timeline",
                        Some(scope.id.clone()),
                    );
                }
            }
            Err(error @ CodecError::ResourceLimit(_)) => return Err(error),
            Err(_) => push_constant_finding(
                findings,
                Check::NativeLinks,
                "Fusion Design scope history-state dependency is cyclic",
                Some(scope.id.clone()),
            ),
            Ok(
                crate::design::feature_project::scope_order::ScopeHistoryPredecessor::None
                | crate::design::feature_project::scope_order::ScopeHistoryPredecessor::Ambiguous,
            ) => {}
        }
    }
    Ok(())
}

/// Validate complete `Base Mesh Feature` record graphs and their neutral links.
/// The neutral feature link is checked only in a document that holds features.
pub(super) fn validate_mesh_features(ctx: &Ctx, findings: &mut Vec<Finding>) {
    let mut feature_ids = HashSet::new();
    let mut scope_records = HashSet::new();
    let mut collection_records = HashSet::new();
    let mut body_records = HashSet::new();
    let mut entry_records = HashMap::new();
    let mut guid_records = HashMap::new();
    let mut wrapper_records = HashSet::new();
    let mut state_records = HashSet::new();
    let mut node_records = HashSet::new();
    let mut auxiliary_records = HashSet::new();
    let mut collection_owner_records = HashSet::new();
    let mut body_owner_records = HashMap::new();
    let mut texture_table_records = HashSet::new();
    let mut filename_records = HashMap::new();
    let mut projected_tessellations = HashSet::new();
    let asset_ids = ctx
        .ir
        .model
        .assets
        .iter()
        .map(|asset| &asset.id)
        .collect::<std::collections::HashSet<_>>();
    let tessellation_ids = ctx
        .ir
        .model
        .tessellations
        .iter()
        .map(|tessellation| tessellation.id.as_str())
        .collect::<std::collections::HashSet<_>>();
    for feature in &ctx.native.design_mesh_features {
        let stream = design_stream(&feature.id);
        let mut valid = feature_ids.insert(feature.id.as_str())
            && collection_records.insert((stream, feature.collection().record().record_index()))
            && texture_table_records
                .insert((stream, feature.texture_table.record().record_index()))
            && collection_owner_records
                .insert((stream, feature.collection_owner.record().record_index()));
        for mesh_scope in feature.scopes() {
            let scope = ctx
                .scopes_by_index
                .get(&(stream, mesh_scope.record().record_index()));
            valid &= scope_records.insert((stream, mesh_scope.record().record_index()))
                && mesh_scope
                    .record()
                    .byte_offset()
                    .checked_add(scope.map_or(0, |scope| scope.frame_length()))
                    == Some(mesh_scope.base_record().byte_offset())
                && scope.is_some_and(|scope| {
                    scope.kind()
                        == crate::records::feature::scope::DesignFeatureKind::BaseMeshFeature
                        && scope.byte_offset() == mesh_scope.record().byte_offset()
                        && scope.paired_byte_offset() == mesh_scope.base_record().byte_offset()
                });
        }

        let mut resources = feature.texture_table.resources().iter().collect::<Vec<_>>();
        resources.sort_by_key(|left| left.filename_ordinal);
        let mut resources_valid = true;
        for resource in &resources {
            let filename_key = (stream, resource.file.record().record_index());
            let filename_record_consistent = filename_records
                .get(&filename_key)
                .is_none_or(|record| *record == resource.file.record());

            filename_records
                .entry(filename_key)
                .or_insert(resource.file.record());
            resources_valid = filename_record_consistent && asset_ids.contains(&resource.asset);
            if !resources_valid {
                break;
            }
        }
        valid &= resources_valid;

        for body in feature.all_bodies() {
            let owner_key = (stream, body.owner_record.record_index());
            let owner_consistent = body_owner_records
                .get(&owner_key)
                .is_none_or(|record| *record == &body.owner_record);

            body_owner_records
                .entry(owner_key)
                .or_insert(&body.owner_record);
            let entry_key = (stream, body.entry.record().record_index());
            let guid_key = (stream, body.guid.record().record_index());
            let entry_consistent = entry_records
                .get(&entry_key)
                .is_none_or(|entry| *entry == &body.entry);
            let guid_consistent = guid_records
                .get(&guid_key)
                .is_none_or(|guid| *guid == &body.guid);
            entry_records.entry(entry_key).or_insert(&body.entry);
            guid_records.entry(guid_key).or_insert(&body.guid);
            let body_valid = body_records.insert((stream, body.placement.record().record_index()))
                && entry_consistent
                && guid_consistent
                && wrapper_records.insert((stream, body.wrapper_record.record_index()))
                && state_records.insert((stream, body.scene_state.record().record_index()))
                && node_records.insert((stream, body.scene_node.record_index()))
                && auxiliary_records.insert((stream, body.scene_auxiliary_record.record_index()))
                && owner_consistent
                && body.scene_node.frame_length() == 133;
            let projection_valid = if body_valid {
                match body.tessellation_id.as_deref() {
                    Some(id) => tessellation_ids.contains(id) && projected_tessellations.insert(id),
                    None => true,
                }
            } else {
                false
            };
            valid &= body_valid && projection_valid;
        }
        for mesh_scope in feature.scopes() {
            let scope = ctx
                .scopes_by_index
                .get(&(stream, mesh_scope.record().record_index()));
            let mut projected = feature
                .bodies()
                .iter()
                .filter(|body| body.scope_record_index == Some(mesh_scope.record().record_index()))
                .collect::<Vec<_>>();
            projected.sort_by_key(|body| body.scope_body_ordinal);
            let projected = || {
                projected
                    .iter()
                    .filter_map(|body| body.tessellation_id.as_deref())
            };
            if ctx.holds_neutral_design() && projected().next().is_some() {
                valid &= scope.is_some_and(|scope| {
                    ctx.ir.model.features.iter().any(|neutral| {
                        neutral.native_ref.as_deref() == Some(scope.id.as_str())
                            && matches!(
                                neutral.evaluation.definition(),
                                cadmpeg_ir::features::FeatureDefinition::Operation(cadmpeg_ir::features::FeatureOperation::MeshImport { tessellations: cadmpeg_ir::features::mesh::MeshSelection::Tessellations(tessellations) })
                                    if tessellations.iter().map(cadmpeg_ir::tessellation::TessellationId::as_str).eq(projected())
                            )
                    })
                });
            }
        }
        if !valid {
            push_constant_finding(
                findings,
                Check::NativeLinks,
                "Fusion Design mesh feature has an invalid frame or object graph",
                Some(feature.id.clone()),
            );
        }
    }
}

/// Validate the ordered Design body-map binding entries and their pair runs.
pub(super) fn validate_body_bindings(
    ctx: &Ctx<'_, '_>,
    findings: &mut Vec<Finding>,
) -> Result<(), CodecError> {
    let native = ctx.native;
    let ownership_types = super::body_ownership::OwnerTypes::new(native, ctx.decode)?;
    let mut binding_offsets = HashSet::new();
    let mut binding_groups =
        std::collections::HashMap::<(&str, u64), Vec<&records::bodies::DesignBodyBinding>>::new();
    for binding in &native.design_body_bindings {
        let native_stream = design_stream(binding.id());
        let source_outside_fragment = ctx.ir.fragment
            && binding.body.is_some()
            && !native.body_native_keys.iter().any(|key| {
                ids::same_native_occurrence(key.source_namespace.as_str(), binding.id())
            });
        if source_outside_fragment {
            findings.push(Finding {
 rule: cadmpeg_ir::rule_name!("f3d/design_records.skipped-f3d-body-binding-source-resolution"), check: Check::FragmentSkipped, severity: Severity::Info,
                entity: Some(binding.id().to_owned()), related: Vec::new(),
                message: "skipped f3d.body_binding_source_resolution: source BREP keys are outside this fragment".into() });
        }
        let resolved_valid = if source_outside_fragment {
            true
        } else if let Some(body) = &binding.body {
            let has_named_source = native.body_native_keys.iter().any(|key| {
                ids::same_native_occurrence(key.source_namespace.as_str(), binding.id())
                    && key.source_brep.as_deref() == Some(binding.blob_name())
            });
            let source_keys = native.body_native_keys.iter().filter(|key| {
                ids::same_native_occurrence(key.source_namespace.as_str(), binding.id())
                    && if has_named_source {
                        key.source_brep.as_deref() == Some(binding.blob_name())
                    } else {
                        key.source_brep.is_none()
                    }
            });
            match crate::brep::resolve_body_selector(source_keys, binding.asm_body_key) {
                Ok(Some(resolved)) => resolved == body,
                Err(error @ CodecError::ResourceLimit(_)) => return Err(error),
                Ok(None) | Err(_) => false,
            }
        } else if binding.history_only {
            !native.body_native_keys.iter().any(|key| {
                ids::same_native_occurrence(key.source_namespace.as_str(), binding.id())
                    && key.source_brep.as_deref() == Some(binding.blob_name())
                    && key.asm_body_key == Some(binding.asm_body_key)
            })
        } else {
            true
        };
        let valid = design_stream_contains_entry(native_stream, binding.stream())
            && ownership_types.matches(binding)
            && resolved_valid
            && binding_offsets.insert((native_stream, binding.asm_body_key_offset()));
        if !valid {
            push_constant_finding(
                findings,
                Check::NativeLinks,
                "Fusion Design body binding has an invalid ordered map entry",
                Some(binding.id().to_owned()),
            );
        }
        binding_groups
            .entry((native_stream, binding.blob_name_offset()))
            .or_default()
            .push(binding);
    }
    for bindings in binding_groups.values_mut() {
        bindings.sort_by_key(|left| left.pair_ordinal());
        let complete = bindings
            .first()
            .is_some_and(|first| usize::try_from(first.pair_count()).ok() == Some(bindings.len()))
            && bindings.iter().enumerate().all(|(ordinal, binding)| {
                usize::try_from(binding.pair_ordinal()).ok() == Some(ordinal)
                    && binding.pair_count() == bindings[0].pair_count()
                    && binding.blob_name() == bindings[0].blob_name()
                    && binding.stream() == bindings[0].stream()
            });
        if !complete {
            push_constant_finding(
                findings,
                Check::NativeLinks,
                "Fusion Design body map has an incomplete ordered pair run",
                bindings.first().map(|binding| binding.id().to_owned()),
            );
        }
    }
    Ok(())
}

/// Validate each Design body-bounds repeated record frame.
pub(super) fn validate_body_bounds(ctx: &Ctx<'_, '_>, findings: &mut Vec<Finding>) {
    let native = ctx.native;
    let mut bounded_bodies = HashSet::new();
    for bounds in &native.design_body_bounds {
        let native_stream = design_stream(bounds.id());
        let mut expected_bindings = native
            .design_body_bindings
            .iter()
            .filter(|binding| {
                design_stream(binding.id()) == native_stream
                    && design_stream_contains_entry(native_stream, binding.stream())
                    && binding.entity_suffix == bounds.entity_suffix()
            })
            .collect::<Vec<_>>();
        expected_bindings.sort_by_key(|left| left.asm_body_key_offset());
        let valid_frame = ctx
            .entities_by_suffix
            .get(&(native_stream, bounds.entity_suffix()))
            .is_some_and(|entity| {
                entity.module() == Some(records::entity_header::DESIGN_MODULE_BODY)
                    && entity.byte_offset == bounds.entity_byte_offset()
            })
            && bounds.body_binding_ids().eq(expected_bindings
                .iter()
                .map(|binding| binding.id().as_str()));
        let valid = if valid_frame {
            bounded_bodies.insert((native_stream, bounds.entity_suffix()))
        } else {
            false
        };
        if !valid {
            push_constant_finding(
                findings,
                Check::NativeLinks,
                "Fusion Design body bounds have an invalid repeated record frame",
                Some(bounds.id().to_owned()),
            );
        }
    }
}
