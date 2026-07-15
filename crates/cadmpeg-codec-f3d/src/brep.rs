// SPDX-License-Identifier: Apache-2.0
//! Build B-rep topology and geometry from a framed SAB record table.
//!
//! [`decode`] follows the topology chain from bodies through vertices and
//! points. It creates analytic carriers for planes, cylinders, cones, spheres,
//! tori, lines, circles, and ellipses. [`crate::nurbs`] supplies cached NURBS
//! surfaces, 3D curves, and pcurves for spline and procedural records.
//!
//! Faces retain their loops and trims when a referenced surface has no decoded
//! shape; a decoded construction produces a [`SurfaceGeometry::Procedural`]
//! carrier, while an undecoded record produces [`SurfaceGeometry::Unknown`]
//! linked to the corresponding [`UnknownRecord`]. Edges retain vertices and
//! parameter ranges when their 3D curve carrier is unavailable. [`Stats`]
//! records these transfer losses for the decode report.
//!
//! ASM model-space lengths become millimetres. Unit vectors, ratios, angles,
//! knots, weights, and UV parameters keep their native scale.

use std::collections::{HashMap, HashSet};

use crate::records::{
    BodyNativeKey, CreationTimestamp, EdgeContinuity, EdgeOwnership, FaceContainment,
    FaceSidedness, MeshSurfaceSentinel, PersistentDesignLink, PersistentSubentityTag,
    SketchCurveLink, TolerantCoedgeExtension, TolerantCoedgeParameters, TolerantEdgeTail,
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
    AttributeId, BodyId, CoedgeId, CurveId, EdgeId, FaceId, LoopId, PcurveId, PointId,
    ProceduralSurfaceId, RegionId, ShellId, SurfaceId, UnknownId, VertexId,
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
    /// Variable-width persistent tag groups attached to solved faces and edges.
    pub persistent_subentity_tags: Vec<PersistentSubentityTag>,
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
    /// Native trailing fields stored on tolerant edges.
    pub tolerant_edge_tails: Vec<TolerantEdgeTail>,
    /// Native trailing fields stored on tolerant vertices.
    pub tolerant_vertex_tails: Vec<TolerantVertexTail>,
    /// Zero-payload mesh-surface records used by emitted faces.
    pub mesh_surface_sentinels: Vec<MeshSurfaceSentinel>,
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
    /// Undecoded face-surface counts by full native record name.
    pub unknown_surface_kinds: std::collections::BTreeMap<String, usize>,
    /// Faces whose surface record explicitly delegates shape to mesh attributes.
    pub mesh_surface_faces: usize,
    /// Spline surface records whose cached B-spline block was decoded into a
    /// NURBS carrier.
    pub nurbs_surfaces: usize,
    /// Procedural curve records whose cached 3D B-spline block was decoded into
    /// a NURBS carrier.
    pub nurbs_curves: usize,
    /// Edges whose 3D curve is a procedural carrier (emitted with no curve).
    pub procedural_curve_edges: usize,
    /// Undecoded edge-curve counts by full native record name.
    pub procedural_curve_kinds: std::collections::BTreeMap<String, usize>,
    /// Coedges that carried an explicit UV pcurve ref with no decodable 2D
    /// carrier on the face surface's parameterization (undecodable bytes, or
    /// UV values on the exact procedural parameterization rather than the
    /// solved cache's).
    pub undecoded_pcurve_refs: usize,
    /// Undecoded coedge-pcurve counts by full native record name.
    pub undecoded_pcurve_kinds: std::collections::BTreeMap<String, usize>,
    /// Procedural blends for which only one of two support families resolved.
    pub partial_procedural_supports: usize,
    /// Record names in the active slice that were neither topology nor a
    /// decoded/preserved carrier (attributes, transforms, refinements, …).
    pub other_records: usize,
    /// Residual record counts by full record name.
    pub other_record_kinds: std::collections::BTreeMap<String, usize>,
}

