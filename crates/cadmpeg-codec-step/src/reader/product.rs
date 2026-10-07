// SPDX-License-Identifier: Apache-2.0
//! STEP product prototypes, occurrence identity, and relative placement.

use crate::ids::{key_word, kind};
use std::collections::{BTreeMap, BTreeSet, VecDeque};

use super::{RecordExt, ValueExt};
use cadmpeg_core::decode::{u64_from_index, DecodeContext, ScopedReservation};
use cadmpeg_core::CodecError;
use cadmpeg_ir::document::CadIr;
use cadmpeg_ir::ids::{BodyId, IdentityKey, IdentityKeyTail, OccurrenceId, ProductDefinitionId};
use cadmpeg_ir::products::{
    Occurrence, OccurrenceParent, ProductDefinition, ProductDefinitionKind, PrototypeReference,
};
use cadmpeg_ir::report::loss::LossNote;
use cadmpeg_ir::transform::{Transform, TransformError};

use crate::ids;
use crate::loss::StepLossCode;
use crate::parse::{Exchange, RawRecord, Value};

use super::decode_text_scoped;
use super::geometry::GeometryData;
use super::topology::TopologyData;
use super::StageOutcome;

const MAX_OCCURRENCES: usize = 100_000;
const MAX_ASSEMBLY_DEPTH: usize = 256;
const PRODUCT_DEFINITION_FORMATION_TYPES: &[&str] = &[
    "PRODUCT_DEFINITION_FORMATION",
    "PRODUCT_DEFINITION_FORMATION_WITH_SPECIFIED_SOURCE",
    "FINAL_SOLUTION",
];
const PRODUCT_DEFINITION_TYPES: &[&str] = &[
    "PRODUCT_DEFINITION",
    "PRODUCT_DEFINITION_WITH_ASSOCIATED_DOCUMENTS",
];
const DRAWING_ITEM_OWNER_TYPES: &[&str] = &[
    "DRAWING_SHEET_REVISION",
    "PRESENTATION_VIEW",
    "DRAUGHTING_MODEL",
    "DRAUGHTING_CALLOUT",
];

pub(super) struct ProductData<'ctx> {
    _storage: ScopedReservation<'ctx>,
    pub(super) product_definition_ids_by_source: BTreeMap<u64, Vec<ProductDefinitionId>>,
    pub(super) product_definition_ids_by_shape: BTreeMap<u64, ProductDefinitionId>,
}

fn join_product_references(
    ids: impl IntoIterator<Item = u64>,
    ctx: &DecodeContext<'_>,
    operation: &'static str,
) -> Result<String, CodecError> {
    let (mut parts, mut storage) = ctx.temporary_vec(0, "STEP product reference fragments")?;
    let mut ids = ids.into_iter();
    while let Some(id) = ctx.next_charged(&mut ids, operation)? {
        let part =
            storage.with_storage(|| ctx.format_retained(format_args!("#{id}"), operation))?;
        ctx.push_scoped_vec(
            &mut storage,
            &mut parts,
            part,
            "STEP product reference fragments",
        )?;
    }
    ctx.join_retained(&parts, ", ", operation)
}

pub(super) fn decode<'ctx>(
    exchange: &Exchange,
    geometry: &GeometryData,
    topology: &TopologyData,
    ir: &mut CadIr,
    ctx: &'ctx DecodeContext<'_>,
    admitted_ir_entities: &mut u64,
) -> Result<
    StageOutcome<(
        ProductData<'ctx>,
        ScopedReservation<'ctx>,
        ScopedReservation<'ctx>,
    )>,
    CodecError,
