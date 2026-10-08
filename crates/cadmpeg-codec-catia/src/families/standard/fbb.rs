//! Byte-level parsing for standard nested CATIA V5 B-rep (`FBB`) streams:
//! edge/vertex tables, trim records, packet triangles, and face parsers.

type FbbEdgeTableOutput = Result<Option<(Vec<EdgeRow>, Vec<usize>, usize, usize)>, CodecError>;
type ScopedEdgeTableOutput = Result<Option<(Vec<EdgeRow>, Vec<usize>, usize)>, CodecError>;

use cadmpeg_core::decode::{DecodeContext, View, WorkBudget};
use cadmpeg_core::CodecError;
use cadmpeg_ir::features::FinitePoint3;
use cadmpeg_ir::scalar::FiniteReal;
use cadmpeg_ir::units::FiniteVector;

use crate::families::standard::topology::{
    reconstruct, reconstruct_incidence, reconstruct_incidence_with_edge_classes_and_mesh,
    BoundaryDraft, CoedgeUse, EdgeBoundaryLayout, EdgeRow, StandardIncidenceEvidence,
    StandardTopologyDraft, TrimRecord,
};
use crate::families::standard::trim_packet::TrimPacket;
use crate::layout::fbb_face_row as fbb_row;
use crate::solve::incidence::{reconstruct_incidence_candidates, IncidenceEndpointDomains};
use crate::solve::mesh_quotient::MeshQuotient;
use crate::solve::missing_edge::{expand_deferred_edge_port_components, motif_port_points};
use crate::solve::union_find::UnionFind;
use std::collections::{BTreeMap, HashMap, HashSet};
use std::num::NonZeroUsize;
use std::sync::Arc;

pub(super) const EDGE_DELIMITER: [u8; 8] = [0x10, 0x24, 0x04, 0xff, 0xff, 0x00, 0x00, 0x00];
const VERTEX_RECORD_BYTES: usize = 3 + 3 * size_of::<f32>();
const TRIM_KINDS: [u8; 14] = [
    0x41, 0x42, 0x43, 0x44, 0x45, 0x46, 0x47, 0x49, 0x4a, 0x4b, 0x4c, 0x4d, 0x4e, 0x4f,
];

// A unit vector is stored as three binary32 values. Rounding a unit
// direction to binary32 changes its squared norm by less than 2.1e-7; this
// bound leaves room for binary32 arithmetic used by a writer without
// admitting a materially non-unit frame.
const FRAME_VECTOR_NORM2_TOLERANCE: f64 = 1.0e-6;

/// Number of face rows in the governing standard topology spine. The spine is
/// the unique largest contiguous stride-eight FBB run; shorter marker runs are
/// not members of this face population. Equal-largest runs leave ownership
/// unresolved.
pub(super) fn standard_face_count(
    ctx: &DecodeContext<'_>,
    bytes: &[u8],
) -> Result<Option<usize>, CodecError> {
    let Some(selected) = selected_standard_run(ctx, bytes)? else {
        return Ok(None);
    };
    let layouts = fbb_population_layouts(ctx, bytes)?;
    if layouts.is_empty()
        || ctx.any_by(
            &layouts,
            |layout| Ok(layout.face_run == selected),
            "catia_standard_iteration",
        )?
    {
        Ok(Some(selected.face_count()))
    } else {
        Ok(None)
    }
}

/// Number of physical edge rows in the admitted standard edge-table form.
///
/// The count is available without solving trim incidence or mesh topology,
/// so it can gate the independent `0x60` support-table walk.
pub(crate) fn standard_edge_count(
    ctx: &DecodeContext<'_>,
    bytes: &[u8],
) -> Result<Option<usize>, CodecError> {
    let Some(face_run) = selected_standard_run(ctx, bytes)? else {
        return Ok(None);
    };
    let after_faces = face_run.after_faces();
    Ok(parse_standard_edge_tables(ctx, bytes, after_faces)?.map(|(rows, _)| rows.len()))
}

/// Number of physical edge rows in the width-selected FBB-only tables.
pub(crate) fn fbb_only_edge_count(
    ctx: &DecodeContext<'_>,
    bytes: &[u8],
) -> Result<Option<usize>, CodecError> {
    let Some(face_run) = largest_fbb_run(ctx, bytes)? else {
        return Ok(None);
    };
    let after_faces = face_run.after_faces();
    Ok(parse_fbb_edge_tables(ctx, bytes, after_faces)?.map(|(rows, _, _, _)| rows.len()))
}

/// RGBA display color for each positional standard face row.
pub(crate) fn standard_face_colors(
    ctx: &DecodeContext<'_>,
    bytes: &[u8],
) -> Result<Option<Vec<[u8; 4]>>, CodecError> {
    let Some(face_run) = selected_standard_run(ctx, bytes)? else {
        return Ok(None);
    };
    let start = face_run.face_start();
    let after_faces = face_run.after_faces();
    let Some(marker) = bytes
        .get(start..start + fbb_row::ALPHA)
        .and_then(|bytes| <[u8; 4]>::try_from(bytes).ok())
    else {
        return Ok(None);
    };
    let Some(face_rows) = bytes.get(start..after_faces) else {
        return Ok(None);
    };
    let Some(row_width) = NonZeroUsize::new(fbb_row::LEN) else {
        return Ok(None);
    };
    let mut colors = Vec::new();
    for row in ctx
        .admit_iter(face_rows, "catia_fbb_face_color_rows")?
        .chunks(row_width)
    {
        if row[..fbb_row::ALPHA] != marker {
            return Ok(None);
        }
        ctx.push_vec(
            &mut colors,
            [
                row[fbb_row::RED],
                row[fbb_row::GREEN],
                row[fbb_row::BLUE],
                row[fbb_row::ALPHA],
            ],
            "catia_fbb_face_colors",
        )?;
    }
    Ok(Some(colors))
}

