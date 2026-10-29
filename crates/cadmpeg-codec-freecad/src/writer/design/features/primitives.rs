// SPDX-License-Identifier: Apache-2.0
//! Analytic primitive dimensions and angular bounds.

use super::super::super::xml::{self, Object};
use super::properties::{angle, length, number, tag, unsupported};
use cadmpeg_core::CodecError;
use serde_json::Value;

pub(super) fn primitive(value: &Value, object: &mut Object) -> Result<(), CodecError> {
    let shape = tag(value);
    let class = match shape {
        "box" => "Box",
        "cylinder" => "Cylinder",
        "cone" => "Cone",
        "sphere" => "Sphere",
        "torus" => "Torus",
        "ellipsoid" => "Ellipsoid",
        "prism" => "Prism",
        "wedge" => "Wedge",
        _ => return unsupported("primitive has no FreeCAD type"),
    };
    object.kind = format!("Part::{class}");
    let dimensions: &[(&str, &str)] = match shape {
        "box" => &[
            ("length", "Length"),
            ("width", "Width"),
            ("height", "Height"),
        ],
        "cylinder" => &[("radius", "Radius"), ("height", "Height")],
        "cone" => &[
            ("radius1", "Radius1"),
            ("radius2", "Radius2"),
            ("height", "Height"),
        ],
        "sphere" => &[("radius", "Radius")],
        "torus" => &[("major_radius", "Radius1"), ("minor_radius", "Radius2")],
        "ellipsoid" => &[
            ("z_radius", "Radius1"),
            ("x_radius", "Radius2"),
            ("y_radius", "Radius3"),
        ],
        "prism" => &[("circumradius", "Circumradius"), ("height", "Height")],
        "wedge" => &[
            ("xmin", "Xmin"),
            ("ymin", "Ymin"),
            ("zmin", "Zmin"),
            ("xmax", "Xmax"),
            ("ymax", "Ymax"),
            ("zmax", "Zmax"),
            ("x2min", "X2min"),
            ("z2min", "Z2min"),
            ("x2max", "X2max"),
            ("z2max", "Z2max"),
        ],
        _ => &[],
    };
    for (source, target) in dimensions {
        length(object, target, number(&value[*source], source)?);
    }
    for (source, target) in [
        ("angle", "Angle"),
        ("latitude1", "Angle1"),
        ("latitude2", "Angle2"),
        ("longitude", "Angle3"),
    ] {
        if !value[source].is_null() {
            angle(object, target, number(&value[source], source)?);
        }
    }
    if shape == "prism" {
        object.set(
            "Polygon",
            xml::scalar(
                "Polygon",
                "App::PropertyInteger",
                "Integer",
                number(&value["sides"], "prism sides")?,
            ),
        );
    }
    Ok(())
}
