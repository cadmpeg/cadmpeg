// SPDX-License-Identifier: Apache-2.0
//! Exact analytic sections selected by Parasolid support references.

use cadmpeg_core::decode::DecodeContext;
use cadmpeg_core::CodecError;
use cadmpeg_ir::geometry::analytic::{CircleCurve, EllipseCurve, LineCurve, PlaneSurface};
use cadmpeg_ir::geometry::{
    CurveGeometry, SolvedCurveGeometry, SolvedSurfaceGeometry, SurfaceGeometry,
};
use cadmpeg_ir::math::{Point3, Vector3};
use cadmpeg_ir::units::UnitVector3;
use cadmpeg_parasolid::records::primary::Node;

use crate::geometry::{self, Nodes};

const EPS_PLANAR_SUPPORT_MM: f64 = 1.0e-9;

const MM_PER_METRE: f64 = cadmpeg_ir::scalar::Magnification::MILLIMETERS_PER_METER.get();

pub(crate) fn support_kinds(nodes: &Nodes<'_>, node: Node<'_>) -> Vec<u16> {
    node.pointer_array::<2>("surface")
        .into_iter()
        .flatten()
        .filter_map(|index| nodes.get(&index).map(Node::kind))
        .collect()
}

pub(crate) fn planar_nurbs_support(nodes: &Nodes<'_>, node: Node<'_>) -> bool {
    node.pointer_array::<2>("surface")
        .into_iter()
        .flatten()
        .filter_map(|index| nodes.get(&index).copied())
        .filter(|support| support.kind() == cadmpeg_parasolid::records::node_type::B_SURFACE)
        .filter_map(|support| geometry::surface(nodes, support))
        .any(|surface| surface.solved().and_then(plane_support).is_some())
}

pub(crate) fn nurbs_isoline(
    ctx: &DecodeContext<'_>,
    nodes: &Nodes<'_>,
    node: Node<'_>,
) -> Result<Option<CurveGeometry>, CodecError> {
    let Some(supports) = node.pointer_array::<2>("surface") else {
        return Ok(None);
    };
    let surfaces = supports.map(|index| {
        nodes
            .get(&index)
            .copied()
            .and_then(|node| geometry::surface(nodes, node))
    });
    let [Some(SurfaceGeometry::Solved(first)), Some(SurfaceGeometry::Solved(second))] = surfaces
    else {
        return Ok(None);
    };
    let ((SolvedSurfaceGeometry::Plane(plane), SolvedSurfaceGeometry::Nurbs(surface))
    | (SolvedSurfaceGeometry::Nurbs(surface), SolvedSurfaceGeometry::Plane(plane))) =
        (&first, &second)
    else {
        return Ok(None);
    };
    let Ok(witnesses) = chart_direction(nodes, node) else {
        return Ok(None);
    };
    Ok(crate::isocurve::interior(ctx, plane, surface, witnesses)?
        .map(|curve| CurveGeometry::Solved(SolvedCurveGeometry::Nurbs(curve))))
}

pub(crate) fn curve(nodes: &Nodes<'_>, node: Node<'_>) -> Result<CurveGeometry, &'static str> {
    let supports = node
        .pointer_array::<2>("surface")
        .ok_or("support references are absent")?;
    let [first, second] = supports.map(|index| nodes.get(&index).copied());
    let [Some(first), Some(second)] = [first, second] else {
        return Err("support references do not resolve");
    };
    let support_reversed =
        (first.char("sense") == Some(b'-')) != (second.char("sense") == Some(b'-'));
    let [Some(SurfaceGeometry::Solved(first)), Some(SurfaceGeometry::Solved(second))] = [
        geometry::surface(nodes, first),
        geometry::surface(nodes, second),
    ] else {
        return Err("support carriers are unsupported");
    };
    let witnesses = chart_direction(nodes, node).ok();
    let mut geometry = solve_supports(&first, &second, witnesses)?;
    if witnesses.is_none() && support_reversed {
        match &mut geometry {
            SolvedCurveGeometry::Line(curve) => curve.reverse_parameterization(),
            SolvedCurveGeometry::Circle(curve) => curve.reverse_parameterization(),
            SolvedCurveGeometry::Ellipse(curve) => curve.reverse_parameterization(),
            _ => return Err("section parameter reversal is unsupported"),
        }
    }
    Ok(CurveGeometry::Solved(geometry))
}

