//! Evidence-preserving gauge quotient for standard mesh candidates.

use std::collections::BTreeMap;

use cadmpeg_core::decode::DecodeContext;
use cadmpeg_core::CodecError;

use super::mesh_quotient::{
    raw_endpoint_relation_state_signature, MeshEndpointRelationChoice,
    MeshEndpointRelationSelection, MeshEndpointRelationStateSignature,
};
use crate::families::standard::topology::{
    CoedgeUse, EdgeBoundaryLayout, EdgeRow, NonEmptyCoedges, StandardTopology,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum MeshEdgeGeometry {
    Line,
    Circle { center: [u64; 3], radius: u64 },
    Bspline,
}

type MeshEdgeGaugeBaseKey = (u8, EdgeBoundaryLayout, MeshEdgeGeometry, usize, [usize; 2]);

type MeshEdgeGaugeKey = (MeshEdgeGaugeBaseKey, Vec<[usize; 2]>);
type MeshTopologyEdgeGaugeBaseKey = (u8, EdgeBoundaryLayout, MeshEdgeGeometry, usize, Vec<usize>);
type MeshTopologyEdgeGaugeKey = (MeshTopologyEdgeGaugeBaseKey, Vec<[usize; 2]>);

pub(in crate::solve) struct MeshCoordinateGauge {
    components: Vec<Vec<Vec<usize>>>,
}

#[derive(Clone, Copy)]
pub(super) struct MeshCandidateGauge<'a> {
    pub(super) edge_rows: &'a [EdgeRow],
    pub(super) edge_faces: &'a [[usize; 2]],
    pub(super) edge_geometry: &'a [MeshEdgeGeometry],
    pub(super) edge_candidates: &'a [Vec<[usize; 2]>],
    pub(super) edge_identity_evidence: &'a [bool],
    pub(super) coordinate_gauge: Option<&'a MeshCoordinateGauge>,
}

fn canonicalize_topology_boundary_gauges(
    ctx: &DecodeContext<'_>,
    topology: &mut StandardTopology,
) -> Result<(), CodecError> {
    fn signature(coedges: &[CoedgeUse]) -> impl Iterator<Item = (usize, bool, usize, usize)> + '_ {
        coedges.iter().map(|coedge| {
            (
                coedge.edge_row,
                coedge.reversed,
                coedge.start_vertex,
                coedge.end_vertex,
            )
        })
    }

    fn rotate_to_minimum(coedges: &mut NonEmptyCoedges) {
        let len = coedges.len();
        let cycle = coedges.as_slice();
        let key = |start: usize| {
            (0..len).map(move |offset| {
                let coedge = cycle[(start + offset) % len];
                (
                    coedge.edge_row,
                    coedge.reversed,
                    coedge.start_vertex,
                    coedge.end_vertex,
                )
            })
        };
        let best = (1..len).fold(0, |best, start| {
            if key(start).lt(key(best)) {
                start
            } else {
                best
            }
        });
        coedges.rotate_left(best);
    }

    for face in &mut topology.faces {
        for boundary in &mut face.boundaries {
            rotate_to_minimum(&mut boundary.coedges);
            let reversed = crate::resource::copy_retained_slice(
                ctx,
                boundary.coedges.as_slice(),
                "catia_mesh_gauge_reversed_coedges",
            )?;
            let mut reversed =
                NonEmptyCoedges::try_from(reversed).map_err(cadmpeg_core::CodecError::malformed)?;
            reversed.reverse();
            for coedge in &mut reversed {
                coedge.reversed = !coedge.reversed;
                std::mem::swap(&mut coedge.start_vertex, &mut coedge.end_vertex);
            }
            rotate_to_minimum(&mut reversed);
            if signature(&reversed)
                .cmp(signature(&boundary.coedges))
                .is_lt()
            {
                boundary.coedges = reversed;
            }
        }
        for index in 1..face.boundaries.len() {
            let mut position = index;
            while position > 0 {
                ctx.charge_work(1, "catia_mesh_gauge_boundary_order")?;
                if signature(&face.boundaries[position].coedges)
                    .cmp(signature(&face.boundaries[position - 1].coedges))
                    .is_ge()
                {
                    break;
                }
                face.boundaries.swap(position, position - 1);
                position -= 1;
            }
        }
    }
    Ok(())
}

#[test]
fn mesh_gauge_reversed_coedges_refuse_before_copy() {
    let topology = || StandardTopology {
        faces: vec![crate::families::standard::topology::FaceTopology {
            boundaries: vec![crate::families::standard::topology::Boundary {
                coedges: NonEmptyCoedges::one(CoedgeUse {
                    edge_row: 0,
                    reversed: false,
                    start_vertex: 0,
                    end_vertex: 1,
                }),
            }],
        }],
        edge_rows: Vec::new(),
        vertex_points: Vec::new(),
        logical_vertex_count: 0,
    };
    crate::test_support::with_service_context(|ctx| {
        let mut candidate = topology();
        canonicalize_topology_boundary_gauges(ctx, &mut candidate)
            .expect("service resource budget");
        assert_eq!(candidate.faces[0].boundaries[0].coedges.len(), 1);
    });
    let refused = crate::test_support::with_collection_limit(0, |ctx| {
        canonicalize_topology_boundary_gauges(ctx, &mut topology())
    });
    assert!(matches!(
        refused,
        Err(CodecError::ResourceLimit(limit))
            if limit.operation == "catia_mesh_gauge_reversed_coedges"
    ));
}

fn normalized_endpoint_options(
    ctx: &DecodeContext<'_>,
    options: &[[usize; 2]],
) -> Result<Vec<[usize; 2]>, CodecError> {
    let mut normalized = Vec::new();
    for &mut_pair in options {
        let mut pair = mut_pair;
        pair.sort_unstable();
        crate::resource::push(ctx, &mut normalized, pair, "catia_gauge_normalized_options")?;
    }
    normalized.sort_unstable();
    normalized.dedup();
    Ok(normalized)
}

fn mesh_edge_gauge_base_key(
    edge: usize,
    edge_rows: &[EdgeRow],
    edge_faces: &[[usize; 2]],
    edge_geometry: &[MeshEdgeGeometry],
) -> Option<MeshEdgeGaugeBaseKey> {
    let row = edge_rows.get(edge)?;
    let mut faces = *edge_faces.get(edge)?;
    faces.sort_unstable();
    Some((
        row.kind,
        row.boundary_layout,
        *edge_geometry.get(edge)?,
        row.handles.len(),
        faces,
    ))
}

fn mapped_normalized_endpoint_options(
    ctx: &DecodeContext<'_>,
    options: &[[usize; 2]],
    permutation: &[usize],
) -> Result<Option<Vec<[usize; 2]>>, CodecError> {
    let mut mapped = Vec::new();
    for pair in options {
        let (Some(&left), Some(&right)) = (permutation.get(pair[0]), permutation.get(pair[1]))
        else {
            return Ok(None);
        };
        crate::resource::push(
            ctx,
            &mut mapped,
            [left, right],
            "catia_gauge_mapped_options",
        )?;
    }
    Ok(Some(normalized_endpoint_options(ctx, &mapped)?))
}

fn coordinate_find(
    ctx: &DecodeContext<'_>,
    parent: &mut [usize],
    node: usize,
) -> Result<usize, CodecError> {
    let _depth = ctx.enter_nested("catia_gauge_coordinate_find")?;
    if parent[node] == node {
        return Ok(node);
    }
    let root = coordinate_find(ctx, parent, parent[node])?;
    parent[node] = root;
    Ok(root)
}

fn coordinate_union(
    ctx: &DecodeContext<'_>,
    parent: &mut [usize],
    left: usize,
    right: usize,
) -> Result<(), CodecError> {
    let left = coordinate_find(ctx, parent, left)?;
    let right = coordinate_find(ctx, parent, right)?;
    if left != right {
        parent[right] = left;
    }
    Ok(())
}

fn enumerate_coordinate_permutations(
    ctx: &DecodeContext<'_>,
    points: &[usize],
    index: usize,
    current: &mut Vec<usize>,
    used: &mut [bool],
    output: &mut Vec<Vec<usize>>,
) -> Result<(), CodecError> {
    if index == points.len() {
        let copy =
            crate::resource::copy_retained_slice(ctx, current, "catia_gauge_permutation_values")?;
        crate::resource::push(ctx, output, copy, "catia_gauge_permutations")?;
        return Ok(());
    }
    for target in 0..points.len() {
        if used[target] {
            continue;
        }
        used[target] = true;
        crate::resource::push(ctx, current, points[target], "catia_gauge_permutation_path")?;
        enumerate_coordinate_permutations(ctx, points, index + 1, current, used, output)?;
        current.pop();
        used[target] = false;
    }
    Ok(())
}

fn bounded_factorial(value: usize, limit: usize) -> Option<usize> {
    let mut result = 1usize;
    for factor in 2..=value {
        result = result.checked_mul(factor)?;
        if result > limit {
            return None;
        }
    }
    Some(result)
}

fn intern_gauge_signatures<T: Ord>(
    ctx: &DecodeContext<'_>,
    signatures: impl IntoIterator<Item = T>,
) -> Result<Vec<usize>, CodecError> {
    let mut ids = BTreeMap::<T, usize>::new();
    let mut colors = Vec::new();
    for signature in signatures {
        let id = if let Some(id) = ids.get(&signature) {
            *id
        } else {
            ctx.charge_collection_items(1, "catia_gauge_signature_keys")?;
            let id = ids.len();
            ids.insert(signature, id);
            id
        };
        crate::resource::push(ctx, &mut colors, id, "catia_gauge_signature_colors")?;
    }
    Ok(colors)
}

