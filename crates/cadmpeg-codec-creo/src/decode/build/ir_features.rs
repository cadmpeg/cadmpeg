// SPDX-License-Identifier: Apache-2.0
//! Feature-tree emission, dimension parameters, and expression coverage.

use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};

use cadmpeg_ir::document::CadIr;
use cadmpeg_ir::features::{
    DistinctMembers, Feature, FeatureDefinition as IrFeatureDefinition, FeatureId as IrFeatureId,
    FeatureOperation as IrFeatureOperation, UnresolvedFamily,
};
use cadmpeg_ir::AnnotationBuilder;
use cadmpeg_ir::Exactness;

use crate::container::ContainerScan;
use crate::feature::schema::SchemaClass;

use super::super::curve_expressions::transfer_curve_expression_features;
use super::super::feature_history::axes::geometry_generator_features;
use super::super::feature_history::dependencies::{
    feature_dependencies, reconcile_feature_links, surface_prototype_feature_dependencies,
};
use super::super::feature_history::dimensions::transfer_feature_dimensions;
use super::super::feature_history::draft::{
    datum_plane_feature_definition, schema_feature_definition, unbounded_feature_plane_definition,
};
use super::super::feature_history::knit::emit_feature_result_topologies;
use super::super::feature_history::link::link_feature_sketch_history;
use super::super::feature_history::named::{
    named_feature_definition, named_or_referenced_feature_definition,
    retain_native_feature_parameters,
};
use super::super::feature_history::outputs::{
    copy_body_id, decoded_feature_reference_name, feature_output_bodies, feature_parameters,
    feature_reference_name, feature_source_properties, insert_feature_source_property,
    schema_operation_kind, SchemaClassList,
};
use super::super::native::annotate;
use super::super::sketch_ids::owning_feature_definition_ref;
use crate::decode::sketch_transfer::constraints::close_sketch_constraint_parameter_references;
use crate::decode::sketch_transfer::recipe::{
    current_feature_operation, current_feature_recipe, current_feature_recipe_parent,
    feature_schema_class, row_feature_schema_classes,
};

fn compose_feature_id<'a>(
    ctx: &'a cadmpeg_core::decode::DecodeContext<'_>,
    feature_id: u32,
) -> Result<(IrFeatureId, cadmpeg_core::decode::ScopedReservation<'a>), cadmpeg_core::CodecError> {
    let namespace = &crate::identity::MODEL_FEATURE;
    let (text, reservation) = ctx.format_scoped(
        format_args!(
            "{}:{}:{}#{feature_id}",
            namespace.format(),
            namespace.scope(),
            namespace.kind()
        ),
        "creo model feature identity",
    )?;
    let id = IrFeatureId::mint(text).map_err(cadmpeg_core::CodecError::malformed)?;
    Ok((id, reservation))
}

fn append_regeneration_edge(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    edges: &mut Vec<(IrFeatureId, IrFeatureId)>,
    child: &IrFeatureId,
    parent: &IrFeatureId,
) -> Result<(), cadmpeg_core::CodecError> {
    ctx.reserve_vec(edges, 1, "creo regeneration edges")?;
    let child = child.try_clone_for_decode(ctx, "creo regeneration child identity")?;
    let parent = parent.try_clone_for_decode(ctx, "creo regeneration parent identity")?;
    edges.push((child, parent));
    Ok(())
}

fn commit_regeneration_edges(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    ir: &mut CadIr,
    edges: &[(IrFeatureId, IrFeatureId)],
) -> Result<(), cadmpeg_core::CodecError> {
    for (child, parent) in ctx.admit_iter(edges, "creo regeneration edge traversal")? {
        ir.model
            .set_feature_regeneration_parent(ctx, child, parent)?;
    }
    Ok(())
}

/// Accept only the fixed model-feature namespace and canonical u32 digits.
/// The prefix has fixed width and the suffix is bounded before parsing.
fn model_feature_number(identity: &str) -> Option<u32> {
    let digits = identity.strip_prefix("creo:model:feature#")?;
    if digits.len() > 10 {
        return None;
    }
    let number = digits.parse::<u32>().ok()?;
    crate::identity::matches_numbered_identity(identity, "creo:model:feature#", number)
        .then_some(number)
}

fn refresh_feature_outputs(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    scan: &ContainerScan,
    ir: &mut CadIr,
) -> Result<(), cadmpeg_core::CodecError> {
    let mut update_storage = ctx.reserve_scoped(0, "Creo feature output update storage")?;
    let mut output_updates = Vec::new();
    for (index, feature) in ctx
        .admit_iter(&ir.model.features, "creo feature output refresh traversal")?
        .enumerate()
    {
        let Some(digits) = feature.id.as_str().strip_prefix("creo:model:feature#") else {
            continue;
        };
        let parsed = if digits.len() <= 10 {
            digits.parse::<u32>()
        } else {
            ctx.parse_text::<u32>(digits, "creo model feature ID parsing")?
        };
        let Ok(feature_id) = parsed else {
            continue;
        };
        let outputs = cadmpeg_ir::features::DistinctMembers::try_from(
            feature_output_bodies(ctx, scan, ir, feature_id)?,
            ctx,
        )
        .map_err(cadmpeg_core::CodecError::from)?;
        update_storage.with_storage(|| {
            ctx.reserve_vec(&mut output_updates, 1, "creo feature output update rows")
        })?;
        output_updates.push((index, outputs));
    }
    for (index, outputs) in
        ctx.admit_iter(output_updates, "creo feature output update traversal")?
    {
        ir.model.features[index].evaluation.set_outputs(outputs);
    }
    Ok(())
}

