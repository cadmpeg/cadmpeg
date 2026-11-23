// SPDX-License-Identifier: Apache-2.0
//! Helical protrusions (`docs/formats/solidedge.md` §6.5): a
//! `JMHelixProtrusionFunction` feature names its axis, its section profile,
//! the end of the profile plane the helix runs from, its height, its pitch,
//! its revolution count and its hand, stated as a helical sweep of the
//! profile.
//!
//! A feature that does not read keeps its unresolved operation and adds
//! `feature.extent-unread`. Stored lengths are metres; the design states
//! millimetres.

use std::collections::BTreeMap;

use cadmpeg_ir::design::construction::{kinds, Construction, FrameValue};
use cadmpeg_ir::design::dimension::Dimension;
use cadmpeg_ir::design::feature::FeatureOperation;
use cadmpeg_ir::design::ops::swept::{HelicalSection, HelicalSweep, Helix, HelixLaw, HelixShape};
use cadmpeg_ir::design::options::{Hand, NativeOption, NativeOptions};
use cadmpeg_ir::document::CadIr;
use cadmpeg_ir::features::geometry::FinitePoint3;
use cadmpeg_ir::math::{Point3, Vector3};
use cadmpeg_ir::report::loss::LossNote;
use cadmpeg_ir::scalar::{FiniteReal, PositiveLength};
use cadmpeg_ir::units::UnitVector3;

use cadmpeg_core::text::NonBlankString;

use crate::construction;
use crate::extents::{
    axis_at, double_slot, is, output, profile_of, profile_selection, Profile, KIND_FINITE,
    MM_PER_M, PART_CLASS,
};
use crate::features::{feature_id, function_span, Kind, Listed};
use crate::loss::SolidEdgeLossCode;
use crate::psm::{Cluster, Object, TypeTable};

/// The function of a helical protrusion.
const HELIX_FUNCTION: &str = "JMHelixProtrusionFunction";

/// Bytes from the end of the function name to each slot (§6.5).
const AXIS_AFTER_NAME: usize = 68;
const FROM_AFTER_NAME: usize = 96;
const KIND_AFTER_NAME: usize = 156;
const HEIGHT_AFTER_NAME: usize = 184;
const PITCH_AFTER_NAME: usize = 212;
const TURNS_AFTER_NAME: usize = 240;
const HAND_AFTER_NAME: usize = 268;
/// The slots whose meaning is not established (§6.5).
const UNSETTLED_AFTER_NAME: [usize; 6] = [352, 408, 436, 464, 492, 520];

/// The values of the slot that names the end of the profile plane the helix
/// runs from.
const FROM_START: f64 = 29.0;
const FROM_END: f64 = 30.0;
/// The values of the hand slot.
const HAND_LEFT: f64 = 1.0;
const HAND_RIGHT: f64 = 2.0;

/// The pitch times the revolutions equals the height within this fraction of
/// the height.
const EPS_LAW: f64 = 1e-6;
/// The profile centre is off the axis when its distance from the axis exceeds
/// this many millimetres.
const EPS_OFF_AXIS: f64 = 1e-9;

fn positive_length(metres: f64, what: &str) -> Result<Dimension<PositiveLength>, String> {
    PositiveLength::new(metres * MM_PER_M)
        .map(Dimension::Literal)
        .ok_or_else(|| format!("the {what} {metres} is not a positive length"))
}

/// The arithmetic mean of the profile's sample points.
fn centre(profile: &Profile) -> Option<Vector3> {
    let count = f64::from(u32::try_from(profile.points.len()).ok()?);
    if profile.points.is_empty() {
        return None;
    }
    let (x, y, z) = profile.points.iter().fold((0.0, 0.0, 0.0), |sum, p| {
        (sum.0 + p.x, sum.1 + p.y, sum.2 + p.z)
    });
    Some(Vector3::new(x / count, y / count, z / count))
}

/// The helix frame: its origin is the stored axis point, its z axis the
/// direction the helix advances along and its x axis the direction from the
/// axis to the centre of the profile.
fn frame(
    axis: &cadmpeg_ir::design::construction::AxisValue,
    reversed: bool,
    profile: &Profile,
) -> Result<FrameValue, String> {
    let direction = Vector3::from(axis.direction);
    let z = if reversed {
        Vector3::new(-direction.x, -direction.y, -direction.z)
    } else {
        direction
    };
    let origin = axis.origin.get();
    let centre = centre(profile).ok_or_else(|| "the profile has no curve".to_owned())?;
    let offset = Vector3::new(
        centre.x - origin.x,
        centre.y - origin.y,
        centre.z - origin.z,
    );
    let along = offset.dot(z);
    let radial = Vector3::new(
        offset.x - along * z.x,
        offset.y - along * z.y,
        offset.z - along * z.z,
    );
    if radial.norm() <= EPS_OFF_AXIS {
        return Err("the profile centre lies on the axis".to_owned());
    }
    let x_axis = UnitVector3::normalized(radial)
        .ok_or_else(|| "the profile centre has no direction from the axis".to_owned())?;
    let z_axis =
        UnitVector3::normalized(z).ok_or_else(|| "the axis direction is not a unit".to_owned())?;
    let y_axis = UnitVector3::normalized(Vector3::from(z_axis).cross(Vector3::from(x_axis)))
        .ok_or_else(|| "the helix frame has no y axis".to_owned())?;
    Ok(FrameValue {
        origin: FinitePoint3::new(Point3::new(origin.x, origin.y, origin.z))
            .ok_or_else(|| "the axis point is not finite".to_owned())?,
        x_axis,
        y_axis,
        z_axis,
    })
}