pub(super) fn build_mesh_coordinate_gauge(
    ctx: &DecodeContext<'_>,
    point_count: usize,
    edge_rows: &[EdgeRow],
    edge_faces: &[[usize; 2]],
    edge_geometry: &[MeshEdgeGeometry],
    edge_candidates: &[Vec<[usize; 2]>],
    edge_identity_evidence: &[bool],
) -> Result<MeshCoordinateGauge, CodecError> {
    const MAX_COORDINATE_GAUGE_PERMUTATIONS: usize = 4_096;
    let identity = || {
        Ok(MeshCoordinateGauge {
            components: Vec::new(),
        })
    };
    if edge_rows.len() != edge_faces.len()
        || edge_rows.len() != edge_geometry.len()
        || edge_rows.len() != edge_candidates.len()
        || edge_rows.len() != edge_identity_evidence.len()
    {
        return identity();
    }

    let mut edge_bases = Vec::new();
    let mut groups = BTreeMap::<MeshEdgeGaugeBaseKey, Vec<usize>>::new();
    for edge in 0..edge_rows.len() {
        let Some(key) = mesh_edge_gauge_base_key(edge, edge_rows, edge_faces, edge_geometry) else {
            return identity();
        };
        crate::resource::push(ctx, &mut edge_bases, key, "catia_gauge_edge_bases")?;
        if !groups.contains_key(&key) {
            ctx.charge_collection_items(1, "catia_gauge_edge_groups")?;
        }
        crate::resource::push(
            ctx,
            groups.entry(key).or_default(),
            edge,
            "catia_gauge_group_edges",
        )?;
    }
    let mut normalized_options = Vec::new();
    for options in edge_candidates {
        let normalized = normalized_endpoint_options(ctx, options)?;
        crate::resource::push(
            ctx,
            &mut normalized_options,
            normalized,
            "catia_gauge_normalized_rows",
        )?;
    }
    let mut parent = Vec::new();
    crate::resource::reserve_vec(ctx, &mut parent, point_count, "catia_gauge_parent")?;
    parent.extend(0..point_count);
    let mut active = ctx.alloc_filled(point_count, false, "catia_coordinate_gauge_active")?;
    for edges in groups.values() {
        let mut group_points = Vec::new();
        for &edge in edges {
            for &point in edge_candidates[edge].iter().flatten() {
                let Some(active_point) = active.get_mut(point) else {
                    return identity();
                };
                *active_point = true;
                if !group_points.contains(&point) {
                    crate::resource::push(
                        ctx,
                        &mut group_points,
                        point,
                        "catia_gauge_group_points",
                    )?;
                }
            }
        }
        if let Some(&first) = group_points.first() {
            for point in group_points.into_iter().skip(1) {
                coordinate_union(ctx, &mut parent, first, point)?;
            }
        }
    }
    let components_by_root = {
        let mut components = BTreeMap::<usize, Vec<usize>>::new();
        for point in 0..point_count {
            if active.get(point).copied().unwrap_or(false) {
                let root = coordinate_find(ctx, &mut parent, point)?;
                if !components.contains_key(&root) {
                    ctx.charge_collection_items(1, "catia_gauge_component_keys")?;
                }
                crate::resource::push(
                    ctx,
                    components.entry(root).or_default(),
                    point,
                    "catia_gauge_component_points",
                )?;
            }
        }
        components
    };
    let mut coordinate_components = Vec::new();
    for points in components_by_root.into_values() {
        crate::resource::push(
            ctx,
            &mut coordinate_components,
            points,
            "catia_gauge_component_rows",
        )?;
    }
    let mut component_by_point = ctx.alloc_filled(
        point_count,
        None::<usize>,
        "catia_coordinate_gauge_components",
    )?;
    for (component, points) in coordinate_components.iter().enumerate() {
        for &point in points {
            let Some(slot) = component_by_point.get_mut(point) else {
                return identity();
            };
            *slot = Some(component);
        }
    }

    let mut option_records = Vec::<(usize, [usize; 2])>::new();
    let mut option_indices_by_edge = Vec::new();
    let mut option_neighbors = ctx.alloc_filled(
        point_count,
        Vec::<usize>::new(),
        "catia_gauge_option_neighbors",
    )?;
    for (edge, options) in normalized_options.iter().enumerate() {
        let mut indices = Vec::new();
        for &pair in options {
            let option = option_records.len();
            crate::resource::push(
                ctx,
                &mut option_records,
                (edge, pair),
                "catia_gauge_option_records",
            )?;
            crate::resource::push(ctx, &mut indices, option, "catia_gauge_option_indices")?;
            for point in pair {
                let Some(neighbors) = option_neighbors.get_mut(point) else {
                    return identity();
                };
                crate::resource::push(ctx, neighbors, option, "catia_gauge_option_arcs")?;
            }
        }
        crate::resource::push(
            ctx,
            &mut option_indices_by_edge,
            indices,
            "catia_gauge_option_rows",
        )?;
    }
    let mut point_colors = intern_gauge_signatures(ctx, (0..point_count).map(|_| ()))?;
    let mut row_colors = intern_gauge_signatures(
        ctx,
        (0..edge_rows.len()).map(|edge| {
            (
                edge_bases[edge],
                edge_identity_evidence[edge],
                edge_identity_evidence[edge].then_some(edge),
            )
        }),
    )?;
    let mut option_colors = intern_gauge_signatures(
        ctx,
        option_records.iter().map(|(edge, _)| row_colors[*edge]),
    )?;
    let refinement_limit = point_count
        .checked_add(edge_rows.len())
        .and_then(|count| count.checked_add(option_records.len()))
        .and_then(|count| count.checked_add(1))
        .ok_or_else(|| {
            ctx.refuse_codec_limit("catia_gauge_refinement_limit", u64::MAX, u64::MAX)
        })?;
    for _ in 0..refinement_limit {
        let next_option_colors = intern_gauge_signatures(
            ctx,
            option_records
                .iter()
                .enumerate()
                .map(|(option, (edge, [left, right]))| {
                    let mut endpoints = [point_colors[*left], point_colors[*right]];
                    endpoints.sort_unstable();
                    (option_colors[option], row_colors[*edge], endpoints)
                }),
        )?;
        let mut row_signatures = Vec::new();
        for edge in 0..edge_rows.len() {
            let mut options = Vec::new();
            for &option in &option_indices_by_edge[edge] {
                crate::resource::push(
                    ctx,
                    &mut options,
                    next_option_colors[option],
                    "catia_gauge_row_option_colors",
                )?;
            }
            options.sort_unstable();
            crate::resource::push(
                ctx,
                &mut row_signatures,
                (
                    row_colors[edge],
                    edge_bases[edge],
                    edge_identity_evidence[edge],
                    edge_identity_evidence[edge].then_some(edge),
                    options,
                ),
                "catia_gauge_row_signatures",
            )?;
        }
        let next_row_colors = intern_gauge_signatures(ctx, row_signatures)?;
        let mut point_signatures = Vec::new();
        for point in 0..point_count {
            let mut options = Vec::new();
            for &option in &option_neighbors[point] {
                crate::resource::push(
                    ctx,
                    &mut options,
                    next_option_colors[option],
                    "catia_gauge_point_option_colors",
                )?;
            }
            options.sort_unstable();
            crate::resource::push(
                ctx,
                &mut point_signatures,
                (point_colors[point], options),
                "catia_gauge_point_signatures",
            )?;
        }
        let next_point_colors = intern_gauge_signatures(ctx, point_signatures)?;
        let stable = next_point_colors == point_colors
            && next_row_colors == row_colors
            && next_option_colors == option_colors;
        point_colors = next_point_colors;
        row_colors = next_row_colors;
        option_colors = next_option_colors;
        if stable {
            break;
        }
    }

    let is_automorphism = |affected_groups: &[&Vec<usize>],
                           permutation: &[usize]|
     -> Result<bool, CodecError> {
        for edges in affected_groups {
            let mut original_unbound = Vec::new();
            let mut mapped_unbound = Vec::new();
            for &edge in *edges {
                if edge_identity_evidence[edge] {
                    continue;
                }
                let original = crate::resource::copy_retained_slice(
                    ctx,
                    &normalized_options[edge],
                    "catia_gauge_original_options",
                )?;
                crate::resource::push(
                    ctx,
                    &mut original_unbound,
                    original,
                    "catia_gauge_original_rows",
                )?;
                if let Some(mapped) =
                    mapped_normalized_endpoint_options(ctx, &edge_candidates[edge], permutation)?
                {
                    crate::resource::push(
                        ctx,
                        &mut mapped_unbound,
                        mapped,
                        "catia_gauge_mapped_rows",
                    )?;
                }
            }
            original_unbound.sort_unstable();
            mapped_unbound.sort_unstable();
            if original_unbound != mapped_unbound {
                return Ok(false);
            }
            for &edge in edges.iter().filter(|edge| edge_identity_evidence[**edge]) {
                if mapped_normalized_endpoint_options(ctx, &edge_candidates[edge], permutation)?
                    != Some(crate::resource::copy_retained_slice(
                        ctx,
                        &normalized_options[edge],
                        "catia_gauge_identity_options",
                    )?)
                {
                    return Ok(false);
                }
            }
        }
        Ok(true)
    };

    let mut components = Vec::new();
    for (component, points) in coordinate_components.into_iter().enumerate() {
        let mut affected_groups = Vec::new();
        for edges in groups.values() {
            if edges.iter().any(|edge| {
                edge_candidates[*edge].iter().flatten().any(|point| {
                    component_by_point.get(*point).copied().flatten() == Some(component)
                })
            }) {
                crate::resource::push(
                    ctx,
                    &mut affected_groups,
                    edges,
                    "catia_gauge_affected_groups",
                )?;
            }
        }
        let mut color_classes = BTreeMap::<usize, Vec<usize>>::new();
        for &point in &points {
            let color = point_colors[point];
            if !color_classes.contains_key(&color) {
                ctx.charge_collection_items(1, "catia_gauge_color_classes")?;
            }
            crate::resource::push(
                ctx,
                color_classes.entry(color).or_default(),
                point,
                "catia_gauge_color_points",
            )?;
        }
        let mut identity_order = Vec::new();
        crate::resource::reserve_vec(
            ctx,
            &mut identity_order,
            point_count,
            "catia_gauge_identity_order",
        )?;
        identity_order.extend(0..point_count);
        let mut local_orders = Vec::new();
        crate::resource::push(
            ctx,
            &mut local_orders,
            identity_order,
            "catia_gauge_local_orders",
        )?;
        let mut bounded = true;
        for class in color_classes.values() {
            let remaining_limit = MAX_COORDINATE_GAUGE_PERMUTATIONS / local_orders.len();
            let Some(class_order_count) = bounded_factorial(class.len(), remaining_limit) else {
                bounded = false;
                break;
            };
            let mut used = ctx.alloc_filled(class.len(), false, "catia_coordinate_gauge_used")?;
            let mut class_orders = Vec::new();
            enumerate_coordinate_permutations(
                ctx,
                class,
                0,
                &mut Vec::new(),
                &mut used,
                &mut class_orders,
            )?;
            let next_len = local_orders.len() * class_order_count;
            let mut next = Vec::new();
            crate::resource::reserve_vec(ctx, &mut next, next_len, "catia_gauge_next_orders")?;
            for permutation in &local_orders {
                for order in &class_orders {
                    let mut permutation = crate::resource::copy_retained_slice(
                        ctx,
                        permutation,
                        "catia_gauge_order_copy",
                    )?;
                    for (&source, &target) in class.iter().zip(order) {
                        permutation[source] = target;
                    }
                    next.push(permutation);
                }
            }
            local_orders = next;
        }
        if !bounded {
            local_orders.clear();
            let mut identity = Vec::new();
            crate::resource::reserve_vec(
                ctx,
                &mut identity,
                point_count,
                "catia_gauge_fallback_identity",
            )?;
            identity.extend(0..point_count);
            crate::resource::push(
                ctx,
                &mut local_orders,
                identity,
                "catia_gauge_fallback_orders",
            )?;
        }
        let mut permutations = Vec::new();
        for permutation in local_orders {
            if is_automorphism(&affected_groups, &permutation)? {
                crate::resource::push(
                    ctx,
                    &mut permutations,
                    permutation,
                    "catia_gauge_kept_permutations",
                )?;
            }
        }
        if permutations.is_empty() {
            let mut identity = Vec::new();
            crate::resource::reserve_vec(
                ctx,
                &mut identity,
                point_count,
                "catia_gauge_empty_identity",
            )?;
            identity.extend(0..point_count);
            crate::resource::push(
                ctx,
                &mut permutations,
                identity,
                "catia_gauge_empty_permutations",
            )?;
        }
        permutations.sort_unstable();
        permutations.dedup();
        crate::resource::push(ctx, &mut components, permutations, "catia_gauge_components")?;
    }
    Ok(MeshCoordinateGauge { components })
}

