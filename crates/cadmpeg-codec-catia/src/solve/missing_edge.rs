//! Mesh missing-edge enumeration for standard nested B-rep streams.
//!
//! Recovers unmatched edge-row placements against serialized face coverage.

use crate::families::standard::fbb::{
    boundary_cycles, largest_fbb_run, parse_edge_tables, parse_fbb_edge_tables,
    parse_standard_edge_tables_scoped, parse_standard_edge_tables_with_width, parse_trim_chain,
    parse_vertex_table, selected_standard_run,
};
#[cfg(test)]
use crate::families::standard::topology::{reconstruct_mesh_selection, StandardTopology};
use crate::families::standard::topology::{EdgeBoundaryLayout, EdgeRow, TrimRecord};
use crate::solve::mesh_quotient::{SearchOutcome, MAX_MESH_CONSTRAINT_OPERATIONS};
use crate::solve::union_find::UnionFind;
use cadmpeg_core::decode::{DecodeContext, View, WorkBudget};
use cadmpeg_core::CodecError;
use std::collections::{BTreeMap, HashMap, HashSet};
use std::sync::Arc;

fn charge_collection_items(
    ctx: &DecodeContext<'_>,
    count: usize,
    operation: &'static str,
) -> Result<(), CodecError> {
    let count =
        u64::try_from(count).map_err(|_| ctx.refuse_codec_limit(operation, u64::MAX, u64::MAX))?;
    ctx.charge_collection_items(count, operation)
}

/// Return the counted physical edge rows in their serialized table order.
///
/// Each row retains its table-kind byte, native handle width semantics, and
/// complete handle sequence even when full topology reconstruction is not yet
/// possible.
pub(crate) fn standard_edge_rows(
    ctx: &DecodeContext<'_>,
    bytes: &[u8],
) -> Result<Option<Vec<EdgeRow>>, CodecError> {
    let Some(face_run) = selected_standard_run(ctx, bytes)? else {
        return Ok(None);
    };
    let after_faces = face_run.after_faces();
    Ok(parse_edge_tables(ctx, bytes, after_faces)?.map(|(rows, _)| rows))
}

fn standard_edge_port_identities_with_namespace(
    ctx: &DecodeContext<'_>,
    bytes: &[u8],
    global: bool,
) -> Result<Option<Vec<[u32; 2]>>, CodecError> {
    let Some(face_run) = selected_standard_run(ctx, bytes)? else {
        return Ok(None);
    };
    let after_faces = face_run.after_faces();
    let Some((edge_rows, scopes, _, _)) =
        parse_standard_edge_tables_scoped(ctx, bytes, after_faces)?
    else {
        return Ok(None);
    };
    let mut identity_by_handle = HashMap::new();
    let mut next_identity = 0u32;
    let mut pairs = Vec::new();
    for (row, scope) in edge_rows.iter().zip(scopes) {
        let (Some(&first), Some(&last)) = (row.handles.first(), row.handles.last()) else {
            return Ok(None);
        };
        let pair = if !global && row.boundary_layout != EdgeBoundaryLayout::CompleteBoundaryRun {
            let start = next_identity;
            let Some(next) = next_identity.checked_add(2) else {
                return Ok(None);
            };
            next_identity = next;
            let Some(end) = start.checked_add(1) else {
                return Ok(None);
            };
            [start, end]
        } else {
            let mut pair = [0; 2];
            for (port, handle) in [first, last].into_iter().enumerate() {
                let key = (if global { 0 } else { scope }, handle);
                let identity = if let Some(identity) = identity_by_handle.get(&key) {
                    *identity
                } else {
                    let identity = next_identity;
                    let Some(next) = next_identity.checked_add(1) else {
                        return Ok(None);
                    };
                    next_identity = next;
                    crate::resource::insert_map(
                        ctx,
                        &mut identity_by_handle,
                        key,
                        identity,
                        "catia_standard_port_handle_ids",
                    )?;
                    identity
                };
                pair[port] = identity;
            }
            pair
        };
        crate::resource::push(ctx, &mut pairs, pair, "catia_standard_port_pairs")?;
    }
    Ok(Some(pairs))
}

fn fbb_edge_port_identities_with_namespace(
    ctx: &DecodeContext<'_>,
    bytes: &[u8],
    global: bool,
) -> Result<Option<Vec<[u32; 2]>>, CodecError> {
    let Some(face_run) = largest_fbb_run(bytes) else {
        return Ok(None);
    };
    let after_faces = face_run.after_faces();
    let Some((edge_rows, scopes, _, _)) = parse_fbb_edge_tables(ctx, bytes, after_faces)? else {
        return Ok(None);
    };
    let mut identity_by_handle = HashMap::new();
    let mut pairs = Vec::new();
    for (row, scope) in edge_rows.iter().zip(scopes) {
        let (Some(&first), Some(&last)) = (row.handles.first(), row.handles.last()) else {
            return Ok(None);
        };
        let mut pair = [0; 2];
        for (port, handle) in [first, last].into_iter().enumerate() {
            let Some(next) = u32::try_from(identity_by_handle.len()).ok() else {
                return Ok(None);
            };
            let key = (if global { 0 } else { scope }, handle);
            pair[port] = if let Some(&identity) = identity_by_handle.get(&key) {
                identity
            } else {
                crate::resource::insert_map(
                    ctx,
                    &mut identity_by_handle,
                    key,
                    next,
                    "catia_fbb_port_handle_ids",
                )?;
                next
            };
        }
        crate::resource::push(ctx, &mut pairs, pair, "catia_fbb_port_pairs")?;
    }
    Ok(Some(pairs))
}

const INDEXED_VISUALIZATION_POINT_MARKER: [u8; 6] = [0xff, 0xff, 0x02, 0x00, 0x01, 0xff];
const INDEXED_VISUALIZATION_POINT_HEADER_LEN: usize = 19;
const RAW_VISUALIZATION_POINT_STRIDE: usize = 12;

/// Bind trim-handle endpoints through the indexed visualization-point lane.
///
/// Admission is atomic: the file must contain one structurally complete table,
/// every terminal handle must select a decoded vertex coordinate exactly, and
/// every decoded vertex coordinate must be selected by at least one terminal
/// handle. Unsupported table modes are left unbound.
pub(crate) fn visualization_endpoint_pairs(
    ctx: &DecodeContext<'_>,
    source: &[u8],
    edge_rows: &[EdgeRow],
    point_coordinates: &[[f32; 3]],
) -> Result<Option<Vec<[usize; 2]>>, CodecError> {
    let mut markers = source
        .windows(INDEXED_VISUALIZATION_POINT_MARKER.len())
        .enumerate()
        .filter_map(|(offset, bytes)| {
            (bytes == INDEXED_VISUALIZATION_POINT_MARKER).then_some(offset)
        });
    let Some(marker) = markers.next() else { return Ok(None) };
    if markers.next().is_some() {
        return Ok(None);
    }
    let Some((count, indexed_count, mode, marker_valid, table)) = (|| {
        Some((
            usize::try_from(View::u32_le_at(source, marker.checked_add(6)?)?).ok()?,
            usize::try_from(View::u32_le_at(source, marker.checked_add(11)?)?).ok()?,
            *source.get(marker.checked_add(18)?)?,
            source.get(marker.checked_add(10)?)? == &0xff
                && source.get(marker.checked_add(15)?..marker.checked_add(18)?)? == [0, 0, 0],
            marker.checked_add(INDEXED_VISUALIZATION_POINT_HEADER_LEN)?,
        ))
    })() else { return Ok(None) };
    if !marker_valid
        || indexed_count > count
    {
        return Ok(None);
    }

    let mut point_by_bits = HashMap::new();
    for (point, coordinates) in point_coordinates.iter().enumerate() {
        let key = coordinates.map(f32::to_bits);
        if crate::resource::insert_map(ctx, &mut point_by_bits, key, point, "catia_visualization_point_bits")?.is_some() {
            return Ok(None);
        }
    }
    let mut terminal_handles = HashSet::new();
    for row in edge_rows {
        for handle in [row.handles.first(), row.handles.last()].into_iter().flatten() {
            crate::resource::insert_set(ctx, &mut terminal_handles, *handle, "catia_visualization_terminal_handles")?;
        }
    }
    if terminal_handles
        .iter()
        .any(|handle| usize::try_from(*handle).map_or(true, |handle| handle >= count))
    {
        return Ok(None);
    }
    let point_by_handle = match mode {
        0 => compressed_visualization_point_bindings(
            ctx,
            source,
            table,
            count,
            &terminal_handles,
            &point_by_bits,
        )?,
        1 if count - indexed_count <= 1 => raw_visualization_point_bindings(
            ctx,
            source,
            table,
            count,
            &terminal_handles,
            &point_by_bits,
        )?,
        _ => return Ok(None),
    };
    let Some(point_by_handle) = point_by_handle else { return Ok(None) };
    let mut matched_points = HashSet::new();
    for point in point_by_handle.values().copied() {
        crate::resource::insert_set(ctx, &mut matched_points, point, "catia_visualization_matched_points")?;
    }
    if matched_points.len() != point_coordinates.len() {
        return Ok(None);
    }

    let mut pairs = Vec::new();
    crate::resource::reserve_vec(ctx, &mut pairs, edge_rows.len(), "catia_visualization_endpoint_pairs")?;
    for row in edge_rows {
        let Some(pair) = (|| Some([
            *point_by_handle.get(row.handles.first()?)?,
            *point_by_handle.get(row.handles.last()?)?,
        ]))() else { return Ok(None) };
        pairs.push(pair);
    }
    Ok(Some(pairs))
}

fn raw_visualization_point_bindings(
    ctx: &DecodeContext<'_>,
    source: &[u8],
    table: usize,
    count: usize,
    terminal_handles: &HashSet<u32>,
    point_by_bits: &HashMap<[u32; 3], usize>,
) -> Result<Option<HashMap<u32, usize>>, CodecError> {
    let Some(extent) = count
        .checked_mul(RAW_VISUALIZATION_POINT_STRIDE)
        .and_then(|bytes| bytes.checked_add(table)) else { return Ok(None) };
    if source.get(table..extent).is_none() { return Ok(None) }
    let mut bindings = HashMap::new();
    for handle in terminal_handles {
        let Some(key) = (|| {
            let index = usize::try_from(*handle).ok()?;
            let at = table.checked_add(index.checked_mul(RAW_VISUALIZATION_POINT_STRIDE)?)?;
            Some([
                View::f32_le_at(source, at)?.to_bits(),
                View::f32_le_at(source, at.checked_add(4)?)?.to_bits(),
                View::f32_le_at(source, at.checked_add(8)?)?.to_bits(),
            ])
        })() else { return Ok(None) };
        let Some(&point) = point_by_bits.get(&key) else { return Ok(None) };
        crate::resource::insert_map(ctx, &mut bindings, *handle, point, "catia_raw_visualization_bindings")?;
    }
    Ok(Some(bindings))
}

fn compressed_visualization_point_bindings(
    ctx: &DecodeContext<'_>,
    source: &[u8],
    controls: usize,
    count: usize,
    terminal_handles: &HashSet<u32>,
    point_by_bits: &HashMap<[u32; 3], usize>,
) -> Result<Option<HashMap<u32, usize>>, CodecError> {
    let Some((scalar_count, scalars)) = (|| {
        let packed_len = count.checked_add(3)? / 4;
        let delimiter = controls.checked_add(packed_len)?;
        if *source.get(delimiter)? != 0xff { return None }
        let scalar_count_at = delimiter.checked_add(1)?;
        let scalar_count = usize::try_from(View::u32_le_at(source, scalar_count_at)?).ok()?;
        let scalars = scalar_count_at.checked_add(4)?;
        let scalar_extent = scalar_count.checked_mul(4)?.checked_add(scalars)?;
        source.get(controls..scalar_extent)?;
        Some((scalar_count, scalars))
    })() else { return Ok(None) };

    let mut previous = None::<[u32; 3]>;
    let mut scalar = 0usize;
    let mut bindings = HashMap::new();
    for index in 0..count {
        let Some(packed) = controls.checked_add(index / 4).and_then(|at| source.get(at)).copied() else { return Ok(None) };
        let code = (packed >> (2 * (index % 4))) & 3;
        let mut read_scalar = || {
            if scalar >= scalar_count {
                return None;
            }
            let at = scalars.checked_add(scalar.checked_mul(4)?)?;
            scalar = scalar.checked_add(1)?;
            Some(View::f32_le_at(source, at)?.to_bits())
        };
        let Some(point) = (|| Some(match (code, previous) {
            (0, _) => [read_scalar()?, read_scalar()?, read_scalar()?],
            (1, Some(previous)) => previous,
            (2, Some(previous)) => [previous[0], previous[1], read_scalar()?],
            (3, Some(previous)) => [previous[0], read_scalar()?, read_scalar()?],
            _ => return None,
        }))() else { return Ok(None) };
        previous = Some(point);
        let Some(handle) = u32::try_from(index).ok() else { return Ok(None) };
        if terminal_handles.contains(&handle) {
            let Some(&point) = point_by_bits.get(&point) else { return Ok(None) };
            crate::resource::insert_map(ctx, &mut bindings, handle, point, "catia_compressed_visualization_bindings")?;
        }
    }
    Ok((scalar == scalar_count && bindings.len() == terminal_handles.len()).then_some(bindings))
}

fn standard_edge_port_identities(
    ctx: &DecodeContext<'_>,
    bytes: &[u8],
) -> Result<Option<Vec<[u32; 2]>>, CodecError> {
    standard_edge_port_identities_with_namespace(ctx, bytes, false)
}

fn fbb_edge_port_identities(
    ctx: &DecodeContext<'_>,
    bytes: &[u8],
) -> Result<Option<Vec<[u32; 2]>>, CodecError> {
    fbb_edge_port_identities_with_namespace(ctx, bytes, false)
}

fn standard_global_edge_port_identities(
    ctx: &DecodeContext<'_>,
    bytes: &[u8],
) -> Result<Option<Vec<[u32; 2]>>, CodecError> {
    standard_edge_port_identities_with_namespace(ctx, bytes, true)
}

pub(crate) fn fbb_global_edge_port_identities(
    ctx: &DecodeContext<'_>,
    bytes: &[u8],
) -> Result<Option<Vec<[u32; 2]>>, CodecError> {
    fbb_edge_port_identities_with_namespace(ctx, bytes, true)
}

/// Select conservative endpoint identities for the bounded topology solver.
pub(crate) fn edge_port_identities(
    ctx: &DecodeContext<'_>,
    bytes: &[u8],
) -> Result<Option<Vec<[u32; 2]>>, CodecError> {
    if let Some(identities) = standard_edge_port_identities(ctx, bytes)? {
        Ok(Some(identities))
    } else {
        fbb_edge_port_identities(ctx, bytes)
    }
}

/// Select endpoint identities from every row's terminal handles in the
/// file-global trim-handle namespace.
fn global_edge_port_identities(
    ctx: &DecodeContext<'_>,
    bytes: &[u8],
) -> Result<Option<Vec<[u32; 2]>>, CodecError> {
    if let Some(identities) = standard_global_edge_port_identities(ctx, bytes)? {
        Ok(Some(identities))
    } else {
        fbb_global_edge_port_identities(ctx, bytes)
    }
}

pub(super) fn solver_ports(
    ctx: &DecodeContext<'_>,
    bytes: &[u8],
    global: bool,
) -> Result<Option<Vec<[u32; 2]>>, CodecError> {
    if global {
        global_edge_port_identities(ctx, bytes)
    } else {
        edge_port_identities(ctx, bytes)
    }
}

/// Extend deferred rows to the complete connected components of the supplied
/// endpoint-port graph. A port-domain inference is valid only when every row
/// in the component contributes a settled endpoint relation.
pub(crate) fn expand_deferred_edge_port_components(
    ctx: &DecodeContext<'_>,
    edge_ports: &[[u32; 2]],
    deferred_edges: &mut [bool],
) -> Result<bool, CodecError> {
    if edge_ports.len() != deferred_edges.len() {
        return Ok(false);
    }
    let mut deferred_ports = HashSet::new();
    for (edge, &deferred) in deferred_edges.iter().enumerate() {
        if deferred {
            for port in edge_ports[edge] {
                crate::resource::insert_set(
                    ctx,
                    &mut deferred_ports,
                    port,
                    "catia_deferred_ports",
                )?;
            }
        }
    }
    let mut changed = true;
    while changed {
        changed = false;
        for (edge, ports) in edge_ports.iter().enumerate() {
            if !deferred_edges[edge] && !ports.iter().any(|port| deferred_ports.contains(port)) {
                continue;
            }
            if !deferred_edges[edge] {
                deferred_edges[edge] = true;
                changed = true;
            }
            for port in ports {
                changed |= crate::resource::insert_set(
                    ctx,
                    &mut deferred_ports,
                    *port,
                    "catia_deferred_ports",
                )?;
            }
        }
    }
    Ok(true)
}

/// Collapse physical edge endpoints through every exact trim-mesh occurrence.
/// The returned component identifiers are compact and stable within this
/// result; they are not coordinate-row indices.
pub(crate) fn standard_mesh_edge_ports(
    ctx: &DecodeContext<'_>,
    bytes: &[u8],
) -> Result<Option<Vec<[u32; 2]>>, CodecError> {
    let Some(analysis) = standard_mesh_analysis(ctx, bytes)? else {
        return Ok(None);
    };
    let Some(local_ports) = global_edge_port_identities(ctx, bytes)? else {
        return Ok(None);
    };
    mesh_edge_ports(ctx, &analysis, &local_ports)
}

fn mesh_edge_ports(
    ctx: &DecodeContext<'_>,
    analysis: &StandardMeshAnalysis,
    local_ports: &[[u32; 2]],
) -> Result<Option<Vec<[u32; 2]>>, CodecError> {
    let edge_rows = &analysis.edge_rows;
    let cycles = &analysis.cycles;
    let occurrences = &analysis.occurrences;
    if local_ports.len() != edge_rows.len() {
        return Ok(None);
    }
    let node_count = edge_rows
        .len()
        .checked_mul(2)
        .ok_or_else(|| ctx.refuse_codec_limit("catia_mesh_edge_port_union", u64::MAX, u64::MAX))?;
    let mut union = UnionFind::charged(ctx, node_count, "catia_mesh_edge_port_union")?;
    let mut node_by_identity = HashMap::new();
    for (edge, ports) in local_ports.iter().enumerate() {
        for (side, identity) in ports.iter().copied().enumerate() {
            let node = edge * 2 + side;
            if let Some(&previous) = node_by_identity.get(&identity) {
                union.union(previous, node);
            } else {
                crate::resource::insert_map(
                    ctx,
                    &mut node_by_identity,
                    identity,
                    node,
                    "catia_mesh_edge_port_identities",
                )?;
            }
        }
    }
    let mut corners = HashMap::new();
    for (edge, row) in edge_rows.iter().enumerate() {
        let Some(_) = row.boundary_pattern() else {
            continue;
        };
        for run in &occurrences[edge] {
            let cycle = &cycles[run.face][run.cycle];
            let before = run.start;
            let after = run.end(cycle.len());
            let before_key = (run.face, run.cycle, before);
            let before_node = if let Some(&node) = corners.get(&before_key) {
                node
            } else {
                let node = union.push_charged(ctx, "catia_mesh_edge_port_corner_nodes")?;
                crate::resource::insert_map(
                    ctx,
                    &mut corners,
                    before_key,
                    node,
                    "catia_mesh_edge_port_corners",
                )?;
                node
            };
            let after_key = (run.face, run.cycle, after);
            let after_node = if let Some(&node) = corners.get(&after_key) {
                node
            } else {
                let node = union.push_charged(ctx, "catia_mesh_edge_port_corner_nodes")?;
                crate::resource::insert_map(
                    ctx,
                    &mut corners,
                    after_key,
                    node,
                    "catia_mesh_edge_port_corners",
                )?;
                node
            };
            if run.reversed {
                union.union(edge * 2 + 1, before_node);
                union.union(edge * 2, after_node);
            } else {
                union.union(edge * 2, before_node);
                union.union(edge * 2 + 1, after_node);
            }
        }
    }
    let mut roots = HashMap::new();
    let mut ports = Vec::new();
    for edge in 0..edge_rows.len() {
        let mut pair = [0u32; 2];
        for (side, node) in [edge * 2, edge * 2 + 1].into_iter().enumerate() {
            let root = union.find(node);
            let next = roots.len();
            let ordinal = if let Some(&existing) = roots.get(&root) {
                existing
            } else {
                crate::resource::insert_map(
                    ctx,
                    &mut roots,
                    root,
                    next,
                    "catia_mesh_edge_port_roots",
                )?;
                next
            };
            let Ok(value) = u32::try_from(ordinal) else {
                return Ok(None);
            };
            pair[side] = value;
        }
        crate::resource::push(ctx, &mut ports, pair, "catia_mesh_edge_ports")?;
    }
    Ok(Some(ports))
}