fn count_kind(counts: &mut std::collections::BTreeMap<String, usize>, kind: &str) {
    *counts.entry(kind.to_owned()).or_default() += 1;
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

fn tolerant_coedge_extension(record: &Record) -> Option<TolerantCoedgeExtension> {
    let target = match record.chunk(13)? {
        Token::Ref(target) => (*target >= 0).then_some(*target),
        _ => return None,
    };
    match record.chunk(14)? {
        Token::Long(0) if matches!(record.chunk(15), Some(Token::Long(0))) => {
            Some(TolerantCoedgeExtension::Empty { target })
        }
        Token::Long(1) => {
            let flag = match record.chunk(15)? {
                Token::True => true,
                Token::False => false,
                _ => return None,
            };
            if !matches!(record.chunk(16), Some(Token::SubtypeOpen)) {
                return None;
            }
            let mut depth = 0usize;
            let mut close = None;
            for (index, token) in record.tokens.iter().enumerate().skip(16) {
                match token {
                    Token::SubtypeOpen => depth += 1,
                    Token::SubtypeClose => {
                        depth = depth.checked_sub(1)?;
                        if depth == 0 {
                            close = Some(index);
                            break;
                        }
                    }
                    _ => {}
                }
            }
            let close = close?;
            let parameter_range = match record.tokens.get(close + 1..) {
                Some([Token::False, Token::False, Token::Long(0)]) => None,
                Some(
                    [Token::True, Token::Double(start), Token::True, Token::Double(end), Token::Long(0)],
                ) if start.is_finite() && end.is_finite() => Some([*start, *end]),
                _ => return None,
            };
            Some(TolerantCoedgeExtension::EmbeddedCurve {
                target,
                flag,
                payload_token_count: u32::try_from(close.checked_sub(17)?).ok()?,
                parameter_range,
            })
        }
        _ => None,
    }
}

fn is_known_record_head(head: &str) -> bool {
    matches!(
        head,
        "body"
            | "region"
            | "lump"
            | "shell"
            | "subshell"
            | "wire"
            | "face"
            | "loop"
            | "point"
            | "asmheader"
    ) || matches!(
        head,
        "coedge" | "tcoedge" | "edge" | "tedge" | "vertex" | "tvertex"
    ) || is_analytic_surface(head)
        || is_analytic_curve(head)
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

fn edge_pcurve_parameter_ranges(edge: &Record) -> Option<[[f64; 2]; 2]> {
    let (Some(Token::Double(start)), Some(Token::Double(end))) = (edge.chunk(4), edge.chunk(6))
    else {
        return None;
    };
    let direct = [*start, *end];
    let negated = [-start, -end];
    Some(if matches!(edge.chunk(9), Some(Token::True)) {
        [negated, direct]
    } else {
        [direct, negated]
    })
}

/// Candidate edge-use intervals whose endpoints lie on this pcurve carrier.
/// Edge sense orders the two signs, but it cannot move a NURBS use outside the
/// carrier's knot domain. The full knot domain is the final fallback.
fn pcurve_ranges_on_domain(
    candidate: &nurbs::NurbsPcurve,
    edge: Option<&Record>,
) -> Option<Vec<[f64; 2]>> {
    let (&first, &last) = (candidate.knots.first()?, candidate.knots.last()?);
    let tolerance = 1.0e-9 * (last - first).abs().max(1.0);
    let mut ranges = edge
        .and_then(edge_pcurve_parameter_ranges)
        .into_iter()
        .flatten()
        .filter_map(|mut range| {
            if range
                .iter()
                .all(|value| *value >= first - tolerance && *value <= last + tolerance)
            {
                for value in &mut range {
                    if *value < first {
                        *value = first;
                    } else if *value > last {
                        *value = last;
                    }
                }
                Some(range)
            } else {
                None
            }
        })
        .collect::<Vec<_>>();
    if !ranges.contains(&[first, last]) {
        ranges.push([first, last]);
    }
    Some(ranges)
}

/// Select the candidate 2D block that is the face surface's parameter-space
/// image of the coedge: its endpoints, mapped through the surface, land on the
/// owning edge's vertex positions. On a non-NURBS surface, or when the edge's
/// vertex positions cannot be read, the first candidate passes unverified. An
/// empty result means no candidate is the surface's image of this edge.
fn select_face_pcurve(
    candidates: Vec<nurbs::NurbsPcurve>,
    surface: Option<&SurfaceGeometry>,
    exact_procedural_parameterization: bool,
    edge: Option<&Record>,
    by_index: &HashMap<i64, &Record>,
) -> Option<(nurbs::NurbsPcurve, [f64; 2])> {
    // Procedural surfaces retain an evaluated NURBS cache, but their pcurves
    // are expressed on the exact construction's parameterization. Evaluating
    // those UVs on the cache can drift between knots and is not a valid
    // candidate test. Candidate discovery orders unambiguous BS2 carriers
    // before ambiguous 3D interpretations, so the first candidate is the
    // authoritative exact-space carrier in this case.
    if exact_procedural_parameterization {
        let candidate = candidates.into_iter().next()?;
        let range = pcurve_ranges_on_domain(&candidate, edge)?[0];
        return Some((candidate, range));
    }
    let Some(SurfaceGeometry::Nurbs(surface)) = surface else {
        let candidate = candidates.into_iter().next()?;
        let range = pcurve_ranges_on_domain(&candidate, edge)?[0];
        return Some((candidate, range));
    };
    let vertex_pair = edge.and_then(|edge| {
        Some((
            vertex_position(by_index, edge.ref_at(3)?)?,
            vertex_position(by_index, edge.ref_at(5)?)?,
        ))
    });
    let Some((start, end)) = vertex_pair else {
        let candidate = candidates.into_iter().next()?;
        let range = pcurve_ranges_on_domain(&candidate, edge)?[0];
        return Some((candidate, range));
    };
    let mut best: Option<(f64, nurbs::NurbsPcurve, [f64; 2])> = None;
    for candidate in candidates {
        let uv_at = |t: f64| {
            eval::nurbs_pcurve_uv(
                candidate.degree,
                &candidate.knots,
                &candidate.control_points,
                candidate.weights.as_deref(),
                t,
            )
        };
        let Some(parameter_ranges) = pcurve_ranges_on_domain(&candidate, edge) else {
            continue;
        };
        let Some((mismatch, range)) = parameter_ranges
            .into_iter()
            .filter_map(|[first, second]| {
                let (uv0, uv1) = (uv_at(first)?, uv_at(second)?);
                let (p0, p1) = (
                    eval::nurbs_surface_point(surface, uv0.u, uv0.v)?,
                    eval::nurbs_surface_point(surface, uv1.u, uv1.v)?,
                );
                // Coedge sense can reverse traversal independently of the
                // edge carrier, so accept either endpoint assignment.
                let forward = distance(p0, start).max(distance(p1, end));
                let reversed = distance(p0, end).max(distance(p1, start));
                Some((forward.min(reversed), [first, second]))
            })
            .min_by(|(left, _), (right, _)| left.total_cmp(right))
        else {
            continue;
        };
        if mismatch <= PCURVE_ENDPOINT_TOLERANCE_MM
            && best
                .as_ref()
                .is_none_or(|(current, _, _)| mismatch < *current)
        {
            best = Some((mismatch, candidate, range));
        }
    }
    best.map(|(_, candidate, range)| (candidate, range))
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

fn reverse_procedural_curve_definition(
    definition: &mut cadmpeg_ir::geometry::ProceduralCurveDefinition,
) {
    if let cadmpeg_ir::geometry::ProceduralCurveDefinition::Helix {
        angle_range,
        minor,
        pitch,
        apex_factor,
        ..
    } = definition
    {
        *angle_range = [-angle_range[1], -angle_range[0]];
        *minor = Vector3::new(-minor.x, -minor.y, -minor.z);
        *pitch = Vector3::new(-pitch.x, -pitch.y, -pitch.z);
        *apex_factor = -*apex_factor;
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
    let release_major = header
        .as_ref()
        .and_then(|header| header.release)
        .map(|release| release / 100);
    let header_scale = header.and_then(|header| header.scale).unwrap_or(1.0);

    let attribute_color = |entity: &Record| attribute_chain_color(entity, &by_index);
    let attribute_name = |entity: &Record| attribute_chain_name(entity, &by_index);

    // Pass 1: classify carriers and decode analytic geometry.
    let mut surface_geo: HashMap<i64, (SurfaceGeometry, bool)> = HashMap::new();
    let mut procedural_surface_defs = HashMap::new();
    let mut curve_geo: HashMap<i64, CurveGeometry> = HashMap::new();
    let mut procedural_curve_defs = HashMap::new();
    let mut cacheless_procedural_curve_defs = HashMap::new();
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
    // surface keeps its topology and gets a construction-backed or unknown
    // carrier.
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
    let mut pcurve_parameter_ranges: HashMap<i64, [f64; 2]> = HashMap::new();
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
        if let Some(geometry) = procedural_surface_defs
            .get(&surf_ref)
            .and_then(|procedural| analytic_procedural_surface(&procedural.definition))
        {
            surface_geo.insert(surf_ref, (geometry, false));
        }
        let exact_cacheless_construction =
            procedural_surface_defs
                .get(&surf_ref)
                .is_some_and(|procedural| {
                    procedural.cache_fit_tolerance.is_none()
                        && procedural_surface_definition_is_exact_carrier(&procedural.definition)
                });
        // A non-analytic surface may still carry a decodable B-spline face
        // cache. Exact cacheless constructions own their nested surface blocks
        // as supports, not as evaluated face caches.
        if !exact_cacheless_construction {
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
        }
        if !surface_geo.contains_key(&surf_ref) && procedural_surface_defs.contains_key(&surf_ref) {
            let analytic_geometry = procedural_surface_defs
                .get(&surf_ref)
                .and_then(|procedural| analytic_procedural_surface(&procedural.definition));
            let construction_is_exact_carrier =
                procedural_surface_defs
                    .get(&surf_ref)
                    .is_some_and(|procedural| {
                        procedural_surface_definition_is_exact_carrier(&procedural.definition)
                    });
            surface_geo.insert(
                surf_ref,
                (
                    if let Some(geometry) = analytic_geometry {
                        geometry
                    } else if construction_is_exact_carrier {
                        SurfaceGeometry::Procedural {
                            construction: ProceduralSurfaceId(format!(
                                "f3d:brep:procedural_surface#{surf_ref}"
                            )),
                        }
                    } else {
                        SurfaceGeometry::Unknown {
                            record: Some(UnknownId(unknown_record_id(surf_rec))),
                        }
                    },
                    false,
                ),
            );
            if !construction_is_exact_carrier {
                undecoded_carriers.insert(surf_ref);
            }
        }
        if surface_geo.contains_key(&surf_ref) {
            kept_surfaces.insert(surf_ref);
        } else {
            unknown_surface_records.insert(surf_ref);
            undecoded_carriers.insert(surf_ref);
            if surf_rec.head == "mesh_surface" && surf_rec.tokens.is_empty() {
                if !out
                    .mesh_surface_sentinels
                    .iter()
                    .any(|sentinel| sentinel.record_index == surf_rec.index as u32)
                {
                    out.mesh_surface_sentinels.push(MeshSurfaceSentinel {
                        id: format!("f3d:asm:mesh-surface-sentinel#{}", surf_rec.index),
                        surface: SurfaceId(id(surf_ref)),
                        record_index: surf_rec.index as u32,
                    });
                }
                out.stats.mesh_surface_faces += 1;
            } else {
                out.stats.unknown_surface_faces += 1;
                count_kind(&mut out.stats.unknown_surface_kinds, &surf_rec.head);
            }
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
                            // every ref-form candidate and keep the one whose
                            // endpoints land on the edge's vertices through
                            // the face surface. An inline scope owns exactly
                            // one BS2 carrier and needs no disambiguation.
                            let inline = matches!(
                                (prec.chunk(3), prec.chunk(4)),
                                (Some(Token::Long(0)), Some(Token::True | Token::False))
                            );
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
                            let edge = ce.ref_at(6).and_then(|edge| by_index.get(&edge)).copied();
                            let decoded = if inline && candidates.len() == 1 {
                                let candidate =
                                    candidates.into_iter().next().expect("one candidate");
                                let range = pcurve_ranges_on_domain(&candidate.curve, edge)
                                    .and_then(|ranges| ranges.into_iter().next());
                                range.map(|range| (candidate.curve, range))
                            } else if candidates
                                .iter()
                                .filter(|candidate| candidate.unambiguous_2d)
                                .count()
                                == 1
                            {
                                let candidate = candidates
                                    .into_iter()
                                    .find(|candidate| candidate.unambiguous_2d)
                                    .expect("one unambiguous candidate");
                                let range = pcurve_ranges_on_domain(&candidate.curve, edge)
                                    .and_then(|ranges| ranges.into_iter().next());
                                range.map(|range| (candidate.curve, range))
                            } else {
                                select_face_pcurve(
                                    candidates
                                        .into_iter()
                                        .map(|candidate| candidate.curve)
                                        .collect(),
                                    face.ref_at(7)
                                        .and_then(|surface| surface_geo.get(&surface))
                                        .map(|(geometry, _)| geometry),
                                    face.ref_at(7).is_some_and(|surface| {
                                        procedural_surface_defs.contains_key(&surface)
                                    }),
                                    edge,
                                    &by_index,
                                )
                            };
                            if let Some((decoded, parameter_range)) = decoded {
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
                                pcurve_parameter_ranges.insert(ci, parameter_range);
                                kept_pcurves.insert(pc);
                            } else {
                                out.stats.undecoded_pcurve_refs += 1;
                                count_kind(&mut out.stats.undecoded_pcurve_kinds, &prec.head);
                            }
                        } else {
                            out.stats.undecoded_pcurve_refs += 1;
                            count_kind(&mut out.stats.undecoded_pcurve_kinds, "dangling-reference");
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
                                            } else if let Some((native_kind, mut definition)) =
                                                nurbs::decode_cacheless_procedural_curve_resolving_refs(
                                                    record_slice(crec, bytes),
                                                    bytes,
                                                    &subtype_tables,
                                                )
                                            {
                                                if record_reversed(crec) {
                                                    reverse_procedural_curve_definition(
                                                        &mut definition,
                                                    );
                                                }
                                                curve_geo.insert(
                                                    cv,
                                                    CurveGeometry::Procedural {
                                                        construction: format!(
                                                            "f3d:brep:procedural_curve#{cv}"
                                                        )
                                                        .into(),
                                                    },
                                                );
                                                cacheless_procedural_curve_defs
                                                    .insert(cv, (native_kind, definition));
                                                kept_curves.insert(cv);
                                            } else {
                                                undecoded_carriers.insert(cv);
                                                out.stats.procedural_curve_edges += 1;
                                                count_kind(
                                                    &mut out.stats.procedural_curve_kinds,
                                                    &crec.head,
                                                );
                                            }
                                        } else {
                                            out.stats.procedural_curve_edges += 1;
                                            count_kind(
                                                &mut out.stats.procedural_curve_kinds,
                                                "dangling-reference",
                                            );
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
    let mut free_vertices_by_shell = HashMap::<i64, Vec<i64>>::new();
    for shell in records.iter().filter(|record| record.head == "shell") {
        let shell_index = shell.index as i64;
        let mut wire_guard = HashSet::new();
        for root in shell_wire_roots(shell, &by_index) {
            let mut wire_ref = Some(root);
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
                let mut wire_edges = Vec::new();
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
                            if !wire_edges.contains(&edge_index) {
                                wire_edges.push(edge_index);
                            }
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
                                                if let Some(curve_record) =
                                                    by_index.get(&curve_index)
                                                {
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
                                                } else if let Some((native_kind, mut definition)) =
                                                    nurbs::decode_cacheless_procedural_curve_resolving_refs(
                                                        record_slice(curve_record, bytes),
                                                        bytes,
                                                        &subtype_tables,
                                                    )
                                                {
                                                    if record_reversed(curve_record) {
                                                        reverse_procedural_curve_definition(
                                                            &mut definition,
                                                        );
                                                    }
                                                    entry.insert(CurveGeometry::Procedural {
                                                        construction: format!(
                                                            "f3d:brep:procedural_curve#{curve_index}"
                                                        )
                                                        .into(),
                                                    });
                                                    cacheless_procedural_curve_defs.insert(
                                                        curve_index,
                                                        (native_kind, definition),
                                                    );
                                                    kept_curves.insert(curve_index);
                                                } else {
                                                    undecoded_carriers.insert(curve_index);
                                                    out.stats.procedural_curve_edges += 1;
                                                    count_kind(
                                                        &mut out.stats.procedural_curve_kinds,
                                                        &curve_record.head,
                                                    );
                                                }
                                                } else {
                                                    out.stats.procedural_curve_edges += 1;
                                                    count_kind(
                                                        &mut out.stats.procedural_curve_kinds,
                                                        "dangling-reference",
                                                    );
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
                let free_vertex = if wire.ref_at(4).is_none() {
                    wire.ref_at(6).filter(|vertex| {
                        by_index
                            .get(vertex)
                            .is_some_and(|record| is_vertex_record(record))
                    })
                } else {
                    None
                };
                if let Some(vertex) = free_vertex {
                    kept_vertices.insert(vertex);
                    let vertices = free_vertices_by_shell.entry(shell_index).or_default();
                    if !vertices.contains(&vertex) {
                        vertices.push(vertex);
                    }
                    if let Some(point) = by_index.get(&vertex).and_then(|record| record.ref_at(5)) {
                        kept_points.insert(point);
                    }
                }
                if let Some(side) = side {
                    out.wire_topologies.push(WireTopology {
                        id: format!("f3d:asm:wire-topology#{wire_index}"),
                        shell: ShellId(id(shell_index)),
                        record_index: wire.index as u32,
                        edges: wire_edges
                            .into_iter()
                            .map(|edge| EdgeId(id(edge)))
                            .collect(),
                        free_vertex: free_vertex.map(|vertex| VertexId(id(vertex))),
                        side,
                    });
                }
                wire_ref = wire.ref_at(3);
            }
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
                                parameter_interval,
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
                                u_sense,
                                v_sense,
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
                        cadmpeg_ir::geometry::ProceduralCurveDefinition::Intersection {
                            context: cadmpeg_ir::geometry::IntcurveSupportContext {
                                sides: std::array::from_fn(|side| {
                                    cadmpeg_ir::geometry::IntcurveSupportSide {
                                        surface: surfaces[side].clone(),
                                        pcurve: pcurves[side].clone(),
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
                                    }
                                }),
                                parameter_range: embedded.parameter_range,
                                discontinuities: embedded.discontinuities,
                            },
                            selector: embedded.selector,
                            third: cadmpeg_ir::geometry::IntcurveSupportSide {
                                surface: Some(surface_ids[2].clone()),
                                pcurve: Some(pcurves[2].clone()),
                            },
                        }
                    } else if let Some((family, embedded, tail)) = procedural.8 {
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
                        cadmpeg_ir::geometry::ProceduralCurveDefinition::SurfaceCurve {
                            family,
                            context: cadmpeg_ir::geometry::IntcurveSupportContext {
                                sides: std::array::from_fn(|side| {
                                    cadmpeg_ir::geometry::IntcurveSupportSide {
                                        surface: surfaces[side].clone(),
                                        pcurve: pcurves[side].clone(),
                                    }
                                }),
                                parameter_range: embedded.parameter_range,
                                discontinuities: embedded.discontinuities,
                            },
                            tail,
                        }
                    } else if let Some(embedded) = procedural.9 {
                        let support_ids: [Option<SurfaceId>; 2] = embedded
                            .context
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
                        let pcurves = embedded.context.pcurves.map(|pcurve| {
                            pcurve.map(|pcurve| PcurveGeometry::Nurbs {
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
                        let pcurves = embedded.context.pcurves.map(|pcurve| {
                            pcurve.map(|pcurve| PcurveGeometry::Nurbs {
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
                        let pcurves = embedded.context.pcurves.map(|pcurve| {
                            pcurve.map(|pcurve| PcurveGeometry::Nurbs {
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
                                native_kind: Some(procedural.0),
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
                } else if let Some((_native_kind, definition)) =
                    cacheless_procedural_curve_defs.remove(&i)
                {
                    out.procedural_curves.push(ProceduralCurve {
                        id: format!("f3d:brep:procedural_curve#{i}").into(),
                        curve: CurveId(id(i)),
                        definition,
                        cache_fit_tolerance: None,
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
                    if let (Some(owning_edge), Some(Token::Long(endpoint_index @ 0..=1))) = (
                        r.ref_at(3).filter(|owner| {
                            by_index
                                .get(owner)
                                .is_some_and(|record| is_edge_record(record))
                        }),
                        r.chunk(4),
                    ) {
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
                            let full_period = (sweep.abs() - std::f64::consts::TAU).abs() < 1.0e-9;
                            if !full_period {
                                a = a.rem_euclid(std::f64::consts::TAU);
                                if std::f64::consts::TAU - a < 1.0e-9 {
                                    a = 0.0;
                                }
                                b = a + sweep;
                            }
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
            let tolerant_tail = match (r.head.as_str(), r.chunk(11), r.chunk(12), r.chunk(13)) {
                (
                    "tedge",
                    Some(Token::Double(tolerance)),
                    Some(Token::Long(first)),
                    Some(Token::Long(second @ 0)),
                ) if tolerance.is_finite() && *tolerance >= 0.0 => {
                    Some((*tolerance, [*first, *second]))
                }
                _ => None,
            };
            out.edges.push(Edge {
                id: EdgeId(id(i)),
                curve,
                start: VertexId(id(start)),
                end: VertexId(id(end)),
                param_range,
                tolerance: tolerant_tail.map(|(tolerance, _)| tolerance * LEN_TO_MM),
            });
            if let Some((_, trailing_integers)) = tolerant_tail {
                out.tolerant_edge_tails.push(TolerantEdgeTail {
                    id: format!("f3d:asm:tolerant-edge-tail#{i}"),
                    edge: EdgeId(id(i)),
                    record_index: r.index as u32,
                    trailing_integers,
                });
            }
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
            let tolerant = if r.head == "tcoedge" {
                match (r.chunk(11), r.chunk(12)) {
                    (Some(Token::Double(start)), Some(Token::Double(end))) => {
                        let extension = match release_major {
                            Some(major) if major > 219 => tolerant_coedge_extension(r),
                            Some(215..=219) => match r.chunk(13) {
                                Some(Token::Ref(target)) => {
                                    Some(TolerantCoedgeExtension::Reference {
                                        target: (*target >= 0).then_some(*target),
                                    })
                                }
                                _ => None,
                            },
                            Some(_) => Some(TolerantCoedgeExtension::None),
                            None => None,
                        };
                        extension.map(|extension| ([*start, *end], extension))
                    }
                    _ => None,
                }
            } else {
                None
            };
            let use_curve = tolerant.as_ref().and_then(|(range, extension)| {
                let TolerantCoedgeExtension::EmbeddedCurve {
                    parameter_range, ..
                } = extension
                else {
                    return None;
                };
                let record_bytes = bytes.get(r.offset..r.offset.checked_add(r.len)?)?;
                let curve =
                    nurbs::decode_curve_cache_resolving_refs(record_bytes, bytes, &subtype_tables)?;
                let curve_id = CurveId(format!("f3d:brep:tolerant-coedge-curve#{i}"));
                out.curves.push(Curve {
                    id: curve_id.clone(),
                    geometry: CurveGeometry::Nurbs(curve),
                    source_object: None,
                });
                Some((curve_id, parameter_range.unwrap_or(*range)))
            });
            out.coedges.push(Coedge {
                id: CoedgeId(id(i)),
                owner_loop: LoopId(id(owner)),
                edge: EdgeId(id(edge)),
                next: CoedgeId(id(next)),
                previous: CoedgeId(id(prev)),
                radial_next: partner.map_or_else(|| CoedgeId(id(i)), |p| CoedgeId(id(p))),
                sense: sense_at(r, 7),
                pcurve: r
                    .ref_at(10)
                    .filter(|p| kept_pcurves.contains(p))
                    .map(|p| PcurveId(id(p))),
                pcurve_parameter_range: pcurve_parameter_ranges.get(&i).copied(),
                use_curve: use_curve.as_ref().map(|(curve, _)| curve.clone()),
                use_curve_parameter_range: use_curve.map(|(_, range)| range),
            });
            if let Some((parameter_range, extension)) = tolerant {
                out.tolerant_coedge_parameters
                    .push(TolerantCoedgeParameters {
                        id: format!("f3d:asm:tolerant-coedge-parameters#{i}"),
                        coedge: CoedgeId(id(i)),
                        record_index: r.index as u32,
                        parameter_range,
                        extension,
                    });
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
                coedges,
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
                    free_vertices: free_vertices_by_shell
                        .get(&i)
                        .into_iter()
                        .flatten()
                        .map(|vertex| VertexId(id(*vertex)))
                        .collect(),
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
            "vertex" | "tvertex" if kept_vertices.contains(&index) => {
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
            "body"
                if out
                    .bodies
                    .iter()
                    .any(|entity| entity.id.0 == id(owner_index)) =>
            {
                Some(AttributeTarget::Body(BodyId(id(owner_index))))
            }
            "face" if kept_faces.contains(&owner_index) => {
                Some(AttributeTarget::Face(FaceId(id(owner_index))))
            }
            "coedge" | "tcoedge" if kept_coedges.contains(&owner_index) => {
                Some(AttributeTarget::Coedge(CoedgeId(id(owner_index))))
            }
            "edge" | "tedge" if kept_edges.contains(&owner_index) => {
                Some(AttributeTarget::Edge(EdgeId(id(owner_index))))
            }
            "vertex" | "tvertex" if kept_vertices.contains(&owner_index) => {
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
    out.persistent_subentity_tags = out
        .attributes
        .iter()
        .flat_map(persistent_subentity_tags)
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
        if !is_known_record_head(&r.head)
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

    let curve_geometries = out
        .curves
        .iter()
        .map(|curve| (curve.id.0.as_str(), &curve.geometry))
        .collect::<HashMap<_, _>>();
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
                "ellipse" => match curve_geometries.get(entity_id.as_str()) {
                    Some(CurveGeometry::Circle { .. }) => {
                        derived_fields.extend(["geometry.axis", "geometry.ref_direction"]);
                    }
                    Some(CurveGeometry::Ellipse { .. }) => {
                        derived_fields.extend(["geometry.axis", "geometry.major_direction"]);
                    }
                    _ => {}
                },
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

fn procedural_surface_definition_is_exact_carrier(
    definition: &nurbs::DecodedProceduralSurfaceDefinition,
) -> bool {
    match definition {
        nurbs::DecodedProceduralSurfaceDefinition::Extrusion { .. }
        | nurbs::DecodedProceduralSurfaceDefinition::Helix(_)
        | nurbs::DecodedProceduralSurfaceDefinition::VertexBlend(_) => true,
        nurbs::DecodedProceduralSurfaceDefinition::ScaledCompoundLoft(construction) => matches!(
            construction.shape,
            nurbs::EmbeddedScaledCompoundLoftShape::None { .. }
        ),
        _ => false,
    }
}

fn analytic_procedural_surface(
    definition: &nurbs::DecodedProceduralSurfaceDefinition,
) -> Option<SurfaceGeometry> {
    match definition {
        nurbs::DecodedProceduralSurfaceDefinition::Extrusion {
            directrix,
            direction,
            ..
        } => {
            let (center, normal, ref_direction, radius) = rational_four_arc_circle(directrix)?;
            let axis = normalized_vector(*direction)?;
            if 1.0 - dot_vector(axis, normal).abs() > 1.0e-10 {
                return None;
            }
            Some(SurfaceGeometry::Cylinder {
                origin: center,
                axis,
                ref_direction,
                radius,
            })
        }
        nurbs::DecodedProceduralSurfaceDefinition::Blend {
            supports,
            spine: Some(spine),
            radius: cadmpeg_ir::geometry::BlendRadiusLaw::Constant { signed_radius },
            cross_section: cadmpeg_ir::geometry::BlendCrossSection::Circular,
            native,
        } => analytic_rolling_ball_surface(supports, native.as_deref(), spine, *signed_radius),
        _ => None,
    }
}

fn analytic_rolling_ball_surface(
    supports: &[Option<SurfaceGeometry>; 2],
    native: Option<&nurbs::EmbeddedRollingBall>,
    spine: &cadmpeg_ir::geometry::NurbsCurve,
    signed_radius: f64,
) -> Option<SurfaceGeometry> {
    let radius = signed_radius.abs();
    if !radius.is_finite() || radius <= f64::EPSILON {
        return None;
    }
    let support = |index: usize| {
        supports[index]
            .as_ref()
            .or_else(|| native.and_then(|native| native.sides[index].surface.as_ref()))
    };
    let first = support(0)?;
    let second = support(1)?;

    if let (
        SurfaceGeometry::Plane {
            origin: first_origin,
            normal: first_normal,
            ..
        },
        SurfaceGeometry::Plane {
            origin: second_origin,
            normal: second_normal,
            ..
        },
    ) = (first, second)
    {
        let (origin, axis) = linear_nurbs_spine(spine)?;
        let tolerance = 1.0e-10
            * radius
                .max(point_vector(*first_origin, *second_origin).norm())
                .max(1.0);
        let first_normal = normalized_vector(*first_normal)?;
        let second_normal = normalized_vector(*second_normal)?;
        let support_intersection = cross_vector(first_normal, second_normal);
        let support_intersection_norm = support_intersection.norm();
        if support_intersection_norm <= 1.0e-10
            || 1.0
                - dot_vector(
                    axis,
                    Vector3::new(
                        support_intersection.x / support_intersection_norm,
                        support_intersection.y / support_intersection_norm,
                        support_intersection.z / support_intersection_norm,
                    ),
                )
                .abs()
                > 1.0e-10
        {
            return None;
        }
        for (plane_origin, plane_normal) in [
            (*first_origin, first_normal),
            (*second_origin, second_normal),
        ] {
            if dot_vector(axis, plane_normal).abs() > 1.0e-10
                || (dot_vector(point_vector(plane_origin, origin), plane_normal).abs() - radius)
                    .abs()
                    > tolerance
            {
                return None;
            }
        }
        return Some(SurfaceGeometry::Cylinder {
            origin,
            axis,
            ref_direction: cadmpeg_ir::geometry::derive_reference_direction(axis),
            radius,
        });
    }

    let (plane, cylinder) = match (first, second) {
        (plane @ SurfaceGeometry::Plane { .. }, cylinder @ SurfaceGeometry::Cylinder { .. })
        | (cylinder @ SurfaceGeometry::Cylinder { .. }, plane @ SurfaceGeometry::Plane { .. }) => {
            (plane, cylinder)
        }
        _ => return None,
    };
    let (center, axis, ref_direction, major_radius) = rational_four_arc_circle(spine)?;
    let SurfaceGeometry::Plane {
        origin: plane_origin,
        normal: plane_normal,
        ..
    } = plane
    else {
        unreachable!()
    };
    let SurfaceGeometry::Cylinder {
        origin: cylinder_origin,
        axis: cylinder_axis,
        radius: cylinder_radius,
        ..
    } = cylinder
    else {
        unreachable!()
    };
    let plane_normal = normalized_vector(*plane_normal)?;
    let cylinder_axis = normalized_vector(*cylinder_axis)?;
    let scale = major_radius.max(radius).max(cylinder_radius.abs()).max(1.0);
    let tolerance = 1.0e-10 * scale;
    let center_offset = point_vector(*cylinder_origin, center);
    let axial_offset = dot_vector(center_offset, cylinder_axis);
    let radial_offset = Vector3::new(
        center_offset.x - axial_offset * cylinder_axis.x,
        center_offset.y - axial_offset * cylinder_axis.y,
        center_offset.z - axial_offset * cylinder_axis.z,
    );
    if 1.0 - dot_vector(axis, plane_normal).abs() > 1.0e-10
        || 1.0 - dot_vector(axis, cylinder_axis).abs() > 1.0e-10
        || (dot_vector(point_vector(*plane_origin, center), plane_normal).abs() - radius).abs()
            > tolerance
        || radial_offset.norm() > tolerance
        || ((major_radius - cylinder_radius.abs()).abs() - radius).abs() > tolerance
    {
        return None;
    }
    Some(SurfaceGeometry::Torus {
        center,
        axis,
        ref_direction,
        major_radius,
        minor_radius: signed_radius,
    })
}

fn linear_nurbs_spine(curve: &cadmpeg_ir::geometry::NurbsCurve) -> Option<(Point3, Vector3)> {
    if curve.degree == 0
        || curve.periodic
        || curve.control_points.len() <= curve.degree as usize
        || curve.knots.len() != curve.control_points.len() + curve.degree as usize + 1
        || curve.knots.iter().any(|knot| !knot.is_finite())
        || curve.knots.windows(2).any(|pair| pair[0] > pair[1])
        || curve
            .control_points
            .iter()
            .any(|point| !point.x.is_finite() || !point.y.is_finite() || !point.z.is_finite())
    {
        return None;
    }
    if let Some(weights) = curve.weights.as_deref() {
        let first_sign = weights.first()?.signum();
        if first_sign == 0.0
            || weights.len() != curve.control_points.len()
            || weights
                .iter()
                .any(|weight| !weight.is_finite() || weight.signum() != first_sign)
        {
            return None;
        }
    }
    let origin = curve.control_points[0];
    let (_, farthest) = curve
        .control_points
        .iter()
        .copied()
        .map(|point| (point_vector(origin, point).norm(), point))
        .max_by(|left, right| left.0.total_cmp(&right.0))?;
    let extent = point_vector(origin, farthest).norm();
    if !extent.is_finite() || extent <= f64::EPSILON {
        return None;
    }
    let axis = normalized_vector(point_vector(origin, farthest))?;
    let tolerance = 1.0e-10 * extent.max(1.0);
    if curve
        .control_points
        .iter()
        .any(|point| cross_vector(axis, point_vector(origin, *point)).norm() > tolerance)
    {
        return None;
    }
    Some((origin, axis))
}

fn normalized_vector(vector: Vector3) -> Option<Vector3> {
    let norm = vector.norm();
    if !norm.is_finite() || norm <= f64::EPSILON {
        return None;
    }
    Some(Vector3::new(
        vector.x / norm,
        vector.y / norm,
        vector.z / norm,
    ))
}

fn rational_four_arc_circle(
    curve: &cadmpeg_ir::geometry::NurbsCurve,
) -> Option<(Point3, Vector3, Vector3, f64)> {
    let weights = curve.weights.as_deref()?;
    let degree = curve.degree as usize;
    if degree < 2
        || curve.periodic
        || curve.control_points.len() != 4 * degree + 1
        || weights.len() != curve.control_points.len()
        || curve.knots.len() != curve.control_points.len() + degree + 1
        || curve.knots.iter().any(|knot| !knot.is_finite())
    {
        return None;
    }
    let knot_tolerance = 1.0e-12
        * (curve.knots[curve.knots.len() - 1] - curve.knots[0])
            .abs()
            .max(1.0);
    let spans = [
        curve.knots[0],
        curve.knots[degree + 1],
        curve.knots[2 * degree + 1],
        curve.knots[3 * degree + 1],
        curve.knots[4 * degree + 1],
    ];
    if spans
        .windows(2)
        .any(|pair| !pair[0].is_finite() || pair[1] - pair[0] <= knot_tolerance)
        || (0..5).any(|span| {
            let range = if span == 0 {
                0..degree + 1
            } else if span == 4 {
                4 * degree + 1..curve.knots.len()
            } else {
                span * degree + 1..(span + 1) * degree + 1
            };
            curve.knots[range]
                .iter()
                .any(|value| (*value - spans[span]).abs() > knot_tolerance)
        })
    {
        return None;
    }
    let homogeneous = curve
        .control_points
        .iter()
        .zip(weights)
        .map(|(point, weight)| {
            let homogeneous = [
                point.x * weight,
                point.y * weight,
                point.z * weight,
                *weight,
            ];
            (point.x.is_finite()
                && point.y.is_finite()
                && point.z.is_finite()
                && weight.is_finite()
                && *weight != 0.0
                && homogeneous.iter().all(|value| value.is_finite()))
            .then_some(homogeneous)
        })
        .collect::<Option<Vec<_>>>()?;
    let quadratics = (0..4)
        .map(|span| {
            reduce_homogeneous_bezier_to_quadratic(
                homogeneous[span * degree..=span * degree + degree].to_vec(),
            )
        })
        .collect::<Option<Vec<_>>>()?;
    let base_weight = quadratics[0][0][3];
    let weight_scale = base_weight.abs().max(1.0);
    let weight_tolerance = 1.0e-10 * weight_scale;
    if !base_weight.is_finite()
        || base_weight == 0.0
        || quadratics.iter().any(|span| {
            (span[0][3] - base_weight).abs() > weight_tolerance
                || (span[2][3] - base_weight).abs() > weight_tolerance
                || (span[1][3] - base_weight * std::f64::consts::FRAC_1_SQRT_2).abs()
                    > weight_tolerance
        })
    {
        return None;
    }
    let quadratic_points = quadratics
        .iter()
        .map(|span| {
            span.map(|point| {
                Point3::new(
                    point[0] / point[3],
                    point[1] / point[3],
                    point[2] / point[3],
                )
            })
        })
        .collect::<Vec<_>>();
    let point_distance = |left: Point3, right: Point3| point_vector(left, right).norm();
    let scale = quadratic_points
        .iter()
        .flat_map(|span| span.windows(2))
        .map(|pair| point_distance(pair[0], pair[1]))
        .fold(0.0_f64, f64::max)
        .max(1.0);
    let tolerance = 1.0e-10 * scale;
    if point_distance(quadratic_points[0][0], quadratic_points[3][2]) > tolerance
        || quadratic_points
            .windows(2)
            .any(|pair| point_distance(pair[0][2], pair[1][0]) > tolerance)
    {
        return None;
    }
    let first_center = point_sum_difference(
        quadratic_points[0][0],
        quadratic_points[0][2],
        quadratic_points[0][1],
    );
    for span in &quadratic_points {
        let [start, control, end] = *span;
        let center = point_sum_difference(start, end, control);
        if point_distance(center, first_center) > tolerance {
            return None;
        }
    }
    let first_radial = point_vector(first_center, quadratic_points[0][0]);
    let radius = first_radial.norm();
    if !radius.is_finite() || radius <= tolerance {
        return None;
    }
    let mut normal = None;
    for span in &quadratic_points {
        let radial = point_vector(first_center, span[0]);
        let next = point_vector(first_center, span[2]);
        if (radial.norm() - radius).abs() > tolerance
            || dot_vector(radial, next).abs() > tolerance * radius
        {
            return None;
        }
        let span_normal = cross_vector(radial, next);
        let span_normal_norm = span_normal.norm();
        if span_normal_norm <= tolerance * radius {
            return None;
        }
        let span_normal = Vector3::new(
            span_normal.x / span_normal_norm,
            span_normal.y / span_normal_norm,
            span_normal.z / span_normal_norm,
        );
        if normal.is_some_and(|normal: Vector3| dot_vector(normal, span_normal) < 1.0 - 1.0e-10) {
            return None;
        }
        normal.get_or_insert(span_normal);
    }
    Some((
        first_center,
        normal?,
        Vector3::new(
            first_radial.x / radius,
            first_radial.y / radius,
            first_radial.z / radius,
        ),
        radius,
    ))
}

fn reduce_homogeneous_bezier_to_quadratic(mut control: Vec<[f64; 4]>) -> Option<[[f64; 4]; 3]> {
    while control.len() > 3 {
        let degree = control.len() - 1;
        let mut reduced = Vec::with_capacity(degree);
        reduced.push(control[0]);
        for index in 1..degree {
            let alpha = index as f64 / degree as f64;
            let denominator = 1.0 - alpha;
            reduced.push(std::array::from_fn(|coordinate| {
                (control[index][coordinate] - alpha * reduced[index - 1][coordinate]) / denominator
            }));
        }
        if reduced.iter().flatten().any(|value| !value.is_finite()) {
            return None;
        }
        let scale = control
            .iter()
            .flatten()
            .fold(1.0_f64, |scale, value| scale.max(value.abs()));
        if (0..4).any(|coordinate| {
            (reduced[degree - 1][coordinate] - control[degree][coordinate]).abs() > 1.0e-10 * scale
        }) {
            return None;
        }
        control = reduced;
    }
    control.try_into().ok()
}

fn point_vector(origin: Point3, point: Point3) -> Vector3 {
    Vector3::new(point.x - origin.x, point.y - origin.y, point.z - origin.z)
}

fn point_sum_difference(first: Point3, second: Point3, subtract: Point3) -> Point3 {
    Point3::new(
        first.x + second.x - subtract.x,
        first.y + second.y - subtract.y,
        first.z + second.z - subtract.z,
    )
}

fn dot_vector(first: Vector3, second: Vector3) -> f64 {
    first.x * second.x + first.y * second.y + first.z * second.z
}

fn cross_vector(first: Vector3, second: Vector3) -> Vector3 {
    Vector3::new(
        first.y * second.z - first.z * second.y,
        first.z * second.x - first.x * second.z,
        first.x * second.y - first.y * second.x,
    )
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
    let mut body_has_wires = HashSet::new();
    let mut face_bodies = HashMap::new();
    for shell in &out.shells {
        let Some(body) = shell_bodies.get(&shell.id) else {
            continue;
        };
        if !shell.wire_edges.is_empty() || !shell.free_vertices.is_empty() {
            body_has_wires.insert(body.clone());
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
        if body_has_wires.contains(&body.id) {
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
    let AttributeTarget::Body(_) = &attribute.target else {
        return Vec::new();
    };
    let Some(family) = attribute.values.iter().position(
        |value| matches!(value, AttributeValue::String(name) if name == "generic_tag_attrib_def"),
    ) else {
        return Vec::new();
    };
    let values = &attribute.values[family + 1..];
    let [AttributeValue::Integer(3), AttributeValue::Integer(3), AttributeValue::Integer(-1), AttributeValue::String(marker), AttributeValue::Integer(group_count), rest @ ..] =
        values
    else {
        return Vec::new();
    };
    if marker != "generic_tag_attrib_def " || *group_count < 0 {
        return Vec::new();
    }
    let Ok(group_count) = usize::try_from(*group_count) else {
        return Vec::new();
    };
    if rest.len() != group_count.saturating_mul(5) {
        return Vec::new();
    }
    let groups = rest
        .chunks_exact(5)
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
    if groups.len() != group_count {
        return Vec::new();
    }
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

fn persistent_subentity_tags(attribute: &SourceAttribute) -> Vec<PersistentSubentityTag> {
    if !matches!(
        attribute.target,
        AttributeTarget::Face(_) | AttributeTarget::Edge(_)
    ) {
        return Vec::new();
    }
    let Some(family) = attribute.values.iter().position(
        |value| matches!(value, AttributeValue::String(name) if name == "generic_tag_attrib_def"),
    ) else {
        return Vec::new();
    };
    let values = &attribute.values[family + 1..];
    let [AttributeValue::Integer(3), AttributeValue::Integer(3), AttributeValue::Integer(-1), AttributeValue::String(marker), AttributeValue::Integer(group_count), rest @ ..] =
        values
    else {
        return Vec::new();
    };
    if marker != "generic_tag_attrib_def " || *group_count < 0 {
        return Vec::new();
    }
    let Ok(group_count) = usize::try_from(*group_count) else {
        return Vec::new();
    };
    let mut position: usize = 0;
    let mut groups = Vec::with_capacity(group_count);
    for ordinal in 0..group_count {
        let Some(
            [AttributeValue::Integer(selector), AttributeValue::String(token), AttributeValue::Integer(0), AttributeValue::Integer(reference_count)],
        ) = rest.get(position..position.saturating_add(4))
        else {
            return Vec::new();
        };
        if token.is_empty() || *reference_count < 0 {
            return Vec::new();
        }
        let Ok(reference_count) = usize::try_from(*reference_count) else {
            return Vec::new();
        };
        let reference_start = position + 4;
        let reference_end = reference_start.saturating_add(reference_count);
        let Some(reference_values) = rest.get(reference_start..reference_end) else {
            return Vec::new();
        };
        let references = reference_values
            .iter()
            .map(|value| match value {
                AttributeValue::Integer(value) => Some(*value),
                _ => None,
            })
            .collect::<Option<Vec<_>>>();
        let Some(design_references) = references else {
            return Vec::new();
        };
        if !matches!(rest.get(reference_end), Some(AttributeValue::Integer(0))) {
            return Vec::new();
        }
        groups.push(PersistentSubentityTag {
            id: format!(
                "f3d:design:persistent-subentity-tag#{}:{ordinal}",
                attribute_key(attribute)
            ),
            target: attribute.target.clone(),
            selector: *selector,
            token: token.clone(),
            design_references,
            ordinal: ordinal as u32,
        });
        position = reference_end + 1;
    }
    if position != rest.len() {
        return Vec::new();
    }
    groups
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

fn shell_wire_roots(shell: &Record, by_index: &HashMap<i64, &Record>) -> Vec<i64> {
    let mut out = shell.ref_at(6).into_iter().collect::<Vec<_>>();
    let mut pending = shell.ref_at(4).into_iter().collect::<Vec<_>>();
    let mut guard = HashSet::new();
    while let Some(index) = pending.pop().filter(|index| guard.insert(*index)) {
        let Some(record) = by_index
            .get(&index)
            .filter(|record| record.head == "subshell")
        else {
            break;
        };
        if let Some(wire) = record.ref_at(7) {
            out.push(wire);
        }
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

    fn exact_circle_directrix() -> cadmpeg_ir::geometry::NurbsCurve {
        let center = Point3::new(2.0, 3.0, 4.0);
        let point = |x, y| Point3::new(center.x + x, center.y + y, center.z);
        cadmpeg_ir::geometry::NurbsCurve {
            degree: 2,
            knots: vec![0.0, 0.0, 0.0, 1.0, 1.0, 2.0, 2.0, 3.0, 3.0, 4.0, 4.0, 4.0],
            control_points: vec![
                point(5.0, 0.0),
                point(5.0, 5.0),
                point(0.0, 5.0),
                point(-5.0, 5.0),
                point(-5.0, 0.0),
                point(-5.0, -5.0),
                point(0.0, -5.0),
                point(5.0, -5.0),
                point(5.0, 0.0),
            ],
            weights: Some(vec![
                1.0,
                std::f64::consts::FRAC_1_SQRT_2,
                1.0,
                std::f64::consts::FRAC_1_SQRT_2,
                1.0,
                std::f64::consts::FRAC_1_SQRT_2,
                1.0,
                std::f64::consts::FRAC_1_SQRT_2,
                1.0,
            ]),
            periodic: false,
        }
    }

    #[test]
    fn exact_circle_extrusion_reduces_to_cylinder_only_along_normal() {
        let definition = |direction| nurbs::DecodedProceduralSurfaceDefinition::Extrusion {
            directrix: exact_circle_directrix(),
            parameter_interval: [0.0, 4.0],
            direction,
            native_position: Point3::new(0.0, 0.0, 0.0),
        };
        let Some(SurfaceGeometry::Cylinder {
            origin,
            axis,
            ref_direction,
            radius,
        }) = analytic_procedural_surface(&definition(Vector3::new(0.0, 0.0, -8.0)))
        else {
            panic!("exact circle extrusion did not reduce")
        };
        assert!(point_vector(Point3::new(2.0, 3.0, 4.0), origin).norm() < 1.0e-12);
        assert_eq!(axis, Vector3::new(0.0, 0.0, -1.0));
        assert!((ref_direction.x - 1.0).abs() < 1.0e-12);
        assert!(ref_direction.y.abs() < 1.0e-12);
        assert!(ref_direction.z.abs() < 1.0e-12);
        assert!((radius - 5.0).abs() < 1.0e-12);
        assert!(analytic_procedural_surface(&definition(Vector3::new(1.0, 0.0, 8.0))).is_none());
        let mut approximate = exact_circle_directrix();
        approximate.control_points[3].x += 1.0e-5;
        assert!(rational_four_arc_circle(&approximate).is_none());
    }

    fn degree_elevated_circle() -> cadmpeg_ir::geometry::NurbsCurve {
        let quadratic = exact_circle_directrix();
        let weights = quadratic.weights.as_deref().unwrap();
        let homogeneous = |index: usize| {
            let point = quadratic.control_points[index];
            let weight = weights[index] * 7.0;
            [point.x * weight, point.y * weight, point.z * weight, weight]
        };
        let combine = |first: [f64; 4], first_scale: f64, second: [f64; 4], second_scale: f64| {
            std::array::from_fn(|coordinate| {
                first_scale * first[coordinate] + second_scale * second[coordinate]
            })
        };
        let mut elevated = Vec::new();
        for span in 0..4 {
            let [first, middle, last] = [
                homogeneous(span * 2),
                homogeneous(span * 2 + 1),
                homogeneous(span * 2 + 2),
            ];
            let span = [
                first,
                combine(first, 1.0 / 3.0, middle, 2.0 / 3.0),
                combine(middle, 2.0 / 3.0, last, 1.0 / 3.0),
                last,
            ];
            elevated.extend_from_slice(if elevated.is_empty() {
                &span
            } else {
                &span[1..]
            });
        }
        let (control_points, weights): (Vec<_>, Vec<_>) = elevated
            .into_iter()
            .map(|point| {
                (
                    Point3::new(
                        point[0] / point[3],
                        point[1] / point[3],
                        point[2] / point[3],
                    ),
                    point[3],
                )
            })
            .unzip();
        cadmpeg_ir::geometry::NurbsCurve {
            degree: 3,
            knots: vec![
                0.0, 0.0, 0.0, 0.0, 1.0, 1.0, 1.0, 2.0, 2.0, 2.0, 3.0, 3.0, 3.0, 4.0, 4.0, 4.0, 4.0,
            ],
            control_points,
            weights: Some(weights),
            periodic: false,
        }
    }

    #[test]
    fn exact_circle_recognition_is_projective_and_degree_invariant() {
        let mut scaled = exact_circle_directrix();
        for weight in scaled.weights.as_mut().unwrap() {
            *weight *= 7.0;
        }
        assert!(rational_four_arc_circle(&scaled).is_some());

        let mut elevated = degree_elevated_circle();
        assert!(rational_four_arc_circle(&elevated).is_some());
        assert!(matches!(
            analytic_procedural_surface(&nurbs::DecodedProceduralSurfaceDefinition::Extrusion {
                directrix: elevated.clone(),
                parameter_interval: [0.0, 4.0],
                direction: Vector3::new(0.0, 0.0, 3.0),
                native_position: Point3::new(0.0, 0.0, 0.0),
            }),
            Some(SurfaceGeometry::Cylinder { .. })
        ));
        elevated.control_points[5].x += 1.0e-5;
        assert!(rational_four_arc_circle(&elevated).is_none());
    }

    fn plane(origin: Point3, normal: Vector3, u_axis: Vector3) -> SurfaceGeometry {
        SurfaceGeometry::Plane {
            origin,
            normal,
            u_axis,
        }
    }

    fn cylinder(origin: Point3, axis: Vector3, radius: f64) -> SurfaceGeometry {
        SurfaceGeometry::Cylinder {
            origin,
            axis,
            ref_direction: Vector3::new(1.0, 0.0, 0.0),
            radius,
        }
    }

    fn linear_spine(points: Vec<Point3>) -> cadmpeg_ir::geometry::NurbsCurve {
        cadmpeg_ir::geometry::NurbsCurve {
            degree: 2,
            knots: vec![0.0, 0.0, 0.0, 1.0, 1.0, 1.0],
            control_points: points,
            weights: None,
            periodic: false,
        }
    }

    #[test]
    fn constant_circular_plane_plane_blend_reduces_to_tangent_cylinder() {
        let mut definition = nurbs::DecodedProceduralSurfaceDefinition::Blend {
            supports: Box::new([
                Some(plane(
                    Point3::new(0.0, 0.0, 0.0),
                    Vector3::new(1.0, 0.0, 0.0),
                    Vector3::new(0.0, 1.0, 0.0),
                )),
                Some(plane(
                    Point3::new(0.0, 0.0, 0.0),
                    Vector3::new(0.0, 1.0, 0.0),
                    Vector3::new(1.0, 0.0, 0.0),
                )),
            ]),
            spine: Some(linear_spine(vec![
                Point3::new(2.0, 2.0, -4.0),
                Point3::new(2.0, 2.0, 0.0),
                Point3::new(2.0, 2.0, 7.0),
            ])),
            radius: cadmpeg_ir::geometry::BlendRadiusLaw::Constant {
                signed_radius: -2.0,
            },
            cross_section: cadmpeg_ir::geometry::BlendCrossSection::Circular,
            native: None,
        };
        assert!(matches!(
            analytic_procedural_surface(&definition),
            Some(SurfaceGeometry::Cylinder {
                origin,
                axis,
                radius,
                ..
            }) if origin == Point3::new(2.0, 2.0, -4.0)
                && axis == Vector3::new(0.0, 0.0, 1.0)
                && radius == 2.0
        ));

        let nurbs::DecodedProceduralSurfaceDefinition::Blend {
            spine: Some(spine), ..
        } = &mut definition
        else {
            unreachable!()
        };
        spine.control_points[1].x = 2.1;
        assert!(analytic_procedural_surface(&definition).is_none());
    }

    #[test]
    fn constant_circular_plane_cylinder_blend_reduces_to_tangent_torus() {
        let mut circle = exact_circle_directrix();
        for point in &mut circle.control_points {
            point.x -= 2.0;
            point.y -= 3.0;
            point.z -= 3.0;
        }
        let mut definition = nurbs::DecodedProceduralSurfaceDefinition::Blend {
            supports: Box::new([
                Some(plane(
                    Point3::new(0.0, 0.0, -1.0),
                    Vector3::new(0.0, 0.0, 1.0),
                    Vector3::new(1.0, 0.0, 0.0),
                )),
                Some(cylinder(
                    Point3::new(0.0, 0.0, 0.0),
                    Vector3::new(0.0, 0.0, 1.0),
                    3.0,
                )),
            ]),
            spine: Some(circle),
            radius: cadmpeg_ir::geometry::BlendRadiusLaw::Constant {
                signed_radius: -2.0,
            },
            cross_section: cadmpeg_ir::geometry::BlendCrossSection::Circular,
            native: None,
        };
        assert!(matches!(
            analytic_procedural_surface(&definition),
            Some(SurfaceGeometry::Torus {
                center,
                axis,
                ref_direction,
                major_radius,
                minor_radius,
            }) if center == Point3::new(0.0, 0.0, 1.0)
                && axis == Vector3::new(0.0, 0.0, 1.0)
                && ref_direction == Vector3::new(1.0, 0.0, 0.0)
                && major_radius == 5.0
                && minor_radius == -2.0
        ));

        let nurbs::DecodedProceduralSurfaceDefinition::Blend { supports, .. } = &mut definition
        else {
            unreachable!()
        };
        supports[0] = Some(plane(
            Point3::new(0.0, 0.0, -1.0),
            Vector3::new(0.0, 1.0, 1.0),
            Vector3::new(1.0, 0.0, 0.0),
        ));
        assert!(analytic_procedural_surface(&definition).is_none());
    }

    #[test]
    fn normalized_topology_heads_are_not_other_records() {
        for head in ["subshell", "wire", "tcoedge", "tedge", "tvertex"] {
            assert!(is_known_record_head(head), "{head}");
        }
    }

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

    #[test]
    fn subshell_wires_project_onto_the_nearest_shell() {
        let mut bytes = Vec::new();
        record(&mut bytes, "asmheader", &[]); // 0
        record(&mut bytes, "shell", &[-1, -1, -1, -1, 2, -1, 4, -1]); // 1
        record(&mut bytes, "subshell", &[-1, -1, -1, 1, -1, 3, -1, 5]); // 2
        record(&mut bytes, "subshell", &[-1, -1, -1, 2, -1, -1, -1, 6]); // 3
        record(&mut bytes, "wire", &[]); // 4
        record(&mut bytes, "wire", &[]); // 5
        record(&mut bytes, "wire", &[]); // 6

        let records = crate::sab::frame(&bytes, 0, bytes.len(), 8)
            .expect("generated subshell-wire bytes must frame");
        let by_index = records
            .iter()
            .map(|record| (record.index as i64, record))
            .collect::<HashMap<_, _>>();
        assert_eq!(shell_wire_roots(&records[1], &by_index), [4, 5, 6]);
    }

    #[test]
    fn exact_procedural_pcurve_bypasses_nurbs_cache_parameterization() {
        let records = [
            Record {
                index: 1,
                name: "point".into(),
                head: "point".into(),
                tokens: vec![Token::Position([0.0, 0.0, 0.0])],
                offset: 0,
                len: 0,
            },
            Record {
                index: 2,
                name: "point".into(),
                head: "point".into(),
                tokens: vec![Token::Position([1.0, 0.0, 0.0])],
                offset: 0,
                len: 0,
            },
            Record {
                index: 3,
                name: "vertex".into(),
                head: "vertex".into(),
                tokens: vec![
                    Token::Ref(-1),
                    Token::Long(-1),
                    Token::Ref(-1),
                    Token::Ref(-1),
                    Token::Long(0),
                    Token::Ref(1),
                ],
                offset: 0,
                len: 0,
            },
            Record {
                index: 4,
                name: "vertex".into(),
                head: "vertex".into(),
                tokens: vec![
                    Token::Ref(-1),
                    Token::Long(-1),
                    Token::Ref(-1),
                    Token::Ref(-1),
                    Token::Long(1),
                    Token::Ref(2),
                ],
                offset: 0,
                len: 0,
            },
            Record {
                index: 5,
                name: "edge".into(),
                head: "edge".into(),
                tokens: vec![
                    Token::Ref(-1),
                    Token::Long(-1),
                    Token::Ref(-1),
                    Token::Ref(3),
                    Token::Double(0.0),
                    Token::Ref(4),
                ],
                offset: 0,
                len: 0,
            },
        ];
        let by_index = records
            .iter()
            .map(|record| (record.index as i64, record))
            .collect::<HashMap<_, _>>();
        let cache = SurfaceGeometry::Nurbs(cadmpeg_ir::geometry::NurbsSurface {
            u_degree: 1,
            v_degree: 1,
            u_knots: vec![0.0, 0.0, 1.0, 1.0],
            v_knots: vec![0.0, 0.0, 1.0, 1.0],
            u_count: 2,
            v_count: 2,
            control_points: vec![
                Point3::new(0.0, 0.0, 0.0),
                Point3::new(0.0, 1.0, 0.0),
                Point3::new(1.0, 0.0, 0.0),
                Point3::new(1.0, 1.0, 0.0),
            ],
            weights: None,
            u_periodic: false,
            v_periodic: false,
        });
        let candidate = || nurbs::NurbsPcurve {
            degree: 1,
            knots: vec![0.0, 0.0, 1.0, 1.0],
            control_points: vec![
                cadmpeg_ir::math::Point2::new(10.0, 10.0),
                cadmpeg_ir::math::Point2::new(11.0, 10.0),
            ],
            weights: None,
            periodic: false,
        };

        assert!(select_face_pcurve(
            vec![candidate()],
            Some(&cache),
            false,
            Some(&records[4]),
            &by_index,
        )
        .is_none());
        assert!(select_face_pcurve(
            vec![candidate()],
            Some(&cache),
            true,
            Some(&records[4]),
            &by_index,
        )
        .is_some());
    }

    #[test]
    fn reversed_edge_negates_its_pcurve_validation_interval() {
        let edge = Record {
            index: 1,
            name: "edge".into(),
            head: "edge".into(),
            tokens: vec![
                Token::Ref(-1),
                Token::Long(-1),
                Token::Ref(-1),
                Token::Ref(2),
                Token::Double(0.55),
                Token::Ref(3),
                Token::Double(0.60),
                Token::Ref(-1),
                Token::Ref(4),
                Token::True,
            ],
            offset: 0,
            len: 0,
        };

        assert_eq!(
            edge_pcurve_parameter_ranges(&edge),
            Some([[-0.55, -0.60], [0.55, 0.60]])
        );
        let candidate = nurbs::NurbsPcurve {
            degree: 1,
            knots: vec![0.0, 0.0, 1.0, 1.0],
            control_points: vec![
                cadmpeg_ir::math::Point2::new(0.0, 0.0),
                cadmpeg_ir::math::Point2::new(1.0, 0.0),
            ],
            weights: None,
            periodic: false,
        };
        assert_eq!(
            pcurve_ranges_on_domain(&candidate, Some(&edge)),
            Some(vec![[0.55, 0.60], [0.0, 1.0]])
        );
    }
}
