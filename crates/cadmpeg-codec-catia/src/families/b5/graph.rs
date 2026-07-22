// SPDX-License-Identifier: Apache-2.0
//! Object-id topology in the CATIA `b5 03` short-frame family.

use std::collections::{BTreeMap, HashMap, HashSet};

use cadmpeg_ir::eval::{nurbs_pcurve_uv, nurbs_surface_point};
use cadmpeg_ir::geometry::{NurbsSurface, ProceduralSurfaceDefinition, SurfaceGeometry};
use cadmpeg_ir::le::f64_at;
use cadmpeg_ir::math::Point2;

use super::vecmath::{add, cross, scale};
use crate::wire;

/// Resolved `b5 03` object-stream topology graph: faces, loops, pcurves, and
/// surfaces bound through the in-stream `object_id` map ([spec §6.6](https://github.com/cadmpeg/cadmpeg/blob/main/docs/formats/catia.md#66-object-stream-topology-b5-03)),
/// together with the `05 08 01` vertex points used to bind edge endpoints.
#[derive(Debug, Clone, PartialEq)]
pub struct B5Graph {
    /// `true` when every serialized face and loop node belongs to the resolved
    /// reference-closed graph; `false` when the graph is its maximal closed
    /// subset.
    pub complete: bool,
    /// `b5 03 5f` face nodes, in stream declaration order (equal to STEP
    /// `ADVANCED_FACE` order, [spec §6.6](https://github.com/cadmpeg/cadmpeg/blob/main/docs/formats/catia.md#66-object-stream-topology-b5-03)).
    pub faces: Vec<B5Face>,
    /// `b5 03 62` loop nodes, keyed by `object_id`.
    pub loops: BTreeMap<u32, B5Loop>,
    /// `b5 03 21` pcurve nodes, keyed by `object_id`.
    pub pcurves: BTreeMap<u32, B5Pcurve>,
    /// Structurally bounded pcurve records whose parameter-space geometry is
    /// not yet assigned, keyed by `object_id`.
    pub opaque_pcurves: BTreeMap<u32, B5OpaquePcurve>,
    /// Pcurve occurrence ids whose support is bound by a loop and both native
    /// edge-endpoint incidence records, but which have no standalone geometry
    /// record.
    pub implicit_pcurves: BTreeMap<u32, u32>,
    /// `b5 03 27/28/2d` analytic surface nodes and `a8 03 34` NURBS
    /// surfaces, keyed by `object_id`.
    pub surfaces: BTreeMap<u32, B5Surface>,
    /// `b5 03 30` offset constructions, keyed by their result surface id.
    pub offset_surfaces: BTreeMap<u32, B5OffsetSurface>,
    /// `b5 03 2c` extrusion constructions, keyed by their result surface id.
    pub extrusion_surfaces: BTreeMap<u32, B5ExtrusionSurface>,
    /// `b5 03 37/3b` support-bound constructions, keyed by result surface id.
    pub supported_surfaces: BTreeMap<u32, B5SupportedSurface>,
    /// Native class-`06` curve-parameter incidences, keyed by object id.
    pub parameter_incidences: BTreeMap<u32, B5ParameterIncidence>,
    /// World-frame `05 08 01` vertex coordinates, in stream order.
    pub vertex_points: Vec<[f64; 3]>,
    /// Logical vertex coordinates resolved from native `5d` identity. Their
    /// edge indices follow the raw `vertex_points` indices.
    pub logical_vertex_points: Vec<[f64; 3]>,
    /// Native `5d` object ids aligned with `logical_vertex_points`.
    pub logical_vertex_refs: Vec<u32>,
    /// Per-edge pair of vertex indices. Raw `vertex_points` occupy the first
    /// index range; native `5d` logical vertices occupy the following range.
    pub edge_vertices: BTreeMap<u32, [usize; 2]>,
    /// Ordered class-`06` start/end parameter-incidence references from each
    /// native class-`5e` edge.
    pub edge_parameter_incidences: BTreeMap<u32, [u32; 2]>,
    /// Maximum incident endpoint residual for each logical vertex, keyed by
    /// the combined vertex index used by `edge_vertices`.
    pub vertex_tolerances: BTreeMap<usize, f64>,
    /// `b5 03 0e`/`0f` line and arc profile curves, keyed by `object_id`;
    /// referenced by `B5Surface::Revolution::profile_curve`.
    pub profiles: BTreeMap<u32, B5Profile>,
}

/// A profile curve swept by a `b5 03 2d` surface of revolution.
#[derive(Debug, Clone, PartialEq)]
pub enum B5Profile {
    /// `b5 03 0e`: a line through `point` along `direction`.
    Line {
        /// A point on the line.
        point: [f64; 3],
        /// Unit direction of the line.
        direction: [f64; 3],
    },
    /// `b5 03 0f`: an arc with a positive radius.
    Arc {
        /// Arc center.
        center: [f64; 3],
        /// Unit vector from `center` toward the zero-angle point.
        direction_x: [f64; 3],
        /// Unit vector orthogonal to `direction_x` completing the arc
        /// plane's basis.
        direction_y: [f64; 3],
        /// Positive arc radius.
        radius: f64,
    },
}

/// A resolved `b5 03` surface node ([spec §6.6](https://github.com/cadmpeg/cadmpeg/blob/main/docs/formats/catia.md#66-object-stream-topology-b5-03)).
#[derive(Debug, Clone, PartialEq)]
pub enum B5Surface {
    /// A NURBS surface whose parameter lattice is decoded but whose pole
    /// representation remains opaque.
    UnresolvedNurbs {
        /// Decoded degree, knot, multiplicity, and pole-cardinality fields.
        header: crate::families::a5a8::records::A8SurfaceHeader,
        /// Exact source payload, including the opaque pole representation.
        payload: Vec<u8>,
    },
    /// An identity-bearing surface record whose carrier geometry remains opaque.
    Unknown {
        /// Source record family.
        family: u8,
        /// Source surface class.
        class: u8,
        /// Exact source payload.
        payload: Vec<u8>,
    },
    /// `b5 03 27`: a plane spanned by `origin`, `direction_u`, and
    /// `direction_v`.
    Plane {
        /// A point on the plane.
        origin: [f64; 3],
        /// First in-plane basis direction, as stored (not necessarily
        /// unit).
        direction_u: [f64; 3],
        /// Second in-plane basis direction, as stored (not necessarily
        /// unit).
        direction_v: [f64; 3],
    },
    /// `b5 03 28`: a cylinder with a positive radius.
    Cylinder {
        /// A point on the cylinder axis.
        origin: [f64; 3],
        /// Unit reference direction orthogonal to `axis`, the zero-angle
        /// ray.
        reference_x: [f64; 3],
        /// Unit cylinder axis, `reference_x × stored_v` normalized.
        axis: [f64; 3],
        /// Positive cylinder radius.
        radius: f64,
    },
    /// `b5 03 29`: a circular cone in its native arc-length/slant chart.
    Cone {
        /// Cone apex.
        apex: [f64; 3],
        /// First transverse unit direction.
        direction_x: [f64; 3],
        /// Second transverse unit direction.
        direction_y: [f64; 3],
        /// Cone-axis unit direction.
        axis: [f64; 3],
        /// Cone half-angle in radians.
        half_angle: f64,
        /// Stored angular-origin offset.
        angular_offset: f64,
        /// Native slant-coordinate range.
        slant_range: [f64; 2],
        /// Divisor mapping native U to azimuth.
        angular_scale: f64,
    },
    /// `b5 03 2b`: a torus in its two arc-length angular coordinates.
    Torus {
        /// Torus center.
        center: [f64; 3],
        /// Zero-major-angle direction.
        direction_x: [f64; 3],
        /// Quarter-turn major-angle direction.
        direction_y: [f64; 3],
        /// Torus axis.
        axis: [f64; 3],
        /// Major radius.
        major_radius: f64,
        /// Minor radius.
        minor_radius: f64,
        /// Divisor mapping native U to the major angle.
        major_scale: f64,
        /// Divisor mapping native V to the minor angle.
        minor_scale: f64,
    },
    /// `b5 03 2d`: a surface of revolution sweeping `profile_curve` about
    /// `axis_origin`/`axis_direction`.
    Revolution {
        /// `object_id` of the swept [`B5Profile`].
        profile_curve: u32,
        /// A point on the revolution axis.
        axis_origin: [f64; 3],
        /// Unit revolution axis.
        axis_direction: [f64; 3],
        /// Nonzero scale mapping a pcurve's `v` parameter to a revolution
        /// angle in radians (`angle = v / gauge_radius`).
        gauge_radius: f64,
    },
    /// An `a8 03 34` inline-pole B-spline surface, resolved through
    /// [`crate::families::a5a8::records::a8_surfaces`] and merged into the same
    /// `object_id` namespace.
    Nurbs(NurbsSurface),
    /// An `a8 03 32` rolling-ball result carrier, resolved through its exact
    /// stored value and derivative jet.
    RollingBall {
        /// Persistent object id of the `a8 03 32` result carrier.
        carrier_object_id: u32,
        /// Exact procedural definition decoded from the stored jet.
        definition: Box<ProceduralSurfaceDefinition>,
    },
}

/// A `b5 03 30` offset construction with an explicit result carrier.
#[derive(Debug, Clone, PartialEq)]
pub struct B5OffsetSurface {
    /// This construction's result surface id.
    pub object_id: u32,
    /// Explicit analytic carrier for the offset result.
    pub carrier_surface: u32,
    /// Surface from which the result is offset.
    pub source_surface: u32,
    /// Signed offset distance in millimetres.
    pub distance: f64,
    /// Native carrier-kind discriminator.
    pub carrier_kind: u8,
    /// Ordered native U and V bounds.
    pub parameter_bounds: [[f64; 2]; 2],
}

/// A `b5 03 2c` extrusion construction with a two-support directrix.
#[derive(Debug, Clone, PartialEq)]
pub struct B5ExtrusionSurface {
    /// This construction's result surface id.
    pub object_id: u32,
    /// Unit world-space extrusion direction.
    pub direction: [f64; 3],
    /// Increasing native U and V intervals.
    pub parameter_bounds: [[f64; 2]; 2],
    /// Exact two-support directrix construction.
    pub directrix: B5ExtrusionDirectrix,
}

/// Two pcurve supports and solved range of an `a8 03 25` directrix.
#[derive(Debug, Clone, PartialEq)]
pub struct B5ExtrusionDirectrix {
    /// Persistent directrix object id.
    pub object_id: u32,
    /// Ordered `(surface, pcurve, pcurve range)` support sides.
    pub supports: [(u32, u32, [f64; 2]); 2],
    /// Increasing solved-curve parameter range.
    pub parameter_range: [f64; 2],
    /// Positive fit tolerance of the serialized sampled cache.
    pub cache_fit_tolerance: f64,
}

/// A support-bound surface construction with an explicit result carrier.
#[derive(Debug, Clone, PartialEq)]
pub struct B5SupportedSurface {
    /// This construction's result surface id.
    pub object_id: u32,
    /// Native construction class (`0x37` or `0x3b`).
    pub class: u8,
    /// Explicit carrier for the result geometry and chart.
    pub carrier_surface: u32,
    /// Ordered construction support surfaces.
    pub support_surfaces: [u32; 2],
    /// Ordered pcurves, one bound to each support surface.
    pub support_pcurves: [u32; 2],
    /// Six native control bytes surrounding the scalar fields.
    pub controls: [u8; 6],
    /// Two finite native construction scalars.
    pub scalars: [f64; 2],
}

/// One class-`06` incidence lane connecting curves to parameters at a vertex.
#[derive(Debug, Clone, PartialEq)]
pub struct B5ParameterIncidence {
    /// This record's stream object id.
    pub object_id: u32,
    /// Ordered curve or pcurve references.
    pub curves: Vec<u32>,
    /// Finite native parameters aligned with `curves`.
    pub parameters: Vec<f64>,
    /// Compact native controls aligned with `curves`.
    pub controls: Vec<u32>,
}

/// A resolved `b5 03 18`, `b5 03 19`, or `b5 03 21` pcurve node, represented as a 2D
/// B-spline curve in a surface's
/// parameter space ([spec §6.6](https://github.com/cadmpeg/cadmpeg/blob/main/docs/formats/catia.md#66-object-stream-topology-b5-03)).
#[derive(Debug, Clone, PartialEq)]
pub struct B5Pcurve {
    /// This record's stream `object_id`.
    pub object_id: u32,
    /// `object_id` of the owning surface, taken directly from the pcurve's
    /// `catia_support_ref` ([spec §6.6](https://github.com/cadmpeg/cadmpeg/blob/main/docs/formats/catia.md#66-object-stream-topology-b5-03)).
    pub surface: u32,
    /// B-spline degree.
    pub degree: u32,
    /// Distinct knot values, strictly increasing.
    pub distinct_knots: Vec<f64>,
    /// Per-knot multiplicities, index-aligned with `distinct_knots`.
    pub multiplicities: Vec<u32>,
    /// `(u, v)` control points in the surface's parameter space.
    pub control_points: Vec<[f64; 2]>,
    /// Per-pole rational weights. `None` denotes a polynomial pcurve.
    pub weights: Option<Vec<f64>>,
    /// The curve's two clamped-end poles lifted through `surface` into
    /// world-frame 3D points, or `None` before [`parse`] resolves them or
    /// when the lift fails (unresolved surface, degenerate revolution
    /// scale, or NURBS evaluation failure).
    pub lifted_endpoints: Option<[[f64; 3]; 2]>,
}

/// An identity- and support-resolved pcurve with opaque chart geometry.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct B5OpaquePcurve {
    /// This record's stream `object_id`.
    pub object_id: u32,
    /// Owning surface object id.
    pub surface: u32,
    /// Native pcurve class.
    pub class: u8,
    /// Exact source payload.
    pub payload: Vec<u8>,
}

/// One length-framed `b5 03` record as found by the stream walk ([spec §6](https://github.com/cadmpeg/cadmpeg/blob/main/docs/formats/catia.md#6-object-stream-record-framing-a5-03-a8-03-b5-03)).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct B5Record {
    /// Byte offset of the `b5 03` marker in the source stream.
    pub offset: usize,
    /// Record family byte (`0xb5` or `0xa8`).
    pub family: u8,
    /// Third header byte: the record's type/class code (`0x5f` face,
    /// `0x62` loop, `0x21` pcurve, `0x27`/`0x28`/`0x2d` surface, `0x5e`
    /// edge, `0x18` line pcurve, `0x0e`/`0x0f` profile, ...).
    pub class: u8,
    /// Dense creation-order `object_id` stored inline at `+4` ([spec §6.5](https://github.com/cadmpeg/cadmpeg/blob/main/docs/formats/catia.md#65-a8-03-common-object-stream-freeform-class)).
    pub object_id: u32,
    /// Raw record payload after the 8-byte header.
    pub payload: Vec<u8>,
}

#[derive(Clone, Copy)]
struct ObjectFrame {
    start: usize,
    end: usize,
    family: u8,
    class: u8,
    object_id: u32,
}

