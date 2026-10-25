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
    let properties = namespace.arena_as::<crate::native::PropertyRecord>("properties")?;
    for record in namespace.arena_as::<AttachmentRecord>("attachments")? {
        let Some(name) = names.get(&record.object) else {
            losses.push(
                FreecadLossCode::ExportNativePropertyNotTransferred
                    .note(format!("attachment {} has an omitted owner", record.id)),
            );
            continue;
        };
        // Missing supports and an unavailable effective frame have explicit
        // unknown spellings. Optional placements without those claims may be
        // genuinely absent and do not by themselves imply lost content.
        let mut unavailable = Vec::new();
        if record.supports.is_none() {
            unavailable.push("supports");
        }
        if record.effective_frame().is_none() {
            unavailable.push("effective_frame");
        }
        if record.map_mode.is_none()
            && properties.iter().any(|property| {
                property.owner == record.object
                    && property.name == "MapMode"
                    && property.body.is_none()
            })
        {
            unavailable.push("map_mode");
        }
        if !unavailable.is_empty() {
            losses.push(
                cadmpeg_ir::codec::write::ExportLossCode::SourceUndecodedOmitted.note(format!(
                    "attachment {} omits unavailable fields {}; its held component properties are written independently",
                    record.id,
                    unavailable.join(", ")
                )),
            );
        }
        let Some(object) = objects.iter_mut().find(|object| object.name == *name) else {
            losses.push(
                FreecadLossCode::ExportNativePropertyNotTransferred
                    .note(format!("attachment {} has an omitted owner", record.id)),
            );
            continue;
        };
        let mut links = Vec::new();
        let mut complete = true;
        for support in record.supports.as_deref().unwrap_or_default() {
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
        let neutral_placement = record.effective_frame().is_some()
            && (object.properties.contains_key("Placement")
                || body_names.values().any(|owner| owner == name));
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
            if unavailable.is_empty() {
                cadmpeg_ir::codec::write::NativeDisposition::Written
            } else {
                cadmpeg_ir::codec::write::NativeDisposition::Dropped
            },
        );
    }
    Ok(())
}

#[cfg(test)]
mod tests;