fn chart_direction(nodes: &Nodes<'_>, node: Node<'_>) -> Result<[Point3; 2], &'static str> {
    let chart_index = node.pointer("chart").ok_or("chart reference is absent")?;
    let chart = nodes
        .get(&chart_index)
        .ok_or("chart reference does not resolve")?;
    let scale = chart.real("base_scale").ok_or("chart scale is absent")?;
    if chart.is_null("base_scale") == Some(true) || !scale.is_finite() || scale == 0.0 {
        return Err("chart scale is invalid");
    }
    let chart_points = chart.hvec_points("hvec").ok_or("chart points are absent")?;
    let points: Option<Vec<_>> = chart_points
        .iter()
        .map(|p| {
            cadmpeg_ir::features::geometry::FinitePoint3::new(Point3::new(
                p[0] * MM_PER_METRE,
                p[1] * MM_PER_METRE,
                p[2] * MM_PER_METRE,
            ))
            .map(cadmpeg_ir::features::geometry::FinitePoint3::get)
        })
        .collect();
    let points = points.ok_or("chart points are nonfinite")?;
    let start = *points.first().ok_or("chart is empty")?;
    let next = *points
        .iter()
        .skip(1)
        .find(|point| **point != start)
        .ok_or("chart has no parameter direction")?;
    Ok(if scale > 0.0 {
        [start, next]
    } else {
        [next, start]
    })
}

pub(crate) fn frame_from_chart(nodes: &Nodes<'_>, node: Node<'_>) -> bool {
    chart_direction(nodes, node).is_ok()
}

fn solve_supports(
    first: &SolvedSurfaceGeometry,
    second: &SolvedSurfaceGeometry,
    points: Option<[Point3; 2]>,
) -> Result<SolvedCurveGeometry, &'static str> {
    let (plane, other) = if let Some(plane) = plane_support(first) {
        (plane, second)
    } else {
        let mut plane = plane_support(second).ok_or("supports have no planar carrier")?;
        if points.is_none() {
            plane = PlaneSurface::try_new(
                plane.origin().get(),
                plane.frame().axis().as_raw().scale(-1.0),
                *plane.frame().reference().as_raw(),
            )
            .map_err(|_| "support plane frame is invalid")?;
        }
        (plane, first)
    };
    section(&plane, other, points).ok_or(if points.is_some() {
        "planar support section is unsupported"
    } else {
        "planar support section needs chart witnesses"
    })
}

fn plane_support(surface: &SolvedSurfaceGeometry) -> Option<PlaneSurface> {
    match surface {
        SolvedSurfaceGeometry::Plane(plane) => Some(*plane),
        SolvedSurfaceGeometry::Nurbs(surface) => {
            let plane = planar_poles(&surface.poles())?;
            let origin = surface.pole(0, 0)?.get();
            let u = (1..surface.u_count())
                .filter_map(|index| surface.pole(index, 0))
                .map(|point| point.get().vector_from(origin))
                .max_by(|first, second| first.norm().total_cmp(&second.norm()))?;
            let v = (1..surface.v_count())
                .filter_map(|index| surface.pole(0, index))
                .map(|point| point.get().vector_from(origin))
                .max_by(|first, second| first.norm().total_cmp(&second.norm()))?;
            let normal = if surface.normal_reversed() {
                u.cross(v).scale(-1.0)
            } else {
                u.cross(v)
            };
            if plane.frame().axis().as_raw().dot(normal) < 0.0 {
                PlaneSurface::try_new(
                    plane.origin().get(),
                    plane.frame().axis().as_raw().scale(-1.0),
                    *plane.frame().reference().as_raw(),
                )
                .ok()
            } else {
                Some(plane)
            }
        }
        _ => None,
    }
}

