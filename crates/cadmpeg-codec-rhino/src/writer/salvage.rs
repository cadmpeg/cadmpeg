// SPDX-License-Identifier: Apache-2.0
//! Standalone geometry retained from bodies whose topology cannot be written.

use std::collections::BTreeSet;

use cadmpeg_core::decode::DecodeContext;
use cadmpeg_ir::codec::write::EncodeBuffer;
use cadmpeg_ir::report::loss::LossNote;
use cadmpeg_ir::CadIr;

use super::{
    attributed_object_record, curve_object_record, presentation, MaterialPlan, WritableFaceSurface,
    WriteError, POINT_CLASS,
};

pub(super) fn write(
    ctx: &DecodeContext<'_>,
    ir: &CadIr,
    body: &CadIr,
    output: &mut EncodeBuffer<'_>,
    materials: &MaterialPlan,
    carried: &mut BTreeSet<String>,
    losses: &mut Vec<LossNote>,
    written_curves: &mut BTreeSet<String>,
) -> Result<usize, WriteError> {
    let mut count = 0;
    for point in &body.model.points {
        if carried.contains(point.id.as_str()) {
            continue;
        }
        let position = point.position().get();
        let mut payload = vec![0x10];
        payload.extend(position.x.to_le_bytes());
        payload.extend(position.y.to_le_bytes());
        payload.extend(position.z.to_le_bytes());
        let display = presentation::display(
            ir,
            &cadmpeg_ir::appearance::AppearanceTarget::Point(point.id.clone()),
            point.source_object.as_ref(),
        );
        ctx.charge_entities(1, "write Rhino object record")?;
        output.append(
            &attributed_object_record(
                ir,
                1,
                POINT_CLASS,
                &payload,
                point.id.as_str(),
                display,
                materials,
            )?,
            "retain Rhino archive bytes",
        )?;
        carried.insert(point.id.to_string());
        count += 1;
    }
    for surface in &body.model.surfaces {
        if carried.contains(surface.id.as_str()) {
            continue;
        }
        let (payload, notes) = match WritableFaceSurface::try_new(surface, body)
            .and_then(|(surface, notes)| Ok((surface.payload()?, notes)))
        {
            Ok(value) => value,
            Err(WriteError::Omit(_)) => continue,
            Err(error) => return Err(error),
        };
        let display = presentation::display(
            ir,
            &cadmpeg_ir::appearance::AppearanceTarget::Surface(surface.id.clone()),
            surface.source_object.as_ref(),
        );
        ctx.charge_entities(
            cadmpeg_core::decode::u64_from_index(payload.record_count),
            "write Rhino surface records",
        )?;
        output.append(
            &super::surface_object_record(ir, &payload, surface.id.as_str(), display, materials)?,
            "retain Rhino archive bytes",
        )?;
        losses.extend(notes);
        if let Some(construction) = surface.geometry.procedural_construction() {
            losses.push(
                crate::loss::RhinoLossCode::WriterProceduralReduced.note(format!(
                    "surface {} procedural construction {} was reduced to a held solved carrier",
                    surface.id, construction
                )),
            );
        }
        carried.insert(surface.id.to_string());
        count += 1;
    }
    let children = super::compounds::children(body);
    let mut pending = body
        .model
        .curves
        .iter()
        .filter(|curve| !children.contains(&curve.id))
        .collect::<std::collections::VecDeque<_>>();
    let mut attempted = BTreeSet::new();
    while let Some(curve) = pending.pop_front() {
        if carried.contains(curve.id.as_str()) {
            continue;
        }
        if !attempted.insert(curve.id.clone()) {
            continue;
        }
        let (payload, notes) = match super::compounds::payload(body, curve, &mut BTreeSet::new()) {
            Ok(value) => value,
            Err(WriteError::Omit(_)) => {
                pending.extend(super::compounds::components(body, curve));
                continue;
            }
            Err(error) => return Err(error),
        };
        let display = presentation::display(
            ir,
            &cadmpeg_ir::appearance::AppearanceTarget::Curve(curve.id.clone()),
            curve.source_object.as_ref(),
        );
        ctx.charge_entities(payload.record_count, "write Rhino curve records")?;
        output.append(
            &curve_object_record(ir, &payload, curve.id.as_str(), display, materials)?,
            "retain Rhino archive bytes",
        )?;
        losses.extend(notes);
        if super::is_support(curve.source_object.as_ref()) {
            losses.push(
                crate::loss::RhinoLossCode::WriterCarrierResolved.note(format!(
                    "support curve {} is written as an independent curve object",
                    curve.id
                )),
            );
        }
        for id in &payload.represented {
            carried.insert(id.to_string());
            written_curves.insert(id.to_string());
        }
        pending.extend(super::compounds::unrepresented_children(
            body,
            curve,
            &payload.represented,
        ));
        count += 1;
    }
    Ok(count)
}
