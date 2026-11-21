// SPDX-License-Identifier: Apache-2.0
//! Analytic boundary representations of CSG primitives and swept areas.
//!
//! Every body is one solid region bounded by one closed shell. A face's ring
//! runs counterclockwise about the face's outward normal, and each edge is
//! used once in each direction. A closed side surface carries a seam: the
//! profile segment or profile vertex the sweep starts from.

use std::collections::BTreeMap;
use std::f64::consts::TAU;

use cadmpeg_ir::draft::ModelDraft;
use cadmpeg_ir::features::geometry::FinitePoint3;
use cadmpeg_ir::geometry::analytic::{
    CircleCurve, ConeSurface, CylinderSurface, LineCurve, PlaneSurface, SphereSurface, TorusSurface,
};
use cadmpeg_ir::geometry::{
    Curve, CurveGeometry, SolvedCurveGeometry, SolvedSurfaceGeometry, Surface, SurfaceGeometry,
};
use cadmpeg_ir::ids::{
    BodyId, CoedgeId, CurveId, EdgeId, FaceId, IdentityKey, LoopId, PointId, RegionId, ShellId,
    SurfaceId, VertexId,
};
use cadmpeg_ir::math::{Point3, Vector3};
use cadmpeg_ir::topology::{
    AnchoredVertexUse, Body, BodyKind, Coedge, Edge, EdgeCarrier, EdgeEnds, Face, FaceLoops, Loop,
    LoopBoundary, LoopRing, Point, Region, Sense, Shell, Vertex,
};

use super::frame::{rotate_about, turn, unit, Frame};
use super::profile::{Profile, Segment, PROFILE_RELATIVE_RESOLUTION};
use crate::ids::{self, kind};

/// A sweep angle within this many radians of a full turn is a full turn.
const FULL_TURN_TOLERANCE: f64 = 1.0e-12;

/// An extruded arc sweeps a circular cylinder when the cosine between its
/// circle's axis and the direction is within this of one.
const ARC_AXIS_PARALLEL_TOLERANCE: f64 = 1.0e-9;

/// A face ring: `(edge, coedge sense)` in traversal order, and each pole
/// occurrence as the ring position of the coedge it follows and the pole
/// vertex.
type PoledRing = (Vec<(usize, Sense)>, Vec<(usize, usize)>);

/// A body under construction for one CSG item.
pub(super) struct SolidBuilder {
    item: u64,
    serial: u64,
    tolerance: f64,
    positions: Vec<Point3>,
    points: Vec<Point>,
    vertices: Vec<Vertex>,
    curves: Vec<Curve>,
    edges: Vec<Edge>,
    surfaces: Vec<Surface>,
    faces: Vec<Face>,
    loops: Vec<Loop>,
    coedges: Vec<Coedge>,
}

/// The built body: its draft and every identity it holds.
pub(super) struct BuiltSolid {
    pub(super) draft: ModelDraft,
    pub(super) body: BodyId,
    pub(super) identities: Vec<String>,
    pub(super) faces: usize,
}

impl SolidBuilder {
    /// A builder for the body of item `item`. Two points closer than
    /// `tolerance` are one vertex.
    pub(super) fn new(item: u64, tolerance: f64) -> Self {
        Self {
            item,
            serial: 0,
            tolerance,
            positions: Vec::new(),
            points: Vec::new(),
            vertices: Vec::new(),
            curves: Vec::new(),
            edges: Vec::new(),
            surfaces: Vec::new(),
            faces: Vec::new(),
            loops: Vec::new(),
            coedges: Vec::new(),
        }
    }

    fn key(&mut self) -> IdentityKey {
        self.serial += 1;
        IdentityKey::from(self.item).dash(self.serial)
    }

    fn shell_id(&self) -> ShellId {
        ShellId::from(ids::csg(kind!("shell"), self.item))
    }

    /// The vertex at `position`, shared with an earlier vertex there.
    pub(super) fn vertex(&mut self, position: Point3) -> Result<usize, String> {
        if let Some(index) = self
            .positions
            .iter()
            .position(|known| known.distance(position) <= self.tolerance)
        {
            return Ok(index);
        }
        let finite = FinitePoint3::new(position)
            .ok_or_else(|| format!("CSG item #{} has a non-finite vertex", self.item))?;
        let key = self.key();
        let point = PointId::from(ids::csg(kind!("point"), key.clone()));
        let vertex = VertexId::from(ids::csg(kind!("vertex"), key));
        self.points.push(Point::new(point.clone(), finite, None));
        self.vertices.push(Vertex {
            id: vertex,
            point,
            tolerance: None,
            native_ref: None,
        });
        self.positions.push(position);
        Ok(self.positions.len() - 1)
    }

