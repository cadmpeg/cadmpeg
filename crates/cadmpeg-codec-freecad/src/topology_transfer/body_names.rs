// SPDX-License-Identifier: Apache-2.0
//! Independent CADIR body names carried by an application string-list property.
//!
//! A `FreeCAD` object can own several `BRep` roots and has only one display Label.
//! The typed custom property preserves those root names without aliasing a
//! feature's display name or inventing construction nodes.

use crate::{brep::ShapePayloadRecord, native::PropertyRecord};
use cadmpeg_core::decode::DecodeContext;
use cadmpeg_core::CodecError;
use cadmpeg_ir::CadIr;

pub(crate) fn apply(
    ctx: &DecodeContext<'_>,
    ir: &mut CadIr,
    payloads: &[ShapePayloadRecord],
    properties: &[PropertyRecord],
) -> Result<(), CodecError> {
    if let Some(refusal) = ctx.resource_refusal() {
        return Err(refusal.into());
    }
    if !payloads
        .iter()
        .any(|payload| names_property(payload, properties).is_some())
    {
        return Ok(());
    }
    let Some(namespace) = ir.native.namespace("fcstd") else {
        return Ok(());
    };
    let resolved = super::source_uses::resolve_namespace(ctx, namespace, payloads)?;
    let mut roots = Vec::new();
    ctx.reserve_working(&mut roots, resolved.len(), "FreeCAD body name root index")?;
    roots.extend(resolved.iter().filter_map(|row| {
        row.root
            .map(|root| (row.id.as_str(), row.owner.as_str(), root))
    }));
    roots.sort_unstable_by(|left, right| left.0.cmp(right.0));
    let mut assignments = Vec::new();
    for payload in payloads {
        let Some(names) = names_property(payload, properties) else {
            continue;
        };
        let mut values = Vec::new();
        ctx.reserve_working(
            &mut values,
            names.values().len(),
            "FreeCAD body name values",
        )?;
        values.extend(
            names
                .values()
                .iter()
                .filter(|value| value.tag == "String")
                .filter_map(|value| value.attributes.get("value")),
        );
        for (body_index, body) in ir.model.bodies.iter().enumerate() {
            let Some(native) = body.native_ref.as_deref() else {
                continue;
            };
            let Ok(position) = roots.binary_search_by(|entry| entry.0.cmp(native)) else {
                continue;
            };
            let (_, owner, root) = roots[position];
            if owner != payload.id {
                continue;
            }
            if let Some(name) = root.checked_sub(1).and_then(|index| values.get(index)) {
                ctx.reserve_working(&mut assignments, 1, "FreeCAD body name assignments")?;
                assignments.push((body_index, *name));
            }
        }
    }
    for (body, name) in assignments {
        ir.model.bodies[body].name =
            serde_json::from_str::<Option<String>>(name).unwrap_or_else(|_| Some(name.clone()));
    }
    Ok(())
}

fn names_property<'a>(
    payload: &ShapePayloadRecord,
    properties: &'a [PropertyRecord],
) -> Option<&'a PropertyRecord> {
    let owner = &properties
        .iter()
        .find(|property| property.id == payload.property)?
        .owner;
    properties.iter().find(|property| {
        property.owner == *owner
            && property.name == "CadmpegBodyNames"
            && property.type_name == "App::PropertyStringList"
    })
}

#[cfg(test)]
mod tests;
