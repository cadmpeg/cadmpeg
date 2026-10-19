// SPDX-License-Identifier: Apache-2.0
//! Sketch carriers authored exclusively from neutral geometry.
use super::super::super::xml;
use cadmpeg_ir::sketches::{SketchEntity, SketchGeometryDefinition as Geometry};

pub(super) fn row(entity: &SketchEntity) -> Option<String> {
    let mut attributes = Vec::new();
    let mut children = String::new();
    let (kind, tag) = match entity.geometry.definition().to_raw() {
        Geometry::Point { position } => {
            attributes.extend([
                ("X", position.u.to_string()),
                ("Y", position.v.to_string()),
                ("Z", "0".into()),
            ]);
            ("Part::GeomPoint", "GeomPoint")
        }
        Geometry::Line { start, end } => {
            attributes.extend([
                ("StartX", start.u.to_string()),
                ("StartY", start.v.to_string()),
                ("StartZ", "0".into()),
                ("EndX", end.u.to_string()),
                ("EndY", end.v.to_string()),
                ("EndZ", "0".into()),
            ]);
            ("Part::GeomLineSegment", "LineSegment")
        }
        Geometry::ReferenceLine { .. } => return None,
        Geometry::Circle { center, radius } => {
            center_attributes(&mut attributes, center.u, center.v);
            attributes.push(("Radius", radius.get().to_string()));
            ("Part::GeomCircle", "Circle")
        }
        Geometry::Arc {
            center,
            radius,
            start_angle,
            end_angle,
        } => {
            center_attributes(&mut attributes, center.u, center.v);
            attributes.extend([
                ("Radius", radius.get().to_string()),
                ("StartAngle", start_angle.get().to_string()),
                ("EndAngle", end_angle.get().to_string()),
            ]);
            ("Part::GeomArcOfCircle", "ArcOfCircle")
        }
        Geometry::Ellipse {
            center,
            major_angle,
            radii,
            bounds,
        } => {
            center_attributes(&mut attributes, center.u, center.v);
            attributes.extend([
                ("AngleXU", major_angle.get().to_string()),
                ("MajorRadius", radii.major_radius.get().to_string()),
                ("MinorRadius", radii.minor_radius.get().to_string()),
            ]);
            if let Some([start, end]) = bounds {
                attributes.extend([
                    ("StartAngle", start.get().to_string()),
                    ("EndAngle", end.get().to_string()),
                ]);
                ("Part::GeomArcOfEllipse", "ArcOfEllipse")
            } else {
                ("Part::GeomEllipse", "Ellipse")
            }
        }
        Geometry::Hyperbola {
            center,
            major_angle,
            major_radius,
            minor_radius,
            bounds,
        } => {
            center_attributes(&mut attributes, center.u, center.v);
            attributes.extend([
                ("AngleXU", major_angle.get().to_string()),
                ("MajorRadius", major_radius.get().to_string()),
                ("MinorRadius", minor_radius.get().to_string()),
            ]);
            if let Some([start, end]) = bounds {
                attributes.extend([
                    ("StartAngle", start.to_string()),
                    ("EndAngle", end.to_string()),
                ]);
                ("Part::GeomArcOfHyperbola", "ArcOfHyperbola")
            } else {
                ("Part::GeomHyperbola", "Hyperbola")
            }
        }
        Geometry::Parabola {
            vertex,
            axis_angle,
            focal_length,
            bounds,
        } => {
            center_attributes(&mut attributes, vertex.u, vertex.v);
            attributes.extend([
                ("AngleXU", axis_angle.get().to_string()),
                ("Focal", focal_length.get().to_string()),
            ]);
            if let Some([start, end]) = bounds {
                attributes.extend([
                    ("StartAngle", start.to_string()),
                    ("EndAngle", end.to_string()),
                ]);
                ("Part::GeomArcOfParabola", "ArcOfParabola")
            } else {
                ("Part::GeomParabola", "Parabola")
            }
        }
        Geometry::Nurbs { curve } => {
            let poles = curve.control_points();
            let weights = curve.weights();
            // OCCT's persisted sketch spline carrier requires positive weights.
            if weights
                .as_ref()
                .is_some_and(|weights| weights.iter().any(|weight| weight.get() <= 0.0))
            {
                return None;
            }
            let mut knots: Vec<(f64, usize)> = Vec::new();
            for knot in curve.knots() {
                if let Some((_, count)) =
                    knots.last_mut().filter(|(previous, _)| *previous == *knot)
                {
                    *count += 1;
                } else {
                    knots.push((*knot, 1));
                }
            }
            attributes.extend([
                ("Degree", curve.degree().to_string()),
                ("IsPeriodic", u8::from(curve.periodic()).to_string()),
                ("PolesCount", poles.len().to_string()),
                ("KnotsCount", knots.len().to_string()),
            ]);
            for (index, pole) in poles.iter().enumerate() {
                let point = pole.get();
                children.push_str(&xml::element(
                    "Pole",
                    &[
                        ("X", point.u.to_string()),
                        ("Y", point.v.to_string()),
                        ("Z", "0".into()),
                        (
                            "Weight",
                            weights
                                .as_ref()
                                .map_or(1.0, |weights| weights[index].get())
                                .to_string(),
                        ),
                    ],
                ));
            }
            for (value, count) in knots {
                children.push_str(&xml::element(
                    "Knot",
                    &[("Value", value.to_string()), ("Mult", count.to_string())],
                ));
            }
            ("Part::GeomBSplineCurve", "BSplineCurve")
        }
        Geometry::Text { .. } | Geometry::ExternalReference { .. } | Geometry::Native { .. } => {
            return None;
        }
    };
    let carrier = if children.is_empty() {
        xml::element(tag, &attributes)
    } else {
        let opening = xml::element(tag, &attributes);
        format!(
            "{}{children}</{tag}>",
            opening.trim_end_matches("/>").to_owned() + ">"
        )
    };
    Some(format!(
        "<Geometry type=\"{kind}\">{carrier}<Construction value=\"{}\"/></Geometry>",
        u8::from(entity.construction)
    ))
}
fn center_attributes(attributes: &mut Vec<(&'static str, String)>, x: f64, y: f64) {
    attributes.extend([
        ("CenterX", x.to_string()),
        ("CenterY", y.to_string()),
        ("CenterZ", "0".into()),
        ("NormalX", "0".into()),
        ("NormalY", "0".into()),
        ("NormalZ", "1".into()),
    ]);
}
