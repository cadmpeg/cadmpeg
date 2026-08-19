//! Evidence-preserving gauge quotient for standard mesh candidates.

use std::collections::BTreeMap;

use cadmpeg_core::decode::alloc_filled;

use super::mesh_quotient::{MeshEndpointRelationChoice, MeshEndpointRelationStateSignature};
use crate::families::standard::topology::{
    CoedgeUse, EdgeBoundaryLayout, EdgeRow, StandardTopology,
};

type MeshEdgeGaugeBaseKey = (u8, EdgeBoundaryLayout, usize, usize, [usize; 2]);

type MeshEdgeGaugeKey = (MeshEdgeGaugeBaseKey, Vec<[usize; 2]>);
type MeshTopologyEdgeGaugeBaseKey = (u8, EdgeBoundaryLayout, usize, usize, Vec<usize>);
type MeshTopologyEdgeGaugeKey = (MeshTopologyEdgeGaugeBaseKey, Vec<[usize; 2]>);

pub(crate) struct MeshCoordinateGauge {
    components: Vec<Vec<Vec<usize>>>,
}

#[derive(Clone, Copy)]
pub(crate) struct MeshCandidateGauge<'a> {
    pub(crate) edge_rows: &'a [EdgeRow],
    pub(crate) edge_faces: &'a [[usize; 2]],
    pub(crate) edge_classes: &'a [usize],
    pub(crate) edge_candidates: &'a [Vec<[usize; 2]>],
    pub(crate) edge_identity_evidence: &'a [bool],
    pub(crate) coordinate_gauge: Option<&'a MeshCoordinateGauge>,
}

fn canonicalize_topology_boundary_gauges(topology: &mut StandardTopology) -> Option<()> {
    fn signature(coedges: &[CoedgeUse]) -> Vec<(usize, bool, usize, usize)> {
        coedges
            .iter()
            .map(|coedge| {
                (
                    coedge.edge_row,
                    coedge.reversed,
                    coedge.start_vertex,
                    coedge.end_vertex,
                )
            })
            .collect()
    }

    fn rotate_to_minimum(coedges: &mut [CoedgeUse]) -> Option<()> {
        let len = coedges.len();
        let best = (0..len).min_by_key(|&start| {
            (0..len)
                .map(|offset| {
                    let coedge = coedges[(start + offset) % len];
                    (
                        coedge.edge_row,
                        coedge.reversed,
                        coedge.start_vertex,
                        coedge.end_vertex,
                    )
                })
                .collect::<Vec<_>>()
        })?;
        coedges.rotate_left(best);
        Some(())
    }

    for face in &mut topology.faces {
        for boundary in &mut face.boundaries {
            rotate_to_minimum(&mut boundary.coedges)?;
            let mut reversed = boundary
                .coedges
                .iter()
                .rev()
                .copied()
                .map(|mut coedge| {
                    coedge.reversed = !coedge.reversed;
                    std::mem::swap(&mut coedge.start_vertex, &mut coedge.end_vertex);
                    coedge
                })
                .collect::<Vec<_>>();
            rotate_to_minimum(&mut reversed)?;
            if signature(&reversed) < signature(&boundary.coedges) {
                boundary.coedges = reversed;
            }
        }
        face.boundaries
            .sort_by_key(|boundary| signature(&boundary.coedges));
    }
    Some(())
}

fn normalized_endpoint_options(options: &[[usize; 2]]) -> Vec<[usize; 2]> {
    let mut options = options
        .iter()
        .copied()
        .map(|mut pair| {
            pair.sort_unstable();
            pair
        })
        .collect::<Vec<_>>();
    options.sort_unstable();
    options.dedup();
    options
}

fn mesh_edge_gauge_base_key(
    edge: usize,
    edge_rows: &[EdgeRow],
    edge_faces: &[[usize; 2]],
    edge_classes: &[usize],
) -> Option<MeshEdgeGaugeBaseKey> {
    let row = edge_rows.get(edge)?;
    let mut faces = *edge_faces.get(edge)?;
    faces.sort_unstable();
    Some((
        row.kind,
        row.boundary_layout,
        *edge_classes.get(edge)?,
        row.handles.len(),
        faces,
    ))
}

fn mapped_normalized_endpoint_options(
    options: &[[usize; 2]],
    permutation: &[usize],
) -> Option<Vec<[usize; 2]>> {
    let mapped = options
        .iter()
        .map(|pair| Some([*permutation.get(pair[0])?, *permutation.get(pair[1])?]))
        .collect::<Option<Vec<_>>>()?;
    Some(normalized_endpoint_options(&mapped))
}

fn coordinate_find(parent: &mut [usize], node: usize) -> usize {
    if parent[node] == node {
        return node;
    }
    let root = coordinate_find(parent, parent[node]);
    parent[node] = root;
    root
}

fn coordinate_union(parent: &mut [usize], left: usize, right: usize) {
    let left = coordinate_find(parent, left);
    let right = coordinate_find(parent, right);
    if left != right {
        parent[right] = left;
    }
}

