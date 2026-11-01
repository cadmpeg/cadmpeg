// SPDX-License-Identifier: Apache-2.0
//! Schema readers over composed prims (spec §10 to §15).
//!
//! Each reader takes a stage prim and returns the schema's data in scene
//! units and the source's own conventions. [`Issues`] collects what the
//! readers could not read, for the loss notes.

pub(crate) mod curves;
pub(crate) mod intrinsic;
pub(crate) mod mesh;
pub(crate) mod patch;
pub(crate) mod physics;
pub(crate) mod physx;
pub(crate) mod shading;
pub(crate) mod texture;
pub(crate) mod xform;

use std::collections::BTreeSet;

use crate::stage::{Prim, Resolved};
use crate::text::value::{typed, Typed};

/// What the schema readers could not read, by attribute path.
#[derive(Debug, Default, Clone)]
pub(crate) struct Issues {
    /// Attributes whose value does not have its declared type, with the
    /// reason.
    pub(crate) malformed: Vec<(String, String)>,
    /// Attributes with time samples or a spline.
    pub(crate) animated: BTreeSet<String>,
}

impl Issues {
    pub(crate) fn malformed(&mut self, prim: &Prim<'_>, name: &str, reason: impl Into<String>) {
        self.malformed
            .push((format!("{}.{name}", prim.path), reason.into()));
    }
}

/// The typed value of attribute `name` at the default time, or `None` when
/// it has no value or its value is malformed.
pub(crate) fn attribute(prim: &Prim<'_>, name: &str, issues: &mut Issues) -> Option<Typed> {
    if prim.is_animated(name) {
        issues.animated.insert(format!("{}.{name}", prim.path));
    }
    let (value, spec) = match prim.resolve(name)? {
        Resolved::Default(value, spec) | Resolved::FirstSample(value, spec) => (value, spec),
        Resolved::Blocked => return None,
    };
    match typed(value, &spec.type_name, spec.array) {
        Ok(typed) => Some(typed),
        Err(error) => {
            issues.malformed(prim, name, error.to_string());
            None
        }
    }
}

/// The token or string value of attribute `name`.
pub(crate) fn token(prim: &Prim<'_>, name: &str, issues: &mut Issues) -> Option<String> {
    let value = attribute(prim, name, issues)?;
    match value.texts() {
        Some([text]) if !value.array => Some(text.clone()),
        _ => {
            issues.malformed(prim, name, "expected one token");
            None
        }
    }
}

/// The token-array value of attribute `name`.
pub(crate) fn tokens(prim: &Prim<'_>, name: &str, issues: &mut Issues) -> Option<Vec<String>> {
    let value = attribute(prim, name, issues)?;
    match value.texts() {
        Some(texts) if value.array => Some(texts.to_vec()),
        _ => {
            issues.malformed(prim, name, "expected a token array");
            None
        }
    }
}

/// The scalar integer value of attribute `name`.
pub(crate) fn integer(prim: &Prim<'_>, name: &str, issues: &mut Issues) -> Option<i128> {
    let value = attribute(prim, name, issues)?;
    match value.integers() {
        Some([number]) if !value.array => Some(*number),
        _ => {
            issues.malformed(prim, name, "expected one integer");
            None
        }
    }
}

/// The integer-array value of attribute `name`.
pub(crate) fn integers(prim: &Prim<'_>, name: &str, issues: &mut Issues) -> Option<Vec<i128>> {
    let value = attribute(prim, name, issues)?;
    match value.integers() {
        Some(values) if value.array && value.width() == 1 => Some(values.to_vec()),
        _ => {
            issues.malformed(prim, name, "expected an integer array");
            None
        }
    }
}

/// The integer-array value of attribute `name`, `width` components per
/// element.
pub(crate) fn integer_tuples(
    prim: &Prim<'_>,
    name: &str,
    width: usize,
    issues: &mut Issues,
) -> Option<Vec<i128>> {
    let value = attribute(prim, name, issues)?;
    match value.integers() {
        Some(values) if value.array && value.width() == width => Some(values.to_vec()),
        _ => {
            issues.malformed(
                prim,
                name,
                format!("expected an array of {width}-component integers"),
            );
            None
        }
    }
}

/// The scalar bool value of attribute `name`.
pub(crate) fn boolean(prim: &Prim<'_>, name: &str, issues: &mut Issues) -> Option<bool> {
    let value = attribute(prim, name, issues)?;
    match value.bools() {
        Some([flag]) if !value.array => Some(*flag),
        _ => {
            issues.malformed(prim, name, "expected one bool");
            None
        }
    }
}

/// The real-array value of attribute `name`, `width` components per element.
pub(crate) fn reals(
    prim: &Prim<'_>,
    name: &str,
    width: usize,
    issues: &mut Issues,
) -> Option<Vec<f64>> {
    let value = attribute(prim, name, issues)?;
    match value.reals() {
        Some(values) if value.array && value.width() == width => Some(values.to_vec()),
        _ => {
            issues.malformed(
                prim,
                name,
                format!("expected an array of {width}-component reals"),
            );
            None
        }
    }
}

/// The tuple value of attribute `name`, `width` real components.
pub(crate) fn vector(
    prim: &Prim<'_>,
    name: &str,
    width: usize,
    issues: &mut Issues,
) -> Option<Vec<f64>> {
    let value = attribute(prim, name, issues)?;
    match value.reals() {
        Some(values) if !value.array && value.width() == width => Some(values.to_vec()),
        _ => {
            issues.malformed(prim, name, format!("expected a {width}-component tuple"));
            None
        }
    }
}

/// The real-array value of attribute `name` as `[x, y, z]` triples.
pub(crate) fn vectors3(prim: &Prim<'_>, name: &str, issues: &mut Issues) -> Option<Vec<[f64; 3]>> {
    let values = reals(prim, name, 3, issues)?;
    Some(
        values
            .chunks_exact(3)
            .map(|chunk| [chunk[0], chunk[1], chunk[2]])
            .collect(),
    )
}

/// One `usize` index from a source integer.
pub(crate) fn index(value: i128) -> Option<usize> {
    usize::try_from(value).ok()
}

#[cfg(test)]
mod tests;
