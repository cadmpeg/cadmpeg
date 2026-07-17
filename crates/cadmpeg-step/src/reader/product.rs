// SPDX-License-Identifier: Apache-2.0
//! STEP product prototypes, occurrence identity, and relative placement.

use std::collections::{BTreeMap, BTreeSet, VecDeque};

use cadmpeg_ir::document::CadIr;
use cadmpeg_ir::ids::{BodyId, OccurrenceId, ProductId};
use cadmpeg_ir::math::{Point3, Vector3};
use cadmpeg_ir::product::{OccurrenceParent, Product, ProductOccurrence};
use cadmpeg_ir::transform::Transform;

use crate::parse::{Exchange, RawRecord, Value};

use super::geometry::GeometryResult;

const MAX_OCCURRENCES: usize = 100_000;
const MAX_ASSEMBLY_DEPTH: usize = 256;

pub(super) struct ProductResult {
    pub typed_records: BTreeSet<u64>,
    pub warnings: Vec<String>,
}

pub(super) fn decode(
    exchange: &Exchange,
    geometry: &GeometryResult,
    ir: &mut CadIr,
) -> ProductResult {
    let mut typed = BTreeSet::new();
    let mut warnings = Vec::new();
    let formations = exchange
        .entities_any(&[
            "PRODUCT_DEFINITION_FORMATION",
            "PRODUCT_DEFINITION_FORMATION_WITH_SPECIFIED_SOURCE",
        ])
        .filter_map(|(id, record)| {
            if !matches!(
                record.simple_name(),
                Some(
                    "PRODUCT_DEFINITION_FORMATION"
                        | "PRODUCT_DEFINITION_FORMATION_WITH_SPECIFIED_SOURCE"
                )
            ) {
                return None;
            }
            Some((id, record.parameter(2)?.reference()?))
        })
        .collect::<BTreeMap<_, _>>();
    let definitions = exchange
        .entities("PRODUCT_DEFINITION")
        .filter_map(|(id, record)| {
            if record.simple_name() != Some("PRODUCT_DEFINITION") {
                return None;
            }
            Some((id, *formations.get(&record.parameter(2)?.reference()?)?))
        })
        .collect::<BTreeMap<_, _>>();
    let shape_bindings = shape_bindings(exchange, &definitions);

    for (step_id, record) in exchange.entities("PRODUCT") {
        if record.simple_name() != Some("PRODUCT") {
            continue;
        }
        let product_id = record
            .parameter(0)
            .and_then(ValueExt::text)
            .unwrap_or_else(|| format!("#{step_id}"));
        let name = record
            .parameter(1)
            .and_then(ValueExt::text)
            .filter(|name| !name.is_empty());
        let bodies = shape_bindings.get(&step_id).cloned().unwrap_or_default();
        ir.model.products.push(Product {
            id: product_ir_id(step_id),
            product_id,
            name,
            bodies,
        });
        typed.insert(step_id);
    }
    typed.extend(formations.keys().copied());
    typed.extend(definitions.keys().copied());

    let usages = exchange
        .entities("NEXT_ASSEMBLY_USAGE_OCCURRENCE")
        .filter_map(|(id, record)| {
            if record.simple_name() != Some("NEXT_ASSEMBLY_USAGE_OCCURRENCE") {
                return None;
            }
            Some((
                id,
                Usage {
                    parent_definition: record.parameter(3)?.reference()?,
                    child_definition: record.parameter(4)?.reference()?,
                    name: record
                        .parameter(1)
                        .and_then(ValueExt::text)
                        .filter(|name| !name.is_empty()),
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
    for (&definition, &product) in &definitions {
        if child_definitions.contains(&definition) {
            continue;
        }
        let id = OccurrenceId(format!("step:product:occurrence#definition-{definition}"));
        ir.model.product_occurrences.push(ProductOccurrence {
            id: id.clone(),
            product: product_ir_id(product),
            parent: OccurrenceParent::Root,
            transform: Transform::identity(),
            name: None,
        });
        definition_occurrences
            .entry(definition)
            .or_default()
            .push(id.clone());
        occurrence_paths.insert(id.clone(), BTreeSet::from([definition]));
        pending_occurrences.push_back((definition, id));
    }
    let placements = occurrence_placements(exchange, geometry, &usages, &mut warnings);
    let mut usage_instances = BTreeMap::<u64, usize>::new();
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
            if ir.model.product_occurrences.len() >= MAX_OCCURRENCES {
                warnings.push(format!(
                    "assembly occurrence expansion exceeds the {MAX_OCCURRENCES}-occurrence limit"
                ));
                break 'expansion;
            }
            ir.model.product_occurrences.push(ProductOccurrence {
                id: id.clone(),
                product: product_ir_id(product),
                parent: OccurrenceParent::Occurrence {
                    occurrence: parent.clone(),
                },
                transform: placements
                    .get(&usage_id)
                    .copied()
                    .unwrap_or_else(Transform::identity),
                name: usage.name.clone(),
            });
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
    apply_body_placements(exchange, geometry, &usages, ir, &mut warnings);
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
        if matches!(
            record.simple_name(),
            Some(
                "APPLICATION_CONTEXT"
                    | "PRODUCT_CONTEXT"
                    | "PRODUCT_DEFINITION_CONTEXT"
                    | "PRODUCT_DEFINITION_SHAPE"
                    | "SHAPE_DEFINITION_REPRESENTATION"
                    | "ITEM_DEFINED_TRANSFORMATION"
                    | "CONTEXT_DEPENDENT_SHAPE_REPRESENTATION"
                    | "REPRESENTATION_MAP"
                    | "MAPPED_ITEM"
            )
        ) || record
            .partial("REPRESENTATION_RELATIONSHIP_WITH_TRANSFORMATION")
            .is_some()
        {
            typed.insert(id);
        }
    }
    ProductResult {
        typed_records: typed,
        warnings,
    }
}

fn apply_body_placements(
    exchange: &Exchange,
    geometry: &GeometryResult,
    usages: &BTreeMap<u64, Usage>,
    ir: &mut CadIr,
    warnings: &mut Vec<String>,
) {
    let pds = exchange
        .entities("PRODUCT_DEFINITION_SHAPE")
        .filter_map(|(id, record)| {
            (record.simple_name() == Some("PRODUCT_DEFINITION_SHAPE"))
                .then_some((id, record.parameter(2)?.reference()?))
        })
        .collect::<BTreeMap<_, _>>();
    let definition_representations = exchange
        .entities("SHAPE_DEFINITION_REPRESENTATION")
        .filter_map(|(_, record)| {
            let definition = *pds.get(&record.parameter(0)?.reference()?)?;
            Some((definition, record.parameter(1)?.reference()?))
        })
        .collect::<BTreeMap<_, _>>();
    let assembly_representations = usages
        .values()
        .filter_map(|usage| {
            definition_representations
                .get(&usage.child_definition)
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
        if item.simple_name() != Some("MAPPED_ITEM") {
            continue;
        }
        let Some(map) = item
            .parameter(1)
            .and_then(ValueExt::reference)
            .and_then(|map| exchange.records.get(&map))
        else {
            continue;
        };
        let Some(origin) = map.parameter(0).and_then(ValueExt::reference) else {
            continue;
        };
        let Some(representation) = map.parameter(1).and_then(ValueExt::reference) else {
            continue;
        };
        if assembly_representations.contains(&representation) {
            continue;
        }
        let Some(target) = item.parameter(2).and_then(ValueExt::reference) else {
            continue;
        };
        let Some(transform) = geometry
            .placements
            .get(&origin)
            .zip(geometry.placements.get(&target))
            .map(|(from, to)| between(*from, *to))
        else {
            warnings.push(format!("MAPPED_ITEM #{id} has no resolved body placement"));
            continue;
        };
        for body in representation_bodies(
            representation,
            exchange,
            &mut representation_cache,
            &mut BTreeSet::new(),
            0,
        ) {
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
) -> BTreeMap<u64, Vec<BodyId>> {
    let pds = exchange
        .entities("PRODUCT_DEFINITION_SHAPE")
        .filter_map(|(id, record)| {
            if record.simple_name() != Some("PRODUCT_DEFINITION_SHAPE") {
                return None;
            }
            Some((id, record.parameter(2)?.reference()?))
        })
        .collect::<BTreeMap<_, _>>();
    let mut result = BTreeMap::<u64, Vec<BodyId>>::new();
    let mut representation_cache = BTreeMap::new();
    for record in exchange
        .records
        .values()
        .filter(|record| record.simple_name() == Some("SHAPE_DEFINITION_REPRESENTATION"))
    {
        if let Some((product, bodies)) = shape_binding(
            record,
            exchange,
            &pds,
            definitions,
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
    representation_cache: &mut BTreeMap<u64, Vec<BodyId>>,
) -> Option<(u64, Vec<BodyId>)> {
    let definition = *pds.get(&record.parameter(0)?.reference()?)?;
    let product = *definitions.get(&definition)?;
    let representation = record.parameter(1)?.reference()?;
    let bodies = representation_bodies(
        representation,
        exchange,
        representation_cache,
        &mut BTreeSet::new(),
        0,
    );
    Some((product, bodies))
}

fn representation_bodies(
    representation: u64,
    exchange: &Exchange,
    cache: &mut BTreeMap<u64, Vec<BodyId>>,
    active: &mut BTreeSet<u64>,
    depth: usize,
) -> Vec<BodyId> {
    if let Some(bodies) = cache.get(&representation) {
        return bodies.clone();
    }
    if depth >= 256 {
        return Vec::new();
    }
    if !active.insert(representation) {
        return Vec::new();
    }
    let bodies = exchange
        .records
        .get(&representation)
        .and_then(|record| record.parameter(1))
        .and_then(ValueExt::list)
        .into_iter()
        .flatten()
        .filter_map(ValueExt::reference)
        .flat_map(|item| {
            let Some(record) = exchange.records.get(&item) else {
                return Vec::new();
            };
            if matches!(
                record.simple_name(),
                Some("SHELL_BASED_SURFACE_MODEL" | "MANIFOLD_SOLID_BREP" | "BREP_WITH_VOIDS")
            ) {
                return vec![BodyId(format!("step:data:body#{item}"))];
            }
            if record.simple_name() == Some("MAPPED_ITEM") {
                let mapped_representation = record
                    .parameter(1)
                    .and_then(ValueExt::reference)
                    .and_then(|map| exchange.records.get(&map))
                    .and_then(|map| map.parameter(1))
                    .and_then(ValueExt::reference);
                if let Some(mapped_representation) = mapped_representation {
                    return representation_bodies(
                        mapped_representation,
                        exchange,
                        cache,
                        active,
                        depth + 1,
                    );
                }
            }
            Vec::new()
        })
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect::<Vec<_>>();
    active.remove(&representation);
    cache.insert(representation, bodies.clone());
    bodies
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
            if record.simple_name() != Some("PRODUCT_DEFINITION_SHAPE") {
                return None;
            }
            Some((id, record.parameter(2)?.reference()?))
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
    let definition_representations = exchange
        .entities("SHAPE_DEFINITION_REPRESENTATION")
        .filter_map(|(_, record)| {
            let shape = record.parameter(0)?.reference()?;
            let definition = *pds.get(&shape)?;
            Some((definition, record.parameter(1)?.reference()?))
        })
        .collect::<BTreeMap<_, _>>();
    let representation_maps = exchange
        .entities("REPRESENTATION_MAP")
        .filter_map(|(id, record)| {
            (record.simple_name() == Some("REPRESENTATION_MAP")).then_some((
                id,
                (
                    record.parameter(0)?.reference()?,
                    record.parameter(1)?.reference()?,
                ),
            ))
        })
        .collect::<BTreeMap<_, _>>();
    let mut placements_by_representation = BTreeMap::<u64, Vec<Transform>>::new();
    for (_, record) in exchange.entities("MAPPED_ITEM") {
        let Some((origin, mapped_representation)) = record
            .parameter(1)
            .and_then(ValueExt::reference)
            .and_then(|map| representation_maps.get(&map).copied())
        else {
            continue;
        };
        let Some(transform) =
            record
                .parameter(2)
                .and_then(ValueExt::reference)
                .and_then(|target| {
                    Some(between(
                        *geometry.placements.get(&origin)?,
                        *geometry.placements.get(&target)?,
                    ))
                })
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
        let Some(&child_representation) = definition_representations.get(&usage.child_definition)
        else {
            continue;
        };
        let placements = placements_by_representation
            .get(&child_representation)
            .map(Vec::as_slice)
            .unwrap_or_default();
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

fn occurrence_placement(
    record: &RawRecord,
    exchange: &Exchange,
    geometry: &GeometryResult,
    pds: &BTreeMap<u64, u64>,
) -> Option<(u64, Transform)> {
    let relation = exchange.records.get(&record.parameter(0)?.reference()?)?;
    let usage = *pds.get(&record.parameter(1)?.reference()?)?;
    let transform_id = relation
        .partial("REPRESENTATION_RELATIONSHIP_WITH_TRANSFORMATION")?
        .parameters
        .first()?
        .reference()?;
    let transform = exchange.records.get(&transform_id)?;
    let from = geometry
        .placements
        .get(&transform.parameter(2)?.reference()?)?;
    let to = geometry
        .placements
        .get(&transform.parameter(3)?.reference()?)?;
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
fn basis(z: Vector3, x: Vector3) -> [[f64; 3]; 3] {
    let y = Vector3::new(
        z.y * x.z - z.z * x.y,
        z.z * x.x - z.x * x.z,
        z.x * x.y - z.y * x.x,
    );
    [[x.x, y.x, z.x], [x.y, y.y, z.y], [x.z, y.z, z.z]]
}
fn product_ir_id(id: u64) -> ProductId {
    ProductId(format!("step:product:product#{id}"))
}

trait RecordExt {
    fn simple_name(&self) -> Option<&str>;
    fn partial(&self, name: &str) -> Option<&crate::parse::PartialRecord>;
    fn parameter(&self, index: usize) -> Option<&Value>;
}
impl RecordExt for RawRecord {
    fn simple_name(&self) -> Option<&str> {
        (self.partials.len() == 1).then(|| self.partials[0].name.as_str())
    }
    fn partial(&self, name: &str) -> Option<&crate::parse::PartialRecord> {
        self.partials.iter().find(|partial| partial.name == name)
    }
    fn parameter(&self, index: usize) -> Option<&Value> {
        self.partials.first()?.parameters.get(index)
    }
}
trait ValueExt {
    fn reference(&self) -> Option<u64>;
    fn list(&self) -> Option<&[Value]>;
    fn text(&self) -> Option<String>;
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
    fn text(&self) -> Option<String> {
        if let Value::String(bytes) = self {
            crate::strings::decode(bytes).ok()
        } else {
            None
        }
    }
}
