// SPDX-License-Identifier: Apache-2.0
//! Placement property admission and quaternion frames.
use crate::native::frame::FiniteFrame;
use crate::native::PropertyRecord;
use crate::resource::retained_format;
use cadmpeg_core::decode::DecodeContext;
use cadmpeg_core::CodecError;
use cadmpeg_ir::features::FiniteVector3;
use cadmpeg_ir::math::Vector3;
use cadmpeg_ir::scalar::FiniteReal;
use cadmpeg_ir::units::{FiniteVector, UnitVector3};

pub(crate) fn placement_matrix(
    ctx: &DecodeContext<'_>,
    property: &PropertyRecord,
) -> Result<Option<FiniteFrame>, CodecError> {
    match placement_matrix_value(property) {
        Ok(value) => Ok(Some(value)),
        Err(issue) => Err(CodecError::Malformed(retained_format(
            ctx,
            format_args!("placement property {} {issue}", property.id),
            "FreeCAD placement error",
        )?)),
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
    let values = FiniteVector::from([px, py, pz, q0, q1, q2, q3]);
    placement_components_admitted(values).ok_or(PlacementIssue::Rotation)
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

#[cfg(test)]
mod tests {
    use crate::native::{PropertyBody, PropertyFamily, PropertyRecord, RetainedXml, ValueRecord};
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};
    use cadmpeg_core::CodecError;

    fn property(runtime_type: &str, values: Vec<ValueRecord>) -> PropertyRecord {
        PropertyRecord {
            id: "source-property".into(),
            owner: "owner".into(),
            name: "Placement".into(),
            type_name: runtime_type.into(),
            family: PropertyFamily::Unknown,
            status: None,
            body: PropertyBody::Persisted {
                values,
                links: Vec::new(),
                side_entries: Vec::new(),
                dynamic: None,
            },
            order: 0,
            xml: RetainedXml::from_text("<Property/>".into(), 0).expect("valid XML span"),
        }
    }

    fn value(tag: &str, attributes: &[(&str, &str)]) -> ValueRecord {
        ValueRecord {
            tag: tag.into(),
            order: 0,
            attributes: attributes
                .iter()
                .map(|(key, value)| ((*key).into(), (*value).into()))
                .collect(),
            text: None,
            raw_xml: String::new(),
        }
    }

    fn assert_placement_issue(property: &PropertyRecord, expected: &str) {
        let arena = DecodeArena::new();
        let policy = DecodePolicy::default();
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("empty root");
        assert!(matches!(super::placement_matrix(&ctx, property),
            Err(CodecError::Malformed(message)) if message == expected));

        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::default();
        policy.limits.max_retained_bytes =
            u64::try_from(expected.len() - 1).expect("message length fits");
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("empty root");
        assert!(matches!(super::placement_matrix(&ctx, property),
            Err(CodecError::ResourceLimit(limit)) if limit.operation == "FreeCAD placement error"));
        assert!(super::placement_matrix_unreported(property).is_none());
    }

    #[test]
    fn placement_runtime_type_error_refuses_at_retained_limit() {
        assert_placement_issue(
            &property("App::PropertyString", Vec::new()),
            "placement property source-property has a non-placement runtime type",
        );
    }

    #[test]
    fn placement_value_count_error_refuses_at_retained_limit() {
        assert_placement_issue(
            &property("App::PropertyPlacement", Vec::new()),
            "placement property source-property requires one placement value",
        );
    }

    #[test]
    fn placement_value_tag_error_refuses_at_retained_limit() {
        assert_placement_issue(
            &property("App::PropertyPlacement", vec![value("Other", &[])]),
            "placement property source-property requires one PropertyPlacement value",
        );
    }

    #[test]
    fn placement_position_error_refuses_at_retained_limit() {
        assert_placement_issue(
            &property(
                "App::PropertyPlacement",
                vec![value("PropertyPlacement", &[])],
            ),
            "placement property source-property has an invalid Px component",
        );
    }

    #[test]
    fn placement_axis_error_refuses_at_retained_limit() {
        assert_placement_issue(
            &property(
                "App::PropertyPlacement",
                vec![value(
                    "PropertyPlacement",
                    &[("Px", "0"), ("Py", "0"), ("Pz", "0"), ("A", "1")],
                )],
            ),
            "placement property source-property has an invalid Ox axis component",
        );
    }

    #[test]
    fn placement_angle_error_refuses_at_retained_limit() {
        assert_placement_issue(
            &property(
                "App::PropertyPlacement",
                vec![value(
                    "PropertyPlacement",
                    &[
                        ("Px", "0"),
                        ("Py", "0"),
                        ("Pz", "0"),
                        ("Ox", "0"),
                        ("Oy", "0"),
                        ("Oz", "1"),
                        ("A", "bad"),
                    ],
                )],
            ),
            "placement property source-property has an invalid A angle component",
        );
    }

    #[test]
    fn placement_quaternion_error_refuses_at_retained_limit() {
        assert_placement_issue(
            &property(
                "App::PropertyPlacement",
                vec![value(
                    "PropertyPlacement",
                    &[("Px", "0"), ("Py", "0"), ("Pz", "0")],
                )],
            ),
            "placement property source-property has an invalid Q0 quaternion component",
        );
    }

    #[test]
    fn placement_rotation_error_refuses_at_retained_limit() {
        assert_placement_issue(
            &property(
                "App::PropertyPlacement",
                vec![value(
                    "PropertyPlacement",
                    &[
                        ("Px", "0"),
                        ("Py", "0"),
                        ("Pz", "0"),
                        ("Q0", "0"),
                        ("Q1", "0"),
                        ("Q2", "0"),
                        ("Q3", "0"),
                    ],
                )],
            ),
            "placement property source-property has an invalid rotation",
        );
    }

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
