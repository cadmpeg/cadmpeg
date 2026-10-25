// SPDX-License-Identifier: Apache-2.0
//! Document-state projections and edits shared by writer contract tests.

use std::collections::BTreeSet;

use cadmpeg_core::CodecError;
use cadmpeg_ir::codec::write::ArenaDisposition;
use cadmpeg_ir::math::Vector3;
use cadmpeg_ir::report::export::ExportReport;
use cadmpeg_ir::schema::EntityKind;
use cadmpeg_ir::{ArenaClass, ArenaName, CadIr};
use serde_json::Value;

/// Translate neutral model coordinates and occurrence placements.
///
/// Native records and source fidelity remain unchanged, so this edit also
/// exercises the writer's comparison against its retained source baseline.
/// Failed finite-coordinate admission leaves the document unchanged.
pub fn translate_model(ir: &mut CadIr, offset: impl Into<Vector3>) -> Result<(), CodecError> {
    let offset = offset.into();
    if !offset.is_finite() {
        return Err(CodecError::InvalidInput(
            "model translation is non-finite".into(),
        ));
    }
    let mut model = serde_json::to_value(&ir.model)
        .map_err(|error| CodecError::InvalidInput(error.to_string()))?;
    for arena in ["points", "curves", "surfaces", "tessellations"] {
        if let Some(records) = model.get_mut(arena) {
            translate_coordinates(records, offset, false)?;
        }
    }
    if let Some(Value::Array(occurrences)) = model.get_mut("occurrences") {
        for occurrence in occurrences {
            if let Some(transform) = occurrence.get_mut("transform") {
                translate_transform(transform, offset)?;
            }
        }
    }
    ir.model = serde_json::from_value(model)
        .map_err(|error| CodecError::InvalidInput(error.to_string()))?;
    Ok(())
}

fn translate_coordinates(
    value: &mut Value,
    offset: Vector3,
    point: bool,
) -> Result<(), CodecError> {
    match value {
        Value::Object(fields) => {
            if point
                && ["x", "y", "z"]
                    .iter()
                    .all(|key| fields.get(*key).is_some_and(Value::is_number))
            {
                for (key, displacement) in ["x", "y", "z"]
                    .into_iter()
                    .zip([offset.x, offset.y, offset.z])
                {
                    if let Some(coordinate) = fields.get_mut(key) {
                        translate_scalar(coordinate, displacement)?;
                    }
                }
                return Ok(());
            }
            // An inline transformed carrier translates its placement once;
            // translating its basis as well would apply the edit twice.
            if fields.get("kind").and_then(Value::as_str) == Some("transformed") {
                if let Some(transform) = fields.get_mut("transform") {
                    translate_transform(transform, offset)?;
                    return Ok(());
                }
            }
            for (key, child) in fields {
                let point = matches!(
                    key.as_str(),
                    "position"
                        | "origin"
                        | "center"
                        | "point"
                        | "apex"
                        | "focus"
                        | "start"
                        | "end"
                        | "control_points"
                        | "points"
                        | "vertices"
                        | "poles"
                ) || (point && key == "rows");
                translate_coordinates(child, offset, point)?;
            }
        }
        Value::Array(values) => {
            if point && values.len() == 3 && values.iter().all(Value::is_number) {
                for (coordinate, displacement) in
                    values.iter_mut().zip([offset.x, offset.y, offset.z])
                {
                    translate_scalar(coordinate, displacement)?;
                }
            } else {
                for child in values {
                    translate_coordinates(child, offset, point)?;
                }
            }
        }
        _ => {}
    }
    Ok(())
}

fn translate_scalar(value: &mut Value, offset: f64) -> Result<(), CodecError> {
    let number = value
        .as_f64()
        .ok_or_else(|| CodecError::InvalidInput("coordinate is not numeric".into()))?
        + offset;
    *value =
        Value::Number(serde_json::Number::from_f64(number).ok_or_else(|| {
            CodecError::InvalidInput("translated coordinate is non-finite".into())
        })?);
    Ok(())
}

