// SPDX-License-Identifier: Apache-2.0
//! OCCT text carriers authored from neutral geometry.

use cadmpeg_ir::features::FinitePoint3;
use cadmpeg_ir::geometry::{SolvedCurveGeometry, SolvedSurfaceGeometry};
use cadmpeg_ir::math::{Point3, Vector3};
use cadmpeg_ir::units::OrthonormalFrame3;

pub(super) mod nurbs;

pub(super) fn point(point: Point3) -> String {
    format!("{} {} {}", point.x, point.y, point.z)
}

fn vector(vector: Vector3) -> String {
    format!("{} {} {}", vector.x, vector.y, vector.z)
}

fn frame(origin: FinitePoint3, frame: &OrthonormalFrame3) -> String {
    let z = *frame.axis().as_raw();
    let x = *frame.reference().as_raw();
    let y = Vector3::new(
        z.y * x.z - z.z * x.y,
        z.z * x.x - z.x * x.z,
        z.x * x.y - z.y * x.x,
    );
    format!(
        "{} {} {} {}",
        point(origin.get()),
        vector(z),
        vector(x),
        vector(y)
    )
}

pub(super) fn curve(curve: &SolvedCurveGeometry) -> Option<String> {
    Some(match curve {
        SolvedCurveGeometry::Line(value) => format!(
            "1 {} {}",
            point(value.origin().get()),
            vector(*value.direction().as_raw())
        ),
        SolvedCurveGeometry::Circle(value) => format!(
            "2 {} {}",
            frame(value.center(), value.frame()),
            value.radius().get()
        ),
        SolvedCurveGeometry::Ellipse(value) => format!(
            "3 {} {} {}",
            frame(value.center(), value.frame()),
            value.major_radius().get(),
            value.minor_radius().get()
        ),
        SolvedCurveGeometry::Parabola(value) => format!(
            "4 {} {}",
            frame(value.vertex(), value.frame()),
            value.focal_distance().get()
        ),
        SolvedCurveGeometry::Hyperbola(value) => format!(
            "5 {} {} {}",
            frame(value.center(), value.frame()),
            value.major_radius().get(),
            value.minor_radius().get()
        ),
        SolvedCurveGeometry::Nurbs(value) => nurbs::curve(value)?,
        SolvedCurveGeometry::Composite { .. }
        | SolvedCurveGeometry::Degenerate(_)
        | SolvedCurveGeometry::Polyline(_)
        | SolvedCurveGeometry::Transformed(_)
        | SolvedCurveGeometry::Unknown { .. } => return None,
    })
}

pub(super) fn surface(surface: &SolvedSurfaceGeometry) -> Option<String> {
    Some(match surface {
        SolvedSurfaceGeometry::Plane(value) => {
            format!("1 {}", frame(value.origin(), value.frame()))
        }
        SolvedSurfaceGeometry::Cylinder(value) => format!(
            "2 {} {}",
            frame(value.origin(), value.frame()),
            value.radius().get()
        ),
        SolvedSurfaceGeometry::Cone(value) if value.ratio().get() == 1.0 => format!(
            "3 {} {} {}",
            frame(value.origin(), value.frame()),
            value.radius().get(),
            value.half_angle().get()
        ),
        SolvedSurfaceGeometry::Sphere(value) => format!(
            "4 {} {}",
            frame(value.center(), value.frame()),
            value.radius().get()
        ),
        SolvedSurfaceGeometry::Torus(value) => format!(
            "5 {} {} {}",
            frame(value.center(), value.frame()),
            value.major_radius().get(),
            value.minor_radius().get()
        ),
        SolvedSurfaceGeometry::Nurbs(value) if !value.normal_reversed() => nurbs::surface(value)?,
        SolvedSurfaceGeometry::Nurbs(_)
        | SolvedSurfaceGeometry::Cone(_)
        | SolvedSurfaceGeometry::Polygonal(_)
        | SolvedSurfaceGeometry::Transformed(_)
        | SolvedSurfaceGeometry::Unknown { .. } => return None,
    })
}