#[cfg(test)]
fn work_refusal_at(
    operation: &'static str,
    mut run: impl FnMut(&DecodeContext<'_>) -> Result<(), CodecError>,
) -> cadmpeg_core::decode::ResourceLimit {
    let mut saved_refusal = None;
    let result = crate::test_support::with_work_refusal(operation, |ctx| {
        let result = run(ctx);
        saved_refusal = ctx.resource_refusal();
        result
    });
    let Err(CodecError::ResourceLimit(refusal)) = result else {
        panic!("{operation}: work refusal required")
    };
    assert_eq!(Some(refusal), saved_refusal);
    assert_eq!(
        refusal.dimension,
        cadmpeg_core::decode::ResourceDimension::WorkUnits
    );
    assert_eq!(refusal.operation, operation);
    refusal
}

fn trim_frame_vectors(
    ctx: &DecodeContext<'_>,
    bytes: &[u8],
    face_start: usize,
    face_count: usize,
) -> Result<Option<Vec<Option<FiniteVector<3>>>>, CodecError> {
    let mut solutions = Vec::new();
    for width in [1, 2, 3] {
        if let Some(records) = parse_trim_chain(ctx, bytes, face_start, face_count, width)? {
            ctx.push_vec(&mut solutions, records, "catia_trim_width_solutions")?;
        }
    }
    let Ok([records]) = <[Vec<TrimRecord>; 1]>::try_from(solutions) else {
        return Ok(None);
    };
    let mut vectors = Vec::new();
    ctx.reserve_vec(&mut vectors, records.len(), "catia_trim_frame_vectors")?;
    for record in ctx.admit_iter(&records, "catia_trim_frame_vectors")? {
        vectors.push(record.frame_vector);
    }
    Ok(Some(vectors))
}

/// Unit frame vector for each positional standard trim packet. The result is
/// index-aligned with the expected face-roster population; packets without the
/// optional vector retain an empty slot. When every detected FBB population
/// has one unique trim chain and their concatenated length equals
/// `expected_face_count`, the result concatenates those population-local
/// vectors in source order; otherwise it uses the established
/// single-population selection.
pub(super) fn standard_face_frame_vectors(
    ctx: &DecodeContext<'_>,
    bytes: &[u8],
    expected_face_count: usize,
) -> Result<Vec<Option<FiniteVector<3>>>, CodecError> {
    let runs = crate::container::fbb_run_ranges(ctx, bytes)?;
    if runs.len() > 1 {
        let mut combined = Vec::new();
        let mut complete = true;
        for range in ctx.admit_iter(&runs, "catia_standard_iteration")? {
            let Some(vectors) =
                trim_frame_vectors(ctx, bytes, range.start, range.len() / fbb_row::LEN)?
            else {
                complete = false;
                break;
            };
            ctx.push_vec(&mut combined, vectors, "catia_trim_population_frames")?;
        }
        if complete {
            let mut combined_len = 0_usize;
            for population in ctx.admit_iter(&combined, "catia_standard_iteration")? {
                combined_len = combined_len.checked_add(population.len()).ok_or_else(|| {
                    ctx.refuse_codec_limit("catia_trim_population_frame_count", u64::MAX, u64::MAX)
                })?;
            }
            if combined_len == expected_face_count {
                let mut vectors = Vec::new();
                ctx.reserve_vec(
                    &mut vectors,
                    expected_face_count,
                    "catia_trim_combined_frames",
                )?;
                for mut population in ctx.admit_iter(combined, "catia_trim_combined_frames")? {
                    ctx.append_vec(&mut vectors, &mut population, "catia_trim_combined_frames")?;
                }
                return Ok(vectors);
            }
        }
    }
    let Some(face_run) = selected_standard_run(ctx, bytes)? else {
        return Ok(Vec::new());
    };
    Ok(
        trim_frame_vectors(ctx, bytes, face_run.face_start(), face_run.face_count())?
            .unwrap_or_default(),
    )
}

/// Return the counted vertex table of an admitted standard nested spine.
pub(super) fn standard_vertex_points(
    ctx: &DecodeContext<'_>,
    bytes: &[u8],
) -> Result<Option<Vec<FinitePoint3>>, CodecError> {
    let Some(face_run) = selected_standard_run(ctx, bytes)? else {
        return Ok(None);
    };
    let after_faces = face_run.after_faces();
    let Some((_, vertex_header)) = parse_standard_edge_tables(ctx, bytes, after_faces)? else {
        return Ok(None);
    };
    parse_vertex_points(ctx, bytes, vertex_header)
}

/// Coordinates from the counted vertex table following a complete FBB-only
/// edge-table walk.
pub(super) fn fbb_only_vertex_points(
    ctx: &DecodeContext<'_>,
    bytes: &[u8],
) -> Result<Option<Vec<FinitePoint3>>, CodecError> {
    let Some(face_run) = largest_fbb_run(ctx, bytes)? else {
        return Ok(None);
    };
    let after_faces = face_run.after_faces();
    let Some((_, _, vertex_header, _)) = parse_fbb_edge_tables(ctx, bytes, after_faces)? else {
        return Ok(None);
    };
    parse_vertex_points(ctx, bytes, vertex_header)
}

/// Parses the counted standard spine, positional trim packets, mesh boundary
/// cycles, physical edge uses, and port/corner vertex equivalence classes.
/// Returns `None` unless every positional face boundary is unambiguous.
pub(crate) fn parse_standard(
    ctx: &DecodeContext<'_>,
    bytes: &[u8],
) -> Result<Option<StandardTopologyDraft>, CodecError> {
    let Some(face_run) = selected_standard_run(ctx, bytes)? else {
        return Ok(None);
    };
    let face_start = face_run.face_start();
    let face_count = face_run.face_count();
    let after_faces = face_run.after_faces();
    let Some((edge_rows, vertex_header, handle_width)) =
        parse_standard_edge_tables_with_width(ctx, bytes, after_faces)?
    else {
        return Ok(None);
    };
    let Some(vertex_points) = parse_vertex_table(ctx, bytes, vertex_header)? else {
        return Ok(None);
    };
    let Some(trims) = parse_trim_chain(ctx, bytes, face_start, face_count, handle_width)? else {
        return Ok(None);
    };
    reconstruct(ctx, edge_rows, vertex_points, &trims)
}

/// Reconstruct regular-motif standard topology by replaying the trim packet's
/// vertex-allocation program. The allocation is accepted only when it covers
/// the complete vertex table and reproduces every supplied circle endpoint
/// anchor.
pub(super) fn parse_standard_motif(
    ctx: &DecodeContext<'_>,
    bytes: &[u8],
    edge_faces: &[[usize; 2]],
    circle_anchors: &[Option<[usize; 2]>],
) -> Result<Option<StandardTopologyDraft>, CodecError> {
    let Some(face_run) = selected_standard_run(ctx, bytes)? else {
        return Ok(None);
    };
    let face_start = face_run.face_start();
    let face_count = face_run.face_count();
    let after_faces = face_run.after_faces();
    let Some((edge_rows, vertex_header, handle_width)) =
        parse_standard_edge_tables_with_width(ctx, bytes, after_faces)?
    else {
        return Ok(None);
    };
    let Some(vertex_points) = parse_vertex_table(ctx, bytes, vertex_header)? else {
        return Ok(None);
    };
    if edge_rows.len() != edge_faces.len() || edge_rows.len() != circle_anchors.len() {
        return Ok(None);
    }
    let Some(trims) = parse_trim_chain(ctx, bytes, face_start, face_count, handle_width)? else {
        return Ok(None);
    };
    let Some(port_points) = motif_port_points(ctx, &trims, vertex_points.len())? else {
        return Ok(None);
    };
    let mut edge_points = Vec::new();
    for row in ctx.admit_iter(&edge_rows, "catia_motif_edge_points")? {
        let port = |handle: Option<&u32>| -> Result<Option<usize>, CodecError> {
            let Some(handle) = handle else {
                return Ok(None);
            };
            Ok(ctx
                .get_hash_map(&port_points, handle, "catia_motif_port_points")?
                .copied())
        };
        let Some(first) = port(row.handles().first())? else {
            return Ok(None);
        };
        let Some(last) = port(row.handles().last())? else {
            return Ok(None);
        };
        ctx.push_vec(&mut edge_points, [first, last], "catia_motif_edge_points")?;
    }
    let unordered = |[start, end]: [usize; 2]| [start.min(end), start.max(end)];
    for (points, anchor) in ctx
        .admit_iter(&edge_points, "catia_standard_iteration")?
        .zip(circle_anchors)
    {
        if anchor.is_some_and(|anchor| unordered(*points) != unordered(anchor)) {
            return Ok(None);
        }
    }
    reconstruct_incidence(
        ctx,
        edge_rows,
        vertex_points,
        edge_faces,
        &edge_points,
        face_count,
    )
}

/// Reconstruct standard topology while treating equal curve-class identifiers
/// as interchangeable serialized edge rows during incidence-slot completion.
pub(super) fn parse_standard_endpoints_with_edge_classes(
    ctx: &DecodeContext<'_>,
    bytes: &[u8],
    edge_faces: &[[usize; 2]],
    edge_points: &[[usize; 2]],
    edge_classes: Option<&[usize]>,
) -> Result<Option<StandardTopologyDraft>, CodecError> {
    let Some(face_run) = selected_standard_run(ctx, bytes)? else {
        return Ok(None);
    };
    let face_count = face_run.face_count();
    let after_faces = face_run.after_faces();
    let Some((edge_rows, vertex_header)) = parse_standard_edge_tables(ctx, bytes, after_faces)?
    else {
        return Ok(None);
    };
    let Some(vertex_points) = parse_vertex_table(ctx, bytes, vertex_header)? else {
        return Ok(None);
    };
    if edge_rows.len() != edge_faces.len()
        || edge_rows.len() != edge_points.len()
        || edge_classes.is_some_and(|classes| classes.len() != edge_rows.len())
    {
        return Ok(None);
    }
    if ctx.any_by(
        edge_points,
        |points| Ok(points.iter().any(|point| *point >= vertex_points.len())),
        "catia_standard_iteration",
    )? {
        return Ok(None);
    }
    reconstruct_incidence_with_edge_classes_and_mesh(
        ctx,
        edge_rows,
        vertex_points,
        edge_faces,
        edge_points,
        face_count,
        StandardIncidenceEvidence {
            edge_classes,
            mesh_bytes: Some(bytes),
        },
    )
}

/// Collapse equal endpoint identities and propagate correlated edge-pair
/// support to a fixpoint. Only serialized pairs supported by both resulting
/// port domains are retained.
pub(super) fn prune_edge_candidates_by_port_domains(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    edge_ports: &[[u32; 2]],
    edge_candidates: &[Vec<[usize; 2]>],
) -> Result<Option<Vec<Vec<[usize; 2]>>>, cadmpeg_core::CodecError> {
    prune_edge_candidates_by_port_domains_with_deferred(ctx, edge_ports, edge_candidates, &[])
}

/// Apply trim-port equality to endpoint candidates whose duplicate face slot
/// is settled. Rows with an open duplicate-face domain do not contribute their
/// candidate set to port-domain propagation; their candidates are filtered by
/// the settled neighbouring ports after that propagation completes.
pub(super) fn prune_edge_candidates_by_port_domains_with_deferred(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    edge_ports: &[[u32; 2]],
    edge_candidates: &[Vec<[usize; 2]>],
    deferred_edges: &[bool],
) -> Result<Option<Vec<Vec<[usize; 2]>>>, cadmpeg_core::CodecError> {
    if edge_ports.len() != edge_candidates.len()
        || ctx.any_by(
            edge_candidates,
            |candidates| Ok(candidates.is_empty()),
            "catia_standard_iteration",
        )?
    {
        return Ok(None);
    }
    if !deferred_edges.is_empty() && deferred_edges.len() != edge_candidates.len() {
        return Ok(None);
    }
    let mut effective_deferred = if deferred_edges.is_empty() {
        ctx.alloc_filled(
            edge_candidates.len(),
            false,
            "catia_port_effective_deferred",
        )?
    } else {
        ctx.copy_slice(deferred_edges, "catia_port_effective_deferred")?
    };
    if !expand_deferred_edge_port_components(ctx, edge_ports, &mut effective_deferred)? {
        return Ok(None);
    }
    let is_deferred = |edge: usize| effective_deferred[edge];
    let mut all_points = HashSet::new();
    for candidates in ctx.admit_iter(edge_candidates, "catia_standard_iteration")? {
        for pair in ctx.admit_iter(candidates, "catia_standard_iteration")? {
            for point in *pair {
                ctx.insert_hash_set(&mut all_points, point, "catia_port_all_points")?;
            }
        }
    }
    // Every deferred edge shares the one all-points domain; the quotient
    // never edits a domain in place.
    let all_points = Arc::new(all_points);
    let mut domains = Vec::new();
    for (edge, candidates) in ctx
        .admit_iter(edge_candidates, "catia_standard_iteration")?
        .enumerate()
    {
        let domain = if is_deferred(edge) {
            Arc::clone(&all_points)
        } else {
            let mut points = HashSet::new();
            for pair in ctx.admit_iter(candidates, "catia_standard_iteration")? {
                for point in *pair {
                    ctx.insert_hash_set(&mut points, point, "catia_port_candidate_domain")?;
                }
            }
            Arc::new(points)
        };
        ctx.push_vec(&mut domains, domain.clone(), "catia_port_domains")?;
        ctx.push_vec(&mut domains, domain, "catia_port_domains")?;
    }
    let mut quotient = MeshQuotient::new_charged(ctx, domains)?;
    let mut node_by_port = HashMap::new();
    for (edge, ports) in ctx
        .admit_iter(edge_ports, "catia_standard_iteration")?
        .enumerate()
    {
        for (endpoint, port) in ports.iter().copied().enumerate() {
            let node = edge * 2 + endpoint;
            if let Some(&previous) = ctx.get_hash_map(&node_by_port, &port, "catia_port_nodes")? {
                if quotient.merge_charged(ctx, previous, node)?.is_none() {
                    return Ok(None);
                }
            } else {
                ctx.insert_hash_map(&mut node_by_port, port, node, "catia_port_nodes")?;
            }
        }
    }
    let mut constrained_candidates = Vec::new();
    ctx.reserve_vec(
        &mut constrained_candidates,
        edge_candidates.len(),
        "catia_port_constrained_edges",
    )?;
    for candidates in ctx.admit_iter(edge_candidates, "catia_standard_iteration")? {
        constrained_candidates.push(ctx.copy_slice(candidates, "catia_port_constrained_pairs")?);
    }
    for (edge, candidates) in ctx
        .admit_iter(&mut constrained_candidates, "catia_port_constrained_edges")?
        .enumerate()
    {
        if is_deferred(edge) {
            ctx.clear_vec(candidates, "catia_port_constrained_edges")?;
        }
    }
    if !quotient.edge_domains_viable(ctx, &constrained_candidates)? {
        return Ok(None);
    }
    let mut result = Vec::new();
    for (edge, candidates) in ctx
        .admit_iter(edge_candidates, "catia_standard_iteration")?
        .enumerate()
    {
        let left = quotient.find(ctx, edge * 2)?;
        let right = quotient.find(ctx, edge * 2 + 1)?;
        let mut filtered = Vec::new();
        let contains = |root: usize, point: usize| {
            ctx.contains_hash_set(
                &quotient.domains()[root],
                &point,
                "catia_port_domain_lookup",
            )
        };
        for &pair in ctx.admit_iter(candidates, "catia_standard_iteration")? {
            let supported = if left == right {
                pair[0] == pair[1] && contains(left, pair[0])?
            } else {
                (contains(left, pair[0])? && contains(right, pair[1])?)
                    || (contains(left, pair[1])? && contains(right, pair[0])?)
            };
            if supported {
                ctx.push_vec(
                    &mut filtered,
                    [pair[0].min(pair[1]), pair[0].max(pair[1])],
                    "catia_port_filtered_pairs",
                )?;
            }
        }
        ctx.sort_unstable_by(
            &mut filtered,
            |value| value,
            Ord::cmp,
            "catia standard port filtered pairs sort",
        )?;
        ctx.dedup_vec(&mut filtered, "catia standard port filtered pairs dedup")?;
        if filtered.is_empty() {
            return Ok(None);
        }
        ctx.push_vec(&mut result, filtered, "catia_port_filtered_edges")?;
    }
    Ok(Some(result))
}

/// Reconstruct standard topology while resolving edges that have multiple
/// geometrically valid endpoint pairs. Candidate pairs and edge rows use their
/// serialized order as the stable gauge when equivalent assignments permute
/// indistinguishable line rows. The selected assignment must close every face
/// cycle and satisfy radial orientation. Search charges the supplied topology
/// phase budget.
pub(super) fn parse_standard_endpoint_candidates(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    bytes: &[u8],
    edge_faces: &[[usize; 2]],
    edge_candidates: &[Vec<[usize; 2]>],
    budget: &WorkBudget<'_>,
) -> Result<Option<StandardTopologyDraft>, cadmpeg_core::CodecError> {
    let Some(face_run) = selected_standard_run(ctx, bytes)? else {
        return Ok(None);
    };
    let face_count = face_run.face_count();
    let after_faces = face_run.after_faces();
    let Some((edge_rows, vertex_header)) = parse_standard_edge_tables(ctx, bytes, after_faces)?
    else {
        return Ok(None);
    };
    let Some(vertex_points) = parse_vertex_table(ctx, bytes, vertex_header)? else {
        return Ok(None);
    };
    if edge_rows.len() != edge_faces.len()
        || edge_rows.len() != edge_candidates.len()
        || ctx.any_by(
            edge_candidates,
            |candidates| Ok(candidates.is_empty()),
            "catia_standard_iteration",
        )?
    {
        return Ok(None);
    }
    for candidates in ctx.admit_iter(edge_candidates, "catia_standard_iteration")? {
        if ctx.any_by(
            candidates,
            |pair| Ok(pair.iter().any(|point| *point >= vertex_points.len())),
            "catia_standard_iteration",
        )? {
            return Ok(None);
        }
    }

    reconstruct_incidence_candidates(
        ctx,
        &edge_rows,
        &vertex_points,
        edge_faces,
        IncidenceEndpointDomains {
            candidates: edge_candidates,
            ports: None,
        },
        face_count,
        budget,
    )
}

/// Reconstruct standard topology from geometric endpoint candidates while
/// enforcing the serialized endpoint-port equality quotient during search.
/// Search charges the supplied topology phase budget.
pub(super) fn parse_standard_port_endpoint_candidates(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    bytes: &[u8],
    edge_faces: &[[usize; 2]],
    edge_candidates: &[Vec<[usize; 2]>],
    edge_ports: &[[u32; 2]],
    budget: &WorkBudget<'_>,
) -> Result<Option<StandardTopologyDraft>, cadmpeg_core::CodecError> {
    let Some(face_run) = selected_standard_run(ctx, bytes)? else {
        return Ok(None);
    };
    let face_count = face_run.face_count();
    let after_faces = face_run.after_faces();
    let Some((edge_rows, vertex_header)) = parse_standard_edge_tables(ctx, bytes, after_faces)?
    else {
        return Ok(None);
    };
    let Some(vertex_points) = parse_vertex_table(ctx, bytes, vertex_header)? else {
        return Ok(None);
    };
    if edge_rows.len() != edge_faces.len()
        || edge_rows.len() != edge_candidates.len()
        || edge_rows.len() != edge_ports.len()
        || ctx.any_by(
            edge_candidates,
            |candidates| Ok(candidates.is_empty()),
            "catia_standard_iteration",
        )?
    {
        return Ok(None);
    }
    for candidates in ctx.admit_iter(edge_candidates, "catia_standard_iteration")? {
        if ctx.any_by(
            candidates,
            |pair| Ok(pair.iter().any(|point| *point >= vertex_points.len())),
            "catia_standard_iteration",
        )? {
            return Ok(None);
        }
    }
    reconstruct_incidence_candidates(
        ctx,
        &edge_rows,
        &vertex_points,
        edge_faces,
        IncidenceEndpointDomains {
            candidates: edge_candidates,
            ports: Some(edge_ports),
        },
        face_count,
        budget,
    )
}

/// Reconstruct an FBB-only topology from one exact endpoint pair per edge row.
///
/// The FBB-only carrier uses its own two-table delimiter walk rather than the
/// standard edge-table grammar. Those counted tables provide the physical edge
/// rows and the counted vertex table provides the coordinate population. When
/// the native endpoint registry has already selected every pair, face
/// incidence can be closed directly. This path does not infer endpoint
/// identities from trim order.
pub(super) fn parse_fbb_endpoints_with_edge_classes(
    ctx: &DecodeContext<'_>,
    bytes: &[u8],
    edge_faces: &[[usize; 2]],
    edge_points: &[[usize; 2]],
    edge_classes: Option<&[usize]>,
) -> Result<Option<StandardTopologyDraft>, CodecError> {
    let Some(face_run) = largest_fbb_run(ctx, bytes)? else {
        return Ok(None);
    };
    let face_count = face_run.face_count();
    let after_faces = face_run.after_faces();
    let Some((edge_rows, _, vertex_header, _)) = parse_fbb_edge_tables(ctx, bytes, after_faces)?
    else {
        return Ok(None);
    };
    let Some(vertex_points) = parse_vertex_table(ctx, bytes, vertex_header)? else {
        return Ok(None);
    };
    if edge_rows.len() != edge_faces.len()
        || edge_rows.len() != edge_points.len()
        || edge_classes.is_some_and(|classes| classes.len() != edge_rows.len())
    {
        return Ok(None);
    }
    if ctx.any_by(
        edge_points,
        |points| Ok(points.iter().any(|point| *point >= vertex_points.len())),
        "catia_standard_iteration",
    )? {
        return Ok(None);
    }
    reconstruct_incidence_with_edge_classes_and_mesh(
        ctx,
        edge_rows,
        vertex_points,
        edge_faces,
        edge_points,
        face_count,
        StandardIncidenceEvidence {
            edge_classes,
            mesh_bytes: Some(bytes),
        },
    )
}

pub(crate) fn parse_fbb_edge_tables(
    ctx: &DecodeContext<'_>,
    bytes: &[u8],
    position: usize,
) -> FbbEdgeTableOutput {
    // FBB-only tables select one width by the complete table-and-vertex walk;
    // accepting the first delimiter match would assign a wrong handle grammar.
    let mut solutions = Vec::new();
    for handle_width in [1, 2, 3] {
        let Some(parsed) = parse_fbb_edge_tables_width(ctx, bytes, position, handle_width)? else {
            continue;
        };
        if vertex_table_end(ctx, bytes, parsed.2)?.is_some() {
            ctx.push_vec(&mut solutions, parsed, "catia_fbb_edge_width_solutions")?;
        }
    }
    Ok(<[_; 1]>::try_from(solutions)
        .ok()
        .map(|[solution]| solution))
}

fn read_handle(bytes: &[u8], position: usize, width: usize) -> Option<u32> {
    match width {
        1 => bytes.get(position).copied().map(u32::from),
        2 => View::u16_be_at(bytes, position).map(u32::from),
        3 => View::u24_be_at(bytes, position),
        _ => None,
    }
}

pub(super) fn parse_fbb_edge_tables_width(
    ctx: &DecodeContext<'_>,
    bytes: &[u8],
    mut position: usize,
    handle_width: usize,
) -> FbbEdgeTableOutput {
    (|| -> Option<Result<_, CodecError>> {
        let mut rows = Vec::new();
        let mut scopes = Vec::new();
        let mut table_count = 0;
        let mut delimiter_family = None;
        loop {
            if bytes.get(position) != Some(&0x01) {
                return None;
            }
            let kind = *bytes.get(position + 1)?;
            let expected_kind = u8::try_from(table_count + 1).ok()?;
            if kind != expected_kind {
                return None;
            }
            position += 2;
            let count = parse_count(bytes, &mut position)?;
            // A row consumes input bytes, so the declared count is walked
            // one charged row at a time.
            let mut rows_left = 0..count;
            loop {
                match ctx.next_charged(&mut rows_left, "catia_fbb_edge_row_walk") {
                    Ok(Some(_)) => {}
                    Ok(None) => break,
                    Err(error) => return Some(Err(error)),
                }
                if bytes.get(position) != Some(&0x02) {
                    return None;
                }
                position += 1;
                let arity = parse_count(bytes, &mut position)?;
                if arity < 2 {
                    return None;
                }
                if arity > bytes.get(position..).map_or(0, <[u8]>::len) / handle_width {
                    return None;
                }
                let mut handles = Vec::new();
                if let Err(error) = ctx.reserve_vec(&mut handles, arity, "catia_fbb_edge_handles") {
                    return Some(Err(error));
                }
                let Some(handle_width_nonzero) = NonZeroUsize::new(handle_width) else {
                    return None;
                };
                let handle_lane_len = match arity.checked_mul(handle_width) {
                    Some(length) => length,
                    None => {
                        return Some(Err(ctx.refuse_codec_limit(
                            "catia_fbb_edge_handle_lane",
                            u64::MAX,
                            u64::MAX,
                        )))
                    }
                };
                let Some(handle_lane_end) = position.checked_add(handle_lane_len) else {
                    return Some(Err(ctx.refuse_codec_limit(
                        "catia_fbb_edge_handle_lane",
                        u64::MAX,
                        u64::MAX,
                    )));
                };
                let Some(handle_lane) = bytes.get(position..handle_lane_end) else {
                    return None;
                };
                let admitted_handle_bytes =
                    match ctx.admit_iter(handle_lane, "catia_fbb_edge_handle_lane") {
                        Ok(bytes) => bytes.chunks(handle_width_nonzero),
                        Err(error) => return Some(Err(CodecError::from(error))),
                    };
                for handle_bytes in admitted_handle_bytes {
                    let Some(handle) = read_handle(handle_bytes, 0, handle_width) else {
                        return None;
                    };
                    handles.push(handle);
                }
                position = handle_lane_end;
                if let Err(error) = ctx.push_vec(
                    &mut rows,
                    EdgeRow::new(kind, handles, EdgeBoundaryLayout::CompleteBoundaryRun)?,
                    "catia_fbb_edge_rows",
                ) {
                    return Some(Err(error));
                }
                if let Err(error) = ctx.push_vec(&mut scopes, table_count, "catia_fbb_edge_scopes")
                {
                    return Some(Err(error));
                }
            }
            table_count += 1;
            let delimiter = bytes.get(position..position + EDGE_DELIMITER.len())?;
            let family = match handle_width {
                2 if delimiter[0] == 0x10
                    && delimiter[1] >= 0x14
                    && delimiter[1] != 0x24
                    && delimiter[1] & 0x0f == 0x04
                    && delimiter[2..] == EDGE_DELIMITER[2..] =>
                {
                    delimiter[1] >> 4
                }
                1 | 3 if delimiter == EDGE_DELIMITER => 0x02,
                _ => return None,
            };
            if delimiter_family
                .replace(family)
                .is_some_and(|value| value != family)
            {
                return None;
            }
            position += EDGE_DELIMITER.len();
            if bytes.get(position..position + 2) == Some(&[0x01, 0x06]) {
                break;
            }
        }
        (table_count == 2).then_some(Ok((rows, scopes, position, handle_width)))
    })()
    .transpose()
}

/// Recover the row layout used by an FBB-only table from its trim boundaries.
///
/// Complete rows remain complete whenever their stored sequence occurs on a
/// recovered cycle. Some mixed FBB tables store flanking corner handles around
/// an interior sample sequence instead; that form is admitted only when the
/// complete sequence has no occurrence and the interior sequence has at most
/// one occurrence per cycle. Rows with no boundary match remain complete so
/// their fixed unmatched span is preserved for the later placement solver.
pub(super) fn classify_fbb_edge_layouts(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    rows: &mut [EdgeRow],
    trims: &[TrimRecord],
) -> Result<Option<()>, cadmpeg_core::CodecError> {
    let mut cycles = Vec::new();
    for trim in ctx.admit_iter(trims, "catia_fbb_classification_trims")? {
        let Some(face) = boundary_cycles(ctx, trim.packet.triangles(ctx)?)? else {
            return Ok(None);
        };
        ctx.push_vec(&mut cycles, face, "catia_fbb_layout_face_cycles")?;
    }
    let mut storage = ctx.reserve_scoped(0, "catia_fbb_cycle_handle_index")?;
    let mut indexed = Vec::new();
    for face in ctx.admit_iter(&cycles, "catia_fbb_cycle_handle_index")? {
        for cycle in ctx.admit_iter(face, "catia_fbb_cycle_handle_index")? {
            let cycle = index_cycle(ctx, &mut storage, cycle)?;
            ctx.push_scoped_vec(
                &mut storage,
                &mut indexed,
                cycle,
                "catia_fbb_cycle_handle_index",
            )?;
        }
    }
    for row in ctx.admit_iter(rows, "catia_fbb_layout_rows")? {
        let complete_matches = ctx.fold(
            &indexed,
            0_usize,
            |total, cycle| {
                Ok(total.saturating_add(pattern_match_count(ctx, cycle, row.handles())?))
            },
            "catia_fbb_complete_pattern_matches",
        )?;
        if complete_matches != 0 {
            continue;
        }
        let Some(end) = row.handles().len().checked_sub(1) else {
            return Ok(None);
        };
        let Some(interior) = row.handles().get(1..end) else {
            continue;
        };
        if interior.is_empty() {
            continue;
        }
        let mut matched = false;
        let mut unique_per_cycle = true;
        for cycle in ctx.admit_iter(&indexed, "catia_fbb_interior_pattern_matches")? {
            let count = pattern_match_count(ctx, cycle, interior)?;
            matched |= count != 0;
            unique_per_cycle &= count <= 1;
        }
        if matched && unique_per_cycle && !row.select_flanking_corners() {
            return Ok(None);
        }
    }
    Ok(Some(()))
}

/// A trim cycle with the positions of each handle, so a pattern is compared
/// only at the starts where its first or last handle occurs.
struct IndexedCycle<'a> {
    handles: &'a [u32],
    positions: HashMap<u32, Vec<usize>>,
}