    fn position(&self, vertex: usize) -> Point3 {
        self.positions[vertex]
    }

    fn curve(&mut self, geometry: SolvedCurveGeometry) -> CurveId {
        let key = self.key();
        let curve = CurveId::from(ids::csg(kind!("curve"), key));
        self.curves.push(Curve {
            parameter_range: None,
            id: curve.clone(),
            geometry: CurveGeometry::Solved(geometry),
            source_object: None,
            native_ref: None,
        });
        curve
    }

    fn edge(&mut self, carrier: EdgeCarrier, start: usize, end: usize) -> usize {
        let key = self.key();
        self.edges.push(Edge {
            id: EdgeId::from(ids::csg(kind!("edge"), key)),
            carrier,
            ends: Some(EdgeEnds::new(
                self.vertices[start].id.clone(),
                self.vertices[end].id.clone(),
            )),
            tolerance: None,
            native_ref: None,
        });
        self.edges.len() - 1
    }

    /// A straight edge from `start` to `end`.
    pub(super) fn line(&mut self, start: usize, end: usize) -> Result<usize, String> {
        let from = self.position(start);
        let offset = self.position(end).vector_from(from);
        let length = offset.norm();
        let direction = unit(offset)
            .ok_or_else(|| format!("CSG item #{} has a zero-length edge", self.item))?;
        let line = LineCurve::try_new(from, direction).map_err(str::to_owned)?;
        let curve = self.curve(SolvedCurveGeometry::Line(line));
        let carrier = EdgeCarrier::new(Some(curve), Some([0.0, length])).map_err(str::to_owned)?;
        Ok(self.edge(carrier, start, end))
    }

    /// A circular edge about `center` counterclockwise about the unit
    /// `normal` from `start` through `sweep` radians to `end`; a full turn
    /// is a closed edge.
    pub(super) fn arc(
        &mut self,
        center: Point3,
        normal: Vector3,
        start: usize,
        end: usize,
        sweep: f64,
    ) -> Result<usize, String> {
        let normal = unit(normal)
            .ok_or_else(|| format!("CSG item #{} has an arc with no axis", self.item))?;
        let offset = in_plane(self.position(start).vector_from(center), normal);
        let radius = offset.norm();
        let reference = unit(offset)
            .ok_or_else(|| format!("CSG item #{} has an arc of zero radius", self.item))?;
        let circle =
            CircleCurve::try_new(center, normal, reference, radius).map_err(str::to_owned)?;
        let curve = self.curve(SolvedCurveGeometry::Circle(circle));
        let carrier = if full_turn(sweep) {
            EdgeCarrier::unbounded(Some(curve))
        } else {
            EdgeCarrier::new(Some(curve), Some([0.0, sweep])).map_err(str::to_owned)?
        };
        Ok(self.edge(carrier, start, end))
    }

    /// The edge of `segment` from vertex `start` to vertex `end`.
    pub(super) fn segment_edge(
        &mut self,
        segment: &Segment,
        start: usize,
        end: usize,
    ) -> Result<usize, String> {
        match *segment {
            Segment::Line { .. } => self.line(start, end),
            Segment::Arc {
                center,
                normal,
                sweep,
                ..
            } => self.arc(center, normal, start, end, sweep),
        }
    }

    /// A face on `geometry` whose rings list `(edge, coedge sense)` in
    /// traversal order; the first ring is the outer one.
    pub(super) fn face(
        &mut self,
        geometry: SolvedSurfaceGeometry,
        sense: Sense,
        rings: Vec<Vec<(usize, Sense)>>,
    ) -> Result<(), String> {
        let rings = rings.into_iter().map(|ring| (ring, Vec::new())).collect();
        self.face_with_poles(geometry, sense, rings)
    }