fn mapped_endpoint_pair(
    pair: Option<[usize; 2]>,
    permutation: Option<&[usize]>,
) -> Option<[usize; 2]> {
    let mut pair = pair?;
    if let Some(permutation) = permutation {
        pair = [*permutation.get(pair[0])?, *permutation.get(pair[1])?];
    }
    pair.sort_unstable();
    Some(pair)
}

fn canonicalize_partial_endpoint_pair_gauge_with_permutation(
    ctx: &DecodeContext<'_>,
    pairs: &[Option<[usize; 2]>],
    gauge: MeshCandidateGauge<'_>,
    permutation: Option<&[usize]>,
) -> Result<Option<Vec<Option<[usize; 2]>>>, CodecError> {
    let edge_count = pairs.len();
    let mut canonical = Vec::new();
    for pair in pairs.iter().copied() {
        let mapped = match pair {
            Some(pair) => {
                let Some(mapped) = mapped_endpoint_pair(Some(pair), permutation) else {
                    return Ok(None);
                };
                Some(mapped)
            }
            None => None,
        };
        crate::resource::push(ctx, &mut canonical, mapped, "catia_gauge_canonical_pairs")?;
    }
    if gauge.edge_rows.len() != edge_count
        || gauge.edge_faces.len() != edge_count
        || gauge.edge_geometry.len() != edge_count
        || gauge.edge_candidates.len() != edge_count
        || gauge.edge_identity_evidence.len() != edge_count
    {
        return Ok(Some(canonical));
    }

    let mut source_groups = BTreeMap::<MeshEdgeGaugeKey, Vec<usize>>::new();
    let mut target_groups = BTreeMap::<MeshEdgeGaugeKey, Vec<usize>>::new();
    for edge in 0..edge_count {
        if gauge.edge_identity_evidence[edge] {
            continue;
        }
        let Some(base) =
            mesh_edge_gauge_base_key(edge, gauge.edge_rows, gauge.edge_faces, gauge.edge_geometry)
        else {
            return Ok(None);
        };
        let source_options = normalized_endpoint_options(ctx, &gauge.edge_candidates[edge])?;
        let target_options = match permutation {
            Some(permutation) => {
                let Some(mapped) = mapped_normalized_endpoint_options(
                    ctx,
                    &gauge.edge_candidates[edge],
                    permutation,
                )?
                else {
                    return Ok(None);
                };
                mapped
            }
            None => crate::resource::copy_retained_slice(
                ctx,
                &source_options,
                "catia_gauge_target_options",
            )?,
        };
        let source_key = (base, target_options);
        if !source_groups.contains_key(&source_key) {
            ctx.charge_collection_items(1, "catia_gauge_source_group_keys")?;
        }
        crate::resource::push(
            ctx,
            source_groups.entry(source_key).or_default(),
            edge,
            "catia_gauge_source_group_edges",
        )?;
        let target_key = (base, source_options);
        if !target_groups.contains_key(&target_key) {
            ctx.charge_collection_items(1, "catia_gauge_target_group_keys")?;
        }
        crate::resource::push(
            ctx,
            target_groups.entry(target_key).or_default(),
            edge,
            "catia_gauge_target_group_edges",
        )?;
    }

    for (key, group) in source_groups {
        let Some(mut slots) = target_groups.remove(&key) else {
            return Ok(None);
        };
        if slots.len() != group.len() {
            return Ok(None);
        }
        slots.sort_unstable();
        let mut ordered = Vec::new();
        for &edge in &group {
            crate::resource::push(
                ctx,
                &mut ordered,
                (canonical[edge], edge),
                "catia_gauge_ordered_group",
            )?;
        }
        ordered.sort_unstable();
        for (slot, (pair, _)) in slots.into_iter().zip(ordered) {
            canonical[slot] = pair;
        }
    }
    if !target_groups.is_empty() {
        return Ok(None);
    }
    Ok(Some(canonical))
}

fn canonicalize_partial_endpoint_pair_gauge(
    ctx: &DecodeContext<'_>,
    pairs: &[Option<[usize; 2]>],
    gauge: MeshCandidateGauge<'_>,
) -> Result<Option<Vec<Option<[usize; 2]>>>, CodecError> {
    let Some(mut canonical) =
        canonicalize_partial_endpoint_pair_gauge_with_permutation(ctx, pairs, gauge, None)?
    else {
        return Ok(None);
    };
    if let Some(coordinate_gauge) = gauge.coordinate_gauge {
        for permutations in &coordinate_gauge.components {
            let mut best =
                crate::resource::copy_retained_slice(ctx, &canonical, "catia_gauge_best_pairs")?;
            for permutation in permutations {
                let Some(candidate) = canonicalize_partial_endpoint_pair_gauge_with_permutation(
                    ctx,
                    &canonical,
                    gauge,
                    Some(permutation),
                )?
                else {
                    return Ok(None);
                };
                if candidate < best {
                    best = candidate;
                }
            }
            canonical = best;
        }
    }
    Ok(Some(canonical))
}

pub(super) fn canonicalize_complete_endpoint_pairs(
    ctx: &DecodeContext<'_>,
    pairs: &[[usize; 2]],
    gauge: MeshCandidateGauge<'_>,
) -> Result<Option<Vec<[usize; 2]>>, CodecError> {
    let mut partial = Vec::new();
    for &pair in pairs {
        crate::resource::push(ctx, &mut partial, Some(pair), "catia_gauge_input_pairs")?;
    }
    let Some(canonical) = canonicalize_partial_endpoint_pair_gauge(ctx, &partial, gauge)? else {
        return Ok(None);
    };
    let mut complete = Vec::new();
    for pair in canonical {
        let Some(pair) = pair else {
            return Ok(None);
        };
        crate::resource::push(ctx, &mut complete, pair, "catia_gauge_complete_pairs")?;
    }
    Ok(Some(complete))
}

fn canonicalize_mesh_edge_row_gauges(
    ctx: &DecodeContext<'_>,
    mut topology: StandardTopology,
    gauge: MeshCandidateGauge<'_>,
    permutation: Option<&[usize]>,
) -> Result<Option<StandardTopology>, CodecError> {
    (|| -> Option<Result<StandardTopology, CodecError>> {
        let edge_count = topology.edge_rows.len();
        if gauge.edge_geometry.len() != edge_count
            || gauge.edge_candidates.len() != edge_count
            || gauge.edge_identity_evidence.len() != edge_count
        {
            return Some(Ok(topology));
        }

        let edge_vertices = match topology.edge_vertices(ctx) {
            Ok(Some(vertices)) => vertices,
            Ok(None) => return None,
            Err(error) => return Some(Err(error)),
        };
        let mut incident_faces = match ctx.alloc_filled(
            edge_count,
            Vec::<usize>::new(),
            "catia_mesh_edge_gauge_faces",
        ) {
            Ok(faces) => faces,
            Err(error) => return Some(Err(error)),
        };
        let mut usage = match ctx.alloc_filled(
            edge_count,
            Vec::<(usize, usize, usize, bool, usize, usize)>::new(),
            "catia_mesh_edge_gauge_usage",
        ) {
            Ok(usage) => usage,
            Err(error) => return Some(Err(error)),
        };
        for (face, face_topology) in topology.faces.iter().enumerate() {
            for (boundary, boundary_topology) in face_topology.boundaries.iter().enumerate() {
                for (position, coedge) in boundary_topology.coedges.iter().enumerate() {
                    let faces = incident_faces.get_mut(coedge.edge_row)?;
                    if !faces.contains(&face) {
                        if let Err(error) = crate::resource::push(
                            ctx,
                            faces,
                            face,
                            "catia_mesh_edge_gauge_incident_face_entries",
                        ) {
                            return Some(Err(error));
                        }
                    }
                    if let Err(error) = crate::resource::push(
                        ctx,
                        usage.get_mut(coedge.edge_row)?,
                        (
                            face,
                            boundary,
                            position,
                            coedge.reversed,
                            coedge.start_vertex,
                            coedge.end_vertex,
                        ),
                        "catia_mesh_edge_gauge_usage_entries",
                    ) {
                        return Some(Err(error));
                    }
                }
            }
        }
        for faces in &mut incident_faces {
            faces.sort_unstable();
        }
        for uses in &mut usage {
            uses.sort_unstable();
        }

        if gauge.edge_faces.len() == edge_count {
            for (edge, actual_faces) in incident_faces.iter().enumerate() {
                let mut expected_faces = *gauge.edge_faces.get(edge)?;
                expected_faces.sort_unstable();
                if *actual_faces != expected_faces {
                    return None;
                }
            }
        }

        let mut endpoint_keys = Vec::new();
        for mut pair in edge_vertices.iter().copied() {
            pair.sort_unstable();
            if let Err(error) = crate::resource::push(
                ctx,
                &mut endpoint_keys,
                pair,
                "catia_mesh_edge_gauge_endpoint_keys",
            ) {
                return Some(Err(error));
            }
        }
        let mut source_option_keys = Vec::new();
        for options in gauge.edge_candidates {
            let key = match normalized_endpoint_options(ctx, options) {
                Ok(key) => key,
                Err(error) => return Some(Err(error)),
            };
            if let Err(error) = crate::resource::push(
                ctx,
                &mut source_option_keys,
                key,
                "catia_mesh_gauge_option_keys",
            ) {
                return Some(Err(error));
            }
        }
        let mut source_groups = BTreeMap::<MeshTopologyEdgeGaugeKey, Vec<usize>>::new();
        let mut target_groups = BTreeMap::<MeshTopologyEdgeGaugeKey, Vec<usize>>::new();
        for edge in 0..edge_count {
            if gauge.edge_identity_evidence[edge] {
                continue;
            }
            let row = &topology.edge_rows[edge];
            let base = (
                row.kind,
                row.boundary_layout,
                gauge.edge_geometry[edge],
                row.handles.len(),
                match crate::resource::copy_retained_slice(
                    ctx,
                    &incident_faces[edge],
                    "catia_mesh_gauge_incident_copy",
                ) {
                    Ok(faces) => faces,
                    Err(error) => return Some(Err(error)),
                },
            );
            let target_options = match permutation {
                Some(permutation) => match mapped_normalized_endpoint_options(
                    ctx,
                    &gauge.edge_candidates[edge],
                    permutation,
                ) {
                    Ok(Some(options)) => options,
                    Ok(None) => return None,
                    Err(error) => return Some(Err(error)),
                },
                None => match crate::resource::copy_retained_slice(
                    ctx,
                    &source_option_keys[edge],
                    "catia_mesh_gauge_target_options",
                ) {
                    Ok(options) => options,
                    Err(error) => return Some(Err(error)),
                },
            };
            let base_copy = match crate::resource::copy_retained_slice(
                ctx,
                &base.4,
                "catia_mesh_gauge_base_faces_copy",
            ) {
                Ok(faces) => (base.0, base.1, base.2, base.3, faces),
                Err(error) => return Some(Err(error)),
            };
            let source_key = (base_copy, target_options);
            if !source_groups.contains_key(&source_key) {
                if let Err(error) = ctx.charge_collection_items(1, "catia_mesh_gauge_source_keys") {
                    return Some(Err(error));
                }
            }
            if let Err(error) = crate::resource::push(
                ctx,
                source_groups.entry(source_key).or_default(),
                edge,
                "catia_mesh_gauge_source_edges",
            ) {
                return Some(Err(error));
            }
            let source_options = match crate::resource::copy_retained_slice(
                ctx,
                &source_option_keys[edge],
                "catia_mesh_gauge_source_options",
            ) {
                Ok(options) => options,
                Err(error) => return Some(Err(error)),
            };
            let target_key = (base, source_options);
            if !target_groups.contains_key(&target_key) {
                if let Err(error) = ctx.charge_collection_items(1, "catia_mesh_gauge_target_keys") {
                    return Some(Err(error));
                }
            }
            if let Err(error) = crate::resource::push(
                ctx,
                target_groups.entry(target_key).or_default(),
                edge,
                "catia_mesh_gauge_target_edges",
            ) {
                return Some(Err(error));
            }
        }

        let mut row_permutation =
            match ctx.alloc_filled(edge_count, 0usize, "catia_mesh_edge_gauge_row_permutation") {
                Ok(permutation) => permutation,
                Err(error) => return Some(Err(error)),
            };
        for (edge, slot) in row_permutation.iter_mut().enumerate() {
            *slot = edge;
        }
        let mut normalize_rows =
            match ctx.alloc_filled(edge_count, false, "catia_mesh_edge_gauge_normalize_rows") {
                Ok(rows) => rows,
                Err(error) => return Some(Err(error)),
            };
        for (key, group) in source_groups {
            let mut slots = target_groups.remove(&key)?;
            if slots.len() != group.len() {
                return None;
            }
            slots.sort_unstable();
            let mut ordered = match crate::resource::copy_retained_slice(
                ctx,
                &group,
                "catia_mesh_gauge_ordered_group",
            ) {
                Ok(group) => group,
                Err(error) => return Some(Err(error)),
            };
            ordered.sort_unstable_by(|left, right| {
                endpoint_keys[*left]
                    .cmp(&endpoint_keys[*right])
                    .then_with(|| usage[*left].cmp(&usage[*right]))
                    .then_with(|| left.cmp(right))
            });
            for (slot, old_edge) in slots.into_iter().zip(ordered) {
                row_permutation[old_edge] = slot;
                normalize_rows[slot] = group.len() > 1 || old_edge != slot;
            }
        }
        if !target_groups.is_empty() {
            return None;
        }
        if !normalize_rows.iter().any(|normalize| *normalize) {
            return Some(Ok(topology));
        }

        let mut permuting = match crate::resource::copy_retained_slice(
            ctx,
            &row_permutation,
            "catia_mesh_gauge_row_permutation_copy",
        ) {
            Ok(permuting) => permuting,
            Err(error) => return Some(Err(error)),
        };
        let mut new_rows = std::mem::take(&mut topology.edge_rows);
        for old_edge in 0..edge_count {
            while permuting[old_edge] != old_edge {
                let new_edge = permuting[old_edge];
                new_rows.swap(old_edge, new_edge);
                permuting.swap(old_edge, new_edge);
            }
        }
        for (edge, row) in new_rows.iter_mut().enumerate() {
            if normalize_rows[edge] {
                row.handles.fill(0);
            }
        }
        for coedge in topology
            .faces
            .iter_mut()
            .flat_map(|face| &mut face.boundaries)
            .flat_map(|boundary| &mut boundary.coedges)
        {
            coedge.edge_row = *row_permutation.get(coedge.edge_row)?;
        }
        topology.edge_rows = new_rows;
        if let Err(error) = canonicalize_topology_boundary_gauges(ctx, &mut topology) {
            return Some(Err(error));
        }
        Some(Ok(topology))
    })()
    .transpose()
}

