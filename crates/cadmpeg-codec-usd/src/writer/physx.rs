// SPDX-License-Identifier: Apache-2.0
//! Writes the settings of the carried `PhysX` schemas (spec §16.8).
//!
//! A relationship target is written as a placeholder that names its record;
//! the placeholders are replaced by prim paths when the layer is complete,
//! since a target can be a prim written after the relationship.

use std::collections::BTreeMap;

use cadmpeg_ir::physics::solver::{SolverSettings, SolverTarget, SolverValue};

use super::text::{quoted, real, real32};
use crate::schema::physx::{find, holder, split, Attribute, Holder, Value};

/// The solver whose settings the writer writes.
const SOLVER: &str = "physx";
/// The delimiter of a relationship-target placeholder.
const MARK: char = '\u{1}';

/// The placeholder of the prim of a target record.
pub(super) fn placeholder(target: &SolverTarget) -> Option<String> {
    let key = match target {
        SolverTarget::Occurrence { occurrence } => format!("occurrence:{occurrence}"),
        SolverTarget::Joint { joint } => format!("joint:{joint}"),
        _ => return None,
    };
    Some(format!("{MARK}{key}{MARK}"))
}

/// Replaces the placeholders in `text` by the paths of `paths`, keyed by
/// placeholder. A line with a placeholder that has no path is dropped.
/// Returns the text and the number of dropped lines.
pub(super) fn resolve(text: &str, paths: &BTreeMap<String, String>) -> (String, usize) {
    let mut out = String::with_capacity(text.len());
    let mut dropped = 0;
    for line in text.split_inclusive('\n') {
        if !line.contains(MARK) {
            out.push_str(line);
            continue;
        }
        let mut resolved = String::with_capacity(line.len());
        let mut complete = true;
        let mut parts = line.split(MARK);
        if let Some(first) = parts.next() {
            resolved.push_str(first);
        }
        while let (Some(key), Some(rest)) = (parts.next(), parts.next()) {
            match paths.get(&format!("{MARK}{key}{MARK}")) {
                Some(path) => resolved.push_str(path),
                None => complete = false,
            }
            resolved.push_str(rest);
        }
        if complete {
            out.push_str(&resolved);
        } else {
            dropped += 1;
        }
    }
    (out, dropped)
}

/// The holder of a settings record's schema, for a `PhysX` record.
pub(super) fn settings_holder(settings: &SolverSettings) -> Option<Holder> {
    (settings.solver == SOLVER)
        .then(|| holder(split(&settings.schema).0))
        .flatten()
}

/// The text of a real value of `attribute` in scene units, or `None` when
/// no unit of the attribute has the value's dimension.
fn real_text(
    attribute: &Attribute,
    value: f64,
    dimension: &cadmpeg_ir::physics::solver::Dimension,
) -> Option<String> {
    let dim = attribute
        .unit
        .candidates()
        .into_iter()
        .find(|candidate| candidate.dimension() == *dimension)?;
    let radians = if dim.degrees {
        std::f64::consts::PI / 180.0
    } else {
        1.0
    };
    let scene = value / dimension.factor(super::MILLIMETRES_PER_UNIT, 1.0, radians);
    Some(if attribute.usd_type.starts_with("double") {
        real(scene)
    } else {
        real32(crate::text::value::round_f32(scene))
    })
}

/// The value text of one property, or `None` when it does not fit its
/// attribute.
fn value_text(attribute: &Attribute, value: &SolverValue) -> Option<String> {
    Some(match (attribute.value, value) {
        (Value::Bool, SolverValue::Bool { value }) => if *value { "1" } else { "0" }.to_owned(),
        (Value::Integer, SolverValue::Integer { value }) => value.to_string(),
        (Value::Real, SolverValue::Real { value, dimension }) => {
            real_text(attribute, value.get(), dimension)?
        }
        (Value::Real, SolverValue::Unbounded { negative }) => {
            if *negative { "-inf" } else { "inf" }.to_owned()
        }
        (Value::Reals, SolverValue::Reals { values, dimension }) => {
            let texts = values
                .iter()
                .map(|value| real_text(attribute, value.get(), dimension))
                .collect::<Option<Vec<_>>>()?;
            if attribute.usd_type.ends_with("[]") {
                format!("[{}]", texts.join(", "))
            } else {
                format!("({})", texts.join(", "))
            }
        }
        (Value::Token, SolverValue::Token { value }) => quoted(value),
        (Value::Tokens, SolverValue::Tokens { values }) => format!(
            "[{}]",
            values
                .iter()
                .map(|value| quoted(value))
                .collect::<Vec<_>>()
                .join(", ")
        ),
        (Value::Targets, SolverValue::Targets { targets }) => format!(
            "[{}]",
            targets
                .iter()
                .map(|target| placeholder(target).map(|key| format!("<{key}>")))
                .collect::<Option<Vec<_>>>()?
                .join(", ")
        ),
        _ => return None,
    })
}

/// The applied-schema metadata and the property lines of a `PhysX` settings
/// record, and the number of its properties that were not written.
pub(super) fn lines(settings: &SolverSettings) -> (String, Vec<String>, usize) {
    let (base, instance) = split(&settings.schema);
    let mut out = Vec::new();
    let mut unwritten = 0;
    for (name, value) in &settings.values {
        let text = find(base, instance, name).and_then(|attribute| {
            let text = value_text(attribute, value)?;
            Some(if attribute.value == Value::Targets {
                format!("rel {name} = {text}")
            } else {
                format!(
                    "{}{} {name} = {text}",
                    if attribute.uniform { "uniform " } else { "" },
                    attribute.usd_type
                )
            })
        });
        match text {
            Some(text) => out.push(text),
            None => unwritten += 1,
        }
    }
    (
        format!("prepend apiSchemas = [{}]", quoted(&settings.schema)),
        out,
        unwritten,
    )
}

#[cfg(test)]
mod tests;