    /// [`Self::face`] whose rings also list their pole occurrences: the ring
    /// position of the coedge each pole follows, and the pole vertex.
    pub(super) fn face_with_poles(
        &mut self,
        geometry: SolvedSurfaceGeometry,
        sense: Sense,
        rings: Vec<PoledRing>,
    ) -> Result<(), String> {
        let key = self.key();
        let surface = SurfaceId::from(ids::csg(kind!("surface"), key.clone()));
        let face = FaceId::from(ids::csg(kind!("face"), key));
        self.surfaces.push(Surface {
            id: surface.clone(),
            geometry: SurfaceGeometry::Solved(geometry),
            source_object: None,
            native_ref: None,
        });
        let mut loop_ids = Vec::new();
        for (ring, poles) in rings {
            let loop_key = self.key();
            let loop_id = LoopId::from(ids::csg(kind!("loop"), loop_key));
            let mut coedges = Vec::new();
            for (edge, coedge_sense) in ring {
                let coedge_key = self.key();
                let coedge = CoedgeId::from(ids::csg(kind!("coedge"), coedge_key));
                self.coedges.push(Coedge {
                    id: coedge.clone(),
                    owner_loop: loop_id.clone(),
                    edge: self.edges[edge].id.clone(),
                    radial_next: coedge.clone(),
                    sense: coedge_sense,
                    pcurves: Vec::new(),
                    use_curve: None,
                    native_ref: None,
                });
                coedges.push(coedge);
            }
            let vertex_uses = poles
                .into_iter()
                .map(|(after, vertex)| AnchoredVertexUse {
                    vertex: self.vertices[vertex].id.clone(),
                    after: coedges[after].clone(),
                    pcurves: Vec::new(),
                })
                .collect();
            let ring = LoopRing::new(coedges, vertex_uses).map_err(|error| error.to_string())?;
            self.loops.push(Loop {
                id: loop_id.clone(),
                face: face.clone(),
                boundary: LoopBoundary::Ring(ring),
                native_ref: None,
            });
            loop_ids.push(loop_id);
        }
        let mut loop_ids = loop_ids.into_iter();
        let loops = match loop_ids.next() {
            Some(outer) => FaceLoops::classified(outer, loop_ids.collect()),
            None => FaceLoops::unspecified(Vec::new()),
        };
        let shell = self.shell_id();
        self.faces.push(Face {
            id: face,
            shell,
            surface,
            sense,
            loops,
            name: None,
            color: None,
            tolerance: None,
            native_ref: None,
        });
        Ok(())
    }

    /// The solid body of every face built so far. Edges that bound no face
    /// and their curves are dropped.
    pub(super) fn finish(mut self, name: Option<String>) -> Result<BuiltSolid, String> {
        let mut uses = BTreeMap::<EdgeId, Vec<usize>>::new();
        for (index, coedge) in self.coedges.iter().enumerate() {
            uses.entry(coedge.edge.clone()).or_default().push(index);
        }
        for partners in uses.values() {
            for (position, &index) in partners.iter().enumerate() {
                let next = partners[(position + 1) % partners.len()];
                self.coedges[index].radial_next = self.coedges[next].id.clone();
            }
        }
        self.edges.retain(|edge| uses.contains_key(&edge.id));
        let carriers: std::collections::BTreeSet<CurveId> = self
            .edges
            .iter()
            .filter_map(|edge| edge.carrier.curve().cloned())
            .collect();
        self.curves.retain(|curve| carriers.contains(&curve.id));
        let body = BodyId::from(ids::csg(kind!("body"), self.item));
        let region = RegionId::from(ids::csg(kind!("region"), self.item));
        let shell = self.shell_id();
        let mut identities = vec![
            body.as_str().to_owned(),
            region.as_str().to_owned(),
            shell.as_str().to_owned(),
        ];
        let faces = self.faces.len();
        let mut draft = ModelDraft::new();
        let failed = |error: cadmpeg_ir::draft::DraftError| error.to_string();
        for point in self.points {
            identities.push(point.id.as_str().to_owned());
            draft.insert(point).map_err(failed)?;
        }
        for vertex in self.vertices {
            identities.push(vertex.id.as_str().to_owned());
            draft.insert(vertex).map_err(failed)?;
        }
        for curve in self.curves {
            identities.push(curve.id.as_str().to_owned());
            draft.insert(curve).map_err(failed)?;
        }
        for edge in self.edges {
            identities.push(edge.id.as_str().to_owned());
            draft.insert(edge).map_err(failed)?;
        }
        for coedge in self.coedges {
            identities.push(coedge.id.as_str().to_owned());
            draft.insert(coedge).map_err(failed)?;
        }
        for face_loop in self.loops {
            identities.push(face_loop.id.as_str().to_owned());
            draft.insert(face_loop).map_err(failed)?;
        }
        for surface in self.surfaces {
            identities.push(surface.id.as_str().to_owned());
            draft.insert(surface).map_err(failed)?;
        }
        for face in self.faces {
            identities.push(face.id.as_str().to_owned());
            draft.insert(face).map_err(failed)?;
        }
        draft
            .insert(Shell::new(shell.clone(), region.clone()))
            .map_err(failed)?;
        draft
            .insert(Region {
                id: region,
                body: body.clone(),
                shells: vec![shell],
                native_ref: None,
            })
            .map_err(failed)?;
        draft
            .insert(Body {
                id: body.clone(),
                kind: BodyKind::Solid,
                transform: None,
                name,
                color: None,
                visible: None,
                native_ref: Some(format!("#{}", self.item)),
            })
            .map_err(failed)?;
        Ok(BuiltSolid {
            draft,
            body,
            identities,
            faces,
        })
    }
}