#[cfg(test)]
#[test]
fn mesh_edge_ports_refuse_each_graph_collection_limit() {
    use cadmpeg_core::decode::{DecodeArena, DecodePolicy, ResourceDimension};

    let analysis = StandardMeshAnalysis {
        edge_rows: vec![EdgeRow {
            kind: 0,
            handles: vec![0],
            boundary_layout:
                crate::families::standard::topology::EdgeBoundaryLayout::CompleteBoundaryRun,
        }],
        cycles: vec![vec![vec![0, 1]]],
        occurrences: vec![vec![MeshEdgeRun {
            edge: 0,
            face: 0,
            cycle: 0,
            start: 0,
            segment_count: 1,
            reversed: false,
        }]],
        fixed_complete_row_spans: false,
    };
    let arena = DecodeArena::new();
    let policy = DecodePolicy::service();
    let (ctx, _) = DecodeContext::from_root_bytes(&[0], &arena, &policy)
        .expect("fixture fits the input limit");
    assert_eq!(
        mesh_edge_ports(&ctx, &analysis, &[[10, 11]]).expect("service resource budget"),
        Some(vec![[0, 1]])
    );

    let mut operations = HashSet::new();
    for cap in 0..=32 {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_collection_items = cap;
        let (ctx, _) = DecodeContext::from_root_bytes(&[0], &arena, &policy)
            .expect("fixture fits the input limit");
        match mesh_edge_ports(&ctx, &analysis, &[[10, 11]]) {
            Err(CodecError::ResourceLimit(limit)) => {
                assert_eq!(limit.dimension, ResourceDimension::CollectionItems);
                operations.insert(limit.operation);
            }
            Ok(Some(_)) => {}
            Ok(None) => panic!("valid edge port graph must reconstruct"),
            Err(error) => panic!("unexpected edge port refusal: {error}"),
        }
    }
    for operation in [
        "catia_mesh_edge_port_union",
        "catia_mesh_edge_port_identities",
        "catia_mesh_edge_port_corner_nodes",
        "catia_mesh_edge_port_corners",
        "catia_mesh_edge_port_roots",
        "catia_mesh_edge_ports",
    ] {
        assert!(operations.contains(operation), "no refusal at {operation}");
    }
}

/// One exact occurrence of a physical edge row on a trim-mesh boundary.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct MeshEdgeRun {
    /// Physical edge-row ordinal.
    pub(crate) edge: usize,
    /// Positional face ordinal.
    pub(crate) face: usize,
    /// Boundary-cycle ordinal within the face.
    pub(crate) cycle: usize,
    /// First covered boundary-segment index in cycle traversal order.
    pub(crate) start: usize,
    /// Number of consecutive boundary segments covered by this occurrence.
    pub(crate) segment_count: usize,
    /// Whether cycle traversal follows the row's handle sequence in reverse.
    pub(crate) reversed: bool,
}

impl MeshEdgeRun {
    fn end(self, cycle_len: usize) -> usize {
        (self.start + self.segment_count) % cycle_len
    }
}

fn mesh_edge_occurrences(
    ctx: &DecodeContext<'_>,
    edge_rows: &[EdgeRow],
    cycles: &[Vec<Vec<u32>>],
) -> Result<Option<Vec<Vec<MeshEdgeRun>>>, CodecError> {
    let mut locations = HashMap::<u32, Vec<(usize, usize, usize)>>::new();
    for (face, face_cycles) in cycles.iter().enumerate() {
        for (cycle, handles) in face_cycles.iter().enumerate() {
            for (position, handle) in handles.iter().copied().enumerate() {
                if let Some(entries) = locations.get_mut(&handle) {
                    crate::resource::push(
                        ctx,
                        entries,
                        (face, cycle, position),
                        "catia_mesh_occurrence_locations",
                    )?;
                } else {
                    let mut entries = Vec::new();
                    crate::resource::push(
                        ctx,
                        &mut entries,
                        (face, cycle, position),
                        "catia_mesh_occurrence_locations",
                    )?;
                    crate::resource::insert_map(
                        ctx,
                        &mut locations,
                        handle,
                        entries,
                        "catia_mesh_occurrence_handles",
                    )?;
                }
            }
        }
    }
    let mut rows = Vec::new();
    for (edge, row) in edge_rows.iter().enumerate() {
        let Some(pattern) = row.boundary_pattern() else {
            crate::resource::push(ctx, &mut rows, Vec::new(), "catia_mesh_occurrence_rows")?;
            continue;
        };
        let Some(&first) = pattern.first() else {
            return Ok(None);
        };
        let Some(&last) = pattern.last() else {
            return Ok(None);
        };
        let mut matches = HashMap::<(usize, usize, usize), bool>::new();
        for &(face, cycle, start) in locations.get(&first).into_iter().flatten() {
            let handles = &cycles[face][cycle];
            if pattern
                .iter()
                .enumerate()
                .all(|(offset, handle)| handles[(start + offset) % handles.len()] == *handle)
            {
                crate::resource::insert_map(
                    ctx,
                    &mut matches,
                    (face, cycle, start),
                    false,
                    "catia_mesh_occurrence_matches",
                )?;
            }
        }
        for &(face, cycle, start) in locations.get(&last).into_iter().flatten() {
            let handles = &cycles[face][cycle];
            if pattern
                .iter()
                .rev()
                .enumerate()
                .all(|(offset, handle)| handles[(start + offset) % handles.len()] == *handle)
                && !matches.contains_key(&(face, cycle, start))
            {
                crate::resource::insert_map(
                    ctx,
                    &mut matches,
                    (face, cycle, start),
                    true,
                    "catia_mesh_occurrence_matches",
                )?;
            }
        }
        let mut cycle_counts = HashMap::new();
        for &(face, cycle, _) in matches.keys() {
            if let Some(count) = cycle_counts.get_mut(&(face, cycle)) {
                *count += 1;
            } else {
                crate::resource::insert_map(
                    ctx,
                    &mut cycle_counts,
                    (face, cycle),
                    1usize,
                    "catia_mesh_occurrence_cycle_counts",
                )?;
            }
        }
        if cycle_counts.values().any(|count| *count > 1) {
            return Ok(None);
        }
        let mut occurrences = Vec::new();
        for ((face, cycle, start), reversed) in matches {
            let cycle_len = cycles[face][cycle].len();
            let Some((start, segment_count)) = row.boundary_span(start, cycle_len) else {
                return Ok(None);
            };
            crate::resource::push(
                ctx,
                &mut occurrences,
                MeshEdgeRun {
                    edge,
                    face,
                    cycle,
                    start,
                    segment_count,
                    reversed,
                },
                "catia_mesh_occurrence_runs",
            )?;
        }
        occurrences.sort_by_key(|occurrence| (occurrence.face, occurrence.cycle, occurrence.start));
        crate::resource::push(ctx, &mut rows, occurrences, "catia_mesh_occurrence_rows")?;
    }
    Ok(Some(rows))
}

#[cfg(test)]
#[test]
fn mesh_edge_occurrences_refuse_nested_collection_limits() {
    use cadmpeg_core::decode::{DecodeArena, DecodePolicy, ResourceDimension};

    let rows = [EdgeRow {
        kind: 0,
        handles: vec![0],
        boundary_layout:
            crate::families::standard::topology::EdgeBoundaryLayout::CompleteBoundaryRun,
    }];
    let cycles = [vec![vec![0, 1]]];
    let arena = DecodeArena::new();
    let policy = DecodePolicy::service();
    let (ctx, _) = DecodeContext::from_root_bytes(&[0], &arena, &policy)
        .expect("fixture fits the input limit");
    assert!(mesh_edge_occurrences(&ctx, &rows, &cycles)
        .expect("service resource budget")
        .is_some());

    let mut operations = HashSet::new();
    for cap in 0..=32 {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_collection_items = cap;
        let (ctx, _) = DecodeContext::from_root_bytes(&[0], &arena, &policy)
            .expect("fixture fits the input limit");
        match mesh_edge_occurrences(&ctx, &rows, &cycles) {
            Err(CodecError::ResourceLimit(limit)) => {
                assert_eq!(limit.dimension, ResourceDimension::CollectionItems);
                operations.insert(limit.operation);
            }
            Ok(Some(_)) => {}
            Ok(None) => panic!("matching boundary handle must admit a run"),
            Err(error) => panic!("unexpected occurrence refusal: {error}"),
        }
    }
    for operation in [
        "catia_mesh_occurrence_handles",
        "catia_mesh_occurrence_locations",
        "catia_mesh_occurrence_matches",
        "catia_mesh_occurrence_cycle_counts",
        "catia_mesh_occurrence_runs",
        "catia_mesh_occurrence_rows",
    ] {
        assert!(operations.contains(operation), "no refusal at {operation}");
    }
}

#[derive(Debug)]
struct StandardMeshAnalysis {
    edge_rows: Vec<EdgeRow>,
    cycles: Vec<Vec<Vec<u32>>>,
    occurrences: Vec<Vec<MeshEdgeRun>>,
    fixed_complete_row_spans: bool,
}

fn standard_mesh_analysis(
    ctx: &DecodeContext<'_>,
    bytes: &[u8],
) -> Result<Option<StandardMeshAnalysis>, CodecError> {
    let Some(face_run) = selected_standard_run(ctx, bytes)? else {
        return Ok(None);
    };
    let face_start = face_run.face_start();
    let face_count = face_run.face_count();
    let after_faces = face_run.after_faces();
    let standard = parse_standard_edge_tables_with_width(ctx, bytes, after_faces)?;
    let parsed = if let Some((rows, _, width)) = standard {
        Some((rows, width, false))
    } else {
        parse_fbb_edge_tables(ctx, bytes, after_faces)?
            .map(|(rows, _, _, width)| (rows, width, true))
    };
    let Some((edge_rows, handle_width, fixed_complete_row_spans)) = parsed else {
        return Ok(None);
    };
    let Some(trims) = parse_trim_chain(ctx, bytes, face_start, face_count, handle_width)? else {
        return Ok(None);
    };
    let mut cycles = Vec::new();
    for trim in &trims {
        let Some(face) = boundary_cycles(ctx, trim.packet.triangles(ctx)?)? else {
            return Ok(None);
        };
        crate::resource::push(ctx, &mut cycles, face, "catia_mesh_analysis_cycles")?;
    }
    let Some(occurrences) = mesh_edge_occurrences(ctx, &edge_rows, &cycles)? else {
        return Ok(None);
    };
    Ok(Some(StandardMeshAnalysis {
        edge_rows,
        cycles,
        occurrences,
        fixed_complete_row_spans,
    }))
}

/// Recover every exact physical-edge occurrence on the trim mesh.
///
/// Standard `u16be` rows match their interior handles and include the two
/// flanking boundary segments. FBB `u24be` rows match their complete handle
/// sequence and cover one fewer segment than handles. A result exists only
/// when exactly one trim-handle width parses the complete face chain.
pub(crate) fn standard_mesh_edge_runs(
    ctx: &DecodeContext<'_>,
    bytes: &[u8],
) -> Result<Option<Vec<MeshEdgeRun>>, CodecError> {
    standard_mesh_analysis(ctx, bytes)?
        .map(|analysis| mesh_edge_runs(ctx, &analysis))
        .transpose()
}

fn mesh_edge_runs(
    ctx: &DecodeContext<'_>,
    analysis: &StandardMeshAnalysis,
) -> Result<Vec<MeshEdgeRun>, CodecError> {
    let mut runs = Vec::new();
    for run in analysis.occurrences.iter().flatten() {
        crate::resource::push(ctx, &mut runs, *run, "catia_mesh_edge_run_rows")?;
    }
    runs.sort_by_key(|run| (run.face, run.cycle, run.start, run.edge));
    Ok(runs)
}

/// Complete repeated standard edge-face slots from exact trim-boundary
/// occurrences. Rows without two distinct matched face occurrences retain
/// their serialized slots for incidence closure.
pub(crate) fn resolve_standard_edge_faces(
    ctx: &DecodeContext<'_>,
    bytes: &[u8],
    serialized: &[[usize; 2]],
) -> Result<Option<Vec<[usize; 2]>>, CodecError> {
    let Some(runs) = standard_mesh_edge_runs(ctx, bytes)? else {
        charge_collection_items(
            ctx,
            serialized.len(),
            "catia standard serialized edge faces",
        )?;
        return Ok(Some(serialized.to_vec()));
    };
    resolve_edge_faces_from_runs(ctx, serialized, &runs)
}

fn repeated_edge_face_handle_candidates_from_sets(
    ctx: &DecodeContext<'_>,
    edge_rows: &[EdgeRow],
    face_handles: &[HashSet<u32>],
    serialized: &[[usize; 2]],
) -> Result<Option<Vec<Vec<usize>>>, CodecError> {
    if edge_rows.len() != serialized.len()
        || serialized
            .iter()
            .flatten()
            .any(|face| *face >= face_handles.len())
    {
        return Ok(None);
    }
    for (row, faces) in edge_rows.iter().zip(serialized) {
        if !row
            .handles
            .iter()
            .all(|handle| face_handles[faces[0]].contains(handle))
        {
            return Ok(None);
        }
    }
    let mut candidates = ctx.alloc_filled(
        edge_rows.len(),
        Vec::new(),
        "catia_repeated_edge_handle_face_candidates",
    )?;
    for (edge, (row, faces)) in edge_rows.iter().zip(serialized).enumerate() {
        if faces[0] != faces[1] || row.handles.len() < 2 {
            continue;
        }
        let mut unique_handles = HashSet::new();
        crate::resource::reserve_set(
            ctx,
            &mut unique_handles,
            row.handles.len(),
            "catia repeated edge unique handles",
        )?;
        unique_handles.extend(row.handles.iter().copied());
        let mut matching = Vec::new();
        for (face, handles) in face_handles.iter().enumerate() {
            if face == faces[0] {
                continue;
            }
            let shared = unique_handles
                .iter()
                .filter(|handle| handles.contains(handle))
                .count();
            let qualifies = if unique_handles.len() >= 4 {
                shared >= 3 && shared >= unique_handles.len().div_ceil(2)
            } else {
                shared == unique_handles.len()
            };
            if qualifies {
                crate::resource::push(
                    ctx,
                    &mut matching,
                    face,
                    "catia repeated edge matching faces",
                )?;
            }
        }
        if unique_handles.len() >= 4 && matching.len() != 1 {
            matching.clear();
        }
        candidates[edge] = matching;
    }
    Ok(Some(candidates))
}

/// Return positive second-face candidates from the global trim-handle
/// namespace. The relation abstains for the complete file unless every edge
/// row is contained by its first serialized face packet.
pub(crate) fn standard_repeated_edge_face_handle_candidates(
    ctx: &DecodeContext<'_>,
    bytes: &[u8],
    serialized: &[[usize; 2]],
) -> Result<Option<Vec<Vec<usize>>>, CodecError> {
    let Some(face_run) = selected_standard_run(ctx, bytes)? else {
        return Ok(None);
    };
    let face_start = face_run.face_start();
    let face_count = face_run.face_count();
    let after_faces = face_run.after_faces();
    let standard = parse_standard_edge_tables_with_width(ctx, bytes, after_faces)?;
    let parsed = if let Some((rows, _, width)) = standard {
        Some((rows, width))
    } else {
        parse_fbb_edge_tables(ctx, bytes, after_faces)?.map(|(rows, _, _, width)| (rows, width))
    };
    let Some((edge_rows, handle_width)) = parsed else {
        return Ok(None);
    };
    let Some(trims) = parse_trim_chain(ctx, bytes, face_start, face_count, handle_width)? else {
        return Ok(None);
    };
    let trim_count = u64::try_from(trims.len()).map_err(|_| {
        ctx.refuse_codec_limit("catia repeated edge face handles", u64::MAX, u64::MAX)
    })?;
    ctx.charge_collection_items(trim_count, "catia repeated edge face handles")?;
    for trim in &trims {
        let handle_count = u64::try_from(trim.packet.handles().len()).map_err(|_| {
            ctx.refuse_codec_limit("catia repeated edge face handle set", u64::MAX, u64::MAX)
        })?;
        ctx.charge_collection_items(handle_count, "catia repeated edge face handle set")?;
    }
    let face_handles = trims
        .into_iter()
        .map(|trim| {
            trim.packet
                .handles()
                .iter()
                .copied()
                .collect::<HashSet<_>>()
        })
        .collect::<Vec<_>>();
    repeated_edge_face_handle_candidates_from_sets(ctx, &edge_rows, &face_handles, serialized)
}

/// Refine unresolved repeated-face domains with positive trim-handle evidence.
///
/// A row already resolved to two distinct faces has consumed its repeated slot;
/// later candidate sources cannot reopen it. For an unresolved row, common
/// carrier and handle candidates are preferred, while handle evidence supplies
/// the domain when carrier geometry abstains.
pub(crate) fn refine_repeated_edge_face_candidates(
    edge_faces: &[[usize; 2]],
    allowed_faces: &mut [Vec<usize>],
    handle_face_candidates: &[Vec<usize>],
) -> Option<()> {
    if edge_faces.len() != allowed_faces.len() || edge_faces.len() != handle_face_candidates.len() {
        return None;
    }
    for (edge, (allowed, handle_candidates)) in allowed_faces
        .iter_mut()
        .zip(handle_face_candidates)
        .enumerate()
    {
        if edge_faces[edge][0] != edge_faces[edge][1] {
            allowed.clear();
            continue;
        }
        if handle_candidates.is_empty() {
            continue;
        }
        let intersection = allowed
            .iter()
            .copied()
            .filter(|face| handle_candidates.contains(face))
            .collect::<Vec<_>>();
        *allowed = if intersection.is_empty() {
            handle_candidates.clone()
        } else {
            intersection
        };
    }
    Some(())
}

/// Resolve optional second-face incidences by complete endpoint-degree closure.
///
/// A repeated serialized pair contributes one use to its named face. Each
/// admitted alternate either remains unused or supplies a second distinct face.
/// Return every assignment found by the bounded search that gives degree two
/// at every used face vertex.
pub(crate) fn repeated_face_endpoint_closures(
    ctx: &DecodeContext<'_>,
    edge_faces: &[[usize; 2]],
    allowed_faces: &[Vec<usize>],
    endpoint_pairs: &[[usize; 2]],
    face_count: usize,
) -> Result<Option<Vec<Vec<[usize; 2]>>>, CodecError> {
    const MAX_STATES: usize = 65_536;
    const MAX_SOLUTIONS: usize = 4_096;

    struct PairDegreeUndo {
        start: (usize, Option<u8>),
        end: Option<(usize, Option<u8>)>,
    }

    fn add_pair(
        ctx: &DecodeContext<'_>,
        degrees: &mut BTreeMap<usize, u8>,
        pair: [usize; 2],
    ) -> Result<Option<PairDegreeUndo>, CodecError> {
        let start_add = 1 + u8::from(pair[0] == pair[1]);
        let Some(start_degree) = degrees
            .get(&pair[0])
            .copied()
            .unwrap_or_default()
            .checked_add(start_add)
        else {
            return Ok(None);
        };
        let end_degree = if pair[0] == pair[1] {
            Some(0)
        } else {
            degrees
                .get(&pair[1])
                .copied()
                .unwrap_or_default()
                .checked_add(1)
        };
        if start_degree > 2 || end_degree.is_none_or(|degree| degree > 2) {
            return Ok(None);
        }
        let start_previous = degrees.get(&pair[0]).copied();
        if start_previous.is_none() {
            charge_collection_items(ctx, 1, "catia missing-edge point degrees")?;
        }
        *degrees.entry(pair[0]).or_default() += start_add;
        let end_previous = if pair[0] == pair[1] {
            None
        } else {
            let previous = degrees.get(&pair[1]).copied();
            if previous.is_none() {
                charge_collection_items(ctx, 1, "catia missing-edge point degrees")?;
            }
            *degrees.entry(pair[1]).or_default() += 1;
            Some((pair[1], previous))
        };
        Ok(Some(PairDegreeUndo {
            start: (pair[0], start_previous),
            end: end_previous,
        }))
    }

    fn remove_pair(degrees: &mut BTreeMap<usize, u8>, undo: &PairDegreeUndo) {
        let (start, previous) = undo.start;
        match previous {
            Some(degree) => {
                degrees.insert(start, degree);
            }
            None => {
                degrees.remove(&start);
            }
        }
        if let Some((end, previous)) = undo.end {
            match previous {
                Some(degree) => {
                    degrees.insert(end, degree);
                }
                None => {
                    degrees.remove(&end);
                }
            }
        }
    }

    struct Search<'a, 'b> {
        ctx: &'a DecodeContext<'b>,
        branches: &'a [(usize, Vec<usize>)],
        owners: &'a [usize],
        endpoint_pairs: &'a [[usize; 2]],
        states: usize,
        exhausted: bool,
        solutions: Vec<Vec<usize>>,
    }

    impl Search<'_, '_> {
        fn visit(
            &mut self,
            degrees: &mut [BTreeMap<usize, u8>],
            assignment: &mut [usize],
            used: &mut [bool],
        ) -> Result<(), CodecError> {
            let _depth = self
                .ctx
                .enter_nested("catia missing-edge endpoint closure")?;
            if self.exhausted {
                return Ok(());
            }
            let Some(unassigned_branch) = used.iter().position(|used| !*used) else {
                if degrees
                    .iter()
                    .all(|face| face.values().all(|degree| *degree == 2))
                {
                    if self.solutions.len() == MAX_SOLUTIONS {
                        self.exhausted = true;
                    } else {
                        charge_collection_items(
                            self.ctx,
                            assignment.len(),
                            "catia missing-edge solution assignment",
                        )?;
                        charge_collection_items(self.ctx, 1, "catia missing-edge solution list")?;
                        self.solutions.push(assignment.to_vec());
                    }
                }
                return Ok(());
            };
            let deficit = degrees.iter().enumerate().find_map(|(face, points)| {
                points
                    .iter()
                    .find_map(|(&point, &degree)| (degree == 1).then_some((face, point)))
            });
            let mut choices = Vec::<(usize, usize)>::new();
            if let Some((face, point)) = deficit {
                for (branch, (edge, faces)) in self.branches.iter().enumerate() {
                    if used[branch] || !self.endpoint_pairs[*edge].contains(&point) {
                        continue;
                    }
                    for &candidate in faces.iter().filter(|candidate| **candidate == face) {
                        charge_collection_items(self.ctx, 1, "catia missing-edge search choices")?;
                        choices.push((branch, candidate));
                    }
                }
            } else {
                charge_collection_items(
                    self.ctx,
                    self.branches[unassigned_branch].1.len(),
                    "catia missing-edge search choices",
                )?;
                choices.extend(
                    self.branches[unassigned_branch]
                        .1
                        .iter()
                        .copied()
                        .map(|face| (unassigned_branch, face)),
                );
            }
            for (branch, face) in choices {
                self.ctx
                    .charge_work(1, "catia missing-edge endpoint closure")?;
                if self.states >= MAX_STATES {
                    self.exhausted = true;
                    return Ok(());
                }
                self.states += 1;
                let (edge, _) = &self.branches[branch];
                let owner = self.owners[branch];
                let adds_incidence = face != owner;
                let undo = if adds_incidence {
                    let Some(undo) =
                        add_pair(self.ctx, &mut degrees[face], self.endpoint_pairs[*edge])?
                    else {
                        continue;
                    };
                    Some(undo)
                } else {
                    None
                };
                assignment[branch] = face;
                used[branch] = true;
                self.visit(degrees, assignment, used)?;
                used[branch] = false;
                if let Some(undo) = undo {
                    remove_pair(&mut degrees[face], &undo);
                }
                if self.exhausted {
                    return Ok(());
                }
            }
            Ok(())
        }
    }

    if edge_faces.len() != allowed_faces.len()
        || edge_faces.len() != endpoint_pairs.len()
        || edge_faces.iter().flatten().any(|face| *face >= face_count)
        || allowed_faces
            .iter()
            .flatten()
            .any(|face| *face >= face_count)
    {
        return Ok(None);
    }
    let mut degrees = ctx.alloc_filled(
        face_count,
        BTreeMap::<usize, u8>::new(),
        "catia missing-edge face degrees",
    )?;
    for (edge, faces) in edge_faces.iter().copied().enumerate() {
        if add_pair(ctx, &mut degrees[faces[0]], endpoint_pairs[edge])?.is_none() {
            return Ok(None);
        }
        if faces[1] != faces[0]
            && add_pair(ctx, &mut degrees[faces[1]], endpoint_pairs[edge])?.is_none()
        {
            return Ok(None);
        }
    }
    let mut branches = Vec::new();
    for (edge, faces) in edge_faces.iter().enumerate() {
        if faces[0] != faces[1] || allowed_faces[edge].is_empty() {
            continue;
        }
        charge_collection_items(ctx, 1, "catia missing-edge branch choices")?;
        let mut choices = vec![faces[0]];
        charge_collection_items(
            ctx,
            allowed_faces[edge]
                .iter()
                .filter(|face| **face != faces[0])
                .count(),
            "catia missing-edge branch choices",
        )?;
        choices.extend(
            allowed_faces[edge]
                .iter()
                .copied()
                .filter(|face| *face != faces[0]),
        );
        choices.sort_unstable();
        choices.dedup();
        charge_collection_items(ctx, 1, "catia missing-edge branches")?;
        branches.push((edge, choices));
    }
    if branches.is_empty() {
        let closed = degrees
            .iter()
            .all(|face| face.values().all(|degree| *degree == 2));
        if closed {
            charge_collection_items(ctx, edge_faces.len(), "catia missing-edge closed faces")?;
            charge_collection_items(ctx, 1, "catia missing-edge closed solutions")?;
        }
        return Ok(Some(
            closed.then(|| edge_faces.to_vec()).into_iter().collect(),
        ));
    }
    charge_collection_items(ctx, branches.len(), "catia missing-edge branch owners")?;
    let owners = branches
        .iter()
        .map(|(edge, _)| edge_faces[*edge][0])
        .collect::<Vec<_>>();
    let mut search = Search {
        ctx,
        branches: &branches,
        owners: &owners,
        endpoint_pairs,
        states: 0,
        exhausted: false,
        solutions: Vec::new(),
    };
    let mut assignment =
        ctx.alloc_filled(branches.len(), 0, "catia missing-edge branch assignment")?;
    let mut used = ctx.alloc_filled(branches.len(), false, "catia missing-edge used branches")?;
    search.visit(&mut degrees, &mut assignment, &mut used)?;
    if search.exhausted {
        return Ok(None);
    }
    charge_collection_items(
        ctx,
        search.solutions.len(),
        "catia missing-edge completed solutions",
    )?;
    for _ in &search.solutions {
        charge_collection_items(ctx, edge_faces.len(), "catia missing-edge completed faces")?;
    }
    Ok(Some(
        search
            .solutions
            .into_iter()
            .map(|solution| {
                let mut completed = edge_faces.to_vec();
                for ((edge, _), face) in branches.iter().zip(solution) {
                    completed[*edge][1] = face;
                }
                completed
            })
            .collect(),
    ))
}

