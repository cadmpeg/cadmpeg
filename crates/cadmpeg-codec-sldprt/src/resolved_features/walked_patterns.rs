//! Circular-pattern operands the exact archive walk read (sldprt.md §2.2,
//! §2.3), written into the history record as a lane-derived group.

use std::collections::HashMap;
use std::f64::consts::TAU;

use cadmpeg_core::text::NonBlankString;
use cadmpeg_ir::features::geometry::FinitePoint3;
use cadmpeg_ir::math::{Point3, Vector3};
use cadmpeg_ir::units::UnitVector3;

use crate::carchive::reader::V;
use crate::carchive::PatternSeeds;
use crate::records::{
    FeatureHistory, CIRCULAR_PATTERN_ANGLE_DIMENSION, CIRCULAR_PATTERN_COUNT_DIMENSION,
    CIRCULAR_PATTERN_WALKED_PROPERTIES,
};

#[cfg(test)]
mod tests;

/// The feature patterns each lane's walk read, by lane id.
pub(crate) type WalkedPatterns = HashMap<String, Vec<PatternSeeds>>;

/// Metres to the millimetres of the model.
const NATIVE_TO_IR: f64 = 1000.0;

/// How far an axis tail's direction may lie from unit length.
const EPS_UNIT_DIRECTION: f64 = 1e-9;

/// How far a circular pattern's angle may lie from a full turn.
const EPS_FULL_TURN_RAD: f64 = 1e-9;

/// The operands a circular pattern's walked object states. Each is `None`
/// where the object states it in no admitted form.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct WalkedCircular {
    /// The seeds' feature names, in list order.
    pub(crate) seeds: Option<Vec<String>>,
    /// A point on the axis, in millimetres, and the unit axis direction.
    pub(crate) axis: Option<(FinitePoint3, UnitVector3)>,
    /// The name of the dimension that holds the instance count, and the count.
    pub(crate) count: Option<(String, u32)>,
    /// The name of the dimension that holds the angle, and the angle in
    /// radians.
    pub(crate) angle: Option<(String, f64)>,
}

/// The walked operands of the circular pattern `name` of lane `lane`: the one
/// walked pattern of that name. `listed` names the dimensions the feature's
/// record lists; the count and the angle are the one integer and the one
/// angle parameter object of the pattern that it lists.
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
        let raw = Vector3::new(tail[4], tail[5], tail[6]);
        if (raw.dot(raw) - 1.0).abs() > EPS_UNIT_DIRECTION {
            return None;
        }
        Some((origin, UnitVector3::normalized(raw)?))
    });
    let listed_value = |class: &str| {
        let mut values = pattern
            .parameters
            .iter()
            .filter_map(|parameter| match &parameter.name {
                V::Str(name) if &*parameter.class == class && listed(name) => {
                    Some((name.clone(), parameter.value))
                }
                _ => None,
            });
        let value = values.next()?;
        values.next().is_none().then_some(value)
    };
    let count = listed_value("moIntegerParameter_c").and_then(|(name, count)| {
        let count = Some(count)
            .filter(|count| count.fract() == 0.0)
            .and_then(cadmpeg_core::convert::truncate_f64_to_u32)
            .filter(|count| *count > 0)?;
        Some((name, count))
    });
    let angle = listed_value("moAngleParameter_c").filter(|(_, angle)| angle.is_finite());
    Some(WalkedCircular {
        seeds,
        axis,
        count,
        angle,
    })
}

/// Give each circular pattern of `histories` whose record states none of the
/// walked group the operands its walked object states, where every lane that
/// walked it agrees on them, its one seed names one other feature of its
/// history, and its angle is a full turn.
pub(crate) fn enrich_circular_patterns(histories: &mut [FeatureHistory], walked: &WalkedPatterns) {
    let mut lanes = walked.keys().collect::<Vec<_>>();
    lanes.sort_unstable();
    for history in histories.iter_mut() {
        for index in 0..history.features.len() {
            let feature = &history.features[index];
            if feature.input_class.as_deref() != Some("moCirPattern_c")
                || feature.states_any(&CIRCULAR_PATTERN_WALKED_PROPERTIES)
            {
                continue;
            }
            let mut readings = lanes.iter().filter_map(|lane| {
                walked_circular(walked, lane, &feature.name, |name| {
                    feature.parameters.contains_key(name)
                })
            });
            let Some(reading) = readings.next() else {
                continue;
            };
            if !readings.all(|other| other == reading) {
                continue;
            }
            let Some(properties) = walked_properties(&reading, history, &feature.name) else {
                continue;
            };
            history.features[index].properties.extend(properties);
        }
    }
}

/// The record fields of `reading`, the walked operands of the pattern
/// `pattern` of `history`.
fn walked_properties(
    reading: &WalkedCircular,
    history: &FeatureHistory,
    pattern: &str,
) -> Option<Vec<(NonBlankString, String)>> {
    let [seed] = reading.seeds.as_deref()? else {
        return None;
    };
    let mut named = history
        .features
        .iter()
        .filter(|candidate| &candidate.name == seed && candidate.name != pattern);
    let seed = named.next()?;
    if named.next().is_some() {
        return None;
    }
    let (origin, direction) = reading.axis?;
    let (count, _) = reading.count.as_ref()?;
    let (angle, value) = reading.angle.as_ref()?;
    if (value - TAU).abs() > EPS_FULL_TURN_RAD {
        return None;
    }
    let origin = origin.get();
    let direction = direction.as_raw();
    Some(vec![
        (
            cadmpeg_core::nonblank_literal!("AxisOrigin"),
            format!("{}mm,{}mm,{}mm", origin.x, origin.y, origin.z),
        ),
        (
            cadmpeg_core::nonblank_literal!("AxisDirection"),
            format!("{},{},{}", direction.x, direction.y, direction.z),
        ),
        (
            cadmpeg_core::nonblank_literal!("Seeds"),
            seed.source_value()?.to_string(),
        ),
        (
            NonBlankString::new(CIRCULAR_PATTERN_COUNT_DIMENSION)?,
            count.clone(),
        ),
        (
            NonBlankString::new(CIRCULAR_PATTERN_ANGLE_DIMENSION)?,
            angle.clone(),
        ),
    ])
}
