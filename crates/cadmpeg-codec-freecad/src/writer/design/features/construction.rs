// SPDX-License-Identifier: Apache-2.0
//! Datum frames and standalone geometric construction features.

use super::super::super::xml::{self, Object};
use super::properties::{angle, boolean, enumeration, length, link, number, tag, unsupported};
use cadmpeg_core::CodecError;
use cadmpeg_ir::transform::Transform;
use serde_json::Value;
use std::collections::HashMap;

pub(super) fn coordinates(value: &Value) -> Result<cadmpeg_ir::math::Vector3, CodecError> {
    Ok(cadmpeg_ir::math::Vector3::new(
        number(&value["x"], "x coordinate")?,
        number(&value["y"], "y coordinate")?,
        number(&value["z"], "z coordinate")?,
    ))
}
pub(super) fn unit(
    value: cadmpeg_ir::math::Vector3,
) -> Result<cadmpeg_ir::math::Vector3, CodecError> {
    cadmpeg_ir::units::UnitVector3::normalized(value)
        .map(|value| *value.as_raw())
        .ok_or_else(|| {
            CodecError::UnsupportedContent("datum direction cannot be normalized".into())
        })
}
pub(super) fn place(
    object: &mut Object,
    origin: cadmpeg_ir::math::Vector3,
    x: cadmpeg_ir::math::Vector3,
    y: cadmpeg_ir::math::Vector3,
    z: cadmpeg_ir::math::Vector3,
) -> Result<(), CodecError> {
    let transform = Transform::affine([
        [x.x, y.x, z.x, origin.x],
        [x.y, y.y, z.y, origin.y],
        [x.z, y.z, z.z, origin.z],
    ])
    .ok_or_else(|| CodecError::malformed("feature frame is non-finite"))?;
    object.set("Placement", super::super::placement(transform)?);
    Ok(())
}
pub(super) fn axis_placement(
    object: &mut Object,
    origin: &Value,
    direction: &Value,
) -> Result<(), CodecError> {
    let z = unit(coordinates(direction)?)?;
    let reference = if z.x.abs() < 0.8 {
        cadmpeg_ir::math::Vector3::new(1.0, 0.0, 0.0)
    } else {
        cadmpeg_ir::math::Vector3::new(0.0, 1.0, 0.0)
    };
    let dot = reference.dot(z);
    let x = unit(cadmpeg_ir::math::Vector3::new(
        reference.x - dot * z.x,
        reference.y - dot * z.y,
        reference.z - dot * z.z,
    ))?;
    let y = z.cross(x);
    place(object, coordinates(origin)?, x, y, z)
}
pub(super) fn frame_placement(object: &mut Object, value: &Value) -> Result<(), CodecError> {
    let origin = coordinates(&value["origin"])?;
    if value["normal"].is_null() {
        place(
            object,
            origin,
            coordinates(&value["x_axis"])?,
            coordinates(&value["y_axis"])?,
            coordinates(&value["z_axis"])?,
        )
    } else {
        let z = unit(coordinates(&value["normal"])?)?;
        let x = unit(coordinates(&value["u_axis"])?)?;
        place(object, origin, x, z.cross(x), z)
    }
}
pub(super) fn construction(
    value: &Value,
    object: &mut Object,
    names: &HashMap<String, String>,
    entries: &mut Vec<(String, Vec<u8>)>,
) -> Result<(), CodecError> {
    match tag(value) {
        "line_segment" => {
            object.kind = "Part::Line".into();
            for (point, suffix) in [("start", "1"), ("end", "2")] {
                for (axis, coordinate) in [("X", "x"), ("Y", "y"), ("Z", "z")] {
                    length(
                        object,
                        &format!("{axis}{suffix}"),
                        number(&value["segment"][point][coordinate], "line endpoint")?,
                    );
                }
            }
        }
        "circular_arc" | "elliptic_arc" => {
            let arc = &value["arc"];
            object.kind = if tag(value) == "circular_arc" {
                "Part::Circle"
            } else {
                "Part::Ellipse"
            }
            .into();
            if tag(value) == "elliptic_arc" {
                let z = unit(coordinates(&arc["normal"])?)?;
                let x = unit(coordinates(&arc["major_axis"])?)?;
                place(object, coordinates(&arc["center"])?, x, z.cross(x), z)?;
            } else {
                axis_placement(object, &arc["center"], &arc["normal"])?;
            }
            if tag(value) == "circular_arc" {
                length(object, "Radius", number(&arc["radius"], "circle radius")?);
            } else {
                length(
                    object,
                    "MajorRadius",
                    number(&arc["major_radius"], "ellipse major radius")?,
                );
                length(
                    object,
                    "MinorRadius",
                    number(&arc["minor_radius"], "ellipse minor radius")?,
                );
            }
            angle(object, "Angle1", number(&arc["start_angle"], "arc start")?);
            angle(object, "Angle2", number(&arc["end_angle"], "arc end")?);
        }
        "polyline" => {
            object.kind = "Part::Polygon".into();
            let points = value["chain"]["points"]
                .as_array()
                .ok_or_else(|| CodecError::UnsupportedContent("polyline points absent".into()))?;
            let count = u32::try_from(points.len()).map_err(|_| {
                CodecError::InvalidInput("(O-capacity) polygon node count exceeds u32".into())
            })?;
            let mut bytes = count.to_le_bytes().to_vec();
            for point in points {
                let point = coordinates(point)?;
                for coordinate in [point.x, point.y, point.z] {
                    bytes.extend_from_slice(&coordinate.to_le_bytes());
                }
            }
            let file = format!("{}_Nodes.bin", object.name);
            object.set(
                "Nodes",
                xml::property(
                    "Nodes",
                    "App::PropertyVectorList",
                    &xml::element("VectorList", &[("file", file.clone())]),
                ),
            );
            entries.push((file, bytes));
            boolean(
                object,
                "Close",
                value["chain"]["closed"].as_bool().unwrap_or(false),
            );
        }
        "regular_polygon_curve" => {
            object.kind = "Part::RegularPolygon".into();
            length(
                object,
                "Circumradius",
                number(&value["circumradius"], "polygon radius")?,
            );
            object.set(
                "Polygon",
                xml::scalar(
                    "Polygon",
                    "App::PropertyInteger",
                    "Integer",
                    number(&value["sides"], "polygon sides")?,
                ),
            );
        }
        "planar_patch" => {
            object.kind = "Part::Plane".into();
            length(object, "Length", number(&value["length"], "patch length")?);
            length(object, "Width", number(&value["width"], "patch width")?);
        }
        "helix" => {
            let shape = &value["shape"];
            object.kind = if tag(shape) == "spiral" {
                "Part::Spiral"
            } else {
                "Part::Helix"
            }
            .into();
            length(object, "Radius", number(&value["radius"], "helix radius")?);
            if !value["segment_turns"].is_null() {
                object.set(
                    "SegmentLength",
                    xml::scalar(
                        "SegmentLength",
                        "App::PropertyQuantityConstraint",
                        "Float",
                        number(&value["segment_turns"], "helix segment turns")?,
                    ),
                );
            }

            axis_placement(object, &value["axis_origin"], &value["axis_direction"])?;
            if tag(shape) == "spiral" {
                length(
                    object,
                    "Growth",
                    number(&shape["radial_growth"], "spiral growth")?,
                );
                length(
                    object,
                    "Rotations",
                    number(&shape["revolutions"], "spiral turns")?,
                );
            } else {
                let law = &shape["law"];
                let (pitch, height) = match tag(law) {
                    "height_pitch" => (
                        number(&law["pitch"], "helix pitch")?,
                        number(&law["height"], "helix height")?,
                    ),
                    "pitch_revolutions" => {
                        let pitch = number(&law["pitch"], "helix pitch")?;
                        (
                            pitch,
                            pitch * number(&law["revolutions"], "helix revolutions")?,
                        )
                    }
                    "height_revolutions" => {
                        let height = number(&law["height"], "helix height")?;
                        (
                            height / number(&law["revolutions"], "helix revolutions")?,
                            height,
                        )
                    }
                    _ => return unsupported("helix travel law has no FreeCAD representation"),
                };
                if pitch <= 0.0 || height <= 0.0 || !pitch.is_finite() || !height.is_finite() {
                    return unsupported(
                        "FreeCAD Part::Helix requires positive finite height and pitch",
                    );
                }
                length(object, "Pitch", pitch);
                length(object, "Height", height);
                angle(
                    object,
                    "Angle",
                    if tag(shape) == "conical" {
                        number(&shape["cone_angle"], "helix cone angle")?
                    } else {
                        0.0
                    },
                );
                enumeration(
                    object,
                    "LocalCoord",
                    usize::from(value["clockwise"].as_bool().unwrap_or(false)),
                );
                enumeration(
                    object,
                    "Style",
                    usize::from(value["construction_style"] == "corrected"),
                );
            }
        }
        "scale" => {
            if !value["center"].is_null() && tag(&value["center"]) != "model_origin" {
                return unsupported("Part::Scale uses the model origin");
            }
            object.kind = "Part::Scale".into();
            link(object, "Base", &value["bodies"], names)?;
            let factors = &value["factors"];
            boolean(object, "Uniform", tag(factors) == "uniform");
            if tag(factors) == "uniform" {
                length(
                    object,
                    "UniformScale",
                    number(&factors["factor"], "scale factor")?,
                );
            } else {
                for (index, field) in ["XScale", "YScale", "ZScale"].into_iter().enumerate() {
                    length(
                        object,
                        field,
                        number(&factors["factors"][index], "scale factor")?,
                    );
                }
            }
        }
        _ => return unsupported("construction feature has no FreeCAD representation"),
    }
    Ok(())
}
