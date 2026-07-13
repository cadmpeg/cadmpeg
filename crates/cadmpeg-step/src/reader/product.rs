// SPDX-License-Identifier: Apache-2.0
//! STEP product prototypes, occurrence identity, and relative placement.

use std::collections::{BTreeMap, BTreeSet};

use cadmpeg_ir::document::CadIr;
use cadmpeg_ir::ids::{BodyId, OccurrenceId, ProductId};
use cadmpeg_ir::math::{Point3, Vector3};
use cadmpeg_ir::product::{OccurrenceParent, Product, ProductOccurrence};
use cadmpeg_ir::transform::Transform;

use crate::parse::{Exchange, RawRecord, Value};

use super::geometry::GeometryResult;

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
        .records
        .iter()
        .filter_map(|(&id, record)| {
            if record.simple_name() != Some("PRODUCT_DEFINITION_FORMATION") {
                return None;
            }
            Some((id, record.parameter(2)?.reference()?))
        })
        .collect::<BTreeMap<_, _>>();
    let definitions = exchange
        .records
        .iter()
        .filter_map(|(&id, record)| {
            if record.simple_name() != Some("PRODUCT_DEFINITION") {
                return None;
            }
            Some((id, *formations.get(&record.parameter(2)?.reference()?)?))
        })
        .collect::<BTreeMap<_, _>>();
    let shape_bindings = shape_bindings(exchange, &definitions);

    for (&step_id, record) in &exchange.records {
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
        .records
        .iter()
        .filter_map(|(&id, record)| {
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
    let mut definition_occurrences = BTreeMap::new();
    for (&definition, &product) in &definitions {
        if child_definitions.contains(&definition) {
            continue;
        }
        let id = OccurrenceId(format!("step:product:occurrence#definition-{definition}"));
        ir.model.occurrences.push(ProductOccurrence {
            id: id.clone(),
            product: product_ir_id(product),
            parent: OccurrenceParent::Root,
            transform: Transform::identity(),
            name: None,
        });
        definition_occurrences.insert(definition, id);
    }
    let placements = occurrence_placements(exchange, geometry, &usages);
    let mut pending = usages.keys().copied().collect::<BTreeSet<_>>();
    while !pending.is_empty() {
        let ready = pending.iter().copied().find(|usage_id| {
            definition_occurrences.contains_key(&usages[usage_id].parent_definition)
        });
        let Some(usage_id) = ready else {
            warnings.push("assembly occurrence graph has no resolvable root".into());
            break;
        };
        pending.remove(&usage_id);
        let usage = &usages[&usage_id];
        let Some(&product) = definitions.get(&usage.child_definition) else {
            warnings.push(format!(
                "NAUO #{usage_id} references an unresolved child definition"
            ));
            continue;
        };
        let id = OccurrenceId(format!("step:product:occurrence#{usage_id}"));
        ir.model.occurrences.push(ProductOccurrence {
            id: id.clone(),
            product: product_ir_id(product),
            parent: OccurrenceParent::Occurrence {
                occurrence: definition_occurrences[&usage.parent_definition].clone(),
            },
            transform: placements
                .get(&usage_id)
                .copied()
                .unwrap_or_else(Transform::identity),
            name: usage.name.clone(),
        });
        definition_occurrences.insert(usage.child_definition, id);
        typed.insert(usage_id);
    }
    for (&id, record) in &exchange.records {
        if matches!(
            record.simple_name(),
            Some("APPLICATION_CONTEXT")
                | Some("PRODUCT_CONTEXT")
                | Some("PRODUCT_DEFINITION_CONTEXT")
                | Some("PRODUCT_DEFINITION_SHAPE")
                | Some("SHAPE_DEFINITION_REPRESENTATION")
                | Some("ITEM_DEFINED_TRANSFORMATION")
                | Some("CONTEXT_DEPENDENT_SHAPE_REPRESENTATION")
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
        .records
        .iter()
        .filter_map(|(&id, record)| {
            if record.simple_name() != Some("PRODUCT_DEFINITION_SHAPE") {
                return None;
            }
            Some((id, record.parameter(2)?.reference()?))
        })
        .collect::<BTreeMap<_, _>>();
    let mut result = BTreeMap::<u64, Vec<BodyId>>::new();
    for record in exchange
        .records
        .values()
        .filter(|record| record.simple_name() == Some("SHAPE_DEFINITION_REPRESENTATION"))
    {
        if let Some((product, bodies)) = shape_binding(record, exchange, &pds, definitions) {
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
) -> Option<(u64, Vec<BodyId>)> {
    let definition = *pds.get(&record.parameter(0)?.reference()?)?;
    let product = *definitions.get(&definition)?;
    let representation = exchange.records.get(&record.parameter(1)?.reference()?)?;
    let bodies = representation
        .parameter(1)?
        .list()?
        .iter()
        .filter_map(ValueExt::reference)
        .filter(|item| {
            exchange.records.get(item).is_some_and(|record| {
                matches!(
                    record.simple_name(),
                    Some("SHELL_BASED_SURFACE_MODEL")
                        | Some("MANIFOLD_SOLID_BREP")
                        | Some("BREP_WITH_VOIDS")
                )
            })
        })
        .map(|item| BodyId(format!("step:data:body#{item}")))
        .collect();
    Some((product, bodies))
}

fn occurrence_placements(
    exchange: &Exchange,
    geometry: &GeometryResult,
    usages: &BTreeMap<u64, Usage>,
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
    for record in exchange
        .records
        .values()
        .filter(|record| record.simple_name() == Some("CONTEXT_DEPENDENT_SHAPE_REPRESENTATION"))
    {
        if let Some((usage, transform)) = occurrence_placement(record, exchange, geometry, &pds) {
            if usages.contains_key(&usage) {
                result.insert(usage, transform);
            }
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