/// The admitted second faces for one repeated edge incidence slot.
///
/// The serialized face is always retained, so the set is never empty. The type
/// states that: it holds the smallest admitted face and the ascending
/// remainder, and there is no spelling of it with no face at all.
struct FaceOptions {
    /// The smallest admitted face.
    first: usize,
    /// The remaining admitted faces, ascending and all greater than `first`.
    rest: Vec<usize>,
}

impl FaceOptions {
    /// The slot's options: `retained`, which is always admitted, together with
    /// `admitted` in ascending order and without repeats.
    ///
    /// `admitted` states the faces the slot allows, in any order, with or
    /// without repeats, and with or without `retained` among them.
    fn from_admitted(
        ctx: &DecodeContext<'_>,
        retained: usize,
        admitted: impl IntoIterator<Item = usize>,
    ) -> Result<Self, CodecError> {
        let mut others = Vec::new();
        for face in admitted.into_iter().filter(|face| *face != retained) {
            crate::resource::push(ctx, &mut others, face, "catia_duplicate_face_options")?;
        }
        others.sort_unstable();
        others.dedup();
        let at = others.partition_point(|face| *face < retained);
        if at == 0 {
            return Ok(Self {
                first: retained,
                rest: others,
            });
        }
        // `others[0]` is smaller than `retained`, so it is the smallest face.
        // Removing it shifts the insertion point of `retained` down by one.
        let first = others.remove(0);
        others.insert(at - 1, retained);
        Ok(Self {
            first,
            rest: others,
        })
    }

    /// How many faces this slot admits. Never zero.
    fn count(&self) -> usize {
        self.rest.len() + 1
    }

    /// The admitted faces, ascending.
    fn iter(&self) -> impl Iterator<Item = usize> + '_ {
        std::iter::once(self.first).chain(self.rest.iter().copied())
    }
}

pub(super) fn unique_duplicate_face_assignment<F>(
    ctx: &DecodeContext<'_>,
    serialized: &[[usize; 2]],
    allowed_faces: &[Vec<usize>],
    face_count: usize,
    mut valid: F,
) -> Result<Option<Vec<[usize; 2]>>, CodecError>
where
    F: FnMut(&[[usize; 2]]) -> Result<bool, CodecError>,
{
    const MAX_STATES: usize = 4_096;

    fn search<F>(
        ctx: &DecodeContext<'_>,
        branches: &[(usize, FaceOptions)],
        at: usize,
        assignment: &mut [[usize; 2]],
        states: &mut usize,
        exhausted: &mut bool,
        solutions: &mut Vec<Vec<[usize; 2]>>,
        valid: &mut F,
    ) -> Result<(), CodecError>
    where
        F: FnMut(&[[usize; 2]]) -> Result<bool, CodecError>,
    {
        if *exhausted || solutions.len() > 1 {
            return Ok(());
        }
        if at == branches.len() {
            if valid(assignment)? && !solutions.iter().any(|solution| solution == assignment) {
                let solution = crate::resource::copy_retained_slice(ctx, assignment, "catia_duplicate_face_solution")?;
                crate::resource::push(ctx, solutions, solution, "catia_duplicate_face_solution_rows")?;
            }
            return Ok(());
        }
        if *states >= MAX_STATES {
            *exhausted = true;
            return Ok(());
        }
        *states += 1;
        let (edge, options) = &branches[at];
        for face in options.iter() {
            assignment[*edge][1] = face;
            search(
                ctx,
                branches,
                at + 1,
                assignment,
                states,
                exhausted,
                solutions,
                valid,
            )?;
            if *exhausted || solutions.len() > 1 {
                return Ok(());
            }
        }
        Ok(())
    }

    if serialized.len() != allowed_faces.len()
        || serialized.iter().flatten().any(|face| *face >= face_count)
        || allowed_faces
            .iter()
            .flatten()
            .any(|face| *face >= face_count)
    {
        return Ok(None);
    }
    let mut unresolved = Vec::new();
    for (edge, faces) in serialized.iter().enumerate() {
        if faces[0] == faces[1] {
            crate::resource::push(ctx, &mut unresolved, edge, "catia_duplicate_face_unresolved")?;
        }
    }
    if unresolved.is_empty() {
        return Ok(Some(crate::resource::copy_retained_slice(ctx, serialized, "catia_duplicate_face_serialized")?));
    }
    let mut assignment = crate::resource::copy_retained_slice(ctx, serialized, "catia_duplicate_face_serialized")?;
    let mut branches = Vec::new();
    for edge in unresolved {
        let retained = assignment[edge][0];
        let options = FaceOptions::from_admitted(ctx, retained, allowed_faces[edge].iter().copied())?;
        if options.rest.is_empty() {
            assignment[edge][1] = options.first;
        } else {
            crate::resource::push(ctx, &mut branches, (edge, options), "catia_duplicate_face_branches")?;
        }
    }
    branches.sort_unstable_by_key(|(edge, options)| (options.count(), *edge));
    let mut states = 0;
    let mut exhausted = false;
    let mut solutions = Vec::new();
    search(
        ctx,
        &branches,
        0,
        &mut assignment,
        &mut states,
        &mut exhausted,
        &mut solutions,
        &mut valid,
    )?;
    Ok((!exhausted)
        .then(|| <[Vec<[usize; 2]>; 1]>::try_from(solutions).ok())
        .flatten()
        .map(|[solution]| solution))
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum DuplicateFaceAssignmentVisit {
    Complete,
    Stopped,
    Exhausted,
}

/// Visit concrete second-face assignments without materializing their product.
///
/// A repeated slot retains its serialized one-face incidence as one choice and
/// adds each distinct admitted second face as another. The visitor returns
/// `true` to continue and `false` to stop after a solved or terminal result.
pub(super) fn visit_duplicate_face_assignments<F>(
    ctx: &DecodeContext<'_>,
    serialized: &[[usize; 2]],
    allowed_faces: &[Vec<usize>],
    face_count: usize,
    max_assignments: usize,
    mut visitor: F,
) -> Result<Option<DuplicateFaceAssignmentVisit>, cadmpeg_core::CodecError>
where
    F: FnMut(&[[usize; 2]]) -> Result<bool, cadmpeg_core::CodecError>,
{
    fn visit<F>(
        ctx: &DecodeContext<'_>,
        branches: &[(usize, Vec<usize>)],
        at: usize,
        assignment: &mut [[usize; 2]],
        max_assignments: usize,
        visited: &mut usize,
        visitor: &mut F,
    ) -> Result<DuplicateFaceAssignmentVisit, cadmpeg_core::CodecError>
    where
        F: FnMut(&[[usize; 2]]) -> Result<bool, cadmpeg_core::CodecError>,
    {
        let _depth = ctx.enter_nested("catia_duplicate_face_visit_depth")?;
        ctx.charge_work(1, "catia_duplicate_face_visit_work")?;
        if at == branches.len() {
            if *visited >= max_assignments {
                return Ok(DuplicateFaceAssignmentVisit::Exhausted);
            }
            *visited += 1;
            return Ok(if visitor(assignment)? {
                DuplicateFaceAssignmentVisit::Complete
            } else {
                DuplicateFaceAssignmentVisit::Stopped
            });
        }
        let (edge, choices) = &branches[at];
        for &face in choices {
            assignment[*edge][1] = face;
            match visit(
                ctx,
                branches,
                at + 1,
                assignment,
                max_assignments,
                visited,
                visitor,
            )? {
                DuplicateFaceAssignmentVisit::Complete => {}
                terminal => return Ok(terminal),
            }
        }
        Ok(DuplicateFaceAssignmentVisit::Complete)
    }

    if serialized.len() != allowed_faces.len()
        || serialized.iter().flatten().any(|face| *face >= face_count)
        || allowed_faces
            .iter()
            .flatten()
            .any(|face| *face >= face_count)
    {
        return Ok(None);
    }
    let mut assignment = crate::resource::copy_retained_slice(ctx, serialized, "catia_duplicate_visit_assignment")?;
    let mut branches = Vec::<(usize, Vec<usize>)>::new();
    for (edge, faces) in serialized.iter().enumerate() {
        let allowed = &allowed_faces[edge];
        if faces[0] != faces[1] {
            if !allowed.is_empty() {
                return Ok(None);
            }
            continue;
        }
        let mut choices = ctx.alloc_filled(1, faces[0], "catia_duplicate_visit_choices")?;
        for face in allowed.iter().copied().filter(|face| *face != faces[0]) {
            crate::resource::push(ctx, &mut choices, face, "catia_duplicate_visit_choices")?;
        }
        choices.sort_unstable();
        choices.dedup();
        if choices.len() > 1 {
            crate::resource::push(ctx, &mut branches, (edge, choices), "catia_duplicate_visit_branches")?;
        }
    }
    branches.sort_unstable_by_key(|(edge, choices)| (choices.len(), *edge));

    let mut visited = 0;
    Ok(Some(visit(
        ctx,
        &branches,
        0,
        &mut assignment,
        max_assignments,
        &mut visited,
        &mut visitor,
    )?))
}

/// Complete repeated standard edge-face slots when carrier incidence and a
/// complete trim-boundary partition select one common assignment.
pub(crate) fn resolve_standard_duplicate_edge_faces(
    ctx: &DecodeContext<'_>,
    bytes: &[u8],
    serialized: &[[usize; 2]],
    allowed_faces: &[Vec<usize>],
) -> Result<Option<Vec<[usize; 2]>>, CodecError> {
    let Some(face_run) = selected_standard_run(ctx, bytes)? else {
        return Ok(None);
    };
    let face_count = face_run.face_count();
    let context = StandardMeshBoundaryContext::parse(ctx, bytes, serialized)?;
    unique_duplicate_face_assignment(ctx, serialized, allowed_faces, face_count, |assignment| {
        if let Some(base) = context.as_ref() {
            let Some(context) = base.with_edge_faces(ctx, assignment)? else {
                return Ok(false);
            };
            Ok(standard_mesh_boundary_assignments_from_context(ctx, &context, None)?.is_some())
        } else {
            Ok(standard_mesh_boundary_assignments(ctx, bytes, assignment, None)?.is_some())
        }
    })
}

pub(super) fn resolve_edge_faces_from_runs(
    ctx: &DecodeContext<'_>,
    serialized: &[[usize; 2]],
    runs: &[MeshEdgeRun],
) -> Result<Option<Vec<[usize; 2]>>, CodecError> {
    let mut occurrence_faces =
        ctx.alloc_filled(serialized.len(), Vec::new(), "catia_edge_run_faces")?;
    for run in runs {
        let Some(faces) = occurrence_faces.get_mut(run.edge) else {
            return Ok(None);
        };
        if !faces.contains(&run.face) {
            crate::resource::push(ctx, faces, run.face, "catia edge run occurrence faces")?;
        }
    }
    charge_collection_items(ctx, serialized.len(), "catia resolved edge faces")?;
    let mut resolved = serialized.to_vec();
    for (faces, occurrences) in resolved.iter_mut().zip(occurrence_faces) {
        if faces[0] != faces[1] || occurrences.len() < 2 {
            continue;
        }
        // Repeated row handles can match more than one face. The occurrence
        // set is then a domain, not a serialized face assignment; retain the
        // duplicate slot for native ownership and endpoint closure to resolve.
        if occurrences.len() > 2 {
            continue;
        }
        if !occurrences.contains(&faces[0]) {
            return Ok(None);
        }
        let Some(face) = occurrences.iter().find(|face| **face != faces[0]) else {
            return Ok(None);
        };
        faces[1] = *face;
    }
    Ok(Some(resolved))
}

/// One uncovered run in a trim-mesh boundary cycle.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct MeshBoundaryGap {
    /// Boundary-cycle ordinal within the face.
    cycle: usize,
    /// First uncovered boundary-segment index.
    start: usize,
    /// Number of consecutive uncovered boundary segments.
    pub(super) length: usize,
}

/// Exact matched and unmatched physical-edge coverage for one trim face.
#[derive(Debug, Clone, PartialEq, Eq)]
struct MeshFaceCoverage {
    /// Positional face ordinal.
    pub(super) face: usize,
    /// Maximal uncovered runs after matching every serialized edge interior.
    pub(super) gaps: Vec<MeshBoundaryGap>,
    /// Incident physical-edge rows with no interior occurrence on this face.
    pub(super) missing_edges: Vec<usize>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum MeshFaceAssignmentDomain {
    Ordered(Vec<Vec<MeshEdgePlacementCandidate>>),
    UnorderedFullCycle(Vec<usize>),
    DeferredValidation(MeshFaceCoverage),
}

#[derive(Debug, Clone)]
pub(super) struct StandardMeshBoundaryContext {
    analysis: Arc<StandardMeshAnalysis>,
    coverage: Vec<MeshFaceCoverage>,
    edge_ports: Vec<[u32; 2]>,
    edge_runs: Vec<MeshEdgeRun>,
    cycle_lengths: Vec<Vec<usize>>,
}

impl StandardMeshBoundaryContext {
    fn parse(
        ctx: &DecodeContext<'_>,
        bytes: &[u8],
        edge_faces: &[[usize; 2]],
    ) -> Result<Option<Self>, CodecError> {
        Self::parse_ports(ctx, bytes, edge_faces, false)
    }

    pub(super) fn parse_ports(
        ctx: &DecodeContext<'_>,
        bytes: &[u8],
        edge_faces: &[[usize; 2]],
        global_handle_ports: bool,
    ) -> Result<Option<Self>, CodecError> {
        let Some(analysis) = standard_mesh_analysis(ctx, bytes)? else {
            return Ok(None);
        };
        let analysis = Arc::new(analysis);
        if analysis.edge_rows.len() != edge_faces.len() {
            return Ok(None);
        }
        let Some(coverage) = mesh_face_coverage(ctx, &analysis, edge_faces)? else {
            return Ok(None);
        };
        let Some(local_ports) = solver_ports(ctx, bytes, global_handle_ports)? else {
            return Ok(None);
        };
        let Some(edge_ports) = mesh_edge_ports(ctx, &analysis, &local_ports)? else {
            return Ok(None);
        };
        let edge_runs = mesh_edge_runs(ctx, &analysis)?;
        let mut cycle_lengths = Vec::new();
        crate::resource::reserve_vec(ctx, &mut cycle_lengths, analysis.cycles.len(), "catia_mesh_cycle_length_rows")?;
        for cycles in &analysis.cycles {
            let mut lengths = Vec::new();
            crate::resource::reserve_vec(ctx, &mut lengths, cycles.len(), "catia_mesh_cycle_lengths")?;
            lengths.extend(cycles.iter().map(Vec::len));
            cycle_lengths.push(lengths);
        }
        Ok(Some(Self {
            analysis,
            coverage,
            edge_ports,
            edge_runs,
            cycle_lengths,
        }))
    }