/// A resolved `b5 03 5f` face node ([spec §6.6](https://github.com/cadmpeg/cadmpeg/blob/main/docs/formats/catia.md#66-object-stream-topology-b5-03)).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct B5Face {
    /// This record's stream `object_id`.
    pub object_id: u32,
    /// `object_id` of the face's surface, taken from the first reference
    /// token.
    pub surface: u32,
    /// `object_id`s of the face's `b5 03 62` loop nodes, in reference
    /// order.
    pub loops: Vec<u32>,
}

/// A resolved `b5 03 62` loop node: payload `<0x80 + n_refs>
/// (pcurve_ref edge_ref)* surface_ref` ([spec §6.6](https://github.com/cadmpeg/cadmpeg/blob/main/docs/formats/catia.md#66-object-stream-topology-b5-03)).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct B5Loop {
    /// This record's stream `object_id`.
    pub object_id: u32,
    /// `object_id`s of the loop's member pcurves (or `0x18` lines), in
    /// serialized order.
    pub pcurves: Vec<u32>,
    /// `object_id`s of the loop's member `b5 03 5e` edges, index-aligned
    /// with `pcurves`.
    pub edges: Vec<u32>,
    /// `object_id` of the loop's surface (the trailing reference token).
    pub surface: u32,
}

/// Resolve the dominant object-stream topology graph through inline object ids.
#[must_use]
pub fn parse(bytes: &[u8]) -> Option<B5Graph> {
    let records = records(bytes);
    let by_id: HashMap<u32, &B5Record> = records
        .iter()
        .map(|record| (record.object_id, record))
        .collect();
    if records.is_empty() || by_id.len() != records.len() {
        return None;
    }
    let a8_headers: BTreeMap<u32, crate::families::a5a8::records::A8SurfaceHeader> =
        crate::families::a5a8::records::a8_surface_headers(bytes)
            .into_iter()
            .map(|header| (header.object_id, header))
            .collect();
    let mut surfaces: BTreeMap<u32, B5Surface> = records
        .iter()
        .filter_map(|record| {
            surface_node(record, a8_headers.get(&record.object_id))
                .map(|surface| (record.object_id, surface))
        })
        .collect();
    for surface_id in topology_surface_references(&records) {
        if surfaces.contains_key(&surface_id) {
            continue;
        }
        let Some(record) = by_id
            .get(&surface_id)
            .filter(|record| is_opaque_surface_class(record.class))
        else {
            continue;
        };
        surfaces.insert(
            surface_id,
            B5Surface::Unknown {
                family: record.family,
                class: record.class,
                payload: record.payload.clone(),
            },
        );
    }
    for surface in crate::families::a5a8::records::resolved_a8_surfaces(bytes) {
        if let SurfaceGeometry::Nurbs(nurbs) = surface.geometry {
            surfaces.insert(surface.object_id, B5Surface::Nurbs(nurbs));
        }
    }
    for jet in crate::families::a5a8::records::a8_freeform_curves(bytes) {
        if let Some(definition) = crate::families::a5a8::records::rolling_ball_jet_definition(&jet)
        {
            surfaces.insert(
                jet.object_id,
                B5Surface::RollingBall {
                    carrier_object_id: jet.object_id,
                    definition: Box::new(definition),
                },
            );
        }
    }
    for record in &records {
        let Some(target) = surface_alias_target(record) else {
            continue;
        };
        if let Some(surface) = surfaces.get(&target).cloned() {
            surfaces.insert(record.object_id, surface);
        }
    }
    let object_stream_pcurves = crate::families::a5a8::records::object_stream_pcurves(bytes)
        .into_iter()
        .map(|pcurve| (pcurve.object_id, (pcurve.support_id, pcurve.range)))
        .collect();
    let extrusion_surfaces: BTreeMap<u32, B5ExtrusionSurface> = records
        .iter()
        .filter_map(|record| {
            parse_extrusion_surface(record, &by_id, &object_stream_pcurves)
                .map(|extrusion| (record.object_id, extrusion))
        })
        .collect();
    let mut offset_surfaces = BTreeMap::new();
    for record in &records {
        let Some(offset) = parse_offset_surface(record, &surfaces, &extrusion_surfaces, &by_id)
        else {
            continue;
        };
        let carrier = if let Some(carrier) = surfaces.get(&offset.carrier_surface).cloned() {
            carrier
        } else {
            let Some(record) = by_id.get(&offset.carrier_surface) else {
                continue;
            };
            B5Surface::Unknown {
                family: record.family,
                class: record.class,
                payload: record.payload.clone(),
            }
        };
        surfaces.insert(record.object_id, carrier);
        offset_surfaces.insert(record.object_id, offset);
    }
    let extrusion_pcurves = extrusion_surfaces
        .values()
        .flat_map(|extrusion| {
            extrusion
                .directrix
                .supports
                .iter()
                .map(|(_, pcurve, _)| *pcurve)
        })
        .collect::<HashSet<_>>();
    let mut supported_surfaces = BTreeMap::new();
    for record in &records {
        let Some(construction) = parse_supported_surface(record) else {
            continue;
        };
        let Some(carrier) = surfaces.get(&construction.carrier_surface).cloned() else {
            continue;
        };
        surfaces.insert(record.object_id, carrier);
        if supported_surface_pcurves_match(&construction, &by_id) {
            supported_surfaces.insert(record.object_id, construction);
        }
    }
    let profiles: BTreeMap<u32, B5Profile> = records
        .iter()
        .filter_map(|record| parse_profile(record).map(|profile| (record.object_id, profile)))
        .collect();
    let mut pcurves: BTreeMap<u32, B5Pcurve> = records
        .iter()
        .filter_map(|record| {
            let pcurve = match record.class {
                0x18 => parse_line_pcurve(record),
                0x19 => parse_circle_pcurve(record),
                0x21 => parse_pcurve(record),
                _ => None,
            }?;
            Some((record.object_id, pcurve))
        })
        .collect();
    let mut circle_candidates = BTreeMap::<u32, Vec<B5Pcurve>>::new();
    for pcurve in circle_pcurves(bytes) {
        if surfaces.contains_key(&pcurve.surface) {
            circle_candidates
                .entry(pcurve.object_id)
                .or_default()
                .push(pcurve);
        }
    }
    for (object_id, candidates) in circle_candidates {
        let mut distinct = candidates.into_iter();
        let Some(candidate) = distinct.next() else {
            continue;
        };
        if distinct.all(|other| other == candidate) {
            pcurves.entry(object_id).or_insert(candidate);
        }
    }
    for jet in crate::families::a5a8::records::object_stream_pcurves(bytes) {
        let directrix_reference = extrusion_pcurves.contains(&jet.object_id);
        if !directrix_reference
            && by_id
                .get(&jet.object_id)
                .is_none_or(|record| record.class != 0x20)
        {
            continue;
        }
        let Some((_, control_points)) = crate::nurbs::quintic_jet_bspline(
            jet.degree,
            &jet.knots,
            &jet.points,
            &jet.first_derivatives,
            &jet.second_derivatives,
        ) else {
            continue;
        };
        pcurves.entry(jet.object_id).or_insert_with(|| B5Pcurve {
            object_id: jet.object_id,
            surface: jet.support_id,
            degree: jet.degree,
            distinct_knots: jet.knots.clone(),
            multiplicities: vec![jet.degree + 1; jet.knots.len()],
            control_points,
            weights: None,
            lifted_endpoints: None,
        });
    }
    let opaque_pcurves: BTreeMap<u32, B5OpaquePcurve> = records
        .iter()
        .filter_map(|record| parse_opaque_pcurve(record).map(|pcurve| (record.object_id, pcurve)))
        .collect();
    let parameter_incidences: BTreeMap<u32, B5ParameterIncidence> = records
        .iter()
        .filter_map(|record| {
            parameter_incidence(record).map(|incidence| (record.object_id, incidence))
        })
        .collect();
    let implicit_pcurves =
        implicit_pcurve_bindings(&records, &by_id, &pcurves, &opaque_pcurves, &surfaces);
    for pcurve in pcurves.values_mut() {
        pcurve.lifted_endpoints = surfaces
            .get(&pcurve.surface)
            .and_then(|surface| lift_pcurve_endpoints(surface, &profiles, &pcurve.control_points));
    }
    let source_face_count = records.iter().filter(|record| record.class == 0x5f).count();
    let mut loops: BTreeMap<u32, B5Loop> = records
        .iter()
        .filter(|record| record.class == 0x62)
        .filter_map(|record| {
            parse_loop(
                record,
                &by_id,
                &pcurves,
                &opaque_pcurves,
                &implicit_pcurves,
                &surfaces,
            )
            .map(|loop_| (record.object_id, loop_))
        })
        .collect();
    let mut faces: Vec<B5Face> = records
        .iter()
        .filter(|record| record.class == 0x5f)
        .filter_map(|record| parse_face(record, &loops, &surfaces))
        .collect();
    if faces.is_empty() || loops.is_empty() {
        return None;
    }
    let vertex_points = crate::families::consolidated::records::object_stream_vertices(bytes)
        .into_iter()
        .map(|point| [point.x, point.y, point.z])
        .collect::<Vec<_>>();
    let geometric_edge_vertices =
        if let Some(vertices) = bind_edge_vertices(&loops, &pcurves, &vertex_points) {
            vertices
        } else {
            pcurves.retain(|object_id, _| by_id.contains_key(object_id));
            loops = records
                .iter()
                .filter(|record| record.class == 0x62)
                .filter_map(|record| {
                    parse_loop(
                        record,
                        &by_id,
                        &pcurves,
                        &opaque_pcurves,
                        &implicit_pcurves,
                        &surfaces,
                    )
                    .map(|loop_| (record.object_id, loop_))
                })
                .collect();
            faces = records
                .iter()
                .filter(|record| record.class == 0x5f)
                .filter_map(|record| parse_face(record, &loops, &surfaces))
                .collect();
            bind_edge_vertices(&loops, &pcurves, &vertex_points)?
        };
    let native_edge_vertices: BTreeMap<u32, [u32; 2]> = records
        .iter()
        .filter(|record| record.class == 0x5e)
        .filter_map(|record| {
            parse_edge_vertex_refs(record).map(|vertices| (record.object_id, vertices))
        })
        .collect();
    let edge_parameter_incidences: BTreeMap<u32, [u32; 2]> = records
        .iter()
        .filter_map(|record| {
            let parameters = parse_edge_parameter_refs(record)?;
            parameters
                .iter()
                .all(|parameter| parameter_incidences.contains_key(parameter))
                .then_some((record.object_id, parameters))
        })
        .collect();
    let native_vertex_coordinates = incidence_vertex_coordinates(
        &native_edge_vertices,
        &by_id,
        &pcurves,
        &surfaces,
        &profiles,
    );
    let bound_vertices = bind_native_vertices(
        &loops,
        &pcurves,
        &native_edge_vertices,
        &geometric_edge_vertices,
        &native_vertex_coordinates,
        &vertex_points,
    );
    let edge_vertices = bound_vertices.edges;
    let logical_vertex_refs = bound_vertices.refs;
    let logical_vertex_points = bound_vertices.points;
    let vertex_tolerances = bound_vertices.tolerances;
    let referenced_loops: std::collections::HashSet<u32> = faces
        .iter()
        .flat_map(|face| face.loops.iter().copied())
        .collect();
    loops.retain(|loop_id, _| referenced_loops.contains(loop_id));
    let complete = faces.len() == source_face_count
        && referenced_loops.iter().all(|loop_id| {
            loops.get(loop_id).is_some_and(|loop_| {
                loop_
                    .pcurves
                    .iter()
                    .zip(&loop_.edges)
                    .all(|(pcurve, edge)| {
                        (pcurves
                            .get(pcurve)
                            .is_some_and(|pcurve| pcurve.surface == loop_.surface)
                            || opaque_pcurves
                                .get(pcurve)
                                .is_some_and(|pcurve| pcurve.surface == loop_.surface)
                            || implicit_pcurves.get(pcurve) == Some(&loop_.surface))
                            && edge_vertices.contains_key(edge)
                    })
                    && loop_chain_senses(loop_, &edge_vertices).is_some()
            })
        });
    Some(B5Graph {
        complete,
        faces,
        loops,
        pcurves,
        opaque_pcurves,
        implicit_pcurves,
        surfaces,
        offset_surfaces,
        extrusion_surfaces,
        supported_surfaces,
        parameter_incidences,
        vertex_points,
        logical_vertex_points,
        logical_vertex_refs,
        edge_vertices,
        edge_parameter_incidences,
        vertex_tolerances,
        profiles,
    })
}

/// Return native start/end vertex identities for every framed `b5 03 5e`
/// edge, keyed by the edge object id.
#[must_use]
pub fn edge_vertex_references(bytes: &[u8]) -> BTreeMap<u32, [u32; 2]> {
    let mut edges = BTreeMap::new();
    let mut ambiguous = HashSet::new();
    for offset in 0..bytes.len().saturating_sub(8) {
        let Some((end, 0xb5, 0x5e, object_id)) = object_frame(bytes, offset) else {
            continue;
        };
        let record = B5Record {
            offset,
            family: 0xb5,
            class: 0x5e,
            object_id,
            payload: bytes[offset + 8..end].to_vec(),
        };
        let Some(vertices) = parse_edge_vertex_refs(&record) else {
            continue;
        };
        if edges
            .insert(object_id, vertices)
            .is_some_and(|existing| existing != vertices)
        {
            ambiguous.insert(object_id);
        }
    }
    edges.retain(|object_id, _| !ambiguous.contains(object_id));
    edges
}

fn parse_edge_refs(record: &B5Record) -> Option<[u32; 5]> {
    (record.class == 0x5e && record.payload.first() == Some(&0x85)).then_some(())?;
    let mut position = 1;
    let references: [u32; 5] = (0..5)
        .map(|_| wire::object_ref(&record.payload, &mut position, true))
        .collect::<Option<Vec<_>>>()?
        .try_into()
        .ok()?;
    matches!(
        record.payload.get(position),
        Some(0x01 | 0x21 | 0x22 | 0x25 | 0x29 | 0x2a)
    )
    .then_some(references)
}

fn parse_edge_vertex_refs(record: &B5Record) -> Option<[u32; 2]> {
    let references = parse_edge_refs(record)?;
    Some([references[1], references[2]])
}

fn parse_edge_parameter_refs(record: &B5Record) -> Option<[u32; 2]> {
    let references = parse_edge_refs(record)?;
    Some([references[3], references[4]])
}