fn enumerate_coordinate_permutations(
    points: &[usize],
    index: usize,
    current: &mut Vec<usize>,
    used: &mut [bool],
    output: &mut Vec<Vec<usize>>,
) {
    if index == points.len() {
        output.push(current.clone());
        return;
    }
    for target in 0..points.len() {
        if used[target] {
            continue;
        }
        used[target] = true;
        current.push(points[target]);
        enumerate_coordinate_permutations(points, index + 1, current, used, output);
        current.pop();
        used[target] = false;
    }
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

fn intern_gauge_signatures<T: Ord>(signatures: impl IntoIterator<Item = T>) -> Vec<usize> {
    let mut ids = BTreeMap::<T, usize>::new();
    signatures
        .into_iter()
        .map(|signature| {
            let next = ids.len();
            *ids.entry(signature).or_insert(next)
        })
        .collect()
}

pub(crate) fn build_mesh_coordinate_gauge(
    point_count: usize,
    edge_rows: &[EdgeRow],
    edge_faces: &[[usize; 2]],
    edge_classes: &[usize],
    edge_candidates: &[Vec<[usize; 2]>],
    edge_identity_evidence: &[bool],
) -> MeshCoordinateGauge {
    const MAX_COORDINATE_GAUGE_PERMUTATIONS: usize = 4_096;
    let identity = || MeshCoordinateGauge {
        components: Vec::new(),
    };
    if edge_rows.len() != edge_faces.len()
        || edge_rows.len() != edge_classes.len()
        || edge_rows.len() != edge_candidates.len()
        || edge_rows.len() != edge_identity_evidence.len()
    {
        return identity();
    }

    let mut edge_bases = Vec::with_capacity(edge_rows.len());
    let mut groups = BTreeMap::<MeshEdgeGaugeBaseKey, Vec<usize>>::new();
    for edge in 0..edge_rows.len() {
        let Some(key) = mesh_edge_gauge_base_key(edge, edge_rows, edge_faces, edge_classes) else {
            return identity();
        };
        edge_bases.push(key);
        groups.entry(key).or_default().push(edge);
    }
    let normalized_options = edge_candidates
        .iter()
        .map(|options| normalized_endpoint_options(options))
        .collect::<Vec<_>>();
    let mut parent = (0..point_count).collect::<Vec<_>>();
    let Ok(mut active) = alloc_filled(point_count, false, "catia_coordinate_gauge_active") else {
        return identity();
    };
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
                coordinate_union(&mut parent, first, point);
            }
        }
    }
    let components_by_root = {
        let mut components = BTreeMap::<usize, Vec<usize>>::new();
        for point in 0..point_count {
            if active.get(point).copied().unwrap_or(false) {
                let root = coordinate_find(&mut parent, point);
                components.entry(root).or_default().push(point);
            }
        }
        components
    };
    let coordinate_components = components_by_root.into_values().collect::<Vec<_>>();
    let Ok(mut component_by_point) = alloc_filled(
        point_count,
        None::<usize>,
        "catia_coordinate_gauge_components",
    ) else {
        return identity();
    };
    for (component, points) in coordinate_components.iter().enumerate() {
        for &point in points {
            let Some(slot) = component_by_point.get_mut(point) else {
                return identity();
            };
            *slot = Some(component);
        }
    }

    let mut option_records = Vec::<(usize, [usize; 2])>::new();
    let mut option_indices_by_edge = Vec::with_capacity(edge_rows.len());
    let mut option_neighbors = (0..point_count)
        .map(|_| Vec::<usize>::new())
        .collect::<Vec<_>>();
    for (edge, options) in normalized_options.iter().enumerate() {
        let mut indices = Vec::with_capacity(options.len());
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
    let mut point_colors = intern_gauge_signatures((0..point_count).map(|_| ()));
    let mut row_colors = intern_gauge_signatures((0..edge_rows.len()).map(|edge| {
        (
            edge_bases[edge],
            edge_identity_evidence[edge],
            edge_identity_evidence[edge].then_some(edge),
        )
    }));
    let mut option_colors =
        intern_gauge_signatures(option_records.iter().map(|(edge, _)| row_colors[*edge]));
    for _ in 0..point_count
        .saturating_add(edge_rows.len())
        .saturating_add(option_records.len())
        .saturating_add(1)
    {
        let next_option_colors = intern_gauge_signatures(option_records.iter().enumerate().map(
            |(option, (edge, [left, right]))| {
                let mut endpoints = [point_colors[*left], point_colors[*right]];
                endpoints.sort_unstable();
                (option_colors[option], row_colors[*edge], endpoints)
            },
        ));
        let next_row_colors = intern_gauge_signatures((0..edge_rows.len()).map(|edge| {
            let mut options = option_indices_by_edge[edge]
                .iter()
                .map(|option| next_option_colors[*option])
                .collect::<Vec<_>>();
            options.sort_unstable();
            (
                row_colors[edge],
                edge_bases[edge],
                edge_identity_evidence[edge],
                edge_identity_evidence[edge].then_some(edge),
                options,
            )
        }));
        let next_point_colors = intern_gauge_signatures((0..point_count).map(|point| {
            let mut options = option_neighbors[point]
                .iter()
                .map(|option| next_option_colors[*option])
                .collect::<Vec<_>>();
            options.sort_unstable();
            (point_colors[point], options)
        }));
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

    let is_automorphism = |affected_groups: &[&Vec<usize>], permutation: &[usize]| {
        for edges in affected_groups {
            let mut original_unbound = edges
                .iter()
                .filter(|edge| !edge_identity_evidence[**edge])
                .map(|edge| normalized_options[*edge].clone())
                .collect::<Vec<_>>();
            let mut mapped_unbound = edges
                .iter()
                .filter(|edge| !edge_identity_evidence[**edge])
                .filter_map(|edge| {
                    mapped_normalized_endpoint_options(&edge_candidates[*edge], permutation)
                })
                .collect::<Vec<_>>();
            original_unbound.sort_unstable();
            mapped_unbound.sort_unstable();
            if original_unbound != mapped_unbound {
                return false;
            }
            for &edge in edges.iter().filter(|edge| edge_identity_evidence[**edge]) {
                if mapped_normalized_endpoint_options(&edge_candidates[edge], permutation)
                    != Some(normalized_options[edge].clone())
                {
                    return false;
                }
            }
        }
        true
    };

    let mut components = Vec::new();
    for (component, points) in coordinate_components.into_iter().enumerate() {
        let affected_groups = groups
            .values()
            .filter(|edges| {
                edges.iter().any(|edge| {
                    edge_candidates[*edge].iter().flatten().any(|point| {
                        component_by_point.get(*point).copied().flatten() == Some(component)
                    })
                })
            })
            .collect::<Vec<_>>();
        let mut color_classes = BTreeMap::<usize, Vec<usize>>::new();
        for &point in &points {
            color_classes
                .entry(point_colors[point])
                .or_default()
                .push(point);
        }
        let mut local_orders = vec![(0..point_count).collect::<Vec<_>>()];
        let mut bounded = true;
        for class in color_classes.values() {
            let remaining_limit = MAX_COORDINATE_GAUGE_PERMUTATIONS / local_orders.len();
            let Some(class_order_count) = bounded_factorial(class.len(), remaining_limit) else {
                bounded = false;
                break;
            };
            let Ok(mut used) = alloc_filled(class.len(), false, "catia_coordinate_gauge_used")
            else {
                bounded = false;
                break;
            };
            let mut class_orders = Vec::with_capacity(class_order_count);
            enumerate_coordinate_permutations(
                class,
                0,
                &mut Vec::new(),
                &mut used,
                &mut class_orders,
            );
            let next_len = local_orders.len() * class_order_count;
            let mut next = Vec::with_capacity(next_len);
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
            local_orders.clear();
            local_orders.push((0..point_count).collect());
        }
        let mut permutations = local_orders
            .into_iter()
            .filter_map(|permutation| {
                is_automorphism(&affected_groups, &permutation).then_some(permutation)
            })
            .collect::<Vec<_>>();
        if permutations.is_empty() {
            permutations.push((0..point_count).collect());
        }
        permutations.sort_unstable();
        permutations.dedup();
        components.push(permutations);
    }
    MeshCoordinateGauge { components }
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
    pairs: &[Option<[usize; 2]>],
    gauge: MeshCandidateGauge<'_>,
    permutation: Option<&[usize]>,
) -> Option<Vec<Option<[usize; 2]>>> {
    let edge_count = pairs.len();
    let mut canonical = Vec::with_capacity(edge_count);
    for pair in pairs.iter().copied() {
        canonical.push(match pair {
            Some(pair) => Some(mapped_endpoint_pair(Some(pair), permutation)?),
            None => None,
        });
    }
    if gauge.edge_rows.len() != edge_count
        || gauge.edge_faces.len() != edge_count
        || gauge.edge_classes.len() != edge_count
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
            mesh_edge_gauge_base_key(edge, gauge.edge_rows, gauge.edge_faces, gauge.edge_classes)?;
        let source_options = normalized_endpoint_options(&gauge.edge_candidates[edge]);
        let target_options = match permutation {
            Some(permutation) => {
                mapped_normalized_endpoint_options(&gauge.edge_candidates[edge], permutation)?
            }
            None => source_options.clone(),
        };
        source_groups
            .entry((base, target_options))
            .or_default()
            .push(edge);
        target_groups
            .entry((base, source_options))
            .or_default()
            .push(edge);
    }

    for (key, group) in source_groups {
        let mut slots = target_groups.remove(&key)?;
        if slots.len() != group.len() {
            return None;
        }
        slots.sort_unstable();
        let mut ordered = group
            .iter()
            .copied()
            .map(|edge| (canonical[edge], edge))
            .collect::<Vec<_>>();
        ordered.sort_unstable();
        let ordered_pairs = ordered
            .into_iter()
            .map(|(pair, _)| pair)
            .collect::<Vec<_>>();
        for (slot, pair) in slots.into_iter().zip(ordered_pairs) {
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

pub(crate) fn canonicalize_complete_endpoint_pairs(
    pairs: &[[usize; 2]],
    gauge: MeshCandidateGauge<'_>,
) -> Option<Vec<[usize; 2]>> {
    let pairs = pairs.iter().copied().map(Some).collect::<Vec<_>>();
    canonicalize_partial_endpoint_pair_gauge(&pairs, gauge)?
        .into_iter()
        .collect()
}

fn canonicalize_mesh_edge_row_gauges(
    mut topology: StandardTopology,
    gauge: MeshCandidateGauge<'_>,
    permutation: Option<&[usize]>,
) -> Option<StandardTopology> {
    let edge_count = topology.edge_rows.len();
    if gauge.edge_classes.len() != edge_count
        || gauge.edge_candidates.len() != edge_count
        || gauge.edge_identity_evidence.len() != edge_count
    {
        return Some(topology);
    }

    let edge_vertices = topology.edge_vertices()?;
    let mut incident_faces = alloc_filled(
        edge_count,
        Vec::<usize>::new(),
        "catia_mesh_edge_gauge_faces",
    )
    .ok()?;
    let mut usage = alloc_filled(
        edge_count,
        Vec::<(usize, usize, usize, bool, usize, usize)>::new(),
        "catia_mesh_edge_gauge_usage",
    )
    .ok()?;
    for (face, face_topology) in topology.faces.iter().enumerate() {
        for (boundary, boundary_topology) in face_topology.boundaries.iter().enumerate() {
            for (position, coedge) in boundary_topology.coedges.iter().enumerate() {
                let faces = incident_faces.get_mut(coedge.edge_row)?;
                if !faces.contains(&face) {
                    faces.push(face);
                }
                usage.get_mut(coedge.edge_row)?.push((
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

    let endpoint_keys = edge_vertices
        .iter()
        .copied()
        .map(|mut pair| {
            pair.sort_unstable();
            pair
        })
        .collect::<Vec<_>>();
    let source_option_keys = gauge
        .edge_candidates
        .iter()
        .map(|options| normalized_endpoint_options(options))
        .collect::<Vec<_>>();
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
            gauge.edge_classes[edge],
            row.handles.len(),
            incident_faces[edge].clone(),
        );
        let target_options = match permutation {
            Some(permutation) => {
                mapped_normalized_endpoint_options(&gauge.edge_candidates[edge], permutation)?
            }
            None => source_option_keys[edge].clone(),
        };
        source_groups
            .entry((base.clone(), target_options))
            .or_default()
            .push(edge);
        target_groups
            .entry((base, source_option_keys[edge].clone()))
            .or_default()
            .push(edge);
    }

    let mut row_permutation = (0..edge_count).collect::<Vec<_>>();
    let mut normalize_rows =
        alloc_filled(edge_count, false, "catia_mesh_edge_gauge_normalize_rows").ok()?;
    for (key, group) in source_groups {
        let mut slots = target_groups.remove(&key)?;
        if slots.len() != group.len() {
            return None;
        }
        slots.sort_unstable();
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
        return Some(topology);
    }

    let old_rows = topology.edge_rows.clone();
    let mut new_rows = old_rows.clone();
    for (old_edge, &new_edge) in row_permutation.iter().enumerate() {
        new_rows[new_edge] = old_rows[old_edge].clone();
    }
    for (edge, row) in new_rows.iter_mut().enumerate() {
        if normalize_rows[edge] {
            row.handles = row.handles.iter().map(|_| 0).collect();
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
    canonicalize_topology_boundary_gauges(&mut topology)?;
    Some(topology)
}

fn permute_mesh_coordinate_labels(
    mut topology: StandardTopology,
    permutation: &[usize],
) -> Option<StandardTopology> {
    if permutation.len() != topology.vertex_points.len() {
        return None;
    }
    let mut seen =
        alloc_filled(permutation.len(), false, "catia_mesh_coordinate_gauge_seen").ok()?;
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
    Some(topology)
}

fn mesh_topology_gauge_key(topology: &StandardTopology) -> Vec<u64> {
    let mut key = Vec::new();
    key.push(topology.vertex_points.len() as u64);
    for point in &topology.vertex_points {
        key.extend(point.iter().map(|coordinate| coordinate.to_bits()));
    }
    key.push(topology.logical_vertex_count as u64);
    key.push(topology.edge_rows.len() as u64);
    for row in &topology.edge_rows {
        key.push(u64::from(row.kind));
        key.push(row.boundary_layout as u64);
        key.push(row.handles.len() as u64);
        key.extend(row.handles.iter().map(|handle| u64::from(*handle)));
    }
    key.push(topology.faces.len() as u64);
    for face in &topology.faces {
        key.push(face.boundaries.len() as u64);
        for boundary in &face.boundaries {
            key.push(boundary.coedges.len() as u64);
            for coedge in &boundary.coedges {
                key.push(coedge.edge_row as u64);
                key.push(u64::from(coedge.reversed));
                key.push(coedge.start_vertex as u64);
                key.push(coedge.end_vertex as u64);
            }
        }
    }
    key
}

fn canonicalize_mesh_coordinate_gauges(
    mut topology: StandardTopology,
    gauge: MeshCandidateGauge<'_>,
) -> Option<StandardTopology> {
    let Some(coordinate_gauge) = gauge.coordinate_gauge else {
        return canonicalize_mesh_edge_row_gauges(topology, gauge, None);
    };
    if coordinate_gauge.components.is_empty() {
        return canonicalize_mesh_edge_row_gauges(topology, gauge, None);
    }
    for permutations in &coordinate_gauge.components {
        let mut best = None::<(Vec<u64>, StandardTopology)>;
        for permutation in permutations {
            let mut candidate = permute_mesh_coordinate_labels(topology.clone(), permutation)?;
            canonicalize_topology_boundary_gauges(&mut candidate)?;
            candidate = canonicalize_mesh_edge_row_gauges(candidate, gauge, Some(permutation))?;
            let key = mesh_topology_gauge_key(&candidate);
            if best.as_ref().is_none_or(|(best_key, _)| key < *best_key) {
                best = Some((key, candidate));
            }
        }
        topology = best?.1;
    }
    Some(topology)
}

fn canonicalize_mesh_candidate(
    source_topology: &StandardTopology,
    point_assignment: &[usize],
    gauge: Option<MeshCandidateGauge<'_>>,
) -> Option<(StandardTopology, Vec<usize>)> {
    let mut topology = source_topology.clone();
    if point_assignment.len() != topology.logical_vertex_count
        || point_assignment.len() != topology.vertex_points.len()
    {
        return None;
    }
    let mut seen = alloc_filled(point_assignment.len(), false, "catia_mesh_vertex_seen").ok()?;
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
        alloc_filled(topology.edge_rows.len(), None, "catia_mesh_edge_vertices").ok()?;
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
    let reverse_edges = edge_vertices
        .into_iter()
        .map(|vertices| vertices.is_some_and(|vertices| vertices[0] > vertices[1]))
        .collect::<Vec<_>>();
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
    canonicalize_topology_boundary_gauges(&mut topology)?;
    if let Some(gauge) = gauge {
        topology = canonicalize_mesh_coordinate_gauges(topology, gauge)?;
    }
    Some((topology, (0..point_assignment.len()).collect::<Vec<_>>()))
}

/// Materialize the canonical representative of one admitted topology orbit.
///
/// Returns `None` when the candidate does not satisfy the canonicalizer's
/// structural invariants or its bounded working storage is unavailable.
pub(crate) fn canonicalize_mesh_candidate_for_output(
    topology: &StandardTopology,
    point_assignment: &[usize],
    gauge: Option<MeshCandidateGauge<'_>>,
) -> Option<(StandardTopology, Vec<usize>)> {
    canonicalize_mesh_candidate(topology, point_assignment, gauge)
}

#[cfg(test)]
pub(crate) fn canonicalize_mesh_vertex_labels(
    topology: &StandardTopology,
    point_assignment: &[usize],
) -> Option<(StandardTopology, Vec<usize>)> {
    canonicalize_mesh_candidate(topology, point_assignment, None)
}

#[cfg(test)]
pub(crate) fn mesh_candidates_equivalent(
    left: &(StandardTopology, Vec<usize>),
    right: &(StandardTopology, Vec<usize>),
) -> bool {
    mesh_candidates_equivalent_with_context(left, right, None)
}

#[cfg(test)]
pub(crate) fn mesh_candidates_equivalent_with_gauge(
    left: &(StandardTopology, Vec<usize>),
    right: &(StandardTopology, Vec<usize>),
    edge_classes: &[usize],
    edge_candidates: &[Vec<[usize; 2]>],
    edge_identity_evidence: &[bool],
) -> bool {
    mesh_candidates_equivalent_with_context(
        left,
        right,
        Some(MeshCandidateGauge {
            edge_rows: &[],
            edge_faces: &[],
            edge_classes,
            edge_candidates,
            edge_identity_evidence,
            coordinate_gauge: None,
        }),
    )
}

pub(crate) fn mesh_candidates_equivalent_with_context(
    left: &(StandardTopology, Vec<usize>),
    right: &(StandardTopology, Vec<usize>),
    gauge: Option<MeshCandidateGauge<'_>>,
) -> bool {
    if left.0.vertex_points != right.0.vertex_points {
        return false;
    }
    let left = canonicalize_mesh_candidate(&left.0, &left.1, gauge);
    let right = canonicalize_mesh_candidate(&right.0, &right.1, gauge);
    let equivalent = matches!((&left, &right), (Some(left), Some(right)) if left == right);
    equivalent
}

#[test]
fn endpoint_pair_gauge_canonicalization_snapshots_row_values() {
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
    let edge_classes = [0, 0];
    let edge_candidates = vec![vec![[0, 1], [2, 3]], vec![[0, 1], [2, 3]]];
    let edge_identity_evidence = [false, false];
    let gauge = MeshCandidateGauge {
        edge_rows: &edge_rows,
        edge_faces: &edge_faces,
        edge_classes: &edge_classes,
        edge_candidates: &edge_candidates,
        edge_identity_evidence: &edge_identity_evidence,
        coordinate_gauge: None,
    };

    let left = [[0, 1], [2, 3]].into_iter().map(Some).collect::<Vec<_>>();
    let right = [[2, 3], [0, 1]].into_iter().map(Some).collect::<Vec<_>>();
    assert_eq!(
        canonicalize_partial_endpoint_pair_gauge(&left, gauge),
        canonicalize_partial_endpoint_pair_gauge(&right, gauge),
    );
}

#[test]
fn mesh_candidate_comparison_collapses_coordinate_row_gauge() {
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
    let edge_classes = vec![0, 0];
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
        edge_classes: &edge_classes,
        edge_candidates: &edge_candidates,
        edge_identity_evidence: &edge_identity_evidence,
        coordinate_gauge: Some(&coordinate_gauge),
    };
    let topology = |swapped: bool| StandardTopology {
        faces: vec![
            crate::families::standard::topology::FaceTopology {
                boundaries: vec![crate::families::standard::topology::Boundary {
                    coedges: vec![
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
                    ],
                }],
            },
            crate::families::standard::topology::FaceTopology {
                boundaries: vec![crate::families::standard::topology::Boundary {
                    coedges: vec![
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
                    ],
                }],
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

    assert!(!mesh_candidates_equivalent_with_context(
        &left, &right, None
    ));
    assert!(mesh_candidates_equivalent_with_context(
        &left,
        &right,
        Some(gauge)
    ));

    let mut mismatched_topology = topology(false);
    mismatched_topology.faces[1].boundaries[0].coedges.pop();
    let mismatched = (mismatched_topology, vec![0, 1, 2, 3]);
    assert!(!mesh_candidates_equivalent_with_context(
        &left,
        &mismatched,
        Some(gauge)
    ));
}

#[test]
fn mesh_candidate_comparison_collapses_independent_seam_row_coordinate_automorphisms() {
    const COMPONENT_COUNT: usize = 3;
    let edge_rows = (0..COMPONENT_COUNT * 2)
        .map(|edge| EdgeRow {
            kind: 2,
            handles: vec![(edge * 2) as u32, (edge * 2 + 1) as u32],
            boundary_layout: EdgeBoundaryLayout::CompleteBoundaryRun,
        })
        .collect::<Vec<_>>();
    let edge_faces = (0..COMPONENT_COUNT * 2).map(|_| [0, 1]).collect::<Vec<_>>();
    let edge_classes = (0..COMPONENT_COUNT)
        .flat_map(|class| [class, class])
        .collect::<Vec<_>>();
    let edge_candidates = (0..COMPONENT_COUNT)
        .flat_map(|component| {
            let point = component * 4;
            let options = vec![[point, point + 3], [point + 1, point + 2]];
            [options.clone(), options]
        })
        .collect::<Vec<_>>();
    let edge_identity_evidence = (0..COMPONENT_COUNT * 2).map(|_| false).collect::<Vec<_>>();
    let coordinate_gauge = build_mesh_coordinate_gauge(
        COMPONENT_COUNT * 4,
        &edge_rows,
        &edge_faces,
        &edge_classes,
        &edge_candidates,
        &edge_identity_evidence,
    );
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
        edge_classes: &edge_classes,
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
            boundaries: vec![crate::families::standard::topology::Boundary {
                coedges: endpoints
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
            }],
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
    let canonical_left = canonicalize_mesh_candidate_for_output(&left.0, &left.1, Some(gauge))
        .expect("seam candidate canonicalization");

    for mask in 1..(1 << COMPONENT_COUNT) {
        let right = (topology(mask), identity.clone());
        assert!(!mesh_candidates_equivalent_with_context(
            &left, &right, None
        ));
        assert!(
            mesh_candidates_equivalent_with_context(&left, &right, Some(gauge)),
            "seam gauge did not collapse mask {mask:#b}"
        );
        assert_eq!(
            canonical_left,
            canonicalize_mesh_candidate_for_output(&right.0, &right.1, Some(gauge))
                .expect("seam candidate canonicalization"),
            "seam gauge representative depends on search arrival order for mask {mask:#b}"
        );
    }

    let mut displaced = topology(0);
    displaced.vertex_points[0][0] = -1.0;
    assert!(!mesh_candidates_equivalent_with_context(
        &left,
        &(displaced, identity),
        Some(gauge)
    ));
}

pub(crate) fn canonicalize_endpoint_relation_state(
    domains: &[Vec<MeshEndpointRelationChoice>],
    assigned: &[Option<[usize; 2]>],
    gauge: MeshCandidateGauge<'_>,
) -> Option<MeshEndpointRelationStateSignature> {
    let assigned = assigned
        .iter()
        .copied()
        .map(|pair| {
            pair.map(|mut pair| {
                pair.sort_unstable();
                pair
            })
        })
        .collect::<Vec<_>>();
    let domains = domains
        .iter()
        .map(|choices| {
            let mut choices = choices
                .iter()
                .map(|choice| {
                    let mut assignments = choice.assignments.clone();
                    assignments.sort_unstable();
                    assignments.dedup();
                    let mut edge_pairs = choice
                        .edge_pairs
                        .iter()
                        .map(|&(edge, mut pair)| {
                            pair.sort_unstable();
                            (edge, pair)
                        })
                        .collect::<Vec<_>>();
                    edge_pairs.sort_unstable();
                    Some((assignments, edge_pairs))
                })
                .collect::<Option<Vec<_>>>()?;
            choices.sort_unstable();
            Some(choices)
        })
        .collect::<Option<Vec<_>>>()?;
    let mut state = (assigned, domains);

    let point_count = gauge
        .coordinate_gauge
        .and_then(|coordinate_gauge| {
            coordinate_gauge
                .components
                .iter()
                .filter_map(|permutations| permutations.first())
                .map(Vec::len)
                .next()
        })
        .unwrap_or_else(|| {
            gauge
                .edge_candidates
                .iter()
                .flatten()
                .flatten()
                .copied()
                .max()
                .map_or(0, |point| point.saturating_add(1))
        });
    let identity = (0..point_count).collect::<Vec<_>>();
    state = map_endpoint_relation_state(&state, gauge, &identity)?;
    if let Some(coordinate_gauge) = gauge.coordinate_gauge {
        for permutations in &coordinate_gauge.components {
            if permutations.len() <= 1 {
                continue;
            }
            let mut best = state.clone();
            for permutation in permutations {
                let candidate = map_endpoint_relation_state(&state, gauge, permutation)?;
                if candidate < best {
                    best = candidate;
                }
            }
            state = best;
        }
    }
    Some(state)
}

fn map_endpoint_relation_state(
    state: &MeshEndpointRelationStateSignature,
    gauge: MeshCandidateGauge<'_>,
    permutation: &[usize],
) -> Option<MeshEndpointRelationStateSignature> {
    let row_mapping = relation_row_gauge_mapping(state, gauge, permutation)?;
    let assigned = state.0.iter().copied().enumerate().try_fold(
        (0..state.0.len())
            .map(|_| None)
            .collect::<Vec<Option<[usize; 2]>>>(),
        |mut mapped, (edge, pair)| {
            let target = *row_mapping.get(edge)?;
            let pair = match pair {
                Some(pair) => Some(mapped_endpoint_pair(Some(pair), Some(permutation))?),
                None => None,
            };
            let slot = mapped.get_mut(target)?;
            *slot = pair;
            Some(mapped)
        },
    )?;
    let domains = state
        .1
        .iter()
        .map(|choices| {
            let mut choices = choices
                .iter()
                .map(|(assignments, pairs)| {
                    let mut mapped_pairs = Vec::with_capacity(pairs.len());
                    for &(edge, pair) in pairs {
                        let target = *row_mapping.get(edge)?;
                        if mapped_pairs
                            .iter()
                            .any(|(candidate, _)| *candidate == target)
                        {
                            return None;
                        }
                        mapped_pairs
                            .push((target, mapped_endpoint_pair(Some(pair), Some(permutation))?));
                    }
                    mapped_pairs.sort_unstable();
                    Some((assignments.clone(), mapped_pairs))
                })
                .collect::<Option<Vec<_>>>()?;
            choices.sort_unstable();
            Some(choices)
        })
        .collect::<Option<Vec<_>>>()?;
    Some((assigned, domains))
}

fn relation_row_gauge_mapping(
    state: &MeshEndpointRelationStateSignature,
    gauge: MeshCandidateGauge<'_>,
    permutation: &[usize],
) -> Option<Vec<usize>> {
    let edge_count = state.0.len();
    let identity = || (0..edge_count).collect::<Vec<_>>();
    if gauge.edge_rows.len() != edge_count
        || gauge.edge_faces.len() != edge_count
        || gauge.edge_classes.len() != edge_count
        || gauge.edge_candidates.len() != edge_count
        || gauge.edge_identity_evidence.len() != edge_count
    {
        return Some(identity());
    }

    let mut source_groups = BTreeMap::<MeshEdgeGaugeKey, Vec<usize>>::new();
    let mut target_groups = BTreeMap::<MeshEdgeGaugeKey, Vec<usize>>::new();
    for edge in 0..edge_count {
        if gauge.edge_identity_evidence[edge] {
            continue;
        }
        let base =
            mesh_edge_gauge_base_key(edge, gauge.edge_rows, gauge.edge_faces, gauge.edge_classes)?;
        let source_options = normalized_endpoint_options(&gauge.edge_candidates[edge]);
        let target_options =
            mapped_normalized_endpoint_options(&gauge.edge_candidates[edge], permutation)?;
        source_groups
            .entry((base, target_options))
            .or_default()
            .push(edge);
        target_groups
            .entry((base, source_options))
            .or_default()
            .push(edge);
    }

    let mut row_mapping = identity();
    for (key, mut source) in source_groups {
        let mut targets = target_groups.remove(&key)?;
        source.sort_unstable();
        targets.sort_unstable();
        if source.len() != targets.len() {
            return None;
        }
        if source.len() == 1 {
            // A one-to-one structural group has no row-order gauge. Its map
            // is fixed by the input option relation and needs no state scan.
            *row_mapping.get_mut(source[0])? = targets[0];
            continue;
        }
        let mut ordered = source
            .into_iter()
            .map(|edge| Some((relation_row_signature(state, edge, permutation)?, edge)))
            .collect::<Option<Vec<_>>>()?;
        ordered.sort_unstable();
        for (target, (_, source)) in targets.into_iter().zip(ordered) {
            *row_mapping.get_mut(source)? = target;
        }
    }
    target_groups.is_empty().then_some(row_mapping)
}

fn relation_row_signature(
    state: &MeshEndpointRelationStateSignature,
    edge: usize,
    permutation: &[usize],
) -> Option<Vec<Option<[usize; 2]>>> {
    let choice_count = state
        .1
        .iter()
        .map(Vec::len)
        .try_fold(0usize, usize::checked_add)?;
    let mut signature = Vec::with_capacity(choice_count.saturating_add(1));
    let assigned = match *state.0.get(edge)? {
        Some(pair) => Some(mapped_endpoint_pair(Some(pair), Some(permutation))?),
        None => None,
    };
    signature.push(assigned);
    for choices in &state.1 {
        for (_, pairs) in choices {
            let mut value = None;
            for &(candidate, pair) in pairs {
                if candidate != edge {
                    continue;
                }
                if value.is_some() {
                    return None;
                }
                value = Some(mapped_endpoint_pair(Some(pair), Some(permutation))?);
            }
            signature.push(value);
        }
    }
    Some(signature)
}

#[test]
fn relation_state_memo_collapses_coordinate_gauge() {
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
    let edge_classes = [0, 0];
    let edge_candidates = vec![vec![[0, 1], [2, 3]], vec![[0, 1], [2, 3]]];
    let edge_identity_evidence = [true, true];
    let coordinate_gauge = MeshCoordinateGauge {
        components: vec![vec![vec![0, 1, 2, 3], vec![2, 3, 0, 1]]],
    };
    let gauge = MeshCandidateGauge {
        edge_rows: &edge_rows,
        edge_faces: &edge_faces,
        edge_classes: &edge_classes,
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
            assignments: vec![0],
            edge_pairs: pairs.into_iter().enumerate().collect(),
        }]];
        canonicalize_endpoint_relation_state(
            &domains,
            &pairs.into_iter().map(Some).collect::<Vec<_>>(),
            gauge,
        )
        .expect("coordinate gauge state")
    };

    assert_eq!(state(false), state(true));
}

#[test]
fn relation_state_memo_uses_coordinate_gauge_domain_alternatives() {
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
    let edge_classes = [0, 0];
    let edge_candidates = vec![vec![[0, 3], [1, 2]], vec![[0, 3], [1, 2]]];
    let edge_identity_evidence = [true, true];
    let coordinate_gauge = MeshCoordinateGauge {
        components: vec![vec![vec![0, 1, 2, 3], vec![1, 0, 3, 2]]],
    };
    let gauge = MeshCandidateGauge {
        edge_rows: &edge_rows,
        edge_faces: &edge_faces,
        edge_classes: &edge_classes,
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
            assignments: vec![0],
            edge_pairs: pairs,
        }]];
        canonicalize_endpoint_relation_state(&domains, &[None, None], gauge)
            .expect("coordinate gauge state")
    };

    assert_eq!(state(false), state(true));
}

#[test]
fn relation_state_memo_applies_one_row_mapping_to_assigned_and_domains() {
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
    let edge_classes = [0, 0];
    let edge_candidates = vec![vec![[0, 3], [1, 2]], vec![[0, 3], [1, 2]]];
    let edge_identity_evidence = [false, false];
    let coordinate_gauge = build_mesh_coordinate_gauge(
        4,
        &edge_rows,
        &edge_faces,
        &edge_classes,
        &edge_candidates,
        &edge_identity_evidence,
    );
    let gauge = MeshCandidateGauge {
        edge_rows: &edge_rows,
        edge_faces: &edge_faces,
        edge_classes: &edge_classes,
        edge_candidates: &edge_candidates,
        edge_identity_evidence: &edge_identity_evidence,
        coordinate_gauge: Some(&coordinate_gauge),
    };
    let state = |assigned: Vec<Option<[usize; 2]>>, pairs: Vec<(usize, [usize; 2])>| {
        let domains = vec![vec![MeshEndpointRelationChoice {
            id: 0,
            assignments: vec![0],
            edge_pairs: pairs,
        }]];
        canonicalize_endpoint_relation_state(&domains, &assigned, gauge)
            .expect("row-coordinate gauge state")
    };

    let left = state(vec![Some([0, 3]), None], vec![(0, [1, 2]), (1, [0, 3])]);
    let right = state(vec![None, Some([0, 3])], vec![(0, [0, 3]), (1, [1, 2])]);

    assert_eq!(left, right);
}
