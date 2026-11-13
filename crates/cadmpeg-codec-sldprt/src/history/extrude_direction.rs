// SPDX-License-Identifier: Apache-2.0
//! Finite B-rep witnesses for implicit extrusion travel.

use std::collections::{HashMap, HashSet};

use cadmpeg_core::decode::WorkBudget;
use cadmpeg_ir::document::Model;
use cadmpeg_ir::features::{
    BooleanOp, ExtrudeDirection, ExtrudeExtent, ExtrudeStart, Feature, FeatureDefinition,
    FeatureId, FeatureOperation, LinearTermination, PlanarProfileRef, ProfileRef,
};
use cadmpeg_ir::geometry::{SolvedCurveGeometry, SolvedSurfaceGeometry};
use cadmpeg_ir::math::{Point2, Point3, Vector3};
use cadmpeg_ir::sketches::{Sketch, SketchEntity, SketchEntityId, SketchGeometryDefinition};
use cadmpeg_ir::topology::Sense;
use cadmpeg_ir::transform::Transform;

const EPS_CARRIER_DISTANCE: f64 = 1.0e-6;
const EPS_BOUNDARY_DISTANCE: f64 = 1.0e-4;
const EPS_CAP_MESH_DISTANCE: f64 = 1.0e-4;
const EPS_CAP_ALIGNMENT: f64 = 1.0e-8;
const MIN_TRIANGLE_DETERMINANT: f64 = 1.0e-24;
const CURVE_SAMPLE_INTERVALS: u32 = 16;
const MAX_WITNESS_WORK: usize = 2_000_000;

pub(crate) fn contradicted_features(model: &Model) -> HashSet<FeatureId> {
    if !model.features.iter().any(|feature| {
        candidate(feature)
            .and_then(|(profile, _, _)| profile_sketch(model, profile))
            .is_some_and(|sketch| sketch.resolved_placement().is_some())
    }) {
        return HashSet::new();
    }
    let work = WorkBudget::new(MAX_WITNESS_WORK);
    let Some(caps) = caps(model, &work) else {
        return HashSet::new();
    };
    let mut contradicted = HashSet::new();
    for feature in &model.features {
        if !work.charge() {
            break;
        }
        if contradicts(model, feature, &caps, &work) {
            contradicted.insert(feature.id.clone());
        }
    }
    contradicted
}

pub(crate) fn retain_contradicted_directions(model: &mut Model) -> bool {
    let contradicted = contradicted_features(model);
    let mut changed = false;
    for feature in &mut model.features {
        if !contradicted.contains(&feature.id) {
            continue;
        }
        let original = feature.evaluation.definition().clone();
        feature.evaluation.edit(|definition, _| retain(definition));
        changed |= original != *feature.evaluation.definition();
        for configuration in model
            .configurations
            .iter_mut()
            .filter(|configuration| configuration.active)
        {
            if let Some(state) = configuration.feature_states.get_mut(&feature.id) {
                if state.definition == original {
                    retain(&mut state.definition);
                }
            }
        }
    }
    changed
}

fn retain(definition: &mut FeatureDefinition) {
    if let FeatureDefinition::Operation(FeatureOperation::Extrude { direction, .. }) = definition {
        *direction = ExtrudeDirection::Unresolved {};
    }
}

fn candidate(feature: &Feature) -> Option<(&PlanarProfileRef, f64, f64)> {
    if feature.suppressed != Some(false) {
        return None;
    }
    match feature.evaluation.definition().operation() {
        FeatureOperation::Extrude {
            profile: ProfileRef::Planar(profile),
            start: ExtrudeStart::ProfilePlane {},
            direction: ExtrudeDirection::ProfileNormal {} | ExtrudeDirection::Unresolved {},
            extent: ExtrudeExtent::OneSided { side },
            op,
            ..
        } => {
            let LinearTermination::Blind { length } = side.termination else {
                return None;
            };
            let sign = match op {
                BooleanOp::Cut => -1.0,
                BooleanOp::NewBody | BooleanOp::Join => 1.0,
                _ => return None,
            };
            Some((profile, length.get(), sign))
        }
        _ => None,
    }
}