fn incidence_vertex_coordinates(
    native_edges: &BTreeMap<u32, [u32; 2]>,
    by_id: &HashMap<u32, &B5Record>,
    pcurves: &BTreeMap<u32, B5Pcurve>,
    surfaces: &BTreeMap<u32, B5Surface>,
    profiles: &BTreeMap<u32, B5Profile>,
) -> BTreeMap<u32, [f64; 3]> {
    native_edges
        .values()
        .flatten()
        .copied()
        .collect::<HashSet<_>>()
        .into_iter()
        .filter_map(|vertex| {
            let incidence = vertex_incidence_ref(by_id.get(&vertex)?)?;
            let incidence_records = counted_references(by_id.get(&incidence)?, 0x05)?;
            let point = incidence_records.into_iter().find_map(|incidence_record| {
                let incidence = parameter_incidence(by_id.get(&incidence_record)?)?;
                incidence
                    .curves
                    .into_iter()
                    .zip(incidence.parameters)
                    .find_map(|(pcurve_id, parameter)| {
                        let pcurve = pcurves.get(&pcurve_id)?;
                        let uv = evaluate_pcurve(pcurve, parameter)?;
                        lift_pcurve_endpoints(surfaces.get(&pcurve.surface)?, profiles, &[uv, uv])
                            .map(|points| points[0])
                    })
            })?;
            Some((vertex, point))
        })
        .collect()
}

fn vertex_incidence_ref(record: &B5Record) -> Option<u32> {
    (record.class == 0x5d && record.payload.first() == Some(&0x81)).then_some(())?;
    let mut position = 1;
    let incidence = wire::object_ref(&record.payload, &mut position, true)?;
    (record.payload.get(position) == Some(&0x00) && position + 1 == record.payload.len())
        .then_some(incidence)
}

fn counted_references(record: &B5Record, class: u8) -> Option<Vec<u32>> {
    (record.class == class).then_some(())?;
    let (references, position) = wire::counted_refs(&record.payload, true)?;
    (position == record.payload.len()).then_some(references)
}

fn parameter_incidence(record: &B5Record) -> Option<B5ParameterIncidence> {
    (record.class == 0x06).then_some(())?;
    let count = usize::from(record.payload.first()?.checked_sub(0x80)?);
    let mut position = 1;
    let references = (0..count)
        .map(|_| wire::object_ref(&record.payload, &mut position, true))
        .collect::<Option<Vec<_>>>()?;
    (record.payload.get(position) == Some(&(0x80u8.checked_add(u8::try_from(count).ok()?)?)))
        .then_some(())?;
    position += 1;
    let mut parameters = Vec::with_capacity(count);
    let mut controls = Vec::with_capacity(count);
    for _ in 0..count {
        let parameter = scalar(&record.payload, position)?;
        if !parameter.is_finite() {
            return None;
        }
        parameters.push(parameter);
        position += 8;
        controls.push(wire::compact_uint(&record.payload, &mut position)?);
    }
    (position == record.payload.len()).then_some(B5ParameterIncidence {
        object_id: record.object_id,
        curves: references,
        parameters,
        controls,
    })
}

fn implicit_pcurve_bindings(
    records: &[B5Record],
    by_id: &HashMap<u32, &B5Record>,
    pcurves: &BTreeMap<u32, B5Pcurve>,
    opaque_pcurves: &BTreeMap<u32, B5OpaquePcurve>,
    surfaces: &BTreeMap<u32, B5Surface>,
) -> BTreeMap<u32, u32> {
    let mut bindings = BTreeMap::new();
    let mut ambiguous = HashSet::new();
    for record in records.iter().filter(|record| record.class == 0x62) {
        let Some(references) = loop_references(record) else {
            continue;
        };
        let Some((&surface, occurrences)) = references.split_last() else {
            continue;
        };
        if !surfaces.contains_key(&surface) {
            continue;
        }
        for occurrence in occurrences.chunks_exact(2) {
            let pcurve = occurrence[0];
            if pcurves.contains_key(&pcurve)
                || opaque_pcurves.contains_key(&pcurve)
                || by_id.contains_key(&pcurve)
            {
                continue;
            }
            let Some(edge_references) = by_id
                .get(&occurrence[1])
                .and_then(|edge| parse_edge_refs(edge))
            else {
                continue;
            };
            let endpoint_incidence_contains = |reference_id| {
                by_id
                    .get(&reference_id)
                    .and_then(|incidence| parameter_incidence(incidence))
                    .is_some_and(|incidence| incidence.curves.contains(&pcurve))
            };
            let curve_wrapper_contains = by_id.get(&edge_references[0]).is_some_and(|wrapper| {
                matches!(wrapper.class, 0x23..=0x25) && record_references(wrapper).contains(&pcurve)
            });
            if !(curve_wrapper_contains
                || endpoint_incidence_contains(edge_references[3])
                    && endpoint_incidence_contains(edge_references[4]))
            {
                continue;
            }
            if bindings
                .insert(pcurve, surface)
                .is_some_and(|existing| existing != surface)
            {
                ambiguous.insert(pcurve);
            }
        }
    }
    bindings.retain(|pcurve, _| !ambiguous.contains(pcurve));
    bindings
}

pub(crate) fn evaluate_pcurve(pcurve: &B5Pcurve, parameter: f64) -> Option<[f64; 2]> {
    let mut knots = Vec::new();
    for (&knot, &multiplicity) in pcurve.distinct_knots.iter().zip(&pcurve.multiplicities) {
        knots.extend(std::iter::repeat_n(
            knot,
            usize::try_from(multiplicity).ok()?,
        ));
    }
    let control_points: Vec<Point2> = pcurve
        .control_points
        .iter()
        .map(|point| Point2::new(point[0], point[1]))
        .collect();
    let point = nurbs_pcurve_uv(
        pcurve.degree,
        &knots,
        &control_points,
        pcurve.weights.as_deref(),
        parameter,
    )?;
    Some([point.u, point.v])
}

struct BoundNativeVertices {
    edges: BTreeMap<u32, [usize; 2]>,
    refs: Vec<u32>,
    points: Vec<[f64; 3]>,
    tolerances: BTreeMap<usize, f64>,
}

fn bind_native_vertices(
    loops: &BTreeMap<u32, B5Loop>,
    pcurves: &BTreeMap<u32, B5Pcurve>,
    native_edges: &BTreeMap<u32, [u32; 2]>,
    geometric_edges: &BTreeMap<u32, [usize; 2]>,
    native_coordinates: &BTreeMap<u32, [f64; 3]>,
    points: &[[f64; 3]],
) -> BoundNativeVertices {
    let constraints: Vec<([u32; 2], [usize; 2])> = native_edges
        .iter()
        .filter_map(|(edge, vertices)| geometric_edges.get(edge).map(|points| (*vertices, *points)))
        .collect();
    let mut adjacency = HashMap::<u32, Vec<usize>>::new();
    for (index, (vertices, _)) in constraints.iter().enumerate() {
        adjacency.entry(vertices[0]).or_default().push(index);
        adjacency.entry(vertices[1]).or_default().push(index);
    }
    let vertex_points = propagate_vertex_points(&constraints, &adjacency, points);
    let mut logical_coordinates: HashMap<u32, [f64; 3]> = vertex_points
        .into_iter()
        .map(|(vertex, point)| (vertex, points[point]))
        .collect();
    logical_coordinates.extend(native_coordinates);
    for loop_ in loops.values() {
        for (&pcurve, &edge) in loop_.pcurves.iter().zip(&loop_.edges) {
            let (Some(vertices), Some(lifted)) = (
                native_edges.get(&edge),
                pcurves
                    .get(&pcurve)
                    .and_then(|pcurve| pcurve.lifted_endpoints),
            ) else {
                continue;
            };
            if lifted
                .iter()
                .flatten()
                .any(|coordinate| coordinate.abs() >= 1e7)
            {
                continue;
            }
            match (
                logical_coordinates.get(&vertices[0]).copied(),
                logical_coordinates.get(&vertices[1]).copied(),
            ) {
                (Some(start), None) => {
                    let start_lane = usize::from(
                        distance_squared(start, lifted[1]) < distance_squared(start, lifted[0]),
                    );
                    logical_coordinates.insert(vertices[1], lifted[1 - start_lane]);
                }
                (None, Some(end)) => {
                    let end_lane = usize::from(
                        distance_squared(end, lifted[1]) < distance_squared(end, lifted[0]),
                    );
                    logical_coordinates.insert(vertices[0], lifted[1 - end_lane]);
                }
                (None, None) => {
                    logical_coordinates.insert(vertices[0], lifted[0]);
                    logical_coordinates.insert(vertices[1], lifted[1]);
                }
                (Some(_), Some(_)) => {}
            }
        }
    }
    let mut logical_vertices: Vec<_> = logical_coordinates.into_iter().collect();
    logical_vertices.sort_unstable_by_key(|(vertex, _)| *vertex);
    let logical_vertex_indices: HashMap<u32, usize> = logical_vertices
        .iter()
        .enumerate()
        .map(|(rank, (vertex, _))| (*vertex, points.len() + rank))
        .collect();
    let logical_vertex_points: Vec<[f64; 3]> =
        logical_vertices.iter().map(|(_, point)| *point).collect();
    let logical_vertex_refs = logical_vertices.iter().map(|(vertex, _)| *vertex).collect();
    let mut edge_vertices = geometric_edges.clone();
    for (&edge, vertices) in native_edges {
        if let (Some(&start), Some(&end)) = (
            logical_vertex_indices.get(&vertices[0]),
            logical_vertex_indices.get(&vertices[1]),
        ) {
            edge_vertices.insert(edge, [start, end]);
        }
    }
    let mut tolerances = BTreeMap::<usize, f64>::new();
    for loop_ in loops.values() {
        for (&pcurve, &edge) in loop_.pcurves.iter().zip(&loop_.edges) {
            let Some(lifted) = pcurves
                .get(&pcurve)
                .and_then(|pcurve| pcurve.lifted_endpoints)
            else {
                continue;
            };
            let Some(&loci) = edge_vertices.get(&edge) else {
                continue;
            };
            let forward = [
                distance_squared(
                    vertex_coordinate(points, &logical_vertex_points, loci[0]),
                    lifted[0],
                )
                .sqrt(),
                distance_squared(
                    vertex_coordinate(points, &logical_vertex_points, loci[1]),
                    lifted[1],
                )
                .sqrt(),
            ];
            let reverse = [
                distance_squared(
                    vertex_coordinate(points, &logical_vertex_points, loci[1]),
                    lifted[0],
                )
                .sqrt(),
                distance_squared(
                    vertex_coordinate(points, &logical_vertex_points, loci[0]),
                    lifted[1],
                )
                .sqrt(),
            ];
            let residuals = if forward[0].max(forward[1]) <= reverse[0].max(reverse[1]) {
                [(loci[0], forward[0]), (loci[1], forward[1])]
            } else {
                [(loci[1], reverse[0]), (loci[0], reverse[1])]
            };
            for (locus, residual) in residuals {
                if residual > POINT_TOLERANCE && residual.is_finite() {
                    tolerances
                        .entry(locus)
                        .and_modify(|tolerance| *tolerance = tolerance.max(residual + 1e-9))
                        .or_insert(residual + 1e-9);
                }
            }
        }
    }
    BoundNativeVertices {
        edges: edge_vertices,
        refs: logical_vertex_refs,
        points: logical_vertex_points,
        tolerances,
    }
}

fn propagate_vertex_points(
    constraints: &[([u32; 2], [usize; 2])],
    adjacency: &HashMap<u32, Vec<usize>>,
    points: &[[f64; 3]],
) -> HashMap<u32, usize> {
    let mut mapping = HashMap::<u32, usize>::new();
    let mut completed = std::collections::HashSet::new();
    for seed in 0..constraints.len() {
        if completed.contains(&seed) {
            continue;
        }
        let candidates = [false, true].map(|reverse| {
            propagate_vertex_component(seed, reverse, constraints, adjacency, points)
        });
        let score = |(candidate, members): &(HashMap<u32, usize>, Vec<usize>)| {
            members
                .iter()
                .map(|&index| {
                    let (vertices, loci) = constraints[index];
                    let assigned = [candidate[&vertices[0]], candidate[&vertices[1]]];
                    let forward = distance_squared(points[assigned[0]], points[loci[0]])
                        + distance_squared(points[assigned[1]], points[loci[1]]);
                    let reverse = distance_squared(points[assigned[0]], points[loci[1]])
                        + distance_squared(points[assigned[1]], points[loci[0]]);
                    forward.min(reverse)
                })
                .sum::<f64>()
        };
        let selected = if score(&candidates[0]) <= score(&candidates[1]) {
            candidates.into_iter().next().unwrap_or_default()
        } else {
            candidates.into_iter().nth(1).unwrap_or_default()
        };
        mapping.extend(selected.0);
        completed.extend(selected.1);
    }
    mapping
}

fn propagate_vertex_component(
    seed: usize,
    reverse: bool,
    constraints: &[([u32; 2], [usize; 2])],
    adjacency: &HashMap<u32, Vec<usize>>,
    points: &[[f64; 3]],
) -> (HashMap<u32, usize>, Vec<usize>) {
    let (seed_vertices, seed_loci) = constraints[seed];
    let mut mapping = HashMap::from([
        (seed_vertices[0], seed_loci[usize::from(reverse)]),
        (seed_vertices[1], seed_loci[usize::from(!reverse)]),
    ]);
    let mut members = Vec::new();
    let mut pending = std::collections::VecDeque::from([seed_vertices[0], seed_vertices[1]]);
    let mut visited = std::collections::HashSet::new();
    while let Some(vertex) = pending.pop_front() {
        for &index in adjacency.get(&vertex).into_iter().flatten() {
            if !visited.insert(index) {
                continue;
            }
            members.push(index);
            let (vertices, loci) = constraints[index];
            let forward = assignment_residual(vertices, loci, &mapping, points);
            let reverse = assignment_residual(vertices, [loci[1], loci[0]], &mapping, points);
            let assigned = if forward <= reverse {
                loci
            } else {
                [loci[1], loci[0]]
            };
            for lane in 0..2 {
                if let std::collections::hash_map::Entry::Vacant(entry) =
                    mapping.entry(vertices[lane])
                {
                    entry.insert(assigned[lane]);
                    pending.push_back(vertices[lane]);
                }
            }
        }
    }
    (mapping, members)
}

fn assignment_residual(
    vertices: [u32; 2],
    loci: [usize; 2],
    mapping: &HashMap<u32, usize>,
    points: &[[f64; 3]],
) -> f64 {
    (0..2)
        .filter_map(|lane| {
            mapping
                .get(&vertices[lane])
                .map(|mapped| distance_squared(points[*mapped], points[loci[lane]]))
        })
        .sum()
}

fn vertex_coordinate(points: &[[f64; 3]], logical_points: &[[f64; 3]], index: usize) -> [f64; 3] {
    if index < points.len() {
        points[index]
    } else {
        logical_points[index - points.len()]
    }
}

pub(crate) fn loop_chain_senses(
    loop_: &B5Loop,
    edge_vertices: &BTreeMap<u32, [usize; 2]>,
) -> Option<Vec<bool>> {
    let first = edge_vertices.get(loop_.edges.first()?)?;
    if loop_.edges.len() == 1 {
        return (first[0] == first[1]).then_some(vec![false]);
    }
    let mut solutions = Vec::new();
    for first_reversed in [false, true] {
        let initial = first[usize::from(first_reversed)];
        let mut current = first[usize::from(!first_reversed)];
        let mut senses = vec![first_reversed];
        for edge_id in &loop_.edges[1..] {
            let endpoints = edge_vertices.get(edge_id)?;
            match (endpoints[0] == current, endpoints[1] == current) {
                (true, false) => {
                    senses.push(false);
                    current = endpoints[1];
                }
                (false, true) => {
                    senses.push(true);
                    current = endpoints[0];
                }
                _ => {
                    senses.clear();
                    break;
                }
            }
        }
        if !senses.is_empty() && current == initial {
            solutions.push(senses);
        }
    }
    (solutions.len() == 1).then(|| solutions.remove(0))
}