/// The part of `vector` perpendicular to the unit vector `normal`.
fn in_plane(vector: Vector3, normal: Vector3) -> Vector3 {
    vector - normal.scale(normal.dot(vector))
}

/// Whether `sweep` radians is a full turn.
pub(super) fn full_turn(sweep: f64) -> bool {
    sweep >= TAU - FULL_TURN_TOLERANCE
}

fn sense_of(forward: bool) -> Sense {
    if forward {
        Sense::Forward
    } else {
        Sense::Reversed
    }
}

fn plane(origin: Point3, normal: Vector3) -> Result<SolvedSurfaceGeometry, String> {
    let normal = unit(normal).ok_or("a plane has a zero normal")?;
    let reference = super::frame::perpendicular(normal).ok_or("a plane has no reference axis")?;
    PlaneSurface::try_new(origin, normal, reference)
        .map(SolvedSurfaceGeometry::Plane)
        .map_err(str::to_owned)
}

/// The edges of every profile loop, in loop order.
fn loop_edges(
    builder: &mut SolidBuilder,
    loops: &[Vec<Segment>],
) -> Result<Vec<Vec<usize>>, String> {
    let mut edges = Vec::new();
    for profile_loop in loops {
        let starts = loop_vertices(builder, profile_loop)?;
        let mut ring = Vec::new();
        for (index, segment) in profile_loop.iter().enumerate() {
            let end = starts[(index + 1) % starts.len()];
            ring.push(builder.segment_edge(segment, starts[index], end)?);
        }
        edges.push(ring);
    }
    Ok(edges)
}

/// The vertex at the start of each segment of a loop; a segment ends at the
/// next segment's start vertex.
fn loop_vertices(
    builder: &mut SolidBuilder,
    profile_loop: &[Segment],
) -> Result<Vec<usize>, String> {
    profile_loop
        .iter()
        .map(|segment| builder.vertex(segment.start()))
        .collect()
}

/// The ring that traverses every loop edge forward.
fn forward_rings(edges: &[Vec<usize>]) -> Vec<Vec<(usize, Sense)>> {
    edges
        .iter()
        .map(|ring| ring.iter().map(|&edge| (edge, Sense::Forward)).collect())
        .collect()
}

/// The rings that traverse every loop edge backward, in reverse order.
fn backward_rings(edges: &[Vec<usize>]) -> Vec<Vec<(usize, Sense)>> {
    edges
        .iter()
        .map(|ring| {
            ring.iter()
                .rev()
                .map(|&edge| (edge, Sense::Reversed))
                .collect()
        })
        .collect()
}

