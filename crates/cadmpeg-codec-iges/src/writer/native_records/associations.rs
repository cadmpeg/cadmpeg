// SPDX-License-Identifier: Apache-2.0
//! Rebuild the required trailing back pointers of typed associativities.

use super::{CadIr, Entity, Targets, Value};

pub(super) fn link(ir: &CadIr, entities: &mut [Entity], targets: &Targets) {
    let Some(records) = ir
        .native
        .namespace("iges")
        .and_then(|namespace| namespace.arenas().get("associativities"))
    else {
        return;
    };
    for record in records {
        let Some([association]) = targets.get(record.id()).map(Vec::as_slice) else {
            continue;
        };
        let Ok(fields) = record.fields() else {
            continue;
        };
        let kind = fields.get("kind").and_then(Value::as_str);
        let mut members = Vec::new();
        match kind {
            Some("single_parent") => {
                members.extend(fields.get("parent").and_then(Value::as_str));
                members.extend(
                    fields
                        .get("children")
                        .and_then(Value::as_array)
                        .into_iter()
                        .flatten()
                        .filter_map(Value::as_str),
                );
            }
            Some("view_list") => {
                members.extend(fields.get("view").and_then(Value::as_str));
                members.extend(
                    fields
                        .get("visible_entities")
                        .and_then(Value::as_array)
                        .into_iter()
                        .flatten()
                        .filter_map(Value::as_str),
                );
            }
            Some("dimensioned_geometry") => {
                members.extend(fields.get("dimension").and_then(Value::as_str));
            }
            _ => continue,
        }
        for member in members {
            let Some([index]) = targets.get(member).map(Vec::as_slice) else {
                continue;
            };
            if !entities[*index].association_links.contains(association) {
                entities[*index].association_links.push(*association);
            }
        }
    }
}
