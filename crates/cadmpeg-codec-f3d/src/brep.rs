// SPDX-License-Identifier: Apache-2.0
//! B-rep topology and analytic geometry decode from a framed SAB `RecordTable`.
//!
//! Given the [`crate::sab`] `RecordTable` of an active model slice, this builds
//! the IR topology graph (`body → lump → shell → face → loop → coedge → edge →
//! vertex → point`) and the analytic surface/curve carriers it references
//! (`plane`, `cone`/cylinder, `sphere`, `torus`, `straight` line,
//! `ellipse`/circle). Fusion `BinaryFile8` model-space lengths are centimetres
//! and are converted to millimetres (×10) at read time; unit vectors, ratios,
//! and angles are not scaled.
//!
//! Free-form spline surfaces and procedural `intcurve` curves are subtype-
//! dispatched constructions this codec does not evaluate, but they carry an
//! inline cached B-spline block ([`crate::nurbs`]) that is decoded into a NURBS
//! carrier where present. A face whose surface cache is a decodable B-spline
//! gets a [`SurfaceGeometry::Nurbs`] carrier; one whose cache is absent or an
//! unparsed procedural form keeps its loops and trims but carries a
//! [`SurfaceGeometry::Unknown`] surface linking to the preserved record bytes
//! ([`UnknownRecord`]). An edge whose 3D curve cache decodes gets a
//! [`CurveGeometry::Nurbs`]; otherwise it is emitted with no attributed curve.
//! What is not understood is reported, never fabricated.

use std::collections::{HashMap, HashSet};

use cadmpeg_ir::attributes::{AttributeTarget, AttributeValue, SourceAttribute};
use cadmpeg_ir::design::{PersistentDesignLink, SketchCurveLink};
use cadmpeg_ir::geometry::{
    BlendSupports, Curve, CurveGeometry, Pcurve, PcurveGeometry, ProceduralCurve,
    ProceduralSurface, Surface, SurfaceGeometry, SurfaceParameterization,
};
use cadmpeg_ir::ids::{
    AttributeId, BodyId, CoedgeId, CurveId, EdgeId, FaceId, LoopId, LumpId, PcurveId, PointId,
    ShellId, SurfaceId, UnknownId, VertexId,
};
use cadmpeg_ir::math::{Point3, Vector3};
use cadmpeg_ir::provenance::{EntityMeta, Exactness, Provenance};
use cadmpeg_ir::topology::{
    Body, Coedge, Color, Edge, Face, Loop, Lump, Point, Sense, Shell, Vertex,
};
use cadmpeg_ir::unknown::UnknownRecord;

use crate::asm_header;
use crate::nurbs;
use crate::sab::{Record, Token};

/// Millimetres per ASM model-space length unit (centimetres).
const LEN_TO_MM: f64 = 10.0;

/// The decoded B-rep graph plus loss accounting.
#[derive(Default)]
pub struct Brep {
    /// Bodies.
    pub bodies: Vec<Body>,
    /// Lumps.
    pub lumps: Vec<Lump>,
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
    /// Native analytic surface parameter frames.
    pub surface_parameterizations: Vec<SurfaceParameterization>,
    /// Typed sketch-curve provenance links.
    pub sketch_curve_links: Vec<SketchCurveLink>,
    /// Persistent design identifiers attached to solved entities.
    pub persistent_design_links: Vec<PersistentDesignLink>,
    /// Native ASM body key by emitted body id, used by Design-side joins.
    pub body_keys: HashMap<BodyId, u64>,
    /// Linked source-native attributes.
    pub attributes: Vec<SourceAttribute>,
    /// Undecoded carrier records preserved verbatim.
    pub unknowns: Vec<UnknownRecord>,
    /// Loss accounting for the report.
    pub stats: Stats,
}

