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
//! carrier, while an undecoded record produces `SurfaceGeometry::Solved(SolvedSurfaceGeometry::Unknown)`
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
pub mod key_maps;
pub mod records;
pub mod stats;
use stats::Stats;
mod topology;
pub mod transfer;

use crate::asm_header;
use crate::ids::IdFormat;
use crate::nurbs;
use crate::nurbs::proc_curve::ProceduralCurveConstruction;
use crate::nurbs::proc_surface::DecodedProceduralSurface;
use crate::sab::Record;
use cadmpeg_ir::attributes::{AttributeTarget, SourceAttribute};
use cadmpeg_ir::geometry::{
    pcurve::{Pcurve, PcurveGeometry},
    Curve, CurveGeometry, ProceduralCurve, ProceduralSurface, Surface, SurfaceGeometry,
};
use cadmpeg_ir::ids::Identity;
use cadmpeg_ir::ids::{CurveId, SurfaceId};
use cadmpeg_ir::topology::{Body, Coedge, Edge, Face, Loop, Point, Region, Shell, Vertex};
use cadmpeg_ir::unknown::UnknownRecord;
use serde::{Deserialize, Serialize};
use serde_value::Value;
use std::collections::{BTreeSet, HashMap, HashSet};

use self::annotations::{emit_annotation_records, AnnotationRecord};
use self::attributes::attribute_owner;
use self::emit::{
    count_other_records, emit_attributes, emit_carrier_records, emit_coedges, emit_containers,
    emit_edges, emit_faces, emit_loops, emit_passthrough_unknowns, emit_pcurves, emit_points,
    emit_vertices, CoedgeDecodeInputs, ContainerInputs, CurveSenseRefs,
};
use self::geometry::{clamp_edge_ranges_to_carrier_domains, classify_body_kinds};
use self::records::{
    BodyNativeKey, EdgeContinuity, EdgeOwnership, FaceNativeKey, FaceSidedness,
    MeshSurfaceSentinel, TolerantCoedgeParameters, TolerantEdgeTail, TolerantVertexTail,
    TransformHints, VertexOwnership, WireTopology,
};
use self::topology::{
    classify_edge_curve_senses, collect_wire_topology, decode_analytic_carriers,
    keep_faces_and_carriers, walk_reachable_topology, TopologyContext,
};
/// The decoded ASM B-rep graph plus loss accounting. Every field is a fact
/// of the ASM stream, independent of the format that references the stream.
#[derive(Default, Serialize)]
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

/// Flat wire shape of [`AsmBrep`].
///
/// The read names every key of the document object in one struct and refuses
/// any other. The join maps are not on the wire: they are projections of the
/// key records, derived on read through [`key_maps`].
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct AsmBrepWire {
    bodies: Vec<Body>,
    regions: Vec<Region>,
    shells: Vec<Shell>,
    faces: Vec<Face>,
    loops: Vec<Loop>,
    coedges: Vec<Coedge>,
    edges: Vec<Edge>,
    vertices: Vec<Vertex>,
    points: Vec<Point>,
    surfaces: Vec<Surface>,
    curves: Vec<Curve>,
    pcurves: Vec<Pcurve>,
    procedural_surfaces: Vec<(SurfaceId, ProceduralSurface)>,
    procedural_curves: Vec<(CurveId, ProceduralCurve)>,
    edge_continuities: Vec<EdgeContinuity>,
    edge_ownerships: Vec<EdgeOwnership>,
    vertex_ownerships: Vec<VertexOwnership>,
    face_sidedness: Vec<FaceSidedness>,
    face_native_keys: Vec<FaceNativeKey>,
    tolerant_coedge_parameters: Vec<TolerantCoedgeParameters>,
    tolerant_edge_tails: Vec<TolerantEdgeTail>,
    tolerant_vertex_tails: Vec<TolerantVertexTail>,
    mesh_surface_sentinels: Vec<MeshSurfaceSentinel>,
    transform_hints: Vec<TransformHints>,
    body_native_keys: Vec<BodyNativeKey>,
    wire_topologies: Vec<WireTopology>,
    attributes: Vec<SourceAttribute>,
    unknowns: Vec<UnknownRecord>,
    stats: Stats,
}