fn parse_profile(record: &B5Record) -> Option<B5Profile> {
    match record.class {
        0x0e => Some(B5Profile::Line {
            point: point(&record.payload, 1)?,
            direction: unit(point(&record.payload, 25)?)?,
        }),
        0x0f if record.payload.first() == Some(&0x80) => {
            let radius = scalar(&record.payload, 73)?;
            (radius > 0.0).then_some(B5Profile::Arc {
                center: point(&record.payload, 1)?,
                direction_x: unit(point(&record.payload, 25)?)?,
                direction_y: unit(point(&record.payload, 49)?)?,
                radius,
            })
        }
        _ => None,
    }
}

fn bind_edge_vertices(
    loops: &BTreeMap<u32, B5Loop>,
    pcurves: &BTreeMap<u32, B5Pcurve>,
    points: &[[f64; 3]],
) -> Option<BTreeMap<u32, [usize; 2]>> {
    let point_index = point_index(points);
    let mut edges: BTreeMap<u32, [usize; 2]> = BTreeMap::new();
    for loop_ in loops.values() {
        for (&pcurve_id, &edge_id) in loop_.pcurves.iter().zip(&loop_.edges) {
            let Some(endpoints) = pcurves
                .get(&pcurve_id)
                .and_then(|pcurve| pcurve.lifted_endpoints)
            else {
                continue;
            };
            let indices: Option<[usize; 2]> = endpoints
                .map(|endpoint| canonical_point(points, &point_index, endpoint))
                .into_iter()
                .collect::<Option<Vec<_>>>()
                .and_then(|indices| indices.try_into().ok());
            let Some(indices) = indices else {
                continue;
            };
            if let Some(previous) = edges.get(&edge_id) {
                let mut previous_sorted = *previous;
                let mut current_sorted = indices;
                previous_sorted.sort_unstable();
                current_sorted.sort_unstable();
                if previous_sorted != current_sorted {
                    return None;
                }
            } else {
                edges.insert(edge_id, indices);
            }
        }
    }
    Some(edges)
}

const POINT_TOLERANCE: f64 = 1.5e-3;

fn point_cell(point: [f64; 3]) -> [i64; 3] {
    point.map(|coordinate| (coordinate / POINT_TOLERANCE).floor() as i64)
}

fn point_index(points: &[[f64; 3]]) -> HashMap<[i64; 3], Vec<usize>> {
    let mut index = HashMap::<[i64; 3], Vec<usize>>::new();
    for (point_index, point) in points.iter().enumerate() {
        index
            .entry(point_cell(*point))
            .or_default()
            .push(point_index);
    }
    index
}

fn canonical_point(
    points: &[[f64; 3]],
    index: &HashMap<[i64; 3], Vec<usize>>,
    endpoint: [f64; 3],
) -> Option<usize> {
    let cell = point_cell(endpoint);
    let mut matches = Vec::new();
    for dx in -1..=1 {
        for dy in -1..=1 {
            for dz in -1..=1 {
                let neighbor = [
                    cell[0].saturating_add(dx),
                    cell[1].saturating_add(dy),
                    cell[2].saturating_add(dz),
                ];
                matches.extend(index.get(&neighbor).into_iter().flatten().filter_map(
                    |&point_index| {
                        (distance_squared(points[point_index], endpoint)
                            <= POINT_TOLERANCE * POINT_TOLERANCE)
                            .then_some(point_index)
                    },
                ));
            }
        }
    }
    matches.into_iter().min()
}

fn distance_squared(left: [f64; 3], right: [f64; 3]) -> f64 {
    (left[0] - right[0]).powi(2) + (left[1] - right[1]).powi(2) + (left[2] - right[2]).powi(2)
}

fn parse_surface(record: &B5Record) -> Option<B5Surface> {
    match record.class {
        0x27 => Some(B5Surface::Plane {
            origin: point(&record.payload, 1)?,
            direction_u: point(&record.payload, 25)?,
            direction_v: point(&record.payload, 49)?,
        }),
        0x28 => {
            let direction_u = unit(point(&record.payload, 25)?)?;
            let axis = unit(cross(direction_u, point(&record.payload, 49)?))?;
            let radius = scalar(&record.payload, 73)?;
            (radius > 0.0).then_some(B5Surface::Cylinder {
                origin: point(&record.payload, 1)?,
                reference_x: direction_u,
                axis,
                radius,
            })
        }
        0x29 => {
            let apex = point(&record.payload, 1)?;
            let direction_x = unit(point(&record.payload, 25)?)?;
            let direction_y = unit(point(&record.payload, 49)?)?;
            let axis = unit(point(&record.payload, 73)?)?;
            let half_angle = scalar(&record.payload, 97)?;
            let angular_offset = scalar(&record.payload, 121)?;
            let mut slant_range = [scalar(&record.payload, 129)?, scalar(&record.payload, 137)?];
            if slant_range[0].abs() <= 1e-12 {
                slant_range[0] = 0.0;
            }
            let angular_scale = scalar(&record.payload, 145)?;
            ((0.0..std::f64::consts::FRAC_PI_2).contains(&half_angle)
                && slant_range[0] >= 0.0
                && slant_range[0] < slant_range[1]
                && angular_scale > 0.0)
                .then_some(B5Surface::Cone {
                    apex,
                    direction_x,
                    direction_y,
                    axis,
                    half_angle,
                    angular_offset,
                    slant_range,
                    angular_scale,
                })
        }
        0x2b => {
            let major_radius = scalar(&record.payload, 97)?;
            let minor_radius = scalar(&record.payload, 105)?;
            let major_scale = scalar(&record.payload, 177)?;
            let minor_scale = scalar(&record.payload, 185)?;
            (record.payload.len() == 201
                && major_radius > 0.0
                && minor_radius > 0.0
                && major_scale > 0.0
                && minor_scale > 0.0)
                .then_some(())?;
            Some(B5Surface::Torus {
                center: point(&record.payload, 1)?,
                direction_x: unit(point(&record.payload, 25)?)?,
                direction_y: unit(point(&record.payload, 49)?)?,
                axis: unit(point(&record.payload, 73)?)?,
                major_radius,
                minor_radius,
                major_scale,
                minor_scale,
            })
        }
        0x2d => {
            let mut position = 1;
            let profile_curve = wire::object_ref(&record.payload, &mut position, true)?;
            let gauge_radius = scalar(&record.payload, position.checked_add(130)?)?;
            (gauge_radius.abs() > f64::EPSILON).then_some(B5Surface::Revolution {
                profile_curve,
                axis_origin: point(&record.payload, position)?,
                axis_direction: unit(point(&record.payload, position.checked_add(72)?)?)?,
                gauge_radius,
            })
        }
        _ => None,
    }
}

fn surface_node(
    record: &B5Record,
    header: Option<&crate::families::a5a8::records::A8SurfaceHeader>,
) -> Option<B5Surface> {
    parse_surface(record).or_else(|| {
        (record.family == 0xa8 && record.class == 0x34).then(|| {
            header.map_or_else(
                || B5Surface::Unknown {
                    family: record.family,
                    class: record.class,
                    payload: record.payload.clone(),
                },
                |header| B5Surface::UnresolvedNurbs {
                    header: header.clone(),
                    payload: record.payload.clone(),
                },
            )
        })
    })
}

fn surface_alias_target(record: &B5Record) -> Option<u32> {
    (record.family == 0xb5 && record.class == 0x2e).then_some(())?;
    let mut position = 0;
    if record.payload.first() == Some(&0x81) {
        position += 1;
    }
    let target = wire::object_ref(&record.payload, &mut position, true)?;
    (position == record.payload.len()).then_some(target)
}

fn parse_offset_surface(
    record: &B5Record,
    surfaces: &BTreeMap<u32, B5Surface>,
    extrusion_surfaces: &BTreeMap<u32, B5ExtrusionSurface>,
    records: &HashMap<u32, &B5Record>,
) -> Option<B5OffsetSurface> {
    (record.family == 0xb5 && record.class == 0x30 && record.payload.first() == Some(&0x82))
        .then_some(())?;
    let mut position = 1;
    let carrier_surface = wire::object_ref(&record.payload, &mut position, true)?;
    let source_surface = wire::object_ref(&record.payload, &mut position, true)?;
    let distance = scalar(&record.payload, position)?;
    position += 8;
    let carrier_kind = *record.payload.get(position)?;
    position += 1;
    let [u0, u1, v0, v1] = line_values::<4>(&record.payload, position)?;
    position += 32;
    if position != record.payload.len() || u0 >= u1 || v0 >= v1 {
        return None;
    }
    let expected_kind = match surfaces.get(&carrier_surface) {
        Some(B5Surface::Plane { .. }) => 0x15,
        Some(B5Surface::Cylinder { .. }) => 0x05,
        Some(B5Surface::Torus { .. }) => 0x0d,
        Some(B5Surface::RollingBall { .. }) => 0x19,
        Some(B5Surface::Unknown {
            family: 0xb5,
            class: 0x2c,
            ..
        }) => {
            let source = extrusion_surfaces.get(&source_surface)?;
            let carrier = extrusion_carrier(records.get(&carrier_surface)?)?;
            if carrier.direction != source.direction
                || carrier.parameter_bounds != [[v0, v1], [u0, u1]]
            {
                return None;
            }
            0x21
        }
        Some(_) => return None,
        None => {
            if let (Some(source), Some(carrier)) = (
                extrusion_surfaces.get(&source_surface),
                records
                    .get(&carrier_surface)
                    .and_then(|record| extrusion_carrier(record)),
            ) {
                if carrier.direction != source.direction
                    || carrier.parameter_bounds != [[v0, v1], [u0, u1]]
                {
                    return None;
                }
                return (carrier_kind == 0x21).then_some(B5OffsetSurface {
                    object_id: record.object_id,
                    carrier_surface,
                    source_surface,
                    distance,
                    carrier_kind,
                    parameter_bounds: [[u0, u1], [v0, v1]],
                });
            }
            let cache = parse_offset_cache(records.get(&carrier_surface)?)?;
            let source = surfaces.get(&source_surface)?;
            let cached_source = surfaces.get(&cache.source_surface)?;
            if source != cached_source
                || distance.to_bits() != cache.distance.to_bits()
                || [u0, v0, u1, v1]
                    .into_iter()
                    .zip(cache.interleaved_bounds)
                    .any(|(left, right)| left.to_bits() != right.to_bits())
            {
                return None;
            }
            0x01
        }
    };
    (carrier_kind == expected_kind).then_some(B5OffsetSurface {
        object_id: record.object_id,
        carrier_surface,
        source_surface,
        distance,
        carrier_kind,
        parameter_bounds: [[u0, u1], [v0, v1]],
    })
}

struct B5OffsetCache {
    source_surface: u32,
    distance: f64,
    interleaved_bounds: [f64; 4],
}

fn parse_offset_cache(record: &B5Record) -> Option<B5OffsetCache> {
    (record.family == 0xb5 && record.class == 0x31 && record.payload.first() == Some(&0x81))
        .then_some(())?;
    let mut position = 1;
    let source_surface = wire::object_ref(&record.payload, &mut position, true)?;
    let [distance, u0, v0, u1, v1] = line_values::<5>(&record.payload, position)?;
    position += 40;
    (position == record.payload.len() && u0 < u1 && v0 < v1).then_some(B5OffsetCache {
        source_surface,
        distance,
        interleaved_bounds: [u0, v0, u1, v1],
    })
}

fn parse_extrusion_surface(
    record: &B5Record,
    records: &HashMap<u32, &B5Record>,
    object_stream_pcurves: &BTreeMap<u32, (u32, [f64; 2])>,
) -> Option<B5ExtrusionSurface> {
    let carrier = extrusion_carrier(record)?;
    let directrix = parse_extrusion_directrix(
        records.get(&carrier.directrix_id)?,
        records,
        object_stream_pcurves,
    )?;
    if directrix
        .parameter_range
        .into_iter()
        .zip(carrier.parameter_bounds[1])
        .any(|(left, right)| left.to_bits() != right.to_bits())
    {
        return None;
    }
    Some(B5ExtrusionSurface {
        object_id: record.object_id,
        direction: carrier.direction,
        parameter_bounds: carrier.parameter_bounds,
        directrix,
    })
}

struct B5ExtrusionCarrier {
    directrix_id: u32,
    direction: [f64; 3],
    parameter_bounds: [[f64; 2]; 2],
}

fn extrusion_carrier(record: &B5Record) -> Option<B5ExtrusionCarrier> {
    (record.family == 0xb5 && record.class == 0x2c && record.payload.first() == Some(&0x81))
        .then_some(())?;
    let mut position = 1;
    let directrix_id = wire::object_ref(&record.payload, &mut position, true)?;
    let values = line_values::<9>(&record.payload, position)?;
    position += 72;
    (record.payload.get(position..) == Some(&[0x05, 0x05])
        && unit([values[0], values[1], values[2]]).is_some()
        && values[3] < values[4]
        && values[5].to_bits() == 1.0f64.to_bits()
        && values[6].to_bits() == 0.0f64.to_bits()
        && values[7] < values[8])
        .then_some(B5ExtrusionCarrier {
            directrix_id,
            direction: [values[0], values[1], values[2]],
            parameter_bounds: [[values[3], values[4]], [values[7], values[8]]],
        })
}

fn parse_extrusion_directrix(
    record: &B5Record,
    records: &HashMap<u32, &B5Record>,
    object_stream_pcurves: &BTreeMap<u32, (u32, [f64; 2])>,
) -> Option<B5ExtrusionDirectrix> {
    (record.family == 0xa8 && record.class == 0x25 && record.payload.first() == Some(&0x82))
        .then_some(())?;
    let mut position = 1;
    let wrapper_id = wire::object_ref(&record.payload, &mut position, true)?;
    let second_pcurve = wire::object_ref(&record.payload, &mut position, true)?;
    let tail = record.payload.len().checked_sub(25)?;
    (position < tail).then_some(())?;
    let parameter_range = line_values::<2>(&record.payload, tail)?;
    let cache_fit_tolerance = scalar(&record.payload, tail + 16)?;
    if record.payload.get(tail + 24) != Some(&0x01)
        || parameter_range[0] >= parameter_range[1]
        || cache_fit_tolerance <= 0.0
    {
        return None;
    }
    let wrapper = records.get(&wrapper_id)?;
    (wrapper.family == 0xb5 && wrapper.class == 0x24 && wrapper.payload.first() == Some(&0x81))
        .then_some(())?;
    let mut wrapper_position = 1;
    let first_pcurve = wire::object_ref(&wrapper.payload, &mut wrapper_position, true)?;
    if wrapper.payload.get(wrapper_position..wrapper_position + 2) != Some(&[0x81, 0x01]) {
        return None;
    }
    wrapper_position += 2;
    let wrapper_values = line_values::<3>(&wrapper.payload, wrapper_position)?;
    wrapper_position += 24;
    if wrapper.payload.get(wrapper_position..) != Some(&[0x01])
        || wrapper_values[2].to_bits() != 0.0f64.to_bits()
        || wrapper_values[..2]
            .iter()
            .zip(parameter_range)
            .any(|(left, right)| left.to_bits() != right.to_bits())
    {
        return None;
    }
    let first = records.get(&first_pcurve)?;
    let first_surface = pcurve_surface_reference(first)?;
    let first_range = analytic_pcurve_range(first)?;
    let &(second_surface, second_range) = object_stream_pcurves.get(&second_pcurve)?;
    Some(B5ExtrusionDirectrix {
        object_id: record.object_id,
        supports: [
            (first_surface, first_pcurve, first_range),
            (second_surface, second_pcurve, second_range),
        ],
        parameter_range,
        cache_fit_tolerance,
    })
}