#[test]
fn mesh_edge_gauge_rows_refuse_each_collection_limit() {
    use crate::families::standard::topology::{Boundary, FaceTopology};
    use std::collections::HashSet;

    let topology = StandardTopology {
        faces: vec![FaceTopology {
            boundaries: vec![Boundary {
                coedges: NonEmptyCoedges::try_from(vec![
                    CoedgeUse {
                        edge_row: 0,
                        reversed: false,
                        start_vertex: 0,
                        end_vertex: 1,
                    },
                    CoedgeUse {
                        edge_row: 1,
                        reversed: false,
                        start_vertex: 1,
                        end_vertex: 0,
                    },
                ])
                .expect("two coedges are nonempty"),
            }],
        }],
        edge_rows: vec![
            EdgeRow {
                kind: 1,
                handles: vec![0, 1],
                boundary_layout: EdgeBoundaryLayout::CompleteBoundaryRun,
            },
            EdgeRow {
                kind: 1,
                handles: vec![1, 0],
                boundary_layout: EdgeBoundaryLayout::CompleteBoundaryRun,
            },
        ],
        vertex_points: vec![[0.0, 0.0, 0.0], [1.0, 0.0, 0.0]],
        logical_vertex_count: 2,
    };
    let edge_faces = [];
    let edge_geometry = [MeshEdgeGeometry::Line; 2];
    let edge_candidates = [vec![[0, 1]], vec![[0, 1]]];
    let edge_identity_evidence = [false; 2];
    let gauge = MeshCandidateGauge {
        edge_rows: &topology.edge_rows,
        edge_faces: &edge_faces,
        edge_geometry: &edge_geometry,
        edge_candidates: &edge_candidates,
        edge_identity_evidence: &edge_identity_evidence,
        coordinate_gauge: None,
    };
    assert!(crate::test_support::with_service_context(|ctx| {
        canonicalize_mesh_edge_row_gauges(ctx, topology.clone(), gauge, None)
    })
    .expect("service resource budget")
    .is_some());

    let mut refused = HashSet::new();
    for cap in 0..128 {
        let result = crate::test_support::with_collection_limit(cap, |ctx| {
            canonicalize_mesh_edge_row_gauges(ctx, topology.clone(), gauge, None)
        });
        if let Err(CodecError::ResourceLimit(limit)) = result {
            refused.insert(limit.operation);
        }
    }
    for operation in [
        "catia_mesh_edge_gauge_incident_face_entries",
        "catia_mesh_edge_gauge_usage_entries",
        "catia_mesh_edge_gauge_endpoint_keys",
        "catia_mesh_edge_gauge_row_permutation",
        "catia_mesh_gauge_row_permutation_copy",
    ] {
        assert!(refused.contains(operation), "no refusal at {operation}");
    }
}

fn permute_mesh_coordinate_labels(
    ctx: &DecodeContext<'_>,
    mut topology: StandardTopology,
    permutation: &[usize],
) -> Result<Option<StandardTopology>, CodecError> {
    (|| -> Option<Result<StandardTopology, CodecError>> {
        if permutation.len() != topology.vertex_points.len() {
            return None;
        }
        let mut seen =
            match ctx.alloc_filled(permutation.len(), false, "catia_mesh_coordinate_gauge_seen") {
                Ok(seen) => seen,
                Err(error) => return Some(Err(error)),
            };
        for &target in permutation {
            let seen_target = seen.get_mut(target)?;
            if std::mem::replace(seen_target, true) {
                return None;
            }
        }
        for coedge in topology
            .faces
            .iter_mut()
            .flat_map(|face| &mut face.boundaries)
            .flat_map(|boundary| &mut boundary.coedges)
        {
            coedge.start_vertex = *permutation.get(coedge.start_vertex)?;
            coedge.end_vertex = *permutation.get(coedge.end_vertex)?;
        }
        Some(Ok(topology))
    })()
    .transpose()
}

fn mesh_topology_gauge_key(
    ctx: &DecodeContext<'_>,
    topology: &StandardTopology,
) -> Result<Vec<u64>, CodecError> {
    let mut key = Vec::new();
    let usize_key = |value: usize| {
        u64::try_from(value)
            .map_err(|_| ctx.refuse_codec_limit("catia_gauge_topology_key", u64::MAX, u64::MAX))
    };
    crate::resource::push(
        ctx,
        &mut key,
        usize_key(topology.vertex_points.len())?,
        "catia_gauge_topology_key",
    )?;
    for point in &topology.vertex_points {
        for coordinate in point {
            crate::resource::push(
                ctx,
                &mut key,
                coordinate.to_bits(),
                "catia_gauge_topology_key",
            )?;
        }
    }
    crate::resource::push(
        ctx,
        &mut key,
        usize_key(topology.logical_vertex_count)?,
        "catia_gauge_topology_key",
    )?;
    crate::resource::push(
        ctx,
        &mut key,
        usize_key(topology.edge_rows.len())?,
        "catia_gauge_topology_key",
    )?;
    for row in &topology.edge_rows {
        crate::resource::push(
            ctx,
            &mut key,
            u64::from(row.kind),
            "catia_gauge_topology_key",
        )?;
        crate::resource::push(
            ctx,
            &mut key,
            row.boundary_layout as u64,
            "catia_gauge_topology_key",
        )?;
        crate::resource::push(
            ctx,
            &mut key,
            usize_key(row.handles.len())?,
            "catia_gauge_topology_key",
        )?;
        for &handle in &row.handles {
            crate::resource::push(ctx, &mut key, u64::from(handle), "catia_gauge_topology_key")?;
        }
    }
    crate::resource::push(
        ctx,
        &mut key,
        usize_key(topology.faces.len())?,
        "catia_gauge_topology_key",
    )?;
    for face in &topology.faces {
        crate::resource::push(
            ctx,
            &mut key,
            usize_key(face.boundaries.len())?,
            "catia_gauge_topology_key",
        )?;
        for boundary in &face.boundaries {
            crate::resource::push(
                ctx,
                &mut key,
                usize_key(boundary.coedges.len())?,
                "catia_gauge_topology_key",
            )?;
            for coedge in &boundary.coedges {
                crate::resource::push(
                    ctx,
                    &mut key,
                    usize_key(coedge.edge_row)?,
                    "catia_gauge_topology_key",
                )?;
                crate::resource::push(
                    ctx,
                    &mut key,
                    u64::from(coedge.reversed),
                    "catia_gauge_topology_key",
                )?;
                crate::resource::push(
                    ctx,
                    &mut key,
                    usize_key(coedge.start_vertex)?,
                    "catia_gauge_topology_key",
                )?;
                crate::resource::push(
                    ctx,
                    &mut key,
                    usize_key(coedge.end_vertex)?,
                    "catia_gauge_topology_key",
                )?;
            }
        }
    }
    Ok(key)
}

fn canonicalize_mesh_coordinate_gauges(
    ctx: &DecodeContext<'_>,
    mut topology: StandardTopology,
    gauge: MeshCandidateGauge<'_>,
) -> Result<Option<StandardTopology>, CodecError> {
    let Some(coordinate_gauge) = gauge.coordinate_gauge else {
        return canonicalize_mesh_edge_row_gauges(ctx, topology, gauge, None);
    };
    if coordinate_gauge.components.is_empty() {
        return canonicalize_mesh_edge_row_gauges(ctx, topology, gauge, None);
    }
    for permutations in &coordinate_gauge.components {
        let mut best = None::<(Vec<u64>, StandardTopology)>;
        for permutation in permutations {
            let Some(mut candidate) =
                permute_mesh_coordinate_labels(ctx, topology.clone_charged(ctx)?, permutation)?
            else {
                return Ok(None);
            };
            canonicalize_topology_boundary_gauges(ctx, &mut candidate)?;
            let Some(canonical) =
                canonicalize_mesh_edge_row_gauges(ctx, candidate, gauge, Some(permutation))?
            else {
                return Ok(None);
            };
            candidate = canonical;
            let key = mesh_topology_gauge_key(ctx, &candidate)?;
            if best.as_ref().is_none_or(|(best_key, _)| key < *best_key) {
                best = Some((key, candidate));
            }
        }
        let Some((_, best)) = best else {
            return Ok(None);
        };
        topology = best;
    }
    Ok(Some(topology))
}