#[derive(Debug)]
struct FeatureRowIndex {
    order: Vec<u32>,
    offsets: HashMap<u32, usize>,
}

fn ordered_row_feature_ids(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    rows: &[crate::feature::rows::FeatureRow],
) -> Result<FeatureRowIndex, cadmpeg_core::CodecError> {
    let mut index = FeatureRowIndex {
        order: Vec::new(),
        offsets: HashMap::new(),
    };
    for row in ctx.admit_iter(rows, "creo ordered feature row traversal")? {
        match ctx.entry_hash_map(
            &mut index.offsets,
            row.feature_id,
            "creo feature row identity nodes",
        )? {
            std::collections::hash_map::Entry::Vacant(entry) => {
                entry.insert(row.offset);
                ctx.push_vec(&mut index.order, row.feature_id, "creo feature row IDs")?;
            }
            std::collections::hash_map::Entry::Occupied(mut entry) => {
                *entry.get_mut() = (*entry.get()).min(row.offset);
            }
        }
    }
    Ok(index)
}

fn merge_feature_source_properties(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    target: &mut BTreeMap<cadmpeg_core::text::NonBlankString, String>,
    incoming: BTreeMap<cadmpeg_core::text::NonBlankString, String>,
) -> Result<(), cadmpeg_core::CodecError> {
    for (key, value) in ctx.admit_iter(incoming, "creo feature source property merge traversal")? {
        ctx.insert_btree_map(target, key, value, "creo IR Feature source property nodes")?;
    }
    Ok(())
}

fn merge_feature_dependencies(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    target: &mut DistinctMembers<IrFeatureId>,
    incoming: Vec<IrFeatureId>,
) -> Result<(), cadmpeg_core::CodecError> {
    for dependency in ctx.admit_iter(incoming, "creo feature dependency merge traversal")? {
        target.insert(ctx, dependency, "creo IR Feature dependency members")?;
    }
    Ok(())
}

