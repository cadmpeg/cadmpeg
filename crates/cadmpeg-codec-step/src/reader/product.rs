// SPDX-License-Identifier: Apache-2.0
//! STEP product prototypes, occurrence identity, and relative placement.

use crate::ids::{key_word, kind};
use std::collections::{BTreeMap, BTreeSet, HashSet, VecDeque};

use super::{named_parameter, RecordExt, ValueExt};
use cadmpeg_core::decode::{u64_from_index, DecodeContext};
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

use super::decode_text_charged;
use super::geometry::GeometryData;
use super::topology::TopologyData;
use super::StageOutcome;

mod bodies;

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

pub(super) struct ProductData {
    pub(super) product_definition_ids_by_source: BTreeMap<u64, Vec<ProductDefinitionId>>,
    pub(super) product_definition_ids_by_shape: BTreeMap<u64, ProductDefinitionId>,
}

fn join_product_references(ids: impl IntoIterator<Item = u64>) -> String {
    ids.into_iter()
        .map(|id| format!("#{id}"))
        .collect::<Vec<_>>()
        .join(", ")
}

pub(super) fn decode(
    exchange: &Exchange,
    geometry: &GeometryData,
    topology: &TopologyData,
    ir: &mut CadIr,
    ctx: &DecodeContext<'_>,
    admitted_ir_entities: &mut u64,
) -> Result<StageOutcome<ProductData>, CodecError> {
    let mut typed = HashSet::new();
    let mut losses = Vec::new();
    let mut formations = BTreeMap::new();
    for (id, record) in exchange.entities_any(PRODUCT_DEFINITION_FORMATION_TYPES) {
        let Some(product) = product_definition_formation_parameters(record)
            .and_then(|parameters| parameters.get(2))
            .and_then(ValueExt::reference)
        else {
            continue;
        };
        formations.insert(id, product);
    }
    let mut definitions = BTreeMap::new();
    for (id, record) in exchange.entities_any(PRODUCT_DEFINITION_TYPES) {
        let Some(product) = product_definition_parameters(record)
            .and_then(|parameters| parameters.get(2))
            .and_then(ValueExt::reference)
            .and_then(|formation| formations.get(&formation).copied())
        else {
            continue;
        };
        definitions.insert(id, product);
    }
    let mut definitions_by_product_in_source_order = BTreeMap::<u64, Vec<u64>>::new();
    for (&definition, &product) in &definitions {
        let grouped = definitions_by_product_in_source_order
            .entry(product)
            .or_default();

        grouped.push(definition);
    }
    for definitions in definitions_by_product_in_source_order.values_mut() {
        definitions.sort_by(|left, right| {
            let start = |definition: &u64| {
                exchange
                    .records()
                    .get(definition)
                    .map(|record| record.span.start)
            };
            let left = start(left);
            let right = start(right);
            (left.is_none(), left).cmp(&(right.is_none(), right))
        });
    }
    let mut definition_descriptions = BTreeMap::<u64, String>::new();
    for (id, record) in exchange.entities_any(PRODUCT_DEFINITION_TYPES) {
        let Some(parameters) = product_definition_parameters(record) else {
            continue;
        };
        let Some(_) = parameters
            .get(2)
            .and_then(ValueExt::reference)
            .and_then(|formation| formations.get(&formation).copied())
        else {
            continue;
        };
        let Some(description) = parameters.get(1).and_then(|value| {
            decode_text_charged(
                exchange,
                value,
                &mut losses,
                id,
                "product definition description",
                StepLossCode::MetadataStringInvalid,
            )
        }) else {
            continue;
        };
        if !description.is_empty() {
            definition_descriptions.entry(id).or_insert(description);
        }
    }
    let mut shape_bindings = shape_bindings(exchange, &definitions, topology, ctx)?;
    let mut definition_counts = BTreeMap::<u64, usize>::new();
    for product in definitions.values() {
        *definition_counts.entry(*product).or_default() += 1;
    }

    let mut definition_prototypes = BTreeMap::<u64, ProductDefinitionId>::new();
    let mut product_definition_ids_by_source = BTreeMap::<u64, Vec<ProductDefinitionId>>::new();

    for (step_id, record) in exchange.entities("PRODUCT") {
        let Some(parameters) = record
            .partial("PRODUCT")
            .map(|partial| partial.parameters.as_slice())
        else {
            continue;
        };
        let product_id = parameters
            .first()
            .and_then(|value| {
                decode_text_charged(
                    exchange,
                    value,
                    &mut losses,
                    step_id,
                    "product identifier",
                    StepLossCode::MetadataStringInvalid,
                )
            })
            .unwrap_or_else(|| format!("#{step_id}"));
        let name = parameters
            .get(1)
            .and_then(|value| {
                decode_text_charged(
                    exchange,
                    value,
                    &mut losses,
                    step_id,
                    "product name",
                    StepLossCode::MetadataStringInvalid,
                )
            })
            .filter(|name| !name.is_empty());
        let product_description = parameters
            .get(2)
            .and_then(|value| {
                decode_text_charged(
                    exchange,
                    value,
                    &mut losses,
                    step_id,
                    "product description",
                    StepLossCode::MetadataStringInvalid,
                )
            })
            .filter(|description| !description.is_empty());
        let product_definitions = definitions_by_product_in_source_order
            .get(&step_id)
            .map(Vec::as_slice)
            .unwrap_or_default();
        let definition_count = definition_counts.get(&step_id).copied().unwrap_or(0);
        let definition_iter = std::iter::once(None)
            .filter(|_| product_definitions.is_empty())
            .chain(product_definitions.iter().copied().map(Some));
        for definition in definition_iter {
            let product_definition_id = definition.map_or_else(
                || Ok::<ProductDefinitionId, CodecError>(product_ir_id(step_id)),
                |definition| {
                    let id = product_definition_ir_id(step_id, definition, definition_count);

                    definition_prototypes.insert(definition, id.clone());
                    Ok(id)
                },
            )?;
            let definition_description = definition
                .and_then(|definition| definition_descriptions.get(&definition))
                .map(std::borrow::ToOwned::to_owned);
            let description = if definition_count <= 1 {
                product_description
                    .as_deref()
                    .map(std::borrow::ToOwned::to_owned)
                    .or(definition_description)
            } else {
                match definition_description {
                    Some(description) => Some(description),
                    None => product_description
                        .as_deref()
                        .map(std::borrow::ToOwned::to_owned),
                }
            };
            let has_shape_binding =
                definition.is_some_and(|definition| shape_bindings.contains_key(&definition));
            let mut bodies = definition
                .and_then(|definition| shape_bindings.remove(&definition))
                .unwrap_or_default();
            let missing = bodies
                .iter()
                .filter(|body| {
                    !ir.model
                        .bodies
                        .iter()
                        .any(|candidate| candidate.id == **body)
                })
                .map(cadmpeg_ir::ids::BodyId::as_str)
                .collect::<Vec<_>>()
                .join(", ");
            bodies.retain(|body| {
                ir.model
                    .bodies
                    .iter()
                    .any(|candidate| candidate.id == *body)
            });
            bodies.sort_by(Ord::cmp);
            bodies.dedup();
            let owner = definition.map_or_else(
                || format!("PRODUCT #{step_id}"),
                |definition| format!("PRODUCT_DEFINITION #{definition}"),
            );
            if !missing.is_empty() {
                losses.push(StepLossCode::DecodeWarning.note(format!(
                    "{owner} omitted uncommitted shape body reference(s): {missing}"
                )));
            }
            if has_shape_binding && bodies.is_empty() {
                losses.push(StepLossCode::DecodeWarning.note(format!(
                    "{owner} has a shape representation with no committed topology body"
                )));
            }

            ir.model.product_definitions.push(ProductDefinition {
                id: product_definition_id.clone(),
                kind: ProductDefinitionKind::Part,
                source_name: name.as_deref().map(std::borrow::ToOwned::to_owned),
                label: name.as_deref().map(std::borrow::ToOwned::to_owned),
                description,
                part_number: Some(product_id.clone()),
                bom_properties: BTreeMap::new(),
                bodies,
                native_ref: Some(
                    definition.map_or_else(|| format!("#{step_id}"), |id| format!("#{id}")),
                ),
            });

            let grouped = product_definition_ids_by_source.entry(step_id).or_default();

            grouped.push(product_definition_id);
        }
        typed.insert(step_id);
    }
    let mut product_definition_ids_by_shape = BTreeMap::new();
    for (shape_id, record) in exchange.entities("PRODUCT_DEFINITION_SHAPE") {
        let Some(prototype) = named_parameter(record, "PRODUCT_DEFINITION_SHAPE", 2)
            .and_then(ValueExt::reference)
            .and_then(|definition| definition_prototypes.get(&definition))
        else {
            continue;
        };

        product_definition_ids_by_shape.insert(shape_id, prototype.clone());
    }
    for id in formations.keys().chain(definitions.keys()) {
        typed.insert(*id);
    }

    let mut usages = BTreeMap::new();
    for (id, record) in exchange.entities("NEXT_ASSEMBLY_USAGE_OCCURRENCE") {
        let name = named_parameter(record, "NEXT_ASSEMBLY_USAGE_OCCURRENCE", 1).and_then(|value| {
            decode_text_charged(
                exchange,
                value,
                &mut losses,
                id,
                "assembly occurrence name",
                StepLossCode::MetadataStringInvalid,
            )
        });
        let Some(parent_definition) = named_parameter(record, "NEXT_ASSEMBLY_USAGE_OCCURRENCE", 3)
            .and_then(ValueExt::reference)
        else {
            continue;
        };
        let Some(child_definition) = named_parameter(record, "NEXT_ASSEMBLY_USAGE_OCCURRENCE", 4)
            .and_then(ValueExt::reference)
        else {
            continue;
        };
        usages.insert(
            id,
            Usage {
                parent_definition,
                child_definition,
                name: name.filter(|name| !name.is_empty()),
            },
        );
    }
    let mut child_definitions = BTreeSet::new();
    for usage in usages.values() {
        child_definitions.insert(usage.child_definition);
    }

    let mut occurrence_paths = BTreeMap::<OccurrenceId, BTreeSet<u64>>::new();
    let mut pending_occurrences = VecDeque::new();
    let mut root_ordinal = 0_u32;
    for &definition in definitions.keys() {
        if child_definitions.contains(&definition) {
            continue;
        }
        let Some(prototype) = definition_prototypes.get(&definition).cloned() else {
            losses.push(StepLossCode::DecodeWarning.note(format!(
                "PRODUCT_DEFINITION #{definition} has no local product prototype"
            )));
            continue;
        };
        let id = OccurrenceId::from(ids::product(
            kind!("occurrence"),
            key_word!("definition").dash(definition),
        ));
        if ir.model.occurrences.len() >= MAX_OCCURRENCES {
            return Err(occurrence_limit_error());
        }

        ir.model.occurrences.push(Occurrence {
            id: id.clone(),
            prototype: PrototypeReference::Local {
                definition: prototype,
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

        occurrence_paths.insert(id.clone(), BTreeSet::from([definition]));
        pending_occurrences.push_back((definition, id));
    }
    let mut ambiguous_placements = BTreeMap::new();
    let mut competing_placements = BTreeMap::new();
    let placements = occurrence_placements(
        exchange,
        geometry,
        &usages,
        &mut losses,
        &mut ambiguous_placements,
        &mut competing_placements,
    )?;
    for (&usage_id, source_ids) in &ambiguous_placements {
        if competing_placements.contains_key(&usage_id) {
            continue;
        }
        let records = join_product_references(source_ids.iter().copied());
        let context_dependent = source_ids.iter().all(|id| {
            exchange.records().get(id).is_some_and(|record| {
                record
                    .partial("CONTEXT_DEPENDENT_SHAPE_REPRESENTATION")
                    .is_some()
            })
        });
        let placement_kind = if context_dependent {
            "CONTEXT_DEPENDENT_SHAPE_REPRESENTATION"
        } else {
            "occurrence-owned mapped"
        };

        losses.push(StepLossCode::NauoPlacementAmbiguous.note(format!("NAUO #{usage_id} has multiple resolved {placement_kind} placements ({records}); no neutral occurrence was admitted and the source placement relations remain opaque")));
    }
    for (&usage_id, source_ids) in &competing_placements {
        let records = join_product_references(source_ids.iter().copied());

        losses.push(StepLossCode::NauoPlacementAmbiguous.note(format!("NAUO #{usage_id} has resolved context-dependent and occurrence-owned mapped placements ({records}); no neutral occurrence was admitted and the source placement relations remain opaque")));
    }
    let mut usage_instances = BTreeMap::<u64, usize>::new();
    let mut missing_placement_reports = BTreeSet::new();
    let mut child_ordinals = BTreeMap::<OccurrenceId, u32>::new();
    let mut usages_by_parent = BTreeMap::<u64, Vec<u64>>::new();
    for (&usage_id, usage) in &usages {
        let grouped = usages_by_parent.entry(usage.parent_definition).or_default();

        grouped.push(usage_id);
    }
    let had_roots = !pending_occurrences.is_empty();
    while let Some((parent_definition, parent)) = pending_occurrences.pop_front() {
        for &usage_id in usages_by_parent
            .get(&parent_definition)
            .into_iter()
            .flatten()
        {
            if ambiguous_placements.contains_key(&usage_id) {
                continue;
            }
            let usage = &usages[&usage_id];
            let Some(prototype) = definition_prototypes.get(&usage.child_definition).cloned()
            else {
                losses.push(StepLossCode::DecodeWarning.note(format!(
                    "NAUO #{usage_id} references an unresolved child definition"
                )));
                continue;
            };
            let parent_path = occurrence_paths.get(&parent);
            if parent_path.is_some_and(|path| path.len() >= MAX_ASSEMBLY_DEPTH) {
                return Err(CodecError::NotImplemented(format!(
                    "STEP assembly nesting exceeds {MAX_ASSEMBLY_DEPTH} levels"
                )));
            }
            if parent_path.is_some_and(|path| path.contains(&usage.child_definition)) {
                losses.push(StepLossCode::DecodeWarning.note(format!(
                    "NAUO #{usage_id} closes an assembly definition cycle"
                )));
                continue;
            }

            let instance = usage_instances.entry(usage_id).or_default();
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
            if ir.model.occurrences.len() >= MAX_OCCURRENCES {
                return Err(occurrence_limit_error());
            }

            let ordinal = child_ordinals.entry(parent.clone()).or_default();
            let transform = if let Some(transform) = placements.get(&usage_id).copied() {
                transform
            } else {
                if missing_placement_reports.insert(usage_id) {
                    losses.push(StepLossCode::NauoPlacementUnresolved.note(format!(
                        "NAUO #{usage_id} has no resolved occurrence transform; \
                             identity placement was used"
                    )));
                }
                Transform::identity()
            };

            ir.model.occurrences.push(Occurrence {
                id: id.clone(),
                prototype: PrototypeReference::Local {
                    definition: prototype,
                },
                parent: OccurrenceParent::Occurrence {
                    occurrence: parent.clone(),
                },
                ordinal: *ordinal,
                transform,
                linked_prototype: None,
                scale: [cadmpeg_ir::scalar::FiniteReal::ONE; 3],
                name: usage.name.as_deref().map(std::borrow::ToOwned::to_owned),
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
                for &definition in parent_path {
                    path.insert(definition);
                }
            }
            path.insert(usage.child_definition);

            occurrence_paths.insert(id.clone(), path);
            pending_occurrences.push_back((usage.child_definition, id));
            typed.insert(usage_id);
        }
    }
    if !had_roots && !usages.is_empty() {
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
        &mut losses,
        ctx,
    )?;
    for (id, record) in exchange.entities_any(&[
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
    ]) {
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
        .any(|name| record.partial(name).is_some())
            || record
                .partial("REPRESENTATION_RELATIONSHIP_WITH_TRANSFORMATION")
                .is_some()
        {
            typed.insert(id);
        }
    }
    for (&usage_id, source_ids) in &ambiguous_placements {
        typed.remove(&usage_id);
        for &source_id in source_ids {
            typed.remove(&source_id);
        }
    }
    Ok(StageOutcome {
        value: ProductData {
            product_definition_ids_by_source,
            product_definition_ids_by_shape,
        },
        claims: typed,
        losses,
        notes: Vec::new(),
    })
}

fn occurrence_limit_error() -> CodecError {
    CodecError::NotImplemented(format!(
        "STEP assembly expands to more than {MAX_OCCURRENCES} occurrences"
    ))
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
    losses: &mut Vec<LossNote>,
    ctx: &DecodeContext<'_>,
) -> Result<(), CodecError> {
    let BodyPlacementSources {
        geometry,
        topology,
        usages,
    } = sources;
    let mut pds = BTreeMap::new();
    for (id, record) in exchange.entities("PRODUCT_DEFINITION_SHAPE") {
        if let Some(definition) =
            named_parameter(record, "PRODUCT_DEFINITION_SHAPE", 2).and_then(ValueExt::reference)
        {
            pds.insert(id, definition);
        }
    }
    let definition_representations = definition_representations(exchange, &pds);
    let mut assembly_representations = BTreeSet::new();
    for representation in usages.values().flat_map(|usage| {
        definition_representations
            .get(&usage.child_definition)
            .into_iter()
            .flatten()
    }) {
        assembly_representations.insert(*representation);
    }

    let mut body_indices = BTreeMap::new();
    for (index, body) in ir.model.bodies.iter().enumerate() {
        body_indices.insert(body.id.clone(), index);
    }
    let mut representation_cache = BTreeMap::new();
    let mut placements_by_body = BTreeMap::<BodyId, Vec<(u64, Transform)>>::new();
    let drawing_owned_items = drawing_owned_items(exchange, ctx)?;
    for (id, item) in exchange.entities("MAPPED_ITEM") {
        if item.partial("MAPPED_ITEM").is_none() {
            continue;
        }
        if drawing_owned_items.contains(&id) {
            continue;
        }
        let Some((representation, origin, target)) = mapped_item_definition(item, exchange) else {
            continue;
        };
        if assembly_representations.contains(&representation) {
            continue;
        }
        if is_two_dimensional_mapping(origin, target, exchange) {
            continue;
        }
        let body_ids = super::topology::representation_bodies(
            representation,
            exchange,
            topology,
            &mut representation_cache,
            &mut BTreeSet::new(),
            ctx,
        )?;
        if body_ids.is_empty() {
            continue;
        }
        let transform = match mapped_item_transform(origin, target, geometry) {
            Ok(Some(transform)) => transform,
            Ok(None) | Err(TransformError::Singular) => {
                losses.push(
                    StepLossCode::DecodeWarning
                        .note(format!("MAPPED_ITEM #{id} has no resolved body placement")),
                );
                continue;
            }
            Err(error) => return Err(placement_error(error)),
        };
        for body in body_ids {
            let grouped = placements_by_body.entry(body).or_default();

            grouped.push((id, transform));
        }
    }
    for (body, placements) in placements_by_body {
        let mut unique = Vec::<(u64, Transform)>::new();
        for placement in placements {
            if unique.iter().all(|(_, existing)| *existing != placement.1) {
                unique.push(placement);
            }
        }
        match unique.as_slice() {
            [(_, transform)] => {
                if let Some(index) = body_indices.get(&body) {
                    ir.model.bodies[*index].transform = Some(*transform);
                }
            }
            [] => {}
            _ => {
                let mapped_items = join_product_references(unique.iter().map(|(id, _)| *id));

                losses.push(StepLossCode::BodyConflictingMappedPlacements.note(format!("body {body} has conflicting standalone MAPPED_ITEM placements ({mapped_items}); no body placement was selected")));
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
    for record in exchange.records().values() {
        let drawing_owner = record
            .partials
            .iter()
            .any(|partial| DRAWING_ITEM_OWNER_TYPES.contains(&partial.name.as_str()));
        if drawing_owner {
            for value in record
                .partials
                .iter()
                .flat_map(|partial| partial.parameters.iter())
            {
                collect_references(value, &mut pending, ctx)?;
            }
        }
    }
    let mut items = BTreeSet::new();
    let mut visited = BTreeSet::new();
    while let Some(id) = pending.pop() {
        if visited.contains(&id) {
            continue;
        }
        visited.insert(id);
        let Some(record) = exchange.records().get(&id) else {
            continue;
        };
        if record.partial("MAPPED_ITEM").is_some() {
            items.insert(id);
            continue;
        }
        if let Some(representation_items) = super::representation::items(record) {
            for item in representation_items {
                pending.push(item);
            }
        }
        for partial in record.partials.iter().filter(|partial| {
            matches!(
                partial.name.as_str(),
                "GEOMETRIC_SET" | "GEOMETRIC_CURVE_SET" | "TESSELLATED_GEOMETRIC_SET"
            )
        }) {
            let Some(values) = partial.parameters.iter().find_map(|value| match value {
                Value::List(values) => Some(values.as_slice()),
                _ => None,
            }) else {
                continue;
            };
            for value in values {
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
            references.push(*id);
        }
        Value::List(values) => {
            for value in values {
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

fn shape_bindings(
    exchange: &Exchange,
    definitions: &BTreeMap<u64, u64>,
    topology: &TopologyData,
    ctx: &DecodeContext<'_>,
) -> Result<BTreeMap<u64, Vec<BodyId>>, CodecError> {
    let mut pds = BTreeMap::new();
    for (id, record) in exchange.entities("PRODUCT_DEFINITION_SHAPE") {
        if let Some(definition) =
            named_parameter(record, "PRODUCT_DEFINITION_SHAPE", 2).and_then(ValueExt::reference)
        {
            pds.insert(id, definition);
        }
    }
    let mut result = BTreeMap::<u64, Vec<BodyId>>::new();
    let ownership =
        bodies::RepresentationOwnership::build(exchange, topology, &pds, definitions, ctx);
    for record in exchange
        .records()
        .values()
        .filter(|record| record.partial("SHAPE_DEFINITION_REPRESENTATION").is_some())
    {
        if let Some((definition, body_ids)) = shape_binding(record, &pds, definitions, &ownership)?
        {
            let grouped = result.entry(definition).or_default();

            grouped.extend(body_ids);
        }
    }
    Ok(result)
}

fn shape_binding(
    record: &RawRecord,
    pds: &BTreeMap<u64, u64>,
    definitions: &BTreeMap<u64, u64>,
    ownership: &bodies::RepresentationOwnership<'_, '_>,
) -> Result<Option<(u64, Vec<BodyId>)>, CodecError> {
    let Some(shape) =
        named_parameter(record, "SHAPE_DEFINITION_REPRESENTATION", 0).and_then(ValueExt::reference)
    else {
        return Ok(None);
    };
    let Some(&definition) = pds.get(&shape) else {
        return Ok(None);
    };
    if !definitions.contains_key(&definition) {
        return Ok(None);
    }
    let Some(representation) =
        named_parameter(record, "SHAPE_DEFINITION_REPRESENTATION", 1).and_then(ValueExt::reference)
    else {
        return Ok(None);
    };
    let bodies = ownership.bodies(definition, representation)?;
    Ok(Some((definition, bodies)))
}

fn definition_representations(
    exchange: &Exchange,
    pds: &BTreeMap<u64, u64>,
) -> std::collections::BTreeMap<u64, std::collections::BTreeSet<u64>> {
    let mut result = BTreeMap::<u64, BTreeSet<u64>>::new();
    for (_, record) in exchange.entities("SHAPE_DEFINITION_REPRESENTATION") {
        let Some(shape) = named_parameter(record, "SHAPE_DEFINITION_REPRESENTATION", 0)
            .and_then(ValueExt::reference)
        else {
            continue;
        };
        let Some(&definition) = pds.get(&shape) else {
            continue;
        };
        let Some(representation) = named_parameter(record, "SHAPE_DEFINITION_REPRESENTATION", 1)
            .and_then(ValueExt::reference)
        else {
            continue;
        };

        let representations = result.entry(definition).or_default();
        representations.insert(representation);
    }
    result
}

fn occurrence_placements(
    exchange: &Exchange,
    geometry: &GeometryData,
    usages: &BTreeMap<u64, Usage>,
    losses: &mut Vec<LossNote>,
    ambiguous: &mut BTreeMap<u64, Vec<u64>>,
    competing: &mut BTreeMap<u64, Vec<u64>>,
) -> Result<BTreeMap<u64, Transform>, CodecError> {
    let mut pds = BTreeMap::new();
    for (&id, record) in exchange.records() {
        if let Some(definition) =
            named_parameter(record, "PRODUCT_DEFINITION_SHAPE", 2).and_then(ValueExt::reference)
        {
            pds.insert(id, definition);
        }
    }
    let definition_representations = definition_representations(exchange, &pds);
    let mut definitions_by_representation = BTreeMap::<u64, BTreeSet<u64>>::new();
    for (&definition, representations) in &definition_representations {
        for &representation in representations {
            let definitions = definitions_by_representation
                .entry(representation)
                .or_default();
            definitions.insert(definition);
        }
    }
    let mut result = BTreeMap::new();
    let mut context_candidates = BTreeMap::<u64, Vec<u64>>::new();
    for (record_id, record) in exchange.entities("CONTEXT_DEPENDENT_SHAPE_REPRESENTATION") {
        match occurrence_placement(
            record,
            exchange,
            geometry,
            &pds,
            usages,
            &definition_representations,
        ) {
            Ok(Some((usage, transform))) => {
                if usages.contains_key(&usage) {
                    let grouped = context_candidates.entry(usage).or_default();

                    grouped.push(record_id);
                    result.insert(usage, transform);
                }
            }
            Ok(None) => {}
            Err(TransformError::Singular) => {
                losses.push(StepLossCode::DecodeWarning.note(format!(
                    "CONTEXT_DEPENDENT_SHAPE_REPRESENTATION #{record_id} has a singular placement"
                )));
            }
            Err(error) => return Err(placement_error(error)),
        }
    }
    for (&usage, source_ids) in &context_candidates {
        if source_ids.len() > 1 {
            let mut copied = Vec::new();

            copied.extend_from_slice(source_ids);
            let mut source_ids = copied;
            source_ids.sort_unstable_by(Ord::cmp);
            source_ids.dedup();
            ambiguous.insert(usage, source_ids);
        }
    }
    let mut occurrence_representations = BTreeMap::<u64, Vec<(u64, u64)>>::new();
    for (record_id, record) in exchange.entities("SHAPE_DEFINITION_REPRESENTATION") {
        let Some(shape) = named_parameter(record, "SHAPE_DEFINITION_REPRESENTATION", 0)
            .and_then(ValueExt::reference)
        else {
            continue;
        };
        let Some(&usage) = pds.get(&shape) else {
            continue;
        };
        if !usages.contains_key(&usage) {
            continue;
        }
        let Some(representation) = named_parameter(record, "SHAPE_DEFINITION_REPRESENTATION", 1)
            .and_then(ValueExt::reference)
        else {
            continue;
        };

        let grouped = occurrence_representations.entry(usage).or_default();

        grouped.push((record_id, representation));
    }
    for (&usage_id, representations) in &occurrence_representations {
        let Some(usage) = usages.get(&usage_id) else {
            continue;
        };
        let Some(child_representations) = definition_representations.get(&usage.child_definition)
        else {
            continue;
        };
        let mut candidates = Vec::new();
        for &(source_id, representation) in representations {
            let Some(record) = exchange.records().get(&representation) else {
                continue;
            };
            let Some(items) = super::representation::items(record) else {
                continue;
            };
            for item_id in items {
                let Some(item) = exchange.records().get(&item_id) else {
                    continue;
                };
                if item.partial("MAPPED_ITEM").is_none() {
                    continue;
                }
                let (mapped_representation, transform) =
                    match mapped_item_placement(item, exchange, geometry) {
                        Ok(Some(placement)) => placement,
                        Ok(None) => continue,
                        Err(TransformError::Singular) => {
                            losses.push(
                                StepLossCode::DecodeWarning.note(format!(
                                    "MAPPED_ITEM #{item_id} has a singular placement"
                                )),
                            );
                            continue;
                        }
                        Err(error) => return Err(placement_error(error)),
                    };
                if child_representations.contains(&mapped_representation) {
                    candidates.push((source_id, transform));
                }
            }
        }
        if result.contains_key(&usage_id)
            && context_candidates
                .get(&usage_id)
                .is_some_and(|source_ids| !source_ids.is_empty())
            && !candidates.is_empty()
        {
            let original = &context_candidates[&usage_id];
            let mut source_ids = Vec::new();

            source_ids.extend_from_slice(original);

            source_ids.extend(candidates.iter().map(|(source_id, _)| *source_id));
            source_ids.sort_unstable_by(Ord::cmp);
            source_ids.dedup();
            result.remove(&usage_id);
            let mut copied = Vec::new();

            copied.extend_from_slice(&source_ids);
            ambiguous.insert(usage_id, copied);
            competing.insert(usage_id, source_ids);
            continue;
        }
        match candidates.as_slice() {
            [(_, transform)] => {
                result.insert(usage_id, *transform);
            }
            [] => {}
            _ => {
                let mut source_ids = Vec::new();

                source_ids.extend(candidates.iter().map(|(source_id, _)| *source_id));
                source_ids.sort_unstable_by(Ord::cmp);
                source_ids.dedup();
                ambiguous.insert(usage_id, source_ids);
            }
        }
    }
    let mut sibling_usage_counts = BTreeMap::<(u64, u64), usize>::new();
    for usage in usages.values() {
        let pair = (usage.parent_definition, usage.child_definition);

        *sibling_usage_counts.entry(pair).or_default() += 1;
    }
    for (&usage_id, usage) in usages {
        if result.contains_key(&usage_id) || ambiguous.contains_key(&usage_id) {
            continue;
        }
        // A parent representation's MAPPED_ITEM identifies its child through
        // the mapping source's mapped representation and that representation's
        // SHAPE_DEFINITION_REPRESENTATION. `representation.items` is a SET,
        // so admission cannot depend on member or record order.
        let Some(parent_representations) = definition_representations.get(&usage.parent_definition)
        else {
            continue;
        };
        let mut placements = Vec::new();
        for &parent_representation in parent_representations {
            let Some(record) = exchange.records().get(&parent_representation) else {
                continue;
            };
            let Some(items) = super::representation::items(record) else {
                continue;
            };
            for item_id in items {
                let Some(item) = exchange.records().get(&item_id) else {
                    continue;
                };
                if item.partial("MAPPED_ITEM").is_none() {
                    continue;
                }
                let (mapped_representation, transform) =
                    match mapped_item_placement(item, exchange, geometry) {
                        Ok(Some(placement)) => placement,
                        Ok(None) => continue,
                        Err(TransformError::Singular) => {
                            losses.push(
                                StepLossCode::DecodeWarning.note(format!(
                                    "MAPPED_ITEM #{item_id} has a singular placement"
                                )),
                            );
                            continue;
                        }
                        Err(error) => return Err(placement_error(error)),
                    };
                let Some(mapped_definitions) =
                    definitions_by_representation.get(&mapped_representation)
                else {
                    continue;
                };
                if mapped_definitions.len() == 1
                    && mapped_definitions.contains(&usage.child_definition)
                    && !placements.contains(&transform)
                {
                    placements.push(transform);
                }
            }
        }
        let sibling_usage_count =
            sibling_usage_counts[&(usage.parent_definition, usage.child_definition)];
        if sibling_usage_count == 1 && placements.len() == 1 {
            result.insert(usage_id, placements[0]);
        } else if !placements.is_empty() {
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
    item: &RawRecord,
    exchange: &Exchange,
    geometry: &GeometryData,
) -> Result<Option<(u64, Transform)>, TransformError> {
    let Some((representation, origin, target)) = mapped_item_definition(item, exchange) else {
        return Ok(None);
    };
    Ok(mapped_item_transform(origin, target, geometry)?
        .map(|transform| (representation, transform)))
}

fn mapped_item_transform(
    origin: u64,
    target: u64,
    geometry: &GeometryData,
) -> Result<Option<Transform>, TransformError> {
    let Some(from) = transformation_item(origin, geometry) else {
        return Ok(None);
    };
    let Some(to) = transformation_item(target, geometry) else {
        return Ok(None);
    };
    to.compose(from.try_inverse_affine()?).map(Some)
}

fn is_two_dimensional_mapping(origin: u64, target: u64, exchange: &Exchange) -> bool {
    [origin, target].into_iter().all(|id| {
        exchange.records().get(&id).is_some_and(|record| {
            record.partial("AXIS2_PLACEMENT_2D").is_some()
                || record
                    .partial("CARTESIAN_TRANSFORMATION_OPERATOR_2D")
                    .is_some()
        })
    })
}

fn mapped_item_definition(item: &RawRecord, exchange: &Exchange) -> Option<(u64, u64, u64)> {
    let map = named_parameter(item, "MAPPED_ITEM", 1)
        .and_then(ValueExt::reference)
        .and_then(|map| exchange.records().get(&map))?;
    let origin = named_parameter(map, "REPRESENTATION_MAP", 0).and_then(ValueExt::reference)?;
    let representation =
        named_parameter(map, "REPRESENTATION_MAP", 1).and_then(ValueExt::reference)?;
    let target = named_parameter(item, "MAPPED_ITEM", 2).and_then(ValueExt::reference)?;
    Some((representation, origin, target))
}

fn occurrence_placement(
    record: &RawRecord,
    exchange: &Exchange,
    geometry: &GeometryData,
    pds: &BTreeMap<u64, u64>,
    usages: &BTreeMap<u64, Usage>,
    definition_representations: &BTreeMap<u64, BTreeSet<u64>>,
) -> Result<Option<(u64, Transform)>, TransformError> {
    let Some((usage, from_id, to_id)) =
        occurrence_placement_definition(record, exchange, pds, usages, definition_representations)
    else {
        return Ok(None);
    };
    Ok(mapped_item_transform(from_id, to_id, geometry)?.map(|transform| (usage, transform)))
}

fn occurrence_placement_definition(
    record: &RawRecord,
    exchange: &Exchange,
    pds: &BTreeMap<u64, u64>,
    usages: &BTreeMap<u64, Usage>,
    definition_representations: &BTreeMap<u64, BTreeSet<u64>>,
) -> Option<(u64, u64, u64)> {
    let relation = exchange.records().get(
        &named_parameter(record, "CONTEXT_DEPENDENT_SHAPE_REPRESENTATION", 0)
            .and_then(ValueExt::reference)?,
    )?;
    let usage = *pds.get(
        &named_parameter(record, "CONTEXT_DEPENDENT_SHAPE_REPRESENTATION", 1)
            .and_then(ValueExt::reference)?,
    )?;
    let usage_data = usages.get(&usage)?;
    let child_representations = definition_representations.get(&usage_data.child_definition)?;
    let parent_representations = definition_representations.get(&usage_data.parent_definition)?;
    let relation_representations = representation_relationship_endpoints(relation)?;
    let transform_id = relation
        .partial("REPRESENTATION_RELATIONSHIP_WITH_TRANSFORMATION")?
        .parameters
        .first()?
        .reference()?;
    let transform = exchange.records().get(&transform_id)?;
    let item_one = named_parameter(transform, "ITEM_DEFINED_TRANSFORMATION", 2)
        .and_then(ValueExt::reference)?;
    let item_two = named_parameter(transform, "ITEM_DEFINED_TRANSFORMATION", 3)
        .and_then(ValueExt::reference)?;
    let child_to_parent = child_representations.contains(&relation_representations.0)
        && parent_representations.contains(&relation_representations.1);
    let parent_to_child = parent_representations.contains(&relation_representations.0)
        && child_representations.contains(&relation_representations.1);
    let (from_id, to_id) = match (child_to_parent, parent_to_child) {
        (true, false) => (item_one, item_two),
        (false, true) => (item_two, item_one),
        _ => return None,
    };
    Some((usage, from_id, to_id))
}

fn transformation_item(id: u64, geometry: &GeometryData) -> Option<Transform> {
    geometry
        .placements
        .get(&id)
        .copied()
        .and_then(super::geometry::placement_transform)
        .or_else(|| geometry.transformation_operators.get(&id).copied())
}

fn representation_relationship_endpoints(record: &RawRecord) -> Option<(u64, u64)> {
    let relationship = record
        .partial("REPRESENTATION_RELATIONSHIP")
        .or_else(|| record.partial("SHAPE_REPRESENTATION_RELATIONSHIP"))?;
    let mut references = relationship
        .parameters
        .iter()
        .filter_map(ValueExt::reference);
    Some((references.next()?, references.next()?))
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

fn product_definition_formation_parameters(record: &RawRecord) -> Option<&[Value]> {
    if let Some(partial) = record.partial("PRODUCT_DEFINITION_FORMATION") {
        return Some(partial.parameters.as_slice());
    }
    match record.simple_name() {
        Some("PRODUCT_DEFINITION_FORMATION_WITH_SPECIFIED_SOURCE" | "FINAL_SOLUTION") => {
            Some(record.partials.first().parameters.as_slice())
        }
        _ => None,
    }
}

fn product_definition_parameters(record: &RawRecord) -> Option<&[Value]> {
    if let Some(partial) = record.partial("PRODUCT_DEFINITION") {
        return Some(partial.parameters.as_slice());
    }
    match record.simple_name() {
        Some("PRODUCT_DEFINITION_WITH_ASSOCIATED_DOCUMENTS") => {
            Some(record.partials.first().parameters.as_slice())
        }
        _ => None,
    }
}

#[cfg(test)]
pub(crate) mod tests;
