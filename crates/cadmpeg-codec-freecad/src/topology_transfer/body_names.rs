// SPDX-License-Identifier: Apache-2.0
//! Independent CADIR body names carried by an application string-list property.
//!
//! A `FreeCAD` object can own several `BRep` roots and has only one display Label.
//! The typed custom property preserves those root names without aliasing a
//! feature's display name or inventing construction nodes.

use crate::{brep::ShapePayloadRecord, native::PropertyRecord};
use cadmpeg_ir::CadIr;

pub(crate) fn apply(
    ir: &mut CadIr,
    payloads: &[ShapePayloadRecord],
    properties: &[PropertyRecord],
) {
    for payload in payloads {
        let Some(owner) = properties
            .iter()
            .find(|property| property.id == payload.property)
            .map(|property| &property.owner)
        else {
            continue;
        };
        let Some(names) = properties.iter().find(|property| {
            property.owner == *owner
                && property.name == "CadmpegBodyNames"
                && property.type_name == "App::PropertyStringList"
        }) else {
            continue;
        };
        let names: Vec<_> = names
            .values()
            .iter()
            .filter(|value| value.tag == "String")
            .filter_map(|value| value.attributes.get("value"))
            .collect();
        let prefix = crate::native::model_id("body", &payload.id, "");
        for (index, body) in ir
            .model
            .bodies
            .iter_mut()
            .filter(|body| body.id.as_str().starts_with(&prefix))
            .enumerate()
        {
            if let Some(name) = names.get(index) {
                body.name = serde_json::from_str::<Option<String>>(name)
                    .unwrap_or_else(|_| Some((*name).clone()));
            }
        }
    }
}
