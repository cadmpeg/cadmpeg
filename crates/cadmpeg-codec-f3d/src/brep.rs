// SPDX-License-Identifier: Apache-2.0
//! Build B-rep topology and geometry from a framed SAB record table.
//!
//! [`decode`] follows the topology chain from bodies through vertices and
//! points. It creates analytic carriers for planes, cylinders, cones, spheres,
//! tori, lines, circles, and ellipses. [`crate::nurbs`] supplies cached NURBS
//! surfaces, 3D curves, and pcurves for spline and procedural records.
//!
//! Faces retain their loops and trims when a referenced surface has no decoded
//! shape; the emitted [`SurfaceGeometry::Unknown`] links to the corresponding
//! [`UnknownRecord`]. Edges retain vertices and parameter ranges when their 3D
//! curve carrier is unavailable. [`Stats`] records these transfer losses for
//! the decode report.
//!
//! ASM model-space lengths become millimetres. Unit vectors, ratios, angles,
//! knots, weights, and UV parameters keep their native scale.

use std::collections::{HashMap, HashSet};

use crate::records::{
    BodyNativeKey, CreationTimestamp, EdgeContinuity, EdgeOwnership, FaceContainment,
    FaceSidedness, PersistentDesignLink, SketchCurveLink, TolerantCoedgeParameters,
    TolerantVertexTail, TransformHints, VertexOwnership, WireSide, WireTopology,
};
use cadmpeg_ir::attributes::{AttributeTarget, AttributeValue, SourceAttribute};
use cadmpeg_ir::eval;
use cadmpeg_ir::geometry::{
    BlendSupport, Curve, CurveGeometry, NurbsCurve, Pcurve, PcurveGeometry, ProceduralCurve,
    ProceduralSurface, ProceduralSurfaceDefinition, RollingBallConstruction,
    RollingBallRadiusSelector, RollingBallSide, RollingBallThirdSide, Surface, SurfaceGeometry,
    VariableBlendConstruction, VariableBlendSide, VertexBlendBoundary, VertexBlendBoundaryGeometry,
    VertexBlendConstruction,
};
use cadmpeg_ir::hash::sha256_hex;
use cadmpeg_ir::ids::{
    AttributeId, BodyId, CoedgeId, CurveId, EdgeId, FaceId, LoopId, PcurveId, PointId, RegionId,
    ShellId, SurfaceId, UnknownId, VertexId,
};
use cadmpeg_ir::math::{Point3, Vector3};
use cadmpeg_ir::topology::{
    Body, Coedge, Color, Edge, Face, Loop, Point, Region, Sense, Shell, Vertex,
};
use cadmpeg_ir::unknown::UnknownRecord;

use crate::asm_header;
use crate::nurbs;
use crate::sab::{Record, Token};

/// Millimetres per ASM model-space length unit (centimetres).
const LEN_TO_MM: f64 = 10.0;

fn embedded_pcurve_geometry(pcurve: nurbs::NurbsPcurve) -> PcurveGeometry {
    PcurveGeometry::Nurbs {
        degree: pcurve.degree,
        knots: pcurve.knots,
        control_points: pcurve.control_points,
        weights: pcurve.weights,
        periodic: pcurve.periodic,
    }
}

/// The decoded B-rep graph plus loss accounting.
#[derive(Default)]
pub struct Brep {
    /// Bodies.
    pub bodies: Vec<Body>,
    /// Regions.
    pub regions: Vec<Region>,
    /// Shells.
    pub shells: Vec<Shell>,
    /// Faces.
    pub faces: Vec<Face>,
    /// Loops.
    pub loops: Vec<Loop>,
    /// Coedges.
    pub coedges: Vec<Coedge>,
    /// Edges.
    pub edges: Vec<Edge>,
    /// Vertices.
    pub vertices: Vec<Vertex>,
    /// Points.
    pub points: Vec<Point>,
    /// Analytic surface carriers.
    pub surfaces: Vec<Surface>,
    /// Analytic curve carriers.
    pub curves: Vec<Curve>,
    /// Parameter-space curve carriers.
    pub pcurves: Vec<Pcurve>,
    /// Native procedural definitions for solved surface carriers.
    pub procedural_surfaces: Vec<ProceduralSurface>,
    /// Native procedural definitions for solved curve caches.
    pub procedural_curves: Vec<ProceduralCurve>,
    /// Typed sketch-curve provenance links.
    pub sketch_curve_links: Vec<SketchCurveLink>,
    /// Persistent design identifiers attached to solved entities.
    pub persistent_design_links: Vec<PersistentDesignLink>,
    /// Original authoring times attached to solved entities.
    pub creation_timestamps: Vec<CreationTimestamp>,
    /// Kernel continuity classifications stored on solved edges.
    pub edge_continuities: Vec<EdgeContinuity>,
    /// Native owner-coedge selectors stored on solved edges.
    pub edge_ownerships: Vec<EdgeOwnership>,
    /// Native owner-edge and endpoint-slot fields stored on solved vertices.
    pub vertex_ownerships: Vec<VertexOwnership>,
    /// Native sidedness fields stored on solved faces.
    pub face_sidedness: Vec<FaceSidedness>,
    /// Native parameter intervals stored on tolerant coedges.
    pub tolerant_coedge_parameters: Vec<TolerantCoedgeParameters>,
    /// Native trailing fields stored on tolerant vertices.
    pub tolerant_vertex_tails: Vec<TolerantVertexTail>,
    /// Native rotation/reflection/shear classifications stored on transforms.
    pub transform_hints: Vec<TransformHints>,
    /// Native ASM body key by emitted body id, used by Design-side joins.
    pub body_keys: HashMap<BodyId, u64>,
    /// Native Design-join key field for every emitted body, including null keys.
    pub body_native_keys: Vec<BodyNativeKey>,
    /// Native wire records projected onto solved shells.
    pub wire_topologies: Vec<WireTopology>,
    /// Linked source-native attributes.
    pub attributes: Vec<SourceAttribute>,
    /// Undecoded carrier records preserved verbatim.
    pub unknowns: Vec<UnknownRecord>,
    /// Loss accounting for the report.
    pub stats: Stats,
    /// Source locations for emitted B-rep and synthetic child records.
    pub annotation_records: Vec<AnnotationRecord>,
}

/// One sparse v1 annotation produced while SAB record offsets are available.
pub struct AnnotationRecord {
    /// Globally unique IR entity id.
    pub id: String,
    /// Byte offset in the decompressed ASM stream.
    pub offset: u64,
    /// Source SAB record name.
    pub tag: String,
    /// Serialized fields whose values were canonically derived.
    pub derived_fields: Vec<&'static str>,
}

/// Counts used to construct the B-rep loss report.
#[derive(Default)]
pub struct Stats {
    /// Faces resting on a spline/procedural surface whose shape was not decoded
    /// into a typed carrier; emitted with an unknown-geometry surface.
    pub unknown_surface_faces: usize,
    /// Spline surface records whose cached B-spline block was decoded into a
    /// NURBS carrier.
    pub nurbs_surfaces: usize,
    /// Procedural curve records whose cached 3D B-spline block was decoded into
    /// a NURBS carrier.
    pub nurbs_curves: usize,
    /// Edges whose 3D curve is a procedural carrier (emitted with no curve).
    pub procedural_curve_edges: usize,
    /// Coedges that carried an explicit UV pcurve ref with no decodable 2D
    /// carrier on the face surface's parameterization (undecodable bytes, or
    /// UV values on the exact procedural parameterization rather than the
    /// solved cache's).
    pub undecoded_pcurve_refs: usize,
    /// Procedural blends for which only one of two support families resolved.
    pub partial_procedural_supports: usize,
    /// Record names in the active slice that were neither topology nor a
    /// decoded/preserved carrier (attributes, transforms, refinements, …).
    pub other_records: usize,
    /// Residual record counts by full record name.
    pub other_record_kinds: std::collections::BTreeMap<String, usize>,
}

// ---- geometry carrier decode -------------------------------------------------

/// Ordered typed values pulled from a carrier record's payload.
struct Carrier {
    positions: Vec<[f64; 3]>,
    vectors: Vec<[f64; 3]>,
    doubles: Vec<f64>,
}

fn collect_carrier(rec: &Record) -> Carrier {
    let mut c = Carrier {
        positions: Vec::new(),
        vectors: Vec::new(),
        doubles: Vec::new(),
    };
    for t in &rec.tokens {
        match t {
            Token::Position(p) => c.positions.push(*p),
            Token::Vector3(v) => c.vectors.push(*v),
            Token::Double(d) => c.doubles.push(*d),
            _ => {}
        }
    }
    c
}

fn scale_point(p: [f64; 3]) -> Point3 {
    Point3::new(p[0] * LEN_TO_MM, p[1] * LEN_TO_MM, p[2] * LEN_TO_MM)
}

fn vec3(v: [f64; 3]) -> Vector3 {
    Vector3::new(v[0], v[1], v[2])
}

