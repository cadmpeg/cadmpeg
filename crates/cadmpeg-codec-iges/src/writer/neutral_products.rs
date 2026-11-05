// SPDX-License-Identifier: Apache-2.0
//! Standard subfigure definitions and root instances authored from CADIR.

use super::{reference_marker, CadIr, Entity, EntityStatus, Placement};
use cadmpeg_ir::products::{OccurrenceParent, PrototypeReference};
use cadmpeg_ir::scalar::FiniteReal;
use std::collections::BTreeMap;

pub(super) fn append(ir: &CadIr, entities: &mut Vec<Entity>, version: crate::IgesVersion) {
    let native = ir.native.namespace("iges");
    let native_definitions = native
        .into_iter()
        .flat_map(|ns| {
            ["subfigure_definitions", "network_definitions"]
                .into_iter()
                .flat_map(|arena| ns.arenas().get(arena).into_iter().flatten())
        })
        .map(|record| record.id())
        .collect::<std::collections::BTreeSet<_>>();
    let mut definitions = BTreeMap::new();
    for product in &ir.model.product_definitions {
        if product
            .native_ref
            .as_deref()
            .is_some_and(|id| native_definitions.contains(id))
        {
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
        entities.push(entity);
    }
    let mut occurrences = ir.model.occurrences.iter().collect::<Vec<_>>();
    occurrences.sort_by_key(|occurrence| occurrence.ordinal);
    'occurrences: for occurrence in occurrences {
        if !matches!(occurrence.parent, OccurrenceParent::Root {}) {
            continue;
        }
        let PrototypeReference::Local { definition } = &occurrence.prototype else {
            continue;
        };
        let Some(index) = definitions.get(definition).copied() else {
            continue;
        };
        let transform = if let Some(transform) = occurrence.transform {
            let mut rows = [[FiniteReal::ONE; 4]; 3];
            for (row, values) in rows.iter_mut().zip(transform.affine_rows()) {
                for (slot, value) in row.iter_mut().zip(values) {
                    let Some(value) = FiniteReal::new(value) else {
                        continue 'occurrences;
                    };
                    *slot = value;
                }
            }
            Some(Placement { rows })
        } else {
            None
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
            status: EntityStatus::Independent,
            subscript: 0,
            parameter_body: format!("{},0,0,0,1;", reference_marker(index)).into_bytes(),
            transform,
            transform_link: None,
        };
        if version.admits(&entity) {
            entities.push(entity);
        }
    }
}