fn pcurve_surface_reference(record: &B5Record) -> Option<u32> {
    (matches!(record.class, 0x18..=0x21)).then_some(())?;
    let mut position = usize::from(record.payload.first() == Some(&0x81));
    wire::object_ref(&record.payload, &mut position, true)
}

fn analytic_pcurve_range(record: &B5Record) -> Option<[f64; 2]> {
    match record.class {
        0x18 => parse_line_pcurve(record).and_then(|pcurve| {
            Some([
                *pcurve.distinct_knots.first()?,
                *pcurve.distinct_knots.last()?,
            ])
        }),
        0x19 => parse_circle_pcurve(record).and_then(|pcurve| {
            Some([
                *pcurve.distinct_knots.first()?,
                *pcurve.distinct_knots.last()?,
            ])
        }),
        _ => None,
    }
}

fn parse_supported_surface(record: &B5Record) -> Option<B5SupportedSurface> {
    (record.family == 0xb5
        && matches!(record.class, 0x37 | 0x3b)
        && record.payload.first() == Some(&0x85))
    .then_some(())?;
    let mut position = 1;
    let references: [u32; 5] = (0..5)
        .map(|_| wire::object_ref(&record.payload, &mut position, true))
        .collect::<Option<Vec<_>>>()?
        .try_into()
        .ok()?;
    (record.payload.len() == position.checked_add(22)?).then_some(())?;
    let controls = [
        record.payload[position],
        record.payload[position + 1],
        record.payload[position + 10],
        record.payload[position + 11],
        record.payload[position + 20],
        record.payload[position + 21],
    ];
    let scalars = [
        scalar(&record.payload, position + 2)?,
        scalar(&record.payload, position + 12)?,
    ];
    Some(B5SupportedSurface {
        object_id: record.object_id,
        class: record.class,
        carrier_surface: references[0],
        support_surfaces: [references[1], references[2]],
        support_pcurves: [references[3], references[4]],
        controls,
        scalars,
    })
}

fn supported_surface_pcurves_match(
    construction: &B5SupportedSurface,
    by_id: &HashMap<u32, &B5Record>,
) -> bool {
    construction
        .support_pcurves
        .iter()
        .zip(construction.support_surfaces)
        .all(|(pcurve_id, support_id)| {
            let Some(pcurve) = by_id.get(pcurve_id) else {
                return false;
            };
            let mut position = 1;
            pcurve.payload.first() == Some(&0x81)
                && wire::object_ref(&pcurve.payload, &mut position, true) == Some(support_id)
        })
}

fn lift_pcurve_endpoints(
    surface: &B5Surface,
    profiles: &BTreeMap<u32, B5Profile>,
    control_points: &[[f64; 2]],
) -> Option<[[f64; 3]; 2]> {
    let endpoints = [*control_points.first()?, *control_points.last()?];
    match surface {
        B5Surface::UnresolvedNurbs { .. }
        | B5Surface::Unknown { .. }
        | B5Surface::RollingBall { .. } => None,
        B5Surface::Plane {
            origin,
            direction_u,
            direction_v,
        } => Some(
            endpoints
                .map(|[u, v]| add(*origin, add(scale(*direction_u, u), scale(*direction_v, v)))),
        ),
        B5Surface::Cylinder {
            origin,
            reference_x,
            axis,
            radius,
        } => {
            let reference_y = cross(*axis, *reference_x);
            Some(endpoints.map(|[u, v]| {
                let angle = u / radius;
                add(
                    *origin,
                    add(
                        scale(
                            add(
                                scale(*reference_x, angle.cos()),
                                scale(reference_y, angle.sin()),
                            ),
                            *radius,
                        ),
                        scale(*axis, v),
                    ),
                )
            }))
        }
        B5Surface::Cone {
            apex,
            direction_x,
            direction_y,
            axis,
            half_angle,
            angular_scale,
            ..
        } => Some(endpoints.map(|[u, v]| {
            let angle = u / angular_scale;
            let radial = add(
                scale(*direction_x, angle.cos()),
                scale(*direction_y, angle.sin()),
            );
            add(
                *apex,
                scale(
                    add(
                        scale(*axis, half_angle.cos()),
                        scale(radial, half_angle.sin()),
                    ),
                    v,
                ),
            )
        })),
        B5Surface::Torus {
            center,
            direction_x,
            direction_y,
            axis,
            major_radius,
            minor_radius,
            major_scale,
            minor_scale,
        } => Some(endpoints.map(|[u, v]| {
            let major_angle = u / major_scale;
            let minor_angle = v / minor_scale;
            let radial = add(
                scale(*direction_x, major_angle.cos()),
                scale(*direction_y, major_angle.sin()),
            );
            add(
                *center,
                add(
                    scale(radial, major_radius + minor_radius * minor_angle.cos()),
                    scale(*axis, minor_radius * minor_angle.sin()),
                ),
            )
        })),
        B5Surface::Revolution {
            profile_curve,
            axis_origin,
            axis_direction,
            gauge_radius,
        } => {
            let profile = profiles.get(profile_curve)?;
            Some(endpoints.map(|[u, v]| {
                let point = match profile {
                    B5Profile::Line { point, direction } => add(*point, scale(*direction, u)),
                    B5Profile::Arc {
                        center,
                        direction_x,
                        direction_y,
                        radius,
                    } => {
                        let angle = u / radius;
                        add(
                            *center,
                            scale(
                                add(
                                    scale(*direction_x, angle.cos()),
                                    scale(*direction_y, angle.sin()),
                                ),
                                *radius,
                            ),
                        )
                    }
                };
                rotate_about_axis(point, *axis_origin, *axis_direction, v / gauge_radius)
            }))
        }
        B5Surface::Nurbs(surface) => Some([
            evaluate_nurbs(surface, endpoints[0][0], endpoints[0][1])?,
            evaluate_nurbs(surface, endpoints[1][0], endpoints[1][1])?,
        ]),
    }
}

fn evaluate_nurbs(surface: &NurbsSurface, u: f64, v: f64) -> Option<[f64; 3]> {
    let point = nurbs_surface_point(surface, u, v)?;
    Some([point.x, point.y, point.z])
}

fn rotate_about_axis(point: [f64; 3], origin: [f64; 3], axis: [f64; 3], angle: f64) -> [f64; 3] {
    let relative = [
        point[0] - origin[0],
        point[1] - origin[1],
        point[2] - origin[2],
    ];
    let cross_term = cross(axis, relative);
    let dot = axis[0] * relative[0] + axis[1] * relative[1] + axis[2] * relative[2];
    add(
        origin,
        add(
            scale(relative, angle.cos()),
            add(
                scale(cross_term, angle.sin()),
                scale(axis, dot * (1.0 - angle.cos())),
            ),
        ),
    )
}

fn scalar(bytes: &[u8], offset: usize) -> Option<f64> {
    let value = f64_at(bytes, offset)?;
    value.is_finite().then_some(value)
}

fn point(bytes: &[u8], offset: usize) -> Option<[f64; 3]> {
    Some([
        scalar(bytes, offset)?,
        scalar(bytes, offset + 8)?,
        scalar(bytes, offset + 16)?,
    ])
}

// `unit` divides by reciprocal-multiply (`scale(value, 1.0 / length)`), a
// bit-level-distinct normalization from the transfer module's per-component
// division. The two must NOT be unified: the affected profiles depend on the
// exact rounding of each form. See `transfer::unit` for the sibling copy.
fn unit(value: [f64; 3]) -> Option<[f64; 3]> {
    let length = value
        .iter()
        .map(|component| component * component)
        .sum::<f64>()
        .sqrt();
    (length > f64::EPSILON).then(|| scale(value, 1.0 / length))
}

fn parse_pcurve(record: &B5Record) -> Option<B5Pcurve> {
    if record.payload.first() != Some(&0x81) {
        return None;
    }
    let mut position = 1;
    let surface = wire::object_ref(&record.payload, &mut position, true)?;
    if record.payload.get(position) != Some(&0x01) {
        return None;
    }
    position += 1;
    let degree = wire::compact_uint(&record.payload, &mut position)?;
    position = position.checked_add(2)?;
    record.payload.get(..position)?;
    let knot_count = usize::try_from(wire::compact_uint(&record.payload, &mut position)?).ok()?;
    if !(2..=4096).contains(&knot_count) {
        return None;
    }
    position += if record.payload.get(position) == Some(&0x08) {
        2
    } else {
        1
    };
    record.payload.get(..position)?;
    let mut distinct_knots = Vec::with_capacity(knot_count);
    for _ in 0..knot_count {
        let value = f64::from_le_bytes(
            record
                .payload
                .get(position..position + 8)?
                .try_into()
                .ok()?,
        );
        if !value.is_finite() {
            return None;
        }
        distinct_knots.push(value);
        position += 8;
    }
    if distinct_knots.windows(2).any(|pair| pair[0] >= pair[1]) {
        return None;
    }
    let mut multiplicities = Vec::with_capacity(knot_count);
    for _ in 0..knot_count {
        multiplicities.push(wire::compact_uint(&record.payload, &mut position)?);
    }
    let pole_count = multiplicities
        .iter()
        .try_fold(0u32, |sum, value| sum.checked_add(*value))?
        .checked_sub(degree + 1)?;
    if !(2..=8192).contains(&pole_count) {
        return None;
    }
    let mut control_points = Vec::with_capacity(usize::try_from(pole_count).ok()?);
    for _ in 0..pole_count {
        let u = f64::from_le_bytes(
            record
                .payload
                .get(position..position + 8)?
                .try_into()
                .ok()?,
        );
        let v = f64::from_le_bytes(
            record
                .payload
                .get(position + 8..position + 16)?
                .try_into()
                .ok()?,
        );
        if !u.is_finite() || !v.is_finite() {
            return None;
        }
        control_points.push([u, v]);
        position += 16;
    }
    if record.payload.get(position..position + 2) != Some(&[0x05, 0x05])
        || record.payload.last() != Some(&0x07)
    {
        return None;
    }
    Some(B5Pcurve {
        object_id: record.object_id,
        surface,
        degree,
        distinct_knots,
        multiplicities,
        control_points,
        weights: None,
        lifted_endpoints: None,
    })
}

fn parse_circle_pcurve(record: &B5Record) -> Option<B5Pcurve> {
    if record.payload.first() != Some(&0x81) {
        return None;
    }
    let mut position = 1;
    let surface = wire::object_ref(&record.payload, &mut position, true)?;
    if record.payload.len() != position.checked_add(58)? {
        return None;
    }
    let center = line_values::<2>(&record.payload, position)?;
    position += 16;
    if record.payload.get(position..position + 2) != Some(&[0x05, 0x05]) {
        return None;
    }
    position += 2;
    let [radius, start, end, orientation, phase] = line_values::<5>(&record.payload, position)?;
    if radius <= 0.0 || start >= end || !matches!(orientation, -1.0 | 1.0) {
        return None;
    }
    let start_angle = phase + orientation * start / radius;
    let end_angle = phase + orientation * end / radius;
    let span_count = ((end_angle - start_angle).abs() / std::f64::consts::FRAC_PI_2).ceil();
    if !span_count.is_finite() || span_count > crate::MAX_EXACT_ARC_SPANS as f64 {
        return None;
    }
    let span_count = (span_count as usize).max(1);
    let control_count = span_count.checked_mul(2)?.checked_add(1)?;
    let mut control_points = Vec::with_capacity(control_count);
    let mut weights = Vec::with_capacity(control_count);
    let mut distinct_knots = vec![start];
    let mut multiplicities = vec![3];
    for span in 0..span_count {
        let fraction0 = span as f64 / span_count as f64;
        let fraction1 = (span + 1) as f64 / span_count as f64;
        let angle0 = start_angle + (end_angle - start_angle) * fraction0;
        let angle1 = start_angle + (end_angle - start_angle) * fraction1;
        let middle = (angle0 + angle1) * 0.5;
        let middle_weight = ((angle1 - angle0) * 0.5).cos();
        if middle_weight <= f64::EPSILON {
            return None;
        }
        if span == 0 {
            control_points.push([
                center[0] + radius * angle0.cos(),
                center[1] + radius * angle0.sin(),
            ]);
            weights.push(1.0);
        }
        control_points.push([
            center[0] + radius / middle_weight * middle.cos(),
            center[1] + radius / middle_weight * middle.sin(),
        ]);
        control_points.push([
            center[0] + radius * angle1.cos(),
            center[1] + radius * angle1.sin(),
        ]);
        weights.extend([middle_weight, 1.0]);
        if span + 1 < span_count {
            distinct_knots.push(start + (end - start) * fraction1);
            multiplicities.push(2);
        }
    }
    distinct_knots.push(end);
    multiplicities.push(3);
    Some(B5Pcurve {
        object_id: record.object_id,
        surface,
        degree: 2,
        distinct_knots,
        multiplicities,
        control_points,
        weights: Some(weights),
        lifted_endpoints: None,
    })
}

fn parse_opaque_pcurve(record: &B5Record) -> Option<B5OpaquePcurve> {
    if !matches!(record.class, 0x1a | 0x1d) || record.payload.first() != Some(&0x81) {
        return None;
    }
    let mut position = 1;
    let surface = wire::object_ref(&record.payload, &mut position, true)?;
    match record.class {
        0x1a => {
            (record.payload.len() == position.checked_add(74)?).then_some(())?;
            line_values::<2>(&record.payload, position)?;
            position += 16;
            (record.payload.get(position..position + 2) == Some(&[0x05, 0x05])).then_some(())?;
            line_values::<7>(&record.payload, position + 2)?;
        }
        0x1d => {
            (record.payload.len() == position.checked_add(99)?).then_some(())?;
            line_values::<4>(&record.payload, position)?;
            position += 32;
            (record.payload.get(position..position + 2) == Some(&[0x05, 0x81])).then_some(())?;
            line_values::<3>(&record.payload, position + 2)?;
            position += 26;
            (record.payload.get(position) == Some(&0x1d)).then_some(())?;
            line_values::<5>(&record.payload, position + 1)?;
        }
        _ => unreachable!(),
    }
    Some(B5OpaquePcurve {
        object_id: record.object_id,
        surface,
        class: record.class,
        payload: record.payload.clone(),
    })
}