impl<'de> Deserialize<'de> for AsmBrep {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let wire = AsmBrepWire::deserialize(deserializer)?;
        Ok(Self {
            bodies: wire.bodies,
            regions: wire.regions,
            shells: wire.shells,
            faces: wire.faces,
            loops: wire.loops,
            coedges: wire.coedges,
            edges: wire.edges,
            vertices: wire.vertices,
            points: wire.points,
            surfaces: wire.surfaces,
            curves: wire.curves,
            pcurves: wire.pcurves,
            procedural_surfaces: wire.procedural_surfaces,
            procedural_curves: wire.procedural_curves,
            edge_continuities: wire.edge_continuities,
            edge_ownerships: wire.edge_ownerships,
            vertex_ownerships: wire.vertex_ownerships,
            face_sidedness: wire.face_sidedness,
            face_native_keys: wire.face_native_keys,
            tolerant_coedge_parameters: wire.tolerant_coedge_parameters,
            tolerant_edge_tails: wire.tolerant_edge_tails,
            tolerant_vertex_tails: wire.tolerant_vertex_tails,
            mesh_surface_sentinels: wire.mesh_surface_sentinels,
            transform_hints: wire.transform_hints,
            body_native_keys: wire.body_native_keys,
            wire_topologies: wire.wire_topologies,
            attributes: wire.attributes,
            unknowns: wire.unknowns,
            stats: wire.stats,
            annotation_records: Vec::new(),
        })
    }
}

impl AsmBrep {
    /// Append a disjoint, already-qualified ASM graph.
    pub fn append(
        &mut self,
        ctx: &cadmpeg_core::decode::DecodeContext<'_>,
        mut other: Self,
    ) -> Result<(), cadmpeg_core::CodecError> {
        macro_rules! append_vecs {
            ($($field:ident),+ $(,)?) => {
                $(ctx.append_vec(&mut self.$field, &mut other.$field, concat!("ASM append ", stringify!($field)))?;)+
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
        self.stats.merge(ctx, other.stats)?;
        Ok(())
    }
}

/// Collect every `id` field value in a serialized value tree.
pub fn collect_owned_ids(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    value: &Value,
    out: &mut HashSet<String, std::collections::hash_map::RandomState>,
) -> Result<(), cadmpeg_core::CodecError> {
    let _depth = ctx.enter_nested("collect ASM owned ids")?;
    match value {
        Value::Map(fields) => {
            if let Some(id) = entity_id(ctx, value)? {
                ctx.insert_string_set(out, id, "ASM owned ids")?;
            }
            for (key, value) in ctx.admit_iter(fields, "ASM serialized map fields")? {
                collect_owned_ids(ctx, key, out)?;
                collect_owned_ids(ctx, value, out)?;
            }
        }
        Value::Seq(items) => {
            for item in ctx.admit_iter(items, "ASM serialized sequence items")? {
                collect_owned_ids(ctx, item, out)?;
            }
        }
        Value::Option(Some(value)) | Value::Newtype(value) => collect_owned_ids(ctx, value, out)?,
        _ => {}
    }
    Ok(())
}

/// The string payload of a serialized value, unwrapping newtype layers.
pub fn value_string<'value>(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    mut value: &'value Value,
) -> Result<Option<&'value str>, cadmpeg_core::CodecError> {
    while let Value::Newtype(inner) = value {
        ctx.charge_work(1, "ASM serialized newtype walk")?;
        value = inner;
    }
    Ok(match value {
        Value::String(value) => Some(value),
        _ => None,
    })
}