fn index_cycle<'a>(
    ctx: &DecodeContext<'_>,
    storage: &mut cadmpeg_core::decode::ScopedReservation<'_>,
    handles: &'a [u32],
) -> Result<IndexedCycle<'a>, CodecError> {
    let mut positions = HashMap::new();
    for (position, &handle) in ctx
        .admit_iter(handles, "catia_fbb_cycle_handle_index")?
        .enumerate()
    {
        storage.with_storage(|| {
            ctx.push_hash_group(
                &mut positions,
                handle,
                position,
                "catia_fbb_cycle_handle_index",
                "catia_fbb_cycle_handle_positions",
            )
        })?;
    }
    Ok(IndexedCycle { handles, positions })
}

/// Number of cycle starts at which the pattern reads forward or backward
/// around the cycle.
fn pattern_match_count(
    ctx: &DecodeContext<'_>,
    cycle: &IndexedCycle<'_>,
    pattern: &[u32],
) -> Result<usize, CodecError> {
    const OPERATION: &str = "catia_fbb_pattern_match";
    let (Some(&first), Some(&last)) = (pattern.first(), pattern.last()) else {
        return Ok(0);
    };
    let length = cycle.handles.len();
    if pattern.len() > length {
        return Ok(0);
    }
    let reads = |start: usize, reversed: bool| {
        ctx.all_by(
            pattern.iter().enumerate(),
            |(offset, &handle)| {
                let expected = if reversed {
                    pattern[pattern.len() - 1 - offset]
                } else {
                    handle
                };
                Ok(cycle.handles[(start + offset) % length] == expected)
            },
            OPERATION,
        )
    };
    let starts = |handle: u32| -> Result<&[usize], CodecError> {
        Ok(ctx
            .get_hash_map(&cycle.positions, &handle, OPERATION)?
            .map_or(&[][..], Vec::as_slice))
    };
    // A start reads forward only where the first handle sits and backward
    // only where the last one does; with equal ends both share one list.
    let mut matches = ctx.fold(
        starts(first)?,
        0_usize,
        |count, &start| Ok(count + usize::from(reads(start, false)? || reads(start, true)?)),
        OPERATION,
    )?;
    if first != last {
        matches = ctx.fold(
            starts(last)?,
            matches,
            |count, &start| Ok(count + usize::from(reads(start, true)?)),
            OPERATION,
        )?;
    }
    Ok(matches)
}

/// Contiguous fixed-width FBB face rows.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct FbbFaceRun {
    face_start: usize,
    after_faces: usize,
}
impl FbbFaceRun {
    /// Constructs a face run with representable byte bounds.
    pub(super) fn try_new(face_start: usize, face_count: usize) -> Option<Self> {
        let after_faces = face_start.checked_add(face_count.checked_mul(fbb_row::LEN)?)?;
        Some(Self {
            face_start,
            after_faces,
        })
    }

    /// Byte offset of the first face row.
    pub(crate) fn face_start(&self) -> usize {
        self.face_start
    }

    /// Number of fixed-width face rows.
    pub(crate) fn face_count(&self) -> usize {
        (self.after_faces - self.face_start) / fbb_row::LEN
    }

    pub(crate) fn after_faces(&self) -> usize {
        self.after_faces
    }
}

/// Find every independently source-closed standard FBB population.
///
/// The result is intentionally not reduced to the largest run. A caller that
/// has a single result may select it; a caller that has multiple results must
/// bind their carrier and incidence rosters before creating neutral bodies.
pub(super) fn standard_fbb_groups(
    ctx: &DecodeContext<'_>,
    bytes: &[u8],
    ranges: &[std::ops::Range<usize>],
) -> Result<Vec<FbbFaceRun>, CodecError> {
    let mut groups = Vec::new();
    for range in ctx.admit_iter(ranges, "catia_standard_iteration")? {
        if let Some(group) =
            parse_standard_group(ctx, bytes, range.start, range.len() / fbb_row::LEN)?
        {
            ctx.push_vec(&mut groups, group, "catia_standard_fbb_groups")?;
        }
    }
    Ok(groups)
}

/// Grammar of a population's edge tables.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum EdgeTableForm {
    Standard,
    FbbOnly,
}

/// A source-closed FBB layout whose topology may still require the global
/// endpoint solver. The edge and vertex counts are structural population
/// keys; they are not body selection by themselves.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct FbbPopulationLayout {
    pub(super) face_run: FbbFaceRun,
    pub(super) edge_count: usize,
    pub(super) vertex_count: usize,
    pub(super) edge_table_form: EdgeTableForm,
}

fn vertex_table_end(
    ctx: &DecodeContext<'_>,
    bytes: &[u8],
    position: usize,
) -> Result<Option<usize>, CodecError> {
    let Some(header_end) = position.checked_add(2) else {
        return Err(ctx.refuse_codec_limit(
            "catia_fbb_vertex_table_header_end",
            u64::MAX,
            u64::MAX,
        ));
    };
    if bytes.get(position..header_end) != Some(&[0x01, 0x06]) {
        return Ok(None);
    }
    let mut cursor = header_end;
    let Some(count) = parse_count(bytes, &mut cursor) else {
        return Ok(None);
    };
    let record_bytes = count.checked_mul(VERTEX_RECORD_BYTES).ok_or_else(|| {
        ctx.refuse_codec_limit("catia_fbb_vertex_record_bytes", u64::MAX, u64::MAX)
    })?;
    let end = cursor
        .checked_add(record_bytes)
        .ok_or_else(|| ctx.refuse_codec_limit("catia_fbb_vertex_table_end", u64::MAX, u64::MAX))?;
    let Some(records) = bytes.get(cursor..end) else {
        return Ok(None);
    };
    let Some(record_width) = NonZeroUsize::new(VERTEX_RECORD_BYTES) else {
        return Ok(None);
    };
    for record in ctx
        .admit_iter(records, "catia_standard_iteration")?
        .chunks(record_width)
    {
        if record.get(..3) != Some(&[0x05, 0x08, 0x01][..]) {
            return Ok(None);
        }
        for offset in [3, 7, 11] {
            let Some(value) = View::f32_le_at(record, offset) else {
                return Ok(None);
            };
            if FiniteReal::new(f64::from(value)).is_none() {
                return Ok(None);
            }
        }
    }
    Ok(Some(end))
}

