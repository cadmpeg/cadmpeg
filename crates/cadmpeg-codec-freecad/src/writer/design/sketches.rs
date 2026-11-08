// SPDX-License-Identifier: Apache-2.0
//! Neutral planar sketch geometry and constraint output.

use super::super::xml::{self, Object};
use crate::loss::FreecadLossCode;
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
) {
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
        let transform = sketch
            .placement
            .resolved()
            .and_then(|(origin, normal, x_axis)| {
                let z = normal.unit_nonzero()?;
                let x = x_axis.unit_nonzero()?;
                let y = cadmpeg_ir::math::Vector3::new(
                    z.y * x.z - z.z * x.y,
                    z.z * x.x - z.x * x.z,
                    z.x * x.y - z.y * x.x,
                );
                let origin = origin.get();
                Transform::affine([
                    [x.x, y.x, z.x, origin.x],
                    [x.y, y.y, z.y, origin.y],
                    [x.z, y.z, z.z, origin.z],
                ])
            });
        if let Some(transform) = transform {
            object.set("Placement", super::placement(transform));
        } else {
            losses.push(FreecadLossCode::ExportFeatureNotTransferred.note(format!(
                "sketch {} has no resolved placement; local geometry is written at the origin",
                sketch.id
            )));
        }
        let mut rows = Vec::new();
        let mut indices = external::write(ir, sketch, object, names, losses);
        let mut entities = ir
            .model
            .sketch_entities
            .iter()
            .filter(|entity| entity.sketch == sketch.id)
            .collect::<Vec<_>>();
        entities.sort_by_key(|entity| source_order(entity.id().as_str()));
        for entity in entities {
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
                // XML geometry references use signed indices.
                let Ok(ordinal) = i64::try_from(rows.len()) else {
                    debug_assert!(false, "a geometry index fits the signed lane");
                    continue;
                };
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
        let mut sources = ir
            .model
            .sketch_constraints
            .iter()
            .filter(|constraint| constraint.sketch == sketch.id)
            .collect::<Vec<_>>();
        sources.sort_by_key(|constraint| source_order(constraint.id.as_str()));
        for constraint in sources {
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
}

/// Arenas hold records in identity order, so `Sketch:10` precedes
/// `Sketch:2`. Geometry and constraint indices are positional in `FreeCAD`:
/// a trailing decimal index orders numerically, ties by the full identity.
fn source_order(id: &str) -> (&str, Option<u64>, &str) {
    let prefix = id.trim_end_matches(|c: char| c.is_ascii_digit());
    let index = id
        .strip_prefix(prefix)
        .and_then(|digits| digits.parse().ok());
    (prefix, index, id)
}

#[cfg(test)]
mod tests;
