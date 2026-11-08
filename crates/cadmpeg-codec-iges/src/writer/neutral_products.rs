// SPDX-License-Identifier: Apache-2.0
//! Standard subfigure definitions and instances authored from CADIR.

use super::{reference_marker, CadIr, Entity, EntityStatus, Placement};
use cadmpeg_ir::products::{OccurrenceParent, PrototypeReference};
use cadmpeg_ir::scalar::FiniteReal;
use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::fmt::Write as _;

/// A product projected from a Type 186 solid is that solid: its label is the
/// solid's Directory label.
fn label_manifold_solid(
    product: &cadmpeg_ir::products::ProductDefinition,
    entities: &mut [Entity],
) {
    let [body] = product.bodies.as_slice() else {
        return;
    };
    let Some(entity) = entities.iter_mut().find(|entity| {
        entity.type_code == 186
            && entity
                .source_targets
                .iter()
                .any(|target| target == body.as_str())
    }) else {
        return;
    };
    if let Some(label) = product.label.as_deref().filter(|label| {
        label.len() <= 8
            && label
                .bytes()
                .all(|byte| byte.is_ascii_graphic() || byte == b' ')
    }) {
        label.clone_into(&mut entity.label);
    }
    entity.source_targets.push(product.id.to_string());
}

pub(super) fn append(ir: &CadIr, entities: &mut Vec<Entity>, version: crate::IgesVersion) {
    let native = ir.native.namespace("iges");
    let native_definitions = native
        .into_iter()
        .flat_map(|ns| {
            [
                "subfigure_definitions",
                "network_definitions",
                "solid_assemblies",
            ]
            .into_iter()
            .flat_map(|arena| ns.arenas().get(arena).into_iter().flatten())
        })
        .map(cadmpeg_ir::NativeRecord::id)
        .collect::<std::collections::BTreeSet<_>>();
    let manifold_solids = native
        .into_iter()
        .flat_map(|ns| ns.arenas().get("manifold_solids").into_iter().flatten())
        .map(cadmpeg_ir::NativeRecord::id)
        .collect::<std::collections::BTreeSet<_>>();
    let mut definitions = BTreeMap::new();
    let mut names = BTreeMap::new();
    for product in &ir.model.product_definitions {
        if product
            .native_ref
            .as_deref()
            .is_some_and(|id| native_definitions.contains(id))
        {
            continue;
        }
        if product
            .native_ref
            .as_deref()
            .is_some_and(|id| manifold_solids.contains(id))
        {
            label_manifold_solid(product, entities);
            continue;
        }
        let Some(name) = product.label.as_deref().filter(|name| {
            !name.trim().is_empty()
                && name
                    .bytes()
                    .all(|byte| byte.is_ascii_graphic() || byte == b' ')
        }) else {
            continue;
        };
        let entity = Entity {
            structure: None,
            property_links: Vec::new(),
            association_links: Vec::new(),
            line_font: None,
            source_targets: vec![product.id.to_string()],
            type_code: 308,
            form: 0,
            label: String::new(),
            status: EntityStatus::Definition,
            subscript: 0,
            parameter_body: format!("0,{}H{name},0;", name.len()).into_bytes(),
            transform: None,
            transform_link: None,
        };
        if !version.admits(&entity) {
            continue;
        }
        definitions.insert(product.id.clone(), entities.len());
        names.insert(product.id.clone(), name.to_owned());
        entities.push(entity);
    }
    let mut uses = BTreeMap::<String, usize>::new();
    for occurrence in &ir.model.occurrences {
        if let PrototypeReference::Local { definition } = &occurrence.prototype {
            *uses.entry(definition.to_string()).or_default() += 1;
        }
    }
    let occurrences = ir
        .model
        .occurrences
        .iter()
        .map(|occurrence| (occurrence.id.to_string(), occurrence))
        .collect::<BTreeMap<_, _>>();
    let mut ordered = ir.model.occurrences.iter().collect::<Vec<_>>();
    ordered.sort_by_key(|occurrence| occurrence.ordinal);
    let mut pending = VecDeque::new();
    let mut children_of = BTreeMap::<String, Vec<_>>::new();
    for occurrence in ordered {
        match &occurrence.parent {
            OccurrenceParent::Root {} => pending.push_back(occurrence),
            OccurrenceParent::Occurrence { occurrence: parent } => {
                children_of
                    .entry(parent.to_string())
                    .or_default()
                    .push(occurrence);
            }
        }
    }
    let mut members = BTreeMap::<String, Vec<usize>>::new();
    let mut children = BTreeMap::<String, BTreeSet<String>>::new();
    while let Some(occurrence) = pending.pop_front() {
        let PrototypeReference::Local { definition } = &occurrence.prototype else {
            continue;
        };
        let Some(index) = definitions.get(definition).copied() else {
            continue;
        };
        let parent_definition = match &occurrence.parent {
            OccurrenceParent::Root {} => None,
            OccurrenceParent::Occurrence { occurrence: parent } => {
                let Some(parent_record) = occurrences.get(parent.as_str()) else {
                    continue;
                };
                let PrototypeReference::Local {
                    definition: parent_definition,
                } = &parent_record.prototype
                else {
                    continue;
                };
                if !definitions.contains_key(parent_definition)
                    || uses.get(parent_definition.as_str()) != Some(&1)
                {
                    continue;
                }
                let mut reachable = BTreeSet::new();
                let mut frontier = vec![definition.to_string()];
                while let Some(current) = frontier.pop() {
                    if reachable.insert(current.clone()) {
                        frontier.extend(children.get(&current).into_iter().flatten().cloned());
                    }
                }
                if reachable.contains(parent_definition.as_str()) {
                    continue;
                }
                Some(parent_definition.to_string())
            }
        };
        let transform = match occurrence.transform {
            Some(transform) => {
                let mut rows = [[FiniteReal::ONE; 4]; 3];
                let mut valid = true;
                for (row, values) in rows.iter_mut().zip(transform.affine_rows()) {
                    for (slot, value) in row.iter_mut().zip(values) {
                        if let Some(value) = FiniteReal::new(value) {
                            *slot = value;
                        } else {
                            valid = false;
                        }
                    }
                }
                if !valid {
                    continue;
                }
                Some(Placement { rows })
            }
            None => None,
        };
        let entity = Entity {
            structure: None,
            property_links: Vec::new(),
            association_links: Vec::new(),
            line_font: None,
            source_targets: vec![occurrence.id.to_string()],
            type_code: 408,
            form: 0,
            label: String::new(),
            status: if parent_definition.is_some() {
                EntityStatus::PhysicallyDependent
            } else {
                EntityStatus::Independent
            },
            subscript: 0,
            parameter_body: format!("{},0,0,0,1;", reference_marker(index)).into_bytes(),
            transform,
            transform_link: None,
        };
        if !version.admits(&entity) {
            continue;
        }
        if let Some(parent) = parent_definition {
            members
                .entry(parent.clone())
                .or_default()
                .push(entities.len());
            children
                .entry(parent)
                .or_default()
                .insert(definition.to_string());
        }
        entities.push(entity);
        pending.extend(
            children_of
                .remove(occurrence.id.as_str())
                .unwrap_or_default(),
        );
    }
    let mut depths = BTreeMap::<String, usize>::new();
    let mut remaining = BTreeMap::<String, usize>::new();
    let mut parents = BTreeMap::<String, Vec<String>>::new();
    for product in definitions.keys() {
        remaining.insert(
            product.to_string(),
            children.get(product.as_str()).map_or(0, BTreeSet::len),
        );
    }
    for (parent, descendants) in &children {
        for child in descendants {
            parents
                .entry(child.clone())
                .or_default()
                .push(parent.clone());
        }
    }
    let mut ready = remaining
        .iter()
        .filter_map(|(id, count)| (*count == 0).then_some(id.clone()))
        .collect::<VecDeque<_>>();
    while let Some(child) = ready.pop_front() {
        let child_depth = depths.get(&child).copied().unwrap_or(0);
        for parent in parents.get(&child).into_iter().flatten() {
            let depth = depths.entry(parent.clone()).or_default();
            *depth = (*depth).max(child_depth + 1);
            if let Some(count) = remaining.get_mut(parent) {
                *count -= 1;
                if *count == 0 {
                    ready.push_back(parent.clone());
                }
            }
        }
    }
    for (product, index) in definitions {
        let Some(name) = names.get(&product) else {
            continue;
        };
        let list = members.get(product.as_str()).map_or(&[][..], Vec::as_slice);
        let mut references = String::new();
        for index in list {
            write!(references, ",{}", reference_marker(*index))
                .expect("writing to a string succeeds");
        }
        let depth = depths.get(product.as_str()).copied().unwrap_or(0);
        entities[index].parameter_body =
            format!("{depth},{}H{name},{}{references};", name.len(), list.len()).into_bytes();
    }
}