/// Return one source-closed FBB population as a self-contained topology
/// spine. The slice starts at its complete trim chain and ends after its
/// counted vertex table, so the existing single-population parsers can be
/// reused without seeing neighboring populations.
pub(super) fn population_spine<'a>(
    ctx: &DecodeContext<'_>,
    bytes: &'a [u8],
    layout: &FbbPopulationLayout,
) -> Result<Option<&'a [u8]>, CodecError> {
    let parsed = match layout.edge_table_form {
        EdgeTableForm::Standard => {
            parse_standard_edge_tables_with_width(ctx, bytes, layout.face_run.after_faces())?
        }
        EdgeTableForm::FbbOnly => parse_fbb_edge_tables(ctx, bytes, layout.face_run.after_faces())?
            .map(|(_, _, vertex_header, handle_width)| (Vec::new(), vertex_header, handle_width)),
    };
    let Some((_, vertex_header, handle_width)) = parsed else {
        return Ok(None);
    };
    let Some((trim_start, _)) = parse_trim_chain_start(
        ctx,
        bytes,
        layout.face_run.face_start(),
        layout.face_run.face_count(),
        handle_width,
    )?
    else {
        return Ok(None);
    };
    let Some(end) = vertex_table_end(ctx, bytes, vertex_header)? else {
        return Ok(None);
    };
    Ok(bytes.get(trim_start..end))
}

/// Find every FBB face run with a complete local trim, edge-table, and vertex
/// walk, without requiring endpoint incidence to be solved.
pub(super) fn fbb_population_layouts(
    ctx: &DecodeContext<'_>,
    bytes: &[u8],
) -> Result<Vec<FbbPopulationLayout>, CodecError> {
    let mut layouts = Vec::new();
    let ranges = crate::container::fbb_run_ranges(ctx, bytes)?;
    for range in ctx.admit_iter(&ranges, "catia_standard_iteration")? {
        let Some(face_run) = FbbFaceRun::try_new(range.start, range.len() / fbb_row::LEN) else {
            continue;
        };
        let after_faces = face_run.after_faces();
        let parsed = parse_standard_edge_tables_with_width(ctx, bytes, after_faces)?.map(
            |(rows, vertex_header, handle_width)| {
                (rows, vertex_header, handle_width, EdgeTableForm::Standard)
            },
        );
        let parsed = if parsed.is_some() {
            parsed
        } else {
            parse_fbb_edge_tables(ctx, bytes, after_faces)?.map(
                |(rows, _, vertex_header, handle_width)| {
                    (rows, vertex_header, handle_width, EdgeTableForm::FbbOnly)
                },
            )
        };
        let Some((edge_rows, vertex_header, handle_width, edge_table_form)) = parsed else {
            continue;
        };
        if vertex_table_end(ctx, bytes, vertex_header)?.is_none() {
            continue;
        }
        let mut count_position = vertex_header + 2;
        let Some(vertex_count) = parse_count(bytes, &mut count_position) else {
            continue;
        };
        if parse_trim_chain(
            ctx,
            bytes,
            face_run.face_start(),
            face_run.face_count(),
            handle_width,
        )?
        .is_none()
        {
            continue;
        }
        ctx.push_vec(
            &mut layouts,
            FbbPopulationLayout {
                face_run,
                edge_count: edge_rows.len(),
                vertex_count,
                edge_table_form,
            },
            "catia_fbb_population_layouts",
        )?;
    }
    Ok(layouts)
}

fn parse_standard_group(
    ctx: &DecodeContext<'_>,
    bytes: &[u8],
    face_start: usize,
    face_count: usize,
) -> Result<Option<FbbFaceRun>, CodecError> {
    let Some(face_run) = FbbFaceRun::try_new(face_start, face_count) else {
        return Ok(None);
    };
    let after_faces = face_run.after_faces();
    let Some((edge_rows, vertex_header, handle_width)) =
        parse_standard_edge_tables_with_width(ctx, bytes, after_faces)?
    else {
        return Ok(None);
    };
    let Some(vertex_points) = parse_vertex_table(ctx, bytes, vertex_header)? else {
        return Ok(None);
    };
    let Some(trims) = parse_trim_chain(ctx, bytes, face_start, face_count, handle_width)? else {
        return Ok(None);
    };
    Ok(reconstruct(ctx, edge_rows, vertex_points, &trims)?.map(|_| face_run))
}

pub(crate) fn selected_standard_run(
    ctx: &DecodeContext<'_>,
    bytes: &[u8],
) -> Result<Option<FbbFaceRun>, CodecError> {
    let ranges = crate::container::fbb_run_ranges(ctx, bytes)?;
    if let [range] = ranges.as_slice() {
        // A single marker run has no competing population to disambiguate.
        return Ok(FbbFaceRun::try_new(range.start, range.len() / fbb_row::LEN));
    }
    let groups = standard_fbb_groups(ctx, bytes, &ranges)?;
    Ok(match groups.as_slice() {
        [group] => Some(*group),
        [] => largest_of_runs(ctx, &ranges)?
            .and_then(|range| FbbFaceRun::try_new(range.start, range.len() / fbb_row::LEN)),
        _ => None,
    })
}

pub(crate) fn largest_fbb_run(
    ctx: &DecodeContext<'_>,
    bytes: &[u8],
) -> Result<Option<FbbFaceRun>, CodecError> {
    let ranges = crate::container::fbb_run_ranges(ctx, bytes)?;
    Ok(largest_of_runs(ctx, &ranges)?
        .and_then(|range| FbbFaceRun::try_new(range.start, range.len() / fbb_row::LEN)))
}

/// The unique longest run; equal-longest runs leave the selection open.
fn largest_of_runs<'a>(
    ctx: &DecodeContext<'_>,
    ranges: &'a [std::ops::Range<usize>],
) -> Result<Option<&'a std::ops::Range<usize>>, CodecError> {
    let (best, tied) = ctx.fold(
        ranges,
        (None::<&std::ops::Range<usize>>, false),
        |(best, tied), range| {
            Ok(match best {
                Some(best) if range.len() < best.len() => (Some(best), tied),
                Some(best) if range.len() == best.len() => (Some(best), true),
                _ => (Some(range), false),
            })
        },
        "catia_fbb_largest_run",
    )?;
    Ok(best.filter(|_| !tied))
}

#[cfg(test)]
mod appearance_tests {
    use super::{standard_face_colors, work_refusal_at};

    #[test]
    fn face_color_row_lane_refuses_work() {
        let bytes = [
            0xb0, 4, 4, 0xff, 0x99, 0x1f, 0x1a, 0xd1, 0xb0, 4, 4, 0xff, 0xff, 0xe0, 0x3d, 0x14,
        ];
        work_refusal_at("catia_fbb_face_color_rows", |ctx| {
            standard_face_colors(ctx, &bytes).map(|_| ())
        });
    }

    #[test]
    fn face_colors_are_abgr_and_require_one_marker_family() {
        let bytes = [
            0xb0, 4, 4, 0xff, 0x99, 0x1f, 0x1a, 0xd1, 0xb0, 4, 4, 0xff, 0xff, 0xe0, 0x3d, 0x14,
        ];
        assert_eq!(
            crate::test_support::with_service_context(|ctx| standard_face_colors(ctx, &bytes))
                .expect("service resource budget"),
            Some(vec![[0xd1, 0x1a, 0x1f, 0x99], [0x14, 0x3d, 0xe0, 0xff]])
        );
        assert!(matches!(
            crate::test_support::with_collection_limit(1, |ctx| standard_face_colors(ctx, &bytes)),
            Err(cadmpeg_core::CodecError::ResourceLimit(limit))
                if limit.operation == "catia_fbb_face_colors"
        ));
        let mut mixed = bytes;
        mixed[8] = 0x30;
        assert_eq!(
            crate::test_support::with_service_context(|ctx| standard_face_colors(ctx, &mixed))
                .expect("service resource budget"),
            None
        );
    }
}

#[cfg(test)]
mod allocation_tests {
    use super::{
        fbb_population_layouts, largest_fbb_run, parse_fbb_edge_tables, parse_standard,
        parse_standard_edge_tables_with_width, parse_trim_chain, parse_trim_record,
        parse_trim_record_layout, parse_trim_record_with_length_encoding, parse_vertex_table,
        standard_face_count, work_refusal_at,
    };
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
    use cadmpeg_core::CodecError;

    fn collection_refusals(
        bytes: &[u8],
        run: impl Fn(&DecodeContext<'_>) -> Result<(), CodecError>,
    ) -> std::collections::HashSet<&'static str> {
        let mut operations = std::collections::HashSet::new();
        for limit in 0..=256 {
            let arena = DecodeArena::new();
            let mut policy = DecodePolicy::service();
            policy.limits.max_collection_items = limit;
            let (ctx, _) = DecodeContext::from_root_bytes(bytes, &arena, &policy)
                .expect("fixture fits the input limit");
            match run(&ctx) {
                Err(CodecError::ResourceLimit(error)) => {
                    assert_eq!(error.dimension, ResourceDimension::CollectionItems);
                    operations.insert(error.operation);
                }
                Ok(()) => {}
                Err(error) => panic!("unexpected collection refusal: {error}"),
            }
        }
        operations
    }

    #[test]
    fn counted_trim_search_and_packet_lanes_refuse_before_growth() {
        let mut bytes = vec![
            0x01, 0x47, 0x01, 0x01, 0x01, 0xff, 0x0a, 0x00, 0x00, 0x00, 0x03, 0x04,
        ];
        for handle in 0u16..10 {
            bytes.extend_from_slice(&handle.to_be_bytes());
        }
        let parsed = crate::test_support::with_service_context(|ctx| {
            parse_trim_chain(ctx, &bytes, bytes.len(), 1, 2)
        })
        .expect("service resource budget")
        .expect("complete trim chain");
        assert_eq!(parsed.len(), 1);
        let operations = collection_refusals(&bytes, |ctx| {
            parse_trim_chain(ctx, &bytes, bytes.len(), 1, 2)?;
            Ok(())
        });
        for operation in [
            "catia_trim_primitive_lengths",
            "catia_trim_predecessor_ends",
            "catia_trim_predecessor_starts",
            "catia_trim_search_frames",
            "catia_trim_packet_handles",
            "catia_trim_fan_lengths",
            "catia_trim_reversed",
            "catia_trim_solution_records",
            "catia_trim_clone_handles",
            "catia_trim_solutions",
        ] {
            assert!(operations.contains(operation), "no refusal at {operation}");
        }
        let operations = collection_refusals(&bytes, |ctx| {
            parse_trim_record_layout(ctx, &bytes, 0, 2)?;
            parse_trim_record(ctx, &bytes, 0, 2)?;
            Ok(())
        });
        assert!(operations.contains("catia_trim_primitive_lengths"));
        assert!(operations.contains("catia_trim_packet_handles"));
    }