/// The OCCT text vocabulary includes the procedural forms `FreeCAD` decodes.
/// Follow current document references, never the source carrier record.
pub(super) fn curve_record(
    ir: &cadmpeg_ir::CadIr,
    value: &cadmpeg_ir::geometry::Curve,
) -> Option<String> {
    curve_at(ir, value, 0)
}
fn curve_at(
    ir: &cadmpeg_ir::CadIr,
    value: &cadmpeg_ir::geometry::Curve,
    depth: usize,
) -> Option<String> {
    use cadmpeg_ir::geometry::{
        CurveGeometry, OffsetSide, ProceduralCurveDefinition as Definition,
    };
    if depth > 64 {
        return None;
    }
    if let CurveGeometry::Procedural { construction, .. } = &value.geometry {
        let definition = ir
            .model
            .procedural_curves
            .iter()
            .find(|item| item.id == *construction)?
            .definition();
        let reference = |id: &cadmpeg_ir::ids::CurveId| {
            ir.model
                .curves
                .iter()
                .find(|item| item.id == *id)
                .and_then(|curve| curve_at(ir, curve, depth + 1))
        };
        let exact = match definition {
            Definition::Subset(value) if *value.sense() => {
                let range = value.parameter_range().endpoints();
                reference(value.source())
                    .map(|basis| format!("8 {} {} {basis}", range[0], range[1]))
            }
            Definition::Offset(value) if value.range().is_none() => {
                let distance = serde_json::to_value(value)
                    .ok()?
                    .get("distance")?
                    .as_f64()?;
                let direction = match value.side() {
                    OffsetSide::PlaneNormal { normal } => Some(normal.get()),
                    OffsetSide::Direction {
                        direction,
                        support: None,
                    } => Some(direction.get()),
                    OffsetSide::Direction {
                        support: Some(_), ..
                    } => None,
                };
                direction.and_then(|direction| {
                    reference(value.source())
                        .map(|basis| format!("9 {distance} {} {basis}", vector(direction)))
                })
            }
            _ => None,
        };
        if exact.is_some() {
            return exact;
        }
    }
    value.geometry.solved().and_then(curve)
}
pub(super) fn surface_record(
    ir: &cadmpeg_ir::CadIr,
    value: &cadmpeg_ir::geometry::Surface,
) -> Option<String> {
    surface_at(ir, value, 0)
}
fn surface_at(
    ir: &cadmpeg_ir::CadIr,
    value: &cadmpeg_ir::geometry::Surface,
    depth: usize,
) -> Option<String> {
    use cadmpeg_ir::geometry::{ProceduralSurfaceDefinition as Definition, SurfaceGeometry};
    if depth > 64 {
        return None;
    }
    if let SurfaceGeometry::Procedural { construction, .. } = &value.geometry {
        let definition = ir
            .model
            .procedural_surfaces
            .iter()
            .find(|item| item.id == *construction)?
            .definition();
        let curve = |id: &cadmpeg_ir::ids::CurveId| {
            ir.model
                .curves
                .iter()
                .find(|item| item.id == *id)
                .and_then(|curve| curve_at(ir, curve, depth + 1))
        };
        let surface = |id: &cadmpeg_ir::ids::SurfaceId| {
            ir.model
                .surfaces
                .iter()
                .find(|item| item.id == *id)
                .and_then(|surface| surface_at(ir, surface, depth + 1))
        };
        let exact = match definition {
            Definition::Extrusion(value) if value.direction().get().norm() > 0.0 => {
                curve(value.directrix())
                    .map(|basis| format!("6 {} {basis}", vector(value.direction().get())))
            }
            Definition::Revolution(value) if *value.transposed() => {
                curve(value.directrix()).map(|basis| {
                    format!(
                        "7 {} {} {basis}",
                        point(value.axis_origin().get()),
                        vector(*value.axis_direction().as_raw())
                    )
                })
            }
            Definition::AxisRevolution(value) => curve(value.directrix()).map(|basis| {
                format!(
                    "7 {} {} {basis}",
                    point(value.axis_origin().get()),
                    vector(*value.axis_direction().as_raw())
                )
            }),
            Definition::Offset(value) => surface(value.support())
                .map(|basis| format!("11 {} {basis}", value.distance().get())),
            Definition::ParallelOffset(value) => surface(value.support())
                .map(|basis| format!("11 {} {basis}", value.distance().get())),
            Definition::Subset(value)
                if value.u_sense() != &Some(false) && value.v_sense() != &Some(false) =>
            {
                let [u, v] = value
                    .parameter_ranges()
                    .map(cadmpeg_ir::geometry::DirectedParameterRange::endpoints);
                surface(value.support())
                    .map(|basis| format!("10 {} {} {} {} {basis}", u[0], u[1], v[0], v[1]))
            }
            _ => None,
        };
        if exact.is_some() {
            return exact;
        }
    }
    value.geometry.solved().and_then(surface)
}