fn canonicalize_mesh_candidate(
    ctx: &DecodeContext<'_>,
    source_topology: &StandardTopology,
    point_assignment: &[usize],
    gauge: Option<MeshCandidateGauge<'_>>,
) -> Result<Option<(StandardTopology, Vec<usize>)>, CodecError> {
    (|| -> Option<Result<(StandardTopology, Vec<usize>), CodecError>> {
        let mut topology = match source_topology.clone_charged(ctx) {
            Ok(topology) => topology,
            Err(error) => return Some(Err(error)),
        };
        if point_assignment.len() != topology.logical_vertex_count
            || point_assignment.len() != topology.vertex_points.len()
        {
            return None;
        }
        let mut seen =
            match ctx.alloc_filled(point_assignment.len(), false, "catia_mesh_vertex_seen") {
                Ok(seen) => seen,
                Err(error) => return Some(Err(error)),
            };
        for &point in point_assignment {
            let entry = seen.get_mut(point)?;
            if std::mem::replace(entry, true) {
                return None;
            }
        }
        for coedge in topology
            .faces
            .iter_mut()
            .flat_map(|face| &mut face.boundaries)
            .flat_map(|boundary| &mut boundary.coedges)
        {
            coedge.start_vertex = *point_assignment.get(coedge.start_vertex)?;
            coedge.end_vertex = *point_assignment.get(coedge.end_vertex)?;
        }
        let mut edge_vertices =
            match ctx.alloc_filled(topology.edge_rows.len(), None, "catia_mesh_edge_vertices") {
                Ok(vertices) => vertices,
                Err(error) => return Some(Err(error)),
            };
        for coedge in topology
            .faces
            .iter()
            .flat_map(|face| &face.boundaries)
            .flat_map(|boundary| &boundary.coedges)
        {
            let vertices = if coedge.reversed {
                [coedge.end_vertex, coedge.start_vertex]
            } else {
                [coedge.start_vertex, coedge.end_vertex]
            };
            let stored = edge_vertices.get_mut(coedge.edge_row)?;
            match stored {
                Some(existing) if *existing != vertices => return None,
                Some(_) => {}
                None => *stored = Some(vertices),
            }
        }
        let mut reverse_edges = Vec::new();
        for vertices in edge_vertices {
            if let Err(error) = crate::resource::push(
                ctx,
                &mut reverse_edges,
                vertices.is_some_and(|vertices| vertices[0] > vertices[1]),
                "catia_mesh_reverse_edges",
            ) {
                return Some(Err(error));
            }
        }
        for coedge in topology
            .faces
            .iter_mut()
            .flat_map(|face| &mut face.boundaries)
            .flat_map(|boundary| &mut boundary.coedges)
        {
            if *reverse_edges.get(coedge.edge_row)? {
                coedge.reversed = !coedge.reversed;
            }
        }
        if let Err(error) = canonicalize_topology_boundary_gauges(ctx, &mut topology) {
            return Some(Err(error));
        }
        if let Some(gauge) = gauge {
            topology = match canonicalize_mesh_coordinate_gauges(ctx, topology, gauge) {
                Ok(Some(topology)) => topology,
                Ok(None) => return None,
                Err(error) => return Some(Err(error)),
            };
        }
        let mut identity = Vec::new();
        if let Err(error) = crate::resource::reserve_vec(
            ctx,
            &mut identity,
            point_assignment.len(),
            "catia_mesh_candidate_identity",
        ) {
            return Some(Err(error));
        }
        identity.extend(0..point_assignment.len());
        Some(Ok((topology, identity)))
    })()
    .transpose()
}

/// Materialize the canonical representative of one admitted topology orbit.
///
/// Returns `None` when the candidate does not satisfy the canonicalizer's
/// structural invariants.
pub(super) fn canonicalize_mesh_candidate_for_output(
    ctx: &DecodeContext<'_>,
    topology: &StandardTopology,
    point_assignment: &[usize],
    gauge: Option<MeshCandidateGauge<'_>>,
) -> Result<Option<(StandardTopology, Vec<usize>)>, CodecError> {
    canonicalize_mesh_candidate(ctx, topology, point_assignment, gauge)
}

#[cfg(test)]
pub(crate) fn canonicalize_mesh_vertex_labels(
    ctx: &DecodeContext<'_>,
    topology: &StandardTopology,
    point_assignment: &[usize],
) -> Result<Option<(StandardTopology, Vec<usize>)>, CodecError> {
    canonicalize_mesh_candidate(ctx, topology, point_assignment, None)
}

#[cfg(test)]
pub(crate) fn mesh_candidates_equivalent(
    ctx: &DecodeContext<'_>,
    left: &(StandardTopology, Vec<usize>),
    right: &(StandardTopology, Vec<usize>),
) -> Result<bool, CodecError> {
    mesh_candidates_equivalent_with_context(ctx, left, right, None)
}

#[cfg(test)]
pub(crate) fn mesh_candidates_equivalent_with_gauge(
    ctx: &DecodeContext<'_>,
    left: &(StandardTopology, Vec<usize>),
    right: &(StandardTopology, Vec<usize>),
    edge_geometry: &[MeshEdgeGeometry],
    edge_candidates: &[Vec<[usize; 2]>],
    edge_identity_evidence: &[bool],
) -> Result<bool, CodecError> {
    mesh_candidates_equivalent_with_context(
        ctx,
        left,
        right,
        Some(MeshCandidateGauge {
            edge_rows: &[],
            edge_faces: &[],
            edge_geometry,
            edge_candidates,
            edge_identity_evidence,
            coordinate_gauge: None,
        }),
    )
}

pub(super) fn mesh_candidates_equivalent_with_context(
    ctx: &DecodeContext<'_>,
    left: &(StandardTopology, Vec<usize>),
    right: &(StandardTopology, Vec<usize>),
    gauge: Option<MeshCandidateGauge<'_>>,
) -> Result<bool, CodecError> {
    if left.0.vertex_points != right.0.vertex_points {
        return Ok(false);
    }
    let left = canonicalize_mesh_candidate(ctx, &left.0, &left.1, gauge)?;
    let right = canonicalize_mesh_candidate(ctx, &right.0, &right.1, gauge)?;
    let equivalent = matches!((&left, &right), (Some(left), Some(right)) if left == right);
    Ok(equivalent)
}

#[test]
fn endpoint_pair_gauge_canonicalization_snapshots_row_values() {
    catia_test_context!(ctx);
    let edge_rows = [
        EdgeRow {
            kind: 1,
            handles: Vec::new(),
            boundary_layout: EdgeBoundaryLayout::CompleteBoundaryRun,
        },
        EdgeRow {
            kind: 1,
            handles: Vec::new(),
            boundary_layout: EdgeBoundaryLayout::CompleteBoundaryRun,
        },
    ];
    let edge_faces = [[0, 1], [0, 1]];
    let edge_geometry = [MeshEdgeGeometry::Line, MeshEdgeGeometry::Line];
    let edge_candidates = vec![vec![[0, 1], [2, 3]], vec![[0, 1], [2, 3]]];
    let edge_identity_evidence = [false, false];
    let gauge = MeshCandidateGauge {
        edge_rows: &edge_rows,
        edge_faces: &edge_faces,
        edge_geometry: &edge_geometry,
        edge_candidates: &edge_candidates,
        edge_identity_evidence: &edge_identity_evidence,
        coordinate_gauge: None,
    };

    let left = [[0, 1], [2, 3]].into_iter().map(Some).collect::<Vec<_>>();
    let right = [[2, 3], [0, 1]].into_iter().map(Some).collect::<Vec<_>>();
    assert_eq!(
        canonicalize_partial_endpoint_pair_gauge(&ctx, &left, gauge)
            .expect("service resource budget"),
        canonicalize_partial_endpoint_pair_gauge(&ctx, &right, gauge)
            .expect("service resource budget"),
    );
}

#[test]
fn complete_endpoint_pair_gauge_charges_options_and_result() {
    let rows = [EdgeRow {
        kind: 1,
        handles: Vec::new(),
        boundary_layout: EdgeBoundaryLayout::CompleteBoundaryRun,
    }];
    let faces = [[0, 1]];
    let geometry = [MeshEdgeGeometry::Line];
    let candidates = [vec![[0, 1]]];
    let evidence = [false];
    let gauge = MeshCandidateGauge {
        edge_rows: &rows,
        edge_faces: &faces,
        edge_geometry: &geometry,
        edge_candidates: &candidates,
        edge_identity_evidence: &evidence,
        coordinate_gauge: None,
    };
    let run = |ctx: &DecodeContext<'_>| canonicalize_complete_endpoint_pairs(ctx, &[[0, 1]], gauge);
    assert_eq!(
        crate::test_support::with_service_context(run).expect("service resource budget"),
        Some(vec![[0, 1]])
    );
    let mut refused = std::collections::HashSet::new();
    for cap in 0..128 {
        match crate::test_support::with_collection_limit(cap, run) {
            Err(CodecError::ResourceLimit(limit)) => {
                refused.insert(limit.operation);
            }
            Ok(Some(_)) => break,
            _ => panic!("unexpected gauge result"),
        }
    }
    for operation in [
        "catia_gauge_input_pairs",
        "catia_gauge_canonical_pairs",
        "catia_gauge_normalized_options",
        "catia_gauge_source_group_keys",
        "catia_gauge_target_group_keys",
        "catia_gauge_complete_pairs",
    ] {
        assert!(refused.contains(operation), "no refusal at {operation}");
    }
}