fn profile_sketch<'a>(model: &'a Model, profile: &'a PlanarProfileRef) -> Option<&'a Sketch> {
    let id = match profile {
        PlanarProfileRef::Sketch(id)
        | PlanarProfileRef::SketchProfiles { sketch: id, .. }
        | PlanarProfileRef::SketchEntities { sketch: id, .. } => id,
        PlanarProfileRef::Feature(id) => {
            let feature = model.features.iter().find(|feature| &feature.id == id)?;
            let FeatureOperation::Sketch { sketch } = feature.evaluation.definition().operation()
            else {
                return None;
            };
            sketch.id()?
        }
        _ => return None,
    };
    model.sketches.iter().find(|sketch| &sketch.id == id)
}

fn profile_points(
    model: &Model,
    profile: &PlanarProfileRef,
    sketch: &Sketch,
) -> Option<Vec<Point3>> {
    let (origin, normal, u_axis) = sketch.resolved_placement()?;
    let normal = normal.get();
    let u_axis = u_axis.get();
    let v_axis = normal.cross(u_axis);
    let entities: HashMap<&SketchEntityId, &SketchEntity> = model
        .sketch_entities
        .iter()
        .filter(|entity| entity.sketch == sketch.id)
        .map(|entity| (entity.id(), entity))
        .collect();
    let mut ids = Vec::new();
    match profile {
        PlanarProfileRef::SketchEntities { entities, .. } => ids.extend(entities.iter()),
        PlanarProfileRef::SketchProfiles { profiles, .. } => {
            for index in profiles {
                ids.extend(
                    sketch
                        .profiles
                        .as_slice()
                        .get(usize::try_from(*index).ok()?)?
                        .iter()
                        .map(|use_| &use_.entity),
                );
            }
        }
        _ if !sketch.profiles.is_empty() => ids.extend(
            sketch
                .profiles
                .as_slice()
                .iter()
                .flatten()
                .map(|use_| &use_.entity),
        ),
        _ => ids.extend(
            model
                .sketch_entities
                .iter()
                .filter(|entity| {
                    entity.sketch == sketch.id
                        && !entity.construction
                        && !matches!(
                            entity.geometry.definition(),
                            SketchGeometryDefinition::Point { .. }
                        )
                })
                .map(SketchEntity::id),
        ),
    }
    let mut seen = HashSet::new();
    ids.retain(|id| seen.insert(*id));
    let mut points = Vec::new();
    for id in ids {
        let entity = entities.get(id)?;
        for point in samples(entity)? {
            let point = origin
                .get()
                .translated(u_axis, point.u)
                .translated(v_axis, point.v);
            if !point.is_finite() {
                return None;
            }
            points.push(point);
        }
    }
    (!points.is_empty()).then_some(points)
}

