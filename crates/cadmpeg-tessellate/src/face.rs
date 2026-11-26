// SPDX-License-Identifier: Apache-2.0
//! The tessellation of one face.

use std::collections::BTreeSet;
use std::f64::consts::TAU;

use cadmpeg_ir::ids::{CoedgeId, FaceId};
use cadmpeg_ir::index::StandardIndex;
use cadmpeg_ir::math::{Point3, Vector3};
use cadmpeg_ir::topology::{Face, FaceLoops, LoopBoundary, Sense};

use crate::boundary::{trace, EdgeCache, Sampler, Traced, TracedLoop};
use crate::domain::{clockwise_on_open_carrier, regions, reversed, Frame, Node, Region, Ring};
use crate::plane::{cross, distance, dot, length, midpoint, scaled, sub, unit, Uv};
use crate::refine::{Limits, Mesh};
use crate::surface::Carrier;
use crate::triangulate::{triangulate, Rings};
use crate::Tessellator;
use cadmpeg_core::decode::WorkBudget;

/// Ratio of a parameter gap between two joined coedges to the size of
/// their parameters above which the gap is a jump across a pole.
const JUNCTION_RATIO: f64 = 1e-7;
/// Evaluation work one face may spend.
const FACE_WORK: usize = 1 << 28;
/// Share of a mesh's area in triangles that face away from the surface
/// above which a face whose evaluation work ran out is refused.
const FOLDED_SHARE: f64 = 1e-3;
/// Deepest bisection of one derived pcurve segment.
const PCURVE_DEPTH: u32 = 24;
/// Largest sample count of one derived pcurve.
const MAX_PCURVE_SAMPLES: usize = 1 << 16;
/// Ratio of a triangle's model area to the squared surface size below which
/// it is dropped as degenerate.
const DEGENERATE_AREA_RATIO: f64 = 1e-20;
/// Ratio of a triangle's spread along one parameter to its spread along
/// the other, both in the surface metric, below which its corners lie on
/// one line of constant parameter.
const ISO_LINE_RATIO: f64 = 1e-9;

/// Why a face has no tessellation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Refusal {
    reason: String,
    misoriented: bool,
}

impl Refusal {
    pub(crate) fn new(reason: impl Into<String>) -> Self {
        Self {
            reason: reason.into(),
            misoriented: false,
        }
    }

    /// A refusal of loops whose orientation, taken from the face sense,
    /// bounds no domain.
    pub(crate) fn misoriented(reason: impl Into<String>) -> Self {
        Self {
            reason: reason.into(),
            misoriented: true,
        }
    }

    /// The reason.
    #[must_use]
    pub fn reason(&self) -> &str {
        &self.reason
    }
}

impl std::fmt::Display for Refusal {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(&self.reason)
    }
}

impl std::error::Error for Refusal {}

/// One coedge of a face in the face's parameter plane, in traversal order.
#[derive(Debug, Clone, PartialEq)]
pub struct CoedgeTrace {
    /// The traced coedge.
    pub coedge: CoedgeId,
    /// Parameters on the face's surface. A seam traced twice by one loop
    /// lies a whole period apart in its two uses.
    pub uv: Vec<[f64; 2]>,
    /// The model points of the samples: edge-curve points, or the pole of a
    /// degenerate edge.
    pub points: Vec<Point3>,
    /// The parameters come from the coedge's own pcurve.
    pub from_pcurve: bool,
}

/// The triangles of one face.
#[derive(Debug, Clone, PartialEq)]
pub struct FaceMesh {
    /// Vertex positions: edge samples on the boundary, surface points
    /// inside.
    pub positions: Vec<Point3>,
    /// Unit normals, oriented with the face.
    pub normals: Vec<Vector3>,
    /// Counterclockwise about the face's oriented normal.
    pub triangles: Vec<[usize; 3]>,
    /// The coedges of every loop, in loop order.
    pub coedges: Vec<CoedgeTrace>,
    /// The largest distance between a boundary sample and the surface.
    pub deviation: f64,
}

