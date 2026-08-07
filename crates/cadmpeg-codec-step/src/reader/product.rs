// SPDX-License-Identifier: Apache-2.0
//! STEP product prototypes, occurrence identity, and relative placement.

use std::collections::{BTreeMap, BTreeSet, VecDeque};

use cadmpeg_ir::document::CadIr;
use cadmpeg_ir::ids::{BodyId, OccurrenceId, ProductDefinitionId};
use cadmpeg_ir::math::{Point3, Vector3};
use cadmpeg_ir::products::{
    Occurrence, OccurrenceParent, ProductDefinition, ProductDefinitionKind, PrototypeReference,
};
use cadmpeg_ir::report::{LossKind, LossNote, Severity};
use cadmpeg_ir::transform::Transform;

use crate::parse::{Exchange, RawRecord, Value};

use super::decode_text;
use super::geometry::GeometryResult;
use super::topology::TopologyResult;

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

pub(super) struct ProductResult {
    pub typed_records: BTreeSet<u64>,
    pub warnings: Vec<String>,
    pub losses: Vec<LossNote>,
}

pub(super) fn decode(
    exchange: &Exchange,
    geometry: &GeometryResult,
    topology: &TopologyResult,
    ir: &mut CadIr,
) -> ProductResult {
    let mut typed = BTreeSet::new();
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
    let shape_bindings = shape_bindings(exchange, &definitions, topology);

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
                    value,
                    &mut losses,
                    step_id,
                    "product identifier",
                    LossKind::MetadataNotTransferred,
                )
            })
            .unwrap_or_else(|| format!("#{step_id}"));
        let name = parameters
            .get(1)
            .and_then(|value| {
                decode_text(
                    value,
                    &mut losses,
                    step_id,
                    "product name",
                    LossKind::MetadataNotTransferred,
                )
            })
            .filter(|name| !name.is_empty());
        let has_shape_binding = shape_bindings.contains_key(&step_id);
        let mut bodies = shape_bindings.get(&step_id).cloned().unwrap_or_default();
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
        if !missing.is_empty() {
            warnings.push(format!(
                "PRODUCT #{step_id} omitted uncommitted shape body reference(s): {}",
                missing.join(", ")
            ));
        }
        if has_shape_binding && bodies.is_empty() {
            warnings.push(format!(
                "PRODUCT #{step_id} has a shape representation with no committed topology body"
            ));
        }
        ir.model.product_definitions.push(ProductDefinition {
            id: product_ir_id(step_id),
            kind: ProductDefinitionKind::Part,
            source_name: name.clone(),
            label: name,
            description: None,
            part_number: Some(product_id),
            bom_properties: BTreeMap::new(),
            bodies,
            native_ref: Some(format!("#{step_id}")),
        });
        typed.insert(step_id);
    }
    typed.extend(formations.keys().copied());
    typed.extend(definitions.keys().copied());

    let usages = exchange
        .entities("NEXT_ASSEMBLY_USAGE_OCCURRENCE")
        .filter_map(|(id, record)| {
            let name =
                named_parameter(record, "NEXT_ASSEMBLY_USAGE_OCCURRENCE", 1).and_then(|value| {
                    decode_text(
                        value,
                        &mut losses,
                        id,
                        "assembly occurrence name",
                        LossKind::MetadataNotTransferred,
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
    let mut definition_occurrences = BTreeMap::<u64, Vec<OccurrenceId>>::new();
    let mut occurrence_paths = BTreeMap::<OccurrenceId, BTreeSet<u64>>::new();
    let mut pending_occurrences = VecDeque::new();
    let mut root_ordinal = 0_u32;
    for (&definition, &product) in &definitions {
        if child_definitions.contains(&definition) {
            continue;
        }
        let id = OccurrenceId(format!("step:product:occurrence#definition-{definition}"));
        ir.model.occurrences.push(Occurrence {
            id: id.clone(),
            prototype: PrototypeReference::Local {
                definition: product_ir_id(product),
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
        root_ordinal = root_ordinal.saturating_add(1);
        definition_occurrences
            .entry(definition)
            .or_default()
            .push(id.clone());
        occurrence_paths.insert(id.clone(), BTreeSet::from([definition]));
        pending_occurrences.push_back((definition, id));
    }
    let placements = occurrence_placements(exchange, geometry, &usages, &mut warnings);
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
            let usage = &usages[&usage_id];
            let Some(&product) = definitions.get(&usage.child_definition) else {
                warnings.push(format!(
                    "NAUO #{usage_id} references an unresolved child definition"
                ));
                continue;
            };
            let parent_path = occurrence_paths.get(&parent).cloned().unwrap_or_default();
            if parent_path.len() >= MAX_ASSEMBLY_DEPTH {
                warnings.push(format!(
                    "NAUO #{usage_id} exceeds the {MAX_ASSEMBLY_DEPTH}-level assembly depth limit"
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
            let id = OccurrenceId(format!("step:product:occurrence#{usage_id}{suffix}"));
            if ir.model.occurrences.len() >= MAX_OCCURRENCES {
                warnings.push(format!(
                    "assembly occurrence expansion exceeds the {MAX_OCCURRENCES}-occurrence limit"
                ));
                break 'expansion;
            }
            let ordinal = child_ordinals.entry(parent.clone()).or_default();
            let transform = if let Some(transform) = placements.get(&usage_id).copied() {
                transform
            } else {
                if missing_placement_reports.insert(usage_id) {
                    losses.push(LossNote {
                        code: LossKind::AssemblyPlacementsNotTransferred,
                        severity: Severity::Error,
                        message: format!(
                            "NAUO #{usage_id} has no resolved occurrence transform; \
                             identity placement was used"
                        ),
                        provenance: None,
                    });
                }
                Transform::identity()
            };
            ir.model.occurrences.push(Occurrence {
                id: id.clone(),
                prototype: PrototypeReference::Local {
                    definition: product_ir_id(product),
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
            *ordinal = ordinal.saturating_add(1);
            let mut path = parent_path;
            path.insert(usage.child_definition);
            occurrence_paths.insert(id.clone(), path);
            definition_occurrences
                .entry(usage.child_definition)
                .or_default()
                .push(id.clone());
            pending_occurrences.push_back((usage.child_definition, id));
            typed.insert(usage_id);
        }
    }
    if !had_roots && !usages.is_empty() {
        warnings.push("assembly occurrence graph has no resolvable root".into());
    }
    apply_body_placements(exchange, geometry, topology, &usages, ir, &mut warnings);
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
    ProductResult {
        typed_records: typed,
        warnings,
        losses,
    }
}

fn apply_body_placements(
    exchange: &Exchange,
    geometry: &GeometryResult,
    topology: &TopologyResult,
    usages: &BTreeMap<u64, Usage>,
    ir: &mut CadIr,
    warnings: &mut Vec<String>,
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
    for (id, item) in exchange.entities("MAPPED_ITEM") {
        if item.partial("MAPPED_ITEM").is_none() {
            continue;
        }
        let Some((representation, origin, target)) = mapped_item_definition(item, exchange) else {
            continue;
        };
        if assembly_representations.contains(&representation) {
            continue;
        }
        let bodies = super::topology::representation_bodies(
            representation,
            exchange,
            topology,
            &mut representation_cache,
            &mut BTreeSet::new(),
            0,
        );
        if bodies.is_empty() {
            continue;
        }
        let Some(transform) = mapped_item_transform(origin, target, geometry) else {
            warnings.push(format!("MAPPED_ITEM #{id} has no resolved body placement"));
            continue;
        };
        for body in bodies {
            if let Some(index) = body_indices.get(&body) {
                ir.model.bodies[*index].transform = Some(transform);
            }
        }
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
    topology: &TopologyResult,
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
        if let Some((product, bodies)) = shape_binding(
            record,
            exchange,
            &pds,
            definitions,
            topology,
            &mut representation_cache,
        ) {
            result.entry(product).or_default().extend(bodies);
        }
    }
    result
}

fn shape_binding(
    record: &RawRecord,
    exchange: &Exchange,
    pds: &BTreeMap<u64, u64>,
    definitions: &BTreeMap<u64, u64>,
    topology: &TopologyResult,
    representation_cache: &mut BTreeMap<u64, Vec<BodyId>>,
) -> Option<(u64, Vec<BodyId>)> {
    let definition = *pds.get(
        &named_parameter(record, "SHAPE_DEFINITION_REPRESENTATION", 0)
            .and_then(ValueExt::reference)?,
    )?;
    let product = *definitions.get(&definition)?;
    let representation = named_parameter(record, "SHAPE_DEFINITION_REPRESENTATION", 1)
        .and_then(ValueExt::reference)?;
    let bodies = super::topology::representation_bodies(
        representation,
        exchange,
        topology,
        representation_cache,
        &mut BTreeSet::new(),
        0,
    );
    Some((product, bodies))
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
    geometry: &GeometryResult,
    usages: &BTreeMap<u64, Usage>,
    warnings: &mut Vec<String>,
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
    let mut result = BTreeMap::new();
    for (_, record) in exchange.entities("CONTEXT_DEPENDENT_SHAPE_REPRESENTATION") {
        if let Some((usage, transform)) = occurrence_placement(record, exchange, geometry, &pds) {
            if usages.contains_key(&usage) {
                result.insert(usage, transform);
            }
        }
    }
    let definition_representations = definition_representations(exchange, &pds);
    let occurrence_representations = exchange
        .entities("SHAPE_DEFINITION_REPRESENTATION")
        .filter_map(|(_, record)| {
            let shape = named_parameter(record, "SHAPE_DEFINITION_REPRESENTATION", 0)
                .and_then(ValueExt::reference)?;
            let usage = *pds.get(&shape)?;
            usages.contains_key(&usage).then_some((
                usage,
                named_parameter(record, "SHAPE_DEFINITION_REPRESENTATION", 1)
                    .and_then(ValueExt::reference)?,
            ))
        })
        .fold(
            BTreeMap::<u64, Vec<u64>>::new(),
            |mut result, (usage, representation)| {
                result.entry(usage).or_default().push(representation);
                result
            },
        );
    for (&usage_id, representations) in &occurrence_representations {
        if result.contains_key(&usage_id) {
            continue;
        }
        let Some(usage) = usages.get(&usage_id) else {
            continue;
        };
        let Some(child_representations) = definition_representations.get(&usage.child_definition)
        else {
            continue;
        };
        let mut candidates = Vec::new();
        for &representation in representations {
            let Some(record) = exchange.records.get(&representation) else {
                continue;
            };
            let Some(items) = representation_items(record) else {
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
                    candidates.push(transform);
                }
            }
        }
        match candidates.as_slice() {
            [transform] => {
                result.insert(usage_id, *transform);
            }
            [] => {}
            _ => warnings.push(format!(
                "NAUO #{usage_id} has an ambiguous occurrence shape placement"
            )),
        }
    }
    let mut placements_by_representation = BTreeMap::<u64, Vec<Transform>>::new();
    for (_, record) in exchange.entities("MAPPED_ITEM") {
        let Some((mapped_representation, transform)) =
            mapped_item_placement(record, exchange, geometry)
        else {
            continue;
        };
        placements_by_representation
            .entry(mapped_representation)
            .or_default()
            .push(transform);
    }
    let mut usage_counts = BTreeMap::<u64, usize>::new();
    for usage in usages.values() {
        *usage_counts.entry(usage.child_definition).or_default() += 1;
    }
    for (&usage_id, usage) in usages {
        if result.contains_key(&usage_id) {
            continue;
        }
        let Some(child_representations) = definition_representations.get(&usage.child_definition)
        else {
            continue;
        };
        let placements = child_representations
            .iter()
            .flat_map(|representation| {
                placements_by_representation
                    .get(representation)
                    .into_iter()
                    .flatten()
                    .copied()
            })
            .collect::<Vec<_>>();
        let matching_usages = usage_counts[&usage.child_definition];
        if matching_usages == 1 && placements.len() == 1 {
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
    geometry: &GeometryResult,
) -> Option<(u64, Transform)> {
    let (representation, origin, target) = mapped_item_definition(item, exchange)?;
    Some((
        representation,
        mapped_item_transform(origin, target, geometry)?,
    ))
}

fn mapped_item_transform(origin: u64, target: u64, geometry: &GeometryResult) -> Option<Transform> {
    let from = geometry
        .placements
        .get(&origin)
        .copied()
        .map(placement_transform)
        .or_else(|| geometry.transformation_operators.get(&origin).copied())?;
    let to = geometry
        .placements
        .get(&target)
        .copied()
        .map(placement_transform)
        .or_else(|| geometry.transformation_operators.get(&target).copied())?;
    Some(to.compose(from.try_inverse_affine()?))
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
    geometry: &GeometryResult,
    pds: &BTreeMap<u64, u64>,
) -> Option<(u64, Transform)> {
    let relation = exchange.records.get(
        &named_parameter(record, "CONTEXT_DEPENDENT_SHAPE_REPRESENTATION", 0)
            .and_then(ValueExt::reference)?,
    )?;
    let usage = *pds.get(
        &named_parameter(record, "CONTEXT_DEPENDENT_SHAPE_REPRESENTATION", 1)
            .and_then(ValueExt::reference)?,
    )?;
    let transform_id = relation
        .partial("REPRESENTATION_RELATIONSHIP_WITH_TRANSFORMATION")?
        .parameters
        .first()?
        .reference()?;
    let transform = exchange.records.get(&transform_id)?;
    let from = geometry.placements.get(
        &named_parameter(transform, "ITEM_DEFINED_TRANSFORMATION", 2)
            .and_then(ValueExt::reference)?,
    )?;
    let to = geometry.placements.get(
        &named_parameter(transform, "ITEM_DEFINED_TRANSFORMATION", 3)
            .and_then(ValueExt::reference)?,
    )?;
    Some((usage, between(*from, *to)))
}

fn between(from: (Point3, Vector3, Vector3), to: (Point3, Vector3, Vector3)) -> Transform {
    let from_basis = basis(from.1, from.2);
    let to_basis = basis(to.1, to.2);
    let mut rotation = [[0.0; 3]; 3];
    for row in 0..3 {
        for column in 0..3 {
            rotation[row][column] = (0..3)
                .map(|axis| to_basis[row][axis] * from_basis[column][axis])
                .sum();
        }
    }
    let source = [from.0.x, from.0.y, from.0.z];
    let target = [to.0.x, to.0.y, to.0.z];
    let mut rows = Transform::identity().rows;
    for row in 0..3 {
        for column in 0..3 {
            rows[row][column] = rotation[row][column];
        }
        rows[row][3] = target[row]
            - (0..3)
                .map(|column| rotation[row][column] * source[column])
                .sum::<f64>();
    }
    Transform { rows }
}

fn placement_transform((origin, z_axis, x_axis): (Point3, Vector3, Vector3)) -> Transform {
    let placement_basis = basis(z_axis, x_axis);
    let mut rows = Transform::identity().rows;
    for row in 0..3 {
        for column in 0..3 {
            rows[row][column] = placement_basis[row][column];
        }
    }
    rows[0][3] = origin.x;
    rows[1][3] = origin.y;
    rows[2][3] = origin.z;
    Transform { rows }
}
fn basis(z: Vector3, x: Vector3) -> [[f64; 3]; 3] {
    let y = Vector3::new(
        z.y * x.z - z.z * x.y,
        z.z * x.x - z.x * x.z,
        z.x * x.y - z.y * x.x,
    );
    [[x.x, y.x, z.x], [x.y, y.y, z.y], [x.z, y.z, z.z]]
}
fn product_ir_id(id: u64) -> ProductDefinitionId {
    ProductDefinitionId(format!("step:product:product#{id}"))
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

fn representation_items(record: &RawRecord) -> Option<Vec<u64>> {
    record
        .partials
        .iter()
        .find(|partial| {
            partial.name == "REPRESENTATION" || partial.name.ends_with("_REPRESENTATION")
        })
        .and_then(|partial| partial.parameters.get(1))
        .and_then(ValueExt::list)
        .map(|items| items.iter().filter_map(ValueExt::reference).collect())
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
    fn list(&self) -> Option<&[Value]>;
}
impl ValueExt for Value {
    fn reference(&self) -> Option<u64> {
        if let Value::Reference(id) = self {
            Some(*id)
        } else {
            None
        }
    }
    fn list(&self) -> Option<&[Value]> {
        if let Value::List(values) = self {
            Some(values)
        } else {
            None
        }
    }
}