> {
    let mut scratch_storage = ctx.reserve_scoped(0, "STEP decode scratch")?;
    let slot_storage = std::cell::RefCell::new(ctx.reserve_scoped(0, "STEP stage report buffers")?);
    let mut index_storage = ctx.reserve_scoped(0, "STEP product result indices")?;
    let mut claim_storage = ctx.reserve_scoped(0, "STEP product claims")?;
    let mut typed = BTreeSet::new();
    let mut losses = Vec::new();
    let mut formations = BTreeMap::new();
    for entity in exchange.entities_any(ctx, PRODUCT_DEFINITION_FORMATION_TYPES)? {
        let (id, record) = entity?;
        let Some(product) = product_definition_formation_parameters(ctx, record)?
            .and_then(|parameters| parameters.get(2))
            .and_then(ValueExt::reference)
        else {
            continue;
        };
        scratch_storage.with_storage(|| {
            ctx.insert_btree_map(&mut formations, id, product, "step_product_formations")
        })?;
    }
    let mut definitions = BTreeMap::new();
    for entity in exchange.entities_any(ctx, PRODUCT_DEFINITION_TYPES)? {
        let (id, record) = entity?;
        let Some(product) = product_definition_parameters(ctx, record)?
            .and_then(|parameters| parameters.get(2))
            .and_then(ValueExt::reference)
            .map(|formation| {
                ctx.get_btree_map(&formations, &formation, "STEP product formations get")
            })
            .transpose()?
            .flatten()
            .copied()
        else {
            continue;
        };
        scratch_storage.with_storage(|| {
            ctx.insert_btree_map(&mut definitions, id, product, "step_product_definitions")
        })?;
    }
    let mut definitions_by_product_in_source_order = BTreeMap::<u64, Vec<(u64, usize)>>::new();
    for (&definition, &product) in ctx.admit_iter(&definitions, "STEP decode traversal")? {
        let grouped = scratch_storage
            .with_storage(|| {
                ctx.entry_btree_map(
                    &mut definitions_by_product_in_source_order,
                    product,
                    "step_product_definition_groups",
                )
            })?
            .or_default();
        scratch_storage.with_storage(|| {
            ctx.reserve_vec(grouped, 1, "step_product_definition_group_members")
        })?;
        let offset = ctx
            .get_btree_map(
                exchange.records(),
                &definition,
                "STEP product definition source offset",
            )?
            .ok_or_else(|| CodecError::malformed("STEP product definition was not indexed"))?
            .span
            .start;
        grouped.push((definition, offset));
    }
    for (_, definitions) in ctx.admit_iter(
        &mut definitions_by_product_in_source_order,
        "STEP product definition group traversal",
    )? {
        ctx.stable_sort_by(
            definitions,
            |value| &value.1,
            Ord::cmp,
            "step_product_definition_group_sort",
        )?;
    }
    let mut definition_descriptions = BTreeMap::<u64, String>::new();
    for entity in exchange.entities_any(ctx, PRODUCT_DEFINITION_TYPES)? {
        let (id, record) = entity?;
        let Some(parameters) = product_definition_parameters(ctx, record)? else {
            continue;
        };
        let Some(_) = parameters
            .get(2)
            .and_then(ValueExt::reference)
            .map(|formation| {
                ctx.get_btree_map(&formations, &formation, "STEP product formations get")
            })
            .transpose()?
            .flatten()
            .copied()
        else {
            continue;
        };
        let Some(description) = parameters
            .get(1)
            .map(|value| {
                decode_text_scoped(
                    exchange,
                    value,
                    (&mut losses, &slot_storage),
                    id,
                    (
                        "product definition description",
                        StepLossCode::MetadataStringInvalid,
                    ),
                    ctx,
                    &mut scratch_storage,
                )
            })
            .transpose()?
            .flatten()
        else {
            continue;
        };
        if !description.is_empty() {
            scratch_storage
                .with_storage(|| {
                    ctx.entry_btree_map(
                        &mut definition_descriptions,
                        id,
                        "step_product_definition_descriptions",
                    )
                })?
                .or_insert(description);
        }
    }
    let (mut shape_bindings, _shape_storage) = ctx
        .with_scoped_storage("STEP product shape bindings scratch", || {
            shape_bindings(exchange, &definitions, topology, ctx)
        })?;
    let (body_ids, _body_index_storage) =
        ctx.with_scoped_storage("STEP product body membership index", || {
            ctx.collect_btree_set(
                ir.model.bodies.iter().map(|body| body.id.as_str()),
                "STEP product body membership index",
            )
        })?;
    let mut prototype_copy_storage =
        ctx.reserve_scoped(0, "step_product_prototype_identity_copy")?;
    let mut definition_prototypes = BTreeMap::<u64, ProductDefinitionId>::new();
    let mut product_definition_ids_by_source = BTreeMap::<u64, Vec<ProductDefinitionId>>::new();

    for (step_id, record) in exchange.entities(ctx, "PRODUCT")? {
        let Some(parameters) = record
            .partial(ctx, "PRODUCT")?
            .map(|partial| partial.parameters.as_slice())
        else {
            continue;
        };
        let mut text_storage = ctx.reserve_scoped(0, "STEP product source text")?;
        let product_id = parameters
            .first()
            .map(|value| {
                decode_text_scoped(
                    exchange,
                    value,
                    (&mut losses, &slot_storage),
                    step_id,
                    ("product identifier", StepLossCode::MetadataStringInvalid),
                    ctx,
                    &mut text_storage,
                )
            })
            .transpose()?
            .flatten()
            .unwrap_or_else(|| format!("#{step_id}"));
        let name = parameters
            .get(1)
            .map(|value| {
                decode_text_scoped(
                    exchange,
                    value,
                    (&mut losses, &slot_storage),
                    step_id,
                    ("product name", StepLossCode::MetadataStringInvalid),
                    ctx,
                    &mut text_storage,
                )
            })
            .transpose()?
            .flatten()
            .filter(|name| !name.is_empty());
        let product_description = parameters
            .get(2)
            .map(|value| {
                decode_text_scoped(
                    exchange,
                    value,
                    (&mut losses, &slot_storage),
                    step_id,
                    ("product description", StepLossCode::MetadataStringInvalid),
                    ctx,
                    &mut text_storage,
                )
            })
            .transpose()?
            .flatten()
            .filter(|description| !description.is_empty());
        let product_definitions = ctx
            .get_btree_map(
                &definitions_by_product_in_source_order,
                &step_id,
                "STEP product definitions_by_product_in_source_order get",
            )?
            .map(Vec::as_slice)
            .unwrap_or_default();
        let definition_count = product_definitions.len();
        let definition_iter = std::iter::once(None)
            .filter(|_| product_definitions.is_empty())
            .chain(
                ctx.admit_iter(product_definitions, "STEP decode chain traversal")?
                    .map(|(id, _)| Some(*id)),
            );
        for definition in definition_iter {
            let product_definition_id = definition.map_or_else(
                || Ok::<ProductDefinitionId, CodecError>(product_ir_id(step_id)),
                |definition| {
                    let id = product_definition_ir_id(step_id, definition, definition_count);

                    scratch_storage.with_storage(|| {
                        ctx.insert_btree_map(
                            &mut definition_prototypes,
                            definition,
                            prototype_copy_storage.with_storage(|| {
                                id.try_clone_for_decode(ctx, "step_product_prototype_identity_copy")
                            })?,
                            "step_product_definition_prototypes",
                        )
                    })?;
                    Ok(id)
                },
            )?;
            let definition_description = definition
                .map(|definition| {
                    ctx.get_btree_map(
                        &definition_descriptions,
                        &definition,
                        "STEP product definition_descriptions get",
                    )
                })
                .transpose()?
                .flatten()
                .map(String::as_str);
            let description = if definition_count <= 1 {
                product_description
                    .as_deref()
                    .map(|text| (text, "step_product_description_copy"))
                    .or(definition_description
                        .map(|text| (text, "step_product_definition_description_copy")))
            } else {
                definition_description
                    .map(|text| (text, "step_product_definition_description_copy"))
                    .or(product_description
                        .as_deref()
                        .map(|text| (text, "step_product_description_copy")))
            }
            .map(|(text, operation)| ctx.copy_retained_text(text, operation))
            .transpose()?;
            let has_shape_binding = definition
                .map(|definition| {
                    ctx.contains_key_btree_map(
                        &shape_bindings,
                        &definition,
                        "STEP product shape_bindings contains_key",
                    )
                })
                .transpose()?
                .unwrap_or(false);
            let source_bodies = definition
                .map(|definition| {
                    ctx.remove_btree_map(
                        &mut shape_bindings,
                        &definition,
                        "STEP product shape_bindings remove",
                    )
                })
                .transpose()?
                .flatten()
                .unwrap_or_default();
            let (mut missing, mut missing_storage) =
                ctx.temporary_vec(0, "STEP missing shape body fragments")?;
            let mut selected_bodies = BTreeSet::new();
            let mut selected_storage = ctx.reserve_scoped(0, "STEP product selected body index")?;
            for batch in ctx.admit_iter(&source_bodies, "STEP product shape batch traversal")? {
                for body in ctx.admit_iter(&batch[..], "STEP product shape body traversal")? {
                    if ctx.contains_btree_set(
                        &body_ids,
                        body.as_str(),
                        "STEP product body membership",
                    )? {
                        selected_storage.with_storage(|| {
                            ctx.insert_btree_set(
                                &mut selected_bodies,
                                body,
                                "STEP product selected body index",
                            )
                        })?;
                    } else {
                        ctx.push_scoped_vec(
                            &mut missing_storage,
                            &mut missing,
                            body.as_str(),
                            "STEP missing shape body fragments",
                        )?;
                    }
                }
            }
            let mut bodies =
                ctx.collection_vec(selected_bodies.len(), "STEP product committed body slots")?;
            for body in ctx.admit_iter(selected_bodies, "STEP product selected body traversal")? {
                bodies
                    .push(body.try_clone_for_decode(ctx, "STEP product committed body identity")?);
            }
            let owner = definition.map_or_else(
                || format!("PRODUCT #{step_id}"),
                |definition| format!("PRODUCT_DEFINITION #{definition}"),
            );
            if !missing.is_empty() {
                let (missing, _missing_text_storage) = ctx
                    .with_scoped_storage("STEP missing shape body text scratch", || {
                        ctx.join_retained(&missing, ", ", "step_missing_shape_body_text")
                    })?;
                slot_storage
                    .borrow_mut()
                    .with_storage(|| ctx.reserve_vec(&mut losses, 1, "step_product_losses"))?;
                losses.push(StepLossCode::DecodeWarning.note(ctx.format_retained(
                    format_args!("{owner} omitted uncommitted shape body reference(s): {missing}"),
                    "step_missing_shape_body_loss_text",
                )?));
            }
            if has_shape_binding && bodies.is_empty() {
                slot_storage
                    .borrow_mut()
                    .with_storage(|| ctx.reserve_vec(&mut losses, 1, "step_product_losses"))?;
                losses.push(StepLossCode::DecodeWarning.note(ctx.format_retained(
                    format_args!(
                        "{owner} has a shape representation with no committed topology body"
                    ),
                    "STEP decode text",
                )?));
            }
            ctx.reserve_vec(
                &mut ir.model.product_definitions,
                1,
                "step_product_definition_ir_items",
            )?;
            ir.model.product_definitions.push(ProductDefinition {
                id: product_definition_id
                    .try_clone_for_decode(ctx, "step_product_identity_copy")?,
                kind: ProductDefinitionKind::Part,
                source_name: name
                    .as_deref()
                    .map(|text| ctx.copy_retained_text(text, "step_product_source_name_copy"))
                    .transpose()?,
                label: name
                    .as_deref()
                    .map(|text| ctx.copy_retained_text(text, "step_product_label_copy"))
                    .transpose()?,
                description,
                part_number: Some(
                    ctx.copy_retained_text(&product_id, "step_product_part_number_copy")?,
                ),
                bom_properties: BTreeMap::new(),
                bodies,
                native_ref: Some(
                    definition.map_or_else(|| format!("#{step_id}"), |id| format!("#{id}")),
                ),
            });

            let grouped = index_storage
                .with_storage(|| {
                    ctx.entry_btree_map(
                        &mut product_definition_ids_by_source,
                        step_id,
                        "step_product_source_groups",
                    )
                })?
                .or_default();
            index_storage.with_storage(|| {
                ctx.reserve_vec(grouped, 1, "step_product_source_group_members")
            })?;
            grouped.push(index_storage.with_storage(|| {
                product_definition_id
                    .try_clone_for_decode(ctx, "STEP product source index identity")
            })?);
        }
        claim_storage.with_storage(|| {
            ctx.insert_btree_set(&mut typed, step_id, "step_product_typed_claims")
        })?;
    }
    let mut product_definition_ids_by_shape = BTreeMap::new();
    for (shape_id, record) in exchange.entities(ctx, "PRODUCT_DEFINITION_SHAPE")? {
        let Some(prototype) = record
            .partial(ctx, "PRODUCT_DEFINITION_SHAPE")?
            .and_then(|partial| partial.parameters.get(2))
            .and_then(ValueExt::reference)
            .map(|definition| {
                ctx.get_btree_map(
                    &definition_prototypes,
                    &definition,
                    "STEP product definition_prototypes get",
                )
            })
            .transpose()?
            .flatten()
        else {
            continue;
        };

        index_storage.with_storage(|| {
            ctx.insert_btree_map(
                &mut product_definition_ids_by_shape,
                shape_id,
                prototype.try_clone_for_decode(ctx, "step_product_identity_copy")?,
                "step_product_shape_prototypes",
            )
        })?;
    }
    for id in ctx
        .admit_iter(&(formations), "STEP decode map traversal")?
        .map(|(key, _)| key)
        .chain(
            ctx.admit_iter(&definitions, "STEP decode chain traversal")?
                .map(|(key, _)| key),
        )
    {
        claim_storage
            .with_storage(|| ctx.insert_btree_set(&mut typed, *id, "step_product_typed_claims"))?;
    }

    let mut usages = BTreeMap::new();
    for (id, record) in exchange.entities(ctx, "NEXT_ASSEMBLY_USAGE_OCCURRENCE")? {
        let name = record
            .partial(ctx, "NEXT_ASSEMBLY_USAGE_OCCURRENCE")?
            .and_then(|partial| partial.parameters.get(1))
            .map(|value| {
                decode_text_scoped(
                    exchange,
                    value,
                    (&mut losses, &slot_storage),
                    id,
                    (
                        "assembly occurrence name",
                        StepLossCode::MetadataStringInvalid,
                    ),
                    ctx,
                    &mut scratch_storage,
                )
            })
            .transpose()?
            .flatten();
        let Some(parent_definition) = record
            .partial(ctx, "NEXT_ASSEMBLY_USAGE_OCCURRENCE")?
            .and_then(|partial| partial.parameters.get(3))
            .and_then(ValueExt::reference)
        else {
            continue;
        };
        let Some(child_definition) = record
            .partial(ctx, "NEXT_ASSEMBLY_USAGE_OCCURRENCE")?
            .and_then(|partial| partial.parameters.get(4))
            .and_then(ValueExt::reference)
        else {
            continue;
        };
        scratch_storage.with_storage(|| {
            ctx.insert_btree_map(
                &mut usages,
                id,
                Usage {
                    parent_definition,
                    child_definition,
                    name: name.filter(|name| !name.is_empty()),
                },
                "step_product_usage_entries",
            )
        })?;
    }
    let mut child_definitions = BTreeSet::new();
    for usage in ctx
        .admit_iter(&(usages), "STEP decode map traversal")?
        .map(|(_, value)| value)
    {
        scratch_storage.with_storage(|| {
            ctx.insert_btree_set(
                &mut child_definitions,
                usage.child_definition,
                "step_product_child_definitions",
            )
        })?;
    }
    let mut occurrence_index_copy_storage =
        ctx.reserve_scoped(0, "step_product_occurrence_index_identity_copy")?;
    let mut occurrence_paths = BTreeMap::<OccurrenceId, BTreeSet<u64>>::new();
    let mut pending_occurrences = VecDeque::new();
    let mut root_ordinal = 0_u32;
    for &definition in ctx
        .admit_iter(&(definitions), "STEP decode map traversal")?
        .map(|(key, _)| key)
    {
        if ctx.contains_btree_set(
            &child_definitions,
            &definition,
            "STEP product child_definitions contains",
        )? {
            continue;
        }
        let Some(prototype) = ctx.get_btree_map(
            &definition_prototypes,
            &definition,
            "STEP product definition_prototypes get",
        )?
        else {
            slot_storage
                .borrow_mut()
                .with_storage(|| ctx.reserve_vec(&mut losses, 1, "step_product_losses"))?;
            losses.push(StepLossCode::DecodeWarning.note(format!(
                "PRODUCT_DEFINITION #{definition} has no local product prototype"
            )));
            continue;
        };
        let id = OccurrenceId::from(ids::product(
            kind!("occurrence"),
            key_word!("definition").dash(definition),
        ));
        let occurrence_cap = occurrence_limit(ctx);
        if ir.model.occurrences.len() >= occurrence_cap {
            return Err(ctx.refuse_codec_limit(
                "step_assembly_occurrence_limit",
                u64_from_index(occurrence_cap),
                u64_from_index(ir.model.occurrences.len()) + 1,
            ));
        }
        ctx.reserve_vec(&mut ir.model.occurrences, 1, "step_root_occurrence_items")?;
        ir.model.occurrences.push(Occurrence {
            id: id.try_clone_for_decode(ctx, "step_product_identity_copy")?,
            prototype: PrototypeReference::Local {
                definition: prototype
                    .try_clone_for_decode(ctx, "step_product_definition_identity_copy")?,
            },
            parent: OccurrenceParent::Root {},
            ordinal: root_ordinal,
            transform: Transform::identity(),
            linked_prototype: None,
            scale: [cadmpeg_ir::scalar::FiniteReal::ONE; 3],
            name: None,
            visible: None,
            link: None,
            native_ref: None,
        });
        ctx.admit_entities(
            u64_from_index(ir.model.entity_count()),
            admitted_ir_entities,
            "step_assembly_occurrence",
        )?;
        root_ordinal = root_ordinal
            .checked_add(1)
            .ok_or_else(|| CodecError::malformed("STEP root occurrence ordinal exceeds u32"))?;
        let mut root_path = BTreeSet::new();
        scratch_storage.with_storage(|| {
            ctx.insert_btree_set(
                &mut root_path,
                definition,
                "step_root_occurrence_path_members",
            )
        })?;

        scratch_storage.with_storage(|| {
            ctx.insert_btree_map(
                &mut occurrence_paths,
                occurrence_index_copy_storage.with_storage(|| {
                    id.try_clone_for_decode(ctx, "step_product_occurrence_index_identity_copy")
                })?,
                root_path,
                "step_root_occurrence_path_map",
            )
        })?;
        scratch_storage.with_storage(|| {
            ctx.push_back(
                &mut pending_occurrences,
                (definition, id),
                "step_pending_occurrence",
            )
        })?;
    }
    let mut ambiguous_placements = BTreeMap::new();
    let mut competing_placements = BTreeMap::new();
    let placements = occurrence_placements(
        exchange,
        geometry,
        &usages,
        (&mut losses, &slot_storage),
        (&mut ambiguous_placements, &mut competing_placements),
        &mut scratch_storage,
        ctx,
    )?;
    for (&usage_id, source_ids) in ctx.admit_iter(&ambiguous_placements, "STEP decode traversal")? {
        if ctx.contains_key_btree_map(
            &competing_placements,
            &usage_id,
            "STEP product competing_placements contains_key",
        )? {
            continue;
        }
        let (records, _record_storage) =
            ctx.with_scoped_storage("STEP product placement detail scratch", || {
                join_product_references(
                    source_ids.iter().copied(),
                    ctx,
                    "step_ambiguous_placement_source_text",
                )
            })?;
        let context_dependent = ctx.all_by(
            source_ids,
            |id| {
                Ok(ctx
                    .get_btree_map(exchange.records(), id, "STEP product record get")?
                    .map(|record| record.partial(ctx, "CONTEXT_DEPENDENT_SHAPE_REPRESENTATION"))
                    .transpose()?
                    .flatten()
                    .is_some())
            },
            "STEP context dependent placement source search",
        )?;
        let placement_kind = if context_dependent {
            "CONTEXT_DEPENDENT_SHAPE_REPRESENTATION"
        } else {
            "occurrence-owned mapped"
        };
        slot_storage
            .borrow_mut()
            .with_storage(|| ctx.reserve_vec(&mut losses, 1, "step_product_losses"))?;
        losses.push(StepLossCode::NauoPlacementAmbiguous.note(ctx.format_retained(format_args!("NAUO #{usage_id} has multiple resolved {placement_kind} placements ({records}); no neutral occurrence was admitted and the source placement relations remain opaque"), "step_ambiguous_placement_loss_text")?));
    }
    for (&usage_id, source_ids) in ctx.admit_iter(&competing_placements, "STEP decode traversal")? {
        let (records, _record_storage) =
            ctx.with_scoped_storage("STEP product placement detail scratch", || {
                join_product_references(
                    source_ids.iter().copied(),
                    ctx,
                    "step_competing_placement_source_text",
                )
            })?;
        slot_storage
            .borrow_mut()
            .with_storage(|| ctx.reserve_vec(&mut losses, 1, "step_product_losses"))?;
        losses.push(StepLossCode::NauoPlacementAmbiguous.note(ctx.format_retained(format_args!("NAUO #{usage_id} has resolved context-dependent and occurrence-owned mapped placements ({records}); no neutral occurrence was admitted and the source placement relations remain opaque"), "step_competing_placement_loss_text")?));
    }
    let mut usage_instances = BTreeMap::<u64, usize>::new();
    let mut missing_placement_reports = BTreeSet::new();
    let mut child_ordinals = BTreeMap::<OccurrenceId, u32>::new();
    let mut usages_by_parent = BTreeMap::<u64, Vec<u64>>::new();
    for (&usage_id, usage) in ctx.admit_iter(&usages, "STEP decode traversal")? {
        let grouped = scratch_storage
            .with_storage(|| {
                ctx.entry_btree_map(
                    &mut usages_by_parent,
                    usage.parent_definition,
                    "step_product_usage_parent_groups",
                )
            })?
            .or_default();
        scratch_storage
            .with_storage(|| ctx.reserve_vec(grouped, 1, "step_product_usage_parent_members"))?;
        grouped.push(usage_id);
    }
    let had_roots = !pending_occurrences.is_empty();
    while let Some((parent_definition, parent)) = pending_occurrences.pop_front() {
        ctx.charge_work(1, "STEP product worklist step")?;
        for &usage_id in ctx.admit_iter(
            ctx.get_btree_map(
                &usages_by_parent,
                &parent_definition,
                "STEP product usages_by_parent get",
            )?
            .map(Vec::as_slice)
            .unwrap_or_default(),
            "STEP child usage traversal",
        )? {
            if ctx.contains_key_btree_map(
                &ambiguous_placements,
                &usage_id,
                "STEP product ambiguous_placements contains_key",
            )? {
                continue;
            }
            let usage = ctx
                .get_btree_map(&usages, &usage_id, "STEP child usage lookup")?
                .ok_or_else(|| CodecError::malformed("STEP child usage was not indexed"))?;
            let Some(prototype) = ctx.get_btree_map(
                &definition_prototypes,
                &usage.child_definition,
                "STEP product definition_prototypes get",
            )?
            else {
                slot_storage
                    .borrow_mut()
                    .with_storage(|| ctx.reserve_vec(&mut losses, 1, "step_product_losses"))?;
                losses.push(StepLossCode::DecodeWarning.note(format!(
                    "NAUO #{usage_id} references an unresolved child definition"
                )));
                continue;
            };
            let parent_path = ctx.get_btree_map(
                &occurrence_paths,
                &parent,
                "STEP product occurrence_paths get",
            )?;
            let depth_limit = assembly_depth_limit(ctx);
            if parent_path.is_some_and(|path| path.len() >= depth_limit) {
                return Err(ctx.refuse_codec_limit(
                    "step_assembly_depth_limit",
                    u64_from_index(depth_limit),
                    u64_from_index(depth_limit) + 1,
                ));
            }
            if parent_path
                .map(|path| {
                    ctx.contains_btree_set(
                        path,
                        &usage.child_definition,
                        "STEP assembly path membership",
                    )
                })
                .transpose()?
                .unwrap_or(false)
            {
                slot_storage
                    .borrow_mut()
                    .with_storage(|| ctx.reserve_vec(&mut losses, 1, "step_product_losses"))?;
                losses.push(StepLossCode::DecodeWarning.note(format!(
                    "NAUO #{usage_id} closes an assembly definition cycle"
                )));
                continue;
            }

            let instance = scratch_storage
                .with_storage(|| {
                    ctx.entry_btree_map(
                        &mut usage_instances,
                        usage_id,
                        "step_usage_instance_counts",
                    )
                })?
                .or_default();
            *instance += 1;
            let suffix = if *instance == 1 {
                IdentityKeyTail::empty()
            } else {
                IdentityKeyTail::empty()
                    .dash(key_word!("instance"))
                    .dash(*instance)
            };
            let id = OccurrenceId::from(ids::product(
                kind!("occurrence"),
                IdentityKey::from(usage_id).with_tail(&suffix),
            ));
            let occurrence_cap = occurrence_limit(ctx);
            if ir.model.occurrences.len() >= occurrence_cap {
                return Err(ctx.refuse_codec_limit(
                    "step_assembly_occurrence_limit",
                    u64_from_index(occurrence_cap),
                    u64_from_index(ir.model.occurrences.len()) + 1,
                ));
            }

            let ordinal = scratch_storage
                .with_storage(|| {
                    ctx.entry_btree_map(
                        &mut child_ordinals,
                        occurrence_index_copy_storage.with_storage(|| {
                            parent.try_clone_for_decode(
                                ctx,
                                "step_product_occurrence_index_identity_copy",
                            )
                        })?,
                        "step_child_occurrence_ordinals",
                    )
                })?
                .or_default();
            let transform = if let Some(transform) = ctx
                .get_btree_map(&placements, &usage_id, "STEP product placements get")?
                .copied()
            {
                transform
            } else {
                if scratch_storage.with_storage(|| {
                    ctx.insert_btree_set(
                        &mut missing_placement_reports,
                        usage_id,
                        "step_missing_placement_reports",
                    )
                })? {
                    slot_storage
                        .borrow_mut()
                        .with_storage(|| ctx.reserve_vec(&mut losses, 1, "step_product_losses"))?;
                    losses.push(StepLossCode::NauoPlacementUnresolved.note(format!(
                        "NAUO #{usage_id} has no resolved occurrence transform; \
                             identity placement was used"
                    )));
                }
                Transform::identity()
            };
            ctx.reserve_vec(&mut ir.model.occurrences, 1, "step_child_occurrence_items")?;
            ir.model.occurrences.push(Occurrence {
                id: id.try_clone_for_decode(ctx, "step_product_identity_copy")?,
                prototype: PrototypeReference::Local {
                    definition: prototype
                        .try_clone_for_decode(ctx, "step_product_definition_identity_copy")?,
                },
                parent: OccurrenceParent::Occurrence {
                    occurrence: parent.try_clone_for_decode(ctx, "step_product_identity_copy")?,
                },
                ordinal: *ordinal,
                transform,
                linked_prototype: None,
                scale: [cadmpeg_ir::scalar::FiniteReal::ONE; 3],
                name: usage
                    .name
                    .as_deref()
                    .map(|text| ctx.copy_retained_text(text, "step_product_occurrence_name_copy"))
                    .transpose()?,
                visible: None,
                link: None,
                native_ref: Some(format!("#{usage_id}")),
            });
            ctx.admit_entities(
                u64_from_index(ir.model.entity_count()),
                admitted_ir_entities,
                "step_assembly_occurrence",
            )?;
            *ordinal = ordinal.checked_add(1).ok_or_else(|| {
                CodecError::malformed("STEP child occurrence ordinal exceeds u32")
            })?;
            let mut path = BTreeSet::new();
            if let Some(parent_path) = parent_path {
                for &definition in
                    ctx.admit_iter(parent_path, "STEP product parent_path traversal")?
                {
                    scratch_storage.with_storage(|| {
                        ctx.insert_btree_set(
                            &mut path,
                            definition,
                            "step_child_occurrence_path_members",
                        )
                    })?;
                }
            }
            scratch_storage.with_storage(|| {
                ctx.insert_btree_set(
                    &mut path,
                    usage.child_definition,
                    "step_child_occurrence_path_members",
                )
            })?;

            scratch_storage.with_storage(|| {
                ctx.insert_btree_map(
                    &mut occurrence_paths,
                    occurrence_index_copy_storage.with_storage(|| {
                        id.try_clone_for_decode(ctx, "step_product_occurrence_index_identity_copy")
                    })?,
                    path,
                    "step_child_occurrence_path_map",
                )
            })?;
            scratch_storage.with_storage(|| {
                ctx.push_back(
                    &mut pending_occurrences,
                    (usage.child_definition, id),
                    "step_pending_occurrence",
                )
            })?;
            claim_storage.with_storage(|| {
                ctx.insert_btree_set(&mut typed, usage_id, "step_product_typed_claims")
            })?;
        }
    }
    if !had_roots && !usages.is_empty() {
        slot_storage
            .borrow_mut()
            .with_storage(|| ctx.reserve_vec(&mut losses, 1, "step_product_losses"))?;
        losses.push(
            StepLossCode::DecodeWarning.note("assembly occurrence graph has no resolvable root"),
        );
    }
    apply_body_placements(
        exchange,
        BodyPlacementSources {
            geometry,
            topology,
            usages: &usages,
        },
        ir,
        (&mut losses, &slot_storage),
        ctx,
    )?;
    for entity in exchange.entities_any(
        ctx,
        &[
            "APPLICATION_CONTEXT",
            "PRODUCT_CONTEXT",
            "PRODUCT_DEFINITION_CONTEXT",
            "PRODUCT_DEFINITION_SHAPE",
            "SHAPE_DEFINITION_REPRESENTATION",
            "ITEM_DEFINED_TRANSFORMATION",
            "CONTEXT_DEPENDENT_SHAPE_REPRESENTATION",
            "REPRESENTATION_MAP",
            "MAPPED_ITEM",
            "SHAPE_REPRESENTATION_RELATIONSHIP",
            "REPRESENTATION_RELATIONSHIP_WITH_TRANSFORMATION",
        ],
    )? {
        let (id, record) = entity?;
        if [
            "APPLICATION_CONTEXT",
            "PRODUCT_CONTEXT",
            "PRODUCT_DEFINITION_CONTEXT",
            "PRODUCT_DEFINITION_SHAPE",
            "SHAPE_DEFINITION_REPRESENTATION",
            "ITEM_DEFINED_TRANSFORMATION",
            "CONTEXT_DEPENDENT_SHAPE_REPRESENTATION",
            "REPRESENTATION_MAP",
            "MAPPED_ITEM",
            "SHAPE_REPRESENTATION_RELATIONSHIP",
        ]
        .iter()
        .map(|name| record.partial(ctx, name))
        .find_map(Result::transpose)
        .transpose()?
        .is_some()
            || record
                .partial(ctx, "REPRESENTATION_RELATIONSHIP_WITH_TRANSFORMATION")?
                .is_some()
        {
            claim_storage.with_storage(|| {
                ctx.insert_btree_set(&mut typed, id, "step_product_typed_claims")
            })?;
        }
    }
    for (&usage_id, source_ids) in ctx.admit_iter(&ambiguous_placements, "STEP decode traversal")? {
        ctx.remove_btree_set(&mut typed, &usage_id, "step_product_typed_claims")?;
        for source_id in ctx.admit_iter(source_ids, "step_product_typed_claims")? {
            ctx.remove_btree_set(&mut typed, source_id, "step_product_typed_claims")?;
        }
    }
    Ok(StageOutcome {
        value: (
            ProductData {
                _storage: index_storage,
                product_definition_ids_by_source,
                product_definition_ids_by_shape,
            },
            claim_storage,
            slot_storage.into_inner(),
        ),
        claims: typed,
        losses,
        notes: Vec::new(),
    })
}

