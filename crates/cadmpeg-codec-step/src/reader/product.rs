// SPDX-License-Identifier: Apache-2.0
//! STEP product prototypes, occurrence identity, and relative placement.

use std::collections::{BTreeMap, BTreeSet, HashSet, VecDeque};

use cadmpeg_core::decode::DecodeContext;
use cadmpeg_core::CodecError;
use cadmpeg_ir::document::CadIr;
use cadmpeg_ir::ids::{BodyId, OccurrenceId, ProductDefinitionId};
use cadmpeg_ir::products::{
    Occurrence, OccurrenceParent, ProductDefinition, ProductDefinitionKind, PrototypeReference,
};
use cadmpeg_ir::report::LossNote;
use cadmpeg_ir::transform::Transform;

use crate::ids::StepIdentity;
use crate::loss::StepLossCode;
use crate::parse::{Exchange, RawRecord, Value};

use super::decode_text;
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

pub(super) struct ProductData {
    pub product_definition_ids_by_source: BTreeMap<u64, Vec<ProductDefinitionId>>,
    pub product_definition_ids_by_shape: BTreeMap<u64, ProductDefinitionId>,
}

pub(super) fn decode(
    exchange: &Exchange,
    geometry: &GeometryData,
    topology: &TopologyData,
    ir: &mut CadIr,
    ctx: Option<&DecodeContext<'_>>,
    admitted_ir_entities: &mut u64,
) -> Result<StageOutcome<ProductData>, CodecError> {
    let mut typed = HashSet::new();
    let mut warnings = Vec::new();
    let mut losses = Vec::new();
    let formations = exchange
        .entities_any(PRODUCT_DEFINITION_FORMATION_TYPES)
        .filter_map(|(id, record)| {
            let parameters = product_definition_formation_parameters(record)?;
            Some((id, parameters.get(2)?.reference()?))
        })
        .collect::<BTreeMap<_, _>>();
    let definitions = exchange
        .entities_any(PRODUCT_DEFINITION_TYPES)
        .filter_map(|(id, record)| {
            let parameters = product_definition_parameters(record)?;
            Some((id, *formations.get(&parameters.get(2)?.reference()?)?))
        })
        .collect::<BTreeMap<_, _>>();
    let mut definitions_by_product_in_source_order = definitions.iter().fold(
        BTreeMap::<u64, Vec<u64>>::new(),
        |mut definitions_by_product, (&definition, &product)| {
            definitions_by_product
                .entry(product)
                .or_default()
                .push(definition);
            definitions_by_product
        },
    );
    for definitions in definitions_by_product_in_source_order.values_mut() {
        definitions.sort_by_key(|definition| {
            exchange
                .records
                .get(definition)
                .map_or(usize::MAX, |record| record.span.start)
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
            decode_text(
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
    let shape_bindings = shape_bindings(exchange, &definitions, topology, ctx);
    let definition_counts =
        definitions
            .values()
            .fold(BTreeMap::<u64, usize>::new(), |mut counts, product| {
                *counts.entry(*product).or_default() += 1;
                counts
            });
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
                decode_text(
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
                decode_text(
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
                decode_text(
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
            .cloned()
            .unwrap_or_default();
        let definition_count = definition_counts.get(&step_id).copied().unwrap_or(0);
        let definition_iter = if product_definitions.is_empty() {
            vec![None]
        } else {
            product_definitions.into_iter().map(Some).collect()
        };
        for definition in definition_iter {
            let product_definition_id = definition.map_or_else(
                || product_ir_id(step_id),
                |definition| {
                    let id = product_definition_ir_id(step_id, definition, definition_count);
                    definition_prototypes.insert(definition, id.clone());
                    id
                },
            );
            let definition_description =
                definition.and_then(|definition| definition_descriptions.get(&definition).cloned());
            let description = if definition_count <= 1 {
                product_description.clone().or(definition_description)
            } else {
                definition_description.or_else(|| product_description.clone())
            };
            let has_shape_binding =
                definition.is_some_and(|definition| shape_bindings.contains_key(&definition));
            let mut bodies = definition
                .and_then(|definition| shape_bindings.get(&definition).cloned())
                .unwrap_or_default();
            let missing = bodies
                .iter()
                .filter(|body| {
                    !ir.model
                        .bodies
                        .iter()
                        .any(|candidate| candidate.id == **body)
                })
                .map(|body| body.0.clone())
                .collect::<Vec<_>>();
            bodies.retain(|body| {
                ir.model
                    .bodies
                    .iter()
                    .any(|candidate| candidate.id == *body)
            });
            bodies.sort();
            bodies.dedup();
            let owner = definition.map_or_else(
                || format!("PRODUCT #{step_id}"),
                |definition| format!("PRODUCT_DEFINITION #{definition}"),
            );
            if !missing.is_empty() {
                warnings.push(format!(
                    "{owner} omitted uncommitted shape body reference(s): {}",
                    missing.join(", ")
                ));
            }
            if has_shape_binding && bodies.is_empty() {
                warnings.push(format!(
                    "{owner} has a shape representation with no committed topology body"
                ));
            }
            ir.model.product_definitions.push(ProductDefinition {
                id: product_definition_id.clone(),
                kind: ProductDefinitionKind::Part,
                source_name: name.clone(),
                label: name.clone(),
                description,
                part_number: Some(product_id.clone()),
                bom_properties: BTreeMap::new(),
                bodies,
                native_ref: Some(
                    definition.map_or_else(|| format!("#{step_id}"), |id| format!("#{id}")),
                ),
            });
            product_definition_ids_by_source
                .entry(step_id)
                .or_default()
                .push(product_definition_id);
        }
        typed.insert(step_id);
    }
    let product_definition_ids_by_shape = exchange
        .entities("PRODUCT_DEFINITION_SHAPE")
        .filter_map(|(shape_id, record)| {
            let definition = named_parameter(record, "PRODUCT_DEFINITION_SHAPE", 2)
                .and_then(ValueExt::reference)?;
            Some((shape_id, definition_prototypes.get(&definition)?.clone()))
        })
        .collect();
    typed.extend(formations.keys().copied());
    typed.extend(definitions.keys().copied());

    let usages = exchange
        .entities("NEXT_ASSEMBLY_USAGE_OCCURRENCE")
        .filter_map(|(id, record)| {
            let name =
                named_parameter(record, "NEXT_ASSEMBLY_USAGE_OCCURRENCE", 1).and_then(|value| {
                    decode_text(
                        exchange,
                        value,
                        &mut losses,
                        id,
                        "assembly occurrence name",
                        StepLossCode::MetadataStringInvalid,
                    )
                });
            Some((
                id,
                Usage {
                    parent_definition: named_parameter(record, "NEXT_ASSEMBLY_USAGE_OCCURRENCE", 3)
                        .and_then(ValueExt::reference)?,
                    child_definition: named_parameter(record, "NEXT_ASSEMBLY_USAGE_OCCURRENCE", 4)
                        .and_then(ValueExt::reference)?,
                    name: name.filter(|name| !name.is_empty()),
                },
            ))
        })
        .collect::<BTreeMap<_, _>>();
    let child_definitions = usages
        .values()
        .map(|usage| usage.child_definition)
        .collect::<BTreeSet<_>>();
    let mut occurrence_paths = BTreeMap::<OccurrenceId, BTreeSet<u64>>::new();
    let mut pending_occurrences = VecDeque::new();
    let mut root_ordinal = 0_u32;
    for &definition in definitions.keys() {
        if child_definitions.contains(&definition) {
            continue;
        }
        let Some(prototype) = definition_prototypes.get(&definition).cloned() else {
            warnings.push(format!(
                "PRODUCT_DEFINITION #{definition} has no local product prototype"
            ));
            continue;
        };
        let id = OccurrenceId(StepIdentity::product(
            "occurrence",
            format!("definition-{definition}"),
        ));
        ir.model.occurrences.push(Occurrence {
            id: id.clone(),
            prototype: PrototypeReference::Local {
                definition: prototype,
            },
            parent: OccurrenceParent::Root,
            ordinal: root_ordinal,
            transform: Transform::identity(),
            prototype_transform: Transform::identity(),
            scale: [1.0; 3],
            name: None,
            linked_subelements: Vec::new(),
            visible: None,
            element_component: None,
            claim_child: None,
            copy_on_change: None,
            copy_on_change_source: None,
            copy_on_change_group: None,
            copy_on_change_touched: None,
            link_transform: None,
            native_ref: None,
        });
        admit_occurrence(ctx, ir, admitted_ir_entities)?;
        root_ordinal = root_ordinal.saturating_add(1);
        occurrence_paths.insert(id.clone(), BTreeSet::from([definition]));
        pending_occurrences.push_back((definition, id));
    }
    let mut ambiguous_placements = BTreeMap::new();
    let mut competing_placements = BTreeMap::new();
    let placements = occurrence_placements(
        exchange,
        geometry,
        &usages,
        &mut warnings,
        &mut ambiguous_placements,
        &mut competing_placements,
    );
    for (&usage_id, source_ids) in &ambiguous_placements {
        if competing_placements.contains_key(&usage_id) {
            continue;
        }
        let records = source_ids
            .iter()
            .map(|id| format!("#{id}"))
            .collect::<Vec<_>>()
            .join(", ");
        losses.push(StepLossCode::NauoPlacementAmbiguous.note(format!(
            "NAUO #{usage_id} has multiple resolved CONTEXT_DEPENDENT_SHAPE_REPRESENTATION placements ({records}); no neutral occurrence was admitted and the source placement relations remain opaque"
        )));
    }
    for (&usage_id, source_ids) in &competing_placements {
        let records = source_ids
            .iter()
            .map(|id| format!("#{id}"))
            .collect::<Vec<_>>()
            .join(", ");
        losses.push(StepLossCode::NauoPlacementAmbiguous.note(format!(
            "NAUO #{usage_id} has resolved context-dependent and occurrence-owned mapped placements ({records}); no neutral occurrence was admitted and the source placement relations remain opaque"
        )));
    }
    let mut usage_instances = BTreeMap::<u64, usize>::new();
    let mut missing_placement_reports = BTreeSet::new();
    let mut child_ordinals = BTreeMap::<OccurrenceId, u32>::new();
    let mut usages_by_parent = BTreeMap::<u64, Vec<u64>>::new();
    for (&usage_id, usage) in &usages {
        usages_by_parent
            .entry(usage.parent_definition)
            .or_default()
            .push(usage_id);
    }
    let had_roots = !pending_occurrences.is_empty();
    'expansion: while let Some((parent_definition, parent)) = pending_occurrences.pop_front() {
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
                warnings.push(format!(
                    "NAUO #{usage_id} references an unresolved child definition"
                ));
                continue;
            };
            let parent_path = occurrence_paths.get(&parent).cloned().unwrap_or_default();
            let depth_limit = assembly_depth_limit(ctx);
            if parent_path.len() >= depth_limit {
                warnings.push(format!(
                    "NAUO #{usage_id} exceeds the {depth_limit}-level assembly depth limit"
                ));
                continue;
            }
            if parent_path.contains(&usage.child_definition) {
                warnings.push(format!(
                    "NAUO #{usage_id} closes an assembly definition cycle"
                ));
                continue;
            }
            let instance = usage_instances.entry(usage_id).or_default();
            *instance += 1;
            let suffix = if *instance == 1 {
                String::new()
            } else {
                format!("-instance-{instance}")
            };
            let id = OccurrenceId(StepIdentity::product(
                "occurrence",
                format!("{usage_id}{suffix}"),
            ));
            let occurrence_cap = occurrence_limit(ctx);
            if ir.model.occurrences.len() >= occurrence_cap {
                warnings.push(format!(
                    "assembly occurrence expansion exceeds the {occurrence_cap}-occurrence limit"
                ));
                break 'expansion;
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
                prototype_transform: Transform::identity(),
                scale: [1.0; 3],
                name: usage.name.clone(),
                linked_subelements: Vec::new(),
                visible: None,
                element_component: None,
                claim_child: None,
                copy_on_change: None,
                copy_on_change_source: None,
                copy_on_change_group: None,
                copy_on_change_touched: None,
                link_transform: None,
                native_ref: Some(format!("#{usage_id}")),
            });
            admit_occurrence(ctx, ir, admitted_ir_entities)?;
            *ordinal = ordinal.saturating_add(1);
            let mut path = parent_path;
            path.insert(usage.child_definition);
            occurrence_paths.insert(id.clone(), path);
            pending_occurrences.push_back((usage.child_definition, id));
            typed.insert(usage_id);
        }
    }
    if !had_roots && !usages.is_empty() {
        warnings.push("assembly occurrence graph has no resolvable root".into());
    }
    apply_body_placements(
        exchange,
        geometry,
        topology,
        &usages,
        ir,
        &mut warnings,
        &mut losses,
        ctx,
    );
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
        warnings,
        losses,
        notes: Vec::new(),
    })
}