    #[test]
    fn cycle_cover_matches_corners_and_coedges_refuse_before_growth() {
        use crate::families::standard::topology::{EdgeBoundaryLayout, EdgeRow};
        use crate::solve::union_find::UnionFind;

        let rows = [[0, 1], [1, 2], [2, 0]].map(|handles| {
            EdgeRow::new(1, handles.to_vec(), EdgeBoundaryLayout::CompleteBoundaryRun)
                .expect("admitted edge row")
        });
        let run = |ctx: &DecodeContext<'_>| {
            let mut union = UnionFind::new(6);
            let ends = super::row_pattern_ends(ctx, &rows)?;
            super::cover_cycle(ctx, &[0, 1, 2], &rows, &ends, &mut union)
        };
        let boundary = crate::test_support::with_service_context(run)
            .expect("service resource budget")
            .expect("complete edge cycle");
        assert_eq!(boundary.coedges.len(), 3);
        let mut operations = std::collections::HashSet::new();
        for cap in 0..32 {
            let result = crate::test_support::with_collection_limit(cap, run);
            if let Err(CodecError::ResourceLimit(refusal)) = result {
                operations.insert(refusal.operation);
            }
        }
        for operation in [
            "catia_fbb_cycle_matches",
            "catia_fbb_corner_union_nodes",
            "catia_fbb_corner_nodes",
            "catia_fbb_cycle_coedges",
        ] {
            assert!(operations.contains(operation), "no refusal at {operation}");
        }
    }

    #[test]
    fn fbb_edge_handle_lane_refuses_work() {
        let bytes = crate::test_support::test_topology::fbb_only_quad_topology_stream();
        let after_faces =
            crate::test_support::with_service_context(|ctx| largest_fbb_run(ctx, &bytes))
                .expect("service scan")
                .expect("FBB face run")
                .after_faces();
        work_refusal_at("catia_fbb_edge_handle_lane", |ctx| {
            parse_fbb_edge_tables(ctx, &bytes, after_faces).map(|_| ())
        });
    }

    #[test]
    fn standard_edge_handle_lane_refuses_work() {
        let bytes = crate::test_support::test_topology::standard_quad_topology_stream();
        let after_faces =
            crate::test_support::with_service_context(|ctx| largest_fbb_run(ctx, &bytes))
                .expect("service scan")
                .expect("standard face run")
                .after_faces();
        work_refusal_at("catia_standard_edge_handle_lane", |ctx| {
            parse_standard_edge_tables_with_width(ctx, &bytes, after_faces).map(|_| ())
        });
    }

    #[test]
    fn fbb_vertex_record_lane_refuses_work() {
        let bytes = crate::test_support::test_topology::fbb_only_quad_topology_stream();
        let after_faces =
            crate::test_support::with_service_context(|ctx| largest_fbb_run(ctx, &bytes))
                .expect("service scan")
                .expect("FBB face run")
                .after_faces();
        let vertex_header = crate::test_support::with_service_context(|ctx| {
            parse_fbb_edge_tables(ctx, &bytes, after_faces)
        })
        .expect("service resource budget")
        .expect("complete edge tables")
        .2;
        work_refusal_at("catia_fbb_vertex_record_lane", |ctx| {
            parse_vertex_table(ctx, &bytes, vertex_header).map(|_| ())
        });
    }

    #[test]
    fn wide_trim_length_lane_refuses_work() {
        let mut bytes = vec![0x01, 0x42, 0x01, 0xff];
        bytes.extend_from_slice(&3_u32.to_le_bytes());
        bytes.extend_from_slice(&3_u16.to_be_bytes());
        for handle in 0_u16..3 {
            bytes.extend_from_slice(&handle.to_be_bytes());
        }
        assert!(crate::test_support::with_service_context(|ctx| {
            parse_trim_record_with_length_encoding(ctx, &bytes, 0, 2, true)
        })
        .expect("service resource budget")
        .is_some());
        work_refusal_at("catia_trim_wide_length_lane", |ctx| {
            parse_trim_record_with_length_encoding(ctx, &bytes, 0, 2, true).map(|_| ())
        });
    }

    #[test]
    fn trim_packet_handle_lane_refuses_work() {
        let mut bytes = vec![
            0x01, 0x47, 0x01, 0x01, 0x01, 0xff, 0x0a, 0x00, 0x00, 0x00, 0x03, 0x04,
        ];
        for handle in 0_u16..10 {
            bytes.extend_from_slice(&handle.to_be_bytes());
        }
        work_refusal_at("catia_trim_packet_handle_lane", |ctx| {
            parse_trim_chain(ctx, &bytes, bytes.len(), 1, 2).map(|_| ())
        });
    }

    #[test]
    fn fbb_boundary_coverage_segments_refuse_work() {
        use crate::families::standard::topology::{EdgeBoundaryLayout, EdgeRow};
        use crate::solve::union_find::UnionFind;

        let rows = [[0, 1], [1, 2], [2, 0]].map(|handles| {
            EdgeRow::new(1, handles.to_vec(), EdgeBoundaryLayout::CompleteBoundaryRun)
                .expect("admitted edge row")
        });
        work_refusal_at("catia_fbb_boundary_coverage_steps", |ctx| {
            let mut union = UnionFind::new(6);
            let ends = super::row_pattern_ends(ctx, &rows)?;
            super::cover_cycle(ctx, &[0, 1, 2], &rows, &ends, &mut union).map(|_| ())
        });
    }

    #[test]
    fn counted_fbb_edge_rows_and_nested_handles_refuse_before_allocation() {
        let bytes = crate::test_support::test_topology::fbb_only_quad_topology_stream();
        let after_faces =
            crate::test_support::with_service_context(|ctx| largest_fbb_run(ctx, &bytes))
                .expect("service scan")
                .expect("FBB face run")
                .after_faces();
        let parsed = crate::test_support::with_service_context(|ctx| {
            parse_fbb_edge_tables(ctx, &bytes, after_faces)
        })
        .expect("service resource budget")
        .expect("complete edge tables");
        assert_eq!(parsed.0.len(), 4);
        let operations = collection_refusals(&bytes, |ctx| {
            parse_fbb_edge_tables(ctx, &bytes, after_faces)?;
            Ok(())
        });
        for operation in [
            "catia_fbb_edge_handles",
            "catia_fbb_edge_rows",
            "catia_fbb_edge_scopes",
            "catia_fbb_edge_width_solutions",
        ] {
            assert!(operations.contains(operation), "no refusal at {operation}");
        }
    }

    #[test]
    fn counted_standard_edge_rows_and_nested_handles_refuse_before_allocation() {
        let bytes = crate::test_support::test_topology::standard_quad_topology_stream();
        let after_faces =
            crate::test_support::with_service_context(|ctx| largest_fbb_run(ctx, &bytes))
                .expect("service scan")
                .expect("standard face run")
                .after_faces();
        let parsed = crate::test_support::with_service_context(|ctx| {
            parse_standard_edge_tables_with_width(ctx, &bytes, after_faces)
        })
        .expect("service resource budget")
        .expect("complete edge tables");
        assert_eq!(parsed.0.len(), 4);
        let operations = collection_refusals(&bytes, |ctx| {
            parse_standard_edge_tables_with_width(ctx, &bytes, after_faces)?;
            Ok(())
        });
        for operation in [
            "catia_standard_edge_handles",
            "catia_standard_edge_rows",
            "catia_standard_edge_scopes",
        ] {
            assert!(operations.contains(operation), "no refusal at {operation}");
        }
    }

    #[test]
    fn counted_fbb_vertex_points_and_coordinate_copy_refuse_before_allocation() {
        let bytes = crate::test_support::test_topology::fbb_only_quad_topology_stream();
        let after_faces =
            crate::test_support::with_service_context(|ctx| largest_fbb_run(ctx, &bytes))
                .expect("service scan")
                .expect("FBB face run")
                .after_faces();
        let vertex_header = crate::test_support::with_service_context(|ctx| {
            parse_fbb_edge_tables(ctx, &bytes, after_faces)
        })
        .expect("service resource budget")
        .expect("complete edge tables")
        .2;
        let points = crate::test_support::with_service_context(|ctx| {
            parse_vertex_table(ctx, &bytes, vertex_header)
        })
        .expect("service resource budget")
        .expect("counted vertices");
        assert_eq!(points.len(), 4);
        let operations = collection_refusals(&bytes, |ctx| {
            parse_vertex_table(ctx, &bytes, vertex_header)?;
            Ok(())
        });
        for operation in ["catia_fbb_vertex_points", "catia_fbb_vertex_coordinates"] {
            assert!(operations.contains(operation), "no refusal at {operation}");
        }

        let refusal = crate::test_support::with_retained_refusal(
            &bytes,
            "catia_fbb_vertex_coordinates",
            |ctx| parse_vertex_table(ctx, &bytes, vertex_header),
        )
        .expect_err("coordinate copy exceeds retained bytes");
        assert!(matches!(refusal, CodecError::ResourceLimit(limit)
            if limit.dimension == ResourceDimension::RetainedBytes
                && limit.operation == "catia_fbb_vertex_coordinates"));
    }

    #[test]
    fn fbb_population_layouts_charge_each_discovered_population() {
        let mut bytes = crate::test_support::test_topology::fbb_only_quad_topology_stream();
        bytes.push(0);
        bytes.extend(crate::test_support::test_topology::fbb_only_quad_topology_stream());
        let layouts =
            crate::test_support::with_service_context(|ctx| fbb_population_layouts(ctx, &bytes))
                .expect("service resource budget");
        assert_eq!(layouts.len(), 2);
        let operations = collection_refusals(&bytes, |ctx| {
            fbb_population_layouts(ctx, &bytes)?;
            Ok(())
        });
        assert!(operations.contains("catia_fbb_population_layouts"));
    }

    #[test]
    fn fbb_boundary_coverage_propagates_collection_refusal() {
        let bytes = crate::test_support::test_topology::standard_quad_topology_stream();
        crate::test_support::with_service_context(|ctx| {
            assert!(parse_standard(ctx, &bytes)
                .expect("service resource budget")
                .is_some());
        });
        let operations = collection_refusals(&bytes, |ctx| {
            parse_standard(ctx, &bytes)?;
            Ok(())
        });
        assert!(operations.contains("catia FBB boundary coverage"));
    }

    #[test]
    fn fbb_only_boundary_coverage_propagates_collection_refusal() {
        let bytes = crate::test_support::test_topology::fbb_only_quad_topology_stream();
        crate::test_support::with_service_context(|ctx| {
            assert!(crate::families::standard::topology::parse_fbb(ctx, &bytes)
                .expect("service resource budget")
                .is_some());
        });
        let operations = collection_refusals(&bytes, |ctx| {
            crate::families::standard::topology::parse_fbb(ctx, &bytes)?;
            Ok(())
        });
        assert!(operations.contains("catia FBB boundary coverage"));
    }

    #[test]
    fn multiple_standard_runs_propagate_coverage_refusal_during_selection() {
        let mut bytes = crate::test_support::test_topology::standard_quad_topology_stream();
        bytes.extend(crate::test_support::test_topology::standard_quad_topology_stream());
        crate::test_support::with_service_context(|ctx| {
            assert_eq!(
                standard_face_count(ctx, &bytes).expect("service resource budget"),
                None
            );
        });
        let operations = collection_refusals(&bytes, |ctx| {
            standard_face_count(ctx, &bytes)?;
            Ok(())
        });
        assert!(operations.contains("catia FBB boundary coverage"));
    }

    #[test]
    fn fbb_reconstruct_union_refuses_before_boundary_selection() {
        let bytes = crate::test_support::test_topology::fbb_only_quad_topology_stream();
        let operations = collection_refusals(&bytes, |ctx| {
            crate::families::standard::topology::parse_fbb(ctx, &bytes)?;
            Ok(())
        });
        assert!(operations.contains("catia_reconstruct_union"));
    }

    #[test]
    fn fbb_reconstruct_boundaries_refuse_collection_limit() {
        let bytes = crate::test_support::test_topology::fbb_only_quad_topology_stream();
        let operations = collection_refusals(&bytes, |ctx| {
            crate::families::standard::topology::parse_fbb(ctx, &bytes)?;
            Ok(())
        });
        assert!(operations.contains("catia_reconstruct_boundaries"));
    }

    #[test]
    fn fbb_reconstruct_faces_refuse_collection_limit() {
        let bytes = crate::test_support::test_topology::fbb_only_quad_topology_stream();
        let operations = collection_refusals(&bytes, |ctx| {
            crate::families::standard::topology::parse_fbb(ctx, &bytes)?;
            Ok(())
        });
        assert!(operations.contains("catia_reconstruct_faces"));
    }

    #[test]
    fn fbb_reconstruct_roots_refuse_collection_limit() {
        let bytes = crate::test_support::test_topology::fbb_only_quad_topology_stream();
        let operations = collection_refusals(&bytes, |ctx| {
            crate::families::standard::topology::parse_fbb(ctx, &bytes)?;
            Ok(())
        });
        assert!(operations.contains("catia_reconstruct_roots"));
    }
}

fn parse_count(bytes: &[u8], position: &mut usize) -> Option<usize> {
    let first = *bytes.get(*position)?;
    *position += 1;
    if first != 0xff {
        return Some(usize::from(first));
    }
    let value = View::u32_le_at(bytes, *position)?;
    *position += 4;
    usize::try_from(value).ok()
}

pub(crate) fn parse_edge_tables(
    ctx: &DecodeContext<'_>,
    bytes: &[u8],
    position: usize,
) -> Result<Option<(Vec<EdgeRow>, usize)>, CodecError> {
    if let Some(result) = parse_standard_edge_tables(ctx, bytes, position)? {
        return Ok(Some(result));
    }
    Ok(parse_fbb_edge_tables(ctx, bytes, position)?
        .map(|(rows, _, vertex_header, _)| (rows, vertex_header)))
}

fn parse_standard_edge_tables(
    ctx: &DecodeContext<'_>,
    bytes: &[u8],
    position: usize,
) -> Result<Option<(Vec<EdgeRow>, usize)>, CodecError> {
    Ok(parse_standard_edge_tables_with_width(ctx, bytes, position)?
        .map(|(rows, vertex_header, _)| (rows, vertex_header)))
}

pub(crate) fn parse_standard_edge_tables_with_width(
    ctx: &DecodeContext<'_>,
    bytes: &[u8],
    position: usize,
) -> Result<Option<(Vec<EdgeRow>, usize, usize)>, CodecError> {
    Ok(parse_standard_edge_tables_scoped(ctx, bytes, position)?
        .map(|(rows, _, vertex_header, handle_width)| (rows, vertex_header, handle_width)))
}

pub(crate) fn parse_standard_edge_tables_scoped(
    ctx: &DecodeContext<'_>,
    bytes: &[u8],
    position: usize,
) -> FbbEdgeTableOutput {
    // The full standard spine uses u16be rows and may contain one or more
    // counted tables. Keep that grammar first so a malformed standard walk
    // cannot silently enter the compact form below.
    if let Some((rows, scopes, vertex_header)) =
        parse_edge_tables_scoped_width(ctx, bytes, position, 2)?
    {
        if vertex_table_end(ctx, bytes, vertex_header)?.is_some() {
            return Ok(Some((rows, scopes, vertex_header, 2)));
        }
    }

    // CATIA also emits a compact standard spine with one kind-01 table. Its
    // rows use one selected width and the table is closed directly by the
    // counted vertex table. A two-table walk belongs to the separate FBB-only
    // family and must not be admitted through this fallback.
    if bytes.get(position..position + 2) != Some(&[0x01, 0x01]) {
        return Ok(None);
    }
    let Some((rows, scopes, vertex_header, handle_width)) =
        parse_edge_tables_scoped_at_with_width(ctx, bytes, position)?
    else {
        return Ok(None);
    };
    Ok((!rows.is_empty()
        && ctx.all_by(&scopes, |scope| Ok(*scope == 0), "catia_standard_iteration")?
        && ctx.all_by(
            &rows,
            |row| Ok(row.boundary_layout() == EdgeBoundaryLayout::CompleteBoundaryRun),
            "catia_standard_iteration",
        )?)
    .then_some((rows, scopes, vertex_header, handle_width)))
}

#[cfg(test)]
pub(super) fn parse_edge_tables_at(
    ctx: &DecodeContext<'_>,
    bytes: &[u8],
    position: usize,
) -> Result<Option<(Vec<EdgeRow>, usize)>, CodecError> {
    Ok(parse_edge_tables_scoped_at(ctx, bytes, position)?
        .map(|(rows, _, vertex_header)| (rows, vertex_header)))
}

#[cfg(test)]
pub(super) fn parse_edge_tables_scoped_at(
    ctx: &DecodeContext<'_>,
    bytes: &[u8],
    position: usize,
) -> ScopedEdgeTableOutput {
    Ok(
        parse_edge_tables_scoped_at_with_width(ctx, bytes, position)?
            .map(|(rows, scopes, vertex_header, _)| (rows, scopes, vertex_header)),
    )
}

fn parse_edge_tables_scoped_at_with_width(
    ctx: &DecodeContext<'_>,
    bytes: &[u8],
    position: usize,
) -> FbbEdgeTableOutput {
    let mut solutions = Vec::new();
    for handle_width in [1, 2, 3] {
        let Some(parsed) = parse_edge_tables_scoped_width(ctx, bytes, position, handle_width)?
        else {
            continue;
        };
        if vertex_table_end(ctx, bytes, parsed.2)?.is_some() {
            ctx.push_vec(
                &mut solutions,
                (parsed.0, parsed.1, parsed.2, handle_width),
                "catia_standard_edge_width_solutions",
            )?;
        }
    }
    Ok(<[_; 1]>::try_from(solutions)
        .ok()
        .map(|[solution]| solution))
}