#[test]
fn mesh_candidate_comparison_collapses_coordinate_row_gauge() {
    catia_test_context!(ctx);
    let edge_rows = vec![
        EdgeRow {
            kind: 1,
            handles: vec![10],
            boundary_layout: EdgeBoundaryLayout::CompleteBoundaryRun,
        },
        EdgeRow {
            kind: 1,
            handles: vec![11],
            boundary_layout: EdgeBoundaryLayout::CompleteBoundaryRun,
        },
    ];
    let edge_faces = vec![[0, 1], [0, 1]];
    let edge_geometry = vec![MeshEdgeGeometry::Line, MeshEdgeGeometry::Line];
    let edge_candidates = vec![vec![[0, 1], [2, 3]], vec![[0, 1], [2, 3]]];
    let edge_identity_evidence = vec![false, false];
    let coordinate_permutation = vec![2, 3, 0, 1];
    let coordinate_identity = (0..4).collect::<Vec<_>>();
    let coordinate_gauge = MeshCoordinateGauge {
        components: vec![vec![coordinate_identity, coordinate_permutation]],
    };
    let gauge = MeshCandidateGauge {
        edge_rows: &edge_rows,
        edge_faces: &edge_faces,
        edge_geometry: &edge_geometry,
        edge_candidates: &edge_candidates,
        edge_identity_evidence: &edge_identity_evidence,
        coordinate_gauge: Some(&coordinate_gauge),
    };
    let topology = |swapped: bool| StandardTopology {
        faces: vec![
            crate::families::standard::topology::FaceTopology {
                boundaries: vec![crate::families::standard::topology::Boundary::new(vec![
                    CoedgeUse {
                        edge_row: 0,
                        reversed: false,
                        start_vertex: if swapped { 2 } else { 0 },
                        end_vertex: if swapped { 3 } else { 1 },
                    },
                    CoedgeUse {
                        edge_row: 1,
                        reversed: false,
                        start_vertex: if swapped { 3 } else { 1 },
                        end_vertex: if swapped { 2 } else { 0 },
                    },
                ])
                .expect("nonempty topology boundary")],
            },
            crate::families::standard::topology::FaceTopology {
                boundaries: vec![crate::families::standard::topology::Boundary::new(vec![
                    CoedgeUse {
                        edge_row: 0,
                        reversed: false,
                        start_vertex: if swapped { 2 } else { 0 },
                        end_vertex: if swapped { 3 } else { 1 },
                    },
                    CoedgeUse {
                        edge_row: 1,
                        reversed: false,
                        start_vertex: if swapped { 3 } else { 1 },
                        end_vertex: if swapped { 2 } else { 0 },
                    },
                ])
                .expect("nonempty topology boundary")],
            },
        ],
        edge_rows: edge_rows.clone(),
        vertex_points: vec![
            [0.0, 0.0, 0.0],
            [1.0, 0.0, 0.0],
            [2.0, 0.0, 0.0],
            [3.0, 0.0, 0.0],
        ],
        logical_vertex_count: 4,
    };
    let left = (topology(false), vec![0, 1, 2, 3]);
    let right = (topology(true), vec![0, 1, 2, 3]);

    assert!(
        !mesh_candidates_equivalent_with_context(&ctx, &left, &right, None)
            .expect("service resource budget")
    );
    assert!(
        mesh_candidates_equivalent_with_context(&ctx, &left, &right, Some(gauge))
            .expect("service resource budget")
    );

    let mut mismatched_topology = topology(false);
    let boundary = &mut mismatched_topology.faces[1].boundaries[0];
    boundary.coedges = NonEmptyCoedges::one(boundary.coedges[0]);
    let mismatched = (mismatched_topology, vec![0, 1, 2, 3]);
    assert!(
        !mesh_candidates_equivalent_with_context(&ctx, &left, &mismatched, Some(gauge))
            .expect("service resource budget")
    );
}

#[test]
fn mesh_candidate_comparison_collapses_independent_seam_row_coordinate_automorphisms() {
    const COMPONENT_COUNT: usize = 3;
    catia_test_context!(ctx);
    let edge_rows = (0..COMPONENT_COUNT * 2)
        .map(|edge| EdgeRow {
            kind: 2,
            handles: vec![(edge * 2) as u32, (edge * 2 + 1) as u32],
            boundary_layout: EdgeBoundaryLayout::CompleteBoundaryRun,
        })
        .collect::<Vec<_>>();
    let edge_faces = (0..COMPONENT_COUNT * 2).map(|_| [0, 1]).collect::<Vec<_>>();
    let edge_geometry = [
        MeshEdgeGeometry::Line,
        MeshEdgeGeometry::Line,
        MeshEdgeGeometry::Circle {
            center: [0, 0, 0],
            radius: 1,
        },
        MeshEdgeGeometry::Circle {
            center: [0, 0, 0],
            radius: 1,
        },
        MeshEdgeGeometry::Bspline,
        MeshEdgeGeometry::Bspline,
    ];
    let edge_candidates = (0..COMPONENT_COUNT)
        .flat_map(|component| {
            let point = component * 4;
            let options = vec![[point, point + 3], [point + 1, point + 2]];
            [options.clone(), options]
        })
        .collect::<Vec<_>>();
    let edge_identity_evidence = (0..COMPONENT_COUNT * 2).map(|_| false).collect::<Vec<_>>();
    let coordinate_gauge = build_mesh_coordinate_gauge(
        &ctx,
        COMPONENT_COUNT * 4,
        &edge_rows,
        &edge_faces,
        &edge_geometry,
        &edge_candidates,
        &edge_identity_evidence,
    )
    .expect("service resource budget");
    assert_eq!(coordinate_gauge.components.len(), COMPONENT_COUNT);
    for component in 0..COMPONENT_COUNT {
        let mut coordinate_swap = (0..COMPONENT_COUNT * 4).collect::<Vec<_>>();
        let point = component * 4;
        coordinate_swap[point] = point + 1;
        coordinate_swap[point + 1] = point;
        coordinate_swap[point + 2] = point + 3;
        coordinate_swap[point + 3] = point + 2;
        assert!(coordinate_gauge
            .components
            .iter()
            .any(|component| component.contains(&coordinate_swap)));
    }
    let gauge = MeshCandidateGauge {
        edge_rows: &edge_rows,
        edge_faces: &edge_faces,
        edge_geometry: &edge_geometry,
        edge_candidates: &edge_candidates,
        edge_identity_evidence: &edge_identity_evidence,
        coordinate_gauge: Some(&coordinate_gauge),
    };

    let topology = |mask: usize| {
        let endpoints = (0..COMPONENT_COUNT)
            .flat_map(|component| {
                let point = component * 4;
                if mask & (1 << component) == 0 {
                    [[point, point + 3], [point + 1, point + 2]]
                } else {
                    [[point + 1, point + 2], [point, point + 3]]
                }
            })
            .collect::<Vec<_>>();
        let face = || crate::families::standard::topology::FaceTopology {
            boundaries: vec![crate::families::standard::topology::Boundary::new(
                endpoints
                    .iter()
                    .copied()
                    .enumerate()
                    .map(|(edge_row, [start_vertex, end_vertex])| CoedgeUse {
                        edge_row,
                        reversed: false,
                        start_vertex,
                        end_vertex,
                    })
                    .collect(),
            )
            .expect("nonempty topology boundary")],
        };
        StandardTopology {
            faces: vec![face(), face()],
            edge_rows: edge_rows.clone(),
            vertex_points: (0..COMPONENT_COUNT * 4)
                .map(|point| [point as f64, 0.0, 0.0])
                .collect(),
            logical_vertex_count: COMPONENT_COUNT * 4,
        }
    };
    let identity = (0..COMPONENT_COUNT * 4).collect::<Vec<_>>();
    let left = (topology(0), identity.clone());
    let canonical_left =
        canonicalize_mesh_candidate_for_output(&ctx, &left.0, &left.1, Some(gauge))
            .expect("service resource budget")
            .expect("seam candidate canonicalization");

    for mask in 1..(1 << COMPONENT_COUNT) {
        let right = (topology(mask), identity.clone());
        assert!(
            !mesh_candidates_equivalent_with_context(&ctx, &left, &right, None)
                .expect("service resource budget")
        );
        assert!(
            mesh_candidates_equivalent_with_context(&ctx, &left, &right, Some(gauge))
                .expect("service resource budget"),
            "seam gauge did not collapse mask {mask:#b}"
        );
        assert_eq!(
            canonical_left,
            canonicalize_mesh_candidate_for_output(&ctx, &right.0, &right.1, Some(gauge))
                .expect("service resource budget")
                .expect("seam candidate canonicalization"),
            "seam gauge representative depends on search arrival order for mask {mask:#b}"
        );
    }

    let mut displaced = topology(0);
    displaced.vertex_points[0][0] = -1.0;
    assert!(!mesh_candidates_equivalent_with_context(
        &ctx,
        &left,
        &(displaced, identity),
        Some(gauge)
    )
    .expect("service resource budget"));

    let mut mismatched_geometry = edge_geometry;
    mismatched_geometry[1] = MeshEdgeGeometry::Circle {
        center: [1, 0, 0],
        radius: 1,
    };
    let mismatched_coordinate_gauge = build_mesh_coordinate_gauge(
        &ctx,
        COMPONENT_COUNT * 4,
        &edge_rows,
        &edge_faces,
        &mismatched_geometry,
        &edge_candidates,
        &edge_identity_evidence,
    )
    .expect("service resource budget");
    let mismatched_gauge = MeshCandidateGauge {
        edge_rows: &edge_rows,
        edge_faces: &edge_faces,
        edge_geometry: &mismatched_geometry,
        edge_candidates: &edge_candidates,
        edge_identity_evidence: &edge_identity_evidence,
        coordinate_gauge: Some(&mismatched_coordinate_gauge),
    };
    let mut row_swapped = topology(0);
    row_swapped.edge_rows.swap(0, 1);
    assert!(!mesh_candidates_equivalent_with_context(
        &ctx,
        &left,
        &(row_swapped, (0..COMPONENT_COUNT * 4).collect()),
        Some(mismatched_gauge),
    )
    .expect("service resource budget"));
}

pub(super) fn canonicalize_endpoint_relation_state(
    ctx: &DecodeContext<'_>,
    domains: &[Vec<MeshEndpointRelationChoice>],
    assigned: &[Option<[usize; 2]>],
    gauge: MeshCandidateGauge<'_>,
) -> Result<Option<MeshEndpointRelationStateSignature>, CodecError> {
    let mut state = raw_endpoint_relation_state_signature(ctx, domains, assigned)?;

    let gauge_point_count = gauge.coordinate_gauge.and_then(|coordinate_gauge| {
        coordinate_gauge
            .components
            .iter()
            .filter_map(|permutations| permutations.first())
            .map(Vec::len)
            .next()
    });
    let point_count = if let Some(count) = gauge_point_count {
        count
    } else if let Some(point) = gauge
        .edge_candidates
        .iter()
        .flatten()
        .flatten()
        .copied()
        .max()
    {
        point.checked_add(1).ok_or_else(|| {
            ctx.refuse_codec_limit("catia_relation_point_count", u64::MAX, u64::MAX)
        })?
    } else {
        0
    };
    let mut identity = Vec::new();
    crate::resource::reserve_vec(ctx, &mut identity, point_count, "catia_relation_identity")?;
    identity.extend(0..point_count);
    let Some(mapped) = map_endpoint_relation_state(ctx, &state, gauge, &identity)? else {
        return Ok(None);
    };
    state = mapped;
    if let Some(coordinate_gauge) = gauge.coordinate_gauge {
        for permutations in &coordinate_gauge.components {
            if permutations.len() <= 1 {
                continue;
            }
            let mut best = copy_endpoint_relation_state(ctx, &state)?;
            for permutation in permutations {
                let Some(candidate) = map_endpoint_relation_state(ctx, &state, gauge, permutation)?
                else {
                    return Ok(None);
                };
                if candidate < best {
                    best = candidate;
                }
            }
            state = best;
        }
    }
    Ok(Some(state))
}