    fn with_edge_faces(
        &self,
        ctx: &DecodeContext<'_>,
        edge_faces: &[[usize; 2]],
    ) -> Result<Option<Self>, CodecError> {
        let Some(coverage) = mesh_face_coverage(ctx, &self.analysis, edge_faces)? else {
            return Ok(None);
        };
        Ok(Some(Self {
            analysis: Arc::clone(&self.analysis),
            coverage,
            edge_ports: crate::resource::copy_retained_slice(ctx, &self.edge_ports, "catia_mesh_context_edge_ports")?,
            edge_runs: crate::resource::copy_retained_slice(ctx, &self.edge_runs, "catia_mesh_context_edge_runs")?,
            cycle_lengths: crate::resource::copy_retained_rows(ctx, &self.cycle_lengths, "catia_mesh_context_cycle_rows", "catia_mesh_context_cycle_lengths")?,
        }))
    }
}

/// One admissible placement of an unmatched physical edge within a recovered
/// trim-boundary gap. Domains contain only placements participating in a
/// complete end-to-end partition of every gap on the face.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
struct MeshEdgePlacementCandidate {
    /// Physical edge-row ordinal.
    pub(super) edge: usize,
    /// Positional face ordinal.
    face: usize,
    /// Boundary-cycle ordinal within the face.
    cycle: usize,
    /// First covered boundary-segment index.
    start: usize,
    /// Number of consecutive boundary segments covered by the edge.
    pub(super) segment_count: usize,
}

impl MeshEdgePlacementCandidate {
    fn end(self, cycle_len: usize) -> usize {
        (self.start + self.segment_count) % cycle_len
    }
}

/// One placement within a complete face assignment, together with the point
/// pairs allowed by its two currently bound trim corners. An absent domain
/// means that at least one corner has no exact point binding.
#[derive(Debug, Clone, PartialEq, Eq)]
struct MeshEdgePlacementEndpointCandidate {
    /// Span-consistent placement in its face boundary.
    placement: MeshEdgePlacementCandidate,
    /// Unordered logical-point pairs allowed at the placement corners.
    pub(super) endpoint_pairs: Option<Vec<[usize; 2]>>,
}

/// One physical-edge use in a complete candidate trim boundary.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct MeshBoundaryEdgeCandidate {
    /// Physical edge-row ordinal.
    pub(crate) edge: usize,
    /// Boundary-segment index at which the use begins.
    pub(crate) start: usize,
    /// Boundary-segment index immediately after the use.
    pub(crate) end: usize,
    /// Stored-row direction when an interior handle sequence fixes it.
    pub(crate) reversed: Option<bool>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum MeshFaceBoundaryDomain {
    Ordered(Vec<MeshFaceBoundaryAssignment>),
    UnorderedFullCycle(Vec<usize>),
    DeferredValidation(MeshDeferredFaceBoundary),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct MeshDeferredFaceBoundary {
    pub(crate) cycles: Vec<MeshDeferredBoundaryCycle>,
    pub(crate) missing_edges: Vec<usize>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct MeshDeferredBoundaryCycle {
    pub(crate) length: usize,
    pub(crate) exact_uses: Vec<(MeshBoundaryEdgeCandidate, usize)>,
}

/// One complete choice of all unmatched placements on a face, expressed as
/// ordered physical-edge uses for each serialized trim cycle.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct MeshFaceBoundaryAssignment {
    /// Trim cycles in serialized cycle order.
    pub(crate) boundaries: Vec<Vec<MeshBoundaryEdgeCandidate>>,
}

/// Recover exact face-local mesh coverage without assigning unmatched edge rows
/// to gaps. A result exists only for a unique trim-handle width and when every
/// matched interior occurs on one of its two serialized incident faces.
#[cfg(test)]
fn standard_mesh_face_coverage(
    ctx: &DecodeContext<'_>,
    bytes: &[u8],
    edge_faces: &[[usize; 2]],
) -> Result<Option<Vec<MeshFaceCoverage>>, CodecError> {
    let Some(analysis) = standard_mesh_analysis(ctx, bytes)? else {
        return Ok(None);
    };
    mesh_face_coverage(ctx, &analysis, edge_faces)
}

fn mesh_face_coverage(
    ctx: &DecodeContext<'_>,
    analysis: &StandardMeshAnalysis,
    edge_faces: &[[usize; 2]],
) -> Result<Option<Vec<MeshFaceCoverage>>, CodecError> {
    let edge_rows = &analysis.edge_rows;
    let cycles = &analysis.cycles;
    let occurrences = &analysis.occurrences;
    if edge_rows.len() != edge_faces.len() {
        return Ok(None);
    }
    if occurrences.iter().enumerate().any(|(edge, values)| {
        values
            .iter()
            .any(|occurrence| !edge_faces[edge].contains(&occurrence.face))
    }) {
        return Ok(None);
    }
    let mut occurrences_by_cycle = Vec::new();
    for face_cycles in cycles {
        let rows = ctx.alloc_filled(
            face_cycles.len(),
            Vec::<MeshEdgeRun>::new(),
            "catia_mesh_cycle_occurrences",
        )?;
        crate::resource::push(
            ctx,
            &mut occurrences_by_cycle,
            rows,
            "catia_mesh_occurrence_faces",
        )?;
    }
    let mut present_edges_by_face = ctx.alloc_filled(
        cycles.len(),
        HashSet::<usize>::new(),
        "catia_mesh_face_edges",
    )?;
    for values in occurrences {
        for &occurrence in values {
            let Some(face_cycles) = occurrences_by_cycle.get_mut(occurrence.face) else {
                return Ok(None);
            };
            let Some(cycle_occurrences) = face_cycles.get_mut(occurrence.cycle) else {
                return Ok(None);
            };
            crate::resource::push(
                ctx,
                cycle_occurrences,
                occurrence,
                "catia_mesh_cycle_occurrence_entries",
            )?;
            crate::resource::insert_set(
                ctx,
                &mut present_edges_by_face[occurrence.face],
                occurrence.edge,
                "catia_mesh_present_face_edges",
            )?;
        }
    }
    let mut edges_by_face =
        ctx.alloc_filled(cycles.len(), Vec::new(), "catia_mesh_edges_by_face")?;
    for (edge, faces) in edge_faces.iter().copied().enumerate() {
        for face in faces {
            if face >= cycles.len() {
                return Ok(None);
            }
        }
        crate::resource::push(
            ctx,
            &mut edges_by_face[faces[0]],
            edge,
            "catia_mesh_face_edge_entries",
        )?;
        if faces[1] != faces[0] {
            crate::resource::push(
                ctx,
                &mut edges_by_face[faces[1]],
                edge,
                "catia_mesh_face_edge_entries",
            )?;
        }
    }
    let mut coverage = Vec::new();
    for (face, face_cycles) in cycles.iter().enumerate() {
        let mut gaps = Vec::new();
        for (cycle_index, cycle) in face_cycles.iter().enumerate() {
            let mut covered = ctx.alloc_filled(cycle.len(), false, "catia_mesh_cycle_coverage")?;
            for occurrence in &occurrences_by_cycle[face][cycle_index] {
                let start = occurrence.start;
                let segment_count = occurrence.segment_count;
                for offset in 0..segment_count {
                    let slot = &mut covered[(start + offset) % cycle.len()];
                    if *slot {
                        return Ok(None);
                    }
                    *slot = true;
                }
            }
            if covered.iter().all(|value| !*value) {
                crate::resource::push(
                    ctx,
                    &mut gaps,
                    MeshBoundaryGap {
                        cycle: cycle_index,
                        start: 0,
                        length: cycle.len(),
                    },
                    "catia_mesh_coverage_gaps",
                )?;
            } else {
                for start in (0..covered.len()).filter(|&index| {
                    !covered[index] && covered[(index + covered.len() - 1) % covered.len()]
                }) {
                    let length = (0..covered.len())
                        .take_while(|offset| !covered[(start + offset) % covered.len()])
                        .count();
                    crate::resource::push(
                        ctx,
                        &mut gaps,
                        MeshBoundaryGap {
                            cycle: cycle_index,
                            start,
                            length,
                        },
                        "catia_mesh_coverage_gaps",
                    )?;
                }
            }
        }
        let mut missing_edges = Vec::new();
        for edge in edges_by_face[face]
            .iter()
            .copied()
            .filter(|edge| !present_edges_by_face[face].contains(edge))
        {
            crate::resource::push(
                ctx,
                &mut missing_edges,
                edge,
                "catia_mesh_coverage_missing_edges",
            )?;
        }
        crate::resource::push(
            ctx,
            &mut coverage,
            MeshFaceCoverage {
                face,
                gaps,
                missing_edges,
            },
            "catia_mesh_coverage_faces",
        )?;
    }
    Ok(Some(coverage))
}

pub(crate) fn bounded_oriented_trail_orders(
    ctx: &DecodeContext<'_>,
    trails: &[Vec<usize>],
    limit: usize,
) -> Result<Option<Vec<Vec<usize>>>, CodecError> {
    fn visit(
        ctx: &DecodeContext<'_>,
        trails: &[Vec<usize>],
        limit: usize,
        used: u64,
        edges: &mut Vec<usize>,
        orders: &mut Vec<Vec<usize>>,
    ) -> Result<bool, CodecError> {
        let _depth = ctx.enter_nested("catia_oriented_trail_order_depth")?;
        ctx.charge_work(1, "catia_oriented_trail_order_work")?;
        if orders.len() > limit {
            return Ok(false);
        }
        if used.count_ones() as usize == trails.len() {
            let order = crate::resource::copy_retained_slice(ctx, edges, "catia_oriented_trail_order_copy")?;
            crate::resource::push(ctx, orders, order, "catia_oriented_trail_orders")?;
            return Ok(orders.len() <= limit);
        }
        for (index, trail) in trails.iter().enumerate() {
            if used & (1 << index) != 0 {
                continue;
            }
            for reversed in [false, true] {
                if reversed && trail.len() == 1 {
                    continue;
                }
                let before = edges.len();
                if reversed {
                    edges.extend(trail.iter().rev());
                } else {
                    edges.extend(trail);
                }
                if !visit(ctx, trails, limit, used | (1 << index), edges, orders)? {
                    return Ok(false);
                }
                edges.truncate(before);
            }
        }
        Ok(true)
    }

    if trails.len() > u64::BITS as usize {
        return Ok(None);
    }
    let Some(edge_count) = trails.iter().try_fold(0usize, |total, trail| total.checked_add(trail.len())) else {
        return Err(ctx.refuse_codec_limit("catia_oriented_trail_scratch", u64::MAX, u64::MAX));
    };
    let mut edges = Vec::new();
    crate::resource::reserve_vec(ctx, &mut edges, edge_count, "catia_oriented_trail_scratch")?;
    let mut orders = Vec::new();
    Ok(visit(ctx, trails, limit, 0, &mut edges, &mut orders)?.then_some(orders))
}

pub(crate) fn bounded_endpoint_cycle_orders(
    ctx: &DecodeContext<'_>,
    missing: &[usize],
    edge_candidates: &[Vec<[usize; 2]>],
    limit: usize,
) -> Result<Option<Vec<Vec<usize>>>, CodecError> {
    struct Search<'a> {
        ctx: &'a DecodeContext<'a>,
        missing: &'a [usize],
        transitions: &'a HashMap<usize, Vec<(usize, usize)>>,
        limit: usize,
        operations_left: usize,
        orders: HashSet<Vec<usize>>,
    }

    impl Search<'_> {
        fn walk(
            &mut self,
            first_point: usize,
            current_point: usize,
            used: u64,
            order: &mut Vec<usize>,
        ) -> Result<bool, CodecError> {
            let _depth = self.ctx.enter_nested("catia_endpoint_cycle_order_depth")?;
            self.ctx.charge_work(1, "catia_endpoint_cycle_order_work")?;
            let Some(operations_left) = self.operations_left.checked_sub(1) else {
                return Ok(false);
            };
            self.operations_left = operations_left;
            if order.len() == self.missing.len() {
                if current_point == first_point {
                    if !self.orders.contains(order) {
                        let saved = crate::resource::copy_retained_slice(self.ctx, order, "catia_endpoint_cycle_order_copy")?;
                        crate::resource::insert_set(self.ctx, &mut self.orders, saved, "catia_endpoint_cycle_orders")?;
                    }
                }
                return Ok(self.orders.len() <= self.limit);
            }
            let Some(transition_count) = self.transitions.get(&current_point).map(Vec::len) else {
                return Ok(true);
            };
            for index in 0..transition_count {
                let (rank, next_point) = self.transitions[&current_point][index];
                let Some(operations_left) = self.operations_left.checked_sub(1) else {
                    return Ok(false);
                };
                self.operations_left = operations_left;
                if used & (1 << rank) != 0 {
                    continue;
                }
                order.push(self.missing[rank]);
                if !self.walk(first_point, next_point, used | (1 << rank), order)? {
                    return Ok(false);
                }
                order.pop();
            }
            Ok(true)
        }
    }

    if missing.is_empty()
        || missing.len() > u64::BITS as usize
        || missing
            .iter()
            .any(|&edge| edge_candidates.get(edge).is_none_or(Vec::is_empty))
    {
        return Ok(None);
    }
    let mut missing = crate::resource::copy_slice(ctx, missing, "catia_endpoint_cycle_missing_edges")?;
    missing.sort_unstable();
    let first_edge = missing[0];
    let mut transitions = HashMap::<usize, Vec<(usize, usize)>>::new();
    for (rank, &edge) in missing.iter().enumerate().skip(1) {
        for &[left, right] in &edge_candidates[edge] {
            crate::resource::admit_map_entry(ctx, &mut transitions, &left, "catia_endpoint_cycle_transition_points")?;
            crate::resource::push(ctx, transitions.entry(left).or_default(), (rank, right), "catia_endpoint_cycle_transition_steps")?;
            if left != right {
                crate::resource::admit_map_entry(ctx, &mut transitions, &right, "catia_endpoint_cycle_transition_points")?;
                crate::resource::push(ctx, transitions.entry(right).or_default(), (rank, left), "catia_endpoint_cycle_transition_steps")?;
            }
        }
    }
    for values in transitions.values_mut() {
        values.sort_unstable();
        values.dedup();
    }
    let mut search = Search {
        ctx,
        missing: &missing,
        transitions: &transitions,
        limit,
        operations_left: match limit.checked_mul(16) {
            Some(operations) => operations,
            None => return Ok(None),
        },
        orders: HashSet::new(),
    };
    let mut first_pairs = crate::resource::copy_slice(ctx, &edge_candidates[first_edge], "catia_endpoint_cycle_first_pairs")?;
    for pair in &mut first_pairs {
        pair.sort_unstable();
    }
    first_pairs.sort_unstable();
    first_pairs.dedup();
    for [first_point, current_point] in first_pairs {
        let mut order = Vec::new();
        crate::resource::reserve_vec(ctx, &mut order, missing.len(), "catia_endpoint_cycle_order_scratch")?;
        order.push(first_edge);
        if !search.walk(first_point, current_point, 1, &mut order)? {
            return Ok(None);
        }
    }
    if search.orders.is_empty() {
        return Ok(None);
    }
    let mut orders = Vec::new();
    crate::resource::reserve_vec(ctx, &mut orders, search.orders.len(), "catia_endpoint_cycle_result_rows")?;
    orders.extend(search.orders);
    orders.sort_unstable();
    Ok(Some(orders))
}

