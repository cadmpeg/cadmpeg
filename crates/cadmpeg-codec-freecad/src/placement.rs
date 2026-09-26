// SPDX-License-Identifier: Apache-2.0
//! Placement property admission and quaternion frames.
use crate::native::frame::FiniteFrame;
use crate::native::{malformed, PropertyRecord};
use cadmpeg_core::CodecError;
use cadmpeg_ir::features::FiniteVector3;
use cadmpeg_ir::math::Vector3;
use cadmpeg_ir::scalar::FiniteReal;
use cadmpeg_ir::units::{FiniteVector, UnitVector3};

pub(crate) fn placement_matrix(
    property: &PropertyRecord,
) -> Result<Option<FiniteFrame>, CodecError> {
    if property.type_name != "App::PropertyPlacement" {
        return Err(CodecError::malformed(format_args!(
            "placement property {} has a non-placement runtime type",
            property.id
        )));
    }
    if property.values().len() != 1 {
        return Err(malformed(format!(
            "placement property {} requires one placement value",
            property.id
        )));
    }
    let value = &property.values()[0];
    if value.tag != "PropertyPlacement" {
        return Err(malformed(format!(
            "placement property {} requires one PropertyPlacement value",
            property.id
        )));
    }
    let number = |name: &str| {
        value
            .attributes
            .get(name)
            .and_then(|value| value.parse().ok())
            .and_then(FiniteReal::new)
    };
    let position = ["Px", "Py", "Pz"].map(|name| {
        number(name).ok_or_else(|| {
            CodecError::malformed(format_args!(
                "placement property {} has an invalid {name} component",
                property.id
            ))
        })
    });
    let [px, py, pz] = position;
    let position = [px?, py?, pz?];
    let quaternion = if value.attributes.contains_key("A") {
        let axis = ["Ox", "Oy", "Oz"].map(|name| {
            number(name).ok_or_else(|| {
                CodecError::malformed(format_args!(
                    "placement property {} has an invalid {name} axis component",
                    property.id
                ))
            })
        });
        let [ox, oy, oz] = axis;
        let [ox, oy, oz] = [ox?, oy?, oz?];
        let angle = number("A").ok_or_else(|| {
            CodecError::malformed(format_args!(
                "placement property {} has an invalid A angle component",
                property.id
            ))
        })?;
        let axis = FiniteVector3::from_components(ox, oy, oz);
        let unit = UnitVector3::normalized_finite_nonzero(axis).unwrap_or(UnitVector3::Z_AXIS);
        let unit = Vector3::from(unit);
        let (x, y, z) = (unit.x, unit.y, unit.z);
        let half_angle = angle.get() / 2.0;
        let scale = half_angle.sin();
        let components = [x * scale, y * scale, z * scale, half_angle.cos()];
        let [Some(q0), Some(q1), Some(q2), Some(q3)] = components.map(FiniteReal::new) else {
            return Err(CodecError::malformed(format_args!(
                "placement property {} has an invalid rotation",
                property.id
            )));
        };
        [q0, q1, q2, q3]
    } else {
        let components = ["Q0", "Q1", "Q2", "Q3"].map(|name| {
            number(name).ok_or_else(|| {
                CodecError::malformed(format_args!(
                    "placement property {} has an invalid {name} quaternion component",
                    property.id
                ))
            })
        });
        let [q0, q1, q2, q3] = components;
        [q0?, q1?, q2?, q3?]
    };
    let [px, py, pz] = position;
    let [q0, q1, q2, q3] = quaternion;
    let values = FiniteVector::from([px, py, pz, q0, q1, q2, q3]);
    let matrix = placement_components_admitted(values).ok_or_else(|| {
        CodecError::malformed(format_args!(
            "placement property {} has an invalid rotation",
            property.id
        ))
    })?;
    Ok(Some(matrix))
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