fn admit_occurrence(
    ctx: Option<&DecodeContext<'_>>,
    ir: &CadIr,
    admitted: &mut u64,
) -> Result<(), CodecError> {
    let current = u64::try_from(ir.model.entity_count()).unwrap_or(u64::MAX);
    if let Some(ctx) = ctx {
        ctx.admit_entities(current, admitted, "step_assembly_occurrence")?;
    } else {
        *admitted = current;
    }
    Ok(())
}

fn occurrence_limit(ctx: Option<&DecodeContext<'_>>) -> usize {
    ctx.and_then(|ctx| usize::try_from(ctx.policy().limits.max_entities).ok())
        .map_or(MAX_OCCURRENCES, |policy| policy.min(MAX_OCCURRENCES))
}

fn assembly_depth_limit(ctx: Option<&DecodeContext<'_>>) -> usize {
    ctx.and_then(|ctx| usize::try_from(ctx.policy().limits.max_recursion_depth).ok())
        .map_or(MAX_ASSEMBLY_DEPTH, |policy| policy.min(MAX_ASSEMBLY_DEPTH))
}

#[allow(clippy::too_many_arguments)] // session ctx is the eighth decode-policy argument
fn apply_body_placements(
    exchange: &Exchange,
    geometry: &GeometryData,
    topology: &TopologyData,
    usages: &BTreeMap<u64, Usage>,
    ir: &mut CadIr,
    warnings: &mut Vec<String>,
    losses: &mut Vec<LossNote>,
    ctx: Option<&DecodeContext<'_>>,
) {
    let pds = exchange
        .entities("PRODUCT_DEFINITION_SHAPE")
        .filter_map(|(id, record)| {
            Some((
                id,
                named_parameter(record, "PRODUCT_DEFINITION_SHAPE", 2)
                    .and_then(ValueExt::reference)?,
            ))
        })
        .collect::<BTreeMap<_, _>>();
    let definition_representations = definition_representations(exchange, &pds);
    let assembly_representations = usages
        .values()
        .flat_map(|usage| {
            definition_representations
                .get(&usage.child_definition)
                .into_iter()
                .flatten()
                .copied()
        })
        .collect::<BTreeSet<_>>();
    let body_indices = ir
        .model
        .bodies
        .iter()
        .enumerate()
        .map(|(index, body)| (body.id.clone(), index))
        .collect::<BTreeMap<_, _>>();
    let mut representation_cache = BTreeMap::new();
    let mut placements_by_body = BTreeMap::<BodyId, Vec<(u64, Transform)>>::new();
    let drawing_owned_items = drawing_owned_items(exchange);
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
        let bodies = super::topology::representation_bodies(
            representation,
            exchange,
            topology,
            &mut representation_cache,
            &mut BTreeSet::new(),
            0,
            ctx,
        );
        if bodies.is_empty() {
            continue;
        }
        let Some(transform) = mapped_item_transform(origin, target, geometry) else {
            warnings.push(format!("MAPPED_ITEM #{id} has no resolved body placement"));
            continue;
        };
        for body in bodies {
            placements_by_body
                .entry(body)
                .or_default()
                .push((id, transform));
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
                let mapped_items = unique
                    .iter()
                    .map(|(id, _)| format!("#{id}"))
                    .collect::<Vec<_>>()
                    .join(", ");
                losses.push(StepLossCode::BodyConflictingMappedPlacements.note(format!(
                        "body {body} has conflicting standalone MAPPED_ITEM placements ({mapped_items}); no body placement was selected"
                    )));
            }
        }
    }
}