/// Counts of what could not be transferred faithfully.
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
    /// Coedges that carried an explicit UV pcurve ref whose carrier could not
    /// be decoded.
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
    matches!(head, "straight" | "ellipse")
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
                    u_axis: Some(u_axis),
                },
                false,
            ))
        }
        "cone" => {
            let axis = *c.vectors.first()?;
            let axis = unit(axis);
            let major = c.vectors.get(1).copied();
            // Doubles are (ratio, sine, cosine, r1). `ratio` (minor/major of an
            // elliptical cone) is not modeled by the IR's circular cone carrier;
            // all corpus cones are circular (ratio 1). `sine` selects cylinder
            // vs cone; `r1` is the explicit base radius.
            let sine = *c.doubles.get(1).unwrap_or(&0.0);
            let r1 = c.doubles.get(3).copied();
            let radius = r1
                .map(|r| r * LEN_TO_MM)
                .or_else(|| major.map(|vector| norm3(vector) * LEN_TO_MM))?;
            let ref_direction = major.map_or_else(|| deterministic_ref_direction(axis), unit);
            if sine.abs() <= f64::EPSILON {
                Some((
                    SurfaceGeometry::Cylinder {
                        origin: scale_point(origin),
                        axis,
                        ref_direction: Some(ref_direction),
                        radius,
                    },
                    false,
                ))
            } else {
                Some((
                    SurfaceGeometry::Cone {
                        origin: scale_point(origin),
                        axis,
                        ref_direction: Some(ref_direction),
                        radius,
                        half_angle: sine.abs().asin(),
                    },
                    false,
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
                    axis: Some(polar_axis),
                    ref_direction: Some(equator),
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
                    ref_direction: Some(ref_direction),
                    major_radius: major * LEN_TO_MM,
                    minor_radius: minor * LEN_TO_MM,
                },
                false,
            ))
        }
        _ => None,
    }
}