fn occurrence_limit(ctx: &DecodeContext<'_>) -> usize {
    usize::try_from(ctx.policy().limits.max_entities)
        .ok()
        .map_or(MAX_OCCURRENCES, |policy| policy.min(MAX_OCCURRENCES))
}

fn assembly_depth_limit(ctx: &DecodeContext<'_>) -> usize {
    usize::try_from(ctx.policy().limits.max_recursion_depth)
        .ok()
        .map_or(MAX_ASSEMBLY_DEPTH, |policy| policy.min(MAX_ASSEMBLY_DEPTH))
}

#[derive(Clone, Copy)]
struct BodyPlacementSources<'a> {
    geometry: &'a GeometryData,
    topology: &'a TopologyData,
    usages: &'a BTreeMap<u64, Usage>,
}

fn apply_body_placements(
    exchange: &Exchange,
    sources: BodyPlacementSources<'_>,
    ir: &mut CadIr,
    (losses, slot_storage): (
        &mut Vec<LossNote>,
        &std::cell::RefCell<cadmpeg_core::decode::ScopedReservation<'_>>,
    ),
    ctx: &DecodeContext<'_>,
) -> Result<(), CodecError> {
    let mut scratch_storage = ctx.reserve_scoped(0, "STEP apply_body_placements scratch")?;
    let BodyPlacementSources {
        geometry,
        topology,
        usages,
    } = sources;
    let mut pds = BTreeMap::new();
    for (id, record) in exchange.entities(ctx, "PRODUCT_DEFINITION_SHAPE")? {
        if let Some(definition) = record
            .partial(ctx, "PRODUCT_DEFINITION_SHAPE")?
            .and_then(|partial| partial.parameters.get(2))
            .and_then(ValueExt::reference)
        {
            scratch_storage.with_storage(|| {
                ctx.insert_btree_map(&mut pds, id, definition, "step_body_placement_shapes")
            })?;
        }
    }
    let (definition_representations, _representation_storage) = ctx
        .with_scoped_storage("STEP definition representation scratch", || {
            definition_representations(exchange, &pds, ctx)
        })?;
    let mut assembly_representations = BTreeSet::new();
    for (_, usage) in ctx.admit_iter(usages, "STEP assembly usage traversal")? {
        if let Some(representations) = ctx.get_btree_map(
            &definition_representations,
            &usage.child_definition,
            "STEP product definition_representations get",
        )? {
            for representation in
                ctx.admit_iter(representations, "STEP assembly representation traversal")?
            {
                scratch_storage.with_storage(|| {
                    ctx.insert_btree_set(
                        &mut assembly_representations,
                        *representation,
                        "step_assembly_representations",
                    )
                })?;
            }
        }
    }
    let mut body_index_copy_storage = ctx.reserve_scoped(0, "step_body_placement_identity_copy")?;
    let mut body_indices = BTreeMap::new();
    for (index, body) in ctx
        .admit_iter(
            &(ir.model.bodies)[..],
            "STEP apply body placements traversal",
        )?
        .enumerate()
    {
        scratch_storage.with_storage(|| {
            ctx.insert_btree_map(
                &mut body_indices,
                body_index_copy_storage.with_storage(|| {
                    body.id
                        .try_clone_for_decode(ctx, "step_body_placement_identity_copy")
                })?,
                index,
                "step_body_placement_indices",
            )
        })?;
    }
    let mut representation_cache = BTreeMap::new();
    let mut placements_by_body = BTreeMap::<&BodyId, Vec<(u64, Transform)>>::new();
    let (mut placement_sources, mut placement_source_storage) =
        ctx.temporary_vec(0, "STEP mapped body placement sources")?;
    let (drawing_owned_items, _drawing_storage) = ctx
        .with_scoped_storage("STEP drawing owned item scratch", || {
            drawing_owned_items(exchange, ctx)
        })?;
    for (id, item) in exchange.entities(ctx, "MAPPED_ITEM")? {
        if item.partial(ctx, "MAPPED_ITEM")?.is_none() {
            continue;
        }
        if ctx.contains_btree_set(
            &drawing_owned_items,
            &id,
            "STEP product drawing_owned_items contains",
        )? {
            continue;
        }
        let Some((representation, origin, target)) = mapped_item_definition(ctx, item, exchange)?
        else {
            continue;
        };
        if ctx.contains_btree_set(
            &assembly_representations,
            &representation,
            "STEP product assembly_representations contains",
        )? {
            continue;
        }
        if is_two_dimensional_mapping(ctx, origin, target, exchange)? {
            continue;
        }
        let bodies = super::topology::representation_bodies(
            representation,
            exchange,
            topology,
            &mut representation_cache,
            &mut BTreeSet::new(),
            ctx,
        )?;
        if bodies.is_empty() {
            continue;
        }
        let transform = match mapped_item_transform(ctx, origin, target, geometry)? {
            Ok(Some(transform)) => transform,
            Ok(None) | Err(TransformError::Singular) => {
                slot_storage
                    .borrow_mut()
                    .with_storage(|| ctx.reserve_vec(losses, 1, "step_product_losses"))?;
                losses.push(
                    StepLossCode::DecodeWarning
                        .note(format!("MAPPED_ITEM #{id} has no resolved body placement")),
                );
                continue;
            }
            Err(error) => return Err(placement_error(error)),
        };
        ctx.push_scoped_vec(
            &mut placement_source_storage,
            &mut placement_sources,
            (id, bodies.into_parts(), transform),
            "STEP mapped body placement sources",
        )?;
    }
    for (id, (bodies, _body_storage), transform) in ctx.admit_iter(
        &placement_sources,
        "STEP mapped body placement source traversal",
    )? {
        for body in ctx.admit_iter(&bodies[..], "STEP product body_ids traversal")? {
            let grouped = scratch_storage
                .with_storage(|| {
                    ctx.entry_btree_map(&mut placements_by_body, body, "step_body_placement_groups")
                })?
                .or_default();
            scratch_storage.with_storage(|| {
                ctx.reserve_vec(grouped, 1, "step_body_placement_group_members")
            })?;
            grouped.push((*id, *transform));
        }
    }
    for (body, placements) in ctx.admit_iter(
        placements_by_body,
        "STEP product placements_by_body traversal",
    )? {
        let mut seen_transforms = BTreeSet::new();
        let mut transform_storage = ctx.reserve_scoped(0, "STEP body transform index")?;
        let mut unique = Vec::<(u64, Transform)>::new();
        for placement in ctx.admit_iter(placements, "STEP product placements traversal")? {
            if transform_storage.with_storage(|| {
                ctx.insert_btree_set(
                    &mut seen_transforms,
                    transform_key(placement.1),
                    "STEP body transform index",
                )
            })? {
                scratch_storage.with_storage(|| {
                    ctx.reserve_vec(&mut unique, 1, "step_unique_body_placements")
                })?;
                unique.push(placement);
            }
        }
        match unique.as_slice() {
            [(_, transform)] => {
                if let Some(index) =
                    ctx.get_btree_map(&body_indices, body, "STEP product body_indices get")?
                {
                    ir.model.bodies[*index].transform = Some(*transform);
                }
            }
            [] => {}
            _ => {
                let (mapped_items, _mapped_item_storage) =
                    ctx.with_scoped_storage("STEP body placement conflict detail scratch", || {
                        join_product_references(
                            unique.iter().map(|(id, _)| *id),
                            ctx,
                            "step_body_conflict_source_text",
                        )
                    })?;
                slot_storage
                    .borrow_mut()
                    .with_storage(|| ctx.reserve_vec(losses, 1, "step_product_losses"))?;
                losses.push(StepLossCode::BodyConflictingMappedPlacements.note(ctx.format_retained(format_args!("body {body} has conflicting standalone MAPPED_ITEM placements ({mapped_items}); no body placement was selected"), "step_body_conflict_loss_text")?));
            }
        }
    }
    Ok(())
}

