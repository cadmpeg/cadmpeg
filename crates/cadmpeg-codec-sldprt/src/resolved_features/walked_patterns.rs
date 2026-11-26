//! Circular-pattern operands the exact archive walk read (sldprt.md §2.2,
//! §2.3), written into the history record as a lane-derived group.

use std::collections::HashMap;
use std::f64::consts::TAU;

use cadmpeg_core::text::NonBlankString;
use cadmpeg_ir::features::geometry::FinitePoint3;
use cadmpeg_ir::math::{Point3, Vector3};
use cadmpeg_ir::units::UnitVector3;

use super::scalars::FeatureObjectNames;
use super::selections::generated_surface_identities;
use crate::carchive::reader::V;
use crate::carchive::PatternSeeds;
use crate::records::{
    Feature, FeatureHistory, FeatureInputLane, CIRCULAR_PATTERN_ANGLE_DIMENSION,
    CIRCULAR_PATTERN_COUNT_DIMENSION, CIRCULAR_PATTERN_SEED_SOURCES,
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

/// How far a circular pattern's angle may lie from a full turn, and its
/// listed angle from its walked angle.
pub(crate) const EPS_FULL_TURN_RAD: f64 = 1e-9;

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
    if matching.next().is_some() || pattern.other_sources {
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
/// walked group the operands its walked object states, where no lane's
/// interval of the pattern holds a seed identity record or a temporary-axis
/// carrier, every lane that walked it walked it once and agrees on them, its
/// one seed names one other feature of its history with a source id no other
/// feature of the history holds, and its angle is a full turn.
pub(crate) fn enrich_circular_patterns(
    histories: &mut [FeatureHistory],
    feature_lanes: &[FeatureInputLane],
    walked: &WalkedPatterns,
) -> Result<(), cadmpeg_core::CodecError> {
    let mut lanes = walked.keys().collect::<Vec<_>>();
    lanes.sort_unstable();
    for history in histories.iter_mut() {
        let mut updates = Vec::new();
        for (index, feature) in history.features.iter().enumerate() {
            if feature.input_class.as_deref() != Some("moCirPattern_c")
                || feature.states_any(&CIRCULAR_PATTERN_WALKED_PROPERTIES)
            {
                continue;
            }
            let mut other_sources = false;
            for lane in feature_lanes {
                other_sources |= interval_states_other_sources(lane, history, feature)?;
            }
            if other_sources {
                continue;
            }
            let readings = lanes
                .iter()
                .filter(|lane| {
                    walked[**lane]
                        .iter()
                        .any(|pattern| pattern.pattern.as_deref() == Some(feature.name.as_str()))
                })
                .map(|lane| {
                    walked_circular(walked, lane, &feature.name, |name| {
                        feature.parameters.contains_key(name)
                    })
                })
                .collect::<Option<Vec<_>>>();
            let Some((reading, others)) = readings.as_deref().and_then(<[_]>::split_first) else {
                continue;
            };
            if others.iter().any(|other| other != reading) {
                continue;
            }
            if let Some(properties) = walked_properties(reading, history, feature) {
                updates.push((index, properties));
            }
        }
        for (index, properties) in updates {
            history.features[index].properties.extend(properties);
        }
    }
    Ok(())
}

/// Whether `lane`'s interval of `pattern`, from its object name to the next
/// object name of a feature of `history`, holds a seed identity record whose
/// first component names the pattern, or a `moTempAxisRef_w` declaration.
fn interval_states_other_sources(
    lane: &FeatureInputLane,
    history: &FeatureHistory,
    pattern: &Feature,
) -> Result<bool, cadmpeg_core::CodecError> {
    let names = FeatureObjectNames::new(&lane.names);
    let Some(start) = names
        .of(pattern)
        .and_then(|name| usize::try_from(name.offset).ok())
    else {
        return Ok(false);
    };
    let end = history
        .features
        .iter()
        .filter_map(|feature| usize::try_from(names.of(feature)?.offset).ok())
        .filter(|offset| *offset > start)
        .min()
        .unwrap_or(lane.native_payload.len())
        .min(lane.native_payload.len());
    if lane.native_payload.get(start..end).is_some_and(|interval| {
        interval
            .windows(CARRIER.len())
            .any(|window| window == CARRIER)
    }) {
        return Ok(true);
    }
    let Some(source) = pattern.source_value() else {
        return Ok(false);
    };
    let discovered;
    let identities = if lane.generated_surface_identities.is_empty() {
        discovered = generated_surface_identities(lane)?;
        &discovered
    } else {
        &lane.generated_surface_identities
    };
    Ok(identities.iter().any(|identity| {
        usize::try_from(identity.offset).is_ok_and(|offset| (start..end).contains(&offset))
            && identity.components.first().is_some_and(|component| {
                cadmpeg_core::decode::View::u32_le_at(component.type_signature.as_ref(), 4)
                    == Some(source)
            })
    }))
}

/// The class name a temporary-axis carrier's declaration writes.
const CARRIER: &[u8] = b"moTempAxisRef_w";

/// The record fields of `reading`, the walked operands of `feature`, a
/// pattern of `history`, where the record's listed count and angle equal the
/// walked ones.
fn walked_properties(
    reading: &WalkedCircular,
    history: &FeatureHistory,
    feature: &Feature,
) -> Option<Vec<(NonBlankString, String)>> {
    let pattern = feature.name.as_str();
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
    let source = seed.source_value()?;
    if history
        .features
        .iter()
        .filter(|candidate| candidate.source_value() == Some(source))
        .count()
        != 1
    {
        return None;
    }
    let (origin, direction) = reading.axis?;
    let (count, walked_count) = reading.count.as_ref()?;
    let (angle, value) = reading.angle.as_ref()?;
    if (value - TAU).abs() > EPS_FULL_TURN_RAD {
        return None;
    }
    let listed_count = feature
        .parameters
        .get(count.as_str())
        .and_then(|text| crate::history::project::pattern::parse_count(text))?;
    let listed_angle = feature
        .parameters
        .get(angle.as_str())
        .and_then(|text| crate::history::literals::parse_positive_angle_rad(text))?;
    if listed_count != *walked_count || (listed_angle.get() - value).abs() > EPS_FULL_TURN_RAD {
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
            NonBlankString::new(CIRCULAR_PATTERN_SEED_SOURCES)?,
            source.to_string(),
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