/// Build the undirected id-adjacency of every entity in the top-level
/// sequences of a serialized value tree.
pub fn collect_entity_adjacency(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    value: &Value,
    owned: &HashSet<String, std::collections::hash_map::RandomState>,
    out: &mut HashMap<String, BTreeSet<String>, std::collections::hash_map::RandomState>,
) -> Result<(), cadmpeg_core::CodecError> {
    let Value::Map(fields) = value else {
        return Ok(());
    };
    for (_, value) in ctx.admit_iter(fields, "ASM adjacency root fields")? {
        let Value::Seq(items) = value else {
            continue;
        };
        for item in ctx.admit_iter(items, "ASM serialized sequence items")? {
            let Some(id) = entity_id(ctx, item)? else {
                continue;
            };
            let mut references = BTreeSet::new();
            let mut reference_storage =
                ctx.reserve_scoped(0, "ASM adjacency scratch references")?;
            reference_storage
                .with_storage(|| collect_references(ctx, item, owned, &mut references))?;
            ctx.remove_btree_set(&mut references, id, "ASM adjacency self reference")?;
            for reference in ctx.admit_iter(references, "ASM adjacency references")? {
                insert_adjacency(ctx, out, id, &reference)?;
                insert_adjacency(ctx, out, &reference, id)?;
            }
        }
    }
    Ok(())
}

fn insert_adjacency(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    out: &mut HashMap<String, BTreeSet<String>>,
    owner: &str,
    reference: &str,
) -> Result<(), cadmpeg_core::CodecError> {
    if !ctx.contains_key_hash_map(out, owner, "ASM adjacency owner lookup")? {
        let key = ctx.copy_retained_text(owner, "ASM adjacency owner")?;

        ctx.insert_hash_map(out, key, BTreeSet::new(), "ASM adjacency owners")?;
    }
    if let Some(references) = ctx.get_mut_hash_map(out, owner, "ASM adjacency owner lookup")? {
        if !ctx.contains_btree_set(references, reference, "ASM adjacency reference lookup")? {
            let reference = ctx.copy_retained_text(reference, "ASM adjacency reference")?;
            ctx.insert_btree_set(references, reference, "ASM adjacency references")?;
        }
    }
    Ok(())
}

/// The `id` field of a serialized entity map.
pub fn entity_id<'value>(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    value: &'value Value,
) -> Result<Option<&'value str>, cadmpeg_core::CodecError> {
    let Value::Map(fields) = value else {
        return Ok(None);
    };
    let field = ctx.find_by(
        fields,
        |(key, _)| Ok(matches!(key, Value::String(name) if name == "id")),
        "ASM serialized entity id",
    )?;
    match field {
        Some((_, value)) => value_string(ctx, value),
        None => Ok(None),
    }
}

/// Collect every string in a serialized value tree that names an owned id.
pub fn collect_references(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    value: &Value,
    owned: &HashSet<String, std::collections::hash_map::RandomState>,
    out: &mut BTreeSet<String>,
) -> Result<(), cadmpeg_core::CodecError> {
    let _depth = ctx.enter_nested("collect ASM references")?;
    match value {
        Value::String(id) => {
            if ctx.contains_hash_set(owned, id.as_str(), "ASM owned reference lookup")?
                && !ctx.contains_btree_set(out, id.as_str(), "ASM reference lookup")?
            {
                let id = ctx.copy_retained_text(id, "ASM reference id")?;
                ctx.insert_btree_set(out, id, "ASM references")?;
            }
        }
        Value::Seq(items) => {
            for item in ctx.admit_iter(items, "ASM serialized sequence items")? {
                collect_references(ctx, item, owned, out)?;
            }
        }
        Value::Map(fields) => {
            for (key, value) in ctx.admit_iter(fields, "ASM serialized map fields")? {
                collect_references(ctx, key, owned, out)?;
                collect_references(ctx, value, owned, out)?;
            }
        }
        Value::Option(Some(value)) | Value::Newtype(value) => {
            collect_references(ctx, value, owned, out)?;
        }
        _ => {}
    }
    Ok(())
}

/// Retain only entities with a reachable `id` in the top-level sequences of a
/// serialized value tree.
pub fn retain_root_entities(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    value: &mut Value,
    reachable: &HashSet<String, std::collections::hash_map::RandomState>,
) -> Result<(), cadmpeg_core::CodecError> {
    let Value::Map(fields) = value else {
        return Ok(());
    };
    for (_, value) in ctx.admit_iter(fields, "ASM retained root fields")? {
        if let Value::Seq(items) = value {
            ctx.retain_vec(
                items,
                |item| match entity_id(ctx, item)? {
                    Some(id) => ctx.contains_hash_set(reachable, id, "ASM root reachability"),
                    None => Ok(true),
                },
                "ASM root entity retention",
            )?;
        }
    }
    Ok(())
}

