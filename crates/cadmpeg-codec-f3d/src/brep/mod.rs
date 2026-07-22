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

pub(crate) mod attributes;
mod emit;
pub(crate) mod geometry;
mod topology;

use crate::asm_header;
use crate::nurbs;
use crate::nurbs::proc_curve::{
    CompoundDefinition, EmbeddedDeformable, EmbeddedIntersection, EmbeddedLawCurve,
    EmbeddedProjection, EmbeddedSilhouette, EmbeddedSpring, EmbeddedSurfaceOffset,
    EmbeddedThreeSurfaceIntersection, EmbeddedTwoSidedOffset, SubsetDefinition,
    VectorOffsetDefinition,
};
use crate::nurbs::proc_surface::DecodedProceduralSurface;
use crate::records::{
    BodyNativeKey, CreationTimestamp, EdgeContinuity, EdgeOwnership, FaceSidedness,
    MeshSurfaceSentinel, PersistentDesignLink, PersistentSubentityTag, SketchCurveLink,
    TolerantCoedgeParameters, TolerantEdgeTail, TolerantVertexTail, TransformHints,
    VertexOwnership, WireTopology,
};
use crate::sab::Record;
use cadmpeg_ir::attributes::{AttributeTarget, SourceAttribute};
use cadmpeg_ir::geometry::{
    Curve, CurveGeometry, Pcurve, PcurveGeometry, ProceduralCurve, ProceduralSurface, Surface,
    SurfaceGeometry,
};
use cadmpeg_ir::ids::BodyId;
use cadmpeg_ir::topology::{Body, Coedge, Edge, Face, Loop, Point, Region, Shell, Vertex};
use cadmpeg_ir::unknown::UnknownRecord;
use serde::{Deserialize, Serialize};
use serde_value::Value;
use std::collections::{HashMap, HashSet};

use self::emit::{
    count_other_records, emit_annotation_records, emit_attributes, emit_carrier_records,
    emit_coedges, emit_containers, emit_edges, emit_faces, emit_loops, emit_passthrough_unknowns,
    emit_pcurves, emit_points, emit_vertices, project_subshell_faces,
};
use self::geometry::{clamp_edge_ranges_to_carrier_domains, classify_body_kinds};
use self::topology::{
    classify_edge_curve_senses, collect_wire_topology, decode_analytic_carriers,
    keep_faces_and_carriers, walk_reachable_topology,
};
pub(crate) fn embedded_pcurve_geometry(pcurve: nurbs::pcurve::NurbsPcurve) -> PcurveGeometry {
    PcurveGeometry::Nurbs {
        degree: pcurve.degree,
        knots: pcurve.knots,
        control_points: pcurve.control_points,
        weights: pcurve.weights,
        periodic: pcurve.periodic,
    }
}

/// The decoded B-rep graph plus loss accounting.
#[derive(Default, Serialize, Deserialize)]
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
    #[serde(skip)]
    pub annotation_records: Vec<AnnotationRecord>,
}

/// One sparse v1 annotation produced while SAB record offsets are available.
pub struct AnnotationRecord {
    /// Globally unique IR entity id.
    pub id: String,
    /// BREP ZIP entry containing the source SAB record.
    pub stream: String,
    /// Byte offset in the decompressed ASM stream.
    pub offset: u64,
    /// Source SAB record name.
    pub tag: String,
    /// Serialized fields whose values were canonically derived.
    pub derived_fields: Vec<&'static str>,
}

/// Counts used to construct the B-rep loss report.
#[derive(Default, Serialize, Deserialize)]
pub struct Stats {
    /// Faces omitted because their required surface reference is null or dangling.
    pub missing_face_surfaces: usize,
    /// Omitted face counts by null/dangling surface-reference condition.
    pub missing_face_surface_kinds: std::collections::BTreeMap<String, usize>,
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

impl Brep {
    /// Map solved bodies to the selector used by this blob's Design body map.
    pub fn body_selectors(&self) -> HashMap<BodyId, u64> {
        let ordinal_mode = self
            .body_native_keys
            .iter()
            .all(|body| body.asm_body_key.is_none());
        self.body_native_keys
            .iter()
            .filter_map(|body| {
                let selector = if ordinal_mode {
                    Some(u64::from(body.body_ordinal))
                } else {
                    body.asm_body_key
                }?;
                Some((body.body.clone(), selector))
            })
            .collect()
    }

