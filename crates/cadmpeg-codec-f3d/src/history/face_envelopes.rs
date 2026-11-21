// SPDX-License-Identifier: Apache-2.0
//! Retain conservative geometry envelopes for transition certificates.

use crate::history_records::AsmHistoricalEnvelope;
use cadmpeg_asm::brep::AsmBrep;
use cadmpeg_ir::features::FinitePoint3;
use cadmpeg_ir::geometry::{CurveGeometry, SolvedCurveGeometry, SolvedSurfaceGeometry};
use cadmpeg_ir::math::{Point3, Vector3};
use cadmpeg_ir::units::FiniteVector;
use std::collections::HashMap;
use std::f64::consts::{PI, TAU};
use std::hash::Hash;

fn index<'a, K: Eq + Hash + 'a, V: 'a>(
    rows: impl Iterator<Item = (&'a K, &'a V)>,
) -> HashMap<&'a K, Option<&'a V>> {
    let mut result = HashMap::new();
    for (key, value) in rows {
        result
            .entry(key)
            .and_modify(|row| *row = None)
            .or_insert(Some(value));
    }
    result
}

fn coordinates(point: Point3) -> [f64; 3] {
    [point.x, point.y, point.z]
}

fn envelope(
    entity: i64,
    points: impl IntoIterator<Item = Point3>,
) -> Option<AsmHistoricalEnvelope> {
    let mut low = [f64::INFINITY; 3];
    let mut high = [f64::NEG_INFINITY; 3];
    for point in points {
        for (index, value) in coordinates(point).into_iter().enumerate() {
            if !value.is_finite() {
                return None;
            }
            low[index] = low[index].min(value);
            high[index] = high[index].max(value);
        }
    }
    low.iter()
        .zip(high)
        .all(|(low, high)| *low <= high)
        .then_some(AsmHistoricalEnvelope {
            entity,
            boundary: Vec::new(),
            low: Point3::new(low[0], low[1], low[2]),
            high: Point3::new(high[0], high[1], high[2]),
        })
}

fn ends(bounds: &AsmHistoricalEnvelope) -> [Point3; 2] {
    [bounds.low, bounds.high]
}

fn conic_points(
    center: Point3,
    mut u: Vector3,
    mut v: Vector3,
    mut range: [f64; 2],
    hyperbolic: bool,
) -> Vec<Point3> {
    if !hyperbolic {
        let [low, high] = range;
        if high - low >= TAU
            || (low + TAU).to_bits() == low.to_bits()
            || (high - TAU).to_bits() == high.to_bits()
        {
            let radius = Vector3::new(u.x.hypot(v.x), u.y.hypot(v.y), u.z.hypot(v.z));
            return vec![
                center.translated(radius, -1.0),
                center.translated(radius, 1.0),
            ];
        }
        let (sine, cosine) = low.sin_cos();
        (u, v) = (
            u.scale(cosine) + v.scale(sine),
            v.scale(cosine) - u.scale(sine),
        );
        range = [0.0, high - low];
    }
    let [low, high] = range;
    let mut parameters = vec![low, high];
    for (a, b) in [u.x, u.y, u.z].into_iter().zip([v.x, v.y, v.z]) {
        if hyperbolic {
            if a != 0.0 && (b / a).abs() < 1.0 {
                let t = (-b / a).atanh();
                if low < t && t < high {
                    parameters.push(t);
                }
            }
        } else {
            let t = b.atan2(a);
            for angle in [t, t + PI] {
                let first = low + (angle - low).rem_euclid(TAU);
                if first <= high {
                    parameters.push(first);
                }
            }
        }
    }
    parameters
        .into_iter()
        .map(|t| {
            let (a, b) = if hyperbolic {
                (t.cosh(), t.sinh())
            } else {
                (t.cos(), t.sin())
            };
            center.translated(u.scale(a) + v.scale(b), 1.0)
        })
        .collect()
}