fn standard_mesh_missing_edge_assignment_domains(
    ctx: &DecodeContext<'_>,
    context: &StandardMeshBoundaryContext,
    edge_candidates: Option<&[Vec<[usize; 2]>]>,
    canonicalize_spans: bool,
    defer_validation: bool,
) -> Result<Option<(Vec<MeshFaceAssignmentDomain>, Vec<MeshEdgeRun>)>, CodecError> {
    const MAX_ASSIGNMENTS_PER_FACE: usize = 65_536;
    // A contradictory face can visit factorially many partial orders without
    // producing one complete assignment, so the assignment cap alone is not a
    // work bound.
    const MAX_SEARCH_STATES_PER_FACE: usize = 4_096;
    // The same face search may be retried with progressively weaker endpoint
    // constraints. Charge every retry and every candidate trim width to one
    // decode-local budget.
    const MAX_SEARCH_STATES: usize = 65_536;
    type PlacementConstraints<'a> = (
        Option<&'a [[u32; 2]]>,
        &'a HashMap<MeshCorner, u32>,
        Option<(&'a [Arc<HashSet<usize>>], &'a [PointTransitions])>,
        &'a MeshCornerPoints,
    );
    type PointTransitions = HashMap<usize, Arc<HashSet<usize>>>;
    type DeadState = (usize, usize, u64, Option<u32>, Vec<usize>, bool);

    #[allow(clippy::too_many_arguments)]
    fn enumerate_face(
        ctx: &DecodeContext<'_>,
        face: usize,
        gaps: &[MeshBoundaryGap],
        cycle_lengths: &[usize],
        missing: &[usize],
        rows: &[EdgeRow],
        fixed_complete_row_spans: bool,
        constraints: PlacementConstraints<'_>,
        canonicalize_spans: bool,
        remaining_states: &mut usize,
    ) -> Result<Option<Vec<Vec<MeshEdgePlacementCandidate>>>, CodecError> {
        struct Search<'a, 'ctx> {
            ctx: &'a DecodeContext<'ctx>,
            face: usize,
            gaps: &'a [MeshBoundaryGap],
            cycle_lengths: &'a [usize],
            missing: &'a [usize],
            rows: &'a [EdgeRow],
            fixed_complete_row_spans: bool,
            edge_ports: Option<&'a [[u32; 2]]>,
            corner_ports: &'a HashMap<MeshCorner, u32>,
            edge_points: Option<&'a [Arc<HashSet<usize>>]>,
            point_transitions: Option<&'a [PointTransitions]>,
            corner_points: &'a MeshCornerPoints,
            canonical_spans: bool,
            canonical_gap_partitions: bool,
            dead_states: HashSet<DeadState>,
            remaining_states: &'a mut usize,
            states: usize,
            assignments: usize,
            complete: Vec<Vec<MeshEdgePlacementCandidate>>,
        }
        impl Search<'_, '_> {
            #[allow(clippy::too_many_arguments)]
            fn walk(
                &mut self,
                gap: usize,
                offset: usize,
                used: u64,
                current_port: Option<u32>,
                current_points: Option<Arc<HashSet<usize>>>,
                gap_placed_start: usize,
                placed: &mut Vec<MeshEdgePlacementCandidate>,
            ) -> Result<Option<()>, CodecError> {
                let mut points = Vec::new();
                if let Some(current) = current_points.as_ref() {
                    crate::resource::reserve_vec(self.ctx, &mut points, current.len(), "catia_gap_state_points")?;
                    points.extend(current.iter().copied());
                }
                points.sort_unstable();
                let has_flexible = placed.len() > gap_placed_start;
                let state = (gap, offset, used, current_port, points, has_flexible);
                if self.dead_states.contains(&state) {
                    return Ok(Some(()));
                }
                let before = self.assignments;
                let Some(()) = self.walk_state(
                    gap,
                    offset,
                    used,
                    current_port,
                    current_points,
                    gap_placed_start,
                    placed,
                )? else {
                    return Ok(None);
                };
                if self.assignments == before {
                    let bytes = state.4.len().checked_mul(std::mem::size_of::<usize>())
                        .and_then(|bytes| u64::try_from(bytes).ok())
                        .ok_or_else(|| self.ctx.refuse_codec_limit("catia_gap_dead_state_points", u64::MAX, u64::MAX))?;
                    self.ctx.charge_retained(bytes, "catia_gap_dead_state_points")?;
                    crate::resource::insert_set(self.ctx, &mut self.dead_states, state, "catia_gap_dead_states")?;
                }
                Ok(Some(()))
            }

            #[allow(clippy::too_many_arguments)]
            fn walk_state(
                &mut self,
                gap: usize,
                offset: usize,
                used: u64,
                current_port: Option<u32>,
                current_points: Option<Arc<HashSet<usize>>>,
                gap_placed_start: usize,
                placed: &mut Vec<MeshEdgePlacementCandidate>,
            ) -> Result<Option<()>, CodecError> {
                let _depth = self.ctx.enter_nested("catia_gap_assignment_depth")?;
                self.ctx.charge_work(1, "catia_gap_assignment_work")?;
                let Some(remaining) = self.remaining_states.checked_sub(1) else {
                    return Ok(None);
                };
                *self.remaining_states = remaining;
                let Some(states) = self.states.checked_add(1) else {
                    return Ok(None);
                };
                self.states = states;
                if self.states > MAX_SEARCH_STATES_PER_FACE {
                    return Ok(None);
                }
                if self.assignments > MAX_ASSIGNMENTS_PER_FACE {
                    return Ok(None);
                }
                if gap == self.gaps.len() {
                    if used.count_ones() as usize == self.missing.len() {
                        self.assignments += 1;
                        let copy = crate::resource::copy_retained_slice(self.ctx, placed, "catia_gap_complete_placement_copy")?;
                        crate::resource::push(self.ctx, &mut self.complete, copy, "catia_gap_complete_assignments")?;
                    }
                    return Ok(Some(()));
                }
                let target = self.gaps[gap].length;
                let can_expand_gap = self.canonical_gap_partitions
                    && offset < target
                    && placed.len() > gap_placed_start;
                if offset == target || can_expand_gap {
                    let value = &self.gaps[gap];
                    let end = (value.start + value.length) % self.cycle_lengths[value.cycle];
                    let port_closes = current_port
                        .zip(
                            self.corner_ports
                                .get(&(self.face, value.cycle, end))
                                .copied(),
                        )
                        .is_none_or(|(actual, expected)| actual == expected);
                    let end_points = self.corner_points.get(&(self.face, value.cycle, end));
                    let points_close = current_points
                        .as_ref()
                        .zip(end_points)
                        .is_none_or(|(actual, expected)| !actual.is_disjoint(expected));
                    if port_closes && points_close {
                        let next_port = self.gaps.get(gap + 1).and_then(|next| {
                            self.corner_ports
                                .get(&(self.face, next.cycle, next.start))
                                .copied()
                        });
                        let next_points = if let Some(source) = self.gaps.get(gap + 1).and_then(|next| {
                            self.corner_points.get(&(self.face, next.cycle, next.start))
                        }) {
                            Some(Arc::new(crate::resource::copy_retained_set(self.ctx, source, "catia_gap_next_corner_points")?))
                        } else {
                            None
                        };
                        let saved = crate::resource::copy_slice(self.ctx, &placed[gap_placed_start..], "catia_gap_saved_placements")?;
                        if offset < target {
                            let slack = target - offset;
                            let Some(flexible) = placed.get_mut(gap_placed_start) else {
                                return Ok(Some(()));
                            };
                            let Some(count) = flexible.segment_count.checked_add(slack) else {
                                return Ok(None);
                            };
                            flexible.segment_count = count;
                            let mut at = value.start;
                            for placement in &mut placed[gap_placed_start..] {
                                placement.start = at % self.cycle_lengths[value.cycle];
                                let Some(next) = at.checked_add(placement.segment_count) else {
                                    return Ok(None);
                                };
                                at = next;
                            }
                        }
                        let Some(()) = self.walk(
                            gap + 1,
                            0,
                            used,
                            next_port,
                            next_points,
                            placed.len(),
                            placed,
                        )? else {
                            return Ok(None);
                        };
                        placed.truncate(gap_placed_start);
                        placed.extend(saved);
                        if offset == target {
                            return Ok(Some(()));
                        }
                    } else if offset == target {
                        return Ok(Some(()));
                    }
                }
                for rank in 0..self.missing.len() {
                    if used & (1 << rank) != 0 {
                        continue;
                    }
                    let edge = self.missing[rank];
                    let remaining = target - offset;
                    let row_span = (self.fixed_complete_row_spans
                        && self.rows[edge].boundary_layout
                            == EdgeBoundaryLayout::CompleteBoundaryRun)
                        .then(|| self.rows[edge].handles.len().checked_sub(1))
                        .flatten();
                    let canonical_span = row_span.or_else(|| {
                        self.canonical_spans
                            .then(|| {
                                if self.canonical_gap_partitions {
                                    return Some(1);
                                }
                                self.missing
                                    .iter()
                                    .enumerate()
                                    .filter(|(other, _)| *other != rank && used & (1 << other) == 0)
                                    .try_fold(remaining, |available, _| available.checked_sub(1))
                            })
                            .flatten()
                    });
                    let first_span = canonical_span.unwrap_or(1);
                    let last_span = canonical_span.unwrap_or(remaining);
                    for segment_count in first_span..=last_span {
                        if segment_count == 0 || segment_count > remaining {
                            continue;
                        }
                        let mut next_ports = match (self.edge_ports, current_port) {
                            (Some(edge_ports), Some(current)) if edge_ports[edge][0] == current => {
                                self.ctx.alloc_filled(1, Some(edge_ports[edge][1]), "catia_gap_next_ports")?
                            }
                            (Some(edge_ports), Some(current)) if edge_ports[edge][1] == current => {
                                self.ctx.alloc_filled(1, Some(edge_ports[edge][0]), "catia_gap_next_ports")?
                            }
                            (Some(_), Some(_)) => continue,
                            (Some(edge_ports), None) => {
                                let mut ports = self.ctx.alloc_filled(1, Some(edge_ports[edge][0]), "catia_gap_next_ports")?;
                                crate::resource::push(self.ctx, &mut ports, Some(edge_ports[edge][1]), "catia_gap_next_ports")?;
                                ports
                            }
                            (None, _) => self.ctx.alloc_filled(1, None, "catia_gap_next_ports")?,
                        };
                        next_ports.sort_unstable();
                        next_ports.dedup();
                        let next_points = if let Some(edge_points) = self.edge_points.filter(|points| !points[edge].is_empty()) {
                            match &current_points {
                                None => Some(Arc::clone(&edge_points[edge])),
                                Some(current) => {
                                    let mut next = HashSet::new();
                                    if let Some(transitions) = self.point_transitions {
                                        for point in current.iter() {
                                            if let Some(points) = transitions[edge].get(point) {
                                                for &point in points.iter() {
                                                    if !next.contains(&point) {
                                                        let bytes = u64::try_from(std::mem::size_of::<usize>())
                                                            .map_err(|_| self.ctx.refuse_codec_limit("catia_gap_transition_points", u64::MAX, u64::MAX))?;
                                                        self.ctx.charge_retained(bytes, "catia_gap_transition_points")?;
                                                        crate::resource::insert_set(self.ctx, &mut next, point, "catia_gap_transition_points")?;
                                                    }
                                                }
                                            }
                                        }
                                    }
                                    let bytes = u64::try_from(std::mem::size_of::<HashSet<usize>>())
                                        .map_err(|_| self.ctx.refuse_codec_limit("catia_gap_transition_set", u64::MAX, u64::MAX))?;
                                    self.ctx.charge_retained(bytes, "catia_gap_transition_set")?;
                                    Some(Arc::new(next))
                                }
                            }
                        } else {
                            None
                        };
                        if next_points.as_ref().is_some_and(|points| points.is_empty()) {
                            continue;
                        }
                        let value = MeshEdgePlacementCandidate {
                            edge,
                            face: self.face,
                            cycle: self.gaps[gap].cycle,
                            start: (self.gaps[gap].start + offset)
                                % self.cycle_lengths[self.gaps[gap].cycle],
                            segment_count,
                        };
                        crate::resource::push(self.ctx, placed, value, "catia_gap_placed_edges")?;
                        for next_port in next_ports {
                            let Some(()) = self.walk(
                                gap,
                                offset + segment_count,
                                used | (1 << rank),
                                next_port,
                                next_points.clone(),
                                gap_placed_start,
                                placed,
                            )? else {
                                return Ok(None);
                            };
                        }
                        placed.pop();
                    }
                }
                drop(current_points);
                Ok(Some(()))
            }
        }

        let (edge_ports, corner_ports, endpoint_constraints, corner_points) = constraints;
        if missing.len() > u64::BITS as usize {
            return Ok(None);
        }
        let (edge_points, point_transitions) = endpoint_constraints.unzip();
        let mut search = Search {
            ctx,
            face,
            gaps,
            cycle_lengths,
            missing,
            rows,
            fixed_complete_row_spans,
            edge_ports,
            corner_ports,
            edge_points,
            point_transitions,
            corner_points,
            // Mesh span allocation does not change the ordered edge uses or
            // their endpoint quotient. Keep one allocation for each edge
            // order and gap partition in topology searches; the public
            // placement API above still enumerates every span allocation.
            canonical_spans: canonicalize_spans,
            canonical_gap_partitions: canonicalize_spans,
            dead_states: HashSet::new(),
            remaining_states,
            states: 0,
            assignments: 0,
            complete: Vec::new(),
        };
        let first_port = gaps
            .first()
            .and_then(|gap| corner_ports.get(&(face, gap.cycle, gap.start)).copied());
        let first_points = if let Some(source) = gaps.first().and_then(|gap| corner_points.get(&(face, gap.cycle, gap.start))) {
            Some(Arc::new(crate::resource::copy_retained_set(ctx, source, "catia_gap_initial_corner_points")?))
        } else {
            None
        };
        if search.walk(0, 0, 0, first_port, first_points, 0, &mut Vec::new())?.is_none() {
            return Ok(None);
        }
        if search.assignments == 0 || search.assignments > MAX_ASSIGNMENTS_PER_FACE {
            return Ok(None);
        }
        Ok(Some(search.complete))
    }

    fn endpoint_trail_assignments(
        ctx: &DecodeContext<'_>,
        face: usize,
        gaps: &[MeshBoundaryGap],
        cycle_lengths: &[usize],
        missing: &[usize],
        rows: &[EdgeRow],
        edge_points: &[Option<[usize; 2]>],
        corner_points: &MeshCornerPoints,
    ) -> Result<Option<Vec<Vec<MeshEdgePlacementCandidate>>>, CodecError> {
        (|| -> Option<Result<Vec<Vec<MeshEdgePlacementCandidate>>, CodecError>> {
        struct EndpointTrail {
            edges: Vec<usize>,
            start: usize,
            end: usize,
        }

        if gaps.is_empty()
            || missing
                .iter()
                .any(|&edge| edge_points.get(edge).is_none_or(Option::is_none))
        {
            return None;
        }
        let mut at_point = HashMap::<usize, Vec<usize>>::new();
        for &edge in missing {
            for point in edge_points[edge]? {
                if let Err(error) = crate::resource::admit_map_entry(ctx, &mut at_point, &point, "catia_trail_point_entries") {
                    return Some(Err(error));
                }
                if let Err(error) = crate::resource::push(ctx, at_point.entry(point).or_default(), edge, "catia_trail_point_edges") {
                    return Some(Err(error));
                }
            }
        }
        if at_point.values().any(|edges| edges.len() > 2) {
            return None;
        }
        let mut unseen = HashSet::new();
        for &edge in missing {
            if let Err(error) = crate::resource::insert_set(ctx, &mut unseen, edge, "catia_trail_unseen_edges") {
                return Some(Err(error));
            }
        }
        let mut trails = Vec::<EndpointTrail>::new();
        while !unseen.is_empty() {
            let first = unseen
                .iter()
                .copied()
                .filter(|edge| {
                    edge_points[*edge]
                        .is_some_and(|pair| pair.iter().any(|point| at_point[point].len() == 1))
                })
                .min()
                .or_else(|| unseen.iter().copied().min())?;
            let endpoints = edge_points[first]?;
            let start = endpoints
                .iter()
                .copied()
                .find(|point| at_point[point].len() == 1)
                .unwrap_or(endpoints[0]);
            let mut point = start;
            let mut edge = first;
            let mut trail = Vec::new();
            loop {
                if !unseen.remove(&edge) {
                    break;
                }
                if let Err(error) = crate::resource::push(ctx, &mut trail, edge, "catia_trail_edges") {
                    return Some(Err(error));
                }
                let endpoints = edge_points[edge]?;
                point = if endpoints[0] == point {
                    endpoints[1]
                } else if endpoints[1] == point {
                    endpoints[0]
                } else {
                    return None;
                };
                let Some(next) = at_point[&point]
                    .iter()
                    .copied()
                    .find(|candidate| unseen.contains(candidate))
                else {
                    break;
                };
                edge = next;
            }
            if point == start && (gaps.len() != 1 || trail.len() != missing.len()) {
                return None;
            }
            if let Err(error) = crate::resource::push(ctx, &mut trails, EndpointTrail {
                edges: trail,
                start,
                end: point,
            }, "catia_trail_rows") {
                return Some(Err(error));
            }
        }
        if gaps.len() > 1 {
            if trails.len() != gaps.len() {
                return None;
            }
            let mut available = trails;
            available.sort_by_key(|trail| trail.edges.iter().copied().min());
            let mut placements = Vec::new();
            if let Err(error) = crate::resource::reserve_vec(ctx, &mut placements, missing.len(), "catia_trail_placements") {
                return Some(Err(error));
            }
            for gap in gaps {
                let mut candidates = Vec::new();
                for (index, trail) in available.iter().enumerate() {
                    for reversed in [false, true] {
                        let trail_start = if reversed { trail.end } else { trail.start };
                        let trail_end = if reversed { trail.start } else { trail.end };
                        let gap_end = (gap.start + gap.length) % cycle_lengths[gap.cycle];
                        let Some(start_points) = corner_points.get(&(face, gap.cycle, gap.start)) else {
                            continue;
                        };
                        let Some(end_points) = corner_points.get(&(face, gap.cycle, gap_end)) else {
                            continue;
                        };
                        if !start_points.contains(&trail_start) || !end_points.contains(&trail_end) {
                            continue;
                        }
                        let span = trail.edges.len();
                        if span <= gap.length {
                            if let Err(error) = crate::resource::push(ctx, &mut candidates, (index, span, reversed), "catia_trail_gap_candidates") {
                                return Some(Err(error));
                            }
                        }
                    }
                }
                let [(trail_index, minimum_span, reversed)] = candidates.as_slice() else {
                    return None;
                };
                let (trail_index, minimum_span, reversed) =
                    (*trail_index, *minimum_span, *reversed);
                let trail = available.remove(trail_index);
                let mut edges = trail.edges;
                if reversed {
                    edges.reverse();
                }
                let slack = gap.length - minimum_span;
                let mut offset = 0usize;
                for (index, edge) in edges.into_iter().enumerate() {
                    let mut segment_count = 1usize;
                    if index == 0 {
                        segment_count = segment_count.checked_add(slack)?;
                    }
                    placements.push(MeshEdgePlacementCandidate {
                        edge,
                        face,
                        cycle: gap.cycle,
                        start: (gap.start + offset) % cycle_lengths[gap.cycle],
                        segment_count,
                    });
                    offset = offset.checked_add(segment_count)?;
                }
            }
            let mut rows = Vec::new();
            if let Err(error) = crate::resource::push(ctx, &mut rows, placements, "catia_trail_placement_rows") {
                return Some(Err(error));
            }
            return Some(Ok(rows));
        }
        let [gap] = gaps else {
            return None;
        };
        if cycle_lengths.len() != 1
            || gap.start != 0
            || gap.cycle != 0
            || gap.length != cycle_lengths[0]
            || gap.length != missing.len()
            || missing.iter().any(|&edge| rows[edge].handles.len() != 2)
        {
            return None;
        }
        if trails.len() > u64::BITS as usize {
            return None;
        }
        let mut trail_edges = Vec::new();
        if let Err(error) = crate::resource::reserve_vec(ctx, &mut trail_edges, trails.len(), "catia_trail_order_input_rows") {
            return Some(Err(error));
        }
        for trail in &trails {
            let copy = match crate::resource::copy_retained_slice(ctx, &trail.edges, "catia_trail_order_input_edges") {
                Ok(copy) => copy,
                Err(error) => return Some(Err(error)),
            };
            trail_edges.push(copy);
        }
        let orders = match bounded_oriented_trail_orders(ctx, &trail_edges, MAX_ASSIGNMENTS_PER_FACE) {
            Ok(Some(orders)) => orders,
            Ok(None) => return None,
            Err(error) => return Some(Err(error)),
        };
        let mut assignments = Vec::new();
        if let Err(error) = crate::resource::reserve_vec(ctx, &mut assignments, orders.len(), "catia_trail_assignment_rows") {
            return Some(Err(error));
        }
        for order in orders {
            let mut placements = Vec::new();
            if let Err(error) = crate::resource::reserve_vec(ctx, &mut placements, order.len(), "catia_trail_assignment_placements") {
                return Some(Err(error));
            }
            placements.extend(order.into_iter().enumerate().map(|(offset, edge)| MeshEdgePlacementCandidate {
                edge,
                face,
                cycle: gap.cycle,
                start: offset,
                segment_count: 1,
            }));
            assignments.push(placements);
        }
        Some(Ok(assignments))
        })().transpose()
    }

    fn endpoint_cycle_assignments(
        ctx: &DecodeContext<'_>,
        face: usize,
        gaps: &[MeshBoundaryGap],
        cycle_lengths: &[usize],
        missing: &[usize],
        rows: &[EdgeRow],
        edge_candidates: &[Vec<[usize; 2]>],
    ) -> Result<Option<Vec<Vec<MeshEdgePlacementCandidate>>>, CodecError> {
        (|| -> Option<Result<Vec<Vec<MeshEdgePlacementCandidate>>, CodecError>> {
        let [gap] = gaps else {
            return None;
        };
        if cycle_lengths.len() != 1
            || gap.start != 0
            || gap.cycle != 0
            || gap.length != cycle_lengths[0]
            || gap.length != missing.len()
            || missing.iter().any(|&edge| rows[edge].handles.len() != 2)
        {
            return None;
        }
        let orders = match bounded_endpoint_cycle_orders(ctx, missing, edge_candidates, MAX_ASSIGNMENTS_PER_FACE) {
            Ok(Some(orders)) => orders,
            Ok(None) => return None,
            Err(error) => return Some(Err(error)),
        };
        let mut assignments = Vec::new();
        if let Err(error) = crate::resource::reserve_vec(ctx, &mut assignments, orders.len(), "catia_cycle_assignment_rows") {
            return Some(Err(error));
        }
        for order in orders {
            let mut placements = Vec::new();
            if let Err(error) = crate::resource::reserve_vec(ctx, &mut placements, order.len(), "catia_cycle_assignment_placements") {
                return Some(Err(error));
            }
            placements.extend(order.into_iter().enumerate().map(|(offset, edge)| MeshEdgePlacementCandidate {
                edge,
                face,
                cycle: 0,
                start: offset,
                segment_count: 1,
            }));
            assignments.push(placements);
        }
        Some(Ok(assignments))
        })().transpose()
    }

    let edge_rows = &context.analysis.edge_rows;
    if edge_candidates.is_some_and(|candidates| candidates.len() != edge_rows.len()) {
        return Ok(None);
    }
    let admit_set_retention = |set: &HashSet<usize>, operation| -> Result<(), CodecError> {
        let bytes = set.len().checked_mul(std::mem::size_of::<usize>())
            .and_then(|bytes| bytes.checked_add(std::mem::size_of::<HashSet<usize>>()))
            .and_then(|bytes| u64::try_from(bytes).ok())
            .ok_or_else(|| ctx.refuse_codec_limit(operation, u64::MAX, u64::MAX))?;
        ctx.charge_retained(bytes, operation)
    };
    let mut edge_point_domains = None;
    let mut edge_point_transitions = None;
    if let Some(candidates) = edge_candidates {
        let mut domains = Vec::new();
        let mut transition_rows = Vec::new();
        crate::resource::reserve_vec(ctx, &mut domains, candidates.len(), "catia_mesh_edge_point_domain_rows")?;
        crate::resource::reserve_vec(ctx, &mut transition_rows, candidates.len(), "catia_mesh_edge_transition_rows")?;
        for pairs in candidates {
            let mut points = HashSet::new();
            let mut transitions = HashMap::<usize, HashSet<usize>>::new();
            for &[left, right] in pairs {
                for point in [left, right] {
                    crate::resource::insert_set(ctx, &mut points, point, "catia_mesh_edge_point_domain_values")?;
                }
                for (from, to) in [(left, right), (right, left)] {
                    crate::resource::admit_map_entry(ctx, &mut transitions, &from, "catia_mesh_edge_transition_points")?;
                    crate::resource::insert_set(ctx, transitions.entry(from).or_default(), to, "catia_mesh_edge_transition_targets")?;
                }
            }
            admit_set_retention(&points, "catia_mesh_edge_point_domain_retained")?;
            domains.push(Arc::new(points));
            let mut retained_transitions = HashMap::new();
            for (point, targets) in transitions {
                admit_set_retention(&targets, "catia_mesh_edge_transition_retained")?;
                crate::resource::insert_map(ctx, &mut retained_transitions, point, Arc::new(targets), "catia_mesh_edge_transition_map")?;
            }
            transition_rows.push(retained_transitions);
        }
        edge_point_domains = Some(domains);
        edge_point_transitions = Some(transition_rows);
    }
    let endpoint_constraints = edge_point_domains
        .as_deref()
        .zip(edge_point_transitions.as_deref());
    let coverage = &context.coverage;
    let edge_ports = &context.edge_ports;
    let complete_boundary_ports = edge_rows
        .iter()
        .all(|row| row.boundary_layout == EdgeBoundaryLayout::CompleteBoundaryRun);
    let placement_ports = complete_boundary_ports.then_some(edge_ports.as_slice());
    let singleton_edge_points = if let Some(candidates) = edge_candidates {
        let mut singleton = Vec::new();
        crate::resource::reserve_vec(ctx, &mut singleton, candidates.len(), "catia_mesh_singleton_edge_points")?;
        singleton.extend(candidates.iter().map(|domain| {
            <[[usize; 2]; 1]>::try_from(domain.as_slice()).ok().map(|[pair]| pair)
        }));
        Some(singleton)
    } else {
        None
    };
    let edge_runs = &context.edge_runs;
    let mut remaining_states = MAX_SEARCH_STATES;
    let mut corner_ports = HashMap::<MeshCorner, u32>::new();
    let mut corner_points = MeshCornerPoints::new();
    for run in edge_runs {
        let length = context.cycle_lengths[run.face][run.cycle];
        let end = (run.start + run.segment_count) % length;
        if edge_rows[run.edge].boundary_layout == EdgeBoundaryLayout::CompleteBoundaryRun {
            let ports = edge_ports[run.edge];
            let oriented = if run.reversed {
                [ports[1], ports[0]]
            } else {
                ports
            };
            for (corner, port) in [(run.start, oriented[0]), (end, oriented[1])] {
                match crate::resource::insert_map(ctx, &mut corner_ports, (run.face, run.cycle, corner), port, "catia_mesh_corner_ports")? {
                    Some(stored) if stored != port => return Ok(None),
                    Some(_) | None => {}
                }
            }
        }
        if let Some(candidates) = edge_candidates {
            let mut points = HashSet::new();
            for &point in candidates[run.edge].iter().flatten() {
                crate::resource::insert_set(ctx, &mut points, point, "catia_mesh_corner_candidate_points")?;
            }
            if !points.is_empty() {
                for corner in [run.start, end] {
                    let key = (run.face, run.cycle, corner);
                    if let Some(stored) = corner_points.get_mut(&key) {
                        stored.retain(|point| points.contains(point));
                    } else {
                        let copied = crate::resource::copy_retained_set(ctx, &points, "catia_mesh_corner_point_copy")?;
                        crate::resource::insert_map(ctx, &mut corner_points, key, copied, "catia_mesh_corner_point_entries")?;
                    }
                }
            }
        }
    }
    let mut assignment_results = Vec::new();
    crate::resource::reserve_vec(ctx, &mut assignment_results, coverage.len(), "catia_mesh_assignment_domain_faces")?;
    for face in coverage {
        let cycle_lengths = &context.cycle_lengths[face.face];
        let unordered_full_cycle = edge_candidates.and_then(|candidates| {
            let [gap] = face.gaps.as_slice() else {
                return None;
            };
            (cycle_lengths.len() == 1
                && gap.cycle == 0
                && gap.start == 0
                && gap.length == cycle_lengths[0]
                && gap.length == face.missing_edges.len()
                && face
                    .missing_edges
                    .iter()
                    .all(|&edge| edge_rows[edge].handles.len() == 2)
                && defer_validation
                && face
                    .missing_edges
                    .iter()
                    .any(|&edge| candidates[edge].len() > 1))
            .then(|| face.missing_edges.as_slice())
        });
        if let Some(edges) = unordered_full_cycle {
            assignment_results.push(MeshFaceAssignmentDomain::UnorderedFullCycle(
                crate::resource::copy_retained_slice(ctx, edges, "catia_mesh_unordered_missing_edges")?,
            ));
            continue;
        }
        let cycle_assignments = if let Some(candidates) = edge_candidates {
            endpoint_cycle_assignments(
                ctx,
                face.face,
                &face.gaps,
                cycle_lengths,
                &face.missing_edges,
                edge_rows,
                candidates,
            )?
        } else {
            None
        };
        let trail_assignments = if cycle_assignments.is_none() {
            if let Some(edge_points) = singleton_edge_points.as_ref() {
                endpoint_trail_assignments(ctx, face.face, &face.gaps, cycle_lengths, &face.missing_edges, edge_rows, edge_points, &corner_points)?
            } else {
                None
            }
        } else {
            None
        };
        let mut assignments = cycle_assignments.or(trail_assignments);
        if assignments.is_none() {
            assignments = enumerate_face(ctx, face.face, &face.gaps, cycle_lengths, &face.missing_edges,
                edge_rows, context.analysis.fixed_complete_row_spans,
                (placement_ports, &corner_ports, endpoint_constraints, &corner_points),
                canonicalize_spans, &mut remaining_states)?;
        }
        if assignments.is_none() {
            assignments = enumerate_face(ctx, face.face, &face.gaps, cycle_lengths, &face.missing_edges,
                edge_rows, context.analysis.fixed_complete_row_spans,
                (None, &HashMap::new(), endpoint_constraints, &corner_points),
                canonicalize_spans, &mut remaining_states)?;
        }
        if assignments.is_none() {
            assignments = enumerate_face(ctx, face.face, &face.gaps, cycle_lengths, &face.missing_edges,
                edge_rows, context.analysis.fixed_complete_row_spans,
                (None, &HashMap::new(), None, &MeshCornerPoints::new()),
                canonicalize_spans, &mut remaining_states)?;
        }
        let domain = if let Some(assignments) = assignments {
            Some(MeshFaceAssignmentDomain::Ordered(assignments))
        } else if defer_validation {
            Some(MeshFaceAssignmentDomain::DeferredValidation(MeshFaceCoverage {
                face: face.face,
                gaps: crate::resource::copy_retained_slice(ctx, &face.gaps, "catia_mesh_deferred_face_gaps")?,
                missing_edges: crate::resource::copy_retained_slice(ctx, &face.missing_edges, "catia_mesh_deferred_face_missing_edges")?,
            }))
        } else {
            None
        };
        let Some(domain) = domain else {
            return Ok(None);
        };
        assignment_results.push(domain);
    }
    Ok(Some((
        assignment_results,
        crate::resource::copy_retained_slice(ctx, edge_runs, "catia_mesh_assignment_edge_runs")?,
    )))
}