fn drawing_owned_items(
    exchange: &Exchange,
    ctx: &DecodeContext<'_>,
) -> Result<BTreeSet<u64>, CodecError> {
    let mut pending = Vec::new();
    for record in ctx
        .admit_iter(exchange.records(), "STEP drawing owned items map traversal")?
        .map(|(_, value)| value)
    {
        let drawing_owner = ctx.any_by(
            &record.partials[..],
            |partial| Ok(DRAWING_ITEM_OWNER_TYPES.contains(&partial.name.as_str())),
            "STEP drawing owned items traversal",
        )?;
        if drawing_owner {
            for partial in
                ctx.admit_iter(&record.partials[..], "STEP drawing owned items traversal")?
            {
                for value in ctx.admit_iter(
                    partial.parameters.as_slice(),
                    "STEP record parameter traversal",
                )? {
                    collect_references(value, &mut pending, ctx)?;
                }
            }
        }
    }
    let mut items = BTreeSet::new();
    let mut visited = BTreeSet::new();
    while let Some(id) = pending.pop() {
        ctx.charge_work(1, "STEP product worklist step")?;
        if ctx.contains_btree_set(&visited, &id, "STEP product visited contains")? {
            continue;
        }
        ctx.insert_btree_set(&mut visited, id, "step_drawing_owned_visited")?;
        let Some(record) = ctx.get_btree_map(exchange.records(), &id, "STEP product record get")?
        else {
            continue;
        };
        if record.partial(ctx, "MAPPED_ITEM")?.is_some() {
            ctx.insert_btree_set(&mut items, id, "step_drawing_owned_items")?;
            continue;
        }
        if let Some(representation_items) = super::representation::items(ctx, record)? {
            for item in representation_items {
                ctx.push_vec(&mut pending, item, "step_drawing_owned_pending")?;
            }
        }
        for partial in ctx
            .admit_iter(&record.partials[..], "STEP drawing owned items traversal")?
            .filter(|partial| {
                matches!(
                    partial.name.as_str(),
                    "GEOMETRIC_SET" | "GEOMETRIC_CURVE_SET" | "TESSELLATED_GEOMETRIC_SET"
                )
            })
        {
            let Some(values) = ctx.find_map(
                &(partial.parameters)[..],
                |value| {
                    Ok(match value {
                        Value::List(values) => Some(values.as_slice()),
                        _ => None,
                    })
                },
                "STEP drawing owned items traversal",
            )?
            else {
                continue;
            };
            for value in ctx.admit_iter(values, "STEP product values traversal")? {
                collect_references(value, &mut pending, ctx)?;
            }
        }
    }
    Ok(items)
}