/// A ring of domain nodes from a traced loop, oriented so that the face lies
/// on its left.
fn ring(traced: &TracedLoop<'_>, left: bool) -> Ring {
    let mut nodes = Vec::new();
    let count = traced.coedges.len();
    for (position, coedge) in traced.coedges.iter().enumerate() {
        let next = traced.coedges[(position + 1) % count].uv.first().copied();
        let samples = coedge.uv.len();
        for (at, (uv, point)) in coedge.uv.iter().zip(&coedge.points).enumerate() {
            let last = at + 1 == samples;
            // A junction kept on both branches of a pole joins them along
            // the pole line.
            let jump = last
                && (position + 1 < count || traced.closes_at_pole)
                && next.is_some_and(|next| {
                    let scale = 1.0 + uv[0].abs() + uv[1].abs();
                    (next[0] - uv[0]).abs().max((next[1] - uv[1]).abs()) > JUNCTION_RATIO * scale
                });
            if last && !jump {
                continue;
            }
            nodes.push(Node {
                uv: *uv,
                point: Some(*point),
                open: coedge.degenerate || jump,
            });
        }
    }
    if left {
        return Ring {
            nodes,
            closure: traced.closure,
        };
    }
    Ring {
        nodes: reversed(&nodes),
        closure: [-traced.closure[0], -traced.closure[1]],
    }
}

pub(crate) fn tessellate(
    tessellator: &mut Tessellator<'_>,
    face_id: &FaceId,
) -> Result<FaceMesh, Refusal> {
    tessellate_within(tessellator, face_id, FACE_WORK)
}

