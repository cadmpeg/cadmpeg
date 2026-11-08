// SPDX-License-Identifier: Apache-2.0
//! A typed text shape payload written back in its own OCCT grammar (W1).
//!
//! The decoder admits every table of a text payload into typed native
//! records. When the neutral records still agree with those tables, the
//! writer emits the tables themselves, so the output re-decodes to the same
//! payload, topology identities and carriers.

use crate::brep::{
    NurbsCurve2d, Polygon3dRecord, PolygonOnTriangulationRecord, ShapePayload,
    ShapePayloadRecord, Tables, TextCurve, TextCurve2d, TextEdgeRepresentation, TextOrientation,
    TextPointRepresentation, TextShapeKind, TextShapeUse, TextSurface, TextTShapeGeometry,
    TextTopologyVersion,
};
use crate::writer::geometry::nurbs;
use cadmpeg_ir::features::{FinitePoint3, FiniteVector3};
use cadmpeg_ir::math::Vector3;
use cadmpeg_ir::units::FinitePoint2;
use std::fmt::Write as _;

/// The OCCT text of one typed payload, or `None` for a binary or empty
/// carrier or a table the grammar cannot restate exactly.
pub(in crate::writer) fn text(
    payload: &ShapePayloadRecord,
    polygons3d: &[Polygon3dRecord],
    polygons_on_triangulations: &[PolygonOnTriangulationRecord],
    root: Option<cadmpeg_ir::transform::Transform>,
) -> Option<String> {
    let ShapePayload::Text { version, .. } = &payload.payload else {
        return None;
    };
    let version = *version;
    let tables = Tables::from_payload(payload, polygons3d, polygons_on_triangulations)
        .ok()
        .flatten()?;
    let mut out = String::new();
    out.push('\n');
    out.push_str(match version {
        TextTopologyVersion::V1 => "CASCADE Topology V1, (c) Matra-Datavision",
        TextTopologyVersion::V2 => "CASCADE Topology V2, (c) Matra-Datavision",
        TextTopologyVersion::V3 => "CASCADE Topology V3, (c) Open Cascade",
    });
    out.push('\n');
    let root_location = root.map(|_| tables.locations.len() + 1);
    writeln!(
        out,
        "Locations {}",
        tables.locations.len() + usize::from(root.is_some())
    )
    .ok()?;
    for location in tables.locations {
        if location.factors.is_empty() {
            out.push_str("1\n");
            for row in location.transform.affine_rows() {
                writeln!(out, "{} {} {} {}", row[0], row[1], row[2], row[3]).ok()?;
            }
        } else {
            out.push('2');
            for factor in &location.factors {
                write!(out, " {} {}", factor.location(), factor.power()).ok()?;
            }
            out.push_str(" 0\n");
        }
    }
    if let Some(transform) = root {
        out.push_str("1\n");
        for row in transform.affine_rows() {
            writeln!(out, "{} {} {} {}", row[0], row[1], row[2], row[3]).ok()?;
        }
    }
    writeln!(out, "Curve2ds {}", tables.curve2ds.len()).ok()?;
    for curve in tables.curve2ds {
        writeln!(out, "{}", curve2d(curve)?).ok()?;
    }
    writeln!(out, "Curves {}", tables.curves.len()).ok()?;
    for value in tables.curves {
        writeln!(out, "{}", curve(value)?).ok()?;
    }
    writeln!(out, "Polygon3D {}", tables.polygons3d.len()).ok()?;
    for polygon in &tables.polygons3d {
        writeln!(out, "{}", crate::brep::polygon3d_text(polygon)).ok()?;
    }
    writeln!(
        out,
        "PolygonOnTriangulations {}",
        tables.polygons_on_triangulations.len()
    )
    .ok()?;
    for polygon in &tables.polygons_on_triangulations {
        writeln!(out, "{}", crate::brep::polygon_on_triangulation_text(polygon)).ok()?;
    }
    writeln!(out, "Surfaces {}", tables.surfaces.len()).ok()?;
    for value in tables.surfaces {
        writeln!(out, "{}", surface(value)?).ok()?;
    }
    writeln!(out, "Triangulations {}", tables.triangulations.len()).ok()?;
    for triangulation in tables.triangulations {
        let nodes = triangulation.nodes();
        let uv = triangulation.uv_nodes();
        let normals = triangulation.normals();
        write!(
            out,
            "{} {} {}",
            nodes.len(),
            triangulation.triangles().len(),
            u8::from(uv.is_some())
        )
        .ok()?;
        if version == TextTopologyVersion::V3 {
            write!(out, " {}", u8::from(normals.is_some())).ok()?;
        } else if normals.is_some() {
            return None;
        }
        writeln!(out, " {}", triangulation.deflection.get()).ok()?;
        for node in nodes {
            writeln!(out, "{}", point3(*node)).ok()?;
        }
        for node in uv.into_iter().flatten() {
            writeln!(out, "{}", point2(*node)).ok()?;
        }
        // Triangles hold zero-based node positions; the grammar is one-based.
        for triangle in triangulation.triangles() {
            let [first, second, third] = triangle.map(|node| node.checked_add(1));
            writeln!(out, "{} {} {}", first?, second?, third?).ok()?;
        }
        for normal in normals.into_iter().flatten() {
            writeln!(out, "{}", vector3(*normal)).ok()?;
        }
    }
    out.push('\n');
    let count = tables.tshapes.len();
    writeln!(out, "TShapes {count}").ok()?;
    for shape in tables.tshapes.iter() {
        writeln!(out, "{}", kind(shape.kind())).ok()?;
        tshape_geometry(&mut out, &shape.geometry, version)?;
        let flags = shape
            .flags
            .iter()
            .map(|flag| if *flag { '1' } else { '0' })
            .collect::<String>();
        writeln!(out, "{flags}").ok()?;
        for child in &shape.children {
            write!(out, "{} ", shape_use(child, count)?).ok()?;
        }
        out.push_str("*\n\n");
    }
    for use_ in tables.roots {
        let mut use_ = use_.clone();
        if let Some(index) = root_location {
            use_.location = crate::brep::LocationRef::from(index);
        }
        writeln!(out, "{}", shape_use(&use_, count)?).ok()?;
    }
    Some(out)
}

