// SPDX-License-Identifier: Apache-2.0
//! Build B-rep topology and geometry from a framed SAB record table.
//!
//! [`decode_with_purpose`] follows the topology chain from bodies through
//! vertices and points. It creates analytic carriers for planes, cylinders,
//! cones, spheres, tori, lines, circles, and ellipses. [`crate::nurbs`]
//! supplies cached NURBS surfaces, 3D curves, and pcurves for spline and
//! procedural records.
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

pub mod annotations;
pub mod attributes;
mod emit;
pub mod geometry;
mod key_maps;
pub mod records;
pub mod stats;
use stats::Stats;
mod topology;
pub mod transfer;

use crate::asm_header;
use crate::ids::IdFormat;
use crate::nurbs;
use crate::nurbs::proc_curve::{
    CompoundDefinition, EmbeddedDeformable, EmbeddedIntersection, EmbeddedLawCurve,
    EmbeddedProjection, EmbeddedSilhouette, EmbeddedSpring, EmbeddedSurfaceCurve,
    EmbeddedSurfaceOffset, EmbeddedThreeSurfaceIntersection, EmbeddedTwoSidedOffset,
    SubsetDefinition, VectorOffsetDefinition,
};
use crate::nurbs::proc_surface::DecodedProceduralSurface;
use crate::sab::Record;
use cadmpeg_ir::attributes::{AttributeTarget, SourceAttribute};
use cadmpeg_ir::geometry::{
    Curve, CurveGeometry, Pcurve, PcurveGeometry, ProceduralCurve, ProceduralSurface, Surface,
    SurfaceGeometry,
};
use cadmpeg_ir::ids::{CurveId, SurfaceId};
use cadmpeg_ir::topology::{Body, Coedge, Edge, Face, Loop, Point, Region, Shell, Vertex};
use cadmpeg_ir::unknown::UnknownRecord;
use serde::{Deserialize, Serialize};
use serde_value::Value;
use std::collections::{HashMap, HashSet};

use self::annotations::{emit_annotation_records, AnnotationRecord};
use self::attributes::attribute_owner;
use self::emit::{
    count_other_records, emit_attributes, emit_carrier_records, emit_coedges, emit_containers,
    emit_edges, emit_faces, emit_loops, emit_passthrough_unknowns, emit_pcurves, emit_points,
    emit_vertices, project_subshell_faces,
};
use self::geometry::{clamp_edge_ranges_to_carrier_domains, classify_body_kinds};
use self::records::{
    BodyNativeKey, EdgeContinuity, EdgeOwnership, FaceNativeKey, FaceSidedness,
    MeshSurfaceSentinel, TolerantCoedgeParameters, TolerantEdgeTail, TolerantVertexTail,
    TransformHints, VertexOwnership, WireTopology,
};
use self::topology::{
    classify_edge_curve_senses, collect_wire_topology, decode_analytic_carriers,
    keep_faces_and_carriers, walk_reachable_topology,
};
pub(crate) fn embedded_pcurve_geometry(pcurve: nurbs::pcurve::NurbsPcurve) -> PcurveGeometry {
    pcurve.into_geometry()
}

/// The decoded ASM B-rep graph plus loss accounting. Every field is a fact
/// of the ASM stream, independent of the format that references the stream.
#[derive(Default, Serialize, Deserialize)]
pub struct AsmBrep {
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
    pub procedural_surfaces: Vec<(SurfaceId, ProceduralSurface)>,
    /// Native procedural definitions for solved curve caches.
    pub procedural_curves: Vec<(CurveId, ProceduralCurve)>,
    /// Kernel continuity classifications stored on solved edges.
    pub edge_continuities: Vec<EdgeContinuity>,
    /// Native owner-coedge selectors stored on solved edges.
    pub edge_ownerships: Vec<EdgeOwnership>,
    /// Native owner-edge and endpoint-slot fields stored on solved vertices.
    pub vertex_ownerships: Vec<VertexOwnership>,
    /// Native sidedness fields stored on solved faces.
    pub face_sidedness: Vec<FaceSidedness>,
    /// Native Design-join key field for every emitted face, including null keys.
    #[serde(flatten, with = "key_maps::faces")]
    pub face_native_keys: Vec<FaceNativeKey>,
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
    /// Native Design-join key field for every emitted body, including null keys.
    #[serde(flatten, with = "key_maps::bodies")]
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

impl AsmBrep {
    /// Append a disjoint, already-qualified ASM graph.
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
            edge_continuities,
            edge_ownerships,
            vertex_ownerships,
            face_sidedness,
            face_native_keys,
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
        self.stats.merge(other.stats);
    }
}

