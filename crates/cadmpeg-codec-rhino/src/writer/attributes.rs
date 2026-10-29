// SPDX-License-Identifier: Apache-2.0
//! Attribute reductions required by native object strings.

use cadmpeg_ir::report::loss::LossNote;
use cadmpeg_ir::CadIr;

use crate::loss::RhinoLossCode;

pub(super) fn name_losses(ir: &CadIr) -> Vec<LossNote> {
    let mut losses = Vec::new();
    let mut inspect = |id: &str, name: Option<&str>| {
        if name.is_some_and(|name| name.contains('\0')) {
            losses.push(RhinoLossCode::WriterAttributesOmitted.note(format!(
                "object {id} name contains a null character; its name attribute is omitted",
            )));
        }
    };
    for body in &ir.model.bodies {
        inspect(body.id.as_str(), body.name.as_deref());
    }
    for point in &ir.model.points {
        inspect(
            point.id.as_str(),
            super::presentation::object_name(
                ir,
                &cadmpeg_ir::appearance::AppearanceTarget::Point(point.id.clone()),
            ),
        );
    }
    for curve in &ir.model.curves {
        inspect(
            curve.id.as_str(),
            super::presentation::object_name(
                ir,
                &cadmpeg_ir::appearance::AppearanceTarget::Curve(curve.id.clone()),
            ),
        );
    }
    for surface in &ir.model.surfaces {
        inspect(
            surface.id.as_str(),
            super::presentation::object_name(
                ir,
                &cadmpeg_ir::appearance::AppearanceTarget::Surface(surface.id.clone()),
            ),
        );
    }
    for mesh in &ir.model.tessellations {
        inspect(
            mesh.id.as_str(),
            super::presentation::object_name(
                ir,
                &cadmpeg_ir::appearance::AppearanceTarget::Tessellation(
                    mesh.id.as_str().to_owned(),
                ),
            ),
        );
    }
    losses
}