fn edge_envelope(
    edge: &cadmpeg_ir::topology::Edge,
    curves: &HashMap<&cadmpeg_ir::ids::CurveId, Option<&cadmpeg_ir::geometry::Curve>>,
    nurbs_bounds: &mut HashMap<cadmpeg_ir::ids::CurveId, Option<AsmHistoricalEnvelope>>,
    endpoints: Option<[Point3; 2]>,
) -> Option<AsmHistoricalEnvelope> {
    let entity = super::stable_ref(edge.id.as_str())?;
    let curve = curves.get(edge.curve()?)?.as_ref()?;
    let CurveGeometry::Solved(geometry) = &curve.geometry else {
        return None;
    };
    let range = edge.param_range().map(FiniteVector::get);
    let mut points = match geometry {
        SolvedCurveGeometry::Line(line) => match range {
            Some(range) => range
                .map(|t| {
                    line.origin()
                        .get()
                        .translated(*line.direction().as_raw(), t)
                })
                .to_vec(),
            None => endpoints?.to_vec(),
        },
        SolvedCurveGeometry::Circle(circle) => {
            let u = circle
                .frame()
                .reference()
                .as_raw()
                .scale(circle.radius().get());
            let v = circle.frame().axis().as_raw().cross(u);
            conic_points(
                circle.center().get(),
                u,
                v,
                range.unwrap_or([0.0, TAU]),
                false,
            )
        }
        SolvedCurveGeometry::Ellipse(ellipse) => {
            let reference = *ellipse.frame().reference().as_raw();
            let u = reference.scale(ellipse.major_radius().get());
            let v = ellipse
                .frame()
                .axis()
                .as_raw()
                .cross(reference)
                .scale(ellipse.minor_radius().get());
            conic_points(
                ellipse.center().get(),
                u,
                v,
                range.unwrap_or([0.0, TAU]),
                false,
            )
        }
        SolvedCurveGeometry::Hyperbola(hyperbola) => {
            let reference = *hyperbola.frame().reference().as_raw();
            let u = reference.scale(hyperbola.major_radius().get());
            let v = hyperbola
                .frame()
                .axis()
                .as_raw()
                .cross(reference)
                .scale(hyperbola.minor_radius().get());
            conic_points(hyperbola.center().get(), u, v, range?, true)
        }
        SolvedCurveGeometry::Nurbs(nurbs) => {
            if !nurbs.periodic() {
                let range = range?;
                let knots = nurbs.knots().as_slice();
                let low = *knots.get(usize::try_from(nurbs.degree()).ok()?)?;
                let high = *knots.get(nurbs.pole_count())?;
                if range[0] < low || range[1] > high {
                    return None;
                }
            }
            let bounds = nurbs_bounds
                .entry(curve.id.clone())
                .or_insert_with(|| {
                    if nurbs
                        .weights()
                        .is_some_and(|weights| weights.iter().any(|w| w.get() <= 0.0))
                    {
                        return None;
                    }
                    envelope(0, nurbs.control_points().into_iter().map(FinitePoint3::get))
                })
                .as_ref()?;
            ends(bounds).to_vec()
        }
        _ => return None,
    };
    if let Some(endpoints) = endpoints {
        points.extend(endpoints);
    }
    envelope(entity, points)
}

fn cone_envelope(
    mut bounds: AsmHistoricalEnvelope,
    cone: &cadmpeg_ir::geometry::analytic::ConeSurface,
) -> Option<AsmHistoricalEnvelope> {
    let slope = cone.half_angle().get().tan();
    if slope == 0.0 || !slope.is_finite() {
        return None;
    }
    let axis = *cone.frame().axis().as_raw();
    let apex = cone
        .origin()
        .get()
        .translated(axis, -cone.radius().get() / slope);
    let u = axis
        .cross(Vector3::new(1.0, 0.0, 0.0))
        .unit()
        .or_else(|| axis.cross(Vector3::new(0.0, 1.0, 0.0)).unit())?;
    let v = axis.cross(u);
    let excludes_apex = [u, v].into_iter().any(|radial| {
        let values: Vec<_> = [bounds.low.x, bounds.high.x]
            .into_iter()
            .flat_map(|x| {
                [bounds.low.y, bounds.high.y]
                    .into_iter()
                    .flat_map(move |y| {
                        [bounds.low.z, bounds.high.z]
                            .into_iter()
                            .map(move |z| Point3::new(x, y, z).vector_from(apex).dot(radial))
                    })
            })
            .collect();
        values.iter().all(|value| *value < 0.0) || values.iter().all(|value| *value > 0.0)
    });
    if !excludes_apex {
        bounds = envelope(bounds.entity, [bounds.low, bounds.high, apex])?;
    }
    Some(bounds)
}

