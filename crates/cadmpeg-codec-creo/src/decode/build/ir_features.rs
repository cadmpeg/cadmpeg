// SPDX-License-Identifier: Apache-2.0
//! Feature-tree emission, dimension parameters, and expression coverage.

use std::collections::{BTreeMap, BTreeSet};

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
use super::super::uniqueness::unique_feature_datum_plane;
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
    ctx.charge_work(
        cadmpeg_core::decode::u64_from_index(text.len()),
        "creo model feature identity grammar",
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
    edges: Vec<(IrFeatureId, IrFeatureId)>,
) -> Result<(), cadmpeg_core::CodecError> {
    for (child, parent) in ctx.admit_iter(&edges, "creo regeneration edge traversal")? {
        ir.model
            .set_feature_regeneration_parent(ctx, child, parent)?;
    }
    Ok(())
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
        let Some(feature_id) = feature
            .id
            .as_str()
            .strip_prefix("creo:model:feature#")
            .and_then(|value| value.parse::<u32>().ok())
        else {
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
    for (index, outputs) in output_updates {
        ir.model.features[index].evaluation.set_outputs(outputs);
    }
    Ok(())
}

fn ordered_row_feature_ids(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    rows: &[crate::feature::rows::FeatureRow],
) -> Result<Vec<u32>, cadmpeg_core::CodecError> {
    let mut seen_storage = ctx.reserve_scoped(0, "Creo row feature identity lookup")?;
    let mut seen = BTreeSet::new();
    let mut ids = Vec::new();
    for row in ctx.admit_iter(rows, "creo ordered feature row traversal")? {
        if seen_storage.with_storage(|| {
            ctx.insert_btree_set(&mut seen, row.feature_id, "creo feature row identity nodes")
        })? {
            ctx.reserve_vec(&mut ids, 1, "creo feature row IDs")?;
            ids.push(row.feature_id);
        }
    }
    Ok(ids)
}

fn merge_feature_source_properties(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    target: &mut BTreeMap<cadmpeg_core::text::NonBlankString, String>,
    incoming: BTreeMap<cadmpeg_core::text::NonBlankString, String>,
) -> Result<(), cadmpeg_core::CodecError> {
    for (key, value) in incoming {
        ctx.insert_btree_map(target, key, value, "creo IR Feature source property nodes")?;
    }
    Ok(())
}

fn merge_feature_dependencies(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    target: &mut DistinctMembers<IrFeatureId>,
    incoming: Vec<IrFeatureId>,
) -> Result<(), cadmpeg_core::CodecError> {
    for dependency in incoming {
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
        if operation_feature_ids.contains(&datum.feature_id) {
            continue;
        }
        let (id, id_bytes) = compose_feature_id(ctx, datum.feature_id)?;
        let mut identity_present = false;
        for feature in ctx.admit_iter(&ir.model.features, "creo existing model feature search")? {
            if ctx.equal(&feature.id, &id, "creo model identity comparison")? {
                identity_present = true;
                break;
            }
        }
        if identity_present {
            continue;
        }
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
        id_bytes.commit()?;
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
                if unique_feature_datum_plane(ctx, &scan.planes.datums, datum.feature_id)?.is_some()
                {
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
    }
    let row_feature_ids =
        lookup_storage.with_storage(|| ordered_row_feature_ids(ctx, &scan.features.rows))?;
    let mut geometry_generator_feature_count = 0;
    let generators = lookup_storage.with_storage(|| geometry_generator_features(ctx, scan))?;
    for generator in ctx.admit_iter(&generators, "creo geometry generator feature traversal")? {
        let feature_id = generator.feature_id;
        let (id, id_bytes) = compose_feature_id(ctx, feature_id)?;
        let mut identity_present = false;
        for feature in ctx.admit_iter(&ir.model.features, "creo existing model feature search")? {
            if ctx.equal(&feature.id, &id, "creo model identity comparison")? {
                identity_present = true;
                break;
            }
        }
        if identity_present {
            continue;
        }
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
        id_bytes.commit()?;
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
                if ctx.any_by(
                    &scan.features.legacy_rounds,
                    |round| Ok(round.feature_id == feature_id),
                    "creo legacy_rounds feature traversal",
                )? {
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
        if !ctx.any_by(
            &ir.model.features,
            |feature| {
                Ok(crate::identity::matches_numbered_identity(
                    feature.id.as_str(),
                    "creo:model:feature#",
                    operation.feature_id,
                ))
            },
            "creo existing model feature search",
        )? {
            ctx.charge_entities(1, "admit Creo model features")?;
        }
        let current_operation =
            current_feature_operation(ctx, &scan.features.operations, operation.feature_id)?;
        let outputs = feature_output_bodies(ctx, scan, ir, operation.feature_id)?;
        let (source_property_nodes, source_properties) =
            feature_source_properties(ctx, scan, operation.feature_id)?;
        let mut source_property_nodes = source_property_nodes;
        let mut source_properties = source_properties;
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
        let (parameter_text_storage, parameter_node_storage, parameters) =
            feature_parameters(ctx, scan, operation.feature_id)?;
        let mut parameter_text_storage = Some(parameter_text_storage);
        let mut parameter_node_storage = Some(parameter_node_storage);
        let mut parameters = parameters;
        let schema_class = feature_schema_class(ctx, scan, operation.feature_id)?;
        let definition = schema_class.map_or_else(
            || {
                current_operation.and_then(|operation| operation.recipe.resolved())
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
                            text_storage.commit()?;
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
        let source_tag = current_feature_recipe(ctx, &scan.features.operations, operation.feature_id)?
            .map(|recipe| ctx.copy_retained_text(recipe.name(), "creo Feature source tag"))
            .transpose()?;
        let native_ref = owning_feature_definition_ref(ctx, scan, operation.feature_id)?;
        let (id, id_bytes) = compose_feature_id(ctx, operation.feature_id)?;
        let parent =
            match current_feature_recipe_parent(ctx, &scan.features.operations, operation.feature_id)? {
                Some(parent_feature_id) => ctx
                    .find_by(
                        &ir.model.features,
                        |feature| {
                            Ok(crate::identity::matches_numbered_identity(
                                feature.id.as_str(),
                                "creo:model:feature#",
                                parent_feature_id,
                            ))
                        },
                        "creo regeneration parent feature search",
                    )?
                    .map(|feature| &feature.id),
                None => None,
            };
        if let Some(parent) = parent {
            lookup_storage.with_storage(|| {
                append_regeneration_edge(ctx, &mut regeneration_edges, &id, parent)
            })?;
        }
        let mut existing_index = None;
        for (index, feature) in ctx
            .admit_iter(&ir.model.features, "creo existing feature update search")?
            .enumerate()
        {
            if ctx.equal(
                &feature.id,
                &id,
                "creo existing feature identity comparison",
            )? {
                existing_index = Some(index);
                break;
            }
        }
        if let Some(existing_index) = existing_index {
            let existing = &mut ir.model.features[existing_index];
            let upgrade_legacy_round = ctx.any_by(
                &scan.features.legacy_rounds,
                |round| Ok(round.feature_id == operation.feature_id),
                "creo legacy_rounds feature traversal",
            )? && matches!(
                &definition,
                IrFeatureDefinition::Operation(IrFeatureOperation::Fillet { .. })
            ) && matches!(
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
            let mut combined_outputs = Vec::new();
            for body in ctx.admit_iter(
                existing.evaluation.outputs(),
                "creo existing feature output traversal",
            )? {
                let copy = copy_body_id(ctx, body)?;
                ctx.reserve_vec(&mut combined_outputs, 1, "creo combined feature outputs")?;
                combined_outputs.push(copy);
            }
            for output in outputs {
                if !ctx.contains(
                    &combined_outputs,
                    &output,
                    "creo combined feature output lookup",
                )? {
                    ctx.reserve_vec(&mut combined_outputs, 1, "creo combined feature outputs")?;
                    combined_outputs.push(output);
                }
            }
            existing.evaluation.set_outputs(
                cadmpeg_ir::features::DistinctMembers::try_from(combined_outputs, ctx)
                    .map_err(cadmpeg_core::CodecError::from)?,
            );
            refresh_feature_outputs(ctx, scan, ir)?;
            continue;
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
        id_bytes.commit()?;
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
        refresh_feature_outputs(ctx, scan, ir)?;
    }
    for feature_id in ctx
        .admit_iter(&row_feature_ids, "creo row model feature traversal")?
        .copied()
    {
        let (id, id_bytes) = compose_feature_id(ctx, feature_id)?;
        let mut identity_present = false;
        for feature in ctx.admit_iter(&ir.model.features, "creo existing model feature search")? {
            if ctx.equal(&feature.id, &id, "creo model identity comparison")? {
                identity_present = true;
                break;
            }
        }
        if identity_present {
            continue;
        }
        let schema_class = feature_schema_class(ctx, scan, feature_id)?;
        let Some(offset) = ctx
            .admit_iter(&scan.features.rows, "creo rows feature traversal")?
            .filter(|row| row.feature_id == feature_id)
            .map(|row| row.offset)
            .min()
        else {
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
        let (parameter_text_storage, parameter_node_storage, parameters) =
            feature_parameters(ctx, scan, feature_id)?;
        let mut parameter_text_storage = Some(parameter_text_storage);
        let mut parameter_node_storage = Some(parameter_node_storage);
        let mut parameters = parameters;
        let (source_property_nodes, source_properties) =
            feature_source_properties(ctx, scan, feature_id)?;
        let mut source_property_nodes = source_property_nodes;
        let mut source_properties = source_properties;
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
                        text_storage.commit()?;
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
        id_bytes.commit()?;
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
        refresh_feature_outputs(ctx, scan, ir)?;
    }
    commit_regeneration_edges(ctx, ir, regeneration_edges)?;
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
        let active_expressions = || {
            Ok::<_, cadmpeg_core::CodecError>(
                ctx.admit_iter(
                    &scan.curves.expressions,
                    "creo active curve expression traversal",
                )?
                .filter(|record| !record.backup),
            )
        };
        let decoded_curve_expression_assignment_count = active_expressions()?
            .map(|record| record.assignments.len())
            .sum::<usize>();
        let decoded_curve_expression_table_cell_assignment_count =
            active_expressions()?.try_fold(0usize, |total, record| {
                let count = ctx
                    .admit_iter(
                        &record.assignments,
                        "creo expression assignment coverage traversal",
                    )?
                    .filter(|assignment| {
                        matches!(
                            &assignment.target,
                            crate::curve::CurveExpressionTarget::TableCell { .. }
                        )
                    })
                    .count();
                total.checked_add(count).ok_or_else(|| {
                    cadmpeg_core::decode::refuse_local_limit(
                        "creo expression coverage count",
                        u64::MAX,
                        u64::MAX,
                    )
                })
            })?;
        let decoded_curve_expression_scoped_symbol_assignment_count = active_expressions()?
            .try_fold(0usize, |total, record| {
                let count = ctx
                    .admit_iter(
                        &record.assignments,
                        "creo expression assignment coverage traversal",
                    )?
                    .filter(|assignment| {
                        matches!(
                            &assignment.target,
                            crate::curve::CurveExpressionTarget::ScopedSymbol { .. }
                        )
                    })
                    .count();
                total.checked_add(count).ok_or_else(|| {
                    cadmpeg_core::decode::refuse_local_limit(
                        "creo expression coverage count",
                        u64::MAX,
                        u64::MAX,
                    )
                })
            })?;
        let decoded_curve_expression_system_symbol_assignment_count = active_expressions()?
            .try_fold(0usize, |total, record| {
                let count = ctx
                    .admit_iter(
                        &record.assignments,
                        "creo expression assignment coverage traversal",
                    )?
                    .filter(|assignment| {
                        matches!(
                            &assignment.target,
                            crate::curve::CurveExpressionTarget::SystemSymbol { .. }
                        )
                    })
                    .count();
                total.checked_add(count).ok_or_else(|| {
                    cadmpeg_core::decode::refuse_local_limit(
                        "creo expression coverage count",
                        u64::MAX,
                        u64::MAX,
                    )
                })
            })?;
        let decoded_curve_expression_function_write_assignment_count = active_expressions()?
            .try_fold(0usize, |total, record| {
                let count = ctx
                    .admit_iter(
                        &record.assignments,
                        "creo expression assignment coverage traversal",
                    )?
                    .filter(|assignment| {
                        matches!(
                            &assignment.target,
                            crate::curve::CurveExpressionTarget::FunctionWrite { .. }
                        )
                    })
                    .count();
                total.checked_add(count).ok_or_else(|| {
                    cadmpeg_core::decode::refuse_local_limit(
                        "creo expression coverage count",
                        u64::MAX,
                        u64::MAX,
                    )
                })
            })?;
        let evaluated_curve_expression_assignment_count =
            active_expressions()?.try_fold(0usize, |total, record| {
                let count = ctx
                    .admit_iter(
                        &record.assignments,
                        "creo expression assignment coverage traversal",
                    )?
                    .filter(|assignment| assignment.value.is_some())
                    .count();
                total.checked_add(count).ok_or_else(|| {
                    cadmpeg_core::decode::refuse_local_limit(
                        "creo expression coverage count",
                        u64::MAX,
                        u64::MAX,
                    )
                })
            })?;
        let decoded_curve_expression_solve_block_count = active_expressions()?
            .map(|record| record.solve_blocks.len())
            .sum::<usize>();
        let decoded_curve_expression_simultaneous_equation_count =
            active_expressions()?.try_fold(0usize, |total, record| {
                let count = ctx
                    .admit_iter(
                        &record.solve_blocks,
                        "creo expression solve block coverage traversal",
                    )?
                    .map(|block| block.equations.len())
                    .sum::<usize>();
                total.checked_add(count).ok_or_else(|| {
                    cadmpeg_core::decode::refuse_local_limit(
                        "creo expression coverage count",
                        u64::MAX,
                        u64::MAX,
                    )
                })
            })?;
        let decoded_curve_expression_solve_assignment_count =
            active_expressions()?.try_fold(0usize, |total, record| {
                let count = ctx
                    .admit_iter(
                        &record.solve_blocks,
                        "creo expression solve block coverage traversal",
                    )?
                    .map(|block| block.assignments.len())
                    .sum::<usize>();
                total.checked_add(count).ok_or_else(|| {
                    cadmpeg_core::decode::refuse_local_limit(
                        "creo expression coverage count",
                        u64::MAX,
                        u64::MAX,
                    )
                })
            })?;
        let decoded_curve_expression_solve_variable_count =
            active_expressions()?.try_fold(0usize, |total, record| {
                let count = ctx
                    .admit_iter(
                        &record.solve_blocks,
                        "creo expression solve block coverage traversal",
                    )?
                    .map(|block| block.unknowns.len())
                    .sum::<usize>();
                total.checked_add(count).ok_or_else(|| {
                    cadmpeg_core::decode::refuse_local_limit(
                        "creo expression coverage count",
                        u64::MAX,
                        u64::MAX,
                    )
                })
            })?;
        let evaluated_curve_expression_solve_block_count =
            active_expressions()?.try_fold(0usize, |total, record| {
                ctx.admit_iter(
                    &record.solve_blocks,
                    "creo evaluated solve block coverage traversal",
                )?
                .try_fold(total, |total, block| {
                    let resolved = ctx.all_by(
                        &block.unknowns,
                        |unknown| Ok(unknown.solution.is_some()),
                        "creo solved unknown coverage traversal",
                    )?;
                    total.checked_add(usize::from(resolved)).ok_or_else(|| {
                        cadmpeg_core::decode::refuse_local_limit(
                            "creo expression coverage count",
                            u64::MAX,
                            u64::MAX,
                        )
                    })
                })
            })?;
        let evaluated_curve_expression_solve_variable_count =
            active_expressions()?.try_fold(0usize, |total, record| {
                ctx.admit_iter(
                    &record.solve_blocks,
                    "creo evaluated solve variable coverage traversal",
                )?
                .try_fold(total, |total, block| {
                    let count = ctx
                        .admit_iter(&block.unknowns, "creo evaluated unknown coverage traversal")?
                        .filter(|unknown| unknown.solution.is_some())
                        .count();
                    total.checked_add(count).ok_or_else(|| {
                        cadmpeg_core::decode::refuse_local_limit(
                            "creo expression coverage count",
                            u64::MAX,
                            u64::MAX,
                        )
                    })
                })
            })?;
        let unresolved_curve_expression_solve_control_count = active_expressions()?
            .filter(|record| record.unresolved_solve_control)
            .count();
        let prohibited_curve_expression_record_count = active_expressions()?
            .filter(|record| !record.prohibited_constructs.is_empty())
            .count();
        let prohibited_curve_expression_kind_count = active_expressions()?
            .map(|record| record.prohibited_constructs.len())
            .sum::<usize>();
        let activation_count = |activation| {
            active_expressions()?.try_fold(0usize, |total, record| {
                let count = ctx
                    .admit_iter(
                        &record.assignments,
                        "creo expression activation coverage traversal",
                    )?
                    .filter(|assignment| assignment.activation == activation)
                    .count();
                total.checked_add(count).ok_or_else(|| {
                    cadmpeg_core::decode::refuse_local_limit(
                        "creo expression coverage count",
                        u64::MAX,
                        u64::MAX,
                    )
                })
            })
        };
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
        for (key, activation) in [
            (
                crate::coverage::ACTIVE_CURVE_EXPRESSION_ASSIGNMENT_COUNT,
                crate::curve::CurveExpressionActivation::Active,
            ),
            (
                crate::coverage::INACTIVE_CURVE_EXPRESSION_ASSIGNMENT_COUNT,
                crate::curve::CurveExpressionActivation::Inactive,
            ),
            (
                crate::coverage::CONDITIONAL_CURVE_EXPRESSION_ASSIGNMENT_COUNT,
                crate::curve::CurveExpressionActivation::Conditional,
            ),
        ] {
            coverage.record(ctx, key, activation_count(activation)?)?;
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