pub(super) fn located_curve(
    ir: &cadmpeg_ir::CadIr,
    value: &cadmpeg_ir::geometry::Curve,
) -> Option<(String, cadmpeg_ir::transform::Transform)> {
    if value.geometry.procedural_construction().is_some() {
        if let Some(row) = curve_record(ir, value) {
            return Some((row, cadmpeg_ir::transform::Transform::identity()));
        }
    }
    if let Some(mut basis) = value.geometry.solved() {
        let mut transform = cadmpeg_ir::transform::Transform::identity();
        while let SolvedCurveGeometry::Transformed(placed) = basis {
            transform = transform.compose(*placed.transform()).ok()?;
            basis = placed.basis();
        }
        if let Some(row) = curve(basis) {
            return Some((row, transform));
        }
    }
    curve_record(ir, value).map(|row| (row, cadmpeg_ir::transform::Transform::identity()))
}

pub(super) fn located_surface(
    ir: &cadmpeg_ir::CadIr,
    value: &cadmpeg_ir::geometry::Surface,
) -> Option<(String, cadmpeg_ir::transform::Transform)> {
    if value.geometry.procedural_construction().is_some() {
        if let Some(row) = surface_record(ir, value) {
            return Some((row, cadmpeg_ir::transform::Transform::identity()));
        }
    }
    if let Some(mut basis) = value.geometry.solved() {
        let mut transform = cadmpeg_ir::transform::Transform::identity();
        while let SolvedSurfaceGeometry::Transformed(placed) = basis {
            transform = transform.compose(*placed.transform()).ok()?;
            basis = placed.basis();
        }
        if let Some(row) = surface(basis) {
            return Some((row, transform));
        }
    }
    surface_record(ir, value).map(|row| (row, cadmpeg_ir::transform::Transform::identity()))
}

/// A decoded placed carrier can share its original base table identity. This
/// is a layout choice only when the current neutral inline basis still agrees.
pub(super) fn curve_layout_key(
    ir: &cadmpeg_ir::CadIr,
    value: &cadmpeg_ir::geometry::Curve,
) -> String {
    if let (Some(home), Some(SolvedCurveGeometry::Transformed(placed))) =
        (value.native_ref.as_deref(), value.geometry.solved())
    {
        let mut bases = ir.model.curves.iter().filter(|curve| {
            curve.id != value.id
                && curve.native_ref.as_deref() == Some(home)
                && curve.geometry.solved() == Some(placed.basis())
        });
        if let Some(base) = bases.next() {
            if bases.next().is_none() {
                return base.id.to_string();
            }
        }
    }
    value.id.to_string()
}
pub(super) fn surface_layout_key(
    ir: &cadmpeg_ir::CadIr,
    value: &cadmpeg_ir::geometry::Surface,
) -> String {
    if let (Some(home), Some(SolvedSurfaceGeometry::Transformed(placed))) =
        (value.native_ref.as_deref(), value.geometry.solved())
    {
        let mut bases = ir.model.surfaces.iter().filter(|surface| {
            surface.id != value.id
                && surface.native_ref.as_deref() == Some(home)
                && surface.geometry.solved() == Some(placed.basis())
        });
        if let Some(base) = bases.next() {
            if bases.next().is_none() {
                return base.id.to_string();
            }
        }
    }
    value.id.to_string()
}

#[cfg(test)]
mod tests;