fn standard_mesh_missing_edge_assignments(
    ctx: &DecodeContext<'_>,
    bytes: &[u8],
    edge_faces: &[[usize; 2]],
    edge_candidates: Option<&[Vec<[usize; 2]>]>,
    canonicalize_spans: bool,
) -> Result<Option<Vec<Vec<Vec<MeshEdgePlacementCandidate>>>>, CodecError> {
    let Some(context) = StandardMeshBoundaryContext::parse(ctx, bytes, edge_faces)? else {
        return Ok(None);
    };
    let Some((domains, _)) = standard_mesh_missing_edge_assignment_domains(
        ctx,
        &context,
        edge_candidates,
        canonicalize_spans,
        false,
    )? else {
        return Ok(None);
    };
    let mut assignments = Vec::new();
    crate::resource::reserve_vec(ctx, &mut assignments, domains.len(), "catia_missing_assignment_faces")?;
    for domain in domains {
        let MeshFaceAssignmentDomain::Ordered(face) = domain else {
            return Ok(None);
        };
        assignments.push(face);
    }
    Ok(Some(assignments))
}

/// Project complete unmatched-edge assignments to the placement domain for
/// each face. FBB complete rows cover exactly one segment per adjacent handle
/// pair. Standard interior rows remain open-span until their sample chain is
/// matched to the boundary.
#[cfg(test)]
fn standard_mesh_missing_edge_placements(
    ctx: &DecodeContext<'_>,
    bytes: &[u8],
    edge_faces: &[[usize; 2]],
) -> Result<Option<Vec<Vec<MeshEdgePlacementCandidate>>>, CodecError> {
    Ok(
        standard_mesh_missing_edge_assignments(ctx, bytes, edge_faces, None, false)?.map(|faces| {
            faces
                .into_iter()
                .map(|assignments| {
                    let mut placements = assignments
                        .into_iter()
                        .flatten()
                        .collect::<HashSet<_>>()
                        .into_iter()
                        .collect::<Vec<_>>();
                    placements.sort_unstable();
                    placements
                })
                .collect()
        }),
    )
}

pub(crate) fn standard_mesh_boundary_assignments(
    ctx: &DecodeContext<'_>,
    bytes: &[u8],
    edge_faces: &[[usize; 2]],
    edge_candidates: Option<&[Vec<[usize; 2]>]>,
) -> Result<Option<Vec<Vec<MeshFaceBoundaryAssignment>>>, CodecError> {
    let Some(context) = StandardMeshBoundaryContext::parse(ctx, bytes, edge_faces)? else {
        return Ok(None);
    };
    standard_mesh_boundary_assignments_from_context(ctx, &context, edge_candidates)
}

fn standard_mesh_boundary_assignments_from_context(
    ctx: &DecodeContext<'_>,
    context: &StandardMeshBoundaryContext,
    edge_candidates: Option<&[Vec<[usize; 2]>]>,
) -> Result<Option<Vec<Vec<MeshFaceBoundaryAssignment>>>, CodecError> {
    let Some(domains) =
        standard_mesh_boundary_domains_from_context(ctx, context, edge_candidates, false)?
    else {
        return Ok(None);
    };
    let mut assignments = Vec::new();
    crate::resource::reserve_vec(ctx, &mut assignments, domains.len(), "catia_boundary_assignment_faces")?;
    for domain in domains {
        let MeshFaceBoundaryDomain::Ordered(face) = domain else {
            return Ok(None);
        };
        assignments.push(face);
    }
    Ok(Some(assignments))
}

pub(super) fn standard_mesh_boundary_domains_from_context(
    ctx: &DecodeContext<'_>,
    context: &StandardMeshBoundaryContext,
    edge_candidates: Option<&[Vec<[usize; 2]>]>,
    defer_validation: bool,
) -> Result<Option<Vec<MeshFaceBoundaryDomain>>, CodecError> {
    let Some((domains, runs)) = standard_mesh_missing_edge_assignment_domains(
        ctx,
        context,
        edge_candidates,
        true,
        defer_validation,
    )? else {
        return Ok(None);
    };
    let cycle_lengths = &context.cycle_lengths;
    let mut resolved = Vec::new();
    crate::resource::reserve_vec(ctx, &mut resolved, domains.len(), "catia_mesh_boundary_domain_faces")?;
    for (face, domain) in domains.into_iter().enumerate() {
        let Some(domain) = (|| -> Result<Option<MeshFaceBoundaryDomain>, CodecError> {
            match domain {
                MeshFaceAssignmentDomain::UnorderedFullCycle(edges) => {
                    Ok(Some(MeshFaceBoundaryDomain::UnorderedFullCycle(edges)))
                }
                MeshFaceAssignmentDomain::DeferredValidation(coverage) => {
                    let mut cycles = Vec::new();
                    crate::resource::reserve_vec(ctx, &mut cycles, cycle_lengths[face].len(), "catia_deferred_boundary_cycles")?;
                    cycles.extend(cycle_lengths[face].iter().copied().map(|length| MeshDeferredBoundaryCycle {
                        length,
                        exact_uses: Vec::new(),
                    }));
                    for run in runs.iter().filter(|run| run.face == face) {
                        let length = cycles[run.cycle].length;
                        let fixed_direction = edge_candidates.is_none()
                            || context.analysis.edge_rows[run.edge].boundary_layout
                                == EdgeBoundaryLayout::CompleteBoundaryRun;
                        crate::resource::push(ctx, &mut cycles[run.cycle].exact_uses, (
                            MeshBoundaryEdgeCandidate {
                                edge: run.edge,
                                start: run.start,
                                end: run.end(length),
                                reversed: fixed_direction.then_some(run.reversed),
                            },
                            run.segment_count,
                        ), "catia_deferred_boundary_exact_uses")?;
                    }
                    for cycle in &mut cycles {
                        cycle
                            .exact_uses
                            .sort_unstable_by_key(|(use_, _)| use_.start);
                    }
                    Ok(Some(MeshFaceBoundaryDomain::DeferredValidation(
                        MeshDeferredFaceBoundary {
                            cycles,
                            missing_edges: coverage.missing_edges,
                        },
                    )))
                }
                MeshFaceAssignmentDomain::Ordered(assignments) => {
                    let mut ordered = Vec::new();
                    for assignment in assignments {
                        let mut boundaries = ctx.alloc_filled(
                            cycle_lengths[face].len(),
                            Vec::new(),
                            "catia_mesh_ordered_boundaries",
                        )?;
                        for run in runs.iter().filter(|run| run.face == face) {
                            let fixed_direction = edge_candidates.is_none()
                                || context.analysis.edge_rows[run.edge].boundary_layout
                                    == EdgeBoundaryLayout::CompleteBoundaryRun;
                            crate::resource::push(
                                ctx,
                                &mut boundaries[run.cycle],
                                (
                                    MeshBoundaryEdgeCandidate {
                                        edge: run.edge,
                                        start: run.start,
                                        end: run.end(cycle_lengths[face][run.cycle]),
                                        reversed: fixed_direction.then_some(run.reversed),
                                    },
                                    run.segment_count,
                                ),
                                "catia_mesh_ordered_boundary_entries",
                            )?;
                        }
                        for placement in assignment {
                            crate::resource::push(
                                ctx,
                                &mut boundaries[placement.cycle],
                                (
                                    MeshBoundaryEdgeCandidate {
                                        edge: placement.edge,
                                        start: placement.start,
                                        end: placement.end(cycle_lengths[face][placement.cycle]),
                                        reversed: None,
                                    },
                                    placement.segment_count,
                                ),
                                "catia_mesh_ordered_boundary_entries",
                            )?;
                        }
                        let mut completed = Vec::new();
                        for (cycle, mut uses) in boundaries.into_iter().enumerate() {
                            uses.sort_unstable_by_key(|(edge, _)| edge.start);
                            let length = cycle_lengths[face][cycle];
                            let mut coverage =
                                ctx.alloc_filled(length, 0u8, "catia_mesh_boundary_coverage")?;
                            for (edge, segment_count) in &uses {
                                for offset in 0..*segment_count {
                                    let covered = &mut coverage[(edge.start + offset) % length];
                                    let Some(count) = covered.checked_add(1) else {
                                        return Ok(None);
                                    };
                                    *covered = count;
                                }
                            }
                            if coverage.iter().any(|count| *count != 1) {
                                return Ok(None);
                            }
                            let mut boundary = Vec::new();
                            for (edge, _) in uses {
                                crate::resource::push(
                                    ctx,
                                    &mut boundary,
                                    edge,
                                    "catia_mesh_completed_boundary_entries",
                                )?;
                            }
                            crate::resource::push(
                                ctx,
                                &mut completed,
                                boundary,
                                "catia_mesh_completed_boundaries",
                            )?;
                        }
                        crate::resource::push(
                            ctx,
                            &mut ordered,
                            MeshFaceBoundaryAssignment {
                                boundaries: completed,
                            },
                            "catia_mesh_ordered_assignments",
                        )?;
                    }
                    Ok(Some(MeshFaceBoundaryDomain::Ordered(ordered)))
                }
            }
        })()?
        else {
            return Ok(None);
        };
        resolved.push(domain);
    }
    Ok(Some(resolved))
}

/// Materialize one complete face-assignment selection and one direction for
/// each ordered edge use into its abstract logical-corner quotient.
#[cfg(test)]
fn parse_standard_mesh_selection(
    ctx: &DecodeContext<'_>,
    bytes: &[u8],
    edge_faces: &[[usize; 2]],
    selected_assignments: &[usize],
    edge_directions: &[Vec<Vec<bool>>],
) -> Result<Option<StandardTopology>, CodecError> {
    let Some(face_run) = selected_standard_run(ctx, bytes)? else {
        return Ok(None);
    };
    let face_count = face_run.face_count();
    let after_faces = face_run.after_faces();
    let Some((edge_rows, vertex_header)) = parse_edge_tables(ctx, bytes, after_faces)? else {
        return Ok(None);
    };
    let Some(vertex_points) = parse_vertex_table(ctx, bytes, vertex_header)? else {
        return Ok(None);
    };
    let Some(assignments) = standard_mesh_boundary_assignments(ctx, bytes, edge_faces, None)?
    else {
        return Ok(None);
    };
    if selected_assignments.len() != face_count || edge_directions.len() != face_count {
        return Ok(None);
    }
    let mut selected = Vec::new();
    for (face, &assignment) in assignments.iter().zip(selected_assignments) {
        let Some(choice) = face.get(assignment) else {
            return Ok(None);
        };
        crate::resource::push(ctx, &mut selected, choice, "catia_mesh_selection_choices")?;
    }
    reconstruct_mesh_selection(ctx, &edge_rows, &vertex_points, &selected, edge_directions)
}

#[derive(Debug)]
struct BoundaryEndpointSupport {
    by_edge: HashMap<usize, HashSet<[usize; 2]>>,
}

fn boundary_endpoint_support(
    ctx: &DecodeContext<'_>,
    boundary: &[MeshBoundaryEdgeCandidate],
    edge_candidates: &[Vec<[usize; 2]>],
    budget: &WorkBudget<'_>,
) -> Result<Option<BoundaryEndpointSupport>, CodecError> {
    #[derive(Clone, Copy)]
    struct State {
        pair: [usize; 2],
        start: usize,
        end: usize,
    }

    let mut layers = Vec::new();
    crate::resource::reserve_vec(ctx, &mut layers, boundary.len(), "catia_boundary_support_layer_rows")?;
    for use_ in boundary {
        let Some(pairs) = edge_candidates.get(use_.edge).filter(|pairs| !pairs.is_empty()) else {
            return Ok(None);
        };
        let Some(count) = pairs.len().checked_mul(2) else {
            return Err(ctx.refuse_codec_limit("catia_boundary_support_layer_states", u64::MAX, u64::MAX));
        };
        let mut states = Vec::new();
        crate::resource::reserve_vec(ctx, &mut states, count, "catia_boundary_support_layer_states")?;
        for &pair in pairs {
            let mut unordered = pair;
            unordered.sort_unstable();
            states.extend([
                State { pair: unordered, start: unordered[0], end: unordered[1] },
                State { pair: unordered, start: unordered[1], end: unordered[0] },
            ]);
        }
        layers.push(states);
    }
    let Some(first_layer) = layers.first() else {
        return Ok(None);
    };
    let Some(layer_states) = layers
        .iter()
        .try_fold(0usize, |total, layer| total.checked_add(layer.len()))
    else {
        return Ok(None);
    };
    let mut first_points = HashSet::new();
    for state in first_layer {
        crate::resource::insert_set(ctx, &mut first_points, state.start, "catia_boundary_first_points")?;
    }
    let make_marks = |row_operation, item_operation| -> Result<Vec<Vec<bool>>, CodecError> {
        let mut marks = Vec::new();
        crate::resource::reserve_vec(ctx, &mut marks, layers.len(), row_operation)?;
        for layer in &layers {
            marks.push(ctx.alloc_filled(layer.len(), false, item_operation)?);
        }
        Ok(marks)
    };
    let mut supported = make_marks("catia_boundary_support_mark_rows", "catia_boundary_layer_marks")?;
    for first_point in first_points {
        let Some(work) = layer_states.checked_mul(3) else {
            return Ok(None);
        };
        if !budget.charge_by(work) {
            return Ok(None);
        }
        let mut forward = make_marks("catia_boundary_forward_mark_rows", "catia_boundary_forward_marks")?;
        for (state, reachable) in first_layer.iter().zip(&mut forward[0]) {
            *reachable = state.start == first_point;
        }
        for layer in 1..layers.len() {
            if !budget.charge_by(layers[layer - 1].len() + layers[layer].len()) {
                return Ok(None);
            }
            let mut reachable_points = HashSet::new();
            for (state, reachable) in layers[layer - 1].iter().zip(&forward[layer - 1]) {
                if *reachable {
                    crate::resource::insert_set(ctx, &mut reachable_points, state.end, "catia_boundary_reachable_points")?;
                }
            }
            for (right, right_state) in layers[layer].iter().enumerate() {
                forward[layer][right] = reachable_points.contains(&right_state.start);
            }
        }
        let mut backward = make_marks("catia_boundary_backward_mark_rows", "catia_boundary_backward_marks")?;
        let last = layers.len() - 1;
        for (state, (reachable, value)) in layers[last]
            .iter()
            .zip(forward[last].iter().zip(&mut backward[last]))
        {
            *value = *reachable && state.end == first_point;
        }
        for layer in (0..last).rev() {
            let mut supported_points = HashSet::new();
            for (state, supported) in layers[layer + 1].iter().zip(&backward[layer + 1]) {
                if *supported {
                    crate::resource::insert_set(ctx, &mut supported_points, state.start, "catia_boundary_supported_points")?;
                }
            }
            for (left, left_state) in layers[layer].iter().enumerate() {
                backward[layer][left] = supported_points.contains(&left_state.end);
            }
        }
        if backward[0].iter().any(|supported| *supported) {
            for layer in 0..layers.len() {
                for state in 0..layers[layer].len() {
                    supported[layer][state] |= forward[layer][state] && backward[layer][state];
                }
            }
        }
    }
    let mut by_edge = HashMap::<usize, HashSet<[usize; 2]>>::new();
    for (layer, use_) in boundary.iter().enumerate() {
        let mut values = HashSet::new();
        for (state, supported) in layers[layer].iter().zip(&supported[layer]) {
            if *supported {
                crate::resource::insert_set(ctx, &mut values, state.pair, "catia_boundary_supported_pairs")?;
            }
        }
        if values.is_empty() {
            return Ok(None);
        }
        crate::resource::admit_map_entry(ctx, &mut by_edge, &use_.edge, "catia_boundary_support_edges")?;
        by_edge.entry(use_.edge)
            .and_modify(|stored| stored.retain(|pair| values.contains(pair)))
            .or_insert(values);
    }
    Ok(by_edge
        .values()
        .all(|domain| !domain.is_empty())
        .then_some(BoundaryEndpointSupport { by_edge }))
}

/// Prune endpoint-pair domains through every ordered trim-boundary candidate.
/// A pair survives only when each incident face retains a complete assignment
/// whose ordered cycles admit a closed head-to-tail traversal using that pair.
pub(crate) fn standard_mesh_prune_endpoint_candidates(
    ctx: &DecodeContext<'_>,
    bytes: &[u8],
    edge_faces: &[[usize; 2]],
    edge_candidates: &[Vec<[usize; 2]>],
) -> Result<Option<Vec<Vec<[usize; 2]>>>, CodecError> {
    if edge_faces.len() != edge_candidates.len() {
        return Ok(None);
    }
    let Some(face_run) = selected_standard_run(ctx, bytes)? else {
        return Ok(None);
    };
    let after_faces = face_run.after_faces();
    let Some((_, vertex_header)) = parse_edge_tables(ctx, bytes, after_faces)? else {
        return Ok(None);
    };
    let Some(vertex_points) = parse_vertex_table(ctx, bytes, vertex_header)? else {
        return Ok(None);
    };
    let point_count = vertex_points.len();
    let mut complete_domain = Vec::new();
    for left in 0..point_count {
        for right in (left + 1)..point_count {
            crate::resource::push(ctx, &mut complete_domain, [left, right], "catia_prune_complete_point_pairs")?;
        }
    }
    let mut candidates = Vec::new();
    crate::resource::reserve_vec(ctx, &mut candidates, edge_candidates.len(), "catia_prune_candidate_rows")?;
    for domain in edge_candidates {
        let source = if domain.is_empty() { &complete_domain } else { domain };
        candidates.push(crate::resource::copy_retained_slice(ctx, source, "catia_prune_candidate_pairs")?);
    }
    let Some(mut faces) = standard_mesh_boundary_assignments(ctx, bytes, edge_faces, None)? else {
        return Ok(None);
    };
    let budget = WorkBudget::new(MAX_MESH_CONSTRAINT_OPERATIONS);
    loop {
        let before = (
            faces.iter().map(Vec::len).sum::<usize>(),
            candidates.iter().map(Vec::len).sum::<usize>(),
        );
        let mut face_supports = Vec::new();
        crate::resource::reserve_vec(ctx, &mut face_supports, faces.len(), "catia_prune_face_support_rows")?;
        for assignments in &mut faces {
            let mut evaluated = Vec::new();
            'assignment: for (index, assignment) in assignments.iter().enumerate() {
                let mut support = HashMap::<usize, HashSet<[usize; 2]>>::new();
                for boundary in &assignment.boundaries {
                    let Some(boundary_support) =
                        boundary_endpoint_support(ctx, boundary, &candidates, &budget)?
                    else {
                        continue 'assignment;
                    };
                    for (edge, domain) in boundary_support.by_edge {
                        crate::resource::admit_map_entry(ctx, &mut support, &edge, "catia_prune_support_edges")?;
                        support
                            .entry(edge)
                            .and_modify(|stored| stored.retain(|pair| domain.contains(pair)))
                            .or_insert(domain);
                    }
                }
                if support.values().all(|domain| !domain.is_empty()) {
                    crate::resource::push(ctx, &mut evaluated, (index, support), "catia_prune_evaluated_assignments")?;
                }
            }
            if evaluated.is_empty() {
                return Ok(None);
            }
            let mut retained_assignments = Vec::new();
            crate::resource::reserve_vec(ctx, &mut retained_assignments, evaluated.len(), "catia_prune_retained_assignments")?;
            for (index, _) in &evaluated {
                retained_assignments.push(MeshFaceBoundaryAssignment {
                    boundaries: crate::resource::copy_retained_rows(ctx, &assignments[*index].boundaries, "catia_prune_retained_boundary_rows", "catia_prune_retained_boundary_uses")?,
                });
            }
            *assignments = retained_assignments;
            let mut support_rows = Vec::new();
            crate::resource::reserve_vec(ctx, &mut support_rows, evaluated.len(), "catia_prune_assignment_supports")?;
            support_rows.extend(evaluated.into_iter().map(|(_, support)| support));
            face_supports.push(support_rows);
        }
        for (edge, domain) in candidates.iter_mut().enumerate() {
            let mut allowed = None::<HashSet<[usize; 2]>>;
            let mut incident = edge_faces[edge];
            incident.sort_unstable();
            for face in incident.into_iter().take(if incident[0] == incident[1] { 1 } else { 2 }) {
                let mut support = HashSet::new();
                for &pair in face_supports[face].iter().filter_map(|assignment| assignment.get(&edge)).flatten() {
                    crate::resource::insert_set(ctx, &mut support, pair, "catia_prune_incident_support_pairs")?;
                }
                if support.is_empty() {
                    return Ok(None);
                }
                if let Some(allowed) = &mut allowed {
                    allowed.retain(|pair| support.contains(pair));
                } else {
                    allowed = Some(support);
                }
            }
            let Some(allowed) = allowed else {
                return Ok(None);
            };
            domain.retain(|pair| {
                let mut pair = *pair;
                pair.sort_unstable();
                allowed.contains(&pair)
            });
            if domain.is_empty() {
                return Ok(None);
            }
        }
        let after = (
            faces.iter().map(Vec::len).sum::<usize>(),
            candidates.iter().map(Vec::len).sum::<usize>(),
        );
        if after == before {
            break;
        }
    }
    Ok(Some(candidates))
}