fn samples(entity: &SketchEntity) -> Option<Vec<Point2>> {
    let (center, major_radius, minor_radius, angle, start, sweep) =
        match entity.geometry.definition() {
            SketchGeometryDefinition::Line { start, end } => {
                let start = start.get();
                let end = end.get();
                return Some(vec![
                    start,
                    end,
                    Point2::new(start.u.midpoint(end.u), start.v.midpoint(end.v)),
                ]);
            }
            SketchGeometryDefinition::Point { position } => return Some(vec![position.get()]),
            SketchGeometryDefinition::Circle { center, radius } => (
                center.get(),
                radius.get(),
                radius.get(),
                0.0,
                0.0,
                std::f64::consts::TAU,
            ),
            SketchGeometryDefinition::Arc {
                center,
                radius,
                start_angle,
                end_angle,
            } => (
                center.get(),
                radius.get(),
                radius.get(),
                0.0,
                start_angle.get(),
                (end_angle.get() - start_angle.get()).rem_euclid(std::f64::consts::TAU),
            ),
            SketchGeometryDefinition::Ellipse {
                center,
                radii,
                major_angle,
                bounds,
            } => {
                let [start, end] = bounds.map_or([0.0, std::f64::consts::TAU], |bounds| {
                    bounds.map(cadmpeg_ir::scalar::Angle::get)
                });
                let sweep = (end - start).rem_euclid(std::f64::consts::TAU);
                (
                    center.get(),
                    radii.major().get(),
                    radii.minor().get(),
                    major_angle.get(),
                    start,
                    if sweep == 0.0 && end != start {
                        std::f64::consts::TAU
                    } else {
                        sweep
                    },
                )
            }
            _ => return None,
        };
    let (axis_sine, axis_cosine) = angle.sin_cos();
    Some(
        (0..=CURVE_SAMPLE_INTERVALS)
            .map(|i| {
                let (sine, cosine) =
                    (start + sweep * f64::from(i) / f64::from(CURVE_SAMPLE_INTERVALS)).sin_cos();
                Point2::new(
                    center.u + major_radius * cosine * axis_cosine
                        - minor_radius * sine * axis_sine,
                    center.v
                        + major_radius * cosine * axis_sine
                        + minor_radius * sine * axis_cosine,
                )
            })
            .collect(),
    )
}

fn contradicts(model: &Model, feature: &Feature, caps: &[Cap], work: &WorkBudget) -> bool {
    let Some((profile, length, operation_sign)) = candidate(feature) else {
        return false;
    };
    let Some(sketch) = profile_sketch(model, profile) else {
        return false;
    };
    let Some((_, normal, _)) = sketch.resolved_placement() else {
        return false;
    };
    let Some(points) = profile_points(model, profile, sketch) else {
        return false;
    };
    let normal = normal.get();
    let parallel = |cap: &&Cap| cap.normal.dot(normal).abs() > 1.0 - EPS_CAP_ALIGNMENT;
    let start = caps
        .iter()
        .filter(parallel)
        .any(|cap| points.iter().all(|point| cap.supports(*point, work)));
    if !start {
        return false;
    }
    let end = |travel: f64| {
        caps.iter().filter(parallel).any(|cap| {
            cap.normal.dot(normal) * cap.sense * travel * operation_sign > 1.0 - EPS_CAP_ALIGNMENT
                && points
                    .iter()
                    .any(|point| cap.supports(point.translated(normal, length * travel), work))
        })
    };
    let decoded = end(1.0);
    let opposite = end(-1.0);
    !decoded && opposite && work.charge()
}

struct Cap {
    origin: Point3,
    normal: Vector3,
    sense: f64,
    vertices: Vec<Point3>,
    triangles: Vec<[Point3; 3]>,
    polygons: Option<Vec<Vec<Point3>>>,
    transform: Transform,
}