fn norm3(v: [f64; 3]) -> f64 {
    (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt()
}

/// Return `v` normalized to unit length, or `v` unchanged if it is degenerate
/// (validation flags a degenerate direction rather than this hiding it).
fn unit(v: [f64; 3]) -> Vector3 {
    let n = norm3(v);
    if n > f64::EPSILON {
        Vector3::new(v[0] / n, v[1] / n, v[2] / n)
    } else {
        vec3(v)
    }
}

/// Whether a record name heads an analytic surface carrier.
fn is_analytic_surface(head: &str) -> bool {
    matches!(head, "plane" | "cone" | "sphere" | "torus")
}

/// Whether a record name heads an analytic curve carrier.
fn is_analytic_curve(head: &str) -> bool {
    matches!(head, "straight" | "ellipse" | "degenerate_curve")
}

/// Decode an analytic surface carrier. Signed sphere and torus radii remain in
/// the IR because they are part of the ASM carrier semantics.
pub(crate) fn decode_surface(rec: &Record) -> Option<(SurfaceGeometry, bool)> {
    let c = collect_carrier(rec);
    let origin = *c.positions.first()?;
    match rec.head.as_str() {
        "plane" => {
            let normal = *c.vectors.first()?;
            let normal = unit(normal);
            let u_axis = c
                .vectors
                .get(1)
                .map_or_else(|| deterministic_ref_direction(normal), |axis| unit(*axis));
            Some((
                SurfaceGeometry::Plane {
                    origin: scale_point(origin),
                    normal,
                    u_axis,
                },
                false,
            ))
        }
        "cone" => {
            let ratio = *c.doubles.first().unwrap_or(&1.0);
            let axis = *c.vectors.first()?;
            let axis = unit(axis);
            let major = c.vectors.get(1).copied();
            // Doubles are (ratio, sine, cosine, u_scale). `ratio` is the
            // minor/major radius ratio. `sine` selects cylinder vs cone. The
            // base radius is the major-axis vector's
            // magnitude; the trailing `u_scale` double is the u-parameter
            // scale, which usually coincides with the radius but diverges on
            // offset-derived surfaces. The signed slope `sine / cosine` is the
            // radius change per unit axis distance, and a negative `cosine`
            // points the surface normal toward the axis.
            let sine = *c.doubles.get(1).unwrap_or(&0.0);
            let cosine = *c.doubles.get(2).unwrap_or(&1.0);
            let u_scale = c.doubles.get(3).copied();
            let radius = major
                .map(|vector| norm3(vector) * LEN_TO_MM)
                .filter(|radius| *radius > f64::EPSILON)
                .or_else(|| u_scale.map(|r| r * LEN_TO_MM))?;
            let ref_direction = major.map_or_else(|| deterministic_ref_direction(axis), unit);
            if sine.abs() <= f64::EPSILON && ratio == 1.0 {
                Some((
                    SurfaceGeometry::Cylinder {
                        origin: scale_point(origin),
                        axis,
                        ref_direction,
                        radius,
                    },
                    cosine < 0.0,
                ))
            } else {
                // The IR cone's radius grows along `+axis`; a negative native
                // slope shrinks it, so the axis flips to compensate. The
                // outward normal is invariant under the flip; the inward
                // normal of a negative `cosine` folds into the face sense.
                let axis = if sine * cosine < 0.0 {
                    Vector3::new(-axis.x, -axis.y, -axis.z)
                } else {
                    axis
                };
                Some((
                    SurfaceGeometry::Cone {
                        origin: scale_point(origin),
                        axis,
                        ref_direction,
                        radius,
                        ratio,
                        half_angle: sine.abs().asin(),
                    },
                    cosine < 0.0,
                ))
            }
        }
        "sphere" => {
            let signed = *c.doubles.first()?;
            let polar_axis = c.vectors.get(1).or_else(|| c.vectors.first()).copied()?;
            let polar_axis = unit(polar_axis);
            let equator = c
                .vectors
                .first()
                .filter(|_| c.vectors.len() > 1)
                .map_or_else(
                    || deterministic_ref_direction(polar_axis),
                    |direction| unit(*direction),
                );
            Some((
                SurfaceGeometry::Sphere {
                    center: scale_point(origin),
                    axis: polar_axis,
                    ref_direction: equator,
                    radius: signed * LEN_TO_MM,
                },
                false,
            ))
        }
        "torus" => {
            let axis = *c.vectors.first()?;
            let axis = unit(axis);
            let ref_direction = c.vectors.get(1).map_or_else(
                || deterministic_ref_direction(axis),
                |direction| unit(*direction),
            );
            let major = *c.doubles.first()?;
            let minor = *c.doubles.get(1)?;
            Some((
                SurfaceGeometry::Torus {
                    center: scale_point(origin),
                    axis,
                    ref_direction,
                    major_radius: major * LEN_TO_MM,
                    minor_radius: minor * LEN_TO_MM,
                },
                false,
            ))
        }
        _ => None,
    }
}

fn deterministic_ref_direction(axis: Vector3) -> Vector3 {
    let candidates = [
        Vector3::new(1.0, 0.0, 0.0),
        Vector3::new(0.0, 1.0, 0.0),
        Vector3::new(0.0, 0.0, 1.0),
    ];
    let basis = candidates
        .into_iter()
        .min_by(|a, b| {
            let a_dot = (a.x * axis.x + a.y * axis.y + a.z * axis.z).abs();
            let b_dot = (b.x * axis.x + b.y * axis.y + b.z * axis.z).abs();
            a_dot.total_cmp(&b_dot)
        })
        .expect("fixed candidate set is non-empty");
    let dot = basis.x * axis.x + basis.y * axis.y + basis.z * axis.z;
    let projected = Vector3::new(
        basis.x - dot * axis.x,
        basis.y - dot * axis.y,
        basis.z - dot * axis.z,
    );
    let length = projected.norm();
    Vector3::new(
        projected.x / length,
        projected.y / length,
        projected.z / length,
    )
}

/// Maximum distance, in millimeters, between a pcurve endpoint mapped through
/// the face surface and the edge's vertex position for the pcurve to count as
/// that surface's parameter-space image.
const PCURVE_ENDPOINT_TOLERANCE_MM: f64 = 0.01;

fn is_vertex_record(record: &Record) -> bool {
    matches!(record.head.as_str(), "vertex" | "tvertex")
}

fn is_edge_record(record: &Record) -> bool {
    matches!(record.head.as_str(), "edge" | "tedge")
}

fn is_coedge_record(record: &Record) -> bool {
    matches!(record.head.as_str(), "coedge" | "tcoedge")
}

/// The millimeter-space position of an edge-record vertex reference.
fn vertex_position(by_index: &HashMap<i64, &Record>, vertex: i64) -> Option<Point3> {
    let vertex_record = by_index.get(&vertex).filter(|r| is_vertex_record(r))?;
    let point_record = by_index.get(&vertex_record.ref_at(5)?)?;
    collect_carrier(point_record)
        .positions
        .first()
        .map(|p| scale_point(*p))
}

fn distance(a: Point3, b: Point3) -> f64 {
    ((a.x - b.x).powi(2) + (a.y - b.y).powi(2) + (a.z - b.z).powi(2)).sqrt()
}

/// Select the candidate 2D block that is the face surface's parameter-space
/// image of the coedge: its endpoints, mapped through the surface, land on the
/// owning edge's vertex positions. On a non-NURBS surface, or when the edge's
/// vertex positions cannot be read, the first candidate passes unverified. An
/// empty result means no candidate is the surface's image of this edge.
fn select_face_pcurve(
    candidates: Vec<nurbs::NurbsPcurve>,
    surface: Option<&SurfaceGeometry>,
    edge: Option<&Record>,
    by_index: &HashMap<i64, &Record>,
) -> Option<nurbs::NurbsPcurve> {
    let Some(SurfaceGeometry::Nurbs(surface)) = surface else {
        return candidates.into_iter().next();
    };
    let vertex_pair = edge.and_then(|edge| {
        Some((
            vertex_position(by_index, edge.ref_at(3)?)?,
            vertex_position(by_index, edge.ref_at(5)?)?,
        ))
    });
    let Some((start, end)) = vertex_pair else {
        return candidates.into_iter().next();
    };
    let mut best: Option<(f64, nurbs::NurbsPcurve)> = None;
    for candidate in candidates {
        let (Some(&t0), Some(&t1)) = (candidate.knots.first(), candidate.knots.last()) else {
            continue;
        };
        let uv_at = |t: f64| {
            eval::nurbs_pcurve_uv(
                candidate.degree,
                &candidate.knots,
                &candidate.control_points,
                candidate.weights.as_deref(),
                t,
            )
        };
        let (Some(uv0), Some(uv1)) = (uv_at(t0), uv_at(t1)) else {
            continue;
        };
        let (Some(p0), Some(p1)) = (
            eval::nurbs_surface_point(surface, uv0.u, uv0.v),
            eval::nurbs_surface_point(surface, uv1.u, uv1.v),
        ) else {
            continue;
        };
        // The candidate's parameter direction is independent of the edge
        // sense, so accept either endpoint assignment.
        let forward = distance(p0, start).max(distance(p1, end));
        let reversed = distance(p0, end).max(distance(p1, start));
        let mismatch = forward.min(reversed);
        if mismatch <= PCURVE_ENDPOINT_TOLERANCE_MM
            && best.as_ref().is_none_or(|(current, _)| mismatch < *current)
        {
            best = Some((mismatch, candidate));
        }
    }
    best.map(|(_, candidate)| candidate)
}

/// Decode an analytic curve carrier.
pub(crate) fn decode_curve(rec: &Record) -> Option<CurveGeometry> {
    let carrier = collect_carrier(rec);
    let base = *carrier.positions.first()?;
    match rec.head.as_str() {
        "straight" => Some(CurveGeometry::Line {
            origin: scale_point(base),
            direction: unit(*carrier.vectors.first()?),
        }),
        "ellipse" => {
            let axis = *carrier.vectors.first()?;
            let reference = *carrier.vectors.get(1)?;
            let ratio = *carrier.doubles.first()?;
            let major_radius = norm3(reference) * LEN_TO_MM;
            if (ratio.abs() - 1.0).abs() <= f64::EPSILON {
                Some(CurveGeometry::Circle {
                    center: scale_point(base),
                    axis: unit(axis),
                    ref_direction: unit(reference),
                    radius: major_radius,
                })
            } else {
                Some(CurveGeometry::Ellipse {
                    center: scale_point(base),
                    axis: unit(axis),
                    major_direction: unit(reference),
                    major_radius,
                    minor_radius: major_radius * ratio.abs(),
                })
            }
        }
        "degenerate_curve" => Some(CurveGeometry::Degenerate {
            point: scale_point(base),
        }),
        _ => None,
    }
}

fn sense_at(rec: &Record, i: usize) -> Sense {
    match rec.chunk(i) {
        Some(Token::True) => Sense::Reversed,
        _ => Sense::Forward,
    }
}

/// The record-level sense bit of an `intcurve` or `spline` carrier: the boolean
/// token immediately before the record's subtype scope ([spec §7.6](https://github.com/cadmpeg/cadmpeg/blob/main/docs/formats/f3d.md#76-intcurve-and-spline-subtypes)). `true`
/// marks geometry as the reverse of its cached definition. A reversed intcurve
/// negates the cache parameterization (`C(t) = cache(-t)`), and a reversed
/// spline surface flips the cache normal.
fn record_reversed(rec: &Record) -> bool {
    rec.tokens
        .windows(2)
        .find_map(|tokens| {
            matches!(tokens[1], Token::SubtypeOpen).then(|| match tokens[0] {
                Token::True => true,
                Token::False => false,
                _ => false,
            })
        })
        .unwrap_or(false)
}

/// Reparameterize a cached B-spline to its record's reversed sense,
/// `C'(t) = C(-t)`, by reversing poles and weights and negating reversed knots.
fn reverse_nurbs_curve(curve: &mut NurbsCurve) {
    curve.control_points.reverse();
    if let Some(weights) = curve.weights.as_mut() {
        weights.reverse();
    }
    curve.knots.reverse();
    for knot in &mut curve.knots {
        *knot = -*knot;
    }
}

/// Reverse a curve carrier to its opposite orientation, `C'(t) = C(-t)`.
/// Lines negate their direction, conics negate their plane normal (flipping
/// the angular sweep while keeping the zero-angle direction), and B-splines
/// reverse poles and knots. Carriers without an orientation pass through.
fn reverse_curve_geometry(geometry: &mut CurveGeometry) {
    match geometry {
        CurveGeometry::Line { direction, .. } => {
            *direction = Vector3::new(-direction.x, -direction.y, -direction.z);
        }
        CurveGeometry::Circle { axis, .. } | CurveGeometry::Ellipse { axis, .. } => {
            *axis = Vector3::new(-axis.x, -axis.y, -axis.z);
        }
        CurveGeometry::Nurbs(curve) => reverse_nurbs_curve(curve),
        _ => {}
    }
}

fn double_at(rec: &Record, i: usize) -> Option<f64> {
    match rec.chunk(i) {
        Some(Token::Double(d)) => Some(*d),
        _ => None,
    }
}

fn pcurve_parameter_range(rec: &Record) -> Option<[f64; 2]> {
    match rec.tokens.as_slice() {
        [.., Token::Double(start), Token::Double(end)] => Some([*start, *end]),
        _ => None,
    }
}

fn pcurve_inline_tail_flags(rec: &Record) -> Option<[bool; 4]> {
    if !matches!(rec.chunk(3), Some(Token::Long(0))) {
        return None;
    }
    let end = rec.tokens.len().checked_sub(2)?;
    let flags = rec.tokens.get(end.checked_sub(4)?..end)?;
    flags
        .iter()
        .map(|token| match token {
            Token::True => Some(true),
            Token::False => Some(false),
            _ => None,
        })
        .collect::<Option<Vec<_>>>()?
        .try_into()
        .ok()
}

/// Decode a framed active slice into the IR B-rep graph.
///
/// `stream` names the source ZIP entry for provenance. Ids are minted as
/// `f3d:brep:entity#<record-index>`, unique across the `RecordTable`.
pub fn decode(records: &[Record], bytes: &[u8], _stream: &str) -> Brep {
    let mut out = Brep::default();

    let id = |i: i64| format!("f3d:brep:entity#{i}");
    // Index records by RecordTable index (== position for a framed slice).
    let by_index: HashMap<i64, &Record> = records.iter().map(|r| (r.index as i64, r)).collect();
    // Subtype-definition positions, built once for every carrier resolution.
    let subtype_tables = nurbs::SubtypeTables::from_records(records, bytes);
    let header = asm_header::parse(bytes);
    let ref_width = header
        .as_ref()
        .map_or(8, |header| usize::from(header.width));
    let header_scale = header.and_then(|header| header.scale).unwrap_or(1.0);

    let attribute_color = |entity: &Record| attribute_chain_color(entity, &by_index);
    let attribute_name = |entity: &Record| attribute_chain_name(entity, &by_index);

    // Pass 1: classify carriers and decode analytic geometry.
    let mut surface_geo: HashMap<i64, (SurfaceGeometry, bool)> = HashMap::new();
    let mut procedural_surface_defs = HashMap::new();
    let mut curve_geo: HashMap<i64, CurveGeometry> = HashMap::new();
    let mut procedural_curve_defs = HashMap::new();
    for r in records {
        if is_analytic_surface(&r.head) {
            if let Some(g) = decode_surface(r) {
                surface_geo.insert(r.index as i64, g);
            }
        } else if is_analytic_curve(&r.head) {
            if let Some(g) = decode_curve(r) {
                curve_geo.insert(r.index as i64, g);
            }
        }
    }
    // Carriers whose native normal points opposite the IR carrier's normal;
    // the reversal folds into the referencing faces' senses.
    let inward_normal_surfaces: HashSet<i64> = surface_geo
        .iter()
        .filter(|(_, (_, inward))| *inward)
        .map(|(&index, _)| index)
        .collect();

    // Pass 2: keep every face whose surface reference resolves to a record,
    // then pull its supporting graph in by shell-reachability. A face on a
    // decoded analytic surface gets that carrier; a face on a spline/procedural
    // surface keeps its topology and gets an unknown-geometry carrier linking to
    // the preserved bytes.
    let mut kept_faces: HashSet<i64> = HashSet::new();
    let mut kept_loops: HashSet<i64> = HashSet::new();
    let mut kept_coedges: HashSet<i64> = HashSet::new();
    let mut kept_edges: HashSet<i64> = HashSet::new();
    let mut kept_vertices: HashSet<i64> = HashSet::new();
    let mut kept_points: HashSet<i64> = HashSet::new();
    let mut kept_surfaces: HashSet<i64> = HashSet::new();
    let mut unknown_surface_records: HashSet<i64> = HashSet::new();
    let mut kept_curves: HashSet<i64> = HashSet::new();
    let mut kept_pcurves: HashSet<i64> = HashSet::new();
    let mut pcurve_geo: HashMap<i64, PcurveGeometry> = HashMap::new();
    // Undecoded carriers referenced by real topology, to preserve as unknowns.
    let mut undecoded_carriers: HashSet<i64> = HashSet::new();

    for r in records {
        if r.head != "face" {
            continue;
        }
        let Some(surf_ref) = r.ref_at(7) else {
            continue;
        };
        let Some(surf_rec) = by_index.get(&surf_ref) else {
            // Dangling surface reference: a face without a resolvable surface
            // cannot be emitted (the IR requires one), so it is dropped.
            continue;
        };
        kept_faces.insert(r.index as i64);
        if let Some(procedural) = nurbs::decode_procedural_surface_resolving_refs(
            record_slice(surf_rec, bytes),
            bytes,
            &subtype_tables,
        ) {
            procedural_surface_defs.insert(surf_ref, procedural);
        }
        // A non-analytic surface may still carry a decodable B-spline face cache.
        if let std::collections::hash_map::Entry::Vacant(e) = surface_geo.entry(surf_ref) {
            if let Some(ns) = nurbs::decode_surface_cache_resolving_refs(
                record_slice(surf_rec, bytes),
                bytes,
                &subtype_tables,
            ) {
                e.insert((SurfaceGeometry::Nurbs(ns), false));
                out.stats.nurbs_surfaces += 1;
            }
        }
        if !surface_geo.contains_key(&surf_ref) && procedural_surface_defs.contains_key(&surf_ref) {
            surface_geo.insert(
                surf_ref,
                (
                    SurfaceGeometry::Unknown {
                        record: Some(UnknownId(unknown_record_id(surf_rec))),
                    },
                    false,
                ),
            );
            undecoded_carriers.insert(surf_ref);
        }
        if surface_geo.contains_key(&surf_ref) {
            kept_surfaces.insert(surf_ref);
        } else {
            unknown_surface_records.insert(surf_ref);
            undecoded_carriers.insert(surf_ref);
            out.stats.unknown_surface_faces += 1;
        }
    }

    // Walk each kept face's loops and coedge rings, collecting supporting graph.
    for &face_idx in &kept_faces.iter().copied().collect::<Vec<_>>() {
        let Some(face) = by_index.get(&face_idx) else {
            continue;
        };
        let mut loop_ref = face.ref_at(4);
        let mut loop_guard = HashSet::new();
        while let Some(li) = loop_ref {
            if !loop_guard.insert(li) {
                break;
            }
            let Some(lp) = by_index.get(&li) else { break };
            if lp.head != "loop" {
                break;
            }
            kept_loops.insert(li);
            // Ring-walk coedges via chunk[3] = next.
            if let Some(first_ce) = lp.ref_at(4) {
                let mut ce_ref = Some(first_ce);
                let mut ce_guard = HashSet::new();
                while let Some(ci) = ce_ref {
                    if !ce_guard.insert(ci) {
                        break;
                    }
                    let Some(ce) = by_index.get(&ci) else { break };
                    if !is_coedge_record(ce) {
                        break;
                    }
                    kept_coedges.insert(ci);
                    if let Some(pc) = ce.ref_at(10) {
                        if let Some(prec) = by_index.get(&pc) {
                            // An inline pcurve carries its own 2D block; a
                            // ref-form pcurve delegates to an intcurve entity
                            // whose record holds several 2D blocks. Decode
                            // every candidate and keep the one whose endpoints
                            // land on the edge's vertices through the face
                            // surface.
                            let candidates = match (prec.chunk(3), prec.chunk(4)) {
                                (Some(Token::Long(0)), Some(Token::True | Token::False)) => {
                                    crate::sab::payload_subtype_span(
                                        bytes,
                                        prec,
                                        5,
                                        ref_width,
                                        "exp_par_cur",
                                    )
                                    .map(|span| {
                                        nurbs::decode_pcurve_cache_candidates_resolving_refs(
                                            span,
                                            bytes,
                                            &subtype_tables,
                                        )
                                    })
                                    .unwrap_or_default()
                                }
                                (Some(Token::Long(1 | 2 | -1)), Some(Token::Ref(reference))) => {
                                    by_index
                                        .get(reference)
                                        .filter(|record| record.head == "intcurve")
                                        .map(|intcurve| {
                                            nurbs::decode_pcurve_cache_candidates_resolving_refs(
                                                record_slice(intcurve, bytes),
                                                bytes,
                                                &subtype_tables,
                                            )
                                        })
                                        .unwrap_or_default()
                                }
                                _ => Vec::new(),
                            };
                            let decoded = select_face_pcurve(
                                candidates,
                                face.ref_at(7)
                                    .and_then(|surface| surface_geo.get(&surface))
                                    .map(|(geometry, _)| geometry),
                                ce.ref_at(6).and_then(|edge| by_index.get(&edge)).copied(),
                                &by_index,
                            );
                            if let Some(decoded) = decoded {
                                pcurve_geo.insert(
                                    pc,
                                    PcurveGeometry::Nurbs {
                                        degree: decoded.degree,
                                        knots: decoded.knots,
                                        control_points: decoded.control_points,
                                        weights: decoded.weights,
                                        periodic: decoded.periodic,
                                    },
                                );
                                kept_pcurves.insert(pc);
                            } else {
                                out.stats.undecoded_pcurve_refs += 1;
                            }
                        } else {
                            out.stats.undecoded_pcurve_refs += 1;
                        }
                    }
                    if let Some(ei) = ce.ref_at(6) {
                        if let Some(edge) = by_index.get(&ei) {
                            // An edge is shared by two coedges; process (and
                            // count its curve loss) only the first time it is
                            // reached so shared edges are not double-counted.
                            if is_edge_record(edge) && kept_edges.insert(ei) {
                                for slot in [3usize, 5] {
                                    if let Some(vi) = edge.ref_at(slot) {
                                        if let Some(v) = by_index.get(&vi) {
                                            if is_vertex_record(v) {
                                                kept_vertices.insert(vi);
                                                if let Some(pi) = v.ref_at(5) {
                                                    kept_points.insert(pi);
                                                }
                                            }
                                        }
                                    }
                                }
                                match edge.ref_at(8) {
                                    Some(cv) if curve_geo.contains_key(&cv) => {
                                        kept_curves.insert(cv);
                                    }
                                    Some(cv) => {
                                        if let Some(crec) = by_index.get(&cv) {
                                            // A procedural curve carries an inline
                                            // 3D B-spline cache in most subtypes.
                                            if let Some(decoded) =
                                                nurbs::decode_procedural_curve_resolving_refs(
                                                    record_slice(crec, bytes),
                                                    bytes,
                                                    &subtype_tables,
                                                )
                                            {
                                                let mut curve = decoded.curve;
                                                // A reversed intcurve parameterizes
                                                // as the negation of its cache; the
                                                // edge's stored range is on the
                                                // reversed parameterization.
                                                if record_reversed(crec) {
                                                    reverse_nurbs_curve(&mut curve);
                                                }
                                                curve_geo.insert(cv, CurveGeometry::Nurbs(curve));
                                                procedural_curve_defs.insert(
                                                    cv,
                                                    (
                                                        decoded.native_kind,
                                                        decoded.definition,
                                                        decoded.vector_offset,
                                                        decoded.subset,
                                                        decoded.compound,
                                                        decoded.embedded_two_sided_offset,
                                                        decoded.embedded_intersection,
                                                        decoded.embedded_three_surface_intersection,
                                                        decoded.embedded_surface_curve,
                                                        decoded.embedded_silhouette,
                                                        decoded.embedded_surface_offset,
                                                        decoded.embedded_spring,
                                                        decoded.embedded_deformable,
                                                        decoded.embedded_projection,
                                                        decoded.embedded_law,
                                                        decoded.cache_fit_tolerance,
                                                    ),
                                                );
                                                out.stats.nurbs_curves += 1;
                                                kept_curves.insert(cv);
                                            } else {
                                                undecoded_carriers.insert(cv);
                                                out.stats.procedural_curve_edges += 1;
                                            }
                                        }
                                    }
                                    _ => {}
                                }
                            }
                        }
                    }
                    ce_ref = ce.ref_at(3);
                    if ce_ref == Some(first_ce) {
                        break;
                    }
                }
            }
            loop_ref = lp.ref_at(3);
        }
    }

    let mut wire_edges_by_shell = HashMap::<i64, Vec<i64>>::new();
    for shell in records.iter().filter(|record| record.head == "shell") {
        let shell_index = shell.index as i64;
        let mut wire_ref = shell.ref_at(6);
        let mut wire_guard = HashSet::new();
        while let Some(wire_index) = wire_ref.filter(|index| wire_guard.insert(*index)) {
            let Some(wire) = by_index
                .get(&wire_index)
                .filter(|record| record.head == "wire")
            else {
                break;
            };
            let side = match wire.chunk(7) {
                Some(Token::True) => Some(WireSide::In),
                Some(Token::False) => Some(WireSide::Out),
                _ => None,
            };
            if let Some(side) = side {
                out.wire_topologies.push(WireTopology {
                    id: format!("f3d:asm:wire-topology#{wire_index}"),
                    shell: ShellId(id(shell_index)),
                    record_index: wire.index as u32,
                    side,
                });
            }
            if let Some(first_coedge) = wire.ref_at(4) {
                let mut coedge_ref = Some(first_coedge);
                let mut coedge_guard = HashSet::new();
                while let Some(coedge_index) =
                    coedge_ref.filter(|index| coedge_guard.insert(*index))
                {
                    let Some(coedge) = by_index
                        .get(&coedge_index)
                        .filter(|record| is_coedge_record(record))
                    else {
                        break;
                    };
                    if let Some(edge_index) = coedge.ref_at(6) {
                        let edges = wire_edges_by_shell.entry(shell_index).or_default();
                        if !edges.contains(&edge_index) {
                            edges.push(edge_index);
                        }
                        if let Some(edge) = by_index.get(&edge_index) {
                            if is_edge_record(edge) && kept_edges.insert(edge_index) {
                                for slot in [3usize, 5] {
                                    if let Some(vertex_index) = edge.ref_at(slot) {
                                        if let Some(vertex) = by_index.get(&vertex_index) {
                                            if is_vertex_record(vertex) {
                                                kept_vertices.insert(vertex_index);
                                                if let Some(point_index) = vertex.ref_at(5) {
                                                    kept_points.insert(point_index);
                                                }
                                            }
                                        }
                                    }
                                }
                                if let Some(curve_index) = edge.ref_at(8) {
                                    match curve_geo.entry(curve_index) {
                                        std::collections::hash_map::Entry::Occupied(_) => {
                                            kept_curves.insert(curve_index);
                                        }
                                        std::collections::hash_map::Entry::Vacant(entry) => {
                                            if let Some(curve_record) = by_index.get(&curve_index) {
                                                if let Some(decoded) =
                                                    nurbs::decode_procedural_curve_resolving_refs(
                                                        record_slice(curve_record, bytes),
                                                        bytes,
                                                        &subtype_tables,
                                                    )
                                                {
                                                    let mut curve = decoded.curve;
                                                    if record_reversed(curve_record) {
                                                        reverse_nurbs_curve(&mut curve);
                                                    }
                                                    entry.insert(CurveGeometry::Nurbs(curve));
                                                    procedural_curve_defs.insert(
                                                        curve_index,
                                                        (
                                                            decoded.native_kind,
                                                            decoded.definition,
                                                            decoded.vector_offset,
                                                            decoded.subset,
                                                            decoded.compound,
                                                            decoded.embedded_two_sided_offset,
                                                            decoded.embedded_intersection,
                                                            decoded.embedded_three_surface_intersection,
                                                            decoded.embedded_surface_curve,
                                                            decoded.embedded_silhouette,
                                                            decoded.embedded_surface_offset,
                                                            decoded.embedded_spring,
                                                            decoded.embedded_deformable,
                                                            decoded.embedded_projection,
                                                            decoded.embedded_law,
                                                            decoded.cache_fit_tolerance,
                                                        ),
                                                    );
                                                    kept_curves.insert(curve_index);
                                                    out.stats.nurbs_curves += 1;
                                                } else {
                                                    undecoded_carriers.insert(curve_index);
                                                    out.stats.procedural_curve_edges += 1;
                                                }
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    }
                    coedge_ref = coedge.ref_at(3);
                    if coedge_ref == Some(first_coedge) {
                        break;
                    }
                }
            }
            wire_ref = wire.ref_at(3);
        }
    }

    // An edge whose sense boolean is reversed traverses its curve as
    // `C(-t)`, with the edge parameters on the reversed parameterization.
    // The IR keeps every edge forward on its curve (the STEP writer's
    // `same_sense = .T.` contract), so carriers referenced only by reversed
    // edges are reversed in place; a carrier shared across both senses keeps
    // its forward orientation and reversed edges point at a `:reversed`
    // clone emitted beside it.
    let mut reversed_curve_refs: HashSet<i64> = HashSet::new();
    let mut forward_curve_refs: HashSet<i64> = HashSet::new();
    for r in records {
        if !is_edge_record(r) || !kept_edges.contains(&(r.index as i64)) {
            continue;
        }
        let Some(curve) = r.ref_at(8).filter(|c| kept_curves.contains(c)) else {
            continue;
        };
        match sense_at(r, 9) {
            Sense::Reversed => reversed_curve_refs.insert(curve),
            Sense::Forward => forward_curve_refs.insert(curve),
        };
    }
    let reversed_curve_id = |c: i64| {
        if reversed_curve_refs.contains(&c) && forward_curve_refs.contains(&c) {
            CurveId(format!("{}:reversed", id(c)))
        } else {
            CurveId(id(c))
        }
    };

    // Pass 3: emit carriers, points, and the reachable topology graph in
    // RecordTable order for deterministic output.
    for r in records {
        let i = r.index as i64;
        match r.head.as_str() {
            _ if kept_surfaces.contains(&i) => {
                // A record index appears at most once in `records`; a duplicate
                // would have consumed the entry already, so skip rather than panic.
                let Some((geometry, _)) = surface_geo.remove(&i) else {
                    continue;
                };
                out.surfaces.push(Surface {
                    id: SurfaceId(id(i)),
                    geometry,
                    source_object: None,
                });
                if let Some(procedural) = procedural_surface_defs.remove(&i) {
                    let definition = match procedural.definition {
                        nurbs::DecodedProceduralSurfaceDefinition::Deformable(embedded) => {
                            let embedded = *embedded;
                            let support = SurfaceId(format!(
                                "f3d:brep:procedural_surface#{i}:deformable:support"
                            ));
                            out.surfaces.push(Surface {
                                id: support.clone(),
                                geometry: embedded.support,
                                source_object: None,
                            });
                            let data = match embedded.data {
                                nurbs::EmbeddedDeformableSurfaceData::Resolved(data) => data,
                                nurbs::EmbeddedDeformableSurfaceData::SurfaceCurve {
                                    surface,
                                    native_id,
                                    flag,
                                    first_parameter,
                                    selector,
                                    second_parameter,
                                    curve,
                                    vectors,
                                    frame_parameter,
                                    flags,
                                    parameter_triples,
                                } => {
                                    let secondary_surface = SurfaceId(format!(
                                        "f3d:brep:procedural_surface#{i}:deformable:secondary"
                                    ));
                                    out.surfaces.push(Surface {
                                        id: secondary_surface.clone(),
                                        geometry: surface,
                                        source_object: None,
                                    });
                                    let curve_id = CurveId(format!(
                                        "f3d:brep:procedural_surface#{i}:deformable:curve"
                                    ));
                                    out.curves.push(Curve {
                                        id: curve_id.clone(),
                                        geometry: CurveGeometry::Nurbs(curve),
                                        source_object: None,
                                    });
                                    cadmpeg_ir::geometry::DeformableSurfaceData::SurfaceCurve {
                                        surface: secondary_surface,
                                        native_id,
                                        flag,
                                        first_parameter,
                                        selector,
                                        second_parameter,
                                        curve: curve_id,
                                        vectors,
                                        frame_parameter,
                                        flags,
                                        parameter_triples,
                                    }
                                }
                                nurbs::EmbeddedDeformableSurfaceData::Full {
                                    leading_vectors,
                                    leading_parameter,
                                    leading_flags,
                                    selector,
                                    surface,
                                    native_id,
                                    flag,
                                    first_parameter,
                                    version_value,
                                    second_parameter,
                                    curve,
                                    frames,
                                    trailing_value,
                                } => {
                                    let secondary_surface = SurfaceId(format!(
                                        "f3d:brep:procedural_surface#{i}:deformable:secondary"
                                    ));
                                    out.surfaces.push(Surface {
                                        id: secondary_surface.clone(),
                                        geometry: surface,
                                        source_object: None,
                                    });
                                    let curve_id = CurveId(format!(
                                        "f3d:brep:procedural_surface#{i}:deformable:curve"
                                    ));
                                    out.curves.push(Curve {
                                        id: curve_id.clone(),
                                        geometry: CurveGeometry::Nurbs(curve),
                                        source_object: None,
                                    });
                                    cadmpeg_ir::geometry::DeformableSurfaceData::Full {
                                        leading_vectors,
                                        leading_parameter,
                                        leading_flags,
                                        selector,
                                        surface: secondary_surface,
                                        native_id,
                                        flag,
                                        first_parameter,
                                        version_value,
                                        second_parameter,
                                        curve: curve_id,
                                        frames,
                                        trailing_value,
                                    }
                                }
                            };
                            ProceduralSurfaceDefinition::Deformable {
                                construction: Box::new(
                                    cadmpeg_ir::geometry::DeformableSurfaceConstruction {
                                        support,
                                        data,
                                        discontinuities: embedded.discontinuities,
                                        discontinuity_flag: embedded.discontinuity_flag,
                                    },
                                ),
                            }
                        }
                        nurbs::DecodedProceduralSurfaceDefinition::Helix(construction) => {
                            ProceduralSurfaceDefinition::Helix { construction }
                        }
                        nurbs::DecodedProceduralSurfaceDefinition::TSpline(construction) => {
                            ProceduralSurfaceDefinition::TSpline { construction }
                        }
                        nurbs::DecodedProceduralSurfaceDefinition::Exact {
                            parameter_ranges,
                            extension,
                        } => ProceduralSurfaceDefinition::Exact {
                            parameter_ranges,
                            extension,
                        },
                        nurbs::DecodedProceduralSurfaceDefinition::Compound {
                            parameters,
                            components,
                        } => {
                            let component_ids = components
                                .into_iter()
                                .enumerate()
                                .map(|(component, geometry)| {
                                    let id = SurfaceId(format!(
                                        "f3d:brep:procedural_surface#{i}:component{component}"
                                    ));
                                    out.surfaces.push(Surface {
                                        id: id.clone(),
                                        geometry,
                                        source_object: None,
                                    });
                                    id
                                })
                                .collect();
                            ProceduralSurfaceDefinition::Compound {
                                parameters,
                                components: component_ids,
                            }
                        }
                        nurbs::DecodedProceduralSurfaceDefinition::Taper {
                            support,
                            reference,
                            pcurve,
                            parameter,
                            taper,
                        } => {
                            let support_id =
                                SurfaceId(format!("f3d:brep:procedural_surface#{i}:support"));
                            out.surfaces.push(Surface {
                                id: support_id.clone(),
                                geometry: support,
                                source_object: None,
                            });
                            let reference_id =
                                CurveId(format!("f3d:brep:procedural_surface#{i}:reference"));
                            out.curves.push(Curve {
                                id: reference_id.clone(),
                                geometry: CurveGeometry::Nurbs(reference),
                                source_object: None,
                            });
                            let pcurve = pcurve.map(|pcurve| PcurveGeometry::Nurbs {
                                degree: pcurve.degree,
                                knots: pcurve.knots,
                                control_points: pcurve.control_points,
                                weights: pcurve.weights,
                                periodic: pcurve.periodic,
                            });
                            ProceduralSurfaceDefinition::Taper {
                                support: support_id,
                                reference: reference_id,
                                pcurve,
                                parameter,
                                taper,
                            }
                        }
                        nurbs::DecodedProceduralSurfaceDefinition::Loft(embedded) => {
                            let sections = embedded.sections.into_iter().enumerate().map(
                                |(section_index, entries)| {
                                    let entries = entries.into_iter().enumerate().map(
                                        |(entry_index, entry)| {
                                            let profile = entry.profile.into_iter().enumerate().map(
                                                |(member_index, member)| {
                                                    let curve = CurveId(format!(
                                                        "f3d:brep:procedural_surface#{i}:loft:{section_index}:{entry_index}:profile:{member_index}"
                                                    ));
                                                    out.curves.push(Curve {
                                                        id: curve.clone(),
                                                        geometry: CurveGeometry::Nurbs(member.curve),
                                                        source_object: None,
                                                    });
                                                    let surface = SurfaceId(format!(
                                                        "f3d:brep:procedural_surface#{i}:loft:{section_index}:{entry_index}:support:{member_index}"
                                                    ));
                                                    out.surfaces.push(Surface {
                                                        id: surface.clone(),
                                                        geometry: member.data.surface,
                                                        source_object: None,
                                                    });
                                                    let pcurve = member.data.pcurve.map(|pcurve| PcurveGeometry::Nurbs {
                                                        degree: pcurve.degree,
                                                        knots: pcurve.knots,
                                                        control_points: pcurve.control_points,
                                                        weights: pcurve.weights,
                                                        periodic: pcurve.periodic,
                                                    });
                                                    cadmpeg_ir::geometry::LoftProfileMember {
                                                        type_code: member.type_code,
                                                        curve,
                                                        data: cadmpeg_ir::geometry::LoftProfileData {
                                                            surface,
                                                            pcurve,
                                                            first_flag: member.data.first_flag,
                                                            asm_extension: member.data.asm_extension,
                                                            subdata: member.data.subdata,
                                                            direction: member.data.direction,
                                                        },
                                                    }
                                                },
                                            ).collect();
                                            let path_curve = CurveId(format!(
                                                "f3d:brep:procedural_surface#{i}:loft:{section_index}:{entry_index}:path"
                                            ));
                                            out.curves.push(Curve {
                                                id: path_curve.clone(),
                                                geometry: CurveGeometry::Nurbs(entry.path.curve),
                                                source_object: None,
                                            });
                                            let auxiliaries = entry.path.auxiliaries.into_iter().enumerate().map(
                                                |(auxiliary_index, geometry)| {
                                                    let id = CurveId(format!(
                                                        "f3d:brep:procedural_surface#{i}:loft:{section_index}:{entry_index}:auxiliary:{auxiliary_index}"
                                                    ));
                                                    out.curves.push(Curve {
                                                        id: id.clone(),
                                                        geometry: CurveGeometry::Nurbs(geometry),
                                                        source_object: None,
                                                    });
                                                    id
                                                },
                                            ).collect();
                                            cadmpeg_ir::geometry::LoftSectionEntry {
                                                parameter: entry.parameter,
                                                profile,
                                                path: cadmpeg_ir::geometry::LoftPath {
                                                    curve: path_curve,
                                                    auxiliaries,
                                                    flag: entry.path.flag,
                                                },
                                            }
                                        },
                                    ).collect();
                                    cadmpeg_ir::geometry::LoftSection { entries }
                                },
                            ).collect::<Vec<_>>().try_into().expect("two loft sections");
                            ProceduralSurfaceDefinition::Loft {
                                sections,
                                parameter_ranges: embedded.parameter_ranges,
                                closures: embedded.closures,
                                singularities: embedded.singularities,
                                mode: embedded.mode,
                                bridge: embedded.bridge,
                            }
                        }
                        nurbs::DecodedProceduralSurfaceDefinition::CompoundLoft(embedded) => {
                            let embedded = *embedded;
                            let map_scale = |out: &mut Brep,
                                             name: &str,
                                             scale: nurbs::EmbeddedCompoundLoftScale| {
                                    let members = scale
                                    .members
                                    .into_iter()
                                    .enumerate()
                                    .map(|(member_index, member)| {
                                        let curve = CurveId(format!(
                                            "f3d:brep:procedural_surface#{i}:cloft:{name}:member:{member_index}:curve"
                                        ));
                                        out.curves.push(Curve {
                                            id: curve.clone(),
                                            geometry: CurveGeometry::Nurbs(member.curve),
                                            source_object: None,
                                        });
                                        let surface = SurfaceId(format!(
                                            "f3d:brep:procedural_surface#{i}:cloft:{name}:member:{member_index}:surface"
                                        ));
                                        out.surfaces.push(Surface {
                                            id: surface.clone(),
                                            geometry: member.data.surface,
                                            source_object: None,
                                        });
                                        cadmpeg_ir::geometry::CompoundLoftScaleMember {
                                            type_code: member.type_code,
                                            curve,
                                            data: cadmpeg_ir::geometry::LoftProfileData {
                                                surface,
                                                pcurve: member
                                                    .data
                                                    .pcurve
                                                    .map(embedded_pcurve_geometry),
                                                first_flag: member.data.first_flag,
                                                asm_extension: member.data.asm_extension,
                                                subdata: member.data.subdata,
                                                direction: member.data.direction,
                                            },
                                        }
                                    })
                                    .collect();
                                    let path = CurveId(format!(
                                        "f3d:brep:procedural_surface#{i}:cloft:{name}:path"
                                    ));
                                    out.curves.push(Curve {
                                        id: path.clone(),
                                        geometry: CurveGeometry::Nurbs(scale.path),
                                        source_object: None,
                                    });
                                    let auxiliaries = scale
                                    .auxiliaries
                                    .into_iter()
                                    .enumerate()
                                    .map(|(index, geometry)| {
                                        let id = CurveId(format!(
                                            "f3d:brep:procedural_surface#{i}:cloft:{name}:auxiliary:{index}"
                                        ));
                                        out.curves.push(Curve {
                                            id: id.clone(),
                                            geometry: CurveGeometry::Nurbs(geometry),
                                            source_object: None,
                                        });
                                        id
                                    })
                                    .collect();
                                    cadmpeg_ir::geometry::CompoundLoftScale {
                                        members,
                                        path,
                                        auxiliaries,
                                        tail: scale.tail,
                                    }
                                };
                            let scales = embedded
                                .scales
                                .into_iter()
                                .enumerate()
                                .map(|(index, scale)| {
                                    scale.map(|scale| {
                                        map_scale(&mut out, &format!("scale{index}"), scale)
                                    })
                                })
                                .collect::<Vec<_>>()
                                .try_into()
                                .expect("four compound-loft scales");
                            let fifth_scale = embedded
                                .fifth_scale
                                .map(|scale| Box::new(map_scale(&mut out, "fifth", *scale)));
                            let tail = match embedded.tail {
                                nurbs::EmbeddedCompoundLoftTail::Six {
                                    flags,
                                    scale,
                                    selector,
                                    direction,
                                    parameter_range,
                                    curve,
                                } => {
                                    let curve_id = CurveId(format!(
                                        "f3d:brep:procedural_surface#{i}:cloft:tail6:curve"
                                    ));
                                    out.curves.push(Curve {
                                        id: curve_id.clone(),
                                        geometry: CurveGeometry::Nurbs(curve),
                                        source_object: None,
                                    });
                                    cadmpeg_ir::geometry::CompoundLoftTail::Six {
                                        flags,
                                        scale: Box::new(map_scale(&mut out, "tail6", *scale)),
                                        selector,
                                        direction,
                                        parameter_range,
                                        curve: curve_id,
                                    }
                                }
                                nurbs::EmbeddedCompoundLoftTail::Seven {
                                    first_flag,
                                    first_scale,
                                    second_flag,
                                    second_scale,
                                    selector,
                                    direction,
                                    trailing_flags,
                                } => cadmpeg_ir::geometry::CompoundLoftTail::Seven {
                                    first_flag,
                                    first_scale: first_scale.map(|scale| {
                                        Box::new(map_scale(&mut out, "tail7:first", *scale))
                                    }),
                                    second_flag,
                                    second_scale: Box::new(map_scale(
                                        &mut out,
                                        "tail7:second",
                                        *second_scale,
                                    )),
                                    selector,
                                    direction,
                                    trailing_flags,
                                },
                                nurbs::EmbeddedCompoundLoftTail::Zero {
                                    flags,
                                    selector,
                                    direction,
                                    trailing_flags,
                                } => {
                                    let direction = match direction {
                                        nurbs::EmbeddedCompoundLoftDirection::Vector(value) => {
                                            cadmpeg_ir::geometry::CompoundLoftDirection::Vector {
                                                value,
                                            }
                                        }
                                        nurbs::EmbeddedCompoundLoftDirection::Curve(curve) => {
                                            let id = CurveId(format!(
                                                "f3d:brep:procedural_surface#{i}:cloft:tail0:direction"
                                            ));
                                            out.curves.push(Curve {
                                                id: id.clone(),
                                                geometry: CurveGeometry::Nurbs(curve),
                                                source_object: None,
                                            });
                                            cadmpeg_ir::geometry::CompoundLoftDirection::Curve {
                                                curve: id,
                                            }
                                        }
                                    };
                                    cadmpeg_ir::geometry::CompoundLoftTail::Zero {
                                        flags,
                                        selector,
                                        direction,
                                        trailing_flags,
                                    }
                                }
                            };
                            ProceduralSurfaceDefinition::CompoundLoft {
                                construction: Box::new(
                                    cadmpeg_ir::geometry::CompoundLoftConstruction {
                                        scales: Box::new(scales),
                                        fifth_scale,
                                        flags: embedded.flags,
                                        tail,
                                    },
                                ),
                            }
                        }
                        nurbs::DecodedProceduralSurfaceDefinition::ScaledCompoundLoft(embedded) => {
                            let embedded = *embedded;
                            let map_scale = |out: &mut Brep,
                                             name: &str,
                                             scale: nurbs::EmbeddedCompoundLoftScale| {
                                let members = scale
                                    .members
                                    .into_iter()
                                    .enumerate()
                                    .map(|(member_index, member)| {
                                        let curve = CurveId(format!(
                                            "f3d:brep:procedural_surface#{i}:scaled_cloft:{name}:member:{member_index}:curve"
                                        ));
                                        out.curves.push(Curve {
                                            id: curve.clone(),
                                            geometry: CurveGeometry::Nurbs(member.curve),
                                            source_object: None,
                                        });
                                        let surface = SurfaceId(format!(
                                            "f3d:brep:procedural_surface#{i}:scaled_cloft:{name}:member:{member_index}:surface"
                                        ));
                                        out.surfaces.push(Surface {
                                            id: surface.clone(),
                                            geometry: member.data.surface,
                                            source_object: None,
                                        });
                                        cadmpeg_ir::geometry::CompoundLoftScaleMember {
                                            type_code: member.type_code,
                                            curve,
                                            data: cadmpeg_ir::geometry::LoftProfileData {
                                                surface,
                                                pcurve: member
                                                    .data
                                                    .pcurve
                                                    .map(embedded_pcurve_geometry),
                                                first_flag: member.data.first_flag,
                                                asm_extension: member.data.asm_extension,
                                                subdata: member.data.subdata,
                                                direction: member.data.direction,
                                            },
                                        }
                                    })
                                    .collect();
                                let path = CurveId(format!(
                                    "f3d:brep:procedural_surface#{i}:scaled_cloft:{name}:path"
                                ));
                                out.curves.push(Curve {
                                    id: path.clone(),
                                    geometry: CurveGeometry::Nurbs(scale.path),
                                    source_object: None,
                                });
                                let auxiliaries = scale
                                    .auxiliaries
                                    .into_iter()
                                    .enumerate()
                                    .map(|(index, geometry)| {
                                        let id = CurveId(format!(
                                            "f3d:brep:procedural_surface#{i}:scaled_cloft:{name}:auxiliary:{index}"
                                        ));
                                        out.curves.push(Curve {
                                            id: id.clone(),
                                            geometry: CurveGeometry::Nurbs(geometry),
                                            source_object: None,
                                        });
                                        id
                                    })
                                    .collect();
                                cadmpeg_ir::geometry::CompoundLoftScale {
                                    members,
                                    path,
                                    auxiliaries,
                                    tail: scale.tail,
                                }
                            };
                            let scales = embedded
                                .scales
                                .into_iter()
                                .enumerate()
                                .map(|(index, scale)| {
                                    scale.map(|scale| {
                                        map_scale(&mut out, &format!("scale{index}"), scale)
                                    })
                                })
                                .collect::<Vec<_>>()
                                .try_into()
                                .expect("three scaled compound-loft scales");
                            let map_direction =
                                |out: &mut Brep, name: &str, direction| match direction {
                                    nurbs::EmbeddedCompoundLoftDirection::Vector(value) => {
                                        cadmpeg_ir::geometry::CompoundLoftDirection::Vector {
                                            value,
                                        }
                                    }
                                    nurbs::EmbeddedCompoundLoftDirection::Curve(curve) => {
                                        let id = CurveId(format!(
                                            "f3d:brep:procedural_surface#{i}:scaled_cloft:{name}"
                                        ));
                                        out.curves.push(Curve {
                                            id: id.clone(),
                                            geometry: CurveGeometry::Nurbs(curve),
                                            source_object: None,
                                        });
                                        cadmpeg_ir::geometry::CompoundLoftDirection::Curve {
                                            curve: id,
                                        }
                                    }
                                };
                            let branch = match embedded.branch {
                                nurbs::EmbeddedScaledCompoundLoftBranch::ExtendedVector {
                                    first_scale,
                                    second_scale,
                                    selector,
                                    direction,
                                } => {
                                    cadmpeg_ir::geometry::ScaledCompoundLoftBranch::ExtendedVector {
                                        first_scale: first_scale.map(|scale| {
                                            Box::new(map_scale(&mut out, "branch:first", *scale))
                                        }),
                                        second_scale: Box::new(map_scale(
                                            &mut out,
                                            "branch:second",
                                            *second_scale,
                                        )),
                                        selector,
                                        direction,
                                    }
                                }
                                nurbs::EmbeddedScaledCompoundLoftBranch::ExtendedCurve {
                                    scale,
                                    flag,
                                    singularity,
                                    curve,
                                } => {
                                    let id = CurveId(format!(
                                        "f3d:brep:procedural_surface#{i}:scaled_cloft:branch:curve"
                                    ));
                                    out.curves.push(Curve {
                                        id: id.clone(),
                                        geometry: CurveGeometry::Nurbs(curve),
                                        source_object: None,
                                    });
                                    cadmpeg_ir::geometry::ScaledCompoundLoftBranch::ExtendedCurve {
                                        scale: scale.map(|scale| {
                                            Box::new(map_scale(&mut out, "branch", *scale))
                                        }),
                                        flag,
                                        singularity,
                                        curve: id,
                                    }
                                }
                                nurbs::EmbeddedScaledCompoundLoftBranch::Direct {
                                    flag,
                                    selector,
                                    direction,
                                } => cadmpeg_ir::geometry::ScaledCompoundLoftBranch::Direct {
                                    flag,
                                    selector,
                                    direction: map_direction(
                                        &mut out,
                                        "branch:direction",
                                        direction,
                                    ),
                                },
                            };
                            let tail_curve = CurveId(format!(
                                "f3d:brep:procedural_surface#{i}:scaled_cloft:tail:curve"
                            ));
                            out.curves.push(Curve {
                                id: tail_curve.clone(),
                                geometry: CurveGeometry::Nurbs(embedded.tail_curve),
                                source_object: None,
                            });
                            let shape = match embedded.shape {
                                nurbs::EmbeddedScaledCompoundLoftShape::Full => {
                                    cadmpeg_ir::geometry::ScaledCompoundLoftShape::Full
                                }
                                nurbs::EmbeddedScaledCompoundLoftShape::None {
                                    parameter_ranges,
                                    parameters,
                                } => cadmpeg_ir::geometry::ScaledCompoundLoftShape::None {
                                    parameter_ranges,
                                    parameters,
                                },
                            };
                            ProceduralSurfaceDefinition::ScaledCompoundLoft {
                                construction: Box::new(
                                    cadmpeg_ir::geometry::ScaledCompoundLoftConstruction {
                                        singularity: embedded.singularity,
                                        shape,
                                        discontinuities: embedded.discontinuities,
                                        discontinuity_flag: embedded.discontinuity_flag,
                                        scales: Box::new(scales),
                                        flags: embedded.flags,
                                        selector: embedded.selector,
                                        branch,
                                        trailing_flags: embedded.trailing_flags,
                                        tail_kind: embedded.tail_kind,
                                        tail_directions: embedded.tail_directions,
                                        tail_singularity: embedded.tail_singularity,
                                        tail_curve,
                                    },
                                ),
                            }
                        }
                        nurbs::DecodedProceduralSurfaceDefinition::Skin(embedded) => {
                            fn map_law_expression(
                                out: &mut Brep,
                                owner: i64,
                                path: &str,
                                expression: nurbs::EmbeddedLawExpression,
                            ) -> cadmpeg_ir::geometry::LawExpression {
                                match expression {
                                    nurbs::EmbeddedLawExpression::Null => {
                                        cadmpeg_ir::geometry::LawExpression::Null
                                    }
                                    nurbs::EmbeddedLawExpression::Integer(value) => {
                                        cadmpeg_ir::geometry::LawExpression::Integer { value }
                                    }
                                    nurbs::EmbeddedLawExpression::Double(value) => {
                                        cadmpeg_ir::geometry::LawExpression::Double { value }
                                    }
                                    nurbs::EmbeddedLawExpression::Point(value) => {
                                        cadmpeg_ir::geometry::LawExpression::Point { value }
                                    }
                                    nurbs::EmbeddedLawExpression::Vector(value) => {
                                        cadmpeg_ir::geometry::LawExpression::Vector { value }
                                    }
                                    nurbs::EmbeddedLawExpression::Transform { scalars, enums } => {
                                        cadmpeg_ir::geometry::LawExpression::Transform {
                                            scalars,
                                            enums,
                                        }
                                    }
                                    nurbs::EmbeddedLawExpression::Edge { curve, parameters } => {
                                        let id = CurveId(format!(
                                            "f3d:brep:procedural_surface#{owner}:skin:law:{path}:edge"
                                        ));
                                        out.curves.push(Curve {
                                            id: id.clone(),
                                            geometry: CurveGeometry::Nurbs(curve),
                                            source_object: None,
                                        });
                                        cadmpeg_ir::geometry::LawExpression::Edge {
                                            curve: id,
                                            parameters,
                                        }
                                    }
                                    nurbs::EmbeddedLawExpression::Spline {
                                        native_id,
                                        knots,
                                        controls,
                                        point,
                                    } => cadmpeg_ir::geometry::LawExpression::Spline {
                                        native_id,
                                        knots,
                                        controls,
                                        point,
                                    },
                                    nurbs::EmbeddedLawExpression::Algebraic {
                                        operator,
                                        operands,
                                    } => cadmpeg_ir::geometry::LawExpression::Algebraic {
                                        operator,
                                        operands: operands
                                            .into_iter()
                                            .enumerate()
                                            .map(|(index, operand)| {
                                                map_law_expression(
                                                    out,
                                                    owner,
                                                    &format!("{path}:{index}"),
                                                    operand,
                                                )
                                            })
                                            .collect(),
                                    },
                                }
                            }
                            let embedded = *embedded;
                            let layout = match embedded.layout {
                                nurbs::EmbeddedSkinSurfaceLayout::Compact {
                                    curve,
                                    subdata,
                                    first_tail,
                                    secondary_curve,
                                    second_tail,
                                } => {
                                    let curve_id = CurveId(format!(
                                        "f3d:brep:procedural_surface#{i}:skin:curve"
                                    ));
                                    out.curves.push(Curve {
                                        id: curve_id.clone(),
                                        geometry: CurveGeometry::Nurbs(curve),
                                        source_object: None,
                                    });
                                    let secondary_id = CurveId(format!(
                                        "f3d:brep:procedural_surface#{i}:skin:secondary"
                                    ));
                                    out.curves.push(Curve {
                                        id: secondary_id.clone(),
                                        geometry: CurveGeometry::Nurbs(secondary_curve),
                                        source_object: None,
                                    });
                                    cadmpeg_ir::geometry::SkinSurfaceLayout::Compact {
                                        curve: curve_id,
                                        subdata,
                                        first_tail,
                                        secondary_curve: secondary_id,
                                        second_tail,
                                    }
                                }
                                nurbs::EmbeddedSkinSurfaceLayout::Profiles {
                                    profiles,
                                    path,
                                    tail,
                                } => {
                                    let profiles = profiles
                                        .into_iter()
                                        .enumerate()
                                        .map(|(index, profile)| {
                                            let curve = CurveId(format!(
                                                "f3d:brep:procedural_surface#{i}:skin:profile:{index}:curve"
                                            ));
                                            out.curves.push(Curve {
                                                id: curve.clone(),
                                                geometry: CurveGeometry::Nurbs(profile.curve),
                                                source_object: None,
                                            });
                                            let surface = SurfaceId(format!(
                                                "f3d:brep:procedural_surface#{i}:skin:profile:{index}:surface"
                                            ));
                                            out.surfaces.push(Surface {
                                                id: surface.clone(),
                                                geometry: profile.data.surface,
                                                source_object: None,
                                            });
                                            cadmpeg_ir::geometry::SkinSurfaceProfile {
                                                type_code: profile.type_code,
                                                curve,
                                                data: cadmpeg_ir::geometry::LoftProfileData {
                                                    surface,
                                                    pcurve: profile
                                                        .data
                                                        .pcurve
                                                        .map(embedded_pcurve_geometry),
                                                    first_flag: profile.data.first_flag,
                                                    asm_extension: profile.data.asm_extension,
                                                    subdata: profile.data.subdata,
                                                    direction: profile.data.direction,
                                                },
                                            }
                                        })
                                        .collect();
                                    let path_id = CurveId(format!(
                                        "f3d:brep:procedural_surface#{i}:skin:path"
                                    ));
                                    out.curves.push(Curve {
                                        id: path_id.clone(),
                                        geometry: CurveGeometry::Nurbs(path),
                                        source_object: None,
                                    });
                                    cadmpeg_ir::geometry::SkinSurfaceLayout::Profiles {
                                        profiles,
                                        path: path_id,
                                        tail,
                                    }
                                }
                            };
                            let parameter_curve = CurveId(format!(
                                "f3d:brep:procedural_surface#{i}:skin:parameter_curve"
                            ));
                            out.curves.push(Curve {
                                id: parameter_curve.clone(),
                                geometry: CurveGeometry::Nurbs(embedded.parameter_curve),
                                source_object: None,
                            });
                            let formula = cadmpeg_ir::geometry::LawFormula {
                                name: embedded.formula.name,
                                variables: embedded
                                    .formula
                                    .variables
                                    .into_iter()
                                    .enumerate()
                                    .map(|(variable_index, variable)| {
                                        map_law_expression(
                                            &mut out,
                                            i,
                                            &variable_index.to_string(),
                                            variable,
                                        )
                                    })
                                    .collect(),
                            };
                            ProceduralSurfaceDefinition::Skin {
                                construction: Box::new(
                                    cadmpeg_ir::geometry::SkinSurfaceConstruction {
                                        surface_boolean: embedded.surface_boolean,
                                        surface_normal: embedded.surface_normal,
                                        surface_direction: embedded.surface_direction,
                                        count: embedded.count,
                                        parameter: embedded.parameter,
                                        inner_count: embedded.inner_count,
                                        layout,
                                        direction: embedded.direction,
                                        trailing_parameter: embedded.trailing_parameter,
                                        formula,
                                        parameter_curve,
                                        discontinuities: embedded.discontinuities,
                                        discontinuity_flag: embedded.discontinuity_flag,
                                    },
                                ),
                            }
                        }
                        nurbs::DecodedProceduralSurfaceDefinition::Net(embedded) => {
                            fn map_net_law(
                                out: &mut Brep,
                                owner: i64,
                                path: &str,
                                expression: nurbs::EmbeddedLawExpression,
                            ) -> cadmpeg_ir::geometry::LawExpression {
                                match expression {
                                    nurbs::EmbeddedLawExpression::Null => {
                                        cadmpeg_ir::geometry::LawExpression::Null
                                    }
                                    nurbs::EmbeddedLawExpression::Integer(value) => {
                                        cadmpeg_ir::geometry::LawExpression::Integer { value }
                                    }
                                    nurbs::EmbeddedLawExpression::Double(value) => {
                                        cadmpeg_ir::geometry::LawExpression::Double { value }
                                    }
                                    nurbs::EmbeddedLawExpression::Point(value) => {
                                        cadmpeg_ir::geometry::LawExpression::Point { value }
                                    }
                                    nurbs::EmbeddedLawExpression::Vector(value) => {
                                        cadmpeg_ir::geometry::LawExpression::Vector { value }
                                    }
                                    nurbs::EmbeddedLawExpression::Transform { scalars, enums } => {
                                        cadmpeg_ir::geometry::LawExpression::Transform {
                                            scalars,
                                            enums,
                                        }
                                    }
                                    nurbs::EmbeddedLawExpression::Edge { curve, parameters } => {
                                        let id = CurveId(format!(
                                            "f3d:brep:procedural_surface#{owner}:net:law:{path}:edge"
                                        ));
                                        out.curves.push(Curve {
                                            id: id.clone(),
                                            geometry: CurveGeometry::Nurbs(curve),
                                            source_object: None,
                                        });
                                        cadmpeg_ir::geometry::LawExpression::Edge {
                                            curve: id,
                                            parameters,
                                        }
                                    }
                                    nurbs::EmbeddedLawExpression::Spline {
                                        native_id,
                                        knots,
                                        controls,
                                        point,
                                    } => cadmpeg_ir::geometry::LawExpression::Spline {
                                        native_id,
                                        knots,
                                        controls,
                                        point,
                                    },
                                    nurbs::EmbeddedLawExpression::Algebraic {
                                        operator,
                                        operands,
                                    } => cadmpeg_ir::geometry::LawExpression::Algebraic {
                                        operator,
                                        operands: operands
                                            .into_iter()
                                            .enumerate()
                                            .map(|(index, operand)| {
                                                map_net_law(
                                                    out,
                                                    owner,
                                                    &format!("{path}:{index}"),
                                                    operand,
                                                )
                                            })
                                            .collect(),
                                    },
                                }
                            }
                            let embedded = *embedded;
                            let sections = embedded
                                .sections
                                .into_iter()
                                .enumerate()
                                .map(|(section_index, entries)| {
                                    let entries = entries
                                        .into_iter()
                                        .enumerate()
                                        .map(|(entry_index, entry)| {
                                            let profile = entry
                                                .profile
                                                .into_iter()
                                                .enumerate()
                                                .map(|(member_index, member)| {
                                                    let curve = CurveId(format!(
                                                        "f3d:brep:procedural_surface#{i}:net:{section_index}:{entry_index}:member:{member_index}:curve"
                                                    ));
                                                    out.curves.push(Curve {
                                                        id: curve.clone(),
                                                        geometry: CurveGeometry::Nurbs(member.curve),
                                                        source_object: None,
                                                    });
                                                    let surface = SurfaceId(format!(
                                                        "f3d:brep:procedural_surface#{i}:net:{section_index}:{entry_index}:member:{member_index}:surface"
                                                    ));
                                                    out.surfaces.push(Surface {
                                                        id: surface.clone(),
                                                        geometry: member.data.surface,
                                                        source_object: None,
                                                    });
                                                    cadmpeg_ir::geometry::LoftProfileMember {
                                                        type_code: member.type_code,
                                                        curve,
                                                        data: cadmpeg_ir::geometry::LoftProfileData {
                                                            surface,
                                                            pcurve: member.data.pcurve.map(
                                                                embedded_pcurve_geometry,
                                                            ),
                                                            first_flag: member.data.first_flag,
                                                            asm_extension: member
                                                                .data
                                                                .asm_extension,
                                                            subdata: member.data.subdata,
                                                            direction: member.data.direction,
                                                        },
                                                    }
                                                })
                                                .collect();
                                            let path = CurveId(format!(
                                                "f3d:brep:procedural_surface#{i}:net:{section_index}:{entry_index}:path"
                                            ));
                                            out.curves.push(Curve {
                                                id: path.clone(),
                                                geometry: CurveGeometry::Nurbs(entry.path.curve),
                                                source_object: None,
                                            });
                                            let auxiliaries = entry
                                                .path
                                                .auxiliaries
                                                .into_iter()
                                                .enumerate()
                                                .map(|(index, geometry)| {
                                                    let id = CurveId(format!(
                                                        "f3d:brep:procedural_surface#{i}:net:{section_index}:{entry_index}:auxiliary:{index}"
                                                    ));
                                                    out.curves.push(Curve {
                                                        id: id.clone(),
                                                        geometry: CurveGeometry::Nurbs(geometry),
                                                        source_object: None,
                                                    });
                                                    id
                                                })
                                                .collect();
                                            cadmpeg_ir::geometry::LoftSectionEntry {
                                                parameter: entry.parameter,
                                                profile,
                                                path: cadmpeg_ir::geometry::LoftPath {
                                                    curve: path,
                                                    auxiliaries,
                                                    flag: entry.path.flag,
                                                },
                                            }
                                        })
                                        .collect();
                                    cadmpeg_ir::geometry::LoftSection { entries }
                                })
                                .collect::<Vec<_>>()
                                .try_into()
                                .expect("two net sections");
                            let formulas = embedded
                                .formulas
                                .into_iter()
                                .enumerate()
                                .map(
                                    |(formula_index, formula)| cadmpeg_ir::geometry::LawFormula {
                                        name: formula.name,
                                        variables: formula
                                            .variables
                                            .into_iter()
                                            .enumerate()
                                            .map(|(index, variable)| {
                                                map_net_law(
                                                    &mut out,
                                                    i,
                                                    &format!("{formula_index}:{index}"),
                                                    variable,
                                                )
                                            })
                                            .collect(),
                                    },
                                )
                                .collect::<Vec<_>>()
                                .try_into()
                                .expect("four net formulas");
                            ProceduralSurfaceDefinition::Net {
                                construction: Box::new(
                                    cadmpeg_ir::geometry::NetSurfaceConstruction {
                                        sections: Box::new(sections),
                                        frame_parameters: embedded.frame_parameters,
                                        flag: embedded.flag,
                                        directions: embedded.directions,
                                        formulas: Box::new(formulas),
                                        discontinuities: embedded.discontinuities,
                                        discontinuity_flag: embedded.discontinuity_flag,
                                    },
                                ),
                            }
                        }
                        nurbs::DecodedProceduralSurfaceDefinition::Sweep(embedded) => {
                            fn map_sweep_law(
                                out: &mut Brep,
                                owner: i64,
                                path: &str,
                                expression: nurbs::EmbeddedLawExpression,
                            ) -> cadmpeg_ir::geometry::LawExpression {
                                match expression {
                                    nurbs::EmbeddedLawExpression::Null => {
                                        cadmpeg_ir::geometry::LawExpression::Null
                                    }
                                    nurbs::EmbeddedLawExpression::Integer(value) => {
                                        cadmpeg_ir::geometry::LawExpression::Integer { value }
                                    }
                                    nurbs::EmbeddedLawExpression::Double(value) => {
                                        cadmpeg_ir::geometry::LawExpression::Double { value }
                                    }
                                    nurbs::EmbeddedLawExpression::Point(value) => {
                                        cadmpeg_ir::geometry::LawExpression::Point { value }
                                    }
                                    nurbs::EmbeddedLawExpression::Vector(value) => {
                                        cadmpeg_ir::geometry::LawExpression::Vector { value }
                                    }
                                    nurbs::EmbeddedLawExpression::Transform { scalars, enums } => {
                                        cadmpeg_ir::geometry::LawExpression::Transform {
                                            scalars,
                                            enums,
                                        }
                                    }
                                    nurbs::EmbeddedLawExpression::Edge { curve, parameters } => {
                                        let id = CurveId(format!(
                                            "f3d:brep:procedural_surface#{owner}:sweep:law:{path}:edge"
                                        ));
                                        out.curves.push(Curve {
                                            id: id.clone(),
                                            geometry: CurveGeometry::Nurbs(curve),
                                            source_object: None,
                                        });
                                        cadmpeg_ir::geometry::LawExpression::Edge {
                                            curve: id,
                                            parameters,
                                        }
                                    }
                                    nurbs::EmbeddedLawExpression::Spline {
                                        native_id,
                                        knots,
                                        controls,
                                        point,
                                    } => cadmpeg_ir::geometry::LawExpression::Spline {
                                        native_id,
                                        knots,
                                        controls,
                                        point,
                                    },
                                    nurbs::EmbeddedLawExpression::Algebraic {
                                        operator,
                                        operands,
                                    } => cadmpeg_ir::geometry::LawExpression::Algebraic {
                                        operator,
                                        operands: operands
                                            .into_iter()
                                            .enumerate()
                                            .map(|(index, operand)| {
                                                map_sweep_law(
                                                    out,
                                                    owner,
                                                    &format!("{path}:{index}"),
                                                    operand,
                                                )
                                            })
                                            .collect(),
                                    },
                                }
                            }
                            let embedded = *embedded;
                            let (profile_geometry, spine_geometry, layout) = match embedded.layout {
                                nurbs::EmbeddedSweepSurfaceLayout::ProfileFirst {
                                    profile,
                                    spine,
                                    secondary_kind,
                                    directions,
                                    origin,
                                    parameters,
                                    formulas,
                                } => {
                                    let formulas = formulas
                                        .into_iter()
                                        .enumerate()
                                        .map(|(formula_index, formula)| {
                                            cadmpeg_ir::geometry::LawFormula {
                                                name: formula.name,
                                                variables: formula
                                                    .variables
                                                    .into_iter()
                                                    .enumerate()
                                                    .map(|(index, variable)| {
                                                        map_sweep_law(
                                                            &mut out,
                                                            i,
                                                            &format!("{formula_index}:{index}"),
                                                            variable,
                                                        )
                                                    })
                                                    .collect(),
                                            }
                                        })
                                        .collect::<Vec<_>>()
                                        .try_into()
                                        .expect("three sweep formulas");
                                    (
                                        profile,
                                        spine,
                                        cadmpeg_ir::geometry::SweepSurfaceLayout::ProfileFirst {
                                            secondary_kind,
                                            directions,
                                            origin,
                                            parameters,
                                            formulas: Box::new(formulas),
                                        },
                                    )
                                }
                                nurbs::EmbeddedSweepSurfaceLayout::ExplicitFormula {
                                    profile,
                                    mode,
                                    profile_range,
                                    profile_frame,
                                    origin,
                                    directions,
                                    trajectory_flag,
                                    path,
                                    path_range,
                                    path_parameter,
                                    formula_flag,
                                    formula,
                                    trailing_flag,
                                } => {
                                    let formula = cadmpeg_ir::geometry::LawFormula {
                                        name: formula.name,
                                        variables: formula
                                            .variables
                                            .into_iter()
                                            .enumerate()
                                            .map(|(index, variable)| {
                                                map_sweep_law(
                                                    &mut out,
                                                    i,
                                                    &format!("explicit:{index}"),
                                                    variable,
                                                )
                                            })
                                            .collect(),
                                    };
                                    (
                                        profile,
                                        path,
                                        cadmpeg_ir::geometry::SweepSurfaceLayout::ExplicitFormula {
                                            mode,
                                            profile_range,
                                            profile_frame,
                                            origin,
                                            directions,
                                            trajectory_flag,
                                            path_range,
                                            path_parameter,
                                            formula_flag,
                                            formula,
                                            trailing_flag,
                                        },
                                    )
                                }
                                nurbs::EmbeddedSweepSurfaceLayout::ExplicitGuide {
                                    profile,
                                    mode,
                                    profile_range,
                                    profile_frame,
                                    origin,
                                    directions,
                                    trajectory_flag,
                                    path,
                                    path_range,
                                    path_parameter,
                                    guide_flags,
                                    guide_curve,
                                    guide_range,
                                    guide_modes,
                                    guide_parameters,
                                    trailing_flags,
                                } => {
                                    let guide_curve_id = CurveId(format!(
                                        "f3d:brep:procedural_surface#{i}:sweep:guide"
                                    ));
                                    out.curves.push(Curve {
                                        id: guide_curve_id.clone(),
                                        geometry: CurveGeometry::Nurbs(guide_curve),
                                        source_object: None,
                                    });
                                    (
                                        profile,
                                        path,
                                        cadmpeg_ir::geometry::SweepSurfaceLayout::ExplicitGuide {
                                            mode,
                                            profile_range,
                                            profile_frame,
                                            origin,
                                            directions,
                                            trajectory_flag,
                                            path_range,
                                            path_parameter,
                                            guide_flags,
                                            guide_curve: guide_curve_id,
                                            guide_range,
                                            guide_modes,
                                            guide_parameters,
                                            trailing_flags,
                                        },
                                    )
                                }
                                nurbs::EmbeddedSweepSurfaceLayout::ExplicitSurface {
                                    profile,
                                    mode,
                                    profile_range,
                                    profile_frame,
                                    origin,
                                    directions,
                                    trajectory_flag,
                                    path,
                                    path_range,
                                    path_parameter,
                                    singularity,
                                    support_surface,
                                    auxiliary_curve,
                                    support_flag,
                                    legacy_flag,
                                } => {
                                    let support_surface_id = SurfaceId(format!(
                                        "f3d:brep:procedural_surface#{i}:sweep:support"
                                    ));
                                    out.surfaces.push(Surface {
                                        id: support_surface_id.clone(),
                                        geometry: support_surface,
                                        source_object: None,
                                    });
                                    let auxiliary_curve = auxiliary_curve.map(|geometry| {
                                        let id = CurveId(format!(
                                            "f3d:brep:procedural_surface#{i}:sweep:auxiliary"
                                        ));
                                        out.curves.push(Curve {
                                            id: id.clone(),
                                            geometry: CurveGeometry::Nurbs(geometry),
                                            source_object: None,
                                        });
                                        id
                                    });
                                    (
                                        profile,
                                        path,
                                        cadmpeg_ir::geometry::SweepSurfaceLayout::ExplicitSurface {
                                            mode,
                                            profile_range,
                                            profile_frame,
                                            origin,
                                            directions,
                                            trajectory_flag,
                                            path_range,
                                            path_parameter,
                                            singularity,
                                            support_surface: support_surface_id,
                                            auxiliary_curve,
                                            support_flag,
                                            legacy_flag,
                                        },
                                    )
                                }
                                nurbs::EmbeddedSweepSurfaceLayout::LawDriven {
                                    profile,
                                    mode,
                                    profile_range,
                                    profile_frame,
                                    origin,
                                    directions,
                                    first_law,
                                    first_mode,
                                    first_range,
                                    law_direction,
                                    path_mode,
                                    path_flag,
                                    path,
                                    path_range,
                                    path_parameter,
                                    second_law_flag,
                                    second_law,
                                    formula_mode,
                                    formula,
                                    trailing_flag,
                                } => {
                                    let first_law =
                                        map_sweep_law(&mut out, i, "law:first", first_law);
                                    let second_law =
                                        map_sweep_law(&mut out, i, "law:second", second_law);
                                    let formula = cadmpeg_ir::geometry::LawFormula {
                                        name: formula.name,
                                        variables: formula
                                            .variables
                                            .into_iter()
                                            .enumerate()
                                            .map(|(index, variable)| {
                                                map_sweep_law(
                                                    &mut out,
                                                    i,
                                                    &format!("law:formula:{index}"),
                                                    variable,
                                                )
                                            })
                                            .collect(),
                                    };
                                    (
                                        profile,
                                        path,
                                        cadmpeg_ir::geometry::SweepSurfaceLayout::LawDriven {
                                            mode,
                                            profile_range,
                                            profile_frame,
                                            origin,
                                            directions,
                                            first_law: Box::new(first_law),
                                            first_mode,
                                            first_range,
                                            law_direction,
                                            path_mode,
                                            path_flag,
                                            path_range,
                                            path_parameter,
                                            second_law_flag,
                                            second_law: Box::new(second_law),
                                            formula_mode,
                                            formula,
                                            trailing_flag,
                                        },
                                    )
                                }
                            };
                            let profile =
                                CurveId(format!("f3d:brep:procedural_surface#{i}:sweep:profile"));
                            out.curves.push(Curve {
                                id: profile.clone(),
                                geometry: CurveGeometry::Nurbs(profile_geometry),
                                source_object: None,
                            });
                            let spine =
                                CurveId(format!("f3d:brep:procedural_surface#{i}:sweep:spine"));
                            out.curves.push(Curve {
                                id: spine.clone(),
                                geometry: CurveGeometry::Nurbs(spine_geometry),
                                source_object: None,
                            });
                            ProceduralSurfaceDefinition::Sweep {
                                profile,
                                spine,
                                native: Some(Box::new(
                                    cadmpeg_ir::geometry::SweepSurfaceConstruction {
                                        primary_kind: embedded.primary_kind,
                                        layout,
                                        discontinuities: embedded.discontinuities,
                                        discontinuity_flag: embedded.discontinuity_flag,
                                    },
                                )),
                            }
                        }
                        nurbs::DecodedProceduralSurfaceDefinition::G2Blend(embedded) => {
                            let embedded = *embedded;
                            let mut add_side = |name: &str, side: nurbs::EmbeddedG2Side| {
                                let surface = SurfaceId(format!(
                                    "f3d:brep:procedural_surface#{i}:g2:{name}:surface"
                                ));
                                out.surfaces.push(Surface {
                                    id: surface.clone(),
                                    geometry: side.surface,
                                    source_object: None,
                                });
                                let curve = CurveId(format!(
                                    "f3d:brep:procedural_surface#{i}:g2:{name}:curve"
                                ));
                                out.curves.push(Curve {
                                    id: curve.clone(),
                                    geometry: CurveGeometry::Nurbs(side.curve),
                                    source_object: None,
                                });
                                let pcurves = side.pcurves.map(|pcurve| {
                                    pcurve.map(|pcurve| PcurveGeometry::Nurbs {
                                        degree: pcurve.degree,
                                        knots: pcurve.knots,
                                        control_points: pcurve.control_points,
                                        weights: pcurve.weights,
                                        periodic: pcurve.periodic,
                                    })
                                });
                                cadmpeg_ir::geometry::G2BlendSide {
                                    label: side.label,
                                    surface,
                                    curve,
                                    pcurves,
                                    direction: side.direction,
                                }
                            };
                            let first = add_side("first", embedded.first);
                            let second = add_side("second", embedded.second);
                            let first_shape = match embedded.first_shape {
                                nurbs::EmbeddedG2FirstShape::Full { surface, tolerance } => {
                                    let surface = surface.map(|geometry| {
                                        let id = SurfaceId(format!(
                                            "f3d:brep:procedural_surface#{i}:g2:first_exact"
                                        ));
                                        out.surfaces.push(Surface {
                                            id: id.clone(),
                                            geometry: SurfaceGeometry::Nurbs(geometry),
                                            source_object: None,
                                        });
                                        id
                                    });
                                    cadmpeg_ir::geometry::G2BlendFirstShape::Full {
                                        surface,
                                        tolerance,
                                    }
                                }
                                nurbs::EmbeddedG2FirstShape::None {
                                    coefficients,
                                    tolerance,
                                    extension,
                                    pcurve,
                                } => cadmpeg_ir::geometry::G2BlendFirstShape::None {
                                    coefficients,
                                    tolerance,
                                    extension,
                                    pcurve: pcurve.map(|pcurve| PcurveGeometry::Nurbs {
                                        degree: pcurve.degree,
                                        knots: pcurve.knots,
                                        control_points: pcurve.control_points,
                                        weights: pcurve.weights,
                                        periodic: pcurve.periodic,
                                    }),
                                },
                            };
                            let second_exact_surface = SurfaceId(format!(
                                "f3d:brep:procedural_surface#{i}:g2:second_exact"
                            ));
                            out.surfaces.push(Surface {
                                id: second_exact_surface.clone(),
                                geometry: SurfaceGeometry::Nurbs(embedded.second_exact_surface),
                                source_object: None,
                            });
                            let center_curve =
                                CurveId(format!("f3d:brep:procedural_surface#{i}:g2:center"));
                            out.curves.push(Curve {
                                id: center_curve.clone(),
                                geometry: CurveGeometry::Nurbs(embedded.center_curve),
                                source_object: None,
                            });
                            ProceduralSurfaceDefinition::G2Blend {
                                construction: Box::new(cadmpeg_ir::geometry::G2BlendConstruction {
                                    first,
                                    singularity: embedded.singularity,
                                    first_shape,
                                    second,
                                    second_exact_surface,
                                    center_curve,
                                    center_parameters: embedded.center_parameters,
                                    center_flag: embedded.center_flag,
                                    parameter_ranges: embedded.parameter_ranges,
                                    trailing_parameters: embedded.trailing_parameters,
                                    discontinuities: embedded.discontinuities,
                                }),
                            }
                        }
                        nurbs::DecodedProceduralSurfaceDefinition::Ruled { first, second } => {
                            let first_id =
                                CurveId(format!("f3d:brep:procedural_surface#{i}:profile0"));
                            let second_id =
                                CurveId(format!("f3d:brep:procedural_surface#{i}:profile1"));
                            out.curves.push(Curve {
                                id: first_id.clone(),
                                geometry: CurveGeometry::Nurbs(first),
                                source_object: None,
                            });
                            out.curves.push(Curve {
                                id: second_id.clone(),
                                geometry: CurveGeometry::Nurbs(second),
                                source_object: None,
                            });
                            ProceduralSurfaceDefinition::Ruled {
                                first: first_id,
                                second: second_id,
                            }
                        }
                        nurbs::DecodedProceduralSurfaceDefinition::Sum {
                            first,
                            second,
                            basepoint,
                        } => {
                            let first_id =
                                CurveId(format!("f3d:brep:procedural_surface#{i}:curve0"));
                            let second_id =
                                CurveId(format!("f3d:brep:procedural_surface#{i}:curve1"));
                            out.curves.push(Curve {
                                id: first_id.clone(),
                                geometry: CurveGeometry::Nurbs(first),
                                source_object: None,
                            });
                            out.curves.push(Curve {
                                id: second_id.clone(),
                                geometry: CurveGeometry::Nurbs(second),
                                source_object: None,
                            });
                            ProceduralSurfaceDefinition::Sum {
                                first: first_id,
                                second: second_id,
                                basepoint,
                            }
                        }
                        nurbs::DecodedProceduralSurfaceDefinition::Revolution {
                            directrix,
                            axis_origin,
                            axis_direction,
                            angular_interval,
                            parameter_interval,
                        } => {
                            let directrix_id =
                                CurveId(format!("f3d:brep:procedural_surface#{i}:directrix"));
                            out.curves.push(Curve {
                                id: directrix_id.clone(),
                                geometry: CurveGeometry::Nurbs(directrix),
                                source_object: None,
                            });
                            ProceduralSurfaceDefinition::Revolution {
                                directrix: directrix_id,
                                axis_origin,
                                axis_direction,
                                angular_interval,
                                parameter_interval: Some(parameter_interval),
                                transposed: false,
                            }
                        }
                        nurbs::DecodedProceduralSurfaceDefinition::Offset {
                            support,
                            distance,
                            u_sense,
                            v_sense,
                            extension_flags,
                        } => {
                            let support_id =
                                SurfaceId(format!("f3d:brep:procedural_surface#{i}:support"));
                            out.surfaces.push(Surface {
                                id: support_id.clone(),
                                geometry: support,
                                source_object: None,
                            });
                            ProceduralSurfaceDefinition::Offset {
                                support: support_id,
                                distance,
                                u_sense: Some(u_sense),
                                v_sense: Some(v_sense),
                                extension_flags,
                            }
                        }
                        nurbs::DecodedProceduralSurfaceDefinition::Extrusion {
                            directrix,
                            parameter_interval,
                            direction,
                            native_position,
                        } => {
                            let directrix_id =
                                CurveId(format!("f3d:brep:procedural_surface#{i}:directrix"));
                            out.curves.push(Curve {
                                id: directrix_id.clone(),
                                geometry: CurveGeometry::Nurbs(directrix),
                                source_object: None,
                            });
                            ProceduralSurfaceDefinition::Extrusion {
                                directrix: directrix_id,
                                parameter_interval: Some(parameter_interval),
                                direction,
                                native_position: Some(native_position),
                            }
                        }
                        nurbs::DecodedProceduralSurfaceDefinition::VariableBlend(construction) => {
                            let mut sides = Vec::with_capacity(2);
                            for (side_index, side) in construction.sides.into_iter().enumerate() {
                                let prefix = format!(
                                    "f3d:brep:procedural_surface#{i}:variable_side{side_index}"
                                );
                                let surface = SurfaceId(format!("{prefix}:surface"));
                                out.surfaces.push(Surface {
                                    id: surface.clone(),
                                    geometry: side.surface,
                                    source_object: None,
                                });
                                let curve = CurveId(format!("{prefix}:curve"));
                                out.curves.push(Curve {
                                    id: curve.clone(),
                                    geometry: CurveGeometry::Nurbs(side.curve),
                                    source_object: None,
                                });
                                sides.push(VariableBlendSide {
                                    label: side.label,
                                    surface,
                                    curve,
                                    pcurve: side.pcurve.map(embedded_pcurve_geometry),
                                    location: side.location,
                                    secondary_pcurve: side
                                        .secondary_pcurve
                                        .map(embedded_pcurve_geometry),
                                    scalar: side.scalar,
                                    tertiary_pcurve: side
                                        .tertiary_pcurve
                                        .map(embedded_pcurve_geometry),
                                });
                            }
                            let [first, second]: [VariableBlendSide; 2] = sides
                                .try_into()
                                .expect("invariant: variable blend has two sides");
                            let mut add_curve = |suffix: &str, geometry: NurbsCurve| {
                                let id = CurveId(format!(
                                    "f3d:brep:procedural_surface#{i}:variable_{suffix}"
                                ));
                                out.curves.push(Curve {
                                    id: id.clone(),
                                    geometry: CurveGeometry::Nurbs(geometry),
                                    source_object: None,
                                });
                                id
                            };
                            let primary_curve = add_curve("primary", construction.primary_curve);
                            let secondary_curve =
                                add_curve("secondary", construction.secondary_curve);
                            let post_curve = add_curve("post", construction.post_curve);
                            ProceduralSurfaceDefinition::VariableBlend {
                                construction: Box::new(VariableBlendConstruction {
                                    sides: Box::new([first, second]),
                                    primary_curve,
                                    offsets: construction.offsets,
                                    radius_kind: construction.radius_kind,
                                    first_value: construction.first_value,
                                    second_value: construction.second_value,
                                    chamfer: construction.chamfer,
                                    single_radius_tail: construction.single_radius_tail,
                                    u_range: construction.u_range,
                                    v_range: construction.v_range,
                                    shape_prefix: construction.shape_prefix,
                                    shape_parameter: construction.shape_parameter,
                                    shape_length: construction.shape_length,
                                    shape_tail: construction.shape_tail,
                                    shape_extensions: construction.shape_extensions,
                                    secondary_curve,
                                    convexity: construction.convexity,
                                    render_blend: construction.render_blend,
                                    post_range: construction.post_range,
                                    post_curve,
                                    post_pcurve: construction
                                        .post_pcurve
                                        .map(embedded_pcurve_geometry),
                                }),
                            }
                        }
                        nurbs::DecodedProceduralSurfaceDefinition::VertexBlend(construction) => {
                            let mut boundaries = Vec::with_capacity(construction.boundaries.len());
                            for (boundary_index, boundary) in
                                construction.boundaries.into_iter().enumerate()
                            {
                                let prefix = format!(
                                    "f3d:brep:procedural_surface#{i}:vertex_boundary{boundary_index}"
                                );
                                let geometry = match boundary.geometry {
                                    nurbs::EmbeddedVertexBlendBoundaryGeometry::Circle {
                                        curve,
                                        form,
                                        twists,
                                        parameters,
                                        sense,
                                    } => {
                                        let id = CurveId(format!("{prefix}:curve"));
                                        out.curves.push(Curve {
                                            id: id.clone(),
                                            geometry: CurveGeometry::Nurbs(curve),
                                            source_object: None,
                                        });
                                        VertexBlendBoundaryGeometry::Circle {
                                            curve: id,
                                            form,
                                            twists,
                                            parameters,
                                            sense,
                                        }
                                    }
                                    nurbs::EmbeddedVertexBlendBoundaryGeometry::Degenerate {
                                        location,
                                        normals,
                                    } => VertexBlendBoundaryGeometry::Degenerate {
                                        location,
                                        normals,
                                    },
                                    nurbs::EmbeddedVertexBlendBoundaryGeometry::Pcurve {
                                        surface,
                                        pcurve,
                                        sense,
                                        fit_tolerance,
                                    } => {
                                        let id = SurfaceId(format!("{prefix}:surface"));
                                        out.surfaces.push(Surface {
                                            id: id.clone(),
                                            geometry: surface,
                                            source_object: None,
                                        });
                                        VertexBlendBoundaryGeometry::Pcurve {
                                            surface: id,
                                            pcurve: pcurve.map(embedded_pcurve_geometry),
                                            sense,
                                            fit_tolerance,
                                        }
                                    }
                                    nurbs::EmbeddedVertexBlendBoundaryGeometry::Plane {
                                        normal,
                                        parameters,
                                        curve,
                                    } => {
                                        let id = CurveId(format!("{prefix}:curve"));
                                        out.curves.push(Curve {
                                            id: id.clone(),
                                            geometry: CurveGeometry::Nurbs(curve),
                                            source_object: None,
                                        });
                                        VertexBlendBoundaryGeometry::Plane {
                                            normal,
                                            parameters,
                                            curve: id,
                                        }
                                    }
                                };
                                boundaries.push(VertexBlendBoundary {
                                    boundary_type: boundary.boundary_type,
                                    magic: boundary.magic,
                                    u_smoothing: boundary.u_smoothing,
                                    v_smoothing: boundary.v_smoothing,
                                    fullness: boundary.fullness,
                                    geometry,
                                });
                            }
                            ProceduralSurfaceDefinition::VertexBlend {
                                construction: Box::new(VertexBlendConstruction {
                                    boundaries,
                                    grid_size: construction.grid_size,
                                    fit_tolerance: construction.fit_tolerance,
                                }),
                            }
                        }
                        nurbs::DecodedProceduralSurfaceDefinition::Blend {
                            supports,
                            spine,
                            radius,
                            cross_section,
                            native,
                        } => {
                            let mut resolved_supports = [None, None];
                            for (side, support) in supports.into_iter().enumerate() {
                                if let Some(support) = support {
                                    let support_id = SurfaceId(format!(
                                        "f3d:brep:procedural_surface#{i}:support{side}"
                                    ));
                                    out.surfaces.push(Surface {
                                        id: support_id.clone(),
                                        geometry: support,
                                        source_object: None,
                                    });
                                    resolved_supports[side] = Some(BlendSupport {
                                        surface: support_id,
                                        reversed: false,
                                    });
                                }
                            }
                            let spine = spine.map(|spine| {
                                let spine_id =
                                    CurveId(format!("f3d:brep:procedural_surface#{i}:spine"));
                                out.curves.push(Curve {
                                    id: spine_id.clone(),
                                    geometry: CurveGeometry::Nurbs(spine),
                                    source_object: None,
                                });
                                spine_id
                            });
                            let native = native.map(|native| {
                                let mut resolved_sides = Vec::with_capacity(2);
                                for (side_index, side) in native.sides.into_iter().enumerate() {
                                    let prefix = format!(
                                        "f3d:brep:procedural_surface#{i}:native_side{side_index}"
                                    );
                                    let surface = side.surface.map(|geometry| {
                                        let id = SurfaceId(format!("{prefix}:surface"));
                                        out.surfaces.push(Surface {
                                            id: id.clone(),
                                            geometry,
                                            source_object: None,
                                        });
                                        id
                                    });
                                    let curve = CurveId(format!("{prefix}:curve"));
                                    out.curves.push(Curve {
                                        id: curve.clone(),
                                        geometry: CurveGeometry::Nurbs(side.curve),
                                        source_object: None,
                                    });
                                    let exact_support = side.exact_support.map(|geometry| {
                                        let id = SurfaceId(format!("{prefix}:exact_support"));
                                        out.surfaces.push(Surface {
                                            id: id.clone(),
                                            geometry: SurfaceGeometry::Nurbs(geometry),
                                            source_object: None,
                                        });
                                        id
                                    });
                                    resolved_sides.push(RollingBallSide {
                                        label: side.label,
                                        surface,
                                        curve,
                                        pcurve: side.pcurve.map(embedded_pcurve_geometry),
                                        location: side.location,
                                        secondary_pcurve: side
                                            .secondary_pcurve
                                            .map(embedded_pcurve_geometry),
                                        exact_support,
                                    });
                                }
                                let [first, second]: [RollingBallSide; 2] = resolved_sides
                                    .try_into()
                                    .expect("invariant: native rolling-ball has two sides");
                                let slice = CurveId(format!(
                                    "f3d:brep:procedural_surface#{i}:native_slice"
                                ));
                                out.curves.push(Curve {
                                    id: slice.clone(),
                                    geometry: CurveGeometry::Nurbs(native.slice),
                                    source_object: None,
                                });
                                let third = native.third.map(|side| {
                                    let prefix =
                                        format!("f3d:brep:procedural_surface#{i}:native_third");
                                    let surface = SurfaceId(format!("{prefix}:surface"));
                                    out.surfaces.push(Surface {
                                        id: surface.clone(),
                                        geometry: side.surface,
                                        source_object: None,
                                    });
                                    let curve = CurveId(format!("{prefix}:curve"));
                                    out.curves.push(Curve {
                                        id: curve.clone(),
                                        geometry: CurveGeometry::Nurbs(side.curve),
                                        source_object: None,
                                    });
                                    Box::new(RollingBallThirdSide {
                                        label: side.label,
                                        surface,
                                        curve,
                                        pcurve: side.pcurve.map(embedded_pcurve_geometry),
                                        direction: side.direction,
                                        secondary_pcurve: side
                                            .secondary_pcurve
                                            .map(embedded_pcurve_geometry),
                                        extension: side.extension,
                                        tertiary_pcurve: side
                                            .tertiary_pcurve
                                            .map(embedded_pcurve_geometry),
                                        flag: side.flag,
                                    })
                                });
                                Box::new(RollingBallConstruction {
                                    sides: Box::new([first, second]),
                                    slice,
                                    offsets: native.offsets,
                                    radius_selector: match native.radius_selector {
                                        nurbs::EmbeddedRollingBallRadiusSelector::None => {
                                            RollingBallRadiusSelector::None
                                        }
                                        nurbs::EmbeddedRollingBallRadiusSelector::Value(value) => {
                                            RollingBallRadiusSelector::Value { value }
                                        }
                                    },
                                    u_range: native.u_range,
                                    v_range: native.v_range,
                                    parameters: native.parameters,
                                    tail: native.tail,
                                    discontinuities: native.discontinuities,
                                    third,
                                })
                            });
                            if resolved_supports
                                .iter()
                                .filter(|support| support.is_some())
                                .count()
                                == 1
                            {
                                out.stats.partial_procedural_supports += 1;
                            }
                            ProceduralSurfaceDefinition::Blend {
                                supports: resolved_supports,
                                spine,
                                radius,
                                cross_section,
                                native,
                            }
                        }
                    };
                    out.procedural_surfaces.push(ProceduralSurface {
                        id: format!("f3d:brep:procedural_surface#{i}").into(),
                        surface: SurfaceId(id(i)),
                        definition,
                        cache_fit_tolerance: procedural.cache_fit_tolerance,
                    });
                }
            }
            _ if unknown_surface_records.contains(&i) => {
                // Topology-known face on an undecoded surface: emit an opaque
                // carrier linking to the preserved record bytes, marked Unknown.
                out.surfaces.push(Surface {
                    id: SurfaceId(id(i)),
                    geometry: SurfaceGeometry::Unknown {
                        record: Some(UnknownId(unknown_record_id(r))),
                    },
                    source_object: None,
                });
            }
            _ if kept_curves.contains(&i) => {
                let Some(mut geometry) = curve_geo.remove(&i) else {
                    continue;
                };
                if reversed_curve_refs.contains(&i) {
                    if forward_curve_refs.contains(&i) {
                        let mut reversed = geometry.clone();
                        reverse_curve_geometry(&mut reversed);
                        out.curves.push(Curve {
                            id: CurveId(format!("{}:reversed", id(i))),
                            geometry: reversed,
                            source_object: None,
                        });
                    } else {
                        reverse_curve_geometry(&mut geometry);
                    }
                }
                out.curves.push(Curve {
                    id: CurveId(id(i)),
                    geometry,
                    source_object: None,
                });
                if let Some(procedural) = procedural_curve_defs.remove(&i) {
                    let definition = if let Some((source, parameter_range, offset, labels, codes)) =
                        procedural.2
                    {
                        let source_id = CurveId(format!("f3d:brep:procedural_curve#{i}:source"));
                        out.curves.push(Curve {
                            id: source_id.clone(),
                            geometry: CurveGeometry::Nurbs(source),
                            source_object: None,
                        });
                        cadmpeg_ir::geometry::ProceduralCurveDefinition::VectorOffset {
                            source: source_id,
                            parameter_range,
                            offset,
                            labels,
                            codes,
                        }
                    } else if let Some((source, parameter_range)) = procedural.3 {
                        let source_id = CurveId(format!("f3d:brep:procedural_curve#{i}:source"));
                        out.curves.push(Curve {
                            id: source_id.clone(),
                            geometry: CurveGeometry::Nurbs(source),
                            source_object: None,
                        });
                        cadmpeg_ir::geometry::ProceduralCurveDefinition::Subset {
                            source: source_id,
                            parameter_range,
                        }
                    } else if let Some(embedded) = procedural.5 {
                        let surfaces: [Option<SurfaceId>; 2] = embedded
                            .surfaces
                            .into_iter()
                            .enumerate()
                            .map(|(side, geometry)| {
                                let geometry = geometry?;
                                let id = SurfaceId(format!(
                                    "f3d:brep:procedural_curve#{i}:support{side}"
                                ));
                                out.surfaces.push(Surface {
                                    id: id.clone(),
                                    geometry,
                                    source_object: None,
                                });
                                Some(id)
                            })
                            .collect::<Vec<_>>()
                            .try_into()
                            .expect("two fixed support sides");
                        let pcurves = embedded.pcurves.map(|pcurve| {
                            pcurve.map(|pcurve| PcurveGeometry::Nurbs {
                                degree: pcurve.degree,
                                knots: pcurve.knots,
                                control_points: pcurve.control_points,
                                weights: pcurve.weights,
                                periodic: pcurve.periodic,
                            })
                        });
                        cadmpeg_ir::geometry::ProceduralCurveDefinition::TwoSidedOffset {
                            context: cadmpeg_ir::geometry::IntcurveSupportContext {
                                sides: std::array::from_fn(|side| {
                                    cadmpeg_ir::geometry::IntcurveSupportSide {
                                        surface: surfaces[side].clone(),
                                        pcurve: pcurves[side].clone(),
                                        pcurve_parameter_range: None,
                                    }
                                }),
                                parameter_range: embedded.parameter_range,
                                discontinuities: embedded.discontinuities,
                            },
                            discontinuity_flag: embedded.discontinuity_flag,
                            offsets: embedded.offsets,
                        }
                    } else if let Some((embedded, discontinuity_flag)) = procedural.6 {
                        let surfaces: [Option<SurfaceId>; 2] = embedded
                            .surfaces
                            .into_iter()
                            .enumerate()
                            .map(|(side, geometry)| {
                                let id = SurfaceId(format!(
                                    "f3d:brep:procedural_curve#{i}:support{side}"
                                ));
                                out.surfaces.push(Surface {
                                    id: id.clone(),
                                    geometry,
                                    source_object: None,
                                });
                                Some(id)
                            })
                            .collect::<Vec<_>>()
                            .try_into()
                            .expect("two fixed support sides");
                        let pcurves = embedded.pcurves.map(|pcurve| {
                            Some(PcurveGeometry::Nurbs {
                                degree: pcurve.degree,
                                knots: pcurve.knots,
                                control_points: pcurve.control_points,
                                weights: pcurve.weights,
                                periodic: pcurve.periodic,
                            })
                        });
                        cadmpeg_ir::geometry::ProceduralCurveDefinition::Intersection {
                            context: cadmpeg_ir::geometry::IntcurveSupportContext {
                                sides: std::array::from_fn(|side| {
                                    cadmpeg_ir::geometry::IntcurveSupportSide {
                                        surface: surfaces[side].clone(),
                                        pcurve: pcurves[side].clone(),
                                        pcurve_parameter_range: None,
                                    }
                                }),
                                parameter_range: embedded.parameter_range,
                                discontinuities: embedded.discontinuities,
                            },
                            discontinuity_flag,
                        }
                    } else if let Some(embedded) = procedural.7 {
                        let surface_ids: [SurfaceId; 3] = embedded
                            .surfaces
                            .into_iter()
                            .enumerate()
                            .map(|(side, geometry)| {
                                let id = SurfaceId(format!(
                                    "f3d:brep:procedural_curve#{i}:support{side}"
                                ));
                                out.surfaces.push(Surface {
                                    id: id.clone(),
                                    geometry,
                                    source_object: None,
                                });
                                id
                            })
                            .collect::<Vec<_>>()
                            .try_into()
                            .expect("three fixed support sides");
                        let pcurves = embedded.pcurves.map(|pcurve| PcurveGeometry::Nurbs {
                            degree: pcurve.degree,
                            knots: pcurve.knots,
                            control_points: pcurve.control_points,
                            weights: pcurve.weights,
                            periodic: pcurve.periodic,
                        });
                        cadmpeg_ir::geometry::ProceduralCurveDefinition::ThreeSurfaceIntersection {
                            context: cadmpeg_ir::geometry::IntcurveSupportContext {
                                sides: std::array::from_fn(|side| {
                                    cadmpeg_ir::geometry::IntcurveSupportSide {
                                        surface: Some(surface_ids[side].clone()),
                                        pcurve: Some(pcurves[side].clone()),
                                        pcurve_parameter_range: None,
                                    }
                                }),
                                parameter_range: embedded.parameter_range,
                                discontinuities: embedded.discontinuities,
                            },
                            selector: embedded.selector,
                            third: cadmpeg_ir::geometry::IntcurveSupportSide {
                                surface: Some(surface_ids[2].clone()),
                                pcurve: Some(pcurves[2].clone()),
                                pcurve_parameter_range: None,
                            },
                        }
                    } else if let Some((family, embedded)) = procedural.8 {
                        let surfaces: [Option<SurfaceId>; 2] = embedded
                            .surfaces
                            .into_iter()
                            .enumerate()
                            .map(|(side, geometry)| {
                                let id = SurfaceId(format!(
                                    "f3d:brep:procedural_curve#{i}:support{side}"
                                ));
                                out.surfaces.push(Surface {
                                    id: id.clone(),
                                    geometry,
                                    source_object: None,
                                });
                                Some(id)
                            })
                            .collect::<Vec<_>>()
                            .try_into()
                            .expect("two fixed support sides");
                        let pcurves = embedded.pcurves.map(|pcurve| {
                            Some(PcurveGeometry::Nurbs {
                                degree: pcurve.degree,
                                knots: pcurve.knots,
                                control_points: pcurve.control_points,
                                weights: pcurve.weights,
                                periodic: pcurve.periodic,
                            })
                        });
                        cadmpeg_ir::geometry::ProceduralCurveDefinition::SurfaceCurve {
                            family,
                            context: cadmpeg_ir::geometry::IntcurveSupportContext {
                                sides: std::array::from_fn(|side| {
                                    cadmpeg_ir::geometry::IntcurveSupportSide {
                                        surface: surfaces[side].clone(),
                                        pcurve: pcurves[side].clone(),
                                        pcurve_parameter_range: None,
                                    }
                                }),
                                parameter_range: embedded.parameter_range,
                                discontinuities: embedded.discontinuities,
                            },
                        }
                    } else if let Some(embedded) = procedural.9 {
                        let support_ids: [Option<SurfaceId>; 2] = embedded
                            .context
                            .surfaces
                            .into_iter()
                            .enumerate()
                            .map(|(side, geometry)| {
                                let id = SurfaceId(format!(
                                    "f3d:brep:procedural_curve#{i}:support{side}"
                                ));
                                out.surfaces.push(Surface {
                                    id: id.clone(),
                                    geometry,
                                    source_object: None,
                                });
                                Some(id)
                            })
                            .collect::<Vec<_>>()
                            .try_into()
                            .expect("two fixed support sides");
                        let pcurves = embedded.context.pcurves.map(|pcurve| {
                            Some(PcurveGeometry::Nurbs {
                                degree: pcurve.degree,
                                knots: pcurve.knots,
                                control_points: pcurve.control_points,
                                weights: pcurve.weights,
                                periodic: pcurve.periodic,
                            })
                        });
                        let cast_surface =
                            SurfaceId(format!("f3d:brep:procedural_curve#{i}:cast_surface"));
                        out.surfaces.push(Surface {
                            id: cast_surface.clone(),
                            geometry: embedded.cast_surface,
                            source_object: None,
                        });
                        cadmpeg_ir::geometry::ProceduralCurveDefinition::Silhouette {
                            context: cadmpeg_ir::geometry::IntcurveSupportContext {
                                sides: std::array::from_fn(|side| {
                                    cadmpeg_ir::geometry::IntcurveSupportSide {
                                        surface: support_ids[side].clone(),
                                        pcurve: pcurves[side].clone(),
                                        pcurve_parameter_range: None,
                                    }
                                }),
                                parameter_range: embedded.context.parameter_range,
                                discontinuities: embedded.context.discontinuities,
                            },
                            silhouette: embedded.silhouette,
                            cast_surface,
                            light_direction: embedded.light_direction,
                        }
                    } else if let Some(embedded) = procedural.10 {
                        let support_ids: [Option<SurfaceId>; 2] = embedded
                            .context
                            .surfaces
                            .into_iter()
                            .enumerate()
                            .map(|(side, geometry)| {
                                let id = SurfaceId(format!(
                                    "f3d:brep:procedural_curve#{i}:support{side}"
                                ));
                                out.surfaces.push(Surface {
                                    id: id.clone(),
                                    geometry,
                                    source_object: None,
                                });
                                Some(id)
                            })
                            .collect::<Vec<_>>()
                            .try_into()
                            .expect("two fixed support sides");
                        let pcurves = embedded.context.pcurves.map(|pcurve| {
                            Some(PcurveGeometry::Nurbs {
                                degree: pcurve.degree,
                                knots: pcurve.knots,
                                control_points: pcurve.control_points,
                                weights: pcurve.weights,
                                periodic: pcurve.periodic,
                            })
                        });
                        let base = CurveId(format!("f3d:brep:procedural_curve#{i}:base"));
                        out.curves.push(Curve {
                            id: base.clone(),
                            geometry: CurveGeometry::Nurbs(embedded.base),
                            source_object: None,
                        });
                        cadmpeg_ir::geometry::ProceduralCurveDefinition::SurfaceOffset {
                            context: cadmpeg_ir::geometry::IntcurveSupportContext {
                                sides: std::array::from_fn(|side| {
                                    cadmpeg_ir::geometry::IntcurveSupportSide {
                                        surface: support_ids[side].clone(),
                                        pcurve: pcurves[side].clone(),
                                        pcurve_parameter_range: None,
                                    }
                                }),
                                parameter_range: embedded.context.parameter_range,
                                discontinuities: embedded.context.discontinuities,
                            },
                            discontinuity_flag: embedded.discontinuity_flag,
                            base_u_range: embedded.base_u_range,
                            base_v_range: embedded.base_v_range,
                            base,
                            base_range: embedded.base_range,
                            distance: embedded.distance,
                            shift: embedded.shift,
                            scale: embedded.scale,
                        }
                    } else if let Some(embedded) = procedural.11 {
                        let support_ids: [Option<SurfaceId>; 2] = embedded
                            .surfaces
                            .into_iter()
                            .enumerate()
                            .map(|(side, geometry)| {
                                geometry.map(|geometry| {
                                    let id = SurfaceId(format!(
                                        "f3d:brep:procedural_curve#{i}:support{side}"
                                    ));
                                    out.surfaces.push(Surface {
                                        id: id.clone(),
                                        geometry,
                                        source_object: None,
                                    });
                                    id
                                })
                            })
                            .collect::<Vec<_>>()
                            .try_into()
                            .expect("two fixed support sides");
                        let pcurves = embedded.pcurves.map(|pcurve| {
                            pcurve.map(|pcurve| PcurveGeometry::Nurbs {
                                degree: pcurve.degree,
                                knots: pcurve.knots,
                                control_points: pcurve.control_points,
                                weights: pcurve.weights,
                                periodic: pcurve.periodic,
                            })
                        });
                        cadmpeg_ir::geometry::ProceduralCurveDefinition::Spring {
                            context: cadmpeg_ir::geometry::IntcurveSupportContext {
                                sides: std::array::from_fn(|side| {
                                    cadmpeg_ir::geometry::IntcurveSupportSide {
                                        surface: support_ids[side].clone(),
                                        pcurve: pcurves[side].clone(),
                                        pcurve_parameter_range: None,
                                    }
                                }),
                                parameter_range: embedded.parameter_range,
                                discontinuities: embedded.discontinuities,
                            },
                            surface_parameter_ranges: embedded.surface_parameter_ranges,
                            first_pcurve_parameter_range: embedded.first_pcurve_parameter_range,
                            discontinuity_flag: embedded.discontinuity_flag,
                            direction: embedded.direction,
                        }
                    } else if let Some(embedded) = procedural.12 {
                        let bend = CurveId(format!("f3d:brep:procedural_curve#{i}:bend"));
                        out.curves.push(Curve {
                            id: bend.clone(),
                            geometry: CurveGeometry::Nurbs(embedded.bend),
                            source_object: None,
                        });
                        let data = match embedded.data {
                            nurbs::EmbeddedDeformableData::VectorField {
                                vectors,
                                parameter_pairs,
                            } => cadmpeg_ir::geometry::DeformableCurveData::VectorField {
                                vectors,
                                parameter_pairs,
                            },
                            nurbs::EmbeddedDeformableData::Surface(geometry) => {
                                let surface = SurfaceId(format!(
                                    "f3d:brep:procedural_curve#{i}:deformation_surface"
                                ));
                                out.surfaces.push(Surface {
                                    id: surface.clone(),
                                    geometry,
                                    source_object: None,
                                });
                                cadmpeg_ir::geometry::DeformableCurveData::Surface { surface }
                            }
                        };
                        cadmpeg_ir::geometry::ProceduralCurveDefinition::Deformable {
                            extension: embedded.extension,
                            bend,
                            data,
                        }
                    } else if let Some(embedded) = procedural.13 {
                        let surfaces: [Option<SurfaceId>; 2] = embedded
                            .surfaces
                            .into_iter()
                            .enumerate()
                            .map(|(side, geometry)| {
                                let id = SurfaceId(format!(
                                    "f3d:brep:procedural_curve#{i}:support{side}"
                                ));
                                out.surfaces.push(Surface {
                                    id: id.clone(),
                                    geometry,
                                    source_object: None,
                                });
                                Some(id)
                            })
                            .collect::<Vec<_>>()
                            .try_into()
                            .expect("two fixed support sides");
                        let pcurves = embedded.pcurves.map(|pcurve| {
                            Some(PcurveGeometry::Nurbs {
                                degree: pcurve.degree,
                                knots: pcurve.knots,
                                control_points: pcurve.control_points,
                                weights: pcurve.weights,
                                periodic: pcurve.periodic,
                            })
                        });
                        let source = CurveId(format!("f3d:brep:procedural_curve#{i}:source"));
                        out.curves.push(Curve {
                            id: source.clone(),
                            geometry: CurveGeometry::Nurbs(embedded.source),
                            source_object: None,
                        });
                        cadmpeg_ir::geometry::ProceduralCurveDefinition::Projection {
                            context: cadmpeg_ir::geometry::IntcurveSupportContext {
                                sides: std::array::from_fn(|side| {
                                    cadmpeg_ir::geometry::IntcurveSupportSide {
                                        surface: surfaces[side].clone(),
                                        pcurve: pcurves[side].clone(),
                                        pcurve_parameter_range: None,
                                    }
                                }),
                                parameter_range: embedded.parameter_range,
                                discontinuities: embedded.discontinuities,
                            },
                            discontinuity_flag: embedded.discontinuity_flag,
                            source,
                            tail: embedded.tail,
                        }
                    } else if let Some(embedded) = procedural.14 {
                        fn map_law_curve(
                            out: &mut Brep,
                            owner: i64,
                            path: &str,
                            expression: nurbs::EmbeddedLawExpression,
                        ) -> cadmpeg_ir::geometry::LawExpression {
                            match expression {
                                nurbs::EmbeddedLawExpression::Null => {
                                    cadmpeg_ir::geometry::LawExpression::Null
                                }
                                nurbs::EmbeddedLawExpression::Integer(value) => {
                                    cadmpeg_ir::geometry::LawExpression::Integer { value }
                                }
                                nurbs::EmbeddedLawExpression::Double(value) => {
                                    cadmpeg_ir::geometry::LawExpression::Double { value }
                                }
                                nurbs::EmbeddedLawExpression::Point(value) => {
                                    cadmpeg_ir::geometry::LawExpression::Point { value }
                                }
                                nurbs::EmbeddedLawExpression::Vector(value) => {
                                    cadmpeg_ir::geometry::LawExpression::Vector { value }
                                }
                                nurbs::EmbeddedLawExpression::Transform { scalars, enums } => {
                                    cadmpeg_ir::geometry::LawExpression::Transform {
                                        scalars,
                                        enums,
                                    }
                                }
                                nurbs::EmbeddedLawExpression::Edge { curve, parameters } => {
                                    let id = CurveId(format!(
                                        "f3d:brep:procedural_curve#{owner}:law:{path}"
                                    ));
                                    out.curves.push(Curve {
                                        id: id.clone(),
                                        geometry: CurveGeometry::Nurbs(curve),
                                        source_object: None,
                                    });
                                    cadmpeg_ir::geometry::LawExpression::Edge {
                                        curve: id,
                                        parameters,
                                    }
                                }
                                nurbs::EmbeddedLawExpression::Spline {
                                    native_id,
                                    knots,
                                    controls,
                                    point,
                                } => cadmpeg_ir::geometry::LawExpression::Spline {
                                    native_id,
                                    knots,
                                    controls,
                                    point,
                                },
                                nurbs::EmbeddedLawExpression::Algebraic { operator, operands } => {
                                    cadmpeg_ir::geometry::LawExpression::Algebraic {
                                        operator,
                                        operands: operands
                                            .into_iter()
                                            .enumerate()
                                            .map(|(index, operand)| {
                                                map_law_curve(
                                                    out,
                                                    owner,
                                                    &format!("{path}:{index}"),
                                                    operand,
                                                )
                                            })
                                            .collect(),
                                    }
                                }
                            }
                        }
                        let surfaces: [Option<SurfaceId>; 2] = embedded
                            .context
                            .surfaces
                            .into_iter()
                            .enumerate()
                            .map(|(side, geometry)| {
                                let id = SurfaceId(format!(
                                    "f3d:brep:procedural_curve#{i}:support{side}"
                                ));
                                out.surfaces.push(Surface {
                                    id: id.clone(),
                                    geometry,
                                    source_object: None,
                                });
                                Some(id)
                            })
                            .collect::<Vec<_>>()
                            .try_into()
                            .expect("two fixed support sides");
                        let pcurves = embedded.context.pcurves.map(|pcurve| {
                            Some(PcurveGeometry::Nurbs {
                                degree: pcurve.degree,
                                knots: pcurve.knots,
                                control_points: pcurve.control_points,
                                weights: pcurve.weights,
                                periodic: pcurve.periodic,
                            })
                        });
                        let mut map_formula = |path: &str, formula: nurbs::EmbeddedLawFormula| {
                            cadmpeg_ir::geometry::LawFormula {
                                name: formula.name,
                                variables: formula
                                    .variables
                                    .into_iter()
                                    .enumerate()
                                    .map(|(index, expression)| {
                                        map_law_curve(
                                            &mut out,
                                            i,
                                            &format!("{path}:{index}"),
                                            expression,
                                        )
                                    })
                                    .collect(),
                            }
                        };
                        cadmpeg_ir::geometry::ProceduralCurveDefinition::Law {
                            context: cadmpeg_ir::geometry::IntcurveSupportContext {
                                sides: std::array::from_fn(|side| {
                                    cadmpeg_ir::geometry::IntcurveSupportSide {
                                        surface: surfaces[side].clone(),
                                        pcurve: pcurves[side].clone(),
                                        pcurve_parameter_range: None,
                                    }
                                }),
                                parameter_range: embedded.context.parameter_range,
                                discontinuities: embedded.context.discontinuities,
                            },
                            extension: embedded.extension,
                            primary: map_formula("primary", embedded.primary),
                            additional: embedded
                                .additional
                                .into_iter()
                                .enumerate()
                                .map(|(index, formula)| {
                                    map_formula(&format!("additional:{index}"), formula)
                                })
                                .collect(),
                        }
                    } else if let Some((parameters, component_parameters, components)) =
                        procedural.4
                    {
                        let components = components
                            .into_iter()
                            .enumerate()
                            .map(|(component, curve)| {
                                let id = CurveId(format!(
                                    "f3d:brep:procedural_curve#{i}:component#{component}"
                                ));
                                out.curves.push(Curve {
                                    id: id.clone(),
                                    geometry: CurveGeometry::Nurbs(curve),
                                    source_object: None,
                                });
                                id
                            })
                            .collect();
                        cadmpeg_ir::geometry::ProceduralCurveDefinition::Compound {
                            parameters,
                            component_parameters,
                            components,
                        }
                    } else {
                        procedural.1.unwrap_or(
                            cadmpeg_ir::geometry::ProceduralCurveDefinition::Unknown {
                                record: None,
                            },
                        )
                    };
                    out.procedural_curves.push(ProceduralCurve {
                        id: format!("f3d:brep:procedural_curve#{i}").into(),
                        curve: CurveId(id(i)),
                        definition,
                        cache_fit_tolerance: procedural.15,
                    });
                }
            }
            _ => {}
        }
    }

    for r in records {
        let i = r.index as i64;
        if kept_pcurves.contains(&i) {
            if let Some(geometry) = pcurve_geo.remove(&i) {
                out.pcurves.push(Pcurve {
                    id: PcurveId(id(i)),
                    geometry,
                    wrapper_reversed: match r.chunk(4) {
                        Some(Token::True) if matches!(r.chunk(3), Some(Token::Long(0))) => {
                            Some(true)
                        }
                        Some(Token::False) if matches!(r.chunk(3), Some(Token::Long(0))) => {
                            Some(false)
                        }
                        _ => None,
                    },
                    native_tail_flags: pcurve_inline_tail_flags(r),
                    parameter_range: pcurve_parameter_range(r),
                    fit_tolerance: match (r.chunk(3), r.chunk(4)) {
                        (Some(Token::Long(0)), Some(Token::True | Token::False)) => {
                            crate::sab::payload_subtype_span(bytes, r, 5, ref_width, "exp_par_cur")
                                .and_then(nurbs::decode_pcurve_fit_tolerance)
                        }
                        _ => None,
                    },
                });
            }
        }
    }
    for r in records {
        let i = r.index as i64;
        if r.head == "point" && kept_points.contains(&i) {
            let c = collect_carrier(r);
            if let Some(p) = c.positions.first() {
                out.points.push(Point {
                    id: PointId(id(i)),
                    position: scale_point(*p),
                    source_object: None,
                });
            }
        }
    }

    for r in records {
        let i = r.index as i64;
        if is_vertex_record(r) && kept_vertices.contains(&i) {
            if let Some(pi) = r.ref_at(5) {
                if kept_points.contains(&pi) {
                    out.vertices.push(Vertex {
                        id: VertexId(id(i)),
                        point: PointId(id(pi)),
                        tolerance: matches!(r.head.as_str(), "tvertex")
                            .then(|| match r.chunk(6) {
                                Some(Token::Double(value)) => Some(*value * LEN_TO_MM),
                                _ => None,
                            })
                            .flatten(),
                    });
                    if r.head == "tvertex" {
                        if let (Some(Token::Float(first)), Some(Token::Float(second))) =
                            (r.chunk(7), r.chunk(8))
                        {
                            out.tolerant_vertex_tails.push(TolerantVertexTail {
                                id: format!("f3d:asm:tolerant-vertex-tail#{i}"),
                                vertex: VertexId(id(i)),
                                record_index: r.index as u32,
                                trailing_floats: [*first, *second],
                            });
                        }
                    }
                    if let (Some(owning_edge), Some(Token::Long(endpoint_index @ 0..=1))) =
                        (r.ref_at(3), r.chunk(4))
                    {
                        out.vertex_ownerships.push(VertexOwnership {
                            id: format!("f3d:asm:vertex-ownership#{i}"),
                            vertex: VertexId(id(i)),
                            record_index: r.index as u32,
                            owning_edge: EdgeId(id(owning_edge)),
                            endpoint_index: *endpoint_index as u8,
                        });
                    }
                }
            }
        }
    }

    for r in records {
        let i = r.index as i64;
        if is_edge_record(r) && kept_edges.contains(&i) {
            let (Some(start), Some(end)) = (r.ref_at(3), r.ref_at(5)) else {
                continue;
            };
            if !kept_vertices.contains(&start) || !kept_vertices.contains(&end) {
                continue;
            }
            let curve = r.ref_at(8).filter(|c| kept_curves.contains(c));
            let param_range = match (double_at(r, 4), double_at(r, 6)) {
                (Some(mut a), Some(mut b)) => {
                    if let Some(curve_record) = curve.and_then(|curve| by_index.get(&curve)) {
                        if curve_record.head == "ellipse" {
                            // Native conic parameters are angles from the
                            // major axis, matching the IR carrier's own
                            // parameterization directly. Wrap the arc start
                            // into the canonical `[0, τ)` domain, preserving
                            // the sweep; a full period keeps its start phase
                            // so the range still anchors on the edge's
                            // vertices.
                            let sweep = b - a;
                            a = a.rem_euclid(std::f64::consts::TAU);
                            if std::f64::consts::TAU - a < 1.0e-9 {
                                a = 0.0;
                            }
                            b = a + sweep;
                        } else if curve_record.head == "straight" {
                            // Native line parameters are multiples of the
                            // stored direction vector, whose length is the
                            // parameter scale; the IR carrier's unit direction
                            // lives in millimeter space.
                            let scale = collect_carrier(curve_record)
                                .vectors
                                .first()
                                .map_or(1.0, |vector| norm3(*vector));
                            a *= scale * LEN_TO_MM;
                            b *= scale * LEN_TO_MM;
                        }
                    }
                    Some([a, b])
                }
                _ => None,
            };
            // A reversed edge's raw parameters already live on the reversed
            // parameterization its (reversed) carrier now exposes, so the
            // range transforms identically for both senses; only the carrier
            // link differs when the curve is shared across senses.
            let curve = curve.map(|c| match sense_at(r, 9) {
                Sense::Reversed => reversed_curve_id(c),
                Sense::Forward => CurveId(id(c)),
            });
            out.edges.push(Edge {
                id: EdgeId(id(i)),
                curve,
                start: VertexId(id(start)),
                end: VertexId(id(end)),
                param_range,
                tolerance: None,
            });
            out.edge_ownerships.push(EdgeOwnership {
                id: format!("f3d:asm:edge-ownership#{i}"),
                edge: EdgeId(id(i)),
                record_index: r.index as u32,
                owner_coedge: r.ref_at(7).map(|owner| CoedgeId(id(owner))),
            });
            if let Some(Token::Str(continuity)) = r.chunk(10) {
                out.edge_continuities.push(EdgeContinuity {
                    id: format!("f3d:asm:edge-continuity#{i}"),
                    edge: EdgeId(id(i)),
                    record_index: r.index as u32,
                    sense: sense_at(r, 9),
                    continuity: continuity.clone(),
                });
            }
        }
    }

    for r in records {
        let i = r.index as i64;
        if is_coedge_record(r) && kept_coedges.contains(&i) {
            let (Some(next), Some(prev), Some(edge), Some(owner)) =
                (r.ref_at(3), r.ref_at(4), r.ref_at(6), r.ref_at(8))
            else {
                continue;
            };
            if !kept_coedges.contains(&next)
                || !kept_coedges.contains(&prev)
                || !kept_edges.contains(&edge)
                || !kept_loops.contains(&owner)
            {
                continue;
            }
            let partner = r.ref_at(5).filter(|p| kept_coedges.contains(p));
            out.coedges.push(Coedge {
                id: CoedgeId(id(i)),
                owner_loop: LoopId(id(owner)),
                edge: EdgeId(id(edge)),
                next: CoedgeId(id(next)),
                previous: CoedgeId(id(prev)),
                radial_next: partner.map_or_else(|| CoedgeId(id(i)), |p| CoedgeId(id(p))),
                sense: sense_at(r, 7),
                pcurves: r
                    .ref_at(10)
                    .filter(|p| kept_pcurves.contains(p))
                    .map(|p| cadmpeg_ir::topology::PcurveUse {
                        pcurve: PcurveId(id(p)),
                        isoparametric: None,
                    })
                    .into_iter()
                    .collect(),
            });
            if r.head == "tcoedge" {
                if let (Some(Token::Double(start)), Some(Token::Double(end))) =
                    (r.chunk(11), r.chunk(12))
                {
                    out.tolerant_coedge_parameters
                        .push(TolerantCoedgeParameters {
                            id: format!("f3d:asm:tolerant-coedge-parameters#{i}"),
                            coedge: CoedgeId(id(i)),
                            record_index: r.index as u32,
                            parameter_range: [*start, *end],
                        });
                }
            }
        }
    }

    for r in records {
        let i = r.index as i64;
        if r.head == "loop" && kept_loops.contains(&i) {
            let Some(owner) = r.ref_at(5) else { continue };
            let coedges = ring_coedges(r, &by_index, &kept_coedges);
            out.loops.push(Loop {
                id: LoopId(id(i)),
                face: FaceId(id(owner)),
                boundary_role: cadmpeg_ir::topology::LoopBoundaryRole::Unspecified,
                coedges,
                vertex_uses: Vec::new(),
            });
        }
    }

    for r in records {
        let i = r.index as i64;
        if r.head == "face" && kept_faces.contains(&i) {
            let (Some(surface), Some(owner)) = (r.ref_at(7), r.ref_at(5)) else {
                continue;
            };
            let loops = loop_chain(r, &by_index, &kept_loops);
            // The face record's sense is relative to its surface record's
            // orientation. A reversed spline record flips the cache normal,
            // and a negative-cosine cone points its normal toward the axis;
            // the IR stores the forward carrier in both cases, so the
            // reversal folds into the face sense to keep the IR
            // self-consistent.
            let native_sense = sense_at(r, 8);
            let mut sense = native_sense;
            if by_index
                .get(&surface)
                .is_some_and(|surf| surf.head == "spline" && record_reversed(surf))
                ^ inward_normal_surfaces.contains(&surface)
            {
                sense = match sense {
                    Sense::Forward => Sense::Reversed,
                    Sense::Reversed => Sense::Forward,
                };
            }
            out.faces.push(Face {
                id: FaceId(id(i)),
                shell: ShellId(id(owner)),
                surface: SurfaceId(id(surface)),
                sense,
                loops,
                name: attribute_name(r),
                color: attribute_color(r),
                tolerance: None,
            });
            let containment = match (r.chunk(9), r.chunk(10)) {
                (Some(Token::True), Some(Token::True)) => Some(FaceContainment::In),
                (Some(Token::True), Some(Token::False)) => Some(FaceContainment::Out),
                _ => None,
            };
            out.face_sidedness.push(FaceSidedness {
                id: format!("f3d:asm:face-sidedness#{i}"),
                face: FaceId(id(i)),
                record_index: r.index as u32,
                native_sense,
                normalized_sense: sense,
                containment,
            });
        }
    }

    // Containers: emitted for every record so back-references resolve, with
    // child lists filtered to reachable entities.
    for r in records {
        let i = r.index as i64;
        match r.head.as_str() {
            "shell" => {
                let Some(owner) = r.ref_at(7) else { continue };
                let faces = shell_faces(r, &by_index, &kept_faces);
                out.shells.push(Shell {
                    id: ShellId(id(i)),
                    region: RegionId(id(owner)),
                    faces,
                    wire_edges: wire_edges_by_shell
                        .get(&i)
                        .into_iter()
                        .flatten()
                        .map(|edge| EdgeId(id(*edge)))
                        .collect(),
                    free_vertices: Vec::new(),
                });
            }
            // ASM release 231 names this record `region`; release 227 streams
            // carry the original ACIS head `lump`. Same layout in both.
            "region" | "lump" => {
                let Some(owner) = r.ref_at(5) else { continue };
                let shells = shell_chain(r, &by_index);
                out.regions.push(Region {
                    id: RegionId(id(i)),
                    body: BodyId(id(owner)),
                    shells,
                });
            }
            "body" => {
                let regions = region_chain(r, &by_index);
                let body_id = BodyId(id(i));
                if let Some(Token::Long(key)) = r.chunk(1) {
                    out.body_native_keys.push(BodyNativeKey {
                        id: format!("f3d:asm:body-native-key#{i}"),
                        body: body_id.clone(),
                        record_index: r.index as u32,
                        asm_body_key: (*key >= 0).then_some(*key as u64),
                    });
                    if *key >= 0 {
                        out.body_keys.insert(body_id.clone(), *key as u64);
                    }
                }
                let transform_record = r.ref_at(5).and_then(|reference| by_index.get(&reference));
                if let Some(transform) = transform_record {
                    let flags = transform
                        .tokens
                        .iter()
                        .filter_map(|token| match token {
                            Token::True => Some(true),
                            Token::False => Some(false),
                            _ => None,
                        })
                        .collect::<Vec<_>>();
                    if let [rotation, reflection, shear] = flags.as_slice() {
                        out.transform_hints.push(TransformHints {
                            id: format!("f3d:asm:transform-hints#{}", transform.index),
                            body: body_id.clone(),
                            record_index: transform.index as u32,
                            rotation: *rotation,
                            reflection: *reflection,
                            shear: *shear,
                        });
                    }
                }
                out.bodies.push(Body {
                    id: body_id,
                    kind: cadmpeg_ir::topology::BodyKind::Solid,
                    regions,
                    transform: transform_record
                        .and_then(|transform| decode_transform(transform, header_scale)),
                    name: attribute_name(r),
                    color: attribute_color(r),
                    visible: None,
                });
            }
            _ => {}
        }
    }

    // A face owned by a subshell is projected onto its nearest shell ancestor;
    // the neutral IR deliberately has no subshell arena. This keeps the exact
    // native ownership graph in the retained source while making every face
    // reachable through the normalized shell.
    let subshell_shells = subshell_ancestor_shells(records, &by_index);
    for face in &mut out.faces {
        let native_owner = face
            .id
            .0
            .rsplit_once('#')
            .and_then(|(_, index)| index.parse::<i64>().ok())
            .and_then(|index| by_index.get(&index))
            .and_then(|record| record.ref_at(5));
        if let Some(shell) = native_owner.and_then(|owner| subshell_shells.get(&owner)) {
            face.shell = ShellId(id(*shell));
        }
    }

    let mut emitted_attributes = HashSet::new();
    for record in records {
        let index = record.index as i64;
        let target = match record.head.as_str() {
            "body" if out.bodies.iter().any(|entity| entity.id.0 == id(index)) => {
                Some(AttributeTarget::Body(BodyId(id(index))))
            }
            "face" if kept_faces.contains(&index) => Some(AttributeTarget::Face(FaceId(id(index)))),
            "coedge" | "tcoedge" if kept_coedges.contains(&index) => {
                Some(AttributeTarget::Coedge(CoedgeId(id(index))))
            }
            "edge" | "tedge" if kept_edges.contains(&index) => {
                Some(AttributeTarget::Edge(EdgeId(id(index))))
            }
            "vertex" if kept_vertices.contains(&index) => {
                Some(AttributeTarget::Vertex(VertexId(id(index))))
            }
            _ => None,
        };
        if let Some(target) = target {
            collect_attributes(
                record,
                &target,
                &by_index,
                &mut emitted_attributes,
                &mut out.attributes,
            );
        }
    }

    for record in records {
        let index = record.index as i64;
        if record.name != "ATTRIB_CUSTOM-attrib" || emitted_attributes.contains(&index) {
            continue;
        }
        let Some(owner) = record.ref_at(4).and_then(|owner| by_index.get(&owner)) else {
            continue;
        };
        let owner_index = owner.index as i64;
        let target = match owner.head.as_str() {
            "body" => Some(AttributeTarget::Body(BodyId(id(owner_index)))),
            "face" if kept_faces.contains(&owner_index) => {
                Some(AttributeTarget::Face(FaceId(id(owner_index))))
            }
            "coedge" | "tcoedge" if kept_coedges.contains(&owner_index) => {
                Some(AttributeTarget::Coedge(CoedgeId(id(owner_index))))
            }
            "edge" | "tedge" if kept_edges.contains(&owner_index) => {
                Some(AttributeTarget::Edge(EdgeId(id(owner_index))))
            }
            "vertex" if kept_vertices.contains(&owner_index) => {
                Some(AttributeTarget::Vertex(VertexId(id(owner_index))))
            }
            _ => None,
        };
        if let Some(target) = target {
            emitted_attributes.insert(index);
            out.attributes.push(source_attribute(record, target));
        }
    }
    out.sketch_curve_links = out
        .attributes
        .iter()
        .filter_map(sketch_curve_link)
        .collect();
    out.persistent_design_links = out
        .attributes
        .iter()
        .flat_map(persistent_design_links)
        .collect();
    out.creation_timestamps = out
        .attributes
        .iter()
        .filter_map(creation_timestamp)
        .collect();

    // Preserve undecoded carriers referenced by real topology as passthrough.
    for r in records {
        let i = r.index as i64;
        if undecoded_carriers.contains(&i) {
            out.unknowns.push(UnknownRecord {
                id: UnknownId(unknown_record_id(r)),
                offset: r.offset as u64,
                byte_len: r.len as u64,
                sha256: sha256_hex(&bytes[r.offset..(r.offset + r.len).min(bytes.len())]),
                data: Some(bytes[r.offset..(r.offset + r.len).min(bytes.len())].to_vec()),
                links: Vec::new(),
            });
        }
    }

    // Count remaining record kinds we neither emitted nor preserved.
    let kept_transforms: HashSet<i64> = records
        .iter()
        .filter(|record| record.head == "body")
        .filter_map(|record| record.ref_at(5))
        .collect();
    let pcurve_intcurves: HashSet<i64> = records
        .iter()
        .filter(|record| kept_pcurves.contains(&(record.index as i64)))
        .filter_map(|record| record.ref_at(4))
        .collect();
    let known_head = |h: &str| {
        matches!(
            h,
            "body"
                | "region"
                | "lump"
                | "shell"
                | "face"
                | "loop"
                | "coedge"
                | "edge"
                | "vertex"
                | "point"
        ) || is_analytic_surface(h)
            || is_analytic_curve(h)
            || h == "asmheader"
    };
    for r in records {
        let i = r.index as i64;
        // Spline/intcurve records that decoded into a NURBS carrier are counted
        // as transferred, not as opaque leftovers.
        let transferred = kept_surfaces.contains(&i)
            || kept_curves.contains(&i)
            || kept_pcurves.contains(&i)
            || kept_transforms.contains(&i)
            || emitted_attributes.contains(&i)
            || pcurve_intcurves.contains(&i);
        if !known_head(&r.head)
            && r.name != "Begin-of-ASM-History-Data"
            && !undecoded_carriers.contains(&i)
            && !transferred
        {
            out.stats.other_records += 1;
            *out.stats
                .other_record_kinds
                .entry(r.name.clone())
                .or_default() += 1;
        }
    }

    let emitted_ids = out
        .bodies
        .iter()
        .map(|entity| entity.id.0.as_str())
        .chain(out.regions.iter().map(|entity| entity.id.0.as_str()))
        .chain(out.shells.iter().map(|entity| entity.id.0.as_str()))
        .chain(out.faces.iter().map(|entity| entity.id.0.as_str()))
        .chain(out.loops.iter().map(|entity| entity.id.0.as_str()))
        .chain(out.coedges.iter().map(|entity| entity.id.0.as_str()))
        .chain(out.edges.iter().map(|entity| entity.id.0.as_str()))
        .chain(out.vertices.iter().map(|entity| entity.id.0.as_str()))
        .chain(out.points.iter().map(|entity| entity.id.0.as_str()))
        .chain(out.surfaces.iter().map(|entity| entity.id.0.as_str()))
        .chain(out.curves.iter().map(|entity| entity.id.0.as_str()))
        .chain(out.pcurves.iter().map(|entity| entity.id.0.as_str()))
        .collect::<HashSet<_>>();
    for record in records {
        let entity_id = id(record.index as i64);
        if emitted_ids.contains(entity_id.as_str()) {
            let mut derived_fields = Vec::new();
            match record.head.as_str() {
                "plane" => {
                    derived_fields.extend(["geometry.normal", "geometry.u_axis"]);
                }
                "cone" => {
                    derived_fields.extend(["geometry.axis", "geometry.ref_direction"]);
                }
                "sphere" => {
                    derived_fields.extend(["geometry.axis", "geometry.ref_direction"]);
                }
                "torus" => {
                    derived_fields.extend(["geometry.axis", "geometry.ref_direction"]);
                }
                "straight" => derived_fields.push("geometry.direction"),
                "ellipse" => {
                    derived_fields.extend(["geometry.axis", "geometry.major_direction"]);
                }
                _ => {}
            }
            if is_edge_record(record) {
                if let Some(curve) = record
                    .ref_at(8)
                    .and_then(|reference| by_index.get(&reference))
                {
                    if curve.head == "ellipse" {
                        derived_fields.push("param_range");
                    }
                }
            }
            out.annotation_records.push(AnnotationRecord {
                id: entity_id,
                offset: record.offset as u64,
                tag: record.name.clone(),
                derived_fields,
            });
        }
        let attribute_id = format!("f3d:brep:attribute#{}", record.index);
        if out
            .attributes
            .iter()
            .any(|attribute| attribute.id.0 == attribute_id)
        {
            out.annotation_records.push(AnnotationRecord {
                id: attribute_id,
                offset: record.offset as u64,
                tag: record.name.clone(),
                derived_fields: Vec::new(),
            });
        }
        let unknown_id = unknown_record_id(record);
        if out
            .unknowns
            .iter()
            .any(|unknown| unknown.id.0 == unknown_id)
        {
            out.annotation_records.push(AnnotationRecord {
                id: unknown_id,
                offset: record.offset as u64,
                tag: record.name.clone(),
                derived_fields: Vec::new(),
            });
        }
        for (synthetic_id, tag) in [
            (
                format!("f3d:brep:procedural_surface#{}", record.index),
                "procedural_surface",
            ),
            (
                format!("f3d:brep:procedural_curve#{}", record.index),
                "procedural_curve",
            ),
        ] {
            if out
                .procedural_surfaces
                .iter()
                .any(|entity| entity.id.0 == synthetic_id)
                || out
                    .procedural_curves
                    .iter()
                    .any(|entity| entity.id.0 == synthetic_id)
            {
                out.annotation_records.push(AnnotationRecord {
                    id: synthetic_id,
                    offset: record.offset as u64,
                    tag: tag.into(),
                    derived_fields: Vec::new(),
                });
            }
        }
    }
    for (entity_id, tag) in out
        .surfaces
        .iter()
        .map(|entity| (entity.id.0.as_str(), "procedural_support"))
        .chain(
            out.curves
                .iter()
                .map(|entity| (entity.id.0.as_str(), "procedural_curve_child")),
        )
    {
        if !entity_id.starts_with("f3d:brep:procedural_surface#") {
            continue;
        }
        let Some(index) = entity_id
            .split_once('#')
            .and_then(|(_, suffix)| suffix.split(':').next())
            .and_then(|value| value.parse::<usize>().ok())
        else {
            continue;
        };
        let Some(record) = records.get(index) else {
            continue;
        };
        out.annotation_records.push(AnnotationRecord {
            id: entity_id.to_owned(),
            offset: record.offset as u64,
            tag: tag.into(),
            derived_fields: Vec::new(),
        });
    }

    classify_body_kinds(&mut out);
    clamp_edge_ranges_to_carrier_domains(&mut out);

    out
}

/// Snap edge parameter ranges that overshoot their B-spline carrier's knot
/// domain by floating-point noise back onto the domain boundary. Native edge
/// ranges and cache knot vectors are stored independently and can disagree in
/// their last few bits; a genuine domain violation is left for validation.
fn clamp_edge_ranges_to_carrier_domains(out: &mut Brep) {
    let domains: HashMap<&str, [f64; 2]> = out
        .curves
        .iter()
        .filter_map(|curve| match &curve.geometry {
            CurveGeometry::Nurbs(nurbs) => {
                let (first, last) = (nurbs.knots.first()?, nurbs.knots.last()?);
                Some((curve.id.0.as_str(), [*first, *last]))
            }
            _ => None,
        })
        .collect();
    for edge in &mut out.edges {
        let Some([start, end]) = edge.param_range.as_mut() else {
            continue;
        };
        let Some([first, last]) = edge
            .curve
            .as_ref()
            .and_then(|curve| domains.get(curve.0.as_str()))
        else {
            continue;
        };
        let tolerance = 1.0e-9 * (last - first).abs().max(1.0);
        if *start < *first && *first - *start <= tolerance {
            *start = *first;
        }
        if *end > *last && *end - *last <= tolerance {
            *end = *last;
        }
    }
}

fn classify_body_kinds(out: &mut Brep) {
    let mut shell_bodies = HashMap::new();
    for region in &out.regions {
        for shell in &region.shells {
            shell_bodies.insert(shell.clone(), region.body.clone());
        }
    }
    let mut body_has_faces = HashSet::new();
    let mut body_has_wire_edges = HashSet::new();
    let mut face_bodies = HashMap::new();
    for shell in &out.shells {
        let Some(body) = shell_bodies.get(&shell.id) else {
            continue;
        };
        if !shell.wire_edges.is_empty() {
            body_has_wire_edges.insert(body.clone());
        }
        if !shell.faces.is_empty() {
            body_has_faces.insert(body.clone());
        }
        for face in &shell.faces {
            face_bodies.insert(face.clone(), body.clone());
        }
    }
    let mut loop_bodies = HashMap::new();
    for face in &out.faces {
        let Some(body) = face_bodies.get(&face.id) else {
            continue;
        };
        for loop_id in &face.loops {
            loop_bodies.insert(loop_id.clone(), body.clone());
        }
    }
    let mut coedge_bodies = HashMap::new();
    for loop_ in &out.loops {
        let Some(body) = loop_bodies.get(&loop_.id) else {
            continue;
        };
        for coedge in &loop_.coedges {
            coedge_bodies.insert(coedge.clone(), body.clone());
        }
    }
    let mut edge_use_counts = HashMap::<_, HashMap<EdgeId, usize>>::new();
    for coedge in &out.coedges {
        if let Some(body) = coedge_bodies.get(&coedge.id) {
            *edge_use_counts
                .entry(body.clone())
                .or_default()
                .entry(coedge.edge.clone())
                .or_default() += 1;
        }
    }
    for body in &mut out.bodies {
        if !body_has_faces.contains(&body.id) {
            body.kind = cadmpeg_ir::topology::BodyKind::Wire;
            continue;
        }
        if body_has_wire_edges.contains(&body.id) {
            body.kind = cadmpeg_ir::topology::BodyKind::General;
            continue;
        }
        let counts = edge_use_counts.get(&body.id);
        body.kind = if counts
            .is_some_and(|counts| !counts.is_empty() && counts.values().all(|count| *count == 2))
        {
            cadmpeg_ir::topology::BodyKind::Solid
        } else {
            cadmpeg_ir::topology::BodyKind::Sheet
        };
    }
}

fn sketch_curve_link(attribute: &SourceAttribute) -> Option<SketchCurveLink> {
    let AttributeTarget::Coedge(coedge) = &attribute.target else {
        return None;
    };
    let family = attribute.values.iter().position(
        |value| matches!(value, AttributeValue::String(name) if name == "sketch_attrib_def"),
    )?;
    let fields = attribute.values[family + 1..]
        .iter()
        .filter_map(|value| match value {
            AttributeValue::String(payload) => Some(
                payload
                    .split_ascii_whitespace()
                    .map(str::parse::<i64>)
                    .collect::<Result<Vec<_>, _>>()
                    .ok(),
            ),
            _ => None,
        })
        .flatten()
        .find(|values| values.len() == 6)
        .unwrap_or_else(|| {
            attribute.values[family + 1..]
                .iter()
                .filter_map(|value| match value {
                    AttributeValue::Integer(value) => Some(*value),
                    _ => None,
                })
                .take(6)
                .collect()
        });
    let [sketch_curve_id, 0, signed_reference, 0, role, closure] = fields.as_slice() else {
        return None;
    };
    Some(SketchCurveLink {
        id: format!("f3d:design:sketch-curve-link#{}", attribute_key(attribute)),
        coedge: coedge.clone(),
        sketch_curve_id: *sketch_curve_id,
        signed_reference: (*signed_reference != -1).then_some(*signed_reference),
        role: *role,
        closure: *closure,
    })
}

fn persistent_design_links(attribute: &SourceAttribute) -> Vec<PersistentDesignLink> {
    let Some(family) = attribute.values.iter().position(
        |value| matches!(value, AttributeValue::String(name) if name == "generic_tag_attrib_def"),
    ) else {
        return Vec::new();
    };
    let groups = attribute.values[family + 1..]
        .windows(5)
        .filter_map(|values| match values {
            [
                AttributeValue::Integer(entity_kind),
                AttributeValue::String(design_id),
                AttributeValue::Integer(design_reference),
                AttributeValue::Integer(0),
                AttributeValue::Integer(0),
            ] if !design_id.is_empty()
                && design_id.bytes().all(|byte| byte.is_ascii_digit()) =>
            {
                Some((*entity_kind, design_id.clone(), *design_reference))
            }
            _ => None,
        })
        .collect::<Vec<_>>();
    let last = groups.len().saturating_sub(1);
    groups
        .into_iter()
        .enumerate()
        .map(
            |(ordinal, (entity_kind, design_id, design_reference))| PersistentDesignLink {
                id: format!(
                    "f3d:design:persistent-design-link#{}:{ordinal}",
                    attribute_key(attribute)
                ),
                target: attribute.target.clone(),
                design_id,
                entity_kind,
                design_reference,
                ordinal: ordinal as u32,
                is_current: ordinal == last,
            },
        )
        .collect()
}

fn creation_timestamp(attribute: &SourceAttribute) -> Option<CreationTimestamp> {
    let family = attribute.values.iter().position(
        |value| matches!(value, AttributeValue::String(name) if name == "Timestamp_attrib_def"),
    )?;
    let marker = attribute.values.get(family + 1)?;
    if !matches!(marker, AttributeValue::Integer(1)) {
        return None;
    }
    let AttributeValue::Float(unix_microseconds) = attribute.values.get(family + 2)? else {
        return None;
    };
    if !unix_microseconds.is_finite() {
        return None;
    }
    Some(CreationTimestamp {
        id: format!("f3d:design:creation-timestamp#{}", attribute_key(attribute)),
        target: attribute.target.clone(),
        record_index: attribute_key(attribute).parse().ok()?,
        unix_microseconds: *unix_microseconds,
    })
}

fn collect_attributes(
    entity: &Record,
    target: &AttributeTarget,
    by_index: &HashMap<i64, &Record>,
    emitted: &mut HashSet<i64>,
    out: &mut Vec<SourceAttribute>,
) {
    let mut current = entity.ref_at(0);
    let mut chain = HashSet::new();
    while let Some(index) = current.filter(|index| chain.insert(*index)) {
        let Some(record) = by_index.get(&index) else {
            break;
        };
        if emitted.insert(index) {
            out.push(source_attribute(record, target.clone()));
        }
        current = record.ref_at(0);
    }
}

/// The numeric record-index key of an attribute id
/// (`f3d:brep:attribute#<index>`), used to key records derived from that
/// attribute.
fn attribute_key(attribute: &SourceAttribute) -> &str {
    attribute
        .id
        .0
        .rsplit('#')
        .next()
        .unwrap_or(attribute.id.0.as_str())
}

fn source_attribute(record: &Record, target: AttributeTarget) -> SourceAttribute {
    SourceAttribute {
        id: AttributeId(format!("f3d:brep:attribute#{}", record.index)),
        target,
        name: record.name.clone(),
        values: record.tokens.iter().map(attribute_value).collect(),
    }
}

fn attribute_value(token: &Token) -> AttributeValue {
    match token {
        Token::Char(value) => AttributeValue::Integer(i64::from(*value)),
        Token::Short(value) => AttributeValue::Integer(i64::from(*value)),
        Token::Long(value) | Token::Enum(value) | Token::Int64(value) => {
            AttributeValue::Integer(*value)
        }
        Token::Float(value) => AttributeValue::Float(f64::from(*value)),
        Token::Double(value) => AttributeValue::Float(*value),
        Token::Str(value) => AttributeValue::String(value.clone()),
        Token::True => AttributeValue::Boolean(true),
        Token::False => AttributeValue::Boolean(false),
        Token::Ref(value) => AttributeValue::Reference(format!("f3d:brep:entity#{value}")),
        Token::SubtypeOpen => AttributeValue::String("subtype_open".into()),
        Token::SubtypeClose => AttributeValue::String("subtype_close".into()),
        Token::Position(value) | Token::Vector3(value) => AttributeValue::Vector(value.to_vec()),
        Token::Vector2(value) => AttributeValue::Vector(value.to_vec()),
    }
}

pub(crate) fn decode_transform(
    record: &Record,
    header_scale: f64,
) -> Option<cadmpeg_ir::transform::Transform> {
    let vectors: Vec<[f64; 3]> = record
        .tokens
        .iter()
        .filter_map(|token| match token {
            Token::Position(value) | Token::Vector3(value) => Some(*value),
            _ => None,
        })
        .collect();
    let scale = record
        .tokens
        .iter()
        .filter_map(|token| match token {
            Token::Double(value) => Some(*value),
            _ => None,
        })
        .next_back()?;
    let [x, y, z, translation] = vectors.as_slice() else {
        return None;
    };
    Some(cadmpeg_ir::transform::Transform {
        rows: [
            [x[0], y[0], z[0], translation[0] * header_scale * LEN_TO_MM],
            [x[1], y[1], z[1], translation[1] * header_scale * LEN_TO_MM],
            [x[2], y[2], z[2], translation[2] * header_scale * LEN_TO_MM],
            [0.0, 0.0, 0.0, scale],
        ],
    })
}

pub(crate) fn attribute_chain_color(
    entity: &Record,
    by_index: &HashMap<i64, &Record>,
) -> Option<Color> {
    let mut current = entity.ref_at(0)?;
    let mut seen = HashSet::new();
    while seen.insert(current) {
        let record = by_index.get(&current)?;
        if record.name.contains("rgb_color") {
            let values: Vec<f64> = record
                .tokens
                .iter()
                .filter_map(|t| match t {
                    Token::Double(value) => Some(*value),
                    _ => None,
                })
                .collect();
            if let [r, g, b, ..] = values.as_slice() {
                if [*r, *g, *b].iter().all(|value| (0.0..=1.0).contains(value)) {
                    return Some(Color {
                        r: *r as f32,
                        g: *g as f32,
                        b: *b as f32,
                        a: 1.0,
                    });
                }
            }
        } else if record.name.contains("truecolor") {
            let packed = record.tokens.iter().find_map(|token| match token {
                Token::Int64(value) | Token::Long(value) => Some(*value as u32),
                _ => None,
            })?;
            return Some(Color {
                r: ((packed >> 16) & 0xff) as f32 / 255.0,
                g: ((packed >> 8) & 0xff) as f32 / 255.0,
                b: (packed & 0xff) as f32 / 255.0,
                a: ((packed >> 24) & 0xff) as f32 / 255.0,
            });
        }
        current = record.ref_at(0)?;
    }
    None
}

fn attribute_chain_name(entity: &Record, by_index: &HashMap<i64, &Record>) -> Option<String> {
    let mut current = entity.ref_at(0)?;
    let mut seen = HashSet::new();
    while seen.insert(current) {
        let record = by_index.get(&current)?;
        if record.name == "string_attrib-name_attrib-gen-attrib" {
            let values = record
                .tokens
                .iter()
                .filter_map(|token| match token {
                    Token::Str(value) => Some(value.as_str()),
                    _ => None,
                })
                .collect::<Vec<_>>();
            if let [.., "name", value] = values.as_slice() {
                if !value.is_empty() {
                    return Some((*value).to_owned());
                }
            }
        }
        current = record.ref_at(0)?;
    }
    None
}

/// The raw bytes of a record within the decompressed stream.
fn record_slice<'a>(rec: &Record, bytes: &'a [u8]) -> &'a [u8] {
    let end = (rec.offset + rec.len).min(bytes.len());
    &bytes[rec.offset..end]
}

/// The `UnknownId` for a preserved carrier record. Shared by the passthrough
/// `UnknownRecord` and any `SurfaceGeometry::Unknown` that links to it, so the
/// reference resolves under validation.
fn unknown_record_id(rec: &Record) -> String {
    format!("f3d:brep:{}#{}", rec.head, rec.index)
}

fn ring_coedges(
    loop_rec: &Record,
    by_index: &HashMap<i64, &Record>,
    kept: &HashSet<i64>,
) -> Vec<CoedgeId> {
    let id = |i: i64| CoedgeId(format!("f3d:brep:entity#{i}"));
    let mut out = Vec::new();
    let Some(first) = loop_rec.ref_at(4) else {
        return out;
    };
    let mut cur = Some(first);
    let mut guard = HashSet::new();
    while let Some(ci) = cur {
        if !guard.insert(ci) || !kept.contains(&ci) {
            break;
        }
        out.push(id(ci));
        let Some(ce) = by_index.get(&ci) else { break };
        cur = ce.ref_at(3);
        if cur == Some(first) {
            break;
        }
    }
    out
}

fn loop_chain(
    face_rec: &Record,
    by_index: &HashMap<i64, &Record>,
    kept: &HashSet<i64>,
) -> Vec<LoopId> {
    let id = |i: i64| LoopId(format!("f3d:brep:entity#{i}"));
    let mut out = Vec::new();
    let mut cur = face_rec.ref_at(4);
    let mut guard = HashSet::new();
    while let Some(li) = cur {
        if !guard.insert(li) {
            break;
        }
        if kept.contains(&li) {
            out.push(id(li));
        }
        let Some(lp) = by_index.get(&li) else { break };
        cur = lp.ref_at(3);
    }
    out
}

fn face_chain(
    shell_rec: &Record,
    by_index: &HashMap<i64, &Record>,
    kept: &HashSet<i64>,
) -> Vec<FaceId> {
    let id = |i: i64| FaceId(format!("f3d:brep:entity#{i}"));
    let mut out = Vec::new();
    let mut cur = shell_rec.ref_at(5);
    let mut guard = HashSet::new();
    while let Some(fi) = cur {
        if !guard.insert(fi) {
            break;
        }
        if kept.contains(&fi) {
            out.push(id(fi));
        }
        let Some(f) = by_index.get(&fi) else { break };
        cur = f.ref_at(3);
    }
    out
}

fn subshell_ancestor_shells(
    records: &[Record],
    by_index: &HashMap<i64, &Record>,
) -> HashMap<i64, i64> {
    let mut out = HashMap::new();
    for record in records.iter().filter(|record| record.head == "subshell") {
        let mut owner = record.ref_at(3);
        let mut guard = HashSet::new();
        while let Some(index) = owner.filter(|index| guard.insert(*index)) {
            let Some(parent) = by_index.get(&index) else {
                break;
            };
            if parent.head == "shell" {
                out.insert(record.index as i64, index);
                break;
            }
            if parent.head != "subshell" {
                break;
            }
            owner = parent.ref_at(3);
        }
    }
    out
}

fn shell_faces(
    shell: &Record,
    by_index: &HashMap<i64, &Record>,
    kept: &HashSet<i64>,
) -> Vec<FaceId> {
    let mut out = face_chain(shell, by_index, kept);
    let mut pending = shell.ref_at(4).into_iter().collect::<Vec<_>>();
    let mut guard = HashSet::new();
    while let Some(index) = pending.pop().filter(|index| guard.insert(*index)) {
        let Some(record) = by_index
            .get(&index)
            .filter(|record| record.head == "subshell")
        else {
            break;
        };
        out.extend(face_chain_from(record.ref_at(6), by_index, kept));
        if let Some(next) = record.ref_at(4) {
            pending.push(next);
        }
        if let Some(child) = record.ref_at(5) {
            pending.push(child);
        }
    }
    out
}

fn face_chain_from(
    mut current: Option<i64>,
    by_index: &HashMap<i64, &Record>,
    kept: &HashSet<i64>,
) -> Vec<FaceId> {
    let mut out = Vec::new();
    let mut guard = HashSet::new();
    while let Some(index) = current.filter(|index| guard.insert(*index)) {
        if kept.contains(&index) {
            out.push(FaceId(format!("f3d:brep:entity#{index}")));
        }
        let Some(face) = by_index.get(&index) else {
            break;
        };
        current = face.ref_at(3);
    }
    out
}

fn shell_chain(region_rec: &Record, by_index: &HashMap<i64, &Record>) -> Vec<ShellId> {
    let id = |i: i64| ShellId(format!("f3d:brep:entity#{i}"));
    let mut out = Vec::new();
    let mut cur = region_rec.ref_at(4);
    let mut guard = HashSet::new();
    while let Some(si) = cur {
        if !guard.insert(si) {
            break;
        }
        out.push(id(si));
        let Some(s) = by_index.get(&si) else { break };
        cur = s.ref_at(0);
    }
    out
}

fn region_chain(body_rec: &Record, by_index: &HashMap<i64, &Record>) -> Vec<RegionId> {
    let id = |i: i64| RegionId(format!("f3d:brep:entity#{i}"));
    let mut out = Vec::new();
    let mut cur = body_rec.ref_at(3);
    let mut guard = HashSet::new();
    while let Some(li) = cur {
        if !guard.insert(li) {
            break;
        }
        out.push(id(li));
        let Some(l) = by_index.get(&li) else { break };
        cur = l.ref_at(0);
    }
    out
}

#[cfg(test)]
mod topology_tests {
    use super::*;

    fn ident(bytes: &mut Vec<u8>, name: &str) {
        bytes.push(0x0d);
        bytes.push(name.len() as u8);
        bytes.extend_from_slice(name.as_bytes());
    }

    fn reference(bytes: &mut Vec<u8>, value: i64) {
        bytes.push(0x0c);
        bytes.extend_from_slice(&value.to_le_bytes());
    }

    fn record(bytes: &mut Vec<u8>, name: &str, fields: &[i64]) {
        ident(bytes, name);
        for field in fields {
            reference(bytes, *field);
        }
        bytes.push(0x11);
    }

    #[test]
    fn generated_subshell_hierarchy_flattens_faces_onto_shell() {
        let mut bytes = Vec::new();
        record(&mut bytes, "asmheader", &[]); // 0
        record(&mut bytes, "shell", &[-1, -1, -1, -1, 2, -1, -1, -1]); // 1
        record(&mut bytes, "subshell", &[-1, -1, -1, 1, -1, 3, 4, -1]); // 2
        record(&mut bytes, "subshell", &[-1, -1, -1, 2, -1, -1, 5, -1]); // 3
        record(&mut bytes, "face", &[-1, -1, -1, -1]); // 4
        record(&mut bytes, "face", &[-1, -1, -1, -1]); // 5

        let records = crate::sab::frame(&bytes, 0, bytes.len(), 8)
            .expect("generated subshell bytes must frame");
        let by_index = records
            .iter()
            .map(|record| (record.index as i64, record))
            .collect::<HashMap<_, _>>();
        let kept = [4, 5].into_iter().collect::<HashSet<_>>();

        assert_eq!(
            shell_faces(&records[1], &by_index, &kept),
            vec![
                FaceId("f3d:brep:entity#4".into()),
                FaceId("f3d:brep:entity#5".into())
            ]
        );
        assert_eq!(
            subshell_ancestor_shells(&records, &by_index).get(&3),
            Some(&1)
        );
    }
}