/// The solid swept by translating `profile` along the unit `direction` by
/// `depth`. The profile's plane must not contain the direction.
pub(super) fn extrude(
    builder: &mut SolidBuilder,
    profile: &Profile,
    direction: Vector3,
    depth: f64,
) -> Result<(), String> {
    let along = profile.normal.dot(direction);
    if along == 0.0 {
        return Err("the extrusion direction lies in the profile plane".to_owned());
    }
    let bottom_normal = profile.normal.scale(-along.signum());
    let (origin, normal) = (profile.origin, profile.normal);
    let profile = profile
        .mapped(
            origin,
            normal,
            |point| point.translated(normal, -normal.dot(point.vector_from(origin))),
            |_| None,
        )
        .oriented(bottom_normal);
    let offset = direction.scale(depth);
    let place = |point: Point3| point.translated(offset, 1.0);
    let keep = |vector: Vector3| vector;
    let top_loops: Vec<Vec<Segment>> = profile
        .loops
        .iter()
        .map(|profile_loop| {
            profile_loop
                .iter()
                .map(|segment| segment.moved(&place, &keep))
                .collect()
        })
        .collect();
    let bottom = loop_edges(builder, &profile.loops)?;
    let top = loop_edges(builder, &top_loops)?;
    builder.face(
        plane(profile.origin, bottom_normal)?,
        Sense::Forward,
        forward_rings(&bottom),
    )?;
    builder.face(
        plane(place(profile.origin), bottom_normal.scale(-1.0))?,
        Sense::Forward,
        backward_rings(&top),
    )?;
    for (loop_index, profile_loop) in profile.loops.iter().enumerate() {
        let count = profile_loop.len();
        let mut rails = Vec::with_capacity(count);
        for segment in profile_loop {
            let start = builder.vertex(segment.start())?;
            let end = builder.vertex(place(segment.start()))?;
            rails.push(builder.line(start, end)?);
        }
        for (index, segment) in profile_loop.iter().enumerate() {
            let tangent = segment
                .tangent_at(0.5)
                .ok_or("a profile segment has no direction")?;
            let outward = direction.cross(tangent);
            let geometry = match *segment {
                Segment::Line { start, .. } => plane(start, outward)?,
                Segment::Arc {
                    center,
                    normal,
                    radius,
                    ..
                } => {
                    let parallel = unit(normal).map_or(0.0, |normal| normal.dot(direction).abs());
                    if (1.0 - parallel).abs() > ARC_AXIS_PARALLEL_TOLERANCE {
                        return Err(
                            "an arc extruded off its circle's axis sweeps an elliptic cylinder"
                                .to_owned(),
                        );
                    }
                    let radial = in_plane(segment.point_at(0.5).vector_from(center), direction);
                    let reference = unit(radial).ok_or("an arc has zero radius")?;
                    let surface = CylinderSurface::try_new(center, direction, reference, radius)
                        .map_err(str::to_owned)?;
                    let forward = outward.dot(radial) > 0.0;
                    builder.face(
                        SolvedSurfaceGeometry::Cylinder(surface),
                        sense_of(forward),
                        vec![side_ring(
                            bottom[loop_index][index],
                            rails[index],
                            top[loop_index][index],
                            rails[(index + 1) % count],
                        )],
                    )?;
                    continue;
                }
            };
            builder.face(
                geometry,
                Sense::Forward,
                vec![side_ring(
                    bottom[loop_index][index],
                    rails[index],
                    top[loop_index][index],
                    rails[(index + 1) % count],
                )],
            )?;
        }
    }
    Ok(())
}

/// The ring of a swept side face: the profile segment backward, the start
/// rail forward, the moved segment forward, and the end rail backward.
fn side_ring(
    segment: usize,
    start_rail: usize,
    moved: usize,
    end_rail: usize,
) -> Vec<(usize, Sense)> {
    vec![
        (segment, Sense::Reversed),
        (start_rail, Sense::Forward),
        (moved, Sense::Forward),
        (end_rail, Sense::Reversed),
    ]
}

/// The axis of a revolution: a point and a unit direction.
#[derive(Debug, Clone, Copy)]
pub(super) struct Axis {
    pub(super) point: Point3,
    pub(super) direction: Vector3,
}

impl Axis {
    fn radial(&self, point: Point3) -> Vector3 {
        let offset = point.vector_from(self.point);
        offset - self.direction.scale(offset.dot(self.direction))
    }

    fn height(&self, point: Point3) -> f64 {
        point.vector_from(self.point).dot(self.direction)
    }

    fn foot(&self, point: Point3) -> Point3 {
        self.point.translated(self.direction, self.height(point))
    }
}

