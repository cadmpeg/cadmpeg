//! Evidence-preserving gauge quotient for standard mesh candidates.

use cadmpeg_ir::features::NonEmptyMembers;

use cadmpeg_core::decode::u64_from_index;

use std::collections::BTreeMap;

use cadmpeg_core::decode::DecodeContext;
use cadmpeg_core::CodecError;

use super::mesh_quotient::{
    raw_endpoint_relation_state_signature, MeshEndpointRelationChoice,
    MeshEndpointRelationSelection, MeshEndpointRelationStateSignature,
};
use crate::families::standard::topology::{
    CoedgeUse, EdgeBoundaryLayout, EdgeRow, StandardTopologyDraft,
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
pub(crate) struct MeshCandidateGauge<'a> {
    pub(super) edge_rows: &'a [EdgeRow],
    pub(super) edge_faces: &'a [[usize; 2]],
    pub(super) edge_geometry: &'a [MeshEdgeGeometry],
    pub(super) edge_candidates: &'a [Vec<[usize; 2]>],
    pub(super) edge_identity_evidence: &'a [bool],
    pub(super) coordinate_gauge: Option<&'a MeshCoordinateGauge>,
}

fn canonicalize_topology_boundary_gauges(topology: &mut StandardTopologyDraft) {
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

    fn rotate_to_minimum(coedges: &mut NonEmptyMembers<CoedgeUse>) {
        let len = coedges.len();
        let cycle = coedges.as_slice();
        let key = |start: usize| {
            signature(cycle)
                .skip(start)
                .chain(signature(cycle).take(start))
        };
        let mut best = 0;
        for start in 1..len {
            if key(start).lt(key(best)) {
                best = start;
            }
        }
        if best != 0 {
            coedges.rotate_left(best);
        }
    }

    for face in &mut topology.faces {
        for boundary in &mut face.boundaries {
            rotate_to_minimum(&mut boundary.coedges);
            let mut reversed = boundary.coedges.clone();
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
        face.boundaries
            .sort_by(|left, right| signature(&left.coedges).cmp(signature(&right.coedges)));
    }
}

#[cfg(test)]
mod boundary_tests {
    use super::{canonicalize_topology_boundary_gauges, CoedgeUse, StandardTopologyDraft};
    use crate::families::standard::topology::{BoundaryDraft, FaceTopologyDraft};

    use cadmpeg_ir::features::NonEmptyMembers;

    fn coedge(edge_row: usize, reversed: bool) -> CoedgeUse {
        CoedgeUse {
            edge_row,
            reversed,
            start_vertex: 0,
            end_vertex: 0,
        }
    }

    fn topology(boundaries: Vec<Vec<CoedgeUse>>) -> StandardTopologyDraft {
        StandardTopologyDraft {
            faces: vec![FaceTopologyDraft {
                boundaries: boundaries
                    .into_iter()
                    .map(|coedges| BoundaryDraft {
                        coedges: NonEmptyMembers::try_from(coedges).expect("nonempty test cycle"),
                    })
                    .collect(),
            }],
            edge_rows: Vec::new(),
            vertex_points: Vec::new(),
            logical_vertex_count: 0,
        }
    }

    #[test]
    fn mesh_gauge_admitted_cycles_keep_canonical_rotation_direction_and_order() {
        let mut candidate = topology(vec![
            vec![coedge(2, false), coedge(0, false), coedge(1, false)],
            vec![coedge(1, true), coedge(0, true)],
            vec![coedge(3, false); 32],
        ]);
        canonicalize_topology_boundary_gauges(&mut candidate);
        assert_eq!(
            candidate,
            topology(vec![
                vec![coedge(0, false), coedge(1, false)],
                vec![coedge(0, false), coedge(1, false), coedge(2, false)],
                vec![coedge(3, false); 32],
            ])
        );
    }
}

fn normalized_endpoint_options(options: &[[usize; 2]]) -> Vec<[usize; 2]> {
    let mut normalized = Vec::new();
    for &mut_pair in options {
        let mut pair = mut_pair;
        pair.sort_unstable_by(Ord::cmp);
        normalized.push(pair);
    }
    normalized.sort_unstable_by(Ord::cmp);
    normalized.dedup();
    normalized
}

fn mesh_edge_gauge_base_key(
    edge: usize,
    edge_rows: &[EdgeRow],
    edge_faces: &[[usize; 2]],
    edge_geometry: &[MeshEdgeGeometry],
) -> Option<MeshEdgeGaugeBaseKey> {
    let (Some(row), Some(&faces), Some(&geometry)) = (
        edge_rows.get(edge),
        edge_faces.get(edge),
        edge_geometry.get(edge),
    ) else {
        return None;
    };
    let mut faces = faces;
    faces.sort_unstable_by(Ord::cmp);
    Some((
        row.kind(),
        row.boundary_layout(),
        geometry,
        row.handles().len(),
        faces,
    ))
}

fn mapped_normalized_endpoint_options(
    options: &[[usize; 2]],
    permutation: &[usize],
) -> Option<Vec<[usize; 2]>> {
    let mut mapped = Vec::new();
    for pair in options {
        let (Some(&left), Some(&right)) = (permutation.get(pair[0]), permutation.get(pair[1]))
        else {
            return None;
        };
        mapped.push([left, right]);
    }
    Some(normalized_endpoint_options(&mapped))
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
    let _depth = ctx.enter_nested("catia_gauge_permutation_depth")?;

    if index == points.len() {
        let copy = current.clone();
        output.push(copy);
        return Ok(());
    }

    for target in 0..points.len() {
        if used[target] {
            continue;
        }
        used[target] = true;
        current.push(points[target]);
        enumerate_coordinate_permutations(ctx, points, index + 1, current, used, output)?;
        current.pop();
        used[target] = false;
    }
    Ok(())
}

/// Return `value!`, or `None` when it exceeds `limit`.
fn bounded_factorial(value: usize, limit: usize) -> Option<usize> {
    let mut result = 1usize;
    for factor in 2..=value {
        match result.checked_mul(factor) {
            Some(next) if next <= limit => result = next,
            _ => return None,
        }
    }
    Some(result)
}

fn intern_gauge_signatures<T: Ord>(
    ctx: &DecodeContext<'_>,
    signatures: impl IntoIterator<Item = T>,
) -> Result<Vec<usize>, CodecError> {
    let mut indexed = signatures.into_iter().enumerate().collect::<Vec<_>>();
    indexed.sort_unstable_by(|left, right| left.1.cmp(&right.1));
    let mut colors = ctx.alloc_filled(indexed.len(), 0usize, "catia_gauge_signature_colors")?;

    let mut first_seen = Vec::<(usize, usize)>::new();
    let mut group = 0usize;
    for (index, (position, signature)) in indexed.iter().enumerate() {
        let same = if let Some(previous) = index.checked_sub(1) {
            let previous = &indexed[previous].1;

            previous == signature
        } else {
            false
        };
        if same {
            first_seen[group].0 = first_seen[group].0.min(*position);
        } else {
            group = first_seen.len();
            first_seen.push((*position, group));
        }
        colors[*position] = group;
    }
    // Colors name signatures by their first source occurrence, independent of
    // the sorted key order used to batch their comparisons.
    first_seen.sort_unstable_by(Ord::cmp);
    let mut labels = ctx.alloc_filled(first_seen.len(), 0usize, "catia_gauge_signature_labels")?;
    for (label, (_, group)) in first_seen.into_iter().enumerate() {
        labels[group] = label;
    }

    for color in &mut colors {
        *color = labels[*color];
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
        edge_bases.push(key);
        groups.entry(key).or_default().push(edge);
    }
    let mut normalized_options = Vec::new();
    for options in edge_candidates {
        let normalized = normalized_endpoint_options(options);
        normalized_options.push(normalized);
    }
    let mut parent = Vec::new();

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
                    group_points.push(point);
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
                components.entry(root).or_default().push(point);
            }
        }
        components
    };
    let mut coordinate_components = Vec::new();
    for points in components_by_root.into_values() {
        coordinate_components.push(points);
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
            option_records.push((edge, pair));
            indices.push(option);
            for point in pair {
                let Some(neighbors) = option_neighbors.get_mut(point) else {
                    return identity();
                };
                neighbors.push(option);
            }
        }
        option_indices_by_edge.push(indices);
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
    let refinement_limit = point_count + edge_rows.len() + option_records.len() + 1;
    for _ in 0..refinement_limit {
        let mut option_signatures = Vec::new();

        for (option, (edge, [left, right])) in option_records.iter().enumerate() {
            let mut endpoints = [point_colors[*left], point_colors[*right]];
            endpoints.sort_unstable_by(Ord::cmp);
            option_signatures.push((option_colors[option], row_colors[*edge], endpoints));
        }
        let next_option_colors = intern_gauge_signatures(ctx, option_signatures)?;
        let mut row_signatures = Vec::new();
        for edge in 0..edge_rows.len() {
            let mut options = Vec::new();
            for &option in &option_indices_by_edge[edge] {
                options.push(next_option_colors[option]);
            }
            options.sort_unstable_by(Ord::cmp);
            row_signatures.push((
                row_colors[edge],
                edge_bases[edge],
                edge_identity_evidence[edge],
                edge_identity_evidence[edge].then_some(edge),
                options,
            ));
        }
        let next_row_colors = intern_gauge_signatures(ctx, row_signatures)?;
        let mut point_signatures = Vec::new();
        for point in 0..point_count {
            let mut options = Vec::new();
            for &option in &option_neighbors[point] {
                options.push(next_option_colors[option]);
            }
            options.sort_unstable_by(Ord::cmp);
            point_signatures.push((point_colors[point], options));
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

    let is_automorphism =
        |affected_groups: &[&Vec<usize>], permutation: &[usize]| -> Result<bool, CodecError> {
            for edges in affected_groups {
                let mut original_unbound = Vec::new();
                let mut mapped_unbound = Vec::new();
                for &edge in *edges {
                    if edge_identity_evidence[edge] {
                        continue;
                    }
                    let original = normalized_options[edge].clone();
                    original_unbound.push(original);
                    if let Some(mapped) =
                        mapped_normalized_endpoint_options(&edge_candidates[edge], permutation)
                    {
                        mapped_unbound.push(mapped);
                    }
                }
                original_unbound.sort_unstable_by(Ord::cmp);
                mapped_unbound.sort_unstable_by(Ord::cmp);
                if original_unbound != mapped_unbound {
                    return Ok(false);
                }
                for &edge in edges.iter().filter(|edge| edge_identity_evidence[**edge]) {
                    let mapped =
                        mapped_normalized_endpoint_options(&edge_candidates[edge], permutation);
                    let original = normalized_options[edge].clone();
                    if mapped != Some(original) {
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
            let mut affected = false;
            for &edge in edges {
                if edge_candidates[edge].iter().flatten().any(|point| {
                    component_by_point.get(*point).copied().flatten() == Some(component)
                }) {
                    affected = true;
                    break;
                }
            }
            if affected {
                affected_groups.push(edges);
            }
        }
        let mut color_classes = BTreeMap::<usize, Vec<usize>>::new();
        for &point in &points {
            let color = point_colors[point];
            color_classes.entry(color).or_default().push(point);
        }
        let mut identity_order = Vec::new();

        identity_order.extend(0..point_count);
        let mut local_orders = vec![identity_order];
        let mut bounded = true;
        for class in color_classes.values() {
            if class.len() == 1 {
                continue;
            }
            let remaining_limit = MAX_COORDINATE_GAUGE_PERMUTATIONS / local_orders.len();
            // A class with more orders than the permutation limit leaves this
            // component with the identity gauge only.
            if bounded_factorial(class.len(), remaining_limit).is_none() {
                bounded = false;
                break;
            }
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

            let mut next = Vec::new();

            for permutation in &local_orders {
                for order in &class_orders {
                    let mut permutation = permutation.clone();

                    for (&source, &target) in class.iter().zip(order) {
                        permutation[source] = target;
                    }
                    next.push(permutation);
                }
            }
            local_orders = next;
        }
        if !bounded {
            let mut identity_order = Vec::new();

            identity_order.extend(0..point_count);
            local_orders.clear();
            local_orders.push(identity_order);
        }
        let mut permutations = Vec::new();
        for permutation in local_orders {
            if is_automorphism(&affected_groups, &permutation)? {
                permutations.push(permutation);
            }
        }
        if permutations.is_empty() {
            let mut identity = Vec::new();

            identity.extend(0..point_count);
            permutations.push(identity);
        }
        permutations.sort_unstable_by(Ord::cmp);

        permutations.dedup();
        components.push(permutations);
    }
    Ok(MeshCoordinateGauge { components })
}

fn mapped_endpoint_pair(
    pair: Option<[usize; 2]>,
    permutation: Option<&[usize]>,
) -> Option<[usize; 2]> {
    let mut pair = pair?;
    if let Some(permutation) = permutation {
        let (Some(&left), Some(&right)) = (permutation.get(pair[0]), permutation.get(pair[1]))
        else {
            return None;
        };
        pair = [left, right];
    }
    pair.sort_unstable_by(Ord::cmp);
    Some(pair)
}

fn canonicalize_partial_endpoint_pair_gauge_with_permutation(
    pairs: &[Option<[usize; 2]>],
    gauge: MeshCandidateGauge<'_>,
    permutation: Option<&[usize]>,
) -> Option<Vec<Option<[usize; 2]>>> {
    let edge_count = pairs.len();
    let mut canonical = Vec::new();
    for pair in pairs.iter().copied() {
        let mapped = match pair {
            Some(pair) => {
                let mapped = mapped_endpoint_pair(Some(pair), permutation)?;
                Some(mapped)
            }
            None => None,
        };
        canonical.push(mapped);
    }
    if gauge.edge_rows.len() != edge_count
        || gauge.edge_faces.len() != edge_count
        || gauge.edge_geometry.len() != edge_count
        || gauge.edge_candidates.len() != edge_count
        || gauge.edge_identity_evidence.len() != edge_count
    {
        return Some(canonical);
    }

    let mut source_groups = BTreeMap::<MeshEdgeGaugeKey, Vec<usize>>::new();
    let mut target_groups = BTreeMap::<MeshEdgeGaugeKey, Vec<usize>>::new();
    for edge in 0..edge_count {
        if gauge.edge_identity_evidence[edge] {
            continue;
        }
        let base =
            mesh_edge_gauge_base_key(edge, gauge.edge_rows, gauge.edge_faces, gauge.edge_geometry)?;
        let source_options = normalized_endpoint_options(&gauge.edge_candidates[edge]);
        let target_options = match permutation {
            Some(permutation) => {
                mapped_normalized_endpoint_options(&gauge.edge_candidates[edge], permutation)?
            }
            None => source_options.clone(),
        };
        let source_key = (base, target_options);
        source_groups.entry(source_key).or_default().push(edge);
        let target_key = (base, source_options);
        target_groups.entry(target_key).or_default().push(edge);
    }

    for (key, group) in source_groups {
        let mut slots = target_groups.remove(&key)?;
        if slots.len() != group.len() {
            return None;
        }
        slots.sort_unstable_by(Ord::cmp);
        let mut ordered = Vec::new();
        for &edge in &group {
            ordered.push((canonical[edge], edge));
        }
        ordered.sort_unstable_by(Ord::cmp);
        for (slot, (pair, _)) in slots.into_iter().zip(ordered) {
            canonical[slot] = pair;
        }
    }
    if !target_groups.is_empty() {
        return None;
    }
    Some(canonical)
}

fn canonicalize_partial_endpoint_pair_gauge(
    pairs: &[Option<[usize; 2]>],
    gauge: MeshCandidateGauge<'_>,
) -> Option<Vec<Option<[usize; 2]>>> {
    let mut canonical =
        canonicalize_partial_endpoint_pair_gauge_with_permutation(pairs, gauge, None)?;
    if let Some(coordinate_gauge) = gauge.coordinate_gauge {
        for permutations in &coordinate_gauge.components {
            let mut best = canonical.clone();
            for permutation in permutations {
                let candidate = canonicalize_partial_endpoint_pair_gauge_with_permutation(
                    &canonical,
                    gauge,
                    Some(permutation),
                )?;
                if candidate < best {
                    best = candidate;
                }
            }
            canonical = best;
        }
    }
    Some(canonical)
}

pub(super) fn canonicalize_complete_endpoint_pairs(
    pairs: &[[usize; 2]],
    gauge: MeshCandidateGauge<'_>,
) -> Option<Vec<[usize; 2]>> {
    let mut partial = Vec::new();
    for &pair in pairs {
        partial.push(Some(pair));
    }
    let canonical = canonicalize_partial_endpoint_pair_gauge(&partial, gauge)?;
    let mut complete = Vec::new();
    for pair in canonical {
        let pair = pair?;
        complete.push(pair);
    }
    Some(complete)
}

fn canonicalize_mesh_edge_row_gauges(
    ctx: &DecodeContext<'_>,
    mut topology: StandardTopologyDraft,
    gauge: MeshCandidateGauge<'_>,
    permutation: Option<&[usize]>,
) -> Result<Option<StandardTopologyDraft>, CodecError> {
    (|| -> Option<Result<StandardTopologyDraft, CodecError>> {
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
                        faces.push(face);
                    }
                    (usage.get_mut(coedge.edge_row)?).push((
                        face,
                        boundary,
                        position,
                        coedge.reversed,
                        coedge.start_vertex,
                        coedge.end_vertex,
                    ));
                }
            }
        }
        for faces in &mut incident_faces {
            faces.sort_unstable_by(Ord::cmp);
        }
        for uses in &mut usage {
            uses.sort_unstable_by(Ord::cmp);
        }

        if gauge.edge_faces.len() == edge_count {
            for (edge, actual_faces) in incident_faces.iter().enumerate() {
                let mut expected_faces = *gauge.edge_faces.get(edge)?;
                expected_faces.sort_unstable_by(Ord::cmp);
                if *actual_faces != expected_faces {
                    return None;
                }
            }
        }

        let mut endpoint_keys = Vec::new();
        for mut pair in edge_vertices.iter().copied() {
            pair.sort_unstable_by(Ord::cmp);
            endpoint_keys.push(pair);
        }
        let mut source_option_keys = Vec::new();
        for options in gauge.edge_candidates {
            let key = normalized_endpoint_options(options);
            source_option_keys.push(key);
        }
        let mut source_groups = BTreeMap::<MeshTopologyEdgeGaugeKey, Vec<usize>>::new();
        let mut target_groups = BTreeMap::<MeshTopologyEdgeGaugeKey, Vec<usize>>::new();
        for edge in 0..edge_count {
            if gauge.edge_identity_evidence[edge] {
                continue;
            }
            let row = &topology.edge_rows[edge];
            let base = (
                row.kind(),
                row.boundary_layout(),
                gauge.edge_geometry[edge],
                row.handles().len(),
                incident_faces[edge].clone(),
            );
            let target_options = match permutation {
                Some(permutation) => {
                    mapped_normalized_endpoint_options(&gauge.edge_candidates[edge], permutation)?
                }
                None => source_option_keys[edge].clone(),
            };
            let base_copy = (base.0, base.1, base.2, base.3, base.4.clone());
            let source_key = (base_copy, target_options);
            source_groups.entry(source_key).or_default().push(edge);
            let source_options = source_option_keys[edge].clone();
            let target_key = (base, source_options);
            target_groups.entry(target_key).or_default().push(edge);
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
            slots.sort_unstable_by(Ord::cmp);
            let mut ordered = group.clone();
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

        let mut permuting = row_permutation.clone();
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
                row.normalize_handles();
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
        canonicalize_topology_boundary_gauges(&mut topology);
        Some(Ok(topology))
    })()
    .transpose()
}