fn circle_pcurves(bytes: &[u8]) -> Vec<B5Pcurve> {
    let mut pcurves = Vec::new();
    for offset in 0..bytes.len().saturating_sub(8) {
        let Some((end, 0xb5, 0x19, object_id)) = object_frame(bytes, offset) else {
            continue;
        };
        let record = B5Record {
            offset,
            family: 0xb5,
            class: 0x19,
            object_id,
            payload: bytes[offset + 8..end].to_vec(),
        };
        if let Some(pcurve) = parse_circle_pcurve(&record) {
            pcurves.push(pcurve);
        }
    }
    pcurves
}

fn parse_line_pcurve(record: &B5Record) -> Option<B5Pcurve> {
    if record.payload.first() != Some(&0x81) {
        return None;
    }
    let mut position = 1;
    let surface = wire::object_ref(&record.payload, &mut position, true)?;
    let mode = *record.payload.get(position)?;
    position += 1;
    let (start, end, control_points) = match mode {
        0x01 if record.payload.len() == position.checked_add(48)? => {
            let [u, v, du, dv, start, end] = line_values::<6>(&record.payload, position)?;
            if du.abs().max(dv.abs()) <= f64::EPSILON {
                return None;
            }
            (
                start,
                end,
                vec![
                    [u + start * du, v + start * dv],
                    [u + end * du, v + end * dv],
                ],
            )
        }
        0x05 if record.payload.len() == position.checked_add(24)? => {
            let [constant, start, end] = line_values::<3>(&record.payload, position)?;
            (start, end, vec![[constant, start], [constant, end]])
        }
        0x09 if record.payload.len() == position.checked_add(24)? => {
            let [constant, start, end] = line_values::<3>(&record.payload, position)?;
            (start, end, vec![[start, constant], [end, constant]])
        }
        _ => return None,
    };
    if start >= end {
        return None;
    }
    Some(B5Pcurve {
        object_id: record.object_id,
        surface,
        degree: 1,
        distinct_knots: vec![start, end],
        multiplicities: vec![2, 2],
        control_points,
        weights: None,
        lifted_endpoints: None,
    })
}

fn line_values<const N: usize>(payload: &[u8], mut position: usize) -> Option<[f64; N]> {
    let mut values = [0.0; N];
    for value in &mut values {
        *value = f64::from_le_bytes(payload.get(position..position + 8)?.try_into().ok()?);
        if !value.is_finite() {
            return None;
        }
        position += 8;
    }
    Some(values)
}

fn records(bytes: &[u8]) -> Vec<B5Record> {
    let mut records = framed_records(bytes);
    let existing: HashSet<u32> = records.iter().map(|record| record.object_id).collect();
    let mut pending: HashSet<u32> = records.iter().flat_map(record_references).collect();
    let mut admitted = HashSet::new();
    loop {
        pending.retain(|object_id| !existing.contains(object_id) && !admitted.contains(object_id));
        if pending.is_empty() {
            break;
        }
        let mut candidates = HashMap::<u32, Option<B5Record>>::new();
        for offset in 0..bytes.len().saturating_sub(8) {
            let Some((end, family, class, object_id)) = object_frame(bytes, offset) else {
                continue;
            };
            if !pending.contains(&object_id) || !is_reference_dependency_class(family, class) {
                continue;
            }
            let header = if family == 0xa8 { 11 } else { 8 };
            let candidate = B5Record {
                offset,
                family,
                class,
                object_id,
                payload: bytes[offset + header..end].to_vec(),
            };
            candidates
                .entry(object_id)
                .and_modify(|slot| {
                    if slot.as_ref().is_some_and(|existing| {
                        existing.family != candidate.family
                            || existing.class != candidate.class
                            || existing.payload != candidate.payload
                    }) {
                        *slot = None;
                    }
                })
                .or_insert(Some(candidate));
        }
        let mut found = candidates.into_values().flatten().collect::<Vec<_>>();
        if found.is_empty() {
            break;
        }
        found.sort_unstable_by_key(|record| record.offset);
        pending.clear();
        for candidate in found {
            admitted.insert(candidate.object_id);
            pending.extend(record_references(&candidate));
            records.push(candidate);
        }
    }
    records
}

fn framed_records(bytes: &[u8]) -> Vec<B5Record> {
    let recurse: fn(&[u8]) -> Vec<B5Record> = framed_records;
    let mut records = Vec::new();
    let mut seen = HashMap::<u32, (u8, Vec<u8>)>::new();
    for run in object_runs(bytes) {
        for frame in run {
            let ObjectFrame {
                start: record_start,
                end: record_end,
                family,
                class,
                object_id,
            } = frame;
            if family == 0xa8 {
                let payload = record_start + 11;
                for mut child in recurse(&bytes[payload..record_end]) {
                    child.offset += payload;
                    if seen
                        .get(&child.object_id)
                        .is_some_and(|(seen_class, seen_payload)| {
                            *seen_class == child.class && *seen_payload == child.payload
                        })
                    {
                        continue;
                    }
                    seen.insert(child.object_id, (child.class, child.payload.clone()));
                    records.push(child);
                }
            }
            if !((family == 0xb5 && is_topology_class(class))
                || (family == 0xa8 && matches!(class, 0x34 | 0x62)))
            {
                continue;
            }
            let header = if family == 0xa8 { 11 } else { 8 };
            let payload = bytes[record_start + header..record_end].to_vec();
            if seen
                .get(&object_id)
                .is_some_and(|(seen_class, seen_payload)| {
                    *seen_class == class && *seen_payload == payload
                })
            {
                continue;
            }
            seen.insert(object_id, (class, payload.clone()));
            records.push(B5Record {
                offset: record_start,
                family,
                class,
                object_id,
                payload,
            });
        }
    }
    records
}

/// Return complete byte ranges for length-closed B5/A8 record runs. A lone
/// frame-like candidate does not establish an object-stream record.
#[must_use]
pub(crate) fn framed_ranges(bytes: &[u8]) -> Vec<std::ops::Range<usize>> {
    object_runs(bytes)
        .into_iter()
        .flatten()
        .map(|frame| frame.start..frame.end)
        .collect()
}

fn object_runs(bytes: &[u8]) -> Vec<Vec<ObjectFrame>> {
    let mut runs = Vec::new();
    let mut position = 0usize;
    while position + 8 <= bytes.len() {
        let Some((first_end, _, _, _)) = object_frame(bytes, position) else {
            position += 1;
            continue;
        };
        let start = position;
        let mut at = position;
        let mut run = Vec::new();
        while let Some((end, family, class, object_id)) = object_frame(bytes, at) {
            run.push(ObjectFrame {
                start: at,
                end,
                family,
                class,
                object_id,
            });
            at = end;
        }
        if run.len() < 2 {
            position = start + 1;
            continue;
        }
        runs.push(run);
        position = at.max(first_end);
    }
    runs
}

fn record_references(record: &B5Record) -> Vec<u32> {
    let mut position = 0;
    let Some(count) = counted_cardinality(&record.payload, &mut position) else {
        return Vec::new();
    };
    (0..count)
        .map_while(|_| wire::object_ref(&record.payload, &mut position, true))
        .collect()
}

fn topology_surface_references(records: &[B5Record]) -> HashSet<u32> {
    records
        .iter()
        .filter_map(|record| {
            let references = record_references(record);
            match record.class {
                0x5f => references.first().copied(),
                0x62 => references.last().copied(),
                _ => None,
            }
        })
        .collect()
}

fn is_referenced_geometry_class(family: u8, class: u8) -> bool {
    (family == 0xa8 && matches!(class, 0x25 | 0x32 | 0x34))
        || (family == 0xb5 && (matches!(class, 0x18..=0x21) || is_surface_class(class)))
}

fn is_reference_dependency_class(family: u8, class: u8) -> bool {
    is_referenced_geometry_class(family, class)
        || (family == 0xb5 && matches!(class, 0x05 | 0x06 | 0x23..=0x25 | 0x5d))
}

fn is_surface_class(class: u8) -> bool {
    matches!(
        class,
        0x27 | 0x28 | 0x29 | 0x2b | 0x2c | 0x2d | 0x2e | 0x30 | 0x31 | 0x34 | 0x37 | 0x38 | 0x3b
    )
}

fn is_opaque_surface_class(class: u8) -> bool {
    matches!(class, 0x2c | 0x2e | 0x30 | 0x37 | 0x38 | 0x3b)
}

fn object_frame(bytes: &[u8], start: usize) -> Option<(usize, u8, u8, u32)> {
    if !matches!(bytes.get(start + 1), Some(0x03 | 0x13 | 0x83)) {
        return None;
    }
    let family = *bytes.get(start)?;
    let class = *bytes.get(start + 2)?;
    let (header, length, object_id) = match family {
        0xb5 => (
            8usize,
            usize::from(*bytes.get(start + 3)?),
            u32::from_le_bytes(bytes.get(start + 4..start + 8)?.try_into().ok()?),
        ),
        0xa8 => (
            11usize,
            usize::try_from(u32::from_le_bytes(
                bytes.get(start + 3..start + 7)?.try_into().ok()?,
            ))
            .ok()?,
            u32::from_le_bytes(bytes.get(start + 7..start + 11)?.try_into().ok()?),
        ),
        _ => return None,
    };
    let end = start.checked_add(header)?.checked_add(length)?;
    (end <= bytes.len()).then_some((end, family, class, object_id))
}

fn is_topology_class(class: u8) -> bool {
    matches!(
        class,
        0x0e | 0x0f | 0x18 | 0x20 | 0x21 | 0x27 | 0x28 | 0x29 | 0x2b | 0x2d | 0x5e | 0x5f | 0x62
    )
}

fn parse_face(
    record: &B5Record,
    loops: &BTreeMap<u32, B5Loop>,
    surfaces: &BTreeMap<u32, B5Surface>,
) -> Option<B5Face> {
    let references = face_references(record)?;
    let surface = *references.first()?;
    if !surfaces.contains_key(&surface) {
        return None;
    }
    let loop_ids: Vec<u32> = references[1..]
        .iter()
        .copied()
        .filter(|reference| loops.contains_key(reference))
        .collect();
    if loop_ids.is_empty() || loop_ids.len() + 1 != references.len() {
        return None;
    }
    Some(B5Face {
        object_id: record.object_id,
        surface,
        loops: loop_ids,
    })
}

fn face_references(record: &B5Record) -> Option<Vec<u32>> {
    if record.class != 0x5f {
        return None;
    }
    if let Some(count) = record
        .payload
        .first()
        .and_then(|lead| lead.checked_sub(0x80))
    {
        let mut position = 1;
        let references = (0..count)
            .map(|_| wire::object_ref(&record.payload, &mut position, true))
            .collect::<Option<Vec<_>>>()?;
        if position < record.payload.len() && record.payload[position] != 0x05 {
            return None;
        }
        Some(references)
    } else {
        uncounted_references(&record.payload)
    }
}

/// Read each face's leading surface reference independently of its loop grammar.
pub(crate) fn face_surface_references(bytes: &[u8]) -> Vec<(u32, u32)> {
    records(bytes)
        .into_iter()
        .filter_map(|record| {
            if record.class != 0x5f {
                return None;
            }
            let mut position = usize::from(*record.payload.first()? >= 0x80);
            if position == 1 && record.payload[0] == 0x80 {
                return None;
            }
            let surface = wire::object_ref(&record.payload, &mut position, true)?;
            Some((record.object_id, surface))
        })
        .collect()
}

fn parse_loop(
    record: &B5Record,
    by_id: &HashMap<u32, &B5Record>,
    parsed_pcurves: &BTreeMap<u32, B5Pcurve>,
    opaque_pcurves: &BTreeMap<u32, B5OpaquePcurve>,
    implicit_pcurves: &BTreeMap<u32, u32>,
    surfaces: &BTreeMap<u32, B5Surface>,
) -> Option<B5Loop> {
    let references = loop_references(record)?;
    let count = references.len();
    let surface = *references.last()?;
    if !surfaces.contains_key(&surface) {
        return None;
    }
    let mut pcurves = Vec::with_capacity((count - 1) / 2);
    let mut edges = Vec::with_capacity((count - 1) / 2);
    for pair in references[..count - 1].chunks_exact(2) {
        if (!parsed_pcurves.contains_key(&pair[0])
            && !opaque_pcurves.contains_key(&pair[0])
            && implicit_pcurves.get(&pair[0]) != Some(&surface))
            || by_id.get(&pair[1])?.class != 0x5e
        {
            return None;
        }
        pcurves.push(pair[0]);
        edges.push(pair[1]);
    }
    Some(B5Loop {
        object_id: record.object_id,
        pcurves,
        edges,
        surface,
    })
}

fn loop_references(record: &B5Record) -> Option<Vec<u32>> {
    (record.class == 0x62).then_some(())?;
    let mut position = 0;
    let count = counted_cardinality(&record.payload, &mut position)?;
    if count < 3 || count % 2 == 0 {
        return None;
    }
    let references = (0..count)
        .map(|_| wire::object_ref(&record.payload, &mut position, true))
        .collect::<Option<Vec<_>>>()?;
    let edge_count = (count - 1) / 2;
    if position < record.payload.len()
        && (counted_cardinality(&record.payload, &mut position)? != edge_count
            || record.payload.get(position) != Some(&0x05))
    {
        return None;
    }
    Some(references)
}

fn counted_cardinality(bytes: &[u8], position: &mut usize) -> Option<usize> {
    let lead = *bytes.get(*position)?;
    if lead >= 0x80 {
        *position += 1;
        Some(usize::from(lead - 0x80))
    } else {
        usize::try_from(wire::object_ref(bytes, position, true)?).ok()
    }
}