pub(super) fn derive(brep: &AsmBrep) -> (Vec<AsmHistoricalEnvelope>, Vec<AsmHistoricalEnvelope>) {
    let curves = index(brep.curves.iter().map(|curve| (&curve.id, curve)));
    let mut nurbs_bounds = HashMap::new();
    let vertices = index(brep.vertices.iter().map(|row| (&row.id, row)));
    let points = index(brep.points.iter().map(|row| (&row.id, row)));
    let mut edges: Vec<_> = brep
        .edges
        .iter()
        .filter_map(|edge| {
            let position = |id: &cadmpeg_ir::ids::VertexId| {
                let vertex = vertices.get(id)?.as_ref()?;
                Some(points.get(&vertex.point)?.as_ref()?.position().get())
            };
            let endpoints = edge
                .ends
                .as_ref()
                .and_then(|ends| Some([position(&ends.start)?, position(&ends.end)?]));
            edge_envelope(edge, &curves, &mut nurbs_bounds, endpoints)
        })
        .collect();
    edges.sort_by_key(|bounds| bounds.entity);
    let edge_bounds: HashMap<_, _> = edges.iter().map(|row| (row.entity, row)).collect();
    let coedges = index(brep.coedges.iter().map(|row| (&row.id, row)));
    let loops = index(brep.loops.iter().map(|row| (&row.id, row)));
    let surfaces = index(brep.surfaces.iter().map(|row| (&row.id, row)));
    let mut faces: Vec<_> = brep
        .faces
        .iter()
        .filter_map(|face| {
            let entity = super::stable_ref(face.id.as_str())?;
            let mut boundary = Vec::new();
            let mut boundary_edges = Vec::new();
            for loop_id in &face.loops {
                for coedge in loops.get(loop_id)?.as_ref()?.coedges() {
                    let coedge = coedges.get(coedge)?.as_ref()?;
                    if coedge.use_curve.is_some() {
                        return None;
                    }
                    let edge = super::stable_ref(coedge.edge.as_str())?;
                    boundary_edges.push(edge);
                    boundary.extend(ends(edge_bounds.get(&edge)?));
                }
            }
            let mut bounds = envelope(entity, boundary)?;
            let cadmpeg_ir::geometry::SurfaceGeometry::Solved(geometry) =
                &surfaces.get(&face.surface)?.as_ref()?.geometry
            else {
                return None;
            };
            match geometry {
                SolvedSurfaceGeometry::Plane(_) | SolvedSurfaceGeometry::Cylinder(_) => {}
                SolvedSurfaceGeometry::Cone(cone) => {
                    bounds = cone_envelope(bounds, cone)?;
                }
                SolvedSurfaceGeometry::Sphere(sphere) => {
                    let center = sphere.center().get();
                    let radius = sphere.radius().get().abs();
                    bounds = envelope(
                        entity,
                        [
                            bounds.low,
                            bounds.high,
                            center.translated(Vector3::new(-radius, -radius, -radius), 1.0),
                            center.translated(Vector3::new(radius, radius, radius), 1.0),
                        ],
                    )?;
                }
                SolvedSurfaceGeometry::Torus(torus) => {
                    let center = torus.center().get();
                    let radius = torus.major_radius().get() + torus.minor_radius().get().abs();
                    bounds = envelope(
                        entity,
                        [
                            bounds.low,
                            bounds.high,
                            center.translated(Vector3::new(-radius, -radius, -radius), 1.0),
                            center.translated(Vector3::new(radius, radius, radius), 1.0),
                        ],
                    )?;
                }
                _ => return None,
            }
            boundary_edges.sort_unstable();
            bounds.boundary = boundary_edges;
            Some(bounds)
        })
        .collect();
    faces.sort_by_key(|bounds| bounds.entity);
    (faces, edges)
}

#[cfg(test)]
mod tests;