/// Tessellates `face_id` with `limit` units of evaluation work.
pub(crate) fn tessellate_within(
    tessellator: &mut Tessellator<'_>,
    face_id: &FaceId,
    limit: usize,
) -> Result<FaceMesh, Refusal> {
    let index = &tessellator.index;
    let options = tessellator.options;
    let face = index
        .faces(face_id.as_str(), StandardIndex)
        .ok_or_else(|| Refusal::new(format!("the face {face_id} is not in the model")))?;
    let work = WorkBudget::new(limit);
    let carrier = Carrier::new(index, &work, &face.surface)?;
    let sense = if face.sense == Sense::Reversed {
        -1.0
    } else {
        1.0
    };
    let mut orientation = sense * carrier.normal_sign;
    let sampler = Sampler {
        index,
        work: &work,
        chord: options.chord,
        angle: options.angle,
    };
    // Loops that bound no domain in the orientation of the face sense, and
    // bound one in the opposite orientation, orient the face.
    let (traced_loops, regions) = match layout(
        &sampler,
        &mut tessellator.edges,
        &carrier,
        face,
        orientation > 0.0,
    ) {
        Err(refusal) if refusal.misoriented => {
            let laid = layout(
                &sampler,
                &mut tessellator.edges,
                &carrier,
                face,
                orientation < 0.0,
            )
            .map_err(|_| refusal)?;
            orientation = -orientation;
            laid
        }
        laid => laid?,
    };
    let mut mesh = Mesh {
        uv: Vec::new(),
        points: Vec::new(),
        normals: Vec::new(),
        scales: Vec::new(),
        triangles: Vec::new(),
        fixed: BTreeSet::new(),
        anisotropy: [1.0, 1.0],
    };
    for region in regions {
        let mut rings_by_index = Vec::new();
        for nodes in std::iter::once(&region.outer).chain(&region.holes) {
            let start = mesh.uv.len();
            for node in nodes {
                let point = match node.point {
                    Some(point) => point,
                    None => carrier
                        .point(node.uv)?
                        .ok_or_else(|| Refusal::new("a domain corner has no surface point"))?,
                };
                mesh.uv.push(node.uv);
                mesh.points.push(point);
                mesh.normals.push(carrier.normal(node.uv)?);
                mesh.scales.push(crate::refine::scales(&carrier, node.uv)?);
            }
            let ring: Vec<usize> = (start..mesh.uv.len()).collect();
            for (at, node) in nodes.iter().enumerate() {
                if !node.open {
                    let from = ring[at];
                    let to = ring[(at + 1) % ring.len()];
                    mesh.fixed.insert((from.min(to), from.max(to)));
                }
            }
            rings_by_index.push(ring);
        }
        let outer = rings_by_index.remove(0);
        let triangles = triangulate(Rings {
            points: &mesh.uv,
            outer,
            holes: rings_by_index,
        })?;
        mesh.triangles.extend(triangles.into_iter().map(Some));
    }
    mesh.flip();
    let limits = Limits {
        chord: options.chord,
        angle: options.angle,
        max_vertices: options.max_vertices,
    };
    let unrefined = mesh.clone();
    if !carrier.planar() {
        mesh.seed(&carrier, &limits, true)?;
    }
    mesh.refine(&carrier, &limits)?;
    mesh.flip();
    // Triangles stretched along a direction the surface turns out to bend
    // in can fold the mesh over; the face then refines in the surface
    // metric.
    if mesh.stretched() && mesh.folded() {
        mesh = unrefined;
        if !carrier.planar() {
            mesh.seed(&carrier, &limits, false)?;
        }
        mesh.refine(&carrier, &limits)?;
        mesh.flip();
    }
    // Refinement reads a surface point that the spent budget withholds as
    // one with no error; a mesh it left folded misses its surface.
    if (work.exhausted() || work.remaining() == 0) && mesh.folded_share() > FOLDED_SHARE {
        return Err(Refusal::new(
            "the face's evaluation work ran out and left its mesh folded",
        ));
    }
    let mut coedges = Vec::new();
    let mut deviation = 0.0_f64;
    for traced in &traced_loops {
        for coedge in &traced.coedges {
            let derived = match options.pcurve_tolerance {
                Some(tolerance) if !coedge.from_pcurve && !coedge.degenerate => {
                    derive(&sampler, &carrier, coedge, tolerance)?
                }
                _ => (coedge.uv.clone(), coedge.points.clone()),
            };
            for (uv, point) in derived.0.iter().zip(&derived.1) {
                if coedge.degenerate {
                    continue;
                }
                if let Some((gap, _)) = carrier.deviation(*point, *uv)? {
                    deviation = deviation.max(gap);
                }
            }
            coedges.push(CoedgeTrace {
                coedge: coedge.coedge.id.clone(),
                uv: derived.0,
                points: derived.1,
                from_pcurve: coedge.from_pcurve,
            });
        }
    }
    Ok(finish(&carrier, mesh, orientation, coedges, deviation))
}

