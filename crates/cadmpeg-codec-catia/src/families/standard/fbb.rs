//! Byte-level parsing for standard nested CATIA V5 B-rep (`FBB`) streams:
//! edge/vertex tables, trim records, packet triangles, and face parsers.

use cadmpeg_core::decode::{DecodeContext, View, WorkBudget};
use cadmpeg_core::CodecError;
use cadmpeg_ir::features::FinitePoint3;
use cadmpeg_ir::scalar::FiniteReal;
use cadmpeg_ir::units::FiniteVector;

use crate::families::standard::topology::{
    reconstruct, reconstruct_incidence, reconstruct_incidence_with_edge_classes_and_mesh, Boundary,
    CoedgeUse, EdgeBoundaryLayout, EdgeRow, StandardIncidenceEvidence, StandardTopology,
    TrimRecord,
};
use crate::families::standard::trim_packet::TrimPacket;
use crate::layout::fbb_face_row as fbb_row;
use crate::solve::incidence::{reconstruct_incidence_candidates, IncidenceEndpointDomains};
use crate::solve::mesh_quotient::MeshQuotient;
use crate::solve::missing_edge::{expand_deferred_edge_port_components, motif_port_points};
use crate::solve::union_find::UnionFind;
use std::collections::{HashMap, HashSet};
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
    if layouts.is_empty() || layouts.iter().any(|layout| layout.face_run == selected) {
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
    let Some(face_run) = largest_fbb_run(bytes) else {
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
    let count = face_run.face_count();
    let Some(marker) = bytes
        .get(start..start + fbb_row::ALPHA)
        .and_then(|bytes| <[u8; 4]>::try_from(bytes).ok())
    else {
        return Ok(None);
    };
    let mut colors = Vec::new();
    for index in 0..count {
        let Some(row) = bytes.get(start + index * fbb_row::LEN..start + (index + 1) * fbb_row::LEN)
        else {
            return Ok(None);
        };
        if row[..fbb_row::ALPHA] != marker {
            return Ok(None);
        }
        crate::resource::push(
            ctx,
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

fn trim_frame_vectors(
    ctx: &DecodeContext<'_>,
    bytes: &[u8],
    face_start: usize,
    face_count: usize,
) -> Result<Option<Vec<Option<FiniteVector<3>>>>, CodecError> {
    let mut solutions = Vec::new();
    for width in [1, 2, 3] {
        if let Some(records) = parse_trim_chain(ctx, bytes, face_start, face_count, width)? {
            crate::resource::push(ctx, &mut solutions, records, "catia_trim_width_solutions")?;
        }
    }
    let Ok([records]) = <[Vec<TrimRecord>; 1]>::try_from(solutions) else {
        return Ok(None);
    };
    let mut vectors = Vec::new();
    crate::resource::reserve_vec(ctx, &mut vectors, records.len(), "catia_trim_frame_vectors")?;
    for record in records {
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
    let runs = crate::container::fbb_run_ranges(bytes);
    if runs.len() > 1 {
        let mut combined = Vec::new();
        let mut complete = true;
        for range in &runs {
            let Some(vectors) =
                trim_frame_vectors(ctx, bytes, range.start, range.len() / fbb_row::LEN)?
            else {
                complete = false;
                break;
            };
            crate::resource::push(ctx, &mut combined, vectors, "catia_trim_population_frames")?;
        }
        if complete && combined.iter().map(Vec::len).sum::<usize>() == expected_face_count {
            let mut vectors = Vec::new();
            crate::resource::reserve_vec(
                ctx,
                &mut vectors,
                expected_face_count,
                "catia_trim_combined_frames",
            )?;
            for population in combined {
                vectors.extend(population);
            }
            return Ok(vectors);
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
    let Some(face_run) = largest_fbb_run(bytes) else {
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
) -> Result<Option<StandardTopology>, CodecError> {
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
) -> Result<Option<StandardTopology>, CodecError> {
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
    let edge_points = edge_rows
        .iter()
        .map(|row| {
            Some([
                *port_points.get(row.handles.first()?)?,
                *port_points.get(row.handles.last()?)?,
            ])
        })
        .collect::<Option<Vec<[usize; 2]>>>();
    let Some(edge_points) = edge_points else {
        return Ok(None);
    };
    let anchors_match = edge_points
        .iter()
        .zip(circle_anchors)
        .all(|(points, anchor)| {
            anchor.is_none_or(|mut anchor| {
                anchor.sort_unstable();
                let mut points = *points;
                points.sort_unstable();
                points == anchor
            })
        });
    if !anchors_match {
        return Ok(None);
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
) -> Result<Option<StandardTopology>, CodecError> {
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
        || edge_points
            .iter()
            .flatten()
            .any(|point| *point >= vertex_points.len())
    {
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
    if edge_ports.len() != edge_candidates.len() || edge_candidates.iter().any(Vec::is_empty) {
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
        crate::resource::copy_slice(ctx, deferred_edges, "catia_port_effective_deferred")?
    };
    if !expand_deferred_edge_port_components(ctx, edge_ports, &mut effective_deferred)? {
        return Ok(None);
    }
    let is_deferred = |edge: usize| effective_deferred[edge];
    let mut all_points = HashSet::new();
    for &point in edge_candidates.iter().flatten().flatten() {
        crate::resource::insert_set(ctx, &mut all_points, point, "catia_port_all_points")?;
    }
    let mut domains = Vec::new();
    for (edge, candidates) in edge_candidates.iter().enumerate() {
        let domain = Arc::new(if is_deferred(edge) {
            let mut copy = HashSet::new();
            crate::resource::reserve_set(
                ctx,
                &mut copy,
                all_points.len(),
                "catia_port_deferred_domain",
            )?;
            copy.extend(all_points.iter().copied());
            copy
        } else {
            let mut points = HashSet::new();
            for &point in candidates.iter().flatten() {
                crate::resource::insert_set(
                    ctx,
                    &mut points,
                    point,
                    "catia_port_candidate_domain",
                )?;
            }
            points
        });
        crate::resource::push(ctx, &mut domains, domain.clone(), "catia_port_domains")?;
        crate::resource::push(ctx, &mut domains, domain, "catia_port_domains")?;
    }
    let mut quotient = MeshQuotient::new_charged(ctx, domains)?;
    let mut node_by_port = HashMap::new();
    for (edge, ports) in edge_ports.iter().enumerate() {
        for (endpoint, port) in ports.iter().copied().enumerate() {
            let node = edge * 2 + endpoint;
            if let Some(&previous) = node_by_port.get(&port) {
                if quotient.merge_charged(ctx, previous, node)?.is_none() {
                    return Ok(None);
                }
            } else {
                crate::resource::insert_map(
                    ctx,
                    &mut node_by_port,
                    port,
                    node,
                    "catia_port_nodes",
                )?;
            }
        }
    }
    let mut constrained_candidates = Vec::new();
    crate::resource::reserve_vec(
        ctx,
        &mut constrained_candidates,
        edge_candidates.len(),
        "catia_port_constrained_edges",
    )?;
    for candidates in edge_candidates {
        constrained_candidates.push(crate::resource::copy_slice(
            ctx,
            candidates,
            "catia_port_constrained_pairs",
        )?);
    }
    for (edge, candidates) in constrained_candidates.iter_mut().enumerate() {
        if is_deferred(edge) {
            candidates.clear();
        }
    }
    if !quotient.edge_domains_viable(ctx, &constrained_candidates)? {
        return Ok(None);
    }
    let mut result = Vec::new();
    for (edge, candidates) in edge_candidates.iter().enumerate() {
        let left = quotient.find(edge * 2);
        let right = quotient.find(edge * 2 + 1);
        let mut filtered = Vec::new();
        for &pair in candidates {
            let supported = if left == right {
                pair[0] == pair[1] && quotient.domains()[left].contains(&pair[0])
            } else {
                (quotient.domains()[left].contains(&pair[0])
                    && quotient.domains()[right].contains(&pair[1]))
                    || (quotient.domains()[left].contains(&pair[1])
                        && quotient.domains()[right].contains(&pair[0]))
            };
            if supported {
                crate::resource::push(ctx, &mut filtered, pair, "catia_port_filtered_pairs")?;
            }
        }
        for pair in &mut filtered {
            pair.sort_unstable();
        }
        filtered.sort_unstable();
        filtered.dedup();
        if filtered.is_empty() {
            return Ok(None);
        }
        crate::resource::push(ctx, &mut result, filtered, "catia_port_filtered_edges")?;
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
) -> Result<Option<StandardTopology>, cadmpeg_core::CodecError> {
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
        || edge_candidates.iter().any(Vec::is_empty)
        || edge_candidates
            .iter()
            .flatten()
            .flatten()
            .any(|point| *point >= vertex_points.len())
    {
        return Ok(None);
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
) -> Result<Option<StandardTopology>, cadmpeg_core::CodecError> {
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
        || edge_candidates.iter().any(Vec::is_empty)
        || edge_candidates
            .iter()
            .flatten()
            .flatten()
            .any(|point| *point >= vertex_points.len())
    {
        return Ok(None);
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
) -> Result<Option<StandardTopology>, CodecError> {
    let Some(face_run) = largest_fbb_run(bytes) else {
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
        || edge_points
            .iter()
            .flatten()
            .any(|point| *point >= vertex_points.len())
    {
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
) -> Result<Option<(Vec<EdgeRow>, Vec<usize>, usize, usize)>, CodecError> {
    // FBB-only tables select one width by the complete table-and-vertex walk;
    // accepting the first delimiter match would assign a wrong handle grammar.
    let mut solutions = Vec::new();
    for handle_width in [1, 2, 3] {
        let Some(parsed) = parse_fbb_edge_tables_width(ctx, bytes, position, handle_width)? else {
            continue;
        };
        if vertex_table_end(bytes, parsed.2).is_some() {
            crate::resource::push(
                ctx,
                &mut solutions,
                parsed,
                "catia_fbb_edge_width_solutions",
            )?;
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
) -> Result<Option<(Vec<EdgeRow>, Vec<usize>, usize, usize)>, CodecError> {
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
            for _ in 0..count {
                if bytes.get(position) != Some(&0x02) {
                    return None;
                }
                position += 1;
                let arity = parse_count(bytes, &mut position)?;
                if arity < 2 {
                    return None;
                }
                if arity > bytes.get(position..).map_or(0, |rest| rest.len()) / handle_width {
                    return None;
                }
                let mut handles = Vec::new();
                if let Err(error) =
                    crate::resource::reserve_vec(ctx, &mut handles, arity, "catia_fbb_edge_handles")
                {
                    return Some(Err(error));
                }
                for _ in 0..arity {
                    handles.push(read_handle(bytes, position, handle_width)?);
                    position += handle_width;
                }
                if let Err(error) = crate::resource::push(
                    ctx,
                    &mut rows,
                    EdgeRow {
                        kind,
                        handles,
                        boundary_layout: EdgeBoundaryLayout::CompleteBoundaryRun,
                    },
                    "catia_fbb_edge_rows",
                ) {
                    return Some(Err(error));
                }
                if let Err(error) =
                    crate::resource::push(ctx, &mut scopes, table_count, "catia_fbb_edge_scopes")
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
    for trim in trims {
        let Some(face) = boundary_cycles(ctx, trim.packet.triangles(ctx)?)? else {
            return Ok(None);
        };
        crate::resource::push(ctx, &mut cycles, face, "catia_fbb_layout_face_cycles")?;
    }
    for row in rows {
        let complete_matches = cycles
            .iter()
            .flat_map(|face| face.iter())
            .map(|cycle| pattern_match_count(cycle, &row.handles))
            .sum::<usize>();
        if complete_matches != 0 {
            continue;
        }
        let Some(end) = row.handles.len().checked_sub(1) else {
            return Ok(None);
        };
        let Some(interior) = row.handles.get(1..end) else {
            continue;
        };
        if interior.is_empty() {
            continue;
        }
        let mut matched = false;
        let mut unique_per_cycle = true;
        for cycle in cycles.iter().flat_map(|face| face.iter()) {
            let count = pattern_match_count(cycle, interior);
            matched |= count != 0;
            unique_per_cycle &= count <= 1;
        }
        if matched && unique_per_cycle {
            row.boundary_layout = EdgeBoundaryLayout::InteriorWithFlankingCorners;
        }
    }
    Ok(Some(()))
}

fn pattern_match_count(cycle: &[u32], pattern: &[u32]) -> usize {
    if pattern.is_empty() || pattern.len() > cycle.len() {
        return 0;
    }
    (0..cycle.len())
        .filter(|start| {
            let forward = pattern
                .iter()
                .enumerate()
                .all(|(offset, handle)| cycle[(*start + offset) % cycle.len()] == *handle);
            let reversed = pattern
                .iter()
                .rev()
                .enumerate()
                .all(|(offset, handle)| cycle[(*start + offset) % cycle.len()] == *handle);
            forward || reversed
        })
        .count()
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
) -> Result<Vec<FbbFaceRun>, CodecError> {
    let mut groups = Vec::new();
    for range in crate::container::fbb_run_ranges(bytes) {
        if let Some(group) =
            parse_standard_group(ctx, bytes, range.start, range.len() / fbb_row::LEN)?
        {
            crate::resource::push(ctx, &mut groups, group, "catia_standard_fbb_groups")?;
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

fn vertex_table_end(bytes: &[u8], position: usize) -> Option<usize> {
    if bytes.get(position..position + 2) != Some(&[0x01, 0x06]) {
        return None;
    }
    let mut cursor = position + 2;
    let count = parse_count(bytes, &mut cursor)?;
    let end = cursor.checked_add(count.checked_mul(VERTEX_RECORD_BYTES)?)?;
    let records = bytes.get(cursor..end)?;
    for record in records.chunks_exact(VERTEX_RECORD_BYTES) {
        if record.get(..3) != Some(&[0x05, 0x08, 0x01][..]) {
            return None;
        }
        for offset in [3, 7, 11] {
            let value = View::f32_le_at(record, offset)?;
            FiniteReal::new(f64::from(value))?;
        }
    }
    Some(end)
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
    let Some(end) = vertex_table_end(bytes, vertex_header) else {
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
    for range in crate::container::fbb_run_ranges(bytes) {
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
        if vertex_table_end(bytes, vertex_header).is_none() {
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
        crate::resource::push(
            ctx,
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
    let ranges = crate::container::fbb_run_ranges(bytes);
    if let [range] = ranges.as_slice() {
        // A single marker run has no competing population to disambiguate.
        return Ok(FbbFaceRun::try_new(range.start, range.len() / fbb_row::LEN));
    }
    let groups = standard_fbb_groups(ctx, bytes)?;
    Ok(match groups.as_slice() {
        [group] => Some(*group),
        [] => largest_fbb_run(bytes),
        _ => None,
    })
}

pub(crate) fn largest_fbb_run(bytes: &[u8]) -> Option<FbbFaceRun> {
    let mut best: Option<FbbFaceRun> = None;
    let mut tied = false;
    let mut position = 0;
    while position + fbb_row::LEN <= bytes.len() {
        if crate::container::is_fbb_row(&bytes[position..]) {
            let start = position;
            let mut count = 0;
            while position + fbb_row::LEN <= bytes.len()
                && crate::container::is_fbb_row(&bytes[position..])
            {
                count += 1;
                position += fbb_row::LEN;
            }
            if best.is_none_or(|run| count > run.face_count()) {
                best = Some(FbbFaceRun::try_new(start, count)?);
                tied = false;
            } else if best.is_some_and(|run| count == run.face_count()) {
                tied = true;
            }
        } else {
            position += 1;
        }
    }
    if tied {
        None
    } else {
        best
    }
}

#[cfg(test)]
mod appearance_tests {
    use super::standard_face_colors;

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
        parse_trim_record_layout, parse_vertex_table, standard_face_count,
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
            "catia_trim_strip_lengths",
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

        let rows = [[0, 1], [1, 2], [2, 0]].map(|handles| EdgeRow {
            kind: 1,
            handles: handles.to_vec(),
            boundary_layout: EdgeBoundaryLayout::CompleteBoundaryRun,
        });
        let run = |ctx: &DecodeContext<'_>| {
            let mut union = UnionFind::new(6);
            super::cover_cycle(ctx, &[0, 1, 2], &rows, &mut union)
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
    fn counted_fbb_edge_rows_and_nested_handles_refuse_before_allocation() {
        let bytes = crate::test_support::test_topology::fbb_only_quad_topology_stream();
        let after_faces = largest_fbb_run(&bytes).expect("FBB face run").after_faces();
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
        let after_faces = largest_fbb_run(&bytes)
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
        let after_faces = largest_fbb_run(&bytes).expect("FBB face run").after_faces();
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

        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_retained_bytes = 4 * std::mem::size_of::<[f64; 3]>() as u64 - 1;
        let (ctx, _) = DecodeContext::from_root_bytes(&bytes, &arena, &policy)
            .expect("fixture fits the input limit");
        let refusal = parse_vertex_table(&ctx, &bytes, vertex_header)
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
) -> Result<Option<(Vec<EdgeRow>, Vec<usize>, usize, usize)>, CodecError> {
    // The full standard spine uses u16be rows and may contain one or more
    // counted tables. Keep that grammar first so a malformed standard walk
    // cannot silently enter the compact form below.
    if let Some((rows, scopes, vertex_header)) =
        parse_edge_tables_scoped_width(ctx, bytes, position, 2)?
    {
        if vertex_table_end(bytes, vertex_header).is_some() {
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
        && scopes.iter().all(|scope| *scope == 0)
        && rows
            .iter()
            .all(|row| row.boundary_layout == EdgeBoundaryLayout::CompleteBoundaryRun))
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
) -> Result<Option<(Vec<EdgeRow>, Vec<usize>, usize)>, CodecError> {
    Ok(
        parse_edge_tables_scoped_at_with_width(ctx, bytes, position)?
            .map(|(rows, scopes, vertex_header, _)| (rows, scopes, vertex_header)),
    )
}

fn parse_edge_tables_scoped_at_with_width(
    ctx: &DecodeContext<'_>,
    bytes: &[u8],
    position: usize,
) -> Result<Option<(Vec<EdgeRow>, Vec<usize>, usize, usize)>, CodecError> {
    let mut solutions = Vec::new();
    for handle_width in [1, 2, 3] {
        let Some(parsed) = parse_edge_tables_scoped_width(ctx, bytes, position, handle_width)?
        else {
            continue;
        };
        if vertex_table_end(bytes, parsed.2).is_some() {
            crate::resource::push(
                ctx,
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
) -> Result<Option<(Vec<EdgeRow>, Vec<usize>, usize)>, CodecError> {
    (|| -> Option<Result<_, CodecError>> {
        let mut rows = Vec::new();
        let mut scopes = Vec::new();
        let mut scope = 0usize;
        loop {
            if bytes.get(position) != Some(&0x01) {
                return None;
            }
            let kind = *bytes.get(position + 1)?;
            if !matches!(kind, 0x01 | 0x02) {
                return None;
            }
            position += 2;
            let count = parse_count(bytes, &mut position)?;
            for _ in 0..count {
                if bytes.get(position) != Some(&0x02) {
                    return None;
                }
                position += 1;
                let arity = parse_count(bytes, &mut position)?;
                if arity < 2 {
                    return None;
                }
                if arity > bytes.get(position..).map_or(0, |rest| rest.len()) / handle_width {
                    return None;
                }
                let mut handles = Vec::new();
                if let Err(error) = crate::resource::reserve_vec(
                    ctx,
                    &mut handles,
                    arity,
                    "catia_standard_edge_handles",
                ) {
                    return Some(Err(error));
                }
                for _ in 0..arity {
                    handles.push(read_handle(bytes, position, handle_width)?);
                    position += handle_width;
                }
                if let Err(error) = crate::resource::push(
                    ctx,
                    &mut rows,
                    EdgeRow {
                        kind,
                        handles,
                        boundary_layout: if arity == 2 {
                            EdgeBoundaryLayout::CompleteBoundaryRun
                        } else {
                            EdgeBoundaryLayout::InteriorWithFlankingCorners
                        },
                    },
                    "catia_standard_edge_rows",
                ) {
                    return Some(Err(error));
                }
                if let Err(error) =
                    crate::resource::push(ctx, &mut scopes, scope, "catia_standard_edge_scopes")
                {
                    return Some(Err(error));
                }
            }
            let mut saw_delimiter = false;
            while bytes.get(position..)?.starts_with(&EDGE_DELIMITER) {
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
    let Some(points) = parse_vertex_points(ctx, bytes, position)? else {
        return Ok(None);
    };
    let count = points.len();
    let bytes_needed = count
        .checked_mul(std::mem::size_of::<[f64; 3]>())
        .and_then(|bytes| u64::try_from(bytes).ok())
        .ok_or_else(|| {
            ctx.refuse_codec_limit("catia_fbb_vertex_coordinates", u64::MAX, u64::MAX)
        })?;
    ctx.charge_retained(bytes_needed, "catia_fbb_vertex_coordinates")?;
    let mut coordinates = Vec::new();
    crate::resource::reserve_vec(ctx, &mut coordinates, count, "catia_fbb_vertex_coordinates")?;
    for point in points {
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
    if count > bytes.get(position..).map_or(0, |rest| rest.len()) / VERTEX_RECORD_BYTES {
        return Ok(None);
    }
    let mut points = Vec::new();
    crate::resource::reserve_vec(ctx, &mut points, count, "catia_fbb_vertex_points")?;
    for _ in 0..count {
        if bytes.get(position..position + 3) != Some(&[0x05, 0x08, 0x01][..]) {
            return Ok(None);
        }
        position += 3;
        let mut coordinates = [FiniteReal::ZERO; 3];
        for coordinate in &mut coordinates {
            let Some(value) = View::f32_le_at(bytes, position) else {
                return Ok(None);
            };
            let Some(finite) = FiniteReal::new(f64::from(value)) else {
                return Ok(None);
            };
            *coordinate = finite;
            position += 4;
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
        (Some((compact_start, compact)), Some((wide_start, wide)))
            if compact_start == wide_start && compact == wide =>
        {
            Some((compact_start, compact))
        }
        (Some(records), None) | (None, Some(records)) => Some(records),
        (None, None) | (Some(_), Some(_)) => None,
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
    let mut predecessors = HashMap::<usize, Vec<usize>>::new();
    for (start, marker) in prefix.windows(2).enumerate() {
        if marker[0] != 0x01 || !TRIM_KINDS.contains(&marker[1]) {
            continue;
        }
        if let Some(layout) =
            parse_trim_record_layout_with_length_encoding(ctx, prefix, start, width, wide_u16be)?
        {
            crate::resource::admit_map_entry(
                ctx,
                &mut predecessors,
                &layout.end,
                "catia_trim_predecessor_ends",
            )?;
            let starts = predecessors.entry(layout.end).or_default();
            crate::resource::push(ctx, starts, start, "catia_trim_predecessor_starts")?;
        }
    }

    let mut solutions = Vec::new();
    let mut reversed = Vec::<TrimRecord>::new();
    crate::resource::reserve_vec(ctx, &mut reversed, record_count, "catia_trim_reversed")?;
    let mut frames = Vec::new();
    crate::resource::push(
        ctx,
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
            let retained_bytes = reversed
                .len()
                .checked_mul(std::mem::size_of::<TrimRecord>())
                .and_then(|bytes| u64::try_from(bytes).ok())
                .ok_or_else(|| {
                    ctx.refuse_codec_limit("catia_trim_solution_records", u64::MAX, u64::MAX)
                })?;
            ctx.charge_retained(retained_bytes, "catia_trim_solution_records")?;
            crate::resource::reserve_vec(
                ctx,
                &mut records,
                reversed.len(),
                "catia_trim_solution_records",
            )?;
            for record in &reversed {
                records.push(TrimRecord {
                    packet: record.packet.try_clone_with_context(ctx)?,
                    frame_vector: record.frame_vector,
                    kind: record.kind,
                });
            }
            records.reverse();
            crate::resource::push(
                ctx,
                &mut solutions,
                (chain_start, records),
                "catia_trim_solutions",
            )?;
            backtrack(&mut frames, &mut reversed);
            continue;
        }
        let predecessor = predecessors
            .get(&frames[frame].end)
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
        crate::resource::push(ctx, &mut reversed, record, "catia_trim_reversed")?;
        crate::resource::push(
            ctx,
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
        let kind = *bytes.get(start + 1)?;
        if !TRIM_KINDS.contains(&kind) {
            return None;
        }
        let mask = kind & 0x0f;
        let mut position = start + 2;
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
        position += 1;
        let handle_count = usize::try_from(View::u32_le_at(bytes, position)?).ok()?;
        position += 4;
        if handle_count == 0 {
            return None;
        }
        let frame_vector = if mask & 8 != 0 {
            let components = [
                f64::from(View::f32_le_at(bytes, position)?),
                f64::from(View::f32_le_at(bytes, position + 4)?),
                f64::from(View::f32_le_at(bytes, position + 8)?),
            ];
            position += 12;
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
        let primitive_count = b.checked_add(c)?;
        if !packed_two_strip_lengths
            && primitive_count > bytes.get(position..).map_or(0, |rest| rest.len())
        {
            return None;
        }
        let lane = if packed_two_strip_lengths {
            TrimLengthLane::PackedTwoStrip
        } else {
            let mut lengths = Vec::new();
            if let Err(error) = crate::resource::reserve_vec(
                ctx,
                &mut lengths,
                primitive_count,
                "catia_trim_primitive_lengths",
            ) {
                return Some(Err(error));
            }
            for _ in 0..primitive_count {
                let length = if wide_u16be {
                    let value = View::u16_be_at(bytes, position)?;
                    position += 2;
                    usize::from(value)
                } else {
                    parse_count(bytes, &mut position)?
                };
                lengths.push(length);
            }
            if 3usize.checked_mul(a)?.checked_add(lengths.iter().sum())? != handle_count {
                return None;
            }
            TrimLengthLane::Decoded(lengths)
        };
        let handle_offset = position;
        let byte_count = match &lane {
            TrimLengthLane::PackedTwoStrip => {
                2usize.checked_add(handle_count.checked_mul(width)?)?
            }
            TrimLengthLane::Decoded(_) => handle_count.checked_mul(width)?,
        };
        let end = handle_offset.checked_add(byte_count)?;
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
                let packed = bytes.get(position..position + 2)?;
                position += 2;
                let mut lengths = Vec::new();
                if let Err(error) =
                    crate::resource::reserve_vec(ctx, &mut lengths, 2, "catia_trim_packed_lengths")
                {
                    return Some(Err(error));
                }
                lengths.push(usize::from(packed[0]));
                lengths.push(usize::from(packed[1]));
                if lengths.iter().sum::<usize>() != layout.handle_count {
                    return None;
                }
                lengths
            }
        };
        let mut handles = Vec::new();
        if let Err(error) = crate::resource::reserve_vec(
            ctx,
            &mut handles,
            layout.handle_count,
            "catia_trim_packet_handles",
        ) {
            return Some(Err(error));
        }
        for _ in 0..layout.handle_count {
            let handle = read_handle(bytes, position, width)?;
            handles.push(handle);
            position += width;
        }

        let (strip_lengths, fan_lengths) = lengths.split_at_checked(layout.strip_count)?;
        let strip_lengths = match crate::resource::copy_retained_slice(
            ctx,
            strip_lengths,
            "catia_trim_strip_lengths",
        ) {
            Ok(lengths) => lengths,
            Err(error) => return Some(Err(error)),
        };
        let fan_lengths = match crate::resource::copy_retained_slice(
            ctx,
            fan_lengths,
            "catia_trim_fan_lengths",
        ) {
            Ok(lengths) => lengths,
            Err(error) => return Some(Err(error)),
        };
        let packet = TrimPacket::try_from((
            layout.independent_count,
            strip_lengths,
            fan_lengths,
            handles,
        ))
        .ok()?;
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
    let mut edge_directions = HashMap::<(u32, u32), u8>::new();
    for &[a, b, c] in triangles {
        for (start, end) in [(a, b), (b, c), (c, a)] {
            if start == end {
                return Ok(None);
            }
            let (edge, direction) = if start < end {
                ((start, end), 1)
            } else {
                ((end, start), 2)
            };
            if let Some(directions) = edge_directions.get_mut(&edge) {
                if *directions & direction != 0 {
                    return Ok(None);
                }
                *directions |= direction;
            } else {
                crate::resource::insert_map(
                    ctx,
                    &mut edge_directions,
                    edge,
                    direction,
                    "catia_boundary_edge_directions",
                )?;
            }
        }
    }
    let mut successors = HashMap::new();
    for (&(low, high), &directions) in &edge_directions {
        let boundary = match directions {
            1 => Some((low, high)),
            2 => Some((high, low)),
            3 => None,
            _ => return Ok(None),
        };
        if let Some((start, end)) = boundary {
            if successors.contains_key(&start) {
                return Ok(None);
            }
            crate::resource::insert_map(
                ctx,
                &mut successors,
                start,
                end,
                "catia_boundary_successors",
            )?;
        }
    }
    let mut seen = HashSet::new();
    let mut cycles = Vec::new();
    for &start in successors.keys() {
        if seen.contains(&start) {
            continue;
        }
        let mut cycle = Vec::new();
        crate::resource::push(ctx, &mut cycle, start, "catia_boundary_cycle_handles")?;
        crate::resource::insert_set(ctx, &mut seen, start, "catia_boundary_seen")?;
        let Some(&mut_current) = successors.get(&start) else {
            return Ok(None);
        };
        let mut current = mut_current;
        while current != start {
            if !crate::resource::insert_set(ctx, &mut seen, current, "catia_boundary_seen")? {
                return Ok(None);
            }
            crate::resource::push(ctx, &mut cycle, current, "catia_boundary_cycle_handles")?;
            let Some(&next) = successors.get(&current) else {
                return Ok(None);
            };
            current = next;
        }
        let minimum = cycle
            .iter()
            .enumerate()
            .min_by_key(|(_, handle)| *handle)
            .map(|(index, _)| index);
        let Some(minimum) = minimum else {
            return Ok(None);
        };
        cycle.rotate_left(minimum);
        crate::resource::push(ctx, &mut cycles, cycle, "catia_boundary_cycles")?;
    }
    cycles.sort();
    Ok((!cycles.is_empty()).then_some(cycles))
}

pub(super) fn cover_cycle(
    ctx: &DecodeContext<'_>,
    cycle: &[u32],
    rows: &[EdgeRow],
    union: &mut UnionFind,
) -> Result<Option<Boundary>, CodecError> {
    cover_cycle_by_rows(ctx, cycle, rows, union)
}

fn cover_cycle_by_rows(
    ctx: &DecodeContext<'_>,
    cycle: &[u32],
    rows: &[EdgeRow],
    union: &mut UnionFind,
) -> Result<Option<Boundary>, CodecError> {
    let length = cycle.len();
    let mut matches = Vec::new();
    for (edge_row, row) in rows.iter().enumerate() {
        let Some(pattern) = row.boundary_pattern() else {
            continue;
        };
        let mut row_match = None;
        for start in 0..length {
            let forward = pattern
                .iter()
                .enumerate()
                .all(|(offset, handle)| cycle[(start + offset) % length] == *handle);
            let reversed = pattern
                .iter()
                .rev()
                .enumerate()
                .all(|(offset, handle)| cycle[(start + offset) % length] == *handle);
            if forward {
                if row_match.replace((start, false)).is_some() {
                    return Ok(None);
                }
            } else if reversed {
                if row_match.replace((start, true)).is_some() {
                    return Ok(None);
                }
            }
        }
        if let Some((start, reversed)) = row_match {
            let Some((boundary_start, segment_count)) = row.boundary_span(start, length) else {
                return Ok(None);
            };
            crate::resource::push(
                ctx,
                &mut matches,
                (boundary_start, segment_count, edge_row, reversed),
                "catia_fbb_cycle_matches",
            )?;
        }
    }
    if matches.is_empty() {
        return Ok(None);
    }
    let mut coverage = ctx.alloc_filled(length, 0_u8, "catia FBB boundary coverage")?;
    for &(start, edge_count, _, _) in &matches {
        for offset in 0..edge_count {
            let index = (start + offset) % length;
            let Some(count) = coverage[index].checked_add(1) else {
                return Ok(None);
            };
            coverage[index] = count;
        }
    }
    if coverage.iter().any(|count| *count != 1) {
        return Ok(None);
    }
    matches.sort_by_key(|entry| entry.0 % length);
    let mut corner_nodes = HashMap::new();
    for &(start, edge_count, _, _) in &matches {
        let end = (start + edge_count) % length;
        for corner in [start % length, end] {
            if !corner_nodes.contains_key(&corner) {
                let node = union.push_charged(ctx, "catia_fbb_corner_union_nodes")?;
                crate::resource::insert_map(
                    ctx,
                    &mut corner_nodes,
                    corner,
                    node,
                    "catia_fbb_corner_nodes",
                )?;
            }
        }
    }
    let mut coedges = Vec::new();
    crate::resource::reserve_vec(ctx, &mut coedges, matches.len(), "catia_fbb_cycle_coedges")?;
    for (start, edge_count, edge_row, reversed) in matches {
        let start_node = corner_nodes[&(start % length)];
        let end_node = corner_nodes[&((start + edge_count) % length)];
        let edge_start = edge_row * 2;
        let edge_end = edge_start + 1;
        if reversed {
            union.union(edge_end, start_node);
            union.union(edge_start, end_node);
        } else {
            union.union(edge_start, start_node);
            union.union(edge_end, end_node);
        }
        coedges.push(CoedgeUse {
            edge_row,
            reversed,
            start_vertex: start_node,
            end_vertex: end_node,
        });
    }
    Ok(Boundary::new(coedges))
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
