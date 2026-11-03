// SPDX-License-Identifier: Apache-2.0
//! Verify saved dimension references against the stored source topology.

use std::collections::{BTreeMap, BTreeSet};

use cadmpeg_ir::drawings::Drawing;
use cadmpeg_ir::geometry::{Curve, SolvedCurveGeometry, SolvedSurfaceGeometry, Surface};
use cadmpeg_ir::ids::{CurveId, VertexId};
use cadmpeg_ir::math::{Point3, Vector3};
use cadmpeg_ir::topology::{Edge, Face, Point, Vertex};
use cadmpeg_ir::transform::Transform;

use crate::brep::TextCurve;

pub(super) struct Sources<'a> {
    curves: BTreeMap<&'a CurveId, &'a Curve>,
    edges: &'a [Edge],
    vertices: BTreeMap<&'a VertexId, Point3>,
    complete: bool,
    surfaces: Vec<&'a Surface>,
}

impl<'a> Sources<'a> {
    pub(super) fn new(
        curves: &'a [Curve],
        edges: &'a [Edge],
        vertices: &'a [Vertex],
        points: &[Point],
        surfaces: &'a [Surface],
        faces: &[Face],
        unplaced: bool,
    ) -> Self {
        let points = points
            .iter()
            .map(|point| (&point.id, point.position().get()))
            .collect::<BTreeMap<_, _>>();
        let vertices = vertices
            .iter()
            .filter_map(|vertex| Some((&vertex.id, *points.get(&vertex.point)?)))
            .collect();
        let used = faces
            .iter()
            .map(|face| &face.surface)
            .collect::<BTreeSet<_>>();
        let surfaces = surfaces
            .iter()
            .filter(|surface| used.contains(&surface.id))
            .collect::<Vec<_>>();
        let complete = unplaced
            && surfaces.len() == used.len()
            && surfaces
                .iter()
                .all(|surface| surface.source_object.is_some());
        Self {
            curves: curves.iter().map(|curve| (&curve.id, curve)).collect(),
            edges,
            vertices,
            complete,
            surfaces,
        }
    }

    pub(super) fn project(&self, view: &Drawing) -> Option<Projection> {
        if !self.complete {
            return None;
        }
        let sources = view.relationships.get("Source")?;
        let owners = sources
            .iter()
            .map(|source| source.local_target())
            .collect::<Option<Vec<_>>>()?;
        if owners.is_empty() || sources.iter().any(|source| !source.subelements.is_empty()) {
            return None;
        }
        let normal = Vector3::from(*view.direction?.as_raw()).unit()?;
        let horizontal = Vector3::from(*view.x_direction?.as_raw());
        let horizontal = (horizontal - normal.scale(normal.dot(horizontal))).unit()?;
        let vertical = normal.cross(horizontal);
        for surface in &self.surfaces {
            let Some(source) = &surface.source_object else {
                return None;
            };
            if owners.contains(&source.object_id.as_str())
                && !bounded_by_edges(surface.geometry.solved()?, normal)
            {
                return None;
            }
        }
        let point = |value: Vector3| [horizontal.dot(value), -vertical.dot(value)];
        let mut primitives = Vec::new();
        let mut seen = BTreeSet::new();
        for edge in self.edges {
            let curve = edge.curve().and_then(|id| self.curves.get(id))?;
            let Some(source) = &curve.source_object else {
                continue;
            };
            if !owners.contains(&source.object_id.as_str()) {
                continue;
            }
            seen.insert(source.object_id.as_str());
            let range = edge.param_range()?.get();
            let mut geometry = curve.geometry.solved()?;
            let mut offset = Vector3::new(0.0, 0.0, 0.0);
            while let SolvedCurveGeometry::Transformed(placed) = geometry {
                offset = offset + translation(placed.transform())?;
                if !offset.is_finite() {
                    return None;
                }
                geometry = placed.basis();
            }
            let primitive = match geometry {
                SolvedCurveGeometry::Line(_) => {
                    let first = self.vertices.get(&edge.start)?;
                    let second = self.vertices.get(&edge.end)?;
                    Primitive::Line([
                        point(Vector3::new(first.x, first.y, first.z)),
                        point(Vector3::new(second.x, second.y, second.z)),
                    ])
                }
                SolvedCurveGeometry::Circle(circle) => {
                    if range != [0.0, std::f64::consts::TAU]
                        || circle.frame().axis().as_raw().dot(normal).abs() != 1.0
                    {
                        return None;
                    }
                    let center = circle.center().get();
                    Primitive::Circle {
                        center: point(Vector3::new(center.x, center.y, center.z) + offset),
                        radius: circle.radius().get(),
                    }
                }
                _ => return None,
            };
            primitives.push(primitive);
        }
        if primitives.is_empty() || owners.iter().any(|owner| !seen.contains(owner)) {
            return None;
        }
        let mut low = [f64::INFINITY; 2];
        let mut high = [f64::NEG_INFINITY; 2];
        for primitive in &primitives {
            let (minimum, maximum) = match primitive {
                Primitive::Line([first, second]) => {
                    if first.iter().chain(second).any(|value| !value.is_finite()) {
                        return None;
                    }
                    (
                        [first[0].min(second[0]), first[1].min(second[1])],
                        [first[0].max(second[0]), first[1].max(second[1])],
                    )
                }
                Primitive::Circle { center, radius } => (
                    [center[0] - radius, center[1] - radius],
                    [center[0] + radius, center[1] + radius],
                ),
            };
            for axis in 0..2 {
                if !minimum[axis].is_finite() || !maximum[axis].is_finite() {
                    return None;
                }
                low[axis] = low[axis].min(minimum[axis]);
                high[axis] = high[axis].max(maximum[axis]);
            }
        }
        let center = [
            f64::midpoint(low[0], high[0]),
            f64::midpoint(low[1], high[1]),
        ];
        for primitive in &mut primitives {
            match primitive {
                Primitive::Line(points) => {
                    for point in points {
                        point[0] -= center[0];
                        point[1] -= center[1];
                    }
                }
                Primitive::Circle { center: point, .. } => {
                    point[0] -= center[0];
                    point[1] -= center[1];
                }
            }
        }
        Some(Projection { primitives })
    }
}

