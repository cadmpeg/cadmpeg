// SPDX-License-Identifier: Apache-2.0
//! Placement property admission and quaternion frames.
use crate::native::frame::FiniteFrame;
use crate::native::{malformed, PropertyRecord};
use cadmpeg_core::CodecError;
use cadmpeg_ir::math::Vector3;

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
            .filter(|value: &f64| value.is_finite())
    };
    let position = ["Px", "Py", "Pz"]
        .into_iter()
        .map(|name| {
            number(name).ok_or_else(|| {
                CodecError::malformed(format_args!(
                    "placement property {} has an invalid {name} component",
                    property.id
                ))
            })
        })
        .collect::<Result<Vec<_>, _>>()?;
    let quaternion = if value.attributes.contains_key("A") {
        let axis = ["Ox", "Oy", "Oz"]
            .into_iter()
            .map(|name| {
                number(name).ok_or_else(|| {
                    CodecError::malformed(format_args!(
                        "placement property {} has an invalid {name} axis component",
                        property.id
                    ))
                })
            })
            .collect::<Result<Vec<_>, _>>()?;
        let angle = number("A").ok_or_else(|| {
            CodecError::malformed(format_args!(
                "placement property {} has an invalid A angle component",
                property.id
            ))
        })?;
        let axis = Vector3::new(axis[0], axis[1], axis[2]);
        let unit = cadmpeg_ir::features::FiniteVector3::new(axis)
            .and_then(cadmpeg_ir::features::FiniteVector3::unit_nonzero)
            .unwrap_or(Vector3::new(0.0, 0.0, 1.0));
        let (x, y, z) = (unit.x, unit.y, unit.z);
        let half_angle = angle / 2.0;
        let scale = half_angle.sin();
        vec![x * scale, y * scale, z * scale, half_angle.cos()]
    } else {
        ["Q0", "Q1", "Q2", "Q3"]
            .into_iter()
            .map(|name| {
                number(name).ok_or_else(|| {
                    CodecError::malformed(format_args!(
                        "placement property {} has an invalid {name} quaternion component",
                        property.id
                    ))
                })
            })
            .collect::<Result<Vec<_>, _>>()?
    };
    let values = position.into_iter().chain(quaternion).collect::<Vec<_>>();
    let matrix = placement_components(&values).ok_or_else(|| {
        CodecError::malformed(format_args!(
            "placement property {} has an invalid rotation",
            property.id
        ))
    })?;
    Ok(Some(matrix))
}

pub(crate) fn placement_components(values: &[f64]) -> Option<FiniteFrame> {
    let [px, py, pz, x, y, z, w] = *<&[f64; 7]>::try_from(values).ok()?;
    if values.iter().any(|value| !value.is_finite()) {
        return None;
    }
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
