// SPDX-License-Identifier: Apache-2.0

use super::{CadIr, Entity, EntityStatus};
use std::collections::BTreeSet;

pub(super) fn append(ir: &CadIr, entities: &mut Vec<Entity>) -> BTreeSet<String> {
    let mut written = BTreeSet::new();
    let Some(records) = ir
        .native
        .namespace("iges")
        .and_then(|ns| ns.arenas().get("entities"))
    else {
        return written;
    };
    for record in records {
        let Some(fields) = record.fields().ok() else {
            continue;
        };
        if fields.get("entity_type").and_then(|value| value.as_i64()) != Some(0)
            || [
                "form",
                "blank_status",
                "subordinate_status",
                "use_flag",
                "hierarchy_status",
                "structure",
                "transform",
                "view",
                "color",
                "line_weight",
                "line_font",
                "level",
                "subscript",
            ]
            .iter()
            .any(|key| fields.get(*key).and_then(|value| value.as_i64()) != Some(0))
            || fields
                .get("parameters")
                .and_then(|value| value.as_array())
                .is_none_or(|values| {
                    values.len() != 1
                        || values[0]["value"]["kind"] != "integer"
                        || values[0]["value"]["value"] != 0
                })
            || ["association_links", "property_links"].iter().any(|key| {
                fields
                    .get(*key)
                    .and_then(|value| value.as_array())
                    .is_none_or(|links| !links.is_empty())
            })
        {
            continue;
        }
        let Some(label) = fields.get("label").cloned().and_then(|value| {
            serde_json::from_value::<cadmpeg_ir::native::bytes::NativeBytes>(value).ok()
        }) else {
            continue;
        };
        let Ok(label) = std::str::from_utf8(label.as_ref()) else {
            continue;
        };
        let label = label.trim();
        if label.len() > 8
            || !label
                .bytes()
                .all(|byte| byte.is_ascii_graphic() || byte == b' ')
        {
            continue;
        }
        entities.push(Entity {
            structure: None,
            property_links: Vec::new(),
            association_links: Vec::new(),
            line_font: None,
            source_targets: vec![record.id().to_owned()],
            type_code: 0,
            form: 0,
            label: label.to_owned(),
            status: EntityStatus::Independent,
            parameter_body: b";".to_vec(),
            transform: None,
            transform_link: None,
            subscript: 0,
        });
        written.insert(record.id().to_owned());
    }
    written
}