    /// Retain the connected entity graph rooted at the body-map keys selected
    /// for one BREP blob.
    pub fn retain_body_keys(
        &mut self,
        selected_keys: &HashSet<u64>,
    ) -> Result<(), cadmpeg_ir::codec::CodecError> {
        let annotations = std::mem::take(&mut self.annotation_records);
        let mut value = serde_value::to_value(&*self).map_err(|error| {
            cadmpeg_ir::codec::CodecError::Malformed(format!("BREP serialization failed: {error}"))
        })?;
        let mut owned = HashSet::new();
        collect_owned_ids(&value, &mut owned);
        let roots = self
            .body_selectors()
            .into_iter()
            .filter(|(_, selector)| selected_keys.contains(selector))
            .map(|(body, _)| body.0)
            .collect::<HashSet<_>>();
        let mut adjacency = HashMap::<String, HashSet<String>>::new();
        collect_entity_adjacency(&value, &owned, &mut adjacency);
        let mut reachable = roots;
        let mut pending = reachable.iter().cloned().collect::<Vec<_>>();
        while let Some(id) = pending.pop() {
            for adjacent in adjacency.get(&id).into_iter().flatten() {
                if reachable.insert(adjacent.clone()) {
                    pending.push(adjacent.clone());
                }
            }
        }
        retain_root_entities(&mut value, &reachable);
        let mut retained: Self = value.deserialize_into().map_err(|error| {
            cadmpeg_ir::codec::CodecError::Malformed(format!(
                "retained BREP graph is invalid: {error}"
            ))
        })?;
        retained
            .body_keys
            .retain(|body, _| reachable.contains(&body.0));
        retained.annotation_records = annotations
            .into_iter()
            .filter(|annotation| reachable.contains(&annotation.id))
            .collect();
        *self = retained;
        Ok(())
    }

    /// Qualify every entity owned by this graph so several BREP blobs can
    /// coexist in one document model without record-index collisions.
    pub fn qualify_ids(&mut self, namespace: &str) -> Result<(), cadmpeg_ir::codec::CodecError> {
        let annotations = std::mem::take(&mut self.annotation_records);
        let mut value = serde_value::to_value(&*self).map_err(|error| {
            cadmpeg_ir::codec::CodecError::Malformed(format!("BREP serialization failed: {error}"))
        })?;
        let mut owned = HashSet::new();
        collect_owned_ids(&value, &mut owned);
        let replacements = owned
            .into_iter()
            .map(|id| {
                let replacement = format!(
                    "f3d:brep/{namespace}/{}",
                    id.strip_prefix("f3d:").unwrap_or(&id)
                );
                (id, replacement)
            })
            .collect::<HashMap<_, _>>();
        remap_owned_ids(&mut value, &replacements);
        let mut qualified: Self = value.deserialize_into().map_err(|error| {
            cadmpeg_ir::codec::CodecError::Malformed(format!("qualified BREP is invalid: {error}"))
        })?;
        qualified.annotation_records = annotations
            .into_iter()
            .map(|mut annotation| {
                if let Some(id) = replacements.get(&annotation.id) {
                    annotation.id.clone_from(id);
                }
                annotation
            })
            .collect();
        *self = qualified;
        Ok(())
    }