fn collect_references(
    value: &Value,
    references: &mut Vec<u64>,
    ctx: &DecodeContext<'_>,
) -> Result<(), CodecError> {
    let _nested = ctx.enter_nested("step_drawing_reference_walk")?;
    match value {
        Value::Reference(id) => {
            ctx.push_vec(references, *id, "step_drawing_owned_pending")?;
        }
        Value::List(values) => {
            for value in
                ctx.admit_iter(values.as_slice(), "STEP collect references value traversal")?
            {
                collect_references(value, references, ctx)?;
            }
        }
        Value::Typed(_, value) => collect_references(value, references, ctx)?,
        _ => {}
    }
    Ok(())
}

struct Usage {
    parent_definition: u64,
    child_definition: u64,
    name: Option<String>,
}

fn shape_bindings<'ctx>(
    exchange: &Exchange,
    definitions: &BTreeMap<u64, u64>,
    topology: &TopologyData,
    ctx: &'ctx DecodeContext<'_>,
) -> Result<BTreeMap<u64, Vec<super::topology::AdmittedRepresentationBodies<'ctx>>>, CodecError> {
    let mut pds = BTreeMap::new();
    for (id, record) in exchange.entities(ctx, "PRODUCT_DEFINITION_SHAPE")? {
        if let Some(definition) = record
            .partial(ctx, "PRODUCT_DEFINITION_SHAPE")?
            .and_then(|partial| partial.parameters.get(2))
            .and_then(ValueExt::reference)
        {
            ctx.insert_btree_map(&mut pds, id, definition, "step_shape_binding_shapes")?;
        }
    }
    let mut result = BTreeMap::new();
    let mut representation_cache = BTreeMap::new();
    for (_, record) in ctx.admit_iter(exchange.records(), "STEP shape bindings map traversal")? {
        if record
            .partial(ctx, "SHAPE_DEFINITION_REPRESENTATION")?
            .is_none()
        {
            continue;
        }
        if let Some((definition, bodies)) = shape_binding(
            record,
            exchange,
            &pds,
            definitions,
            topology,
            &mut representation_cache,
            ctx,
        )? {
            let grouped = ctx
                .entry_btree_map(&mut result, definition, "step_shape_binding_groups")?
                .or_default();
            ctx.push_vec(grouped, bodies, "step_shape_binding_bodies")?;
        }
    }
    Ok(result)
}

