//! Mesh missing-edge enumeration for standard nested B-rep streams.
//!
//! Recovers unmatched edge-row placements against serialized face coverage.

use cadmpeg_core::decode::{index_from_u32, u64_from_index};

type MissingEdgeDomainsOutput =
    Result<Option<(Vec<MeshFaceAssignmentDomain>, Vec<MeshEdgeRun>)>, CodecError>;

use crate::families::standard::fbb::{
    boundary_cycles, largest_fbb_run, parse_edge_tables, parse_fbb_edge_tables,
    parse_standard_edge_tables_scoped, parse_standard_edge_tables_with_width, parse_trim_chain,
    parse_vertex_table, selected_standard_run,
};
#[cfg(test)]
use crate::families::standard::topology::{reconstruct_mesh_selection, StandardTopologyDraft};
use crate::families::standard::topology::{EdgeBoundaryLayout, EdgeRow, TrimRecord};
use crate::solve::mesh_quotient::{SearchOutcome, MAX_MESH_CONSTRAINT_OPERATIONS};
use crate::solve::union_find::UnionFind;
use cadmpeg_core::decode::{DecodeContext, View, WorkBudget};
use cadmpeg_core::CodecError;
use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::sync::Arc;

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
    let (parsed, _parsed_storage) =
        ctx.with_scoped_storage("catia_standard_port_rows", || -> Result<_, CodecError> {
            let Some(face_run) = selected_standard_run(ctx, bytes)? else {
                return Ok(None);
            };
            let after_faces = face_run.after_faces();
            let Some((edge_rows, scopes, _, _)) =
                parse_standard_edge_tables_scoped(ctx, bytes, after_faces)?
            else {
                return Ok(None);
            };
            Ok(Some((edge_rows, scopes)))
        })?;
    let Some((edge_rows, scopes)) = parsed else {
        return Ok(None);
    };
    let mut scratch = ctx.reserve_scoped(0, "catia_standard_port_handle_ids")?;
    let mut identity_by_handle = HashMap::new();
    let mut next_identity = 0u32;
    let mut pairs = ctx.collection_vec(edge_rows.len(), "catia_standard_port_pairs")?;
    let mut charged_steps = edge_rows.iter().zip(scopes);
    while let Some((row, scope)) =
        ctx.next_charged(&mut charged_steps, "catia_standard_port_pairs")?
    {
        let (Some(&first), Some(&last)) = (row.handles().first(), row.handles().last()) else {
            return Ok(None);
        };
        let pair = if !global && row.boundary_layout() != EdgeBoundaryLayout::CompleteBoundaryRun {
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
                let identity = if let Some(identity) =
                    ctx.get_hash_map(&identity_by_handle, &key, "catia_standard_port_handle_ids")?
                {
                    *identity
                } else {
                    let identity = next_identity;
                    let Some(next) = next_identity.checked_add(1) else {
                        return Ok(None);
                    };
                    next_identity = next;
                    scratch.with_storage(|| {
                        ctx.insert_hash_map(
                            &mut identity_by_handle,
                            key,
                            identity,
                            "catia_standard_port_handle_ids",
                        )
                    })?;
                    identity
                };
                pair[port] = identity;
            }
            pair
        };
        pairs.push(pair);
    }
    Ok(Some(pairs))
}

fn fbb_edge_port_identities_with_namespace(
    ctx: &DecodeContext<'_>,
    bytes: &[u8],
    global: bool,
) -> Result<Option<Vec<[u32; 2]>>, CodecError> {
    let (parsed, _parsed_storage) =
        ctx.with_scoped_storage("catia_fbb_port_rows", || -> Result<_, CodecError> {
            let Some(face_run) = largest_fbb_run(ctx, bytes)? else {
                return Ok(None);
            };
            let after_faces = face_run.after_faces();
            let Some((edge_rows, scopes, _, _)) = parse_fbb_edge_tables(ctx, bytes, after_faces)?
            else {
                return Ok(None);
            };
            Ok(Some((edge_rows, scopes)))
        })?;
    let Some((edge_rows, scopes)) = parsed else {
        return Ok(None);
    };
    let mut scratch = ctx.reserve_scoped(0, "catia_fbb_port_handle_ids")?;
    let mut identity_by_handle = HashMap::new();
    let mut pairs = ctx.collection_vec(edge_rows.len(), "catia_fbb_port_pairs")?;
    let mut charged_steps = edge_rows.iter().zip(scopes);
    while let Some((row, scope)) = ctx.next_charged(&mut charged_steps, "catia_fbb_port_pairs")? {
        let (Some(&first), Some(&last)) = (row.handles().first(), row.handles().last()) else {
            return Ok(None);
        };
        let mut pair = [0; 2];
        for (port, handle) in [first, last].into_iter().enumerate() {
            let Some(next) = u32::try_from(identity_by_handle.len()).ok() else {
                return Ok(None);
            };
            let key = (if global { 0 } else { scope }, handle);
            pair[port] = if let Some(&identity) =
                ctx.get_hash_map(&identity_by_handle, &key, "catia_fbb_port_handle_ids")?
            {
                identity
            } else {
                scratch.with_storage(|| {
                    ctx.insert_hash_map(
                        &mut identity_by_handle,
                        key,
                        next,
                        "catia_fbb_port_handle_ids",
                    )
                })?;
                next
            };
        }
        pairs.push(pair);
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
    const OPERATION: &str = "catia_visualization_point_marker";
    let Some(marker) = ctx.find_bytes(source, &INDEXED_VISUALIZATION_POINT_MARKER, OPERATION)?
    else {
        return Ok(None);
    };
    if ctx
        .find_bytes_from(
            source,
            &INDEXED_VISUALIZATION_POINT_MARKER,
            marker + 1,
            OPERATION,
        )?
        .is_some()
    {
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
    })() else {
        return Ok(None);
    };
    if !marker_valid || indexed_count > count {
        return Ok(None);
    }

    let mut scratch = ctx.reserve_scoped(0, "catia_visualization_point_bits")?;
    let mut point_by_bits = HashMap::new();
    let mut charged_steps = point_coordinates.iter().enumerate();
    while let Some((point, coordinates)) =
        ctx.next_charged(&mut charged_steps, "catia_visualization_point_bits")?
    {
        let key = coordinates.map(f32::to_bits);
        if scratch
            .with_storage(|| {
                ctx.insert_hash_map(
                    &mut point_by_bits,
                    key,
                    point,
                    "catia_visualization_point_bits",
                )
            })?
            .is_some()
        {
            return Ok(None);
        }
    }
    let mut terminal_handles = BTreeSet::new();
    for row in ctx.admit_iter(edge_rows, "catia_visualization_terminal_handles")? {
        for handle in [row.handles().first(), row.handles().last()]
            .into_iter()
            .flatten()
        {
            scratch.with_storage(|| {
                ctx.insert_btree_set(
                    &mut terminal_handles,
                    *handle,
                    "catia_visualization_terminal_handles",
                )
            })?;
        }
    }
    // The handles are ascending, so the last one bounds them all.
    if terminal_handles
        .last()
        .is_some_and(|handle| usize::try_from(*handle).map_or(true, |handle| handle >= count))
    {
        return Ok(None);
    }
    let point_by_handle = scratch.with_storage(|| match mode {
        0 => compressed_visualization_point_bindings(
            ctx,
            source,
            table,
            count,
            &terminal_handles,
            &point_by_bits,
        ),
        1 if count - indexed_count <= 1 => raw_visualization_point_bindings(
            ctx,
            source,
            table,
            count,
            &terminal_handles,
            &point_by_bits,
        ),
        _ => Ok(None),
    })?;
    let Some(point_by_handle) = point_by_handle else {
        return Ok(None);
    };
    let mut matched_points = HashSet::new();
    for (_, &point) in ctx.admit_iter(&point_by_handle, "catia_visualization_matched_points")? {
        scratch.with_storage(|| {
            ctx.insert_hash_set(
                &mut matched_points,
                point,
                "catia_visualization_matched_points",
            )
        })?;
    }
    if matched_points.len() != point_coordinates.len() {
        return Ok(None);
    }

    let mut pairs = ctx.collection_vec(edge_rows.len(), "catia_visualization_endpoint_pairs")?;
    let mut charged_steps = edge_rows.iter();
    while let Some(row) =
        ctx.next_charged(&mut charged_steps, "catia_visualization_endpoint_pairs")?
    {
        let mut pair = [0; 2];
        for (slot, handle) in [row.handles().first(), row.handles().last()]
            .into_iter()
            .enumerate()
        {
            let Some(&point) = handle
                .map(|handle| {
                    ctx.get_btree_map(
                        &point_by_handle,
                        handle,
                        "catia_visualization_endpoint_pairs",
                    )
                })
                .transpose()?
                .flatten()
            else {
                return Ok(None);
            };
            pair[slot] = point;
        }
        pairs.push(pair);
    }
    Ok(Some(pairs))
}

fn raw_visualization_point_bindings(
    ctx: &DecodeContext<'_>,
    source: &[u8],
    table: usize,
    count: usize,
    terminal_handles: &BTreeSet<u32>,
    point_by_bits: &HashMap<[u32; 3], usize>,
) -> Result<Option<BTreeMap<u32, usize>>, CodecError> {
    let Some(extent) = count
        .checked_mul(RAW_VISUALIZATION_POINT_STRIDE)
        .and_then(|bytes| bytes.checked_add(table))
    else {
        return Ok(None);
    };
    if source.get(table..extent).is_none() {
        return Ok(None);
    }
    let mut bindings = BTreeMap::new();
    let mut charged_steps = terminal_handles.iter();
    while let Some(handle) =
        ctx.next_charged(&mut charged_steps, "catia_raw_visualization_bindings")?
    {
        let Some(key) = (|| {
            let index = usize::try_from(*handle).ok()?;
            let at = table.checked_add(index.checked_mul(RAW_VISUALIZATION_POINT_STRIDE)?)?;
            Some([
                View::f32_le_at(source, at)?.to_bits(),
                View::f32_le_at(source, at.checked_add(4)?)?.to_bits(),
                View::f32_le_at(source, at.checked_add(8)?)?.to_bits(),
            ])
        })() else {
            return Ok(None);
        };
        let Some(&point) =
            ctx.get_hash_map(point_by_bits, &key, "catia_raw_visualization_bindings")?
        else {
            return Ok(None);
        };
        ctx.insert_btree_map(
            &mut bindings,
            *handle,
            point,
            "catia_raw_visualization_bindings",
        )?;
    }
    Ok(Some(bindings))
}