fn copy_endpoint_relation_state(
    ctx: &DecodeContext<'_>,
    state: &MeshEndpointRelationStateSignature,
) -> Result<MeshEndpointRelationStateSignature, CodecError> {
    let assigned =
        crate::resource::copy_retained_slice(ctx, &state.0, "catia_relation_state_assigned_copy")?;
    let mut domains = Vec::new();
    for row in &state.1 {
        let mut choices = Vec::new();
        for selection in row {
            let copied = match selection {
                MeshEndpointRelationSelection::Deferred => MeshEndpointRelationSelection::Deferred,
                MeshEndpointRelationSelection::Enumerated {
                    assignments,
                    edge_pairs,
                } => MeshEndpointRelationSelection::Enumerated {
                    assignments: crate::resource::copy_retained_slice(
                        ctx,
                        assignments,
                        "catia_relation_state_assignment_copy",
                    )?,
                    edge_pairs: crate::resource::copy_retained_slice(
                        ctx,
                        edge_pairs,
                        "catia_relation_state_pair_copy",
                    )?,
                },
            };
            crate::resource::push(
                ctx,
                &mut choices,
                copied,
                "catia_relation_state_choice_copy",
            )?;
        }
        crate::resource::push(
            ctx,
            &mut domains,
            choices,
            "catia_relation_state_domain_copy",
        )?;
    }
    Ok((assigned, domains))
}

fn map_endpoint_relation_state(
    ctx: &DecodeContext<'_>,
    state: &MeshEndpointRelationStateSignature,
    gauge: MeshCandidateGauge<'_>,
    permutation: &[usize],
) -> Result<Option<MeshEndpointRelationStateSignature>, CodecError> {
    let Some(row_mapping) = relation_row_gauge_mapping(ctx, state, gauge, permutation)? else {
        return Ok(None);
    };
    let mut assigned = ctx.alloc_filled(state.0.len(), None, "catia_relation_mapped_assigned")?;
    for (edge, pair) in state.0.iter().copied().enumerate() {
        let Some(&target) = row_mapping.get(edge) else {
            return Ok(None);
        };
        let pair = match pair {
            Some(pair) => {
                let Some(pair) = mapped_endpoint_pair(Some(pair), Some(permutation)) else {
                    return Ok(None);
                };
                Some(pair)
            }
            None => None,
        };
        let Some(slot) = assigned.get_mut(target) else {
            return Ok(None);
        };
        *slot = pair;
    }
    let mut domains = Vec::new();
    for row in &state.1 {
        let mut choices = Vec::new();
        for selection in row {
            let mapped = match selection {
                MeshEndpointRelationSelection::Deferred => MeshEndpointRelationSelection::Deferred,
                MeshEndpointRelationSelection::Enumerated {
                    assignments,
                    edge_pairs: pairs,
                } => {
                    let mut mapped_pairs = Vec::new();
                    for &(edge, pair) in pairs {
                        let Some(&target) = row_mapping.get(edge) else {
                            return Ok(None);
                        };
                        if mapped_pairs
                            .iter()
                            .any(|(candidate, _)| *candidate == target)
                        {
                            return Ok(None);
                        }
                        let Some(pair) = mapped_endpoint_pair(Some(pair), Some(permutation)) else {
                            return Ok(None);
                        };
                        crate::resource::push(
                            ctx,
                            &mut mapped_pairs,
                            (target, pair),
                            "catia_relation_mapped_pairs",
                        )?;
                    }
                    mapped_pairs.sort_unstable();
                    MeshEndpointRelationSelection::Enumerated {
                        assignments: crate::resource::copy_retained_slice(
                            ctx,
                            assignments,
                            "catia_relation_mapped_assignments",
                        )?,
                        edge_pairs: mapped_pairs,
                    }
                }
            };
            crate::resource::push(ctx, &mut choices, mapped, "catia_relation_mapped_choices")?;
        }
        choices.sort_unstable();
        crate::resource::push(ctx, &mut domains, choices, "catia_relation_mapped_domains")?;
    }
    Ok(Some((assigned, domains)))
}

fn relation_row_gauge_mapping(
    ctx: &DecodeContext<'_>,
    state: &MeshEndpointRelationStateSignature,
    gauge: MeshCandidateGauge<'_>,
    permutation: &[usize],
) -> Result<Option<Vec<usize>>, CodecError> {
    let edge_count = state.0.len();
    let identity = || -> Result<Vec<usize>, CodecError> {
        let mut rows = Vec::new();
        crate::resource::reserve_vec(ctx, &mut rows, edge_count, "catia_relation_row_identity")?;
        rows.extend(0..edge_count);
        Ok(rows)
    };
    if gauge.edge_rows.len() != edge_count
        || gauge.edge_faces.len() != edge_count
        || gauge.edge_geometry.len() != edge_count
        || gauge.edge_candidates.len() != edge_count
        || gauge.edge_identity_evidence.len() != edge_count
    {
        return Ok(Some(identity()?));
    }

    let mut source_groups = BTreeMap::<MeshEdgeGaugeKey, Vec<usize>>::new();
    let mut target_groups = BTreeMap::<MeshEdgeGaugeKey, Vec<usize>>::new();
    for edge in 0..edge_count {
        if gauge.edge_identity_evidence[edge] {
            continue;
        }
        let Some(base) =
            mesh_edge_gauge_base_key(edge, gauge.edge_rows, gauge.edge_faces, gauge.edge_geometry)
        else {
            return Ok(None);
        };
        let source_options = normalized_endpoint_options(ctx, &gauge.edge_candidates[edge])?;
        let Some(target_options) =
            mapped_normalized_endpoint_options(ctx, &gauge.edge_candidates[edge], permutation)?
        else {
            return Ok(None);
        };
        let source_key = (base, target_options);
        if !source_groups.contains_key(&source_key) {
            ctx.charge_collection_items(1, "catia_relation_source_keys")?;
        }
        crate::resource::push(
            ctx,
            source_groups.entry(source_key).or_default(),
            edge,
            "catia_relation_source_edges",
        )?;
        let target_key = (base, source_options);
        if !target_groups.contains_key(&target_key) {
            ctx.charge_collection_items(1, "catia_relation_target_keys")?;
        }
        crate::resource::push(
            ctx,
            target_groups.entry(target_key).or_default(),
            edge,
            "catia_relation_target_edges",
        )?;
    }

    let mut row_mapping = identity()?;
    for (key, mut source) in source_groups {
        let Some(mut targets) = target_groups.remove(&key) else {
            return Ok(None);
        };
        source.sort_unstable();
        targets.sort_unstable();
        if source.len() != targets.len() {
            return Ok(None);
        }
        if source.len() == 1 {
            // A one-to-one structural group has no row-order gauge. Its map
            // is fixed by the input option relation and needs no state scan.
            let Some(slot) = row_mapping.get_mut(source[0]) else {
                return Ok(None);
            };
            *slot = targets[0];
            continue;
        }
        let mut ordered = Vec::new();
        for edge in source {
            let Some(signature) = relation_row_signature(ctx, state, edge, permutation)? else {
                return Ok(None);
            };
            crate::resource::push(
                ctx,
                &mut ordered,
                (signature, edge),
                "catia_relation_ordered_rows",
            )?;
        }
        ordered.sort_unstable();
        for (target, (_, source)) in targets.into_iter().zip(ordered) {
            let Some(slot) = row_mapping.get_mut(source) else {
                return Ok(None);
            };
            *slot = target;
        }
    }
    Ok(target_groups.is_empty().then_some(row_mapping))
}

fn relation_row_signature(
    ctx: &DecodeContext<'_>,
    state: &MeshEndpointRelationStateSignature,
    edge: usize,
    permutation: &[usize],
) -> Result<Option<Vec<Option<[usize; 2]>>>, CodecError> {
    let mut signature = Vec::new();
    let Some(&edge_pair) = state.0.get(edge) else {
        return Ok(None);
    };
    let assigned = match edge_pair {
        Some(pair) => {
            let Some(mapped) = mapped_endpoint_pair(Some(pair), Some(permutation)) else {
                return Ok(None);
            };
            Some(mapped)
        }
        None => None,
    };
    crate::resource::push(
        ctx,
        &mut signature,
        assigned,
        "catia_relation_row_signature",
    )?;
    for choices in &state.1 {
        for selection in choices {
            let pairs = selection.edge_pairs();
            let mut value = None;
            for &(candidate, pair) in pairs {
                if candidate != edge {
                    continue;
                }
                if value.is_some() {
                    return Ok(None);
                }
                let Some(mapped) = mapped_endpoint_pair(Some(pair), Some(permutation)) else {
                    return Ok(None);
                };
                value = Some(mapped);
            }
            crate::resource::push(ctx, &mut signature, value, "catia_relation_row_signature")?;
        }
    }
    Ok(Some(signature))
}

#[test]
fn relation_state_memo_collapses_coordinate_gauge() {
    catia_test_context!(ctx);
    let edge_rows = [
        EdgeRow {
            kind: 1,
            handles: vec![10],
            boundary_layout: EdgeBoundaryLayout::CompleteBoundaryRun,
        },
        EdgeRow {
            kind: 1,
            handles: vec![11],
            boundary_layout: EdgeBoundaryLayout::CompleteBoundaryRun,
        },
    ];
    let edge_faces = [[0, 1], [0, 1]];
    let edge_geometry = [MeshEdgeGeometry::Line, MeshEdgeGeometry::Line];
    let edge_candidates = vec![vec![[0, 1], [2, 3]], vec![[0, 1], [2, 3]]];
    let edge_identity_evidence = [true, true];
    let coordinate_gauge = MeshCoordinateGauge {
        components: vec![vec![vec![0, 1, 2, 3], vec![2, 3, 0, 1]]],
    };
    let gauge = MeshCandidateGauge {
        edge_rows: &edge_rows,
        edge_faces: &edge_faces,
        edge_geometry: &edge_geometry,
        edge_candidates: &edge_candidates,
        edge_identity_evidence: &edge_identity_evidence,
        coordinate_gauge: Some(&coordinate_gauge),
    };
    let state = |swapped: bool| {
        let pairs = if swapped {
            [[2, 3], [0, 1]]
        } else {
            [[0, 1], [2, 3]]
        };
        let domains = vec![vec![MeshEndpointRelationChoice {
            id: 0,
            selection: MeshEndpointRelationSelection::Enumerated {
                assignments: vec![0],
                edge_pairs: pairs.into_iter().enumerate().collect(),
            },
        }]];
        canonicalize_endpoint_relation_state(
            &ctx,
            &domains,
            &pairs.into_iter().map(Some).collect::<Vec<_>>(),
            gauge,
        )
        .expect("service resource budget")
        .expect("coordinate gauge state")
    };

    assert_eq!(state(false), state(true));
}