    /// Append a disjoint, already-qualified BREP graph.
    pub fn append(&mut self, mut other: Self) {
        macro_rules! append_vecs {
            ($($field:ident),+ $(,)?) => {
                $(self.$field.append(&mut other.$field);)+
            };
        }
        append_vecs!(
            bodies,
            regions,
            shells,
            faces,
            loops,
            coedges,
            edges,
            vertices,
            points,
            surfaces,
            curves,
            pcurves,
            procedural_surfaces,
            procedural_curves,
            sketch_curve_links,
            persistent_design_links,
            persistent_subentity_tags,
            creation_timestamps,
            edge_continuities,
            edge_ownerships,
            vertex_ownerships,
            face_sidedness,
            tolerant_coedge_parameters,
            tolerant_edge_tails,
            tolerant_vertex_tails,
            mesh_surface_sentinels,
            transform_hints,
            body_native_keys,
            wire_topologies,
            attributes,
            unknowns,
            annotation_records,
        );
        self.body_keys.extend(other.body_keys);
        self.stats.merge(other.stats);
    }
}

impl Stats {
    fn merge(&mut self, other: Self) {
        macro_rules! add_counts {
            ($($field:ident),+ $(,)?) => {
                $(self.$field += other.$field;)+
            };
        }
        add_counts!(
            missing_face_surfaces,
            unknown_surface_faces,
            mesh_surface_faces,
            nurbs_surfaces,
            nurbs_curves,
            procedural_curve_edges,
            undecoded_pcurve_refs,
            partial_procedural_supports,
            other_records,
        );
        for (target, source) in [
            (
                &mut self.missing_face_surface_kinds,
                other.missing_face_surface_kinds,
            ),
            (&mut self.unknown_surface_kinds, other.unknown_surface_kinds),
            (
                &mut self.procedural_curve_kinds,
                other.procedural_curve_kinds,
            ),
            (
                &mut self.undecoded_pcurve_kinds,
                other.undecoded_pcurve_kinds,
            ),
            (&mut self.other_record_kinds, other.other_record_kinds),
        ] {
            for (kind, count) in source {
                *target.entry(kind).or_default() += count;
            }
        }
    }
}

fn collect_owned_ids(value: &Value, out: &mut HashSet<String>) {
    match value {
        Value::Map(fields) => {
            if let Some(id) = fields
                .get(&Value::String("id".into()))
                .and_then(value_string)
            {
                out.insert(id.to_owned());
            }
            for (key, value) in fields {
                collect_owned_ids(key, out);
                collect_owned_ids(value, out);
            }
        }
        Value::Seq(items) => {
            for item in items {
                collect_owned_ids(item, out);
            }
        }
        Value::Option(Some(value)) | Value::Newtype(value) => collect_owned_ids(value, out),
        _ => {}
    }
}

fn value_string(value: &Value) -> Option<&str> {
    match value {
        Value::String(value) => Some(value),
        Value::Newtype(value) => value_string(value),
        _ => None,
    }
}

fn collect_entity_adjacency(
    value: &Value,
    owned: &HashSet<String>,
    out: &mut HashMap<String, HashSet<String>>,
) {
    let Value::Map(fields) = value else {
        return;
    };
    for value in fields.values() {
        let Value::Seq(items) = value else {
            continue;
        };
        for item in items {
            let Some(id) = entity_id(item) else {
                continue;
            };
            let mut references = HashSet::new();
            collect_references(item, owned, &mut references);
            references.remove(id);
            for reference in references {
                out.entry(id.to_owned())
                    .or_default()
                    .insert(reference.clone());
                out.entry(reference).or_default().insert(id.to_owned());
            }
        }
    }
}

pub(crate) fn entity_id(value: &Value) -> Option<&str> {
    let Value::Map(fields) = value else {
        return None;
    };
    fields
        .get(&Value::String("id".into()))
        .and_then(value_string)
}

fn collect_references(value: &Value, owned: &HashSet<String>, out: &mut HashSet<String>) {
    match value {
        Value::String(id) if owned.contains(id) => {
            out.insert(id.clone());
        }
        Value::Seq(items) => {
            for item in items {
                collect_references(item, owned, out);
            }
        }
        Value::Map(fields) => {
            for (key, value) in fields {
                collect_references(key, owned, out);
                collect_references(value, owned, out);
            }
        }
        Value::Option(Some(value)) | Value::Newtype(value) => {
            collect_references(value, owned, out);
        }
        _ => {}
    }
}

fn retain_root_entities(value: &mut Value, reachable: &HashSet<String>) {
    let Value::Map(fields) = value else {
        return;
    };
    for value in fields.values_mut() {
        let Value::Seq(items) = value else {
            continue;
        };
        items.retain(|item| entity_id(item).is_none_or(|id| reachable.contains(id)));
    }
}

fn remap_owned_ids(value: &mut Value, replacements: &HashMap<String, String>) {
    match value {
        Value::String(id) => {
            if let Some(replacement) = replacements.get(id) {
                id.clone_from(replacement);
            }
        }
        Value::Seq(items) => {
            for item in items {
                remap_owned_ids(item, replacements);
            }
        }
        Value::Map(fields) => {
            let entries = std::mem::take(fields);
            for (mut key, mut item) in entries {
                remap_owned_ids(&mut key, replacements);
                remap_owned_ids(&mut item, replacements);
                fields.insert(key, item);
            }
        }
        Value::Option(Some(value)) | Value::Newtype(value) => {
            remap_owned_ids(value, replacements);
        }
        _ => {}
    }
}

pub(crate) fn count_kind(counts: &mut std::collections::BTreeMap<String, usize>, kind: &str) {
    *counts.entry(kind.to_owned()).or_default() += 1;
}

// ---- geometry carrier decode -------------------------------------------------

/// Formats the stable IR id for the entity emitted from record `index`.
pub(crate) fn id(index: i64) -> String {
    format!("f3d:brep:entity#{index}")
}

/// Decoded procedural-curve construction fields captured for a cached
/// `intcurve`, in the declaration order of
/// [`DecodedProceduralCurve`].
type ProceduralCurveTail = (
    String,
    Option<cadmpeg_ir::geometry::ProceduralCurveDefinition>,
    Option<VectorOffsetDefinition>,
    Option<SubsetDefinition>,
    Option<CompoundDefinition>,
    Option<EmbeddedTwoSidedOffset>,
    Option<(EmbeddedIntersection, bool)>,
    Option<EmbeddedThreeSurfaceIntersection>,
    Option<(
        cadmpeg_ir::geometry::SurfaceCurveFamily,
        EmbeddedIntersection,
        Option<cadmpeg_ir::geometry::SurfaceCurveTail>,
    )>,
    Option<EmbeddedSilhouette>,
    Option<EmbeddedSurfaceOffset>,
    Option<EmbeddedSpring>,
    Option<EmbeddedDeformable>,
    Option<EmbeddedProjection>,
    Option<EmbeddedLawCurve>,
    Option<f64>,
);

/// Decoded carrier geometry keyed by `RecordTable` index. The reachability and
/// emit passes read decoded shapes from here and consume them (`remove`) as the
/// owning surface or curve record is emitted.
#[derive(Default)]
pub(crate) struct Carriers {
    surface_geo: HashMap<i64, (SurfaceGeometry, bool)>,
    procedural_surface_defs: HashMap<i64, DecodedProceduralSurface>,
    curve_geo: HashMap<i64, CurveGeometry>,
    procedural_curve_defs: HashMap<i64, ProceduralCurveTail>,
    cacheless_procedural_curve_defs:
        HashMap<i64, (String, cadmpeg_ir::geometry::ProceduralCurveDefinition)>,
    pcurve_geo: HashMap<i64, PcurveGeometry>,
    pcurve_parameter_ranges: HashMap<i64, [f64; 2]>,
}

/// Record indices reached from kept faces by the shell/loop/coedge walk,
/// grouped by entity kind. Every emit pass filters `records` against these
/// sets so only reachable entities appear in the output.
#[derive(Default)]
pub(crate) struct Reachable {
    faces: HashSet<i64>,
    loops: HashSet<i64>,
    coedges: HashSet<i64>,
    edges: HashSet<i64>,
    vertices: HashSet<i64>,
    points: HashSet<i64>,
    surfaces: HashSet<i64>,
    curves: HashSet<i64>,
    pcurves: HashSet<i64>,
    unknown_surface_records: HashSet<i64>,
    cached_unknown_procedural_surfaces: HashSet<i64>,
    undecoded_carriers: HashSet<i64>,
}

/// Wire-edge and free-vertex reachability collected per shell during the
/// topology walk, consumed when emitting shell containers.
#[derive(Default)]
pub(crate) struct WireShellTopology {
    wire_edges_by_shell: HashMap<i64, Vec<i64>>,
    free_vertices_by_shell: HashMap<i64, Vec<i64>>,
}

/// Decode a framed active slice into the IR B-rep graph.
///
/// `stream` names the source ZIP entry for provenance. Ids are minted as
/// `f3d:brep:entity#<record-index>`, unique across the `RecordTable`.
pub fn decode(records: &[Record], bytes: &[u8], stream: &str) -> Brep {
    let mut out = Brep::default();

    // Index records by RecordTable index (== position for a framed slice).
    let by_index: HashMap<i64, &Record> = records.iter().map(|r| (r.index as i64, r)).collect();
    // Subtype-definition positions, built once for every carrier resolution.
    let subtype_tables = nurbs::subtypes::SubtypeTables::from_records(records, bytes);
    let header = asm_header::parse(bytes);
    let ref_width = header
        .as_ref()
        .map_or(8, |header| usize::from(header.width));
    let release_major = header
        .as_ref()
        .and_then(|header| header.release)
        .map(|release| release / 100);
    let header_scale = header.and_then(|header| header.scale).unwrap_or(1.0);

    let (mut carriers, inward_normal_surfaces) = decode_analytic_carriers(records);
    let mut reach = Reachable::default();

    keep_faces_and_carriers(
        &mut out,
        records,
        bytes,
        &by_index,
        &subtype_tables,
        &mut carriers,
        &mut reach,
    );
    walk_reachable_topology(
        &mut out,
        &by_index,
        bytes,
        ref_width,
        &subtype_tables,
        &mut carriers,
        &mut reach,
    );
    let wire = collect_wire_topology(
        &mut out,
        records,
        &by_index,
        bytes,
        &subtype_tables,
        &mut carriers,
        &mut reach,
    );

    let (reversed_curve_refs, forward_curve_refs) = classify_edge_curve_senses(records, &reach);

    emit_carrier_records(
        &mut out,
        records,
        bytes,
        &mut carriers,
        &reach,
        &reversed_curve_refs,
        &forward_curve_refs,
    );
    emit_pcurves(&mut out, records, bytes, ref_width, &mut carriers, &reach);
    emit_points(&mut out, records, &reach);
    emit_vertices(&mut out, records, &by_index, &reach);
    emit_edges(
        &mut out,
        records,
        &by_index,
        &reach,
        &reversed_curve_refs,
        &forward_curve_refs,
    );
    emit_coedges(
        &mut out,
        records,
        bytes,
        &subtype_tables,
        release_major,
        &carriers,
        &reach,
    );
    emit_loops(&mut out, records, &by_index, &reach);
    emit_faces(
        &mut out,
        records,
        &by_index,
        &reach,
        &inward_normal_surfaces,
    );
    emit_containers(
        &mut out,
        records,
        &by_index,
        &reach,
        &wire,
        stream,
        header_scale,
    );
    project_subshell_faces(&mut out, records, &by_index);
    let emitted_attributes = emit_attributes(&mut out, records, &by_index, &reach);
    emit_passthrough_unknowns(&mut out, records, bytes, &reach);
    count_other_records(&mut out, records, &reach, &emitted_attributes);
    emit_annotation_records(&mut out, records, &by_index, stream);

    classify_body_kinds(&mut out);
    clamp_edge_ranges_to_carrier_domains(&mut out);

    out
}

pub(crate) fn inherited_attribute_target(
    mut owner: i64,
    by_index: &HashMap<i64, &Record>,
    targets: &HashMap<i64, AttributeTarget>,
) -> Option<AttributeTarget> {
    let mut visited = HashSet::new();
    while visited.insert(owner) {
        if let Some(target) = targets.get(&owner) {
            return Some(target.clone());
        }
        let attribute = by_index.get(&owner)?;
        if !attribute.name.ends_with("-attrib") {
            return None;
        }
        owner = attribute.ref_at(4)?;
    }
    None
}

#[cfg(test)]
mod tests;