fn drawing_owned_items(exchange: &Exchange) -> BTreeSet<u64> {
    let mut pending = Vec::new();
    for record in exchange.records.values() {
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
                collect_references(value, &mut pending);
            }
        }
    }
    let mut items = BTreeSet::new();
    let mut visited = BTreeSet::new();
    while let Some(id) = pending.pop() {
        if !visited.insert(id) {
            continue;
        }
        let Some(record) = exchange.records.get(&id) else {
            continue;
        };
        if record.partial("MAPPED_ITEM").is_some() {
            items.insert(id);
            continue;
        }
        if let Some(representation_items) = super::representation::items(record) {
            pending.extend(representation_items);
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
                collect_references(value, &mut pending);
            }
        }
    }
    items
}

fn collect_references(value: &Value, references: &mut Vec<u64>) {
    match value {
        Value::Reference(id) => {
            references.push(*id);
        }
        Value::List(values) => {
            for value in values {
                collect_references(value, references);
            }
        }
        Value::Typed(_, value) => collect_references(value, references),
        _ => {}
    }
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
    ctx: Option<&DecodeContext<'_>>,
) -> BTreeMap<u64, Vec<BodyId>> {
    let pds = exchange
        .entities("PRODUCT_DEFINITION_SHAPE")
        .filter_map(|(id, record)| {
            Some((
                id,
                named_parameter(record, "PRODUCT_DEFINITION_SHAPE", 2)
                    .and_then(ValueExt::reference)?,
            ))
        })
        .collect::<BTreeMap<_, _>>();
    let mut result = BTreeMap::<u64, Vec<BodyId>>::new();
    let mut representation_cache = BTreeMap::new();
    for record in exchange
        .records
        .values()
        .filter(|record| record.partial("SHAPE_DEFINITION_REPRESENTATION").is_some())
    {
        if let Some((definition, bodies)) = shape_binding(
            record,
            exchange,
            &pds,
            definitions,
            topology,
            &mut representation_cache,
            ctx,
        ) {
            result.entry(definition).or_default().extend(bodies);
        }
    }
    result
}