#[test]
fn endpoint_relation_gauge_charges_nested_state_copies() {
    let candidates = [vec![[0, 1]]];
    let coordinate_gauge = MeshCoordinateGauge {
        components: vec![vec![vec![0, 1], vec![1, 0]]],
    };
    let gauge = MeshCandidateGauge {
        edge_rows: &[],
        edge_faces: &[],
        edge_geometry: &[],
        edge_candidates: &candidates,
        edge_identity_evidence: &[],
        coordinate_gauge: Some(&coordinate_gauge),
    };
    let domains = [vec![MeshEndpointRelationChoice {
        id: 0,
        selection: MeshEndpointRelationSelection::Enumerated {
            assignments: vec![0],
            edge_pairs: vec![(0, [0, 1])],
        },
    }]];
    let run = |ctx: &DecodeContext<'_>| {
        canonicalize_endpoint_relation_state(ctx, &domains, &[Some([0, 1])], gauge)
    };
    assert!(crate::test_support::with_service_context(run)
        .expect("service resource budget")
        .is_some());
    let mut refused = std::collections::HashSet::new();
    for cap in 0..256 {
        match crate::test_support::with_collection_limit(cap, run) {
            Err(CodecError::ResourceLimit(limit)) => {
                refused.insert(limit.operation);
            }
            Ok(Some(_)) => break,
            _ => panic!("unexpected relation state result"),
        }
    }
    for operation in [
        "catia_relation_normalized_assignments",
        "catia_relation_normalized_pairs",
        "catia_relation_signature_choices",
        "catia_relation_identity",
        "catia_relation_row_identity",
        "catia_relation_mapped_assigned",
        "catia_relation_state_assignment_copy",
        "catia_relation_state_pair_copy",
    ] {
        assert!(refused.contains(operation), "no refusal at {operation}");
    }
}

#[test]
fn relation_state_memo_uses_coordinate_gauge_domain_alternatives() {
    catia_test_context!(ctx);
    let edge_rows = [
        EdgeRow {
            kind: 1,
            handles: vec![10],
            boundary_layout: EdgeBoundaryLayout::CompleteBoundaryRun,
        },
        EdgeRow {
            kind: 1,
            handles: vec![11],
            boundary_layout: EdgeBoundaryLayout::CompleteBoundaryRun,
        },
    ];
    let edge_faces = [[0, 1], [0, 1]];
    let edge_geometry = [MeshEdgeGeometry::Line, MeshEdgeGeometry::Line];
    let edge_candidates = vec![vec![[0, 3], [1, 2]], vec![[0, 3], [1, 2]]];
    let edge_identity_evidence = [true, true];
    let coordinate_gauge = MeshCoordinateGauge {
        components: vec![vec![vec![0, 1, 2, 3], vec![1, 0, 3, 2]]],
    };
    let gauge = MeshCandidateGauge {
        edge_rows: &edge_rows,
        edge_faces: &edge_faces,
        edge_geometry: &edge_geometry,
        edge_candidates: &edge_candidates,
        edge_identity_evidence: &edge_identity_evidence,
        coordinate_gauge: Some(&coordinate_gauge),
    };
    let state = |swapped: bool| {
        let pairs = if swapped {
            vec![(0, [1, 2]), (1, [0, 3])]
        } else {
            vec![(0, [0, 3]), (1, [1, 2])]
        };
        let domains = vec![vec![MeshEndpointRelationChoice {
            id: 0,
            selection: MeshEndpointRelationSelection::Enumerated {
                assignments: vec![0],
                edge_pairs: pairs,
            },
        }]];
        canonicalize_endpoint_relation_state(&ctx, &domains, &[None, None], gauge)
            .expect("service resource budget")
            .expect("coordinate gauge state")
    };

    assert_eq!(state(false), state(true));
}

#[test]
fn relation_state_memo_applies_one_row_mapping_to_assigned_and_domains() {
    catia_test_context!(ctx);
    let edge_rows = [
        EdgeRow {
            kind: 2,
            handles: vec![10, 11],
            boundary_layout: EdgeBoundaryLayout::CompleteBoundaryRun,
        },
        EdgeRow {
            kind: 2,
            handles: vec![20, 21],
            boundary_layout: EdgeBoundaryLayout::CompleteBoundaryRun,
        },
    ];
    let edge_faces = [[0, 1], [0, 1]];
    let edge_geometry = [MeshEdgeGeometry::Line, MeshEdgeGeometry::Line];
    let edge_candidates = vec![vec![[0, 3], [1, 2]], vec![[0, 3], [1, 2]]];
    let edge_identity_evidence = [false, false];
    let coordinate_gauge = build_mesh_coordinate_gauge(
        &ctx,
        4,
        &edge_rows,
        &edge_faces,
        &edge_geometry,
        &edge_candidates,
        &edge_identity_evidence,
    )
    .expect("service resource budget");
    let gauge = MeshCandidateGauge {
        edge_rows: &edge_rows,
        edge_faces: &edge_faces,
        edge_geometry: &edge_geometry,
        edge_candidates: &edge_candidates,
        edge_identity_evidence: &edge_identity_evidence,
        coordinate_gauge: Some(&coordinate_gauge),
    };
    let state = |assigned: Vec<Option<[usize; 2]>>, pairs: Vec<(usize, [usize; 2])>| {
        let domains = vec![vec![MeshEndpointRelationChoice {
            id: 0,
            selection: MeshEndpointRelationSelection::Enumerated {
                assignments: vec![0],
                edge_pairs: pairs,
            },
        }]];
        canonicalize_endpoint_relation_state(&ctx, &domains, &assigned, gauge)
            .expect("service resource budget")
            .expect("row-coordinate gauge state")
    };

    let left = state(vec![Some([0, 3]), None], vec![(0, [1, 2]), (1, [0, 3])]);
    let right = state(vec![None, Some([0, 3])], vec![(0, [0, 3]), (1, [1, 2])]);

    assert_eq!(left, right);
}

#[test]
fn mesh_coordinate_gauge_propagates_collection_refusals() {
    use cadmpeg_core::decode::{DecodeArena, DecodePolicy, ResourceDimension};

    let edge_rows = [
        EdgeRow {
            kind: 2,
            handles: vec![10, 11],
            boundary_layout: EdgeBoundaryLayout::CompleteBoundaryRun,
        },
        EdgeRow {
            kind: 2,
            handles: vec![20, 21],
            boundary_layout: EdgeBoundaryLayout::CompleteBoundaryRun,
        },
    ];
    let edge_faces = [[0, 1], [0, 1]];
    let edge_geometry = [MeshEdgeGeometry::Line, MeshEdgeGeometry::Line];
    let edge_candidates = vec![vec![[0, 3], [1, 2]], vec![[0, 3], [1, 2]]];
    let edge_identity_evidence = [false, false];
    let run = |ctx: &DecodeContext<'_>| {
        build_mesh_coordinate_gauge(
            ctx,
            4,
            &edge_rows,
            &edge_faces,
            &edge_geometry,
            &edge_candidates,
            &edge_identity_evidence,
        )
    };
    catia_test_context!(service_ctx);
    assert!(!run(&service_ctx)
        .expect("service resource budget")
        .components
        .is_empty());

    let mut refused = std::collections::HashSet::new();
    for limit in 0..2048 {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_collection_items = limit;
        let (ctx, _) = DecodeContext::from_root_bytes(&[0], &arena, &policy)
            .expect("fixture fits the input limit");
        match run(&ctx) {
            Err(CodecError::ResourceLimit(error)) => {
                assert_eq!(error.dimension, ResourceDimension::CollectionItems);
                refused.insert(error.operation.to_owned());
            }
            Ok(_) => break,
            Err(error) => panic!("unexpected refusal: {error}"),
        }
    }
    for operation in [
        "catia_coordinate_gauge_active",
        "catia_coordinate_gauge_components",
        "catia_coordinate_gauge_used",
        "catia_gauge_edge_bases",
        "catia_gauge_group_edges",
        "catia_gauge_normalized_options",
        "catia_gauge_option_arcs",
        "catia_gauge_signature_keys",
        "catia_gauge_signature_colors",
        "catia_gauge_permutation_values",
        "catia_gauge_permutations",
        "catia_gauge_components",
    ] {
        assert!(refused.contains(operation), "no refusal at {operation}");
    }
}

#[test]
fn mesh_candidate_canonicalization_propagates_collection_refusals() {
    use cadmpeg_core::decode::{DecodeArena, DecodePolicy, ResourceDimension};

    let edge_rows = vec![EdgeRow {
        kind: 1,
        handles: vec![10],
        boundary_layout: EdgeBoundaryLayout::CompleteBoundaryRun,
    }];
    let topology = StandardTopology {
        faces: vec![crate::families::standard::topology::FaceTopology {
            boundaries: vec![
                crate::families::standard::topology::Boundary::new(vec![CoedgeUse {
                    edge_row: 0,
                    reversed: false,
                    start_vertex: 0,
                    end_vertex: 1,
                }])
                .expect("nonempty topology boundary"),
            ],
        }],
        edge_rows: edge_rows.clone(),
        vertex_points: vec![[0.0, 0.0, 0.0], [1.0, 0.0, 0.0]],
        logical_vertex_count: 2,
    };
    let candidate = (topology, vec![0, 1]);
    let edge_geometry = [MeshEdgeGeometry::Line];
    let edge_candidates = vec![vec![[0, 1]]];
    let edge_identity_evidence = [false];
    let coordinate_gauge = MeshCoordinateGauge {
        components: vec![vec![vec![0, 1]]],
    };
    let gauge = MeshCandidateGauge {
        edge_rows: &edge_rows,
        edge_faces: &[],
        edge_geometry: &edge_geometry,
        edge_candidates: &edge_candidates,
        edge_identity_evidence: &edge_identity_evidence,
        coordinate_gauge: Some(&coordinate_gauge),
    };
    let run = |ctx: &DecodeContext<'_>| {
        mesh_candidates_equivalent_with_context(ctx, &candidate, &candidate, Some(gauge))
    };
    catia_test_context!(service_ctx);
    assert!(run(&service_ctx).expect("service resource budget"));
    assert!(canonicalize_mesh_candidate_for_output(
        &service_ctx,
        &candidate.0,
        &candidate.1,
        Some(gauge),
    )
    .expect("service resource budget")
    .is_some());

    let mut refused = std::collections::HashSet::new();
    for limit in 0..512 {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_collection_items = limit;
        let (ctx, _) = DecodeContext::from_root_bytes(&[0], &arena, &policy)
            .expect("fixture fits the input limit");
        match run(&ctx) {
            Err(CodecError::ResourceLimit(error)) => {
                assert_eq!(error.dimension, ResourceDimension::CollectionItems);
                refused.insert(error.operation.to_owned());
            }
            Ok(true) => break,
            Ok(false) => panic!("a candidate must equal itself"),
            Err(error) => panic!("unexpected refusal: {error}"),
        }
    }
    for operation in [
        "catia_mesh_vertex_seen",
        "catia_mesh_edge_vertices",
        "catia_mesh_coordinate_gauge_seen",
        "catia_mesh_edge_gauge_faces",
        "catia_mesh_edge_gauge_usage",
        "catia_mesh_edge_gauge_normalize_rows",
        "catia_gauge_topology_key",
        "catia_mesh_reverse_edges",
        "catia_mesh_candidate_identity",
    ] {
        assert!(refused.contains(operation), "no refusal at {operation}");
    }

    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_collection_items = 7;
    let (ctx, _) = DecodeContext::from_root_bytes(&[0], &arena, &policy)
        .expect("fixture fits the input limit");
    let error =
        canonicalize_mesh_candidate_for_output(&ctx, &candidate.0, &candidate.1, Some(gauge))
            .expect_err("canonicalization exceeds the collection limit");
    assert!(matches!(error, CodecError::ResourceLimit(limit)
        if limit.dimension == ResourceDimension::CollectionItems
            && limit.operation == "catia_mesh_vertex_seen"));
}