fn cross(a: Vector3, b: Vector3) -> Vector3 {
    Vector3::new(
        a.y * b.z - a.z * b.y,
        a.z * b.x - a.x * b.z,
        a.x * b.y - a.y * b.x,
    )
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

fn decode_surface_parameterization(
    rec: &Record,
    surface: SurfaceId,
    meta: EntityMeta,
) -> Option<SurfaceParameterization> {
    let carrier = collect_carrier(rec);
    let origin = scale_point(*carrier.positions.first()?);
    let (u_reference, v_reference) = match rec.head.as_str() {
        "plane" => {
            let normal = unit(*carrier.vectors.first()?);
            let u = unit(*carrier.vectors.get(1)?);
            (u, cross(normal, u))
        }
        "cone" => (
            unit(*carrier.vectors.get(1)?),
            unit(*carrier.vectors.first()?),
        ),
        "sphere" => (
            unit(*carrier.vectors.first()?),
            unit(*carrier.vectors.get(1)?),
        ),
        "torus" => (
            unit(*carrier.vectors.get(1)?),
            unit(*carrier.vectors.first()?),
        ),
        _ => return None,
    };
    Some(SurfaceParameterization {
        surface,
        origin,
        u_reference,
        v_reference,
        meta,
    })
}

/// Decode an analytic curve carrier.
pub(crate) fn decode_curve(rec: &Record) -> Option<CurveGeometry> {
    let c = collect_carrier(rec);
    let base = *c.positions.first()?;
    match rec.head.as_str() {
        "straight" => {
            let dir = *c.vectors.first()?;
            Some(CurveGeometry::Line {
                origin: scale_point(base),
                direction: unit(dir),
            })
        }
        "ellipse" => {
            let axis = *c.vectors.first()?;
            let refv = *c.vectors.get(1)?;
            let ratio = *c.doubles.first()?;
            let r_major = norm3(refv) * LEN_TO_MM;
            if (ratio.abs() - 1.0).abs() <= f64::EPSILON {
                Some(CurveGeometry::Circle {
                    center: scale_point(base),
                    axis: unit(axis),
                    radius: r_major,
                })
            } else {
                Some(CurveGeometry::Ellipse {
                    center: scale_point(base),
                    axis: unit(axis),
                    major_direction: unit(refv),
                    major_radius: r_major,
                    minor_radius: r_major * ratio.abs(),
                })
            }
        }
        _ => None,
    }
}

// ---- topology record views ---------------------------------------------------

fn sense_at(rec: &Record, i: usize) -> Sense {
    match rec.chunk(i) {
        Some(Token::True) => Sense::Reversed,
        _ => Sense::Forward,
    }
}

fn double_at(rec: &Record, i: usize) -> Option<f64> {
    match rec.chunk(i) {
        Some(Token::Double(d)) => Some(*d),
        _ => None,
    }
}

/// Decode a framed active slice into the IR B-rep graph.
///
/// `stream` names the source ZIP entry for provenance. Ids are minted as
/// `f3d#<record-index>`, unique across the `RecordTable`.
pub fn decode(records: &[Record], bytes: &[u8], stream: &str) -> Brep {
    let mut out = Brep::default();

    let id = |i: i64| format!("f3d#{i}");
    let meta = |rec: &Record| EntityMeta {
        provenance: Provenance {
            format: "f3d".to_string(),
            stream: stream.to_string(),
            offset: rec.offset as u64,
            tag: Some(rec.name.clone()),
        },
        exactness: Exactness::ByteExact,
    };

    // Index records by RecordTable index (== position for a framed slice).
    let by_index: HashMap<i64, &Record> = records.iter().map(|r| (r.index as i64, r)).collect();
    let header_scale = asm_header::parse(bytes)
        .and_then(|header| header.scale)
        .unwrap_or(1.0);

    let attribute_color = |entity: &Record| attribute_chain_color(entity, &by_index);

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
        // A non-analytic surface may still carry a decodable B-spline face cache.
        if let std::collections::hash_map::Entry::Vacant(e) = surface_geo.entry(surf_ref) {
            if let Some(ns) =
                nurbs::decode_surface_cache_resolving_refs(record_slice(surf_rec, bytes), bytes)
            {
                e.insert((SurfaceGeometry::Nurbs(ns), false));
                out.stats.nurbs_surfaces += 1;
                if let Some(procedural) = nurbs::decode_procedural_surface_resolving_refs(
                    record_slice(surf_rec, bytes),
                    bytes,
                ) {
                    procedural_surface_defs.insert(surf_ref, procedural);
                }
            }
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
                    if ce.head != "coedge" {
                        break;
                    }
                    kept_coedges.insert(ci);
                    if let Some(pc) = ce.ref_at(10) {
                        if let Some(prec) = by_index.get(&pc) {
                            let decoded = nurbs::decode_pcurve_cache_resolving_refs(
                                record_slice(prec, bytes),
                                bytes,
                            )
                            .or_else(|| {
                                prec.ref_at(4)
                                    .and_then(|reference| by_index.get(&reference))
                                    .and_then(|intcurve| {
                                        nurbs::decode_intcurve_pcurve_cache_resolving_refs(
                                            record_slice(intcurve, bytes),
                                            bytes,
                                        )
                                    })
                            });
                            if let Some(decoded) = decoded {
                                pcurve_geo.insert(
                                    pc,
                                    PcurveGeometry::Nurbs {
                                        degree: decoded.degree,
                                        knots: decoded.knots,
                                        control_points: decoded.control_points,
                                        weights: None,
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
                            if edge.head == "edge" && kept_edges.insert(ei) {
                                for slot in [3usize, 5] {
                                    if let Some(vi) = edge.ref_at(slot) {
                                        if let Some(v) = by_index.get(&vi) {
                                            if v.head == "vertex" {
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
                                                )
                                            {
                                                curve_geo.insert(
                                                    cv,
                                                    CurveGeometry::Nurbs(decoded.curve),
                                                );
                                                procedural_curve_defs.insert(
                                                    cv,
                                                    (
                                                        decoded.native_kind,
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
                    meta: meta(r),
                });
                if is_analytic_surface(&r.head) {
                    if let Some(parameterization) =
                        decode_surface_parameterization(r, SurfaceId(id(i)), meta(r))
                    {
                        out.surface_parameterizations.push(parameterization);
                    }
                }
                if let Some(procedural) = procedural_surface_defs.remove(&i) {
                    if matches!(
                        &procedural.definition,
                        cadmpeg_ir::geometry::ProceduralSurfaceDefinition::RollingBallBlend {
                            supports: BlendSupports::Partial(_),
                            ..
                        }
                    ) {
                        out.stats.partial_procedural_supports += 1;
                    }
                    out.procedural_surfaces.push(ProceduralSurface {
                        surface: SurfaceId(id(i)),
                        definition: procedural.definition,
                        cache_fit_tolerance: procedural.cache_fit_tolerance,
                        meta: meta(r),
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
                    meta: EntityMeta {
                        provenance: Provenance {
                            format: "f3d".to_string(),
                            stream: stream.to_string(),
                            offset: r.offset as u64,
                            tag: Some(r.name.clone()),
                        },
                        exactness: Exactness::Unknown,
                    },
                });
            }
            _ if kept_curves.contains(&i) => {
                let Some(geometry) = curve_geo.remove(&i) else {
                    continue;
                };
                out.curves.push(Curve {
                    id: CurveId(id(i)),
                    geometry,
                    meta: meta(r),
                });
                if let Some(procedural) = procedural_curve_defs.remove(&i) {
                    out.procedural_curves.push(ProceduralCurve {
                        curve: CurveId(id(i)),
                        native_kind: procedural.0,
                        cache_fit_tolerance: procedural.1,
                        meta: meta(r),
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
                    meta: meta(r),
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
                    meta: meta(r),
                });
            }
        }
    }

    for r in records {
        let i = r.index as i64;
        if r.head == "vertex" && kept_vertices.contains(&i) {
            if let Some(pi) = r.ref_at(5) {
                if kept_points.contains(&pi) {
                    out.vertices.push(Vertex {
                        id: VertexId(id(i)),
                        point: PointId(id(pi)),
                        tolerance: None,
                        meta: meta(r),
                    });
                }
            }
        }
    }

    for r in records {
        let i = r.index as i64;
        if r.head == "edge" && kept_edges.contains(&i) {
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
                            let carrier = collect_carrier(curve_record);
                            let ratio = carrier.doubles.first().copied().unwrap_or(1.0);
                            if ratio > 0.0 {
                                a += std::f64::consts::FRAC_PI_2;
                                b += std::f64::consts::FRAC_PI_2;
                            }
                            if (b - a).abs() >= std::f64::consts::TAU - 1.0e-12 {
                                a = 0.0;
                                b = std::f64::consts::TAU;
                            }
                        }
                    }
                    Some([a, b])
                }
                _ => None,
            };
            out.edges.push(Edge {
                id: EdgeId(id(i)),
                curve: curve.map(|c| CurveId(id(c))),
                start: VertexId(id(start)),
                end: VertexId(id(end)),
                param_range,
                tolerance: None,
                meta: meta(r),
            });
        }
    }

    for r in records {
        let i = r.index as i64;
        if r.head == "coedge" && kept_coedges.contains(&i) {
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
                partner: partner.map(|p| CoedgeId(id(p))),
                radial_next: partner.map(|p| CoedgeId(id(p))),
                sense: sense_at(r, 7),
                pcurve: r
                    .ref_at(10)
                    .filter(|p| kept_pcurves.contains(p))
                    .map(|p| PcurveId(id(p))),
                meta: meta(r),
            });
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
                meta: meta(r),
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
            out.faces.push(Face {
                id: FaceId(id(i)),
                shell: ShellId(id(owner)),
                surface: SurfaceId(id(surface)),
                sense: sense_at(r, 8),
                loops,
                name: None,
                color: attribute_color(r),
                tolerance: None,
                meta: meta(r),
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
                let faces = face_chain(r, &by_index, &kept_faces);
                out.shells.push(Shell {
                    id: ShellId(id(i)),
                    lump: LumpId(id(owner)),
                    faces,
                    wire_edges: Vec::new(),
                    free_vertices: Vec::new(),
                    meta: meta(r),
                });
            }
            "lump" => {
                let Some(owner) = r.ref_at(5) else { continue };
                let shells = shell_chain(r, &by_index);
                out.lumps.push(Lump {
                    id: LumpId(id(i)),
                    body: BodyId(id(owner)),
                    shells,
                    meta: meta(r),
                });
            }
            "body" => {
                let lumps = lump_chain(r, &by_index);
                let body_id = BodyId(id(i));
                if let Some(Token::Long(key)) = r.chunk(1) {
                    if *key >= 0 {
                        out.body_keys.insert(body_id.clone(), *key as u64);
                    }
                }
                out.bodies.push(Body {
                    id: body_id,
                    kind: cadmpeg_ir::topology::BodyKind::Solid,
                    lumps,
                    transform: r
                        .ref_at(5)
                        .and_then(|reference| by_index.get(&reference))
                        .and_then(|transform| decode_transform(transform, header_scale)),
                    name: None,
                    color: attribute_color(r),
                    meta: meta(r),
                });
            }
            _ => {}
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
            "coedge" if kept_coedges.contains(&index) => {
                Some(AttributeTarget::Coedge(CoedgeId(id(index))))
            }
            "edge" if kept_edges.contains(&index) => Some(AttributeTarget::Edge(EdgeId(id(index)))),
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
                stream,
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
            "coedge" if kept_coedges.contains(&owner_index) => {
                Some(AttributeTarget::Coedge(CoedgeId(id(owner_index))))
            }
            "edge" if kept_edges.contains(&owner_index) => {
                Some(AttributeTarget::Edge(EdgeId(id(owner_index))))
            }
            "vertex" if kept_vertices.contains(&owner_index) => {
                Some(AttributeTarget::Vertex(VertexId(id(owner_index))))
            }
            _ => None,
        };
        if let Some(target) = target {
            emitted_attributes.insert(index);
            out.attributes
                .push(source_attribute(record, target, stream));
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
                meta: EntityMeta {
                    provenance: Provenance {
                        format: "f3d".to_string(),
                        stream: stream.to_string(),
                        offset: r.offset as u64,
                        tag: Some(r.name.clone()),
                    },
                    exactness: Exactness::Unknown,
                },
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
            "body" | "lump" | "shell" | "face" | "loop" | "coedge" | "edge" | "vertex" | "point"
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

    out
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
        coedge: coedge.clone(),
        sketch_curve_id: *sketch_curve_id,
        signed_reference: (*signed_reference != -1).then_some(*signed_reference),
        role: *role,
        closure: *closure,
        meta: attribute.meta.clone(),
    })
}

fn persistent_design_links(attribute: &SourceAttribute) -> Vec<PersistentDesignLink> {
    let Some(family) = attribute.values.iter().position(
        |value| matches!(value, AttributeValue::String(name) if name == "generic_tag_attrib_def"),
    ) else {
        return Vec::new();
    };
    let ids: Vec<String> = attribute.values[family + 1..]
        .iter()
        .filter_map(|value| match value {
            AttributeValue::String(value)
                if value.trim() != "generic_tag_attrib_def"
                    && !value.is_empty()
                    && value.bytes().all(|byte| byte.is_ascii_digit()) =>
            {
                Some(value.clone())
            }
            _ => None,
        })
        .collect();
    let last = ids.len().saturating_sub(1);
    ids.into_iter()
        .enumerate()
        .map(|(ordinal, design_id)| PersistentDesignLink {
            target: attribute.target.clone(),
            design_id,
            ordinal: ordinal as u32,
            is_current: ordinal == last,
            meta: attribute.meta.clone(),
        })
        .collect()
}

fn collect_attributes(
    entity: &Record,
    target: &AttributeTarget,
    by_index: &HashMap<i64, &Record>,
    stream: &str,
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
            out.push(source_attribute(record, target.clone(), stream));
        }
        current = record.ref_at(0);
    }
}

fn source_attribute(record: &Record, target: AttributeTarget, stream: &str) -> SourceAttribute {
    SourceAttribute {
        id: AttributeId(format!("f3d:attribute#{}", record.index)),
        target,
        name: record.name.clone(),
        values: record.tokens.iter().map(attribute_value).collect(),
        meta: EntityMeta {
            provenance: Provenance {
                format: "f3d".into(),
                stream: stream.into(),
                offset: record.offset as u64,
                tag: Some(record.name.clone()),
            },
            exactness: Exactness::ByteExact,
        },
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
        Token::Ref(value) => AttributeValue::Reference(format!("f3d#{value}")),
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

/// The raw bytes of a record within the decompressed stream.
fn record_slice<'a>(rec: &Record, bytes: &'a [u8]) -> &'a [u8] {
    let end = (rec.offset + rec.len).min(bytes.len());
    &bytes[rec.offset..end]
}

/// The `UnknownId` for a preserved carrier record. Shared by the passthrough
/// `UnknownRecord` and any `SurfaceGeometry::Unknown` that links to it, so the
/// reference resolves under validation.
fn unknown_record_id(rec: &Record) -> String {
    format!("f3d:{}#{}", rec.head, rec.index)
}

fn ring_coedges(
    loop_rec: &Record,
    by_index: &HashMap<i64, &Record>,
    kept: &HashSet<i64>,
) -> Vec<CoedgeId> {
    let id = |i: i64| CoedgeId(format!("f3d#{i}"));
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
    let id = |i: i64| LoopId(format!("f3d#{i}"));
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
    let id = |i: i64| FaceId(format!("f3d#{i}"));
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

fn shell_chain(lump_rec: &Record, by_index: &HashMap<i64, &Record>) -> Vec<ShellId> {
    let id = |i: i64| ShellId(format!("f3d#{i}"));
    let mut out = Vec::new();
    let mut cur = lump_rec.ref_at(4);
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

fn lump_chain(body_rec: &Record, by_index: &HashMap<i64, &Record>) -> Vec<LumpId> {
    let id = |i: i64| LumpId(format!("f3d#{i}"));
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

fn sha256_hex(bytes: &[u8]) -> String {
    use std::fmt::Write as _;

    use sha2::{Digest, Sha256};
    let mut h = Sha256::new();
    h.update(bytes);
    let digest = h.finalize();
    let mut s = String::with_capacity(digest.len() * 2);
    for b in digest {
        let _ = write!(s, "{b:02x}");
    }
    s
}