fn shape_binding<'a>(
    record: &RawRecord,
    exchange: &Exchange,
    pds: &BTreeMap<u64, u64>,
    definitions: &BTreeMap<u64, u64>,
    topology: &TopologyData,
    representation_cache: &mut BTreeMap<u64, super::topology::AdmittedRepresentationBodies<'a>>,
    ctx: &'a DecodeContext<'_>,
) -> Result<Option<(u64, super::topology::AdmittedRepresentationBodies<'a>)>, CodecError> {
    let Some(shape) = record
        .partial(ctx, "SHAPE_DEFINITION_REPRESENTATION")?
        .and_then(|partial| partial.parameters.first())
        .and_then(ValueExt::reference)
    else {
        return Ok(None);
    };
    let Some(&definition) = ctx.get_btree_map(pds, &shape, "STEP product pds get")? else {
        return Ok(None);
    };
    if !ctx.contains_key_btree_map(
        definitions,
        &definition,
        "STEP product definitions contains_key",
    )? {
        return Ok(None);
    }
    let Some(representation) = record
        .partial(ctx, "SHAPE_DEFINITION_REPRESENTATION")?
        .and_then(|partial| partial.parameters.get(1))
        .and_then(ValueExt::reference)
    else {
        return Ok(None);
    };
    let bodies = super::topology::representation_bodies(
        representation,
        exchange,
        topology,
        representation_cache,
        &mut BTreeSet::new(),
        ctx,
    )?;
    Ok(Some((definition, bodies)))
}

fn definition_representations(
    exchange: &Exchange,
    pds: &BTreeMap<u64, u64>,
    ctx: &DecodeContext<'_>,
) -> Result<BTreeMap<u64, BTreeSet<u64>>, CodecError> {
    let mut result = BTreeMap::<u64, BTreeSet<u64>>::new();
    for (_, record) in exchange.entities(ctx, "SHAPE_DEFINITION_REPRESENTATION")? {
        let Some(shape) = record
            .partial(ctx, "SHAPE_DEFINITION_REPRESENTATION")?
            .and_then(|partial| partial.parameters.first())
            .and_then(ValueExt::reference)
        else {
            continue;
        };
        let Some(&definition) = ctx.get_btree_map(pds, &shape, "STEP product pds get")? else {
            continue;
        };
        let Some(representation) = record
            .partial(ctx, "SHAPE_DEFINITION_REPRESENTATION")?
            .and_then(|partial| partial.parameters.get(1))
            .and_then(ValueExt::reference)
        else {
            continue;
        };

        let representations = ctx
            .entry_btree_map(
                &mut result,
                definition,
                "step_definition_representation_groups",
            )?
            .or_default();
        ctx.insert_btree_set(
            representations,
            representation,
            "step_definition_representation_members",
        )?;
    }
    Ok(result)
}

type PlacementSourceIds = BTreeMap<u64, Vec<u64>>;

