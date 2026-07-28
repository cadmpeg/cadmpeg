// SPDX-License-Identifier: Apache-2.0
//! Transfer of complete CATIA sketch declarations to neutral sketch identities.

use std::collections::{HashMap, HashSet};

use cadmpeg_ir::document::CadIr;
use cadmpeg_ir::sketches::{Sketch, SketchId, SketchPlacement};

use crate::native::{CatiaNative, CatiaObjectRecord};
use crate::object_graph::{PayloadField, PayloadSubtype};

/// Transfer identity-complete sketch declarations.
pub(crate) fn transfer_sketches(ir: &mut CadIr, native: &CatiaNative) -> HashSet<String> {
    let records = native
        .object_graphs
        .iter()
        .flat_map(|graph| &graph.records)
        .map(|record| (record.id.as_str(), record))
        .collect::<HashMap<_, _>>();
    let mut consumed_object_records = HashSet::new();

    for object in &native.design_objects {
        if object.owner_record.is_none() {
            continue;
        }
        let declarations = object
            .fields
            .iter()
            .filter_map(|field| records.get(field.as_str()).copied())
            .filter(|record| matches!(record.class_name.as_deref(), Some("PRTSketch" | "Sketch")))
            .collect::<Vec<_>>();
        let Some((declaration_class, declaration_entry)) =
            declarations.first().and_then(|record| {
                record
                    .class_name
                    .as_deref()
                    .zip(record.class_entry.as_deref())
            })
        else {
            continue;
        };
        if declarations.iter().any(|record| {
            record.class_name.as_deref() != Some(declaration_class)
                || record.class_entry.as_deref() != Some(declaration_entry)
                || !complete_sketch_declaration(record, &object.id, object.owner_entity_id)
        }) {
            continue;
        }

        ir.model.sketches.push(Sketch {
            id: SketchId(format!("{}:sketch", object.id)),
            name: None,
            configuration: None,
            placement: SketchPlacement::Unresolved,
            profiles: Vec::new(),
            native_ref: Some(object.id.clone()),
        });
        consumed_object_records.extend(
            declarations
                .into_iter()
                .map(|declaration| declaration.id.clone()),
        );
    }

    consumed_object_records
}

fn complete_sketch_declaration(
    record: &CatiaObjectRecord,
    design_object: &str,
    owner_entity_id: u32,
) -> bool {
    record.design_object.as_deref() == Some(design_object)
        && record.owner_entity_id == Some(owner_entity_id)
        && record.storage_ref.is_none()
        && record.references.is_empty()
        && record.subtype == PayloadSubtype::Empty
        && record.payload.size == 1
        && record.payload.fields == [PayloadField::Terminator]
}
