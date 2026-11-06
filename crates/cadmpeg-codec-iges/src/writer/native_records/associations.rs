// SPDX-License-Identifier: Apache-2.0
//! Rebuild the required trailing back pointers of typed associativities.

use super::{CadIr, Entity, Targets, Value};

pub(super) fn link(ir: &CadIr, entities: &mut [Entity], targets: &Targets) {
    let records = ir
        .native
        .namespace("iges")
        .and_then(|namespace| namespace.arenas().get("associativities"))
        .into_iter()
        .flatten();
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
            Some("flow") if fields.get("form").and_then(Value::as_i64) == Some(18) => {
                for name in ["connections", "joins", "continuations"] {
                    members.extend(
                        fields
                            .get(name)
                            .and_then(Value::as_array)
                            .into_iter()
                            .flatten()
                            .filter_map(Value::as_str),
                    );
                }
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
    for record in ir
        .native
        .namespace("iges")
        .and_then(|ns| ns.arenas().get("groups"))
        .into_iter()
        .flatten()
    {
        let Ok(fields) = record.fields() else {
            continue;
        };
        if fields
            .get("back_pointers_required")
            .and_then(Value::as_bool)
            != Some(true)
        {
            continue;
        }
        let Some([group]) = targets.get(record.id()).map(Vec::as_slice) else {
            continue;
        };
        for member in fields
            .get("members")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .filter_map(Value::as_str)
        {
            let Some([index]) = targets.get(member).map(Vec::as_slice) else {
                continue;
            };
            if !entities[*index].association_links.contains(group) {
                entities[*index].association_links.push(*group);
            }
        }
    }
}