fn planar_poles(points: &[cadmpeg_ir::features::geometry::FinitePoint3]) -> Option<PlaneSurface> {
    let origin = points.first()?.get();
    let direction = points
        .iter()
        .map(|point| point.get().vector_from(origin))
        .max_by(|first, second| first.norm().total_cmp(&second.norm()))?;
    let reference = UnitVector3::normalized(direction)?;
    let cross = points
        .iter()
        .map(|point| reference.as_raw().cross(point.get().vector_from(origin)))
        .max_by(|first, second| first.norm().total_cmp(&second.norm()))?;
    let normal = UnitVector3::normalized(cross)?;
    if points.iter().any(|point| {
        point.get().vector_from(origin).dot(*normal.as_raw()).abs() > EPS_PLANAR_SUPPORT_MM
    }) {
        return None;
    }
    PlaneSurface::try_new(origin, *normal.as_raw(), *reference.as_raw()).ok()
}

fn oriented_normal(normal: Vector3, center: Point3, points: [Point3; 2]) -> Vector3 {
    if points[0]
        .vector_from(center)
        .cross(points[1].vector_from(center))
        .dot(normal)
        < 0.0
    {
        normal.scale(-1.0)
    } else {
        normal
    }
}

fn section(
    plane: &PlaneSurface,
    other: &SolvedSurfaceGeometry,
    points: Option<[Point3; 2]>,
) -> Option<SolvedCurveGeometry> {
    let normal = *plane.frame().axis().as_raw();
    match other {
        SolvedSurfaceGeometry::Cylinder(cylinder) => {
            let axis = *cylinder.frame().axis().as_raw();
            let origin = cylinder.origin().get();
            let cosine = normal.dot(axis);
            let height = plane.origin().get().vector_from(origin).dot(normal);
            let radius = cylinder.radius().get();
            if cosine == 0.0 {
                let points = points?;
                let cross = axis.cross(normal);
                let half_width = (radius * radius - height * height).sqrt();
                let centre = origin.translated(normal, height);
                let candidates = [
                    centre.translated(cross, half_width),
                    centre.translated(cross, -half_width),
                ];
                let radial_distance = |point: Point3| {
                    let delta = points[0].vector_from(point);
                    (delta - axis.scale(delta.dot(axis))).norm()
                };
                let origin = if radial_distance(candidates[0]) <= radial_distance(candidates[1]) {
                    candidates[0]
                } else {
                    candidates[1]
                };
                let direction = if points[1].vector_from(points[0]).dot(axis) < 0.0 {
                    axis.scale(-1.0)
                } else {
                    axis
                };
                return LineCurve::try_new(origin, direction)
                    .ok()
                    .map(SolvedCurveGeometry::Line);
            }
            let centre = origin.translated(axis, height / cosine);
            let projected = axis - normal.scale(cosine);
            let reference = UnitVector3::normalized(projected)
                .map_or(*cylinder.frame().reference().as_raw(), |value| {
                    *value.as_raw()
                });
            let normal = points.map_or(normal, |points| oriented_normal(normal, centre, points));
            EllipseCurve::try_new(centre, normal, reference, radius / cosine.abs(), radius)
                .ok()
                .map(SolvedCurveGeometry::Ellipse)
        }
        SolvedSurfaceGeometry::Sphere(sphere) => {
            let centre = sphere.center().get();
            let height = plane.origin().get().vector_from(centre).dot(normal);
            let centre = centre.translated(normal, height);
            let radius = (sphere.radius().get().powi(2) - height * height).sqrt();
            let normal = points.map_or(normal, |points| oriented_normal(normal, centre, points));
            CircleCurve::try_new(centre, normal, *plane.frame().reference().as_raw(), radius)
                .ok()
                .map(SolvedCurveGeometry::Circle)
        }
        SolvedSurfaceGeometry::Plane(second) => {
            let second_normal = *second.frame().axis().as_raw();
            let cross = normal.cross(second_normal);
            let square = cross.dot(cross);
            if square == 0.0 {
                return None;
            }
            let first_height = plane
                .origin()
                .get()
                .vector_from(Point3::new(0.0, 0.0, 0.0))
                .dot(normal);
            let second_height = second
                .origin()
                .get()
                .vector_from(Point3::new(0.0, 0.0, 0.0))
                .dot(second_normal);
            let location = (second_normal.scale(first_height) - normal.scale(second_height))
                .cross(cross)
                .scale(1.0 / square);
            let origin = Point3::new(location.x, location.y, location.z);
            let direction = *UnitVector3::normalized(cross)?.as_raw();
            let direction = if points
                .is_some_and(|points| points[1].vector_from(points[0]).dot(direction) < 0.0)
            {
                direction.scale(-1.0)
            } else {
                direction
            };
            LineCurve::try_new(origin, direction)
                .ok()
                .map(SolvedCurveGeometry::Line)
        }
        SolvedSurfaceGeometry::Nurbs(surface) => {
            crate::isocurve::section(plane, surface, points?).map(SolvedCurveGeometry::Nurbs)
        }
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::{planar_poles, plane_support, section, solve_supports};
    use cadmpeg_ir::geometry::analytic::{CylinderSurface, PlaneSurface};
    use cadmpeg_ir::geometry::SolvedCurveGeometry;
    use cadmpeg_ir::geometry::SolvedSurfaceGeometry;
    use cadmpeg_ir::math::{Point3, Vector3};

    const EPS_SECTION_RADIUS: f64 = 1.0e-12;
    const EPS_SECTION_LOCATION: f64 = 1.0e-12;

    fn cylinder() -> SolvedSurfaceGeometry {
        SolvedSurfaceGeometry::Cylinder(
            CylinderSurface::try_new(
                Point3::new(0.0, 0.0, 0.0),
                Vector3::new(0.0, 0.0, 1.0),
                Vector3::new(1.0, 0.0, 0.0),
                2.0,
            )
            .expect("cylinder"),
        )
    }

    #[test]
    fn inclined_plane_cuts_a_cylinder_in_an_ellipse() {
        let diagonal = std::f64::consts::FRAC_1_SQRT_2;
        let plane = PlaneSurface::try_new(
            Point3::new(0.0, 0.0, 0.0),
            Vector3::new(diagonal, 0.0, diagonal),
            Vector3::new(0.0, 1.0, 0.0),
        )
        .expect("plane");
        let curve = section(
            &plane,
            &cylinder(),
            Some([Point3::new(-2.0, 0.0, 2.0), Point3::new(0.0, 2.0, 0.0)]),
        )
        .expect("ellipse section");
        let SolvedCurveGeometry::Ellipse(ellipse) = curve else {
            panic!("ellipse section");
        };
        assert!(
            (ellipse.major_radius().get() - 2.0 * std::f64::consts::SQRT_2).abs()
                < EPS_SECTION_RADIUS
        );
        assert!((ellipse.minor_radius().get() - 2.0).abs() < EPS_SECTION_RADIUS);
    }

    #[test]
    fn parallel_plane_uses_the_chart_to_select_a_generator() {
        let plane = PlaneSurface::try_new(
            Point3::new(0.0, 1.0, 0.0),
            Vector3::new(0.0, 1.0, 0.0),
            Vector3::new(1.0, 0.0, 0.0),
        )
        .expect("plane");
        let start = Point3::new(3.0f64.sqrt(), 1.0, 0.0);
        let curve = section(
            &plane,
            &cylinder(),
            Some([start, Point3::new(start.x, start.y, 1.0)]),
        )
        .expect("line section");
        let SolvedCurveGeometry::Line(line) = curve else {
            panic!("line section");
        };
        assert!(line.origin().get().distance(start) < EPS_SECTION_LOCATION);
    }
    #[test]
    fn planar_support_requires_all_poles_on_one_plane() {
        let mut poles = [
            Point3::new(0.0, 0.0, 0.0),
            Point3::new(2.0, 0.0, 0.0),
            Point3::new(0.0, 3.0, 0.0),
            Point3::new(2.0, 3.0, 0.0),
        ]
        .map(|point| {
            cadmpeg_ir::features::geometry::FinitePoint3::new(point).expect("finite pole")
        });
        assert!(planar_poles(&poles).is_some());
        poles[3] = cadmpeg_ir::features::geometry::FinitePoint3::new(Point3::new(2.0, 3.0, 1.0))
            .expect("finite pole");
        assert!(planar_poles(&poles).is_none());
    }
    #[test]
    fn collinear_leading_poles_preserve_a_planar_support() {
        let poles = [
            Point3::new(0.0, 0.0, 0.0),
            Point3::new(1.0, 2.0, 3.0),
            Point3::new(3.0, 6.0, 9.0),
            Point3::new(4.0, 1.0, 0.0),
            Point3::new(5.0, 3.0, 3.0),
        ]
        .map(|point| {
            cadmpeg_ir::features::geometry::FinitePoint3::new(point).expect("finite pole")
        });
        assert!(planar_poles(&poles).is_some());
    }
    #[test]
    fn absent_chart_uses_ordered_support_normals_for_an_exact_ellipse() {
        let plane = SolvedSurfaceGeometry::Plane(
            PlaneSurface::try_new(
                Point3::new(0.0, 0.0, 0.0),
                Vector3::new(0.0, 0.0, 1.0),
                Vector3::new(1.0, 0.0, 0.0),
            )
            .expect("plane"),
        );
        let cylinder = cylinder();
        let first = solve_supports(&plane, &cylinder, None).expect("ellipse");
        let second = solve_supports(&cylinder, &plane, None).expect("ellipse");
        let (SolvedCurveGeometry::Ellipse(first), SolvedCurveGeometry::Ellipse(second)) =
            (first, second)
        else {
            panic!("ellipse sections");
        };
        assert!((first.major_radius().get() - 2.0).abs() < EPS_SECTION_RADIUS);
        assert!((second.minor_radius().get() - 2.0).abs() < EPS_SECTION_RADIUS);
        assert!(
            first
                .frame()
                .axis()
                .as_raw()
                .dot(*second.frame().axis().as_raw())
                < 0.0
        );
    }
    #[test]
    fn planar_nurbs_support_keeps_its_parameter_normal() {
        use cadmpeg_ir::geometry::nurbs::{NurbsSurface, NurbsSurfaceAxis, NurbsSurfaceLanes};
        for reversed in [false, true] {
            let surface = NurbsSurface::from_lanes(
                NurbsSurfaceAxis::new(1, vec![0.0, 0.0, 1.0, 1.0], false),
                NurbsSurfaceAxis::new(1, vec![0.0, 0.0, 1.0, 1.0], false),
                NurbsSurfaceLanes::new(
                    vec![
                        vec![Point3::new(0.0, 0.0, 0.0), Point3::new(0.0, 1.0, 0.0)],
                        vec![Point3::new(1.0, 0.0, 0.0), Point3::new(1.0, 1.0, 0.0)],
                    ],
                    None::<Vec<Vec<f64>>>,
                ),
                reversed,
            )
            .expect("bilinear plane");
            let plane = plane_support(&SolvedSurfaceGeometry::Nurbs(surface)).expect("plane");
            let expected = Vector3::new(0.0, 0.0, if reversed { -1.0 } else { 1.0 });
            assert!(plane.frame().axis().as_raw().dot(expected) > 0.0);
        }
    }
}
