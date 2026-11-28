// SPDX-License-Identifier: Apache-2.0
//! Placement property admission and quaternion frames.
use crate::native::frame::FiniteFrame;
use crate::native::PropertyRecord;
use cadmpeg_core::CodecError;
use cadmpeg_ir::features::geometry::FiniteVector3;
use cadmpeg_ir::math::Vector3;
use cadmpeg_ir::scalar::FiniteReal;
use cadmpeg_ir::units::{FiniteVector, UnitVector3};

pub(crate) fn placement_matrix(
    property: &PropertyRecord,
) -> Result<Option<FiniteFrame>, CodecError> {
    match placement_matrix_value(property) {
        Ok(value) => Ok(Some(value)),
        Err(issue) => Err(CodecError::malformed(format_args!(
            "placement property {} {issue}",
            property.id
        ))),
    }
}

#[derive(Clone, Copy)]
enum PlacementIssue {
    RuntimeType,
    ValueCount,
    ValueTag,
    Position(&'static str),
    Axis(&'static str),
    Angle,
    Quaternion(&'static str),
    Rotation,
}

impl std::fmt::Display for PlacementIssue {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::RuntimeType => formatter.write_str("has a non-placement runtime type"),
            Self::ValueCount => formatter.write_str("requires one placement value"),
            Self::ValueTag => formatter.write_str("requires one PropertyPlacement value"),
            Self::Position(name) => write!(formatter, "has an invalid {name} component"),
            Self::Axis(name) => write!(formatter, "has an invalid {name} axis component"),
            Self::Angle => formatter.write_str("has an invalid A angle component"),
            Self::Quaternion(name) => {
                write!(formatter, "has an invalid {name} quaternion component")
            }
            Self::Rotation => formatter.write_str("has an invalid rotation"),
        }
    }
}

fn placement_matrix_value(property: &PropertyRecord) -> Result<FiniteFrame, PlacementIssue> {
    let values = FiniteVector::from(placement_value_components(property)?);
    placement_components_admitted(values).ok_or(PlacementIssue::Rotation)
}

/// The components `px, py, pz, qx, qy, qz, qw` a placement property stores,
/// with an axis and angle as its quaternion.
pub(crate) fn placement_values(property: &PropertyRecord) -> Option<[FiniteReal; 7]> {
    placement_value_components(property).ok()
}

fn placement_value_components(
    property: &PropertyRecord,
) -> Result<[FiniteReal; 7], PlacementIssue> {
    if property.type_name != "App::PropertyPlacement" {
        return Err(PlacementIssue::RuntimeType);
    }
    if property.values().len() != 1 {
        return Err(PlacementIssue::ValueCount);
    }
    let value = &property.values()[0];
    if value.tag != "PropertyPlacement" {
        return Err(PlacementIssue::ValueTag);
    }
    let number = |name: &str| {
        value
            .attributes
            .get(name)
            .and_then(|value| value.parse().ok())
            .and_then(FiniteReal::new)
    };
    let position =
        ["Px", "Py", "Pz"].map(|name| number(name).ok_or(PlacementIssue::Position(name)));
    let [px, py, pz] = position;
    let position = [px?, py?, pz?];
    let quaternion = if value.attributes.contains_key("A") {
        let axis = ["Ox", "Oy", "Oz"].map(|name| number(name).ok_or(PlacementIssue::Axis(name)));
        let [ox, oy, oz] = axis;
        let [ox, oy, oz] = [ox?, oy?, oz?];
        let angle = number("A").ok_or(PlacementIssue::Angle)?;
        let axis = FiniteVector3::from_components(ox, oy, oz);
        let unit = UnitVector3::normalized_nonzero(axis).unwrap_or(UnitVector3::Z_AXIS);
        let unit = Vector3::from(unit);
        let (x, y, z) = (unit.x, unit.y, unit.z);
        let half_angle = angle.get() / 2.0;
        let scale = half_angle.sin();
        let components = [x * scale, y * scale, z * scale, half_angle.cos()];
        let [Some(q0), Some(q1), Some(q2), Some(q3)] = components.map(FiniteReal::new) else {
            return Err(PlacementIssue::Rotation);
        };
        [q0, q1, q2, q3]
    } else {
        let components = ["Q0", "Q1", "Q2", "Q3"]
            .map(|name| number(name).ok_or(PlacementIssue::Quaternion(name)));
        let [q0, q1, q2, q3] = components;
        [q0?, q1?, q2?, q3?]
    };
    let [px, py, pz] = position;
    let [q0, q1, q2, q3] = quaternion;
    Ok([px, py, pz, q0, q1, q2, q3])
}

pub(crate) fn placement_matrix_unreported(property: &PropertyRecord) -> Option<FiniteFrame> {
    placement_matrix_value(property).ok()
}

pub(crate) fn placement_components(values: &[f64]) -> Option<FiniteFrame> {
    let [px, py, pz, x, y, z, w] = *<&[f64; 7]>::try_from(values).ok()?;
    placement_components_admitted(FiniteVector::new([px, py, pz, x, y, z, w])?)
}

fn placement_components_admitted(values: FiniteVector<7>) -> Option<FiniteFrame> {
    let [px, py, pz, x, y, z, w] = values.get();
    let scale = x.abs().max(y.abs()).max(z.abs()).max(w.abs());
    if scale == 0.0 {
        return None;
    }
    let (x, y, z, w) = (x / scale, y / scale, z / scale, w / scale);
    let norm = x.hypot(y).hypot(z).hypot(w);
    let (x, y, z, w) = (x / norm, y / norm, z / norm, w / norm);
    FiniteFrame::try_from([
        [
            1.0 - 2.0 * (y * y + z * z),
            2.0 * (x * y - z * w),
            2.0 * (x * z + y * w),
            px,
        ],
        [
            2.0 * (x * y + z * w),
            1.0 - 2.0 * (x * x + z * z),
            2.0 * (y * z - x * w),
            py,
        ],
        [
            2.0 * (x * z - y * w),
            2.0 * (y * z + x * w),
            1.0 - 2.0 * (x * x + y * y),
            pz,
        ],
        [0.0, 0.0, 0.0, 1.0],
    ])
    .ok()
}

/// The tolerance `FreeCAD`'s `Rotation::getYawPitchRoll` uses to detect the
/// poles of the pitch angle of a rotation.
const EPS_PITCH_POLE: f64 = 16.0 * f64::EPSILON;

/// The axis and the angle in radians that a placement property stores for
/// its rotation (`Ox`, `Oy`, `Oz`, and `A`), where it stores them.
pub(crate) fn placement_axis_angle(property: &PropertyRecord) -> Option<([f64; 3], f64)> {
    let [value] = property.values() else {
        return None;
    };
    let number = |name: &str| {
        value
            .attributes
            .get(name)?
            .parse::<f64>()
            .ok()
            .filter(|number| number.is_finite())
    };
    Some(([number("Ox")?, number("Oy")?, number("Oz")?], number("A")?))
}

/// The intrinsic Z, Y, X angles of the rotation in `rows`, as `FreeCAD`'s
/// `Rotation::getYawPitchRoll` gives them: at a pole of the pitch the yaw is zero.
pub(crate) fn yaw_pitch_roll(rows: [[f64; 4]; 4]) -> (f64, f64, f64) {
    let sine = -rows[2][0];
    if (sine - 1.0).abs() <= EPS_PITCH_POLE {
        (
            0.0,
            std::f64::consts::FRAC_PI_2,
            rows[0][1].atan2(rows[1][1]),
        )
    } else if (sine + 1.0).abs() <= EPS_PITCH_POLE {
        (
            0.0,
            -std::f64::consts::FRAC_PI_2,
            (-rows[0][1]).atan2(rows[1][1]),
        )
    } else {
        (
            rows[1][0].atan2(rows[0][0]),
            sine.clamp(-1.0, 1.0).asin(),
            rows[2][1].atan2(rows[2][2]),
        )
    }
}

#[cfg(test)]
mod tests {

    #[test]
    fn numerical_audit_quaternion_frame_is_invariant_under_nonzero_scaling() {
        for scale in [f64::from_bits(1), 1.0e-200, 1.0, 1.0e200, f64::MAX] {
            let frame =
                super::placement_components(&[2.0, 3.0, 4.0, 0.0, 0.0, scale, 0.0]).unwrap();
            assert_eq!(
                frame.rows(),
                [
                    [-1.0, 0.0, 0.0, 2.0],
                    [0.0, -1.0, 0.0, 3.0],
                    [0.0, 0.0, 1.0, 4.0],
                    [0.0, 0.0, 0.0, 1.0],
                ]
            );
        }
        assert!(super::placement_components(&[0.0; 7]).is_none());
    }
}
