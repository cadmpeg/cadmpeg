//! Circular-pattern operands the exact archive walk read (sldprt.md §2.2,
//! §2.3).

use std::collections::HashMap;

use cadmpeg_ir::features::geometry::FinitePoint3;
use cadmpeg_ir::math::{Point3, Vector3};
use cadmpeg_ir::units::UnitVector3;

use crate::carchive::reader::V;
use crate::carchive::PatternSeeds;

#[cfg(test)]
mod tests;

/// The feature patterns each lane's walk read, by lane id.
pub(crate) type WalkedPatterns = HashMap<String, Vec<PatternSeeds>>;

/// Metres to the millimetres of the model.
const NATIVE_TO_IR: f64 = 1000.0;

/// The operands a circular pattern's walked object states. Each is `None`
/// where the object states it in no admitted form.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct WalkedCircular {
    /// The seeds' feature names, in list order.
    pub(crate) seeds: Option<Vec<String>>,
    /// A point on the axis, in millimetres, and the axis direction.
    pub(crate) axis: Option<(FinitePoint3, UnitVector3)>,
    /// The instance count.
    pub(crate) count: Option<u32>,
    /// The angle, in radians.
    pub(crate) angle: Option<f64>,
}

/// The walked operands of `feature`, a circular pattern of lane `lane`: the
/// one walked pattern of the feature's name. `listed` names the dimensions
/// the feature's record lists; the count and the angle are the one integer
/// and the one angle parameter object of the pattern that it lists.
pub(crate) fn walked_circular(
    walked: &WalkedPatterns,
    lane: &str,
    name: &str,
    listed: impl Fn(&str) -> bool,
) -> Option<WalkedCircular> {
    let mut matching = walked
        .get(lane)?
        .iter()
        .filter(|pattern| pattern.pattern.as_deref() == Some(name));
    let pattern = matching.next()?;
    if matching.next().is_some() {
        return None;
    }
    let seeds = pattern
        .seeds
        .iter()
        .cloned()
        .collect::<Option<Vec<_>>>()
        .filter(|seeds| !seeds.is_empty());
    let axis = pattern.axis.and_then(|(tail, _)| {
        let origin = FinitePoint3::new(Point3::new(
            tail[1] * NATIVE_TO_IR,
            tail[2] * NATIVE_TO_IR,
            tail[3] * NATIVE_TO_IR,
        ))?;
        let direction = UnitVector3::normalized(Vector3::new(tail[4], tail[5], tail[6]))?;
        Some((origin, direction))
    });
    let listed_value = |class: &str| {
        let mut values = pattern.parameters.iter().filter(|parameter| {
            &*parameter.class == class && matches!(&parameter.name, V::Str(name) if listed(name))
        });
        let value = values.next()?.value;
        values.next().is_none().then_some(value)
    };
    let count = listed_value("moIntegerParameter_c")
        .filter(|count| count.fract() == 0.0)
        .and_then(cadmpeg_core::convert::truncate_f64_to_u32)
        .filter(|count| *count > 0);
    let angle = listed_value("moAngleParameter_c").filter(|angle| angle.is_finite());
    Some(WalkedCircular {
        seeds,
        axis,
        count,
        angle,
    })
}