fn parse_edge_tables_scoped_width(
    ctx: &DecodeContext<'_>,
    bytes: &[u8],
    mut position: usize,
    handle_width: usize,
) -> ScopedEdgeTableOutput {
    (|| -> Option<Result<_, CodecError>> {
        let mut rows = Vec::new();
        let mut scopes = Vec::new();
        let mut scope = 0usize;
        loop {
            if let Err(error) = ctx.charge_work(1, "catia_standard_edge_table_scopes") {
                return Some(Err(error));
            }
            if bytes.get(position) != Some(&0x01) {
                return None;
            }
            let kind = *bytes.get(position + 1)?;
            if !matches!(kind, 0x01 | 0x02) {
                return None;
            }
            position += 2;
            let count = parse_count(bytes, &mut position)?;
            // A row consumes input bytes, so the declared count is walked
            // one charged row at a time.
            let mut rows_left = 0..count;
            loop {
                match ctx.next_charged(&mut rows_left, "catia_fbb_edge_row_walk") {
                    Ok(Some(_)) => {}
                    Ok(None) => break,
                    Err(error) => return Some(Err(error)),
                }
                if bytes.get(position) != Some(&0x02) {
                    return None;
                }
                position += 1;
                let arity = parse_count(bytes, &mut position)?;
                if arity < 2 {
                    return None;
                }
                if arity > bytes.get(position..).map_or(0, <[u8]>::len) / handle_width {
                    return None;
                }
                let mut handles = Vec::new();
                if let Err(error) =
                    ctx.reserve_vec(&mut handles, arity, "catia_standard_edge_handles")
                {
                    return Some(Err(error));
                }
                let Some(handle_width_nonzero) = NonZeroUsize::new(handle_width) else {
                    return None;
                };
                let handle_lane_len = match arity.checked_mul(handle_width) {
                    Some(length) => length,
                    None => {
                        return Some(Err(ctx.refuse_codec_limit(
                            "catia_standard_edge_handle_lane",
                            u64::MAX,
                            u64::MAX,
                        )))
                    }
                };
                let Some(handle_lane_end) = position.checked_add(handle_lane_len) else {
                    return Some(Err(ctx.refuse_codec_limit(
                        "catia_standard_edge_handle_lane",
                        u64::MAX,
                        u64::MAX,
                    )));
                };
                let Some(handle_lane) = bytes.get(position..handle_lane_end) else {
                    return None;
                };
                let admitted_handle_bytes =
                    match ctx.admit_iter(handle_lane, "catia_standard_edge_handle_lane") {
                        Ok(bytes) => bytes.chunks(handle_width_nonzero),
                        Err(error) => return Some(Err(CodecError::from(error))),
                    };
                for handle_bytes in admitted_handle_bytes {
                    let Some(handle) = read_handle(handle_bytes, 0, handle_width) else {
                        return None;
                    };
                    handles.push(handle);
                }
                position = handle_lane_end;
                if let Err(error) = ctx.push_vec(
                    &mut rows,
                    EdgeRow::new(
                        kind,
                        handles,
                        if arity == 2 {
                            EdgeBoundaryLayout::CompleteBoundaryRun
                        } else {
                            EdgeBoundaryLayout::InteriorWithFlankingCorners
                        },
                    )?,
                    "catia_standard_edge_rows",
                ) {
                    return Some(Err(error));
                }
                if let Err(error) = ctx.push_vec(&mut scopes, scope, "catia_standard_edge_scopes") {
                    return Some(Err(error));
                }
            }
            let mut saw_delimiter = false;
            while bytes.get(position..)?.starts_with(&EDGE_DELIMITER) {
                if let Err(error) = ctx.charge_work(1, "catia_standard_edge_delimiters") {
                    return Some(Err(error));
                }
                saw_delimiter = true;
                position += EDGE_DELIMITER.len();
            }
            if !saw_delimiter {
                return None;
            }
            if bytes.get(position..position + 2) == Some(&[0x01, 0x06]) {
                break;
            }
            scope = scope.checked_add(1)?;
        }
        Some(Ok((rows, scopes, position)))
    })()
    .transpose()
}

pub(crate) fn parse_vertex_table(
    ctx: &DecodeContext<'_>,
    bytes: &[u8],
    position: usize,
) -> Result<Option<Vec<[f64; 3]>>, CodecError> {
    let mut scratch = ctx.reserve_scoped(0, "catia_fbb_vertex_points")?;
    let Some(points) = scratch.with_storage(|| parse_vertex_points(ctx, bytes, position))? else {
        return Ok(None);
    };
    let mut coordinates = Vec::new();
    ctx.reserve_vec(
        &mut coordinates,
        points.len(),
        "catia_fbb_vertex_coordinates",
    )?;
    for point in ctx.admit_iter(&points, "catia_fbb_vertex_coordinates")? {
        let coordinate: [f64; 3] = point.get().into();
        coordinates.push(coordinate);
    }
    Ok(Some(coordinates))
}

/// The counted `05 08 01` vertex table at `position`, every coordinate
/// admitted finite.
fn parse_vertex_points(
    ctx: &DecodeContext<'_>,
    bytes: &[u8],
    mut position: usize,
) -> Result<Option<Vec<FinitePoint3>>, CodecError> {
    let Some(rest) = bytes.get(position..) else {
        return Ok(None);
    };
    if !rest.starts_with(&[0x01, 0x06]) {
        return Ok(None);
    }
    position += 2;
    let Some(count) = parse_count(bytes, &mut position) else {
        return Ok(None);
    };
    if count > bytes.get(position..).map_or(0, <[u8]>::len) / VERTEX_RECORD_BYTES {
        return Ok(None);
    }
    let vertex_lane_len = match count.checked_mul(VERTEX_RECORD_BYTES) {
        Some(length) => length,
        None => {
            return Err(ctx.refuse_codec_limit("catia_fbb_vertex_record_lane", u64::MAX, u64::MAX))
        }
    };
    let Some(vertex_lane_end) = position.checked_add(vertex_lane_len) else {
        return Err(ctx.refuse_codec_limit("catia_fbb_vertex_record_lane", u64::MAX, u64::MAX));
    };
    let Some(vertex_lane) = bytes.get(position..vertex_lane_end) else {
        return Ok(None);
    };
    let Some(record_width) = NonZeroUsize::new(VERTEX_RECORD_BYTES) else {
        return Ok(None);
    };
    let mut points = Vec::new();
    ctx.reserve_vec(&mut points, count, "catia_fbb_vertex_points")?;
    for record in ctx
        .admit_iter(vertex_lane, "catia_fbb_vertex_record_lane")?
        .chunks(record_width)
    {
        if record.get(..3) != Some(&[0x05, 0x08, 0x01][..]) {
            return Ok(None);
        }
        let mut coordinate_position = 3;
        let mut coordinates = [FiniteReal::ZERO; 3];
        for coordinate in &mut coordinates {
            let Some(value) = View::f32_le_at(record, coordinate_position) else {
                return Ok(None);
            };
            let Some(finite) = FiniteReal::new(f64::from(value)) else {
                return Ok(None);
            };
            *coordinate = finite;
            coordinate_position += size_of::<f32>();
        }
        let [x, y, z] = coordinates;
        points.push(FinitePoint3::from_coordinates(x, y, z));
    }
    Ok(Some(points))
}

pub(crate) fn parse_trim_chain(
    ctx: &DecodeContext<'_>,
    bytes: &[u8],
    end: usize,
    record_count: usize,
    width: usize,
) -> Result<Option<Vec<TrimRecord>>, CodecError> {
    Ok(parse_trim_chain_start(ctx, bytes, end, record_count, width)?.map(|(_, records)| records))
}

fn parse_trim_chain_start(
    ctx: &DecodeContext<'_>,
    bytes: &[u8],
    end: usize,
    record_count: usize,
    width: usize,
) -> Result<Option<(usize, Vec<TrimRecord>)>, CodecError> {
    let compact =
        parse_trim_chain_with_length_encoding(ctx, bytes, end, record_count, width, false)?;
    let wide_u16be = if width == 2 {
        parse_trim_chain_with_length_encoding(ctx, bytes, end, record_count, width, true)?
    } else {
        None
    };
    Ok(match (compact, wide_u16be) {
        (Some((compact_start, compact)), Some((wide_start, wide))) => (compact_start == wide_start
            && ctx.equal(&compact, &wide, "catia_trim_chain_encodings")?)
        .then_some((compact_start, compact)),
        (Some(records), None) | (None, Some(records)) => Some(records),
        (None, None) => None,
    })
}

fn parse_trim_chain_with_length_encoding(
    ctx: &DecodeContext<'_>,
    bytes: &[u8],
    end: usize,
    record_count: usize,
    width: usize,
    wide_u16be: bool,
) -> Result<Option<(usize, Vec<TrimRecord>)>, CodecError> {
    struct Frame {
        end: usize,
        remaining: usize,
        next_predecessor: usize,
    }

    fn backtrack(frames: &mut Vec<Frame>, reversed: &mut Vec<TrimRecord>) {
        let had_parent = frames.len() > 1;
        frames.pop();
        if had_parent {
            reversed.pop();
        }
    }

    let Some(prefix) = bytes.get(..end) else {
        return Ok(None);
    };
    let mut predecessor_storage = ctx.reserve_scoped(0, "catia_trim_predecessor_ends")?;
    let mut predecessors = HashMap::<usize, Vec<usize>>::new();
    let Some(marker_width) = NonZeroUsize::new(2) else {
        return Ok(None);
    };
    for (start, marker) in ctx
        .admit_iter(prefix, "catia_standard_iteration")?
        .windows(marker_width)
        .enumerate()
    {
        if marker[0] != 0x01 || !TRIM_KINDS.contains(&marker[1]) {
            continue;
        }
        if let Some(layout) =
            parse_trim_record_layout_with_length_encoding(ctx, prefix, start, width, wide_u16be)?
        {
            predecessor_storage.with_storage(|| {
                ctx.push_hash_group(
                    &mut predecessors,
                    layout.end,
                    start,
                    "catia_trim_predecessor_ends",
                    "catia_trim_predecessor_starts",
                )
            })?;
        }
    }

    let mut solutions = Vec::new();
    let mut reversed = Vec::<TrimRecord>::new();
    ctx.reserve_vec(&mut reversed, record_count, "catia_trim_reversed")?;
    let mut frames = Vec::new();
    ctx.push_vec(
        &mut frames,
        Frame {
            end,
            remaining: record_count,
            next_predecessor: 0,
        },
        "catia_trim_search_frames",
    )?;
    while !frames.is_empty() && solutions.len() <= 1 {
        let frame = frames.len() - 1;
        if frames[frame].remaining == 0 {
            let chain_start = frames[frame].end;
            let mut records = Vec::new();
            ctx.reserve_vec(&mut records, reversed.len(), "catia_trim_solution_records")?;
            for record in ctx.admit_iter(&reversed, "catia_standard_iteration")?.rev() {
                records.push(TrimRecord {
                    packet: record.packet.try_clone_with_context(ctx)?,
                    frame_vector: record.frame_vector,
                    kind: record.kind,
                });
            }
            ctx.push_vec(
                &mut solutions,
                (chain_start, records),
                "catia_trim_solutions",
            )?;
            backtrack(&mut frames, &mut reversed);
            continue;
        }
        let predecessor = ctx
            .get_hash_map(
                &predecessors,
                &frames[frame].end,
                "catia_trim_predecessor_ends",
            )?
            .and_then(|records| records.get(frames[frame].next_predecessor))
            .copied();
        let Some(start) = predecessor else {
            backtrack(&mut frames, &mut reversed);
            continue;
        };
        frames[frame].next_predecessor += 1;
        let Some(record) =
            parse_trim_record_with_length_encoding(ctx, prefix, start, width, wide_u16be)?
        else {
            continue;
        };
        let remaining = frames[frame].remaining - 1;
        ctx.push_vec(&mut reversed, record, "catia_trim_reversed")?;
        ctx.push_vec(
            &mut frames,
            Frame {
                end: start,
                remaining,
                next_predecessor: 0,
            },
            "catia_trim_search_frames",
        )?;
    }
    Ok(<[(usize, Vec<TrimRecord>); 1]>::try_from(solutions)
        .ok()
        .map(|[solution]| solution))
}

#[derive(Debug, Clone, PartialEq)]
enum TrimLengthLane {
    Decoded(Vec<usize>),
    PackedTwoStrip,
}

#[derive(Debug, Clone, PartialEq)]
pub(in crate::families::standard) struct TrimRecordLayout {
    kind: u8,
    independent_count: usize,
    strip_count: usize,
    lane: TrimLengthLane,
    frame_vector: Option<FiniteVector<3>>,
    pub(super) handle_offset: usize,
    pub(super) handle_count: usize,
    pub(super) end: usize,
}

#[cfg(test)]
pub(super) fn parse_trim_record_layout(
    ctx: &DecodeContext<'_>,
    bytes: &[u8],
    start: usize,
    width: usize,
) -> Result<Option<TrimRecordLayout>, CodecError> {
    let compact = parse_trim_record_layout_with_length_encoding(ctx, bytes, start, width, false)?;
    let wide_u16be = if width == 2 {
        parse_trim_record_layout_with_length_encoding(ctx, bytes, start, width, true)?
    } else {
        None
    };
    Ok(match (compact, wide_u16be) {
        (Some(compact), Some(wide)) if compact == wide => Some(compact),
        (Some(layout), None) | (None, Some(layout)) => Some(layout),
        (None, None) | (Some(_), Some(_)) => None,
    })
}

