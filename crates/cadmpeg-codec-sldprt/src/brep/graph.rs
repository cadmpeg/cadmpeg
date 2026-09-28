// SPDX-License-Identifier: Apache-2.0
//! Build IR arenas from parsed Parasolid topology records and carriers.
//!
//! The graph builder walks each face bridge through its loop and coedge rings,
//! resolves edge and vertex uses, closes emitted loops, and groups faces under
//! explicit body records. It derives one body hierarchy when those records are
//! absent. It also derives supported pcurves and periodic seams.

use cadmpeg_ir::features::FinitePoint3;
use cadmpeg_ir::math::Point3;
use std::collections::{HashMap, HashSet};
use std::hash::Hash;

use cadmpeg_core::decode::DecodeContext;
use cadmpeg_ir::annotations::{AnnotationBuilder, Annotations, StreamHandle};
use cadmpeg_ir::eval::{
    analytic_surface_parameters, nurbs_curve_parameter_domain, nurbs_curve_point_at,
    nurbs_pcurve_uv, nurbs_surface_isocurve, nurbs_surface_parameter_near_point,
    nurbs_surface_parameter_segment_chord_bound, nurbs_surface_parameter_within_tolerance,
    nurbs_surface_point, surface_point,
};
use cadmpeg_ir::geometry::{
    nurbs::{knots_nondecreasing, SurfaceParameterAxis},
    pcurve::{Pcurve, PcurveGeometry, PcurveNurbs, PolarPcurveNurbs},
    BlendCrossSection, BlendRadiusLaw, BlendSupport, Curve, CurveGeometry, ProceduralSurface,
    ProceduralSurfaceDefinition, SolvedCurveGeometry, SolvedSurfaceGeometry, Surface,
    SurfaceGeometry,
};
use cadmpeg_ir::ids::{
    BodyId, CoedgeId, CurveId, EdgeId, FaceId, LoopId, PcurveId, PointId, ProceduralSurfaceId,
    RegionId, ShellId, SurfaceId, VertexId,
};
use cadmpeg_ir::topology::{
    Body, BodyKind, Coedge, Edge, Face, Loop, Point, Region, Sense, Shell, Vertex,
};
use cadmpeg_ir::unknown::UnknownRecord;
use cadmpeg_ir::Exactness;

use super::attrib;
use super::blend::BlendSupportRef;
use super::entity;
use super::index::{scan_carriers, CarrierIndex, IndexedCurve};
use super::offset::OffsetCarrier;
use super::sweep::{self, SweepKind};
use super::topology;
use super::typed;
use super::{CurveCarrier, LEN_TO_MM};
use crate::parasolid::StreamHeader;

const EPS_NORMAL_NONZERO: f64 = 1.0e-12;
const EPS_PLANAR_DISTANCE: f64 = 1.0e-6;
const EPS_GEOMETRY_RESIDUAL: f64 = 1.0e-9;
const EPS_AXIS_ALIGNMENT: f64 = 1.0e-9;
const EPS_CIRCLE_RADIUS_MATCH: f64 = 1.0e-6;
const EPS_RADIUS_ABSOLUTE: f64 = 1.0e-6;
const EPS_RADIUS_RELATIVE: f64 = 1.0e-9;
const EPS_NURBS_WEIGHT: f64 = 1.0e-12;
const EPS_POINT_DISTANCE: f64 = 1.0e-12;
const EPS_SWEEP_EXTENT_ABSOLUTE_MM: f64 = 1.0e-6;
const EPS_SWEEP_EXTENT_RELATIVE: f64 = 1.0e-3;

/// Decoded B-rep arenas, provenance, and transfer statistics.
#[derive(Default)]
pub(crate) struct Brep {
    /// Source locations for decoded entities.
    pub(crate) annotations: Annotations,
    /// Top-level solid or sheet bodies.
    pub(crate) bodies: Vec<Body>,
    /// Solid regions / sheet regions owned by each body.
    pub(crate) regions: Vec<Region>,
    /// Shells owned by each region.
    pub(crate) shells: Vec<Shell>,
    /// Faces reached through face-use bridge records.
    pub(crate) faces: Vec<Face>,
    /// Loops reached through `00 0f` loop heads.
    pub(crate) loops: Vec<Loop>,
    /// Coedges in loop-ring order.
    pub(crate) coedges: Vec<Coedge>,
    /// Edges resolved from edge-use records.
    pub(crate) edges: Vec<Edge>,
    /// Vertices resolved from vertex-use and world-point records.
    pub(crate) vertices: Vec<Vertex>,
    /// World points converted to millimetres.
    pub(crate) points: Vec<Point>,
    /// Analytic, NURBS, or opaque support surfaces.
    pub(crate) surfaces: Vec<Surface>,
    /// Exact procedural constructions behind emitted support surfaces.
    pub(crate) procedural_surfaces: Vec<ProceduralSurface>,
    /// Analytic, NURBS, or opaque support curves.
    pub(crate) curves: Vec<Curve>,
    /// Pcurves derived for supported analytic and NURBS-boundary cases.
    pub(crate) pcurves: Vec<Pcurve>,
    /// Records whose carrier kind this codec does not type, retained as
    /// opaque payloads.
    pub(crate) unknowns: Vec<UnknownRecord>,
    /// Per-face RGB colors resolved from native entity records.
    pub(crate) face_colors: Vec<OwnedFaceColor>,
    /// Per-face producing-feature identities resolved from Parasolid attributes.
    pub(crate) face_atoms: Vec<attrib::FaceAtom>,
    /// Source-local sequence-to-attribute links carried by face bridge
    /// records. The decode boundary retains this map only for the active
    /// source because SWIFT identifiers resolve in that source namespace.
    pub(crate) face_bridge_sequences: Vec<(u32, u16)>,
    /// Source-local sequence-to-attribute links carried by edge-use records.
    /// The decode boundary retains this map only for the active source because
    /// SWIFT identifiers resolve in that source namespace.
    pub(crate) edge_use_sequences: Vec<(u32, u16)>,
    /// Source-local sequence-to-attribute links carried by vertex-use records.
    /// The decode boundary retains this map only for the active source because
    /// SWIFT identifiers resolve in that source namespace.
    pub(crate) vertex_use_sequences: Vec<(u32, u16)>,
    /// Body-to-history ordinals resolved from Parasolid attributes.
    pub(crate) body_modifiers: Vec<attrib::BodyModifier>,
    /// Refusals charged while building this B-rep, each naming the instance
    /// whose lane the reader refused. The model carries the absence of each
    /// refused carrier, so the decode continues and reports the loss.
    pub(crate) losses: Vec<cadmpeg_ir::report::loss::LossNote>,
    /// Loss accounting for this decode.
    pub(crate) stats: Stats,
}

/// A resolved face color retains the stream that admitted its source row.
///
/// Alternate configuration sites are merged into one B-rep, so the selected
/// site cannot serve as a provenance owner for every color row.
#[derive(Debug, Clone)]
pub(crate) struct OwnedFaceColor {
    pub(crate) value: entity::FaceColor,
    pub(crate) source_stream: cadmpeg_ir::StreamName,
    /// Site qualifier applied when this color came from an unselected site.
    /// A color attribute is only site-local, so an unbound color still needs
    /// this qualifier after alternate sites are merged.
    pub(crate) site_key: Option<String>,
}

impl Brep {
    pub(crate) fn neutral_entity_count(&self) -> Result<u64, cadmpeg_core::CodecError> {
        let counts = [
            self.bodies.len(),
            self.regions.len(),
            self.shells.len(),
            self.faces.len(),
            self.loops.len(),
            self.coedges.len(),
            self.edges.len(),
            self.vertices.len(),
            self.points.len(),
            self.surfaces.len(),
            self.procedural_surfaces.len(),
            self.curves.len(),
            self.pcurves.len(),
        ];
        let total = counts.into_iter().try_fold(0_usize, |total, count| {
            total.checked_add(count).ok_or_else(|| {
                cadmpeg_core::CodecError::NotImplemented(
                    "SLDPRT B-rep entity count exceeds usize".into(),
                )
            })
        })?;
        u64::try_from(total).map_err(|_| {
            cadmpeg_core::CodecError::NotImplemented("SLDPRT B-rep entity count exceeds u64".into())
        })
    }

    /// Qualify every document-arena identity and internal reference by one site key.
    pub(crate) fn qualify_ids(&mut self, site: &str) -> Result<(), cadmpeg_core::CodecError> {
        // The site qualifier is admitted once, here, as a key tail. Appending
        // an admitted tail to an admitted key cannot leave the grammar, so no
        // identity below is rebuilt from text.
        let tail =
            cadmpeg_ir::ids::IdentityKeyTail::try_new(format!("@{site}")).map_err(|error| {
                cadmpeg_core::CodecError::malformed(format_args!(
                    "SLDPRT site qualifier is not identity key text: {error}"
                ))
            })?;
        let qualify = |value: &str| {
            value.split_once('#').map_or_else(
                || value.to_owned(),
                |(namespace, key)| format!("{namespace}#{key}@{site}"),
            )
        };
        for body in &mut self.bodies {
            body.id = qualified(&body.id, &tail);
            body.regions
                .iter_mut()
                .for_each(|id| *id = qualified(id, &tail));
        }
        for region in &mut self.regions {
            region.id = qualified(&region.id, &tail);
            region.body = qualified(&region.body, &tail);
            region
                .shells
                .iter_mut()
                .for_each(|id| *id = qualified(id, &tail));
        }
        for shell in &mut self.shells {
            shell.id = qualified(&shell.id, &tail);
            shell.region = qualified(&shell.region, &tail);
            shell
                .edit_topology(|faces, wire_edges, free_vertices| {
                    for id in faces {
                        *id = qualified(id, &tail);
                    }
                    for id in wire_edges {
                        *id = qualified(id, &tail);
                    }
                    for id in free_vertices {
                        *id = qualified(id, &tail);
                    }
                })
                .map_err(|error| {
                    cadmpeg_core::CodecError::malformed(format_args!(
                        "qualified shell topology is invalid: {error}"
                    ))
                })?;
        }
        for face in &mut self.faces {
            face.id = qualified(&face.id, &tail);
            face.shell = qualified(&face.shell, &tail);
            face.surface = qualified(&face.surface, &tail);
            let qualify_loop =
                |id: &cadmpeg_ir::ids::LoopId| -> cadmpeg_ir::ids::LoopId { qualified(id, &tail) };
            face.loops = match &face.loops {
                cadmpeg_ir::topology::FaceLoops::Unspecified { loops } => {
                    cadmpeg_ir::topology::FaceLoops::unspecified(
                        loops.iter().map(qualify_loop).collect(),
                    )
                }
                cadmpeg_ir::topology::FaceLoops::Classified { outer, inner } => {
                    cadmpeg_ir::topology::FaceLoops::classified(
                        qualify_loop(outer),
                        inner.iter().map(qualify_loop).collect(),
                    )
                }
            };
        }
        for loop_ in &mut self.loops {
            loop_.id = qualified(&loop_.id, &tail);
            loop_.face = qualified(&loop_.face, &tail);
            match &mut loop_.boundary {
                cadmpeg_ir::topology::LoopBoundary::Vertex { vertex, pcurves } => {
                    *vertex = qualified(vertex, &tail);
                    for pcurve in pcurves {
                        pcurve.pcurve = qualified(&pcurve.pcurve, &tail);
                    }
                }
                cadmpeg_ir::topology::LoopBoundary::Ring(ring) => {
                    let mut coedges = ring.coedges().to_vec();
                    let mut vertex_uses = ring.vertex_uses().to_vec();
                    for id in &mut coedges {
                        *id = qualified(id, &tail);
                    }
                    for vertex_use in &mut vertex_uses {
                        vertex_use.vertex = qualified(&vertex_use.vertex, &tail);
                        vertex_use.after = qualified(&vertex_use.after, &tail);
                        for pcurve in &mut vertex_use.pcurves {
                            pcurve.pcurve = qualified(&pcurve.pcurve, &tail);
                        }
                    }
                    *ring = cadmpeg_ir::topology::LoopRing::new(coedges, vertex_uses).map_err(
                        |error| {
                            cadmpeg_core::CodecError::malformed(format_args!(
                                "qualified loop ring is invalid: {error}"
                            ))
                        },
                    )?;
                }
            }
        }
        for coedge in &mut self.coedges {
            coedge.id = qualified(&coedge.id, &tail);
            coedge.owner_loop = qualified(&coedge.owner_loop, &tail);
            coedge.edge = qualified(&coedge.edge, &tail);
            coedge.radial_next = qualified(&coedge.radial_next, &tail);
            for use_ in &mut coedge.pcurves {
                use_.pcurve = qualified(&use_.pcurve, &tail);
            }
        }
        for edge in &mut self.edges {
            edge.id = qualified(&edge.id, &tail);
            edge.map_curve(|curve| qualified(curve, &tail));
            edge.start = qualified(&edge.start, &tail);
            edge.end = qualified(&edge.end, &tail);
        }
        for vertex in &mut self.vertices {
            vertex.id = qualified(&vertex.id, &tail);
            vertex.point = qualified(&vertex.point, &tail);
        }
        self.points.iter_mut().for_each(|point| {
            point.id = qualified(&point.id, &tail);
        });
        for surface in &mut self.surfaces {
            surface.id = qualified(&surface.id, &tail);
            match &mut surface.geometry {
                SurfaceGeometry::Procedural { construction, .. } => {
                    *construction = qualified(construction, &tail);
                }
                SurfaceGeometry::Solved(SolvedSurfaceGeometry::Unknown {
                    record: Some(record),
                }) => {
                    *record = qualified(record, &tail);
                }
                SurfaceGeometry::Solved(_) => {}
            }
        }
        for procedural in &mut self.procedural_surfaces {
            procedural.id = qualified(&procedural.id, &tail);
            procedural.edit_definition(|definition| match definition {
                ProceduralSurfaceDefinition::Blend(definition_payload) => {
                    let mut supports = definition_payload.supports().clone();
                    let mut spine = definition_payload.spine().clone();

                    for support in supports.iter_mut().flatten() {
                        support.surface = qualified(&support.surface, &tail);
                    }
                    if let Some(spine) = &mut spine {
                        *spine = qualified(spine, &tail);
                    }
                    definition_payload.set_supports(supports);
                    definition_payload.set_spine(spine);
                }
                ProceduralSurfaceDefinition::Offset(definition_payload) => {
                    definition_payload.set_support(qualified(definition_payload.support(), &tail));
                }
                _ => {}
            });
        }
        for curve in &mut self.curves {
            curve.id = qualified(&curve.id, &tail);
            if let CurveGeometry::Solved(SolvedCurveGeometry::Unknown {
                record: Some(record),
            }) = &mut curve.geometry
            {
                *record = qualified(record, &tail);
            }
        }
        self.pcurves.iter_mut().for_each(|pcurve| {
            pcurve.id = qualified(&pcurve.id, &tail);
        });
        for record in &mut self.unknowns {
            let id = qualified(record.id(), &tail);
            record.set_id(id);
            record
                .links_mut()
                .iter_mut()
                .for_each(|link| *link = qualify(link));
        }
        for color in &mut self.face_colors {
            if let Some(target) = &mut color.value.target {
                *target = qualify(target);
            }
            color.site_key = Some(site.to_owned());
        }
        for atom in &mut self.face_atoms {
            atom.face = qualified(&atom.face, &tail);
        }
        for modifier in &mut self.body_modifiers {
            if let Some(target) = &mut modifier.target {
                *target = qualify(target);
            }
        }
        self.annotations.map_ids(qualify)?;

        Ok(())
    }
}

fn reserve_graph_map_key<K: Eq + Hash, V>(
    ctx: &DecodeContext<'_>,
    map: &mut HashMap<K, V>,
    key: &K,
    operation: &'static str,
) -> Result<(), cadmpeg_core::CodecError> {
    if !map.contains_key(key) {
        ctx.charge_collection_items(1, operation)?;
        map.try_reserve(1)
            .map_err(|_| ctx.refuse_codec_limit(operation, u64::MAX - 1, u64::MAX))?;
    }
    Ok(())
}

fn reserve_graph_set_key<T: Eq + Hash>(
    ctx: &DecodeContext<'_>,
    set: &mut HashSet<T>,
    key: &T,
    operation: &'static str,
) -> Result<(), cadmpeg_core::CodecError> {
    if !set.contains(key) {
        ctx.charge_collection_items(1, operation)?;
        set.try_reserve(1)
            .map_err(|_| ctx.refuse_codec_limit(operation, u64::MAX - 1, u64::MAX))?;
    }
    Ok(())
}

fn collect_graph_ids<'a>(
    ctx: &DecodeContext<'_>,
    ids: impl IntoIterator<Item = &'a str>,
    operation: &'static str,
) -> Result<HashSet<&'a str>, cadmpeg_core::CodecError> {
    let mut collected = HashSet::new();
    for id in ids {
        reserve_graph_set_key(ctx, &mut collected, &id, operation)?;
        collected.insert(id);
    }
    Ok(collected)
}

fn collect_graph_map<K: Eq + Hash, V>(
    ctx: &DecodeContext<'_>,
    entries: impl IntoIterator<Item = (K, V)>,
    operation: &'static str,
) -> Result<HashMap<K, V>, cadmpeg_core::CodecError> {
    let mut collected = HashMap::new();
    for (key, value) in entries {
        reserve_graph_map_key(ctx, &mut collected, &key, operation)?;
        collected.insert(key, value);
    }
    Ok(collected)
}

fn copy_surface_carrier_geometry(
    ctx: &DecodeContext<'_>,
    geometry: &SurfaceGeometry,
) -> Result<SurfaceGeometry, cadmpeg_core::CodecError> {
    if let SurfaceGeometry::Solved(SolvedSurfaceGeometry::Nurbs(nurbs)) = geometry {
        ctx.charge_collection_items(nurbs.u_knots().as_slice().len() as u64, "copy Parasolid surface u knots")?;
        ctx.charge_collection_items(nurbs.v_knots().as_slice().len() as u64, "copy Parasolid surface v knots")?;
        ctx.charge_collection_items(nurbs.u_count() as u64, "copy Parasolid surface pole rows")?;
        for _ in 0..nurbs.u_count() {
            ctx.charge_collection_items(nurbs.v_count() as u64, "copy Parasolid surface poles")?;
        }
        let copied = nurbs.try_clone().map_err(|_| {
            ctx.refuse_codec_limit("copy Parasolid NURBS surface", u64::MAX - 1, u64::MAX)
        })?;
        Ok(SurfaceGeometry::Solved(SolvedSurfaceGeometry::Nurbs(copied)))
    } else {
        Ok(geometry.clone())
    }
}

fn copy_curve_carrier_geometry(
    ctx: &DecodeContext<'_>,
    geometry: &CurveGeometry,
) -> Result<CurveGeometry, cadmpeg_core::CodecError> {
    if let CurveGeometry::Solved(SolvedCurveGeometry::Nurbs(nurbs)) = geometry {
        ctx.charge_collection_items(nurbs.knots().as_slice().len() as u64, "copy Parasolid curve knots")?;
        ctx.charge_collection_items(nurbs.pole_count() as u64, "copy Parasolid curve poles")?;
        let copied = nurbs.try_clone().map_err(|_| {
            ctx.refuse_codec_limit("copy Parasolid NURBS curve", u64::MAX - 1, u64::MAX)
        })?;
        Ok(CurveGeometry::Solved(SolvedCurveGeometry::Nurbs(copied)))
    } else {
        Ok(geometry.clone())
    }
}

fn shell_face_components(
    ctx: &DecodeContext<'_>,
    out: &Brep,
    native_shell_id: &str,
) -> Result<Vec<Vec<FaceId>>, cadmpeg_core::CodecError> {
    let mut candidates = Vec::new();
    for face in &out.faces {
        ctx.charge_work(1, "select Parasolid shell faces")?;
        if face.shell.as_str() == native_shell_id {
            ctx.reserve_collection_vec(&mut candidates, 1, "select Parasolid shell faces")?;
            candidates.push(&face.id);
        }
    }
    let mut candidate_ids = HashSet::new();
    for face in &candidates {
        let key = face.as_str();
        reserve_graph_set_key(ctx, &mut candidate_ids, &key, "index Parasolid shell faces")?;
        candidate_ids.insert(key);
    }
    let mut loop_faces = HashMap::new();
    for loop_ in &out.loops {
        ctx.charge_work(1, "index Parasolid shell loops")?;
        if candidate_ids.contains(loop_.face.as_str()) {
            let key = loop_.id.as_str();
            reserve_graph_map_key(ctx, &mut loop_faces, &key, "index Parasolid shell loops")?;
            loop_faces.insert(key, loop_.face.as_str());
        }
    }
    let mut faces_by_edge = HashMap::<&str, HashSet<&str>>::new();
    for coedge in &out.coedges {
        ctx.charge_work(1, "index Parasolid shell edges")?;
        if let Some(face) = loop_faces.get(coedge.owner_loop.as_str()) {
            let key = coedge.edge.as_str();
            reserve_graph_map_key(ctx, &mut faces_by_edge, &key, "index Parasolid shell edges")?;
            let edge_faces = faces_by_edge.entry(key).or_default();
            reserve_graph_set_key(ctx, edge_faces, face, "index Parasolid edge faces")?;
            edge_faces.insert(*face);
        }
    }
    let mut neighbors = HashMap::<&str, HashSet<&str>>::new();
    for edge_faces in faces_by_edge.values() {
        for &face in edge_faces {
            reserve_graph_map_key(ctx, &mut neighbors, &face, "index Parasolid face neighbors")?;
            let adjacent = neighbors.entry(face).or_default();
            for &other in edge_faces {
                ctx.charge_work(1, "index Parasolid face neighbors")?;
                if other != face {
                    reserve_graph_set_key(ctx, adjacent, &other, "index Parasolid face neighbors")?;
                    adjacent.insert(other);
                }
            }
        }
    }

    // The walk moves over borrowed keys, so the face each key names is looked
    // up rather than rebuilt from its text.
    let mut faces_by_key = HashMap::new();
    for face in &candidates {
        let key = face.as_str();
        reserve_graph_map_key(ctx, &mut faces_by_key, &key, "index Parasolid face identities")?;
        faces_by_key.insert(key, *face);
    }
    let mut assigned = HashSet::new();
    let mut components = Vec::new();
    for face in &candidates {
        let key = face.as_str();
        reserve_graph_set_key(ctx, &mut assigned, &key, "walk Parasolid shell components")?;
        if !assigned.insert(face.as_str()) {
            continue;
        }
        let mut component = Vec::new();
        let mut pending = Vec::new();
        ctx.reserve_collection_vec(&mut pending, 1, "walk Parasolid shell components")?;
        pending.push(face.as_str());
        while let Some(current) = pending.pop() {
            let Some(face) = faces_by_key.get(current) else {
                continue;
            };
            let mut id = String::new();
            ctx.reserve_retained_string(
                &mut id,
                face.as_str().len(),
                "copy Parasolid face identity",
            )?;
            id.push_str(face.as_str());
            let id = FaceId::mint(id).map_err(|error| {
                cadmpeg_core::CodecError::malformed(format_args!(
                    "Parasolid face identity is invalid: {error}"
                ))
            })?;
            ctx.reserve_collection_vec(&mut component, 1, "walk Parasolid shell components")?;
            component.push(id);
            for &neighbor in neighbors.get(current).into_iter().flatten() {
                ctx.charge_work(1, "walk Parasolid shell components")?;
                reserve_graph_set_key(ctx, &mut assigned, &neighbor, "walk Parasolid shell components")?;
                if assigned.insert(neighbor) {
                    ctx.reserve_collection_vec(&mut pending, 1, "walk Parasolid shell components")?;
                    pending.push(neighbor);
                }
            }
        }
        component.sort_by(|left, right| left.as_str().cmp(right.as_str()));
        ctx.reserve_collection_vec(&mut components, 1, "group Parasolid shell components")?;
        components.push(component);
    }
    Ok(components)
}

#[derive(Debug, Clone)]
struct BodyRecord {
    attr: u16,
    kind: BodyKind,
    refs: Vec<u16>,
    offset: usize,
    regions: Vec<RegionRecord>,
}

#[derive(Debug, Clone)]
struct RegionRecord {
    attr: u16,
    offset: usize,
    shells: Vec<ShellRecord>,
}

#[derive(Debug, Clone)]
struct ShellRecord {
    attr: u16,
    offset: usize,
    refs: Vec<u16>,
}

/// Transfer limitations found while building a [`Brep`].
#[derive(Default)]
pub(crate) struct Stats {
    /// Framed top-level model entity records across the selected stream site.
    pub(crate) source_entity_records: usize,
    /// Face-color bindings withheld because current records conflict.
    pub(crate) unresolved_face_colors: usize,
    /// Face owners with multiple non-equivalent bridge uses.
    pub(crate) ambiguous_face_owners: usize,
    /// Canonical faces that no explicit body record claims.
    pub(crate) unclaimed_faces: usize,
    /// Faces on a support surface this codec does not type; emitted with an
    /// unknown-geometry carrier.
    pub(crate) unknown_surface_faces: usize,
    /// Hidden procedural support surfaces whose carrier geometry remains opaque.
    pub(crate) unknown_procedural_supports: usize,
    /// Edges whose support curve is an untyped carrier (emitted with no curve).
    pub(crate) unknown_curve_edges: usize,
    /// Pcurves withheld because geometric inverse selection was ambiguous.
    pub(crate) ambiguous_pcurve_parameters: usize,
    /// NURBS edge carriers whose vertex range is off their bound surface.
    pub(crate) off_surface_nurbs_pcurves: usize,
    /// No explicit body record was available, so one body hierarchy was derived.
    pub(crate) synthetic_body_grouping: bool,
}

/// The `sldprt:brep` shell namespace, which three routes mint under.
fn shell_namespace() -> cadmpeg_ir::ids::IdentityNamespace {
    cadmpeg_ir::identity_namespace!("sldprt", "brep", "shell")
}

/// The `sldprt:brep` region namespace.
fn region_namespace() -> cadmpeg_ir::ids::IdentityNamespace {
    cadmpeg_ir::identity_namespace!("sldprt", "brep", "region")
}

/// The `sldprt:brep` body namespace.
fn body_namespace() -> cadmpeg_ir::ids::IdentityNamespace {
    cadmpeg_ir::identity_namespace!("sldprt", "brep", "body")
}

/// The `sldprt:brep` pcurve namespace.
fn pcurve_namespace() -> cadmpeg_ir::ids::IdentityNamespace {
    cadmpeg_ir::identity_namespace!("sldprt", "brep", "pcurve")
}

/// The component split of one native shell: `<shell>.component-<ordinal>`.
fn shell_component(shell: &ShellId, component: usize) -> ShellId {
    ShellId::from(
        cadmpeg_ir::ids::Identity::from(shell.clone()).with_key_tail(
            &cadmpeg_ir::ids::IdentityKeyTail::empty()
                .then(cadmpeg_ir::identity_key!(".component-"))
                .then(component),
        ),
    )
}

/// Append the site qualifier to one typed identity's key.
///
/// The tail carries the key grammar, and appending it to an admitted key
/// keeps it, so this operation has no failing branch.
fn qualified<T>(id: &T, site: &cadmpeg_ir::ids::IdentityKeyTail) -> T
where
    T: Clone + Into<cadmpeg_ir::ids::Identity> + From<cadmpeg_ir::ids::Identity>,
{
    T::from(id.clone().into().with_key_tail(site))
}

fn id_face(a: u16) -> FaceId {
    FaceId::compose(
        &cadmpeg_ir::identity_namespace!("sldprt", "brep", "face"),
        a,
    )
}
fn id_surf(a: u16) -> SurfaceId {
    SurfaceId::compose(
        &cadmpeg_ir::identity_namespace!("sldprt", "brep", "surf"),
        a,
    )
}
fn id_loop(a: u16) -> LoopId {
    LoopId::compose(
        &cadmpeg_ir::identity_namespace!("sldprt", "brep", "loop"),
        a,
    )
}
fn id_coedge(a: u16) -> CoedgeId {
    CoedgeId::compose(
        &cadmpeg_ir::identity_namespace!("sldprt", "brep", "coedge"),
        a,
    )
}
fn id_edge(a: u16) -> EdgeId {
    EdgeId::compose(
        &cadmpeg_ir::identity_namespace!("sldprt", "brep", "edge"),
        a,
    )
}
fn id_curve(a: u16) -> CurveId {
    CurveId::compose(
        &cadmpeg_ir::identity_namespace!("sldprt", "brep", "curve"),
        a,
    )
}
fn id_vertex(a: u16) -> VertexId {
    VertexId::compose(
        &cadmpeg_ir::identity_namespace!("sldprt", "brep", "vertex"),
        a,
    )
}
fn id_point(a: u16) -> PointId {
    PointId::compose(
        &cadmpeg_ir::identity_namespace!("sldprt", "brep", "point"),
        a,
    )
}
fn id_closed_point(edge: u16) -> PointId {
    PointId::compose(
        &cadmpeg_ir::identity_namespace!("sldprt", "brep", "point"),
        cadmpeg_ir::identity_key!("closed-circle-").then(edge),
    )
}
fn id_closed_vertex(edge: u16) -> VertexId {
    VertexId::compose(
        &cadmpeg_ir::identity_namespace!("sldprt", "brep", "vertex"),
        cadmpeg_ir::identity_key!("closed-circle-").then(edge),
    )
}

/// One face-use's decoded loops: ordered coedge rings, keyed by loop attr.
#[derive(Clone, PartialEq, Eq)]
struct WalkedFace {
    bridge_attr: u16,
    surface_attr: u16,
    sense: Sense,
    /// `(loop_attr, ordered_coedge_attrs)` in sibling order.
    loops: Vec<(u16, Vec<u16>)>,
}

/// Follow the sibling loop-head chain of a bridge and each loop's coedge ring,
/// returning the ordered structure with cycles guarded.
/// Resolve a face whose `refs[4]` carrier is a swept/spun construction to a
/// solved NURBS patch. Returns `(geometry, record offset, annotation tag,
/// derived exactness)`. A spun surface is exact for an exact profile; a swept
/// surface patch is derived because its ruling extent comes from the face's
/// vertex points rather than a stored interval.
fn resolve_sweep_surface(
    ctx: &DecodeContext<'_>,
    carriers: &CarrierIndex,
    tables: &topology::Tables,
    face: &WalkedFace,
    refusal: &mut crate::lane_refusal::LaneRefusals,
) -> Result<Option<(
    SolvedSurfaceGeometry,
    usize,
    &'static str,
    Option<Exactness>,
)> , cadmpeg_core::CodecError> {
    let Some(construction) = carriers.sweep(face.surface_attr) else { return Ok(None) };
    let Some(profile) = carriers.curve(construction.profile_attr) else { return Ok(None) };
    let record = format!(
        "sldprt sweep construction at byte {} for surface attr {}",
        construction.offset, face.surface_attr
    );
    let Some(curve) = sweep::profile_nurbs(ctx, &profile.carrier().geometry, &record, refusal)? else { return Ok(None) };
    let profile_derived = matches!(profile, IndexedCurve::Derived(_));
    match &construction.kind {
        SweepKind::Spun { base, axis } => Ok(sweep::spun_nurbs(
                ctx, &curve, *base, *axis, &record, refusal,
            )?.map(|surface| (
            SolvedSurfaceGeometry::Nurbs(surface),
            construction.offset,
            "00_44",
            profile_derived.then_some(Exactness::Derived),
        ))),
        SweepKind::Swept { direction } => {
            let unit_direction = *direction;
            let direction = direction.as_raw();
            // Ruling extent: face vertex travel bracketed by the profile poles'
            // own travel along the sweep direction, in millimetres.
            let project = |p: &cadmpeg_ir::math::Point3| {
                p.x * direction.x + p.y * direction.y + p.z * direction.z
            };
            let mut point_lo = f64::INFINITY;
            let mut point_hi = f64::NEG_INFINITY;
            for (_, ring) in &face.loops {
                for ce_attr in ring {
                    let Some(vuse) = tables.coedges().get(ce_attr).map(|ce| ce.refs[4]) else {
                        continue;
                    };
                    let Some(coordinates) = tables
                        .vertex_uses()
                        .get(&vuse)
                        .map(|vu| vu.refs[4])
                        .and_then(|pa| tables.points().get(&pa))
                        .map(|p| p.xyz_m)
                    else {
                        continue;
                    };
                    let travel = coordinates[0] * LEN_TO_MM * direction.x
                        + coordinates[1] * LEN_TO_MM * direction.y
                        + coordinates[2] * LEN_TO_MM * direction.z;
                    point_lo = point_lo.min(travel);
                    point_hi = point_hi.max(travel);
                }
            }
            if point_lo > point_hi {
                return Ok(None);
            }
            let mut pole_lo = f64::INFINITY;
            let mut pole_hi = f64::NEG_INFINITY;
            for index in 0..curve.pole_count() {
                let point = match curve.pole_rows() {
                    cadmpeg_ir::geometry::nurbs::NurbsPoles3::Polynomial { points } => points[index].get(),
                    cadmpeg_ir::geometry::nurbs::NurbsPoles3::Rational { points } => points[index].point.get(),
                };
                let travel = project(&point);
                pole_lo = pole_lo.min(travel);
                pole_hi = pole_hi.max(travel);
            }
            let v_start = point_lo - pole_hi;
            let v_end = point_hi - pole_lo;
            // Two independently derived pads on the same swept extent: an
            // absolute one in millimetres and one relative to the travel the
            // poles and points state. The extent must clear both, so the
            // looser is the pad.
            let pad = looser_tolerance(
                EPS_SWEEP_EXTENT_ABSOLUTE_MM,
                (v_end - v_start) * EPS_SWEEP_EXTENT_RELATIVE,
            );
            Ok(sweep::swept_nurbs(
                    ctx,
                    &curve,
                    unit_direction,
                    v_start - pad,
                    v_end + pad,
                    &record,
                    refusal,
                )?.map(|surface| (
                SolvedSurfaceGeometry::Nurbs(surface),
                construction.offset,
                "00_43",
                Some(Exactness::Derived),
            )))
        }
    }
}

fn id_hidden_support_surface(attr: u16) -> SurfaceId {
    SurfaceId::compose(
        &cadmpeg_ir::identity_namespace!("sldprt", "brep", "hidden-support-surf"),
        attr,
    )
}

fn id_offset_construction(attr: u16) -> ProceduralSurfaceId {
    ProceduralSurfaceId::compose(
        &cadmpeg_ir::identity_namespace!("sldprt", "brep", "offset-support-construction"),
        attr,
    )
}

struct BrepSink<'ctx, 'arena, 'out> {
    ctx: &'ctx DecodeContext<'arena>,
    out: &'out mut Brep,
}

fn emit_offset_surface(
    sink: &mut BrepSink<'_, '_, '_>,
    annotations: &mut AnnotationBuilder,
    source_stream: &cadmpeg_ir::annotations::StreamHandle,
    surface: SurfaceId,
    construction: ProceduralSurfaceId,
    support: SurfaceId,
    offset: &OffsetCarrier,
) -> Result<(), cadmpeg_core::CodecError> {
    annotations
        .note(&surface, source_stream, offset.offset as u64)
        .tag("00_3c");
    let payload = cadmpeg_ir::geometry::surface_payloads::OffsetSurfaceConstruction::legacy(
        support,
        offset.distance,
        None,
        None,
        false,
        cadmpeg_ir::geometry::LegacyExtensionFlags::Absent {},
        None,
    );
    let procedural = ProceduralSurface::new(
        construction.clone(),
        ProceduralSurfaceDefinition::Offset(payload),
        None,
    );
    admit_brep_entity(sink.ctx)?;
    sink.ctx.reserve_collection_vec(&mut sink.out.procedural_surfaces, 1, "collect Parasolid offset constructions")?;
    sink.out.procedural_surfaces.push(procedural);
    let geometry = SurfaceGeometry::Procedural {
        construction,
        cache: None,
    };
    admit_brep_entity(sink.ctx)?;
    sink.ctx.reserve_collection_vec(&mut sink.out.surfaces, 1, "collect Parasolid offset surfaces")?;
    sink.out.surfaces.push(Surface {
        id: surface,
        source_object: None,
        geometry,
    });
    Ok(())
}

/// Whether one referenced support can be emitted without a procedural cycle.
///
/// An untyped reference is still a valid opaque support surface. Offset
/// carriers recurse because a cycle cannot define a surface.
fn support_is_acyclic(
    ctx: &DecodeContext<'_>,
    attr: u16,
    carriers: &CarrierIndex,
    resolving: &mut HashSet<u16>,
) -> Result<bool, cadmpeg_core::CodecError> {
    let _depth = ctx.enter_nested("check Parasolid surface support")?;
    ctx.charge_work(1, "check Parasolid surface support")?;
    reserve_graph_set_key(ctx, resolving, &attr, "track Parasolid surface support")?;
    if !resolving.insert(attr) {
        return Ok(false);
    }
    let acyclic = if carriers.surface(attr).is_some() {
        Ok(true)
    } else if let Some(offset) = carriers.offset(attr) {
        support_is_acyclic(ctx, offset.support, carriers, resolving)
    } else {
        Ok(true)
    };
    resolving.remove(&attr);
    acyclic
}

/// Resolve one carrier as a support of a procedural construction. A carrier
/// owned by an emitted face reuses that face's surface identity. Otherwise the
/// decoder emits a hidden analytic, NURBS, recursive offset, or opaque support
/// surface. Offset cycles invalidate the complete construction.
fn ensure_surface_support(
    sink: &mut BrepSink<'_, '_, '_>,
    attr: u16,
    carriers: &CarrierIndex,
    emitted_face_surface_by_carrier: &HashMap<u16, u16>,
    annotations: &mut AnnotationBuilder,
    source_stream: &cadmpeg_ir::annotations::StreamHandle,
    resolving: &mut HashSet<u16>,
) -> Result<Option<SurfaceId>, cadmpeg_core::CodecError> {
    let _depth = sink.ctx.enter_nested("resolve Parasolid surface support")?;
    sink.ctx.charge_work(1, "resolve Parasolid surface support")?;
    reserve_graph_set_key(sink.ctx, resolving, &attr, "track resolved Parasolid support")?;
    if !resolving.insert(attr) {
        return Ok(None);
    }
    let result = (|| -> Result<Option<SurfaceId>, cadmpeg_core::CodecError> {
        if let Some(carrier) = carriers.surface(attr) {
            let id = emitted_face_surface_by_carrier.get(&attr).map_or_else(
                || id_hidden_support_surface(attr),
                |bridge| id_surf(*bridge),
            );
            if !sink.out.surfaces.iter().any(|surface| surface.id == id)
                && !emitted_face_surface_by_carrier.contains_key(&attr)
            {
                let geometry = copy_surface_carrier_geometry(sink.ctx, &carrier.geometry)?;
                if let SurfaceGeometry::Solved(solved) = &geometry {
                    if annotate_surface_frame(annotations, id.as_str(), solved).is_err() {
                        return Ok(None);
                    }
                }
                annotations
                    .note(&id, source_stream, carrier.offset as u64)
                    .tag("procedural_support");
                admit_brep_entity(sink.ctx)?;
                sink.ctx.reserve_collection_vec(&mut sink.out.surfaces, 1, "collect Parasolid support surfaces")?;
                sink.out.surfaces.push(Surface {
                    id: id.clone(),
                    source_object: None,
                    geometry,
                });
            }
            Ok(Some(id))
        } else if let Some(offset) = carriers.offset(attr) {
            let Some(support) = ensure_surface_support(
                sink,
                offset.support,
                carriers,
                emitted_face_surface_by_carrier,
                annotations,
                source_stream,
                resolving,
            )?
            else {
                return Ok(None);
            };
            let surface = emitted_face_surface_by_carrier.get(&attr).map_or_else(
                || id_hidden_support_surface(attr),
                |bridge| id_surf(*bridge),
            );
            if !emitted_face_surface_by_carrier.contains_key(&attr)
                && !sink
                    .out
                    .surfaces
                    .iter()
                    .any(|candidate| candidate.id == surface)
            {
                let construction = id_offset_construction(attr);
                emit_offset_surface(
                    sink,
                    annotations,
                    source_stream,
                    surface.clone(),
                    construction,
                    support,
                    offset,
                )?;
            }
            Ok(Some(surface))
        } else {
            let surface = emitted_face_surface_by_carrier.get(&attr).map_or_else(
                || id_hidden_support_surface(attr),
                |bridge| id_surf(*bridge),
            );
            if !emitted_face_surface_by_carrier.contains_key(&attr)
                && !sink
                    .out
                    .surfaces
                    .iter()
                    .any(|candidate| candidate.id == surface)
            {
                annotations.exactness(&surface, Exactness::Unknown);
                sink.out.stats.unknown_procedural_supports += 1;
                admit_brep_entity(sink.ctx)?;
                sink.ctx.reserve_collection_vec(&mut sink.out.surfaces, 1, "collect Parasolid opaque supports")?;
                sink.out.surfaces.push(Surface {
                    id: surface.clone(),
                    source_object: None,
                    geometry: SurfaceGeometry::Solved(SolvedSurfaceGeometry::Unknown {
                        record: None,
                    }),
                });
            }
            Ok(Some(surface))
        }
    })();
    resolving.remove(&attr);
    result
}

fn walk_face(
    ctx: &DecodeContext<'_>,
    bridge: &topology::Bridge,
    t: &topology::Tables,
) -> Result<WalkedFace, cadmpeg_core::CodecError> {
    let surface_attr = bridge.refs[4];
    let mut loops = Vec::new();
    let mut loop_ref = bridge.refs[2];
    let mut loop_guard = HashSet::new();
    while loop_ref != 0 {
        ctx.charge_work(1, "walk Parasolid face loops")?;
        reserve_graph_set_key(ctx, &mut loop_guard, &loop_ref, "walk Parasolid face loops")?;
        if !loop_guard.insert(loop_ref) {
            break;
        }
        let Some(lp) = t.loops().get(&loop_ref) else {
            break;
        };
        let owner_bridge = lp.refs[2];
        let same_face_use = owner_bridge == bridge.attr
            || bridge.owner.is_some_and(|owner| {
                t.bridges()
                    .get(&owner_bridge)
                    .and_then(|candidate| candidate.owner)
                    == Some(owner)
            });
        if !same_face_use {
            break;
        }
        let first = lp.refs[1];
        let mut ring = Vec::new();
        let mut ce_ref = first;
        let mut ce_guard = HashSet::new();
        let mut ring_closed = false;
        while ce_ref != 0 {
            ctx.charge_work(1, "walk Parasolid face coedges")?;
            reserve_graph_set_key(ctx, &mut ce_guard, &ce_ref, "walk Parasolid face coedges")?;
            if !ce_guard.insert(ce_ref) {
                break;
            }
            let Some(ce) = t.coedges().get(&ce_ref) else {
                break;
            };
            if ce.refs[1] != loop_ref {
                break;
            }
            ctx.reserve_collection_vec(&mut ring, 1, "walk Parasolid face coedges")?;
            ring.push(ce_ref);
            ce_ref = ce.refs[3];
            if ce_ref == first {
                ring_closed = true;
                break;
            }
        }
        if ring_closed {
            ctx.reserve_collection_vec(&mut loops, 1, "walk Parasolid face loops")?;
            loops.push((loop_ref, ring));
        }
        loop_ref = lp.refs[3];
    }
    Ok(WalkedFace {
        bridge_attr: bridge.attr,
        surface_attr,
        sense: bridge.sense,
        loops,
    })
}

fn edge_parameter_range(
    carrier: &CurveCarrier,
    endpoints: Option<[cadmpeg_ir::math::Point3; 2]>,
) -> Result<Option<([f64; 2], bool)>, cadmpeg_core::CodecError> {
    const TOLERANCE_MM: f64 = 1.0e-7;

    let Some(range) = carrier.parameter_range else {
        return Ok(None);
    };
    let range = range.get();
    let range = match &carrier.geometry {
        CurveGeometry::Solved(SolvedCurveGeometry::Line(_)) => {
            range.map(|parameter| parameter * LEN_TO_MM)
        }
        _ => range,
    };
    let range = if range[0] <= range[1] {
        range
    } else {
        [range[1], range[0]]
    };
    let Some(endpoints) = endpoints else {
        return Ok(Some((range, false)));
    };
    let geometry = &carrier.geometry;
    let first = cadmpeg_ir::eval::finite_or_refusal(cadmpeg_ir::eval::curve_point(
        geometry, range[0],
    ))?;
    let second = cadmpeg_ir::eval::finite_or_refusal(cadmpeg_ir::eval::curve_point(
        geometry, range[1],
    ))?;
    let (Some(first), Some(second)) = (first, second) else {
        return Ok(None);
    };
    let distance = |left: cadmpeg_ir::math::Point3, right: cadmpeg_ir::math::Point3| {
        (left.x - right.x)
            .hypot(left.y - right.y)
            .hypot(left.z - right.z)
    };
    if distance(first.get(), endpoints[0]).max(distance(second.get(), endpoints[1])) <= TOLERANCE_MM
    {
        Ok(Some((range, false)))
    } else if distance(first.get(), endpoints[1]).max(distance(second.get(), endpoints[0]))
        <= TOLERANCE_MM
    {
        Ok(Some((range, true)))
    } else {
        Ok(None)
    }
}

/// Resolve the coedge that defines an edge's stored direction.
///
/// Bare records carry an explicit coedge attr in `refs[0]`. Prefixed records
/// carry no such slot. An absent or source-null slot is resolved only when exactly
/// one same-edge forward coedge exists. A non-sentinel explicit reference is
/// authoritative: a dangling, cross-edge, or reversed reference is rejected.
fn canonical_coedge_attr(
    edge_attr: u16,
    edge_use: Option<&topology::EdgeUse>,
    coedges: &HashMap<u16, topology::Coedge>,
) -> Option<u16> {
    if let Some(explicit) = edge_use
        .and_then(|record| record.references.canonical())
        .filter(|attr| *attr > 1)
    {
        let coedge = coedges.get(&explicit)?;
        return (coedge.refs[6] == edge_attr && coedge.sense == Sense::Forward).then_some(explicit);
    }

    let mut candidates = coedges
        .iter()
        .filter(|(_, coedge)| coedge.refs[6] == edge_attr && coedge.sense == Sense::Forward);
    let (&attr, _) = candidates.next()?;
    candidates.next().is_none().then_some(attr)
}

fn edge_end_vuse(canonical: u16, ring_end: u16, coedges: &HashMap<u16, topology::Coedge>) -> u16 {
    let Some(twin) = coedges
        .get(&canonical)
        .map(|coedge| coedge.refs[5])
        .filter(|twin| *twin != canonical)
    else {
        return ring_end;
    };
    let Some(twin_record) = coedges.get(&twin) else {
        return ring_end;
    };
    if twin_record.refs[5] != canonical {
        return ring_end;
    }
    twin_record.refs[4]
}

fn surface_sense(sense: Sense, orientation_reversed: bool) -> Sense {
    match (sense, orientation_reversed) {
        (Sense::Forward, true) => Sense::Reversed,
        (Sense::Reversed, true) => Sense::Forward,
        (sense, false) => sense,
    }
}

/// The body bytes a Parasolid stream header names.
///
/// # Errors
///
/// Refuses a body offset past the payload, naming the offset and the payload
/// length: a header that states its body outside its own stream is a framing
/// error, not an empty body.
fn header_body<'a>(
    payload: &'a [u8],
    header: &StreamHeader,
) -> Result<&'a [u8], cadmpeg_core::CodecError> {
    payload.get(header.body_offset..).ok_or_else(|| {
        cadmpeg_core::CodecError::malformed(format!(
            "sldprt Parasolid stream header states body offset {} past its {}-byte payload",
            header.body_offset,
            payload.len()
        ))
    })
}

/// Decode one parsed Parasolid stream into B-rep arenas.
///
/// `stream` names the provenance stream recorded in [`Brep::annotations`].
pub(crate) fn decode(
    ctx: &DecodeContext<'_>,
    payload: &[u8],
    header: &StreamHeader,
    stream: &cadmpeg_ir::StreamName,
) -> Result<Brep, cadmpeg_core::CodecError> {
    decode_body(ctx, header_body(payload, header)?, stream)
}

fn is_deltas_stream(header: &StreamHeader) -> bool {
    header
        .description
        .as_bytes()
        .windows(b"deltas".len())
        .any(|window| window.eq_ignore_ascii_case(b"deltas"))
}

fn selected_typed_face_offsets(
    ctx: &DecodeContext<'_>,
    facts: &typed::Facts,
    attrs: Option<&HashSet<u16>>,
) -> Result<HashSet<usize>, cadmpeg_core::CodecError> {
    let mut offsets = HashSet::new();
    if let Some(attrs) = attrs {
        for face in &facts.faces {
            ctx.charge_work(1, "select typed Parasolid face offsets")?;
            if attrs.contains(&face.attr) {
                reserve_graph_set_key(
                    ctx,
                    &mut offsets,
                    &face.offset,
                    "index typed Parasolid face offsets",
                )?;
                offsets.insert(face.offset);
            }
        }
    }
    Ok(offsets)
}

fn append_entity_facts(
    ctx: &DecodeContext<'_>,
    target: &mut entity::Facts,
    mut source: entity::Facts,
) -> Result<(), cadmpeg_core::CodecError> {
    ctx.reserve_precharged_vec(
        &mut target.face_colors,
        source.face_colors.len(),
        "merge Parasolid face colors",
    )?;
    target.face_colors.append(&mut source.face_colors);
    ctx.reserve_precharged_vec(
        &mut target.face_color_versions,
        source.face_color_versions.len(),
        "merge Parasolid face color versions",
    )?;
    target
        .face_color_versions
        .append(&mut source.face_color_versions);
    ctx.reserve_precharged_vec(
        &mut target.face_atoms,
        source.face_atoms.len(),
        "merge Parasolid face atoms",
    )?;
    target.face_atoms.append(&mut source.face_atoms);
    ctx.reserve_precharged_vec(
        &mut target.body_modifiers,
        source.body_modifiers.len(),
        "merge Parasolid body modifiers",
    )?;
    target.body_modifiers.append(&mut source.body_modifiers);
    target.entity_count += source.entity_count;
    target.unresolved_face_colors += source.unresolved_face_colors;
    Ok(())
}

/// Decode related partition and deltas streams as one record source.
///
/// Partition records are the base set. Deltas records fill missing subordinate
/// records and point updates, but do not replace a same-identity partition
/// topology or carrier record. `stream` names the combined provenance source.
pub(crate) fn decode_bodies(
    ctx: &DecodeContext<'_>,
    bodies: &[(&[u8], &StreamHeader)],
    stream: &cadmpeg_ir::StreamName,
) -> Result<Brep, cadmpeg_core::CodecError> {
    let mut carriers = CarrierIndex::default();
    let mut tables = topology::Tables::default();
    let mut facts = entity::Facts::default();
    let mut typed_facts = typed::Facts::default();
    let mut initialized = false;
    let mut ordered = Vec::new();
    ctx.reserve_collection_vec(&mut ordered, bodies.len(), "order Parasolid body streams")?;
    ordered.extend(bodies.iter());
    ordered.sort_by_key(|(_, header)| is_deltas_stream(header));
    let mut entity_streams = Vec::new();
    ctx.reserve_collection_vec(
        &mut entity_streams,
        ordered.len(),
        "index Parasolid body streams",
    )?;
    for (payload, header) in &ordered {
        let body = header_body(payload, header)?;
        entity_streams.push((body, is_deltas_stream(header)));
    }
    for (body, _) in &entity_streams {
        admit_brep_scan_candidates(ctx, body)?;
    }
    let mut typed_streams = Vec::new();
    ctx.reserve_collection_vec(
        &mut typed_streams,
        entity_streams.len(),
        "index typed Parasolid streams",
    )?;
    for (body, _) in &entity_streams {
        typed_streams.push(typed::scan(body, ctx)?);
    }
    for stream_typed_facts in &typed_streams {
        typed_facts.merge_missing(ctx, stream_typed_facts.try_clone(ctx)?)?;
    }
    let typed_bridge_attrs = typed_facts.valid_ownership_face_attrs(ctx)?;
    let selected_bridge_attrs = typed_bridge_attrs.as_ref();
    for (stream_order, ((payload, header), stream_typed_facts)) in
        ordered.into_iter().zip(typed_streams).enumerate()
    {
        let body = header_body(payload, header)?;
        let is_deltas = is_deltas_stream(header);
        let typed_face_offsets =
            selected_typed_face_offsets(ctx, &stream_typed_facts, typed_bridge_attrs.as_ref())?;
        carriers.merge_missing(ctx, scan_carriers(ctx, body)?)?;
        let curve_attrs = carriers.curve_attrs(ctx)?;
        let scanned_tables = if is_deltas {
            topology::scan_deltas_with_curve_attrs_excluding(
                ctx,
                body,
                &curve_attrs,
                &typed_face_offsets,
            )
        } else {
            topology::scan_with_curve_attrs_excluding(ctx, body, &curve_attrs, &typed_face_offsets)
        }?;
        let mut scanned_facts = entity::scan_metadata(ctx, body, is_deltas)?;
        for color in &mut scanned_facts.face_colors {
            color.stream_order = stream_order;
        }
        for version in &mut scanned_facts.face_color_versions {
            version.stream_order = stream_order;
        }
        if !initialized || !is_deltas {
            if initialized {
                tables.merge_deltas(ctx, scanned_tables, selected_bridge_attrs)?;
                append_entity_facts(ctx, &mut facts, scanned_facts)?;
            } else {
                tables = scanned_tables;
                facts = scanned_facts;
                initialized = true;
            }
        } else {
            tables.merge_deltas(ctx, scanned_tables, selected_bridge_attrs)?;
            append_entity_facts(ctx, &mut facts, scanned_facts)?;
        }
    }
    decode_graph(ctx, &mut carriers, &tables, facts, &typed_facts, stream)
}

fn decode_body(
    ctx: &DecodeContext<'_>,
    body: &[u8],
    stream: &cadmpeg_ir::StreamName,
) -> Result<Brep, cadmpeg_core::CodecError> {
    admit_brep_scan_candidates(ctx, body)?;
    let mut carriers = scan_carriers(ctx, body)?;
    let curve_attrs = carriers.curve_attrs(ctx)?;
    let typed_facts = typed::scan(body, ctx)?;
    let typed_face_attrs = typed_facts.valid_ownership_face_attrs(ctx)?;
    let typed_face_offsets =
        selected_typed_face_offsets(ctx, &typed_facts, typed_face_attrs.as_ref())?;
    let t =
        topology::scan_with_curve_attrs_excluding(ctx, body, &curve_attrs, &typed_face_offsets)?;
    let entity_facts = entity::scan_metadata(ctx, body, false)?;
    decode_graph(ctx, &mut carriers, &t, entity_facts, &typed_facts, stream)
}

fn admit_brep_scan_candidates(
    ctx: &DecodeContext<'_>,
    body: &[u8],
) -> Result<(), cadmpeg_core::CodecError> {
            let count = body
            .windows(3)
            .filter(|marker| {
                marker[0] == 0
                    && (matches!(
                        marker[1],
                        0x0c | 0x0d
                            | 0x0e
                            | 0x0f
                            | 0x10
                            | 0x11
                            | 0x12
                            | 0x13
                            | 0x1d
                            | 0x1e
                            | 0x1f
                            | 0x20
                            | 0x26
                            | 0x28
                            | 0x29
                            | 0x2d
                            | 0x32
                            | 0x33
                            | 0x34
                            | 0x35
                            | 0x36
                            | 0x38
                            | 0x3c
                            | 0x43
                            | 0x44
                            | 0x4f
                            | 0x50
                            | 0x51
                            | 0x52
                            | 0x53
                            | 0x7c
                            | 0x7e
                            | 0x7f
                            | 0x80
                            | 0x85
                            | 0x86
                            | 0x88
                            | 0xcc
                    ) || marker[1..] == [0x01, 0x5a])
            })
            .count();
        let count = u64::try_from(count).map_err(|_| {
            cadmpeg_core::CodecError::NotImplemented(
                "Parasolid scan candidate count exceeds u64".into(),
            )
        })?;
        ctx.charge_collection_items(count, "admit Parasolid scan candidates")?;

    Ok(())
}

fn admit_brep_entity(ctx: &DecodeContext<'_>) -> Result<(), cadmpeg_core::CodecError> {
            ctx.charge_entities(1, "admit SLDPRT B-rep entity")?;

    Ok(())
}

fn unique_body_modifiers(
    ctx: &DecodeContext<'_>,
    modifiers: Vec<attrib::BodyModifier>,
) -> Result<Vec<attrib::BodyModifier>, cadmpeg_core::CodecError> {
    let mut by_attr = HashMap::<u16, Option<attrib::BodyModifier>>::new();
    for modifier in modifiers {
        ctx.charge_work(1, "select Parasolid body modifiers")?;
        reserve_graph_map_key(
            ctx,
            &mut by_attr,
            &modifier.body_attr,
            "index Parasolid body modifiers",
        )?;
        match by_attr.entry(modifier.body_attr) {
            std::collections::hash_map::Entry::Vacant(entry) => {
                entry.insert(Some(modifier));
            }
            std::collections::hash_map::Entry::Occupied(mut entry) => {
                if entry
                    .get()
                    .as_ref()
                    .is_some_and(|previous| previous.history_ordinal != modifier.history_ordinal)
                {
                    *entry.get_mut() = None;
                }
            }
        }
    }
    let mut out = Vec::new();
    for modifier in by_attr.into_values().flatten() {
        ctx.reserve_collection_vec(&mut out, 1, "collect Parasolid body modifiers")?;
        out.push(modifier);
    }
    out.sort_by_key(|modifier| modifier.body_attr);
    Ok(out)
}

fn unique_face_colors(
    ctx: &DecodeContext<'_>,
    colors: Vec<entity::FaceColor>,
    versions: Vec<entity::FaceColorVersion>,
) -> Result<(Vec<entity::FaceColor>, usize), cadmpeg_core::CodecError> {
    let mut current_versions = HashMap::<u16, (u32, usize)>::new();
    for version in versions {
        ctx.charge_work(1, "select current Parasolid face colors")?;
        reserve_graph_map_key(
            ctx,
            &mut current_versions,
            &version.face_attr,
            "index Parasolid face color versions",
        )?;
        current_versions
            .entry(version.face_attr)
            .and_modify(|current| {
                *current = (*current).max((version.seq, version.stream_order));
            })
            .or_insert((version.seq, version.stream_order));
    }

    let mut by_face = HashMap::<u16, Vec<entity::FaceColor>>::new();
    for color in colors {
        ctx.charge_work(1, "select Parasolid face colors")?;
        if current_versions.get(&color.face_attr) == Some(&(color.face_seq, color.stream_order)) {
            reserve_graph_map_key(ctx, &mut by_face, &color.face_attr, "index Parasolid face colors")?;
            let candidates = by_face.entry(color.face_attr).or_default();
            ctx.reserve_collection_vec(candidates, 1, "collect Parasolid face color candidates")?;
            candidates.push(color);
        }
    }

    let mut unresolved = 0;
    let mut selected = Vec::new();
    for mut candidates in by_face.into_values() {
        let first = &candidates[0];
        if candidates.iter().all(|candidate| {
            candidate.color_attr == first.color_attr && candidate.color == first.color
        }) {
            ctx.reserve_collection_vec(&mut selected, 1, "collect selected Parasolid face colors")?;
            selected.push(candidates.swap_remove(0));
        } else {
            unresolved += 1;
        }
    }

    let mut by_color = HashMap::<u16, Vec<entity::FaceColor>>::new();
    for color in selected {
        ctx.charge_work(1, "resolve Parasolid color identities")?;
        reserve_graph_map_key(ctx, &mut by_color, &color.color_attr, "index Parasolid color identities")?;
        let candidates = by_color.entry(color.color_attr).or_default();
        ctx.reserve_collection_vec(candidates, 1, "collect Parasolid color identity candidates")?;
        candidates.push(color);
    }
    let mut out = Vec::new();
    for candidates in by_color.into_values() {
        let first = &candidates[0];
        if candidates
            .iter()
            .all(|candidate| candidate.color == first.color)
        {
            ctx.reserve_collection_vec(&mut out, candidates.len(), "collect resolved Parasolid face colors")?;
            out.extend(candidates);
        } else {
            unresolved += candidates.len();
        }
    }
    out.sort_by_key(|color| (color.face_attr, color.offset));
    Ok((out, unresolved))
}

fn typed_body_records(
    ctx: &DecodeContext<'_>,
    facts: &typed::Facts,
    tables: &topology::Tables,
) -> Result<Option<Vec<BodyRecord>>, cadmpeg_core::CodecError> {
    let mut bridge_attrs = HashSet::new();
    for attr in tables.bridges().keys().copied() {
        reserve_graph_set_key(ctx, &mut bridge_attrs, &attr, "index Parasolid body bridges")?;
        bridge_attrs.insert(attr);
    }
    let Some(hierarchies) = facts.hierarchies(ctx, &bridge_attrs)? else {
        return Ok(None);
    };
    let mut records = Vec::new();
    for hierarchy in hierarchies {
        ctx.charge_work(1, "assemble typed Parasolid bodies")?;
        let mut body_refs = Vec::new();
        for attr in hierarchy
            .regions
            .iter()
            .map(|region| region.attr)
            .chain(hierarchy.shells.iter().map(|shell| shell.attr))
            .chain(hierarchy.faces.iter().map(|(face, _)| *face))
        {
            ctx.reserve_collection_vec(&mut body_refs, 1, "collect typed Parasolid body references")?;
            body_refs.push(attr);
        }
        body_refs.sort_unstable();
        body_refs.dedup();
        let mut regions = Vec::new();
        for region in &hierarchy.regions {
            let mut shells = Vec::new();
            for shell in hierarchy.shells.iter().filter(|shell| {
                u16::try_from(shell.refs[6]).ok() == Some(region.attr)
            }) {
                let mut refs = Vec::new();
                for face_attr in hierarchy
                    .faces
                    .iter()
                    .filter(|(_, shell_attr)| *shell_attr == shell.attr)
                    .map(|(face_attr, _)| *face_attr)
                {
                    ctx.reserve_collection_vec(&mut refs, 1, "collect typed Parasolid shell faces")?;
                    refs.push(face_attr);
                }
                refs.sort_unstable();
                refs.dedup();
                ctx.reserve_collection_vec(&mut shells, 1, "collect typed Parasolid region shells")?;
                shells.push(ShellRecord {
                    attr: shell.attr,
                    offset: shell.offset,
                    refs,
                });
            }
            shells.sort_by_key(|shell| shell.attr);
            ctx.reserve_collection_vec(&mut regions, 1, "collect typed Parasolid body regions")?;
            regions.push(RegionRecord {
                attr: region.attr,
                offset: region.offset,
                shells,
            });
        }
        regions.sort_by_key(|region| region.attr);
        ctx.reserve_collection_vec(&mut records, 1, "collect typed Parasolid body records")?;
        records.push(BodyRecord {
            attr: hierarchy.body.attr,
            kind: hierarchy.body.kind,
            refs: body_refs,
            offset: hierarchy.body.offset,
            regions,
        });
    }
    records.sort_by_key(|record| record.attr);
    Ok((!records.is_empty()).then_some(records))
}

fn sorted_topology_sequences(
    ctx: &DecodeContext<'_>,
    pairs: impl ExactSizeIterator<Item = (u32, u16)>,
    operation: &'static str,
) -> Result<Vec<(u32, u16)>, cadmpeg_core::CodecError> {
    let count = pairs.len();
    let work = u64::try_from(count)
        .map_err(|_| ctx.refuse_codec_limit(operation, u64::MAX - 1, u64::MAX))?;
    ctx.charge_work(work, operation)?;
    let mut sequences = Vec::new();
    ctx.reserve_collection_vec(&mut sequences, count, operation)?;
    sequences.extend(pairs);
    sequences.sort_unstable();
    sequences.dedup();
    Ok(sequences)
}

fn sorted_graph_attrs(
    ctx: &DecodeContext<'_>,
    attrs: impl ExactSizeIterator<Item = u16>,
    operation: &'static str,
) -> Result<Vec<u16>, cadmpeg_core::CodecError> {
    let count = attrs.len();
    let work = u64::try_from(count)
        .map_err(|_| ctx.refuse_codec_limit(operation, u64::MAX - 1, u64::MAX))?;
    ctx.charge_work(work, operation)?;
    let mut sorted = Vec::new();
    ctx.reserve_collection_vec(&mut sorted, count, operation)?;
    sorted.extend(attrs);
    sorted.sort_unstable();
    Ok(sorted)
}

fn copy_graph_stream_name(
    ctx: &DecodeContext<'_>,
    stream: &cadmpeg_ir::StreamName,
) -> Result<cadmpeg_ir::StreamName, cadmpeg_core::CodecError> {
    let mut name = String::new();
    ctx.reserve_retained_string(&mut name, stream.as_str().len(), "copy Parasolid graph stream name")?;
    name.push_str(stream.as_str());
    cadmpeg_ir::StreamName::try_from(name)
        .map_err(|_| cadmpeg_core::CodecError::malformed("empty Parasolid graph stream name"))
}

fn decode_graph(
    ctx: &DecodeContext<'_>,
    carriers: &mut CarrierIndex,
    t: &topology::Tables,
    entity_facts: entity::Facts,
    typed_facts: &typed::Facts,
    stream: &cadmpeg_ir::StreamName,
) -> Result<Brep, cadmpeg_core::CodecError> {
    let typed_records = typed_body_records(ctx, typed_facts, t)?;
    let body_records = typed_records.unwrap_or_default();
    let body_modifiers = unique_body_modifiers(ctx, entity_facts.body_modifiers)?;
    let (face_colors, conflicting_face_colors) =
        unique_face_colors(ctx, entity_facts.face_colors, entity_facts.face_color_versions)?;
    let face_bridge_sequences = sorted_topology_sequences(
        ctx,
        t.bridges().values().map(|bridge| (bridge.sequence, bridge.attr)),
        "collect Parasolid face bridge sequences",
    )?;
    let edge_use_sequences = sorted_topology_sequences(
        ctx,
        t.edge_uses().values().map(|edge_use| (edge_use.sequence, edge_use.attr)),
        "collect Parasolid edge use sequences",
    )?;
    let vertex_use_sequences = sorted_topology_sequences(
        ctx,
        t.vertex_uses().values().map(|vertex_use| (vertex_use.sequence, vertex_use.attr)),
        "collect Parasolid vertex use sequences",
    )?;

    let mut owned_face_colors = Vec::new();
    for value in face_colors {
        ctx.reserve_collection_vec(&mut owned_face_colors, 1, "collect Parasolid face colors")?;
        owned_face_colors.push(OwnedFaceColor {
            value,
            source_stream: copy_graph_stream_name(ctx, stream)?,
            site_key: None,
        });
    }
    let mut out = Brep {
        face_colors: owned_face_colors,
        face_bridge_sequences,
        edge_use_sequences,
        vertex_use_sequences,
        body_modifiers,
        losses: std::mem::take(&mut carriers.lane_refusals),
        stats: Stats {
            source_entity_records: entity_facts.entity_count,
            unresolved_face_colors: entity_facts.unresolved_face_colors + conflicting_face_colors,
            ..Stats::default()
        },
        ..Brep::default()
    };
    let mut annotations = AnnotationBuilder::new();
    let source_stream = StreamHandle::new(copy_graph_stream_name(ctx, stream)?);
    if t.bridges().is_empty() {
        return Ok(out);
    }

    // Walk every face-use bridge to collect its ordered loop/coedge structure.
    // A bridge owner identifies the canonical face entity, not an additional
    // face identity. Equivalent bridge payloads are duplicate uses; distinct
    // payloads have no source selector and must remain unresolved together.
    let mut faces = Vec::new();
    let mut owned_faces = HashMap::<u16, Vec<(&topology::Bridge, WalkedFace)>>::new();
    for bridge in t.bridges().values() {
        ctx.charge_work(1, "walk Parasolid face bridges")?;
        let face = walk_face(ctx, bridge, t)?;
        if let Some(owner) = bridge.owner {
            reserve_graph_map_key(ctx, &mut owned_faces, &owner, "index Parasolid face owners")?;
            let uses = owned_faces.entry(owner).or_default();
            ctx.reserve_collection_vec(uses, 1, "collect Parasolid face uses")?;
            uses.push((bridge, face));
        } else {
            ctx.reserve_collection_vec(&mut faces, 1, "collect Parasolid faces")?;
            faces.push(face);
        }
    }
    let mut ambiguous_face_owners = 0;
    for mut uses in owned_faces.into_values() {
        let work = u64::try_from(uses.len()).map_err(|_| {
            ctx.refuse_codec_limit("resolve Parasolid face owners", u64::MAX - 1, u64::MAX)
        })?;
        ctx.charge_work(work, "resolve Parasolid face owners")?;
        uses.sort_by_key(|(bridge, _)| (bridge.offset, bridge.attr));
        let Some((first_bridge, first_face)) = uses.first() else {
            continue;
        };
        let equivalent = uses.iter().skip(1).all(|(bridge, face)| {
            bridge.refs == first_bridge.refs
                && bridge.sense == first_bridge.sense
                && face.surface_attr == first_face.surface_attr
                && face.sense == first_face.sense
                && face.loops == first_face.loops
        });
        if equivalent {
            if let Some((_, first_face)) = uses.into_iter().next() {
                ctx.reserve_collection_vec(&mut faces, 1, "collect Parasolid faces")?;
                faces.push(first_face);
            }
        } else {
            ambiguous_face_owners += 1;
        }
    }
    faces.sort_by_key(|face| face.bridge_attr);
    out.stats.ambiguous_face_owners += ambiguous_face_owners;

    // Edge attr -> [(coedge attr, start vuse, next coedge's start vuse)] from
    // the ring walk. The ring order supplies a boundary edge's second endpoint;
    // a reciprocal twin supplies it for a two-sided edge.
    let mut edge_incidence: HashMap<u16, Vec<(u16, u16, u16)>> = HashMap::new();

    for f in &faces {
        for (_loop_attr, ring) in &f.loops {
            let k = ring.len();
            for (i, &ce_attr) in ring.iter().enumerate() {
                ctx.charge_work(1, "index Parasolid edge incidences")?;
                let Some(ce) = t.coedges().get(&ce_attr) else {
                    continue;
                };
                let next_attr = ring[(i + 1) % k];
                let start_vuse = ce.refs[4];
                let next_vuse = t.coedges().get(&next_attr).map_or(0, |next| next.refs[4]);
                let edge_attr = ce.refs[6];
                if edge_attr != 0 {
                    reserve_graph_map_key(ctx, &mut edge_incidence, &edge_attr, "index Parasolid edge incidences")?;
                    let incidences = edge_incidence.entry(edge_attr).or_default();
                    ctx.reserve_collection_vec(incidences, 1, "collect Parasolid edge incidences")?;
                    incidences.push((ce_attr, start_vuse, next_vuse));
                }
            }
        }
    }

    // Kept-entity sets, so only chain-reachable records are emitted.
    let mut kept_vertices: HashSet<u16> = HashSet::new();
    let mut kept_points: HashSet<u16> = HashSet::new();
    // Edge attr -> (canonical start vuse, canonical end vuse, curve carrier attr).
    let mut edge_ends: HashMap<u16, (u16, u16, u16)> = HashMap::new();

    for (edge_attr, incidences) in edge_incidence {
        ctx.charge_work(1, "resolve Parasolid edge incidences")?;
        let canonical =
            canonical_coedge_attr(edge_attr, t.edge_uses().get(&edge_attr), t.coedges());
        let Some(canonical) = canonical else {
            continue;
        };
        let Some((_, start_vuse, ring_end_vuse)) = incidences
            .iter()
            .find(|(coedge_attr, _, _)| *coedge_attr == canonical)
        else {
            continue;
        };
        let end_vuse = edge_end_vuse(canonical, *ring_end_vuse, t.coedges());
        let curve_attr = t
            .edge_uses()
            .get(&edge_attr)
            .map_or(0, |edge_use| edge_use.references.curve());
        reserve_graph_map_key(ctx, &mut edge_ends, &edge_attr, "index Parasolid edge endpoints")?;
        edge_ends.insert(edge_attr, (*start_vuse, end_vuse, curve_attr));
        for vuse in [*start_vuse, end_vuse] {
            if vuse == 0 {
                continue;
            }
            if let Some(vu) = t.vertex_uses().get(&vuse) {
                let point_attr = vu.refs[4];
                if t.points().contains_key(&point_attr) {
                    reserve_graph_set_key(ctx, &mut kept_vertices, &vuse, "track Parasolid vertices")?;
                    kept_vertices.insert(vuse);
                    reserve_graph_set_key(ctx, &mut kept_points, &point_attr, "track Parasolid points")?;
                    kept_points.insert(point_attr);
                }
            }
        }
    }

    // Points.
    let point_attrs = sorted_graph_attrs(ctx, kept_points.iter().copied(), "order Parasolid points")?;
    for a in point_attrs {
        let rec = &t.points()[&a];
        annotations
            .note(id_point(a), &source_stream, rec.offset as u64)
            .tag("00_1d");
        let [x, y, z] = rec.xyz_m;
        let finite_position = cadmpeg_ir::features::FinitePoint3::new(
            cadmpeg_ir::math::Point3::new(x * LEN_TO_MM, y * LEN_TO_MM, z * LEN_TO_MM),
        )
        .ok_or(Point::NON_FINITE_POSITION)
        .map_err(cadmpeg_core::CodecError::malformed)?;
        admit_brep_entity(ctx)?;
        ctx.reserve_collection_vec(&mut out.points, 1, "collect Parasolid points")?;
        out.points
            .push(Point::new(id_point(a), finite_position, None));
    }

    // Vertices.
    let vuse_attrs = sorted_graph_attrs(ctx, kept_vertices.iter().copied(), "order Parasolid vertices")?;
    for a in vuse_attrs {
        let rec = &t.vertex_uses()[&a];
        let point_attr = rec.refs[4];
        annotations
            .note(id_vertex(a), &source_stream, rec.offset as u64)
            .tag("00_12");
        admit_brep_entity(ctx)?;
        ctx.reserve_collection_vec(&mut out.vertices, 1, "collect Parasolid vertices")?;
        out.vertices.push(Vertex {
            id: id_vertex(a),
            point: id_point(point_attr),
            tolerance: None,
        });
    }

    // Curves and edges. An edge keeps a curve only when its carrier decodes to a
    // curve kind; a nonzero-but-untyped carrier is counted as loss.
    let mut emitted_curves: HashSet<u16> = HashSet::new();
    let mut edge_set: HashSet<u16> = HashSet::new();
    let mut edge_endpoint_positions = HashMap::<u16, [cadmpeg_ir::math::Point3; 2]>::new();
    let mut reversed_edge_orientation = HashSet::<u16>::new();
    let edge_attrs = sorted_graph_attrs(ctx, edge_ends.keys().copied(), "order Parasolid edges")?;
    for e in edge_attrs {
        let (start_v, end_v, curve_attr) = edge_ends[&e];
        let resolved_endpoints = kept_vertices.contains(&start_v) && kept_vertices.contains(&end_v);
        let closed_circle_point = (!resolved_endpoints && start_v <= 1 && end_v <= 1)
            .then(|| carriers.curve(curve_attr))
            .flatten()
            .and_then(|carrier| match &carrier.carrier().geometry {
                CurveGeometry::Solved(SolvedCurveGeometry::Circle(circle_curve)) => {
                    let center = circle_curve.center().get();
                    let ref_direction = circle_curve.frame().reference().as_raw();
                    let radius = circle_curve.radius().get();
                    Some(cadmpeg_ir::math::Point3::new(
                        center.x + ref_direction.x * radius,
                        center.y + ref_direction.y * radius,
                        center.z + ref_direction.z * radius,
                    ))
                }
                _ => None,
            });
        if !resolved_endpoints && closed_circle_point.is_none() {
            continue;
        }
        let (mut start_id, mut end_id) = if let Some(position) = closed_circle_point {
            let point_id = id_closed_point(e);
            let vertex_id = id_closed_vertex(e);
            annotations
                .note(&point_id, &source_stream, 0)
                .tag("derived_closed_circle_seam");
            annotations.exactness(&point_id, Exactness::Derived);
            annotations
                .note(&vertex_id, &source_stream, 0)
                .tag("derived_closed_circle_seam");
            annotations.exactness(&vertex_id, Exactness::Derived);
            let finite_position = cadmpeg_ir::features::FinitePoint3::new(position)
                .ok_or(Point::NON_FINITE_POSITION)
                .map_err(cadmpeg_core::CodecError::malformed)?;
            admit_brep_entity(ctx)?;
            ctx.reserve_collection_vec(&mut out.points, 1, "collect Parasolid closed-circle points")?;
            out.points
                .push(Point::new(point_id.clone(), finite_position, None));
            admit_brep_entity(ctx)?;
            ctx.reserve_collection_vec(&mut out.vertices, 1, "collect Parasolid closed-circle vertices")?;
            out.vertices.push(Vertex {
                id: vertex_id.clone(),
                point: point_id,
                tolerance: None,
            });
            (vertex_id.clone(), vertex_id)
        } else {
            (id_vertex(start_v), id_vertex(end_v))
        };
        if resolved_endpoints {
            let position = |vertex_use: u16| {
                let point_attr = &t.vertex_uses().get(&vertex_use)?.refs[4];
                let [x, y, z] = t.points().get(point_attr)?.xyz_m;
                Some(cadmpeg_ir::math::Point3::new(
                    x * LEN_TO_MM,
                    y * LEN_TO_MM,
                    z * LEN_TO_MM,
                ))
            };
            if let (Some(start), Some(end)) = (position(start_v), position(end_v)) {
                reserve_graph_map_key(ctx, &mut edge_endpoint_positions, &e, "index Parasolid edge endpoint positions")?;
                edge_endpoint_positions.insert(e, [start, end]);
            }
        }
        let parameter_range = carriers
            .curve(curve_attr)
            .map(|carrier| {
                edge_parameter_range(carrier.carrier(), edge_endpoint_positions.get(&e).copied())
            })
            .transpose()?
            .flatten();
        if parameter_range.is_some_and(|(_, reversed)| reversed) {
            std::mem::swap(&mut start_id, &mut end_id);
            if let Some(endpoints) = edge_endpoint_positions.get_mut(&e) {
                endpoints.swap(0, 1);
            }
            reserve_graph_set_key(ctx, &mut reversed_edge_orientation, &e, "track reversed Parasolid edges")?;
            reversed_edge_orientation.insert(e);
        }
        let eu = t.edge_uses().get(&e);
        let mut curve = None;
        if curve_attr != 0 {
            match carriers.curve(curve_attr) {
                Some(indexed) => {
                    let carrier = indexed.carrier();
                    reserve_graph_set_key(ctx, &mut emitted_curves, &curve_attr, "track emitted Parasolid curves")?;
                    if emitted_curves.insert(curve_attr) {
                        emit_curve(ctx, &mut out, carrier)?;
                        if matches!(indexed, IndexedCurve::Derived(_)) {
                            let offset = carrier.offset;
                            annotations
                                .note(id_curve(curve_attr), &source_stream, offset as u64)
                                .tag("surface_intersection");
                            annotations.exactness(id_curve(curve_attr), Exactness::Derived);
                        }
                    }
                    curve = Some(id_curve(curve_attr));
                }
                _ => {
                    reserve_graph_set_key(ctx, &mut emitted_curves, &curve_attr, "track emitted Parasolid curves")?;
                    if emitted_curves.insert(curve_attr) {
                        let offset = eu.map_or(0, |record| record.offset);
                        annotations
                            .note(id_curve(curve_attr), &source_stream, offset as u64)
                            .tag("unknown_curve");
                        annotations.exactness(id_curve(curve_attr), Exactness::Unknown);
                        admit_brep_entity(ctx)?;
                        ctx.reserve_collection_vec(&mut out.curves, 1, "collect unknown Parasolid curves")?;
                        out.curves.push(Curve {
                            id: id_curve(curve_attr),
                            source_object: None,
                            geometry: CurveGeometry::Solved(SolvedCurveGeometry::Unknown {
                                record: None,
                            }),
                        });
                    }
                    curve = Some(id_curve(curve_attr));
                    out.stats.unknown_curve_edges += 1;
                }
            }
        }
        let off = eu.map_or(0, |r| r.offset);
        annotations
            .note(id_edge(e), &source_stream, off as u64)
            .tag("00_10");
        admit_brep_entity(ctx)?;
        ctx.reserve_collection_vec(&mut out.edges, 1, "collect Parasolid edges")?;
        out.edges.push(Edge {
            id: id_edge(e),
            carrier: cadmpeg_ir::topology::EdgeCarrier::new(
                curve,
                parameter_range.map(|(range, _)| range),
            )
            .map_err(cadmpeg_core::CodecError::malformed)?,
            start: start_id,
            end: end_id,
            tolerance: None,
        });
        reserve_graph_set_key(ctx, &mut edge_set, &e, "track emitted Parasolid edges")?;
        edge_set.insert(e);
    }

    // A loop is kept only when its whole ring resolves: every coedge exists and
    // its edge was emitted. A partial ring is dropped whole, so an emitted
    // coedge's `next`/`prev` never dangle and every emitted loop closes.
    let mut kept_loops: HashSet<u16> = HashSet::new();
    for f in &faces {
        for (loop_attr, ring) in &f.loops {
            let work = u64::try_from(ring.len()).map_err(|_| {
                ctx.refuse_codec_limit("check Parasolid loop ring", u64::MAX - 1, u64::MAX)
            })?;
            ctx.charge_work(work, "check Parasolid loop ring")?;
            let ok = !ring.is_empty()
                && ring.iter().all(|c| {
                    t.coedges()
                        .get(c)
                        .is_some_and(|ce| edge_set.contains(&ce.refs[6]))
                });
            if ok {
                reserve_graph_set_key(ctx, &mut kept_loops, loop_attr, "track kept Parasolid loops")?;
                kept_loops.insert(*loop_attr);
            }
        }
    }
    let mut emitted_coedges = HashSet::new();
    for f in &faces {
        for (loop_attr, ring) in &f.loops {
            if kept_loops.contains(loop_attr) {
                for coedge in ring.iter().copied() {
                    ctx.charge_work(1, "index emitted Parasolid coedges")?;
                    reserve_graph_set_key(ctx, &mut emitted_coedges, &coedge, "track emitted Parasolid coedges")?;
                    emitted_coedges.insert(coedge);
                }
            }
        }
    }

    // Coedges of kept loops: `next`/`prev` from the ring order, partner from a
    // mutual twin that is itself emitted.
    for f in &faces {
        for (loop_attr, ring) in &f.loops {
            if !kept_loops.contains(loop_attr) {
                continue;
            }
            for &ce_attr in ring {
                let ce = &t.coedges()[&ce_attr];
                let edge_attr = ce.refs[6];
                let twin = ce.refs[5];
                let partner = t
                    .coedges()
                    .get(&twin)
                    .filter(|tw| tw.refs[5] == ce_attr)
                    .filter(|_| emitted_coedges.contains(&twin))
                    .map(|_| id_coedge(twin));
                annotations
                    .note(id_coedge(ce_attr), &source_stream, ce.offset as u64)
                    .tag("00_11");
                let mut pcurve_refusal = crate::lane_refusal::LaneRefusals::new();
                let pcurve_refusal = &mut pcurve_refusal;
                let pcurves = if let Some((_, _, curve_attr)) = edge_ends.get(&edge_attr) {
                    (|| -> Result<_, cadmpeg_core::CodecError> {
                        let Some(IndexedCurve::Derived(intersection)) = carriers.curve(*curve_attr)
                        else {
                            return Ok(None);
                        };
                        let support_data = &intersection.support_data;
                        let curve_carrier = &intersection.carrier;
                        let Some(SolvedCurveGeometry::Nurbs(curve)) =
                            curve_carrier.geometry.solved()
                        else {
                            return Ok(None);
                        };
                        let Some(surface) = carriers.surface(f.surface_attr) else {
                            return Ok(None);
                        };
                        let surface = &surface.geometry;
                        let Some(endpoint_positions) = edge_endpoint_positions.get(&edge_attr)
                        else {
                            return Ok(None);
                        };
                        let Some((geometry, parameter_range, source)) =
                            intersection_support_pcurve(
                                ctx,
                                support_data,
                                curve,
                                f.surface_attr,
                                surface,
                                *endpoint_positions,
                                pcurve_refusal,
                            )?
                        else {
                            return Ok(None);
                        };
                        let Some(finite_range) =
                            cadmpeg_ir::units::FiniteVector::new(parameter_range)
                        else {
                            return Ok(None);
                        };
                        let Ok(fit_tolerance) = cadmpeg_ir::geometry::FitTolerance::try_new(
                            support_data.fit_tolerance_mm,
                        ) else {
                            return Ok(None);
                        };
                        let id = PcurveId::compose(
                            &pcurve_namespace(),
                            cadmpeg_ir::identity_key!("intersection:").then(ce_attr),
                        );
                        let offset = curve_carrier.offset;
                        annotations
                            .note(&id, &source_stream, offset as u64)
                            .tag(match source {
                                IntersectionPcurveSource::StoredCache => "surface_intersection_uv",
                                IntersectionPcurveSource::AnalyticInverse => {
                                    "derived_intersection_analytic_uv"
                                }
                                IntersectionPcurveSource::NurbsInverse => {
                                    "derived_intersection_nurbs_uv"
                                }
                            });
                        annotations.exactness(&id, Exactness::Derived);
                        admit_brep_entity(ctx)?;
                        ctx.reserve_collection_vec(&mut out.pcurves, 1, "collect intersection Parasolid pcurves")?;
                        out.pcurves.push(Pcurve {
                            id: id.clone(),
                            geometry,
                            metadata: cadmpeg_ir::geometry::pcurve::PcurveMetadata::general(
                                None,
                                Some(finite_range),
                                Some(fit_tolerance),
                            ),
                        });
                        Ok(Some(
                            cadmpeg_ir::geometry::DirectedParameterRange::new(parameter_range).map(
                                |range| {
                                    vec![cadmpeg_ir::topology::PcurveUse {
                                        pcurve: id,
                                        isoparametric: None,
                                        parameter_range: Some(range),
                                    }]
                                },
                            ),
                        ))
                    })()?
                } else {
                    None
                };
                // The sink is drained before the `?` below: an error on that
                // route must not drop a refusal the walk above already pushed.
                for record in pcurve_refusal.take_records() {
                    let note = crate::loss::spline_lane_refusal(
                        ctx, format_args!("intersection pcurve for coedge {ce_attr}: {record}"),
                    )?;
                    ctx.reserve_collection_vec(&mut out.losses, 1, "collect intersection pcurve losses")?;
                    out.losses.push(note);
                }
                let pcurves = pcurves
                    .transpose()
                    .map_err(cadmpeg_core::CodecError::malformed)?
                    .unwrap_or_default();
                let mut sense = ce.sense;
                if reversed_edge_orientation.contains(&edge_attr) {
                    sense = match sense {
                        Sense::Forward => Sense::Reversed,
                        Sense::Reversed => Sense::Forward,
                    };
                }
                admit_brep_entity(ctx)?;
                ctx.reserve_collection_vec(&mut out.coedges, 1, "collect Parasolid coedges")?;
                out.coedges.push(Coedge {
                    id: id_coedge(ce_attr),
                    owner_loop: id_loop(*loop_attr),
                    edge: id_edge(edge_attr),
                    radial_next: partner.unwrap_or_else(|| id_coedge(ce_attr)),
                    sense,
                    use_curve: None,
                    pcurves,
                });
            }
        }
    }

    // Loops.
    for f in &faces {
        for (loop_attr, ring) in &f.loops {
            if !kept_loops.contains(loop_attr) {
                continue;
            }
            let mut coedges = Vec::new();
            ctx.reserve_collection_vec(&mut coedges, ring.len(), "collect Parasolid loop coedges")?;
            coedges.extend(ring.iter().map(|a| id_coedge(*a)));
            let off = t.loops().get(loop_attr).map_or(0, |r| r.offset);
            annotations
                .note(id_loop(*loop_attr), &source_stream, off as u64)
                .tag("00_0f");
            let Ok(ring) = cadmpeg_ir::topology::LoopRing::new(coedges, Vec::new()) else {
                continue;
            };
            admit_brep_entity(ctx)?;
            ctx.reserve_collection_vec(&mut out.loops, 1, "collect Parasolid loops")?;
            out.loops.push(Loop {
                id: id_loop(*loop_attr),
                face: id_face(f.bridge_attr),
                boundary: cadmpeg_ir::topology::LoopBoundary::Ring(ring),
            });
        }
    }
    let loop_set = kept_loops;

    // Surfaces + faces.
    let bind_bridges = |body_records: &[BodyRecord],
                        faces: &[WalkedFace]|
     -> Result<(HashMap<u16, usize>, HashMap<u16, u16>), cadmpeg_core::CodecError> {
        let mut bridge_group = HashMap::new();
        let mut bridge_shell = HashMap::new();
        for (group, body_record) in body_records.iter().enumerate() {
            for face in faces {
                ctx.charge_work(1, "bind Parasolid face bridges")?;
                let owner = t.bridges().get(&face.bridge_attr).and_then(|r| r.owner);
                if body_record.refs.contains(&face.bridge_attr)
                    || owner.is_some_and(|owner| body_record.refs.contains(&owner))
                {
                    reserve_graph_map_key(ctx, &mut bridge_group, &face.bridge_attr, "index Parasolid bridge groups")?;
                    bridge_group.insert(face.bridge_attr, group);
                    if let Some(shell) = body_record
                        .regions
                        .iter()
                        .flat_map(|region| &region.shells)
                        .find(|shell| {
                            shell.refs.contains(&face.bridge_attr)
                                || owner.is_some_and(|owner| shell.refs.contains(&owner))
                        })
                    {
                        reserve_graph_map_key(ctx, &mut bridge_shell, &face.bridge_attr, "index Parasolid bridge shells")?;
                        bridge_shell.insert(face.bridge_attr, shell.attr);
                    }
                }
            }
        }
        Ok((bridge_group, bridge_shell))
    };
    let (bridge_group, bridge_shell) = bind_bridges(&body_records, &faces)?;
    if !body_records.is_empty() {
        out.stats.unclaimed_faces += faces
            .iter()
            .filter(|face| !bridge_group.contains_key(&face.bridge_attr))
            .count();
        faces.retain(|face| bridge_group.contains_key(&face.bridge_attr));
    }
    let mut face_edges_by_surface_carrier = HashMap::<u16, Vec<HashSet<u16>>>::new();
    for face in &faces {
        let mut edges = HashSet::new();
        for (_, ring) in &face.loops {
            for coedge in ring {
                ctx.charge_work(1, "index Parasolid face edges")?;
                if let Some(edge) = t.coedges().get(coedge).map(|coedge| coedge.refs[6]) {
                    if edge != 0 {
                        reserve_graph_set_key(ctx, &mut edges, &edge, "track Parasolid face edges")?;
                        edges.insert(edge);
                    }
                }
            }
        }
        reserve_graph_map_key(ctx, &mut face_edges_by_surface_carrier, &face.surface_attr, "index Parasolid surface face edges")?;
        let groups = face_edges_by_surface_carrier.entry(face.surface_attr).or_default();
        ctx.reserve_collection_vec(groups, 1, "collect Parasolid surface face edges")?;
        groups.push(edges);
    }
    let mut emitted_face_surface_by_carrier = HashMap::<u16, u16>::new();
    for face in &faces {
        ctx.charge_work(1, "select Parasolid face surface carriers")?;
        if face
            .loops
            .iter()
            .any(|(loop_attr, _)| loop_set.contains(loop_attr))
        {
            reserve_graph_map_key(ctx, &mut emitted_face_surface_by_carrier, &face.surface_attr, "index Parasolid face surface carriers")?;
            emitted_face_surface_by_carrier
                .entry(face.surface_attr)
                .and_modify(|bridge| *bridge = (*bridge).min(face.bridge_attr))
                .or_insert(face.bridge_attr);
        }
    }
    for f in &faces {
        let mut loops = Vec::new();
        for (loop_attr, _) in &f.loops {
            ctx.charge_work(1, "select Parasolid face loops")?;
            if loop_set.contains(loop_attr) {
                ctx.reserve_collection_vec(&mut loops, 1, "collect Parasolid face loops")?;
                loops.push(id_loop(*loop_attr));
            }
        }
        if loops.is_empty() {
            continue;
        }
        // Support surface: a decoded surface carrier, else an opaque carrier.
        let surf_off = t.bridges().get(&f.bridge_attr).map_or(0, |r| r.offset);
        let mut surface_orientation_reversed = false;
        match carriers.surface(f.surface_attr) {
            Some(c) => {
                surface_orientation_reversed = c.orientation_reversed;
                annotations
                    .note(id_surf(f.bridge_attr), &source_stream, c.offset as u64)
                    .tag("compact_surface");
                let geometry = copy_surface_carrier_geometry(ctx, &c.geometry)?;
                if let SurfaceGeometry::Solved(solved) = &geometry {
                    annotate_surface_frame(
                        &mut annotations,
                        id_surf(f.bridge_attr).as_str(),
                        solved,
                    )?;
                }
                admit_brep_entity(ctx)?;
                out.surfaces.push(Surface {
                    id: id_surf(f.bridge_attr),
                    source_object: None,
                    geometry,
                });
            }
            _ => {
                let resolved_offset = if let Some(offset) = carriers.offset(f.surface_attr) {
                    if support_is_acyclic(ctx, offset.support, carriers, &mut HashSet::new())? {
                        ensure_surface_support(
                            &mut BrepSink { ctx, out: &mut out },
                            offset.support,
                            carriers,
                            &emitted_face_surface_by_carrier,
                            &mut annotations,
                            &source_stream,
                            &mut HashSet::new(),
                        )?
                        .map(|support| (offset, support))
                    } else {
                        None
                    }
                } else {
                    None
                };
                let blend_attrs = (|| -> Result<_, cadmpeg_core::CodecError> {
                    let Some(blend) = carriers.blend(f.surface_attr) else {
                        return Ok(None);
                    };
                    let mut face_edges = HashSet::new();
                    for (_, ring) in &f.loops {
                        for coedge in ring {
                            ctx.charge_work(1, "select Parasolid blend face edges")?;
                            if let Some(edge) = t.coedges().get(coedge).map(|coedge| coedge.refs[6]) {
                                if edge != 0 {
                                    reserve_graph_set_key(ctx, &mut face_edges, &edge, "collect Parasolid blend face edges")?;
                                    face_edges.insert(edge);
                                }
                            }
                        }
                    }
                    let [Some(first_attr), Some(second_attr)] =
                        blend.supports.map(|support| match support {
                            BlendSupportRef::Surface(attr) => Some(attr),
                            BlendSupportRef::Pair(attr) => {
                                let pair = carriers.blend_support_pair(attr)?;
                                carriers.curve(pair.intersection)?;
                                let mut adjacent = pair.supports.iter().filter_map(|candidate| {
                                    face_edges_by_surface_carrier
                                        .get(candidate)?
                                        .iter()
                                        .any(|edges| !face_edges.is_disjoint(edges))
                                        .then_some(*candidate)
                                });
                                let support = adjacent.next()?;
                                if adjacent.next().is_some() {
                                    return None;
                                }
                                Some(support)
                            }
                        })
                    else {
                        return Ok(None);
                    };
                    for attr in [first_attr, second_attr] {
                        if !support_is_acyclic(ctx, attr, carriers, &mut HashSet::new())? {
                            return Ok(None);
                        }
                    }
                    Ok(Some((blend, first_attr, second_attr)))
                })()?;
                let resolved_blend = if let Some((blend, first_attr, second_attr)) = blend_attrs {
                    if let Some(first) = ensure_surface_support(
                        &mut BrepSink { ctx, out: &mut out },
                        first_attr,
                        carriers,
                        &emitted_face_surface_by_carrier,
                        &mut annotations,
                        &source_stream,
                        &mut HashSet::new(),
                    )? {
                        ensure_surface_support(
                            &mut BrepSink { ctx, out: &mut out },
                            second_attr,
                            carriers,
                            &emitted_face_surface_by_carrier,
                            &mut annotations,
                            &source_stream,
                            &mut HashSet::new(),
                        )?
                        .map(|second| (blend, first, second))
                    } else {
                        None
                    }
                } else {
                    None
                };
                if let Some((offset, support)) = resolved_offset {
                    let construction = ProceduralSurfaceId::compose(
                        &cadmpeg_ir::identity_namespace!("sldprt", "brep", "offset-construction"),
                        f.bridge_attr,
                    );
                    emit_offset_surface(
                        &mut BrepSink { ctx, out: &mut out },
                        &mut annotations,
                        &source_stream,
                        id_surf(f.bridge_attr),
                        construction,
                        support,
                        offset,
                    )?;
                } else if let Some((blend, first, second)) = resolved_blend {
                    let spine = if let Some(indexed) = carriers.curve(blend.spine) {
                        let carrier = indexed.carrier();
                        reserve_graph_set_key(ctx, &mut emitted_curves, &blend.spine, "track Parasolid blend spines")?;
                        if emitted_curves.insert(blend.spine) {
                            emit_curve(ctx, &mut out, carrier)?;
                            annotations
                                .note(id_curve(blend.spine), &source_stream, carrier.offset as u64)
                                .tag("blend_spine");
                        }
                        Some(id_curve(blend.spine))
                    } else {
                        None
                    };
                    let procedural_id = ProceduralSurfaceId::compose(
                        &cadmpeg_ir::identity_namespace!("sldprt", "brep", "blend-construction"),
                        f.bridge_attr,
                    );
                    let admitted_payload = BlendRadiusLaw::constant(blend.signed_radius)
                        .and_then(|radius| {
                            cadmpeg_ir::geometry::surface_payloads::BlendSurfacePayload::try_new(
                                [
                                    Some(BlendSupport {
                                        surface: first,
                                        reversed: blend.reversed[0],
                                    }),
                                    Some(BlendSupport {
                                        surface: second,
                                        reversed: blend.reversed[1],
                                    }),
                                ],
                                spine,
                                radius,
                                BlendCrossSection::Circular,
                                cadmpeg_ir::geometry::CacheContract::from_form(None),
                            )
                        })
                        .map_err(cadmpeg_core::CodecError::malformed)?;
                    admit_brep_entity(ctx)?;
                    ctx.reserve_collection_vec(&mut out.procedural_surfaces, 1, "collect Parasolid blend constructions")?;
                    out.procedural_surfaces.push(ProceduralSurface::new(
                        procedural_id.clone(),
                        ProceduralSurfaceDefinition::Blend(admitted_payload),
                        None,
                    ));
                    let geometry = SurfaceGeometry::Procedural {
                        construction: procedural_id,
                        cache: None,
                    };
                    annotations
                        .note(id_surf(f.bridge_attr), &source_stream, blend.offset as u64)
                        .tag("00_38");
                    admit_brep_entity(ctx)?;
                    ctx.reserve_collection_vec(&mut out.surfaces, 1, "collect Parasolid blend surfaces")?;
                    out.surfaces.push(Surface {
                        id: id_surf(f.bridge_attr),
                        source_object: None,
                        geometry,
                    });
                } else if let Some((geometry, offset, tag, exactness)) = {
                    let mut sweep_refusal = crate::lane_refusal::LaneRefusals::new();
                    let resolved = resolve_sweep_surface(ctx, carriers, t, f, &mut sweep_refusal)?;
                    for record in sweep_refusal.take_records() {
                        let note = crate::loss::spline_lane_refusal(
                            ctx, format_args!("swept surface for face attr {}: {record}", f.surface_attr),
                        )?;
                        ctx.reserve_collection_vec(&mut out.losses, 1, "collect swept surface losses")?;
                        out.losses.push(note);
                    }
                    resolved
                } {
                    annotations
                        .note(id_surf(f.bridge_attr), &source_stream, offset as u64)
                        .tag(tag);
                    if let Some(exactness) = exactness {
                        annotations.exactness(id_surf(f.bridge_attr), exactness);
                    }
                    admit_brep_entity(ctx)?;
                    ctx.reserve_collection_vec(&mut out.surfaces, 1, "collect Parasolid swept surfaces")?;
                    out.surfaces.push(Surface {
                        id: id_surf(f.bridge_attr),
                        source_object: None,
                        geometry: SurfaceGeometry::Solved(geometry),
                    });
                } else {
                    out.stats.unknown_surface_faces += 1;
                    annotations
                        .note(id_surf(f.bridge_attr), &source_stream, surf_off as u64)
                        .tag("unknown_surface");
                    annotations.exactness(id_surf(f.bridge_attr), Exactness::Unknown);
                    admit_brep_entity(ctx)?;
                    ctx.reserve_collection_vec(&mut out.surfaces, 1, "collect Parasolid opaque surfaces")?;
                    out.surfaces.push(Surface {
                        id: id_surf(f.bridge_attr),
                        source_object: None,
                        geometry: SurfaceGeometry::Solved(SolvedSurfaceGeometry::Unknown {
                            record: None,
                        }),
                    });
                }
            }
        }
        annotations
            .note(id_face(f.bridge_attr), &source_stream, surf_off as u64)
            .tag("00_0e");
        admit_brep_entity(ctx)?;
        ctx.reserve_collection_vec(&mut out.faces, 1, "collect Parasolid faces")?;
        out.faces.push(Face {
            id: id_face(f.bridge_attr),
            shell: ShellId::compose(
                &shell_namespace(),
                bridge_shell
                    .get(&f.bridge_attr)
                    .copied()
                    .or_else(|| bridge_group.get(&f.bridge_attr).copied().map(|v| v as u16))
                    .unwrap_or(0),
            ),
            surface: id_surf(f.bridge_attr),
            sense: surface_sense(f.sense, surface_orientation_reversed),
            loops: cadmpeg_ir::topology::FaceLoops::unspecified(loops),
            name: None,
            color: t
                .bridges()
                .get(&f.bridge_attr)
                .and_then(|bridge| bridge.owner)
                .and_then(|owner| {
                    out.face_colors
                        .iter()
                        .find(|entry| entry.value.face_attr == owner)
                })
                .map(|entry| entry.value.color),
            tolerance: None,
        });
    }
    let mut emitted_faces = HashMap::new();
    ctx.charge_collection_items(out.faces.len() as u64, "index emitted Parasolid faces")?;
    emitted_faces.try_reserve(out.faces.len()).map_err(|_| {
        ctx.refuse_codec_limit("index emitted Parasolid faces", u64::MAX - 1, u64::MAX)
    })?;
    emitted_faces.extend(out.faces.iter().map(|face| (face.id.as_str(), &face.id)));
    for appearance in &mut out.face_colors {
        appearance.value.target = faces
            .iter()
            .find(|face| {
                t.bridges()
                    .get(&face.bridge_attr)
                    .and_then(|bridge| bridge.owner)
                    == Some(appearance.value.face_attr)
            })
            .map(|face| id_face(face.bridge_attr))
            .filter(|face| emitted_faces.contains_key(face.as_str()))
            .map(cadmpeg_ir::ids::FaceId::into_string);
    }
    let mut bound_faces = HashSet::new();
    for atom in entity_facts.face_atoms {
        let Some(identity) = atom.identity else {
            continue;
        };
        let Some(face) = emitted_faces.get(id_face(atom.face_attr).as_str()) else {
            continue;
        };
        reserve_graph_set_key(ctx, &mut bound_faces, &atom.face_attr, "track bound Parasolid faces")?;
        if bound_faces.insert(atom.face_attr) {
            ctx.reserve_collection_vec(&mut out.face_atoms, 1, "collect Parasolid face atoms")?;
            out.face_atoms.push(attrib::FaceAtom {
                face: (*face).clone(),
                identity,
            });
        }
    }
    solve_face_orientation(ctx, &mut out)?;
    synthesize_cylinder_seams(ctx, &mut out, &mut annotations, &source_stream)?;
    synthesize_sphere_seams(ctx, &mut out, &mut annotations, &source_stream)?;
    derive_planar_pcurves(ctx, &mut out, &mut annotations, &source_stream)?;
    derive_cylindrical_pcurves(ctx, &mut out, &mut annotations, &source_stream)?;
    derive_revolved_circle_pcurves(ctx, &mut out, &mut annotations, &source_stream)?;
    derive_spherical_pcurves(ctx, &mut out, &mut annotations, &source_stream)?;
    derive_nurbs_isoparametric_pcurves(ctx, &mut out, &mut annotations, &source_stream)?;
    prune_rejected_topology(ctx, &mut out)?;

    if out.faces.is_empty() {
        return Ok(Brep {
            losses: out.losses,
            stats: out.stats,
            ..Brep::default()
        });
    }
    let synthetic_grouping = body_records.is_empty();
    out.stats.synthetic_body_grouping = synthetic_grouping;

    for (group, body_record) in body_records
        .iter()
        .map(Some)
        .chain(std::iter::once(None).take(usize::from(synthetic_grouping)))
        .enumerate()
    {
        let body_id = BodyId::compose(
            &body_namespace(),
            body_record.map_or(0_u16, |record| record.attr),
        );
        let mut annotate_group = |id: &str, source: Option<(usize, &str)>| {
            let (offset, tag, exactness) = source.map_or(
                (0, "synthetic_grouping", Exactness::Derived),
                |(offset, tag)| (offset, tag, Exactness::ByteExact),
            );
            annotations.note(id, &source_stream, offset as u64).tag(tag);
            annotations.exactness(id, exactness);
        };
        annotate_group(
            body_id.as_str(),
            body_record.map(|record| (record.offset, "00_51_body")),
        );
        let native_regions = body_record.map_or(&[][..], |record| record.regions.as_slice());
        let mut body_regions = Vec::new();
        if native_regions.is_empty() {
            let region_id = RegionId::compose(&region_namespace(), group);
            let native_shell_id = ShellId::compose(&shell_namespace(), group);
            annotate_group(region_id.as_str(), None);
            let mut region_shells = Vec::new();
            for (component, faces) in shell_face_components(ctx, &out, native_shell_id.as_str())?
                .into_iter()
                .enumerate()
            {
                let shell_id = if component == 0 {
                    native_shell_id.clone()
                } else {
                    shell_component(&native_shell_id, component)
                };
                annotate_group(shell_id.as_str(), None);
                let mut face_ids = HashSet::new();
                ctx.charge_collection_items(faces.len() as u64, "index synthetic shell faces")?;
                face_ids.try_reserve(faces.len()).map_err(|_| {
                    ctx.refuse_codec_limit("index synthetic shell faces", u64::MAX - 1, u64::MAX)
                })?;
                face_ids.extend(faces.iter().map(cadmpeg_ir::ids::FaceId::as_str));
                for face in &mut out.faces {
                    if face_ids.contains(face.id.as_str()) {
                        face.shell = shell_id.clone();
                    }
                }
                admit_brep_entity(ctx)?;
                ctx.reserve_collection_vec(&mut out.shells, 1, "collect synthetic Parasolid shells")?;
                out.shells.push(
                    match Shell::new(
                        shell_id.clone(),
                        region_id.clone(),
                        faces,
                        Vec::new(),
                        Vec::new(),
                    ) {
                        Ok(shell) => shell,
                        Err(_) => {
                            continue;
                        }
                    },
                );
                ctx.reserve_collection_vec(&mut region_shells, 1, "collect synthetic region shells")?;
                region_shells.push(shell_id);
            }
            admit_brep_entity(ctx)?;
            ctx.reserve_collection_vec(&mut out.regions, 1, "collect synthetic Parasolid regions")?;
            out.regions.push(Region {
                id: region_id.clone(),
                body: body_id.clone(),
                shells: region_shells,
            });
            ctx.reserve_collection_vec(&mut body_regions, 1, "collect synthetic body regions")?;
            body_regions.push(region_id);
        } else {
            for region in native_regions {
                let region_id = RegionId::compose(&region_namespace(), region.attr);
                annotate_group(region_id.as_str(), Some((region.offset, "00_51_region")));
                let mut region_shells = Vec::new();
                for shell in &region.shells {
                    let native_shell_id = ShellId::compose(&shell_namespace(), shell.attr);
                    for (component, faces) in shell_face_components(ctx, &out, native_shell_id.as_str())?
                        .into_iter()
                        .enumerate()
                    {
                        let shell_id = if component == 0 {
                            native_shell_id.clone()
                        } else {
                            shell_component(&native_shell_id, component)
                        };
                        annotate_group(
                            shell_id.as_str(),
                            (component == 0).then_some((shell.offset, "00_51_shell")),
                        );
                        let mut face_ids = HashSet::new();
                        ctx.charge_collection_items(faces.len() as u64, "index native shell faces")?;
                        face_ids.try_reserve(faces.len()).map_err(|_| {
                            ctx.refuse_codec_limit("index native shell faces", u64::MAX - 1, u64::MAX)
                        })?;
                        face_ids.extend(faces.iter().map(cadmpeg_ir::ids::FaceId::as_str));
                        for face in &mut out.faces {
                            if face_ids.contains(face.id.as_str()) {
                                face.shell = shell_id.clone();
                            }
                        }
                        admit_brep_entity(ctx)?;
                        ctx.reserve_collection_vec(&mut out.shells, 1, "collect native Parasolid shells")?;
                        out.shells.push(
                            match Shell::new(
                                shell_id.clone(),
                                region_id.clone(),
                                faces,
                                Vec::new(),
                                Vec::new(),
                            ) {
                                Ok(shell) => shell,
                                Err(_) => {
                                    continue;
                                }
                            },
                        );
                        ctx.reserve_collection_vec(&mut region_shells, 1, "collect native region shells")?;
                        region_shells.push(shell_id);
                    }
                }
                admit_brep_entity(ctx)?;
                ctx.reserve_collection_vec(&mut out.regions, 1, "collect native Parasolid regions")?;
                out.regions.push(Region {
                    id: region_id.clone(),
                    body: body_id.clone(),
                    shells: region_shells,
                });
                ctx.reserve_collection_vec(&mut body_regions, 1, "collect native body regions")?;
                body_regions.push(region_id);
            }
        }
        admit_brep_entity(ctx)?;
        ctx.reserve_collection_vec(&mut out.bodies, 1, "collect Parasolid bodies")?;
        out.bodies.push(Body {
            id: body_id,
            kind: body_record.map_or(BodyKind::Solid, |record| record.kind),
            regions: body_regions,
            transform: None,
            name: None,
            color: None,
            visible: None,
        });
    }

    let mut body_ids_by_attr = HashMap::<u16, Option<&str>>::new();
    for body in &out.bodies {
        let Some(attr) = body
            .id
            .as_str()
            .strip_prefix("sldprt:brep:body#")
            .and_then(|value| value.parse::<u16>().ok())
        else {
            continue;
        };
        reserve_graph_map_key(ctx, &mut body_ids_by_attr, &attr, "index Parasolid body attributes")?;
        match body_ids_by_attr.entry(attr) {
            std::collections::hash_map::Entry::Vacant(entry) => {
                entry.insert(Some(body.id.as_str()));
            }
            std::collections::hash_map::Entry::Occupied(mut entry) => {
                *entry.get_mut() = None;
            }
        }
    }
    for modifier in &mut out.body_modifiers {
        modifier.target = if let Some(Some(id)) = body_ids_by_attr.get(&modifier.body_attr) {
            let mut target = String::new();
            ctx.reserve_retained_string(&mut target, id.len(), "copy Parasolid modifier body ID")?;
            target.push_str(id);
            Some(target)
        } else {
            None
        };
    }

    for curve in &out.curves {
        let Some(attr) = curve
            .id
            .as_str()
            .strip_prefix("sldprt:brep:curve#")
            .and_then(|value| value.parse::<u16>().ok())
        else {
            continue;
        };
        if let Some(indexed) = carriers.curve(attr) {
            let carrier = indexed.carrier();
            annotations
                .note(&curve.id, &source_stream, carrier.offset as u64)
                .tag("compact_curve");
            if matches!(
                curve.geometry,
                CurveGeometry::Solved(SolvedCurveGeometry::Unknown { .. })
            ) {
                annotations.exactness(&curve.id, Exactness::Unknown);
            }
        }
    }
    out.bodies.sort_by(|a, b| a.id.cmp(&b.id));
    out.regions.sort_by(|a, b| a.id.cmp(&b.id));
    out.shells.sort_by(|a, b| a.id.cmp(&b.id));
    out.faces.sort_by(|a, b| a.id.cmp(&b.id));
    out.loops.sort_by(|a, b| a.id.cmp(&b.id));
    out.coedges.sort_by(|a, b| a.id.cmp(&b.id));
    out.edges.sort_by(|a, b| a.id.cmp(&b.id));
    out.vertices.sort_by(|a, b| a.id.cmp(&b.id));
    out.points.sort_by(|a, b| a.id.cmp(&b.id));
    out.surfaces.sort_by(|a, b| a.id.cmp(&b.id));
    out.procedural_surfaces.sort_by(|a, b| a.id.cmp(&b.id));
    out.curves.sort_by(|a, b| a.id.cmp(&b.id));
    out.pcurves.sort_by(|a, b| a.id.cmp(&b.id));
    out.annotations = annotations.build();
    let retained_ids = collect_graph_ids(ctx, out
        .bodies
        .iter()
        .map(|entity| entity.id.as_str())
        .chain(out.regions.iter().map(|entity| entity.id.as_str()))
        .chain(out.shells.iter().map(|entity| entity.id.as_str()))
        .chain(out.faces.iter().map(|entity| entity.id.as_str()))
        .chain(out.loops.iter().map(|entity| entity.id.as_str()))
        .chain(out.coedges.iter().map(|entity| entity.id.as_str()))
        .chain(out.edges.iter().map(|entity| entity.id.as_str()))
        .chain(out.vertices.iter().map(|entity| entity.id.as_str()))
        .chain(out.points.iter().map(|entity| entity.id.as_str()))
        .chain(out.surfaces.iter().map(|entity| entity.id.as_str()))
        .chain(
            out.procedural_surfaces
                .iter()
                .map(|entity| entity.id.as_str()),
        )
        .chain(out.curves.iter().map(|entity| entity.id.as_str()))
        .chain(out.pcurves.iter().map(|entity| entity.id.as_str())),
        "index retained Parasolid entities")?;
    out.annotations
        .provenance
        .retain(|id, _| retained_ids.contains(id.as_str()));
    let mut annotations = AnnotationBuilder::resume(std::mem::take(&mut out.annotations));
    annotations.retain_exactness(|id| retained_ids.contains(id));
    out.annotations = annotations.build();
    Ok(out)
}

fn prune_rejected_topology(
    ctx: &DecodeContext<'_>,
    out: &mut Brep,
) -> Result<(), cadmpeg_core::CodecError> {
    let kept_loops = collect_graph_ids(ctx, out
        .faces
        .iter()
        .flat_map(|face| &face.loops)
        .map(cadmpeg_ir::ids::LoopId::as_str), "track retained Parasolid loops")?;
    out.loops.retain(|loop_| kept_loops.contains(loop_.id.as_str()));

    let kept_coedges = collect_graph_ids(ctx, out
        .loops
        .iter()
        .flat_map(cadmpeg_ir::topology::Loop::coedges)
        .map(cadmpeg_ir::ids::CoedgeId::as_str), "track retained Parasolid coedges")?;
    out.coedges
        .retain(|coedge| kept_coedges.contains(coedge.id.as_str()));
    for coedge in &mut out.coedges {
        if !kept_coedges.contains(coedge.radial_next.as_str()) {
            coedge.radial_next = coedge.id.clone();
        }
    }

    let kept_pcurves = collect_graph_ids(ctx, out
        .coedges
        .iter()
        .flat_map(|coedge| &coedge.pcurves)
        .map(|use_| &use_.pcurve)
        .map(cadmpeg_ir::ids::PcurveId::as_str), "track retained Parasolid pcurves")?;
    out.pcurves
        .retain(|pcurve| kept_pcurves.contains(pcurve.id.as_str()));

    let kept_edges = collect_graph_ids(ctx, out
        .coedges
        .iter()
        .map(|coedge| coedge.edge.as_str()), "track retained Parasolid edges")?;
    out.edges.retain(|edge| kept_edges.contains(edge.id.as_str()));

    let kept_vertices = collect_graph_ids(ctx, out
        .edges
        .iter()
        .flat_map(|edge| [&edge.start, &edge.end])
        .map(cadmpeg_ir::ids::VertexId::as_str), "track retained Parasolid vertices")?;
    out.vertices
        .retain(|vertex| kept_vertices.contains(vertex.id.as_str()));

    let kept_points = collect_graph_ids(ctx, out
        .vertices
        .iter()
        .map(|vertex| vertex.point.as_str()), "track retained Parasolid points")?;
    out.points.retain(|point| kept_points.contains(point.id.as_str()));

    let kept_curves = collect_graph_ids(ctx, out
        .edges
        .iter()
        .filter_map(|edge| edge.curve().map(cadmpeg_ir::ids::CurveId::as_str))
        .chain(out.procedural_surfaces.iter().filter_map(|surface| {
        if let ProceduralSurfaceDefinition::Blend(definition_payload) = surface.definition() {
            definition_payload.spine().as_ref().map(cadmpeg_ir::ids::CurveId::as_str)
        } else {
            None
        }
    })), "track retained Parasolid curves")?;
    out.curves.retain(|curve| kept_curves.contains(curve.id.as_str()));
    out.stats.unknown_curve_edges = out
        .edges
        .iter()
        .filter(|edge| {
            edge.curve().is_some_and(|curve_id| {
                out.curves.iter().any(|curve| {
                    curve.id == *curve_id
                        && matches!(
                            curve.geometry,
                            CurveGeometry::Solved(SolvedCurveGeometry::Unknown { .. })
                        )
                })
            })
        })
        .count();
    Ok(())
}

fn annotate_surface_frame(
    annotations: &mut AnnotationBuilder,
    id: &str,
    mut geometry: &SolvedSurfaceGeometry,
) -> Result<(), cadmpeg_core::CodecError> {
    loop {
        match geometry {
            SolvedSurfaceGeometry::Plane(_) => {
                annotations
                    .derived(id.to_owned(), "geometry.u_axis")
                    .map_err(cadmpeg_core::CodecError::malformed)?;
                break;
            }
            SolvedSurfaceGeometry::Cylinder(_)
            | SolvedSurfaceGeometry::Cone(_)
            | SolvedSurfaceGeometry::Torus(_) => {
                annotations
                    .derived(id.to_owned(), "geometry.ref_direction")
                    .map_err(cadmpeg_core::CodecError::malformed)?;
                break;
            }
            SolvedSurfaceGeometry::Sphere(_) => {
                annotations
                    .derived(id, "geometry.axis")
                    .map_err(cadmpeg_core::CodecError::malformed)?
                    .derived(id.to_owned(), "geometry.ref_direction")
                    .map_err(cadmpeg_core::CodecError::malformed)?;
                break;
            }
            SolvedSurfaceGeometry::Transformed(placed) => geometry = placed.basis(),
            SolvedSurfaceGeometry::Nurbs(_)
            | SolvedSurfaceGeometry::Polygonal(_)
            | SolvedSurfaceGeometry::Unknown { .. } => break,
        }
    }
    Ok(())
}

fn derive_planar_pcurves(
    ctx: &DecodeContext<'_>,
    out: &mut Brep,
    annotations: &mut AnnotationBuilder,
    source_stream: &cadmpeg_ir::annotations::StreamHandle,
) -> Result<(), cadmpeg_core::CodecError> {
    let loop_faces = collect_graph_map(ctx,
        out.loops.iter().map(|lp| (&lp.id, &lp.face)),
        "index Parasolid pcurve loop faces")?;
    let faces = collect_graph_map(ctx, out.faces.iter().map(|face| (&face.id, face)),
        "index Parasolid pcurve faces")?;
    let surfaces = collect_graph_map(ctx,
        out.surfaces.iter().map(|surface| (&surface.id, surface)),
        "index Parasolid pcurve surfaces")?;
    let edges = collect_graph_map(ctx, out.edges.iter().map(|edge| (&edge.id, edge)),
        "index Parasolid pcurve edges")?;
    let curves = collect_graph_map(ctx, out.curves.iter().map(|curve| (&curve.id, curve)),
        "index Parasolid pcurve curves")?;
    let mut derived = Vec::new();
    for coedge in &out.coedges {
        let Some(face_id) = loop_faces.get(&coedge.owner_loop) else {
            continue;
        };
        let Some(face) = faces.get(face_id) else {
            continue;
        };
        let Some(surface) = surfaces.get(&face.surface) else {
            continue;
        };
        if !matches!(
            surface.geometry,
            SurfaceGeometry::Solved(SolvedSurfaceGeometry::Plane(_))
        ) {
            continue;
        }
        let Some(SolvedSurfaceGeometry::Plane(plane_surface)) = surface.geometry.solved() else {
            continue;
        };
        let origin = plane_surface.origin().get();
        let normal = *plane_surface.frame().axis().as_raw();
        let u_reference = *plane_surface.frame().reference().as_raw();
        let v_reference = cadmpeg_ir::math::Vector3::new(
            normal.y * u_reference.z - normal.z * u_reference.y,
            normal.z * u_reference.x - normal.x * u_reference.z,
            normal.x * u_reference.y - normal.y * u_reference.x,
        );
        let Some(edge) = edges.get(&coedge.edge) else {
            continue;
        };
        let Some(curve) = edge.curve().and_then(|id| curves.get(id).copied()) else {
            continue;
        };
        let uv = |point: cadmpeg_ir::math::Point3| {
            let d = [point.x - origin.x, point.y - origin.y, point.z - origin.z];
            cadmpeg_ir::math::Point2::new(
                d[0] * u_reference.x + d[1] * u_reference.y + d[2] * u_reference.z,
                d[0] * v_reference.x + d[1] * v_reference.y + d[2] * v_reference.z,
            )
        };
        let project_direction = |direction: cadmpeg_ir::math::Vector3| {
            let projected = cadmpeg_ir::math::Point2::new(
                direction.x * u_reference.x
                    + direction.y * u_reference.y
                    + direction.z * u_reference.z,
                direction.x * v_reference.x
                    + direction.y * v_reference.y
                    + direction.z * v_reference.z,
            );
            let norm = (projected.u * projected.u + projected.v * projected.v).sqrt();
            (norm > EPS_NORMAL_NONZERO)
                .then(|| cadmpeg_ir::math::Point2::new(projected.u / norm, projected.v / norm))
        };
        let plane_distance = |point: cadmpeg_ir::math::Point3| {
            (point.x - origin.x) * normal.x
                + (point.y - origin.y) * normal.y
                + (point.z - origin.z) * normal.z
        };
        let geometry = match &curve.geometry {
            CurveGeometry::Solved(SolvedCurveGeometry::Line(line_curve)) => {
                let curve_origin = line_curve.origin().get();
                let direction = *line_curve.direction().as_raw();
                if plane_distance(curve_origin).abs() > EPS_PLANAR_DISTANCE
                    || (direction.x * normal.x + direction.y * normal.y + direction.z * normal.z)
                        .abs()
                        > EPS_GEOMETRY_RESIDUAL
                {
                    continue;
                }
                let Some(direction) = project_direction(direction) else {
                    continue;
                };
                PcurveGeometry::Line(
                    match cadmpeg_ir::geometry::pcurve::LinePcurve::try_new(
                        uv(curve_origin),
                        direction,
                    ) {
                        Ok(payload) => payload,
                        Err(_) => continue,
                    },
                )
            }
            CurveGeometry::Solved(SolvedCurveGeometry::Circle(circle_curve)) => {
                let center = circle_curve.center().get();
                let axis = circle_curve.frame().axis().as_raw();
                let ref_direction = circle_curve.frame().reference().as_raw();
                let radius = circle_curve.radius().get();
                let axis_dot = axis.x * normal.x + axis.y * normal.y + axis.z * normal.z;
                if axis_dot.abs() < 1.0 - EPS_AXIS_ALIGNMENT
                    || plane_distance(center).abs() > EPS_PLANAR_DISTANCE
                {
                    continue;
                }
                let Some(ref_direction) = project_direction(*ref_direction) else {
                    continue;
                };
                PcurveGeometry::Circle(
                    match cadmpeg_ir::geometry::pcurve::CirclePcurve::try_new(
                        uv(center),
                        ref_direction,
                        if axis_dot < 0.0 {
                            cadmpeg_ir::math::Point2::new(ref_direction.v, -ref_direction.u)
                        } else {
                            cadmpeg_ir::math::Point2::new(-ref_direction.v, ref_direction.u)
                        },
                        radius,
                    ) {
                        Ok(payload) => payload,
                        Err(_) => continue,
                    },
                )
            }
            CurveGeometry::Solved(SolvedCurveGeometry::Ellipse(ellipse_curve)) => {
                let center = ellipse_curve.center().get();
                let axis = ellipse_curve.frame().axis().as_raw();
                let major_direction = ellipse_curve.frame().reference().as_raw();
                let major_radius = ellipse_curve.major_radius().get();
                let minor_radius = ellipse_curve.minor_radius().get();
                let axis_dot = axis.x * normal.x + axis.y * normal.y + axis.z * normal.z;
                if axis_dot.abs() < 1.0 - EPS_AXIS_ALIGNMENT
                    || plane_distance(center).abs() > EPS_PLANAR_DISTANCE
                {
                    continue;
                }
                let Some(major_direction) = project_direction(*major_direction) else {
                    continue;
                };
                PcurveGeometry::Ellipse(match cadmpeg_ir::geometry::pcurve::EllipsePcurve::try_new(
                    uv(center),
                    major_direction,
                    if axis_dot < 0.0 {
                        cadmpeg_ir::math::Point2::new(major_direction.v, -major_direction.u)
                    } else {
                        cadmpeg_ir::math::Point2::new(-major_direction.v, major_direction.u)
                    },
                    major_radius,
                    minor_radius,
                ) {
                    Ok(payload) => payload,
                    Err(_) => continue,
                })
            }
            _ => continue,
        };
        let id = PcurveId::compose(&pcurve_namespace(), coedge.id.key());
        let pcurve = Pcurve {
            id: id.clone(),
            geometry,
            metadata: cadmpeg_ir::geometry::pcurve::PcurveMetadata::default(),
        };
        ctx.reserve_collection_vec(&mut derived, 1, "collect derived Parasolid pcurves")?;
        derived.push((coedge.id.clone(), id, pcurve));
    }
    let coedge_indices = collect_graph_map(ctx,
        out.coedges.iter().enumerate().map(|(index, coedge)| (coedge.id.clone(), index)),
        "index Parasolid derived coedges")?;
    for (coedge_id, id, pcurve) in derived {
        if let Some(index) = coedge_indices.get(&coedge_id) {
            let mut uses = Vec::new();
            ctx.reserve_collection_vec(&mut uses, 1, "bind derived Parasolid pcurve")?;
            uses.push(cadmpeg_ir::topology::PcurveUse {
                pcurve: id.clone(),
                isoparametric: None,
                parameter_range: None,
            });
            out.coedges[*index].pcurves = uses;
        }
        annotations
            .note(&id, source_stream, 0)
            .tag("derived_planar_pcurve");
        annotations.exactness(&id, Exactness::Derived);
        admit_brep_entity(ctx)?;
        ctx.reserve_collection_vec(&mut out.pcurves, 1, "collect derived Parasolid pcurves")?;
        out.pcurves.push(pcurve);
    }
    Ok(())
}

fn derive_cylindrical_pcurves(
    ctx: &DecodeContext<'_>,
    out: &mut Brep,
    annotations: &mut AnnotationBuilder,
    source_stream: &cadmpeg_ir::annotations::StreamHandle,
) -> Result<(), cadmpeg_core::CodecError> {
    let mut refusals = Vec::new();
    let loop_faces = collect_graph_map(ctx,
        out.loops.iter().map(|lp| (&lp.id, &lp.face)),
        "index Parasolid pcurve loop faces")?;
    let faces = collect_graph_map(ctx, out.faces.iter().map(|face| (&face.id, face)),
        "index Parasolid pcurve faces")?;
    let surfaces = collect_graph_map(ctx,
        out.surfaces.iter().map(|surface| (&surface.id, surface)),
        "index Parasolid pcurve surfaces")?;
    let edges = collect_graph_map(ctx, out.edges.iter().map(|edge| (&edge.id, edge)),
        "index Parasolid pcurve edges")?;
    let curves = collect_graph_map(ctx, out.curves.iter().map(|curve| (&curve.id, curve)),
        "index Parasolid pcurve curves")?;
    let points = collect_graph_map(ctx, out.points.iter().map(|point| (&point.id, point)),
        "index Parasolid pcurve points")?;
    let vertex_points = collect_graph_map(ctx,
        out.vertices.iter().filter_map(|vertex| points.get(&vertex.point).map(|point| (&vertex.id, *point))),
        "index Parasolid pcurve vertex points")?;
    let position = |vertex_id: &VertexId| {
        vertex_points
            .get(vertex_id)
            .map(|point| point.position().get())
    };
    let mut derived = Vec::new();
    for coedge in &out.coedges {
        if !coedge.pcurves.is_empty() {
            continue;
        }
        let Some(face_id) = loop_faces.get(&coedge.owner_loop) else {
            continue;
        };
        let Some(face) = faces.get(face_id) else {
            continue;
        };
        let Some(surface) = surfaces.get(&face.surface) else {
            continue;
        };
        let Some(SolvedSurfaceGeometry::Cylinder(cylinder_surface)) = surface.geometry.solved()
        else {
            continue;
        };
        let origin = cylinder_surface.origin().get();
        let axis = cylinder_surface.frame().axis().as_raw();
        let u_reference = cylinder_surface.frame().reference().as_raw();
        let radius = cylinder_surface.radius().get();
        let Some(edge) = edges.get(&coedge.edge) else {
            continue;
        };
        let Some(curve) = edge.curve().and_then(|id| curves.get(id).copied()) else {
            continue;
        };
        let cross = cadmpeg_ir::math::Vector3::new(
            axis.y * u_reference.z - axis.z * u_reference.y,
            axis.z * u_reference.x - axis.x * u_reference.z,
            axis.x * u_reference.y - axis.y * u_reference.x,
        );
        let dot = |a: [f64; 3], b: cadmpeg_ir::math::Vector3| a[0] * b.x + a[1] * b.y + a[2] * b.z;
        let mut parameter_range = None;
        let geometry = match &curve.geometry {
            CurveGeometry::Solved(SolvedCurveGeometry::Circle(circle_curve))
                if {
                    let circle_axis = circle_curve.frame().axis().as_raw();
                    let circle_radius = circle_curve.radius().get();
                    (circle_radius.abs() - radius.abs()).abs() < EPS_CIRCLE_RADIUS_MATCH
                        && (circle_axis.x * axis.x
                            + circle_axis.y * axis.y
                            + circle_axis.z * axis.z)
                            .abs()
                            > 1.0 - EPS_AXIS_ALIGNMENT
                } =>
            {
                let center = circle_curve.center().get();
                let circle_axis = circle_curve.frame().axis().as_raw();
                let circle_reference = circle_curve.frame().reference().as_raw();
                let d = [
                    center.x - origin.x,
                    center.y - origin.y,
                    center.z - origin.z,
                ];
                let axial = dot(d, *axis);
                let radial = [
                    d[0] - axial * axis.x,
                    d[1] - axial * axis.y,
                    d[2] - axial * axis.z,
                ];
                if dot(
                    radial,
                    cadmpeg_ir::math::Vector3::new(radial[0], radial[1], radial[2]),
                )
                .sqrt()
                    > EPS_PLANAR_DISTANCE
                {
                    continue;
                }
                let Some((phase, sense)) =
                    circle_azimuth_parameter(*axis, *u_reference, *circle_axis, *circle_reference)
                else {
                    continue;
                };
                PcurveGeometry::Line(
                    match cadmpeg_ir::geometry::pcurve::LinePcurve::try_new(
                        cadmpeg_ir::math::Point2::new(phase, axial),
                        cadmpeg_ir::math::Point2::new(sense, 0.0),
                    ) {
                        Ok(payload) => payload,
                        Err(_) => continue,
                    },
                )
            }
            CurveGeometry::Solved(SolvedCurveGeometry::Line(line_curve))
                if {
                    let direction = *line_curve.direction().as_raw();
                    (direction.x * axis.x + direction.y * axis.y + direction.z * axis.z).abs()
                        > 1.0 - EPS_AXIS_ALIGNMENT
                } =>
            {
                let direction = *line_curve.direction().as_raw();
                let Some(start) = position(&edge.start) else {
                    continue;
                };
                let d = [start.x - origin.x, start.y - origin.y, start.z - origin.z];
                let v = dot(d, *axis);
                let radial = [d[0] - v * axis.x, d[1] - v * axis.y, d[2] - v * axis.z];
                let u = dot(radial, cross).atan2(dot(radial, *u_reference));
                PcurveGeometry::Line(
                    match cadmpeg_ir::geometry::pcurve::LinePcurve::try_new(
                        cadmpeg_ir::math::Point2::new(u, v),
                        cadmpeg_ir::math::Point2::new(
                            0.0,
                            if dot([direction.x, direction.y, direction.z], *axis) >= 0.0 {
                                1.0
                            } else {
                                -1.0
                            },
                        ),
                    ) {
                        Ok(payload) => payload,
                        Err(_) => continue,
                    },
                )
            }
            CurveGeometry::Solved(SolvedCurveGeometry::Ellipse(ellipse_curve)) => {
                let center = ellipse_curve.center().get();
                let ellipse_axis = ellipse_curve.frame().axis().as_raw();
                let major_direction = ellipse_curve.frame().reference().as_raw();
                let major_radius = ellipse_curve.major_radius().get();
                let minor_radius = ellipse_curve.minor_radius().get();
                let minor_direction = cadmpeg_ir::math::Vector3::new(
                    ellipse_axis.y * major_direction.z - ellipse_axis.z * major_direction.y,
                    ellipse_axis.z * major_direction.x - ellipse_axis.x * major_direction.z,
                    ellipse_axis.x * major_direction.y - ellipse_axis.y * major_direction.x,
                );
                let relative = [
                    center.x - origin.x,
                    center.y - origin.y,
                    center.z - origin.z,
                ];
                let major = [
                    major_radius * major_direction.x,
                    major_radius * major_direction.y,
                    major_radius * major_direction.z,
                ];
                let minor = [
                    minor_radius * minor_direction.x,
                    minor_radius * minor_direction.y,
                    minor_radius * minor_direction.z,
                ];
                let radial_center = cadmpeg_ir::math::Point2::new(
                    dot(relative, *u_reference),
                    dot(relative, cross),
                );
                let radial_cos =
                    cadmpeg_ir::math::Point2::new(dot(major, *u_reference), dot(major, cross));
                let radial_sin =
                    cadmpeg_ir::math::Point2::new(dot(minor, *u_reference), dot(minor, cross));
                let norm = |value: cadmpeg_ir::math::Point2| value.u.hypot(value.v);
                let product = |a: cadmpeg_ir::math::Point2, b: cadmpeg_ir::math::Point2| {
                    a.u * b.u + a.v * b.v
                };
                let tolerance = EPS_RADIUS_ABSOLUTE.max(radius.abs() * EPS_RADIUS_RELATIVE);
                if norm(radial_center) > tolerance
                    || (norm(radial_cos) - radius.abs()).abs() > tolerance
                    || (norm(radial_sin) - radius.abs()).abs() > tolerance
                    || product(radial_cos, radial_sin).abs() > tolerance * radius.abs()
                {
                    continue;
                }
                PcurveGeometry::PolarHarmonic(
                    match cadmpeg_ir::geometry::pcurve::PolarHarmonicPcurve::try_new(
                        radial_center,
                        radial_cos,
                        radial_sin,
                        dot(relative, *axis),
                        dot(major, *axis),
                        dot(minor, *axis),
                    ) {
                        Ok(payload) => payload,
                        Err(_) => continue,
                    },
                )
            }
            CurveGeometry::Solved(SolvedCurveGeometry::Nurbs(nurbs)) => {
                let project = |point: cadmpeg_ir::math::Point3| {
                    let relative = [point.x - origin.x, point.y - origin.y, point.z - origin.z];
                    cadmpeg_ir::math::Point2::new(
                        dot(relative, *u_reference),
                        dot(relative, cross),
                    )
                };
                ctx.charge_work(nurbs.pole_count() as u64, "project Parasolid cylinder poles")?;
                let mut radial_control_points = Vec::new();
                ctx.reserve_collection_vec(&mut radial_control_points, nurbs.pole_count(), "collect Parasolid cylinder radial poles")?;
                let curve_weights = match nurbs.pole_rows() {
                    cadmpeg_ir::geometry::nurbs::NurbsPoles3::Polynomial { points } => {
                        radial_control_points.extend(points.iter().map(|point| project(point.get())));
                        None
                    }
                    cadmpeg_ir::geometry::nurbs::NurbsPoles3::Rational { points } => {
                        radial_control_points.extend(points.iter().map(|pole| project(pole.point.get())));
                        let mut weights = Vec::new();
                        ctx.reserve_collection_vec(&mut weights, points.len(), "collect Parasolid cylinder pole weights")?;
                        weights.extend(points.iter().map(|pole| pole.weight.get()));
                        Some(weights)
                    }
                };
                if !quadratic_nurbs_has_constant_radius(
                    &radial_control_points,
                    curve_weights.as_deref(),
                    nurbs.knots(),
                    cylinder_surface.radius(),
                ) {
                    continue;
                }
                parameter_range = if let Some(range) = edge.param_range() {
                    Some(range.get())
                } else {
                    let (Some(start), Some(end)) = (position(&edge.start), position(&edge.end))
                    else {
                        continue;
                    };
                    match (
                        nurbs_parameter_at_point(ctx, nurbs, start)?,
                        nurbs_parameter_at_point(ctx, nurbs, end)?,
                    ) {
                        (InverseResolution::Unique(start), InverseResolution::Unique(end)) => {
                            Some([start.min(end), start.max(end)])
                        }
                        (InverseResolution::Ambiguous, _) | (_, InverseResolution::Ambiguous) => {
                            out.stats.ambiguous_pcurve_parameters += 1;
                            continue;
                        }
                        _ => continue,
                    }
                };
                // One pole row carries its radial and axial halves together,
                // so the two projections are built into one list.
                let poles = nurbs
                    .control_points()
                    .iter()
                    .zip(&radial_control_points)
                    .map(
                        |(point, radial)| cadmpeg_ir::geometry::pcurve::PolarNurbsPole {
                            radial: *radial,
                            axial: dot(
                                [point.x - origin.x, point.y - origin.y, point.z - origin.z],
                                *axis,
                            ),
                        },
                    )
                    .collect::<Vec<_>>();
                let polar = match PolarPcurveNurbs::from_checked_lanes(
                    nurbs.degree(),
                    nurbs.knots().clone(),
                    poles,
                    nurbs.weights(),
                    nurbs.periodic(),
                ) {
                    Ok(polar) => polar,
                    Err(error) => {
                        let note = crate::loss::spline_lane_refusal(
                            ctx, format_args!("cylindrical pcurve for edge {}: {error}", edge.id),
                        )?;
                        ctx.reserve_collection_vec(&mut refusals, 1, "collect cylindrical pcurve losses")?;
                        refusals.push(note);
                        continue;
                    }
                };
                PcurveGeometry::PolarNurbs { nurbs: polar }
            }
            _ => continue,
        };
        let id = PcurveId::compose(
            &pcurve_namespace(),
            cadmpeg_ir::identity_key!("cylinder:").then(coedge.id.key()),
        );
        let parameter_range = match parameter_range.map(cadmpeg_ir::units::FiniteVector::new) {
            Some(None) => continue,
            Some(Some(range)) => Some(range),
            None => None,
        };
        ctx.reserve_collection_vec(&mut derived, 1, "collect derived Parasolid pcurves")?;
        derived.push((
            coedge.id.clone(),
            id.clone(),
            Pcurve {
                id,
                geometry,
                metadata: cadmpeg_ir::geometry::pcurve::PcurveMetadata::general(
                    None,
                    parameter_range,
                    None,
                ),
            },
        ));
    }
    let coedge_indices = collect_graph_map(ctx,
        out.coedges.iter().enumerate().map(|(index, coedge)| (coedge.id.clone(), index)),
        "index Parasolid derived coedges")?;
    for (coedge_id, id, pcurve) in derived {
        if let Some(index) = coedge_indices.get(&coedge_id) {
            let mut uses = Vec::new();
            ctx.reserve_collection_vec(&mut uses, 1, "bind derived Parasolid pcurve")?;
            uses.push(cadmpeg_ir::topology::PcurveUse {
                pcurve: id.clone(),
                isoparametric: None,
                parameter_range: None,
            });
            out.coedges[*index].pcurves = uses;
        }
        annotations
            .note(&id, source_stream, 0)
            .tag("derived_cylindrical_pcurve");
        annotations.exactness(&id, Exactness::Derived);
        admit_brep_entity(ctx)?;
        ctx.reserve_collection_vec(&mut out.pcurves, 1, "collect derived Parasolid pcurves")?;
        out.pcurves.push(pcurve);
    }
    ctx.reserve_precharged_vec(&mut out.losses, refusals.len(), "move cylindrical pcurve losses")?;
    out.losses.extend(refusals);
    Ok(())
}

enum InverseResolution<T> {
    NoMatch,
    Unique(T),
    Ambiguous,
}

const INVERSE_SAMPLE_COUNT: usize = 32;
const INVERSE_PARAMETER_TOLERANCE: f64 = 1.0e-10;
const INVERSE_ABSOLUTE_TOLERANCE_MM: f64 = 1.0e-6;
const INVERSE_RELATIVE_TOLERANCE: f64 = 8.0 * f64::EPSILON;
const NURBS_POLE_ROUNDOFF_FACTOR: f64 = 256.0 * f64::EPSILON;
const NURBS_CACHE_SAMPLES_PER_SPAN: usize = 8;
const NURBS_ENDPOINT_TOLERANCE_MM: f64 = 1.0e-6;

/// The looser of two acceptance tolerances.
///
/// Two independently derived bounds gate the same acceptance, and the
/// acceptance holds when either of them does, so the looser bound is the gate.
/// This is not a floor under a value: both arguments are tolerances.
fn looser_tolerance(left: f64, right: f64) -> f64 {
    if left >= right {
        left
    } else {
        right
    }
}

/// The largest absolute coordinate the point set states, or `None` when it
/// states none.
///
/// This is the model's own coordinate scale, not a bound on it: a
/// sub-millimetre model states a sub-millimetre scale, and an empty point set
/// states no scale at all.
fn coordinate_scale(points: impl IntoIterator<Item = cadmpeg_ir::math::Point3>) -> Option<f64> {
    points
        .into_iter()
        .flat_map(|point| [point.x.abs(), point.y.abs(), point.z.abs()])
        .filter(|magnitude| magnitude.is_finite())
        .fold(None, |largest, magnitude| match largest {
            Some(known) if known >= magnitude => Some(known),
            _ => Some(magnitude),
        })
}

fn inverse_coordinate_tolerance(points: impl IntoIterator<Item = cadmpeg_ir::math::Point3>) -> f64 {
    // The absolute inverse-projection tolerance against the coordinate-scaled
    // relative one. A point set that states no coordinate scale states only the
    // absolute bound.
    match coordinate_scale(points) {
        Some(scale) => looser_tolerance(
            INVERSE_ABSOLUTE_TOLERANCE_MM,
            scale * INVERSE_RELATIVE_TOLERANCE,
        ),
        None => INVERSE_ABSOLUTE_TOLERANCE_MM,
    }
}

fn golden_section_minimum<F>(
    mut left: f64,
    mut right: f64,
    objective: &mut F,
) -> Result<Option<(f64, f64)>, cadmpeg_core::decode::ResourceLimit>
where
    F: FnMut(f64) -> Result<Option<f64>, cadmpeg_core::decode::ResourceLimit>,
{
    let ratio = (5.0_f64.sqrt() - 1.0) / 2.0;
    let Some(a) = cadmpeg_ir::math::interpolate(left, right, 1.0 - ratio) else {
        return Ok(None);
    };
    let Some(b) = cadmpeg_ir::math::interpolate(left, right, ratio) else {
        return Ok(None);
    };
    let mut a = a.get();
    let mut b = b.get();
    let Some(mut da) = objective(a)? else {
        return Ok(None);
    };
    let Some(mut db) = objective(b)? else {
        return Ok(None);
    };
    for _ in 0..80 {
        if da <= db {
            right = b;
            b = a;
            db = da;
            let Some(next) = cadmpeg_ir::math::interpolate(left, right, 1.0 - ratio) else {
                return Ok(None);
            };
            a = next.get();
            let Some(next) = objective(a)? else {
                return Ok(None);
            };
            da = next;
        } else {
            left = a;
            a = b;
            da = db;
            let Some(next) = cadmpeg_ir::math::interpolate(left, right, ratio) else {
                return Ok(None);
            };
            b = next.get();
            let Some(next) = objective(b)? else {
                return Ok(None);
            };
            db = next;
        }
    }
    let Some(parameter) = cadmpeg_ir::math::interpolate(left, right, 0.5) else {
        return Ok(None);
    };
    let parameter = parameter.get();
    Ok(objective(parameter)?.map(|distance| (parameter, distance)))
}

/// Find one representative for every sampled local minimum of an objective on
/// each nonzero knot span. Endpoints are always retained, so a candidate at a
/// knot is not lost when adjacent spans share it.
fn sampled_parameter_minima<F>(
    ctx: &DecodeContext<'_>,
    knots: &[f64],
    domain: [f64; 2],
    mut objective: F,
) -> Result<Option<Vec<(f64, f64)>>, cadmpeg_core::CodecError>
where
    F: FnMut(f64) -> Result<Option<f64>, cadmpeg_core::decode::ResourceLimit>,
{
    let mut candidates = Vec::new();
    for span in knots.windows(2).filter(|span| span[0] < span[1]) {
        ctx.charge_work(1, "sample Parasolid inverse knot spans")?;
        let start = span[0].max(domain[0]);
        let end = span[1].min(domain[1]);
        if start >= end {
            continue;
        }
        let mut samples = Vec::new();
        ctx.reserve_collection_vec(&mut samples, INVERSE_SAMPLE_COUNT + 1, "sample Parasolid inverse span")?;
        for index in 0..=INVERSE_SAMPLE_COUNT {
            ctx.charge_work(1, "sample Parasolid inverse span")?;
            let Some(parameter) = cadmpeg_ir::math::interpolate(
                start,
                end,
                index as f64 / INVERSE_SAMPLE_COUNT as f64,
            ) else {
                return Ok(None);
            };
            let parameter = parameter.get();
            let Some(distance) = objective(parameter)? else {
                return Ok(None);
            };
            samples.push((parameter, distance));
        }
        ctx.reserve_collection_vec(&mut candidates, samples.len(), "collect Parasolid inverse candidates")?;
        candidates.extend(samples.iter().copied());
        for index in 1..INVERSE_SAMPLE_COUNT {
            if samples[index].1 <= samples[index - 1].1 && samples[index].1 <= samples[index + 1].1
            {
                let Some(minimum) = golden_section_minimum(
                    samples[index - 1].0,
                    samples[index + 1].0,
                    &mut objective,
                )? else {
                    return Ok(None);
                };
                ctx.reserve_collection_vec(&mut candidates, 1, "collect Parasolid inverse minima")?;
                candidates.push(minimum);
            }
        }
    }
    Ok(Some(candidates))
}

fn unique_inverse_parameter(
    mut candidates: Vec<(f64, f64)>,
    tolerance: f64,
    parameter_domain: [f64; 2],
) -> InverseResolution<f64> {
    let tolerance_squared = tolerance * tolerance;
    candidates.retain(|(parameter, error)| {
        parameter.is_finite() && error.is_finite() && *error <= tolerance_squared
    });
    candidates.sort_by(|left, right| left.0.total_cmp(&right.0));
    let parameter_tolerance = (INVERSE_PARAMETER_TOLERANCE * parameter_domain[1]
        - INVERSE_PARAMETER_TOLERANCE * parameter_domain[0])
        .abs();
    let mut unique_len = 0;
    for index in 0..candidates.len() {
        let candidate = candidates[index];
        if unique_len > 0
            && (candidate.0 - candidates[unique_len - 1].0).abs() <= parameter_tolerance
        {
            if candidate.1 < candidates[unique_len - 1].1 {
                candidates[unique_len - 1] = candidate;
            }
        } else {
            candidates[unique_len] = candidate;
            unique_len += 1;
        }
    }
    candidates.truncate(unique_len);
    match candidates.as_slice() {
        [] => InverseResolution::NoMatch,
        [(parameter, _)] => InverseResolution::Unique(*parameter),
        _ => InverseResolution::Ambiguous,
    }
}

fn nurbs_parameter_at_point(
    ctx: &DecodeContext<'_>,
    nurbs: &cadmpeg_ir::geometry::nurbs::NurbsCurve,
    target: cadmpeg_ir::math::Point3,
) -> Result<InverseResolution<f64>, cadmpeg_core::CodecError> {
    let squared_distance = |parameter: f64| {
        let Some(point) =
            cadmpeg_ir::eval::finite_or_refusal(nurbs_curve_point_at(nurbs, parameter))?
        else {
            return Ok(None);
        };
        Ok(Some(
            (point.x - target.x).powi(2)
                + (point.y - target.y).powi(2)
                + (point.z - target.z).powi(2),
        ))
    };
    let Some(domain) = nurbs_curve_parameter_domain(nurbs)
        .map(cadmpeg_ir::topology::IncreasingParameterInterval::endpoints)
    else {
        return Ok(InverseResolution::NoMatch);
    };
    let Some(candidates) = sampled_parameter_minima(ctx, nurbs.knots(), domain, squared_distance)? else {
        return Ok(InverseResolution::NoMatch);
    };
    let tolerance = match nurbs.pole_rows() {
        cadmpeg_ir::geometry::nurbs::NurbsPoles3::Polynomial { points } => {
            inverse_coordinate_tolerance(points.iter().copied().map(FinitePoint3::get).chain(std::iter::once(target)))
        }
        cadmpeg_ir::geometry::nurbs::NurbsPoles3::Rational { points } => {
            inverse_coordinate_tolerance(points.iter().map(|pole| pole.point.get()).chain(std::iter::once(target)))
        }
    };
    Ok(unique_inverse_parameter(
        candidates,
        tolerance,
        domain,
    ))
}

fn quadratic_nurbs_has_constant_radius(
    radial_control_points: &[cadmpeg_ir::math::Point2],
    weights: Option<&[f64]>,
    knots: &[f64],
    radius: cadmpeg_ir::scalar::PositiveLength,
) -> bool {
    if radial_control_points.len() < 3
        || radial_control_points.len().is_multiple_of(2)
        || knots.len() != radial_control_points.len() + 3
        || weights.is_some_and(|weights| weights.len() != radial_control_points.len())
    {
        return false;
    }
    let radius = radius.get();
    let mut runs = 0usize;
    let mut previous = 0.0;
    let mut count = 0usize;
    for knot in knots {
        if !knot.is_finite() {
            return false;
        }
        if runs == 0 {
            previous = *knot;
            count = 1;
            runs = 1;
        } else if *knot == previous {
            count += 1;
        } else {
            if *knot <= previous || count != if runs == 1 { 3 } else { 2 } {
                return false;
            }
            previous = *knot;
            count = 1;
            runs += 1;
        }
    }
    if runs < 2
        || count != 3
        || runs - 1 != (radial_control_points.len() - 1) / 2
    {
        return false;
    }
    let weight = |index: usize| weights.map_or(1.0, |weights| weights[index]);
    let choose_2 = [1.0, 2.0, 1.0];
    let choose_4 = [1.0, 4.0, 6.0, 4.0, 1.0];
    let tolerance = EPS_RADIUS_ABSOLUTE.max(radius * radius * EPS_RADIUS_RELATIVE);
    for start in (0..radial_control_points.len() - 1).step_by(2) {
        let homogeneous = [0, 1, 2].map(|offset| {
                let weight = weight(start + offset);
                let point = radial_control_points[start + offset];
                (point.u * weight, point.v * weight, weight)
            });
        for (degree, &denominator) in choose_4.iter().enumerate() {
            let mut identity = 0.0_f64;
            for i in 0usize..=2 {
                let Some(j) = degree.checked_sub(i) else {
                    continue;
                };
                if j > 2 {
                    continue;
                }
                let factor = choose_2[i] * choose_2[j] / denominator;
                identity += factor
                    * (homogeneous[i].0 * homogeneous[j].0 + homogeneous[i].1 * homogeneous[j].1
                        - radius * radius * homogeneous[i].2 * homogeneous[j].2);
            }
            if identity.abs() > tolerance {
                return false;
            }
        }
    }
    true
}

fn circle_azimuth_parameter(
    surface_axis: cadmpeg_ir::math::Vector3,
    surface_reference: cadmpeg_ir::math::Vector3,
    circle_axis: cadmpeg_ir::math::Vector3,
    circle_reference: cadmpeg_ir::math::Vector3,
) -> Option<(f64, f64)> {
    let axis_dot = surface_axis.x * circle_axis.x
        + surface_axis.y * circle_axis.y
        + surface_axis.z * circle_axis.z;
    if axis_dot.abs() < 1.0 - EPS_AXIS_ALIGNMENT {
        return None;
    }
    let surface_tangent = cadmpeg_ir::math::Vector3::new(
        surface_axis.y * surface_reference.z - surface_axis.z * surface_reference.y,
        surface_axis.z * surface_reference.x - surface_axis.x * surface_reference.z,
        surface_axis.x * surface_reference.y - surface_axis.y * surface_reference.x,
    );
    let phase = (circle_reference.x * surface_tangent.x
        + circle_reference.y * surface_tangent.y
        + circle_reference.z * surface_tangent.z)
        .atan2(
            circle_reference.x * surface_reference.x
                + circle_reference.y * surface_reference.y
                + circle_reference.z * surface_reference.z,
        );
    Some((phase, axis_dot.signum()))
}

fn derive_revolved_circle_pcurves(
    ctx: &DecodeContext<'_>,
    out: &mut Brep,
    annotations: &mut AnnotationBuilder,
    source_stream: &cadmpeg_ir::annotations::StreamHandle,
) -> Result<(), cadmpeg_core::CodecError> {
    let loop_faces = collect_graph_map(ctx,
        out.loops.iter().map(|lp| (&lp.id, &lp.face)),
        "index Parasolid pcurve loop faces")?;
    let faces = collect_graph_map(ctx, out.faces.iter().map(|face| (&face.id, face)),
        "index Parasolid pcurve faces")?;
    let surfaces = collect_graph_map(ctx,
        out.surfaces.iter().map(|surface| (&surface.id, surface)),
        "index Parasolid pcurve surfaces")?;
    let edges = collect_graph_map(ctx, out.edges.iter().map(|edge| (&edge.id, edge)),
        "index Parasolid pcurve edges")?;
    let curves = collect_graph_map(ctx, out.curves.iter().map(|curve| (&curve.id, curve)),
        "index Parasolid pcurve curves")?;
    let dot = |a: [f64; 3], b: cadmpeg_ir::math::Vector3| a[0] * b.x + a[1] * b.y + a[2] * b.z;
    let mut derived = Vec::new();
    for coedge in &out.coedges {
        if !coedge.pcurves.is_empty() {
            continue;
        }
        let Some(surface) = loop_faces
            .get(&coedge.owner_loop)
            .and_then(|face_id| faces.get(face_id))
            .and_then(|face| surfaces.get(&face.surface))
        else {
            continue;
        };
        let Some(CurveGeometry::Solved(SolvedCurveGeometry::Circle(circle_curve))) = edges
            .get(&coedge.edge)
            .and_then(|edge| edge.curve())
            .and_then(|curve_id| curves.get(curve_id))
            .map(|curve| &curve.geometry)
        else {
            continue;
        };
        let circle_center = circle_curve.center().get();
        let circle_axis = circle_curve.frame().axis().as_raw();
        let circle_reference = circle_curve.frame().reference().as_raw();
        let circle_radius = circle_curve.radius().get();
        let (surface_axis, surface_reference, v) = match &surface.geometry {
            SurfaceGeometry::Solved(SolvedSurfaceGeometry::Cone(cone_surface))
                if {
                    let ratio = cone_surface.ratio().get();
                    (ratio - 1.0).abs() < EPS_NORMAL_NONZERO
                } =>
            {
                let origin = cone_surface.origin().get();
                let axis = cone_surface.frame().axis().as_raw();
                let ref_direction = cone_surface.frame().reference().as_raw();
                let radius = cone_surface.radius().get();
                let half_angle = cone_surface.half_angle().get();
                let d = [
                    circle_center.x - origin.x,
                    circle_center.y - origin.y,
                    circle_center.z - origin.z,
                ];
                let v = dot(d, *axis);
                let radial = [d[0] - v * axis.x, d[1] - v * axis.y, d[2] - v * axis.z];
                let expected_radius = radius + v * half_angle.tan();
                if dot(
                    radial,
                    cadmpeg_ir::math::Vector3::new(radial[0], radial[1], radial[2]),
                )
                .sqrt()
                    > EPS_PLANAR_DISTANCE
                    || (circle_radius.abs() - expected_radius.abs()).abs() > EPS_CIRCLE_RADIUS_MATCH
                {
                    continue;
                }
                (*axis, *ref_direction, v)
            }
            SurfaceGeometry::Solved(SolvedSurfaceGeometry::Torus(torus_surface)) => {
                let center = torus_surface.center().get();
                let axis = torus_surface.frame().axis().as_raw();
                let ref_direction = torus_surface.frame().reference().as_raw();
                let major_radius = torus_surface.major_radius().get();
                let minor_radius = torus_surface.minor_radius().get();
                let d = [
                    circle_center.x - center.x,
                    circle_center.y - center.y,
                    circle_center.z - center.z,
                ];
                let height = dot(d, *axis);
                let radial = [
                    d[0] - height * axis.x,
                    d[1] - height * axis.y,
                    d[2] - height * axis.z,
                ];
                if dot(
                    radial,
                    cadmpeg_ir::math::Vector3::new(radial[0], radial[1], radial[2]),
                )
                .sqrt()
                    > EPS_PLANAR_DISTANCE
                    || ((circle_radius.abs() - major_radius).hypot(height) - minor_radius.abs())
                        .abs()
                        > EPS_RADIUS_ABSOLUTE.max(minor_radius.abs() * EPS_RADIUS_RELATIVE)
                {
                    continue;
                }
                (
                    *axis,
                    *ref_direction,
                    height.atan2(circle_radius.abs() - major_radius),
                )
            }
            _ => continue,
        };
        let Some((phase, sense)) = circle_azimuth_parameter(
            surface_axis,
            surface_reference,
            *circle_axis,
            *circle_reference,
        ) else {
            continue;
        };
        let id = PcurveId::compose(
            &pcurve_namespace(),
            cadmpeg_ir::identity_key!("revolved-circle:").then(coedge.id.key()),
        );
        ctx.reserve_collection_vec(&mut derived, 1, "collect derived Parasolid pcurves")?;
        derived.push((
            coedge.id.clone(),
            id.clone(),
            Pcurve {
                id,
                geometry: PcurveGeometry::Line(
                    match cadmpeg_ir::geometry::pcurve::LinePcurve::try_new(
                        cadmpeg_ir::math::Point2::new(phase, v),
                        cadmpeg_ir::math::Point2::new(sense, 0.0),
                    ) {
                        Ok(payload) => payload,
                        Err(_) => continue,
                    },
                ),
                metadata: cadmpeg_ir::geometry::pcurve::PcurveMetadata::default(),
            },
        ));
    }
    let coedge_indices = collect_graph_map(ctx,
        out.coedges.iter().enumerate().map(|(index, coedge)| (coedge.id.clone(), index)),
        "index Parasolid derived coedges")?;
    for (coedge_id, id, pcurve) in derived {
        if let Some(index) = coedge_indices.get(&coedge_id) {
            let mut uses = Vec::new();
            ctx.reserve_collection_vec(&mut uses, 1, "bind derived Parasolid pcurve")?;
            uses.push(cadmpeg_ir::topology::PcurveUse {
                pcurve: id.clone(),
                isoparametric: None,
                parameter_range: None,
            });
            out.coedges[*index].pcurves = uses;
        }
        annotations
            .note(&id, source_stream, 0)
            .tag("derived_revolved_circle_pcurve");
        annotations.exactness(&id, Exactness::Derived);
        admit_brep_entity(ctx)?;
        ctx.reserve_collection_vec(&mut out.pcurves, 1, "collect derived Parasolid pcurves")?;
        out.pcurves.push(pcurve);
    }
    Ok(())
}

/// Answer the sphere latitude of a circle whose plane is normal to the sphere
/// axis, or refuse a plane the sphere does not carry.
///
/// `height` is the signed axial distance from the sphere center to the circle
/// plane, so a circle on the sphere has `height.abs() <= radius.abs()` and the
/// latitude is `asin(height / radius)`. The radius match that precedes this
/// compares the circle radius against `sqrt(radius^2 - height^2)`, which
/// saturates at zero, so a small circle passes that match at any height beyond
/// the pole. The height is therefore stated here against the same distance
/// tolerance, and only the excess that tolerance admits is mapped onto the
/// pole. A larger height states a circle on another sphere.
fn sphere_latitude(height: f64, radius: f64) -> Option<f64> {
    let pole = radius.abs() + EPS_CIRCLE_RADIUS_MATCH;
    if !(-pole..=pole).contains(&height) {
        return None;
    }
    let sine = height / radius;
    if sine < -1.0 {
        return Some(-std::f64::consts::FRAC_PI_2);
    }
    if sine > 1.0 {
        return Some(std::f64::consts::FRAC_PI_2);
    }
    Some(sine.asin())
}

fn derive_spherical_pcurves(
    ctx: &DecodeContext<'_>,
    out: &mut Brep,
    annotations: &mut AnnotationBuilder,
    source_stream: &cadmpeg_ir::annotations::StreamHandle,
) -> Result<(), cadmpeg_core::CodecError> {
    let loop_faces = collect_graph_map(ctx,
        out.loops.iter().map(|lp| (&lp.id, &lp.face)),
        "index Parasolid pcurve loop faces")?;
    let faces = collect_graph_map(ctx, out.faces.iter().map(|face| (&face.id, face)),
        "index Parasolid pcurve faces")?;
    let surfaces = collect_graph_map(ctx,
        out.surfaces.iter().map(|surface| (&surface.id, surface)),
        "index Parasolid pcurve surfaces")?;
    let edges = collect_graph_map(ctx, out.edges.iter().map(|edge| (&edge.id, edge)),
        "index Parasolid pcurve edges")?;
    let curves = collect_graph_map(ctx, out.curves.iter().map(|curve| (&curve.id, curve)),
        "index Parasolid pcurve curves")?;
    let mut derived = Vec::new();
    for coedge in &out.coedges {
        if !coedge.pcurves.is_empty() {
            continue;
        }
        let Some(face_id) = loop_faces.get(&coedge.owner_loop) else {
            continue;
        };
        let Some(face) = faces.get(face_id) else {
            continue;
        };
        let Some(surface) = surfaces.get(&face.surface) else {
            continue;
        };
        let Some(SolvedSurfaceGeometry::Sphere(sphere_surface)) = surface.geometry.solved() else {
            continue;
        };
        let sphere_center = sphere_surface.center().get();
        let v_reference = *sphere_surface.frame().axis().as_raw();
        let u_reference = *sphere_surface.frame().reference().as_raw();
        let radius = sphere_surface.radius().get();
        let Some(edge) = edges.get(&coedge.edge) else {
            continue;
        };
        let Some(CurveGeometry::Solved(SolvedCurveGeometry::Circle(circle_curve))) = edge
            .curve()
            .and_then(|id| curves.get(id).copied())
            .map(|curve| &curve.geometry)
        else {
            continue;
        };
        let center = circle_curve.center().get();
        let axis = *circle_curve.frame().axis().as_raw();
        let circle_radius = circle_curve.radius().get();
        let axis_dot = axis.dot(v_reference);
        let reference = *circle_curve.frame().reference().as_raw();
        let tangent = v_reference.cross(u_reference);
        let offset = center.vector_from(sphere_center);
        // Allow rounding of the frame projections as well as the existing
        // absolute carrier fit tolerance; never form squared radii.
        let fit_tolerance = EPS_CIRCLE_RADIUS_MATCH + 64.0 * f64::EPSILON * radius;
        let (origin, direction) = if axis_dot.abs() > 1.0 - EPS_AXIS_ALIGNMENT {
            let height = offset.dot(v_reference);
            let transverse = cadmpeg_ir::math::Vector3::new(
                offset.x - height * v_reference.x,
                offset.y - height * v_reference.y,
                offset.z - height * v_reference.z,
            );
            let Some(latitude) = sphere_latitude(height, radius) else {
                continue;
            };
            let sine = (height / radius).clamp(-1.0, 1.0);
            let section_radius = radius * ((1.0 - sine.abs()) * (1.0 + sine.abs())).sqrt();
            if transverse.norm() > fit_tolerance
                || (section_radius - circle_radius).abs() > fit_tolerance
            {
                continue;
            }
            let phase = reference.dot(tangent).atan2(reference.dot(u_reference));
            (
                cadmpeg_ir::math::Point2::new(phase, latitude),
                cadmpeg_ir::math::Point2::new(axis_dot.signum(), 0.0),
            )
        } else if axis_dot.abs() < EPS_AXIS_ALIGNMENT
            && (circle_radius - radius).abs() <= fit_tolerance
            && offset.norm() <= fit_tolerance
        {
            // With equator = sphere_axis × circle_axis, increasing latitude
            // follows the circle's positive orientation. Its reference fixes
            // the initial latitude, including a reference at either pole.
            let Some(equator) = sphere_surface
                .frame()
                .axis()
                .finite_cross(*circle_curve.frame().axis())
                .unit_nonzero()
            else {
                continue;
            };
            let longitude = equator.dot(tangent).atan2(equator.dot(u_reference));
            let phase = reference.dot(v_reference).atan2(reference.dot(equator));
            (
                cadmpeg_ir::math::Point2::new(longitude, phase),
                cadmpeg_ir::math::Point2::new(0.0, 1.0),
            )
        } else {
            continue;
        };
        let Ok(line) = cadmpeg_ir::geometry::pcurve::LinePcurve::try_new(origin, direction) else {
            continue;
        };
        let geometry = PcurveGeometry::Line(line);
        // Verify both frame axes and their opposite points before assigning a
        // derived support relation. Near-aligned frames still need a physical fit.
        let mut fits = true;
        for parameter in [
            0.0,
            std::f64::consts::FRAC_PI_2,
            std::f64::consts::PI,
            -std::f64::consts::FRAC_PI_2,
        ] {
            let Some(uv) = cadmpeg_ir::eval::finite_or_refusal(
                cadmpeg_ir::eval::pcurve_uv(&geometry, parameter),
            )? else {
                fits = false;
                break;
            };
            let Some(lifted) = cadmpeg_ir::eval::finite_or_refusal(
                surface_point(&surface.geometry, uv.u, uv.v),
            )? else {
                fits = false;
                break;
            };
            let Some(curve_point) = cadmpeg_ir::eval::finite_or_refusal(
                cadmpeg_ir::eval::curve_point(
                    &CurveGeometry::Solved(SolvedCurveGeometry::Circle(*circle_curve)),
                    parameter,
                ),
            )? else {
                fits = false;
                break;
            };
            if lifted.distance(curve_point.get()) > fit_tolerance {
                fits = false;
                break;
            }
        }
        if !fits {
            continue;
        }
        let id = PcurveId::compose(
            &pcurve_namespace(),
            cadmpeg_ir::identity_key!("sphere:").then(coedge.id.key()),
        );
        ctx.reserve_collection_vec(&mut derived, 1, "collect derived Parasolid pcurves")?;
        derived.push((
            coedge.id.clone(),
            id.clone(),
            Pcurve {
                id,
                geometry,
                metadata: cadmpeg_ir::geometry::pcurve::PcurveMetadata::default(),
            },
        ));
    }
    let coedge_indices = collect_graph_map(ctx,
        out.coedges.iter().enumerate().map(|(index, coedge)| (coedge.id.clone(), index)),
        "index Parasolid derived coedges")?;
    for (coedge_id, id, pcurve) in derived {
        if let Some(index) = coedge_indices.get(&coedge_id) {
            let mut uses = Vec::new();
            ctx.reserve_collection_vec(&mut uses, 1, "bind derived Parasolid pcurve")?;
            uses.push(cadmpeg_ir::topology::PcurveUse {
                pcurve: id.clone(),
                isoparametric: None,
                parameter_range: None,
            });
            out.coedges[*index].pcurves = uses;
        }
        annotations
            .note(&id, source_stream, 0)
            .tag("derived_spherical_pcurve");
        annotations.exactness(&id, Exactness::Derived);
        admit_brep_entity(ctx)?;
        ctx.reserve_collection_vec(&mut out.pcurves, 1, "collect derived Parasolid pcurves")?;
        out.pcurves.push(pcurve);
    }
    Ok(())
}

fn derive_nurbs_isoparametric_pcurves(
    ctx: &DecodeContext<'_>,
    out: &mut Brep,
    annotations: &mut AnnotationBuilder,
    source_stream: &cadmpeg_ir::annotations::StreamHandle,
) -> Result<(), cadmpeg_core::CodecError> {
    let loop_faces = collect_graph_map(ctx,
        out.loops.iter().map(|lp| (&lp.id, &lp.face)),
        "index Parasolid pcurve loop faces")?;
    let faces = collect_graph_map(ctx, out.faces.iter().map(|face| (&face.id, face)),
        "index Parasolid pcurve faces")?;
    let surfaces = collect_graph_map(ctx,
        out.surfaces.iter().map(|surface| (&surface.id, surface)),
        "index Parasolid pcurve surfaces")?;
    let edges = collect_graph_map(ctx, out.edges.iter().map(|edge| (&edge.id, edge)),
        "index Parasolid pcurve edges")?;
    let curves = collect_graph_map(ctx, out.curves.iter().map(|curve| (&curve.id, curve)),
        "index Parasolid pcurve curves")?;
    let mut lane_refusals = crate::lane_refusal::LaneRefusals::new();
    let vertices = collect_graph_map(ctx,
        out.vertices.iter().map(|vertex| (&vertex.id, vertex)),
        "index Parasolid pcurve vertices")?;
    let points = collect_graph_map(ctx, out.points.iter().map(|point| (&point.id, point)),
        "index Parasolid pcurve points")?;
    let mut derived = Vec::new();
    for coedge in &out.coedges {
        if !coedge.pcurves.is_empty() {
            continue;
        }
        let Some(face_id) = loop_faces.get(&coedge.owner_loop) else {
            continue;
        };
        let Some(face) = faces.get(face_id) else {
            continue;
        };
        let Some(SurfaceGeometry::Solved(SolvedSurfaceGeometry::Nurbs(surface))) =
            surfaces.get(&face.surface).map(|item| &item.geometry)
        else {
            continue;
        };
        let Some(edge) = edges.get(&coedge.edge) else {
            continue;
        };
        let Some(curve) = edge
            .curve()
            .and_then(|id| curves.get(id).copied())
            .map(|item| &item.geometry)
        else {
            continue;
        };
        let endpoints = [edge.start.clone(), edge.end.clone()].map(|vertex_id| {
            let vertex = vertices.get(&vertex_id)?;
            Some(points.get(&vertex.point)?.position().get())
        });
        let endpoints = match endpoints {
            [Some(start), Some(end)] => Some([start, end]),
            _ => None,
        };
        let (geometry, parameter_range, fit_tolerance, cache) = match curve {
            CurveGeometry::Solved(SolvedCurveGeometry::Nurbs(curve)) => {
                let Some(parameter_range) = nurbs_edge_parameter_range(ctx, edge, curve, endpoints)?
                else {
                    continue;
                };
                let resolution = match derive_nurbs_edge_pcurve(surface, curve, parameter_range) {
                    Ok(resolution) => resolution,
                    Err(NurbsPcurveFailure::Carrier(error)) => {
                        lane_refusals.note(
                            ctx,
                            format_args!("isoparametric pcurve for edge {}", edge.id.as_str()),
                            &error,
                        )?;
                        continue;
                    }
                    Err(NurbsPcurveFailure::Resource(limit)) => return Err(limit.into()),
                };
                match resolution {
                    NurbsPcurveResolution::Exact(geometry) => {
                        (geometry, Some(parameter_range), None, false)
                    }
                    NurbsPcurveResolution::Cache {
                        geometry,
                        fit_tolerance,
                    } => (geometry, Some(parameter_range), Some(fit_tolerance), true),
                    NurbsPcurveResolution::OffSurface => {
                        out.stats.off_surface_nurbs_pcurves += 1;
                        continue;
                    }
                    NurbsPcurveResolution::Ambiguous => {
                        out.stats.ambiguous_pcurve_parameters += 1;
                        continue;
                    }
                    NurbsPcurveResolution::NoMatch => continue,
                }
            }
            CurveGeometry::Solved(SolvedCurveGeometry::Line(line_curve)) => {
                let origin = line_curve.origin().get();
                let direction = *line_curve.direction().as_raw();
                let resolution = resolve_axis_candidates([
                    ruled_surface_line_pcurve(ctx, surface, SurfaceParameterAxis::U, origin, direction)?,
                    ruled_surface_line_pcurve(ctx, surface, SurfaceParameterAxis::V, origin, direction)?,
                ]);
                match resolution {
                    InverseResolution::Unique(geometry) => (geometry, None, None, false),
                    InverseResolution::Ambiguous => {
                        out.stats.ambiguous_pcurve_parameters += 1;
                        continue;
                    }
                    InverseResolution::NoMatch => continue,
                }
            }
            _ => continue,
        };
        let id = PcurveId::compose(
            &pcurve_namespace(),
            if cache {
                cadmpeg_ir::identity_key!("nurbs-surface-cache:")
            } else {
                cadmpeg_ir::identity_key!("nurbs-isoparametric:")
            }
            .then(coedge.id.key()),
        );
        let parameter_range = match parameter_range.map(cadmpeg_ir::units::FiniteVector::new) {
            Some(None) => continue,
            Some(Some(range)) => Some(range),
            None => None,
        };
        let Ok(fit_tolerance) = fit_tolerance
            .map(cadmpeg_ir::geometry::FitTolerance::try_new)
            .transpose()
        else {
            continue;
        };
        ctx.reserve_collection_vec(&mut derived, 1, "collect derived Parasolid pcurves")?;
        derived.push((
            coedge.id.clone(),
            id.clone(),
            Pcurve {
                id,
                geometry,
                metadata: cadmpeg_ir::geometry::pcurve::PcurveMetadata::general(
                    None,
                    parameter_range,
                    fit_tolerance,
                ),
            },
            cache,
        ));
    }
    let coedge_indices = collect_graph_map(ctx,
        out.coedges.iter().enumerate().map(|(index, coedge)| (coedge.id.clone(), index)),
        "index Parasolid derived coedges")?;
    // The sink is drained before the `?` below: an error on that route must
    // not drop a refusal the walk above already pushed.
    for record in lane_refusals.take_records() {
        let note = crate::loss::spline_lane_refusal(ctx, &record)?;
        ctx.reserve_collection_vec(&mut out.losses, 1, "collect isoparametric pcurve losses")?;
        out.losses.push(note);
    }
    for (coedge_id, id, pcurve, cache) in derived {
        if let Some(index) = coedge_indices.get(&coedge_id) {
            let mut uses = Vec::new();
            ctx.reserve_collection_vec(&mut uses, 1, "bind derived Parasolid pcurve")?;
            uses.push(cadmpeg_ir::topology::PcurveUse {
                pcurve: id.clone(),
                isoparametric: None,
                parameter_range: (pcurve.parameter_range())
                    .map(|range| cadmpeg_ir::geometry::DirectedParameterRange::new(range.get()))
                    .transpose()
                    .map_err(cadmpeg_core::CodecError::malformed)?,
            });
            out.coedges[*index].pcurves = uses;
        }
        annotations.note(&id, source_stream, 0).tag(if cache {
            "derived_nurbs_surface_cache_pcurve"
        } else {
            "derived_nurbs_isoparametric_pcurve"
        });
        annotations.exactness(&id, Exactness::Derived);
        admit_brep_entity(ctx)?;
        ctx.reserve_collection_vec(&mut out.pcurves, 1, "collect derived Parasolid pcurves")?;
        out.pcurves.push(pcurve);
    }
    Ok(())
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum IntersectionPcurveSource {
    StoredCache,
    AnalyticInverse,
    NurbsInverse,
}

/// Conservative maximum separation between one analytic surface image of a
/// linear UV segment and the straight chord through its endpoints.
fn analytic_pcurve_chord_bound(
    surface: &SurfaceGeometry,
    start: cadmpeg_ir::math::Point2,
    end: cadmpeg_ir::math::Point2,
) -> Option<f64> {
    let du = (end.u - start.u).abs();
    let dv = (end.v - start.v).abs();
    let second_derivative_bound = match surface {
        SurfaceGeometry::Solved(SolvedSurfaceGeometry::Plane(_)) => 0.0,
        SurfaceGeometry::Solved(SolvedSurfaceGeometry::Cylinder(cylinder_surface)) => {
            let radius = cylinder_surface.radius().get();
            radius.abs() * du * du
        }
        SurfaceGeometry::Solved(SolvedSurfaceGeometry::Cone(cone_surface)) => {
            let radius = cone_surface.radius().get();
            let ratio = cone_surface.ratio().get();
            let half_angle = cone_surface.half_angle().get();
            let slope = half_angle.tan().abs();
            let radial_scale = 1.0f64.max(ratio.abs());
            let max_radius = (radius + start.v * half_angle.tan())
                .abs()
                .max((radius + end.v * half_angle.tan()).abs());
            radial_scale * (max_radius * du * du + 2.0 * slope * du * dv)
        }
        SurfaceGeometry::Solved(SolvedSurfaceGeometry::Sphere(sphere_surface)) => {
            let radius = sphere_surface.radius().get();
            radius.abs() * (du + dv).powi(2)
        }
        SurfaceGeometry::Solved(SolvedSurfaceGeometry::Torus(torus_surface)) => {
            let major_radius = torus_surface.major_radius().get();
            let minor_radius = torus_surface.minor_radius().get();
            (major_radius.abs() + minor_radius.abs()) * du * du
                + 2.0 * minor_radius.abs() * du * dv
                + minor_radius.abs() * dv * dv
        }
        _ => return None,
    };
    let bound = second_derivative_bound / 8.0;
    bound.is_finite().then_some(bound)
}

fn intersection_support_pcurve(
    ctx: &DecodeContext<'_>,
    support_data: &super::intersection::IntersectionSupportData,
    chart: &cadmpeg_ir::geometry::nurbs::NurbsCurve,
    surface_attr: u16,
    surface: &SurfaceGeometry,
    edge_endpoints: [cadmpeg_ir::math::Point3; 2],
    refusal: &mut crate::lane_refusal::LaneRefusals,
) -> Result<Option<(PcurveGeometry, [f64; 2], IntersectionPcurveSource)>, cadmpeg_core::CodecError> {
    macro_rules! some_or_none {
        ($value:expr) => {
            match $value {
                Some(value) => value,
                None => return Ok(None),
            }
        };
    }
    (|| -> Result<Option<(PcurveGeometry, [f64; 2], IntersectionPcurveSource)>, cadmpeg_core::CodecError> {
        if chart.degree() != 1
            || matches!(chart.pole_rows(), cadmpeg_ir::geometry::nurbs::NurbsPoles3::Rational { .. })
            || chart.periodic()
            || !support_data.fit_tolerance_mm.is_finite()
            || support_data.fit_tolerance_mm <= 0.0
        {
            return Ok(None);
        }
        let parameter_range = some_or_none!(nurbs_curve_parameter_domain(chart)).endpoints();
        let cadmpeg_ir::geometry::nurbs::NurbsPoles3::Polynomial { points: chart_points } = chart.pole_rows() else { return Ok(None) };
        let support_index = match support_data.supports.map(|support| support == surface_attr) {
            [true, false] => 0,
            [false, true] => 1,
            _ => return Ok(None),
        };
        let squared_distance = |left: cadmpeg_ir::math::Point3, right: cadmpeg_ir::math::Point3| {
            (left.x - right.x).powi(2) + (left.y - right.y).powi(2) + (left.z - right.z).powi(2)
        };
        let model_endpoints = [
            *some_or_none!(chart_points.first()),
            *some_or_none!(chart_points.last()),
        ];
        let direct_error = squared_distance(model_endpoints[0].get(), edge_endpoints[0])
            + squared_distance(model_endpoints[1].get(), edge_endpoints[1]);
        let reverse_error = squared_distance(model_endpoints[0].get(), edge_endpoints[1])
            + squared_distance(model_endpoints[1].get(), edge_endpoints[0]);
        let targets = if direct_error <= reverse_error {
            edge_endpoints
        } else {
            [edge_endpoints[1], edge_endpoints[0]]
        };
        let (mut control_points, source) = if let Some(support_uv) = &support_data.support_uv {
            let source_points = &support_uv[support_index];
            let mut control_points = Vec::new();
            ctx.reserve_collection_vec(&mut control_points, source_points.len(), "copy intersection support UV controls")?;
            control_points.extend_from_slice(source_points);
            match surface {
                SurfaceGeometry::Solved(SolvedSurfaceGeometry::Plane(_)) => {
                    for point in &mut control_points {
                        point.u *= LEN_TO_MM;
                        point.v *= LEN_TO_MM;
                    }
                }
                SurfaceGeometry::Solved(SolvedSurfaceGeometry::Cylinder(_)) => {
                    for point in &mut control_points {
                        point.v *= LEN_TO_MM;
                    }
                }
                SurfaceGeometry::Solved(SolvedSurfaceGeometry::Cone(_)) => {
                    for point in &mut control_points {
                        point.v *= LEN_TO_MM;
                    }
                }
                SurfaceGeometry::Solved(SolvedSurfaceGeometry::Sphere(_)) => {}
                SurfaceGeometry::Solved(SolvedSurfaceGeometry::Torus(_)) => {}
                SurfaceGeometry::Solved(SolvedSurfaceGeometry::Nurbs(_)) => {}
                _ => return Ok(None),
            }
            (control_points, IntersectionPcurveSource::StoredCache)
        } else {
            match surface {
                SurfaceGeometry::Solved(SolvedSurfaceGeometry::Nurbs(surface)) => {
                    let mut control_points = Vec::new();
                    ctx.reserve_collection_vec(&mut control_points, chart_points.len(), "solve intersection support UV controls")?;
                    for point in chart_points {
                        let parameters = match nurbs_surface_parameter_within_tolerance(
                            surface,
                            point.get(),
                            control_points.last().copied(),
                            support_data.fit_tolerance_mm,
                        ) {
                            Ok(Some(parameters)) => parameters,
                            Ok(None) => return Ok(None),
                            Err(limit) => return Err(limit.into()),
                        };
                        control_points.push(parameters.get());
                    }
                    (control_points, IntersectionPcurveSource::NurbsInverse)
                }
                _ => {
                    let mut control_points = Vec::new();
                    ctx.reserve_collection_vec(&mut control_points, chart_points.len(), "project intersection analytic controls")?;
                    for point in chart_points {
                        let parameters = some_or_none!(analytic_surface_parameters(surface, point.get()));
                        control_points.push(cadmpeg_ir::math::Point2::from(parameters));
                    }
                    for index in 1..control_points.len() {
                        let previous = control_points[index - 1];
                        match surface {
                            SurfaceGeometry::Solved(SolvedSurfaceGeometry::Cylinder(_)) => {
                                control_points[index].u += ((previous.u - control_points[index].u)
                                    / std::f64::consts::TAU)
                                    .round()
                                    * std::f64::consts::TAU;
                            }
                            SurfaceGeometry::Solved(SolvedSurfaceGeometry::Cone(_)) => {
                                control_points[index].u += ((previous.u - control_points[index].u)
                                    / std::f64::consts::TAU)
                                    .round()
                                    * std::f64::consts::TAU;
                            }
                            SurfaceGeometry::Solved(SolvedSurfaceGeometry::Sphere(_)) => {
                                control_points[index].u += ((previous.u - control_points[index].u)
                                    / std::f64::consts::TAU)
                                    .round()
                                    * std::f64::consts::TAU;
                            }
                            SurfaceGeometry::Solved(SolvedSurfaceGeometry::Torus(_)) => {
                                control_points[index].u += ((previous.u - control_points[index].u)
                                    / std::f64::consts::TAU)
                                    .round()
                                    * std::f64::consts::TAU;
                            }
                            SurfaceGeometry::Solved(SolvedSurfaceGeometry::Plane(_)) => {}
                            _ => return Ok(None),
                        }
                        if matches!(
                            surface,
                            SurfaceGeometry::Solved(SolvedSurfaceGeometry::Torus(_))
                        ) {
                            control_points[index].v += ((previous.v - control_points[index].v)
                                / std::f64::consts::TAU)
                                .round()
                                * std::f64::consts::TAU;
                        }
                    }
                    (control_points, IntersectionPcurveSource::AnalyticInverse)
                }
            }
        };
        if control_points.len() != chart_points.len() {
            return Ok(None);
        }
        if let SurfaceGeometry::Solved(SolvedSurfaceGeometry::Nurbs(surface)) = surface {
            let tolerance = inverse_coordinate_tolerance(edge_endpoints);
            let last = control_points.len() - 1;
            for (index, target) in [(0, targets[0]), (last, targets[1])] {
                control_points[index] = match nurbs_surface_parameter_within_tolerance(
                    surface,
                    target,
                    Some(control_points[index]),
                    tolerance,
                ) {
                    Ok(Some(parameters)) => parameters.get(),
                    Ok(None) => return Ok(None),
                    Err(limit) => return Err(limit.into()),
                };
            }
        } else {
            let adjust_periodic = |parameter: f64, reference: f64| {
                parameter
                    + ((reference - parameter) / std::f64::consts::TAU).round()
                        * std::f64::consts::TAU
            };
            let last = control_points.len() - 1;
            for (index, target) in [(0, targets[0]), (last, targets[1])] {
                let reference = control_points[index];
                let mut parameters =
                    cadmpeg_ir::math::Point2::from(some_or_none!(analytic_surface_parameters(surface, target)));
                match surface {
                    SurfaceGeometry::Solved(SolvedSurfaceGeometry::Cylinder(_)) => {
                        parameters.u = adjust_periodic(parameters.u, reference.u);
                    }
                    SurfaceGeometry::Solved(SolvedSurfaceGeometry::Cone(_)) => {
                        parameters.u = adjust_periodic(parameters.u, reference.u);
                    }
                    SurfaceGeometry::Solved(SolvedSurfaceGeometry::Sphere(_)) => {
                        parameters.u = adjust_periodic(parameters.u, reference.u);
                    }
                    SurfaceGeometry::Solved(SolvedSurfaceGeometry::Torus(_)) => {
                        parameters.u = adjust_periodic(parameters.u, reference.u);
                    }
                    SurfaceGeometry::Solved(SolvedSurfaceGeometry::Plane(_)) => {}
                    _ => return Ok(None),
                }
                if matches!(
                    surface,
                    SurfaceGeometry::Solved(SolvedSurfaceGeometry::Torus(_))
                ) {
                    parameters.v = adjust_periodic(parameters.v, reference.v);
                }
                control_points[index] = parameters;
            }
        }
        let tolerance = inverse_coordinate_tolerance(edge_endpoints);
        for (parameters, target) in [some_or_none!(control_points.first()), some_or_none!(control_points.last())]
            .into_iter()
            .zip(targets)
        {
            let point = match surface_point(surface, parameters.u, parameters.v) {
                Ok(point) => point.get(),
                Err(cadmpeg_ir::eval::EvaluationFailure::ResourceLimit(limit)) => {
                    return Err(limit.into())
                }
                Err(_) => return Ok(None),
            };
            if squared_distance(point, target) > tolerance * tolerance {
                return Ok(None);
            }
        }
        let mut mapped_points = Vec::new();
        ctx.reserve_collection_vec(&mut mapped_points, control_points.len(), "map intersection support controls")?;
        for parameters in &control_points {
            let point = match surface_point(surface, parameters.u, parameters.v) {
                Ok(point) => point.get(),
                Err(cadmpeg_ir::eval::EvaluationFailure::ResourceLimit(limit)) => {
                    return Err(limit.into())
                }
                Err(_) => return Ok(None),
            };
            mapped_points.push(point);
        }
        let mut control_errors = Vec::new();
        ctx.reserve_collection_vec(&mut control_errors, mapped_points.len(), "check intersection support control errors")?;
        control_errors.extend(mapped_points.iter().zip(chart_points).map(|(point, target)| {
            squared_distance(*point, target.get()).sqrt()
        }));
        if control_errors
            .iter()
            .any(|error| !error.is_finite() || *error > support_data.fit_tolerance_mm)
        {
            return Ok(None);
        }
        for ((parameters, chord), endpoint_errors) in control_points
            .windows(2)
            .zip(chart_points.windows(2))
            .zip(control_errors.windows(2))
        {
            let exceeds = match surface {
                SurfaceGeometry::Solved(SolvedSurfaceGeometry::Nurbs(surface)) => {
                    match nurbs_surface_parameter_segment_chord_bound(
                        surface,
                        [parameters[0], parameters[1]],
                        [chord[0].get(), chord[1].get()],
                    ) {
                        Ok(error) => {
                            error.is_none_or(|error| error > support_data.fit_tolerance_mm)
                        }
                        Err(limit) => return Err(limit.into()),
                    }
                }
                _ => analytic_pcurve_chord_bound(surface, parameters[0], parameters[1]).is_none_or(
                    |curvature_error| {
                        curvature_error + endpoint_errors[0].max(endpoint_errors[1])
                            > support_data.fit_tolerance_mm
                    },
                ),
            };
            if exceeds {
                return Ok(None);
            }
        }
        let knots_source = chart.knots().as_slice();
        let mut knots = Vec::new();
        ctx.reserve_collection_vec(&mut knots, knots_source.len(), "copy intersection support pcurve knots")?;
        knots.extend_from_slice(knots_source);
        ctx.charge_collection_items(
            u64::try_from(control_points.len()).map_err(|_| ctx.refuse_codec_limit("admit intersection support pcurve controls", u64::MAX - 1, u64::MAX))?,
            "admit intersection support pcurve controls",
        )?;
        let nurbs =
            match PcurveNurbs::from_lanes(1, knots, control_points, None, false) {
                Ok(nurbs) => nurbs,
                Err(error) => {
                    refusal.note(
                        ctx,
                        format_args!(
                            "sldprt intersection support pcurve for surface attr {surface_attr}"
                        ),
                        &error,
                    )?;
                    return Ok(None);
                }
            };
        Ok(Some((
            PcurveGeometry::Nurbs { nurbs },
            parameter_range,
            source,
        )))
    })()
}

fn resolve_axis_candidates<T, const N: usize>(
    candidates: [InverseResolution<T>; N],
) -> InverseResolution<T> {
    let mut unique = None;
    for candidate in candidates {
        match candidate {
            InverseResolution::NoMatch => {}
            InverseResolution::Ambiguous => return InverseResolution::Ambiguous,
            InverseResolution::Unique(value) if unique.is_none() => unique = Some(value),
            InverseResolution::Unique(_) => return InverseResolution::Ambiguous,
        }
    }
    unique.map_or(InverseResolution::NoMatch, InverseResolution::Unique)
}

#[derive(Debug)]
enum NurbsPcurveFailure {
    Carrier(cadmpeg_ir::geometry::nurbs::NurbsError),
    Resource(cadmpeg_core::decode::ResourceLimit),
}

impl From<cadmpeg_ir::geometry::nurbs::NurbsError> for NurbsPcurveFailure {
    fn from(error: cadmpeg_ir::geometry::nurbs::NurbsError) -> Self {
        Self::Carrier(error)
    }
}

impl From<cadmpeg_core::decode::ResourceLimit> for NurbsPcurveFailure {
    fn from(limit: cadmpeg_core::decode::ResourceLimit) -> Self {
        Self::Resource(limit)
    }
}

fn nurbs_boundary_pcurve(
    surface: &cadmpeg_ir::geometry::nurbs::NurbsSurface,
    curve: &cadmpeg_ir::geometry::nurbs::NurbsCurve,
    fixed_axis: SurfaceParameterAxis,
) -> Result<InverseResolution<PcurveGeometry>, cadmpeg_core::decode::ResourceLimit> {
    let (fixed_degree, fixed_count, fixed_knots) = match fixed_axis {
        SurfaceParameterAxis::U => (
            surface.u_degree() as usize,
            surface.u_count(),
            surface.u_knots(),
        ),
        SurfaceParameterAxis::V => (
            surface.v_degree() as usize,
            surface.v_count(),
            surface.v_knots(),
        ),
    };
    let (varying_degree, varying_knots) = match fixed_axis {
        SurfaceParameterAxis::U => (surface.v_degree() as usize, surface.v_knots()),
        SurfaceParameterAxis::V => (surface.u_degree() as usize, surface.u_knots()),
    };
    let (Some(&fixed_min), Some(&fixed_max), Some(&varying_min)) = (
        fixed_knots.get(fixed_degree),
        fixed_knots.get(fixed_count),
        varying_knots.get(varying_degree),
    ) else {
        return Ok(InverseResolution::NoMatch);
    };
    if !fixed_min.is_finite()
        || !fixed_max.is_finite()
        || fixed_min >= fixed_max
        || !varying_min.is_finite()
    {
        return Ok(InverseResolution::NoMatch);
    }
    let tolerance = inverse_coordinate_tolerance(
        (0..surface.u_count())
            .flat_map(|u| (0..surface.v_count()).filter_map(move |v| surface.pole(u, v)))
            .chain((0..curve.pole_count()).filter_map(|index| curve.pole_rows().point_at(index)))
            .map(FinitePoint3::get),
    );
    let same_curve = |candidate: &cadmpeg_ir::geometry::nurbs::NurbsCurve| {
        candidate.degree() == curve.degree()
            && candidate.knots() == curve.knots()
            && candidate.periodic() == curve.periodic()
            && candidate.pole_count() == curve.pole_count()
            && (0..curve.pole_count()).all(|index| {
                let (Some(candidate), Some(actual)) = (
                    candidate.pole_rows().point_at(index),
                    curve.pole_rows().point_at(index),
                ) else {
                    return false;
                };
                    (candidate.x - actual.x).powi(2)
                        + (candidate.y - actual.y).powi(2)
                        + (candidate.z - actual.z).powi(2)
                        <= tolerance * tolerance
                })
            && (0..curve.pole_count()).all(|index| {
                match (
                    candidate.pole_rows().weight_at(index),
                    curve.pole_rows().weight_at(index),
                ) {
                    (None, None) => true,
                    (Some(candidate), Some(actual)) => {
                        (candidate - actual).abs() <= EPS_NURBS_WEIGHT
                    }
                    _ => false,
                }
            })
    };
    let mut fixed = None;
    for parameter in [fixed_min, fixed_max]
        .into_iter()
        .filter(|parameter| parameter.is_finite())
    {
        if nurbs_surface_isocurve(surface, fixed_axis, parameter)?
            .is_some_and(|candidate| same_curve(&candidate))
        {
            if fixed.is_some() {
                return Ok(InverseResolution::Ambiguous);
            }
            fixed = Some(parameter);
        }
    }
    let Some(fixed) = fixed else {
        return Ok(InverseResolution::NoMatch);
    };
    Ok(InverseResolution::Unique(match fixed_axis {
        SurfaceParameterAxis::U => PcurveGeometry::Line(
            match cadmpeg_ir::geometry::pcurve::LinePcurve::try_new(
                cadmpeg_ir::math::Point2::new(fixed, varying_min),
                cadmpeg_ir::math::Point2::new(0.0, 1.0),
            ) {
                Ok(payload) => payload,
                Err(_) => return Ok(InverseResolution::NoMatch),
            },
        ),
        SurfaceParameterAxis::V => PcurveGeometry::Line(
            match cadmpeg_ir::geometry::pcurve::LinePcurve::try_new(
                cadmpeg_ir::math::Point2::new(varying_min, fixed),
                cadmpeg_ir::math::Point2::new(1.0, 0.0),
            ) {
                Ok(payload) => payload,
                Err(_) => return Ok(InverseResolution::NoMatch),
            },
        ),
    }))
}

fn nurbs_strict_isocurve_pcurve(
    surface: &cadmpeg_ir::geometry::nurbs::NurbsSurface,
    curve: &cadmpeg_ir::geometry::nurbs::NurbsCurve,
) -> Result<InverseResolution<PcurveGeometry>, cadmpeg_core::decode::ResourceLimit> {
    let axis_candidate = |fixed_axis| -> Result<
        InverseResolution<PcurveGeometry>,
        cadmpeg_core::decode::ResourceLimit,
    > {
        let (uc, vc) = (surface.u_count(), surface.v_count());
        let (fixed_degree, fixed_count, fixed_knots, fixed_periodic) = match fixed_axis {
            SurfaceParameterAxis::U => (
                surface.u_degree(),
                uc,
                surface.u_knots(),
                surface.u_periodic(),
            ),
            SurfaceParameterAxis::V => (
                surface.v_degree(),
                vc,
                surface.v_knots(),
                surface.v_periodic(),
            ),
        };
        let (varying_degree, varying_count, varying_knots, varying_periodic) = match fixed_axis {
            SurfaceParameterAxis::U => (
                surface.v_degree(),
                vc,
                surface.v_knots(),
                surface.v_periodic(),
            ),
            SurfaceParameterAxis::V => (
                surface.u_degree(),
                uc,
                surface.u_knots(),
                surface.u_periodic(),
            ),
        };
        if curve.degree() != varying_degree
            || curve.knots() != varying_knots
            || curve.periodic() != varying_periodic
            || curve.pole_count() != varying_count
        {
            return Ok(InverseResolution::NoMatch);
        }
        let (Some(&fixed_min), Some(&fixed_max)) = (fixed_knots.get(1), fixed_knots.get(2)) else {
            return nurbs_boundary_pcurve(surface, curve, fixed_axis);
        };
        if fixed_degree != 1
            || fixed_count != 2
            || fixed_periodic
            || fixed_knots.as_slice() != [fixed_min, fixed_min, fixed_max, fixed_max]
            || fixed_min >= fixed_max
        {
            return nurbs_boundary_pcurve(surface, curve, fixed_axis);
        }
        let pole_indices = |varying: usize| match fixed_axis {
            SurfaceParameterAxis::U => (varying, vc + varying),
            SurfaceParameterAxis::V => (varying * vc, varying * vc + 1),
        };
        let weight_mismatch = (0..varying_count).any(|varying| {
            let (a, b) = pole_indices(varying);
            match (
                surface.weight(a / vc, a % vc),
                surface.weight(b / vc, b % vc),
                curve.pole_rows().weight_at(varying),
            ) {
                (None, None, None) => false,
                (Some(a), Some(b), Some(actual)) => {
                    (a.get() - b.get()).abs() > EPS_NURBS_WEIGHT
                        || (a.get() - actual).abs() > EPS_NURBS_WEIGHT
                }
                _ => true,
            }
        });
        if weight_mismatch {
            return Ok(InverseResolution::NoMatch);
        }
        let pole_at = |index: usize| surface.pole(index / vc, index % vc);
        let mut delta_squared = 0.0;
        let mut relative_dot_delta = 0.0;
        for varying in 0..varying_count {
            let (a_index, b_index) = pole_indices(varying);
            let (Some(a), Some(b), Some(point)) = (
                pole_at(a_index),
                pole_at(b_index),
                curve.pole_rows().point_at(varying),
            ) else {
                return Ok(InverseResolution::NoMatch);
            };
            let delta = [b.x - a.x, b.y - a.y, b.z - a.z];
            let relative = [point.x - a.x, point.y - a.y, point.z - a.z];
            delta_squared += delta.iter().map(|value| value * value).sum::<f64>();
            relative_dot_delta += relative
                .iter()
                .zip(delta)
                .map(|(relative, delta)| relative * delta)
                .sum::<f64>();
        }
        let tolerance = inverse_coordinate_tolerance(
            (0..surface.u_count())
                .flat_map(|u| (0..surface.v_count()).filter_map(move |v| surface.pole(u, v)))
                .chain((0..curve.pole_count()).filter_map(|index| curve.pole_rows().point_at(index)))
                .map(FinitePoint3::get),
        );
        if delta_squared <= f64::EPSILON {
            let all_equal = (0..varying_count).all(|varying| {
                let (Some(a), Some(point)) = (
                    pole_at(pole_indices(varying).0),
                    curve.pole_rows().point_at(varying),
                ) else {
                    return false;
                };
                (point.x - a.x).powi(2) + (point.y - a.y).powi(2) + (point.z - a.z).powi(2)
                    <= tolerance * tolerance
            });
            return Ok(if all_equal {
                InverseResolution::Ambiguous
            } else {
                InverseResolution::NoMatch
            });
        }
        let factor = relative_dot_delta / delta_squared;
        let residual_squared = (0..varying_count)
            .filter_map(|varying| {
                let (a_index, b_index) = pole_indices(varying);
                let a = pole_at(a_index)?;
                let b = pole_at(b_index)?;
                let point = curve.pole_rows().point_at(varying)?;
                Some((point.x - (a.x + factor * (b.x - a.x))).powi(2)
                    + (point.y - (a.y + factor * (b.y - a.y))).powi(2)
                    + (point.z - (a.z + factor * (b.z - a.z))).powi(2))
            })
            .fold(0.0_f64, f64::max);
        let parameter_tolerance = INVERSE_PARAMETER_TOLERANCE;
        if !factor.is_finite()
            || factor < -parameter_tolerance
            || factor > 1.0 + parameter_tolerance
            || residual_squared > tolerance * tolerance
        {
            return Ok(InverseResolution::NoMatch);
        }
        let fixed = fixed_min + factor.clamp(0.0, 1.0) * (fixed_max - fixed_min);
        let Some(varying_degree) = usize::try_from(varying_degree).ok() else {
            return Ok(InverseResolution::NoMatch);
        };
        let (Some(&varying_min), Some(&varying_max)) = (
            varying_knots.get(varying_degree),
            varying_knots.get(varying_count),
        ) else {
            return Ok(InverseResolution::NoMatch);
        };
        if !varying_min.is_finite() || !varying_max.is_finite() || varying_min >= varying_max {
            return Ok(InverseResolution::NoMatch);
        }
        Ok(InverseResolution::Unique(match fixed_axis {
            SurfaceParameterAxis::U => PcurveGeometry::Line(
                match cadmpeg_ir::geometry::pcurve::LinePcurve::try_new(
                    cadmpeg_ir::math::Point2::new(fixed, varying_min),
                    cadmpeg_ir::math::Point2::new(0.0, 1.0),
                ) {
                    Ok(payload) => payload,
                    Err(_) => return Ok(InverseResolution::NoMatch),
                },
            ),
            SurfaceParameterAxis::V => PcurveGeometry::Line(
                match cadmpeg_ir::geometry::pcurve::LinePcurve::try_new(
                    cadmpeg_ir::math::Point2::new(varying_min, fixed),
                    cadmpeg_ir::math::Point2::new(1.0, 0.0),
                ) {
                    Ok(payload) => payload,
                    Err(_) => return Ok(InverseResolution::NoMatch),
                },
            ),
        }))
    };
    Ok(resolve_axis_candidates([
        axis_candidate(SurfaceParameterAxis::U)?,
        axis_candidate(SurfaceParameterAxis::V)?,
    ]))
}

fn nurbs_active_domain(knots: &[f64], degree: u32, count: usize) -> Option<[f64; 2]> {
    let degree = usize::try_from(degree).ok()?;
    if count <= degree || knots.len() != count.checked_add(degree)?.checked_add(1)? {
        return None;
    }
    let domain = [*knots.get(degree)?, *knots.get(count)?];
    (domain[0] < domain[1]).then_some(domain)
}

fn nurbs_roundoff_equal(left: f64, right: f64, scale: f64) -> bool {
    left.is_finite()
        && right.is_finite()
        && (left - right).abs()
            <= NURBS_POLE_ROUNDOFF_FACTOR
                * 1.0_f64.max(scale.abs()).max(left.abs()).max(right.abs())
}

fn nurbs_representation_matches(
    expected: &cadmpeg_ir::geometry::nurbs::NurbsCurve,
    actual: &cadmpeg_ir::geometry::nurbs::NurbsCurve,
) -> bool {
    let expected_points = expected.control_points();
    let actual_points = actual.control_points();
    let expected_weights = expected.weights();
    let actual_weights = actual.weights();
    if expected.degree() != actual.degree()
        || expected.periodic() != actual.periodic()
        || expected.knots().len() != actual.knots().len()
        || expected_points.len() != actual_points.len()
    {
        return false;
    }
    let scale = expected_points
        .iter()
        .chain(&actual_points)
        .flat_map(|point| [point.x.abs(), point.y.abs(), point.z.abs()])
        .chain(
            expected_weights
                .iter()
                .flatten()
                .map(|weight| weight.get().abs()),
        )
        .chain(
            actual_weights
                .iter()
                .flatten()
                .map(|weight| weight.get().abs()),
        )
        .fold(1.0_f64, f64::max);
    expected
        .knots()
        .iter()
        .zip(actual.knots().iter())
        .all(|(left, right)| nurbs_roundoff_equal(*left, *right, scale))
        && expected_points
            .iter()
            .zip(&actual_points)
            .all(|(left, right)| {
                nurbs_roundoff_equal(left.x, right.x, scale)
                    && nurbs_roundoff_equal(left.y, right.y, scale)
                    && nurbs_roundoff_equal(left.z, right.z, scale)
            })
        && match (&expected_weights, &actual_weights) {
            (None, None) => true,
            (Some(expected), Some(actual)) => {
                expected.len() == actual.len()
                    && expected
                        .iter()
                        .zip(actual)
                        .all(|(left, right)| nurbs_roundoff_equal(left.get(), right.get(), scale))
            }
            _ => false,
        }
}

fn nurbs_homogeneous_controls(
    curve: &cadmpeg_ir::geometry::nurbs::NurbsCurve,
) -> Option<Vec<[f64; 4]>> {
    curve
        .control_points()
        .iter()
        .enumerate()
        .map(|(index, point)| {
            let weight = curve.weights().map_or(1.0, |weights| weights[index].get());
            (weight > 0.0).then_some([point.x * weight, point.y * weight, point.z * weight, weight])
        })
        .collect()
}

fn insert_nurbs_homogeneous_knot(
    degree: usize,
    knots: &[f64],
    controls: &[[f64; 4]],
    value: f64,
) -> Option<(Vec<f64>, Vec<[f64; 4]>)> {
    if degree == 0
        || controls.is_empty()
        || knots.len() != controls.len().checked_add(degree)?.checked_add(1)?
        || !value.is_finite()
        || knots.iter().any(|knot| !knot.is_finite())
        || !knots_nondecreasing(knots)
    {
        return None;
    }
    let n = controls.len() - 1;
    let domain = [*knots.get(degree)?, *knots.get(n + 1)?];
    if value < domain[0] || value > domain[1] {
        return None;
    }
    let span = if value == domain[1] {
        n
    } else {
        (degree..=n).find(|index| knots[*index] <= value && value < knots[*index + 1])?
    };
    let multiplicity = knots.iter().filter(|knot| **knot == value).count();
    if multiplicity > degree {
        return None;
    }
    let mut inserted_knots = Vec::with_capacity(knots.len() + 1);
    inserted_knots.extend_from_slice(&knots[..=span]);
    inserted_knots.push(value);
    inserted_knots.extend_from_slice(&knots[span + 1..]);

    let mut inserted_controls = vec![[0.0; 4]; controls.len() + 1];
    let prefix_end = span - degree + 1;
    inserted_controls[..prefix_end].copy_from_slice(&controls[..prefix_end]);
    // The knot span never precedes the multiplicity of the inserted value: a
    // refused subtraction states that, where a saturating one would alias an
    // impossible span with span 0 and copy the wrong control run.
    let middle_end = span.checked_sub(multiplicity)?;
    let suffix_start = middle_end;
    inserted_controls[(suffix_start + 1)..].copy_from_slice(&controls[suffix_start..]);
    let middle_start = prefix_end;
    if middle_start <= middle_end {
        for index in middle_start..=middle_end {
            let denominator = knots[index + degree] - knots[index];
            if !denominator.is_finite() || denominator <= 0.0 {
                return None;
            }
            let alpha = (value - knots[index]) / denominator;
            inserted_controls[index] = std::array::from_fn(|axis| {
                alpha * controls[index][axis] + (1.0 - alpha) * controls[index - 1][axis]
            });
        }
    }
    Some((inserted_knots, inserted_controls))
}

/// The knots, control points and weights of one clamped NURBS segment, before
/// the carrier mints them.
type ClampedCurveLanes = (Vec<f64>, Vec<cadmpeg_ir::math::Point3>, Option<Vec<f64>>);

/// The clamped segment lanes of `curve` over `domain`. `None` when the curve
/// does not clamp to the domain at all; the lanes themselves are minted by the
/// caller.
fn clamp_nurbs_curve_to_domain_lanes(
    curve: &cadmpeg_ir::geometry::nurbs::NurbsCurve,
    domain: [f64; 2],
) -> Option<ClampedCurveLanes> {
    if curve.periodic()
        || !domain[0].is_finite()
        || !domain[1].is_finite()
        || domain[0] >= domain[1]
    {
        return None;
    }
    let degree = usize::try_from(curve.degree()).ok()?;
    let original_domain = nurbs_curve_parameter_domain(curve)?.endpoints();
    if domain[0] < original_domain[0] || domain[1] > original_domain[1] {
        return None;
    }
    let mut knots = curve.knots().to_vec();
    let mut controls = nurbs_homogeneous_controls(curve)?;
    let full_multiplicity = degree.checked_add(1)?;
    for value in domain {
        let multiplicity = knots.iter().filter(|knot| **knot == value).count();
        if multiplicity > full_multiplicity {
            return None;
        }
        for _ in multiplicity..full_multiplicity {
            (knots, controls) = insert_nurbs_homogeneous_knot(degree, &knots, &controls, value)?;
        }
    }
    let start = knots.iter().position(|knot| *knot == domain[0])?;
    let end = knots.iter().position(|knot| *knot == domain[1])?;
    let end_last = knots.iter().rposition(|knot| *knot == domain[1])?;
    if end <= start || end_last < end || end - start == 0 {
        return None;
    }
    let segment_controls = controls.get(start..end)?.to_vec();
    let segment_knots = knots.get(start..=end_last)?.to_vec();
    if segment_knots.len() != segment_controls.len().checked_add(degree)?.checked_add(1)? {
        return None;
    }
    let rational = curve.weights().is_some();
    let mut control_points = Vec::with_capacity(segment_controls.len());
    let mut weights = rational.then(Vec::new);
    for [x, y, z, weight] in segment_controls {
        if !weight.is_finite() || weight <= 0.0 {
            return None;
        }
        control_points.push(cadmpeg_ir::math::Point3::new(
            x / weight,
            y / weight,
            z / weight,
        ));
        if let Some(weights) = &mut weights {
            weights.push(weight);
        }
    }
    Some((segment_knots, control_points, weights))
}

fn clamp_nurbs_curve_to_domain(
    curve: &cadmpeg_ir::geometry::nurbs::NurbsCurve,
    domain: [f64; 2],
) -> Result<Option<cadmpeg_ir::geometry::nurbs::NurbsCurve>, cadmpeg_ir::geometry::nurbs::NurbsError>
{
    let Some((segment_knots, control_points, weights)) =
        clamp_nurbs_curve_to_domain_lanes(curve, domain)
    else {
        return Ok(None);
    };
    Ok(Some(cadmpeg_ir::geometry::nurbs::NurbsCurve::from_lanes(
        curve.degree(),
        segment_knots,
        control_points,
        weights,
        false,
    )?))
}

fn extended_nurbs_isocurve_axis_candidate(
    surface: &cadmpeg_ir::geometry::nurbs::NurbsSurface,
    curve: &cadmpeg_ir::geometry::nurbs::NurbsCurve,
    fixed_axis: SurfaceParameterAxis,
) -> Result<InverseResolution<PcurveGeometry>, NurbsPcurveFailure> {
    let (fixed_degree, fixed_count, fixed_knots, fixed_periodic) = match fixed_axis {
        SurfaceParameterAxis::U => (
            surface.u_degree(),
            surface.u_count(),
            surface.u_knots(),
            surface.u_periodic(),
        ),
        SurfaceParameterAxis::V => (
            surface.v_degree(),
            surface.v_count(),
            surface.v_knots(),
            surface.v_periodic(),
        ),
    };
    let (varying_degree, varying_count, varying_knots, varying_periodic) = match fixed_axis {
        SurfaceParameterAxis::U => (
            surface.v_degree(),
            surface.v_count(),
            surface.v_knots(),
            surface.v_periodic(),
        ),
        SurfaceParameterAxis::V => (
            surface.u_degree(),
            surface.u_count(),
            surface.u_knots(),
            surface.u_periodic(),
        ),
    };
    if fixed_periodic || varying_periodic || curve.periodic() || curve.degree() != varying_degree {
        return Ok(InverseResolution::NoMatch);
    }
    let Some(fixed_domain) = nurbs_active_domain(fixed_knots, fixed_degree, fixed_count) else {
        return Ok(InverseResolution::NoMatch);
    };
    let Some(varying_domain) = nurbs_active_domain(varying_knots, varying_degree, varying_count)
    else {
        return Ok(InverseResolution::NoMatch);
    };
    let Some(curve_domain) = nurbs_curve_parameter_domain(curve)
        .map(cadmpeg_ir::topology::IncreasingParameterInterval::endpoints)
    else {
        return Ok(InverseResolution::NoMatch);
    };
    if curve_domain[0] > varying_domain[0] || curve_domain[1] < varying_domain[1] {
        return Ok(InverseResolution::NoMatch);
    }

    let mut fixed_values = vec![fixed_domain[0], fixed_domain[1]];
    let overlap = [
        curve_domain[0].max(varying_domain[0]),
        curve_domain[1].min(varying_domain[1]),
    ];
    if overlap[0] < overlap[1] {
        let parameter = overlap[0].midpoint(overlap[1]);
        let point = match nurbs_curve_point_at(curve, parameter) {
            Ok(point) => Some(point),
            Err(cadmpeg_ir::eval::EvaluationFailure::ResourceLimit(limit)) => {
                return Err(limit.into())
            }
            Err(_) => None,
        };
        if let Some(point) = point {
            let tolerance = inverse_coordinate_tolerance(
                surface
                    .poles()
                    .into_iter()
                    .map(FinitePoint3::get)
                    .chain(std::iter::once(point.get())),
            );
            if let Some(parameters) =
                nurbs_surface_parameter_near_point(surface, point.get(), None)?
            {
                let mapped = match nurbs_surface_point(surface, parameters.u, parameters.v) {
                    Ok(mapped) => Some(mapped),
                    Err(cadmpeg_ir::eval::EvaluationFailure::ResourceLimit(limit)) => {
                        return Err(limit.into())
                    }
                    Err(_) => None,
                };
                if mapped
                    .is_some_and(|mapped| Point3::distance(point.get(), mapped.get()) <= tolerance)
                {
                    fixed_values.push(match fixed_axis {
                        SurfaceParameterAxis::U => parameters.u,
                        SurfaceParameterAxis::V => parameters.v,
                    });
                }
            }
        }
    }
    let parameter_tolerance = (INVERSE_PARAMETER_TOLERANCE * fixed_domain[1]
        - INVERSE_PARAMETER_TOLERANCE * fixed_domain[0])
        .abs();
    let mut unique_fixed_values = Vec::new();
    for value in fixed_values {
        if value.is_finite()
            && value >= fixed_domain[0] - parameter_tolerance
            && value <= fixed_domain[1] + parameter_tolerance
            && !unique_fixed_values
                .iter()
                .any(|known: &f64| (value - *known).abs() <= parameter_tolerance)
        {
            unique_fixed_values.push(value.clamp(fixed_domain[0], fixed_domain[1]));
        }
    }
    // The clamp does not vary with the candidate value, and the first candidate
    // already reaches it, so it is minted once here.
    let clamped = if unique_fixed_values.is_empty() {
        None
    } else {
        clamp_nurbs_curve_to_domain(curve, varying_domain)?
    };
    let Some(clamped) = clamped else {
        return Ok(InverseResolution::NoMatch);
    };
    let mut matched = None;
    for fixed in unique_fixed_values {
        if nurbs_surface_isocurve(surface, fixed_axis, fixed)?
            .is_some_and(|expected| nurbs_representation_matches(&expected, &clamped))
        {
            if matched.is_some() {
                return Ok(InverseResolution::Ambiguous);
            }
            matched = Some(fixed);
        }
    }
    let Some(fixed) = matched else {
        return Ok(InverseResolution::NoMatch);
    };
    Ok(InverseResolution::Unique(match fixed_axis {
        SurfaceParameterAxis::U => PcurveGeometry::Line(
            match cadmpeg_ir::geometry::pcurve::LinePcurve::try_new(
                cadmpeg_ir::math::Point2::new(fixed, varying_domain[0]),
                cadmpeg_ir::math::Point2::new(0.0, 1.0),
            ) {
                Ok(payload) => payload,
                Err(_) => return Ok(InverseResolution::NoMatch),
            },
        ),
        SurfaceParameterAxis::V => PcurveGeometry::Line(
            match cadmpeg_ir::geometry::pcurve::LinePcurve::try_new(
                cadmpeg_ir::math::Point2::new(varying_domain[0], fixed),
                cadmpeg_ir::math::Point2::new(1.0, 0.0),
            ) {
                Ok(payload) => payload,
                Err(_) => return Ok(InverseResolution::NoMatch),
            },
        ),
    }))
}

fn extended_nurbs_isocurve_pcurve(
    surface: &cadmpeg_ir::geometry::nurbs::NurbsSurface,
    curve: &cadmpeg_ir::geometry::nurbs::NurbsCurve,
) -> Result<InverseResolution<PcurveGeometry>, NurbsPcurveFailure> {
    Ok(resolve_axis_candidates([
        extended_nurbs_isocurve_axis_candidate(surface, curve, SurfaceParameterAxis::U)?,
        extended_nurbs_isocurve_axis_candidate(surface, curve, SurfaceParameterAxis::V)?,
    ]))
}

fn nurbs_isocurve_pcurve(
    surface: &cadmpeg_ir::geometry::nurbs::NurbsSurface,
    curve: &cadmpeg_ir::geometry::nurbs::NurbsCurve,
) -> Result<InverseResolution<PcurveGeometry>, NurbsPcurveFailure> {
    match nurbs_strict_isocurve_pcurve(surface, curve)? {
        InverseResolution::NoMatch => extended_nurbs_isocurve_pcurve(surface, curve),
        other => Ok(other),
    }
}

enum NurbsPcurveResolution {
    Exact(PcurveGeometry),
    Cache {
        geometry: PcurveGeometry,
        fit_tolerance: f64,
    },
    OffSurface,
    NoMatch,
    Ambiguous,
}

fn nurbs_curve_sample_parameters(
    curve: &cadmpeg_ir::geometry::nurbs::NurbsCurve,
    range: [f64; 2],
) -> Option<Vec<f64>> {
    let domain = nurbs_curve_parameter_domain(curve)?.endpoints();
    if !range[0].is_finite()
        || !range[1].is_finite()
        || range[0] >= range[1]
        || range[0] < domain[0]
        || range[1] > domain[1]
    {
        return None;
    }
    let mut parameters = vec![range[0], range[1]];
    for span in curve.knots().windows(2) {
        let start = span[0].max(range[0]);
        let end = span[1].min(range[1]);
        if !span[0].is_finite() || !span[1].is_finite() || start >= end {
            continue;
        }
        for index in 0..=NURBS_CACHE_SAMPLES_PER_SPAN {
            let fraction = index as f64 / NURBS_CACHE_SAMPLES_PER_SPAN as f64;
            parameters.push(cadmpeg_ir::math::interpolate(start, end, fraction)?.get());
        }
    }
    parameters.sort_by(f64::total_cmp);
    // Every distinct sample participates in the fit bound, including tiny spans.
    parameters.dedup();
    (!parameters.is_empty()).then_some(parameters)
}

fn nurbs_edge_endpoint_parameters(
    surface: &cadmpeg_ir::geometry::nurbs::NurbsSurface,
    curve: &cadmpeg_ir::geometry::nurbs::NurbsCurve,
    range: [f64; 2],
) -> Result<Option<[cadmpeg_ir::math::Point2; 2]>, cadmpeg_core::decode::ResourceLimit> {
    let Some(first) = cadmpeg_ir::eval::finite_or_refusal(nurbs_curve_point_at(curve, range[0]))?
    else {
        return Ok(None);
    };
    let Some(last) = cadmpeg_ir::eval::finite_or_refusal(nurbs_curve_point_at(curve, range[1]))?
    else {
        return Ok(None);
    };
    // The inverse-projection tolerance of this surface's coordinates against
    // the NURBS endpoint tolerance.
    let tolerance = looser_tolerance(
        inverse_coordinate_tolerance(
            surface
                .poles()
                .into_iter()
                .chain([first, last])
                .map(FinitePoint3::get),
        ),
        NURBS_ENDPOINT_TOLERANCE_MM,
    );
    let project =
        |point| -> Result<Option<cadmpeg_ir::math::Point2>, cadmpeg_core::decode::ResourceLimit> {
            let Some(parameters) = nurbs_surface_parameter_near_point(surface, point, None)? else {
                return Ok(None);
            };
            let Some(mapped) = cadmpeg_ir::eval::finite_or_refusal(nurbs_surface_point(
                surface,
                parameters.u,
                parameters.v,
            ))?
            else {
                return Ok(None);
            };
            Ok((Point3::distance(point, mapped.get()) <= tolerance).then_some(parameters.get()))
        };
    let Some(start) = project(first.get())? else {
        return Ok(None);
    };
    let Some(end) = project(last.get())? else {
        return Ok(None);
    };
    Ok(Some([start, end]))
}

fn nurbs_curve_surface_deviation(
    surface: &cadmpeg_ir::geometry::nurbs::NurbsSurface,
    curve: &cadmpeg_ir::geometry::nurbs::NurbsCurve,
    range: [f64; 2],
) -> Result<Option<f64>, cadmpeg_core::decode::ResourceLimit> {
    let Some(parameters) = nurbs_curve_sample_parameters(curve, range) else {
        return Ok(None);
    };
    let mut seed = None;
    let mut maximum = 0.0_f64;
    for parameter in parameters {
        let Some(point) =
            cadmpeg_ir::eval::finite_or_refusal(nurbs_curve_point_at(curve, parameter))?
        else {
            return Ok(None);
        };
        let projected = match seed {
            Some(seed) => nurbs_surface_parameter_near_point(surface, point.get(), Some(seed))?,
            None => None,
        };
        let parameters = match projected {
            Some(parameters) => Some(parameters),
            None => nurbs_surface_parameter_near_point(surface, point.get(), None)?,
        };
        let Some(parameters) = parameters else {
            return Ok(None);
        };
        let parameters = parameters.get();
        let Some(surface_point) = cadmpeg_ir::eval::finite_or_refusal(nurbs_surface_point(
            surface,
            parameters.u,
            parameters.v,
        ))?
        else {
            return Ok(None);
        };
        seed = Some(parameters);
        maximum = maximum.max(Point3::distance(point.get(), surface_point.get()));
    }
    Ok(maximum.is_finite().then_some(maximum))
}

/// The degree-one pcurve lanes a cached projection states, and the fit
/// tolerance they reach. `None` when the curve is not a degree-one cache
/// candidate or a projection does not land on the surface.
fn nurbs_degree_one_cache_lanes(
    surface: &cadmpeg_ir::geometry::nurbs::NurbsSurface,
    curve: &cadmpeg_ir::geometry::nurbs::NurbsCurve,
    range: [f64; 2],
) -> Result<Option<(Vec<cadmpeg_ir::math::Point2>, f64)>, cadmpeg_core::decode::ResourceLimit> {
    if curve.degree() != 1 || curve.pole_rows().weight_at(0).is_some() || curve.periodic() {
        return Ok(None);
    }
    let curve_points = curve.pole_rows().raw_points();
    let mut control_points = Vec::with_capacity(curve_points.len());
    let mut seed = None;
    for point in &curve_points {
        let projected = match seed {
            Some(seed) => nurbs_surface_parameter_near_point(surface, *point, Some(seed))?,
            None => None,
        };
        let parameters = match projected {
            Some(parameters) => Some(parameters),
            None => nurbs_surface_parameter_near_point(surface, *point, None)?,
        };
        let Some(parameters) = parameters else {
            return Ok(None);
        };
        let parameters = parameters.get();
        seed = Some(parameters);
        control_points.push(parameters);
    }
    let Some(parameters) = nurbs_curve_sample_parameters(curve, range) else {
        return Ok(None);
    };
    let mut fit_tolerance = 0.0_f64;
    for parameter in parameters {
        let Some(model_point) =
            cadmpeg_ir::eval::finite_or_refusal(nurbs_curve_point_at(curve, parameter))?
        else {
            return Ok(None);
        };
        let Some(uv) = cadmpeg_ir::eval::finite_or_refusal(nurbs_pcurve_uv(
            1,
            curve.knots(),
            &control_points,
            None,
            parameter,
        ))?
        else {
            return Ok(None);
        };
        let Some(mapped_point) =
            cadmpeg_ir::eval::finite_or_refusal(nurbs_surface_point(surface, uv.u, uv.v))?
        else {
            return Ok(None);
        };
        fit_tolerance = fit_tolerance.max(Point3::distance(model_point.get(), mapped_point.get()));
    }
    if !fit_tolerance.is_finite() {
        return Ok(None);
    }
    Ok(Some((control_points, fit_tolerance)))
}

fn nurbs_degree_one_cache_pcurve(
    surface: &cadmpeg_ir::geometry::nurbs::NurbsSurface,
    curve: &cadmpeg_ir::geometry::nurbs::NurbsCurve,
    range: [f64; 2],
) -> Result<Option<(PcurveGeometry, f64)>, NurbsPcurveFailure> {
    let Some((control_points, fit_tolerance)) =
        nurbs_degree_one_cache_lanes(surface, curve, range)?
    else {
        return Ok(None);
    };
    let nurbs = PcurveNurbs::from_lanes(1, curve.knots().to_vec(), control_points, None, false)?;
    Ok(Some((PcurveGeometry::Nurbs { nurbs }, fit_tolerance)))
}

fn nurbs_edge_parameter_range(
    ctx: &DecodeContext<'_>,
    edge: &Edge,
    curve: &cadmpeg_ir::geometry::nurbs::NurbsCurve,
    endpoints: Option<[cadmpeg_ir::math::Point3; 2]>,
) -> Result<Option<[f64; 2]>, cadmpeg_core::CodecError> {
    let Some(domain) = nurbs_curve_parameter_domain(curve) else {
        return Ok(None);
    };
    let domain = domain.endpoints();
    let range = if let Some(range) = edge.param_range() {
        range.get()
    } else {
        let Some([start, end]) = endpoints else {
            return Ok(None);
        };
        match (
            nurbs_parameter_at_point(ctx, curve, start)?,
            nurbs_parameter_at_point(ctx, curve, end)?,
        ) {
            (InverseResolution::Unique(start), InverseResolution::Unique(end)) => [start, end],
            _ => return Ok(None),
        }
    };
    if !range[0].is_finite()
        || !range[1].is_finite()
        || range[0] == range[1]
        || range
            .iter()
            .any(|parameter| *parameter < domain[0] || *parameter > domain[1])
    {
        return Ok(None);
    }
    Ok(Some([range[0].min(range[1]), range[0].max(range[1])]))
}

fn derive_nurbs_edge_pcurve(
    surface: &cadmpeg_ir::geometry::nurbs::NurbsSurface,
    curve: &cadmpeg_ir::geometry::nurbs::NurbsCurve,
    range: [f64; 2],
) -> Result<NurbsPcurveResolution, NurbsPcurveFailure> {
    if nurbs_edge_endpoint_parameters(surface, curve, range)?.is_none() {
        return Ok(NurbsPcurveResolution::OffSurface);
    }
    if nurbs_curve_surface_deviation(surface, curve, range)?.is_none() {
        return Ok(NurbsPcurveResolution::NoMatch);
    }
    Ok(match nurbs_isocurve_pcurve(surface, curve)? {
        InverseResolution::Unique(geometry) => NurbsPcurveResolution::Exact(geometry),
        InverseResolution::Ambiguous => NurbsPcurveResolution::Ambiguous,
        InverseResolution::NoMatch => match nurbs_degree_one_cache_pcurve(surface, curve, range)? {
            Some((geometry, fit_tolerance)) => NurbsPcurveResolution::Cache {
                geometry,
                fit_tolerance,
            },
            None => NurbsPcurveResolution::NoMatch,
        },
    })
}

fn ruled_surface_line_pcurve(
    ctx: &DecodeContext<'_>,
    surface: &cadmpeg_ir::geometry::nurbs::NurbsSurface,
    fixed_axis: SurfaceParameterAxis,
    line_origin: cadmpeg_ir::math::Point3,
    line_direction: cadmpeg_ir::math::Vector3,
) -> Result<InverseResolution<PcurveGeometry>, cadmpeg_core::CodecError> {
    let (uc, vc) = (surface.u_count(), surface.v_count());
    let (varying_degree, varying_count, varying_knots, varying_periodic) = match fixed_axis {
        SurfaceParameterAxis::U => (
            surface.v_degree(),
            vc,
            surface.v_knots(),
            surface.v_periodic(),
        ),
        SurfaceParameterAxis::V => (
            surface.u_degree(),
            uc,
            surface.u_knots(),
            surface.u_periodic(),
        ),
    };
    let (fixed_degree, fixed_count, fixed_knots) = match fixed_axis {
        SurfaceParameterAxis::U => (surface.u_degree(), uc, surface.u_knots()),
        SurfaceParameterAxis::V => (surface.v_degree(), vc, surface.v_knots()),
    };
    let (Some(&varying_min), Some(&varying_max)) = (varying_knots.get(1), varying_knots.get(2))
    else {
        return Ok(InverseResolution::NoMatch);
    };
    if varying_degree != 1
        || varying_count != 2
        || varying_periodic
        || varying_knots.as_slice() != [varying_min, varying_min, varying_max, varying_max]
        || varying_min >= varying_max
        || surface.pole_weights().is_some_and(|weights| {
            (0..fixed_count).any(|fixed| {
                let (a, b) = match fixed_axis {
                    SurfaceParameterAxis::U => (fixed * vc, fixed * vc + 1),
                    SurfaceParameterAxis::V => (fixed, vc + fixed),
                };
                (weights[a].get() - weights[b].get()).abs() > EPS_NURBS_WEIGHT
            })
        })
    {
        return Ok(InverseResolution::NoMatch);
    }
    let Some(fixed_degree) = usize::try_from(fixed_degree).ok() else {
        return Ok(InverseResolution::NoMatch);
    };
    let (Some(&fixed_min), Some(&fixed_max)) =
        (fixed_knots.get(fixed_degree), fixed_knots.get(fixed_count))
    else {
        return Ok(InverseResolution::NoMatch);
    };
    if !fixed_min.is_finite() || !fixed_max.is_finite() || fixed_min >= fixed_max {
        return Ok(InverseResolution::NoMatch);
    }
    let evaluate_ruling = |fixed: f64| {
        let parameters = |varying| match fixed_axis {
            SurfaceParameterAxis::U => (fixed, varying),
            SurfaceParameterAxis::V => (varying, fixed),
        };
        let (u0, v0) = parameters(varying_min);
        let (u1, v1) = parameters(varying_max);
        let Some(first) =
            cadmpeg_ir::eval::finite_or_refusal(nurbs_surface_point(surface, u0, v0))?
        else {
            return Ok(None);
        };
        let Some(second) =
            cadmpeg_ir::eval::finite_or_refusal(nurbs_surface_point(surface, u1, v1))?
        else {
            return Ok(None);
        };
        Ok(Some((first, second)))
    };
    let direction_squared = line_direction.x * line_direction.x
        + line_direction.y * line_direction.y
        + line_direction.z * line_direction.z;
    if direction_squared <= f64::EPSILON {
        return Ok(InverseResolution::NoMatch);
    }
    let perpendicular_squared = |point: cadmpeg_ir::math::Point3| {
        let relative = [
            point.x - line_origin.x,
            point.y - line_origin.y,
            point.z - line_origin.z,
        ];
        let along = (relative[0] * line_direction.x
            + relative[1] * line_direction.y
            + relative[2] * line_direction.z)
            / direction_squared;
        (relative[0] - along * line_direction.x).powi(2)
            + (relative[1] - along * line_direction.y).powi(2)
            + (relative[2] - along * line_direction.z).powi(2)
    };
    let objective = |parameter: f64| {
        let Some((a, b)) = evaluate_ruling(parameter)? else {
            return Ok(None);
        };
        Ok(Some(perpendicular_squared(a.get()).max(perpendicular_squared(b.get()))))
    };
    let Some(candidates) = sampled_parameter_minima(ctx, fixed_knots, [fixed_min, fixed_max], objective)?
    else {
        return Ok(InverseResolution::NoMatch);
    };
    let resolution = unique_inverse_parameter(
        candidates,
        inverse_coordinate_tolerance(
            surface
                .poles()
                .into_iter()
                .map(FinitePoint3::get)
                .chain(std::iter::once(line_origin)),
        ),
        [fixed_min, fixed_max],
    );
    let fixed = match resolution {
        InverseResolution::Unique(fixed) => fixed,
        InverseResolution::NoMatch => return Ok(InverseResolution::NoMatch),
        InverseResolution::Ambiguous => return Ok(InverseResolution::Ambiguous),
    };
    let Some((a, b)) = evaluate_ruling(fixed)? else {
        return Ok(InverseResolution::NoMatch);
    };
    let delta = [b.x - a.x, b.y - a.y, b.z - a.z];
    let delta_squared = delta[0] * delta[0] + delta[1] * delta[1] + delta[2] * delta[2];
    if delta_squared <= f64::EPSILON {
        return Ok(InverseResolution::NoMatch);
    }
    let project = |value: [f64; 3]| {
        (value[0] * delta[0] + value[1] * delta[1] + value[2] * delta[2]) / delta_squared
    };
    let offset = project([
        line_origin.x - a.x,
        line_origin.y - a.y,
        line_origin.z - a.z,
    ]);
    let rate = project([line_direction.x, line_direction.y, line_direction.z]);
    if rate == 0.0 {
        return Ok(InverseResolution::NoMatch);
    }
    let domain = varying_max - varying_min;
    Ok(InverseResolution::Unique(match fixed_axis {
        SurfaceParameterAxis::U => PcurveGeometry::Line(
            match cadmpeg_ir::geometry::pcurve::LinePcurve::try_new(
                cadmpeg_ir::math::Point2::new(fixed, varying_min + offset * domain),
                cadmpeg_ir::math::Point2::new(0.0, rate * domain),
            ) {
                Ok(payload) => payload,
                Err(_) => return Ok(InverseResolution::NoMatch),
            },
        ),
        SurfaceParameterAxis::V => PcurveGeometry::Line(
            match cadmpeg_ir::geometry::pcurve::LinePcurve::try_new(
                cadmpeg_ir::math::Point2::new(varying_min + offset * domain, fixed),
                cadmpeg_ir::math::Point2::new(rate * domain, 0.0),
            ) {
                Ok(payload) => payload,
                Err(_) => return Ok(InverseResolution::NoMatch),
            },
        ),
    }))
}

fn solve_face_orientation(
    ctx: &DecodeContext<'_>,
    out: &mut Brep,
) -> Result<(), cadmpeg_core::CodecError> {
    let mut loop_faces = HashMap::new();
    for lp in &out.loops {
        reserve_graph_map_key(ctx, &mut loop_faces, &lp.id, "index oriented Parasolid loops")?;
        loop_faces.insert(lp.id.clone(), lp.face.clone());
    }
    let mut uses: HashMap<EdgeId, Vec<(FaceId, bool)>> = HashMap::new();
    for coedge in &out.coedges {
        ctx.charge_work(1, "orient Parasolid face uses")?;
        if let Some(face) = loop_faces.get(&coedge.owner_loop) {
            reserve_graph_map_key(ctx, &mut uses, &coedge.edge, "index Parasolid edge face uses")?;
            let edge_uses = uses.entry(coedge.edge.clone()).or_default();
            ctx.reserve_collection_vec(edge_uses, 1, "collect Parasolid edge face uses")?;
            edge_uses.push((face.clone(), coedge.sense == Sense::Reversed));
        }
    }
    let mut adjacency: HashMap<FaceId, Vec<(FaceId, bool)>> = HashMap::new();
    for edge_uses in uses.values().filter(|uses| uses.len() == 2) {
        let (a, a_reversed) = &edge_uses[0];
        let (b, b_reversed) = &edge_uses[1];
        let parity = *a_reversed == *b_reversed;
        for (face, neighbor) in [(a, b), (b, a)] {
            reserve_graph_map_key(ctx, &mut adjacency, face, "index Parasolid face adjacency")?;
            let neighbors = adjacency.entry(face.clone()).or_default();
            ctx.reserve_collection_vec(neighbors, 1, "collect Parasolid face adjacency")?;
            neighbors.push((neighbor.clone(), parity));
        }
    }
    let mut initial = HashMap::new();
    for face in &out.faces {
        reserve_graph_map_key(ctx, &mut initial, &face.id, "index initial Parasolid face senses")?;
        initial.insert(face.id.clone(), face.sense == Sense::Reversed);
    }
    let mut solved = HashMap::new();
    for root in out.faces.iter().map(|face| face.id.clone()) {
        ctx.charge_work(1, "solve Parasolid face senses")?;
        if solved.contains_key(&root) {
            continue;
        }
        reserve_graph_map_key(ctx, &mut solved, &root, "track solved Parasolid face senses")?;
        solved.insert(root.clone(), initial[&root]);
        let mut pending = Vec::new();
        ctx.reserve_collection_vec(&mut pending, 1, "walk Parasolid face senses")?;
        pending.push(root);
        while let Some(face) = pending.pop() {
            ctx.charge_work(1, "walk Parasolid face senses")?;
            let sense = solved[&face];
            for (neighbor, parity) in adjacency.get(&face).into_iter().flatten() {
                ctx.charge_work(1, "walk Parasolid face adjacency")?;
                if !solved.contains_key(neighbor) {
                    reserve_graph_map_key(ctx, &mut solved, neighbor, "track solved Parasolid face senses")?;
                    solved.insert(neighbor.clone(), sense ^ parity);
                    ctx.reserve_collection_vec(&mut pending, 1, "walk Parasolid face senses")?;
                    pending.push(neighbor.clone());
                }
            }
        }
    }
    for face in &mut out.faces {
        face.sense = if solved.get(&face.id).copied().unwrap_or(false) {
            Sense::Reversed
        } else {
            Sense::Forward
        };
    }
    Ok(())
}

fn synthesize_cylinder_seams(
    ctx: &DecodeContext<'_>,
    out: &mut Brep,
    annotations: &mut AnnotationBuilder,
    source_stream: &cadmpeg_ir::annotations::StreamHandle,
) -> Result<(), cadmpeg_core::CodecError> {
    let surfaces = collect_graph_map(ctx,
        out.surfaces.iter().map(|surface| (&surface.id, surface)),
        "index Parasolid pcurve surfaces")?;
    let loops = collect_graph_map(ctx, out.loops.iter().map(|lp| (&lp.id, lp)),
        "index Parasolid seam loops")?;
    let coedges = collect_graph_map(ctx,
        out.coedges.iter().map(|coedge| (&coedge.id, coedge)),
        "index Parasolid seam coedges")?;
    let edges = collect_graph_map(ctx, out.edges.iter().map(|edge| (&edge.id, edge)),
        "index Parasolid pcurve edges")?;
    let curves = collect_graph_map(ctx, out.curves.iter().map(|curve| (&curve.id, curve)),
        "index Parasolid pcurve curves")?;
    let mut candidates = Vec::new();
    for face in &out.faces {
        let Some(surface) = surfaces.get(&face.surface) else {
            continue;
        };
        let Some(SolvedSurfaceGeometry::Cylinder(cylinder_surface)) = surface.geometry.solved()
        else {
            continue;
        };
        let ref_direction = *cylinder_surface.frame().reference().as_raw();
        if face.loops.len() != 2 {
            continue;
        }
        let mut members = face.loops.iter();
        let (Some(a), Some(b)) = (
            members.next().and_then(|id| loops.get(id)),
            members.next().and_then(|id| loops.get(id)),
        ) else {
            continue;
        };
        if a.coedges().len() != 1 || b.coedges().len() != 1 {
            continue;
        }
        let Some(ca) = coedges.get(&a.coedges()[0]) else {
            continue;
        };
        let Some(cb) = coedges.get(&b.coedges()[0]) else {
            continue;
        };
        let Some(ea) = edges.get(&ca.edge) else {
            continue;
        };
        let Some(eb) = edges.get(&cb.edge) else {
            continue;
        };
        let seam_point = |edge: &Edge| {
            if edge.start != edge.end {
                return None;
            }
            let curve = curves.get(edge.curve()?)?;
            let Some(SolvedCurveGeometry::Circle(circle_curve)) = curve.geometry.solved() else {
                return None;
            };
            let center = circle_curve.center().get();
            let radius = circle_curve.radius().get();
            Some(cadmpeg_ir::math::Point3::new(
                center.x - ref_direction.x * radius,
                center.y - ref_direction.y * radius,
                center.z - ref_direction.z * radius,
            ))
        };
        if let (Some(pa), Some(pb)) = (seam_point(ea), seam_point(eb)) {
            ctx.reserve_collection_vec(&mut candidates, 1, "collect Parasolid cylinder seam candidates")?;
            candidates.push((
                face.id.clone(),
                a.id.clone(),
                b.id.clone(),
                ca.id.clone(),
                cb.id.clone(),
                ea.start.clone(),
                eb.start.clone(),
                pa,
                pb,
            ));
        }
    }

    let mut removed = HashSet::new();
    let mut coedge_indices = collect_graph_map(ctx,
        out.coedges.iter().enumerate().map(|(index, coedge)| (coedge.id.clone(), index)),
        "index Parasolid seam coedges")?;
    for (face_id, loop_a, loop_b, circle_a, circle_b, vertex_a, vertex_b, pa, pb) in candidates {
        for (vertex_id, position) in [(&vertex_a, pa), (&vertex_b, pb)] {
            let Some(point_id) = out
                .vertices
                .iter()
                .find(|vertex| vertex.id == *vertex_id)
                .map(|vertex| vertex.point.clone())
            else {
                continue;
            };
            if let Some(point) = out.points.iter_mut().find(|point| point.id == point_id) {
                point.set_position(
                    cadmpeg_ir::features::FinitePoint3::new(position).ok_or_else(|| {
                        cadmpeg_core::CodecError::Malformed(Point::NON_FINITE_POSITION.into())
                    })?,
                );
            }
        }
        let direction = cadmpeg_ir::math::Vector3::new(pb.x - pa.x, pb.y - pa.y, pb.z - pa.z);
        let norm = direction.norm();
        if norm == 0.0 {
            continue;
        }
        let direction = cadmpeg_ir::math::Vector3::new(
            direction.x / norm,
            direction.y / norm,
            direction.z / norm,
        );
        let suffix = cadmpeg_ir::identity_key!("seam:").then(face_id.key());
        let curve_id = CurveId::compose(
            &cadmpeg_ir::identity_namespace!("sldprt", "brep", "curve"),
            suffix.clone(),
        );
        let edge_id = EdgeId::compose(
            &cadmpeg_ir::identity_namespace!("sldprt", "brep", "edge"),
            suffix.clone(),
        );
        let coedge_namespace = cadmpeg_ir::identity_namespace!("sldprt", "brep", "coedge");
        let seam_a = CoedgeId::compose(&coedge_namespace, suffix.clone().colon(0_u16));
        let seam_b = CoedgeId::compose(&coedge_namespace, suffix.colon(1_u16));
        for id in [
            curve_id.as_str(),
            edge_id.as_str(),
            seam_a.as_str(),
            seam_b.as_str(),
        ] {
            annotations
                .note(id, source_stream, 0)
                .tag("derived_periodic_seam");
            annotations.exactness(id, Exactness::Derived);
        }
        admit_brep_entity(ctx)?;
        ctx.reserve_collection_vec(&mut out.curves, 1, "collect Parasolid cylinder seam curves")?;
        out.curves.push(Curve {
            id: curve_id.clone(),
            source_object: None,
            geometry: CurveGeometry::Solved(SolvedCurveGeometry::Line(
                match cadmpeg_ir::geometry::analytic::LineCurve::try_new(pa, direction) {
                    Ok(payload) => payload,
                    Err(_) => continue,
                },
            )),
        });
        admit_brep_entity(ctx)?;
        ctx.reserve_collection_vec(&mut out.edges, 1, "collect Parasolid cylinder seam edges")?;
        out.edges.push(Edge {
            id: edge_id.clone(),
            carrier: cadmpeg_ir::topology::EdgeCarrier::new(Some(curve_id), Some([0.0, norm]))
                .map_err(cadmpeg_core::CodecError::malformed)?,
            start: vertex_a,
            end: vertex_b,
            tolerance: None,
        });
        reserve_graph_map_key(ctx, &mut coedge_indices, &seam_a, "index generated Parasolid seam coedges")?;
        coedge_indices.insert(seam_a.clone(), out.coedges.len());
        admit_brep_entity(ctx)?;
        ctx.reserve_collection_vec(&mut out.coedges, 1, "collect Parasolid cylinder seam coedges")?;
        out.coedges.push(Coedge {
            id: seam_a.clone(),
            owner_loop: loop_a.clone(),
            edge: edge_id.clone(),
            radial_next: seam_b.clone(),
            sense: Sense::Forward,
            use_curve: None,
            pcurves: Vec::new(),
        });
        reserve_graph_map_key(ctx, &mut coedge_indices, &seam_b, "index generated Parasolid seam coedges")?;
        coedge_indices.insert(seam_b.clone(), out.coedges.len());
        admit_brep_entity(ctx)?;
        ctx.reserve_collection_vec(&mut out.coedges, 1, "collect Parasolid cylinder seam coedges")?;
        out.coedges.push(Coedge {
            id: seam_b.clone(),
            owner_loop: loop_a.clone(),
            edge: edge_id,
            radial_next: seam_a.clone(),
            sense: Sense::Reversed,
            use_curve: None,
            pcurves: Vec::new(),
        });
        let ring_ids = [circle_a.clone(), seam_a, circle_b.clone(), seam_b];
        let mut ring_members = Vec::new();
        ctx.reserve_collection_vec(&mut ring_members, ring_ids.len(), "build Parasolid cylinder seam ring")?;
        ring_members.extend(ring_ids.iter().cloned());
        let ring = cadmpeg_ir::topology::LoopRing::new(ring_members, Vec::new()).map_err(
            |error| {
                cadmpeg_core::CodecError::malformed(format_args!(
                    "generated periodic seam ring is invalid: {error}"
                ))
            },
        )?;
        for id in &ring_ids {
            if let Some(coedge_index) = coedge_indices.get(id) {
                out.coedges[*coedge_index].owner_loop = loop_a.clone();
            }
        }
        if let Some(lp) = out.loops.iter_mut().find(|lp| lp.id == loop_a) {
            lp.boundary = cadmpeg_ir::topology::LoopBoundary::Ring(ring);
        }
        if let Some(face) = out.faces.iter_mut().find(|face| face.id == face_id) {
            face.loops = cadmpeg_ir::topology::FaceLoops::unspecified(
                ctx.alloc_filled(1, loop_a, "bind Parasolid cylinder seam loop")?
            );
        }
        reserve_graph_set_key(ctx, &mut removed, &loop_b, "track replaced Parasolid seam loops")?;
        removed.insert(loop_b);
    }
    out.loops.retain(|lp| !removed.contains(&lp.id));
    Ok(())
}

fn synthesize_sphere_seams(
    ctx: &DecodeContext<'_>,
    out: &mut Brep,
    annotations: &mut AnnotationBuilder,
    source_stream: &cadmpeg_ir::annotations::StreamHandle,
) -> Result<(), cadmpeg_core::CodecError> {
    let surface_geometry = collect_graph_map(ctx,
        out.surfaces.iter().map(|surface| (&surface.id, &surface.geometry)),
        "index Parasolid sphere geometry")?;
    let loop_coedges = collect_graph_map(ctx,
        out.loops.iter().map(|lp| (&lp.id, lp.coedges())),
        "index Parasolid sphere loop coedges")?;
    let coedge_edges = collect_graph_map(ctx,
        out.coedges.iter().map(|coedge| (&coedge.id, &coedge.edge)),
        "index Parasolid sphere coedge edges")?;
    let edge_indices = collect_graph_map(ctx,
        out.edges.iter().enumerate().map(|(index, edge)| (&edge.id, index)),
        "index Parasolid sphere edges")?;
    let curve_geometry = collect_graph_map(ctx,
        out.curves.iter().map(|curve| (&curve.id, &curve.geometry)),
        "index Parasolid sphere curves")?;
    let vertex_points = collect_graph_map(ctx,
        out.vertices.iter().filter_map(|vertex| {
            out.points.iter().find(|point| point.id == vertex.point)
                .map(|point| (&vertex.id, point.position().get()))
        }), "index Parasolid sphere vertex points")?;
    let mut existing = Vec::new();
    for face in &out.faces {
        let Some(SurfaceGeometry::Solved(SolvedSurfaceGeometry::Sphere(sphere_surface))) =
            surface_geometry.get(&face.surface).copied()
        else {
            continue;
        };
        let center = sphere_surface.center().get();
        let axis = sphere_surface.frame().axis().as_raw();
        let radius = sphere_surface.radius().get();
        let mut face_loops = face.loops.iter();
        let (Some(loop_id), None) = (face_loops.next(), face_loops.next()) else {
            continue;
        };
        let Some(coedge_ids) = loop_coedges.get(loop_id).copied() else {
            continue;
        };
        if coedge_ids.len() != 4 {
            continue;
        }
        let mut seam_edges = Vec::new();
        for coedge in coedge_ids {
            ctx.charge_work(1, "select Parasolid sphere seam edges")?;
            if let Some(index) = coedge_edges.get(coedge).and_then(|edge| edge_indices.get(*edge)) {
                if out.edges[*index].curve().is_none() {
                    ctx.reserve_collection_vec(&mut seam_edges, 1, "collect Parasolid sphere seam edges")?;
                    seam_edges.push(*index);
                }
            }
        }
        let circle_count = coedge_ids
            .iter()
            .filter_map(|coedge| coedge_edges.get(coedge).copied())
            .filter_map(|edge| edge_indices.get(edge).copied())
            .filter(|index| {
                out.edges[*index]
                    .curve()
                    .and_then(|curve| curve_geometry.get(curve))
                    .is_some_and(|geometry| {
                        matches!(
                            geometry,
                            CurveGeometry::Solved(SolvedCurveGeometry::Circle(_))
                        )
                    })
            })
            .count();
        if let [edge_index] = seam_edges.as_slice() {
            if circle_count != 3 {
                continue;
            }
            let north = cadmpeg_ir::math::Point3::new(
                center.x + radius * axis.x,
                center.y + radius * axis.y,
                center.z + radius * axis.z,
            );
            // The analytic sphere axis fixes the seam pole. An existing
            // endpoint is a topology carrier, not a pole selector; choosing
            // the nearer pole lets a stale or reversed endpoint change the
            // sphere parameterization.
            let point = north;
            ctx.reserve_collection_vec(&mut existing, 1, "collect Parasolid sphere seam repairs")?;
            existing.push((*edge_index, point));
        }
    }
    for (edge_index, point) in existing {
        let Ok(degenerate) = cadmpeg_ir::geometry::analytic::DegenerateCurve::try_new(point) else {
            continue;
        };

        let seam_vertices = [
            out.edges[edge_index].start.clone(),
            out.edges[edge_index].end.clone(),
        ];
        for vertex_id in seam_vertices {
            let Some(point_id) = out
                .vertices
                .iter()
                .find(|vertex| vertex.id == vertex_id)
                .map(|vertex| vertex.point.clone())
            else {
                continue;
            };
            if let Some(vertex_point) = out.points.iter_mut().find(|item| item.id == point_id) {
                vertex_point.set_position(degenerate.point());
            }
        }
        let curve_id = CurveId::compose(
            &cadmpeg_ir::identity_namespace!("sldprt", "brep", "curve"),
            cadmpeg_ir::identity_key!("sphere-seam:").then(out.edges[edge_index].id.key()),
        );
        annotations
            .note(curve_id.as_str(), source_stream, 0)
            .tag("derived_sphere_seam");
        annotations.exactness(curve_id.as_str(), Exactness::Derived);
        admit_brep_entity(ctx)?;
        ctx.reserve_collection_vec(&mut out.curves, 1, "collect repaired Parasolid sphere seam curves")?;
        out.curves.push(Curve {
            id: curve_id.clone(),
            source_object: None,
            geometry: CurveGeometry::Solved(SolvedCurveGeometry::Degenerate(degenerate)),
        });
        out.edges[edge_index]
            .set_curve(Some(curve_id))
            .map_err(cadmpeg_core::CodecError::malformed)?;
    }

    let surfaces = collect_graph_map(ctx,
        out.surfaces.iter().map(|surface| (&surface.id, surface)),
        "index Parasolid pcurve surfaces")?;
    let loops = collect_graph_map(ctx, out.loops.iter().map(|lp| (&lp.id, lp)),
        "index Parasolid seam loops")?;
    let coedges = collect_graph_map(ctx,
        out.coedges.iter().map(|coedge| (&coedge.id, coedge)),
        "index Parasolid seam coedges")?;
    let edges = collect_graph_map(ctx, out.edges.iter().map(|edge| (&edge.id, edge)),
        "index Parasolid pcurve edges")?;
    let curves = collect_graph_map(ctx, out.curves.iter().map(|curve| (&curve.id, curve)),
        "index Parasolid pcurve curves")?;
    let mut candidates = Vec::new();
    for (face_index, face) in out.faces.iter().enumerate() {
        let Some(surface) = surfaces.get(&face.surface) else {
            continue;
        };
        let Some(SolvedSurfaceGeometry::Sphere(sphere_surface)) = surface.geometry.solved() else {
            continue;
        };
        let center = sphere_surface.center().get();
        let radius = sphere_surface.radius().get();
        if face.loops.len() != 1 {
            continue;
        }
        let Some(lp) = face.loops.iter().next().and_then(|id| loops.get(id)) else {
            continue;
        };
        if lp.coedges().len() != 3 {
            continue;
        }
        let all_circles = lp.coedges().iter().all(|id| {
            coedges
                .get(id)
                .and_then(|coedge| edges.get(&coedge.edge))
                .and_then(|edge| edge.curve())
                .is_some_and(|curve_id| {
                    curves.get(curve_id).is_some_and(|curve| {
                        matches!(
                            curve.geometry,
                            CurveGeometry::Solved(SolvedCurveGeometry::Circle(_))
                        )
                    })
                })
        });
        let Some(SolvedSurfaceGeometry::Sphere(sphere_surface)) = surface.geometry.solved() else {
            continue;
        };
        let axis = *sphere_surface.frame().axis().as_raw();
        if all_circles {
            let seam_point = cadmpeg_ir::math::Point3::new(
                center.x + radius * axis.x,
                center.y + radius * axis.y,
                center.z + radius * axis.z,
            );
            let pole_candidates = lp
                .coedges()
                .iter()
                .filter_map(|id| coedges.get(id))
                .filter_map(|coedge| edges.get(&coedge.edge))
                .flat_map(|edge| [&edge.start, &edge.end])
                .filter(|vertex| {
                    vertex_points.get(vertex).is_some_and(|point| {
                        let dx = point.x - seam_point.x;
                        let dy = point.y - seam_point.y;
                        let dz = point.z - seam_point.z;
                        dx * dx + dy * dy + dz * dz <= EPS_POINT_DISTANCE
                    })
                });
            let mut pole_vertices = Vec::new();
            for vertex in pole_candidates {
                ctx.reserve_collection_vec(&mut pole_vertices, 1, "collect Parasolid sphere pole vertices")?;
                pole_vertices.push(vertex.clone());
            }
            pole_vertices.sort_by(|left, right| left.as_str().cmp(right.as_str()));
            pole_vertices.dedup();
            let mut ring = Vec::new();
            ctx.reserve_collection_vec(&mut ring, lp.coedges().len(), "copy Parasolid sphere seam ring")?;
            ring.extend_from_slice(lp.coedges());
            ctx.reserve_collection_vec(&mut candidates, 1, "collect Parasolid sphere seam candidates")?;
            candidates.push((
                face_index,
                face.id.clone(),
                lp.id.clone(),
                ring,
                seam_point,
                pole_vertices.first().cloned(),
            ));
        }
    }
    let mut coedge_indices = collect_graph_map(ctx,
        out.coedges.iter().enumerate().map(|(index, coedge)| (coedge.id.clone(), index)),
        "index Parasolid seam coedges")?;
    for (face_index, _face, loop_id, mut ring, seam_point, pole_vertex) in candidates {
        let Ok(degenerate) = cadmpeg_ir::geometry::analytic::DegenerateCurve::try_new(seam_point)
        else {
            continue;
        };
        let Ok(pcurve) = cadmpeg_ir::geometry::pcurve::LinePcurve::try_new(
            cadmpeg_ir::math::Point2::new(0.0, std::f64::consts::FRAC_PI_2),
            cadmpeg_ir::math::Point2::new(1.0, 0.0),
        ) else {
            continue;
        };

        let seam_face_key = cadmpeg_ir::identity_key!("sphere-seam-face:").then(face_index);
        let curve_id = CurveId::compose(
            &cadmpeg_ir::identity_namespace!("sldprt", "brep", "curve"),
            seam_face_key.clone(),
        );
        let edge_id = EdgeId::compose(
            &cadmpeg_ir::identity_namespace!("sldprt", "brep", "edge"),
            seam_face_key.clone(),
        );
        let coedge_id = CoedgeId::compose(
            &cadmpeg_ir::identity_namespace!("sldprt", "brep", "coedge"),
            seam_face_key.clone(),
        );
        let pcurve_id = PcurveId::compose(&pcurve_namespace(), seam_face_key.clone());
        let pole_vertex = match pole_vertex {
            Some(vertex) => vertex,
            None => {
                let point_id = PointId::compose(
                    &cadmpeg_ir::identity_namespace!("sldprt", "brep", "point"),
                    seam_face_key.clone(),
                );
                let vertex_id = VertexId::compose(
                    &cadmpeg_ir::identity_namespace!("sldprt", "brep", "vertex"),
                    seam_face_key.clone(),
                );
                for id in [point_id.as_str(), vertex_id.as_str()] {
                    annotations
                        .note(id, source_stream, 0)
                        .tag("derived_sphere_seam");
                    annotations.exactness(id, Exactness::Derived);
                }
                admit_brep_entity(ctx)?;
                ctx.reserve_collection_vec(&mut out.points, 1, "collect Parasolid sphere seam points")?;
                out.points
                    .push(Point::new(point_id.clone(), degenerate.point(), None));
                admit_brep_entity(ctx)?;
                ctx.reserve_collection_vec(&mut out.vertices, 1, "collect Parasolid sphere seam vertices")?;
                out.vertices.push(Vertex {
                    id: vertex_id.clone(),
                    point: point_id,
                    tolerance: None,
                });
                vertex_id
            }
        };
        for id in [
            curve_id.as_str(),
            edge_id.as_str(),
            coedge_id.as_str(),
            pcurve_id.as_str(),
        ] {
            annotations
                .note(id, source_stream, 0)
                .tag("derived_sphere_seam");
            annotations.exactness(id, Exactness::Derived);
        }
        admit_brep_entity(ctx)?;
        ctx.reserve_collection_vec(&mut out.curves, 1, "collect Parasolid sphere seam curves")?;
        out.curves.push(Curve {
            id: curve_id.clone(),
            source_object: None,
            geometry: CurveGeometry::Solved(SolvedCurveGeometry::Degenerate(degenerate)),
        });
        admit_brep_entity(ctx)?;
        ctx.reserve_collection_vec(&mut out.edges, 1, "collect Parasolid sphere seam edges")?;
        out.edges.push(Edge {
            id: edge_id.clone(),
            carrier: cadmpeg_ir::topology::EdgeCarrier::unbounded(Some(curve_id)),
            start: pole_vertex.clone(),
            end: pole_vertex,
            tolerance: None,
        });
        let Some(full_turn) = cadmpeg_ir::units::FiniteVector::new([0.0, std::f64::consts::TAU])
        else {
            continue;
        };
        admit_brep_entity(ctx)?;
        ctx.reserve_collection_vec(&mut out.pcurves, 1, "collect Parasolid sphere seam pcurves")?;
        out.pcurves.push(Pcurve {
            id: pcurve_id.clone(),
            geometry: PcurveGeometry::Line(pcurve),
            metadata: cadmpeg_ir::geometry::pcurve::PcurveMetadata::general(
                None,
                Some(full_turn),
                None,
            ),
        });
        ctx.reserve_collection_vec(&mut ring, 1, "extend Parasolid sphere seam ring")?;
        ring.push(coedge_id.clone());
        reserve_graph_map_key(ctx, &mut coedge_indices, &coedge_id, "index generated Parasolid sphere coedges")?;
        coedge_indices.insert(coedge_id.clone(), out.coedges.len());
        let mut pcurve_uses = Vec::new();
        ctx.reserve_collection_vec(&mut pcurve_uses, 1, "bind Parasolid sphere seam pcurve")?;
        pcurve_uses.push(cadmpeg_ir::topology::PcurveUse {
            pcurve: pcurve_id,
            isoparametric: None,
            parameter_range: Some(
                cadmpeg_ir::geometry::DirectedParameterRange::new([0.0, std::f64::consts::TAU])
                    .map_err(cadmpeg_core::CodecError::malformed)?,
            ),
        });
        admit_brep_entity(ctx)?;
        ctx.reserve_collection_vec(&mut out.coedges, 1, "collect Parasolid sphere seam coedges")?;
        out.coedges.push(Coedge {
            id: coedge_id.clone(),
            owner_loop: loop_id.clone(),
            edge: edge_id,
            radial_next: coedge_id.clone(),
            sense: Sense::Forward,
            use_curve: None,
            pcurves: pcurve_uses,
        });
        let ring = cadmpeg_ir::topology::LoopRing::new(ring, Vec::new()).map_err(|error| {
            cadmpeg_core::CodecError::malformed(format_args!(
                "generated sphere seam ring is invalid: {error}"
            ))
        })?;
        if let Some(lp) = out.loops.iter_mut().find(|lp| lp.id == loop_id) {
            lp.boundary = cadmpeg_ir::topology::LoopBoundary::Ring(ring);
        }
    }
    Ok(())
}

fn emit_curve(
    ctx: &DecodeContext<'_>,
    out: &mut Brep,
    carrier: &CurveCarrier,
) -> Result<(), cadmpeg_core::CodecError> {
    admit_brep_entity(ctx)?;
    ctx.reserve_collection_vec(&mut out.curves, 1, "collect Parasolid curves")?;
    out.curves.push(Curve {
        id: id_curve(carrier.attr),
        source_object: None,
        geometry: copy_curve_carrier_geometry(ctx, &carrier.geometry)?,
    });
    Ok(())
}

#[cfg(test)]
mod tests {
    fn one_face_shell() -> super::Brep {
        use cadmpeg_ir::ids::{FaceId, ShellId, SurfaceId};
        use cadmpeg_ir::topology::{Face, FaceLoops, Sense};

        super::Brep {
            faces: vec![Face {
                id: FaceId::mint("test:model:face#1").expect("face id"),
                shell: ShellId::mint("test:model:shell#1").expect("shell id"),
                surface: SurfaceId::mint("test:model:surface#1").expect("surface id"),
                sense: Sense::Forward,
                loops: FaceLoops::unspecified(Vec::new()),
                name: None,
                color: None,
                tolerance: None,
            }],
            ..Default::default()
        }
    }

    #[test]
    fn shell_components_refuse_collection_limit() {
        use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};

        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_collection_items = 0;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("root");
        assert!(matches!(
            super::shell_face_components(&ctx, &one_face_shell(), "test:model:shell#1"),
            Err(cadmpeg_core::CodecError::ResourceLimit(_))
        ));
    }

    #[test]
    fn shell_components_refuse_work_limit() {
        use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};

        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_work_units = 0;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("root");
        assert!(matches!(
            super::shell_face_components(&ctx, &one_face_shell(), "test:model:shell#1"),
            Err(cadmpeg_core::CodecError::ResourceLimit(_))
        ));
    }

    #[test]
    fn numerical_followup_inverse_ambiguity_is_independent_of_parameter_units() {
        for domain in [1e-200, 1e-12, 1.0, 1e200] {
            assert!(matches!(
                super::unique_inverse_parameter(
                    vec![(0.25 * domain, 0.), (0.75 * domain, 0.)],
                    0.001,
                    [0., domain]
                ),
                super::InverseResolution::Ambiguous
            ));
            assert!(matches!(
                super::unique_inverse_parameter(
                    vec![(0.25 * domain, 0.), (0.25 * domain, 0.)],
                    0.001,
                    [0., domain]
                ),
                super::InverseResolution::Unique(_)
            ));
        }
    }

    use super::sphere_latitude;
    use super::unique_face_colors;
    use crate::brep::entity;
    use crate::brep::topology::{Bridge, Coedge, EdgeReferences, EdgeUse, Loop, Tables};
    use cadmpeg_ir::geometry::{SolvedCurveGeometry, SolvedSurfaceGeometry};
use cadmpeg_ir::topology::Color;
use cadmpeg_ir::topology::Sense;

fn with_test_context<T>(f: impl FnOnce(&cadmpeg_core::decode::DecodeContext<'_>) -> T) -> T {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};

    let arena = DecodeArena::new();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &DecodePolicy::service())
        .expect("test context");
    f(&ctx)
}

    fn intersection_support_pcurve(
        support_data: &crate::brep::intersection::IntersectionSupportData,
        chart: &cadmpeg_ir::geometry::nurbs::NurbsCurve,
        surface_attr: u16,
        surface: &cadmpeg_ir::geometry::SurfaceGeometry,
        edge_endpoints: [cadmpeg_ir::math::Point3; 2],
        refusal: &mut crate::lane_refusal::LaneRefusals,
    ) -> Result<Option<(super::PcurveGeometry, [f64; 2], super::IntersectionPcurveSource)>, cadmpeg_core::CodecError> {
        let arena = cadmpeg_core::decode::DecodeArena::new();
        let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(
            &[], &arena, &cadmpeg_core::decode::DecodePolicy::service(),
        ).expect("empty test root fits service policy");
        super::intersection_support_pcurve(
            &ctx, support_data, chart, surface_attr, surface, edge_endpoints, refusal,
        )
    }

    fn test_nurbs_curve(
        degree: u32,
        knots: Vec<f64>,
        control_points: Vec<cadmpeg_ir::math::Point3>,
        weights: Option<Vec<f64>>,
    ) -> cadmpeg_ir::geometry::nurbs::NurbsCurve {
        cadmpeg_ir::geometry::nurbs::NurbsCurve::from_lanes(
            degree,
            knots,
            control_points,
            weights,
            false,
        )
        .expect("valid test NURBS curve")
    }

    // The fixture helper states each independent NURBS grid parameter explicitly.
    #[allow(clippy::too_many_arguments)]
    fn test_nurbs_surface(
        u_degree: u32,
        v_degree: u32,
        u_knots: Vec<f64>,
        v_knots: Vec<f64>,
        _u_count: u32,
        v_count: u32,
        control_points: &[cadmpeg_ir::math::Point3],
        weights: Option<Vec<f64>>,
    ) -> cadmpeg_ir::geometry::nurbs::NurbsSurface {
        cadmpeg_ir::geometry::nurbs::NurbsSurface::from_lanes(
            cadmpeg_ir::geometry::nurbs::NurbsSurfaceAxis::new(u_degree, u_knots, false),
            cadmpeg_ir::geometry::nurbs::NurbsSurfaceAxis::new(v_degree, v_knots, false),
            cadmpeg_ir::geometry::nurbs::NurbsSurfaceLanes::new(
                control_points
                    .chunks(v_count as usize)
                    .map(<[_]>::to_vec)
                    .collect(),
                weights.map(|values| values.chunks(v_count as usize).map(<[_]>::to_vec).collect()),
            ),
            false,
        )
        .expect("valid test NURBS surface")
    }

    #[test]
    fn sphere_latitude_refuses_a_circle_plane_beyond_the_pole() {
        use std::f64::consts::FRAC_PI_2;
        // Planes the sphere carries: the equator and the pole.
        assert_eq!(sphere_latitude(0.0, 2.0), Some(0.0));
        assert_eq!(sphere_latitude(2.0, 2.0), Some(FRAC_PI_2));
        // The rounding the radius match admits reaches the pole and no further.
        assert_eq!(sphere_latitude(2.0 + 1.0e-9, 2.0), Some(FRAC_PI_2));
        // A plane beyond that band states a circle on another sphere. The
        // radius match alone admits it, because a circle radius near zero
        // matches the saturated `sqrt(radius^2 - height^2)`.
        assert_eq!(sphere_latitude(2.1, 2.0), None);
        // A signed radius keeps the signed ratio.
        assert_eq!(sphere_latitude(2.0, -2.0), Some(-FRAC_PI_2));
    }

    #[test]
    fn line_edge_parameters_convert_from_metres_to_millimetres() {
        let carrier = crate::brep::CurveCarrier {
            attr: 1,
            offset: 0,
            end: 0,
            geometry: cadmpeg_ir::geometry::CurveGeometry::Solved(SolvedCurveGeometry::Line(
                cadmpeg_ir::geometry::analytic::LineCurve::try_new(
                    cadmpeg_ir::math::Point3::new(0.0, 17.5, 0.0),
                    cadmpeg_ir::math::Vector3::new(0.0, -1.0, 0.0),
                )
                .unwrap(),
            )),
            parameter_range: Some(
                cadmpeg_ir::units::FiniteVector::new([-0.014, 0.0165])
                    .expect("finite fixture range"),
            ),
        };

        let endpoints = [
            cadmpeg_ir::math::Point3::new(0.0, 1.0, 0.0),
            cadmpeg_ir::math::Point3::new(0.0, 31.5, 0.0),
        ];
        assert_eq!(
            super::edge_parameter_range(&carrier, Some(endpoints)).unwrap(),
            Some(([-14.0, 16.5], true))
        );
    }

    fn bridge_record(attr: u16, refs: [u16; 5]) -> Bridge {
        Bridge {
            attr,
            sequence: 0,
            refs,
            sense: Sense::Forward,
            owner: None,
            offset: 0,
        }
    }

    fn loop_record(attr: u16, refs: [u16; 4]) -> Loop {
        Loop {
            attr,
            refs,
            offset: 0,
        }
    }

    fn coedge_record(attr: u16, refs: [u16; 9]) -> Coedge {
        Coedge {
            attr,
            refs,
            sense: Sense::Forward,
            offset: 0,
        }
    }

    #[test]
    fn face_walk_rejects_a_loop_owned_by_another_bridge() {
        let bridge = bridge_record(10, [0, 0, 20, 0, 30]);
        let mut tables = Tables::default();
        let arena = cadmpeg_core::decode::DecodeArena::new();
        let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(
            &[], &arena, &cadmpeg_core::decode::DecodePolicy::service(),
        ).expect("test context");
        tables.insert_loop(&ctx, loop_record(20, [0, 40, 11, 0])).expect("loop");
        tables.insert_coedge(&ctx, coedge_record(40, [0, 0, 0, 40, 0, 0, 0, 0, 0])).expect("coedge");

        let face = super::walk_face(&ctx, &bridge, &tables).expect("face walk");

        assert!(face.loops.is_empty());
    }

    #[test]
    fn face_walk_rejects_a_ring_owned_by_another_loop() {
        let bridge = bridge_record(10, [0, 0, 20, 0, 30]);
        let mut tables = Tables::default();
        let arena = cadmpeg_core::decode::DecodeArena::new();
        let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(
            &[], &arena, &cadmpeg_core::decode::DecodePolicy::service(),
        ).expect("test context");
        tables.insert_loop(&ctx, loop_record(20, [0, 40, 10, 0])).expect("loop");
        tables.insert_coedge(&ctx, coedge_record(40, [0, 21, 0, 40, 0, 0, 0, 0, 0])).expect("coedge");

        let face = super::walk_face(&ctx, &bridge, &tables).expect("face walk");

        assert!(face.loops.is_empty());
    }

    #[test]
    fn face_walk_refuses_collection_limit() {
        use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};

        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_collection_items = 0;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("root");
        let bridge = bridge_record(10, [0, 0, 20, 0, 30]);
        assert!(matches!(
            super::walk_face(&ctx, &bridge, &Tables::default()),
            Err(cadmpeg_core::CodecError::ResourceLimit(_))
        ));
    }

    #[test]
    fn face_walk_refuses_work_limit() {
        use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};

        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_work_units = 0;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("root");
        let bridge = bridge_record(10, [0, 0, 20, 0, 30]);
        assert!(matches!(
            super::walk_face(&ctx, &bridge, &Tables::default()),
            Err(cadmpeg_core::CodecError::ResourceLimit(_))
        ));
    }

    fn face_color(
        face_attr: u16,
        color_attr: u16,
        face_seq: u32,
        rgb: [f32; 3],
    ) -> entity::FaceColor {
        entity::FaceColor {
            face_attr,
            color_attr,
            face_seq,
            stream_order: 0,
            color: Color::new(rgb[0], rgb[1], rgb[2], 1.0).expect("valid color"),
            offset: usize::from(face_attr),
            target: None,
        }
    }

    fn face_color_version(
        face_attr: u16,
        seq: u32,
        stream_order: usize,
    ) -> entity::FaceColorVersion {
        entity::FaceColorVersion {
            face_attr,
            seq,
            stream_order,
        }
    }

    #[test]
    fn current_uncolored_face_version_removes_an_older_color() {
        let colors = vec![face_color(700, 900, 1, [0.25, 0.5, 0.75])];

        let (resolved, unresolved) = with_test_context(|ctx| {
            unique_face_colors(
                ctx,
                colors,
                vec![face_color_version(700, 1, 0), face_color_version(700, 2, 0)],
            ).expect("face colors")
        });

        assert!(resolved.is_empty());
        assert_eq!(unresolved, 0);
    }

    #[test]
    fn later_stream_replaces_an_equal_sequence_face_color() {
        let old = face_color(700, 900, 2, [0.25, 0.5, 0.75]);
        let mut current = face_color(700, 901, 2, [0.75, 0.5, 0.25]);
        current.stream_order = 1;

        let (resolved, unresolved) = with_test_context(|ctx| {
            unique_face_colors(
                ctx,
                vec![old, current],
                vec![face_color_version(700, 2, 0), face_color_version(700, 2, 1)],
            ).expect("face colors")
        });

        assert_eq!(resolved.len(), 1);
        assert_eq!(resolved[0].color_attr, 901);
        assert_eq!(unresolved, 0);
    }

    #[test]
    fn conflicting_current_face_colors_remain_unresolved() {
        let colors = vec![
            face_color(700, 900, 2, [0.25, 0.5, 0.75]),
            face_color(700, 901, 2, [0.75, 0.5, 0.25]),
        ];

        let (resolved, unresolved) = with_test_context(|ctx| {
            unique_face_colors(
                ctx,
                colors,
                vec![face_color_version(700, 2, 0), face_color_version(700, 2, 0)],
            ).expect("face colors")
        });

        assert!(resolved.is_empty());
        assert_eq!(unresolved, 1);
    }

    #[test]
    fn conflicting_reuse_of_one_color_identity_remains_unresolved() {
        let colors = vec![
            face_color(700, 900, 2, [0.25, 0.5, 0.75]),
            face_color(701, 900, 2, [0.75, 0.5, 0.25]),
        ];

        let (resolved, unresolved) = with_test_context(|ctx| {
            unique_face_colors(
                ctx,
                colors,
                vec![face_color_version(700, 2, 0), face_color_version(701, 2, 0)],
            ).expect("face colors")
        });

        assert!(resolved.is_empty());
        assert_eq!(unresolved, 2);
    }

    #[test]
    fn intersection_uv_converts_length_parameters_and_exact_endpoints() {
        let surface =
            cadmpeg_ir::geometry::SurfaceGeometry::Solved(SolvedSurfaceGeometry::Cylinder(
                cadmpeg_ir::geometry::analytic::CylinderSurface::try_new(
                    cadmpeg_ir::math::Point3::new(0.0, 0.0, 0.0),
                    cadmpeg_ir::math::Vector3::new(0.0, 0.0, 1.0),
                    cadmpeg_ir::math::Vector3::new(1.0, 0.0, 0.0),
                    2.0,
                )
                .unwrap(),
            ));
        let endpoints = [
            cadmpeg_ir::eval::surface_point(&surface, 0.0, 3.0)
                .expect("cylinder start")
                .get(),
            cadmpeg_ir::eval::surface_point(&surface, 0.5, 2.0)
                .expect("cylinder end")
                .get(),
        ];
        let chart = test_nurbs_curve(1, vec![0.0, 0.0, 1.0, 1.0], endpoints.to_vec(), None);
        let support_data = super::super::intersection::IntersectionSupportData {
            supports: [10, 11],
            fit_tolerance_mm: 0.2,
            support_uv: Some([
                vec![
                    cadmpeg_ir::math::Point2::new(0.0, 0.0029),
                    cadmpeg_ir::math::Point2::new(0.5, 0.0018),
                ],
                vec![
                    cadmpeg_ir::math::Point2::new(0.0, 0.0),
                    cadmpeg_ir::math::Point2::new(1.0, 0.0),
                ],
            ]),
        };
        let (geometry, range, source) = intersection_support_pcurve(
            &support_data,
            &chart,
            10,
            &surface,
            endpoints,
            &mut crate::lane_refusal::LaneRefusals::new(),
        )
        .expect("resource allocation did not fail")
        .expect("support parameterization");
        let cadmpeg_ir::geometry::pcurve::PcurveGeometry::Nurbs { nurbs } = geometry else {
            panic!("expected solved UV NURBS");
        };
        assert_eq!(
            nurbs.control_points()[0],
            cadmpeg_ir::math::Point2::new(0.0, 3.0)
        );
        assert_eq!(
            nurbs.control_points()[1],
            cadmpeg_ir::math::Point2::new(0.5, 2.0)
        );
        assert_eq!(range, [0.0, 1.0]);
        assert_eq!(source, super::IntersectionPcurveSource::StoredCache);

        let ambiguous = super::super::intersection::IntersectionSupportData {
            supports: [10, 10],
            ..support_data.clone()
        };
        assert!(intersection_support_pcurve(
            &ambiguous,
            &chart,
            10,
            &surface,
            endpoints,
            &mut crate::lane_refusal::LaneRefusals::new()
        )
        .expect("resource allocation did not fail")
        .is_none());

        let malformed = super::super::intersection::IntersectionSupportData {
            support_uv: Some([Vec::new(), Vec::new()]),
            ..support_data
        };
        assert!(intersection_support_pcurve(
            &malformed,
            &chart,
            10,
            &surface,
            endpoints,
            &mut crate::lane_refusal::LaneRefusals::new()
        )
        .expect("resource allocation did not fail")
        .is_none());
    }

    #[test]
    fn intersection_support_pcurve_refuses_collection_limit() {
        let surface = cadmpeg_ir::geometry::SurfaceGeometry::Solved(
            SolvedSurfaceGeometry::Cylinder(
                cadmpeg_ir::geometry::analytic::CylinderSurface::try_new(
                    cadmpeg_ir::math::Point3::new(0.0, 0.0, 0.0),
                    cadmpeg_ir::math::Vector3::new(0.0, 0.0, 1.0),
                    cadmpeg_ir::math::Vector3::new(1.0, 0.0, 0.0),
                    2.0,
                ).expect("valid cylinder"),
            ),
        );
        let endpoints = [
            cadmpeg_ir::eval::surface_point(&surface, 0.0, 3.0).expect("start").get(),
            cadmpeg_ir::eval::surface_point(&surface, 0.5, 2.0).expect("end").get(),
        ];
        let chart = test_nurbs_curve(1, vec![0.0, 0.0, 1.0, 1.0], endpoints.to_vec(), None);
        let support_data = super::super::intersection::IntersectionSupportData {
            supports: [10, 11],
            fit_tolerance_mm: 0.2,
            support_uv: Some([
                vec![cadmpeg_ir::math::Point2::new(0.0, 0.0029), cadmpeg_ir::math::Point2::new(0.5, 0.0018)],
                vec![cadmpeg_ir::math::Point2::new(0.0, 0.0), cadmpeg_ir::math::Point2::new(1.0, 0.0)],
            ]),
        };
        let arena = cadmpeg_core::decode::DecodeArena::new();
        let mut policy = cadmpeg_core::decode::DecodePolicy::service();
        policy.limits.max_collection_items = 1;
        let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(
            &[], &arena, &policy,
        ).expect("empty root fits policy");
        let Err(cadmpeg_core::CodecError::ResourceLimit(limit)) = super::intersection_support_pcurve(
            &ctx, &support_data, &chart, 10, &surface, endpoints,
            &mut crate::lane_refusal::LaneRefusals::new(),
        ) else { panic!("two UV controls exceed one collection item") };
        assert_eq!(limit.dimension, cadmpeg_core::decode::ResourceDimension::CollectionItems);
        assert!(intersection_support_pcurve(
            &support_data, &chart, 10, &surface, endpoints,
            &mut crate::lane_refusal::LaneRefusals::new(),
        ).expect("service policy").is_some());
    }

    #[test]
    fn analytic_intersection_chart_derives_continuous_uv_without_cache() {
        let surface =
            cadmpeg_ir::geometry::SurfaceGeometry::Solved(SolvedSurfaceGeometry::Cylinder(
                cadmpeg_ir::geometry::analytic::CylinderSurface::try_new(
                    cadmpeg_ir::math::Point3::new(0.0, 0.0, 0.0),
                    cadmpeg_ir::math::Vector3::new(0.0, 0.0, 1.0),
                    cadmpeg_ir::math::Vector3::new(1.0, 0.0, 0.0),
                    2.0,
                )
                .unwrap(),
            ));
        let model_points = [(3.0, 1.0), (3.2, 2.0), (3.4, 3.0)]
            .map(|(u, v)| {
                cadmpeg_ir::eval::surface_point(&surface, u, v)
                    .expect("cylinder point")
                    .get()
            })
            .to_vec();
        let endpoints = [model_points[0], model_points[2]];
        let chart = test_nurbs_curve(1, vec![0.0, 0.0, 0.5, 1.0, 1.0], model_points, None);
        let support_data = super::super::intersection::IntersectionSupportData {
            supports: [10, 11],
            fit_tolerance_mm: 0.011,
            support_uv: None,
        };

        let (geometry, _, source) = intersection_support_pcurve(
            &support_data,
            &chart,
            10,
            &surface,
            endpoints,
            &mut crate::lane_refusal::LaneRefusals::new(),
        )
        .expect("resource allocation did not fail")
        .expect("analytic support inversion");
        let cadmpeg_ir::geometry::pcurve::PcurveGeometry::Nurbs { nurbs } = geometry else {
            panic!("expected solved UV NURBS");
        };
        for (point, expected) in
            nurbs
                .control_points()
                .iter()
                .zip([(3.0, 1.0), (3.2, 2.0), (3.4, 3.0)])
        {
            assert!((point.u - expected.0).abs() < 1.0e-12);
            assert!((point.v - expected.1).abs() < 1.0e-12);
        }
        assert_eq!(source, super::IntersectionPcurveSource::AnalyticInverse);

        let under_toleranced = super::super::intersection::IntersectionSupportData {
            fit_tolerance_mm: 0.009,
            ..support_data
        };
        assert!(intersection_support_pcurve(
            &under_toleranced,
            &chart,
            10,
            &surface,
            endpoints,
            &mut crate::lane_refusal::LaneRefusals::new()
        )
        .expect("resource allocation did not fail")
        .is_none());
    }

    #[test]
    fn analytic_torus_chart_unwraps_both_periodic_parameters() {
        let surface = cadmpeg_ir::geometry::SurfaceGeometry::Solved(SolvedSurfaceGeometry::Torus(
            cadmpeg_ir::geometry::analytic::TorusSurface::try_new(
                cadmpeg_ir::math::Point3::new(0.0, 0.0, 0.0),
                cadmpeg_ir::math::Vector3::new(0.0, 0.0, 1.0),
                cadmpeg_ir::math::Vector3::new(1.0, 0.0, 0.0),
                5.0,
                1.0,
            )
            .unwrap(),
        ));
        let expected = [(3.0, 3.0), (3.2, 3.2), (3.4, 3.4)];
        let model_points = expected
            .map(|(u, v)| {
                cadmpeg_ir::eval::surface_point(&surface, u, v)
                    .expect("torus point")
                    .get()
            })
            .to_vec();
        let endpoints = [model_points[0], model_points[2]];
        let chart = test_nurbs_curve(1, vec![0.0, 0.0, 0.5, 1.0, 1.0], model_points, None);
        let support_data = super::super::intersection::IntersectionSupportData {
            supports: [10, 11],
            fit_tolerance_mm: 0.08,
            support_uv: None,
        };

        let (geometry, _, _) = intersection_support_pcurve(
            &support_data,
            &chart,
            10,
            &surface,
            endpoints,
            &mut crate::lane_refusal::LaneRefusals::new(),
        )
        .expect("resource allocation did not fail")
        .expect("torus support inversion");
        let cadmpeg_ir::geometry::pcurve::PcurveGeometry::Nurbs { nurbs } = geometry else {
            panic!("expected solved UV NURBS");
        };
        for (point, expected) in nurbs.control_points().iter().zip(expected) {
            assert!((point.u - expected.0).abs() < 1.0e-12);
            assert!((point.v - expected.1).abs() < 1.0e-12);
        }
    }

    #[test]
    fn nurbs_intersection_chart_inverts_with_continuation_seeds() {
        let nurbs = test_nurbs_surface(
            1,
            1,
            vec![0.0, 0.0, 1.0, 1.0],
            vec![0.0, 0.0, 1.0, 1.0],
            2,
            2,
            &[
                cadmpeg_ir::math::Point3::new(0.0, 0.0, 0.0),
                cadmpeg_ir::math::Point3::new(0.0, 1.0, 0.0),
                cadmpeg_ir::math::Point3::new(1.0, 0.0, 0.0),
                cadmpeg_ir::math::Point3::new(1.0, 1.0, 0.0),
            ],
            None,
        );
        let surface = cadmpeg_ir::geometry::SurfaceGeometry::Solved(SolvedSurfaceGeometry::Nurbs(
            nurbs.clone(),
        ));
        let expected = [(0.2, 0.1), (0.5, 0.4), (0.8, 0.7)];
        let model_points = expected
            .map(|(u, v)| {
                cadmpeg_ir::eval::nurbs_surface_point(&nurbs, u, v)
                    .expect("surface point")
                    .get()
            })
            .to_vec();
        let endpoints = [model_points[0], model_points[2]];
        let chart = test_nurbs_curve(1, vec![0.0, 0.0, 0.5, 1.0, 1.0], model_points, None);
        let support_data = super::super::intersection::IntersectionSupportData {
            supports: [10, 11],
            fit_tolerance_mm: 1.0e-9,
            support_uv: None,
        };

        let (geometry, _, source) = intersection_support_pcurve(
            &support_data,
            &chart,
            10,
            &surface,
            endpoints,
            &mut crate::lane_refusal::LaneRefusals::new(),
        )
        .expect("resource allocation did not fail")
        .expect("NURBS support inversion");
        let cadmpeg_ir::geometry::pcurve::PcurveGeometry::Nurbs { nurbs } = geometry else {
            panic!("expected solved UV NURBS");
        };
        for (point, expected) in nurbs.control_points().iter().zip(expected) {
            assert!((point.u - expected.0).abs() < 1.0e-10);
            assert!((point.v - expected.1).abs() < 1.0e-10);
        }
        assert_eq!(source, super::IntersectionPcurveSource::NurbsInverse);
    }

    #[test]
    fn nurbs_intersection_chart_requires_a_complete_chord_certificate() {
        let nurbs = test_nurbs_surface(
            1,
            1,
            vec![0.0, 0.0, 1.0, 1.0],
            vec![0.0, 0.0, 1.0, 1.0],
            2,
            2,
            &[
                cadmpeg_ir::math::Point3::new(0.0, 0.0, 0.0),
                cadmpeg_ir::math::Point3::new(0.0, 1.0, 0.0),
                cadmpeg_ir::math::Point3::new(1.0, 0.0, 0.0),
                cadmpeg_ir::math::Point3::new(1.0, 1.0, 1.0),
            ],
            None,
        );
        let surface =
            cadmpeg_ir::geometry::SurfaceGeometry::Solved(SolvedSurfaceGeometry::Nurbs(nurbs));
        let endpoints = [
            cadmpeg_ir::math::Point3::new(0.0, 0.0, 0.0),
            cadmpeg_ir::math::Point3::new(1.0, 1.0, 1.0),
        ];
        let chart = test_nurbs_curve(1, vec![0.0, 0.0, 1.0, 1.0], endpoints.to_vec(), None);
        let support_data = |fit_tolerance_mm| super::super::intersection::IntersectionSupportData {
            supports: [10, 11],
            fit_tolerance_mm,
            support_uv: None,
        };

        assert!(intersection_support_pcurve(
            &support_data(0.3),
            &chart,
            10,
            &surface,
            endpoints,
            &mut crate::lane_refusal::LaneRefusals::new(),
        )
        .expect("resource allocation did not fail")
        .is_none());
        assert!(intersection_support_pcurve(
            &support_data(0.34),
            &chart,
            10,
            &surface,
            endpoints,
            &mut crate::lane_refusal::LaneRefusals::new(),
        )
        .expect("resource allocation did not fail")
        .is_some());
    }

    #[test]
    fn canonical_edge_direction_uses_explicit_or_unique_forward_coedge() {
        use std::collections::HashMap;

        let record = |attr, refs, sense| Coedge {
            attr,
            refs,
            sense,
            offset: 0,
        };
        let edge = |attr, references| EdgeUse {
            attr,
            references,
            sequence: 0,
            offset: 0,
        };
        let mut coedges = HashMap::from([
            (
                10,
                record(10, [0, 0, 0, 0, 101, 0, 7, 0, 0], Sense::Reversed),
            ),
            (
                11,
                record(11, [0, 0, 0, 0, 102, 10, 7, 0, 0], Sense::Forward),
            ),
        ]);
        let prefixed_edge = edge(7, EdgeReferences::Compact { curve: 300 });

        assert_eq!(
            super::canonical_coedge_attr(7, Some(&prefixed_edge), &coedges),
            Some(11)
        );

        let bare_edge = edge(7, EdgeReferences::Bare([11, 0, 0, 300, 0, 0]));
        assert_eq!(
            super::canonical_coedge_attr(7, Some(&bare_edge), &coedges),
            Some(11)
        );

        let sentinel_edge = edge(7, EdgeReferences::Bare([1, 0, 0, 300, 0, 0]));
        assert_eq!(
            super::canonical_coedge_attr(7, Some(&sentinel_edge), &coedges),
            Some(11)
        );

        let reversed_edge = edge(7, EdgeReferences::Bare([10, 0, 0, 300, 0, 0]));
        assert_eq!(
            super::canonical_coedge_attr(7, Some(&reversed_edge), &coedges),
            None
        );

        coedges.insert(
            12,
            record(12, [0, 0, 0, 0, 103, 0, 7, 0, 0], Sense::Forward),
        );
        assert_eq!(
            super::canonical_coedge_attr(7, Some(&prefixed_edge), &coedges),
            None
        );
    }

    #[test]
    fn boundary_coedge_uses_ring_endpoint_but_reciprocal_twin_supplies_edge_end() {
        use std::collections::HashMap;

        let record = |attr, refs| Coedge {
            attr,
            refs,
            sense: Sense::Forward,
            offset: 0,
        };
        let boundary = HashMap::from([(10, record(10, [0, 0, 0, 0, 101, 10, 7, 0, 0]))]);
        assert_eq!(super::edge_end_vuse(10, 102, &boundary), 102);

        let reciprocal = HashMap::from([
            (10, record(10, [0, 0, 0, 0, 101, 11, 7, 0, 0])),
            (11, record(11, [0, 0, 0, 0, 102, 10, 7, 0, 0])),
        ]);
        assert_eq!(super::edge_end_vuse(10, 103, &reciprocal), 102);
    }

    #[test]
    fn normalized_surface_parameter_reversal_toggles_face_sense() {
        use cadmpeg_ir::topology::Sense;

        assert_eq!(super::surface_sense(Sense::Forward, false), Sense::Forward);
        assert_eq!(
            super::surface_sense(Sense::Reversed, false),
            Sense::Reversed
        );
        assert_eq!(super::surface_sense(Sense::Forward, true), Sense::Reversed);
        assert_eq!(super::surface_sense(Sense::Reversed, true), Sense::Forward);
    }

    #[test]
    fn shared_edge_coedge_parity_orients_connected_faces() {
        use cadmpeg_ir::ids::{CoedgeId, EdgeId, FaceId, LoopId, ShellId, SurfaceId};
        use cadmpeg_ir::topology::{Coedge, Face, Loop, Sense};

        let face = |id: &str, lp: &str| Face {
            id: FaceId::mint(format!("test:model:entity#{id}")).expect("identity grammar"),
            shell: ShellId::mint("test:model:entity#shell").expect("identity grammar"),
            surface: SurfaceId::mint(format!("test:model:entity#surface-{id}"))
                .expect("identity grammar"),
            sense: Sense::Forward,
            loops: cadmpeg_ir::topology::FaceLoops::unspecified(vec![LoopId::mint(format!(
                "test:model:entity#{lp}"
            ))
            .expect("identity grammar")]),
            name: None,
            color: None,
            tolerance: None,
        };
        let lp = |id: &str, face: &str, coedge: &str| Loop {
            id: LoopId::mint(format!("test:model:entity#{id}")).expect("identity grammar"),
            face: FaceId::mint(format!("test:model:entity#{face}")).expect("identity grammar"),
            boundary: cadmpeg_ir::topology::LoopBoundary::Ring(
                cadmpeg_ir::topology::LoopRing::new(
                    vec![CoedgeId::mint(format!("test:model:entity#{coedge}"))
                        .expect("identity grammar")],
                    Vec::new(),
                )
                .expect("valid loop ring"),
            ),
        };
        let coedge = |id: &str, lp: &str, radial: &str, sense| Coedge {
            id: CoedgeId::mint(format!("test:model:entity#{id}")).expect("identity grammar"),
            owner_loop: LoopId::mint(format!("test:model:entity#{lp}")).expect("identity grammar"),
            edge: EdgeId::mint("test:model:entity#edge").expect("identity grammar"),
            radial_next: CoedgeId::mint(format!("test:model:entity#{radial}"))
                .expect("identity grammar"),
            sense,
            use_curve: None,
            pcurves: Vec::new(),
        };
        let mut brep = super::Brep {
            faces: vec![face("face-a", "loop-a"), face("face-b", "loop-b")],
            loops: vec![
                lp("loop-a", "face-a", "coedge-a"),
                lp("loop-b", "face-b", "coedge-b"),
            ],
            coedges: vec![
                coedge("coedge-a", "loop-a", "coedge-b", Sense::Forward),
                coedge("coedge-b", "loop-b", "coedge-a", Sense::Forward),
            ],
            ..Default::default()
        };

        with_test_context(|ctx| super::solve_face_orientation(ctx, &mut brep))
            .expect("orient first face pair");
        assert_eq!(brep.faces[0].sense, Sense::Forward);
        assert_eq!(brep.faces[1].sense, Sense::Reversed);

        brep.faces[1].sense = Sense::Reversed;
        brep.coedges[1].sense = Sense::Reversed;
        with_test_context(|ctx| super::solve_face_orientation(ctx, &mut brep))
            .expect("orient reversed face pair");
        assert_eq!(brep.faces[0].sense, Sense::Forward);
        assert_eq!(brep.faces[1].sense, Sense::Forward);
    }

    #[test]
    fn geometry_free_stream_does_not_report_synthetic_body_grouping() {
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(
        &[], &arena, &cadmpeg_core::decode::DecodePolicy::service(),
    ).unwrap();
        let decoded = super::decode_body(&ctx, &[], &cadmpeg_ir::stream_name!("empty"))
            .expect("valid exactness fields");

        assert!(decoded.faces.is_empty());
        assert!(!decoded.stats.synthetic_body_grouping);
    }

    #[test]
    fn native_brep_scan_candidates_refuse_collection_limit_before_parsing() {
        use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};

        let body = crate::test_support::parasolid::triangle_body();
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_collection_items = 0;
        let (ctx, _) = DecodeContext::from_root_bytes(&body, &arena, &policy).expect("root");
        let Err(error) = super::decode_body(
            &ctx,
            &body,
            &cadmpeg_ir::stream_name!("candidate-admission"),
        ) else {
            panic!("expected candidate admission refusal");
        };
        assert!(matches!(error,
            cadmpeg_core::CodecError::ResourceLimit(limit)
                if limit.dimension == ResourceDimension::CollectionItems
                    && limit.operation == "admit Parasolid scan candidates"));

        let arena = DecodeArena::new();
        let (ctx, _) =
            DecodeContext::from_root_bytes(&body, &arena, &DecodePolicy::service()).expect("root");
        super::decode_body(
            &ctx,
            &body,
            &cadmpeg_ir::stream_name!("candidate-admission"),
        )
        .expect("service profile admits native B-rep candidates");
    }

    #[test]
    fn typed_body_records_preserve_stored_sheet_kind_and_links() {
        use crate::brep::typed::{BodyNode, FaceNode, Facts, RegionNode, ShellNode};
        use cadmpeg_ir::topology::BodyKind;
        use std::collections::HashSet;

        let facts = Facts {
            bodies: vec![BodyNode {
                attr: 3,
                node_id: 7,
                topology_refs: [7, 8, 9, 10, 1, 12, 11],
                ownership_refs: Vec::new(),
                kind: BodyKind::Sheet,
                offset: 1,
                end: 2,
            }],
            shells: vec![ShellNode {
                attr: 7,
                node_id: 814,
                refs: [1, 3, 1, 38, 1, 1, 39, 1],
                offset: 3,
                end: 4,
            }],
            regions: vec![
                RegionNode {
                    attr: 11,
                    node_id: 244,
                    refs: [1, 3, 39, 1, 44],
                    offset: 5,
                    end: 6,
                },
                RegionNode {
                    attr: 39,
                    node_id: 815,
                    refs: [1, 3, 1, 11, 7],
                    offset: 7,
                    end: 8,
                },
            ],
            faces: vec![FaceNode {
                attr: 100,
                node_id: 900,
                refs: [1, 1, 49, 7, 8],
                sense: Sense::Forward,
                offset: 9,
                end: 10,
            }],
        };
        let mut tables = Tables::default();
        let arena = cadmpeg_core::decode::DecodeArena::new();
        let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(
            &[], &arena, &cadmpeg_core::decode::DecodePolicy::service(),
        ).expect("test context");
        tables.insert_bridge(&ctx, Bridge {
            attr: 100,
            sequence: 0,
            refs: [1, 1, 49, 7, 8],
            sense: Sense::Forward,
            owner: None,
            offset: 11,
        }).expect("bridge");

        let records = super::typed_body_records(&ctx, &facts, &tables)
            .expect("body record allocation")
            .expect("typed body records");
        assert_eq!(records.len(), 1);
        assert_eq!(records[0].kind, BodyKind::Sheet);
        assert_eq!(records[0].regions.len(), 2);
        assert!(records[0].refs.contains(&100));
        assert_eq!(records[0].regions[1].shells[0].refs, vec![100]);
        assert_eq!(
            facts
                .hierarchies(&ctx, &HashSet::from([100]))
                .expect("hierarchy allocation")
                .expect("typed hierarchy")[0]
                .body
                .kind,
            BodyKind::Sheet
        );
    }

    #[test]
    fn ambiguous_face_owner_stats_survive_when_all_uses_are_withheld() {
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(
        &[], &arena, &cadmpeg_core::decode::DecodePolicy::service(),
    ).unwrap();
        let bridge = |attr, surface, offset| Bridge {
            attr,
            sequence: 0,
            refs: [0, 0, 0, 0, surface],
            sense: Sense::Forward,
            owner: Some(700),
            offset,
        };
        let mut tables = super::topology::Tables::default();
        tables.insert_bridge(&ctx, bridge(10, 100, 20)).expect("bridge");
        tables.insert_bridge(&ctx, bridge(11, 200, 10)).expect("bridge");
        let decoded = super::decode_graph(
            &ctx,
            &mut crate::brep::index::CarrierIndex::default(),
            &tables,
            super::entity::Facts {
                entity_count: 1,
                ..Default::default()
            },
            &super::typed::Facts::default(),
            &cadmpeg_ir::stream_name!("empty"),
        )
        .expect("valid exactness fields");

        assert!(decoded.faces.is_empty());
        assert_eq!(decoded.stats.ambiguous_face_owners, 1);
    }

    #[test]
    fn topology_pruning_retains_a_procedural_blend_spine() {
        use cadmpeg_ir::geometry::{
            BlendCrossSection, BlendRadiusLaw, Curve, CurveGeometry, ProceduralSurface,
            ProceduralSurfaceDefinition, SolvedCurveGeometry,
        };
        use cadmpeg_ir::ids::{CurveId, ProceduralSurfaceId};

        let spine = CurveId::mint("test:model:entity#spine").expect("identity grammar");
        let mut brep = super::Brep {
            curves: vec![Curve {
                id: spine.clone(),
                geometry: CurveGeometry::Solved(SolvedCurveGeometry::Line(
                    cadmpeg_ir::geometry::analytic::LineCurve::try_new(
                        cadmpeg_ir::math::Point3::new(0.0, 0.0, 0.0),
                        cadmpeg_ir::math::Vector3::new(1.0, 0.0, 0.0),
                    )
                    .unwrap(),
                )),
                source_object: None,
            }],
            procedural_surfaces: vec![ProceduralSurface::new(
                ProceduralSurfaceId::mint("test:model:entity#blend").expect("identity grammar"),
                ProceduralSurfaceDefinition::Blend(
                    cadmpeg_ir::geometry::surface_payloads::BlendSurfacePayload::try_new(
                        [None, None],
                        Some(spine.clone()),
                        BlendRadiusLaw::constant(0.5).unwrap(),
                        BlendCrossSection::Circular,
                        cadmpeg_ir::geometry::CacheContract::from_form(None),
                    )
                    .unwrap(),
                ),
                None,
            )],
            ..Default::default()
        };

        let arena = cadmpeg_core::decode::DecodeArena::new();
        let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(
            &[], &arena, &cadmpeg_core::decode::DecodePolicy::service(),
        ).expect("test context");
        super::prune_rejected_topology(&ctx, &mut brep).expect("prune topology");
        assert_eq!(brep.curves.first().map(|curve| &curve.id), Some(&spine));
    }

    #[test]
    fn homogeneous_quadratic_identity_proves_constant_radius() {
        let radius = 2.0;
        let checked_radius =
            cadmpeg_ir::scalar::PositiveLength::new(radius).expect("positive radius");
        let controls = [
            cadmpeg_ir::math::Point2::new(radius, 0.0),
            cadmpeg_ir::math::Point2::new(radius, radius),
            cadmpeg_ir::math::Point2::new(0.0, radius),
        ];
        let weights = [1.0, std::f64::consts::FRAC_1_SQRT_2, 1.0];
        let knots = [0.0, 0.0, 0.0, 1.0, 1.0, 1.0];
        assert!(super::quadratic_nurbs_has_constant_radius(
            &controls,
            Some(&weights),
            &knots,
            checked_radius,
        ));

        let mut invalid = controls;
        invalid[1].u += 0.01;
        assert!(!super::quadratic_nurbs_has_constant_radius(
            &invalid,
            Some(&weights),
            &knots,
            checked_radius,
        ));
    }

    #[test]
    fn interior_ruled_surface_line_has_affine_isoparametric_inverse() {
        let surface = test_nurbs_surface(
            1,
            1,
            vec![0.0, 0.0, 1.0, 1.0],
            vec![0.0, 0.0, 1.0, 1.0],
            2,
            2,
            &[
                cadmpeg_ir::math::Point3::new(0.0, 0.0, 0.0),
                cadmpeg_ir::math::Point3::new(0.0, 1.0, 0.0),
                cadmpeg_ir::math::Point3::new(1.0, 0.0, 0.0),
                cadmpeg_ir::math::Point3::new(2.0, 1.0, 0.0),
            ],
            None,
        );
        let geometry = match with_test_context(|ctx| super::ruled_surface_line_pcurve(
            ctx,
            &surface,
            cadmpeg_ir::geometry::nurbs::SurfaceParameterAxis::V,
            cadmpeg_ir::math::Point3::new(0.0, 0.5, 0.0),
            cadmpeg_ir::math::Vector3::new(1.0, 0.0, 0.0),
        ))
        .expect("ruling evaluation")
        {
            super::InverseResolution::Unique(geometry) => geometry,
            super::InverseResolution::NoMatch => panic!("interior ruling did not match"),
            super::InverseResolution::Ambiguous => panic!("interior ruling was ambiguous"),
        };
        let cadmpeg_ir::geometry::pcurve::PcurveGeometry::Line(line_pcurve) = geometry else {
            panic!("expected affine line pcurve");
        };
        let origin = line_pcurve.origin().as_raw();
        let direction = line_pcurve.direction().as_raw();
        assert!(origin.u.abs() < 1.0e-12);
        assert!((origin.v - 0.5).abs() < 1.0e-12);
        assert!((direction.u - 2.0 / 3.0).abs() < 1.0e-12);
        assert!(direction.v.abs() < 1.0e-12);
    }

    #[test]
    fn interior_linear_axis_rational_nurbs_isocurve_has_exact_pcurve() {
        let surface = test_nurbs_surface(
            2,
            1,
            vec![0.0, 0.0, 0.0, 1.0, 1.0, 1.0],
            vec![-0.1, -0.1, 0.9, 0.9],
            3,
            2,
            &[
                cadmpeg_ir::math::Point3::new(0.0, 0.0, -1.0),
                cadmpeg_ir::math::Point3::new(0.0, 0.0, 3.0),
                cadmpeg_ir::math::Point3::new(1.0, 1.0, -1.0),
                cadmpeg_ir::math::Point3::new(1.0, 1.0, 3.0),
                cadmpeg_ir::math::Point3::new(2.0, 0.0, -1.0),
                cadmpeg_ir::math::Point3::new(2.0, 0.0, 3.0),
            ],
            Some(vec![1.0, 1.0, 2.0, 2.0, 1.0, 1.0]),
        );
        let curve = test_nurbs_curve(
            2,
            surface.u_knots().to_vec(),
            vec![
                cadmpeg_ir::math::Point3::new(0.0, 0.0, 0.0),
                cadmpeg_ir::math::Point3::new(1.0, 1.0, 0.0),
                cadmpeg_ir::math::Point3::new(2.0, 0.0, 0.0),
            ],
            Some(vec![1.0, 2.0, 1.0]),
        );
        let geometry =
            match super::nurbs_isocurve_pcurve(&surface, &curve).expect("isocurve lanes pair") {
                super::InverseResolution::Unique(geometry) => geometry,
                super::InverseResolution::NoMatch => panic!("interior isocurve did not match"),
                super::InverseResolution::Ambiguous => panic!("interior isocurve was ambiguous"),
            };
        let cadmpeg_ir::geometry::pcurve::PcurveGeometry::Line(line_pcurve) = geometry else {
            panic!("expected isoparametric line pcurve");
        };
        let origin = line_pcurve.origin().as_raw();
        let direction = line_pcurve.direction().as_raw();
        assert!(origin.u.abs() < 1.0e-12);
        assert!((origin.v - 0.15).abs() < 1.0e-12);
        assert!((direction.u - 1.0).abs() < 1.0e-12);
        assert!(direction.v.abs() < 1.0e-12);
    }

    #[test]
    fn extended_nurbs_isocurve_clamps_the_carrier_before_matching() {
        let surface = test_nurbs_surface(
            1,
            1,
            vec![0.0, 0.0, 1.0, 1.0],
            vec![0.0, 0.0, 1.0, 1.0],
            2,
            2,
            &[
                cadmpeg_ir::math::Point3::new(0.0, 0.0, 0.0),
                cadmpeg_ir::math::Point3::new(0.0, 1.0, 0.0),
                cadmpeg_ir::math::Point3::new(1.0, 0.0, 0.0),
                cadmpeg_ir::math::Point3::new(1.0, 1.0, 0.0),
            ],
            None,
        );
        let curve = test_nurbs_curve(
            1,
            vec![-1.0, -1.0, 2.0, 2.0],
            vec![
                cadmpeg_ir::math::Point3::new(0.5, -1.0, 0.0),
                cadmpeg_ir::math::Point3::new(0.5, 2.0, 0.0),
            ],
            None,
        );
        let resolution = super::derive_nurbs_edge_pcurve(&surface, &curve, [0.2, 0.8])
            .expect("isocurve lanes pair");
        let super::NurbsPcurveResolution::Exact(
            cadmpeg_ir::geometry::pcurve::PcurveGeometry::Line(line_pcurve),
        ) = resolution
        else {
            panic!("extended isocurve was not certified");
        };
        let origin = line_pcurve.origin().as_raw();
        let direction = line_pcurve.direction().as_raw();
        assert!((origin.u - 0.5).abs() < 1e-12);
        assert!(origin.v.abs() < 1e-12);
        assert!(direction.u.abs() < 1e-12);
        assert!((direction.v - 1.0).abs() < 1e-12);
        let clamped = super::clamp_nurbs_curve_to_domain(&curve, [0.0, 1.0])
            .expect("the clamped lanes are a curve")
            .expect("clamped segment");
        let expected = cadmpeg_ir::eval::nurbs_surface_isocurve(
            &surface,
            cadmpeg_ir::geometry::nurbs::SurfaceParameterAxis::U,
            0.5,
        )
        .expect("resource allocation did not fail")
        .expect("surface isocurve");
        assert!(super::nurbs_representation_matches(&expected, &clamped));
    }

    #[test]
    fn extended_quadratic_isocurve_preserves_the_inserted_homogeneous_segment() {
        let surface = test_nurbs_surface(
            1,
            2,
            vec![0.0, 0.0, 1.0, 1.0],
            vec![0.0, 0.0, 0.0, 1.0, 1.0, 1.0],
            2,
            3,
            &[
                cadmpeg_ir::math::Point3::new(0.0, 0.0, 0.0),
                cadmpeg_ir::math::Point3::new(0.0, 0.5, 0.0),
                cadmpeg_ir::math::Point3::new(0.0, 1.0, 0.0),
                cadmpeg_ir::math::Point3::new(1.0, 0.0, 0.0),
                cadmpeg_ir::math::Point3::new(1.0, 0.5, 0.0),
                cadmpeg_ir::math::Point3::new(1.0, 1.0, 0.0),
            ],
            None,
        );
        let curve = test_nurbs_curve(
            2,
            vec![-1.0, -1.0, -1.0, 2.0, 2.0, 2.0],
            vec![
                cadmpeg_ir::math::Point3::new(0.5, -1.0, 0.0),
                cadmpeg_ir::math::Point3::new(0.5, 0.5, 0.0),
                cadmpeg_ir::math::Point3::new(0.5, 2.0, 0.0),
            ],
            None,
        );
        let resolution = super::derive_nurbs_edge_pcurve(&surface, &curve, [0.1, 0.9])
            .expect("isocurve lanes pair");
        assert!(
            matches!(resolution, super::NurbsPcurveResolution::Exact(cadmpeg_ir::geometry::pcurve::PcurveGeometry::Line(
                            line_pcurve,
                        )) if {
                            let origin = line_pcurve.origin().as_raw();
            let direction = line_pcurve.direction().as_raw();
                            (origin.u - 0.5).abs() <= f64::EPSILON * 64.0
                                && origin.v.abs() <= f64::EPSILON * 64.0
                                && direction.u.abs() <= f64::EPSILON * 64.0
                                && (direction.v - 1.0).abs() <= f64::EPSILON * 64.0
                        })
        );
        let clamped = super::clamp_nurbs_curve_to_domain(&curve, [0.0, 1.0])
            .expect("the clamped lanes are a curve")
            .expect("clamped quadratic segment");
        let expected = cadmpeg_ir::eval::nurbs_surface_isocurve(
            &surface,
            cadmpeg_ir::geometry::nurbs::SurfaceParameterAxis::U,
            0.5,
        )
        .expect("resource allocation did not fail")
        .expect("quadratic surface isocurve");
        assert!(super::nurbs_representation_matches(&expected, &clamped));
    }

    #[test]
    fn extended_rational_isocurve_compares_weights_after_homogeneous_clamping() {
        let surface = test_nurbs_surface(
            1,
            1,
            vec![0.0, 0.0, 1.0, 1.0],
            vec![0.0, 0.0, 1.0, 1.0],
            2,
            2,
            &[
                cadmpeg_ir::math::Point3::new(0.0, 0.0, 0.0),
                cadmpeg_ir::math::Point3::new(0.0, 1.0, 0.0),
                cadmpeg_ir::math::Point3::new(1.0, 0.0, 0.0),
                cadmpeg_ir::math::Point3::new(1.0, 1.0, 0.0),
            ],
            Some(vec![1.0, 1.2, 1.0, 1.2]),
        );
        let curve = test_nurbs_curve(
            1,
            vec![-1.0, -1.0, 2.0, 2.0],
            vec![
                cadmpeg_ir::math::Point3::new(0.5, -1.5, 0.0),
                cadmpeg_ir::math::Point3::new(0.5, 2.4 / 1.4, 0.0),
            ],
            Some(vec![0.8, 1.4]),
        );
        let resolution = super::derive_nurbs_edge_pcurve(&surface, &curve, [0.2, 0.8])
            .expect("isocurve lanes pair");
        assert!(
            matches!(resolution, super::NurbsPcurveResolution::Exact(cadmpeg_ir::geometry::pcurve::PcurveGeometry::Line(
                            line_pcurve,
                        )) if {
                            let origin = line_pcurve.origin().as_raw();
            let direction = line_pcurve.direction().as_raw();
                            (origin.u - 0.5).abs() <= f64::EPSILON * 64.0
                                && origin.v.abs() <= f64::EPSILON * 64.0
                                && direction.u.abs() <= f64::EPSILON * 64.0
                                && (direction.v - 1.0).abs() <= f64::EPSILON * 64.0
                        })
        );
        let clamped = super::clamp_nurbs_curve_to_domain(&curve, [0.0, 1.0])
            .expect("the clamped lanes are a curve")
            .expect("clamped rational segment");
        let expected = cadmpeg_ir::eval::nurbs_surface_isocurve(
            &surface,
            cadmpeg_ir::geometry::nurbs::SurfaceParameterAxis::U,
            0.5,
        )
        .expect("resource allocation did not fail")
        .expect("rational surface isocurve");
        assert!(super::nurbs_representation_matches(&expected, &clamped));
    }

    #[test]
    fn degree_one_nurbs_cache_pcurve_keeps_measured_chordal_error() {
        let surface = test_nurbs_surface(
            2,
            1,
            vec![0.0, 0.0, 0.0, 1.0, 1.0, 1.0],
            vec![0.0, 0.0, 1.0, 1.0],
            3,
            2,
            &[
                cadmpeg_ir::math::Point3::new(0.0, 0.0, 0.0),
                cadmpeg_ir::math::Point3::new(0.0, 1.0, 0.0),
                cadmpeg_ir::math::Point3::new(0.5, 0.0, 1.0),
                cadmpeg_ir::math::Point3::new(0.5, 1.0, 1.0),
                cadmpeg_ir::math::Point3::new(1.0, 0.0, 0.0),
                cadmpeg_ir::math::Point3::new(1.0, 1.0, 0.0),
            ],
            None,
        );
        let curve = test_nurbs_curve(
            1,
            vec![0.0, 0.0, 1.0, 1.0],
            vec![
                cadmpeg_ir::math::Point3::new(0.0, 0.0, 0.0),
                cadmpeg_ir::math::Point3::new(1.0, 0.0, 0.0),
            ],
            None,
        );
        let resolution = super::derive_nurbs_edge_pcurve(&surface, &curve, [0.0, 1.0])
            .expect("isocurve lanes pair");
        let super::NurbsPcurveResolution::Cache {
            geometry: cadmpeg_ir::geometry::pcurve::PcurveGeometry::Nurbs { nurbs },
            fit_tolerance,
        } = resolution
        else {
            panic!("degree-one cache was not accepted");
        };
        assert_eq!(nurbs.degree(), 1);
        assert_eq!(nurbs.knots(), curve.knots());
        assert_eq!(nurbs.control_points().len(), 2);
        assert!(nurbs.weights().is_none());
        assert!(!nurbs.periodic());
        assert!(fit_tolerance > 0.4);
        assert!(fit_tolerance < 0.6);
    }

    #[test]
    fn off_surface_nurbs_edge_is_classified_before_cache_inversion() {
        let surface = test_nurbs_surface(
            1,
            1,
            vec![0.0, 0.0, 1.0, 1.0],
            vec![0.0, 0.0, 1.0, 1.0],
            2,
            2,
            &[
                cadmpeg_ir::math::Point3::new(0.0, 0.0, 0.0),
                cadmpeg_ir::math::Point3::new(0.0, 1.0, 0.0),
                cadmpeg_ir::math::Point3::new(1.0, 0.0, 0.0),
                cadmpeg_ir::math::Point3::new(1.0, 1.0, 0.0),
            ],
            None,
        );
        let curve = test_nurbs_curve(
            1,
            vec![0.0, 0.0, 1.0, 1.0],
            vec![
                cadmpeg_ir::math::Point3::new(0.0, 0.0, 10.0),
                cadmpeg_ir::math::Point3::new(1.0, 0.0, 10.0),
            ],
            None,
        );
        assert!(matches!(
            super::derive_nurbs_edge_pcurve(&surface, &curve, [0.0, 1.0])
                .expect("isocurve lanes pair"),
            super::NurbsPcurveResolution::OffSurface
        ));
    }

    #[test]
    fn v_linear_surface_line_has_axis_symmetric_inverse() {
        let surface = test_nurbs_surface(
            2,
            1,
            vec![0.0, 0.0, 0.0, 1.0, 1.0, 1.0],
            vec![0.0, 0.0, 1.0, 1.0],
            3,
            2,
            &[
                cadmpeg_ir::math::Point3::new(0.0, 0.0, 0.0),
                cadmpeg_ir::math::Point3::new(0.0, 1.0, 0.0),
                cadmpeg_ir::math::Point3::new(0.5, 0.0, 0.0),
                cadmpeg_ir::math::Point3::new(0.5, 1.0, 0.0),
                cadmpeg_ir::math::Point3::new(1.0, 0.0, 0.0),
                cadmpeg_ir::math::Point3::new(1.0, 1.0, 0.0),
            ],
            None,
        );
        let geometry = match with_test_context(|ctx| super::ruled_surface_line_pcurve(
            ctx,
            &surface,
            cadmpeg_ir::geometry::nurbs::SurfaceParameterAxis::U,
            cadmpeg_ir::math::Point3::new(0.5, 0.0, 0.0),
            cadmpeg_ir::math::Vector3::new(0.0, 1.0, 0.0),
        ))
        .expect("ruling evaluation")
        {
            super::InverseResolution::Unique(geometry) => geometry,
            super::InverseResolution::NoMatch => panic!("transposed ruling did not match"),
            super::InverseResolution::Ambiguous => panic!("transposed ruling was ambiguous"),
        };
        let cadmpeg_ir::geometry::pcurve::PcurveGeometry::Line(line_pcurve) = geometry else {
            panic!("expected affine line pcurve");
        };
        let origin = line_pcurve.origin().as_raw();
        let direction = line_pcurve.direction().as_raw();
        assert!((origin.u - 0.5).abs() < 1.0e-8);
        assert!(origin.v.abs() < 1.0e-12);
        assert!(direction.u.abs() < 1.0e-12);
        assert!((direction.v - 1.0).abs() < 1.0e-12);
    }

    #[test]
    fn repeated_ruled_surface_line_candidates_are_ambiguous() {
        let surface = test_nurbs_surface(
            1,
            2,
            vec![0.0, 0.0, 1.0, 1.0],
            vec![0.0, 0.0, 0.0, 1.0, 1.0, 1.0],
            2,
            3,
            &[
                cadmpeg_ir::math::Point3::new(0.0, 0.0, 0.0),
                cadmpeg_ir::math::Point3::new(0.0, 1.0, 0.0),
                cadmpeg_ir::math::Point3::new(0.0, 0.0, 0.0),
                cadmpeg_ir::math::Point3::new(1.0, 0.0, 0.0),
                cadmpeg_ir::math::Point3::new(1.0, 1.0, 0.0),
                cadmpeg_ir::math::Point3::new(1.0, 0.0, 0.0),
            ],
            None,
        );
        assert!(matches!(
            with_test_context(|ctx| super::ruled_surface_line_pcurve(
                ctx,
                &surface,
                cadmpeg_ir::geometry::nurbs::SurfaceParameterAxis::V,
                cadmpeg_ir::math::Point3::new(0.0, 0.0, 0.0),
                cadmpeg_ir::math::Vector3::new(1.0, 0.0, 0.0),
            )).expect("ruling evaluation"),
            super::InverseResolution::Ambiguous
        ));
    }

    #[test]
    fn repeated_nurbs_endpoint_candidates_are_ambiguous() {
        let curve = test_nurbs_curve(
            2,
            vec![0.0, 0.0, 0.0, 1.0, 1.0, 1.0],
            vec![
                cadmpeg_ir::math::Point3::new(0.0, 0.0, 0.0),
                cadmpeg_ir::math::Point3::new(1.0, 0.0, 0.0),
                cadmpeg_ir::math::Point3::new(0.0, 0.0, 0.0),
            ],
            None,
        );
        assert!(matches!(
            with_test_context(|ctx| super::nurbs_parameter_at_point(ctx, &curve, cadmpeg_ir::math::Point3::new(0.0, 0.0, 0.0))).expect("inverse evaluation"),
            super::InverseResolution::Ambiguous
        ));
    }

    #[test]
    fn ambiguous_cylindrical_endpoint_withholds_the_derived_pcurve() {
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(
        &[], &arena, &cadmpeg_core::decode::DecodePolicy::service(),
    ).unwrap();
        use cadmpeg_ir::annotations::AnnotationBuilder;
        use cadmpeg_ir::geometry::{Curve, Surface};
        use cadmpeg_ir::ids::{CurveId, EdgeId, FaceId, LoopId, PointId, SurfaceId, VertexId};
        use cadmpeg_ir::topology::{Coedge, Edge, Face, Loop, Point, Sense, Vertex};

        let surface_id = SurfaceId::mint("test:model:entity#surface").expect("identity grammar");
        let curve_id = CurveId::mint("test:model:entity#curve").expect("identity grammar");
        let loop_id = LoopId::mint("test:model:entity#loop").expect("identity grammar");
        let edge_id = EdgeId::mint("test:model:entity#edge").expect("identity grammar");
        let start_vertex =
            VertexId::mint("test:model:entity#start-vertex").expect("identity grammar");
        let end_vertex = VertexId::mint("test:model:entity#end-vertex").expect("identity grammar");
        let start_point = PointId::mint("test:model:entity#start-point").expect("identity grammar");
        let end_point = PointId::mint("test:model:entity#end-point").expect("identity grammar");
        let coedge_id =
            cadmpeg_ir::ids::CoedgeId::mint("test:model:entity#coedge").expect("identity grammar");
        let mut brep = super::Brep {
            surfaces: vec![Surface {
                id: surface_id.clone(),
                geometry: cadmpeg_ir::geometry::SurfaceGeometry::Solved(
                    SolvedSurfaceGeometry::Cylinder(
                        cadmpeg_ir::geometry::analytic::CylinderSurface::try_new(
                            cadmpeg_ir::math::Point3::new(0.0, 0.0, 0.0),
                            cadmpeg_ir::math::Vector3::new(0.0, 0.0, 1.0),
                            cadmpeg_ir::math::Vector3::new(1.0, 0.0, 0.0),
                            1000.0,
                        )
                        .unwrap(),
                    ),
                ),
                source_object: None,
            }],
            curves: vec![Curve {
                id: curve_id.clone(),
                geometry: cadmpeg_ir::geometry::CurveGeometry::Solved(SolvedCurveGeometry::Nurbs(
                    test_nurbs_curve(
                        2,
                        vec![0.0, 0.0, 0.0, 1.0, 1.0, 1.0],
                        vec![
                            cadmpeg_ir::math::Point3::new(1000.0, 0.0, 0.0),
                            cadmpeg_ir::math::Point3::new(1000.0, 0.0, 1000.0),
                            cadmpeg_ir::math::Point3::new(1000.0, 0.0, 0.0),
                        ],
                        None,
                    ),
                )),
                source_object: None,
            }],
            faces: vec![Face {
                id: FaceId::mint("test:model:entity#face").expect("identity grammar"),
                shell: cadmpeg_ir::ids::ShellId::mint("test:model:entity#shell")
                    .expect("identity grammar"),
                surface: surface_id,
                sense: Sense::Forward,
                loops: cadmpeg_ir::topology::FaceLoops::unspecified(vec![loop_id.clone()]),
                name: None,
                color: None,
                tolerance: None,
            }],
            loops: vec![Loop {
                id: loop_id.clone(),
                face: FaceId::mint("test:model:entity#face").expect("identity grammar"),
                boundary: cadmpeg_ir::topology::LoopBoundary::Ring(
                    cadmpeg_ir::topology::LoopRing::new(vec![coedge_id.clone()], Vec::new())
                        .expect("valid loop ring"),
                ),
            }],
            coedges: vec![Coedge {
                id: coedge_id,
                owner_loop: loop_id,
                edge: edge_id.clone(),
                radial_next: cadmpeg_ir::ids::CoedgeId::mint("test:model:entity#coedge")
                    .expect("identity grammar"),
                sense: Sense::Forward,
                pcurves: Vec::new(),
                use_curve: None,
            }],
            edges: vec![Edge {
                id: edge_id,
                carrier: cadmpeg_ir::topology::EdgeCarrier::unbounded(Some(curve_id)),
                start: start_vertex.clone(),
                end: end_vertex.clone(),
                tolerance: None,
            }],
            vertices: vec![
                Vertex {
                    id: start_vertex,
                    point: start_point.clone(),
                    tolerance: None,
                },
                Vertex {
                    id: end_vertex,
                    point: end_point.clone(),
                    tolerance: None,
                },
            ],
            points: vec![
                Point::new(
                    start_point,
                    cadmpeg_ir::features::FinitePoint3::new(cadmpeg_ir::math::Point3::new(
                        1000.0, 0.0, 0.0,
                    ))
                    .expect("a finite position is a point"),
                    None,
                ),
                Point::new(
                    end_point,
                    cadmpeg_ir::features::FinitePoint3::new(cadmpeg_ir::math::Point3::new(
                        1000.0, 0.0, 0.0,
                    ))
                    .expect("a finite position is a point"),
                    None,
                ),
            ],
            ..Default::default()
        };
        let mut annotations = AnnotationBuilder::new();
        let source_stream =
            cadmpeg_ir::annotations::StreamHandle::new(cadmpeg_ir::stream_name!("test"));
        super::derive_cylindrical_pcurves(&ctx, &mut brep, &mut annotations, &source_stream)
            .expect("cylindrical pcurve derivation");

        assert!(brep.pcurves.is_empty());
        assert_eq!(brep.stats.ambiguous_pcurve_parameters, 1);
    }
}

#[cfg(test)]
mod numerical_range_tests;