fn permute_mesh_coordinate_labels(
    ctx: &DecodeContext<'_>,
    mut topology: StandardTopologyDraft,
    permutation: &[usize],
) -> Result<Option<StandardTopologyDraft>, CodecError> {
    (|| -> Option<Result<StandardTopologyDraft, CodecError>> {
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

fn mesh_topology_gauge_key(topology: &StandardTopologyDraft) -> Vec<u64> {
    let mut key = Vec::new();
    key.push(u64_from_index(topology.vertex_points.len()));
    for point in &topology.vertex_points {
        for coordinate in point {
            key.push(coordinate.to_bits());
        }
    }
    key.push(u64_from_index(topology.logical_vertex_count));
    key.push(u64_from_index(topology.edge_rows.len()));
    for row in &topology.edge_rows {
        key.push(u64::from(row.kind()));
        key.push(u64::from(row.boundary_layout()));
        key.push(u64_from_index(row.handles().len()));
        for &handle in row.handles() {
            key.push(u64::from(handle));
        }
    }
    key.push(u64_from_index(topology.faces.len()));
    for face in &topology.faces {
        key.push(u64_from_index(face.boundaries.len()));
        for boundary in &face.boundaries {
            key.push(u64_from_index(boundary.coedges.len()));
            for coedge in &boundary.coedges {
                key.push(u64_from_index(coedge.edge_row));
                key.push(u64::from(coedge.reversed));
                key.push(u64_from_index(coedge.start_vertex));
                key.push(u64_from_index(coedge.end_vertex));
            }
        }
    }
    key
}

fn canonicalize_mesh_coordinate_gauges(
    ctx: &DecodeContext<'_>,
    mut topology: StandardTopologyDraft,
    gauge: MeshCandidateGauge<'_>,
) -> Result<Option<StandardTopologyDraft>, CodecError> {
    let Some(coordinate_gauge) = gauge.coordinate_gauge else {
        return canonicalize_mesh_edge_row_gauges(ctx, topology, gauge, None);
    };
    if coordinate_gauge.components.is_empty() {
        return canonicalize_mesh_edge_row_gauges(ctx, topology, gauge, None);
    }
    for permutations in &coordinate_gauge.components {
        let mut best = None::<(Vec<u64>, StandardTopologyDraft)>;
        for permutation in permutations {
            let Some(mut candidate) =
                permute_mesh_coordinate_labels(ctx, topology.clone(), permutation)?
            else {
                return Ok(None);
            };
            canonicalize_topology_boundary_gauges(&mut candidate);
            let Some(canonical) =
                canonicalize_mesh_edge_row_gauges(ctx, candidate, gauge, Some(permutation))?
            else {
                return Ok(None);
            };
            candidate = canonical;
            let key = mesh_topology_gauge_key(&candidate);
            let improves = if let Some((best_key, _)) = &best {
                key < *best_key
            } else {
                true
            };
            if improves {
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
    source_topology: &StandardTopologyDraft,
    point_assignment: &[usize],
    gauge: Option<MeshCandidateGauge<'_>>,
) -> Result<Option<(StandardTopologyDraft, Vec<usize>)>, CodecError> {
    (|| -> Option<Result<(StandardTopologyDraft, Vec<usize>), CodecError>> {
        let mut topology = source_topology.clone();
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
            reverse_edges.push(vertices.is_some_and(|vertices| vertices[0] > vertices[1]));
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
        canonicalize_topology_boundary_gauges(&mut topology);
        if let Some(gauge) = gauge {
            topology = match canonicalize_mesh_coordinate_gauges(ctx, topology, gauge) {
                Ok(Some(topology)) => topology,
                Ok(None) => return None,
                Err(error) => return Some(Err(error)),
            };
        }
        let mut identity = Vec::new();
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
    topology: &StandardTopologyDraft,
    point_assignment: &[usize],
    gauge: Option<MeshCandidateGauge<'_>>,
) -> Result<Option<(StandardTopologyDraft, Vec<usize>)>, CodecError> {
    canonicalize_mesh_candidate(ctx, topology, point_assignment, gauge)
}

#[cfg(test)]
pub(crate) fn canonicalize_mesh_vertex_labels(
    ctx: &DecodeContext<'_>,
    topology: &StandardTopologyDraft,
    point_assignment: &[usize],
) -> Result<Option<(StandardTopologyDraft, Vec<usize>)>, CodecError> {
    canonicalize_mesh_candidate(ctx, topology, point_assignment, None)
}

#[cfg(test)]
pub(crate) fn mesh_candidates_equivalent_with_gauge(
    ctx: &DecodeContext<'_>,
    left: &(StandardTopologyDraft, Vec<usize>),
    right: &(StandardTopologyDraft, Vec<usize>),
    edge_geometry: &[MeshEdgeGeometry],
    edge_candidates: &[Vec<[usize; 2]>],
    edge_identity_evidence: &[bool],
) -> Result<bool, CodecError> {
    mesh_candidates_equivalent(
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

pub(crate) fn mesh_candidates_equivalent(
    ctx: &DecodeContext<'_>,
    left: &(StandardTopologyDraft, Vec<usize>),
    right: &(StandardTopologyDraft, Vec<usize>),
    gauge: Option<MeshCandidateGauge<'_>>,
) -> Result<bool, CodecError> {
    if left.0.vertex_points != right.0.vertex_points {
        return Ok(false);
    }
    let left = canonicalize_mesh_candidate(ctx, &left.0, &left.1, gauge)?;
    let right = canonicalize_mesh_candidate(ctx, &right.0, &right.1, gauge)?;
    let (Some(left), Some(right)) = (&left, &right) else {
        return Ok(false);
    };
    Ok(left == right)
}

#[test]
fn endpoint_pair_gauge_canonicalization_snapshots_row_values() {
    let edge_rows = [
        {
            assert!(EdgeRow::new(1, Vec::new(), EdgeBoundaryLayout::CompleteBoundaryRun).is_none());
            EdgeRow::new(1, vec![0, 0], EdgeBoundaryLayout::CompleteBoundaryRun)
                .expect("admitted edge row")
        },
        {
            assert!(EdgeRow::new(1, Vec::new(), EdgeBoundaryLayout::CompleteBoundaryRun).is_none());
            EdgeRow::new(1, vec![0, 0], EdgeBoundaryLayout::CompleteBoundaryRun)
                .expect("admitted edge row")
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
        canonicalize_partial_endpoint_pair_gauge(&left, gauge)
            .expect("canonicalize partial endpoint pair gauge"),
        canonicalize_partial_endpoint_pair_gauge(&right, gauge)
            .expect("canonicalize partial endpoint pair gauge"),
    );
}

#[test]
fn mesh_candidate_comparison_collapses_coordinate_row_gauge() {
    catia_test_context!(ctx);
    let edge_rows = vec![
        {
            assert!(EdgeRow::new(1, vec![10], EdgeBoundaryLayout::CompleteBoundaryRun).is_none());
            EdgeRow::new(1, vec![10, 10], EdgeBoundaryLayout::CompleteBoundaryRun)
                .expect("admitted edge row")
        },
        {
            assert!(EdgeRow::new(1, vec![11], EdgeBoundaryLayout::CompleteBoundaryRun).is_none());
            EdgeRow::new(1, vec![11, 11], EdgeBoundaryLayout::CompleteBoundaryRun)
                .expect("admitted edge row")
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
    let topology = |swapped: bool| StandardTopologyDraft {
        faces: vec![
            crate::families::standard::topology::FaceTopologyDraft {
                boundaries: vec![
                    crate::families::standard::topology::BoundaryDraft::new(vec![
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
                    .expect("nonempty topology boundary"),
                ],
            },
            crate::families::standard::topology::FaceTopologyDraft {
                boundaries: vec![
                    crate::families::standard::topology::BoundaryDraft::new(vec![
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
                    .expect("nonempty topology boundary"),
                ],
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

    assert!(!mesh_candidates_equivalent(&ctx, &left, &right, None)
        .expect("check that the mesh candidates are equivalent"));
    assert!(mesh_candidates_equivalent(&ctx, &left, &right, Some(gauge))
        .expect("check that the mesh candidates are equivalent"));

    let mut mismatched_topology = topology(false);
    let boundary = &mut mismatched_topology.faces[1].boundaries[0];
    boundary.coedges = NonEmptyMembers::<CoedgeUse>::one(boundary.coedges[0]);
    let mismatched = (mismatched_topology, vec![0, 1, 2, 3]);
    assert!(
        !mesh_candidates_equivalent(&ctx, &left, &mismatched, Some(gauge))
            .expect("check that the mesh candidates are equivalent")
    );
}

#[test]
fn mesh_candidate_comparison_collapses_independent_seam_row_coordinate_automorphisms() {
    const COMPONENT_COUNT: usize = 3;
    catia_test_context!(ctx);
    let edge_rows = (0..COMPONENT_COUNT * 2)
        .map(|edge| {
            EdgeRow::new(
                2,
                vec![
                    u32::try_from(edge * 2).expect("fixture value fits u32"),
                    u32::try_from(edge * 2 + 1).expect("fixture value fits u32"),
                ],
                EdgeBoundaryLayout::CompleteBoundaryRun,
            )
            .expect("admitted edge row")
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
    .expect("build mesh coordinate gauge");
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
        let face = || crate::families::standard::topology::FaceTopologyDraft {
            boundaries: vec![crate::families::standard::topology::BoundaryDraft::new(
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
        StandardTopologyDraft {
            faces: vec![face(), face()],
            edge_rows: edge_rows.clone(),
            vertex_points: (0..COMPONENT_COUNT * 4)
                .map(|point| {
                    [
                        cadmpeg_core::convert::f64_from_index(point)
                            .expect("fixture index is exactly representable"),
                        0.0,
                        0.0,
                    ]
                })
                .collect(),
            logical_vertex_count: COMPONENT_COUNT * 4,
        }
    };
    let identity = (0..COMPONENT_COUNT * 4).collect::<Vec<_>>();
    let left = (topology(0), identity.clone());
    let canonical_left =
        canonicalize_mesh_candidate_for_output(&ctx, &left.0, &left.1, Some(gauge))
            .expect("canonicalize mesh candidate for output")
            .expect("seam candidate canonicalization");

    for mask in 1..(1 << COMPONENT_COUNT) {
        let right = (topology(mask), identity.clone());
        assert!(!mesh_candidates_equivalent(&ctx, &left, &right, None)
            .expect("check that the mesh candidates are equivalent"));
        assert!(
            mesh_candidates_equivalent(&ctx, &left, &right, Some(gauge))
                .expect("check that the mesh candidates are equivalent"),
            "seam gauge did not collapse mask {mask:#b}"
        );
        assert_eq!(
            canonical_left,
            canonicalize_mesh_candidate_for_output(&ctx, &right.0, &right.1, Some(gauge))
                .expect("canonicalize mesh candidate for output")
                .expect("seam candidate canonicalization"),
            "seam gauge representative depends on search arrival order for mask {mask:#b}"
        );
    }

    let mut displaced = topology(0);
    displaced.vertex_points[0][0] = -1.0;
    assert!(
        !mesh_candidates_equivalent(&ctx, &left, &(displaced, identity), Some(gauge))
            .expect("check that the mesh candidates are equivalent")
    );

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
    .expect("build mesh coordinate gauge");
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
    assert!(!mesh_candidates_equivalent(
        &ctx,
        &left,
        &(row_swapped, (0..COMPONENT_COUNT * 4).collect()),
        Some(mismatched_gauge),
    )
    .expect("check that the mesh candidates are equivalent"));
}

pub(super) fn canonicalize_endpoint_relation_state(
    ctx: &DecodeContext<'_>,
    domains: &[Vec<MeshEndpointRelationChoice>],
    assigned: &[Option<[usize; 2]>],
    gauge: MeshCandidateGauge<'_>,
) -> Result<Option<MeshEndpointRelationStateSignature>, CodecError> {
    let mut state = raw_endpoint_relation_state_signature(domains, assigned);

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
        point
            .checked_add(1)
            .ok_or_else(|| CodecError::malformed("catia endpoint relation point index overflows"))?
    } else {
        0
    };
    let mut identity = Vec::new();

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
            let mut best = state.clone();
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

fn map_endpoint_relation_state(
    ctx: &DecodeContext<'_>,
    state: &MeshEndpointRelationStateSignature,
    gauge: MeshCandidateGauge<'_>,
    permutation: &[usize],
) -> Result<Option<MeshEndpointRelationStateSignature>, CodecError> {
    let Some(row_mapping) = relation_row_gauge_mapping(state, gauge, permutation)? else {
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
                        mapped_pairs.push((target, pair));
                    }
                    mapped_pairs.sort_unstable_by(Ord::cmp);
                    MeshEndpointRelationSelection::Enumerated {
                        assignments: assignments.clone(),
                        edge_pairs: mapped_pairs,
                    }
                }
            };
            choices.push(mapped);
        }

        choices.sort_unstable_by(Ord::cmp);
        domains.push(choices);
    }
    Ok(Some((assigned, domains)))
}

fn relation_row_gauge_mapping(
    state: &MeshEndpointRelationStateSignature,
    gauge: MeshCandidateGauge<'_>,
    permutation: &[usize],
) -> Result<Option<Vec<usize>>, CodecError> {
    let edge_count = state.0.len();
    let identity = || -> Result<Vec<usize>, CodecError> {
        let mut rows = Vec::new();

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
        let source_options = normalized_endpoint_options(&gauge.edge_candidates[edge]);
        let Some(target_options) =
            mapped_normalized_endpoint_options(&gauge.edge_candidates[edge], permutation)
        else {
            return Ok(None);
        };
        let source_key = (base, target_options);
        source_groups.entry(source_key).or_default().push(edge);
        let target_key = (base, source_options);
        target_groups.entry(target_key).or_default().push(edge);
    }

    let mut row_mapping = identity()?;
    for (key, mut source) in source_groups {
        let Some(mut targets) = target_groups.remove(&key) else {
            return Ok(None);
        };
        source.sort_unstable_by(Ord::cmp);
        targets.sort_unstable_by(Ord::cmp);
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
            let Some(signature) = relation_row_signature(state, edge, permutation) else {
                return Ok(None);
            };
            ordered.push((signature, edge));
        }
        ordered.sort_unstable_by(Ord::cmp);
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
    state: &MeshEndpointRelationStateSignature,
    edge: usize,
    permutation: &[usize],
) -> Option<Vec<Option<[usize; 2]>>> {
    let mut signature = Vec::new();
    let &edge_pair = state.0.get(edge)?;
    let assigned = match edge_pair {
        Some(pair) => {
            let mapped = mapped_endpoint_pair(Some(pair), Some(permutation))?;
            Some(mapped)
        }
        None => None,
    };
    signature.push(assigned);
    for choices in &state.1 {
        for selection in choices {
            let pairs = selection.edge_pairs();
            let mut value = None;
            for &(candidate, pair) in pairs {
                if candidate != edge {
                    continue;
                }
                if value.is_some() {
                    return None;
                }
                let mapped = mapped_endpoint_pair(Some(pair), Some(permutation))?;
                value = Some(mapped);
            }
            signature.push(value);
        }
    }
    Some(signature)
}

#[test]
fn relation_state_memo_collapses_coordinate_gauge() {
    catia_test_context!(ctx);
    let edge_rows = [
        {
            assert!(EdgeRow::new(1, vec![10], EdgeBoundaryLayout::CompleteBoundaryRun).is_none());
            EdgeRow::new(1, vec![10, 10], EdgeBoundaryLayout::CompleteBoundaryRun)
                .expect("admitted edge row")
        },
        {
            assert!(EdgeRow::new(1, vec![11], EdgeBoundaryLayout::CompleteBoundaryRun).is_none());
            EdgeRow::new(1, vec![11, 11], EdgeBoundaryLayout::CompleteBoundaryRun)
                .expect("admitted edge row")
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
        .expect("canonicalize endpoint relation state")
        .expect("coordinate gauge state")
    };

    assert_eq!(state(false), state(true));
}

#[test]
fn relation_state_memo_uses_coordinate_gauge_domain_alternatives() {
    catia_test_context!(ctx);
    let edge_rows = [
        {
            assert!(EdgeRow::new(1, vec![10], EdgeBoundaryLayout::CompleteBoundaryRun).is_none());
            EdgeRow::new(1, vec![10, 10], EdgeBoundaryLayout::CompleteBoundaryRun)
                .expect("admitted edge row")
        },
        {
            assert!(EdgeRow::new(1, vec![11], EdgeBoundaryLayout::CompleteBoundaryRun).is_none());
            EdgeRow::new(1, vec![11, 11], EdgeBoundaryLayout::CompleteBoundaryRun)
                .expect("admitted edge row")
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
            .expect("canonicalize endpoint relation state")
            .expect("coordinate gauge state")
    };

    assert_eq!(state(false), state(true));
}

#[test]
fn relation_state_memo_applies_one_row_mapping_to_assigned_and_domains() {
    catia_test_context!(ctx);
    let edge_rows = [
        EdgeRow::new(2, vec![10, 11], EdgeBoundaryLayout::CompleteBoundaryRun)
            .expect("admitted edge row"),
        EdgeRow::new(2, vec![20, 21], EdgeBoundaryLayout::CompleteBoundaryRun)
            .expect("admitted edge row"),
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
    .expect("build mesh coordinate gauge");
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
            .expect("canonicalize endpoint relation state")
            .expect("row-coordinate gauge state")
    };

    let left = state(vec![Some([0, 3]), None], vec![(0, [1, 2]), (1, [0, 3])]);
    let right = state(vec![None, Some([0, 3])], vec![(0, [0, 3]), (1, [1, 2])]);

    assert_eq!(left, right);
}

#[cfg(test)]
mod work_tests;

#[test]
fn single_coedge_boundary_keeps_its_coedge() {
    let topology = || StandardTopologyDraft {
        faces: vec![crate::families::standard::topology::FaceTopologyDraft {
            boundaries: vec![crate::families::standard::topology::BoundaryDraft {
                coedges: NonEmptyMembers::<CoedgeUse>::one(CoedgeUse {
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
    let mut candidate = topology();
    canonicalize_topology_boundary_gauges(&mut candidate);
    assert_eq!(candidate.faces[0].boundaries[0].coedges.len(), 1);
}

#[test]
fn edge_row_gauge_canonicalizes_a_two_edge_loop() {
    use crate::families::standard::topology::{BoundaryDraft, FaceTopologyDraft};

    let topology = StandardTopologyDraft {
        faces: vec![FaceTopologyDraft {
            boundaries: vec![BoundaryDraft {
                coedges: NonEmptyMembers::<CoedgeUse>::try_from(vec![
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
            EdgeRow::new(1, vec![0, 1], EdgeBoundaryLayout::CompleteBoundaryRun)
                .expect("admitted edge row"),
            EdgeRow::new(1, vec![1, 0], EdgeBoundaryLayout::CompleteBoundaryRun)
                .expect("admitted edge row"),
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
    .expect("canonicalize mesh edge row gauges")
    .is_some());
}

#[test]
fn single_edge_complete_endpoint_pair_canonicalizes_unchanged() {
    let rows = [{
        assert!(EdgeRow::new(1, Vec::new(), EdgeBoundaryLayout::CompleteBoundaryRun).is_none());
        EdgeRow::new(1, vec![0, 0], EdgeBoundaryLayout::CompleteBoundaryRun)
            .expect("admitted edge row")
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
    let run = || canonicalize_complete_endpoint_pairs(&[[0, 1]], gauge);
    assert_eq!(run(), Some(vec![[0, 1]]));
}

#[test]
fn endpoint_relation_state_canonicalizes_under_a_swap_gauge() {
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
        .expect("canonicalize endpoint relation state")
        .is_some());
}

#[test]
fn coordinate_gauge_has_components_for_two_parallel_lines() {
    let edge_rows = [
        EdgeRow::new(2, vec![10, 11], EdgeBoundaryLayout::CompleteBoundaryRun)
            .expect("admitted edge row"),
        EdgeRow::new(2, vec![20, 21], EdgeBoundaryLayout::CompleteBoundaryRun)
            .expect("admitted edge row"),
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
    assert!(!crate::test_support::with_service_context(run)
        .expect("build mesh coordinate gauge")
        .components
        .is_empty());
}

#[test]
fn mesh_candidate_with_coordinate_gauge_is_equivalent_to_itself() {
    let edge_rows = vec![{
        assert!(EdgeRow::new(1, vec![10], EdgeBoundaryLayout::CompleteBoundaryRun).is_none());
        EdgeRow::new(1, vec![10, 10], EdgeBoundaryLayout::CompleteBoundaryRun)
            .expect("admitted edge row")
    }];
    let topology = StandardTopologyDraft {
        faces: vec![crate::families::standard::topology::FaceTopologyDraft {
            boundaries: vec![
                crate::families::standard::topology::BoundaryDraft::new(vec![CoedgeUse {
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
        mesh_candidates_equivalent(ctx, &candidate, &candidate, Some(gauge))
    };
    catia_test_context!(service_ctx);
    assert!(crate::test_support::with_service_context(run)
        .expect("check that the mesh candidates are equivalent"));
    assert!(canonicalize_mesh_candidate_for_output(
        &service_ctx,
        &candidate.0,
        &candidate.1,
        Some(gauge),
    )
    .expect("canonicalize mesh candidate for output")
    .is_some());
}