fn occurrence_placements(
    exchange: &Exchange,
    geometry: &GeometryData,
    usages: &BTreeMap<u64, Usage>,
    (losses, slot_storage): (
        &mut Vec<LossNote>,
        &std::cell::RefCell<cadmpeg_core::decode::ScopedReservation<'_>>,
    ),
    (ambiguous, competing): (&mut PlacementSourceIds, &mut PlacementSourceIds),
    scratch_storage: &mut ScopedReservation<'_>,
    ctx: &DecodeContext<'_>,
) -> Result<BTreeMap<u64, Transform>, CodecError> {
    let mut pds = BTreeMap::new();
    for (&id, record) in
        ctx.admit_iter(exchange.records(), "STEP occurrence placements traversal")?
    {
        if let Some(definition) = record
            .partial(ctx, "PRODUCT_DEFINITION_SHAPE")?
            .and_then(|partial| partial.parameters.get(2))
            .and_then(ValueExt::reference)
        {
            scratch_storage.with_storage(|| {
                ctx.insert_btree_map(&mut pds, id, definition, "step_occurrence_placement_shapes")
            })?;
        }
    }
    let (definition_representations, _representation_storage) = ctx
        .with_scoped_storage("STEP definition representation scratch", || {
            definition_representations(exchange, &pds, ctx)
        })?;
    let mut definitions_by_representation = BTreeMap::<u64, BTreeSet<u64>>::new();
    for (&definition, representations) in ctx.admit_iter(
        &definition_representations,
        "STEP occurrence placements traversal",
    )? {
        for &representation in
            ctx.admit_iter(representations, "STEP product representations traversal")?
        {
            let definitions = scratch_storage
                .with_storage(|| {
                    ctx.entry_btree_map(
                        &mut definitions_by_representation,
                        representation,
                        "step_represented_definition_groups",
                    )
                })?
                .or_default();
            scratch_storage.with_storage(|| {
                ctx.insert_btree_set(
                    definitions,
                    definition,
                    "step_represented_definition_members",
                )
            })?;
        }
    }
    let mut result = BTreeMap::new();
    let mut context_candidates = BTreeMap::<u64, Vec<u64>>::new();
    for (record_id, record) in exchange.entities(ctx, "CONTEXT_DEPENDENT_SHAPE_REPRESENTATION")? {
        match occurrence_placement(
            ctx,
            record,
            exchange,
            geometry,
            &pds,
            usages,
            &definition_representations,
        )? {
            Ok(Some((usage, transform))) => {
                if ctx.contains_key_btree_map(usages, &usage, "STEP product usages contains_key")? {
                    let grouped = scratch_storage
                        .with_storage(|| {
                            ctx.entry_btree_map(
                                &mut context_candidates,
                                usage,
                                "step_context_candidate_groups",
                            )
                        })?
                        .or_default();
                    scratch_storage.with_storage(|| {
                        ctx.reserve_vec(grouped, 1, "step_context_candidate_members")
                    })?;
                    grouped.push(record_id);
                    scratch_storage.with_storage(|| {
                        ctx.insert_btree_map(
                            &mut result,
                            usage,
                            transform,
                            "step_occurrence_placement_results",
                        )
                    })?;
                }
            }
            Ok(None) => {}
            Err(TransformError::Singular) => {
                slot_storage
                    .borrow_mut()
                    .with_storage(|| ctx.reserve_vec(losses, 1, "step_product_losses"))?;
                losses.push(StepLossCode::DecodeWarning.note(format!(
                    "CONTEXT_DEPENDENT_SHAPE_REPRESENTATION #{record_id} has a singular placement"
                )));
            }
            Err(error) => return Err(placement_error(error)),
        }
    }
    for (&usage, source_ids) in
        ctx.admit_iter(&context_candidates, "STEP occurrence placements traversal")?
    {
        if source_ids.len() > 1 {
            let mut copied = Vec::new();
            scratch_storage.with_storage(|| {
                ctx.reserve_vec(
                    &mut copied,
                    source_ids.len(),
                    "step_ambiguous_context_source_copy",
                )
            })?;
            ctx.charge_work(
                u64_from_index(source_ids.len()),
                "STEP placement source copy",
            )?;
            copied.extend_from_slice(source_ids);
            let mut source_ids = copied;
            ctx.sort_unstable_by(
                &mut source_ids,
                |value| value,
                Ord::cmp,
                "step_ambiguous_context_source_sort",
            )?;
            ctx.dedup_vec(&mut source_ids, "STEP placement source deduplication")?;
            scratch_storage.with_storage(|| {
                ctx.insert_btree_map(
                    ambiguous,
                    usage,
                    source_ids,
                    "step_ambiguous_placement_groups",
                )
            })?;
        }
    }
    let mut occurrence_representations = BTreeMap::<u64, Vec<(u64, u64)>>::new();
    for (record_id, record) in exchange.entities(ctx, "SHAPE_DEFINITION_REPRESENTATION")? {
        let Some(shape) = record
            .partial(ctx, "SHAPE_DEFINITION_REPRESENTATION")?
            .and_then(|partial| partial.parameters.first())
            .and_then(ValueExt::reference)
        else {
            continue;
        };
        let Some(&usage) = ctx.get_btree_map(&pds, &shape, "STEP product pds get")? else {
            continue;
        };
        if !ctx.contains_key_btree_map(usages, &usage, "STEP product usages contains_key")? {
            continue;
        }
        let Some(representation) = record
            .partial(ctx, "SHAPE_DEFINITION_REPRESENTATION")?
            .and_then(|partial| partial.parameters.get(1))
            .and_then(ValueExt::reference)
        else {
            continue;
        };

        let grouped = scratch_storage
            .with_storage(|| {
                ctx.entry_btree_map(
                    &mut occurrence_representations,
                    usage,
                    "step_occurrence_representation_groups",
                )
            })?
            .or_default();
        scratch_storage.with_storage(|| {
            ctx.reserve_vec(grouped, 1, "step_occurrence_representation_members")
        })?;
        grouped.push((record_id, representation));
    }
    for (&usage_id, representations) in ctx.admit_iter(
        &occurrence_representations,
        "STEP occurrence placements traversal",
    )? {
        let Some(usage) = ctx.get_btree_map(usages, &usage_id, "STEP product usages get")? else {
            continue;
        };
        let Some(child_representations) = ctx.get_btree_map(
            &definition_representations,
            &usage.child_definition,
            "STEP product definition_representations get",
        )?
        else {
            continue;
        };
        let mut candidates = Vec::new();
        for &(source_id, representation) in
            ctx.admit_iter(representations, "STEP product representations traversal")?
        {
            let Some(record) = ctx.get_btree_map(
                exchange.records(),
                &representation,
                "STEP product record get",
            )?
            else {
                continue;
            };
            let Some(items) = super::representation::items(ctx, record)? else {
                continue;
            };
            for item_id in items {
                let Some(item) =
                    ctx.get_btree_map(exchange.records(), &item_id, "STEP product record get")?
                else {
                    continue;
                };
                if item.partial(ctx, "MAPPED_ITEM")?.is_none() {
                    continue;
                }
                let (mapped_representation, transform) =
                    match mapped_item_placement(ctx, item, exchange, geometry)? {
                        Ok(Some(placement)) => placement,
                        Ok(None) => continue,
                        Err(TransformError::Singular) => {
                            slot_storage.borrow_mut().with_storage(|| {
                                ctx.reserve_vec(losses, 1, "step_product_losses")
                            })?;
                            losses.push(
                                StepLossCode::DecodeWarning.note(format!(
                                    "MAPPED_ITEM #{item_id} has a singular placement"
                                )),
                            );
                            continue;
                        }
                        Err(error) => return Err(placement_error(error)),
                    };
                if ctx.contains_btree_set(
                    child_representations,
                    &mapped_representation,
                    "STEP product child_representations contains",
                )? {
                    scratch_storage.with_storage(|| {
                        ctx.reserve_vec(&mut candidates, 1, "step_occurrence_placement_candidates")
                    })?;
                    candidates.push((source_id, transform));
                }
            }
        }
        if ctx.contains_key_btree_map(&result, &usage_id, "STEP product result contains_key")?
            && ctx
                .get_btree_map(
                    &context_candidates,
                    &usage_id,
                    "STEP product context_candidates get",
                )?
                .is_some_and(|source_ids| !source_ids.is_empty())
            && !candidates.is_empty()
        {
            let original = ctx
                .get_btree_map(
                    &context_candidates,
                    &usage_id,
                    "STEP context placement source lookup",
                )?
                .ok_or_else(|| CodecError::malformed("STEP context placement was not indexed"))?;
            let mut source_ids = Vec::new();
            scratch_storage.with_storage(|| {
                ctx.reserve_vec(
                    &mut source_ids,
                    original.len(),
                    "step_competing_context_source_copy",
                )
            })?;
            ctx.charge_work(u64_from_index(original.len()), "STEP context source copy")?;
            source_ids.extend_from_slice(original);
            scratch_storage.with_storage(|| {
                ctx.reserve_vec(
                    &mut source_ids,
                    candidates.len(),
                    "step_competing_mapped_sources",
                )
            })?;
            source_ids.extend(
                ctx.admit_iter(&candidates, "STEP mapped placement source traversal")?
                    .map(|(source_id, _)| *source_id),
            );
            ctx.sort_unstable_by(
                &mut source_ids,
                |value| value,
                Ord::cmp,
                "step_competing_mapped_source_sort",
            )?;
            ctx.dedup_vec(&mut source_ids, "STEP placement source deduplication")?;
            ctx.remove_btree_map(&mut result, &usage_id, "STEP product result remove")?;
            let mut copied = Vec::new();
            scratch_storage.with_storage(|| {
                ctx.reserve_vec(&mut copied, source_ids.len(), "step_competing_source_copy")
            })?;
            ctx.charge_work(
                u64_from_index(source_ids.len()),
                "STEP competing source copy",
            )?;
            copied.extend_from_slice(&source_ids);
            scratch_storage.with_storage(|| {
                ctx.insert_btree_map(
                    ambiguous,
                    usage_id,
                    copied,
                    "step_ambiguous_placement_groups",
                )
            })?;
            scratch_storage.with_storage(|| {
                ctx.insert_btree_map(
                    competing,
                    usage_id,
                    source_ids,
                    "step_competing_placement_groups",
                )
            })?;
            continue;
        }
        match candidates.as_slice() {
            [(_, transform)] => {
                scratch_storage.with_storage(|| {
                    ctx.insert_btree_map(
                        &mut result,
                        usage_id,
                        *transform,
                        "step_occurrence_placement_results",
                    )
                })?;
            }
            [] => {}
            _ => {
                let mut source_ids = Vec::new();
                scratch_storage.with_storage(|| {
                    ctx.reserve_vec(
                        &mut source_ids,
                        candidates.len(),
                        "step_ambiguous_mapped_sources",
                    )
                })?;
                source_ids.extend(
                    ctx.admit_iter(&candidates, "STEP mapped placement source traversal")?
                        .map(|(source_id, _)| *source_id),
                );
                ctx.sort_unstable_by(
                    &mut source_ids,
                    |value| value,
                    Ord::cmp,
                    "step_ambiguous_mapped_source_sort",
                )?;
                ctx.dedup_vec(&mut source_ids, "STEP placement source deduplication")?;
                scratch_storage.with_storage(|| {
                    ctx.insert_btree_map(
                        ambiguous,
                        usage_id,
                        source_ids,
                        "step_ambiguous_placement_groups",
                    )
                })?;
            }
        }
    }
    let mut sibling_usage_counts = BTreeMap::<(u64, u64), usize>::new();
    for usage in ctx
        .admit_iter(usages, "STEP occurrence placements map traversal")?
        .map(|(_, value)| value)
    {
        let pair = (usage.parent_definition, usage.child_definition);

        *scratch_storage
            .with_storage(|| {
                ctx.entry_btree_map(&mut sibling_usage_counts, pair, "step_sibling_usage_counts")
            })?
            .or_default() += 1;
    }
    for (&usage_id, usage) in ctx.admit_iter(usages, "STEP occurrence placements traversal")? {
        if ctx.contains_key_btree_map(&result, &usage_id, "STEP product result contains_key")?
            || ctx.contains_key_btree_map(
                ambiguous,
                &usage_id,
                "STEP ambiguous placement lookup",
            )?
        {
            continue;
        }
        // A parent representation's MAPPED_ITEM identifies its child through
        // the mapping source's mapped representation and that representation's
        // SHAPE_DEFINITION_REPRESENTATION. `representation.items` is a SET,
        // so admission cannot depend on member or record order.
        let Some(parent_representations) = ctx.get_btree_map(
            &definition_representations,
            &usage.parent_definition,
            "STEP product definition_representations get",
        )?
        else {
            continue;
        };
        let mut placements = Vec::new();
        let mut transform_keys = BTreeSet::new();
        let mut transform_storage = ctx.reserve_scoped(0, "STEP occurrence transform index")?;
        for &parent_representation in ctx.admit_iter(
            parent_representations,
            "STEP product parent_representations traversal",
        )? {
            let Some(record) = ctx.get_btree_map(
                exchange.records(),
                &parent_representation,
                "STEP product record get",
            )?
            else {
                continue;
            };
            let Some(items) = super::representation::items(ctx, record)? else {
                continue;
            };
            for item_id in items {
                let Some(item) =
                    ctx.get_btree_map(exchange.records(), &item_id, "STEP product record get")?
                else {
                    continue;
                };
                if item.partial(ctx, "MAPPED_ITEM")?.is_none() {
                    continue;
                }
                let (mapped_representation, transform) =
                    match mapped_item_placement(ctx, item, exchange, geometry)? {
                        Ok(Some(placement)) => placement,
                        Ok(None) => continue,
                        Err(TransformError::Singular) => {
                            slot_storage.borrow_mut().with_storage(|| {
                                ctx.reserve_vec(losses, 1, "step_product_losses")
                            })?;
                            losses.push(
                                StepLossCode::DecodeWarning.note(format!(
                                    "MAPPED_ITEM #{item_id} has a singular placement"
                                )),
                            );
                            continue;
                        }
                        Err(error) => return Err(placement_error(error)),
                    };
                let Some(mapped_definitions) = ctx.get_btree_map(
                    &definitions_by_representation,
                    &mapped_representation,
                    "STEP product definitions_by_representation get",
                )?
                else {
                    continue;
                };
                if mapped_definitions.len() == 1
                    && ctx.contains_btree_set(
                        mapped_definitions,
                        &usage.child_definition,
                        "STEP product mapped_definitions contains",
                    )?
                    && transform_storage.with_storage(|| {
                        ctx.insert_btree_set(
                            &mut transform_keys,
                            transform_key(transform),
                            "STEP occurrence transform index",
                        )
                    })?
                {
                    scratch_storage.with_storage(|| {
                        ctx.reserve_vec(&mut placements, 1, "step_fallback_occurrence_placements")
                    })?;
                    placements.push(transform);
                }
            }
        }
        let sibling_usage_count = *ctx
            .get_btree_map(
                &sibling_usage_counts,
                &(usage.parent_definition, usage.child_definition),
                "STEP sibling usage count lookup",
            )?
            .ok_or_else(|| CodecError::malformed("STEP sibling usage count was not indexed"))?;
        if sibling_usage_count == 1 && placements.len() == 1 {
            scratch_storage.with_storage(|| {
                ctx.insert_btree_map(
                    &mut result,
                    usage_id,
                    placements[0],
                    "step_occurrence_placement_results",
                )
            })?;
        } else if !placements.is_empty() {
            slot_storage
                .borrow_mut()
                .with_storage(|| ctx.reserve_vec(losses, 1, "step_product_losses"))?;
            losses.push(StepLossCode::DecodeWarning.note(format!(
                "NAUO #{usage_id} has an ambiguous mapped-item placement"
            )));
        }
    }
    Ok(result)
}

fn placement_error(error: TransformError) -> CodecError {
    CodecError::malformed(format_args!("invalid STEP placement: {error}"))
}

fn mapped_item_placement(
    ctx: &DecodeContext<'_>,
    item: &RawRecord,
    exchange: &Exchange,
    geometry: &GeometryData,
) -> Result<Result<Option<(u64, Transform)>, TransformError>, CodecError> {
    let Some((representation, origin, target)) = mapped_item_definition(ctx, item, exchange)?
    else {
        return Ok(Ok(None));
    };
    Ok(mapped_item_transform(ctx, origin, target, geometry)?
        .map(|transform| transform.map(|transform| (representation, transform))))
}

fn mapped_item_transform(
    ctx: &DecodeContext<'_>,
    origin: u64,
    target: u64,
    geometry: &GeometryData,
) -> Result<Result<Option<Transform>, TransformError>, CodecError> {
    let Some(from) = transformation_item(ctx, origin, geometry)? else {
        return Ok(Ok(None));
    };
    let Some(to) = transformation_item(ctx, target, geometry)? else {
        return Ok(Ok(None));
    };
    Ok(from
        .try_inverse_affine()
        .and_then(|from| to.compose(from))
        .map(Some))
}

