// SPDX-License-Identifier: Apache-2.0
//! Text emission from the current canonical aggregate, without neutral replay.
use super::*;
use crate::brep::triangulation::TextTriangulation;
use crate::brep::{TextCurve, TextSurface};
use cadmpeg_ir::{
    features::{FinitePoint3, FiniteVector3},
    math::Vector3,
};

fn vector(value: FiniteVector3) -> String {
    let value = value.get();
    format!("{} {} {}", value.x, value.y, value.z)
}
fn frame(
    origin: FinitePoint3,
    axis: FiniteVector3,
    reference: FiniteVector3,
    reversed: bool,
) -> String {
    let z = axis.get();
    let x = reference.get();
    let sign = if reversed { -1.0 } else { 1.0 };
    let y = Vector3::new(
        sign * (z.y * x.z - z.z * x.y),
        sign * (z.z * x.x - z.x * x.z),
        sign * (z.x * x.y - z.y * x.x),
    );
    format!(
        "{} {} {} {} {} {}",
        super::super::super::super::geometry::point(origin.get()),
        vector(axis),
        vector(reference),
        y.x,
        y.y,
        y.z
    )
}
pub(super) fn curve(value: &TextCurve) -> Option<String> {
    Some(match value {
        TextCurve::Line { origin, direction } => format!(
            "1 {} {}",
            super::super::super::super::geometry::point(origin.get()),
            vector(*direction)
        ),
        TextCurve::Circle {
            center,
            axis,
            ref_direction,
            radius,
        } => format!(
            "2 {} {}",
            frame(*center, *axis, *ref_direction, false),
            radius.get()
        ),
        TextCurve::Ellipse {
            center,
            axis,
            major_direction,
            major_radius,
            minor_radius,
        } => format!(
            "3 {} {} {}",
            frame(*center, *axis, *major_direction, false),
            major_radius.get(),
            minor_radius.get()
        ),
        TextCurve::Parabola {
            vertex,
            axis,
            major_direction,
            focal_distance,
        } => format!(
            "4 {} {}",
            frame(*vertex, *axis, *major_direction, false),
            focal_distance.get()
        ),
        TextCurve::Hyperbola {
            center,
            axis,
            major_direction,
            major_radius,
            minor_radius,
        } => format!(
            "5 {} {} {}",
            frame(*center, *axis, *major_direction, false),
            major_radius.get(),
            minor_radius.get()
        ),
        TextCurve::Nurbs(value) => super::super::super::super::geometry::nurbs::curve(value)?,
        TextCurve::Trimmed {
            parameter_range,
            basis,
        } => format!(
            "8 {} {} {}",
            parameter_range[0].get(),
            parameter_range[1].get(),
            curve(basis.curve())?
        ),
        TextCurve::Offset {
            distance,
            direction,
            basis,
        } => format!(
            "9 {} {} {}",
            distance.get(),
            vector(*direction),
            curve(basis.curve())?
        ),
    })
}
pub(super) fn surface(value: &TextSurface) -> Option<String> {
    Some(match value {
        TextSurface::Plane {
            origin,
            axis,
            u_axis,
            v_reversed,
        } => format!("1 {}", frame(*origin, *axis, *u_axis, *v_reversed)),
        TextSurface::Cylinder {
            origin,
            axis,
            ref_direction,
            radius,
            u_reversed,
        } => format!(
            "2 {} {}",
            frame(*origin, *axis, *ref_direction, *u_reversed),
            radius.get()
        ),
        TextSurface::Cone {
            origin,
            axis,
            ref_direction,
            radius,
            half_angle,
            u_reversed,
        } => format!(
            "3 {} {} {}",
            frame(*origin, *axis, *ref_direction, *u_reversed),
            radius.get(),
            half_angle.get()
        ),
        TextSurface::Sphere {
            center,
            axis,
            ref_direction,
            radius,
            u_reversed,
        } => format!(
            "4 {} {}",
            frame(*center, *axis, *ref_direction, *u_reversed),
            radius.get()
        ),
        TextSurface::Torus {
            center,
            axis,
            ref_direction,
            major_radius,
            minor_radius,
            u_reversed,
        } => format!(
            "5 {} {} {}",
            frame(*center, *axis, *ref_direction, *u_reversed),
            major_radius.get(),
            minor_radius.get()
        ),
        TextSurface::Nurbs(value) if !value.normal_reversed() => {
            super::super::super::super::geometry::nurbs::surface(value)?
        }
        TextSurface::Nurbs(_) => return None,
        TextSurface::Extrusion {
            direction,
            directrix,
        } => format!("6 {} {}", vector(*direction), curve(directrix.curve())?),
        TextSurface::Revolution {
            axis_origin,
            axis_direction,
            directrix,
        } => format!(
            "7 {} {} {}",
            super::super::super::super::geometry::point(axis_origin.get()),
            vector(*axis_direction),
            curve(directrix.curve())?
        ),
        TextSurface::Trimmed {
            parameter_ranges,
            basis,
        } => format!(
            "10 {} {} {} {} {}",
            parameter_ranges[0][0].get(),
            parameter_ranges[0][1].get(),
            parameter_ranges[1][0].get(),
            parameter_ranges[1][1].get(),
            surface(basis.surface())?
        ),
        TextSurface::Offset { distance, basis } => {
            format!("11 {} {}", distance.get(), surface(basis.surface())?)
        }
    })
}
pub(super) fn triangulation(value: &TextTriangulation, version: u8) -> Result<String, CodecError> {
    let mut text = format!(
        "{} {} {}",
        value.nodes().len(),
        value.triangles().len(),
        u8::from(value.uv_nodes().is_some())
    );
    if version == 3 {
        text.push_str(&format!(" {}", u8::from(value.normals().is_some())));
    }
    text.push_str(&format!(" {}", value.deflection.get()));
    for point in value.nodes() {
        text.push_str(&format!(
            " {}",
            super::super::super::super::geometry::point(point.get())
        ));
    }
    if let Some(nodes) = value.uv_nodes() {
        for point in nodes {
            let p = point.get();
            text.push_str(&format!(" {} {}", p.u, p.v));
        }
    }
    for triangle in value.triangles() {
        for index in triangle {
            let index = index
                .checked_add(1)
                .ok_or_else(|| CodecError::malformed("native triangle index overflows"))?;
            text.push_str(&format!(" {index}"));
        }
    }
    if let Some(normals) = value.normals() {
        for normal in normals {
            text.push_str(&format!(" {}", vector(*normal)));
        }
    }
    Ok(text)
}