pub(super) fn emit_model_features(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    scan: &ContainerScan,
    ir: &mut CadIr,
    annotations: &mut AnnotationBuilder,
    source_carriers: &crate::decode::source_carriers::SourceUnitCarriers,
) -> Result<usize, cadmpeg_core::CodecError> {
    let mut lookup_storage = ctx.reserve_scoped(0, "Creo feature emission lookup storage")?;
    let mut model_features = HashMap::<u32, usize>::new();
    for (index, feature) in ctx
        .admit_iter(&ir.model.features, "creo model feature index traversal")?
        .enumerate()
    {
        if let Some(feature_id) = model_feature_number(feature.id.as_str()) {
            lookup_storage.with_storage(|| {
                ctx.entry_hash_map(
                    &mut model_features,
                    feature_id,
                    "creo model feature index nodes",
                )?
                .or_insert(index);
                Ok::<_, cadmpeg_core::CodecError>(())
            })?;
        }
    }
    let mut datum_unique = HashMap::<u32, bool>::new();
    for datum in ctx.admit_iter(
        &scan.planes.datums,
        "creo datum feature uniqueness traversal",
    )? {
        lookup_storage.with_storage(|| {
            match ctx.entry_hash_map(
                &mut datum_unique,
                datum.feature_id,
                "creo datum feature uniqueness nodes",
            )? {
                std::collections::hash_map::Entry::Vacant(entry) => {
                    entry.insert(true);
                }
                std::collections::hash_map::Entry::Occupied(mut entry) => {
                    *entry.get_mut() = false;
                }
            }
            Ok::<_, cadmpeg_core::CodecError>(())
        })?;
    }
    let mut legacy_rounds = HashSet::new();
    for round in ctx.admit_iter(
        &scan.features.legacy_rounds,
        "creo legacy round feature index traversal",
    )? {
        lookup_storage.with_storage(|| {
            ctx.insert_hash_set(
                &mut legacy_rounds,
                round.feature_id,
                "creo legacy round feature index nodes",
            )
        })?;
    }
    let mut regeneration_edges = Vec::new();
    let prototype_feature_dependencies =
        lookup_storage.with_storage(|| surface_prototype_feature_dependencies(ctx, scan))?;
    let mut operation_feature_ids = BTreeSet::new();
    for operation in ctx.admit_iter(
        &scan.features.operations,
        "creo operation feature ID traversal",
    )? {
        lookup_storage.with_storage(|| {
            ctx.insert_btree_set(
                &mut operation_feature_ids,
                operation.feature_id,
                "creo operation feature identity nodes",
            )
        })?;
    }
    for datum in ctx.admit_iter(&scan.planes.datums, "creo datum feature traversal")? {
        if ctx.contains_btree_set(
            &operation_feature_ids,
            &datum.feature_id,
            "creo operation feature ID lookup",
        )? {
            continue;
        }
        if model_features.contains_key(&datum.feature_id) {
            continue;
        }
        let id_parts = compose_feature_id(ctx, datum.feature_id)?;
        let id_bytes = id_parts.1;
        let id = id_parts.0;
        annotate(
            ctx,
            annotations,
            &id,
            "ActDatums",
            cadmpeg_core::decode::u64_from_index(datum.offset_in_payload),
            "datum_plane_feature",
            Exactness::Derived,
        )?;
        ctx.charge_entities(1, "admit Creo model features")?;
        let id = id_bytes.commit_value(id)?;
        let feature = Feature {
            id,
            ordinal: cadmpeg_core::decode::u64_from_index(ir.model.features.len()),
            name: None,
            suppressed: Some(false),
            dependencies: cadmpeg_ir::features::DistinctMembers::default(),
            source_properties: BTreeMap::new(),
            source_tag: None,
            source_text: None,
            source_content: cadmpeg_ir::features::FeatureContent::default(),

            evaluation: cadmpeg_ir::features::FeatureEvaluation::from_definition(
                if datum_unique.get(&datum.feature_id) == Some(&true) {
                    datum_plane_feature_definition(&datum.plane())
                } else {
                    IrFeatureDefinition::Operation(IrFeatureOperation::Unresolved {
                        family: UnresolvedFamily::DatumPlane,
                    })
                },
            ),
            native_ref: None,
        };
        source_carriers.admit_feature(ctx, ir, feature)?;
        lookup_storage.with_storage(|| {
            ctx.insert_hash_map(
                &mut model_features,
                datum.feature_id,
                ir.model.features.len() - 1,
                "creo model feature index nodes",
            )
        })?;
    }
    let row_feature_ids =
        lookup_storage.with_storage(|| ordered_row_feature_ids(ctx, &scan.features.rows))?;
    let mut geometry_generator_feature_count = 0;
    let generators = lookup_storage.with_storage(|| geometry_generator_features(ctx, scan))?;
    for generator in ctx.admit_iter(&generators, "creo geometry generator feature traversal")? {
        let feature_id = generator.feature_id;
        if model_features.contains_key(&feature_id) {
            continue;
        }
        let id_parts = compose_feature_id(ctx, feature_id)?;
        let id_bytes = id_parts.1;
        let id = id_parts.0;
        annotate(
            ctx,
            annotations,
            &id,
            "VisibGeom",
            cadmpeg_core::decode::u64_from_index(generator.offset),
            "geometry_generator_feature",
            Exactness::ByteExact,
        )?;
        ctx.charge_entities(1, "admit Creo model features")?;
        let id = id_bytes.commit_value(id)?;
        let feature = Feature {
            id,
            ordinal: cadmpeg_core::decode::u64_from_index(ir.model.features.len()),
            name: None,
            suppressed: Some(false),
            dependencies: cadmpeg_ir::features::DistinctMembers::default(),
            source_properties: BTreeMap::new(),
            source_tag: None,
            source_text: None,
            source_content: cadmpeg_ir::features::FeatureContent::default(),

            evaluation: cadmpeg_ir::features::FeatureEvaluation::new(
                if legacy_rounds.contains(&feature_id) {
                    schema_feature_definition(
                        ctx,
                        scan,
                        ir,
                        source_carriers,
                        feature_id,
                        Some(SchemaClass::Round),
                        "Fillet",
                    )?
                } else {
                    IrFeatureDefinition::Operation(IrFeatureOperation::StoredGeometry {})
                },
                cadmpeg_ir::features::DistinctMembers::try_from(
                    feature_output_bodies(ctx, scan, ir, feature_id)?,
                    ctx,
                )
                .map_err(cadmpeg_core::CodecError::from)?,
            ),
            native_ref: None,
        };
        source_carriers.admit_feature(ctx, ir, feature)?;
        lookup_storage.with_storage(|| {
            ctx.insert_hash_map(
                &mut model_features,
                feature_id,
                ir.model.features.len() - 1,
                "creo model feature index nodes",
            )
        })?;
        refresh_feature_outputs(ctx, scan, ir)?;
        geometry_generator_feature_count += 1;
    }
    let operation_ordinal_base = ir.model.features.len();
    for (operation_index, operation) in ctx
        .admit_iter(
            &scan.features.operations,
            "creo model operation feature traversal",
        )?
        .enumerate()
    {
        if !model_features.contains_key(&operation.feature_id) {
            ctx.charge_entities(1, "admit Creo model features")?;
        }
        let current_operation =
            current_feature_operation(ctx, &scan.features.operations, operation.feature_id)?;
        let outputs = feature_output_bodies(ctx, scan, ir, operation.feature_id)?;
        let source_property_storage = feature_source_properties(ctx, scan, operation.feature_id)?;
        let mut source_property_nodes = source_property_storage.1;
        let mut source_properties = source_property_storage.0;
        if let Some(prefix) = current_operation
            .and_then(crate::feature::operations::FeatureOperation::stored_name_prefix)
        {
            insert_feature_source_property(
                ctx,
                &mut source_property_nodes,
                &mut source_properties,
                "mdl_stored_name_prefix",
                char::from(prefix),
            )?;
        }
        let parameter_storage = feature_parameters(ctx, scan, operation.feature_id)?;
        let mut parameter_text_storage = Some(parameter_storage.1);
        let mut parameter_node_storage = Some(parameter_storage.2);
        let mut parameters = parameter_storage.0;
        let schema_class = feature_schema_class(ctx, scan, operation.feature_id)?;
        let definition = schema_class.map_or_else(
            || {
                current_operation
                    .and_then(|operation| operation.recipe.resolved())
                    .map(|_| {
                        schema_feature_definition(
                            ctx,
                            scan,
                            ir,
                            source_carriers,
                            operation.feature_id,
                            None,
                            operation.kind.as_str(),
                        )
                    })
                    .or_else(|| {
                        current_operation.and_then(|operation| {
                            named_or_referenced_feature_definition(
                                ctx,
                                scan,
                                ir,
                                source_carriers,
                                operation.feature_id,
                                operation.kind.as_str(),
                            )
                            .transpose()
                        })
                    })
                    .or_else(|| {
                        match unbounded_feature_plane_definition(
                            ctx,
                            scan,
                            ir,
                            source_carriers,
                            operation.feature_id,
                        ) {
                            Ok(Some(definition)) => Some(Ok(definition)),
                            Ok(None) => None,
                            Err(error) => Some(Err(error)),
                        }
                    })
                    .unwrap_or_else(|| {
                        let kind: cadmpeg_ir::features::NativeFeatureKind = ctx
                            .copy_retained_text(
                                current_operation
                                    .map_or("Native Feature", |operation| operation.kind.as_str()),
                                "creo native Feature kind",
                            )?
                            .into();
                        if let Some(text_storage) = parameter_text_storage.take() {
                            parameters =
                                text_storage.commit_value(std::mem::take(&mut parameters))?;
                        }
                        let parameters = cadmpeg_core::text::named_entries_for_decode(
                            ctx,
                            format_args!("creo:model:feature#{}", operation.feature_id),
                            std::mem::take(&mut parameters),
                        )?;
                        drop(parameter_node_storage.take());
                        Ok(IrFeatureDefinition::Operation(IrFeatureOperation::Native {
                            kind,
                            parameters,
                        }))
                    })
            },
            |schema_class| {
                schema_feature_definition(
                    ctx,
                    scan,
                    ir,
                    source_carriers,
                    operation.feature_id,
                    Some(schema_class),
                    operation.kind.as_str(),
                )
            },
        )?;
        retain_native_feature_parameters(
            ctx,
            &mut source_property_nodes,
            &mut source_properties,
            &definition,
            &parameters,
        )?;
        drop(parameters);
        drop(parameter_node_storage.take());
        drop(parameter_text_storage.take());
        let dependencies = feature_dependencies(
            ctx,
            scan,
            ir,
            operation.feature_id,
            &prototype_feature_dependencies,
        )?;
        let operation_section = ctx
            .find_by(
                &scan.framing.sections,
                |section| Ok(section.contains(operation.offset)),
                "creo sections feature traversal",
            )?
            .map_or("MdlStatus", |section| section.name());
        let name = current_operation
            .filter(|operation| operation.display_name_stored())
            .and_then(|operation| {
                operation
                    .name
                    .stored_name_bytes()
                    .map(|bytes| (operation, bytes))
            })
            .map(|(operation, bytes)| {
                let mut prefix_bytes = [0u8; 4];
                let stripped = operation
                    .stored_name_prefix()
                    .and_then(|prefix| {
                        let prefix = char::from(prefix).encode_utf8(&mut prefix_bytes);
                        bytes.strip_prefix(prefix.as_bytes())
                    })
                    .unwrap_or(bytes);
                ctx.copy_retained_lossy_utf8(stripped, "creo stored Feature name")
            })
            .transpose()?;
        let source_tag =
            current_feature_recipe(ctx, &scan.features.operations, operation.feature_id)?
                .map(|recipe| ctx.copy_retained_text(recipe.name(), "creo Feature source tag"))
                .transpose()?;
        let native_ref = owning_feature_definition_ref(ctx, scan, operation.feature_id)?;
        let parent_index =
            current_feature_recipe_parent(ctx, &scan.features.operations, operation.feature_id)?
                .and_then(|parent_id| model_features.get(&parent_id))
                .copied();
        if let Some(&existing_index) = model_features.get(&operation.feature_id) {
            if let Some(parent_index) = parent_index {
                lookup_storage.with_storage(|| {
                    append_regeneration_edge(
                        ctx,
                        &mut regeneration_edges,
                        &ir.model.features[existing_index].id,
                        &ir.model.features[parent_index].id,
                    )
                })?;
            }
            let existing = &mut ir.model.features[existing_index];
            let upgrade_legacy_round = legacy_rounds.contains(&operation.feature_id)
                && matches!(
                    &definition,
                    IrFeatureDefinition::Operation(IrFeatureOperation::Fillet { .. })
                )
                && matches!(
                    existing.evaluation.definition(),
                    IrFeatureDefinition::Operation(IrFeatureOperation::StoredGeometry {})
                );
            if upgrade_legacy_round {
                source_carriers.replace_feature_definition(ctx, existing, definition)?;
            }
            if name.is_some() {
                existing.name = name;
            }
            merge_feature_dependencies(ctx, &mut existing.dependencies, dependencies)?;
            let mut incoming_nodes = ctx.reserve_scoped(0, "named entry map nodes")?;
            let incoming = incoming_nodes.with_storage(|| {
                cadmpeg_core::text::named_entries_for_decode(
                    ctx,
                    format_args!("creo:model:feature#{}", operation.feature_id),
                    source_properties,
                )
                .map_err(cadmpeg_core::CodecError::from)
            })?;
            drop(source_property_nodes);
            merge_feature_source_properties(ctx, &mut existing.source_properties, incoming)?;
            drop(incoming_nodes);
            if source_tag.is_some() {
                existing.source_tag = source_tag;
            }
            if existing.native_ref.is_none() {
                existing.native_ref = native_ref;
            }
            let mut output_storage =
                ctx.reserve_scoped(0, "creo combined feature output index storage")?;
            let mut additions = Vec::new();
            {
                let mut members = HashSet::new();
                for body in ctx.admit_iter(
                    existing.evaluation.outputs(),
                    "creo existing feature output index traversal",
                )? {
                    output_storage.with_storage(|| {
                        ctx.insert_hash_set(
                            &mut members,
                            body,
                            "creo combined feature output lookup",
                        )
                    })?;
                }
                for output in
                    ctx.admit_iter(&outputs, "creo incoming feature output index traversal")?
                {
                    let added = output_storage.with_storage(|| {
                        ctx.insert_hash_set(
                            &mut members,
                            output,
                            "creo combined feature output lookup",
                        )
                    })?;
                    ctx.push_scoped_vec(
                        &mut output_storage,
                        &mut additions,
                        added,
                        "creo combined feature output decisions",
                    )?;
                }
            }
            let mut combined_outputs = Vec::new();
            for body in ctx.admit_iter(
                existing.evaluation.outputs(),
                "creo existing feature output traversal",
            )? {
                let copy = copy_body_id(ctx, body)?;
                ctx.push_vec(&mut combined_outputs, copy, "creo combined feature outputs")?;
            }
            for (index, output) in ctx
                .admit_iter(outputs, "creo incoming feature output traversal")?
                .enumerate()
            {
                if additions[index] {
                    ctx.push_vec(
                        &mut combined_outputs,
                        output,
                        "creo combined feature outputs",
                    )?;
                }
            }
            existing.evaluation.set_outputs(
                cadmpeg_ir::features::DistinctMembers::try_from(combined_outputs, ctx)
                    .map_err(cadmpeg_core::CodecError::from)?,
            );
            refresh_feature_outputs(ctx, scan, ir)?;
            continue;
        }
        let id_parts = compose_feature_id(ctx, operation.feature_id)?;
        let id_bytes = id_parts.1;
        let id = id_parts.0;
        if let Some(parent_index) = parent_index {
            lookup_storage.with_storage(|| {
                append_regeneration_edge(
                    ctx,
                    &mut regeneration_edges,
                    &id,
                    &ir.model.features[parent_index].id,
                )
            })?;
        }
        let (operation_annotation_kind, operation_exactness) = if operation.display_state_conflict {
            ("feature_operation_state_consensus", Exactness::Derived)
        } else if operation.display_name_stored() {
            ("feature_operation_name", Exactness::ByteExact)
        } else {
            ("feature_recipe", Exactness::ByteExact)
        };
        annotate(
            ctx,
            annotations,
            &id,
            operation_section,
            cadmpeg_core::decode::u64_from_index(operation.offset),
            operation_annotation_kind,
            operation_exactness,
        )?;
        let id = id_bytes.commit_value(id)?;
        let feature = Feature {
            id,
            ordinal: cadmpeg_core::decode::u64_from_index(operation_ordinal_base + operation_index),
            name,
            suppressed: Some(false),
            dependencies: cadmpeg_ir::features::DistinctMembers::try_from(dependencies, ctx)
                .map_err(cadmpeg_core::CodecError::from)?,
            source_properties: {
                let source_properties = cadmpeg_core::text::named_entries_for_decode(
                    ctx,
                    format_args!("creo:model:feature#{}", operation.feature_id),
                    source_properties,
                )?;
                drop(source_property_nodes);
                source_properties
            },
            source_tag,
            source_text: None,
            source_content: cadmpeg_ir::features::FeatureContent::default(),

            evaluation: cadmpeg_ir::features::FeatureEvaluation::new(
                definition,
                cadmpeg_ir::features::DistinctMembers::try_from(outputs, ctx)
                    .map_err(cadmpeg_core::CodecError::from)?,
            ),
            native_ref,
        };
        source_carriers.admit_feature(ctx, ir, feature)?;
        lookup_storage.with_storage(|| {
            ctx.insert_hash_map(
                &mut model_features,
                operation.feature_id,
                ir.model.features.len() - 1,
                "creo model feature index nodes",
            )
        })?;
        refresh_feature_outputs(ctx, scan, ir)?;
    }
    for feature_id in ctx
        .admit_iter(&row_feature_ids.order, "creo row model feature traversal")?
        .copied()
    {
        if model_features.contains_key(&feature_id) {
            continue;
        }
        let id_parts = compose_feature_id(ctx, feature_id)?;
        let id_bytes = id_parts.1;
        let id = id_parts.0;
        let schema_class = feature_schema_class(ctx, scan, feature_id)?;
        let Some(&offset) = row_feature_ids.offsets.get(&feature_id) else {
            continue;
        };
        ctx.charge_entities(1, "admit Creo model features")?;
        let reference_name = feature_reference_name(ctx, scan, feature_id)?
            .map(|bytes| decoded_feature_reference_name(ctx, bytes))
            .transpose()?;
        let reference_name = reference_name.as_deref();
        let kind = reference_name.unwrap_or_else(|| {
            schema_class
                .and_then(schema_operation_kind)
                .unwrap_or("Native Feature")
        });
        annotate(
            ctx,
            annotations,
            &id,
            "AllFeatur",
            cadmpeg_core::decode::u64_from_index(offset),
            "schema_feature_operation",
            Exactness::ByteExact,
        )?;
        let parameter_storage = feature_parameters(ctx, scan, feature_id)?;
        let mut parameter_text_storage = Some(parameter_storage.1);
        let mut parameter_node_storage = Some(parameter_storage.2);
        let mut parameters = parameter_storage.0;
        let source_property_storage = feature_source_properties(ctx, scan, feature_id)?;
        let mut source_property_nodes = source_property_storage.1;
        let mut source_properties = source_property_storage.0;
        let definition = schema_class.map_or_else(
            || match match named_feature_definition(
                ctx,
                scan,
                ir,
                source_carriers,
                feature_id,
                kind,
            )? {
                Some(definition) => Some(definition),
                None => {
                    unbounded_feature_plane_definition(ctx, scan, ir, source_carriers, feature_id)?
                }
            } {
                Some(definition) => Ok(definition),
                None => {
                    let kind: cadmpeg_ir::features::NativeFeatureKind = ctx
                        .copy_retained_text(kind, "creo native row Feature kind")?
                        .into();
                    if let Some(text_storage) = parameter_text_storage.take() {
                        parameters = text_storage.commit_value(std::mem::take(&mut parameters))?;
                    }
                    let parameters = cadmpeg_core::text::named_entries_for_decode(
                        ctx,
                        format_args!("creo:model:feature#{feature_id}"),
                        std::mem::take(&mut parameters),
                    )?;
                    drop(parameter_node_storage.take());
                    Ok(IrFeatureDefinition::Operation(IrFeatureOperation::Native {
                        kind,
                        parameters,
                    }))
                }
            },
            |schema_class| {
                schema_feature_definition(
                    ctx,
                    scan,
                    ir,
                    source_carriers,
                    feature_id,
                    Some(schema_class),
                    kind,
                )
            },
        )?;
        let row_schema_classes = row_feature_schema_classes(ctx, &scan.features.rows, feature_id)?;
        if schema_class.is_none() {
            insert_feature_source_property(
                ctx,
                &mut source_property_nodes,
                &mut source_properties,
                "featdefs_schema_state",
                if row_schema_classes.is_empty() {
                    "absent"
                } else {
                    "ambiguous"
                },
            )?;
        }
        if !row_schema_classes.is_empty() {
            insert_feature_source_property(
                ctx,
                &mut source_property_nodes,
                &mut source_properties,
                "featdefs_row_schema_classes",
                SchemaClassList(&row_schema_classes),
            )?;
        }
        retain_native_feature_parameters(
            ctx,
            &mut source_property_nodes,
            &mut source_properties,
            &definition,
            &parameters,
        )?;
        drop(parameters);
        drop(parameter_node_storage.take());
        drop(parameter_text_storage.take());
        let id = id_bytes.commit_value(id)?;
        let feature = Feature {
            id,
            ordinal: cadmpeg_core::decode::u64_from_index(ir.model.features.len()),
            name: Some(match reference_name {
                Some(name) => ctx.copy_retained_text(name, "creo row Feature name")?,
                None => ctx.format_retained(
                    format_args!("{kind} id {feature_id}"),
                    "creo row Feature name",
                )?,
            }),
            suppressed: Some(false),
            dependencies: cadmpeg_ir::features::DistinctMembers::try_from(
                feature_dependencies(ctx, scan, ir, feature_id, &prototype_feature_dependencies)?,
                ctx,
            )
            .map_err(cadmpeg_core::CodecError::from)?,
            source_properties: {
                let source_properties = cadmpeg_core::text::named_entries_for_decode(
                    ctx,
                    format_args!("creo:model:feature#{feature_id}"),
                    source_properties,
                )?;
                drop(source_property_nodes);
                source_properties
            },
            source_tag: None,
            source_text: None,
            source_content: cadmpeg_ir::features::FeatureContent::default(),

            evaluation: cadmpeg_ir::features::FeatureEvaluation::new(
                definition,
                cadmpeg_ir::features::DistinctMembers::try_from(
                    feature_output_bodies(ctx, scan, ir, feature_id)?,
                    ctx,
                )
                .map_err(cadmpeg_core::CodecError::from)?,
            ),
            native_ref: owning_feature_definition_ref(ctx, scan, feature_id)?,
        };
        source_carriers.admit_feature(ctx, ir, feature)?;
        lookup_storage.with_storage(|| {
            ctx.insert_hash_map(
                &mut model_features,
                feature_id,
                ir.model.features.len() - 1,
                "creo model feature index nodes",
            )
        })?;
        refresh_feature_outputs(ctx, scan, ir)?;
    }
    commit_regeneration_edges(ctx, ir, &regeneration_edges)?;
    Ok(geometry_generator_feature_count)
}