/// The traced loops of `face` and the regions they bound, with the face on
/// the left of each loop when `left` holds.
fn layout<'ir>(
    sampler: &Sampler<'_, 'ir>,
    cache: &mut EdgeCache,
    carrier: &Carrier<'_, 'ir>,
    face: &'ir Face,
    left: bool,
) -> Result<(Vec<TracedLoop<'ir>>, Vec<Region>), Refusal> {
    let index = sampler.index;
    let mut traced_loops = Vec::new();
    let mut declared_outer = None;
    let mut poles: Vec<(Uv, Option<Point3>)> = Vec::new();
    let boundary = face.loops.boundary().ok_or_else(|| {
        Refusal::new(format!(
            "the face {} does not state its boundary in full, so its extent is not stated",
            face.id
        ))
    })?;
    for loop_id in boundary {
        let loop_ = index
            .loops(loop_id.as_str(), StandardIndex)
            .ok_or_else(|| Refusal::new(format!("the loop {loop_id} is not in the model")))?;
        match &loop_.boundary {
            LoopBoundary::Vertex { vertex, .. } => {
                let vertex = index
                    .vertices(vertex.as_str(), StandardIndex)
                    .and_then(|vertex| index.points(vertex.point.as_str(), StandardIndex));
                if let Some(point) = vertex {
                    let point = point.position().get();
                    if let Some(uv) = carrier.invert(point, None)? {
                        poles.push((uv, Some(point)));
                    }
                }
            }
            LoopBoundary::Ring(ring) => {
                let coedges = ring
                    .coedges()
                    .iter()
                    .map(|id| {
                        index.coedges(id.as_str(), StandardIndex).ok_or_else(|| {
                            Refusal::new(format!("the coedge {id} is not in the model"))
                        })
                    })
                    .collect::<Result<Vec<_>, _>>()?;
                if matches!(&face.loops, FaceLoops::Classified { outer, .. } if outer == loop_id) {
                    declared_outer = Some(traced_loops.len());
                }
                traced_loops.push(trace(sampler, cache, carrier, &coedges, left)?);
            }
        }
    }
    if let Some(apex) = carrier.apex {
        poles.push(([0.0, apex], carrier.point([0.0, apex])?));
    }
    for &circle in &carrier.pole_circles {
        let point = carrier.point([0.0, circle])?;
        for turn in [-TAU, 0.0, TAU] {
            poles.push(([0.0, circle + turn], point));
        }
    }
    let frame = Frame {
        periods: carrier.periods,
        domain: carrier.domain,
        poles,
        narrow: sampler.chord,
    };
    let rings: Vec<Ring> = traced_loops
        .iter()
        .map(|traced| ring(traced, left))
        .collect();
    if let Some(outer) = declared_outer.and_then(|at| rings.get(at)) {
        if clockwise_on_open_carrier(outer, &frame)? {
            return Err(Refusal::misoriented(
                "the declared outer loop runs clockwise around the face",
            ));
        }
    }
    Ok((traced_loops, regions(rings, &frame)?))
}

/// The derived pcurve of a coedge: its trace bisected until each segment
/// mapped onto the surface lies within `tolerance` of the edge curve.
fn derive(
    sampler: &Sampler<'_, '_>,
    carrier: &Carrier<'_, '_>,
    traced: &Traced<'_>,
    tolerance: f64,
) -> Result<(Vec<Uv>, Vec<Point3>), Refusal> {
    let Some(edge) = sampler
        .index
        .edges(traced.coedge.edge.as_str(), StandardIndex)
    else {
        return Ok((traced.uv.clone(), traced.points.clone()));
    };
    let Some(curve) = edge.curve() else {
        return Ok((traced.uv.clone(), traced.points.clone()));
    };
    let derivation = Derivation {
        sampler,
        carrier,
        curve,
        tolerance,
    };
    let mut out = (vec![traced.uv[0]], vec![traced.points[0]]);
    for at in 1..traced.uv.len() {
        let a = (
            traced.params[at - 1],
            traced.uv[at - 1],
            traced.points[at - 1],
        );
        let b = (traced.params[at], traced.uv[at], traced.points[at]);
        derivation.bisect(a, b, 0, &mut out)?;
        if out.0.len() > MAX_PCURVE_SAMPLES {
            return Ok((traced.uv.clone(), traced.points.clone()));
        }
    }
    Ok(out)
}

/// The fixed inputs of one derived pcurve.
struct Derivation<'s, 'a, 'b, 'c, 'd> {
    sampler: &'s Sampler<'a, 'b>,
    carrier: &'s Carrier<'c, 'd>,
    curve: &'s cadmpeg_ir::ids::CurveId,
    tolerance: f64,
}

impl Derivation<'_, '_, '_, '_, '_> {
    fn bisect(
        &self,
        a: (f64, Uv, Point3),
        b: (f64, Uv, Point3),
        depth: u32,
        out: &mut (Vec<Uv>, Vec<Point3>),
    ) -> Result<(), Refusal> {
        let middle = f64::midpoint(a.0, b.0);
        let seed = midpoint(a.1, b.1);
        let Some(on_curve) = self.sampler.curve_point(self.curve, middle)? else {
            keep(b, out);
            return Ok(());
        };
        let Some(on_chord) = self.carrier.point(seed)? else {
            keep(b, out);
            return Ok(());
        };
        let projected = self.carrier.project(on_curve, seed)?.unwrap_or(seed);
        let projected = [
            self.carrier.nearest_branch(0, projected[0], seed[0]),
            self.carrier.nearest_branch(1, projected[1], seed[1]),
        ];
        // A curve that lies off the surface by itself cannot be followed more
        // closely than that distance.
        let off = self
            .carrier
            .point(projected)?
            .map_or(0.0, |on_surface| distance(on_curve, on_surface));
        match distance(on_curve, on_chord) {
            gap if gap > self.tolerance + off && depth < PCURVE_DEPTH => {
                let mid = (middle, projected, on_curve);
                self.bisect(a, mid, depth + 1, out)?;
                self.bisect(mid, b, depth + 1, out)
            }
            _ => {
                keep(b, out);
                Ok(())
            }
        }
    }
}