fn caps(model: &Model, work: &WorkBudget) -> Option<Vec<Cap>> {
    let indexing_work = [
        model.surfaces.len(),
        model.shells.len(),
        model.regions.len(),
        model.bodies.len(),
        model.faces.len(),
        model.loops.len(),
        model.coedges.len(),
        model.edges.len(),
        model.vertices.len(),
        model.points.len(),
        model.curves.len(),
    ]
    .into_iter()
    .try_fold(0_usize, usize::checked_add)?;
    if !work.charge_by(indexing_work) {
        return None;
    }
    let surfaces: HashMap<_, _> = model
        .surfaces
        .iter()
        .map(|s| (&s.id, &s.geometry))
        .collect();
    let shells: HashMap<_, _> = model.shells.iter().map(|s| (&s.id, &s.region)).collect();
    let regions: HashMap<_, _> = model
        .bodies
        .iter()
        .flat_map(|b| {
            b.regions
                .iter()
                .map(move |r| (r, b.transform.unwrap_or_else(Transform::identity)))
        })
        .collect();
    let loops: HashMap<_, _> = model.loops.iter().map(|l| (&l.id, l)).collect();
    let coedges: HashMap<_, _> = model.coedges.iter().map(|c| (&c.id, c)).collect();
    let edges: HashMap<_, _> = model.edges.iter().map(|e| (&e.id, e)).collect();
    let vertices: HashMap<_, _> = model.vertices.iter().map(|v| (&v.id, &v.point)).collect();
    let points: HashMap<_, _> = model
        .points
        .iter()
        .map(|p| (&p.id, p.position().get()))
        .collect();
    let curves: HashMap<_, _> = model.curves.iter().map(|c| (&c.id, &c.geometry)).collect();
    let mut result = Vec::new();
    let mut indices = HashMap::new();
    for face in &model.faces {
        let Some(SolvedSurfaceGeometry::Plane(plane)) =
            surfaces.get(&face.surface).and_then(|s| s.solved())
        else {
            continue;
        };
        let Some(transform) = shells
            .get(&face.shell)
            .and_then(|r| regions.get(r))
            .copied()
        else {
            continue;
        };
        let Some(origin) = transform.apply_point(plane.origin().get()) else {
            continue;
        };
        let Some(normal) = transform.apply_normal(*plane.frame().axis().as_raw()) else {
            continue;
        };
        let mut cap = Cap {
            origin: origin.get(),
            normal: *normal.as_raw(),
            sense: if face.sense == Sense::Reversed {
                -1.0
            } else {
                1.0
            },
            vertices: Vec::new(),
            triangles: Vec::new(),
            polygons: Some(Vec::new()),
            transform,
        };
        for lid in &face.loops {
            let polygon = (|| {
                let loop_ = loops.get(lid)?;
                let mut polygon = Vec::new();
                let mut first = None;
                let mut previous = None;
                for cid in loop_.coedges() {
                    if !work.charge() {
                        return None;
                    }
                    let coedge = coedges.get(cid)?;
                    let edge = edges.get(&coedge.edge)?;
                    let (a, b) = if coedge.sense == Sense::Reversed {
                        (&edge.end, &edge.start)
                    } else {
                        (&edge.start, &edge.end)
                    };
                    let ap = transform.apply_point(*points.get(vertices.get(a)?)?)?.get();
                    let bp = transform.apply_point(*points.get(vertices.get(b)?)?)?.get();
                    cap.vertices.extend([ap, bp]);
                    if previous.is_some_and(|p| p != a) {
                        return None;
                    }
                    first.get_or_insert(a);
                    previous = Some(b);
                    if !matches!(
                        curves.get(edge.curve()?)?.solved(),
                        Some(SolvedCurveGeometry::Line(_))
                    ) {
                        return None;
                    }
                    polygon.push(ap);
                }
                (first.is_some() && first == previous).then_some(polygon)
            })();
            match (cap.polygons.as_mut(), polygon) {
                (Some(polygons), Some(polygon)) => polygons.push(polygon),
                _ => cap.polygons = None,
            }
        }
        indices.insert(&face.id, result.len());
        result.push(cap);
    }
    for mesh in &model.tessellations {
        let face_count = mesh
            .faces
            .iter()
            .filter(|face| indices.contains_key(*face))
            .count();
        if face_count == 0 {
            continue;
        }
        let work_count = mesh
            .triangle_count()
            .checked_mul(face_count)?
            .checked_mul(3)?
            .checked_add(mesh.vertex_count())?;
        if !work.charge_by(work_count) {
            return None;
        }
        let vertices = mesh.vertices();
        let triangles = mesh.triangles();
        for face in &mesh.faces {
            let Some(index) = indices.get(face) else {
                continue;
            };
            let cap = &mut result[*index];
            for triangle in &triangles {
                let points = triangle.map(|i| {
                    vertices
                        .get(cadmpeg_core::decode::index_from_u32(i))
                        .and_then(|p| cap.transform.apply_point(p.get()))
                        .map(cadmpeg_ir::features::FinitePoint3::get)
                });
                if let [Some(a), Some(b), Some(c)] = points {
                    if [a, b, c].iter().all(|p| {
                        p.vector_from(cap.origin).dot(cap.normal).abs() <= EPS_CAP_MESH_DISTANCE
                    }) {
                        cap.triangles.push([a, b, c].map(|p| {
                            p.translated(cap.normal, -p.vector_from(cap.origin).dot(cap.normal))
                        }));
                    }
                }
            }
        }
    }
    Some(result)
}