fn shape_binding(
    record: &RawRecord,
    exchange: &Exchange,
    pds: &BTreeMap<u64, u64>,
    definitions: &BTreeMap<u64, u64>,
    topology: &TopologyData,
    representation_cache: &mut BTreeMap<u64, Vec<BodyId>>,
    ctx: Option<&DecodeContext<'_>>,
) -> Option<(u64, Vec<BodyId>)> {
    let definition = *pds.get(
        &named_parameter(record, "SHAPE_DEFINITION_REPRESENTATION", 0)
            .and_then(ValueExt::reference)?,
    )?;
    definitions.get(&definition)?;
    let representation = named_parameter(record, "SHAPE_DEFINITION_REPRESENTATION", 1)
        .and_then(ValueExt::reference)?;
    let bodies = super::topology::representation_bodies(
        representation,
        exchange,
        topology,
        representation_cache,
        &mut BTreeSet::new(),
        0,
        ctx,
    );
    Some((definition, bodies))
}

fn definition_representations(
    exchange: &Exchange,
    pds: &BTreeMap<u64, u64>,
) -> BTreeMap<u64, BTreeSet<u64>> {
    exchange
        .entities("SHAPE_DEFINITION_REPRESENTATION")
        .filter_map(|(_, record)| {
            let shape = named_parameter(record, "SHAPE_DEFINITION_REPRESENTATION", 0)
                .and_then(ValueExt::reference)?;
            let definition = *pds.get(&shape)?;
            Some((
                definition,
                named_parameter(record, "SHAPE_DEFINITION_REPRESENTATION", 1)
                    .and_then(ValueExt::reference)?,
            ))
        })
        .fold(
            BTreeMap::<u64, BTreeSet<u64>>::new(),
            |mut result, (definition, representation)| {
                result.entry(definition).or_default().insert(representation);
                result
            },
        )
}