/// Rewrite every string in a serialized value tree through `replacements`.
pub fn remap_owned_ids(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    value: &mut Value,
    replacements: &HashMap<String, String, std::collections::hash_map::RandomState>,
) -> Result<(), cadmpeg_core::CodecError> {
    let _depth = ctx.enter_nested("remap ASM owned ids")?;
    match value {
        Value::String(id) => {
            if let Some(replacement) =
                ctx.get_hash_map(replacements, id.as_str(), "ASM replacement id lookup")?
            {
                *id = ctx.copy_retained_text(replacement, "ASM remapped id")?;
            }
        }
        Value::Seq(items) => {
            for item in ctx.admit_iter(items, "ASM serialized sequence items")? {
                remap_owned_ids(ctx, item, replacements)?;
            }
        }
        Value::Map(fields) => {
            let entries = std::mem::take(fields);
            for (mut key, mut item) in ctx.admit_iter(entries, "ASM remapped map fields")? {
                remap_owned_ids(ctx, &mut key, replacements)?;
                remap_owned_ids(ctx, &mut item, replacements)?;
                ctx.insert_btree_map(fields, key, item, "ASM remapped fields")?;
            }
        }
        Value::Option(Some(value)) | Value::Newtype(value) => {
            remap_owned_ids(ctx, value, replacements)?;
        }
        _ => {}
    }
    Ok(())
}

fn count_kind(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    counts: &mut std::collections::BTreeMap<String, usize>,
    kind: &str,
) -> Result<(), cadmpeg_core::CodecError> {
    if let Some(count) = ctx.get_mut_btree_map(counts, kind, "ASM loss kind lookup")? {
        *count += 1;
        return Ok(());
    }
    let key = ctx.copy_retained_text(kind, "ASM loss kind")?;
    ctx.insert_btree_map(counts, key, 1, "ASM loss kind")?;
    Ok(())
}

// ---- geometry carrier decode -------------------------------------------------

/// Formats the stable IR id for the entity emitted from record `index`.
pub fn id(format: IdFormat, index: i64) -> Identity {
    format.brep_identity(&cadmpeg_ir::identity_component!("entity"), index)
}

/// Construction and fit metadata separated from the cache geometry.
enum ProceduralCurveSource {
    Cached {
        construction: Box<ProceduralCurveConstruction>,
        cache_fit_tolerance: Option<f64>,
        parsed_domain: Option<[f64; 2]>,
    },
    Cacheless(Box<cadmpeg_ir::geometry::ProceduralCurveDefinition>),
}

#[derive(Clone, Copy, PartialEq, Eq, Hash)]
struct PcurveRecordIndex(i64);

impl cadmpeg_core::decode::cost::DecodeCost for PcurveRecordIndex {
    const FIXED_BYTES: Option<u64> =
        Some(cadmpeg_core::decode::u64_from_index(std::mem::size_of::<
            Self,
        >()));
    fn decode_cost(
        &self,
        _ctx: &cadmpeg_core::decode::DecodeContext<'_>,
        _operation: &'static str,
    ) -> Result<u64, cadmpeg_core::CodecError> {
        Ok(cadmpeg_core::decode::u64_from_index(std::mem::size_of::<
            Self,
        >()))
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Hash)]
struct CoedgeRecordIndex(i64);

impl cadmpeg_core::decode::cost::DecodeCost for CoedgeRecordIndex {
    const FIXED_BYTES: Option<u64> =
        Some(cadmpeg_core::decode::u64_from_index(std::mem::size_of::<
            Self,
        >()));
    fn decode_cost(
        &self,
        _ctx: &cadmpeg_core::decode::DecodeContext<'_>,
        _operation: &'static str,
    ) -> Result<u64, cadmpeg_core::CodecError> {
        Ok(cadmpeg_core::decode::u64_from_index(std::mem::size_of::<
            Self,
        >()))
    }
}

