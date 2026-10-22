// SPDX-License-Identifier: Apache-2.0
//! Typed attachment supports and frames accompanying neutral geometry.

use super::super::xml::{self, Object};
use crate::{loss::FreecadLossCode, native::AttachmentRecord};
use cadmpeg_core::CodecError;
use cadmpeg_ir::{document::CadIr, report::loss::LossNote, transform::Transform};
use std::collections::HashMap;

pub(crate) fn write(
    ir: &CadIr,
    objects: &mut [Object],
    names: &HashMap<String, String>,
    body_names: &HashMap<String, String>,
    losses: &mut Vec<LossNote>,
    outcomes: &mut cadmpeg_ir::codec::write::NativeDispositions,
) -> Result<(), CodecError> {
    let Some(namespace) = ir.native.namespace("fcstd") else {
        return Ok(());
    };
    let existing: std::collections::HashSet<_> =
        objects.iter().map(|object| object.name.clone()).collect();
    for record in namespace.arena_as::<AttachmentRecord>("attachments")? {
        let Some(name) = names.get(&record.object) else {
            losses.push(
                FreecadLossCode::ExportNativePropertyNotTransferred
                    .note(format!("attachment {} has an omitted owner", record.id)),
            );
            continue;
        };
        let Some(object) = objects.iter_mut().find(|object| object.name == *name) else {
            losses.push(
                FreecadLossCode::ExportNativePropertyNotTransferred
                    .note(format!("attachment {} has an omitted owner", record.id)),
            );
            continue;
        };
        let mut links = Vec::new();
        let mut complete = true;
        for support in &record.supports {
            let Some(support) = support else {
                links.push(xml::element(
                    "Link",
                    &[("obj", String::new()), ("sub", String::new())],
                ));
                continue;
            };
            let target = support
                .object()
                .and_then(|id| names.get(id))
                .filter(|name| existing.contains(*name));
            let Some(target) = target.filter(|_| support.document().is_none()) else {
                complete = false;
                break;
            };
            if support.subelements().is_empty() {
                links.push(xml::element(
                    "Link",
                    &[("obj", target.clone()), ("sub", String::new())],
                ));
            } else {
                for sub in support.subelements() {
                    links.push(xml::element(
                        "Link",
                        &[("obj", target.clone()), ("sub", sub.clone())],
                    ));
                }
            }
        }
        if !complete {
            losses.push(
                FreecadLossCode::ExportNativePropertyNotTransferred
                    .note(format!("attachment {} has an omitted support", record.id)),
            );
            continue;
        }
        if !links.is_empty() {
            object.set(
                "Support",
                xml::property(
                    "Support",
                    "App::PropertyLinkSubList",
                    &format!(
                        "<LinkSubList count=\"{}\">{}</LinkSubList>",
                        links.len(),
                        links.join("")
                    ),
                ),
            );
        }
        if let Some(mode) = record.map_mode {
            object.set(
                "MapMode",
                xml::scalar("MapMode", "App::PropertyEnumeration", "Integer", mode),
            );
        }
        // Neutral geometry/placement already incorporates the effective frame.
        // An offset would apply that transform a second time. Rebuild a neutral
        // placement with identity local offset instead.
        let neutral_placement = object.properties.contains_key("Placement")
            || body_names.values().any(|owner| owner == name);
        if neutral_placement {
            if record.offset().is_some() {
                object.set(
                    "AttachmentOffset",
                    super::super::design::placement(Transform::identity())?
                        .replace("name=\"Placement\"", "name=\"AttachmentOffset\""),
                );
            }
        } else {
            if let Some(placement) = record.placement() {
                object.set(
                    "Placement",
                    super::super::design::placement(placement.transform())?,
                );
            }
            if let Some(offset) = record.offset() {
                object.set(
                    "AttachmentOffset",
                    super::super::design::placement(offset.transform())?
                        .replace("name=\"Placement\"", "name=\"AttachmentOffset\""),
                );
            }
        }
        super::super::dispositions::record(
            outcomes,
            "attachments",
            &record.id,
            cadmpeg_ir::codec::write::NativeDisposition::Written,
        );
    }
    Ok(())
}