pub(super) fn finish_feature_transfers(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    scan: &ContainerScan,
    ir: &mut CadIr,
    annotations: &mut AnnotationBuilder,
    coverage: &mut cadmpeg_ir::report::decode::Coverage,
    source_carriers: &mut crate::decode::source_carriers::SourceUnitCarriers,
) -> Result<(usize, usize), cadmpeg_core::CodecError> {
    let mut lookup_storage = ctx.reserve_scoped(0, "Creo final feature prototype lookup")?;
    let prototype_feature_dependencies =
        lookup_storage.with_storage(|| surface_prototype_feature_dependencies(ctx, scan))?;
    link_feature_sketch_history(ctx, scan, ir)?;
    reconcile_feature_links(ctx, scan, ir, &prototype_feature_dependencies)?;
    let feature_result_topology_count = emit_feature_result_topologies(ctx, scan, ir)?;
    let feature_result_edge_count = ctx
        .admit_iter(
            &ir.model.feature_result_topologies,
            "creo feature_result_topologies feature traversal",
        )?
        .map(|state| state.edges().len())
        .sum::<usize>();
    let (transferred_feature_dimension_count, dimension_parameters) =
        transfer_feature_dimensions(ctx, scan, ir, annotations, source_carriers)?;
    let transferred_curve_expression_parameter_count = transfer_curve_expression_features(
        ctx,
        scan,
        ir,
        annotations,
        &dimension_parameters,
        source_carriers,
    )?;
    {
        let mut decoded_curve_expression_assignment_count = 0usize;
        let mut decoded_curve_expression_table_cell_assignment_count = 0usize;
        let mut decoded_curve_expression_scoped_symbol_assignment_count = 0usize;
        let mut decoded_curve_expression_system_symbol_assignment_count = 0usize;
        let mut decoded_curve_expression_function_write_assignment_count = 0usize;
        let mut evaluated_curve_expression_assignment_count = 0usize;
        let mut decoded_curve_expression_solve_block_count = 0usize;
        let mut decoded_curve_expression_simultaneous_equation_count = 0usize;
        let mut decoded_curve_expression_solve_assignment_count = 0usize;
        let mut decoded_curve_expression_solve_variable_count = 0usize;
        let mut evaluated_curve_expression_solve_block_count = 0usize;
        let mut evaluated_curve_expression_solve_variable_count = 0usize;
        let mut unresolved_curve_expression_solve_control_count = 0usize;
        let mut prohibited_curve_expression_record_count = 0usize;
        let mut prohibited_curve_expression_kind_count = 0usize;
        let mut activation_counts = [0usize; 3];
        for record in ctx
            .admit_iter(
                &scan.curves.expressions,
                "creo active curve expression traversal",
            )?
            .filter(|record| !record.backup)
        {
            for (total, count) in [
                (
                    &mut decoded_curve_expression_assignment_count,
                    record.assignments.len(),
                ),
                (
                    &mut decoded_curve_expression_solve_block_count,
                    record.solve_blocks.len(),
                ),
                (
                    &mut unresolved_curve_expression_solve_control_count,
                    usize::from(record.unresolved_solve_control),
                ),
                (
                    &mut prohibited_curve_expression_record_count,
                    usize::from(!record.prohibited_constructs.is_empty()),
                ),
                (
                    &mut prohibited_curve_expression_kind_count,
                    record.prohibited_constructs.len(),
                ),
            ] {
                *total = total.checked_add(count).ok_or_else(|| {
                    cadmpeg_core::decode::refuse_local_limit(
                        "creo expression coverage count",
                        u64::MAX,
                        u64::MAX,
                    )
                })?;
            }
            for assignment in ctx.admit_iter(
                &record.assignments,
                "creo expression assignment coverage traversal",
            )? {
                use crate::curve::{CurveExpressionActivation, CurveExpressionTarget};
                for (total, counted) in [
                    (
                        &mut decoded_curve_expression_table_cell_assignment_count,
                        matches!(&assignment.target, CurveExpressionTarget::TableCell { .. }),
                    ),
                    (
                        &mut decoded_curve_expression_scoped_symbol_assignment_count,
                        matches!(
                            &assignment.target,
                            CurveExpressionTarget::ScopedSymbol { .. }
                        ),
                    ),
                    (
                        &mut decoded_curve_expression_system_symbol_assignment_count,
                        matches!(
                            &assignment.target,
                            CurveExpressionTarget::SystemSymbol { .. }
                        ),
                    ),
                    (
                        &mut decoded_curve_expression_function_write_assignment_count,
                        matches!(
                            &assignment.target,
                            CurveExpressionTarget::FunctionWrite { .. }
                        ),
                    ),
                    (
                        &mut evaluated_curve_expression_assignment_count,
                        assignment.value.is_some(),
                    ),
                ] {
                    *total = total.checked_add(usize::from(counted)).ok_or_else(|| {
                        cadmpeg_core::decode::refuse_local_limit(
                            "creo expression coverage count",
                            u64::MAX,
                            u64::MAX,
                        )
                    })?;
                }
                let activation = match assignment.activation {
                    CurveExpressionActivation::Active => 0,
                    CurveExpressionActivation::Inactive => 1,
                    CurveExpressionActivation::Conditional => 2,
                };
                activation_counts[activation] = activation_counts[activation]
                    .checked_add(1)
                    .ok_or_else(|| {
                        cadmpeg_core::decode::refuse_local_limit(
                            "creo expression coverage count",
                            u64::MAX,
                            u64::MAX,
                        )
                    })?;
            }
            for block in ctx.admit_iter(
                &record.solve_blocks,
                "creo expression solve block coverage traversal",
            )? {
                for (total, count) in [
                    (
                        &mut decoded_curve_expression_simultaneous_equation_count,
                        block.equations.len(),
                    ),
                    (
                        &mut decoded_curve_expression_solve_assignment_count,
                        block.assignments.len(),
                    ),
                    (
                        &mut decoded_curve_expression_solve_variable_count,
                        block.unknowns.len(),
                    ),
                ] {
                    *total = total.checked_add(count).ok_or_else(|| {
                        cadmpeg_core::decode::refuse_local_limit(
                            "creo expression coverage count",
                            u64::MAX,
                            u64::MAX,
                        )
                    })?;
                }
                let mut all_solved = true;
                for unknown in
                    ctx.admit_iter(&block.unknowns, "creo evaluated unknown coverage traversal")?
                {
                    let solved = unknown.solution.is_some();
                    all_solved &= solved;
                    evaluated_curve_expression_solve_variable_count =
                        evaluated_curve_expression_solve_variable_count
                            .checked_add(usize::from(solved))
                            .ok_or_else(|| {
                                cadmpeg_core::decode::refuse_local_limit(
                                    "creo expression coverage count",
                                    u64::MAX,
                                    u64::MAX,
                                )
                            })?;
                }
                evaluated_curve_expression_solve_block_count =
                    evaluated_curve_expression_solve_block_count
                        .checked_add(usize::from(all_solved))
                        .ok_or_else(|| {
                            cadmpeg_core::decode::refuse_local_limit(
                                "creo expression coverage count",
                                u64::MAX,
                                u64::MAX,
                            )
                        })?;
            }
        }
        coverage.record(
            ctx,
            crate::coverage::DECODED_ACTIVE_CURVE_EXPRESSION_ASSIGNMENT_COUNT,
            decoded_curve_expression_assignment_count,
        )?;
        coverage.record(
            ctx,
            crate::coverage::TRANSFERRED_CURVE_EXPRESSION_PARAMETER_COUNT,
            transferred_curve_expression_parameter_count,
        )?;
        coverage.record(
            ctx,
            crate::coverage::DECODED_ACTIVE_CURVE_EXPRESSION_TABLE_CELL_ASSIGNMENT_COUNT,
            decoded_curve_expression_table_cell_assignment_count,
        )?;
        coverage.record(
            ctx,
            crate::coverage::DECODED_ACTIVE_CURVE_EXPRESSION_SCOPED_SYMBOL_ASSIGNMENT_COUNT,
            decoded_curve_expression_scoped_symbol_assignment_count,
        )?;
        coverage.record(
            ctx,
            crate::coverage::DECODED_ACTIVE_CURVE_EXPRESSION_SYSTEM_SYMBOL_ASSIGNMENT_COUNT,
            decoded_curve_expression_system_symbol_assignment_count,
        )?;
        coverage.record(
            ctx,
            crate::coverage::DECODED_ACTIVE_CURVE_EXPRESSION_FUNCTION_WRITE_ASSIGNMENT_COUNT,
            decoded_curve_expression_function_write_assignment_count,
        )?;
        coverage.record(
            ctx,
            crate::coverage::EVALUATED_ACTIVE_CURVE_EXPRESSION_ASSIGNMENT_COUNT,
            evaluated_curve_expression_assignment_count,
        )?;
        coverage.record(
            ctx,
            crate::coverage::DECODED_ACTIVE_CURVE_EXPRESSION_SOLVE_BLOCK_COUNT,
            decoded_curve_expression_solve_block_count,
        )?;
        coverage.record(
            ctx,
            crate::coverage::DECODED_ACTIVE_CURVE_EXPRESSION_SIMULTANEOUS_EQUATION_COUNT,
            decoded_curve_expression_simultaneous_equation_count,
        )?;
        coverage.record(
            ctx,
            crate::coverage::DECODED_ACTIVE_CURVE_EXPRESSION_SOLVE_ASSIGNMENT_COUNT,
            decoded_curve_expression_solve_assignment_count,
        )?;
        coverage.record(
            ctx,
            crate::coverage::DECODED_ACTIVE_CURVE_EXPRESSION_SOLVE_VARIABLE_COUNT,
            decoded_curve_expression_solve_variable_count,
        )?;
        coverage.record(
            ctx,
            crate::coverage::EVALUATED_ACTIVE_CURVE_EXPRESSION_SOLVE_BLOCK_COUNT,
            evaluated_curve_expression_solve_block_count,
        )?;
        coverage.record(
            ctx,
            crate::coverage::EVALUATED_ACTIVE_CURVE_EXPRESSION_SOLVE_VARIABLE_COUNT,
            evaluated_curve_expression_solve_variable_count,
        )?;
        coverage.record(
            ctx,
            crate::coverage::UNRESOLVED_ACTIVE_CURVE_EXPRESSION_SOLVE_CONTROL_COUNT,
            unresolved_curve_expression_solve_control_count,
        )?;
        coverage.record(
            ctx,
            crate::coverage::PROHIBITED_ACTIVE_CURVE_EXPRESSION_RECORD_COUNT,
            prohibited_curve_expression_record_count,
        )?;
        coverage.record(
            ctx,
            crate::coverage::PROHIBITED_ACTIVE_CURVE_EXPRESSION_KIND_COUNT,
            prohibited_curve_expression_kind_count,
        )?;
        for (key, count) in [
            (
                crate::coverage::ACTIVE_CURVE_EXPRESSION_ASSIGNMENT_COUNT,
                activation_counts[0],
            ),
            (
                crate::coverage::INACTIVE_CURVE_EXPRESSION_ASSIGNMENT_COUNT,
                activation_counts[1],
            ),
            (
                crate::coverage::CONDITIONAL_CURVE_EXPRESSION_ASSIGNMENT_COUNT,
                activation_counts[2],
            ),
        ] {
            coverage.record(ctx, key, count)?;
        }
        let (decoded_dimension_count, resolved_dimension_count) = ctx
            .admit_iter(
                &scan.features.definitions,
                "creo feature dimension coverage traversal",
            )?
            .filter_map(|definition| definition.dimensions.as_ref())
            .try_fold((0usize, 0usize), |total, table| {
                ctx.admit_iter(&table.rows, "creo feature dimension row coverage traversal")?
                    .try_fold(total, |(decoded, resolved), dimension| {
                        Ok::<_, cadmpeg_core::CodecError>((
                            decoded.checked_add(1).ok_or_else(|| {
                                cadmpeg_core::decode::refuse_local_limit(
                                    "creo dimension coverage count",
                                    u64::MAX,
                                    u64::MAX,
                                )
                            })?,
                            resolved
                                .checked_add(usize::from(dimension.value.resolved().is_some()))
                                .ok_or_else(|| {
                                    cadmpeg_core::decode::refuse_local_limit(
                                        "creo dimension coverage count",
                                        u64::MAX,
                                        u64::MAX,
                                    )
                                })?,
                        ))
                    })
            })?;
        coverage.record(
            ctx,
            crate::coverage::DECODED_FEATURE_DIMENSION_COUNT,
            decoded_dimension_count,
        )?;
        coverage.record(
            ctx,
            crate::coverage::TRANSFERRED_FEATURE_DIMENSION_PARAMETER_COUNT,
            transferred_feature_dimension_count,
        )?;
        coverage.record(
            ctx,
            crate::coverage::RESOLVED_FEATURE_DIMENSION_VALUE_COUNT,
            resolved_dimension_count,
        )?;
        coverage.record(
            ctx,
            crate::coverage::UNRESOLVED_FEATURE_DIMENSION_VALUE_COUNT,
            decoded_dimension_count
                .checked_sub(resolved_dimension_count)
                .ok_or_else(|| {
                    cadmpeg_core::CodecError::malformed(
                        "resolved dimension count exceeds decoded count",
                    )
                })?,
        )?;
    }
    close_sketch_constraint_parameter_references(ctx, ir)?;
    Ok((feature_result_topology_count, feature_result_edge_count))
}

#[cfg(test)]
mod tests;