fn kind(kind: TextShapeKind) -> &'static str {
    match kind {
        TextShapeKind::Vertex => "Ve",
        TextShapeKind::Edge => "Ed",
        TextShapeKind::Wire => "Wi",
        TextShapeKind::Face => "Fa",
        TextShapeKind::Shell => "Sh",
        TextShapeKind::Solid => "So",
        TextShapeKind::CompSolid => "CS",
        TextShapeKind::Compound => "Co",
    }
}

/// A use is written as its index counted from the end of the table.
fn shape_use(value: &TextShapeUse, count: usize) -> Option<String> {
    let sign = match value.orientation {
        TextOrientation::Forward => '+',
        TextOrientation::Reversed => '-',
        TextOrientation::Internal => 'i',
        TextOrientation::External => 'e',
    };
    let encoded = count.checked_sub(value.shape)?.checked_add(1)?;
    Some(format!("{sign}{encoded} {}", usize::from(value.location)))
}

fn tshape_geometry(
    out: &mut String,
    geometry: &TextTShapeGeometry,
    version: TextTopologyVersion,
) -> Option<()> {
    match geometry {
        TextTShapeGeometry::Vertex {
            tolerance,
            point,
            representations,
        } => {
            writeln!(out, "{}", tolerance.get()).ok()?;
            writeln!(out, "{}", point3(*point)).ok()?;
            for representation in representations {
                match representation {
                    TextPointRepresentation::Curve3d {
                        parameter,
                        curve,
                        location,
                    } => writeln!(out, "{} 1 {curve} {location}", parameter.get()),
                    TextPointRepresentation::Pcurve {
                        parameter,
                        curve,
                        surface,
                        location,
                    } => writeln!(out, "{} 2 {curve} {surface} {location}", parameter.get()),
                    TextPointRepresentation::Surface {
                        parameter,
                        second_parameter,
                        surface,
                        location,
                    } => writeln!(
                        out,
                        "{} 3 {} {surface} {location}",
                        parameter.get(),
                        second_parameter.get()
                    ),
                }
                .ok()?;
            }
            out.push_str("0 0\n");
        }
        TextTShapeGeometry::Edge {
            tolerance,
            same_parameter,
            same_range,
            degenerated,
            representations,
        } => {
            writeln!(
                out,
                "{} {} {} {}",
                tolerance.get(),
                u8::from(*same_parameter),
                u8::from(*same_range),
                u8::from(*degenerated)
            )
            .ok()?;
            for representation in representations {
                edge_representation(out, representation, version)?;
            }
            out.push_str("0\n");
        }
        TextTShapeGeometry::Face {
            natural_restriction,
            tolerance,
            surface,
            location,
            triangulation,
        } => {
            write!(
                out,
                "{} {} {} {}",
                u8::from(*natural_restriction),
                tolerance.get(),
                surface.map_or(0, |surface| surface.index()),
                usize::from(*location)
            )
            .ok()?;
            if let Some(triangulation) = triangulation {
                write!(out, " 2 {}", triangulation.index()).ok()?;
            }
            out.push('\n');
        }
        TextTShapeGeometry::Wire
        | TextTShapeGeometry::Shell
        | TextTShapeGeometry::Solid
        | TextTShapeGeometry::CompSolid
        | TextTShapeGeometry::Compound => {}
    }
    Some(())
}