/// Decoded carrier geometry keyed by `RecordTable` index. The reachability and
/// emit passes read decoded shapes from here and consume them (`remove`) as the
/// owning surface or curve record is emitted.
#[derive(Default)]
struct Carriers {
    /// Source record indices of synthetic procedural support surfaces.
    procedural_support_sources: Vec<(i64, SurfaceId)>,
    /// Source record indices of synthetic procedural surface curves.
    procedural_curve_child_sources: Vec<(i64, CurveId)>,
    surface_geo: HashMap<i64, SurfaceGeometry>,
    procedural_surface_defs: HashMap<i64, DecodedProceduralSurface>,
    curve_geo: HashMap<i64, CurveGeometry>,
    procedural_curve_defs: HashMap<i64, ProceduralCurveSource>,
    pcurve_geo: HashMap<PcurveRecordIndex, PcurveGeometry>,
    pcurve_parameter_ranges: HashMap<CoedgeRecordIndex, [f64; 2]>,
}

/// Record indices reached from kept faces by the shell/loop/coedge walk,
/// grouped by entity kind. Every emit pass filters `records` against these
/// sets so only reachable entities appear in the output.
#[derive(Default)]
struct Reachable {
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
struct WireShellTopology {
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
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    records: &[Record],
    bytes: &[u8],
    stream: &str,
    format: IdFormat,
    purpose: DecodePurpose,
) -> Result<AsmBrep, cadmpeg_core::CodecError> {
    let header = asm_header::parse(ctx, bytes)?.map(|header| header.metadata);
    decode_with_header(
        ctx,
        records,
        bytes,
        header.as_ref(),
        stream,
        format,
        purpose,
    )
}

/// Decode a framed slice whose header the caller supplies.
///
/// The text encoding ([`crate::sat`]) carries the same header fields in ASCII
/// header lines rather than the binary layout, so its caller parses them and
/// passes the result here. `bytes` remains the source byte image: unknown
/// records retain their byte extents from it.
pub fn decode_with_header(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    records: &[Record],
    bytes: &[u8],
    header: Option<&crate::kernel_header::KernelHeader>,
    stream: &str,
    format: IdFormat,
    purpose: DecodePurpose,
) -> Result<AsmBrep, cadmpeg_core::CodecError> {
    let mut out = AsmBrep::default();

    let mut scratch = ctx.reserve_scoped(0, "ASM decode scratch")?;
    let mut by_index = HashMap::new();
    for record in ctx.admit_iter(records, "index ASM records")? {
        let index = i64::try_from(record.index).map_err(|_| {
            ctx.refuse_codec_limit(
                "ASM record index",
                i64::MAX.unsigned_abs(),
                cadmpeg_core::decode::u64_from_index(record.index),
            )
        })?;
        scratch.with_storage(|| {
            ctx.insert_hash_map(&mut by_index, index, record, "index ASM records")
        })?;
    }
    // Subtype-definition positions, built once for every carrier resolution.
    let mut subtype_storage = ctx.reserve_scoped(0, "index ASM subtype definitions")?;
    let token_table = subtype_storage
        .with_storage(|| nurbs::toks::SubtypeTable::from_records(ctx, records))?
        .with_save_format_version(header.and_then(|header| header.save_format_version));
    nurbs::toks::admit_subtype_references(ctx, records, &token_table)?;
    let save_format_major = header.and_then(crate::kernel_header::KernelHeader::save_format_major);
    let saved_entity_limit = header
        .and_then(|header| header.entity_count)
        .and_then(|count| i64::try_from(count).ok());
    let header_scale = header.and_then(|header| header.scale).unwrap_or(1.0);

    let (mut carriers, inward_normal_surfaces) =
        decode_analytic_carriers(ctx, records, &mut scratch)?;
    let mut reach = Reachable::default();

    let topology_context = TopologyContext {
        ctx,
        by_index: &by_index,
        token_table: &token_table,
        purpose,
        format,
    };
    keep_faces_and_carriers(
        topology_context,
        &mut out,
        records,
        &mut carriers,
        &mut reach,
        &mut scratch,
    )?;
    walk_reachable_topology(
        topology_context,
        &mut out,
        records,
        &mut carriers,
        &mut reach,
        &mut scratch,
    )?;
    let wire = collect_wire_topology(
        topology_context,
        &mut out,
        records,
        saved_entity_limit,
        &mut carriers,
        &mut reach,
        &mut scratch,
    )?;

    let (reversed_curve_refs, forward_curve_refs) =
        scratch.with_storage(|| classify_edge_curve_senses(ctx, records, &reach))?;

    emit_carrier_records(
        ctx,
        &mut out,
        records,
        (&mut carriers, &mut scratch, purpose),
        &reach,
        CurveSenseRefs {
            reversed_curve_refs: &reversed_curve_refs,
            forward_curve_refs: &forward_curve_refs,
        },
        format,
    )?;
    emit_pcurves(ctx, &mut out, records, &mut carriers, &reach, format)?;
    emit_points(ctx, &mut out, records, &reach, format)?;
    emit_vertices(ctx, &mut out, records, &by_index, &reach, format)?;
    emit_edges(
        ctx,
        &mut out,
        records,
        &by_index,
        &reach,
        CurveSenseRefs {
            reversed_curve_refs: &reversed_curve_refs,
            forward_curve_refs: &forward_curve_refs,
        },
        format,
    )?;
    emit_coedges(
        ctx,
        &mut out,
        records,
        CoedgeDecodeInputs {
            token_table: &token_table,
            save_format_major,
        },
        &carriers,
        &reach,
        format,
    )?;
    emit_loops(ctx, &mut out, records, &by_index, &reach, format)?;
    emit_faces(
        ctx,
        &mut out,
        records,
        &by_index,
        &reach,
        &inward_normal_surfaces,
        format,
    )?;
    emit_containers(
        ctx,
        &mut out,
        ContainerInputs {
            records,
            by_index: &by_index,
            reach: &reach,
            wire: &wire,
            stream,
            header_scale,
            format,
        },
    )?;
    let emitted_attributes = emit_attributes(
        ctx,
        &mut out,
        records,
        &by_index,
        &reach,
        format,
        &mut scratch,
    )?;
    if purpose == DecodePurpose::Model {
        emit_passthrough_unknowns(ctx, &mut out, records, bytes, &reach, format)?;
        count_other_records(ctx, &mut out, records, &reach, &emitted_attributes)?;
        emit_annotation_records(
            ctx,
            &mut out,
            records,
            &by_index,
            &mut carriers,
            stream,
            format,
        )?;

        classify_body_kinds(ctx, &mut out)?;
        clamp_edge_ranges_to_carrier_domains(ctx, &mut out)?;
    }

    Ok(out)
}

fn inherited_attribute_target(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    mut owner: i64,
    by_index: &HashMap<i64, &Record>,
    targets: &HashMap<i64, AttributeTarget>,
) -> Result<Option<AttributeTarget>, cadmpeg_core::CodecError> {
    let mut visited = HashSet::new();
    let mut storage = ctx.reserve_scoped(0, "ASM inherited attribute visited")?;
    loop {
        ctx.charge_work(1, "ASM inherited attribute walk")?;
        if !storage.with_storage(|| {
            ctx.insert_hash_set(&mut visited, owner, "ASM inherited attribute visited")
        })? {
            return Ok(None);
        }
        if let Some(target) = ctx.get_hash_map(targets, &owner, "ASM inherited target lookup")? {
            return target
                .try_clone_for_decode(ctx, "ASM inherited attribute target")
                .map(Some);
        }
        let Some(attribute) = ctx.get_hash_map(by_index, &owner, "ASM inherited record lookup")? else {
            return Ok(None);
        };
        if !attribute.name.ends_with("-attrib") {
            return Ok(None);
        }
        let Some(parent) = attribute_owner(attribute) else {
            return Ok(None);
        };
        owner = parent;
    }
}

#[cfg(test)]
mod tests;

mod identity_rewrite;