impl Cap {
    fn supports(&self, p: Point3, work: &WorkBudget) -> bool {
        if !work.charge()
            || p.vector_from(self.origin).dot(self.normal).abs() > EPS_CARRIER_DISTANCE
        {
            return false;
        }
        if !work.charge_by(self.vertices.len()) {
            return false;
        }
        if self
            .vertices
            .iter()
            .any(|v| p.distance(*v) <= EPS_CARRIER_DISTANCE)
        {
            return true;
        }
        if !self.triangles.is_empty() {
            return self
                .triangles
                .iter()
                .any(|triangle| work.charge() && triangle_contains(p, *triangle));
        }
        let Some(polygons) = &self.polygons else {
            return false;
        };
        let n = self.normal;
        let project = |p: Point3| {
            if n.x.abs() >= n.y.abs().max(n.z.abs()) {
                Point2::new(p.y, p.z)
            } else if n.y.abs() >= n.z.abs() {
                Point2::new(p.x, p.z)
            } else {
                Point2::new(p.x, p.y)
            }
        };
        let p = project(p);
        let mut inside = false;
        for polygon in polygons {
            for (a, b) in polygon
                .iter()
                .zip(polygon.iter().cycle().skip(1))
                .take(polygon.len())
            {
                if !work.charge() {
                    return false;
                }
                let a = project(*a);
                let b = project(*b);
                let dx = b.u - a.u;
                let dy = b.v - a.v;
                let length = dx.hypot(dy);
                let along = (p.u - a.u) * dx + (p.v - a.v) * dy;
                if length > EPS_BOUNDARY_DISTANCE
                    && ((p.u - a.u) * dy - (p.v - a.v) * dx).abs() <= EPS_BOUNDARY_DISTANCE * length
                    && along >= -EPS_BOUNDARY_DISTANCE
                    && along <= length * length + EPS_BOUNDARY_DISTANCE * length
                {
                    return true;
                }
                if (a.v > p.v) != (b.v > p.v) && p.u < (b.u - a.u) * (p.v - a.v) / (b.v - a.v) + a.u
                {
                    inside = !inside;
                }
            }
        }
        inside
    }
}

fn triangle_contains(point: Point3, [origin, first, second]: [Point3; 3]) -> bool {
    let first_edge = first.vector_from(origin);
    let second_edge = second.vector_from(origin);
    let offset = point.vector_from(origin);
    let first_squared = first_edge.dot(first_edge);
    let second_squared = second_edge.dot(second_edge);
    let mixed = first_edge.dot(second_edge);
    let determinant = first_squared * second_squared - mixed * mixed;
    if determinant.abs() < MIN_TRIANGLE_DETERMINANT {
        return false;
    }
    let first_weight =
        (offset.dot(first_edge) * second_squared - offset.dot(second_edge) * mixed) / determinant;
    let second_weight =
        (offset.dot(second_edge) * first_squared - offset.dot(first_edge) * mixed) / determinant;
    let slack = EPS_BOUNDARY_DISTANCE / first_edge.norm().max(second_edge.norm()).max(1.0);
    first_weight >= -slack && second_weight >= -slack && first_weight + second_weight <= 1.0 + slack
}

#[cfg(test)]
mod tests;