/// Collect every `id` field value in a serialized value tree.
#[allow(clippy::implicit_hasher)]
pub fn collect_owned_ids(value: &Value, out: &mut HashSet<String>) {
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

/// The string payload of a serialized value, unwrapping newtype layers.
pub fn value_string(value: &Value) -> Option<&str> {
    match value {
        Value::String(value) => Some(value),
        Value::Newtype(value) => value_string(value),
        _ => None,
    }
}

/// Build the undirected id-adjacency of every entity in the top-level
/// sequences of a serialized value tree.
#[allow(clippy::implicit_hasher)]
pub fn collect_entity_adjacency(
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

/// The `id` field of a serialized entity map.
pub fn entity_id(value: &Value) -> Option<&str> {
    let Value::Map(fields) = value else {
        return None;
    };
    fields
        .get(&Value::String("id".into()))
        .and_then(value_string)
}

/// Collect every string in a serialized value tree that names an owned id.
#[allow(clippy::implicit_hasher)]
pub fn collect_references(value: &Value, owned: &HashSet<String>, out: &mut HashSet<String>) {
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

/// Retain only entities with a reachable `id` in the top-level sequences of a
/// serialized value tree.
#[allow(clippy::implicit_hasher)]
pub fn retain_root_entities(value: &mut Value, reachable: &HashSet<String>) {
    let Value::Map(fields) = value else {
        return;
    };
    for (name, value) in fields {
        match value {
            Value::Seq(items) => {
                items.retain(|item| entity_id(item).is_none_or(|id| reachable.contains(id)));
            }
            Value::Map(keys) if matches!(name, Value::String(name) if matches!(name.as_str(), "body_keys" | "face_keys")) =>
            {
                keys.retain(|key, _| matches!(key, Value::String(id) if reachable.contains(id)));
            }
            _ => {}
        }
    }
}

/// Rewrite every string in a serialized value tree through `replacements`.
#[allow(clippy::implicit_hasher)]
pub fn remap_owned_ids(value: &mut Value, replacements: &HashMap<String, String>) {
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
pub fn id(format: IdFormat<'_>, index: i64) -> String {
    format!("{format}:brep:entity#{index}")
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
    Option<EmbeddedSurfaceCurve>,
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
    saved_free_edges: Vec<i64>,
}

/// Which outputs a decode materializes.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum DecodePurpose {
    /// Transfer complete neutral geometry and retained native records.
    Model,
    /// Transfer stable topology plus measurements used by history binding.
    History,
}

/// Decode a framed active slice into the ASM B-rep graph.
///
/// `stream` names the source ZIP entry for provenance. Ids are minted as
/// `<format>:brep:entity#<record-index>`, unique across the `RecordTable`.
/// [`DecodePurpose::History`] skips free-form carrier shapes because
/// historical binding consumes stable record identities, not control data.
pub fn decode_with_purpose(
    records: &[Record],
    bytes: &[u8],
    stream: &str,
    format: IdFormat<'_>,
    purpose: DecodePurpose,
) -> AsmBrep {
    let header = asm_header::parse(bytes);
    decode_with_header(records, bytes, header, stream, format, purpose)
}

/// Decode a framed slice whose header the caller supplies.
///
/// The text encoding ([`crate::sat`]) carries the same header fields in ASCII
/// header lines rather than the binary layout, so its caller parses them and
/// passes the result here. `bytes` remains the source byte image: unknown
/// records retain their byte extents from it.
pub fn decode_with_header(
    records: &[Record],
    bytes: &[u8],
    header: Option<crate::kernel_header::KernelHeader>,
    stream: &str,
    format: IdFormat<'_>,
    purpose: DecodePurpose,
) -> AsmBrep {
    let mut out = AsmBrep::default();

    // Index records by RecordTable index (== position for a framed slice).
    let by_index: HashMap<i64, &Record> = records.iter().map(|r| (r.index as i64, r)).collect();
    // Subtype-definition positions, built once for every carrier resolution.
    let token_table = nurbs::toks::SubtypeTable::from_records(records).with_save_format_version(
        header
            .as_ref()
            .and_then(|header| header.save_format_version),
    );
    let save_format_major = header
        .as_ref()
        .and_then(crate::kernel_header::KernelHeader::save_format_major);
    let saved_entity_limit = header
        .as_ref()
        .and_then(|header| header.entity_count)
        .and_then(|count| i64::try_from(count).ok());
    let header_scale = header.and_then(|header| header.scale).unwrap_or(1.0);

    let (mut carriers, inward_normal_surfaces) = decode_analytic_carriers(records);
    let mut reach = Reachable::default();

    keep_faces_and_carriers(
        &mut out,
        records,
        &by_index,
        &token_table,
        &mut carriers,
        &mut reach,
        purpose,
        format,
    );
    walk_reachable_topology(
        &mut out,
        &by_index,
        &token_table,
        &mut carriers,
        &mut reach,
        purpose,
        format,
    );
    let wire = collect_wire_topology(
        &mut out,
        records,
        &by_index,
        saved_entity_limit,
        &token_table,
        &mut carriers,
        &mut reach,
        purpose,
        format,
    );

    let (reversed_curve_refs, forward_curve_refs) = classify_edge_curve_senses(records, &reach);

    emit_carrier_records(
        &mut out,
        records,
        &mut carriers,
        &reach,
        &reversed_curve_refs,
        &forward_curve_refs,
        format,
    );
    emit_pcurves(&mut out, records, &mut carriers, &reach, format);
    emit_points(&mut out, records, &reach, format);
    emit_vertices(&mut out, records, &by_index, &reach, format);
    emit_edges(
        &mut out,
        records,
        &by_index,
        &reach,
        &reversed_curve_refs,
        &forward_curve_refs,
        format,
    );
    emit_coedges(
        &mut out,
        records,
        &token_table,
        save_format_major,
        &carriers,
        &reach,
        format,
    );
    emit_loops(&mut out, records, &by_index, &reach, format);
    emit_faces(
        &mut out,
        records,
        &by_index,
        &reach,
        &inward_normal_surfaces,
        format,
    );
    emit_containers(
        &mut out,
        records,
        &by_index,
        &reach,
        &wire,
        stream,
        header_scale,
        format,
    );
    project_subshell_faces(&mut out, records, &by_index, format);
    let emitted_attributes = emit_attributes(&mut out, records, &by_index, &reach, format);
    if purpose == DecodePurpose::Model {
        emit_passthrough_unknowns(&mut out, records, bytes, &reach, format);
        count_other_records(&mut out, records, &reach, &emitted_attributes);
        emit_annotation_records(&mut out, records, &by_index, stream, format);

        classify_body_kinds(&mut out);
        clamp_edge_ranges_to_carrier_domains(&mut out);
    }

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
        owner = attribute_owner(attribute)?;
    }
    None
}

#[cfg(test)]
mod tests;