fn is_two_dimensional_mapping(
    ctx: &DecodeContext<'_>,
    origin: u64,
    target: u64,
    exchange: &Exchange,
) -> Result<bool, CodecError> {
    for id in [origin, target] {
        let Some(record) = ctx.get_btree_map(exchange.records(), &id, "STEP product record get")?
        else {
            return Ok(false);
        };
        let placement = record.partial(ctx, "AXIS2_PLACEMENT_2D")?.is_some();
        if !placement
            && record
                .partial(ctx, "CARTESIAN_TRANSFORMATION_OPERATOR_2D")?
                .is_none()
        {
            return Ok(false);
        }
    }
    Ok(true)
}

fn mapped_item_definition(
    ctx: &DecodeContext<'_>,
    item: &RawRecord,
    exchange: &Exchange,
) -> Result<Option<(u64, u64, u64)>, CodecError> {
    let Some(map) = item
        .partial(ctx, "MAPPED_ITEM")?
        .and_then(|partial| partial.parameters.get(1))
        .and_then(ValueExt::reference)
        .map(|map| ctx.get_btree_map(exchange.records(), &map, "STEP product record get"))
        .transpose()?
        .flatten()
    else {
        return Ok(None);
    };
    let Some(origin) = map
        .partial(ctx, "REPRESENTATION_MAP")?
        .and_then(|partial| partial.parameters.first())
        .and_then(ValueExt::reference)
    else {
        return Ok(None);
    };
    let Some(representation) = map
        .partial(ctx, "REPRESENTATION_MAP")?
        .and_then(|partial| partial.parameters.get(1))
        .and_then(ValueExt::reference)
    else {
        return Ok(None);
    };
    let Some(target) = item
        .partial(ctx, "MAPPED_ITEM")?
        .and_then(|partial| partial.parameters.get(2))
        .and_then(ValueExt::reference)
    else {
        return Ok(None);
    };
    Ok(Some((representation, origin, target)))
}

fn occurrence_placement(
    ctx: &DecodeContext<'_>,
    record: &RawRecord,
    exchange: &Exchange,
    geometry: &GeometryData,
    pds: &BTreeMap<u64, u64>,
    usages: &BTreeMap<u64, Usage>,
    definition_representations: &BTreeMap<u64, BTreeSet<u64>>,
) -> Result<Result<Option<(u64, Transform)>, TransformError>, CodecError> {
    let Some((usage, from_id, to_id)) = occurrence_placement_definition(
        ctx,
        record,
        exchange,
        pds,
        usages,
        definition_representations,
    )?
    else {
        return Ok(Ok(None));
    };
    Ok(mapped_item_transform(ctx, from_id, to_id, geometry)?
        .map(|transform| transform.map(|transform| (usage, transform))))
}

fn occurrence_placement_definition(
    ctx: &DecodeContext<'_>,
    record: &RawRecord,
    exchange: &Exchange,
    pds: &BTreeMap<u64, u64>,
    usages: &BTreeMap<u64, Usage>,
    definition_representations: &BTreeMap<u64, BTreeSet<u64>>,
) -> Result<Option<(u64, u64, u64)>, CodecError> {
    let Some(relation) = record
        .partial(ctx, "CONTEXT_DEPENDENT_SHAPE_REPRESENTATION")?
        .and_then(|partial| partial.parameters.first())
        .and_then(ValueExt::reference)
        .map(|id| ctx.get_btree_map(exchange.records(), &id, "STEP product record get"))
        .transpose()?
        .flatten()
    else {
        return Ok(None);
    };
    let Some(usage) = record
        .partial(ctx, "CONTEXT_DEPENDENT_SHAPE_REPRESENTATION")?
        .and_then(|partial| partial.parameters.get(1))
        .and_then(ValueExt::reference)
        .map(|id| ctx.get_btree_map(pds, &id, "STEP product pds get"))
        .transpose()?
        .flatten()
        .copied()
    else {
        return Ok(None);
    };
    let Some(usage_data) = ctx.get_btree_map(usages, &usage, "STEP product usages get")? else {
        return Ok(None);
    };
    let Some(child_representations) = ctx.get_btree_map(
        definition_representations,
        &usage_data.child_definition,
        "STEP product definition_representations get",
    )?
    else {
        return Ok(None);
    };
    let Some(parent_representations) = ctx.get_btree_map(
        definition_representations,
        &usage_data.parent_definition,
        "STEP product definition_representations get",
    )?
    else {
        return Ok(None);
    };
    let Some(relation_representations) = representation_relationship_endpoints(ctx, relation)?
    else {
        return Ok(None);
    };
    let Some(transform_id) = relation
        .partial(ctx, "REPRESENTATION_RELATIONSHIP_WITH_TRANSFORMATION")?
        .and_then(|partial| partial.parameters.first())
        .and_then(ValueExt::reference)
    else {
        return Ok(None);
    };
    let Some(transform) =
        ctx.get_btree_map(exchange.records(), &transform_id, "STEP product record get")?
    else {
        return Ok(None);
    };
    let Some(item_one) = transform
        .partial(ctx, "ITEM_DEFINED_TRANSFORMATION")?
        .and_then(|partial| partial.parameters.get(2))
        .and_then(ValueExt::reference)
    else {
        return Ok(None);
    };
    let Some(item_two) = transform
        .partial(ctx, "ITEM_DEFINED_TRANSFORMATION")?
        .and_then(|partial| partial.parameters.get(3))
        .and_then(ValueExt::reference)
    else {
        return Ok(None);
    };
    let child_to_parent = ctx.contains_btree_set(
        child_representations,
        &relation_representations.0,
        "STEP product child_representations contains",
    )? && ctx.contains_btree_set(
        parent_representations,
        &relation_representations.1,
        "STEP product parent_representations contains",
    )?;
    let parent_to_child = ctx.contains_btree_set(
        parent_representations,
        &relation_representations.0,
        "STEP product parent_representations contains",
    )? && ctx.contains_btree_set(
        child_representations,
        &relation_representations.1,
        "STEP product child_representations contains",
    )?;
    let (from_id, to_id) = match (child_to_parent, parent_to_child) {
        (true, false) => (item_one, item_two),
        (false, true) => (item_two, item_one),
        _ => return Ok(None),
    };
    Ok(Some((usage, from_id, to_id)))
}

fn transform_key(transform: Transform) -> [[u64; 4]; 4] {
    transform
        .rows()
        .map(|row| row.map(|value| if value == 0.0 { 0 } else { value.to_bits() }))
}

fn transformation_item(
    ctx: &DecodeContext<'_>,
    id: u64,
    geometry: &GeometryData,
) -> Result<Option<Transform>, CodecError> {
    if let Some(transform) = ctx
        .get_btree_map(&geometry.placements, &id, "STEP placement transform lookup")?
        .copied()
        .and_then(super::geometry::placement_transform)
    {
        return Ok(Some(transform));
    }
    Ok(ctx
        .get_btree_map(
            &geometry.transformation_operators,
            &id,
            "STEP transformation operator lookup",
        )?
        .copied())
}

fn representation_relationship_endpoints(
    ctx: &DecodeContext<'_>,
    record: &RawRecord,
) -> Result<Option<(u64, u64)>, CodecError> {
    let mut relationship = record.partial(ctx, "REPRESENTATION_RELATIONSHIP")?;
    if relationship.is_none() {
        relationship = record.partial(ctx, "SHAPE_REPRESENTATION_RELATIONSHIP")?;
    }
    let Some(relationship) = relationship else {
        return Ok(None);
    };
    let values = relationship.parameters.as_slice();
    let Some(first) = ctx.position_by(
        values,
        |value| Ok(value.reference().is_some()),
        "STEP representation relationship first endpoint",
    )?
    else {
        return Ok(None);
    };
    let first_id = values[first].reference();
    let second = ctx.find_map(
        &values[first + 1..],
        |value| Ok(value.reference()),
        "STEP representation relationship second endpoint",
    )?;
    Ok(first_id.zip(second))
}

fn product_ir_id(id: u64) -> ProductDefinitionId {
    ProductDefinitionId::from(ids::product(kind!("product"), id))
}

fn product_definition_ir_id(
    product: u64,
    definition: u64,
    definition_count: usize,
) -> ProductDefinitionId {
    if definition_count == 1 {
        product_ir_id(product)
    } else {
        ProductDefinitionId::from(ids::product(
            kind!("product"),
            IdentityKey::from(product)
                .dash(key_word!("definition"))
                .dash(definition),
        ))
    }
}

fn product_definition_formation_parameters<'a>(
    ctx: &DecodeContext<'_>,
    record: &'a RawRecord,
) -> Result<Option<&'a [Value]>, CodecError> {
    if let Some(partial) = record.partial(ctx, "PRODUCT_DEFINITION_FORMATION")? {
        return Ok(Some(partial.parameters.as_slice()));
    }
    Ok(match record.simple_name() {
        Some("PRODUCT_DEFINITION_FORMATION_WITH_SPECIFIED_SOURCE" | "FINAL_SOLUTION") => {
            Some(record.partials.first().parameters.as_slice())
        }
        _ => None,
    })
}

fn product_definition_parameters<'a>(
    ctx: &DecodeContext<'_>,
    record: &'a RawRecord,
) -> Result<Option<&'a [Value]>, CodecError> {
    if let Some(partial) = record.partial(ctx, "PRODUCT_DEFINITION")? {
        return Ok(Some(partial.parameters.as_slice()));
    }
    Ok(match record.simple_name() {
        Some("PRODUCT_DEFINITION_WITH_ASSOCIATED_DOCUMENTS") => {
            Some(record.partials.first().parameters.as_slice())
        }
        _ => None,
    })
}

#[cfg(test)]
pub(crate) mod tests;