fn parse_trim_record_layout_with_length_encoding(
    ctx: &DecodeContext<'_>,
    bytes: &[u8],
    start: usize,
    width: usize,
    wide_u16be: bool,
) -> Result<Option<TrimRecordLayout>, CodecError> {
    (|| -> Option<Result<TrimRecordLayout, CodecError>> {
        if wide_u16be && width != 2 {
            return None;
        }
        if bytes.get(start) != Some(&0x01) {
            return None;
        }
        let Some(kind_position) = start.checked_add(1) else {
            return Some(Err(ctx.refuse_codec_limit(
                "catia_trim_kind_position",
                u64::MAX,
                u64::MAX,
            )));
        };
        let kind = *bytes.get(kind_position)?;
        if !TRIM_KINDS.contains(&kind) {
            return None;
        }
        let mask = kind & 0x0f;
        let Some(mut position) = start.checked_add(2) else {
            return Some(Err(ctx.refuse_codec_limit(
                "catia_trim_initial_position",
                u64::MAX,
                u64::MAX,
            )));
        };
        let a = if mask & 1 != 0 {
            parse_count(bytes, &mut position)?
        } else {
            0
        };
        let b_start = position;
        let b = if mask & 2 != 0 {
            parse_count(bytes, &mut position)?
        } else {
            0
        };
        let c = if mask & 4 != 0 {
            parse_count(bytes, &mut position)?
        } else {
            0
        };
        if bytes.get(position) != Some(&0xff) {
            return None;
        }
        let Some(next_position) = position.checked_add(1) else {
            return Some(Err(ctx.refuse_codec_limit(
                "catia_trim_handle_count_position",
                u64::MAX,
                u64::MAX,
            )));
        };
        position = next_position;
        let handle_count = usize::try_from(View::u32_le_at(bytes, position)?).ok()?;
        let Some(next_position) = position.checked_add(4) else {
            return Some(Err(ctx.refuse_codec_limit(
                "catia_trim_frame_vector_position",
                u64::MAX,
                u64::MAX,
            )));
        };
        position = next_position;
        if handle_count == 0 {
            return None;
        }
        let frame_vector = if mask & 8 != 0 {
            let Some(second_component_position) = position.checked_add(4) else {
                return Some(Err(ctx.refuse_codec_limit(
                    "catia_trim_frame_component_position",
                    u64::MAX,
                    u64::MAX,
                )));
            };
            let Some(third_component_position) = position.checked_add(8) else {
                return Some(Err(ctx.refuse_codec_limit(
                    "catia_trim_frame_component_position",
                    u64::MAX,
                    u64::MAX,
                )));
            };
            let components = [
                f64::from(View::f32_le_at(bytes, position)?),
                f64::from(View::f32_le_at(bytes, second_component_position)?),
                f64::from(View::f32_le_at(bytes, third_component_position)?),
            ];
            let Some(next_position) = position.checked_add(12) else {
                return Some(Err(ctx.refuse_codec_limit(
                    "catia_trim_length_position",
                    u64::MAX,
                    u64::MAX,
                )));
            };
            position = next_position;
            let norm2 = components.iter().map(|value| value * value).sum::<f64>();
            let components = FiniteVector::new(components)?;
            if (norm2 - 1.0).abs() >= FRAME_VECTOR_NORM2_TOLERANCE {
                return None;
            }
            Some(components)
        } else {
            None
        };

        // A two-strip packet stores K0 and K1 as two raw bytes before the H lane.
        // The bytes are not a handle.  At width two they happen to occupy one
        // handle-sized slot; at width three they do not, so sizing the lane as
        // `(N + 1) * width` would consume one byte from the next packet.
        let packed_two_strip_lengths =
            kind == 0x42 && b == 2 && bytes.get(b_start).is_some_and(|encoded| *encoded == 2);
        let Some(primitive_count) = b.checked_add(c) else {
            return Some(Err(ctx.refuse_codec_limit(
                "catia_trim_primitive_count",
                u64::MAX,
                u64::MAX,
            )));
        };
        if !packed_two_strip_lengths
            && primitive_count > bytes.get(position..).map_or(0, <[u8]>::len)
        {
            return None;
        }
        let lane = if packed_two_strip_lengths {
            TrimLengthLane::PackedTwoStrip
        } else {
            let mut lengths = Vec::new();
            if wide_u16be {
                let length_lane_len = match primitive_count.checked_mul(size_of::<u16>()) {
                    Some(length) => length,
                    None => {
                        return Some(Err(ctx.refuse_codec_limit(
                            "catia_trim_wide_length_lane",
                            u64::MAX,
                            u64::MAX,
                        )))
                    }
                };
                let Some(length_lane_end) = position.checked_add(length_lane_len) else {
                    return Some(Err(ctx.refuse_codec_limit(
                        "catia_trim_wide_length_lane",
                        u64::MAX,
                        u64::MAX,
                    )));
                };
                let Some(length_lane) = bytes.get(position..length_lane_end) else {
                    return None;
                };
                let Some(length_width) = NonZeroUsize::new(size_of::<u16>()) else {
                    return None;
                };
                if let Err(error) = ctx.reserve_vec(
                    &mut lengths,
                    primitive_count,
                    "catia_trim_primitive_lengths",
                ) {
                    return Some(Err(error));
                }
                let admitted_lengths =
                    match ctx.admit_iter(length_lane, "catia_trim_wide_length_lane") {
                        Ok(bytes) => bytes.chunks(length_width),
                        Err(error) => return Some(Err(CodecError::from(error))),
                    };
                for encoded_length in admitted_lengths {
                    let Some(length) = View::u16_be_at(encoded_length, 0) else {
                        return None;
                    };
                    lengths.push(usize::from(length));
                }
                position = length_lane_end;
            } else {
                // Each varint length consumes input bytes, so the declared
                // count is walked one charged length at a time.
                let mut lengths_left = 0..primitive_count;
                loop {
                    match ctx.next_charged(&mut lengths_left, "catia_trim_primitive_lengths") {
                        Ok(Some(_)) => {}
                        Ok(None) => break,
                        Err(error) => return Some(Err(error)),
                    }
                    let length = parse_count(bytes, &mut position)?;
                    if let Err(error) =
                        ctx.push_vec(&mut lengths, length, "catia_trim_primitive_lengths")
                    {
                        return Some(Err(error));
                    }
                }
            }
            let mut length_total = 0_usize;
            let admitted_lengths = match ctx.admit_iter(&lengths, "catia_trim_length_totals") {
                Ok(lengths) => lengths,
                Err(error) => return Some(Err(CodecError::from(error))),
            };
            for length in admitted_lengths {
                let Some(next) = length_total.checked_add(*length) else {
                    return Some(Err(ctx.refuse_codec_limit(
                        "catia_trim_length_total",
                        u64::MAX,
                        u64::MAX,
                    )));
                };
                length_total = next;
            }
            let Some(expected_handles) = 3_usize
                .checked_mul(a)
                .and_then(|count| count.checked_add(length_total))
            else {
                return Some(Err(ctx.refuse_codec_limit(
                    "catia_trim_expected_handle_count",
                    u64::MAX,
                    u64::MAX,
                )));
            };
            if expected_handles != handle_count {
                return None;
            }
            TrimLengthLane::Decoded(lengths)
        };
        let handle_offset = position;
        let handle_bytes = match handle_count.checked_mul(width) {
            Some(bytes) => bytes,
            None => {
                return Some(Err(ctx.refuse_codec_limit(
                    "catia_trim_handle_bytes",
                    u64::MAX,
                    u64::MAX,
                )))
            }
        };
        let byte_count = match &lane {
            TrimLengthLane::PackedTwoStrip => match 2_usize.checked_add(handle_bytes) {
                Some(bytes) => bytes,
                None => {
                    return Some(Err(ctx.refuse_codec_limit(
                        "catia_trim_packed_handle_bytes",
                        u64::MAX,
                        u64::MAX,
                    )))
                }
            },
            TrimLengthLane::Decoded(_) => handle_bytes,
        };
        let Some(end) = handle_offset.checked_add(byte_count) else {
            return Some(Err(ctx.refuse_codec_limit(
                "catia_trim_handle_end",
                u64::MAX,
                u64::MAX,
            )));
        };
        bytes.get(handle_offset..end)?;
        Some(Ok(TrimRecordLayout {
            kind,
            independent_count: a,
            strip_count: b,
            lane,
            frame_vector,
            handle_offset,
            handle_count,
            end,
        }))
    })()
    .transpose()
}

#[cfg(test)]
pub(super) fn parse_trim_record(
    ctx: &DecodeContext<'_>,
    bytes: &[u8],
    start: usize,
    width: usize,
) -> Result<Option<TrimRecord>, CodecError> {
    let compact = parse_trim_record_with_length_encoding(ctx, bytes, start, width, false)?;
    let wide_u16be = if width == 2 {
        parse_trim_record_with_length_encoding(ctx, bytes, start, width, true)?
    } else {
        None
    };
    Ok(match (compact, wide_u16be) {
        (Some(compact), Some(wide)) if compact == wide => Some(compact),
        (Some(record), None) | (None, Some(record)) => Some(record),
        (None, None) | (Some(_), Some(_)) => None,
    })
}

fn parse_trim_record_with_length_encoding(
    ctx: &DecodeContext<'_>,
    bytes: &[u8],
    start: usize,
    width: usize,
    wide_u16be: bool,
) -> Result<Option<TrimRecord>, CodecError> {
    let Some(layout) =
        parse_trim_record_layout_with_length_encoding(ctx, bytes, start, width, wide_u16be)?
    else {
        return Ok(None);
    };
    (|| -> Option<Result<TrimRecord, CodecError>> {
        let mut position = layout.handle_offset;
        let lengths = match layout.lane {
            TrimLengthLane::Decoded(lengths) => lengths,
            TrimLengthLane::PackedTwoStrip => {
                let Some(packed_end) = position.checked_add(2) else {
                    return Some(Err(ctx.refuse_codec_limit(
                        "catia_trim_packed_length_end",
                        u64::MAX,
                        u64::MAX,
                    )));
                };
                let packed = bytes.get(position..packed_end)?;
                position += 2;
                if usize::from(packed[0]) + usize::from(packed[1]) != layout.handle_count {
                    return None;
                }
                let mut lengths = Vec::new();
                if let Err(error) = ctx.reserve_vec(&mut lengths, 2, "catia_trim_packed_lengths") {
                    return Some(Err(error));
                }
                lengths.push(usize::from(packed[0]));
                lengths.push(usize::from(packed[1]));
                lengths
            }
        };
        let mut handles = Vec::new();
        if let Err(error) = ctx.reserve_vec(
            &mut handles,
            layout.handle_count,
            "catia_trim_packet_handles",
        ) {
            return Some(Err(error));
        }
        let Some(handle_width) = NonZeroUsize::new(width) else {
            return None;
        };
        let Some(handle_lane) = bytes.get(position..layout.end) else {
            return None;
        };
        let admitted_handle_bytes =
            match ctx.admit_iter(handle_lane, "catia_trim_packet_handle_lane") {
                Ok(bytes) => bytes.chunks(handle_width),
                Err(error) => return Some(Err(CodecError::from(error))),
            };
        for handle_bytes in admitted_handle_bytes {
            let Some(handle) = read_handle(handle_bytes, 0, width) else {
                return None;
            };
            handles.push(handle);
        }

        if layout.strip_count > lengths.len() {
            return None;
        }
        let mut strip_lengths = lengths;
        let fan_lengths = match ctx.split_off_vec(
            &mut strip_lengths,
            layout.strip_count,
            "catia_trim_fan_lengths",
        ) {
            Ok(lengths) => lengths,
            Err(error) => return Some(Err(error)),
        };
        let packet = match TrimPacket::try_from(
            ctx,
            (
                layout.independent_count,
                strip_lengths,
                fan_lengths,
                handles,
            ),
        ) {
            Ok(Some(packet)) => packet,
            Ok(None) => return None,
            Err(error) => return Some(Err(error)),
        };
        Some(Ok(TrimRecord {
            packet,
            frame_vector: layout.frame_vector,
            kind: layout.kind,
        }))
    })()
    .transpose()
}

pub(crate) fn boundary_cycles(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    triangles: &[[u32; 3]],
) -> Result<Option<Vec<Vec<u32>>>, cadmpeg_core::CodecError> {
    let mut edge_directions = BTreeMap::<(u32, u32), u8>::new();
    for &[a, b, c] in ctx.admit_iter(triangles, "catia_standard_iteration")? {
        for (start, end) in [(a, b), (b, c), (c, a)] {
            if start == end {
                return Ok(None);
            }
            let (edge, direction) = if start < end {
                ((start, end), 1)
            } else {
                ((end, start), 2)
            };
            if let Some(directions) = ctx.get_mut_btree_map(
                &mut edge_directions,
                &edge,
                "catia_boundary_edge_directions",
            )? {
                if *directions & direction != 0 {
                    return Ok(None);
                }
                *directions |= direction;
            } else {
                ctx.insert_btree_map(
                    &mut edge_directions,
                    edge,
                    direction,
                    "catia_boundary_edge_directions",
                )?;
            }
        }
    }
    let mut successors = BTreeMap::new();
    for (&(low, high), &directions) in
        ctx.admit_iter(&edge_directions, "catia_standard_iteration")?
    {
        let boundary = match directions {
            1 => Some((low, high)),
            2 => Some((high, low)),
            3 => None,
            _ => return Ok(None),
        };
        if let Some((start, end)) = boundary {
            if ctx
                .insert_btree_map(&mut successors, start, end, "catia_boundary_successors")?
                .is_some()
            {
                return Ok(None);
            }
        }
    }
    // Starts are visited in ascending order, so each cycle is first reached
    // from its smallest handle and cycles are produced in ascending order of
    // their first handle.
    let mut seen = HashSet::new();
    let mut cycles = Vec::new();
    for (&start, &first) in ctx.admit_iter(&successors, "catia_standard_iteration")? {
        if ctx.contains_hash_set(&seen, &start, "catia_boundary_seen")? {
            continue;
        }
        let mut cycle = Vec::new();
        ctx.push_vec(&mut cycle, start, "catia_boundary_cycle_handles")?;
        ctx.insert_hash_set(&mut seen, start, "catia_boundary_seen")?;
        let mut current = first;
        while current != start {
            if !ctx.insert_hash_set(&mut seen, current, "catia_boundary_seen")? {
                return Ok(None);
            }
            ctx.push_vec(&mut cycle, current, "catia_boundary_cycle_handles")?;
            let Some(&next) =
                ctx.get_btree_map(&successors, &current, "catia_boundary_successors")?
            else {
                return Ok(None);
            };
            current = next;
        }
        ctx.push_vec(&mut cycles, cycle, "catia_boundary_cycles")?;
    }
    Ok((!cycles.is_empty()).then_some(cycles))
}