/// The solid swept by turning `profile` about `axis` through `angle`
/// radians, counterclockwise about the axis direction. The profile lies in a
/// plane through the axis, on one side of it.
pub(super) fn revolve(
    builder: &mut SolidBuilder,
    profile: &Profile,
    axis: Axis,
    angle: f64,
) -> Result<(), String> {
    let samples = profile.sample_points();
    let away = samples
        .iter()
        .map(|point| axis.radial(*point))
        .max_by(|left, right| left.norm().total_cmp(&right.norm()))
        .and_then(unit)
        .ok_or("the profile lies on the revolution axis")?;
    let tolerance = builder
        .tolerance
        .max(PROFILE_RELATIVE_RESOLUTION * diagonal(&samples));
    let sweep_direction = axis.direction.cross(away);
    for point in &samples {
        let offset = point.vector_from(axis.point);
        if offset.dot(sweep_direction).abs() > tolerance {
            return Err("the profile plane does not contain the revolution axis".to_owned());
        }
        if axis.radial(*point).dot(away) < -tolerance {
            return Err("the profile crosses the revolution axis".to_owned());
        }
    }
    let profile = meridian(profile, axis, away, tolerance).oriented(sweep_direction.scale(-1.0));
    let full = full_turn(angle);
    let place = |point: Point3| rotate_about(point, axis.point, axis.direction, angle);
    let spin = |vector: Vector3| turn(vector, axis.direction, angle);
    let on_axis = |point: Point3| axis.radial(point).norm() <= tolerance;
    let start = loop_edges(builder, &profile.loops)?;
    let end = if full {
        start.clone()
    } else {
        let mut end = Vec::with_capacity(start.len());
        for (profile_loop, start_ring) in profile.loops.iter().zip(&start) {
            let moved: Vec<Segment> = profile_loop
                .iter()
                .map(|segment| segment.moved(&place, &spin))
                .collect();
            let starts = loop_vertices(builder, &moved)?;
            let mut ring = Vec::with_capacity(start_ring.len());
            for (index, (segment, &edge)) in profile_loop.iter().zip(start_ring).enumerate() {
                let fixed = matches!(segment, Segment::Line { .. })
                    && on_axis(segment.start())
                    && on_axis(segment.end());
                ring.push(if fixed {
                    edge
                } else {
                    builder.segment_edge(
                        &moved[index],
                        starts[index],
                        starts[(index + 1) % starts.len()],
                    )?
                });
            }
            end.push(ring);
        }
        end
    };
    if !full {
        let cap_normal = sweep_direction.scale(-1.0);
        builder.face(
            plane(profile.origin, cap_normal)?,
            Sense::Forward,
            forward_rings(&start),
        )?;
        builder.face(
            plane(place(profile.origin), spin(cap_normal).scale(-1.0))?,
            Sense::Forward,
            backward_rings(&end),
        )?;
    }
    for (loop_index, profile_loop) in profile.loops.iter().enumerate() {
        let count = profile_loop.len();
        let mut rails = Vec::with_capacity(count);
        for segment in profile_loop {
            let point = segment.start();
            if on_axis(point) {
                rails.push(None);
                continue;
            }
            let from = builder.vertex(point)?;
            let to = if full {
                from
            } else {
                builder.vertex(place(point))?
            };
            rails.push(Some(builder.arc(
                axis.foot(point),
                axis.direction,
                from,
                to,
                angle,
            )?));
        }
        for (index, segment) in profile_loop.iter().enumerate() {
            if matches!(segment, Segment::Line { .. })
                && on_axis(segment.start())
                && on_axis(segment.end())
            {
                continue;
            }
            let start_pole = match rails[index] {
                Some(_) => None,
                None => Some(builder.vertex(segment.start())?),
            };
            let end_pole = match rails[(index + 1) % count] {
                Some(_) => None,
                None => Some(builder.vertex(segment.end())?),
            };
            let mut ring = vec![(start[loop_index][index], Sense::Reversed, start_pole)];
            if let Some(rail) = rails[index] {
                ring.push((rail, Sense::Forward, None));
            }
            ring.push((end[loop_index][index], Sense::Forward, end_pole));
            if let Some(rail) = rails[(index + 1) % count] {
                ring.push((rail, Sense::Reversed, None));
            }
            if end_pole.is_some() && start_pole.is_none() {
                ring.rotate_left(1);
            }
            let poles = ring
                .iter()
                .enumerate()
                .filter_map(|(position, entry)| entry.2.map(|pole| (position, pole)))
                .collect();
            let coedges = ring.iter().map(|entry| (entry.0, entry.1)).collect();
            let (geometry, sense) = revolved_surface(segment, axis, sweep_direction, tolerance)?;
            builder.face_with_poles(geometry, sense, vec![(coedges, poles)])?;
        }
    }
    Ok(())
}