enum Primitive {
    Line([[f64; 2]; 2]),
    Circle { center: [f64; 2], radius: f64 },
}

pub(super) struct Projection {
    primitives: Vec<Primitive>,
}

impl Projection {
    pub(super) fn contains(
        &self,
        curve: &TextCurve,
        range: [cadmpeg_ir::scalar::FiniteReal; 2],
    ) -> bool {
        match curve {
            TextCurve::Line { origin, direction } => {
                let origin = origin.get();
                let direction = direction.get();
                if origin.z != 0.0 || direction.z != 0.0 {
                    return false;
                }
                let points = range.map(|parameter| {
                    [
                        origin.x + direction.x * parameter.get(),
                        origin.y + direction.y * parameter.get(),
                    ]
                });
                self.primitives.iter().any(|primitive| matches!(primitive, Primitive::Line(candidate) if *candidate == points || *candidate == [points[1],points[0]]))
            }
            TextCurve::Circle { center, radius, .. } => {
                let center = center.get();
                center.z == 0.0 && self.primitives.iter().any(|primitive| matches!(primitive, Primitive::Circle { center: candidate, radius: candidate_radius } if *candidate == [center.x,center.y] && *candidate_radius == radius.get()))
            }
            _ => false,
        }
    }
}

fn translation(transform: &Transform) -> Option<Vector3> {
    let rows = transform.affine_rows();
    (rows.map(|row| [row[0], row[1], row[2]])
        == [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]])
    .then_some(Vector3::new(rows[0][3], rows[1][3], rows[2][3]))
}

fn bounded_by_edges(mut geometry: &SolvedSurfaceGeometry, normal: Vector3) -> bool {
    let mut translations_only = true;
    while let SolvedSurfaceGeometry::Transformed(placed) = geometry {
        translations_only &= translation(placed.transform()).is_some();
        geometry = placed.basis();
    }
    match geometry {
        SolvedSurfaceGeometry::Plane(_) => true,
        SolvedSurfaceGeometry::Cylinder(cylinder) => {
            translations_only && cylinder.frame().axis().as_raw().dot(normal).abs() == 1.0
        }
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use cadmpeg_ir::geometry::analytic::{CylinderSurface, PlaneSurface, SphereSurface};

    #[test]
    fn envelopes_refuse_surfaces_that_need_silhouettes() {
        let origin = Point3::new(0.0, 0.0, 0.0);
        let normal = Vector3::new(0.0, 0.0, 1.0);
        let horizontal = Vector3::new(1.0, 0.0, 0.0);
        let plane = SolvedSurfaceGeometry::Plane(
            PlaneSurface::try_new(origin, normal, horizontal).unwrap(),
        );
        let cylinder = SolvedSurfaceGeometry::Cylinder(
            CylinderSurface::try_new(origin, normal, horizontal, 7.0).unwrap(),
        );
        let sphere = SolvedSurfaceGeometry::Sphere(
            SphereSurface::try_new(origin, normal, horizontal, 7.0).unwrap(),
        );
        assert!(bounded_by_edges(&plane, horizontal));
        assert!(bounded_by_edges(&cylinder, normal));
        assert!(!bounded_by_edges(&cylinder, horizontal));
        assert!(!bounded_by_edges(&sphere, normal));
        assert!(!bounded_by_edges(
            &SolvedSurfaceGeometry::Unknown { record: None },
            normal
        ));
    }
}