fn compressed_visualization_point_bindings(
    ctx: &DecodeContext<'_>,
    source: &[u8],
    controls: usize,
    count: usize,
    terminal_handles: &BTreeSet<u32>,
    point_by_bits: &HashMap<[u32; 3], usize>,
) -> Result<Option<BTreeMap<u32, usize>>, CodecError> {
    let Some((scalar_count, scalars)) = (|| {
        let packed_len = count.checked_add(3)? / 4;
        let delimiter = controls.checked_add(packed_len)?;
        if *source.get(delimiter)? != 0xff {
            return None;
        }
        let scalar_count_at = delimiter.checked_add(1)?;
        let scalar_count = usize::try_from(View::u32_le_at(source, scalar_count_at)?).ok()?;
        let scalars = scalar_count_at.checked_add(4)?;
        let scalar_extent = scalar_count.checked_mul(4)?.checked_add(scalars)?;
        source.get(controls..scalar_extent)?;
        Some((scalar_count, scalars))
    })() else {
        return Ok(None);
    };

    let mut previous = None::<[u32; 3]>;
    let mut scalar = 0usize;
    let mut bindings = BTreeMap::new();
    let mut charged_steps = 0..count;
    while let Some(index) =
        ctx.next_charged(&mut charged_steps, "catia_compressed_visualization_points")?
    {
        let Some(packed) = controls
            .checked_add(index / 4)
            .and_then(|at| source.get(at))
            .copied()
        else {
            return Ok(None);
        };
        let code = (packed >> (2 * (index % 4))) & 3;
        let mut read_scalar = || {
            if scalar >= scalar_count {
                return None;
            }
            let at = scalars.checked_add(scalar.checked_mul(4)?)?;
            scalar = scalar.checked_add(1)?;
            Some(View::f32_le_at(source, at)?.to_bits())
        };
        let Some(point) = (|| {
            Some(match (code, previous) {
                (0, _) => [read_scalar()?, read_scalar()?, read_scalar()?],
                (1, Some(previous)) => previous,
                (2, Some(previous)) => [previous[0], previous[1], read_scalar()?],
                (3, Some(previous)) => [previous[0], read_scalar()?, read_scalar()?],
                _ => return None,
            })
        })() else {
            return Ok(None);
        };
        previous = Some(point);
        let Some(handle) = u32::try_from(index).ok() else {
            return Ok(None);
        };
        if ctx.contains_btree_set(
            terminal_handles,
            &handle,
            "catia_compressed_visualization_bindings",
        )? {
            let Some(&point) = ctx.get_hash_map(
                point_by_bits,
                &point,
                "catia_compressed_visualization_bindings",
            )?
            else {
                return Ok(None);
            };
            ctx.insert_btree_map(
                &mut bindings,
                handle,
                point,
                "catia_compressed_visualization_bindings",
            )?;
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
    const OPERATION: &str = "catia_deferred_ports";
    if edge_ports.len() != deferred_edges.len() {
        return Ok(false);
    }
    let mut scratch = ctx.reserve_scoped(0, OPERATION)?;
    // Breadth-first search over ports: a deferred row defers every row that
    // shares one of its ports.
    let mut edges_by_port = HashMap::<u32, Vec<usize>>::new();
    let mut queue = Vec::new();
    for (edge, ports) in ctx.admit_iter(edge_ports, OPERATION)?.enumerate() {
        for &port in unique_ports(ports) {
            scratch.with_storage(|| {
                ctx.push_hash_group(&mut edges_by_port, port, edge, OPERATION, OPERATION)
            })?;
        }
        if deferred_edges[edge] {
            scratch.with_storage(|| ctx.push_vec(&mut queue, edge, OPERATION))?;
        }
    }
    let mut seen_ports = HashSet::new();
    while let Some(edge) = queue.pop() {
        ctx.charge_work(1, "catia_missing_edge_iteration")?;
        for &port in unique_ports(&edge_ports[edge]) {
            if !scratch.with_storage(|| ctx.insert_hash_set(&mut seen_ports, port, OPERATION))? {
                continue;
            }
            let Some(neighbors) = ctx.get_hash_map(&edges_by_port, &port, OPERATION)? else {
                continue;
            };
            for &neighbor in ctx.admit_iter(neighbors, OPERATION)? {
                if !deferred_edges[neighbor] {
                    deferred_edges[neighbor] = true;
                    scratch.with_storage(|| ctx.push_vec(&mut queue, neighbor, OPERATION))?;
                }
            }
        }
    }
    Ok(true)
}

/// The distinct ports of an edge.
fn unique_ports(ports: &[u32; 2]) -> &[u32] {
    if ports[0] == ports[1] {
        &ports[..1]
    } else {
        &ports[..]
    }
}

/// Collapse physical edge endpoints through every exact trim-mesh occurrence.
/// The returned component identifiers are compact and stable within this
/// result; they are not coordinate-row indices.
pub(crate) fn standard_mesh_edge_ports(
    ctx: &DecodeContext<'_>,
    bytes: &[u8],
) -> Result<Option<Vec<[u32; 2]>>, CodecError> {
    let (analysis, _analysis_storage) = ctx
        .with_scoped_storage("catia_mesh_port_analysis", || {
            standard_mesh_analysis(ctx, bytes)
        })?;
    let Some(analysis) = analysis else {
        return Ok(None);
    };
    let (local_ports, _local_port_storage) = ctx
        .with_scoped_storage("catia_mesh_local_ports", || {
            global_edge_port_identities(ctx, bytes)
        })?;
    let Some(local_ports) = local_ports else {
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
    let mut scratch = ctx.reserve_scoped(0, "catia_mesh_edge_port_identities")?;
    let mut node_by_identity = HashMap::new();
    for (edge, ports) in ctx
        .admit_iter(local_ports, "catia_mesh_edge_port_identities")?
        .enumerate()
    {
        for (side, identity) in ports.iter().copied().enumerate() {
            let node = edge * 2 + side;
            if let Some(&previous) = ctx.get_hash_map(
                &node_by_identity,
                &identity,
                "catia_mesh_edge_port_identities",
            )? {
                union.union(ctx, previous, node)?;
            } else {
                scratch.with_storage(|| {
                    ctx.insert_hash_map(
                        &mut node_by_identity,
                        identity,
                        node,
                        "catia_mesh_edge_port_identities",
                    )
                })?;
            }
        }
    }
    let mut corners = HashMap::new();
    for (edge, row) in ctx
        .admit_iter(edge_rows, "catia_mesh_edge_port_corners")?
        .enumerate()
    {
        let Some(_) = row.boundary_pattern() else {
            continue;
        };
        for run in ctx.admit_iter(&occurrences[edge], "catia_mesh_edge_port_corners")? {
            let cycle = &cycles[run.face][run.cycle];
            let mut nodes = [0; 2];
            for (slot, position) in [run.start, run.end(cycle.len())].into_iter().enumerate() {
                let key = (run.face, run.cycle, position);
                nodes[slot] = if let Some(&node) =
                    ctx.get_hash_map(&corners, &key, "catia_mesh_edge_port_corners")?
                {
                    node
                } else {
                    let node = union.push_charged(ctx, "catia_mesh_edge_port_corner_nodes")?;
                    scratch.with_storage(|| {
                        ctx.insert_hash_map(&mut corners, key, node, "catia_mesh_edge_port_corners")
                    })?;
                    node
                };
            }
            let [before_node, after_node] = nodes;
            if run.reversed {
                union.union(ctx, edge * 2 + 1, before_node)?;
                union.union(ctx, edge * 2, after_node)?;
            } else {
                union.union(ctx, edge * 2, before_node)?;
                union.union(ctx, edge * 2 + 1, after_node)?;
            }
        }
    }
    // Roots are forest nodes, so a keyed array numbers them in first-use order.
    let mut ordinal_of_root = scratch
        .with_storage(|| ctx.alloc_filled(union.len(), None, "catia_mesh_edge_port_roots"))?;
    let mut next_ordinal = 0usize;
    let mut ports = ctx.collection_vec(edge_rows.len(), "catia_mesh_edge_ports")?;
    let mut charged_steps = 0..edge_rows.len();
    while let Some(edge) = ctx.next_charged(&mut charged_steps, "catia_mesh_edge_ports")? {
        let mut pair = [0u32; 2];
        for (side, node) in [edge * 2, edge * 2 + 1].into_iter().enumerate() {
            let root = union.find(ctx, node)?;
            let ordinal = *ordinal_of_root[root].get_or_insert_with(|| {
                next_ordinal += 1;
                next_ordinal - 1
            });
            let Ok(value) = u32::try_from(ordinal) else {
                return Ok(None);
            };
            pair[side] = value;
        }
        ports.push(pair);
    }
    Ok(Some(ports))
}

#[cfg(test)]
#[test]
fn mesh_edge_ports_refuse_each_graph_collection_limit() {
    use cadmpeg_core::decode::{DecodeArena, DecodePolicy, ResourceDimension};

    let analysis = StandardMeshAnalysis {
        edge_rows: vec![{
            assert!(EdgeRow::new(
                0,
                vec![0],
                crate::families::standard::topology::EdgeBoundaryLayout::CompleteBoundaryRun
            )
            .is_none());
            EdgeRow::new(
                1,
                vec![0, 0],
                crate::families::standard::topology::EdgeBoundaryLayout::CompleteBoundaryRun,
            )
            .expect("admitted edge row")
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
    /// BoundaryDraft-cycle ordinal within the face.
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
    const OPERATION: &str = "catia_mesh_occurrence_matches";
    let mut scratch = ctx.reserve_scoped(0, "catia_mesh_occurrence_handles")?;
    let mut locations = HashMap::<u32, Vec<(usize, usize, usize)>>::new();
    for (face, face_cycles) in ctx
        .admit_iter(cycles, "catia_mesh_occurrence_locations")?
        .enumerate()
    {
        for (cycle, handles) in ctx
            .admit_iter(face_cycles, "catia_mesh_occurrence_locations")?
            .enumerate()
        {
            for (position, &handle) in ctx
                .admit_iter(handles, "catia_mesh_occurrence_locations")?
                .enumerate()
            {
                scratch.with_storage(|| {
                    ctx.push_hash_group(
                        &mut locations,
                        handle,
                        (face, cycle, position),
                        "catia_mesh_occurrence_handles",
                        "catia_mesh_occurrence_locations",
                    )
                })?;
            }
        }
    }
    let mut rows = ctx.collection_vec(edge_rows.len(), "catia_mesh_occurrence_rows")?;
    let mut charged_steps = edge_rows.iter().enumerate();
    while let Some((edge, row)) =
        ctx.next_charged(&mut charged_steps, "catia_mesh_occurrence_rows")?
    {
        let Some(pattern) = row.boundary_pattern() else {
            rows.push(Vec::new());
            continue;
        };
        let (Some(&first), Some(&last)) = (pattern.first(), pattern.last()) else {
            return Ok(None);
        };
        // A forward match at a start position wins over a reversed one.
        let (matches, _matches_storage) = ctx.with_scoped_storage(
            OPERATION,
            || -> Result<BTreeMap<(usize, usize, usize), bool>, CodecError> {
                let mut matches = BTreeMap::new();
                for (anchor, reversed) in [(first, false), (last, true)] {
                    let Some(anchored) = ctx.get_hash_map(&locations, &anchor, OPERATION)? else {
                        continue;
                    };
                    for &(face, cycle, start) in ctx.admit_iter(anchored, OPERATION)? {
                        let handles = &cycles[face][cycle];
                        let at = |offset: usize, handle: &u32| {
                            Ok(handles[(start + offset) % handles.len()] == *handle)
                        };
                        let matched = if reversed {
                            ctx.all_by(
                                pattern.iter().rev().enumerate(),
                                |(offset, handle)| at(offset, handle),
                                OPERATION,
                            )?
                        } else {
                            ctx.all_by(
                                pattern.iter().enumerate(),
                                |(offset, handle)| at(offset, handle),
                                OPERATION,
                            )?
                        };
                        if matched
                            && !(reversed
                                && ctx.contains_key_btree_map(
                                    &matches,
                                    &(face, cycle, start),
                                    OPERATION,
                                )?)
                        {
                            ctx.insert_btree_map(
                                &mut matches,
                                (face, cycle, start),
                                reversed,
                                OPERATION,
                            )?;
                        }
                    }
                }
                Ok(matches)
            },
        )?;
        // Matches are in (face, cycle, start) order, so a cycle holding two
        // occurrences shows as two neighbouring keys.
        let mut previous_cycle = None;
        let mut occurrences = Vec::new();
        let mut charged_steps = matches.iter();
        while let Some((&(face, cycle, start), &reversed)) =
            ctx.next_charged(&mut charged_steps, "catia_mesh_occurrence_runs")?
        {
            if previous_cycle.replace((face, cycle)) == Some((face, cycle)) {
                return Ok(None);
            }
            let cycle_len = cycles[face][cycle].len();
            let Some((start, segment_count)) = row.boundary_span(start, cycle_len) else {
                return Ok(None);
            };
            ctx.push_vec(
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
        rows.push(occurrences);
    }
    Ok(Some(rows))
}

#[cfg(test)]
#[test]
fn mesh_edge_occurrences_refuse_nested_collection_limits() {
    use cadmpeg_core::decode::{DecodeArena, DecodePolicy, ResourceDimension};

    let rows = [{
        assert!(EdgeRow::new(
            0,
            vec![0],
            crate::families::standard::topology::EdgeBoundaryLayout::CompleteBoundaryRun
        )
        .is_none());
        EdgeRow::new(
            1,
            vec![0, 1, 0],
            crate::families::standard::topology::EdgeBoundaryLayout::CompleteBoundaryRun,
        )
        .expect("admitted edge row")
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
    let (trims, _trim_storage) = ctx.with_scoped_storage("catia_mesh_analysis_trims", || {
        parse_trim_chain(ctx, bytes, face_start, face_count, handle_width)
    })?;
    let Some(trims) = trims else {
        return Ok(None);
    };
    let mut cycles = ctx.collection_vec(trims.len(), "catia_mesh_analysis_cycles")?;
    let mut charged_steps = trims.iter();
    while let Some(trim) = ctx.next_charged(&mut charged_steps, "catia_mesh_analysis_cycles")? {
        let (triangles, _triangle_storage) = ctx
            .with_scoped_storage("catia_mesh_analysis_triangles", || {
                trim.packet.triangles(ctx)
            })?;
        let Some(face) = boundary_cycles(ctx, triangles)? else {
            return Ok(None);
        };
        cycles.push(face);
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
    let (analysis, _analysis_storage) = ctx
        .with_scoped_storage("catia_mesh_run_analysis", || {
            standard_mesh_analysis(ctx, bytes)
        })?;
    analysis
        .map(|analysis| mesh_edge_runs(ctx, &analysis))
        .transpose()
}

fn mesh_edge_runs(
    ctx: &DecodeContext<'_>,
    analysis: &StandardMeshAnalysis,
) -> Result<Vec<MeshEdgeRun>, CodecError> {
    let mut runs = Vec::new();
    for occurrences in ctx.admit_iter(&analysis.occurrences, "catia_mesh_edge_run_rows")? {
        ctx.extend_vec(&mut runs, occurrences, "catia_mesh_edge_run_rows")?;
    }
    ctx.stable_sort_by_key(
        &mut runs,
        |value| (value.face, value.cycle, value.start, value.edge),
        Ord::cmp,
        "catia_mesh_edge_run_sort",
    )?;
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
    let (runs, _runs_storage) = ctx.with_scoped_storage("catia_edge_face_run_workspace", || {
        standard_mesh_edge_runs(ctx, bytes)
    })?;
    let Some(runs) = runs else {
        return Ok(Some(
            ctx.copy_slice(serialized, "catia standard serialized edge faces")?,
        ));
    };
    resolve_edge_faces_from_runs(ctx, serialized, &runs)
}

/// Second-face candidates for repeated rows from each face packet's trim
/// handles. A short row needs a face sharing all of its distinct handles; a
/// long row needs the one face sharing at least three and at least half.
fn repeated_edge_face_handle_candidates_from_sets<H: AsRef<[u32]>>(
    ctx: &DecodeContext<'_>,
    edge_rows: &[EdgeRow],
    face_handles: &[H],
    serialized: &[[usize; 2]],
) -> Result<Option<Vec<Vec<usize>>>, CodecError> {
    const OPERATION: &str = "catia repeated edge face handles";
    if edge_rows.len() != serialized.len()
        || ctx.any_by(
            serialized,
            |faces| Ok(faces[0] >= face_handles.len() || faces[1] >= face_handles.len()),
            OPERATION,
        )?
    {
        return Ok(None);
    }
    let mut scratch = ctx.reserve_scoped(0, OPERATION)?;
    // Each handle lists the faces whose packets carry it, ascending.
    let mut faces_by_handle = HashMap::<u32, Vec<usize>>::new();
    for (face, handles) in ctx.admit_iter(face_handles, OPERATION)?.enumerate() {
        for &handle in ctx.admit_iter(handles.as_ref(), OPERATION)? {
            let listed = ctx
                .get_hash_map(&faces_by_handle, &handle, OPERATION)?
                .is_some_and(|faces| faces.last() == Some(&face));
            if !listed {
                scratch.with_storage(|| {
                    ctx.push_hash_group(
                        &mut faces_by_handle,
                        handle,
                        face,
                        OPERATION,
                        "catia repeated edge face handle set",
                    )
                })?;
            }
        }
    }
    let carries = |face: usize, handle: u32| -> Result<bool, CodecError> {
        Ok(
            match ctx.get_hash_map(&faces_by_handle, &handle, OPERATION)? {
                Some(faces) => ctx.binary_search(faces, &face, OPERATION)?.is_ok(),
                None => false,
            },
        )
    };
    let mut charged_steps = edge_rows.iter().zip(serialized);
    while let Some((row, faces)) = ctx.next_charged(&mut charged_steps, OPERATION)? {
        if !ctx.all_by(
            row.handles(),
            |&handle| carries(faces[0], handle),
            OPERATION,
        )? {
            return Ok(None);
        }
    }
    let mut candidates = ctx.collect_indexed_vec(
        edge_rows.len(),
        "catia_repeated_edge_handle_face_candidates",
        |_| Ok(Vec::new()),
    )?;
    for (edge, (row, faces)) in ctx
        .admit_iter(edge_rows, OPERATION)?
        .zip(serialized)
        .enumerate()
    {
        if faces[0] != faces[1] || row.handles().len() < 2 {
            continue;
        }
        let (counts, _counts_storage) = ctx.with_scoped_storage(
            "catia repeated edge unique handles",
            || -> Result<(usize, BTreeMap<usize, usize>), CodecError> {
                let mut unique =
                    ctx.copy_slice(row.handles(), "catia repeated edge unique handles")?;
                ctx.sort_unstable_by(
                    &mut unique,
                    |value| value,
                    Ord::cmp,
                    "catia repeated edge unique handles",
                )?;
                ctx.dedup_vec(&mut unique, "catia repeated edge unique handles")?;
                let mut shared = BTreeMap::<usize, usize>::new();
                for handle in ctx.admit_iter(&unique, "catia repeated edge matching faces")? {
                    let Some(carrying) = ctx.get_hash_map(&faces_by_handle, handle, OPERATION)?
                    else {
                        continue;
                    };
                    for &face in ctx.admit_iter(carrying, "catia repeated edge matching faces")? {
                        if face != faces[0] {
                            *ctx.entry_btree_map(
                                &mut shared,
                                face,
                                "catia repeated edge matching faces",
                            )?
                            .or_insert(0) += 1;
                        }
                    }
                }
                Ok((unique.len(), shared))
            },
        )?;
        let (unique, shared) = counts;
        let mut matching = Vec::new();
        for (&face, &count) in ctx.admit_iter(&shared, "catia repeated edge matching faces")? {
            let qualifies = if unique >= 4 {
                count >= 3 && count >= unique.div_ceil(2)
            } else {
                count == unique
            };
            if qualifies {
                ctx.push_vec(&mut matching, face, "catia repeated edge matching faces")?;
            }
        }
        if unique >= 4 && matching.len() != 1 {
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
    let (parsed, _parsed_storage) = ctx.with_scoped_storage(
        "catia_repeated_face_parse_workspace",
        || -> Result<_, CodecError> {
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
                parse_fbb_edge_tables(ctx, bytes, after_faces)?
                    .map(|(rows, _, _, width)| (rows, width))
            };
            let Some((edge_rows, handle_width)) = parsed else {
                return Ok(None);
            };
            let Some(trims) = parse_trim_chain(ctx, bytes, face_start, face_count, handle_width)?
            else {
                return Ok(None);
            };
            Ok(Some((edge_rows, trims)))
        },
    )?;
    let Some((edge_rows, trims)) = parsed else {
        return Ok(None);
    };
    let (face_handles, _face_handle_storage) =
        ctx.with_scoped_storage("catia repeated edge face handles", || {
            ctx.collect_vec(
                trims.iter().map(|trim| trim.packet.handles()),
                "catia repeated edge face handles",
            )
        })?;
    repeated_edge_face_handle_candidates_from_sets(ctx, &edge_rows, &face_handles, serialized)
}

/// Refine unresolved repeated-face domains with positive trim-handle evidence.
///
/// A row already resolved to two distinct faces has consumed its repeated slot;
/// later candidate sources cannot reopen it. For an unresolved row, common
/// carrier and handle candidates are preferred, while handle evidence supplies
/// the domain when carrier geometry abstains.
pub(crate) fn refine_repeated_edge_face_candidates(
    ctx: &DecodeContext<'_>,
    edge_faces: &[[usize; 2]],
    allowed_faces: &mut [Vec<usize>],
    handle_face_candidates: &[Vec<usize>],
) -> Result<Option<()>, CodecError> {
    const OPERATION: &str = "catia_repeated_face_intersection";
    if edge_faces.len() != allowed_faces.len() || edge_faces.len() != handle_face_candidates.len() {
        return Ok(None);
    }
    for (edge, (allowed, handle_candidates)) in ctx
        .admit_iter(&mut *allowed_faces, OPERATION)?
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
        let mut intersection = Vec::new();
        for &face in ctx.admit_iter(&*allowed, OPERATION)? {
            if ctx.contains(handle_candidates, &face, OPERATION)? {
                ctx.push_vec(&mut intersection, face, OPERATION)?;
            }
        }
        *allowed = if intersection.is_empty() {
            ctx.copy_slice(handle_candidates, "catia_repeated_face_handle_copy")?
        } else {
            intersection
        };
    }
    Ok(Some(()))
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
    const DEGREES: &str = "catia missing-edge point degrees";

    struct PairDegreeUndo {
        start: (usize, Option<u8>),
        end: Option<(usize, Option<u8>)>,
    }

    fn add_pair(
        ctx: &DecodeContext<'_>,
        degrees: &mut BTreeMap<usize, u8>,
        pair: [usize; 2],
    ) -> Result<Option<PairDegreeUndo>, CodecError> {
        let loop_edge = pair[0] == pair[1];
        let start_previous = ctx.get_btree_map(degrees, &pair[0], DEGREES)?.copied();
        let Some(start_degree) = start_previous
            .unwrap_or_default()
            .checked_add(1 + u8::from(loop_edge))
        else {
            return Ok(None);
        };
        let end_previous = if loop_edge {
            None
        } else {
            Some(ctx.get_btree_map(degrees, &pair[1], DEGREES)?.copied())
        };
        let end_degree = match end_previous {
            None => Some(0),
            Some(previous) => previous.unwrap_or_default().checked_add(1),
        };
        let Some(end_degree) = end_degree.filter(|degree| *degree <= 2) else {
            return Ok(None);
        };
        if start_degree > 2 {
            return Ok(None);
        }
        ctx.insert_btree_map(degrees, pair[0], start_degree, DEGREES)?;
        if !loop_edge {
            ctx.insert_btree_map(degrees, pair[1], end_degree, DEGREES)?;
        }
        Ok(Some(PairDegreeUndo {
            start: (pair[0], start_previous),
            end: end_previous.map(|previous| (pair[1], previous)),
        }))
    }

    fn remove_pair(
        ctx: &DecodeContext<'_>,
        degrees: &mut BTreeMap<usize, u8>,
        undo: &PairDegreeUndo,
    ) -> Result<(), CodecError> {
        for (point, previous) in [Some(undo.start), undo.end].into_iter().flatten() {
            match previous {
                Some(degree) => {
                    if let Some(stored) = ctx.get_mut_btree_map(degrees, &point, DEGREES)? {
                        *stored = degree;
                    }
                }
                None => {
                    ctx.remove_btree_map(degrees, &point, DEGREES)?;
                }
            }
        }
        Ok(())
    }

    /// Whether every used point of every face has degree two.
    fn closed(
        ctx: &DecodeContext<'_>,
        degrees: &[BTreeMap<usize, u8>],
    ) -> Result<bool, CodecError> {
        ctx.all_by(
            degrees,
            |face| ctx.all_by(face, |(_, degree)| Ok(*degree == 2), DEGREES),
            DEGREES,
        )
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
            const OPERATION: &str = "catia missing-edge endpoint closure";
            let ctx = self.ctx;
            let _depth = ctx.enter_nested(OPERATION)?;
            if self.exhausted {
                return Ok(());
            }
            let Some(unassigned_branch) =
                ctx.position_by(used.iter(), |used| Ok(!*used), OPERATION)?
            else {
                if closed(ctx, degrees)? {
                    if self.solutions.len() == MAX_SOLUTIONS {
                        self.exhausted = true;
                    } else {
                        let solution =
                            ctx.copy_slice(assignment, "catia missing-edge solution assignment")?;
                        ctx.push_vec(
                            &mut self.solutions,
                            solution,
                            "catia missing-edge solution list",
                        )?;
                    }
                }
                return Ok(());
            };
            // A degree-one point must be completed first; otherwise branch on
            // the first unassigned row.
            let deficit = ctx.find_map(
                degrees.iter().enumerate(),
                |(face, points)| {
                    ctx.find_map(
                        points,
                        |(&point, &degree)| Ok((degree == 1).then_some((face, point))),
                        OPERATION,
                    )
                },
                OPERATION,
            )?;
            let (choices, _choices_storage) = ctx.with_scoped_storage(
                "catia missing-edge search choices",
                || -> Result<Vec<(usize, usize)>, CodecError> {
                    let mut choices = Vec::new();
                    if let Some((face, point)) = deficit {
                        for (branch, (edge, faces)) in
                            ctx.admit_iter(self.branches, OPERATION)?.enumerate()
                        {
                            if used[branch] || !self.endpoint_pairs[*edge].contains(&point) {
                                continue;
                            }
                            for &candidate in ctx.admit_iter(faces, OPERATION)? {
                                if candidate == face {
                                    ctx.push_vec(
                                        &mut choices,
                                        (branch, candidate),
                                        "catia missing-edge search choices",
                                    )?;
                                }
                            }
                        }
                    } else {
                        choices = ctx.collect_vec(
                            self.branches[unassigned_branch]
                                .1
                                .iter()
                                .map(|&face| (unassigned_branch, face)),
                            "catia missing-edge search choices",
                        )?;
                    }
                    Ok(choices)
                },
            )?;
            for (branch, face) in choices {
                ctx.charge_work(1, OPERATION)?;
                if self.states >= MAX_STATES {
                    self.exhausted = true;
                    return Ok(());
                }
                self.states += 1;
                let (edge, _) = &self.branches[branch];
                let owner = self.owners[branch];
                let undo = if face != owner {
                    let Some(undo) = add_pair(ctx, &mut degrees[face], self.endpoint_pairs[*edge])?
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
                    remove_pair(ctx, &mut degrees[face], &undo)?;
                }
                if self.exhausted {
                    return Ok(());
                }
            }
            Ok(())
        }
    }

    const VALIDATION: &str = "catia missing-edge face validation";
    let face_out_of_range = |face: &usize| Ok(*face >= face_count);
    if edge_faces.len() != allowed_faces.len()
        || edge_faces.len() != endpoint_pairs.len()
        || ctx.any_by(
            edge_faces,
            |faces| Ok(faces[0] >= face_count || faces[1] >= face_count),
            VALIDATION,
        )?
        || ctx.any_by(
            allowed_faces,
            |faces| ctx.any_by(faces, face_out_of_range, VALIDATION),
            VALIDATION,
        )?
    {
        return Ok(None);
    }
    let mut scratch = ctx.reserve_scoped(0, "catia missing-edge face degrees")?;
    let search_result = scratch.with_storage(
        || -> Result<Option<(Vec<(usize, Vec<usize>)>, Vec<Vec<usize>>)>, CodecError> {
            let mut degrees =
                ctx.collect_indexed_vec(face_count, "catia missing-edge face degrees", |_| {
                    Ok(BTreeMap::<usize, u8>::new())
                })?;
            let mut charged_steps = edge_faces.iter().enumerate();
            while let Some((edge, faces)) =
                ctx.next_charged(&mut charged_steps, "catia missing-edge face degrees")?
            {
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
            for (edge, faces) in ctx
                .admit_iter(edge_faces, "catia missing-edge branches")?
                .enumerate()
            {
                if faces[0] != faces[1] || allowed_faces[edge].is_empty() {
                    continue;
                }
                let capacity = allowed_faces[edge].len().checked_add(1).ok_or_else(|| {
                    ctx.refuse_codec_limit("catia missing-edge branch choices", u64::MAX, u64::MAX)
                })?;
                let mut choices =
                    ctx.collection_vec(capacity, "catia missing-edge branch choices")?;
                choices.push(faces[0]);
                for &face in
                    ctx.admit_iter(&allowed_faces[edge], "catia missing-edge branch choices")?
                {
                    if face != faces[0] {
                        choices.push(face);
                    }
                }
                ctx.sort_unstable_by(
                    &mut choices,
                    |value| value,
                    Ord::cmp,
                    "catia missing edge duplicate face choices sort",
                )?;
                ctx.dedup_vec(&mut choices, "catia missing-edge branch choices")?;
                ctx.push_vec(
                    &mut branches,
                    (edge, choices),
                    "catia missing-edge branches",
                )?;
            }
            if branches.is_empty() {
                // Without branches the serialized faces are the one candidate.
                let mut solutions = Vec::new();
                if closed(ctx, &degrees)? {
                    ctx.push_vec(
                        &mut solutions,
                        Vec::new(),
                        "catia missing-edge closed solutions",
                    )?;
                }
                return Ok(Some((branches, solutions)));
            }
            let mut owners =
                ctx.collection_vec(branches.len(), "catia missing-edge branch owners")?;
            for (edge, _) in ctx.admit_iter(&branches, "catia missing-edge branch owners")? {
                owners.push(edge_faces[*edge][0]);
            }
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
            let mut used =
                ctx.alloc_filled(branches.len(), false, "catia missing-edge used branches")?;
            search.visit(&mut degrees, &mut assignment, &mut used)?;
            if search.exhausted {
                return Ok(None);
            }
            let solutions = search.solutions;
            Ok(Some((branches, solutions)))
        },
    )?;
    let Some((branches, solutions)) = search_result else {
        return Ok(None);
    };
    let mut completed_solutions =
        ctx.collection_vec(solutions.len(), "catia missing-edge completed solutions")?;
    for solution in ctx.admit_iter(&solutions, "catia missing-edge completed solutions")? {
        let mut completed = ctx.copy_slice(edge_faces, "catia missing-edge completed faces")?;
        for ((edge, _), &face) in ctx
            .admit_iter(&branches, "catia missing-edge completed faces")?
            .zip(solution)
        {
            completed[*edge][1] = face;
        }
        completed_solutions.push(completed);
    }
    Ok(Some(completed_solutions))
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
        const OPERATION: &str = "catia_duplicate_face_options";
        let mut faces = ctx.collect_vec(admitted, OPERATION)?;
        ctx.push_vec(&mut faces, retained, OPERATION)?;
        ctx.sort_unstable_by(
            &mut faces,
            |value| value,
            Ord::cmp,
            "catia_duplicate_face_options_sort",
        )?;
        ctx.dedup_vec(&mut faces, OPERATION)?;
        let rest = ctx.split_off_vec(&mut faces, 1, OPERATION)?;
        Ok(Self {
            first: faces[0],
            rest,
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

/// Whether any serialized or admitted face is outside the face run.
fn faces_out_of_range(
    ctx: &DecodeContext<'_>,
    serialized: &[[usize; 2]],
    allowed_faces: &[Vec<usize>],
    face_count: usize,
) -> Result<bool, CodecError> {
    const OPERATION: &str = "catia_duplicate_face_validation";
    Ok(ctx.any_by(
        serialized,
        |faces| Ok(faces[0] >= face_count || faces[1] >= face_count),
        OPERATION,
    )? || ctx.any_by(
        allowed_faces,
        |faces| ctx.any_by(faces, |face| Ok(*face >= face_count), OPERATION),
        OPERATION,
    )?)
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
        states: &WorkBudget<'_>,
        outcome: &mut SearchOutcome<Vec<[usize; 2]>>,
        valid: &mut F,
    ) -> Result<(), CodecError>
    where
        F: FnMut(&[[usize; 2]]) -> Result<bool, CodecError>,
    {
        let _depth = ctx.enter_nested("catia_duplicate_face_search_depth")?;
        if outcome.is_closed() {
            return Ok(());
        }
        if at == branches.len() {
            if valid(assignment)? {
                match outcome {
                    SearchOutcome::Open => {
                        *outcome = SearchOutcome::Solved(
                            ctx.copy_slice(assignment, "catia_duplicate_face_solution")?,
                        );
                    }
                    SearchOutcome::Solved(previous) => {
                        if !ctx.equal(
                            previous.as_slice(),
                            &*assignment,
                            "catia_duplicate_face_solution",
                        )? {
                            *outcome = SearchOutcome::Ambiguous;
                        }
                    }
                    SearchOutcome::Ambiguous | SearchOutcome::Exhausted => {}
                }
            }
            return Ok(());
        }
        // One local unit visits one nonterminal branch frame.
        if !states.charge() {
            outcome.exhaust();
            return Ok(());
        }
        let (edge, options) = &branches[at];
        let mut faces = options.iter();
        while let Some(face) = ctx.next_charged(&mut faces, "catia_duplicate_face_search")? {
            assignment[*edge][1] = face;
            search(ctx, branches, at + 1, assignment, states, outcome, valid)?;
            if outcome.is_closed() {
                return Ok(());
            }
        }
        Ok(())
    }

    if serialized.len() != allowed_faces.len()
        || faces_out_of_range(ctx, serialized, allowed_faces, face_count)?
    {
        return Ok(None);
    }
    let mut scratch = ctx.reserve_scoped(0, "catia_duplicate_face_unresolved")?;
    let mut unresolved = Vec::new();
    for (edge, faces) in ctx
        .admit_iter(serialized, "catia_duplicate_face_unresolved")?
        .enumerate()
    {
        if faces[0] == faces[1] {
            scratch.with_storage(|| {
                ctx.push_vec(&mut unresolved, edge, "catia_duplicate_face_unresolved")
            })?;
        }
    }
    if unresolved.is_empty() {
        return Ok(Some(
            ctx.copy_slice(serialized, "catia_duplicate_face_serialized")?,
        ));
    }
    let mut assignment =
        scratch.with_storage(|| ctx.copy_slice(serialized, "catia_duplicate_face_serialized"))?;
    let mut branches = Vec::new();
    for &edge in ctx.admit_iter(&unresolved, "catia_duplicate_face_branches")? {
        let retained = assignment[edge][0];
        let options = scratch.with_storage(|| {
            FaceOptions::from_admitted(ctx, retained, allowed_faces[edge].iter().copied())
        })?;
        if options.rest.is_empty() {
            assignment[edge][1] = options.first;
        } else {
            scratch.with_storage(|| {
                ctx.push_vec(
                    &mut branches,
                    (edge, options),
                    "catia_duplicate_face_branches",
                )
            })?;
        }
    }
    ctx.sort_unstable_by_key(
        &mut branches,
        |value| {
            let (left_edge, left) = value;
            (left.count(), *left_edge)
        },
        Ord::cmp,
        "catia missing edge duplicate face branches sort",
    )?;
    let states = ctx.work_budget(cadmpeg_core::decode::u64_from_index(MAX_STATES));
    let (outcome, outcome_storage) = ctx.with_scoped_storage(
        "catia_duplicate_face_solution",
        || -> Result<_, CodecError> {
            let mut outcome = SearchOutcome::Open;
            search(
                ctx,
                &branches,
                0,
                &mut assignment,
                &states,
                &mut outcome,
                &mut valid,
            )?;
            Ok(outcome)
        },
    )?;
    match outcome {
        SearchOutcome::Solved(solution) => {
            outcome_storage.commit()?;
            Ok(Some(solution))
        }
        SearchOutcome::Open | SearchOutcome::Ambiguous | SearchOutcome::Exhausted => Ok(None),
    }
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
        if at == branches.len() {
            ctx.charge_work(1, "catia_duplicate_face_visit_work")?;
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
        let mut choices = choices.iter();
        while let Some(&face) = ctx.next_charged(&mut choices, "catia_duplicate_face_visit_work")? {
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
        || faces_out_of_range(ctx, serialized, allowed_faces, face_count)?
    {
        return Ok(None);
    }
    let mut scratch = ctx.reserve_scoped(0, "catia_duplicate_visit_branches")?;
    let mut assignment =
        scratch.with_storage(|| ctx.copy_slice(serialized, "catia_duplicate_visit_assignment"))?;
    let mut branches = Vec::<(usize, Vec<usize>)>::new();
    let mut charged_steps = serialized.iter().enumerate();
    while let Some((edge, faces)) =
        ctx.next_charged(&mut charged_steps, "catia_duplicate_visit_branches")?
    {
        let allowed = &allowed_faces[edge];
        if faces[0] != faces[1] {
            if !allowed.is_empty() {
                return Ok(None);
            }
            continue;
        }
        let choices = scratch.with_storage(|| -> Result<Vec<usize>, CodecError> {
            let capacity = allowed.len().checked_add(1).ok_or_else(|| {
                ctx.refuse_codec_limit("catia_duplicate_visit_choices", u64::MAX, u64::MAX)
            })?;
            let mut choices = ctx.collection_vec(capacity, "catia_duplicate_visit_choices")?;
            choices.push(faces[0]);
            for &face in ctx.admit_iter(allowed, "catia_duplicate_visit_choices")? {
                if face != faces[0] {
                    choices.push(face);
                }
            }
            ctx.sort_unstable_by(
                &mut choices,
                |value| value,
                Ord::cmp,
                "catia missing edge duplicate visit choices sort",
            )?;
            ctx.dedup_vec(&mut choices, "catia_duplicate_visit_choices")?;
            Ok(choices)
        })?;
        if choices.len() > 1 {
            scratch.with_storage(|| {
                ctx.push_vec(
                    &mut branches,
                    (edge, choices),
                    "catia_duplicate_visit_branches",
                )
            })?;
        }
    }
    ctx.sort_unstable_by_key(
        &mut branches,
        |value| {
            let (left_edge, left) = value;
            (left.len(), *left_edge)
        },
        Ord::cmp,
        "catia missing edge duplicate visit branches sort",
    )?;

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
        if let Some((base, _base_storage)) = context.as_ref() {
            let Some((context, _context_storage)) = base.with_edge_faces(ctx, assignment)? else {
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
    let mut scratch = ctx.reserve_scoped(0, "catia_edge_run_faces")?;
    let mut occurrence_faces = scratch.with_storage(|| {
        ctx.collect_indexed_vec(serialized.len(), "catia_edge_run_faces", |_| {
            Ok(BTreeSet::new())
        })
    })?;
    let mut charged_steps = runs.iter();
    while let Some(run) = ctx.next_charged(&mut charged_steps, "catia edge run occurrence faces")? {
        let Some(faces) = occurrence_faces.get_mut(run.edge) else {
            return Ok(None);
        };
        scratch.with_storage(|| {
            ctx.insert_btree_set(faces, run.face, "catia edge run occurrence faces")
        })?;
    }
    let mut resolved = ctx.copy_slice(serialized, "catia resolved edge faces")?;
    let mut charged_steps = resolved.iter_mut().zip(&occurrence_faces);
    while let Some((faces, occurrences)) =
        ctx.next_charged(&mut charged_steps, "catia resolved edge faces")?
    {
        if faces[0] != faces[1] || occurrences.len() < 2 {
            continue;
        }
        // Repeated row handles can match more than one face. The occurrence
        // set is then a domain, not a serialized face assignment; retain the
        // duplicate slot for native ownership and endpoint closure to resolve.
        if occurrences.len() > 2 {
            continue;
        }
        if !ctx.contains_btree_set(occurrences, &faces[0], "catia resolved edge faces")? {
            return Ok(None);
        }
        let Some(&face) = occurrences.iter().find(|face| **face != faces[0]) else {
            return Ok(None);
        };
        faces[1] = face;
    }
    Ok(Some(resolved))
}

/// One uncovered run in a trim-mesh boundary cycle.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct MeshBoundaryGap {
    /// BoundaryDraft-cycle ordinal within the face.
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

#[derive(Debug)]
pub(super) struct StandardMeshBoundaryContext {
    analysis: Arc<StandardMeshAnalysis>,
    coverage: Vec<MeshFaceCoverage>,
    edge_ports: Arc<Vec<[u32; 2]>>,
    edge_runs: Arc<Vec<MeshEdgeRun>>,
    cycle_lengths: Arc<Vec<Vec<usize>>>,
}

impl StandardMeshBoundaryContext {
    fn parse<'storage>(
        ctx: &'storage DecodeContext<'_>,
        bytes: &[u8],
        edge_faces: &[[usize; 2]],
    ) -> Result<Option<(Self, cadmpeg_core::decode::ScopedReservation<'storage>)>, CodecError> {
        Self::parse_ports(ctx, bytes, edge_faces, false)
    }

    pub(super) fn parse_ports<'storage>(
        ctx: &'storage DecodeContext<'_>,
        bytes: &[u8],
        edge_faces: &[[usize; 2]],
        global_handle_ports: bool,
    ) -> Result<Option<(Self, cadmpeg_core::decode::ScopedReservation<'storage>)>, CodecError> {
        let (context, storage) = ctx.with_scoped_storage(
            "catia_mesh_boundary_context",
            || -> Result<_, CodecError> {
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
                let (local_ports, _local_port_storage) = ctx
                    .with_scoped_storage("catia_mesh_context_local_ports", || {
                        solver_ports(ctx, bytes, global_handle_ports)
                    })?;
                let Some(local_ports) = local_ports else {
                    return Ok(None);
                };
                let Some(edge_ports) = mesh_edge_ports(ctx, &analysis, &local_ports)? else {
                    return Ok(None);
                };
                let edge_runs = mesh_edge_runs(ctx, &analysis)?;
                let cycle_lengths = mesh_cycle_lengths(ctx, &analysis.cycles)?;
                Ok(Some(Self {
                    analysis,
                    coverage,
                    edge_ports: Arc::new(edge_ports),
                    edge_runs: Arc::new(edge_runs),
                    cycle_lengths: Arc::new(cycle_lengths),
                }))
            },
        )?;
        Ok(context.map(|context| (context, storage)))
    }

    fn with_edge_faces<'storage>(
        &self,
        ctx: &'storage DecodeContext<'_>,
        edge_faces: &[[usize; 2]],
    ) -> Result<Option<(Self, cadmpeg_core::decode::ScopedReservation<'storage>)>, CodecError> {
        let (context, storage) = ctx.with_scoped_storage(
            "catia_mesh_boundary_context",
            || -> Result<_, CodecError> {
                let Some(coverage) = mesh_face_coverage(ctx, &self.analysis, edge_faces)? else {
                    return Ok(None);
                };
                Ok(Some(Self {
                    analysis: Arc::clone(&self.analysis),
                    coverage,
                    edge_ports: Arc::clone(&self.edge_ports),
                    edge_runs: Arc::clone(&self.edge_runs),
                    cycle_lengths: Arc::clone(&self.cycle_lengths),
                }))
            },
        )?;
        Ok(context.map(|context| (context, storage)))
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
    /// BoundaryDraft-cycle ordinal within the face.
    cycle: usize,
    /// First covered boundary-segment index.
    start: usize,
    /// Number of consecutive boundary segments covered by the edge.
    pub(super) segment_count: usize,
}

impl cadmpeg_core::decode::cost::DecodeCost for MeshEdgePlacementCandidate {
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
    /// BoundaryDraft-segment index at which the use begins.
    pub(crate) start: usize,
    /// BoundaryDraft-segment index immediately after the use.
    pub(crate) end: usize,
    /// Stored-row direction when an interior handle sequence fixes it.
    pub(crate) reversed: Option<bool>,
}

impl cadmpeg_core::decode::cost::DecodeCost for MeshBoundaryEdgeCandidate {
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
    let (analysis, _analysis_storage) = ctx
        .with_scoped_storage("catia_mesh_coverage_analysis", || {
            standard_mesh_analysis(ctx, bytes)
        })?;
    let Some(analysis) = analysis else {
        return Ok(None);
    };
    mesh_face_coverage(ctx, &analysis, edge_faces)
}

/// The segment count of each boundary cycle of each face.
fn mesh_cycle_lengths(
    ctx: &DecodeContext<'_>,
    cycles: &[Vec<Vec<u32>>],
) -> Result<Vec<Vec<usize>>, CodecError> {
    let mut cycle_lengths = ctx.collection_vec(cycles.len(), "catia_mesh_cycle_length_rows")?;
    for face_cycles in ctx.admit_iter(cycles, "catia_mesh_cycle_length_rows")? {
        let mut lengths = ctx.collection_vec(face_cycles.len(), "catia_mesh_cycle_lengths")?;
        for cycle in ctx.admit_iter(face_cycles, "catia_mesh_cycle_lengths")? {
            lengths.push(cycle.len());
        }
        cycle_lengths.push(lengths);
    }
    Ok(cycle_lengths)
}

fn mesh_face_coverage(
    ctx: &DecodeContext<'_>,
    analysis: &StandardMeshAnalysis,
    edge_faces: &[[usize; 2]],
) -> Result<Option<Vec<MeshFaceCoverage>>, CodecError> {
    const OPERATION: &str = "catia_mesh_cycle_coverage";
    let edge_rows = &analysis.edge_rows;
    let cycles = &analysis.cycles;
    let occurrences = &analysis.occurrences;
    if edge_rows.len() != edge_faces.len() {
        return Ok(None);
    }
    if ctx.any_by(
        occurrences.iter().enumerate(),
        |(edge, values)| {
            ctx.any_by(
                values,
                |occurrence| Ok(!edge_faces[edge].contains(&occurrence.face)),
                OPERATION,
            )
        },
        OPERATION,
    )? {
        return Ok(None);
    }
    let mut scratch = ctx.reserve_scoped(0, OPERATION)?;
    let mut occurrences_by_cycle =
        scratch.with_storage(|| ctx.collection_vec(cycles.len(), "catia_mesh_occurrence_faces"))?;
    for face_cycles in ctx.admit_iter(cycles, "catia_mesh_occurrence_faces")? {
        let rows = scratch.with_storage(|| {
            ctx.collect_indexed_vec(face_cycles.len(), "catia_mesh_cycle_occurrences", |_| {
                Ok(Vec::<MeshEdgeRun>::new())
            })
        })?;
        occurrences_by_cycle.push(rows);
    }
    let mut present_edges_by_face = scratch.with_storage(|| {
        ctx.collect_indexed_vec(cycles.len(), "catia_mesh_face_edges", |_| {
            Ok(HashSet::<usize>::new())
        })
    })?;
    let mut charged_steps = occurrences.iter();
    while let Some(values) =
        ctx.next_charged(&mut charged_steps, "catia_mesh_cycle_occurrence_entries")?
    {
        let mut charged_steps = values.iter();
        while let Some(&occurrence) =
            ctx.next_charged(&mut charged_steps, "catia_mesh_cycle_occurrence_entries")?
        {
            let Some(face_cycles) = occurrences_by_cycle.get_mut(occurrence.face) else {
                return Ok(None);
            };
            let Some(cycle_occurrences) = face_cycles.get_mut(occurrence.cycle) else {
                return Ok(None);
            };
            scratch.with_storage(|| {
                ctx.push_vec(
                    cycle_occurrences,
                    occurrence,
                    "catia_mesh_cycle_occurrence_entries",
                )?;
                ctx.insert_hash_set(
                    &mut present_edges_by_face[occurrence.face],
                    occurrence.edge,
                    "catia_mesh_present_face_edges",
                )
            })?;
        }
    }
    let mut edges_by_face = scratch.with_storage(|| {
        ctx.collect_indexed_vec(cycles.len(), "catia_mesh_edges_by_face", |_| Ok(Vec::new()))
    })?;
    let mut charged_steps = edge_faces.iter().enumerate();
    while let Some((edge, &faces)) =
        ctx.next_charged(&mut charged_steps, "catia_mesh_face_edge_entries")?
    {
        if faces[0] >= cycles.len() || faces[1] >= cycles.len() {
            return Ok(None);
        }
        for face in [Some(faces[0]), (faces[1] != faces[0]).then_some(faces[1])]
            .into_iter()
            .flatten()
        {
            scratch.with_storage(|| {
                ctx.push_vec(
                    &mut edges_by_face[face],
                    edge,
                    "catia_mesh_face_edge_entries",
                )
            })?;
        }
    }
    let mut coverage = ctx.collection_vec(cycles.len(), "catia_mesh_coverage_faces")?;
    let mut charged_steps = cycles.iter().enumerate();
    while let Some((face, face_cycles)) =
        ctx.next_charged(&mut charged_steps, "catia_mesh_coverage_faces")?
    {
        let mut gaps = Vec::new();
        let mut charged_steps = face_cycles.iter().enumerate();
        while let Some((cycle_index, cycle)) = ctx.next_charged(&mut charged_steps, OPERATION)? {
            let (covered, _covered_storage) = ctx.with_scoped_storage(OPERATION, || {
                let mut covered = ctx.alloc_filled(cycle.len(), false, OPERATION)?;
                let mut charged_steps = occurrences_by_cycle[face][cycle_index].iter();
                while let Some(occurrence) = ctx.next_charged(&mut charged_steps, OPERATION)? {
                    let mut charged_steps = 0..occurrence.segment_count;
                    while let Some(offset) = ctx.next_charged(&mut charged_steps, OPERATION)? {
                        let slot = &mut covered[(occurrence.start + offset) % cycle.len()];
                        if *slot {
                            return Ok(None);
                        }
                        *slot = true;
                    }
                }
                Ok::<_, CodecError>(Some(covered))
            })?;
            let Some(covered) = covered else {
                return Ok(None);
            };
            if ctx.all_by(&covered, |value| Ok(!*value), OPERATION)? {
                ctx.push_vec(
                    &mut gaps,
                    MeshBoundaryGap {
                        cycle: cycle_index,
                        start: 0,
                        length: cycle.len(),
                    },
                    "catia_mesh_coverage_gaps",
                )?;
                continue;
            }
            // A gap starts at an uncovered segment after a covered one; each
            // uncovered segment belongs to one gap, so the walks share one pass.
            let len = covered.len();
            let mut charged_steps = 0..len;
            while let Some(start) = ctx.next_charged(&mut charged_steps, OPERATION)? {
                if covered[start] || !covered[(start + len - 1) % len] {
                    continue;
                }
                let Some(length) = ctx.position_by(
                    0..len,
                    |offset| Ok(covered[(start + offset) % len]),
                    OPERATION,
                )?
                else {
                    return Ok(None);
                };
                ctx.push_vec(
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
        let mut missing_edges = Vec::new();
        for &edge in ctx.admit_iter(&edges_by_face[face], "catia_mesh_coverage_missing_edges")? {
            if !ctx.contains_hash_set(
                &present_edges_by_face[face],
                &edge,
                "catia_mesh_coverage_missing_edges",
            )? {
                ctx.push_vec(
                    &mut missing_edges,
                    edge,
                    "catia_mesh_coverage_missing_edges",
                )?;
            }
        }
        coverage.push(MeshFaceCoverage {
            face,
            gaps,
            missing_edges,
        });
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
        (edge_storage, order_storage): (
            &mut cadmpeg_core::decode::ScopedReservation<'_>,
            &mut cadmpeg_core::decode::ScopedReservation<'_>,
        ),
    ) -> Result<bool, CodecError> {
        const OPERATION: &str = "catia_oriented_trail_order_work";
        let _depth = ctx.enter_nested("catia_oriented_trail_order_depth")?;
        ctx.charge_work(1, OPERATION)?;
        if orders.len() > limit {
            return Ok(false);
        }
        if index_from_u32(used.count_ones()) == trails.len() {
            order_storage.with_storage(|| {
                let order = ctx.copy_slice(edges, "catia_oriented_trail_order_copy")?;
                ctx.push_vec(orders, order, "catia_oriented_trail_orders")
            })?;
            return Ok(orders.len() <= limit);
        }
        let mut charged_steps = trails.iter().enumerate();
        while let Some((index, trail)) = ctx.next_charged(&mut charged_steps, OPERATION)? {
            if used & (1 << index) != 0 {
                continue;
            }
            for reversed in [false, true] {
                if reversed && trail.len() == 1 {
                    continue;
                }
                let before = edges.len();
                edge_storage.with_storage(|| {
                    ctx.extend_vec(edges, trail, "catia_oriented_trail_scratch")
                })?;
                if reversed {
                    ctx.reverse(&mut edges[before..], "catia_oriented_trail_scratch")?;
                }
                if !visit(
                    ctx,
                    trails,
                    limit,
                    used | (1 << index),
                    edges,
                    orders,
                    (edge_storage, order_storage),
                )? {
                    return Ok(false);
                }
                edges.truncate(before);
            }
        }
        Ok(true)
    }

    if trails.len() > index_from_u32(u64::BITS) {
        return Ok(None);
    }
    let mut edge_count = 0usize;
    for trail in ctx.admit_iter(trails, "catia_oriented_trail_scratch")? {
        edge_count = edge_count.checked_add(trail.len()).ok_or_else(|| {
            ctx.refuse_codec_limit("catia_oriented_trail_scratch", u64::MAX, u64::MAX)
        })?;
    }
    let mut scratch = ctx.reserve_scoped(0, "catia_oriented_trail_scratch")?;
    let mut edges = scratch.with_storage(|| {
        let mut edges = Vec::new();
        ctx.reserve_capacity(&mut edges, edge_count, "catia_oriented_trail_scratch")?;
        Ok::<_, CodecError>(edges)
    })?;
    let mut orders = Vec::new();
    let mut order_storage = ctx.reserve_scoped(0, "catia_oriented_trail_orders")?;
    if !visit(
        ctx,
        trails,
        limit,
        0,
        &mut edges,
        &mut orders,
        (&mut scratch, &mut order_storage),
    )? {
        return Ok(None);
    }
    order_storage.commit()?;
    Ok(Some(orders))
}

pub(crate) fn bounded_endpoint_cycle_orders(
    ctx: &DecodeContext<'_>,
    missing: &[usize],
    edge_candidates: &[Vec<[usize; 2]>],
    limit: usize,
) -> Result<Option<Vec<Vec<usize>>>, CodecError> {
    const OPERATION: &str = "catia_endpoint_cycle_order_work";

    struct Search<'a> {
        ctx: &'a DecodeContext<'a>,
        missing: &'a [usize],
        transitions: &'a BTreeMap<usize, Vec<(usize, usize)>>,
        limit: usize,
        budget: WorkBudget<'a>,
        orders: BTreeSet<Vec<usize>>,
        order_storage: cadmpeg_core::decode::ScopedReservation<'a>,
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
            // One local unit visits a state or a transition.
            if !self.budget.charge() {
                return Ok(false);
            }
            if order.len() == self.missing.len() {
                if current_point == first_point
                    && !self.ctx.contains_btree_set(
                        &self.orders,
                        &*order,
                        "catia_endpoint_cycle_orders",
                    )?
                {
                    let ctx = self.ctx;
                    self.order_storage.with_storage(|| {
                        let saved = ctx.copy_slice(order, "catia_endpoint_cycle_order_copy")?;
                        ctx.insert_btree_set(&mut self.orders, saved, "catia_endpoint_cycle_orders")
                    })?;
                }
                return Ok(self.orders.len() <= self.limit);
            }
            let Some(steps) = self.ctx.get_btree_map(
                self.transitions,
                &current_point,
                "catia_endpoint_cycle_transition_points",
            )?
            else {
                return Ok(true);
            };
            for &(rank, next_point) in steps {
                if !self.budget.charge() {
                    return Ok(false);
                }
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
        || missing.len() > index_from_u32(u64::BITS)
        || ctx.any_by(
            missing,
            |&edge| Ok(edge_candidates.get(edge).is_none_or(Vec::is_empty)),
            OPERATION,
        )?
    {
        return Ok(None);
    }
    let mut scratch = ctx.reserve_scoped(0, "catia_endpoint_cycle_missing_edges")?;
    let missing = scratch.with_storage(|| {
        let mut missing = ctx.copy_slice(missing, "catia_endpoint_cycle_missing_edges")?;
        ctx.sort_unstable_by(
            &mut missing,
            |value| value,
            Ord::cmp,
            "catia_endpoint_cycle_missing_sort",
        )?;
        Ok::<_, CodecError>(missing)
    })?;
    let first_edge = missing[0];
    let mut transitions = BTreeMap::<usize, Vec<(usize, usize)>>::new();
    for (offset, &edge) in ctx
        .admit_iter(&missing[1..], "catia_endpoint_cycle_transition_steps")?
        .enumerate()
    {
        let rank = offset + 1;
        for &[left, right] in ctx.admit_iter(
            &edge_candidates[edge],
            "catia_endpoint_cycle_transition_steps",
        )? {
            for (from, to) in [
                Some((left, right)),
                (left != right).then_some((right, left)),
            ]
            .into_iter()
            .flatten()
            {
                scratch.with_storage(|| {
                    ctx.push_btree_group(
                        &mut transitions,
                        from,
                        (rank, to),
                        "catia_endpoint_cycle_transition_points",
                        "catia_endpoint_cycle_transition_steps",
                    )
                })?;
            }
        }
    }
    for (_, values) in ctx.admit_iter(&mut transitions, "catia_endpoint_cycle_transition_sort")? {
        ctx.sort_unstable_by(
            &mut *values,
            |value| value,
            Ord::cmp,
            "catia_endpoint_cycle_transition_sort",
        )?;
        ctx.dedup_vec(values, "catia_endpoint_cycle_transition_sort")?;
    }
    let mut search = Search {
        ctx,
        missing: &missing,
        transitions: &transitions,
        limit,
        budget: match limit.checked_mul(16) {
            Some(operations) => ctx.work_budget(u64_from_index(operations)),
            None => return Ok(None),
        },
        orders: BTreeSet::new(),
        order_storage: ctx.reserve_scoped(0, "catia_endpoint_cycle_orders")?,
    };
    let first_pairs = scratch.with_storage(|| {
        let mut first_pairs = ctx.collect_vec(
            edge_candidates[first_edge]
                .iter()
                .map(|&[left, right]| [left.min(right), left.max(right)]),
            "catia_endpoint_cycle_first_pairs",
        )?;
        ctx.sort_unstable_by(
            &mut first_pairs,
            |value| value,
            Ord::cmp,
            "catia_endpoint_cycle_first_pairs_sort",
        )?;
        ctx.dedup_vec(&mut first_pairs, "catia_endpoint_cycle_first_pairs")?;
        Ok::<_, CodecError>(first_pairs)
    })?;
    let mut order = scratch
        .with_storage(|| ctx.collection_vec(missing.len(), "catia_endpoint_cycle_order_scratch"))?;
    let mut charged_steps = first_pairs.iter();
    while let Some(&[first_point, current_point]) =
        ctx.next_charged(&mut charged_steps, "catia_endpoint_cycle_first_pairs")?
    {
        order.clear();
        order.push(first_edge);
        if !search.walk(first_point, current_point, 1, &mut order)? {
            return Ok(None);
        }
    }
    if search.orders.is_empty() {
        return Ok(None);
    }
    // The orders come out of the set ascending.
    ctx.try_collect_vec(
        search
            .orders
            .iter()
            .map(|order| ctx.copy_slice(order, "catia_endpoint_cycle_result_copy")),
        "catia_endpoint_cycle_result_rows",
    )
    .map(Some)
}

/// Per-edge endpoint point domains and, for each point, the points an edge
/// can lead to from it.
struct EndpointDomains {
    points: Vec<BTreeSet<usize>>,
    transitions: Vec<BTreeMap<usize, BTreeSet<usize>>>,
}

/// The points a gap walk may currently stand on: an edge's endpoint domain,
/// a bound corner's points, or a set derived during the walk.
#[derive(Clone, Copy)]
enum PointSet<'s> {
    Edge(usize),
    Corner(MeshCorner),
    Derived(&'s BTreeSet<usize>),
}

fn point_set<'s>(
    ctx: &DecodeContext<'_>,
    endpoints: Option<&'s EndpointDomains>,
    corner_points: &'s MeshCornerPoints,
    set: PointSet<'s>,
) -> Result<&'s BTreeSet<usize>, CodecError> {
    let found = match set {
        PointSet::Edge(edge) => endpoints.and_then(|domains| domains.points.get(edge)),
        PointSet::Corner(corner) => {
            ctx.get_hash_map(corner_points, &corner, "catia_gap_corner_points")?
        }
        PointSet::Derived(points) => Some(points),
    };
    found.ok_or_else(|| CodecError::malformed("gap point set is missing"))
}

fn standard_mesh_missing_edge_assignment_domains(
    ctx: &DecodeContext<'_>,
    context: &StandardMeshBoundaryContext,
    edge_candidates: Option<&[Vec<[usize; 2]>]>,
    canonicalize_spans: bool,
    defer_validation: bool,
) -> MissingEdgeDomainsOutput {
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
        Option<&'a EndpointDomains>,
        &'a MeshCornerPoints,
    );
    type DeadState = (usize, usize, u64, Option<u32>, Vec<usize>, bool);

    struct EnumerateFaceInputs<'input0, 'input1, 'input2, 'input3, 'input4, 'input5> {
        face: usize,
        gaps: &'input0 [MeshBoundaryGap],
        cycle_lengths: &'input1 [usize],
        missing: &'input2 [usize],
        rows: &'input3 [EdgeRow],
        fixed_complete_row_spans: bool,
        constraints: PlacementConstraints<'input4>,
        canonicalize_spans: bool,
        remaining_states: &'input5 WorkBudget<'input5>,
    }

    fn enumerate_face(
        ctx: &DecodeContext<'_>,
        inputs: EnumerateFaceInputs<'_, '_, '_, '_, '_, '_>,
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
            endpoints: Option<&'a EndpointDomains>,
            corner_points: &'a MeshCornerPoints,
            canonical_spans: bool,
            canonical_gap_partitions: bool,
            dead_states: HashMap<DeadState, cadmpeg_core::decode::ScopedReservation<'a>>,
            storage: cadmpeg_core::decode::ScopedReservation<'a>,
            remaining_states: &'a WorkBudget<'a>,
            states: usize,
            assignments: usize,
            complete: Vec<Vec<MeshEdgePlacementCandidate>>,
            complete_storage: cadmpeg_core::decode::ScopedReservation<'a>,
        }
        struct GapSearchState<'input0> {
            gap: usize,
            offset: usize,
            used: u64,
            current_port: Option<u32>,
            current_points: Option<PointSet<'input0>>,
            gap_placed_start: usize,
            placed: &'input0 mut Vec<MeshEdgePlacementCandidate>,
        }
        impl<'a, 'ctx> Search<'a, 'ctx> {
            fn walk(&mut self, inputs: GapSearchState<'_>) -> Result<Option<()>, CodecError> {
                let ctx = self.ctx;
                let (points, points_storage) =
                    ctx.with_scoped_storage("catia_gap_state_points", || {
                        match inputs.current_points {
                            Some(set) => {
                                let current =
                                    point_set(ctx, self.endpoints, self.corner_points, set)?;
                                ctx.collect_vec(current.iter().copied(), "catia_gap_state_points")
                            }
                            None => Ok(Vec::new()),
                        }
                    })?;
                let has_flexible = inputs.placed.len() > inputs.gap_placed_start;
                let state = (
                    inputs.gap,
                    inputs.offset,
                    inputs.used,
                    inputs.current_port,
                    points,
                    has_flexible,
                );
                if ctx
                    .get_hash_map(&self.dead_states, &state, "catia_gap_dead_states")?
                    .is_some()
                {
                    return Ok(Some(()));
                }
                let before = self.assignments;
                let Some(()) = self.walk_state(inputs)? else {
                    return Ok(None);
                };
                if self.assignments == before {
                    let dead_states = &mut self.dead_states;
                    self.storage.with_storage(|| {
                        ctx.insert_hash_map(
                            dead_states,
                            state,
                            points_storage,
                            "catia_gap_dead_states",
                        )
                    })?;
                }
                Ok(Some(()))
            }

            fn walk_state(&mut self, inputs: GapSearchState<'_>) -> Result<Option<()>, CodecError> {
                let GapSearchState {
                    gap,
                    offset,
                    used,
                    current_port,
                    current_points,
                    gap_placed_start,
                    placed,
                } = inputs;
                let ctx = self.ctx;

                let _depth = ctx.enter_nested("catia_gap_assignment_depth")?;
                // One local unit visits one gap-search state across all retries.
                if !self.remaining_states.charge() {
                    return Ok(None);
                }
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
                    if index_from_u32(used.count_ones()) == self.missing.len() {
                        self.assignments += 1;
                        self.complete_storage.with_storage(|| {
                            let copy =
                                ctx.copy_slice(placed, "catia_gap_complete_placement_copy")?;
                            ctx.push_vec(&mut self.complete, copy, "catia_gap_complete_assignments")
                        })?;
                    }
                    return Ok(Some(()));
                }
                let target = self.gaps[gap].length;
                let can_expand_gap = self.canonical_gap_partitions
                    && offset < target
                    && placed.len() > gap_placed_start;
                if offset == target || can_expand_gap {
                    let value = self.gaps[gap];
                    let end = (value.start + value.length) % self.cycle_lengths[value.cycle];
                    let end_corner = (self.face, value.cycle, end);
                    let port_closes = match current_port {
                        Some(actual) => ctx
                            .get_hash_map(self.corner_ports, &end_corner, "catia_gap_corner_ports")?
                            .is_none_or(|expected| actual == *expected),
                        None => true,
                    };
                    let end_points = ctx.get_hash_map(
                        self.corner_points,
                        &end_corner,
                        "catia_gap_corner_points",
                    )?;
                    let points_close = match current_points.zip(end_points) {
                        Some((set, expected)) => {
                            let actual = point_set(ctx, self.endpoints, self.corner_points, set)?;
                            ctx.any_by(
                                expected,
                                |point| {
                                    ctx.contains_btree_set(
                                        actual,
                                        point,
                                        "catia_gap_corner_point_overlap",
                                    )
                                },
                                "catia_gap_corner_point_overlap",
                            )?
                        }
                        None => true,
                    };
                    if port_closes && points_close {
                        let next_corner = self
                            .gaps
                            .get(gap + 1)
                            .map(|next| (self.face, next.cycle, next.start));
                        let next_port = match next_corner {
                            Some(corner) => ctx
                                .get_hash_map(self.corner_ports, &corner, "catia_gap_corner_ports")?
                                .copied(),
                            None => None,
                        };
                        let next_points = match next_corner {
                            Some(corner) => ctx
                                .contains_key_hash_map(
                                    self.corner_points,
                                    &corner,
                                    "catia_gap_next_corner_points",
                                )?
                                .then_some(PointSet::Corner(corner)),
                            None => None,
                        };
                        let (saved, _saved_storage) =
                            ctx.with_scoped_storage("catia_gap_saved_placements", || {
                                ctx.copy_slice(
                                    &placed[gap_placed_start..],
                                    "catia_gap_saved_placements",
                                )
                            })?;
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
                            let mut charged_steps = placed[gap_placed_start..].iter_mut();
                            while let Some(placement) =
                                ctx.next_charged(&mut charged_steps, "catia_gap_saved_placements")?
                            {
                                placement.start = at % self.cycle_lengths[value.cycle];
                                let Some(next) = at.checked_add(placement.segment_count) else {
                                    return Ok(None);
                                };
                                at = next;
                            }
                        }
                        let next_start = placed.len();
                        let Some(()) = self.walk(GapSearchState {
                            gap: gap + 1,
                            offset: 0,
                            used,
                            current_port: next_port,
                            current_points: next_points,
                            gap_placed_start: next_start,
                            placed,
                        })?
                        else {
                            return Ok(None);
                        };
                        placed.truncate(gap_placed_start);
                        ctx.extend_vec(placed, &saved, "catia_gap_placed_edges")?;
                        if offset == target {
                            return Ok(Some(()));
                        }
                    } else if offset == target {
                        return Ok(Some(()));
                    }
                }
                let mut charged_steps = 0..self.missing.len();
                while let Some(rank) =
                    ctx.next_charged(&mut charged_steps, "catia_gap_assignment_work")?
                {
                    if used & (1 << rank) != 0 {
                        continue;
                    }
                    let edge = self.missing[rank];
                    let remaining = target - offset;
                    let row_span = (self.fixed_complete_row_spans
                        && self.rows[edge].boundary_layout()
                            == EdgeBoundaryLayout::CompleteBoundaryRun)
                        .then(|| self.rows[edge].handles().len().checked_sub(1))
                        .flatten();
                    // Every other unused edge still needs one segment.
                    let canonical_span = row_span.or_else(|| {
                        self.canonical_spans
                            .then(|| {
                                if self.canonical_gap_partitions {
                                    return Some(1);
                                }
                                let others =
                                    self.missing.len() - index_from_u32(used.count_ones()) - 1;
                                remaining.checked_sub(others)
                            })
                            .flatten()
                    });
                    // The edge's next ports, at most two.
                    let next_ports: [Option<Option<u32>>; 2] = match (self.edge_ports, current_port)
                    {
                        (Some(edge_ports), Some(current)) if edge_ports[edge][0] == current => {
                            [Some(Some(edge_ports[edge][1])), None]
                        }
                        (Some(edge_ports), Some(current)) if edge_ports[edge][1] == current => {
                            [Some(Some(edge_ports[edge][0])), None]
                        }
                        (Some(_), Some(_)) => continue,
                        (Some(edge_ports), None) => {
                            let [first, second] = edge_ports[edge];
                            [
                                Some(Some(first.min(second))),
                                (first != second).then_some(Some(first.max(second))),
                            ]
                        }
                        (None, _) => [Some(None), None],
                    };
                    let derived = match (self.endpoints, current_points) {
                        (Some(endpoints), Some(set)) if !endpoints.points[edge].is_empty() => {
                            Some(self.derive_points(endpoints, edge, set)?)
                        }
                        _ => None,
                    };
                    let next_points = match self.endpoints {
                        Some(endpoints) if !endpoints.points[edge].is_empty() => {
                            match derived.as_ref() {
                                Some((points, _storage)) if points.is_empty() => continue,
                                Some((points, _storage)) => Some(PointSet::Derived(points)),
                                None => Some(PointSet::Edge(edge)),
                            }
                        }
                        _ => None,
                    };
                    let first_span = canonical_span.unwrap_or(1);
                    let last_span = canonical_span.unwrap_or(remaining);
                    let mut charged_steps = first_span..=last_span;
                    while let Some(segment_count) =
                        ctx.next_charged(&mut charged_steps, "catia_gap_assignment_work")?
                    {
                        if segment_count == 0 || segment_count > remaining {
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
                        ctx.push_vec(placed, value, "catia_gap_placed_edges")?;
                        for next_port in next_ports.into_iter().flatten() {
                            let Some(()) = self.walk(GapSearchState {
                                gap,
                                offset: offset + segment_count,
                                used: used | (1 << rank),
                                current_port: next_port,
                                current_points: next_points,
                                gap_placed_start,
                                placed,
                            })?
                            else {
                                return Ok(None);
                            };
                        }
                        placed.pop();
                    }
                }
                Ok(Some(()))
            }

            /// The points an edge leads to from the current points, held by
            /// the caller while its branch walks that edge.
            fn derive_points(
                &self,
                endpoints: &EndpointDomains,
                edge: usize,
                current: PointSet<'_>,
            ) -> Result<(BTreeSet<usize>, cadmpeg_core::decode::ScopedReservation<'a>), CodecError>
            {
                const OPERATION: &str = "catia_gap_transition_points";
                let ctx = self.ctx;
                let source = point_set(ctx, self.endpoints, self.corner_points, current)?;
                let transitions = &endpoints.transitions[edge];
                ctx.with_scoped_storage(OPERATION, || {
                    let mut next = BTreeSet::new();
                    for point in ctx.admit_iter(source, OPERATION)? {
                        let Some(targets) = ctx.get_btree_map(transitions, point, OPERATION)?
                        else {
                            continue;
                        };
                        for &target in ctx.admit_iter(targets, OPERATION)? {
                            ctx.insert_btree_set(&mut next, target, OPERATION)?;
                        }
                    }
                    Ok::<_, CodecError>(next)
                })
            }
        }

        let EnumerateFaceInputs {
            face,
            gaps,
            cycle_lengths,
            missing,
            rows,
            fixed_complete_row_spans,
            constraints,
            canonicalize_spans,
            remaining_states,
        } = inputs;

        let (edge_ports, corner_ports, endpoints, corner_points) = constraints;
        if missing.len() > index_from_u32(u64::BITS) {
            return Ok(None);
        }
        let mut storage = ctx.reserve_scoped(0, "catia_gap_placed_edges")?;
        // A walk places each missing edge at most once.
        let mut placed =
            storage.with_storage(|| ctx.collection_vec(missing.len(), "catia_gap_placed_edges"))?;
        let first_corner = gaps.first().map(|gap| (face, gap.cycle, gap.start));
        let first_port = match first_corner {
            Some(corner) => ctx
                .get_hash_map(corner_ports, &corner, "catia_gap_corner_ports")?
                .copied(),
            None => None,
        };
        let first_points = match first_corner {
            Some(corner) => ctx
                .contains_key_hash_map(corner_points, &corner, "catia_gap_initial_corner_points")?
                .then_some(PointSet::Corner(corner)),
            None => None,
        };
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
            endpoints,
            corner_points,
            // Mesh span allocation does not change the ordered edge uses or
            // their endpoint quotient. Keep one allocation for each edge
            // order and gap partition in topology searches; the public
            // placement API above still enumerates every span allocation.
            canonical_spans: canonicalize_spans,
            canonical_gap_partitions: canonicalize_spans,
            dead_states: HashMap::new(),
            storage,
            remaining_states,
            states: 0,
            assignments: 0,
            complete: Vec::new(),
            complete_storage: ctx.reserve_scoped(0, "catia_gap_complete_assignments")?,
        };
        if search
            .walk(GapSearchState {
                gap: 0,
                offset: 0,
                used: 0,
                current_port: first_port,
                current_points: first_points,
                gap_placed_start: 0,
                placed: &mut placed,
            })?
            .is_none()
        {
            return Ok(None);
        }
        if search.assignments == 0 || search.assignments > MAX_ASSIGNMENTS_PER_FACE {
            return Ok(None);
        }
        search.complete_storage.commit()?;
        Ok(Some(search.complete))
    }

    /// Placements built from oriented trails of edges with single endpoint
    /// pairs; each gap takes the one trail its corner points admit.
    fn endpoint_trail_assignments(
        ctx: &DecodeContext<'_>,
        face: usize,
        (gaps, cycle_lengths): (&[MeshBoundaryGap], &[usize]),
        missing: &[usize],
        rows: &[EdgeRow],
        edge_points: &[Option<[usize; 2]>],
        corner_points: &MeshCornerPoints,
    ) -> Result<Option<Vec<Vec<MeshEdgePlacementCandidate>>>, CodecError> {
        const OPERATION: &str = "catia_trail_edges";
        struct EndpointTrail {
            edges: Vec<usize>,
            start: usize,
            end: usize,
            /// The smallest edge, which orders the trails.
            first_edge: usize,
        }

        if gaps.is_empty()
            || ctx.any_by(
                missing,
                |&edge| Ok(edge_points.get(edge).is_none_or(Option::is_none)),
                OPERATION,
            )?
        {
            return Ok(None);
        }
        let endpoints_of = |edge: usize| {
            edge_points[edge].ok_or_else(|| CodecError::malformed("trail edge has no endpoints"))
        };
        let mut scratch = ctx.reserve_scoped(0, "catia_trail_point_entries")?;
        // Each point lists the missing edges ending at it; a trail point has
        // at most two.
        let mut at_point = HashMap::<usize, Vec<usize>>::new();
        let mut charged_steps = missing.iter();
        while let Some(&edge) = ctx.next_charged(&mut charged_steps, "catia_trail_point_edges")? {
            for point in endpoints_of(edge)? {
                scratch.with_storage(|| {
                    ctx.push_hash_group(
                        &mut at_point,
                        point,
                        edge,
                        "catia_trail_point_entries",
                        "catia_trail_point_edges",
                    )
                })?;
                if ctx
                    .get_hash_map(&at_point, &point, "catia_trail_point_entries")?
                    .is_some_and(|edges| edges.len() > 2)
                {
                    return Ok(None);
                }
            }
        }
        let incident = |point: usize| -> Result<&[usize], CodecError> {
            Ok(ctx
                .get_hash_map(&at_point, &point, "catia_trail_point_entries")?
                .map_or(&[][..], Vec::as_slice))
        };
        let mut unseen = BTreeSet::new();
        for &edge in ctx.admit_iter(missing, "catia_trail_unseen_edges")? {
            scratch.with_storage(|| {
                ctx.insert_btree_set(&mut unseen, edge, "catia_trail_unseen_edges")
            })?;
        }
        let mut trails = Vec::<EndpointTrail>::new();
        while let Some(&smallest) = unseen.first() {
            ctx.charge_work(1, "catia_missing_edge_iteration")?;
            // Start at the smallest edge with an open end, else the smallest edge.
            let open_start = ctx.find_by(
                unseen.iter().copied(),
                |&edge| {
                    let pair = endpoints_of(edge)?;
                    Ok(incident(pair[0])?.len() == 1 || incident(pair[1])?.len() == 1)
                },
                OPERATION,
            )?;
            let first = open_start.unwrap_or(smallest);
            let endpoints = endpoints_of(first)?;
            let start = if incident(endpoints[0])?.len() == 1 {
                endpoints[0]
            } else if incident(endpoints[1])?.len() == 1 {
                endpoints[1]
            } else {
                endpoints[0]
            };
            let mut point = start;
            let mut edge = first;
            let mut trail = Vec::new();
            loop {
                ctx.charge_work(1, "catia_missing_edge_iteration")?;
                if !ctx.remove_btree_set(&mut unseen, &edge, "catia_trail_unseen_edges")? {
                    break;
                }
                scratch.with_storage(|| ctx.push_vec(&mut trail, edge, OPERATION))?;
                let endpoints = endpoints_of(edge)?;
                point = if endpoints[0] == point {
                    endpoints[1]
                } else if endpoints[1] == point {
                    endpoints[0]
                } else {
                    return Ok(None);
                };
                let Some(next) = ctx.find_by(
                    incident(point)?.iter().copied(),
                    |candidate| ctx.contains_btree_set(&unseen, candidate, OPERATION),
                    OPERATION,
                )?
                else {
                    break;
                };
                edge = next;
            }
            if point == start && (gaps.len() != 1 || trail.len() != missing.len()) {
                return Ok(None);
            }
            let first_edge = ctx.min(&trail, OPERATION)?.copied().unwrap_or(first);
            scratch.with_storage(|| {
                ctx.push_vec(
                    &mut trails,
                    EndpointTrail {
                        edges: trail,
                        start,
                        end: point,
                        first_edge,
                    },
                    "catia_trail_rows",
                )
            })?;
        }
        if gaps.len() > 1 {
            if trails.len() != gaps.len() {
                return Ok(None);
            }
            let mut available = trails;
            ctx.stable_sort_by_key(
                &mut available,
                |trail| trail.first_edge,
                Ord::cmp,
                "catia_trail_sort",
            )?;
            let mut placements = ctx.collection_vec(missing.len(), "catia_trail_placements")?;
            let mut charged_steps = gaps.iter();
            while let Some(gap) =
                ctx.next_charged(&mut charged_steps, "catia_trail_gap_candidates")?
            {
                let gap_end = (gap.start + gap.length) % cycle_lengths[gap.cycle];
                let start_points = ctx.get_hash_map(
                    corner_points,
                    &(face, gap.cycle, gap.start),
                    "catia_trail_gap_candidates",
                )?;
                let end_points = ctx.get_hash_map(
                    corner_points,
                    &(face, gap.cycle, gap_end),
                    "catia_trail_gap_candidates",
                )?;
                let (Some(start_points), Some(end_points)) = (start_points, end_points) else {
                    return Ok(None);
                };
                let mut candidate = None;
                let mut charged_steps = available.iter().enumerate();
                while let Some((index, trail)) =
                    ctx.next_charged(&mut charged_steps, "catia_trail_gap_candidates")?
                {
                    for reversed in [false, true] {
                        let (trail_start, trail_end) = if reversed {
                            (trail.end, trail.start)
                        } else {
                            (trail.start, trail.end)
                        };
                        if trail.edges.len() <= gap.length
                            && ctx.contains_btree_set(
                                start_points,
                                &trail_start,
                                "catia_trail_gap_candidates",
                            )?
                            && ctx.contains_btree_set(
                                end_points,
                                &trail_end,
                                "catia_trail_gap_candidates",
                            )?
                        {
                            // A gap must admit exactly one oriented trail.
                            if candidate.replace((index, reversed)).is_some() {
                                return Ok(None);
                            }
                        }
                    }
                }
                let Some((trail_index, reversed)) = candidate else {
                    return Ok(None);
                };
                let mut trail = available.swap_remove(trail_index);
                if reversed {
                    ctx.reverse(&mut trail.edges, "catia_trail_edges")?;
                }
                let slack = gap.length - trail.edges.len();
                let mut offset = 0usize;
                let mut charged_steps = trail.edges.iter().enumerate();
                while let Some((index, &edge)) =
                    ctx.next_charged(&mut charged_steps, "catia_trail_placements")?
                {
                    let Some(segment_count) = 1usize.checked_add(usize::from(index == 0) * slack)
                    else {
                        return Ok(None);
                    };
                    placements.push(MeshEdgePlacementCandidate {
                        edge,
                        face,
                        cycle: gap.cycle,
                        start: (gap.start + offset) % cycle_lengths[gap.cycle],
                        segment_count,
                    });
                    let Some(next) = offset.checked_add(segment_count) else {
                        return Ok(None);
                    };
                    offset = next;
                }
            }
            let mut rows = ctx.collection_vec(1, "catia_trail_placement_rows")?;
            rows.push(placements);
            return Ok(Some(rows));
        }
        let [gap] = gaps else {
            return Ok(None);
        };
        if cycle_lengths.len() != 1
            || gap.start != 0
            || gap.cycle != 0
            || gap.length != cycle_lengths[0]
            || gap.length != missing.len()
            || ctx.any_by(
                missing,
                |&edge| Ok(rows[edge].handles().len() != 2),
                OPERATION,
            )?
            || trails.len() > index_from_u32(u64::BITS)
        {
            return Ok(None);
        }
        let trail_edges = scratch.with_storage(|| {
            ctx.collect_vec(
                trails.into_iter().map(|trail| trail.edges),
                "catia_trail_order_input_rows",
            )
        })?;
        let Some(orders) =
            bounded_oriented_trail_orders(ctx, &trail_edges, MAX_ASSIGNMENTS_PER_FACE)?
        else {
            return Ok(None);
        };
        cycle_order_placements(ctx, face, gap.cycle, orders).map(Some)
    }

    /// One placement row per order, each edge covering one segment.
    fn cycle_order_placements(
        ctx: &DecodeContext<'_>,
        face: usize,
        cycle: usize,
        orders: Vec<Vec<usize>>,
    ) -> Result<Vec<Vec<MeshEdgePlacementCandidate>>, CodecError> {
        let mut assignments = ctx.collection_vec(orders.len(), "catia_cycle_assignment_rows")?;
        for order in ctx.admit_iter(orders, "catia_cycle_assignment_rows")? {
            assignments.push(
                ctx.collect_vec(
                    order.into_iter().enumerate().map(|(offset, edge)| {
                        MeshEdgePlacementCandidate {
                            edge,
                            face,
                            cycle,
                            start: offset,
                            segment_count: 1,
                        }
                    }),
                    "catia_cycle_assignment_placements",
                )?,
            );
        }
        Ok(assignments)
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
        let [gap] = gaps else {
            return Ok(None);
        };
        if cycle_lengths.len() != 1
            || gap.start != 0
            || gap.cycle != 0
            || gap.length != cycle_lengths[0]
            || gap.length != missing.len()
            || ctx.any_by(
                missing,
                |&edge| Ok(rows[edge].handles().len() != 2),
                "catia_cycle_assignment_rows",
            )?
        {
            return Ok(None);
        }
        let Some(orders) =
            bounded_endpoint_cycle_orders(ctx, missing, edge_candidates, MAX_ASSIGNMENTS_PER_FACE)?
        else {
            return Ok(None);
        };
        cycle_order_placements(ctx, face, 0, orders).map(Some)
    }

    let edge_rows = &context.analysis.edge_rows;
    if edge_candidates.is_some_and(|candidates| candidates.len() != edge_rows.len()) {
        return Ok(None);
    }
    let mut scratch = ctx.reserve_scoped(0, "catia_mesh_edge_point_domain_rows")?;
    let endpoint_domains = match edge_candidates {
        Some(candidates) => Some(scratch.with_storage(
            || -> Result<EndpointDomains, CodecError> {
                let mut points_by_edge =
                    ctx.collection_vec(candidates.len(), "catia_mesh_edge_point_domain_rows")?;
                let mut transitions_by_edge =
                    ctx.collection_vec(candidates.len(), "catia_mesh_edge_transition_rows")?;
                for pairs in ctx.admit_iter(candidates, "catia_mesh_edge_point_domain_rows")? {
                    let mut points = BTreeSet::new();
                    let mut transitions = BTreeMap::<usize, BTreeSet<usize>>::new();
                    for &[left, right] in
                        ctx.admit_iter(pairs, "catia_mesh_edge_point_domain_values")?
                    {
                        for point in [left, right] {
                            ctx.insert_btree_set(
                                &mut points,
                                point,
                                "catia_mesh_edge_point_domain_values",
                            )?;
                        }
                        for (from, to) in [(left, right), (right, left)] {
                            ctx.insert_btree_group_set(
                                &mut transitions,
                                from,
                                to,
                                "catia_mesh_edge_transition_points",
                                "catia_mesh_edge_transition_targets",
                            )?;
                        }
                    }
                    points_by_edge.push(points);
                    transitions_by_edge.push(transitions);
                }
                Ok(EndpointDomains {
                    points: points_by_edge,
                    transitions: transitions_by_edge,
                })
            },
        )?),
        None => None,
    };
    let coverage = &context.coverage;
    let edge_ports = context.edge_ports.as_slice();
    let complete_boundary_ports = ctx.all_by(
        edge_rows,
        |row| Ok(row.boundary_layout() == EdgeBoundaryLayout::CompleteBoundaryRun),
        "catia_mesh_corner_ports",
    )?;
    let placement_ports = complete_boundary_ports.then_some(edge_ports);
    let singleton_edge_points = match edge_candidates {
        Some(candidates) => Some(scratch.with_storage(|| {
            ctx.collect_vec(
                candidates.iter().map(|domain| match domain.as_slice() {
                    [pair] => Some(*pair),
                    _ => None,
                }),
                "catia_mesh_singleton_edge_points",
            )
        })?),
        None => None,
    };
    let edge_runs = context.edge_runs.as_slice();
    let remaining_states = ctx.work_budget(u64_from_index(MAX_SEARCH_STATES));
    let mut corner_ports = HashMap::<MeshCorner, u32>::new();
    let mut corner_points = MeshCornerPoints::new();
    let mut charged_steps = edge_runs.iter();
    while let Some(run) = ctx.next_charged(&mut charged_steps, "catia_mesh_corner_ports")? {
        let length = context.cycle_lengths[run.face][run.cycle];
        let end = (run.start + run.segment_count) % length;
        if edge_rows[run.edge].boundary_layout() == EdgeBoundaryLayout::CompleteBoundaryRun {
            let ports = edge_ports[run.edge];
            let oriented = if run.reversed {
                [ports[1], ports[0]]
            } else {
                ports
            };
            for (corner, port) in [(run.start, oriented[0]), (end, oriented[1])] {
                let stored = scratch.with_storage(|| {
                    ctx.insert_hash_map(
                        &mut corner_ports,
                        (run.face, run.cycle, corner),
                        port,
                        "catia_mesh_corner_ports",
                    )
                })?;
                if stored.is_some_and(|stored| stored != port) {
                    return Ok(None);
                }
            }
        }
        if let Some(candidates) = edge_candidates {
            let (points, _points_storage) =
                ctx.with_scoped_storage("catia_mesh_corner_candidate_points", || {
                    let mut points = BTreeSet::new();
                    for pair in
                        ctx.admit_iter(&candidates[run.edge], "catia_mesh_corner_candidate_points")?
                    {
                        for &point in pair {
                            ctx.insert_btree_set(
                                &mut points,
                                point,
                                "catia_mesh_corner_candidate_points",
                            )?;
                        }
                    }
                    Ok::<_, CodecError>(points)
                })?;
            if !points.is_empty() {
                for corner in [run.start, end] {
                    let key = (run.face, run.cycle, corner);
                    if let Some(stored) = ctx.get_mut_hash_map(
                        &mut corner_points,
                        &key,
                        "catia_mesh_corner_point_entries",
                    )? {
                        ctx.retain_btree_set(
                            stored,
                            |point| {
                                ctx.contains_btree_set(
                                    &points,
                                    point,
                                    "catia_mesh_corner_point_filter",
                                )
                            },
                            "catia_mesh_corner_point_filter",
                        )?;
                    } else {
                        scratch.with_storage(|| {
                            let copied = ctx.collect_btree_set(
                                points.iter().copied(),
                                "catia_mesh_corner_point_copy",
                            )?;
                            ctx.insert_hash_map(
                                &mut corner_points,
                                key,
                                copied,
                                "catia_mesh_corner_point_entries",
                            )
                        })?;
                    }
                }
            }
        }
    }
    let mut assignment_results =
        ctx.collection_vec(coverage.len(), "catia_mesh_assignment_domain_faces")?;
    let mut charged_steps = coverage.iter();
    while let Some(face) =
        ctx.next_charged(&mut charged_steps, "catia_mesh_assignment_domain_faces")?
    {
        let cycle_lengths = &context.cycle_lengths[face.face];
        let unordered_full_cycle = match (edge_candidates, face.gaps.as_slice()) {
            (Some(candidates), [gap]) => {
                defer_validation
                    && cycle_lengths.len() == 1
                    && gap.cycle == 0
                    && gap.start == 0
                    && gap.length == cycle_lengths[0]
                    && gap.length == face.missing_edges.len()
                    && ctx.all_by(
                        &face.missing_edges,
                        |&edge| Ok(edge_rows[edge].handles().len() == 2),
                        "catia_mesh_unordered_missing_edges",
                    )?
                    && ctx.any_by(
                        &face.missing_edges,
                        |&edge| Ok(candidates[edge].len() > 1),
                        "catia_mesh_unordered_missing_edges",
                    )?
            }
            _ => false,
        };
        if unordered_full_cycle {
            assignment_results.push(MeshFaceAssignmentDomain::UnorderedFullCycle(
                ctx.copy_slice(&face.missing_edges, "catia_mesh_unordered_missing_edges")?,
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
                endpoint_trail_assignments(
                    ctx,
                    face.face,
                    (&face.gaps, cycle_lengths),
                    &face.missing_edges,
                    edge_rows,
                    edge_points,
                    &corner_points,
                )?
            } else {
                None
            }
        } else {
            None
        };
        let mut assignments = cycle_assignments.or(trail_assignments);
        let unconstrained_ports = HashMap::new();
        let unconstrained_points = MeshCornerPoints::new();
        for constraints in [
            (
                placement_ports,
                &corner_ports,
                endpoint_domains.as_ref(),
                &corner_points,
            ),
            (
                None,
                &unconstrained_ports,
                endpoint_domains.as_ref(),
                &corner_points,
            ),
            (None, &unconstrained_ports, None, &unconstrained_points),
        ] {
            if assignments.is_some() {
                break;
            }
            assignments = enumerate_face(
                ctx,
                EnumerateFaceInputs {
                    face: face.face,
                    gaps: &face.gaps,
                    cycle_lengths,
                    missing: &face.missing_edges,
                    rows: edge_rows,
                    fixed_complete_row_spans: context.analysis.fixed_complete_row_spans,
                    constraints,
                    canonicalize_spans,
                    remaining_states: &remaining_states,
                },
            )?;
        }
        let domain = if let Some(assignments) = assignments {
            MeshFaceAssignmentDomain::Ordered(assignments)
        } else if defer_validation {
            MeshFaceAssignmentDomain::DeferredValidation(MeshFaceCoverage {
                face: face.face,
                gaps: ctx.copy_slice(&face.gaps, "catia_mesh_deferred_face_gaps")?,
                missing_edges: ctx.copy_slice(
                    &face.missing_edges,
                    "catia_mesh_deferred_face_missing_edges",
                )?,
            })
        } else {
            return Ok(None);
        };
        assignment_results.push(domain);
    }
    Ok(Some((
        assignment_results,
        ctx.copy_slice(edge_runs, "catia_mesh_assignment_edge_runs")?,
    )))
}

fn standard_mesh_missing_edge_assignments(
    ctx: &DecodeContext<'_>,
    bytes: &[u8],
    edge_faces: &[[usize; 2]],
    edge_candidates: Option<&[Vec<[usize; 2]>]>,
    canonicalize_spans: bool,
) -> Result<Option<Vec<Vec<Vec<MeshEdgePlacementCandidate>>>>, CodecError> {
    let Some((context, _context_storage)) =
        StandardMeshBoundaryContext::parse(ctx, bytes, edge_faces)?
    else {
        return Ok(None);
    };
    let Some((domains, _)) = standard_mesh_missing_edge_assignment_domains(
        ctx,
        &context,
        edge_candidates,
        canonicalize_spans,
        false,
    )?
    else {
        return Ok(None);
    };
    let mut assignments = ctx.collection_vec(domains.len(), "catia_missing_assignment_faces")?;
    let mut charged_steps = domains.into_iter();
    while let Some(domain) =
        ctx.next_charged(&mut charged_steps, "catia_missing_assignment_faces")?
    {
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
    let Some((context, _context_storage)) =
        StandardMeshBoundaryContext::parse(ctx, bytes, edge_faces)?
    else {
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
    let mut assignments = ctx.collection_vec(domains.len(), "catia_boundary_assignment_faces")?;
    let mut charged_steps = domains.into_iter();
    while let Some(domain) =
        ctx.next_charged(&mut charged_steps, "catia_boundary_assignment_faces")?
    {
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
    )?
    else {
        return Ok(None);
    };
    let cycle_lengths = context.cycle_lengths.as_slice();
    let fixed_direction = |edge: usize| {
        edge_candidates.is_none()
            || context.analysis.edge_rows[edge].boundary_layout()
                == EdgeBoundaryLayout::CompleteBoundaryRun
    };
    let mut resolved = ctx.collection_vec(domains.len(), "catia_mesh_boundary_domain_faces")?;
    let mut charged_steps = domains.into_iter().enumerate();
    while let Some((face, domain)) =
        ctx.next_charged(&mut charged_steps, "catia_mesh_boundary_domain_faces")?
    {
        // The runs are ordered by face, so each face's runs are one slice.
        let face_start =
            ctx.partition_point(&runs, |run| Ok(run.face < face), "catia_mesh_face_runs")?;
        let face_end = face_start
            + ctx.partition_point(
                &runs[face_start..],
                |run| Ok(run.face == face),
                "catia_mesh_face_runs",
            )?;
        let face_runs = &runs[face_start..face_end];
        let domain = match domain {
            MeshFaceAssignmentDomain::UnorderedFullCycle(edges) => {
                MeshFaceBoundaryDomain::UnorderedFullCycle(edges)
            }
            MeshFaceAssignmentDomain::DeferredValidation(coverage) => {
                let mut cycles = ctx
                    .collection_vec(cycle_lengths[face].len(), "catia_deferred_boundary_cycles")?;
                for &length in
                    ctx.admit_iter(&cycle_lengths[face], "catia_deferred_boundary_cycles")?
                {
                    cycles.push(MeshDeferredBoundaryCycle {
                        length,
                        exact_uses: Vec::new(),
                    });
                }
                for run in ctx.admit_iter(face_runs, "catia_deferred_boundary_exact_uses")? {
                    let length = cycles[run.cycle].length;
                    ctx.push_vec(
                        &mut cycles[run.cycle].exact_uses,
                        (
                            MeshBoundaryEdgeCandidate {
                                edge: run.edge,
                                start: run.start,
                                end: run.end(length),
                                reversed: fixed_direction(run.edge).then_some(run.reversed),
                            },
                            run.segment_count,
                        ),
                        "catia_deferred_boundary_exact_uses",
                    )?;
                }
                for cycle in
                    ctx.admit_iter(&mut cycles, "catia_deferred_boundary_exact_uses_sort")?
                {
                    ctx.sort_unstable_by(
                        &mut cycle.exact_uses,
                        |value| &value.0.start,
                        Ord::cmp,
                        "catia_deferred_boundary_exact_uses_sort",
                    )?;
                }
                MeshFaceBoundaryDomain::DeferredValidation(MeshDeferredFaceBoundary {
                    cycles,
                    missing_edges: coverage.missing_edges,
                })
            }
            MeshFaceAssignmentDomain::Ordered(assignments) => {
                let mut ordered =
                    ctx.collection_vec(assignments.len(), "catia_mesh_ordered_assignments")?;
                let mut charged_steps = assignments.into_iter();
                while let Some(assignment) =
                    ctx.next_charged(&mut charged_steps, "catia_mesh_ordered_assignments")?
                {
                    let Some(boundaries) = ordered_boundary_cycles(
                        ctx,
                        &cycle_lengths[face],
                        face_runs,
                        &assignment,
                        fixed_direction,
                    )?
                    else {
                        return Ok(None);
                    };
                    ordered.push(MeshFaceBoundaryAssignment { boundaries });
                }
                MeshFaceBoundaryDomain::Ordered(ordered)
            }
        };
        resolved.push(domain);
    }
    Ok(Some(resolved))
}

/// The edge uses of each boundary cycle in start order, from the matched runs
/// and one complete placement assignment; `None` unless every cycle segment is
/// covered exactly once.
fn ordered_boundary_cycles(
    ctx: &DecodeContext<'_>,
    cycle_lengths: &[usize],
    face_runs: &[MeshEdgeRun],
    assignment: &[MeshEdgePlacementCandidate],
    fixed_direction: impl Fn(usize) -> bool,
) -> Result<Option<Vec<Vec<MeshBoundaryEdgeCandidate>>>, CodecError> {
    const OPERATION: &str = "catia_mesh_ordered_boundary_entries";
    let mut scratch = ctx.reserve_scoped(0, OPERATION)?;
    let mut boundaries = scratch.with_storage(|| {
        ctx.collect_indexed_vec(cycle_lengths.len(), "catia_mesh_ordered_boundaries", |_| {
            Ok(Vec::new())
        })
    })?;
    for run in ctx.admit_iter(face_runs, OPERATION)? {
        scratch.with_storage(|| {
            ctx.push_vec(
                &mut boundaries[run.cycle],
                (
                    MeshBoundaryEdgeCandidate {
                        edge: run.edge,
                        start: run.start,
                        end: run.end(cycle_lengths[run.cycle]),
                        reversed: fixed_direction(run.edge).then_some(run.reversed),
                    },
                    run.segment_count,
                ),
                OPERATION,
            )
        })?;
    }
    for placement in ctx.admit_iter(assignment, OPERATION)? {
        scratch.with_storage(|| {
            ctx.push_vec(
                &mut boundaries[placement.cycle],
                (
                    MeshBoundaryEdgeCandidate {
                        edge: placement.edge,
                        start: placement.start,
                        end: placement.end(cycle_lengths[placement.cycle]),
                        reversed: None,
                    },
                    placement.segment_count,
                ),
                OPERATION,
            )
        })?;
    }
    let mut completed = ctx.collection_vec(boundaries.len(), "catia_mesh_completed_boundaries")?;
    let mut charged_steps = boundaries.iter_mut().enumerate();
    while let Some((cycle, uses)) =
        ctx.next_charged(&mut charged_steps, "catia_mesh_completed_boundaries")?
    {
        ctx.sort_unstable_by(
            &mut *uses,
            |value| &value.0.start,
            Ord::cmp,
            "catia_mesh_ordered_boundary_sort",
        )?;
        let length = cycle_lengths[cycle];
        let (coverage, _coverage_storage) =
            ctx.with_scoped_storage("catia_mesh_boundary_coverage", || {
                let mut coverage = ctx.alloc_filled(length, 0u8, "catia_mesh_boundary_coverage")?;
                let mut charged_steps = uses.iter();
                while let Some((edge, segment_count)) =
                    ctx.next_charged(&mut charged_steps, "catia_mesh_boundary_coverage")?
                {
                    let mut charged_steps = 0..*segment_count;
                    while let Some(offset) =
                        ctx.next_charged(&mut charged_steps, "catia_mesh_boundary_coverage")?
                    {
                        let covered = &mut coverage[(edge.start + offset) % length];
                        let Some(count) = covered.checked_add(1) else {
                            return Ok(None);
                        };
                        *covered = count;
                    }
                }
                Ok::<_, CodecError>(Some(coverage))
            })?;
        let Some(coverage) = coverage else {
            return Ok(None);
        };
        if ctx.any_by(
            &coverage,
            |count| Ok(*count != 1),
            "catia_mesh_boundary_coverage",
        )? {
            return Ok(None);
        }
        let mut boundary =
            ctx.collection_vec(uses.len(), "catia_mesh_completed_boundary_entries")?;
        for (edge, _) in ctx.admit_iter(&*uses, "catia_mesh_completed_boundary_entries")? {
            boundary.push(*edge);
        }
        completed.push(boundary);
    }
    Ok(Some(completed))
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
) -> Result<Option<StandardTopologyDraft>, CodecError> {
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
        ctx.push_vec(&mut selected, choice, "catia_mesh_selection_choices")?;
    }
    reconstruct_mesh_selection(ctx, &edge_rows, &vertex_points, &selected, edge_directions)
}

#[derive(Debug)]
struct BoundaryEndpointSupport {
    by_edge: BTreeMap<usize, BTreeSet<[usize; 2]>>,
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

    let mut scratch = ctx.reserve_scoped(0, "catia_boundary_support_layer_rows")?;
    let mut layers = scratch
        .with_storage(|| ctx.collection_vec(boundary.len(), "catia_boundary_support_layer_rows"))?;
    let mut charged_steps = boundary.iter();
    while let Some(use_) =
        ctx.next_charged(&mut charged_steps, "catia_boundary_support_layer_rows")?
    {
        let Some(pairs) = edge_candidates
            .get(use_.edge)
            .filter(|pairs| !pairs.is_empty())
        else {
            return Ok(None);
        };
        let Some(count) = pairs.len().checked_mul(2) else {
            return Err(ctx.refuse_codec_limit(
                "catia_boundary_support_layer_states",
                u64::MAX,
                u64::MAX,
            ));
        };
        let states = scratch.with_storage(|| -> Result<Vec<State>, CodecError> {
            let mut states = ctx.collection_vec(count, "catia_boundary_support_layer_states")?;
            for &pair in ctx.admit_iter(pairs, "catia_boundary_support_layer_states")? {
                let unordered = [pair[0].min(pair[1]), pair[0].max(pair[1])];
                states.push(State {
                    pair: unordered,
                    start: unordered[0],
                    end: unordered[1],
                });
                states.push(State {
                    pair: unordered,
                    start: unordered[1],
                    end: unordered[0],
                });
            }
            Ok(states)
        })?;
        layers.push(states);
    }
    let Some(first_layer) = layers.first() else {
        return Ok(None);
    };
    let mut first_points = BTreeSet::new();
    for state in ctx.admit_iter(first_layer, "catia_boundary_first_points")? {
        scratch.with_storage(|| {
            ctx.insert_btree_set(
                &mut first_points,
                state.start,
                "catia_boundary_first_points",
            )
        })?;
    }
    let make_marks = |row_operation, item_operation| -> Result<Vec<Vec<bool>>, CodecError> {
        let mut marks = ctx.collection_vec(layers.len(), row_operation)?;
        for layer in ctx.admit_iter(&layers, row_operation)? {
            marks.push(ctx.alloc_filled(layer.len(), false, item_operation)?);
        }
        Ok(marks)
    };
    let mut supported = scratch.with_storage(|| {
        make_marks(
            "catia_boundary_support_mark_rows",
            "catia_boundary_layer_marks",
        )
    })?;
    let mut starts = first_points.iter();
    while let Some(&first_point) = ctx.next_charged(&mut starts, "catia_boundary_first_points")? {
        // One local unit visits one propagation or union state. Point-set
        // lookups and the closure query charge their own context work.
        let mut pass_storage = ctx.reserve_scoped(0, "catia_boundary_forward_marks")?;
        let mut forward = pass_storage.with_storage(|| {
            make_marks(
                "catia_boundary_forward_mark_rows",
                "catia_boundary_forward_marks",
            )
        })?;
        for (state, reachable) in first_layer.iter().zip(&mut forward[0]) {
            if !budget.charge() {
                return Ok(None);
            }
            *reachable = state.start == first_point;
        }
        let mut forward_layers = 1..layers.len();
        while let Some(layer) =
            ctx.next_charged(&mut forward_layers, "catia_boundary_forward_layers")?
        {
            let mut points_storage = ctx.reserve_scoped(0, "catia_boundary_reachable_points")?;
            let mut reachable_points = HashSet::new();
            for (state, reachable) in layers[layer - 1].iter().zip(&forward[layer - 1]) {
                if !budget.charge() {
                    return Ok(None);
                }
                if *reachable {
                    points_storage.with_storage(|| {
                        ctx.insert_hash_set(
                            &mut reachable_points,
                            state.end,
                            "catia_boundary_reachable_points",
                        )
                    })?;
                }
            }
            for (right, right_state) in layers[layer].iter().enumerate() {
                if !budget.charge() {
                    return Ok(None);
                }
                forward[layer][right] = ctx.contains_hash_set(
                    &reachable_points,
                    &right_state.start,
                    "catia_boundary_reachable_points",
                )?;
            }
        }
        let mut backward = pass_storage.with_storage(|| {
            make_marks(
                "catia_boundary_backward_mark_rows",
                "catia_boundary_backward_marks",
            )
        })?;
        let last = layers.len() - 1;
        for (state, (reachable, value)) in layers[last]
            .iter()
            .zip(forward[last].iter().zip(&mut backward[last]))
        {
            if !budget.charge() {
                return Ok(None);
            }
            *value = *reachable && state.end == first_point;
        }
        let mut backward_layers = (0..last).rev();
        while let Some(layer) =
            ctx.next_charged(&mut backward_layers, "catia_boundary_backward_layers")?
        {
            let mut points_storage = ctx.reserve_scoped(0, "catia_boundary_supported_points")?;
            let mut supported_points = HashSet::new();
            for (state, supported) in layers[layer + 1].iter().zip(&backward[layer + 1]) {
                if !budget.charge() {
                    return Ok(None);
                }
                if *supported {
                    points_storage.with_storage(|| {
                        ctx.insert_hash_set(
                            &mut supported_points,
                            state.start,
                            "catia_boundary_supported_points",
                        )
                    })?;
                }
            }
            for (left, left_state) in layers[layer].iter().enumerate() {
                if !budget.charge() {
                    return Ok(None);
                }
                backward[layer][left] = ctx.contains_hash_set(
                    &supported_points,
                    &left_state.end,
                    "catia_boundary_supported_points",
                )?;
            }
        }
        if ctx.any_by(
            &backward[0],
            |supported| Ok(*supported),
            "catia_boundary_closed_support",
        )? {
            let mut charged_steps = 0..layers.len();
            while let Some(layer) =
                ctx.next_charged(&mut charged_steps, "catia_boundary_union_layers")?
            {
                for state in 0..layers[layer].len() {
                    if !budget.charge() {
                        return Ok(None);
                    }
                    supported[layer][state] |= forward[layer][state] && backward[layer][state];
                }
            }
        }
    }
    let mut by_edge = BTreeMap::<usize, BTreeSet<[usize; 2]>>::new();
    let mut charged_steps = boundary.iter().enumerate();
    while let Some((layer, use_)) =
        ctx.next_charged(&mut charged_steps, "catia_boundary_support_edges")?
    {
        let mut values = BTreeSet::new();
        for (state, supported) in ctx
            .admit_iter(&layers[layer], "catia_boundary_supported_pairs")?
            .zip(&supported[layer])
        {
            if *supported {
                ctx.insert_btree_set(&mut values, state.pair, "catia_boundary_supported_pairs")?;
            }
        }
        if values.is_empty() {
            return Ok(None);
        }
        if let Some(stored) =
            ctx.get_mut_btree_map(&mut by_edge, &use_.edge, "catia_boundary_support_edges")?
        {
            ctx.retain_btree_set(
                stored,
                |pair| ctx.contains_btree_set(&values, pair, "catia_boundary_supported_pairs"),
                "catia_boundary_supported_pairs",
            )?;
        } else {
            ctx.insert_btree_map(
                &mut by_edge,
                use_.edge,
                values,
                "catia_boundary_support_edges",
            )?;
        }
    }
    let complete = ctx.all_by(
        &by_edge,
        |(_, domain)| Ok(!domain.is_empty()),
        "catia_boundary_support_edges",
    )?;
    Ok(complete.then_some(BoundaryEndpointSupport { by_edge }))
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
    const OPERATION: &str = "catia_prune_support_edges";
    if edge_faces.len() != edge_candidates.len() {
        return Ok(None);
    }
    let Some(face_run) = selected_standard_run(ctx, bytes)? else {
        return Ok(None);
    };
    let after_faces = face_run.after_faces();
    let Some(vertex_header) = ({
        let (rows, _row_storage) = ctx.with_scoped_storage("catia_prune_edge_workspace", || {
            parse_edge_tables(ctx, bytes, after_faces)
        })?;
        rows.map(|(_, header)| header)
    }) else {
        return Ok(None);
    };
    let Some(point_count) = ({
        let (points, _point_storage) = ctx
            .with_scoped_storage("catia_prune_vertex_workspace", || {
                parse_vertex_table(ctx, bytes, vertex_header)
            })?;
        points.map(|points| points.len())
    }) else {
        return Ok(None);
    };
    let mut scratch = ctx.reserve_scoped(0, "catia_prune_complete_point_pairs")?;
    // An edge without candidates may take any pair of distinct points.
    let complete_domain = if ctx.any_by(
        edge_candidates,
        |domain| Ok(domain.is_empty()),
        "catia_prune_complete_point_pairs",
    )? {
        scratch.with_storage(|| -> Result<Vec<[usize; 2]>, CodecError> {
            let mut complete = Vec::new();
            for left in ctx.admit_iter(&(0..point_count), "catia_prune_complete_point_pairs")? {
                for right in ctx.admit_iter(
                    &((left + 1)..point_count),
                    "catia_prune_complete_point_pairs",
                )? {
                    ctx.push_vec(
                        &mut complete,
                        [left, right],
                        "catia_prune_complete_point_pairs",
                    )?;
                }
            }
            Ok(complete)
        })?
    } else {
        Vec::new()
    };
    let mut candidates = ctx.collection_vec(edge_candidates.len(), "catia_prune_candidate_rows")?;
    for domain in ctx.admit_iter(edge_candidates, "catia_prune_candidate_rows")? {
        let source = if domain.is_empty() {
            &complete_domain
        } else {
            domain
        };
        candidates.push(ctx.copy_slice(source, "catia_prune_candidate_pairs")?);
    }
    let (faces, _face_storage) = ctx
        .with_scoped_storage("catia_prune_boundary_workspace", || {
            standard_mesh_boundary_assignments(ctx, bytes, edge_faces, None)
        })?;
    let Some(mut faces) = faces else {
        return Ok(None);
    };
    let budget = ctx.work_budget(u64_from_index(MAX_MESH_CONSTRAINT_OPERATIONS));
    let size = |faces: &[Vec<MeshFaceBoundaryAssignment>],
                candidates: &[Vec<[usize; 2]>]|
     -> Result<(usize, usize), CodecError> {
        let mut assignments = 0usize;
        for face in ctx.admit_iter(faces, "catia_missing_edge_iteration")? {
            assignments += face.len();
        }
        let mut pairs = 0usize;
        for domain in ctx.admit_iter(candidates, "catia_missing_edge_iteration")? {
            pairs += domain.len();
        }
        Ok((assignments, pairs))
    };
    loop {
        let before = size(&faces, &candidates)?;
        let mut round_storage = ctx.reserve_scoped(0, "catia_prune_face_support_rows")?;
        let mut face_supports = round_storage
            .with_storage(|| ctx.collection_vec(faces.len(), "catia_prune_face_support_rows"))?;
        let mut charged_steps = faces.iter_mut();
        while let Some(assignments) =
            ctx.next_charged(&mut charged_steps, "catia_prune_face_support_rows")?
        {
            let mut evaluated = Vec::new();
            let mut evaluated_storage =
                ctx.reserve_scoped(0, "catia_prune_evaluated_assignments")?;
            'assignment: for (index, assignment) in ctx
                .admit_iter(&*assignments, "catia_prune_evaluated_assignments")?
                .enumerate()
            {
                let mut support = BTreeMap::<usize, BTreeSet<[usize; 2]>>::new();
                let mut support_storage =
                    ctx.reserve_scoped(0, "catia_prune_assignment_support")?;
                let mut boundaries = assignment.boundaries.iter();
                while let Some(boundary) = ctx.next_charged(&mut boundaries, OPERATION)? {
                    let Some(boundary_support) = support_storage.with_storage(|| {
                        boundary_endpoint_support(ctx, boundary, &candidates, &budget)
                    })?
                    else {
                        continue 'assignment;
                    };
                    for (edge, domain) in ctx.admit_iter(boundary_support.by_edge, OPERATION)? {
                        if let Some(stored) =
                            ctx.get_mut_btree_map(&mut support, &edge, OPERATION)?
                        {
                            ctx.retain_btree_set(
                                stored,
                                |pair| ctx.contains_btree_set(&domain, pair, OPERATION),
                                OPERATION,
                            )?;
                        } else {
                            support_storage.with_storage(|| {
                                ctx.insert_btree_map(&mut support, edge, domain, OPERATION)
                            })?;
                        }
                    }
                }
                if ctx.all_by(&support, |(_, domain)| Ok(!domain.is_empty()), OPERATION)? {
                    evaluated_storage.with_storage(|| {
                        ctx.push_vec(
                            &mut evaluated,
                            (index, support, support_storage),
                            "catia_prune_evaluated_assignments",
                        )
                    })?;
                }
            }
            if evaluated.is_empty() {
                return Ok(None);
            }
            let (mut kept, kept_storage) = ctx
                .with_scoped_storage("catia_prune_assignment_marks", || {
                    ctx.alloc_filled(assignments.len(), false, "catia_prune_assignment_marks")
                })?;
            for &(index, _, _) in ctx.admit_iter(&evaluated, "catia_prune_retained_assignments")? {
                kept[index] = true;
            }
            let mut index = 0;
            ctx.retain_vec(
                assignments,
                |_| {
                    let selected = kept[index];
                    index += 1;
                    Ok(selected)
                },
                "catia_prune_retained_assignments",
            )?;
            let support_rows = round_storage.with_storage(|| {
                ctx.collect_vec(
                    evaluated
                        .into_iter()
                        .map(|(_, support, storage)| (support, storage)),
                    "catia_prune_assignment_supports",
                )
            })?;
            drop((evaluated_storage, kept, kept_storage));
            face_supports.push(support_rows);
        }
        let mut charged_steps = candidates.iter_mut().enumerate();
        while let Some((edge, domain)) =
            ctx.next_charged(&mut charged_steps, "catia_prune_incident_support_pairs")?
        {
            let [first, second] = edge_faces[edge];
            let mut allowed = None;
            for face in [
                Some(first.min(second)),
                (first != second).then_some(first.max(second)),
            ]
            .into_iter()
            .flatten()
            {
                let (support, support_storage) = ctx.with_scoped_storage(
                    "catia_prune_incident_support_pairs",
                    || -> Result<BTreeSet<[usize; 2]>, CodecError> {
                        let mut support = BTreeSet::new();
                        for (assignment, _storage) in ctx.admit_iter(
                            &face_supports[face],
                            "catia_prune_incident_support_pairs",
                        )? {
                            let Some(pairs) = ctx.get_btree_map(
                                assignment,
                                &edge,
                                "catia_prune_incident_support_pairs",
                            )?
                            else {
                                continue;
                            };
                            for &pair in
                                ctx.admit_iter(pairs, "catia_prune_incident_support_pairs")?
                            {
                                ctx.insert_btree_set(
                                    &mut support,
                                    pair,
                                    "catia_prune_incident_support_pairs",
                                )?;
                            }
                        }
                        Ok(support)
                    },
                )?;
                if support.is_empty() {
                    return Ok(None);
                }
                if let Some((allowed, _storage)) = &mut allowed {
                    ctx.retain_btree_set(
                        allowed,
                        |pair| {
                            ctx.contains_btree_set(
                                &support,
                                pair,
                                "catia_prune_incident_support_pairs",
                            )
                        },
                        "catia_prune_incident_support_pairs",
                    )?;
                } else {
                    allowed = Some((support, support_storage));
                }
            }
            let Some((allowed, _allowed_storage)) = allowed else {
                return Ok(None);
            };
            ctx.retain_vec(
                domain,
                |pair| {
                    ctx.contains_btree_set(
                        &allowed,
                        &[pair[0].min(pair[1]), pair[0].max(pair[1])],
                        "catia missing edge allowed pairs",
                    )
                },
                "catia missing edge allowed pairs",
            )?;
            if domain.is_empty() {
                return Ok(None);
            }
        }
        if size(&faces, &candidates)? == before {
            break;
        }
    }
    Ok(Some(candidates))
}

type MeshCorner = (usize, usize, usize);
type MeshCornerPoints = HashMap<MeshCorner, BTreeSet<usize>>;

struct MeshAssignmentCorners {
    assignments: Vec<Vec<Vec<MeshEdgePlacementCandidate>>>,
    corner_points: HashMap<MeshCorner, [Option<usize>; 2]>,
    cycle_lengths: Vec<Vec<usize>>,
}

fn standard_mesh_assignment_corner_points(
    ctx: &DecodeContext<'_>,
    bytes: &[u8],
    edge_faces: &[[usize; 2]],
    edge_points: &[Option<[usize; 2]>],
) -> Result<Option<MeshAssignmentCorners>, CodecError> {
    const OPERATION: &str = "catia_corner_point_narrowing";
    let (analysis, _analysis_storage) = ctx
        .with_scoped_storage("catia_corner_mesh_analysis", || {
            standard_mesh_analysis(ctx, bytes)
        })?;
    let Some(analysis) = analysis else {
        return Ok(None);
    };
    let edge_rows = &analysis.edge_rows;
    if edge_rows.len() != edge_points.len() || edge_rows.len() != edge_faces.len() {
        return Ok(None);
    }
    let (runs, _run_storage) = ctx.with_scoped_storage("catia_corner_run_workspace", || {
        mesh_edge_runs(ctx, &analysis)
    })?;
    let Some(assignments) =
        standard_mesh_missing_edge_assignments(ctx, bytes, edge_faces, None, true)?
    else {
        return Ok(None);
    };
    let cycle_lengths = mesh_cycle_lengths(ctx, &analysis.cycles)?;
    let mut scratch = ctx.reserve_scoped(0, "catia_corner_run_constraints")?;
    let mut corner_points = HashMap::<MeshCorner, [Option<usize>; 2]>::new();
    // Each run binds its two corners to its endpoint pair; the constraints of
    // each corner are indexed for narrowing.
    let mut run_constraints = Vec::new();
    let mut constraints_by_corner = HashMap::<MeshCorner, Vec<usize>>::new();
    let mut charged_steps = runs.iter();
    while let Some(run) = ctx.next_charged(&mut charged_steps, "catia_corner_run_constraints")? {
        let Some(pair) = edge_points[run.edge] else {
            continue;
        };
        let positions = [
            (run.face, run.cycle, run.start),
            (
                run.face,
                run.cycle,
                run.end(cycle_lengths[run.face][run.cycle]),
            ),
        ];
        for position in positions {
            if let Some(stored) =
                ctx.get_mut_hash_map(&mut corner_points, &position, "catia_corner_point_entries")?
            {
                // Each corner starts with the two endpoints of one run.
                for point in &mut *stored {
                    *point = point.filter(|point| pair.contains(point));
                }
                if stored.iter().all(Option::is_none) {
                    return Ok(None);
                }
            } else {
                let points = [Some(pair[0]), (pair[1] != pair[0]).then_some(pair[1])];
                ctx.insert_hash_map(
                    &mut corner_points,
                    position,
                    points,
                    "catia_corner_point_entries",
                )?;
            }
            let index = run_constraints.len();
            scratch.with_storage(|| {
                ctx.push_hash_group(
                    &mut constraints_by_corner,
                    position,
                    index,
                    "catia_corner_run_constraints",
                    "catia_corner_run_constraints",
                )
            })?;
        }
        scratch.with_storage(|| {
            ctx.push_vec(
                &mut run_constraints,
                (positions[0], positions[1], pair),
                "catia_corner_run_constraints",
            )
        })?;
    }
    // A corner left with one point removes that point from the corner across
    // the run and keeps it within the run's pair. Narrowing only removes
    // points, so the fixed point does not depend on the order constraints are
    // revisited; a constraint is revisited when one of its corners shrinks.
    let mut queued = scratch.with_storage(|| {
        ctx.alloc_filled(
            run_constraints.len(),
            true,
            "catia_corner_point_constraint_round",
        )
    })?;
    let mut pending = scratch.with_storage(|| {
        ctx.collect_vec(
            (0..run_constraints.len()).rev(),
            "catia_corner_point_constraint_round",
        )
    })?;
    while let Some(constraint) = pending.pop() {
        ctx.charge_work(1, "catia_corner_point_constraint_round")?;
        queued[constraint] = false;
        let (left, right, pair) = run_constraints[constraint];
        let single = |corner: &MeshCorner| -> Result<Option<usize>, CodecError> {
            let points = ctx
                .get_hash_map(&corner_points, corner, OPERATION)?
                .ok_or_else(|| CodecError::malformed("corner has no point set"))?;
            Ok(match *points {
                [Some(point), None] | [None, Some(point)] => Some(point),
                _ => None,
            })
        };
        let left_single = single(&left)?;
        let right_single = single(&right)?;
        for (single, target) in [(left_single, right), (right_single, left)] {
            let Some(point) = single else {
                continue;
            };
            let stored = ctx
                .get_mut_hash_map(&mut corner_points, &target, OPERATION)?
                .ok_or_else(|| CodecError::malformed("corner has no point set"))?;
            let before = *stored;
            for candidate in &mut *stored {
                *candidate =
                    candidate.filter(|candidate| *candidate != point && pair.contains(candidate));
            }
            if stored.iter().all(Option::is_none) {
                return Ok(None);
            }
            if *stored == before {
                continue;
            }
            let Some(affected) = ctx.get_hash_map(&constraints_by_corner, &target, OPERATION)?
            else {
                continue;
            };
            for &other in ctx.admit_iter(affected, OPERATION)? {
                if !std::mem::replace(&mut queued[other], true) {
                    scratch.with_storage(|| {
                        ctx.push_vec(&mut pending, other, "catia_corner_point_constraint_round")
                    })?;
                }
            }
        }
    }
    Ok(Some(MeshAssignmentCorners {
        assignments,
        corner_points,
        cycle_lengths,
    }))
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
    const OPERATION: &str = "catia_placement_endpoint_candidate_pairs";
    let (corners, _corner_storage) = ctx
        .with_scoped_storage("catia_placement_corner_workspace", || {
            standard_mesh_assignment_corner_points(ctx, bytes, edge_faces, edge_points)
        })?;
    let Some(MeshAssignmentCorners {
        assignments,
        corner_points,
        cycle_lengths,
    }) = corners
    else {
        return Ok(None);
    };
    let mut faces = ctx.collection_vec(assignments.len(), "catia_placement_endpoint_face_rows")?;
    for face in ctx.admit_iter(assignments, "catia_placement_endpoint_face_rows")? {
        let mut face_assignments =
            ctx.collection_vec(face.len(), "catia_placement_endpoint_assignment_rows")?;
        for assignment in ctx.admit_iter(face, "catia_placement_endpoint_assignment_rows")? {
            let mut placements =
                ctx.collection_vec(assignment.len(), "catia_placement_endpoint_placement_rows")?;
            for placement in
                ctx.admit_iter(assignment, "catia_placement_endpoint_placement_rows")?
            {
                let starts = ctx.get_hash_map(
                    &corner_points,
                    &(placement.face, placement.cycle, placement.start),
                    OPERATION,
                )?;
                let ends = ctx.get_hash_map(
                    &corner_points,
                    &(
                        placement.face,
                        placement.cycle,
                        placement.end(cycle_lengths[placement.face][placement.cycle]),
                    ),
                    OPERATION,
                )?;
                let endpoint_pairs = if let Some((starts, ends)) = starts.zip(ends) {
                    // At most two points per corner give four pairs.
                    let mut ordered = [None; 4];
                    let mut count = 0;
                    for &start in starts.iter().flatten() {
                        for &end in ends.iter().flatten() {
                            if start != end {
                                ordered[count] = Some([start.min(end), start.max(end)]);
                                count += 1;
                            }
                        }
                    }
                    ordered.sort_unstable();
                    let mut pairs = Vec::new();
                    let mut previous = None;
                    for pair in ordered.into_iter().flatten() {
                        if previous != Some(pair) {
                            ctx.push_vec(&mut pairs, pair, OPERATION)?;
                        }
                        previous = Some(pair);
                    }
                    Some(pairs)
                } else {
                    None
                };
                placements.push(MeshEdgePlacementEndpointCandidate {
                    placement,
                    endpoint_pairs,
                });
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
    const OPERATION: &str = "catia_placement_face_domain_pairs";
    let Some(mut faces) =
        standard_mesh_missing_edge_endpoint_assignments(ctx, bytes, edge_faces, edge_points)?
    else {
        return Ok(None);
    };
    let mut scratch = ctx.reserve_scoped(0, "catia_placement_face_edges")?;
    // The edges incident to each face, ascending.
    let mut edges_by_face = scratch.with_storage(|| {
        ctx.collect_indexed_vec(
            faces.len(),
            "catia_placement_face_edges",
            |_| Ok(Vec::new()),
        )
    })?;
    for (edge, &incident) in ctx
        .admit_iter(edge_faces, "catia_placement_face_edges")?
        .enumerate()
    {
        for face in [
            Some(incident[0]),
            (incident[1] != incident[0]).then_some(incident[1]),
        ]
        .into_iter()
        .flatten()
        {
            if let Some(edges) = edges_by_face.get_mut(face) {
                scratch.with_storage(|| ctx.push_vec(edges, edge, "catia_placement_face_edges"))?;
            }
        }
    }
    let size = |faces: &[Vec<Vec<MeshEdgePlacementEndpointCandidate>>]| -> Result<(usize, usize), CodecError> {
        let mut assignments = 0usize;
        let mut pairs = 0usize;
        for face in ctx.admit_iter(faces, "catia_missing_edge_iteration")? {
            assignments += face.len();
            for assignment in ctx.admit_iter(face, "catia_missing_edge_iteration")? {
                for candidate in ctx.admit_iter(assignment, "catia_missing_edge_iteration")? {
                    pairs += candidate.endpoint_pairs.as_ref().map_or(0, Vec::len);
                }
            }
        }
        Ok((assignments, pairs))
    };
    loop {
        let before = size(&faces)?;
        let mut round_storage = ctx.reserve_scoped(0, "catia_placement_face_domain_entries")?;
        let mut face_domains = HashMap::<
            (usize, usize),
            Option<(
                BTreeSet<[usize; 2]>,
                cadmpeg_core::decode::ScopedReservation<'_>,
            )>,
        >::new();
        for (face, assignments) in ctx.admit_iter(&faces, OPERATION)?.enumerate() {
            for &edge in ctx.admit_iter(&edges_by_face[face], OPERATION)? {
                let mut domain = BTreeSet::new();
                let mut domain_storage = ctx.reserve_scoped(0, OPERATION)?;
                let mut complete = true;
                let mut assignments = assignments.iter();
                while let Some(assignment) = ctx.next_charged(&mut assignments, OPERATION)? {
                    let Some(pairs) = ctx
                        .find_by(
                            assignment,
                            |candidate| Ok(candidate.placement.edge == edge),
                            OPERATION,
                        )?
                        .and_then(|candidate| candidate.endpoint_pairs.as_ref())
                    else {
                        complete = false;
                        break;
                    };
                    for &pair in ctx.admit_iter(pairs, OPERATION)? {
                        domain_storage
                            .with_storage(|| ctx.insert_btree_set(&mut domain, pair, OPERATION))?;
                    }
                }
                round_storage.with_storage(|| {
                    ctx.insert_hash_map(
                        &mut face_domains,
                        (face, edge),
                        complete.then_some((domain, domain_storage)),
                        "catia_placement_face_domain_entries",
                    )
                })?;
            }
        }
        let mut charged_steps = faces.iter_mut();
        while let Some(assignments) =
            ctx.next_charged(&mut charged_steps, "catia_placement_kept_assignments")?
        {
            ctx.retain_mut(
                assignments,
                |assignment| {
                    let mut charged_steps = assignment.iter_mut();
                    while let Some(candidate) = ctx.next_charged(&mut charged_steps, OPERATION)? {
                        let edge = candidate.placement.edge;
                        let seed = edge_points[edge];
                        let opposite_face = edge_faces[edge]
                            .into_iter()
                            .find(|&face| face != candidate.placement.face);
                        let opposite = match opposite_face {
                            Some(face) => ctx
                                .get_hash_map(&face_domains, &(face, edge), OPERATION)?
                                .and_then(Option::as_ref),
                            None => None,
                        };
                        let Some(domain) = &mut candidate.endpoint_pairs else {
                            continue;
                        };
                        ctx.retain_vec(
                            domain,
                            |pair| {
                                Ok(seed.is_none_or(|seed| same_unordered_pair(*pair, seed))
                                    && match opposite {
                                        Some((opposite, _storage)) => {
                                            ctx.contains_btree_set(opposite, pair, OPERATION)?
                                        }
                                        None => true,
                                    })
                            },
                            OPERATION,
                        )?;
                        if domain.is_empty() {
                            return Ok(false);
                        }
                    }
                    Ok(true)
                },
                "catia_placement_kept_assignments",
            )?;
            if assignments.is_empty() {
                return Ok(None);
            }
        }
        if size(&faces)? == before {
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
    let (edge_rows, _edge_rows_storage) = ctx
        .with_scoped_storage("catia_placement_edge_rows", || {
            standard_edge_rows(ctx, bytes)
        })?;
    let Some(edge_rows) = edge_rows else {
        return Ok(None);
    };
    if edge_rows.len() != edge_points.len() || edge_rows.len() != edge_faces.len() {
        return Ok(None);
    }
    let (assignments, _assignment_storage) =
        ctx.with_scoped_storage("catia_placement_assignment_workspace", || {
            standard_mesh_pruned_missing_edge_endpoint_assignments(
                ctx,
                bytes,
                edge_faces,
                edge_points,
            )
        })?;
    let Some(assignments) = assignments else {
        return Ok(None);
    };
    let mut domains =
        ctx.collect_indexed_vec(edge_rows.len(), "catia_placement_endpoint_domains", |_| {
            Ok(Vec::new())
        })?;
    let mut scratch = ctx.reserve_scoped(0, "catia_placement_counts")?;
    let mut placement_counts = scratch
        .with_storage(|| ctx.alloc_filled(edge_rows.len(), 0usize, "catia_placement_counts"))?;
    let mut bound_counts = scratch.with_storage(|| {
        ctx.alloc_filled(edge_rows.len(), 0usize, "catia_placement_bound_counts")
    })?;
    for face in ctx.admit_iter(assignments, "catia_placement_candidates")? {
        let (placements, _placements_storage) =
            ctx.with_scoped_storage("catia_placement_candidates", || {
                let mut placements = Vec::new();
                for assignment in ctx.admit_iter(face, "catia_placement_candidates")? {
                    ctx.extend_vec(&mut placements, assignment, "catia_placement_candidates")?;
                }
                ctx.sort_unstable_by(
                    &mut placements,
                    |value| &value.placement,
                    Ord::cmp,
                    "catia missing edge placement candidates sort",
                )?;
                ctx.dedup_by_key(
                    &mut placements,
                    |candidate| Ok(candidate.placement),
                    "catia_placement_candidates",
                )?;
                Ok::<_, CodecError>(placements)
            })?;
        for candidate in ctx.admit_iter(placements, "catia_placement_endpoint_pairs")? {
            let edge = candidate.placement.edge;
            placement_counts[edge] += 1;
            if let Some(pairs) = candidate.endpoint_pairs {
                bound_counts[edge] += 1;
                ctx.extend_vec(&mut domains[edge], pairs, "catia_placement_endpoint_pairs")?;
            }
        }
    }
    for (edge, domain) in ctx
        .admit_iter(&mut domains, "catia missing edge placement domain sort")?
        .enumerate()
    {
        if bound_counts[edge] == placement_counts[edge] {
            ctx.sort_unstable_by(
                domain,
                |value| value,
                Ord::cmp,
                "catia missing edge placement domain sort",
            )?;
            ctx.dedup_vec(domain, "catia missing edge placement domain sort")?;
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
    if let Some(&stored) = ctx.get_hash_map(port_points, &port, "catia_port_bound_points")? {
        return Ok(stored == point);
    }
    ctx.insert_hash_map(port_points, port, point, "catia_port_bound_points")?;
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
    const OPERATION: &str = "catia_port_incident_edges";
    if edge_ports.len() != endpoint_pairs.len() {
        return Ok(None);
    }
    if !ordered_endpoint_pairs.is_empty() && ordered_endpoint_pairs.len() != endpoint_pairs.len() {
        return Ok(None);
    }
    let mut resolved = ctx.copy_slice(endpoint_pairs, "catia_port_resolved_pairs")?;
    let mut scratch = ctx.reserve_scoped(0, OPERATION)?;
    let mut edges_by_port = BTreeMap::<u32, Vec<usize>>::new();
    for (edge, ports) in ctx.admit_iter(edge_ports, OPERATION)?.enumerate() {
        for &port in unique_ports(ports) {
            scratch.with_storage(|| {
                ctx.push_btree_group(
                    &mut edges_by_port,
                    port,
                    edge,
                    "catia_port_edge_entries",
                    OPERATION,
                )
            })?;
        }
    }
    let mut port_points = HashMap::<u32, usize>::new();
    let mut bind = |port_points: &mut HashMap<u32, usize>, port, point| {
        scratch.with_storage(|| bind_port_point(ctx, port_points, port, point))
    };

    let mut charged_steps = ordered_endpoint_pairs.iter().enumerate();
    while let Some((edge, ordered)) =
        ctx.next_charged(&mut charged_steps, "catia_port_ordered_seeds")?
    {
        let Some(ordered) = ordered else { continue };
        if resolved[edge].is_some_and(|pair| !same_unordered_pair(pair, *ordered)) {
            return Ok(None);
        }
        let ports = edge_ports[edge];
        if ports[0] == ports[1] && ordered[0] != ordered[1] {
            return Ok(None);
        }
        if !bind(&mut port_points, ports[0], ordered[0])?
            || !bind(&mut port_points, ports[1], ordered[1])?
        {
            return Ok(None);
        }
        resolved[edge] = Some(*ordered);
    }

    // A port whose resolved rows share exactly one point is bound to it.
    let mut charged_steps = edges_by_port.iter();
    while let Some((&port, edges)) = ctx.next_charged(&mut charged_steps, OPERATION)? {
        let mut common: Option<([usize; 2], usize)> = None;
        for &edge in ctx.admit_iter(edges, OPERATION)? {
            let Some(pair) = resolved[edge] else { continue };
            let points = if pair[0] == pair[1] {
                ([pair[0], pair[0]], 1)
            } else {
                (pair, 2)
            };
            common = Some(match common {
                None => points,
                Some((current, count)) => {
                    let mut shared = [0; 2];
                    let mut shared_count = 0;
                    for &point in &current[..count] {
                        if points.0[..points.1].contains(&point) {
                            shared[shared_count] = point;
                            shared_count += 1;
                        }
                    }
                    (shared, shared_count)
                }
            });
        }
        if let Some((points, 1)) = common {
            if !bind(&mut port_points, port, points[0])? {
                return Ok(None);
            }
        }
    }

    let mut queue = scratch.with_storage(|| {
        let mut queue = std::collections::VecDeque::new();
        for edge in ctx.admit_iter(&(0..edge_ports.len()), "catia_edge_port_initial_queue")? {
            ctx.push_back(&mut queue, edge, "catia_edge_port_initial_queue")?;
        }
        Ok::<_, CodecError>(queue)
    })?;
    let mut queued = scratch
        .with_storage(|| ctx.alloc_filled(edge_ports.len(), true, "catia_edge_port_queue"))?;
    while let Some(edge) = queue.pop_front() {
        ctx.charge_work(1, "catia_missing_edge_iteration")?;
        queued[edge] = false;
        let ports = edge_ports[edge];
        let bound = |port: &u32| -> Result<Option<usize>, CodecError> {
            Ok(ctx
                .get_hash_map(&port_points, port, "catia_port_propagated_points")?
                .copied())
        };
        let inserted = if let Some([left, right]) = resolved[edge] {
            match (bound(&ports[0])?, bound(&ports[1])?) {
                (Some(point), None) if point == left => Some((ports[1], right)),
                (Some(point), None) if point == right => Some((ports[1], left)),
                (None, Some(point)) if point == left => Some((ports[0], right)),
                (None, Some(point)) if point == right => Some((ports[0], left)),
                (Some(_), None) | (None, Some(_)) => return Ok(None),
                (Some(left_point), Some(right_point))
                    if !same_unordered_pair([left_point, right_point], [left, right]) =>
                {
                    return Ok(None);
                }
                _ => None,
            }
        } else {
            None
        };
        if let Some((port, point)) = inserted {
            scratch.with_storage(|| {
                ctx.insert_hash_map(
                    &mut port_points,
                    port,
                    point,
                    "catia_port_propagated_points",
                )
            })?;
        }
        if let (Some(left), Some(right)) = (
            ctx.get_hash_map(&port_points, &ports[0], "catia_port_propagated_points")?
                .copied(),
            ctx.get_hash_map(&port_points, &ports[1], "catia_port_propagated_points")?
                .copied(),
        ) {
            if ports[0] == ports[1] || left != right {
                if resolved[edge].is_some_and(|pair| !same_unordered_pair(pair, [left, right])) {
                    return Ok(None);
                }
                resolved[edge] = Some([left, right]);
            }
        }
        if let Some((port, _)) = inserted {
            let Some(neighbors) = ctx.get_btree_map(&edges_by_port, &port, OPERATION)? else {
                return Ok(None);
            };
            for &neighbor in ctx.admit_iter(neighbors, OPERATION)? {
                if !queued[neighbor] {
                    queued[neighbor] = true;
                    scratch.with_storage(|| {
                        ctx.push_back(&mut queue, neighbor, "catia_edge_port_neighbor_queue")
                    })?;
                }
            }
        }
    }
    let (resolved_rows, _resolved_storage) =
        ctx.with_scoped_storage("catia_port_resolved_port_rows", || {
            let mut resolved_ports = Vec::new();
            let mut resolved_candidates = Vec::new();
            for (ports, pair) in ctx
                .admit_iter(edge_ports, "catia_port_resolved_port_rows")?
                .zip(&resolved)
            {
                if let Some(pair) = *pair {
                    ctx.push_vec(&mut resolved_ports, *ports, "catia_port_resolved_port_rows")?;
                    ctx.push_vec(
                        &mut resolved_candidates,
                        ctx.alloc_filled(1, pair, "catia_port_resolved_candidate_pair")?,
                        "catia_port_resolved_candidate_rows",
                    )?;
                }
            }
            Ok::<_, CodecError>((resolved_ports, resolved_candidates))
        })?;
    let (resolved_ports, resolved_candidates) = resolved_rows;
    let (viable, _viability_storage) = ctx.with_scoped_storage(
        "catia_port_resolved_viability",
        || -> Result<bool, CodecError> {
            Ok(edge_port_candidate_assignment(
                ctx,
                &resolved_ports,
                &resolved_candidates,
                false,
                true,
            )?
            .is_some())
        },
    )?;
    if !viable {
        return Ok(None);
    }
    Ok(Some(resolved))
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
    let mut scratch = ctx.reserve_scoped(0, "catia_ordered_seed_deferred_copy")?;
    let mut effective_deferred = scratch
        .with_storage(|| ctx.copy_slice(deferred_edges, "catia_ordered_seed_deferred_copy"))?;
    if !expand_deferred_edge_port_components(ctx, edge_ports, &mut effective_deferred)? {
        return Ok(None);
    }
    let mut masked_pairs =
        scratch.with_storage(|| ctx.copy_slice(endpoint_pairs, "catia_ordered_seed_pair_copy"))?;
    for (edge, &deferred) in ctx
        .admit_iter(&effective_deferred, "catia_ordered_seed_pair_copy")?
        .enumerate()
    {
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
    const OPERATION: &str = "catia_partial_known_port_rows";
    if edge_ports.len() != endpoint_pairs.len() {
        return Ok(None);
    }
    if !ordered_endpoint_pairs.is_empty() && ordered_endpoint_pairs.len() != endpoint_pairs.len() {
        return Ok(None);
    }
    let mut resolved = ctx.copy_slice(endpoint_pairs, "catia_partial_port_resolved_pairs")?;
    let mut charged_steps = ordered_endpoint_pairs.iter().enumerate();
    while let Some((edge, ordered)) =
        ctx.next_charged(&mut charged_steps, "catia_partial_port_resolved_pairs")?
    {
        let Some(ordered) = ordered else { continue };
        if resolved[edge].is_some_and(|pair| !same_unordered_pair(pair, *ordered)) {
            return Ok(None);
        }
        resolved[edge] = Some(*ordered);
    }
    let mut scratch = ctx.reserve_scoped(0, OPERATION)?;
    let mut known = Vec::new();
    for (edge, ports) in ctx.admit_iter(edge_ports, OPERATION)?.enumerate() {
        if let Some(ports) = ports {
            scratch.with_storage(|| ctx.push_vec(&mut known, (edge, *ports), OPERATION))?;
        }
    }
    if known.is_empty() {
        return Ok(Some(resolved));
    }
    let (rows, _rows_storage) = ctx.with_scoped_storage("catia_partial_port_rows", || {
        let mut ports = ctx.collection_vec(known.len(), "catia_partial_port_rows")?;
        let mut pairs = ctx.collection_vec(known.len(), "catia_partial_pair_rows")?;
        let mut ordered = ctx.collection_vec(known.len(), "catia_partial_ordered_rows")?;
        for &(edge, port) in ctx.admit_iter(&known, "catia_partial_port_rows")? {
            ports.push(port);
            pairs.push(resolved[edge]);
            ordered.push(ordered_endpoint_pairs.get(edge).copied().flatten());
        }
        Ok::<_, CodecError>((ports, pairs, ordered))
    })?;
    let (ports, pairs, ordered) = rows;
    let (propagated, _propagated_storage) = ctx
        .with_scoped_storage("catia_partial_propagation_workspace", || {
            propagate_edge_port_points_with_ordered_seeds(ctx, &ports, &pairs, &ordered)
        })?;
    let Some(propagated) = propagated else {
        return Ok(None);
    };
    for ((edge, _), pair) in ctx
        .admit_iter(&known, "catia_partial_port_resolved_pairs")?
        .zip(propagated)
    {
        resolved[*edge] = pair;
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

type PortBindingUndo = [Option<(u32, usize)>; 2];

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
    map_storage: cadmpeg_core::decode::ScopedReservation<'a>,
    outcome_storage: cadmpeg_core::decode::ScopedReservation<'a>,
}

impl<'a, 'b> PortCandidateSearch<'a, 'b> {
    /// The orientations of a candidate pair that agree with the bound ports
    /// (and, under a point bijection, with the bound points).
    fn compatible(
        &self,
        edge: usize,
        pair: [usize; 2],
    ) -> Result<[Option<[usize; 2]>; 2], CodecError> {
        const OPERATION: &str = "catia_port_search_points";
        let mut oriented = [
            Some(pair),
            (pair[0] != pair[1]).then_some([pair[1], pair[0]]),
        ];
        for option in &mut oriented {
            let Some(points) = *option else {
                continue;
            };
            let mut compatible =
                (self.ports[edge][0] == self.ports[edge][1]) == (points[0] == points[1]);
            for (&port, point) in self.ports[edge].iter().zip(points) {
                if !compatible {
                    break;
                }
                compatible = self
                    .ctx
                    .get_hash_map(&self.port_points, &port, OPERATION)?
                    .is_none_or(|stored| *stored == point)
                    && (!self.mode.enforces_point_bijection()
                        || self
                            .ctx
                            .get_hash_map(&self.point_ports, &point, OPERATION)?
                            .is_none_or(|stored| *stored == port));
            }
            if !compatible {
                *option = None;
            }
        }
        Ok(oriented)
    }

    fn assign(&mut self, edge: usize, points: [usize; 2]) -> Result<PortBindingUndo, CodecError> {
        let mut inserted = [None; 2];
        for (slot, (&port, point)) in self.ports[edge].iter().zip(points).enumerate() {
            if !self.ctx.contains_key_hash_map(
                &self.port_points,
                &port,
                "catia_port_search_points",
            )? {
                self.map_storage.with_storage(|| {
                    self.ctx.insert_hash_map(
                        &mut self.port_points,
                        port,
                        point,
                        "catia_port_search_points",
                    )
                })?;
                if self.mode.enforces_point_bijection() {
                    self.map_storage.with_storage(|| {
                        self.ctx.insert_hash_map(
                            &mut self.point_ports,
                            point,
                            port,
                            "catia_port_search_reverse_points",
                        )
                    })?;
                }
                inserted[slot] = Some((port, point));
            }
        }
        self.edge_pairs[edge] = Some(points);
        Ok(inserted)
    }

    fn unassign(&mut self, edge: usize, inserted: PortBindingUndo) -> Result<(), CodecError> {
        self.edge_pairs[edge] = None;
        for (port, point) in inserted.into_iter().flatten() {
            self.ctx
                .remove_hash_map(&mut self.port_points, &port, "catia_port_search_points")?;
            if self.mode.enforces_point_bijection() {
                self.ctx.remove_hash_map(
                    &mut self.point_ports,
                    &point,
                    "catia_port_search_reverse_points",
                )?;
            }
        }
        Ok(())
    }

    fn rollback(&mut self, propagated: Vec<(usize, PortBindingUndo)>) -> Result<(), CodecError> {
        for (edge, inserted) in self
            .ctx
            .admit_iter(propagated, "catia_port_search_propagated")?
            .rev()
        {
            self.unassign(edge, inserted)?;
        }
        Ok(())
    }

    fn search(&mut self) -> Result<(), CodecError> {
        // Native-port binding precedes geometric incidence fallback but can
        // still contain symmetric coordinate assignments. Ambiguity beyond
        // this bound is retained for later paths rather than partially bound.
        const MAX_STATES: usize = 1_024;
        const OPERATION: &str = "catia_port_candidate_search";
        let ctx = self.ctx;
        if self.outcome.is_closed()
            || (!self.mode.requires_unique() && matches!(self.outcome, SearchOutcome::Solved(_)))
        {
            return Ok(());
        }
        let _depth = ctx.enter_nested(OPERATION)?;
        ctx.charge_work(1, OPERATION)?;
        let mut propagated = Vec::new();
        let mut propagated_storage = ctx.reserve_scoped(0, "catia_port_search_propagated")?;
        // Assign every edge with one compatible orientation until none is
        // forced, then branch on the edge with the fewest.
        let branch = loop {
            let mut best = None;
            let mut progress = false;
            let mut incomplete = false;
            let mut charged_steps = 0..self.ports.len();
            while let Some(edge) =
                ctx.next_charged(&mut charged_steps, "catia_missing_edge_iteration")?
            {
                if self.edge_pairs[edge].is_some() {
                    continue;
                }
                incomplete = true;
                let mut first = None;
                let mut count = 0usize;
                for &pair in ctx.admit_iter(&self.candidates[edge], OPERATION)? {
                    for points in self.compatible(edge, pair)?.into_iter().flatten() {
                        count += 1;
                        first.get_or_insert(points);
                    }
                }
                let Some(first) = first else {
                    self.rollback(propagated)?;
                    return Ok(());
                };
                if count > 1 {
                    if best.is_none_or(|(stored, _)| count < stored) {
                        best = Some((count, edge));
                    }
                    continue;
                }
                let inserted = self.assign(edge, first)?;
                propagated_storage.with_storage(|| {
                    ctx.push_vec(
                        &mut propagated,
                        (edge, inserted),
                        "catia_port_search_propagated",
                    )
                })?;
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
            // No branch remains only when every edge is assigned.
            match &self.outcome {
                SearchOutcome::Open => {
                    let candidate = self.outcome_storage.with_storage(|| {
                        ctx.collect_options(
                            self.edge_pairs.iter().copied(),
                            "catia_port_search_solution",
                        )
                    })?;
                    if let Some(candidate) = candidate {
                        self.outcome = SearchOutcome::Solved(candidate);
                    }
                }
                SearchOutcome::Solved(previous) => {
                    let equivalent = previous.len() == self.edge_pairs.len()
                        && ctx.all_by(
                            previous.iter().zip(&self.edge_pairs),
                            |(&left, &right)| {
                                Ok(right.is_some_and(|right| {
                                    port_candidate_pair_key(left) == port_candidate_pair_key(right)
                                }))
                            },
                            "catia_port_search_solution",
                        )?;
                    if !equivalent {
                        self.outcome = SearchOutcome::Ambiguous;
                        self.outcome_storage =
                            ctx.reserve_scoped(0, "catia_port_search_solution")?;
                    }
                }
                SearchOutcome::Ambiguous | SearchOutcome::Exhausted => {}
            }
            self.rollback(propagated)?;
            return Ok(());
        };
        if self.states >= MAX_STATES {
            self.outcome.exhaust();
        } else {
            self.states += 1;
            let mut candidates = 0..self.candidates[edge].len();
            'candidates: while let Some(candidate) = ctx.next_charged(&mut candidates, OPERATION)? {
                for points in self
                    .compatible(edge, self.candidates[edge][candidate])?
                    .into_iter()
                    .flatten()
                {
                    let inserted = self.assign(edge, points)?;
                    self.search()?;
                    self.unassign(edge, inserted)?;
                    if self.outcome.is_closed()
                        || (!self.mode.requires_unique()
                            && matches!(self.outcome, SearchOutcome::Solved(_)))
                    {
                        break 'candidates;
                    }
                }
            }
        }
        self.rollback(propagated)
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
    for pair in ctx.admit_iter(&mut pairs, "catia missing edge port pair sort")? {
        *pair = port_candidate_pair_key(*pair);
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
    let mut scratch = ctx.reserve_scoped(0, "catia_candidate_settled_edges")?;
    let mut effective_deferred =
        scratch.with_storage(|| ctx.copy_slice(deferred_edges, "catia_candidate_deferred_copy"))?;
    if !expand_deferred_edge_port_components(ctx, ports, &mut effective_deferred)? {
        return Ok(None);
    }
    let mut settled = Vec::new();
    let mut settled_ports = Vec::new();
    let mut settled_candidates = Vec::new();
    for (edge, &deferred) in ctx
        .admit_iter(&effective_deferred, "catia_candidate_settled_edges")?
        .enumerate()
    {
        if !deferred {
            scratch.with_storage(|| {
                ctx.push_vec(&mut settled, edge, "catia_candidate_settled_edges")?;
                ctx.push_vec(
                    &mut settled_ports,
                    ports[edge],
                    "catia_candidate_settled_ports",
                )?;
                ctx.push_vec(
                    &mut settled_candidates,
                    ctx.copy_slice(&candidates[edge], "catia_candidate_settled_pairs")?,
                    "catia_candidate_settled_rows",
                )
            })?;
        }
    }
    let Some(settled_pairs) = scratch.with_storage(|| {
        unique_mesh_edge_port_candidate_pairs(ctx, &settled_ports, &settled_candidates)
    })?
    else {
        return Ok(None);
    };
    let mut resolved = ctx.alloc_filled(candidates.len(), None, "catia_candidate_resolved")?;
    for (&edge, pair) in ctx
        .admit_iter(&settled, "catia_candidate_resolved")?
        .zip(settled_pairs)
    {
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
    const OPERATION: &str = "catia_port_dependency_ports";
    if ports.len() != candidates.len()
        || ctx.any_by(candidates, |pairs| Ok(pairs.is_empty()), OPERATION)?
    {
        return Ok(None);
    }
    let mode = match (require_unique, enforce_point_bijection) {
        (false, true) => PortCandidateSearchMode::FirstNative,
        (true, true) => PortCandidateSearchMode::UniqueNative,
        (true, false) => PortCandidateSearchMode::UniqueMesh,
        (false, false) => return Ok(None),
    };
    let mut dependencies = UnionFind::charged(ctx, ports.len(), "catia_port_dependency_union")?;
    let mut scratch = ctx.reserve_scoped(0, OPERATION)?;
    let mut edge_by_port = HashMap::new();
    let mut edge_by_point = HashMap::new();
    for (edge, edge_ports) in ctx.admit_iter(ports, OPERATION)?.enumerate() {
        for &port in edge_ports {
            if let Some(previous) = scratch
                .with_storage(|| ctx.insert_hash_map(&mut edge_by_port, port, edge, OPERATION))?
            {
                dependencies.union(ctx, previous, edge)?;
            }
        }
        if enforce_point_bijection {
            for &point in ctx
                .admit_iter(&candidates[edge], "catia_port_dependency_points")?
                .flatten()
            {
                if let Some(previous) = scratch.with_storage(|| {
                    ctx.insert_hash_map(
                        &mut edge_by_point,
                        point,
                        edge,
                        "catia_port_dependency_points",
                    )
                })? {
                    dependencies.union(ctx, previous, edge)?;
                }
            }
        }
    }
    // Components open in first-edge order and list their edges ascending.
    let mut component_of_root = scratch
        .with_storage(|| ctx.alloc_filled(ports.len(), None, "catia_port_component_roots"))?;
    let mut components: Vec<Vec<usize>> = Vec::new();
    for edge in ctx.admit_iter(&(0..ports.len()), "catia_port_component_edges")? {
        let root = dependencies.find(ctx, edge)?;
        let component = if let Some(component) = component_of_root[root] {
            component
        } else {
            let component = components.len();
            scratch.with_storage(|| {
                ctx.push_vec(&mut components, Vec::new(), "catia_port_components")
            })?;
            component_of_root[root] = Some(component);
            component
        };
        scratch.with_storage(|| {
            ctx.push_vec(
                &mut components[component],
                edge,
                "catia_port_component_edges",
            )
        })?;
    }
    let mut solution =
        scratch.with_storage(|| ctx.alloc_filled(ports.len(), None, "catia_edge_port_solution"))?;
    let mut charged_steps = components.iter();
    while let Some(component) =
        ctx.next_charged(&mut charged_steps, "catia_port_component_ports")?
    {
        let (rows, _rows_storage) =
            ctx.with_scoped_storage("catia_port_component_ports", || {
                let mut component_ports =
                    ctx.collection_vec(component.len(), "catia_port_component_ports")?;
                let mut component_candidates =
                    ctx.collection_vec(component.len(), "catia_port_component_candidates")?;
                for &edge in ctx.admit_iter(component, "catia_port_component_ports")? {
                    component_ports.push(ports[edge]);
                    component_candidates.push(
                        ctx.copy_slice(&candidates[edge], "catia_port_component_candidate_pairs")?,
                    );
                }
                Ok::<_, CodecError>((component_ports, component_candidates))
            })?;
        let (component_ports, component_candidates) = rows;
        let mut search_storage = ctx.reserve_scoped(0, "catia_edge_port_pairs")?;
        let mut search = search_storage.with_storage(|| {
            Ok::<_, CodecError>(PortCandidateSearch {
                ctx,
                ports: &component_ports,
                candidates: &component_candidates,
                port_points: HashMap::new(),
                point_ports: HashMap::new(),
                edge_pairs: ctx.alloc_filled(component.len(), None, "catia_edge_port_pairs")?,
                outcome: SearchOutcome::Open,
                states: 0,
                mode,
                map_storage: ctx.reserve_scoped(0, "catia_port_search_points")?,
                outcome_storage: ctx.reserve_scoped(0, "catia_port_search_solution")?,
            })
        })?;
        search_storage.with_storage(|| search.search())?;
        let SearchOutcome::Solved(component_solution) = search.outcome else {
            return Ok(None);
        };
        for (&edge, pair) in ctx
            .admit_iter(component, "catia_edge_port_solution")?
            .zip(component_solution)
        {
            solution[edge] = Some(pair);
        }
    }
    let mut result = ctx.collection_vec(ports.len(), "catia_port_assignment_result")?;
    let mut charged_steps = solution.iter();
    while let Some(pair) = ctx.next_charged(&mut charged_steps, "catia_port_assignment_result")? {
        let Some(pair) = *pair else {
            return Ok(None);
        };
        result.push(pair);
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
        if !ctx.contains_key_hash_map(seen, &handle, "catia_motif_port_points")? {
            let next = seen.len();
            ctx.insert_hash_map(seen, handle, next, "catia_motif_port_points")?;
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
        ctx.charge_work(1, "catia_missing_edge_iteration")?;
        let Some((first, last)) = columns(&trims[at]) else {
            return Ok(None);
        };
        emit_column(ctx, &mut seen, first)?;
        emit_column(ctx, &mut seen, last)?;
        at += 1;
    }
    while at < trims.len() {
        ctx.charge_work(1, "catia_missing_edge_iteration")?;
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