/// The diagonal of the box that holds `points`.
fn diagonal(points: &[Point3]) -> f64 {
    let mut low = [f64::INFINITY; 3];
    let mut high = [f64::NEG_INFINITY; 3];
    for point in points {
        for (axis, value) in [point.x, point.y, point.z].into_iter().enumerate() {
            low[axis] = low[axis].min(value);
            high[axis] = high[axis].max(value);
        }
    }
    (0..3)
        .map(|axis| (high[axis] - low[axis]).max(0.0).powi(2))
        .sum::<f64>()
        .sqrt()
}

/// The profile in the half-plane of `axis` toward `away`, with each
/// distance from the axis and each height along it merged with an equal one
/// within `tolerance`, and a distance within `tolerance` of zero on the axis.
fn meridian(profile: &Profile, axis: Axis, away: Vector3, tolerance: f64) -> Profile {
    let mut radii = vec![0.0];
    let mut heights = Vec::new();
    let snap = |values: &mut Vec<f64>, value: f64| {
        if let Some(known) = values
            .iter()
            .find(|known| (*known - value).abs() <= tolerance)
        {
            return *known;
        }
        values.push(value);
        value
    };
    let mut points = profile.sample_points();
    for segment in profile.loops.iter().flatten() {
        if let Segment::Arc { center, .. } = segment {
            points.push(*center);
        }
    }
    let mut table = Vec::new();
    for point in points {
        let radius = snap(&mut radii, axis.radial(point).norm());
        let height = snap(&mut heights, axis.height(point));
        table.push((point, radius, height));
    }
    let place = |point: Point3| {
        let radius = table
            .iter()
            .find(|(known, ..)| known.distance(point) == 0.0)
            .map_or_else(|| axis.radial(point).norm(), |entry| entry.1);
        let height = table
            .iter()
            .find(|(known, ..)| known.distance(point) == 0.0)
            .map_or_else(|| axis.height(point), |entry| entry.2);
        axis.point
            .translated(axis.direction, height)
            .translated(away, radius)
    };
    let on_axis =
        |point: Point3| (axis.radial(place(point)).norm() <= tolerance).then_some(axis.direction);
    profile.mapped(axis.point, axis.direction.cross(away), place, on_axis)
}

/// The surface that `segment` sweeps about `axis`, and the face sense that
/// points it out of the solid.
fn revolved_surface(
    segment: &Segment,
    axis: Axis,
    sweep_direction: Vector3,
    tolerance: f64,
) -> Result<(SolvedSurfaceGeometry, Sense), String> {
    let tangent = segment
        .tangent_at(0.5)
        .ok_or("a profile segment has no direction")?;
    let outward = sweep_direction.cross(tangent);
    let middle = segment.point_at(0.5);
    let reference = unit(axis.radial(middle)).ok_or("a profile segment lies on the axis")?;
    match *segment {
        Segment::Line { start, end } => {
            let (radius_start, radius_end) = (axis.radial(start).norm(), axis.radial(end).norm());
            let (height_start, height_end) = (axis.height(start), axis.height(end));
            if (height_end - height_start).abs() <= tolerance {
                let normal = axis.direction;
                let surface = PlaneSurface::try_new(axis.foot(start), normal, reference)
                    .map_err(str::to_owned)?;
                return Ok((
                    SolvedSurfaceGeometry::Plane(surface),
                    sense_of(outward.dot(normal) > 0.0),
                ));
            }
            if (radius_end - radius_start).abs() <= tolerance {
                let surface =
                    CylinderSurface::try_new(axis.point, axis.direction, reference, radius_start)
                        .map_err(str::to_owned)?;
                return Ok((
                    SolvedSurfaceGeometry::Cylinder(surface),
                    sense_of(outward.dot(reference) > 0.0),
                ));
            }
            let slope = (radius_end - radius_start) / (height_end - height_start);
            let surface = ConeSurface::try_new(
                axis.foot(start),
                axis.direction,
                reference,
                radius_start,
                1.0,
                slope.atan(),
            )
            .map_err(str::to_owned)?;
            let natural = reference - axis.direction.scale(slope);
            Ok((
                SolvedSurfaceGeometry::Cone(surface),
                sense_of(outward.dot(natural) > 0.0),
            ))
        }
        Segment::Arc { center, radius, .. } => {
            let natural = middle.vector_from(center);
            let sense = sense_of(outward.dot(natural) > 0.0);
            let major = axis.radial(center).norm();
            if major <= tolerance {
                let surface = SphereSurface::try_new(center, axis.direction, reference, radius)
                    .map_err(str::to_owned)?;
                return Ok((SolvedSurfaceGeometry::Sphere(surface), sense));
            }
            let tube_reference =
                unit(axis.radial(center)).ok_or("an arc centre lies on the axis")?;
            let surface = TorusSurface::try_new(
                axis.foot(center),
                axis.direction,
                tube_reference,
                major,
                radius,
            )
            .map_err(str::to_owned)?;
            Ok((SolvedSurfaceGeometry::Torus(surface), sense))
        }
    }
}