fn edge_representation(
    out: &mut String,
    representation: &TextEdgeRepresentation,
    version: TextTopologyVersion,
) -> Option<()> {
    let uv = |endpoints: &Option<[FinitePoint2; 2]>| -> Option<String> {
        match (version, endpoints) {
            (TextTopologyVersion::V2, Some([first, last])) => {
                Some(format!(" {} {}", point2(*first), point2(*last)))
            }
            (TextTopologyVersion::V2, None) => None,
            (_, Some(_)) => None,
            (_, None) => Some(String::new()),
        }
    };
    match representation {
        TextEdgeRepresentation::Curve3d {
            curve,
            location,
            parameter_range,
        } => writeln!(
            out,
            "1 {curve} {location} {} {}",
            parameter_range[0].get(),
            parameter_range[1].get()
        ),
        TextEdgeRepresentation::Pcurve {
            curve,
            surface,
            location,
            parameter_range,
            uv_endpoints,
        } => writeln!(
            out,
            "2 {curve} {surface} {location} {} {}{}",
            parameter_range[0].get(),
            parameter_range[1].get(),
            uv(uv_endpoints)?
        ),
        TextEdgeRepresentation::PcurvePair {
            curves,
            continuity,
            surface,
            location,
            parameter_range,
            uv_endpoints,
        } => writeln!(
            out,
            "3 {} {} {continuity} {surface} {location} {} {}{}",
            curves[0],
            curves[1],
            parameter_range[0].get(),
            parameter_range[1].get(),
            uv(uv_endpoints)?
        ),
        TextEdgeRepresentation::Regularity {
            continuity,
            surfaces,
            locations,
        } => writeln!(
            out,
            "4 {continuity} {} {} {} {}",
            surfaces[0], locations[0], surfaces[1], locations[1]
        ),
        TextEdgeRepresentation::Polygon3d { polygon, location } => {
            writeln!(out, "5 {polygon} {location}")
        }
        TextEdgeRepresentation::PolygonOnTriangulation {
            polygon,
            triangulation,
            location,
        } => writeln!(out, "6 {polygon} {triangulation} {location}"),
        TextEdgeRepresentation::PolygonPair {
            polygons,
            triangulation,
            location,
        } => writeln!(
            out,
            "7 {} {} {triangulation} {location}",
            polygons[0], polygons[1]
        ),
    }
    .ok()
}

fn point3(point: FinitePoint3) -> String {
    let point = point.get();
    format!("{} {} {}", point.x, point.y, point.z)
}

fn vector3(vector: FiniteVector3) -> String {
    let vector = vector.get();
    format!("{} {} {}", vector.x, vector.y, vector.z)
}

fn point2(point: FinitePoint2) -> String {
    let point = point.get();
    format!("{} {}", point.u, point.v)
}

fn cross(left: Vector3, right: Vector3) -> Vector3 {
    Vector3::new(
        left.y.mul_add(right.z, -left.z * right.y),
        left.z.mul_add(right.x, -left.x * right.z),
        left.x.mul_add(right.y, -left.y * right.x),
    )
}