fn uncounted_references(bytes: &[u8]) -> Option<Vec<u32>> {
    let mut position = 0;
    let mut references = Vec::new();
    while position < bytes.len() {
        references.push(wire::object_ref(bytes, &mut position, true)?);
    }
    Some(references)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn face_surface_references_do_not_require_resolved_loops() {
        let mut bytes = Vec::new();
        for (object_id, surface_id) in [(500u32, 100u8), (501, 100), (500, 101)] {
            bytes.extend_from_slice(&[0xb5, 0x03, 0x5f, 5]);
            bytes.extend_from_slice(&object_id.to_le_bytes());
            bytes.extend_from_slice(&[0x82, 0x08, surface_id, 0x00, 0x05]);
        }
        assert_eq!(
            face_surface_references(&bytes),
            vec![(500, 100), (501, 100), (500, 101)]
        );
    }

    #[test]
    fn one_edge_loop_closes_on_one_native_vertex() {
        let loop_ = B5Loop {
            object_id: 1,
            pcurves: vec![2],
            edges: vec![3],
            surface: 4,
        };

        assert_eq!(
            loop_chain_senses(&loop_, &BTreeMap::from([(3, [0, 0])])),
            Some(vec![false])
        );
        assert_eq!(
            loop_chain_senses(&loop_, &BTreeMap::from([(3, [0, 1])])),
            None
        );
    }

    #[test]
    fn opaque_pcurve_occurrences_defer_endpoint_binding_to_native_edges() {
        let loop_ = B5Loop {
            object_id: 1,
            pcurves: vec![2],
            edges: vec![3],
            surface: 4,
        };
        assert_eq!(
            bind_edge_vertices(&BTreeMap::from([(1, loop_)]), &BTreeMap::new(), &[]),
            Some(BTreeMap::new())
        );
    }

    #[test]
    fn circle_pcurve_rejects_unbounded_subdivision_counts() {
        let mut payload = vec![0x81, 0x81];
        payload.extend_from_slice(&[0; 16]);
        payload.extend_from_slice(&[0x05, 0x05]);
        for value in [1.0e-300, 0.0, 1.0, 1.0, 0.0] {
            payload.extend_from_slice(&f64::to_le_bytes(value));
        }
        let record = B5Record {
            offset: 0,
            family: 0xb5,
            class: 0x19,
            object_id: 0,
            payload,
        };
        assert!(parse_circle_pcurve(&record).is_none());
    }

    #[test]
    fn sparse_reference_tokens_fill_selected_id_bytes() {
        let mut position = 0;
        assert_eq!(
            wire::object_ref(&[0x28, 0x34, 0x02], &mut position, true),
            Some(0x02_0034)
        );
        assert_eq!(position, 3);
        position = 0;
        assert_eq!(
            wire::object_ref(&[0x20, 0x07], &mut position, true),
            Some(0x07_0000)
        );
        assert_eq!(position, 2);
        position = 0;
        assert_eq!(wire::object_ref(&[0x8b], &mut position, true), Some(11));
        assert_eq!(position, 1);
    }

    #[test]
    fn counted_cardinality_widens_with_reference_tokens() {
        let mut position = 0;
        assert_eq!(counted_cardinality(&[0x81], &mut position), Some(1));
        assert_eq!(position, 1);
        position = 0;
        assert_eq!(counted_cardinality(&[0x08, 0x81], &mut position), Some(129));
        assert_eq!(position, 2);
        position = 0;
        assert_eq!(
            counted_cardinality(&[0x18, 0x35, 0x01], &mut position),
            Some(309)
        );
        assert_eq!(position, 3);
    }

    #[test]
    fn revolution_surface_accepts_sparse_profile_reference() {
        let mut payload = vec![0; 175];
        payload[1..4].copy_from_slice(&[0x30, 0x86, 0x16]);
        payload[4..12].copy_from_slice(&1.0f64.to_le_bytes());
        payload[76..84].copy_from_slice(&1.0f64.to_le_bytes());
        payload[134..142].copy_from_slice(&2.0f64.to_le_bytes());
        let record = B5Record {
            offset: 0,
            family: 0xb5,
            class: 0x2d,
            object_id: 0x16_8601,
            payload,
        };
        assert_eq!(
            parse_surface(&record),
            Some(B5Surface::Revolution {
                profile_curve: 0x16_8600,
                axis_origin: [1.0, 0.0, 0.0],
                axis_direction: [1.0, 0.0, 0.0],
                gauge_radius: 2.0,
            })
        );
    }

    #[test]
    fn cone_surface_reads_the_native_slant_chart() {
        let mut payload = vec![0; 185];
        payload[0] = 0x80;
        for (offset, values) in [
            (1, [1.0f64, 2.0, 3.0]),
            (25, [1.0, 0.0, 0.0]),
            (49, [0.0, 1.0, 0.0]),
            (73, [0.0, 0.0, 1.0]),
        ] {
            for (index, value) in values.into_iter().enumerate() {
                payload[offset + 8 * index..offset + 8 * index + 8]
                    .copy_from_slice(&value.to_le_bytes());
            }
        }
        for (offset, value) in [
            (97, 0.25f64),
            (121, 0.5),
            (129, 0.0),
            (137, 8.0),
            (145, 3.0),
        ] {
            payload[offset..offset + 8].copy_from_slice(&value.to_le_bytes());
        }
        let record = B5Record {
            offset: 0,
            family: 0xb5,
            class: 0x29,
            object_id: 7,
            payload,
        };
        assert_eq!(
            parse_surface(&record),
            Some(B5Surface::Cone {
                apex: [1.0, 2.0, 3.0],
                direction_x: [1.0, 0.0, 0.0],
                direction_y: [0.0, 1.0, 0.0],
                axis: [0.0, 0.0, 1.0],
                half_angle: 0.25,
                angular_offset: 0.5,
                slant_range: [0.0, 8.0],
                angular_scale: 3.0,
            })
        );
    }

    #[test]
    fn torus_surface_separates_geometric_radii_from_chart_scales() {
        let mut payload = vec![0; 201];
        payload[0] = 0x80;
        for (offset, values) in [
            (1, [1.0f64, 2.0, 3.0]),
            (25, [1.0, 0.0, 0.0]),
            (49, [0.0, 1.0, 0.0]),
            (73, [0.0, 0.0, 1.0]),
        ] {
            for (index, value) in values.into_iter().enumerate() {
                payload[offset + 8 * index..offset + 8 * index + 8]
                    .copy_from_slice(&value.to_le_bytes());
            }
        }
        payload[97..105].copy_from_slice(&5.0f64.to_le_bytes());
        payload[105..113].copy_from_slice(&2.0f64.to_le_bytes());
        payload[177..185].copy_from_slice(&4.0f64.to_le_bytes());
        payload[185..193].copy_from_slice(&3.0f64.to_le_bytes());
        let record = B5Record {
            offset: 0,
            family: 0xb5,
            class: 0x2b,
            object_id: 7,
            payload,
        };
        assert_eq!(
            parse_surface(&record),
            Some(B5Surface::Torus {
                center: [1.0, 2.0, 3.0],
                direction_x: [1.0, 0.0, 0.0],
                direction_y: [0.0, 1.0, 0.0],
                axis: [0.0, 0.0, 1.0],
                major_radius: 5.0,
                minor_radius: 2.0,
                major_scale: 4.0,
                minor_scale: 3.0,
            })
        );
    }

    #[test]
    fn circle_pcurve_preserves_arc_length_parameterization() {
        let mut payload = vec![0x81, 0x18, 0x34, 0x12];
        for value in [0.0, 0.0, 2.0, 0.0, 2.0 * std::f64::consts::PI, 1.0, 0.0] {
            if payload.len() == 20 {
                payload.extend_from_slice(&[0x05, 0x05]);
            }
            payload.extend_from_slice(&value.to_le_bytes());
        }
        let record = B5Record {
            offset: 0,
            family: 0xb5,
            class: 0x19,
            object_id: 0x1235,
            payload,
        };
        let pcurve = parse_circle_pcurve(&record).expect("circle pcurve");
        assert_eq!(pcurve.surface, 0x1234);
        assert_eq!(pcurve.degree, 2);
        assert_eq!(
            pcurve.distinct_knots,
            [0.0, std::f64::consts::PI, 2.0 * std::f64::consts::PI]
        );
        assert_eq!(pcurve.multiplicities, [3, 2, 3]);
        assert_eq!(pcurve.control_points.len(), 5);
        let weights = pcurve.weights.expect("rational weights");
        assert_eq!(weights.len(), 5);
        assert!((weights[1] - std::f64::consts::FRAC_1_SQRT_2).abs() < 1e-12);
        assert!((pcurve.control_points[0][0] - 2.0).abs() < 1e-12);
        assert!((pcurve.control_points[4][0] + 2.0).abs() < 1e-12);
    }

    #[test]
    fn pcurve_evaluation_preserves_the_native_parameter() {
        let pcurve = B5Pcurve {
            object_id: 1,
            surface: 2,
            degree: 1,
            distinct_knots: vec![-1.0, 1.0],
            multiplicities: vec![2, 2],
            control_points: vec![[2.0, 3.0], [4.0, 7.0]],
            weights: None,
            lifted_endpoints: None,
        };
        assert_eq!(evaluate_pcurve(&pcurve, 0.0), Some([3.0, 5.0]));
        assert_eq!(evaluate_pcurve(&pcurve, -1.0), Some([2.0, 3.0]));
        assert_eq!(evaluate_pcurve(&pcurve, 1.0), Some([4.0, 7.0]));
    }

    #[test]
    fn opaque_conic_pcurves_retain_support_identity_and_payload() {
        let mut ellipse = vec![0x81, 0x82];
        ellipse.extend_from_slice(&[0; 16]);
        ellipse.extend_from_slice(&[0x05, 0x05]);
        ellipse.extend_from_slice(&[0; 56]);
        let ellipse_record = B5Record {
            offset: 0,
            family: 0xb5,
            class: 0x1a,
            object_id: 7,
            payload: ellipse.clone(),
        };
        assert_eq!(
            parse_opaque_pcurve(&ellipse_record),
            Some(B5OpaquePcurve {
                object_id: 7,
                surface: 2,
                class: 0x1a,
                payload: ellipse,
            })
        );

        let mut class_1d = vec![0x81, 0x82];
        class_1d.extend_from_slice(&[0; 32]);
        class_1d.extend_from_slice(&[0x05, 0x81]);
        class_1d.extend_from_slice(&[0; 24]);
        class_1d.push(0x1d);
        class_1d.extend_from_slice(&[0; 40]);
        let class_1d_record = B5Record {
            class: 0x1d,
            payload: class_1d.clone(),
            ..ellipse_record
        };
        assert_eq!(
            parse_opaque_pcurve(&class_1d_record),
            Some(B5OpaquePcurve {
                object_id: 7,
                surface: 2,
                class: 0x1d,
                payload: class_1d,
            })
        );
    }

    #[test]
    fn freeform_surface_alias_requires_one_complete_reference() {
        let alias = B5Record {
            offset: 0,
            family: 0xb5,
            class: 0x2e,
            object_id: 9,
            payload: vec![0x38, 0x34, 0x12, 0x00],
        };
        assert_eq!(surface_alias_target(&alias), Some(0x1234));

        let counted = B5Record {
            payload: vec![0x81, 0x38, 0x34, 0x12, 0x00],
            ..alias.clone()
        };
        assert_eq!(surface_alias_target(&counted), Some(0x1234));

        let mut tailed = alias.clone();
        tailed.payload.push(0x05);
        assert_eq!(surface_alias_target(&tailed), None);
    }

    #[test]
    fn offset_surface_separates_result_carrier_source_and_bounds() {
        let carrier = B5Surface::Plane {
            origin: [0.0; 3],
            direction_u: [1.0, 0.0, 0.0],
            direction_v: [0.0, 1.0, 0.0],
        };
        let surfaces = BTreeMap::from([(2, carrier)]);
        let mut payload = vec![0x82, 0x82, 0x83];
        payload.extend_from_slice(&(-0.5f64).to_le_bytes());
        payload.push(0x15);
        for value in [-2.0f64, 3.0, -4.0, 5.0] {
            payload.extend_from_slice(&value.to_le_bytes());
        }
        let record = B5Record {
            offset: 0,
            family: 0xb5,
            class: 0x30,
            object_id: 9,
            payload,
        };
        assert_eq!(
            parse_offset_surface(&record, &surfaces, &BTreeMap::new(), &HashMap::new()),
            Some(B5OffsetSurface {
                object_id: 9,
                carrier_surface: 2,
                source_surface: 3,
                distance: -0.5,
                carrier_kind: 0x15,
                parameter_bounds: [[-2.0, 3.0], [-4.0, 5.0]],
            })
        );
    }

    #[test]
    fn offset_surface_accepts_an_identity_checked_class_31_cache() {
        assert!(is_referenced_geometry_class(0xb5, 0x31));
        let source = B5Surface::Nurbs(NurbsSurface {
            u_degree: 1,
            v_degree: 1,
            u_count: 2,
            v_count: 2,
            control_points: vec![cadmpeg_ir::math::Point3::new(0.0, 0.0, 0.0); 4],
            u_knots: vec![0.0, 0.0, 1.0, 1.0],
            v_knots: vec![0.0, 0.0, 1.0, 1.0],
            weights: None,
            u_periodic: false,
            v_periodic: false,
        });
        let surfaces = BTreeMap::from([(3, source.clone()), (4, source)]);
        let mut cache_payload = vec![0x81, 0x84];
        for value in [-0.5f64, -2.0, -4.0, 3.0, 5.0] {
            cache_payload.extend_from_slice(&value.to_le_bytes());
        }
        let cache = B5Record {
            offset: 0,
            family: 0xb5,
            class: 0x31,
            object_id: 2,
            payload: cache_payload,
        };
        let records = HashMap::from([(2, &cache)]);
        let mut payload = vec![0x82, 0x82, 0x83];
        payload.extend_from_slice(&(-0.5f64).to_le_bytes());
        payload.push(0x01);
        for value in [-2.0f64, 3.0, -4.0, 5.0] {
            payload.extend_from_slice(&value.to_le_bytes());
        }
        let record = B5Record {
            offset: 0,
            family: 0xb5,
            class: 0x30,
            object_id: 9,
            payload,
        };

        assert_eq!(
            parse_offset_surface(&record, &surfaces, &BTreeMap::new(), &records),
            Some(B5OffsetSurface {
                object_id: 9,
                carrier_surface: 2,
                source_surface: 3,
                distance: -0.5,
                carrier_kind: 0x01,
                parameter_bounds: [[-2.0, 3.0], [-4.0, 5.0]],
            })
        );
    }

    #[test]
    fn extrusion_surface_binds_two_mapped_directrix_supports() {
        let mut pcurve_payload = vec![0x81, 0x86, 0x05];
        for value in [2.0f64, -3.0, 4.0] {
            pcurve_payload.extend_from_slice(&value.to_le_bytes());
        }
        let pcurve = B5Record {
            offset: 0,
            family: 0xb5,
            class: 0x18,
            object_id: 3,
            payload: pcurve_payload,
        };
        let mut wrapper_payload = vec![0x81, 0x83, 0x81, 0x01];
        for value in [-3.0f64, 4.0, 0.0] {
            wrapper_payload.extend_from_slice(&value.to_le_bytes());
        }
        wrapper_payload.push(0x01);
        let wrapper = B5Record {
            offset: 0,
            family: 0xb5,
            class: 0x24,
            object_id: 2,
            payload: wrapper_payload,
        };
        let mut directrix_payload = vec![0x82, 0x82, 0x84, 0x00];
        for value in [-3.0f64, 4.0, 0.01] {
            directrix_payload.extend_from_slice(&value.to_le_bytes());
        }
        directrix_payload.push(0x01);
        let directrix = B5Record {
            offset: 0,
            family: 0xa8,
            class: 0x25,
            object_id: 5,
            payload: directrix_payload,
        };
        let records = HashMap::from([(2, &wrapper), (3, &pcurve), (5, &directrix)]);
        let pcurves = BTreeMap::from([(4, (7, [10.0, 20.0]))]);
        let mut payload = vec![0x81, 0x85];
        for value in [0.0f64, 0.0, 1.0, -2.0, 6.0, 1.0, 0.0, -3.0, 4.0] {
            payload.extend_from_slice(&value.to_le_bytes());
        }
        payload.extend_from_slice(&[0x05, 0x05]);
        let record = B5Record {
            offset: 0,
            family: 0xb5,
            class: 0x2c,
            object_id: 8,
            payload,
        };

        assert_eq!(
            parse_extrusion_surface(&record, &records, &pcurves),
            Some(B5ExtrusionSurface {
                object_id: 8,
                direction: [0.0, 0.0, 1.0],
                parameter_bounds: [[-2.0, 6.0], [-3.0, 4.0]],
                directrix: B5ExtrusionDirectrix {
                    object_id: 5,
                    supports: [(6, 3, [-3.0, 4.0]), (7, 4, [10.0, 20.0])],
                    parameter_range: [-3.0, 4.0],
                    cache_fit_tolerance: 0.01,
                },
            })
        );
    }

    #[test]
    fn supported_surface_preserves_ordered_support_pcurves() {
        let pcurve0 = B5Record {
            offset: 0,
            family: 0xb5,
            class: 0x18,
            object_id: 5,
            payload: vec![0x81, 0x83],
        };
        let mut payload = vec![0x85, 0x82, 0x83, 0x84, 0x85, 0x86, 0x09, 0x05];
        payload.extend_from_slice(&2.5f64.to_le_bytes());
        payload.extend_from_slice(&[0x03, 0x05]);
        payload.extend_from_slice(&0.0f64.to_le_bytes());
        payload.extend_from_slice(&[0x01, 0x05]);
        let record = B5Record {
            class: 0x37,
            object_id: 7,
            payload,
            ..pcurve0.clone()
        };
        assert_eq!(
            parse_supported_surface(&record),
            Some(B5SupportedSurface {
                object_id: 7,
                class: 0x37,
                carrier_surface: 2,
                support_surfaces: [3, 4],
                support_pcurves: [5, 6],
                controls: [0x09, 0x05, 0x03, 0x05, 0x01, 0x05],
                scalars: [2.5, 0.0],
            })
        );
        let construction = parse_supported_surface(&record).expect("supported surface");
        let pcurve0 = B5Record {
            object_id: 5,
            payload: vec![0x81, 0x83],
            ..pcurve0.clone()
        };
        let pcurve1 = B5Record {
            object_id: 6,
            payload: vec![0x81, 0x84],
            ..pcurve0.clone()
        };
        let records = HashMap::from([(5, &pcurve0), (6, &pcurve1)]);
        assert!(supported_surface_pcurves_match(&construction, &records));

        let wrong = B5Record {
            payload: vec![0x81, 0x82],
            ..pcurve1
        };
        assert!(!supported_surface_pcurves_match(
            &construction,
            &HashMap::from([(5, &pcurve0), (6, &wrong)])
        ));
    }

    #[test]
    fn record_walk_includes_wide_header_loop_nodes() {
        let mut bytes = vec![0xa8, 0x03, 0x62];
        bytes.extend_from_slice(&3u32.to_le_bytes());
        bytes.extend_from_slice(&7u32.to_le_bytes());
        bytes.extend_from_slice(&[0x83, 0x81, 0x82]);
        bytes.extend_from_slice(&[0xb5, 0x03, 0x5e, 0x00]);
        bytes.extend_from_slice(&8u32.to_le_bytes());
        assert_eq!(
            records(&bytes),
            vec![
                B5Record {
                    offset: 0,
                    family: 0xa8,
                    class: 0x62,
                    object_id: 7,
                    payload: vec![0x83, 0x81, 0x82],
                },
                B5Record {
                    offset: 14,
                    family: 0xb5,
                    class: 0x5e,
                    object_id: 8,
                    payload: Vec::new(),
                },
            ]
        );
    }

    #[test]
    fn record_walk_retains_opaque_a8_surface_nodes() {
        let mut bytes = vec![0xa8, 0x03, 0x34];
        bytes.extend_from_slice(&3u32.to_le_bytes());
        bytes.extend_from_slice(&7u32.to_le_bytes());
        bytes.extend_from_slice(&[1, 2, 3]);
        bytes.extend_from_slice(&[0xb5, 0x03, 0x5e, 0x00]);
        bytes.extend_from_slice(&8u32.to_le_bytes());
        let records = records(&bytes);
        assert_eq!(records.len(), 2);
        assert_eq!(records[0].family, 0xa8);
        assert_eq!(records[0].class, 0x34);
        assert_eq!(records[0].object_id, 7);
        assert_eq!(records[0].payload, [1, 2, 3]);
        assert_eq!(
            surface_node(&records[0], None),
            Some(B5Surface::Unknown {
                family: 0xa8,
                class: 0x34,
                payload: vec![1, 2, 3],
            })
        );
    }

    #[test]
    fn record_walk_descends_into_length_bounded_a8_wrappers() {
        let mut payload = vec![0xb5, 0x03, 0x27, 0x00];
        payload.extend_from_slice(&1u32.to_le_bytes());
        payload.extend_from_slice(&[0xb5, 0x03, 0x5e, 0x00]);
        payload.extend_from_slice(&2u32.to_le_bytes());

        let mut bytes = vec![0xa8, 0x03, 0x34];
        bytes.extend_from_slice(&16u32.to_le_bytes());
        bytes.extend_from_slice(&7u32.to_le_bytes());
        bytes.extend_from_slice(&payload);
        bytes.extend_from_slice(&[0xb5, 0x03, 0x5e, 0x00]);
        bytes.extend_from_slice(&3u32.to_le_bytes());

        assert_eq!(
            records(&bytes)
                .iter()
                .map(|record| (record.offset, record.object_id, record.class))
                .collect::<Vec<_>>(),
            vec![(11, 1, 0x27), (19, 2, 0x5e), (0, 7, 0x34), (27, 3, 0x5e)]
        );
    }

    #[test]
    fn record_walk_crosses_alternate_flag_bridge_records() {
        let mut bytes = vec![0xb5, 0x03, 0x27, 0x00];
        bytes.extend_from_slice(&1u32.to_le_bytes());
        bytes.extend_from_slice(&[0xb5, 0x13, 0x5b, 0x00]);
        bytes.extend_from_slice(&2u32.to_le_bytes());
        bytes.extend_from_slice(&[0xb5, 0x03, 0x5e, 0x00]);
        bytes.extend_from_slice(&3u32.to_le_bytes());
        assert_eq!(
            records(&bytes)
                .iter()
                .map(|record| (record.object_id, record.class))
                .collect::<Vec<_>>(),
            vec![(1, 0x27), (3, 0x5e)]
        );
    }

    #[test]
    fn record_walk_admits_unique_isolated_geometry_by_topology_reference() {
        fn append(bytes: &mut Vec<u8>, class: u8, object_id: u32, payload: &[u8]) {
            bytes.extend_from_slice(&[
                0xb5,
                0x03,
                class,
                u8::try_from(payload.len()).expect("test payload fits the B5 length lane"),
            ]);
            bytes.extend_from_slice(&object_id.to_le_bytes());
            bytes.extend_from_slice(payload);
        }

        let mut bytes = Vec::new();
        append(&mut bytes, 0x27, 1, &[]);
        bytes.push(0xff);
        append(&mut bytes, 0x19, 2, &[]);
        bytes.push(0xff);
        append(&mut bytes, 0x62, 4, &[0x83, 0x82, 0x83, 0x81]);
        append(&mut bytes, 0x5e, 3, &[]);
        append(&mut bytes, 0x5f, 5, &[0x82, 0x81, 0x84]);

        let parsed = records(&bytes);
        assert_eq!(
            parsed
                .iter()
                .map(|record| (record.object_id, record.class))
                .collect::<std::collections::BTreeSet<_>>(),
            std::collections::BTreeSet::from([
                (1, 0x27),
                (2, 0x19),
                (3, 0x5e),
                (4, 0x62),
                (5, 0x5f),
            ])
        );

        bytes.push(0xff);
        append(&mut bytes, 0x19, 2, &[0x01]);
        assert!(!records(&bytes).iter().any(|record| record.object_id == 2));
    }

    #[test]
    fn record_walk_closes_native_vertex_incidence_dependencies() {
        fn append(bytes: &mut Vec<u8>, class: u8, object_id: u32, payload: &[u8]) {
            bytes.extend_from_slice(&[0xb5, 0x03, class, payload.len() as u8]);
            bytes.extend_from_slice(&object_id.to_le_bytes());
            bytes.extend_from_slice(payload);
        }

        let mut bytes = Vec::new();
        append(&mut bytes, 0x18, 2, &[0x81, 0x81]);
        bytes.push(0xff);
        append(&mut bytes, 0x5d, 6, &[0x81, 0x87, 0x00]);
        bytes.push(0xff);
        append(&mut bytes, 0x05, 7, &[0x81, 0x88]);
        bytes.push(0xff);
        let mut parameter = vec![0x81, 0x82, 0x81];
        parameter.extend_from_slice(&0.5f64.to_le_bytes());
        parameter.push(0x05);
        append(&mut bytes, 0x06, 8, &parameter);
        bytes.push(0xff);
        append(
            &mut bytes,
            0x5e,
            10,
            &[0x85, 0x82, 0x86, 0x86, 0x88, 0x88, 0x21],
        );
        append(&mut bytes, 0x5f, 11, &[]);

        assert_eq!(
            records(&bytes)
                .iter()
                .map(|record| (record.object_id, record.class))
                .collect::<std::collections::BTreeSet<_>>(),
            std::collections::BTreeSet::from([
                (2, 0x18),
                (6, 0x5d),
                (7, 0x05),
                (8, 0x06),
                (10, 0x5e),
                (11, 0x5f),
            ])
        );
    }

    #[test]
    fn native_vertex_graph_accepts_distinct_loci_for_one_tolerant_vertex() {
        let points = [
            [0.0, 0.0, 0.0],
            [1.0, 0.0, 0.0],
            [1.0, 5e-3, 0.0],
            [0.0, 5e-3, 0.0],
        ];
        let constraints = [([10, 11], [0, 1]), ([11, 12], [2, 3]), ([12, 10], [3, 0])];
        let adjacency = HashMap::from([(10, vec![0, 2]), (11, vec![0, 1]), (12, vec![1, 2])]);
        let mapping = propagate_vertex_points(&constraints, &adjacency, &points);
        assert_eq!(
            mapping,
            HashMap::from([(10, 0usize), (11, 1usize), (12, 3usize)])
        );
    }

    #[test]
    fn edge_record_exposes_native_start_and_end_vertex_refs() {
        let record = B5Record {
            offset: 0,
            family: 0xb5,
            class: 0x5e,
            object_id: 17,
            payload: vec![0x85, 0x92, 0x8f, 0x95, 0x93, 0x94, 0x21],
        };
        assert_eq!(parse_edge_vertex_refs(&record), Some([15, 21]));
        assert_eq!(parse_edge_parameter_refs(&record), Some([19, 20]));

        let mut standard = record;
        *standard.payload.last_mut().expect("tail") = 0x01;
        assert_eq!(parse_edge_vertex_refs(&standard), Some([15, 21]));
        for tail in [0x22, 0x25, 0x29, 0x2a] {
            *standard.payload.last_mut().expect("tail") = tail;
            assert_eq!(parse_edge_vertex_refs(&standard), Some([15, 21]));
        }
        *standard.payload.last_mut().expect("tail") = 0x01;

        let mut bytes = vec![0xb5, 0x03, 0x5e, 7];
        bytes.extend_from_slice(&standard.object_id.to_le_bytes());
        bytes.extend_from_slice(&standard.payload);
        assert_eq!(
            edge_vertex_references(&bytes),
            BTreeMap::from([(17, [15, 21])])
        );
    }

    #[test]
    fn loop_and_endpoint_incidences_bind_an_unframed_pcurve_occurrence() {
        let incidence_payload = |parameter: f64, control| {
            let mut payload = vec![0x81, 0x89, 0x81];
            payload.extend_from_slice(&parameter.to_le_bytes());
            payload.push(control);
            payload
        };
        let records = vec![
            B5Record {
                offset: 0,
                family: 0xb5,
                class: 0x62,
                object_id: 1,
                payload: vec![0x83, 0x89, 0x8a, 0x8b],
            },
            B5Record {
                offset: 1,
                family: 0xb5,
                class: 0x5e,
                object_id: 10,
                payload: vec![0x85, 0x8c, 0x8d, 0x8e, 0x8f, 0x90, 0x21],
            },
            B5Record {
                offset: 2,
                family: 0xb5,
                class: 0x06,
                object_id: 15,
                payload: incidence_payload(0.0, 0x15),
            },
            B5Record {
                offset: 3,
                family: 0xb5,
                class: 0x06,
                object_id: 16,
                payload: incidence_payload(1.0, 0x05),
            },
        ];
        let by_id = records
            .iter()
            .map(|record| (record.object_id, record))
            .collect();
        let surfaces = BTreeMap::from([(
            11,
            B5Surface::Unknown {
                family: 0xb5,
                class: 0x34,
                payload: Vec::new(),
            },
        )]);

        assert_eq!(
            implicit_pcurve_bindings(
                &records,
                &by_id,
                &BTreeMap::new(),
                &BTreeMap::new(),
                &surfaces,
            ),
            BTreeMap::from([(9, 11)])
        );
    }

    #[test]
    fn parameter_incidence_retains_aligned_compact_controls() {
        let mut payload = vec![0x82, 0x89, 0x8a, 0x82];
        payload.extend_from_slice(&1.25f64.to_le_bytes());
        payload.push(0x15);
        payload.extend_from_slice(&2.5f64.to_le_bytes());
        payload.push(0x2d);
        let incidence = parameter_incidence(&B5Record {
            offset: 0,
            family: 0xb5,
            class: 0x06,
            object_id: 17,
            payload,
        })
        .expect("parameter incidence");

        assert_eq!(incidence.object_id, 17);
        assert_eq!(incidence.curves, [9, 10]);
        assert_eq!(incidence.parameters, [1.25, 2.5]);
        assert_eq!(incidence.controls, [5, 11]);
    }

    #[test]
    fn loop_and_edge_curve_wrapper_bind_an_unframed_pcurve_occurrence() {
        let records = vec![
            B5Record {
                offset: 0,
                family: 0xb5,
                class: 0x62,
                object_id: 1,
                payload: vec![0x83, 0x89, 0x8a, 0x8b],
            },
            B5Record {
                offset: 1,
                family: 0xb5,
                class: 0x5e,
                object_id: 10,
                payload: vec![0x85, 0x8c, 0x8d, 0x8e, 0x8f, 0x90, 0x22],
            },
            B5Record {
                offset: 2,
                family: 0xb5,
                class: 0x25,
                object_id: 12,
                payload: vec![0x82, 0x89, 0x91, 0x81],
            },
        ];
        let by_id = records
            .iter()
            .map(|record| (record.object_id, record))
            .collect();
        let surfaces = BTreeMap::from([(
            11,
            B5Surface::Unknown {
                family: 0xb5,
                class: 0x34,
                payload: Vec::new(),
            },
        )]);

        assert_eq!(
            implicit_pcurve_bindings(
                &records,
                &by_id,
                &BTreeMap::new(),
                &BTreeMap::new(),
                &surfaces,
            ),
            BTreeMap::from([(9, 11)])
        );
    }
}