fn helix(
    cluster: &Cluster<'_>,
    types: &TypeTable,
    object: &Object<'_>,
    end: usize,
    profile: &Profile,
) -> Result<FeatureOperation, String> {
    let kind = double_slot(cluster, types, object, end, KIND_AFTER_NAME, "extent kind")?;
    if !is(kind, KIND_FINITE) {
        return Err(format!("the extent kind {kind} of a helix is not 13"));
    }
    let from = double_slot(cluster, types, object, end, FROM_AFTER_NAME, "start end")?;
    let reversed = if is(from, FROM_START) {
        false
    } else if is(from, FROM_END) {
        true
    } else {
        return Err(format!("the start-end value {from} is not 29 or 30"));
    };
    let hand = double_slot(cluster, types, object, end, HAND_AFTER_NAME, "hand")?;
    let hand = if is(hand, HAND_RIGHT) {
        Hand::Right
    } else if is(hand, HAND_LEFT) {
        Hand::Left
    } else {
        return Err(format!("the hand value {hand} is not 1 or 2"));
    };
    let height = double_slot(cluster, types, object, end, HEIGHT_AFTER_NAME, "height")?;
    let pitch = double_slot(cluster, types, object, end, PITCH_AFTER_NAME, "pitch")?;
    let turns = double_slot(cluster, types, object, end, TURNS_AFTER_NAME, "revolutions")?;
    if !(height > 0.0 && pitch > 0.0 && turns > 0.0) {
        return Err("the height, pitch or revolutions is not positive".to_owned());
    }
    if (pitch * turns - height).abs() > EPS_LAW * height {
        return Err("the pitch times the revolutions is not the height".to_owned());
    }
    let axis = axis_at(cluster, types, object, end, AXIS_AFTER_NAME)?;
    let placement = frame(&axis, reversed, profile)?;
    let mut native_options = NativeOptions::new();
    let revolutions =
        FiniteReal::new(turns).ok_or_else(|| "the revolutions are not finite".to_owned())?;
    insert(
        &mut native_options,
        "revolutions",
        NativeOption::Real(Dimension::Literal(revolutions)),
    );
    for after in UNSETTLED_AFTER_NAME {
        // A slot that holds no number states nothing.
        if let Ok(value) = double_slot(cluster, types, object, end, after, "unsettled slot") {
            if let Some(value) = FiniteReal::new(value) {
                insert(
                    &mut native_options,
                    &format!("slot_{after}"),
                    NativeOption::Real(Dimension::Literal(value)),
                );
            }
        }
    }
    Ok(FeatureOperation::HelicalSweep(Box::new(HelicalSweep {
        section: HelicalSection::Profile(profile_selection(&profile.id)?),
        helix: Helix {
            placement: Construction::<kinds::Frame> {
                resolved: Some(placement),
                ..construction::empty()
            },
            start: None,
            shape: HelixShape::Helix {
                law: Some(HelixLaw::PitchHeight {
                    pitch: positive_length(pitch, "pitch")?,
                    height: positive_length(height, "height")?,
                }),
                taper: None,
            },
            hand: Some(Dimension::Literal(hand)),
        },
        output: output(false),
        native_options,
        source_settings: BTreeMap::new(),
    })))
}

fn insert(options: &mut NativeOptions, name: &str, value: NativeOption) {
    if let Some(key) = NonBlankString::new(name.to_owned()) {
        options.insert(key, value);
    }
}

/// State the helical sweep of each helical protrusion whose profile is a
/// stated sketch. A feature that does not read keeps its unresolved operation
/// and adds `feature.extent-unread`.
pub(crate) fn project(
    cluster: &Cluster<'_>,
    types: &TypeTable,
    listed: &[Listed],
    ir: &mut CadIr,
    losses: &mut Vec<LossNote>,
) {
    for entry in listed {
        let Kind::Function(Some(function)) = &entry.kind else {
            continue;
        };
        if function != HELIX_FUNCTION {
            continue;
        }
        let Some(object) = cluster.object(entry.object) else {
            continue;
        };
        if types.class_name(object.form, object.flags) != Some(PART_CLASS) {
            continue;
        }
        let Some((_, end)) = function_span(&object) else {
            continue;
        };
        let id = feature_id(entry.object);
        let read = match profile_of(ir, &id) {
            Some(profile) => helix(cluster, types, &object, end, &profile),
            None => Err("the profile is not a stated sketch".to_owned()),
        };
        match read {
            Ok(operation) => {
                if let Some(feature) = ir.design.features.iter_mut().find(|f| f.id == id) {
                    feature.definition.operation = operation;
                }
            }
            Err(reason) => losses.push(SolidEdgeLossCode::FeatureExtentUnread.note(format!(
                "the helix of feature object {:#x} ({function}) does not read: {reason}; the \
                 feature stays unresolved",
                entry.object
            ))),
        }
    }
}

#[cfg(test)]
mod tests;