type MeshCorner = (usize, usize, usize);
type MeshCornerPoints = HashMap<MeshCorner, HashSet<usize>>;

// The tuple carries one coupled result; a separate alias would add no invariant.
#[allow(clippy::type_complexity)]
fn standard_mesh_assignment_corner_points(
    ctx: &DecodeContext<'_>,
    bytes: &[u8],
    edge_faces: &[[usize; 2]],
    edge_points: &[Option<[usize; 2]>],
) -> Result<
    Option<(
        Vec<Vec<Vec<MeshEdgePlacementCandidate>>>,
        MeshCornerPoints,
        Vec<Vec<usize>>,
    )>,
    CodecError,
> {
    (|| -> Option<Result<_, CodecError>> {
        let analysis = match standard_mesh_analysis(ctx, bytes) {
            Ok(Some(analysis)) => analysis,
            Ok(None) => return None,
            Err(error) => return Some(Err(error)),
        };
        let edge_rows = &analysis.edge_rows;
        if edge_rows.len() != edge_points.len() || edge_rows.len() != edge_faces.len() {
            return None;
        }
        let runs = match mesh_edge_runs(ctx, &analysis) {
            Ok(runs) => runs,
            Err(error) => return Some(Err(error)),
        };
        let assignments =
            match standard_mesh_missing_edge_assignments(ctx, bytes, edge_faces, None, true) {
                Ok(Some(assignments)) => assignments,
                Ok(None) => return None,
                Err(error) => return Some(Err(error)),
            };
        let mut cycle_lengths = Vec::new();
        if let Err(error) = crate::resource::reserve_vec(ctx, &mut cycle_lengths, analysis.cycles.len(), "catia_missing_cycle_length_rows") {
            return Some(Err(error));
        }
        for cycles in &analysis.cycles {
            let mut lengths = Vec::new();
            if let Err(error) = crate::resource::reserve_vec(ctx, &mut lengths, cycles.len(), "catia_missing_cycle_lengths") {
                return Some(Err(error));
            }
            lengths.extend(cycles.iter().map(Vec::len));
            cycle_lengths.push(lengths);
        }
        let mut corner_points = MeshCornerPoints::new();
        let mut run_constraints = Vec::new();
        for run in runs {
            let Some(pair) = edge_points[run.edge] else {
                continue;
            };
            let mut candidates = HashSet::new();
            for point in pair {
                if let Err(error) = crate::resource::insert_set(ctx, &mut candidates, point, "catia_corner_candidate_points") {
                    return Some(Err(error));
                }
            }
            let positions = [
                (run.face, run.cycle, run.start),
                (
                    run.face,
                    run.cycle,
                    run.end(cycle_lengths[run.face][run.cycle]),
                ),
            ];
            for position in positions {
                if let Some(stored) = corner_points.get_mut(&position) {
                    stored.retain(|point| candidates.contains(point));
                    if stored.is_empty() {
                        return None;
                    }
                } else {
                    let copied = match crate::resource::copy_retained_set(ctx, &candidates, "catia_corner_point_copy") {
                        Ok(copied) => copied,
                        Err(error) => return Some(Err(error)),
                    };
                    if let Err(error) = crate::resource::insert_map(ctx, &mut corner_points, position, copied, "catia_corner_point_entries") {
                        return Some(Err(error));
                    }
                }
            }
            if let Err(error) = crate::resource::push(ctx, &mut run_constraints, (positions[0], positions[1], pair), "catia_corner_run_constraints") {
                return Some(Err(error));
            }
        }
        loop {
            let before = corner_points.values().map(HashSet::len).sum::<usize>();
            for &(left, right, pair) in &run_constraints {
                let left_points = corner_points.get(&left)?;
                let right_points = corner_points.get(&right)?;
                let left_single = (left_points.len() == 1).then(|| left_points.iter().copied().next()).flatten();
                let right_single = (right_points.len() == 1).then(|| right_points.iter().copied().next()).flatten();
                if let Some(point) = left_single {
                    corner_points
                        .get_mut(&right)?
                        .retain(|candidate| *candidate != point && pair.contains(candidate));
                }
                if let Some(point) = right_single {
                    corner_points
                        .get_mut(&left)?
                        .retain(|candidate| *candidate != point && pair.contains(candidate));
                }
                if corner_points.get(&left)?.is_empty() || corner_points.get(&right)?.is_empty() {
                    return None;
                }
            }
            let after = corner_points.values().map(HashSet::len).sum::<usize>();
            if after == before {
                break;
            }
        }
        Some(Ok((assignments, corner_points, cycle_lengths)))
    })()
    .transpose()
}

/// Retain endpoint constraints on each placement inside each complete face
/// assignment. Assignment and placement order are unchanged from the serialized
/// face and edge order.
fn standard_mesh_missing_edge_endpoint_assignments(
    ctx: &DecodeContext<'_>,
    bytes: &[u8],
    edge_faces: &[[usize; 2]],
    edge_points: &[Option<[usize; 2]>],
) -> Result<Option<Vec<Vec<Vec<MeshEdgePlacementEndpointCandidate>>>>, CodecError> {
    let Some((assignments, corner_points, cycle_lengths)) =
        standard_mesh_assignment_corner_points(ctx, bytes, edge_faces, edge_points)?
    else {
        return Ok(None);
    };
    let mut faces = Vec::new();
    crate::resource::reserve_vec(ctx, &mut faces, assignments.len(), "catia_placement_endpoint_face_rows")?;
    for face in assignments {
        let mut face_assignments = Vec::new();
        crate::resource::reserve_vec(ctx, &mut face_assignments, face.len(), "catia_placement_endpoint_assignment_rows")?;
        for assignment in face {
            let mut placements = Vec::new();
            crate::resource::reserve_vec(ctx, &mut placements, assignment.len(), "catia_placement_endpoint_placement_rows")?;
            for placement in assignment {
                let endpoint_pairs = if let Some((starts, ends)) = corner_points
                    .get(&(placement.face, placement.cycle, placement.start))
                    .zip(corner_points.get(&(
                        placement.face,
                        placement.cycle,
                        placement.end(cycle_lengths[placement.face][placement.cycle]),
                    ))) {
                    let mut pairs = Vec::new();
                    for &start in starts {
                        for &end in ends {
                            if start != end {
                                let mut pair = [start, end];
                                pair.sort_unstable();
                                crate::resource::push(ctx, &mut pairs, pair, "catia_placement_endpoint_candidate_pairs")?;
                            }
                        }
                    }
                    pairs.sort_unstable();
                    pairs.dedup();
                    Some(pairs)
                } else {
                    None
                };
                placements.push(MeshEdgePlacementEndpointCandidate { placement, endpoint_pairs });
            }
            face_assignments.push(placements);
        }
        faces.push(face_assignments);
    }
    Ok(Some(faces))
}

/// Enforce resolved edge endpoint pairs and complete opposite-face placement
/// domains across correlated face assignments. A face assignment is removed as
/// a unit when any of its placements has no compatible endpoint pair.
fn standard_mesh_pruned_missing_edge_endpoint_assignments(
    ctx: &DecodeContext<'_>,
    bytes: &[u8],
    edge_faces: &[[usize; 2]],
    edge_points: &[Option<[usize; 2]>],
) -> Result<Option<Vec<Vec<Vec<MeshEdgePlacementEndpointCandidate>>>>, CodecError> {
    let Some(mut faces) =
        standard_mesh_missing_edge_endpoint_assignments(ctx, bytes, edge_faces, edge_points)?
    else {
        return Ok(None);
    };
    loop {
        let before = (
            faces.iter().map(Vec::len).sum::<usize>(),
            faces
                .iter()
                .flatten()
                .flatten()
                .filter_map(|candidate| candidate.endpoint_pairs.as_ref().map(Vec::len))
                .sum::<usize>(),
        );
        let mut face_domains = HashMap::<(usize, usize), Option<HashSet<[usize; 2]>>>::new();
        for (face, assignments) in faces.iter().enumerate() {
            for edge in edge_faces
                .iter()
                .enumerate()
                .filter_map(|(edge, incident)| incident.contains(&face).then_some(edge))
            {
                let mut domain = HashSet::new();
                let mut complete = true;
                for assignment in assignments {
                    let Some(pairs) = assignment.iter()
                        .find(|candidate| candidate.placement.edge == edge)
                        .and_then(|candidate| candidate.endpoint_pairs.as_ref()) else {
                        complete = false;
                        break;
                    };
                    for &pair in pairs {
                        crate::resource::insert_set(ctx, &mut domain, pair, "catia_placement_face_domain_pairs")?;
                    }
                }
                crate::resource::insert_map(ctx, &mut face_domains, (face, edge), complete.then_some(domain), "catia_placement_face_domain_entries")?;
            }
        }
        for assignments in &mut faces {
            assignments.retain_mut(|assignment| {
                assignment.iter_mut().all(|candidate| {
                    let edge = candidate.placement.edge;
                    let seed = edge_points[edge].map(|mut pair| {
                        pair.sort_unstable();
                        pair
                    });
                    let opposite = edge_faces[edge]
                        .into_iter()
                        .find(|&face| face != candidate.placement.face)
                        .and_then(|face| face_domains.get(&(face, edge)))
                        .and_then(Option::as_ref);
                    let Some(domain) = &mut candidate.endpoint_pairs else {
                        return true;
                    };
                    domain.retain(|pair| {
                        seed.is_none_or(|seed| same_unordered_pair(*pair, seed))
                            && opposite.is_none_or(|opposite| opposite.contains(pair))
                    });
                    !domain.is_empty()
                })
            });
            if assignments.is_empty() {
                return Ok(None);
            }
        }
        let after = (
            faces.iter().map(Vec::len).sum::<usize>(),
            faces
                .iter()
                .flatten()
                .flatten()
                .filter_map(|candidate| candidate.endpoint_pairs.as_ref().map(Vec::len))
                .sum::<usize>(),
        );
        if after == before {
            break;
        }
    }
    Ok(Some(faces))
}

/// Derive endpoint-pair domains for unmatched rows whose candidate placement
/// corners are both bound by exact matched edge runs. Input pairs are physical
/// edge-row ordered; pair orientation is ignored in the returned domains
/// because a missing placement has not yet selected its traversal direction.
pub(crate) fn standard_mesh_placement_endpoint_pairs(
    ctx: &DecodeContext<'_>,
    bytes: &[u8],
    edge_faces: &[[usize; 2]],
    edge_points: &[Option<[usize; 2]>],
) -> Result<Option<Vec<Vec<[usize; 2]>>>, CodecError> {
    let Some(edge_rows) = standard_edge_rows(ctx, bytes)? else {
        return Ok(None);
    };
    if edge_rows.len() != edge_points.len() || edge_rows.len() != edge_faces.len() {
        return Ok(None);
    }
    let Some(assignments) = standard_mesh_pruned_missing_edge_endpoint_assignments(
        ctx,
        bytes,
        edge_faces,
        edge_points,
    )?
    else {
        return Ok(None);
    };
    let mut domains = ctx.alloc_filled(
        edge_rows.len(),
        Vec::new(),
        "catia_placement_endpoint_domains",
    )?;
    let mut placement_counts =
        ctx.alloc_filled(edge_rows.len(), 0usize, "catia_placement_counts")?;
    let mut bound_counts =
        ctx.alloc_filled(edge_rows.len(), 0usize, "catia_placement_bound_counts")?;
    for face in assignments {
        let mut placements = Vec::new();
        for placement in face.into_iter().flatten() {
            crate::resource::push(
                ctx,
                &mut placements,
                placement,
                "catia_placement_candidates",
            )?;
        }
        placements.sort_unstable_by_key(|candidate| candidate.placement);
        placements.dedup_by_key(|candidate| candidate.placement);
        for candidate in placements {
            let edge = candidate.placement.edge;
            placement_counts[edge] += 1;
            if let Some(pairs) = candidate.endpoint_pairs {
                bound_counts[edge] += 1;
                for pair in pairs {
                    if !domains[edge].contains(&pair) {
                        crate::resource::push(
                            ctx,
                            &mut domains[edge],
                            pair,
                            "catia_placement_endpoint_pairs",
                        )?;
                    }
                }
            }
        }
    }
    for (edge, domain) in domains.iter_mut().enumerate() {
        if bound_counts[edge] == placement_counts[edge] {
            domain.sort_unstable();
        } else {
            domain.clear();
        }
    }
    Ok(Some(domains))
}

fn bind_port_point(
    ctx: &DecodeContext<'_>,
    port_points: &mut HashMap<u32, usize>,
    port: u32,
    point: usize,
) -> Result<bool, CodecError> {
    if let Some(&stored) = port_points.get(&port) {
        return Ok(stored == point);
    }
    crate::resource::insert_map(ctx, port_points, port, point, "catia_port_bound_points")?;
    Ok(true)
}

/// Propagate byte-level endpoint ports through independently resolved physical
/// edge endpoint pairs. The result is rejected atomically when any port mapping
/// contradicts a resolved pair.
pub(super) fn propagate_edge_port_points(
    ctx: &DecodeContext<'_>,
    edge_ports: &[[u32; 2]],
    endpoint_pairs: &[Option<[usize; 2]>],
) -> Result<Option<Vec<Option<[usize; 2]>>>, CodecError> {
    propagate_edge_port_points_with_ordered_seeds(ctx, edge_ports, endpoint_pairs, &[])
}

/// Propagate endpoint points through physical edge ports, retaining an
/// independently decoded orientation when one is available.
///
/// `ordered_endpoint_pairs` is indexed like `edge_ports`. A supplied pair is
/// in the physical row direction, not merely an unordered candidate. It must
/// agree with an existing candidate when that candidate is present. Such a
/// seed is the only valid way to orient a port component whose resolved rows
/// all carry the same unordered pair.
pub(crate) fn propagate_edge_port_points_with_ordered_seeds(
    ctx: &DecodeContext<'_>,
    edge_ports: &[[u32; 2]],
    endpoint_pairs: &[Option<[usize; 2]>],
    ordered_endpoint_pairs: &[Option<[usize; 2]>],
) -> Result<Option<Vec<Option<[usize; 2]>>>, CodecError> {
    (|| -> Option<Result<Vec<Option<[usize; 2]>>, CodecError>> {
        if edge_ports.len() != endpoint_pairs.len() {
            return None;
        }
        if !ordered_endpoint_pairs.is_empty()
            && ordered_endpoint_pairs.len() != endpoint_pairs.len()
        {
            return None;
        }
        let mut resolved = match crate::resource::copy_slice(ctx, endpoint_pairs, "catia_port_resolved_pairs") {
            Ok(resolved) => resolved,
            Err(error) => return Some(Err(error)),
        };
        let mut edges_by_port = HashMap::<u32, Vec<usize>>::new();
        for (edge, ports) in edge_ports.iter().enumerate() {
            if let Err(error) = crate::resource::admit_map_entry(ctx, &mut edges_by_port, &ports[0], "catia_port_edge_entries") {
                return Some(Err(error));
            }
            if let Err(error) = crate::resource::push(ctx, edges_by_port.entry(ports[0]).or_default(), edge, "catia_port_incident_edges") {
                return Some(Err(error));
            }
            if ports[1] != ports[0] {
                if let Err(error) = crate::resource::admit_map_entry(ctx, &mut edges_by_port, &ports[1], "catia_port_edge_entries") {
                    return Some(Err(error));
                }
                if let Err(error) = crate::resource::push(ctx, edges_by_port.entry(ports[1]).or_default(), edge, "catia_port_incident_edges") {
                    return Some(Err(error));
                }
            }
        }
        let mut port_points = HashMap::<u32, usize>::new();

        if !ordered_endpoint_pairs.is_empty() {
            for (edge, ordered) in ordered_endpoint_pairs.iter().enumerate() {
                let Some(ordered) = ordered else { continue };
                if resolved[edge].is_some_and(|pair| !same_unordered_pair(pair, *ordered)) {
                    return None;
                }
                let ports = edge_ports[edge];
                if ports[0] == ports[1] && ordered[0] != ordered[1] {
                    return None;
                }
                let first = match bind_port_point(ctx, &mut port_points, ports[0], ordered[0]) {
                    Ok(first) => first,
                    Err(error) => return Some(Err(error)),
                };
                if !first {
                    return None;
                }
                let second = match bind_port_point(ctx, &mut port_points, ports[1], ordered[1]) {
                    Ok(second) => second,
                    Err(error) => return Some(Err(error)),
                };
                if !second {
                    return None;
                }
                resolved[edge] = Some(*ordered);
            }
        }

        for (&port, edges) in &edges_by_port {
            let mut intersection: Option<HashSet<usize>> = None;
            for &edge in edges {
                let Some(pair) = resolved[edge] else { continue };
                let mut points = HashSet::new();
                for point in pair {
                    if let Err(error) = crate::resource::insert_set(ctx, &mut points, point, "catia_port_pair_points") {
                        return Some(Err(error));
                    }
                }
                if let Some(current) = intersection.take() {
                    let mut common = HashSet::new();
                    for &point in current.intersection(&points) {
                        if let Err(error) = crate::resource::insert_set(ctx, &mut common, point, "catia_port_intersection_points") {
                            return Some(Err(error));
                        }
                    }
                    intersection = Some(common);
                } else {
                    intersection = Some(points);
                }
            }
            if let Some(points) = intersection {
                if points.len() == 1 {
                    let point = *points.iter().next()?;
                    match bind_port_point(ctx, &mut port_points, port, point) {
                        Ok(true) => {}
                        Ok(false) => return None,
                        Err(error) => return Some(Err(error)),
                    }
                }
            }
        }

        let mut queue = std::collections::VecDeque::new();
        for edge in 0..edge_ports.len() {
            if let Err(error) = crate::resource::push_back(ctx, &mut queue, edge, "catia_edge_port_initial_queue") {
                return Some(Err(error));
            }
        }
        let mut queued = match ctx.alloc_filled(edge_ports.len(), true, "catia_edge_port_queue") {
            Ok(queued) => queued,
            Err(error) => return Some(Err(error)),
        };
        while let Some(edge) = queue.pop_front() {
            queued[edge] = false;
            let ports = edge_ports[edge];
            let inserted = if let Some([left, right]) = resolved[edge] {
                match (
                    port_points.get(&ports[0]).copied(),
                    port_points.get(&ports[1]).copied(),
                ) {
                    (Some(point), None) if point == left => Some((ports[1], right)),
                    (Some(point), None) if point == right => Some((ports[1], left)),
                    (None, Some(point)) if point == left => Some((ports[0], right)),
                    (None, Some(point)) if point == right => Some((ports[0], left)),
                    (Some(_), None) | (None, Some(_)) => return None,
                    (Some(left_point), Some(right_point))
                        if !same_unordered_pair([left_point, right_point], [left, right]) =>
                    {
                        return None;
                    }
                    _ => None,
                }
            } else {
                None
            };
            if let Some((port, point)) = inserted {
                if let Err(error) = crate::resource::insert_map(ctx, &mut port_points, port, point, "catia_port_propagated_points") {
                    return Some(Err(error));
                }
            }
            if let (Some(&left), Some(&right)) =
                (port_points.get(&ports[0]), port_points.get(&ports[1]))
            {
                if ports[0] == ports[1] || left != right {
                    if resolved[edge].is_some_and(|pair| !same_unordered_pair(pair, [left, right]))
                    {
                        return None;
                    }
                    resolved[edge] = Some([left, right]);
                }
            }
            if let Some((port, _)) = inserted {
                for &neighbor in edges_by_port.get(&port)? {
                    if !queued[neighbor] {
                        queued[neighbor] = true;
                        if let Err(error) = crate::resource::push_back(ctx, &mut queue, neighbor, "catia_edge_port_neighbor_queue") {
                            return Some(Err(error));
                        }
                    }
                }
            }
        }
        let mut resolved_ports = Vec::new();
        let mut resolved_candidates = Vec::new();
        for (ports, pair) in edge_ports.iter().copied().zip(resolved.iter().copied()) {
            if let Some(pair) = pair {
                if let Err(error) = crate::resource::push(ctx, &mut resolved_ports, ports, "catia_port_resolved_port_rows") {
                    return Some(Err(error));
                }
                let candidate = match ctx.alloc_filled(1, pair, "catia_port_resolved_candidate_pair") {
                    Ok(candidate) => candidate,
                    Err(error) => return Some(Err(error)),
                };
                if let Err(error) = crate::resource::push(ctx, &mut resolved_candidates, candidate, "catia_port_resolved_candidate_rows") {
                    return Some(Err(error));
                }
            }
        }
        match edge_port_candidate_assignment(
            ctx,
            &resolved_ports,
            &resolved_candidates,
            false,
            true,
        ) {
            Ok(Some(_)) => {}
            Ok(None) => return None,
            Err(error) => return Some(Err(error)),
        }
        Some(Ok(resolved))
    })()
    .transpose()
}