/// The planar profile in the frame's xz half-plane `x ≥ 0` through the
/// points `(x, z)`, in order, as straight segments.
pub(super) fn polygon_profile(frame: &Frame, corners: &[[f64; 2]]) -> Profile {
    let points: Vec<Point3> = corners
        .iter()
        .map(|&[x, z]| frame.point(x, 0.0, z))
        .collect();
    let count = points.len();
    let loop_segments = (0..count)
        .map(|index| Segment::Line {
            start: points[index],
            end: points[(index + 1) % count],
        })
        .collect();
    Profile {
        origin: frame.origin,
        normal: frame.y,
        loops: vec![loop_segments],
    }
}

/// The profile in the frame's xy plane through the points `(x, y)`.
pub(super) fn base_polygon(frame: &Frame, corners: &[[f64; 2]]) -> Profile {
    let points: Vec<Point3> = corners
        .iter()
        .map(|&[x, y]| frame.point(x, y, 0.0))
        .collect();
    let count = points.len();
    let loop_segments = (0..count)
        .map(|index| Segment::Line {
            start: points[index],
            end: points[(index + 1) % count],
        })
        .collect();
    Profile {
        origin: frame.origin,
        normal: frame.z,
        loops: vec![loop_segments],
    }
}

/// The disc of `radius` about the frame origin in its xy plane.
pub(super) fn disc_profile(frame: &Frame, radius: f64) -> Profile {
    Profile {
        origin: frame.origin,
        normal: frame.z,
        loops: vec![vec![Segment::Arc {
            center: frame.origin,
            normal: frame.z,
            radius,
            start: frame.point(radius, 0.0, 0.0),
            sweep: TAU,
        }]],
    }
}

/// The half-disc of `radius` about the frame origin in its xz half-plane
/// `x ≥ 0`, bounded by the z axis.
pub(super) fn half_disc_profile(frame: &Frame, radius: f64) -> Profile {
    let south = frame.point(0.0, 0.0, -radius);
    let north = frame.point(0.0, 0.0, radius);
    Profile {
        origin: frame.origin,
        normal: frame.y,
        loops: vec![vec![
            Segment::Arc {
                center: frame.origin,
                normal: frame.y.scale(-1.0),
                radius,
                start: south,
                sweep: std::f64::consts::PI,
            },
            Segment::Line {
                start: north,
                end: south,
            },
        ]],
    }
}

/// The disc of `radius` about frame point `(x, 0, z)` in the frame's xz
/// plane.
pub(super) fn tube_profile(frame: &Frame, center_x: f64, radius: f64) -> Profile {
    let center = frame.point(center_x, 0.0, 0.0);
    Profile {
        origin: frame.origin,
        normal: frame.y,
        loops: vec![vec![Segment::Arc {
            center,
            normal: frame.y,
            radius,
            start: frame.point(center_x + radius, 0.0, 0.0),
            sweep: TAU,
        }]],
    }
}

/// The revolution axis along the frame's z axis.
pub(super) fn frame_axis(frame: &Frame) -> Axis {
    Axis {
        point: frame.origin,
        direction: frame.z,
    }
}