fn keep(b: (f64, Uv, Point3), out: &mut (Vec<Uv>, Vec<Point3>)) {
    out.0.push(b.1);
    out.1.push(b.2);
}

/// Whether the corners of triangle `corners` lie on one line of constant
/// `u` or constant `v`, judged in the surface metric at those corners. On
/// such a line the parameter chord is the line's own trace, so the triangle
/// covers no part of the face, though its corners can span a curve on the
/// surface, as along a seam.
fn on_iso_line(uv: &[Uv], scales: &[[f64; 2]], corners: [usize; 3]) -> bool {
    let mut spread = [0.0_f64; 2];
    for (axis, width) in spread.iter_mut().enumerate() {
        let metric = corners
            .iter()
            .map(|corner| scales[*corner][axis])
            .sum::<f64>()
            / 3.0;
        let values = corners.map(|corner| uv[corner][axis] * metric);
        let low = values.iter().copied().fold(f64::INFINITY, f64::min);
        let high = values.iter().copied().fold(f64::NEG_INFINITY, f64::max);
        *width = high - low;
    }
    (0..2).any(|axis| spread[axis] <= ISO_LINE_RATIO * spread[1 - axis])
}

/// The output mesh: triangles wound about the oriented normal, unit vertex
/// normals, and no degenerate triangles.
fn finish(
    carrier: &Carrier<'_, '_>,
    mesh: Mesh,
    orientation: f64,
    coedges: Vec<CoedgeTrace>,
    deviation: f64,
) -> FaceMesh {
    let smallest = DEGENERATE_AREA_RATIO * carrier.size * carrier.size;
    let mut triangles = Vec::new();
    let mut accumulated: Vec<Vector3> = vec![Vector3::new(0.0, 0.0, 0.0); mesh.points.len()];
    for [a, b, c] in mesh.triangles.into_iter().flatten() {
        let (pa, pb, pc) = (mesh.points[a], mesh.points[b], mesh.points[c]);
        let normal = cross(sub(pb, pa), sub(pc, pa));
        if length(normal) <= smallest || on_iso_line(&mesh.uv, &mesh.scales, [a, b, c]) {
            continue;
        }
        let triangle = if orientation > 0.0 {
            [a, b, c]
        } else {
            [a, c, b]
        };
        let facing = scaled(normal, orientation);
        for corner in [a, b, c] {
            let sum = accumulated[corner];
            accumulated[corner] =
                Vector3::new(sum.x + facing.x, sum.y + facing.y, sum.z + facing.z);
        }
        triangles.push(triangle);
    }
    let normals = mesh
        .normals
        .iter()
        .zip(&accumulated)
        .map(|(surface, sum)| {
            let fallback = unit(*sum).unwrap_or(Vector3::new(0.0, 0.0, 1.0));
            match surface.and_then(unit) {
                Some(normal) => {
                    let oriented = scaled(normal, orientation);
                    if dot(oriented, fallback) < 0.0 && length(*sum) > 0.0 {
                        scaled(oriented, -1.0)
                    } else {
                        oriented
                    }
                }
                None => fallback,
            }
        })
        .collect();
    FaceMesh {
        positions: mesh.points,
        normals,
        triangles,
        coedges,
        deviation,
    }
}

#[cfg(test)]
mod tests;