/// Propagate endpoint points while leaving every port component that touches
/// an unresolved row to the joint topology solver. Independently ordered
/// seeds remain usable, but unordered candidate rows in that component do not
/// orient or constrain one another prematurely.
pub(crate) fn propagate_edge_port_points_with_ordered_seeds_and_deferred(
    ctx: &DecodeContext<'_>,
    edge_ports: &[[u32; 2]],
    endpoint_pairs: &[Option<[usize; 2]>],
    ordered_endpoint_pairs: &[Option<[usize; 2]>],
    deferred_edges: &[bool],
) -> Result<Option<Vec<Option<[usize; 2]>>>, CodecError> {
    if edge_ports.len() != endpoint_pairs.len()
        || deferred_edges.len() != endpoint_pairs.len()
        || (!ordered_endpoint_pairs.is_empty()
            && ordered_endpoint_pairs.len() != endpoint_pairs.len())
    {
        return Ok(None);
    }
    let mut effective_deferred =
        crate::resource::copy_slice(ctx, deferred_edges, "catia_ordered_seed_deferred_copy")?;
    if !expand_deferred_edge_port_components(ctx, edge_ports, &mut effective_deferred)? {
        return Ok(None);
    }
    let mut masked_pairs =
        crate::resource::copy_slice(ctx, endpoint_pairs, "catia_ordered_seed_pair_copy")?;
    for (edge, deferred) in effective_deferred.into_iter().enumerate() {
        if deferred
            && ordered_endpoint_pairs
                .get(edge)
                .copied()
                .flatten()
                .is_none()
        {
            masked_pairs[edge] = None;
        }
    }
    propagate_edge_port_points_with_ordered_seeds(
        ctx,
        edge_ports,
        &masked_pairs,
        ordered_endpoint_pairs,
    )
}

/// Propagate ordered endpoint seeds through the subset of rows with native
/// port identities. Rows without a port pair retain their independent seed or
/// candidate, but cannot participate in port propagation.
pub(crate) fn propagate_partial_edge_port_points_with_ordered_seeds(
    ctx: &DecodeContext<'_>,
    edge_ports: &[Option<[u32; 2]>],
    endpoint_pairs: &[Option<[usize; 2]>],
    ordered_endpoint_pairs: &[Option<[usize; 2]>],
) -> Result<Option<Vec<Option<[usize; 2]>>>, CodecError> {
    if edge_ports.len() != endpoint_pairs.len() {
        return Ok(None);
    }
    if !ordered_endpoint_pairs.is_empty() && ordered_endpoint_pairs.len() != endpoint_pairs.len() {
        return Ok(None);
    }
    let mut resolved = crate::resource::copy_slice(ctx, endpoint_pairs, "catia_partial_port_resolved_pairs")?;
    if !ordered_endpoint_pairs.is_empty() {
        for (edge, ordered) in ordered_endpoint_pairs.iter().enumerate() {
            let Some(ordered) = ordered else { continue };
            if resolved[edge].is_some_and(|pair| !same_unordered_pair(pair, *ordered)) {
                return Ok(None);
            }
            resolved[edge] = Some(*ordered);
        }
    }
    let mut known = Vec::new();
    for (edge, ports) in edge_ports.iter().enumerate() {
        if let Some(ports) = ports {
            crate::resource::push(ctx, &mut known, (edge, *ports), "catia_partial_known_port_rows")?;
        }
    }
    if known.is_empty() {
        return Ok(Some(resolved));
    }
    let mut ports = Vec::new();
    let mut pairs = Vec::new();
    let mut ordered = Vec::new();
    crate::resource::reserve_vec(ctx, &mut ports, known.len(), "catia_partial_port_rows")?;
    crate::resource::reserve_vec(ctx, &mut pairs, known.len(), "catia_partial_pair_rows")?;
    crate::resource::reserve_vec(ctx, &mut ordered, known.len(), "catia_partial_ordered_rows")?;
    for &(edge, port) in &known {
        ports.push(port);
        pairs.push(resolved[edge]);
        ordered.push(ordered_endpoint_pairs.get(edge).copied().flatten());
    }
    let Some(propagated) =
        propagate_edge_port_points_with_ordered_seeds(ctx, &ports, &pairs, &ordered)?
    else {
        return Ok(None);
    };
    for ((edge, _), pair) in known.into_iter().zip(propagated) {
        resolved[edge] = pair;
    }
    Ok(Some(resolved))
}

#[derive(Clone, Copy)]
enum PortCandidateSearchMode {
    FirstNative,
    UniqueNative,
    UniqueMesh,
}

impl PortCandidateSearchMode {
    fn requires_unique(self) -> bool {
        matches!(self, Self::UniqueNative | Self::UniqueMesh)
    }

    fn enforces_point_bijection(self) -> bool {
        matches!(self, Self::FirstNative | Self::UniqueNative)
    }
}

fn port_candidate_pair_key(pair: [usize; 2]) -> [usize; 2] {
    if pair[0] <= pair[1] {
        pair
    } else {
        [pair[1], pair[0]]
    }
}

struct PortCandidateSearch<'a, 'b> {
    ctx: &'a DecodeContext<'b>,
    ports: &'a [[u32; 2]],
    candidates: &'a [Vec<[usize; 2]>],
    port_points: HashMap<u32, usize>,
    point_ports: HashMap<usize, u32>,
    edge_pairs: Vec<Option<[usize; 2]>>,
    outcome: SearchOutcome<Vec<[usize; 2]>>,
    states: usize,
    mode: PortCandidateSearchMode,
}

impl PortCandidateSearch<'_, '_> {
    fn compatible(&self, edge: usize, pair: [usize; 2]) -> [Option<[usize; 2]>; 2] {
        let mut oriented = [
            Some(pair),
            (pair[0] != pair[1]).then_some([pair[1], pair[0]]),
        ];
        for option in &mut oriented {
            let Some(points) = option else {
                continue;
            };
            let compatible = (self.ports[edge][0] == self.ports[edge][1])
                == (points[0] == points[1])
                && self.ports[edge]
                    .iter()
                    .zip(points.iter().copied())
                    .all(|(&port, point)| {
                        self.port_points
                            .get(&port)
                            .is_none_or(|stored| *stored == point)
                            && (!self.mode.enforces_point_bijection()
                                || self
                                    .point_ports
                                    .get(&point)
                                    .is_none_or(|stored| *stored == port))
                    });
            if !compatible {
                *option = None;
            }
        }
        oriented
    }

    fn assign(&mut self, edge: usize, points: [usize; 2]) -> Result<Vec<(u32, usize)>, CodecError> {
        let mut inserted = Vec::new();
        for (&port, point) in self.ports[edge].iter().zip(points) {
            if !self.port_points.contains_key(&port) {
                crate::resource::insert_map(
                    self.ctx,
                    &mut self.port_points,
                    port,
                    point,
                    "catia_port_search_points",
                )?;
                if self.mode.enforces_point_bijection() {
                    crate::resource::insert_map(
                        self.ctx,
                        &mut self.point_ports,
                        point,
                        port,
                        "catia_port_search_reverse_points",
                    )?;
                }
                crate::resource::push(
                    self.ctx,
                    &mut inserted,
                    (port, point),
                    "catia_port_search_inserted",
                )?;
            }
        }
        self.edge_pairs[edge] = Some(points);
        Ok(inserted)
    }

    fn unassign(&mut self, edge: usize, inserted: Vec<(u32, usize)>) {
        self.edge_pairs[edge] = None;
        for (port, point) in inserted {
            self.port_points.remove(&port);
            if self.mode.enforces_point_bijection() {
                self.point_ports.remove(&point);
            }
        }
    }

    fn rollback(&mut self, propagated: Vec<(usize, Vec<(u32, usize)>)>) {
        for (edge, inserted) in propagated.into_iter().rev() {
            self.unassign(edge, inserted);
        }
    }

    fn search(&mut self) -> Result<(), CodecError> {
        // Native-port binding precedes geometric incidence fallback but can
        // still contain symmetric coordinate assignments. Ambiguity beyond
        // this bound is retained for later paths rather than partially bound.
        const MAX_STATES: usize = 1_024;
        let _depth = self.ctx.enter_nested("catia_port_candidate_search")?;
        self.ctx.charge_work(1, "catia_port_candidate_search")?;
        if self.outcome.is_closed()
            || (!self.mode.requires_unique() && matches!(self.outcome, SearchOutcome::Solved(_)))
        {
            return Ok(());
        }
        let mut propagated = Vec::new();
        let branch = loop {
            let mut best = None;
            let mut progress = false;
            let mut incomplete = false;
            for edge in 0..self.ports.len() {
                if self.edge_pairs[edge].is_some() {
                    continue;
                }
                incomplete = true;
                let mut options = self.candidates[edge]
                    .iter()
                    .flat_map(|pair| self.compatible(edge, *pair).into_iter().flatten());
                let Some(first) = options.next() else {
                    self.rollback(propagated);
                    return Ok(());
                };
                if options.next().is_some() {
                    let count = 2 + options.count();
                    if best.is_none_or(|(stored, _)| count < stored) {
                        best = Some((count, edge));
                    }
                    continue;
                }
                let inserted = self.assign(edge, first)?;
                crate::resource::push(
                    self.ctx,
                    &mut propagated,
                    (edge, inserted),
                    "catia_port_search_propagated",
                )?;
                progress = true;
            }
            if !incomplete {
                break None;
            }
            if progress {
                continue;
            }
            break best.map(|(_, edge)| edge);
        };
        let Some(edge) = branch else {
            let mut candidate = Vec::new();
            let mut complete = true;
            for pair in &self.edge_pairs {
                let Some(pair) = *pair else {
                    complete = false;
                    break;
                };
                crate::resource::push(
                    self.ctx,
                    &mut candidate,
                    pair,
                    "catia_port_search_solution",
                )?;
            }
            if complete {
                self.outcome.record_solved(candidate, |previous, next| {
                    previous.len() == next.len()
                        && previous.iter().zip(next).all(|(&left, &right)| {
                            port_candidate_pair_key(left) == port_candidate_pair_key(right)
                        })
                });
            }
            self.rollback(propagated);
            return Ok(());
        };
        if self.states >= MAX_STATES {
            self.outcome.exhaust();
        } else {
            self.states += 1;
            'candidates: for candidate in 0..self.candidates[edge].len() {
                for points in self
                    .compatible(edge, self.candidates[edge][candidate])
                    .into_iter()
                    .flatten()
                {
                    let inserted = self.assign(edge, points)?;
                    self.search()?;
                    self.unassign(edge, inserted);
                    if !self.mode.requires_unique()
                        && matches!(self.outcome, SearchOutcome::Solved(_))
                    {
                        break 'candidates;
                    }
                }
            }
        }
        self.rollback(propagated);
        Ok(())
    }
}

/// Bind native edge endpoint identities to coordinate rows while respecting
/// every edge's geometrically admissible unordered endpoint pairs.
pub(crate) fn bind_edge_port_candidates(
    ctx: &DecodeContext<'_>,
    ports: &[[u32; 2]],
    candidates: &[Vec<[usize; 2]>],
) -> Result<Option<Vec<[usize; 2]>>, CodecError> {
    edge_port_candidate_assignment(ctx, ports, candidates, true, true)
}

/// Resolve mesh-port endpoint candidates without imposing a point-to-port
/// bijection. Mesh ports are occurrence identities: several incident ports
/// may terminate at one coordinate row. The result is canonicalized as
/// unordered endpoint pairs and is returned only when port equality admits
/// exactly one such assignment.
pub(crate) fn unique_mesh_edge_port_candidate_pairs(
    ctx: &DecodeContext<'_>,
    ports: &[[u32; 2]],
    candidates: &[Vec<[usize; 2]>],
) -> Result<Option<Vec<[usize; 2]>>, CodecError> {
    let Some(mut pairs) = edge_port_candidate_assignment(ctx, ports, candidates, true, false)?
    else {
        return Ok(None);
    };
    for pair in &mut pairs {
        pair.sort_unstable();
    }
    Ok(Some(pairs))
}

/// Resolve only settled mesh-port rows when repeated-face rows still carry
/// an open face domain. Deferred rows contribute neither candidate support nor
/// connectivity to this search and remain unresolved in the result.
pub(crate) fn unique_mesh_edge_port_candidate_pairs_with_deferred(
    ctx: &DecodeContext<'_>,
    ports: &[[u32; 2]],
    candidates: &[Vec<[usize; 2]>],
    deferred_edges: &[bool],
) -> Result<Option<Vec<Option<[usize; 2]>>>, CodecError> {
    if ports.len() != candidates.len() || deferred_edges.len() != candidates.len() {
        return Ok(None);
    }
    let mut effective_deferred =
        crate::resource::copy_slice(ctx, deferred_edges, "catia_candidate_deferred_copy")?;
    if !expand_deferred_edge_port_components(ctx, ports, &mut effective_deferred)? {
        return Ok(None);
    }
    let mut settled = Vec::new();
    let mut settled_ports = Vec::new();
    let mut settled_candidates = Vec::new();
    for edge in 0..ports.len() {
        if !effective_deferred[edge] {
            crate::resource::push(ctx, &mut settled, edge, "catia_candidate_settled_edges")?;
            crate::resource::push(
                ctx,
                &mut settled_ports,
                ports[edge],
                "catia_candidate_settled_ports",
            )?;
            crate::resource::push(
                ctx,
                &mut settled_candidates,
                crate::resource::copy_slice(
                    ctx,
                    &candidates[edge],
                    "catia_candidate_settled_pairs",
                )?,
                "catia_candidate_settled_rows",
            )?;
        }
    }
    let Some(settled_pairs) =
        unique_mesh_edge_port_candidate_pairs(ctx, &settled_ports, &settled_candidates)?
    else {
        return Ok(None);
    };
    let mut resolved = ctx.alloc_filled(candidates.len(), None, "catia_candidate_resolved")?;
    for (edge, pair) in settled.into_iter().zip(settled_pairs) {
        resolved[edge] = Some(pair);
    }
    Ok(Some(resolved))
}

fn edge_port_candidate_assignment(
    ctx: &DecodeContext<'_>,
    ports: &[[u32; 2]],
    candidates: &[Vec<[usize; 2]>],
    require_unique: bool,
    enforce_point_bijection: bool,
) -> Result<Option<Vec<[usize; 2]>>, CodecError> {
    if ports.len() != candidates.len() || candidates.iter().any(Vec::is_empty) {
        return Ok(None);
    }
    let mode = match (require_unique, enforce_point_bijection) {
        (false, true) => PortCandidateSearchMode::FirstNative,
        (true, true) => PortCandidateSearchMode::UniqueNative,
        (true, false) => PortCandidateSearchMode::UniqueMesh,
        (false, false) => return Ok(None),
    };
    let mut dependencies = UnionFind::charged(ctx, ports.len(), "catia_port_dependency_union")?;
    let mut edge_by_port = HashMap::new();
    let mut edge_by_point = HashMap::new();
    for edge in 0..ports.len() {
        for port in ports[edge] {
            if let Some(previous) = crate::resource::insert_map(
                ctx,
                &mut edge_by_port,
                port,
                edge,
                "catia_port_dependency_ports",
            )? {
                dependencies.union(previous, edge);
            }
        }
        if enforce_point_bijection {
            for point in candidates[edge].iter().flatten() {
                if let Some(previous) = crate::resource::insert_map(
                    ctx,
                    &mut edge_by_point,
                    *point,
                    edge,
                    "catia_port_dependency_points",
                )? {
                    dependencies.union(previous, edge);
                }
            }
        }
    }
    let mut groups = HashMap::<usize, Vec<usize>>::new();
    for edge in 0..ports.len() {
        let root = dependencies.find(edge);
        if let Some(group) = groups.get_mut(&root) {
            crate::resource::push(ctx, group, edge, "catia_port_component_edges")?;
        } else {
            let mut group = Vec::new();
            crate::resource::push(ctx, &mut group, edge, "catia_port_component_edges")?;
            crate::resource::insert_map(
                ctx,
                &mut groups,
                root,
                group,
                "catia_port_component_roots",
            )?;
        }
    }
    let mut components = Vec::new();
    for group in groups.into_values() {
        crate::resource::push(ctx, &mut components, group, "catia_port_components")?;
    }
    components.sort_by_key(|component| component[0]);
    let mut solution = ctx.alloc_filled(ports.len(), None, "catia_edge_port_solution")?;
    for component in components {
        let mut component_ports = Vec::new();
        let mut component_candidates = Vec::new();
        for &edge in &component {
            crate::resource::push(
                ctx,
                &mut component_ports,
                ports[edge],
                "catia_port_component_ports",
            )?;
            let pairs = crate::resource::copy_slice(
                ctx,
                &candidates[edge],
                "catia_port_component_candidate_pairs",
            )?;
            crate::resource::push(
                ctx,
                &mut component_candidates,
                pairs,
                "catia_port_component_candidates",
            )?;
        }
        let mut search = PortCandidateSearch {
            ctx,
            ports: &component_ports,
            candidates: &component_candidates,
            port_points: HashMap::new(),
            point_ports: HashMap::new(),
            edge_pairs: ctx.alloc_filled(component.len(), None, "catia_edge_port_pairs")?,
            outcome: SearchOutcome::Open,
            states: 0,
            mode,
        };
        search.search()?;
        let SearchOutcome::Solved(component_solution) = search.outcome else {
            return Ok(None);
        };
        for (&edge, pair) in component.iter().zip(component_solution) {
            solution[edge] = Some(pair);
        }
    }
    let mut result = Vec::new();
    for pair in solution {
        let Some(pair) = pair else {
            return Ok(None);
        };
        crate::resource::push(ctx, &mut result, pair, "catia_port_assignment_result")?;
    }
    Ok(Some(result))
}

pub(crate) fn same_unordered_pair(left: [usize; 2], right: [usize; 2]) -> bool {
    left == right || left == [right[1], right[0]]
}

pub(crate) fn motif_port_points(
    ctx: &DecodeContext<'_>,
    trims: &[TrimRecord],
    vertex_count: usize,
) -> Result<Option<HashMap<u32, usize>>, CodecError> {
    fn columns(record: &TrimRecord) -> Option<([u32; 2], [u32; 2])> {
        Some((
            [
                *record.packet.handles().first()?,
                *record.packet.handles().get(1)?,
            ],
            [
                *record
                    .packet
                    .handles()
                    .get(record.packet.handles().len().checked_sub(2)?)?,
                *record.packet.handles().last()?,
            ],
        ))
    }
    fn emit(
        ctx: &DecodeContext<'_>,
        seen: &mut HashMap<u32, usize>,
        handle: u32,
    ) -> Result<(), CodecError> {
        if !seen.contains_key(&handle) {
            let next = seen.len();
            crate::resource::insert_map(ctx, seen, handle, next, "catia_motif_port_points")?;
        }
        Ok(())
    }
    fn emit_column(
        ctx: &DecodeContext<'_>,
        seen: &mut HashMap<u32, usize>,
        column: [u32; 2],
    ) -> Result<(), CodecError> {
        emit(ctx, seen, column[0])?;
        emit(ctx, seen, column[1])
    }

    let mut seen = HashMap::new();
    let mut at = 0usize;
    let Some(first_three) = trims.get(0..3) else {
        return Ok(None);
    };
    if first_three.iter().all(|record| record.kind == 0x4a) {
        let Some((first_a, first_b)) = columns(&trims[0]) else {
            return Ok(None);
        };
        let Some((third_a, third_b)) = columns(&trims[2]) else {
            return Ok(None);
        };
        for column in [third_a, first_b, first_a, third_b] {
            emit_column(ctx, &mut seen, column)?;
        }
        at = 3;
    }
    if trims.get(at..at + 3).is_some_and(|records| {
        records[0].kind == 0x42 && records[1].kind == 0x4a && records[2].kind == 0x42
    }) {
        let Some((strip0_first, strip0_last)) = columns(&trims[at]) else {
            return Ok(None);
        };
        let Some((quad_first, _)) = columns(&trims[at + 1]) else {
            return Ok(None);
        };
        let Some((strip1_first, _)) = columns(&trims[at + 2]) else {
            return Ok(None);
        };
        for column in [strip0_last, strip0_first, quad_first, strip1_first] {
            emit_column(ctx, &mut seen, column)?;
        }
        at += 3;
    }
    while trims.get(at).is_some_and(|record| record.kind == 0x4a) {
        let Some((first, last)) = columns(&trims[at]) else {
            return Ok(None);
        };
        emit_column(ctx, &mut seen, first)?;
        emit_column(ctx, &mut seen, last)?;
        at += 1;
    }
    while at < trims.len() {
        if trims.get(at..at + 3).is_some_and(|records| {
            records[0].kind == 0x42 && records[1].kind == 0x4a && records[2].kind == 0x42
        }) {
            let Some(([a0, b0], [a1, b1])) = columns(&trims[at]) else {
                return Ok(None);
            };
            let Some(([c, d], [qa, qb])) = columns(&trims[at + 1]) else {
                return Ok(None);
            };
            let Some(([e, g], [sc, sd])) = columns(&trims[at + 2]) else {
                return Ok(None);
            };
            if [qa, qb] == [a0, b0] && [sc, sd] == [c, d] {
                for handle in [a1, b1, b0, d, g, a0, c, e] {
                    emit(ctx, &mut seen, handle)?;
                }
                at += 3;
                continue;
            }
        }
        if trims
            .get(at..at + 2)
            .is_some_and(|records| records[0].kind == 0x4a && records[1].kind == 0x4a)
            && trims[at].packet.handles().len() >= 4
            && trims[at + 1].packet.handles().len() >= 2
        {
            for handle in [
                trims[at + 1].packet.handles()[0],
                trims[at].packet.handles()[2],
                trims[at].packet.handles()[3],
                trims[at + 1].packet.handles()[1],
            ] {
                emit(ctx, &mut seen, handle)?;
            }
            at += 2;
            continue;
        }
        return Ok(None);
    }
    Ok((at == trims.len() && seen.len() == vertex_count).then_some(seen))
}

#[cfg(test)]
mod tests;