/// Rows indexed by the first and last handle of their boundary pattern, so a
/// cycle position is compared only with the rows that can start there.
pub(super) struct RowPatternEnds {
    by_first: HashMap<u32, Vec<usize>>,
    by_last: HashMap<u32, Vec<usize>>,
}

pub(super) fn row_pattern_ends(
    ctx: &DecodeContext<'_>,
    rows: &[EdgeRow],
) -> Result<RowPatternEnds, CodecError> {
    const OPERATION: &str = "catia_fbb_row_pattern_ends";
    let mut ends = RowPatternEnds {
        by_first: HashMap::new(),
        by_last: HashMap::new(),
    };
    for (row, value) in ctx.admit_iter(rows, OPERATION)?.enumerate() {
        let Some(pattern) = value.boundary_pattern() else {
            continue;
        };
        let (Some(&first), Some(&last)) = (pattern.first(), pattern.last()) else {
            continue;
        };
        ctx.push_hash_group(&mut ends.by_first, first, row, OPERATION, OPERATION)?;
        if last != first {
            ctx.push_hash_group(&mut ends.by_last, last, row, OPERATION, OPERATION)?;
        }
    }
    Ok(ends)
}

pub(super) fn cover_cycle(
    ctx: &DecodeContext<'_>,
    cycle: &[u32],
    rows: &[EdgeRow],
    ends: &RowPatternEnds,
    union: &mut UnionFind,
) -> Result<Option<BoundaryDraft>, CodecError> {
    cover_cycle_by_rows(ctx, cycle, rows, ends, union)
}

fn cover_cycle_by_rows(
    ctx: &DecodeContext<'_>,
    cycle: &[u32],
    rows: &[EdgeRow],
    ends: &RowPatternEnds,
    union: &mut UnionFind,
) -> Result<Option<BoundaryDraft>, CodecError> {
    const OPERATION: &str = "catia_fbb_cycle_row_matches";
    let length = cycle.len();
    let mut storage = ctx.reserve_scoped(0, OPERATION)?;
    let reads = |pattern: &[u32], start: usize, reversed: bool| {
        ctx.all_by(
            pattern.iter().enumerate(),
            |(offset, &handle)| {
                let expected = if reversed {
                    pattern[pattern.len() - 1 - offset]
                } else {
                    handle
                };
                Ok(cycle[(start + offset) % length] == expected)
            },
            OPERATION,
        )
    };
    // A row matches where its pattern reads forward from its first handle or
    // backward from its last one; forward wins at a shared start. A row that
    // matches at two starts leaves the cycle unresolved.
    let mut row_matches = BTreeMap::<usize, (usize, bool)>::new();
    for (start, handle) in ctx.admit_iter(cycle, OPERATION)?.enumerate() {
        for (rows_here, from_first) in [(&ends.by_first, true), (&ends.by_last, false)] {
            let Some(candidates) = ctx.get_hash_map(rows_here, handle, OPERATION)? else {
                continue;
            };
            for &edge_row in ctx.admit_iter(candidates, OPERATION)? {
                let Some(pattern) = rows[edge_row].boundary_pattern() else {
                    continue;
                };
                let reversed = if from_first && reads(pattern, start, false)? {
                    false
                } else if (!from_first || pattern.first() == pattern.last())
                    && reads(pattern, start, true)?
                {
                    true
                } else {
                    continue;
                };
                if !ctx.insert_scoped_btree_map_if_vacant(
                    &mut storage,
                    &mut row_matches,
                    edge_row,
                    (start, reversed),
                    OPERATION,
                    OPERATION,
                )? {
                    return Ok(None);
                }
            }
        }
    }
    let mut matches = Vec::new();
    for (&edge_row, &(start, reversed)) in ctx.admit_iter(&row_matches, OPERATION)? {
        let Some((boundary_start, segment_count)) = rows[edge_row].boundary_span(start, length)
        else {
            return Ok(None);
        };
        ctx.push_vec(
            &mut matches,
            (boundary_start, segment_count, edge_row, reversed),
            "catia_fbb_cycle_matches",
        )?;
    }
    if matches.is_empty() {
        return Ok(None);
    }
    let mut coverage = ctx.alloc_filled(length, 0_u8, "catia FBB boundary coverage")?;
    for &(start, edge_count, edge_row, _) in ctx.admit_iter(&matches, "catia_standard_iteration")? {
        let Some(row) = rows.get(edge_row) else {
            return Ok(None);
        };
        // A row with N + 1 handles represents exactly N boundary steps. Use
        // that existing input slice to admit the cyclic coverage traversal;
        // this also preserves rows that span the cycle more than once.
        let Some(boundary_steps) = row.handles().get(..edge_count) else {
            return Ok(None);
        };
        for (offset, _) in ctx
            .admit_iter(boundary_steps, "catia_fbb_boundary_coverage_steps")?
            .enumerate()
        {
            let index = (start + offset) % length;
            let Some(count) = coverage[index].checked_add(1) else {
                return Ok(None);
            };
            coverage[index] = count;
        }
    }
    if ctx.any_by(
        &coverage,
        |count| Ok(*count != 1),
        "catia_standard_iteration",
    )? {
        return Ok(None);
    }
    ctx.stable_sort_by_key(
        &mut matches,
        |value| value.0 % length,
        Ord::cmp,
        "catia_cover_cycle_rows_sort",
    )?;
    let mut corner_nodes = HashMap::new();
    for &(start, edge_count, _, _) in ctx.admit_iter(&matches, "catia_standard_iteration")? {
        let end = (start + edge_count) % length;
        for corner in [start % length, end] {
            if !ctx.contains_key_hash_map(&corner_nodes, &corner, "catia_fbb_corner_nodes")? {
                let node = union.push_charged(ctx, "catia_fbb_corner_union_nodes")?;
                storage.with_storage(|| {
                    ctx.insert_hash_map(&mut corner_nodes, corner, node, "catia_fbb_corner_nodes")
                })?;
            }
        }
    }
    let corner_node = |corner: usize| -> Result<usize, CodecError> {
        ctx.get_hash_map(&corner_nodes, &corner, "catia_fbb_corner_nodes")?
            .copied()
            .ok_or_else(|| CodecError::malformed("FBB cycle corner has no node"))
    };
    let mut coedges = Vec::new();
    ctx.reserve_vec(&mut coedges, matches.len(), "catia_fbb_cycle_coedges")?;
    for &(start, edge_count, edge_row, reversed) in
        ctx.admit_iter(&matches, "catia_standard_iteration")?
    {
        let start_node = corner_node(start % length)?;
        let end_node = corner_node((start + edge_count) % length)?;
        let edge_start = edge_row * 2;
        let edge_end = edge_start + 1;
        if reversed {
            union.union(ctx, edge_end, start_node)?;
            union.union(ctx, edge_start, end_node)?;
        } else {
            union.union(ctx, edge_start, start_node)?;
            union.union(ctx, edge_end, end_node)?;
        }
        coedges.push(CoedgeUse {
            edge_row,
            reversed,
            start_vertex: start_node,
            end_vertex: end_node,
        });
    }
    Ok(BoundaryDraft::new(coedges))
}

#[cfg(test)]
mod endpoint_tests {
    use super::parse_fbb_endpoints_with_edge_classes;

    fn synthetic_fbb_triangle() -> Vec<u8> {
        let mut bytes = vec![0x01, 0x41, 0x01, 0xff];
        bytes.extend_from_slice(&3u32.to_le_bytes());
        bytes.extend_from_slice(&[0, 1, 2]);

        bytes.extend_from_slice(&[0x30, 0x04, 0x04, 0xff, 0, 0, 0, 0]);
        bytes.extend_from_slice(&[0x30, 0x04, 0x04, 0xff, 0, 0, 0, 0]);

        bytes.extend_from_slice(&[0x01, 0x01, 0x03]);
        for handles in [[0, 1], [1, 2], [2, 0]] {
            bytes.extend_from_slice(&[0x02, 0x02]);
            bytes.extend_from_slice(&handles);
        }
        bytes.extend_from_slice(&[
            0x10, 0x24, 0x04, 0xff, 0xff, 0x00, 0x00, 0x00, 0x01, 0x02, 0x00, 0x10, 0x24, 0x04,
            0xff, 0xff, 0x00, 0x00, 0x00, 0x01, 0x06, 0x03,
        ]);
        for point in [[0.0_f32, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]] {
            bytes.extend_from_slice(&[0x05, 0x08, 0x01]);
            for coordinate in point {
                bytes.extend_from_slice(&coordinate.to_le_bytes());
            }
        }
        bytes
    }

    #[test]
    fn fbb_endpoint_reconstruction_uses_the_native_edge_pairs() {
        let arena = cadmpeg_core::decode::DecodeArena::new();
        let policy = cadmpeg_core::decode::DecodePolicy::service();
        let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(&[0], &arena, &policy)
            .expect("fixture fits the input limit");
        let bytes = synthetic_fbb_triangle();
        let topology = parse_fbb_endpoints_with_edge_classes(
            &ctx,
            &bytes,
            &[[0, 1], [0, 1], [0, 1]],
            &[[0, 1], [1, 2], [0, 2]],
            Some(&[0, 1, 2]),
        )
        .expect("service resource budget")
        .expect("native endpoint pairs close the FBB face");

        assert_eq!(topology.face_count(), 2);
        assert_eq!(topology.edge_rows().len(), 3);
        assert_eq!(
            topology.vertex_points(),
            &[[0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]]
        );
        assert_eq!(
            topology
                .edge_vertices(&ctx)
                .expect("service resource budget"),
            Some(vec![[0, 1], [1, 2], [0, 2]])
        );
    }
}

#[cfg(test)]
mod tests {
    use super::{boundary_cycles, fbb_row, FbbFaceRun};

    #[test]
    fn fbb_pattern_scans_preserve_both_directions_and_refuse_caller_work() {
        let count = |cycle: &[u32], pattern: &[u32]| {
            crate::test_support::with_service_context(|ctx| {
                let mut storage = ctx.reserve_scoped(0, "test cycle index")?;
                let cycle = super::index_cycle(ctx, &mut storage, cycle)?;
                super::pattern_match_count(ctx, &cycle, pattern)
            })
            .expect("service budget")
        };
        assert_eq!(count(&[1, 2, 3], &[3, 1]), 1);
        assert_eq!(count(&[1, 2, 3], &[1, 3]), 1);
        assert_eq!(count(&[1, 2, 3], &[2, 2]), 0);
        assert_eq!(count(&[1, 2, 1, 2], &[1, 2, 1]), 2);
        assert_eq!(count(&[1, 2, 1, 2], &[2, 1]), 4);
        for operation in ["catia_fbb_cycle_handle_index", "catia_fbb_pattern_match"] {
            super::work_refusal_at(operation, |ctx| {
                let mut storage = ctx.reserve_scoped(0, "test cycle index")?;
                let cycle = super::index_cycle(ctx, &mut storage, &[1, 2, 3])?;
                super::pattern_match_count(ctx, &cycle, &[1, 2]).map(drop)
            });
        }
    }
    use std::collections::HashSet;

    #[test]
    fn face_run_admission_checks_byte_bounds() {
        assert!(FbbFaceRun::try_new(0, usize::MAX).is_none());
        assert!(FbbFaceRun::try_new(usize::MAX, 1).is_none());
        let run = FbbFaceRun::try_new(7, 2).expect("bounded face run");
        assert_eq!(run.face_start(), 7);
        assert_eq!(run.face_count(), 2);
        assert_eq!(run.after_faces(), 7 + 2 * fbb_row::LEN);
        let empty = FbbFaceRun::try_new(usize::MAX, 0).expect("empty face run");
        assert_eq!(empty.face_count(), 0);
        assert_eq!(empty.after_faces(), usize::MAX);
    }

    #[test]
    fn boundary_cycles_cancel_opposite_triangle_edges() {
        let triangles = [[0, 1, 2], [0, 2, 3]];
        assert_eq!(
            crate::test_support::with_service_context(|ctx| boundary_cycles(ctx, &triangles))
                .expect("service resource budget"),
            Some(vec![vec![0, 1, 2, 3]])
        );
    }

    #[test]
    fn boundary_cycles_reject_duplicate_directed_edges() {
        let triangles = [[0, 1, 2], [0, 1, 3]];
        assert_eq!(
            crate::test_support::with_service_context(|ctx| boundary_cycles(ctx, &triangles))
                .expect("service resource budget"),
            None
        );
    }

    #[test]
    fn boundary_cycles_refuse_each_counted_collection() {
        use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
        use cadmpeg_core::CodecError;

        let triangles = [[0, 1, 2], [0, 2, 3]];
        let mut operations = HashSet::new();
        for cap in 0..=48 {
            let arena = DecodeArena::new();
            let mut policy = DecodePolicy::service();
            policy.limits.max_collection_items = cap;
            let (ctx, _) = DecodeContext::from_root_bytes(&[0], &arena, &policy)
                .expect("fixture fits the input limit");
            match boundary_cycles(&ctx, &triangles) {
                Err(CodecError::ResourceLimit(limit)) => {
                    assert_eq!(limit.dimension, ResourceDimension::CollectionItems);
                    operations.insert(limit.operation);
                }
                Ok(Some(cycles)) => assert_eq!(cycles, vec![vec![0, 1, 2, 3]]),
                Ok(None) => panic!("two adjacent triangles must form a boundary"),
                Err(error) => panic!("unexpected boundary refusal: {error}"),
            }
        }
        for operation in [
            "catia_boundary_edge_directions",
            "catia_boundary_successors",
            "catia_boundary_cycle_handles",
            "catia_boundary_seen",
            "catia_boundary_cycles",
        ] {
            assert!(operations.contains(operation), "no refusal at {operation}");
        }
    }
}
