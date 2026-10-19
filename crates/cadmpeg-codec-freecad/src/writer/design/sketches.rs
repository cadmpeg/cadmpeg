// SPDX-License-Identifier: Apache-2.0
//! Neutral planar sketch geometry and constraint output.

use super::super::xml::{self, Object};
use crate::loss::FreecadLossCode;
use cadmpeg_core::CodecError;
use cadmpeg_ir::document::CadIr;
use cadmpeg_ir::report::loss::LossNote;
use cadmpeg_ir::transform::Transform;
use std::collections::HashMap;

mod constraints;
mod external;
mod geometry;
mod native;

pub(crate) fn write(
    ir: &CadIr,
    objects: &mut Vec<Object>,
    names: &mut HashMap<String, String>,
    losses: &mut Vec<LossNote>,
) -> Result<(), CodecError> {
    for sketch in &ir.model.sketches {
        let feature_name = ir.model.features.iter().find(|feature| matches!(feature.evaluation.definition().operation(), cadmpeg_ir::features::FeatureOperation::Sketch { sketch: binding } if binding.id() == Some(&sketch.id))).and_then(|feature| names.get(feature.id.as_str())).cloned();
        let name = feature_name
            .or_else(|| {
                sketch
                    .native_ref
                    .as_ref()
                    .and_then(|native| names.get(native).cloned())
            })
            .unwrap_or_else(|| format!("Sketch{}", objects.len()));
        let index = if let Some(index) = objects.iter().position(|object| object.name == name) {
            index
        } else {
            objects.push(Object::new(
                name.clone(),
                "Sketcher::SketchObject",
                sketch.name.as_deref().unwrap_or(sketch.id.as_str()),
            ));
            objects.len() - 1
        };
        names.insert(sketch.id.as_str().to_owned(), name);
        let object = &mut objects[index];
        object.kind = "Sketcher::SketchObject".into();
        if let Some((origin, normal, x_axis)) = sketch.placement.resolved() {
            let z = normal
                .unit_nonzero()
                .ok_or_else(|| CodecError::malformed("sketch normal is zero"))?;
            let x = x_axis
                .unit_nonzero()
                .ok_or_else(|| CodecError::malformed("sketch axis is zero"))?;
            let y = cadmpeg_ir::math::Vector3::new(
                z.y * x.z - z.z * x.y,
                z.z * x.x - z.x * x.z,
                z.x * x.y - z.y * x.x,
            );
            let origin = origin.get();
            let transform = Transform::affine([
                [x.x, y.x, z.x, origin.x],
                [x.y, y.y, z.y, origin.y],
                [x.z, y.z, z.z, origin.z],
            ])
            .ok_or_else(|| CodecError::malformed("sketch placement is non-finite"))?;
            object.set("Placement", super::placement(transform)?);
        } else {
            losses.push(FreecadLossCode::ExportFeatureNotTransferred.note(format!(
                "sketch {} has no resolved placement; local geometry is written at the origin",
                sketch.id
            )));
        }
        let mut rows = Vec::new();
        let mut indices = external::write(ir, sketch, object, names, losses)?;
        for entity in ir
            .model
            .sketch_entities
            .iter()
            .filter(|entity| entity.sketch == sketch.id)
        {
            if external::is_external(entity) {
                continue;
            }
            let id = entity.id().as_str();
            let reserved = if id.ends_with(":reference-horizontal-axis") {
                Some((-1, 0))
            } else if id.ends_with(":reference-vertical-axis") {
                Some((-2, 0))
            } else if id.ends_with(":reference-root-point") {
                Some((-1, 1))
            } else {
                None
            };
            if let Some(index) = reserved {
                indices.insert(id.to_owned(), index);
                continue;
            }
            let row = geometry::row(entity).or_else(|| native::geometry(ir, entity));
            if let Some(row) = row {
                // (O-capacity) XML geometry references use signed indices.
                let ordinal = i64::try_from(rows.len()).map_err(|_| {
                    CodecError::InvalidInput("FreeCAD sketch index exceeds i64".into())
                })?;
                indices.insert(id.to_owned(), (ordinal, 0));
                rows.push(row);
            } else {
                losses.push(FreecadLossCode::ExportFeatureNotTransferred.note(format!(
                    "sketch entity {} has no synthesized FreeCAD geometry encoding and is omitted",
                    entity.id()
                )));
            }
        }
        object.set(
            "Geometry",
            xml::property(
                "Geometry",
                "Part::PropertyGeometryList",
                &format!(
                    "<GeometryList count=\"{}\">{}</GeometryList>",
                    rows.len(),
                    rows.join("")
                ),
            ),
        );
        let mut constraints = Vec::new();
        for constraint in ir
            .model
            .sketch_constraints
            .iter()
            .filter(|constraint| constraint.sketch == sketch.id)
        {
            if let Some(row) = constraints::row(ir, constraint, &indices)
                .or_else(|| native::constraint(ir, constraint, &indices))
            {
                names.insert(
                    format!("constraint-path:{}", constraint.id),
                    format!("Constraints[{}]", constraints.len()),
                );
                constraints.push(row);
            } else {
                losses.push(FreecadLossCode::ExportFeatureNotTransferred.note(format!("sketch constraint {} has no synthesized FreeCAD relation encoding and is omitted", constraint.id)));
            }
        }
        object.set(
            "Constraints",
            xml::property(
                "Constraints",
                "Sketcher::PropertyConstraintList",
                &format!(
                    "<ConstraintList count=\"{}\">{}</ConstraintList>",
                    constraints.len(),
                    constraints.join("")
                ),
            ),
        );
    }
    Ok(())
}

#[cfg(test)]
mod tests;