/// An OCCT axis placement: origin, main direction, X direction and the Y
/// direction the decoder reads only for its handedness.
fn frame(origin: FinitePoint3, axis: FiniteVector3, x: FiniteVector3, y_reversed: bool) -> String {
    let y = cross(axis.get(), x.get());
    let y = if y_reversed {
        Vector3::new(-y.x, -y.y, -y.z)
    } else {
        y
    };
    format!(
        "{} {} {} {} {} {}",
        point3(origin),
        vector3(axis),
        vector3(x),
        y.x,
        y.y,
        y.z
    )
}

fn curve2d(value: &TextCurve2d) -> Option<String> {
    Some(match value {
        TextCurve2d::Line { origin, direction } => {
            format!("1 {} {}", point2(*origin), point2(*direction))
        }
        TextCurve2d::Circle {
            center,
            x_axis,
            y_axis,
            radius,
        } => format!(
            "2 {} {} {} {}",
            point2(*center),
            point2(*x_axis),
            point2(*y_axis),
            radius.get()
        ),
        TextCurve2d::Ellipse {
            center,
            x_axis,
            y_axis,
            major_radius,
            minor_radius,
        } => format!(
            "3 {} {} {} {} {}",
            point2(*center),
            point2(*x_axis),
            point2(*y_axis),
            major_radius.get(),
            minor_radius.get()
        ),
        TextCurve2d::Parabola {
            vertex,
            x_axis,
            y_axis,
            focal_distance,
        } => format!(
            "4 {} {} {} {}",
            point2(*vertex),
            point2(*x_axis),
            point2(*y_axis),
            focal_distance.get()
        ),
        TextCurve2d::Hyperbola {
            center,
            x_axis,
            y_axis,
            major_radius,
            minor_radius,
        } => format!(
            "5 {} {} {} {} {}",
            point2(*center),
            point2(*x_axis),
            point2(*y_axis),
            major_radius.get(),
            minor_radius.get()
        ),
        TextCurve2d::Nurbs(value) => nurbs_curve2d(value)?,
        TextCurve2d::Trimmed {
            parameter_range,
            basis,
        } => format!(
            "8 {} {} {}",
            parameter_range[0].get(),
            parameter_range[1].get(),
            curve2d(basis.curve())?
        ),
        TextCurve2d::Offset { distance, basis } => {
            format!("9 {} {}", distance.get(), curve2d(basis.curve())?)
        }
    })
}

fn nurbs_curve2d(value: &NurbsCurve2d) -> Option<String> {
    let knots = value
        .knots
        .iter()
        .map(|knot| knot.get())
        .collect::<Vec<_>>();
    let axis = nurbs::axis(
        &knots,
        value.degree,
        value.control_points.len(),
        value.periodic,
    )?;
    if !nurbs::cyclic_tail(&value.control_points, &axis)
        || value
            .weights
            .as_ref()
            .is_some_and(|weights| !nurbs::cyclic_tail(weights, &axis))
    {
        return None;
    }
    let mut output = format!(
        "7 {} {} {} {} {}",
        u8::from(value.weights.is_some()),
        u8::from(value.periodic),
        value.degree,
        axis.count,
        axis.rows.len()
    );
    for (index, pole) in value.control_points.iter().take(axis.count).enumerate() {
        write!(output, " {}", point2(*pole)).ok()?;
        if let Some(weights) = &value.weights {
            write!(output, " {}", weights.get(index)?.get()).ok()?;
        }
    }
    for (knot, multiplicity) in axis.rows {
        write!(output, " {knot} {multiplicity}").ok()?;
    }
    Some(output)
}

fn curve(value: &TextCurve) -> Option<String> {
    Some(match value {
        TextCurve::Line { origin, direction } => {
            format!("1 {} {}", point3(*origin), vector3(*direction))
        }
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
        TextCurve::Nurbs(value) => nurbs::curve(value)?,
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
            vector3(*direction),
            curve(basis.curve())?
        ),
    })
}

fn surface(value: &TextSurface) -> Option<String> {
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
        TextSurface::Extrusion {
            direction,
            directrix,
        } => format!("6 {} {}", vector3(*direction), curve(directrix.curve())?),
        TextSurface::Revolution {
            axis_origin,
            axis_direction,
            directrix,
        } => format!(
            "7 {} {} {}",
            point3(*axis_origin),
            vector3(*axis_direction),
            curve(directrix.curve())?
        ),
        TextSurface::Nurbs(value) => nurbs::surface(value)?,
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