fn occurrence_placements(
    exchange: &Exchange,
    geometry: &GeometryData,
    usages: &BTreeMap<u64, Usage>,
    warnings: &mut Vec<String>,
    ambiguous: &mut BTreeMap<u64, Vec<u64>>,
    competing: &mut BTreeMap<u64, Vec<u64>>,
) -> BTreeMap<u64, Transform> {
    let pds = exchange
        .records
        .iter()
        .filter_map(|(&id, record)| {
            Some((
                id,
                named_parameter(record, "PRODUCT_DEFINITION_SHAPE", 2)
                    .and_then(ValueExt::reference)?,
            ))
        })
        .collect::<BTreeMap<_, _>>();
    let definition_representations = definition_representations(exchange, &pds);
    let mut definitions_by_representation = BTreeMap::<u64, BTreeSet<u64>>::new();
    for (&definition, representations) in &definition_representations {
        for &representation in representations {
            definitions_by_representation
                .entry(representation)
                .or_default()
                .insert(definition);
        }
    }
    let representation_links = representation_links(exchange);
    let mut result = BTreeMap::new();
    let mut context_candidates = BTreeMap::<u64, Vec<u64>>::new();
    for (record_id, record) in exchange.entities("CONTEXT_DEPENDENT_SHAPE_REPRESENTATION") {
        if let Some((usage, transform)) = occurrence_placement(
            record,
            exchange,
            geometry,
            &pds,
            usages,
            &definition_representations,
            &representation_links,
        ) {
            if usages.contains_key(&usage) {
                context_candidates.entry(usage).or_default().push(record_id);
                result.insert(usage, transform);
            }
        }
    }
    for (&usage, source_ids) in &context_candidates {
        if source_ids.len() > 1 {
            let mut source_ids = source_ids.clone();
            source_ids.sort_unstable();
            source_ids.dedup();
            ambiguous.insert(usage, source_ids);
        }
    }
    let occurrence_representations = exchange
        .entities("SHAPE_DEFINITION_REPRESENTATION")
        .filter_map(|(record_id, record)| {
            let shape = named_parameter(record, "SHAPE_DEFINITION_REPRESENTATION", 0)
                .and_then(ValueExt::reference)?;
            let usage = *pds.get(&shape)?;
            usages.contains_key(&usage).then_some((
                usage,
                (
                    record_id,
                    named_parameter(record, "SHAPE_DEFINITION_REPRESENTATION", 1)
                        .and_then(ValueExt::reference)?,
                ),
            ))
        })
        .fold(
            BTreeMap::<u64, Vec<(u64, u64)>>::new(),
            |mut result, (usage, representation)| {
                result.entry(usage).or_default().push(representation);
                result
            },
        );
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
            let Some(record) = exchange.records.get(&representation) else {
                continue;
            };
            let Some(items) = super::representation::items(record) else {
                continue;
            };
            for item_id in items {
                let Some(item) = exchange.records.get(&item_id) else {
                    continue;
                };
                if item.partial("MAPPED_ITEM").is_none() {
                    continue;
                }
                let Some((mapped_representation, transform)) =
                    mapped_item_placement(item, exchange, geometry)
                else {
                    continue;
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
            let mut source_ids = context_candidates[&usage_id].clone();
            source_ids.extend(candidates.iter().map(|(source_id, _)| *source_id));
            source_ids.sort_unstable();
            source_ids.dedup();
            result.remove(&usage_id);
            ambiguous.insert(usage_id, source_ids.clone());
            competing.insert(usage_id, source_ids);
            continue;
        }
        match candidates.as_slice() {
            [(_, transform)] => {
                result.insert(usage_id, *transform);
            }
            [] => {}
            _ => warnings.push(format!(
                "NAUO #{usage_id} has an ambiguous occurrence shape placement"
            )),
        }
    }
    let mut sibling_usage_counts = BTreeMap::<(u64, u64), usize>::new();
    for usage in usages.values() {
        *sibling_usage_counts
            .entry((usage.parent_definition, usage.child_definition))
            .or_default() += 1;
    }
    for (&usage_id, usage) in usages {
        if result.contains_key(&usage_id) {
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
            let Some(record) = exchange.records.get(&parent_representation) else {
                continue;
            };
            let Some(items) = super::representation::items(record) else {
                continue;
            };
            for item_id in items {
                let Some(item) = exchange.records.get(&item_id) else {
                    continue;
                };
                if item.partial("MAPPED_ITEM").is_none() {
                    continue;
                }
                let Some((mapped_representation, transform)) =
                    mapped_item_placement(item, exchange, geometry)
                else {
                    continue;
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
            warnings.push(format!(
                "NAUO #{usage_id} has an ambiguous mapped-item placement"
            ));
        }
    }
    result
}

fn mapped_item_placement(
    item: &RawRecord,
    exchange: &Exchange,
    geometry: &GeometryData,
) -> Option<(u64, Transform)> {
    let (representation, origin, target) = mapped_item_definition(item, exchange)?;
    Some((
        representation,
        mapped_item_transform(origin, target, geometry)?,
    ))
}

fn mapped_item_transform(origin: u64, target: u64, geometry: &GeometryData) -> Option<Transform> {
    let from = transformation_item(origin, geometry)?;
    let to = transformation_item(target, geometry)?;
    Some(to.compose(from.try_inverse_affine()?))
}

fn is_two_dimensional_mapping(origin: u64, target: u64, exchange: &Exchange) -> bool {
    [origin, target].into_iter().all(|id| {
        exchange.records.get(&id).is_some_and(|record| {
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
        .and_then(|map| exchange.records.get(&map))?;
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
    representation_links: &BTreeMap<u64, BTreeSet<u64>>,
) -> Option<(u64, Transform)> {
    let relation = exchange.records.get(
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
    let transform = exchange.records.get(&transform_id)?;
    let item_one = named_parameter(transform, "ITEM_DEFINED_TRANSFORMATION", 2)
        .and_then(ValueExt::reference)?;
    let item_two = named_parameter(transform, "ITEM_DEFINED_TRANSFORMATION", 3)
        .and_then(ValueExt::reference)?;
    let child_to_parent = representation_matches(
        relation_representations.0,
        child_representations,
        representation_links,
    ) && representation_matches(
        relation_representations.1,
        parent_representations,
        representation_links,
    );
    let parent_to_child = representation_matches(
        relation_representations.0,
        parent_representations,
        representation_links,
    ) && representation_matches(
        relation_representations.1,
        child_representations,
        representation_links,
    );
    let (from_id, to_id) = match (child_to_parent, parent_to_child) {
        (true, false) => (item_one, item_two),
        (false, true) => (item_two, item_one),
        _ => return None,
    };
    let from = transformation_item(from_id, geometry)?;
    let to = transformation_item(to_id, geometry)?;
    Some((usage, to.compose(from.try_inverse_affine()?)))
}

fn transformation_item(id: u64, geometry: &GeometryData) -> Option<Transform> {
    geometry
        .placements
        .get(&id)
        .copied()
        .map(super::geometry::placement_transform)
        .or_else(|| geometry.transformation_operators.get(&id).copied())
}

fn representation_links(exchange: &Exchange) -> BTreeMap<u64, BTreeSet<u64>> {
    let mut links = BTreeMap::<u64, BTreeSet<u64>>::new();
    for record in exchange.records.values() {
        let Some(relationship) = record.partial("SHAPE_REPRESENTATION_RELATIONSHIP") else {
            continue;
        };
        if relationship.parameters.is_empty() {
            continue;
        }
        let Some((left, right)) = representation_relationship_endpoints(record) else {
            continue;
        };
        links.entry(left).or_default().insert(right);
        links.entry(right).or_default().insert(left);
    }
    links
}

fn representation_matches(
    candidate: u64,
    definitions: &BTreeSet<u64>,
    links: &BTreeMap<u64, BTreeSet<u64>>,
) -> bool {
    if definitions.contains(&candidate) {
        return true;
    }
    let mut pending = VecDeque::from([candidate]);
    let mut visited = BTreeSet::from([candidate]);
    while let Some(current) = pending.pop_front() {
        for &linked in links.get(&current).into_iter().flatten() {
            if definitions.contains(&linked) {
                return true;
            }
            if visited.insert(linked) {
                pending.push_back(linked);
            }
        }
    }
    false
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
    ProductDefinitionId(StepIdentity::product("product", id))
}

fn product_definition_ir_id(
    product: u64,
    definition: u64,
    definition_count: usize,
) -> ProductDefinitionId {
    if definition_count == 1 {
        product_ir_id(product)
    } else {
        ProductDefinitionId(StepIdentity::product(
            "product",
            format!("{product}-definition-{definition}"),
        ))
    }
}

fn product_definition_formation_parameters(record: &RawRecord) -> Option<&[Value]> {
    if let Some(partial) = record.partial("PRODUCT_DEFINITION_FORMATION") {
        return Some(partial.parameters.as_slice());
    }
    match record.simple_name() {
        Some("PRODUCT_DEFINITION_FORMATION_WITH_SPECIFIED_SOURCE" | "FINAL_SOLUTION") => record
            .partials
            .first()
            .map(|partial| partial.parameters.as_slice()),
        _ => None,
    }
}

fn product_definition_parameters(record: &RawRecord) -> Option<&[Value]> {
    if let Some(partial) = record.partial("PRODUCT_DEFINITION") {
        return Some(partial.parameters.as_slice());
    }
    match record.simple_name() {
        Some("PRODUCT_DEFINITION_WITH_ASSOCIATED_DOCUMENTS") => record
            .partials
            .first()
            .map(|partial| partial.parameters.as_slice()),
        _ => None,
    }
}

fn named_parameter<'a>(record: &'a RawRecord, name: &str, index: usize) -> Option<&'a Value> {
    record.partial(name)?.parameters.get(index)
}

trait RecordExt {
    fn simple_name(&self) -> Option<&str>;
    fn partial(&self, name: &str) -> Option<&crate::parse::PartialRecord>;
}
impl RecordExt for RawRecord {
    fn simple_name(&self) -> Option<&str> {
        (self.partials.len() == 1).then(|| self.partials[0].name.as_str())
    }
    fn partial(&self, name: &str) -> Option<&crate::parse::PartialRecord> {
        self.partials.iter().find(|partial| partial.name == name)
    }
}
trait ValueExt {
    fn reference(&self) -> Option<u64>;
}
impl ValueExt for Value {
    fn reference(&self) -> Option<u64> {
        if let Value::Reference(id) = self {
            Some(*id)
        } else {
            None
        }
    }
}

#[cfg(test)]
pub(crate) mod tests;