fn translate_transform(value: &mut Value, offset: Vector3) -> Result<(), CodecError> {
    let transform: cadmpeg_ir::transform::Transform = serde_json::from_value(value.clone())
        .map_err(|error| CodecError::InvalidInput(error.to_string()))?;
    let mut rows = transform.affine_rows();
    for (row, displacement) in rows.iter_mut().zip([offset.x, offset.y, offset.z]) {
        row[3] += displacement;
    }
    let transform = cadmpeg_ir::transform::Transform::affine(rows)
        .ok_or_else(|| CodecError::InvalidInput("translated placement is non-finite".into()))?;
    *value = serde_json::to_value(transform)
        .map_err(|error| CodecError::InvalidInput(error.to_string()))?;
    Ok(())
}

/// Neutral document JSON without scope, native records, or the neutral classes
/// explicitly excluded by the caller. A projection without design resolves
/// model PMI against the canonical parameter cache and removes attributes owned
/// by design entities; all other model content remains compared.
pub fn neutral_view(ir: &CadIr, exclude: &[ArenaClass]) -> Value {
    // Arena storage order is not content. Keep nested order (loops, poles,
    // dependency lists) and every identity/reference while canonicalizing only
    // the registry's entity arenas, as the document digest does.
    let mut normalized = ir.clone();
    if exclude.contains(&ArenaClass::Design) && !exclude.contains(&ArenaClass::Model) {
        // A geometry projection has no parameter arena to resolve these bindings.
        // Compare its literal against the same canonical cache used by the full
        // document, rather than dropping model PMI or comparing binding syntax.
        for (original, projected) in ir.model.pmi.iter().zip(&mut normalized.model.pmi) {
            if let (
                cadmpeg_ir::pmi::PmiDefinition::Dimension(original),
                cadmpeg_ir::pmi::PmiDefinition::Dimension(projected),
            ) = (&original.definition, &mut projected.definition)
            {
                if original.nominal().is_some() {
                    let nominal = original
                        .resolved_nominal(ir)
                        .expect("admitted PMI nominal resolves from its canonical parameter cache");
                    projected
                        .set_nominal(Some(nominal.into()))
                        .expect("resolved PMI nominal retains its admitted quantity");
                }
            }
        }
        normalized
            .model
            .attributes
            .retain(|attribute| match attribute.target {
                cadmpeg_ir::attributes::AttributeTarget::Feature(_)
                | cadmpeg_ir::attributes::AttributeTarget::Parameter(_)
                | cadmpeg_ir::attributes::AttributeTarget::Sketch(_) => false,
                cadmpeg_ir::attributes::AttributeTarget::Record { arena, .. } => {
                    arena.class() != ArenaClass::Design
                }
                _ => true,
            });
    }
    normalized.model.finalize();
    let mut value =
        serde_json::to_value(&normalized).expect("serialize admitted contract document");
    let document = value.as_object_mut().expect("document is an object");
    document.remove("scope");
    document.remove("native");

    if let Some(Value::Object(model)) = document.get_mut("model") {
        for arena in ArenaName::ALL {
            if exclude.contains(&arena.class()) {
                model.remove(arena.as_str());
            }
        }
    }
    value
}

/// Neutral arenas the actual output path declares fully written.
pub fn written_arenas(report: &ExportReport) -> BTreeSet<ArenaName> {
    ArenaName::ALL
        .iter()
        .copied()
        .zip(EntityKind::ALL)
        .filter(|(_, kind)| report.coverage.disposition(*kind) == ArenaDisposition::Written)
        .map(|(arena, _)| arena)
        .collect()
}

/// Compare every fully written neutral arena with the repository's numeric
/// tolerance and one global bijection of declared identities and references.
/// All record values, reference aliasing, and nested ordering remain content.
/// An input class outside scope is unknown, so its empty neutral projection is
/// not a conservation claim against records reconstructed from typed native data.
/// A held class remains compared even when all its input arenas are empty.
pub fn assert_written_arenas_agree(report: &ExportReport, left: &CadIr, right: &CadIr) {
    let held = written_arenas(report)
        .into_iter()
        .filter(|arena| left.scope.holds(arena.class()))
        .collect();
    identity::assert_agree(&held, left, right);
}

pub mod expressions;
mod identity;

pub mod iges;
pub mod parts;
pub mod zip;

#[cfg(test)]
mod tests;
