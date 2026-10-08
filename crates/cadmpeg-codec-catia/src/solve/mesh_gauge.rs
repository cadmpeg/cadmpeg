//! Evidence-preserving gauge quotient for standard mesh candidates.

use std::cmp::Ordering;
use std::collections::BTreeMap;

use cadmpeg_core::decode::cost::DecodeCost;
use cadmpeg_core::decode::DecodeContext;
use cadmpeg_core::CodecError;
#[cfg(test)]
use cadmpeg_ir::features::NonEmptyMembers;

use super::mesh_quotient::{
    raw_endpoint_relation_state_signature, MeshEndpointRelationChoice,
    MeshEndpointRelationSelection, MeshEndpointRelationStateSignature,
};
use super::union_find::UnionFind;
use crate::families::standard::topology::{
    CoedgeUse, EdgeBoundaryLayout, EdgeRow, StandardTopologyDraft,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum MeshEdgeGeometry {
    Line,
    Circle { center: [u64; 3], radius: u64 },
    Bspline,
}

impl DecodeCost for MeshEdgeGeometry {
    const FIXED_BYTES: Option<u64> =
        Some(cadmpeg_core::decode::u64_from_index(std::mem::size_of::<
            Self,
        >()));
    fn decode_cost(
        &self,
        _ctx: &DecodeContext<'_>,
        _operation: &'static str,
    ) -> Result<u64, CodecError> {
        Ok(cadmpeg_core::decode::u64_from_index(std::mem::size_of::<
            Self,
        >()))
    }
}

type MeshEdgeGaugeBaseKey = (u8, EdgeBoundaryLayout, MeshEdgeGeometry, usize, [usize; 2]);

type MeshEdgeGaugeKey = (MeshEdgeGaugeBaseKey, Vec<[usize; 2]>);

/// A topology edge row's structural class: kind, layout, geometry, handle
/// count, incident faces and endpoint options.
type MeshTopologyEdgeClassKey<'a> = (
    u8,
    EdgeBoundaryLayout,
    MeshEdgeGeometry,
    usize,
    &'a [usize],
    &'a [[usize; 2]],
);

/// One coedge use of an edge row: face, boundary, position, orientation and
/// endpoint vertices.
type MeshEdgeUsage = (usize, usize, usize, bool, usize, usize);

/// Structural row classes paired with the rows they map onto. A class without
/// targets maps onto its own rows.
type MeshRowClasses = Vec<(Vec<usize>, Option<Vec<usize>>)>;

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

/// Orders the two points of a pair; the fixed two-element step needs no
/// admission.
fn ordered_pair([left, right]: [usize; 2]) -> [usize; 2] {
    if right < left {
        [right, left]
    } else {
        [left, right]
    }
}

fn coedge_key(coedge: &CoedgeUse) -> (usize, bool, usize, usize) {
    (
        coedge.edge_row,
        coedge.reversed,
        coedge.start_vertex,
        coedge.end_vertex,
    )
}

/// The key of a coedge traversed against its boundary direction.
fn reversed_coedge_key(coedge: &CoedgeUse) -> (usize, bool, usize, usize) {
    (
        coedge.edge_row,
        !coedge.reversed,
        coedge.end_vertex,
        coedge.start_vertex,
    )
}

/// Returns the start of the least rotation of a cycle of `len` keys,
/// preferring the earliest start among equal rotations.
///
/// The two-candidate scan raises `first + second + matched` by at least one
/// per step and stops once any of them reaches `len`, so it takes fewer than
/// `3 * len` steps.
fn least_rotation<K: Ord>(
    ctx: &DecodeContext<'_>,
    len: usize,
    key: impl Fn(usize) -> K,
    operation: &'static str,
) -> Result<usize, CodecError> {
    let (mut first, mut second, mut matched) = (0, 1, 0);
    let mut steps = 0..len.saturating_mul(3);
    while first < len && second < len && matched < len {
        if ctx.next_charged(&mut steps, operation)?.is_none() {
            return Err(CodecError::malformed(
                "least cycle rotation exceeded its step bound",
            ));
        }
        match key((first + matched) % len).cmp(&key((second + matched) % len)) {
            Ordering::Equal => {
                matched += 1;
                continue;
            }
            Ordering::Greater => first += matched + 1,
            Ordering::Less => second += matched + 1,
        }
        if first == second {
            second += 1;
        }
        matched = 0;
    }
    Ok(first.min(second))
}

/// Puts every boundary cycle in its least rotation and direction, then orders
/// each face's boundaries.
fn canonicalize_topology_boundary_gauges(
    ctx: &DecodeContext<'_>,
    topology: &mut StandardTopologyDraft,
) -> Result<(), CodecError> {
    for face in ctx.admit_iter(&mut topology.faces, "catia_mesh_gauge_faces")? {
        for boundary in ctx.admit_iter(&mut face.boundaries, "catia_mesh_gauge_boundaries")? {
            let coedges: &mut [CoedgeUse] = &mut boundary.coedges;
            let len = coedges.len();
            let forward = least_rotation(
                ctx,
                len,
                |index| coedge_key(&coedges[index]),
                "catia_mesh_gauge_forward_cycle_compare",
            )?;
            // Position `index` of the reversed cycle is the coedge at
            // `len - 1 - index`, traversed backwards.
            let reversed_key = |index: usize| reversed_coedge_key(&coedges[len - 1 - index]);
            let reversed = least_rotation(
                ctx,
                len,
                reversed_key,
                "catia_mesh_gauge_reversed_cycle_compare",
            )?;
            let direction = ctx
                .find_map(
                    0..len,
                    |offset| {
                        let order = reversed_key((reversed + offset) % len)
                            .cmp(&coedge_key(&coedges[(forward + offset) % len]));
                        Ok(Some(order).filter(|order| order.is_ne()))
                    },
                    "catia_mesh_gauge_direction_compare",
                )?
                .unwrap_or(Ordering::Equal);
            if direction.is_lt() {
                ctx.reverse(coedges, "catia_mesh_gauge_cycle_reverse")?;
                for coedge in ctx.admit_iter(&mut *coedges, "catia_mesh_gauge_cycle_orient")? {
                    coedge.reversed = !coedge.reversed;
                    std::mem::swap(&mut coedge.start_vertex, &mut coedge.end_vertex);
                }
                if reversed != 0 {
                    ctx.rotate_left(coedges, reversed, "catia_mesh_gauge_cycle_rotation")?;
                }
            } else if forward != 0 {
                ctx.rotate_left(coedges, forward, "catia_mesh_gauge_cycle_rotation")?;
            }
        }
        ctx.stable_sort_by(
            &mut face.boundaries,
            |value| value.coedges.as_slice(),
            |left, right| {
                left.iter()
                    .map(coedge_key)
                    .cmp(right.iter().map(coedge_key))
            },
            "catia_mesh_gauge_boundary_order",
        )?;
    }
    Ok(())
}

#[cfg(test)]
mod boundary_tests {
    use super::{canonicalize_topology_boundary_gauges, CoedgeUse, StandardTopologyDraft};
    use crate::families::standard::topology::{BoundaryDraft, FaceTopologyDraft};
    use cadmpeg_core::decode::ResourceDimension;
    use cadmpeg_core::CodecError;
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
    fn mesh_gauge_equal_forward_cycle_keys_refuse_before_comparison() {
        let mut candidate = topology(vec![vec![coedge(0, false); 32]]);
        let expected = candidate.clone();
        let result = crate::test_support::with_work_limit(32, |ctx| {
            canonicalize_topology_boundary_gauges(ctx, &mut candidate)
        });
        assert!(matches!(
            result,
            Err(CodecError::ResourceLimit(limit))
                if limit.dimension == ResourceDimension::WorkUnits
                    && limit.operation == "catia_mesh_gauge_forward_cycle_compare"
        ));
        assert_eq!(candidate, expected);
    }

    #[test]
    fn mesh_gauge_equal_reversed_cycle_keys_refuse_before_comparison() {
        let candidate = topology(vec![vec![coedge(0, false); 2]]);
        let result = crate::test_support::with_work_refusal(
            "catia_mesh_gauge_reversed_cycle_compare",
            |ctx| canonicalize_topology_boundary_gauges(ctx, &mut candidate.clone()),
        );
        assert!(matches!(
            result,
            Err(CodecError::ResourceLimit(limit))
                if limit.dimension == ResourceDimension::WorkUnits
                    && limit.operation == "catia_mesh_gauge_reversed_cycle_compare"
        ));
    }

    #[test]
    fn mesh_gauge_direction_signature_refuses_before_comparison() {
        let candidate = topology(vec![vec![coedge(0, false)]]);
        let result =
            crate::test_support::with_work_refusal("catia_mesh_gauge_direction_compare", |ctx| {
                canonicalize_topology_boundary_gauges(ctx, &mut candidate.clone())
            });
        assert!(matches!(
            result,
            Err(CodecError::ResourceLimit(limit))
                if limit.dimension == ResourceDimension::WorkUnits
                    && limit.operation == "catia_mesh_gauge_direction_compare"
        ));
    }

    #[test]
    fn mesh_gauge_boundary_sort_refuses_long_signature_bytes() {
        let candidate = topology(vec![vec![coedge(1, false); 16], vec![coedge(0, false); 16]]);
        let result =
            crate::test_support::with_work_refusal("catia_mesh_gauge_boundary_order", |ctx| {
                canonicalize_topology_boundary_gauges(ctx, &mut candidate.clone())
            });
        assert!(matches!(
            result,
            Err(CodecError::ResourceLimit(limit))
                if limit.dimension == ResourceDimension::WorkUnits
                    && limit.operation == "catia_mesh_gauge_boundary_order"
        ));
        let mut short = topology(vec![vec![coedge(1, false)], vec![coedge(0, false)]]);
        crate::test_support::with_service_context(|ctx| {
            canonicalize_topology_boundary_gauges(ctx, &mut short)
        })
        .expect("short boundary signatures fit the service limit");
        assert_eq!(
            short,
            topology(vec![vec![coedge(0, false)], vec![coedge(1, false)]])
        );
    }

    #[test]
    fn mesh_gauge_admitted_cycles_keep_canonical_rotation_direction_and_order() {
        let mut candidate = topology(vec![
            vec![coedge(2, false), coedge(0, false), coedge(1, false)],
            vec![coedge(1, true), coedge(0, true)],
            vec![coedge(3, false); 32],
        ]);
        crate::test_support::with_service_context(|ctx| {
            canonicalize_topology_boundary_gauges(ctx, &mut candidate)
        })
        .expect("service work limit admits the cycles");
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

fn normalized_endpoint_options(
    ctx: &DecodeContext<'_>,
    options: &[[usize; 2]],
) -> Result<Vec<[usize; 2]>, CodecError> {
    let mut normalized = ctx.collect_vec(
        options.iter().map(|&pair| ordered_pair(pair)),
        "catia_gauge_normalized_options",
    )?;
    ctx.sort_unstable_by(
        &mut normalized,
        |value| value,
        Ord::cmp,
        "catia mesh gauge normalized endpoint options sort",
    )?;
    ctx.dedup_vec(&mut normalized, "catia_gauge_normalized_options_dedup")?;
    Ok(normalized)
}

fn mesh_edge_gauge_base_key(
    row: &EdgeRow,
    faces: [usize; 2],
    geometry: MeshEdgeGeometry,
) -> MeshEdgeGaugeBaseKey {
    (
        row.kind(),
        row.boundary_layout(),
        geometry,
        row.handles().len(),
        ordered_pair(faces),
    )
}

/// Relabels endpoint options through `permutation` and normalizes them.
/// Returns `None` when an option names a point outside the permutation.
fn mapped_normalized_endpoint_options(
    ctx: &DecodeContext<'_>,
    options: &[[usize; 2]],
    permutation: &[usize],
) -> Result<Option<Vec<[usize; 2]>>, CodecError> {
    let Some(mut mapped) = ctx.collect_options(
        options
            .iter()
            .map(|&pair| mapped_endpoint_pair(pair, Some(permutation))),
        "catia_gauge_mapped_options",
    )?
    else {
        return Ok(None);
    };
    ctx.sort_unstable_by(
        &mut mapped,
        |value| value,
        Ord::cmp,
        "catia mesh gauge normalized endpoint options sort",
    )?;
    ctx.dedup_vec(&mut mapped, "catia_gauge_normalized_options_dedup")?;
    Ok(Some(mapped))
}

/// Computes `value!` when it does not exceed `limit`.
fn bounded_factorial(
    ctx: &DecodeContext<'_>,
    value: usize,
    limit: usize,
) -> Result<usize, CodecError> {
    let mut result = 1usize;
    let mut factors = 2..value.saturating_add(1);
    while let Some(factor) = ctx.next_charged(&mut factors, "catia_gauge_permutation_count")? {
        result = result.checked_mul(factor).ok_or_else(|| {
            ctx.refuse_codec_limit(
                "catia_gauge_permutation_limit",
                cadmpeg_core::decode::u64_from_index(limit),
                u64::MAX,
            )
        })?;
        if result > limit {
            return Err(ctx.refuse_codec_limit(
                "catia_gauge_permutation_limit",
                cadmpeg_core::decode::u64_from_index(limit),
                cadmpeg_core::decode::u64_from_index(result),
            ));
        }
    }
    Ok(result)
}

/// Steps `order` to its lexicographic successor in place. Returns `false`
/// when `order` is already the last ordering.
fn advance_permutation(ctx: &DecodeContext<'_>, order: &mut [usize]) -> Result<bool, CodecError> {
    let Some(pivot) = ctx.find_by(
        (1..order.len()).rev(),
        |&index| Ok(order[index - 1] < order[index]),
        "catia_gauge_permutation_pivot",
    )?
    else {
        return Ok(false);
    };
    let pivot = pivot - 1;
    let Some(successor) = ctx.find_by(
        (pivot + 1..order.len()).rev(),
        |&index| Ok(order[index] > order[pivot]),
        "catia_gauge_permutation_successor",
    )?
    else {
        return Err(CodecError::malformed(
            "permutation pivot has no larger successor",
        ));
    };
    order.swap(pivot, successor);
    ctx.reverse(&mut order[pivot + 1..], "catia_gauge_permutation_suffix")?;
    Ok(true)
}

/// Lists the `count` orderings of the ascending point class `class` in
/// lexicographic order.
fn enumerate_coordinate_permutations(
    ctx: &DecodeContext<'_>,
    class: &[usize],
    count: usize,
) -> Result<Vec<Vec<usize>>, CodecError> {
    let (mut order, _order_storage) =
        ctx.copy_temporary_slice(class, "catia_gauge_permutation_values")?;
    let mut orders = Vec::new();
    let mut indices = 0..count;
    while let Some(index) = ctx.next_charged(&mut indices, "catia_gauge_permutation_search")? {
        if index > 0 && !advance_permutation(ctx, &mut order)? {
            break;
        }
        let emitted = ctx.copy_slice(&order, "catia_gauge_permutation_values")?;
        ctx.push_vec(&mut orders, emitted, "catia_gauge_permutations")?;
    }
    Ok(orders)
}

/// Numbers signatures by their first occurrence and returns the colors with
/// the number of distinct signatures.
fn intern_gauge_signatures<T: Ord + DecodeCost>(
    ctx: &DecodeContext<'_>,
    signatures: Vec<T>,
) -> Result<(Vec<usize>, usize), CodecError> {
    let mut key_storage = ctx.reserve_scoped(0, "catia_gauge_signature_keys")?;
    let mut ids = BTreeMap::<T, usize>::new();
    let mut colors = Vec::new();
    for signature in ctx.admit_iter(signatures, "catia_gauge_signature_rows")? {
        let next = ids.len();
        let id = key_storage.with_storage(|| {
            Ok::<_, CodecError>(
                *ctx.entry_btree_map(&mut ids, signature, "catia_gauge_signature_keys")?
                    .or_insert(next),
            )
        })?;
        ctx.push_vec(&mut colors, id, "catia_gauge_signature_colors")?;
    }
    Ok((colors, ids.len()))
}

/// The structural groups of one coordinate component: each group's rows and
/// the sorted endpoint options of its rows without identity evidence.
type AffectedGroups<'a> = Vec<(&'a [usize], Vec<&'a [[usize; 2]]>)>;

/// Tests whether relabeling points by `permutation` maps every structural
/// group's endpoint options onto themselves, keeping rows with identity
/// evidence fixed.
fn is_coordinate_automorphism(
    ctx: &DecodeContext<'_>,
    groups: &AffectedGroups<'_>,
    normalized_options: &[Vec<[usize; 2]>],
    edge_identity_evidence: &[bool],
    permutation: &[usize],
) -> Result<bool, CodecError> {
    ctx.all_by(
        groups,
        |(edges, original_unbound)| {
            let mut group_storage = ctx.reserve_scoped(0, "catia_gauge_mapped_rows")?;
            let mut mapped_unbound = Vec::new();
            if !ctx.all_by(
                *edges,
                |&edge| {
                    let (mapped, mapped_storage) =
                        ctx.with_scoped_storage("catia_gauge_mapped_options", || {
                            mapped_normalized_endpoint_options(
                                ctx,
                                &normalized_options[edge],
                                permutation,
                            )
                        })?;
                    let Some(mapped) = mapped else {
                        return Ok(false);
                    };
                    if edge_identity_evidence[edge] {
                        if !ctx.equal(
                            &mapped,
                            &normalized_options[edge],
                            "catia_gauge_identity_row_compare",
                        )? {
                            return Ok(false);
                        }
                    } else {
                        group_storage.with_storage(|| {
                            ctx.push_vec(
                                &mut mapped_unbound,
                                (mapped, mapped_storage),
                                "catia_gauge_mapped_rows",
                            )
                        })?;
                    }
                    Ok(true)
                },
                "catia_gauge_automorphism_rows",
            )? {
                return Ok(false);
            }
            ctx.sort_unstable_by(
                &mut mapped_unbound,
                |value| &value.0,
                Ord::cmp,
                "catia_gauge_mapped_rows_sort",
            )?;
            ctx.all_by(
                original_unbound.iter().zip(&mapped_unbound),
                |(original, mapped)| {
                    ctx.equal(
                        *original,
                        mapped.0.as_slice(),
                        "catia_gauge_automorphism_rows_compare",
                    )
                },
                "catia_gauge_automorphism_rows_compare",
            )
        },
        "catia_gauge_automorphism_groups",
    )
}

pub(super) fn build_mesh_coordinate_gauge<'storage>(
    ctx: &'storage DecodeContext<'_>,
    point_count: usize,
    edge_rows: &[EdgeRow],
    edge_faces: &[[usize; 2]],
    edge_geometry: &[MeshEdgeGeometry],
    edge_candidates: &[Vec<[usize; 2]>],
    edge_identity_evidence: &[bool],
) -> Result<
    (
        MeshCoordinateGauge,
        cadmpeg_core::decode::ScopedReservation<'storage>,
    ),
    CodecError,
> {
    let mut scratch = ctx.reserve_scoped(0, "catia_gauge_build_scratch")?;
    scratch.with_storage(|| {
        const MAX_COORDINATE_GAUGE_PERMUTATIONS: usize = 4_096;
        let identity = || {
            Ok((
                MeshCoordinateGauge {
                    components: Vec::new(),
                },
                ctx.reserve_scoped(0, "catia_gauge_components")?,
            ))
        };
        let edge_count = edge_rows.len();
        if point_count == 0
            || edge_count == 0
            || edge_count != edge_faces.len()
            || edge_count != edge_geometry.len()
            || edge_count != edge_candidates.len()
            || edge_count != edge_identity_evidence.len()
        {
            return identity();
        }

        let mut edge_bases = Vec::new();
        let mut groups = BTreeMap::<MeshEdgeGaugeBaseKey, Vec<usize>>::new();
        let mut normalized_options = Vec::new();
        for edge in ctx.admit_iter(0..edge_count, "catia_gauge_edges")? {
            let key =
                mesh_edge_gauge_base_key(&edge_rows[edge], edge_faces[edge], edge_geometry[edge]);
            ctx.push_vec(&mut edge_bases, key, "catia_gauge_edge_bases")?;
            ctx.push_btree_group(
                &mut groups,
                key,
                edge,
                "catia_gauge_edge_groups",
                "catia_gauge_group_edges",
            )?;
            let normalized = normalized_endpoint_options(ctx, &edge_candidates[edge])?;
            ctx.push_vec(
                &mut normalized_options,
                normalized,
                "catia_gauge_normalized_rows",
            )?;
        }

        // Points named by one structural group may be exchanged with each other,
        // so each group's points join one coordinate component.
        let (mut active, active_storage) = ctx
            .with_scoped_storage("catia_coordinate_gauge_active", || {
                ctx.alloc_filled(point_count, false, "catia_coordinate_gauge_active")
            })?;
        let mut components_by_point = UnionFind::charged(ctx, point_count, "catia_gauge_parent")?;
        for (_, edges) in ctx.admit_iter(&groups, "catia_gauge_group_scan")? {
            let mut first = None;
            for &edge in ctx.admit_iter(edges, "catia_gauge_group_edge_scan")? {
                for &point in ctx
                    .admit_iter(&normalized_options[edge], "catia_gauge_group_point_scan")?
                    .flatten()
                {
                    let Some(active_point) = active.get_mut(point) else {
                        return identity();
                    };
                    *active_point = true;
                    match first {
                        None => first = Some(point),
                        Some(first) => components_by_point.union(ctx, first, point)?,
                    }
                }
            }
        }
        let (mut component_of_root, root_storage) = ctx
            .with_scoped_storage("catia_gauge_component_roots", || {
                ctx.alloc_filled(point_count, None::<usize>, "catia_gauge_component_roots")
            })?;
        let mut component_by_point = ctx.alloc_filled(
            point_count,
            None::<usize>,
            "catia_coordinate_gauge_components",
        )?;
        let mut coordinate_components = Vec::<Vec<usize>>::new();
        for point in ctx.admit_iter(0..point_count, "catia_gauge_component_scan")? {
            if !active[point] {
                continue;
            }
            let root = components_by_point.find(ctx, point)?;
            let component = match component_of_root[root] {
                Some(component) => component,
                None => {
                    let component = coordinate_components.len();
                    ctx.push_vec(
                        &mut coordinate_components,
                        Vec::new(),
                        "catia_gauge_component_rows",
                    )?;
                    component_of_root[root] = Some(component);
                    component
                }
            };
            ctx.push_vec(
                &mut coordinate_components[component],
                point,
                "catia_gauge_component_points",
            )?;
            component_by_point[point] = Some(component);
        }
        drop(components_by_point);
        drop((active, active_storage, component_of_root, root_storage));
        if coordinate_components.is_empty() {
            return identity();
        }

        // Options are listed edge by edge, so an edge's options are the range
        // between consecutive offsets.
        let ((option_records, option_offsets, option_neighbors), option_storage) = ctx
            .with_scoped_storage("catia_gauge_option_graph", || {
                let mut option_records = Vec::<(usize, [usize; 2])>::new();
                let mut option_offsets = Vec::new();
                ctx.push_vec(&mut option_offsets, 0, "catia_gauge_option_offsets")?;
                let mut option_neighbors =
                    ctx.collect_indexed_vec(point_count, "catia_gauge_option_neighbors", |_| {
                        Ok(Vec::<usize>::new())
                    })?;
                for (edge, options) in ctx
                    .admit_iter(&normalized_options, "catia_gauge_option_edges")?
                    .enumerate()
                {
                    for &pair in ctx.admit_iter(options, "catia_gauge_option_scan")? {
                        let option = option_records.len();
                        ctx.push_vec(
                            &mut option_records,
                            (edge, pair),
                            "catia_gauge_option_records",
                        )?;
                        for point in pair {
                            let neighbors = &mut option_neighbors[point];
                            ctx.push_vec(neighbors, option, "catia_gauge_option_arcs")?;
                        }
                    }
                    ctx.push_vec(
                        &mut option_offsets,
                        option_records.len(),
                        "catia_gauge_option_offsets",
                    )?;
                }

                Ok::<_, CodecError>((option_records, option_offsets, option_neighbors))
            })?;

        // Color refinement over points, rows and options. Every signature
        // carries its previous color, so a round only splits classes: an
        // unchanged class count means an unchanged partition, and numbering by
        // first occurrence then reproduces the same colors.
        let (
            (
                mut point_colors,
                mut point_color_count,
                mut row_colors,
                mut row_color_count,
                mut option_colors,
                mut option_color_count,
            ),
            mut color_storage,
        ) = ctx.with_scoped_storage("catia_gauge_colors", || {
            let point_colors = ctx.alloc_filled(point_count, 0usize, "catia_gauge_point_colors")?;
            let point_color_count = usize::from(point_count > 0);
            let (row_signatures, row_signature_storage) =
                ctx.with_scoped_storage("catia_gauge_row_signatures", || {
                    ctx.collect_indexed_vec(edge_count, "catia_gauge_row_signatures", |edge| {
                        Ok((
                            edge_bases[edge],
                            edge_identity_evidence[edge],
                            edge_identity_evidence[edge].then_some(edge),
                        ))
                    })
                })?;
            let (row_colors, row_color_count) = intern_gauge_signatures(ctx, row_signatures)?;
            drop(row_signature_storage);
            let (option_signatures, option_signature_storage) =
                ctx.with_scoped_storage("catia_gauge_option_signatures", || {
                    ctx.collect_vec(
                        option_records.iter().map(|&(edge, _)| row_colors[edge]),
                        "catia_gauge_option_signatures",
                    )
                })?;
            let (option_colors, option_color_count) =
                intern_gauge_signatures(ctx, option_signatures)?;
            drop(option_signature_storage);
            Ok::<_, CodecError>((
                point_colors,
                point_color_count,
                row_colors,
                row_color_count,
                option_colors,
                option_color_count,
            ))
        })?;
        let refinement_limit = point_count
            .checked_add(edge_count)
            .and_then(|count| count.checked_add(option_records.len()))
            .and_then(|count| count.checked_add(1))
            .ok_or_else(|| {
                ctx.refuse_codec_limit("catia_gauge_refinement_limit", u64::MAX, u64::MAX)
            })?;
        let mut rounds = 0..refinement_limit;
        while ctx
            .next_charged(&mut rounds, "catia_gauge_refinement_rounds")?
            .is_some()
        {
            let (
                (
                    next_option_colors,
                    next_option_count,
                    next_row_colors,
                    next_row_count,
                    next_point_colors,
                    next_point_count,
                ),
                next_color_storage,
            ) = ctx.with_scoped_storage("catia_gauge_colors", || {
                let (option_signatures, option_signature_storage) =
                    ctx.with_scoped_storage("catia_gauge_option_signatures", || {
                        ctx.collect_vec(
                            option_records.iter().enumerate().map(
                                |(option, &(edge, [left, right]))| {
                                    (
                                        option_colors[option],
                                        row_colors[edge],
                                        ordered_pair([point_colors[left], point_colors[right]]),
                                    )
                                },
                            ),
                            "catia_gauge_option_signatures",
                        )
                    })?;
                let (next_option_colors, next_option_count) =
                    intern_gauge_signatures(ctx, option_signatures)?;
                drop(option_signature_storage);
                let (row_signatures, row_signature_storage) =
                    ctx.with_scoped_storage("catia_gauge_row_signatures", || {
                        let mut row_signatures = Vec::new();
                        for edge in ctx.admit_iter(0..edge_count, "catia_gauge_row_refinement")? {
                            let mut options = ctx.copy_slice(
                                &next_option_colors[option_offsets[edge]..option_offsets[edge + 1]],
                                "catia_gauge_row_option_colors",
                            )?;
                            ctx.sort_unstable_by(
                                &mut options,
                                |value| value,
                                Ord::cmp,
                                "catia_gauge_row_option_sort",
                            )?;
                            ctx.push_vec(
                                &mut row_signatures,
                                (row_colors[edge], options),
                                "catia_gauge_row_signatures",
                            )?;
                        }
                        Ok::<_, CodecError>(row_signatures)
                    })?;
                let (next_row_colors, next_row_count) =
                    intern_gauge_signatures(ctx, row_signatures)?;
                drop(row_signature_storage);
                let (point_signatures, point_signature_storage) =
                    ctx.with_scoped_storage("catia_gauge_point_signatures", || {
                        let mut point_signatures = Vec::new();
                        for (point, neighbors) in ctx
                            .admit_iter(&option_neighbors, "catia_gauge_point_refinement")?
                            .enumerate()
                        {
                            let mut options = ctx.collect_vec(
                                neighbors.iter().map(|&option| next_option_colors[option]),
                                "catia_gauge_point_option_colors",
                            )?;
                            ctx.sort_unstable_by(
                                &mut options,
                                |value| value,
                                Ord::cmp,
                                "catia_gauge_point_option_sort",
                            )?;
                            ctx.push_vec(
                                &mut point_signatures,
                                (point_colors[point], options),
                                "catia_gauge_point_signatures",
                            )?;
                        }
                        Ok::<_, CodecError>(point_signatures)
                    })?;
                let (next_point_colors, next_point_count) =
                    intern_gauge_signatures(ctx, point_signatures)?;
                drop(point_signature_storage);
                Ok::<_, CodecError>((
                    next_option_colors,
                    next_option_count,
                    next_row_colors,
                    next_row_count,
                    next_point_colors,
                    next_point_count,
                ))
            })?;
            let stable = next_point_count == point_color_count
                && next_row_count == row_color_count
                && next_option_count == option_color_count;
            point_colors = next_point_colors;
            point_color_count = next_point_count;
            row_colors = next_row_colors;
            row_color_count = next_row_count;
            option_colors = next_option_colors;
            option_color_count = next_option_count;
            color_storage = next_color_storage;
            if stable {
                break;
            }
        }
        drop((
            option_records,
            option_offsets,
            option_neighbors,
            option_storage,
        ));

        // A group's points all lie in one component, so each group affects the
        // component of its first point; groups without points constrain nothing.
        let mut affected_groups = ctx.collect_indexed_vec(
            coordinate_components.len(),
            "catia_gauge_affected_groups",
            |_| Ok(Vec::new()),
        )?;
        for (_, edges) in ctx.admit_iter(&groups, "catia_gauge_affected_group_scan")? {
            let first_point = ctx.find_map(
                edges,
                |&edge| Ok(normalized_options[edge].first().map(|pair| pair[0])),
                "catia_gauge_affected_edge_scan",
            )?;
            let Some(component) = first_point.and_then(|point| component_by_point[point]) else {
                continue;
            };
            let mut original_unbound = Vec::new();
            for &edge in ctx.admit_iter(edges, "catia_gauge_original_rows")? {
                if !edge_identity_evidence[edge] {
                    ctx.push_vec(
                        &mut original_unbound,
                        normalized_options[edge].as_slice(),
                        "catia_gauge_original_rows",
                    )?;
                }
            }
            ctx.sort_unstable_by(
                &mut original_unbound,
                |value| value,
                Ord::cmp,
                "catia_gauge_original_rows_sort",
            )?;
            ctx.push_vec(
                &mut affected_groups[component],
                (edges.as_slice(), original_unbound),
                "catia_gauge_affected_group_rows",
            )?;
        }

        let mut output_storage = ctx.reserve_scoped(0, "catia_gauge_components")?;
        let mut components = Vec::new();
        for (points, affected) in ctx
            .admit_iter(coordinate_components, "catia_gauge_component_orders")?
            .zip(&affected_groups)
        {
            let (color_classes, _class_storage) =
                ctx.with_scoped_storage("catia_gauge_color_classes", || {
                    let mut color_classes = BTreeMap::<usize, Vec<usize>>::new();
                    for &point in ctx.admit_iter(&points, "catia_gauge_color_scan")? {
                        ctx.push_btree_group(
                            &mut color_classes,
                            point_colors[point],
                            point,
                            "catia_gauge_color_classes",
                            "catia_gauge_color_points",
                        )?;
                    }
                    Ok::<_, CodecError>(color_classes)
                })?;
            // The identity comes first: it is the first ordering of every class.
            let (mut local_orders, mut order_storage) =
                ctx.with_scoped_storage("catia_gauge_local_orders", || {
                    let mut local_orders = Vec::new();
                    let identity_order =
                        ctx.collect_indexed_vec(point_count, "catia_gauge_identity_order", Ok)?;
                    ctx.push_vec(
                        &mut local_orders,
                        identity_order,
                        "catia_gauge_local_orders",
                    )?;
                    Ok::<_, CodecError>(local_orders)
                })?;
            for (_, class) in ctx.admit_iter(&color_classes, "catia_gauge_color_class_scan")? {
                let remaining_limit = MAX_COORDINATE_GAUGE_PERMUTATIONS / local_orders.len();
                let class_order_count = bounded_factorial(ctx, class.len(), remaining_limit)?;
                if class_order_count == 1 {
                    continue;
                }
                let (class_orders, _class_orders_storage) = ctx
                    .with_scoped_storage("catia_gauge_class_orders", || {
                        enumerate_coordinate_permutations(ctx, class, class_order_count)
                    })?;
                let (next, next_order_storage) =
                    ctx.with_scoped_storage("catia_gauge_next_orders", || {
                        let mut next = Vec::new();
                        for permutation in
                            ctx.admit_iter(&local_orders, "catia_gauge_order_product")?
                        {
                            for order in
                                ctx.admit_iter(&class_orders, "catia_gauge_order_product")?
                            {
                                let mut permutation =
                                    ctx.copy_slice(permutation, "catia_gauge_order_copy")?;
                                for (&source, &target) in ctx
                                    .admit_iter(class, "catia_gauge_order_mapping")?
                                    .zip(order)
                                {
                                    permutation[source] = target;
                                }
                                ctx.push_vec(&mut next, permutation, "catia_gauge_next_orders")?;
                            }
                        }
                        Ok::<_, CodecError>(next)
                    })?;
                local_orders = next;
                order_storage = next_order_storage;
            }
            // Orderings of disjoint classes compose into distinct permutations,
            // and the identity is always an automorphism.
            let mut permutations = Vec::new();
            for (index, permutation) in ctx
                .admit_iter(local_orders, "catia_gauge_automorphism_scan")?
                .enumerate()
            {
                if index == 0
                    || is_coordinate_automorphism(
                        ctx,
                        affected,
                        &normalized_options,
                        edge_identity_evidence,
                        &permutation,
                    )?
                {
                    output_storage.with_storage(|| {
                        let kept =
                            ctx.copy_slice(&permutation, "catia_gauge_kept_permutation_values")?;
                        ctx.push_vec(&mut permutations, kept, "catia_gauge_kept_permutations")
                    })?;
                }
            }
            drop(order_storage);
            output_storage.with_storage(|| {
                ctx.push_vec(&mut components, permutations, "catia_gauge_components")
            })?;
        }
        drop((point_colors, row_colors, option_colors, color_storage));
        Ok((MeshCoordinateGauge { components }, output_storage))
    })
}

/// Relabels and orders an endpoint pair. Returns `None` when a point lies
/// outside the permutation.
fn mapped_endpoint_pair(pair: [usize; 2], permutation: Option<&[usize]>) -> Option<[usize; 2]> {
    let Some(permutation) = permutation else {
        return Some(ordered_pair(pair));
    };
    Some(ordered_pair([
        *permutation.get(pair[0])?,
        *permutation.get(pair[1])?,
    ]))
}

/// Groups the rows without identity evidence by structure and endpoint
/// options, pairing each group of rows with the rows whose options they take
/// under `permutation`. Both sides list rows in ascending order. Returns
/// `None` when the groups do not correspond.
fn gauge_row_classes(
    ctx: &DecodeContext<'_>,
    gauge: MeshCandidateGauge<'_>,
    permutation: Option<&[usize]>,
) -> Result<Option<MeshRowClasses>, CodecError> {
    let (groups, _group_storage) = ctx.with_scoped_storage("catia_gauge_row_groups", || {
        let mut source_groups = BTreeMap::<MeshEdgeGaugeKey, Vec<usize>>::new();
        let mut target_groups = BTreeMap::<MeshEdgeGaugeKey, Vec<usize>>::new();
        for edge in ctx.admit_iter(0..gauge.edge_rows.len(), "catia_gauge_class_edges")? {
            if gauge.edge_identity_evidence[edge] {
                continue;
            }
            let base = mesh_edge_gauge_base_key(
                &gauge.edge_rows[edge],
                gauge.edge_faces[edge],
                gauge.edge_geometry[edge],
            );
            let source_options = normalized_endpoint_options(ctx, &gauge.edge_candidates[edge])?;
            let Some(permutation) = permutation else {
                ctx.push_btree_group(
                    &mut source_groups,
                    (base, source_options),
                    edge,
                    "catia_gauge_source_group_keys",
                    "catia_gauge_source_group_edges",
                )?;
                continue;
            };
            let Some(target_options) =
                mapped_normalized_endpoint_options(ctx, &source_options, permutation)?
            else {
                return Ok(None);
            };
            ctx.push_btree_group(
                &mut source_groups,
                (base, target_options),
                edge,
                "catia_gauge_source_group_keys",
                "catia_gauge_source_group_edges",
            )?;
            ctx.push_btree_group(
                &mut target_groups,
                (base, source_options),
                edge,
                "catia_gauge_target_group_keys",
                "catia_gauge_target_group_edges",
            )?;
        }
        Ok::<_, CodecError>(Some((source_groups, target_groups)))
    })?;
    let Some((source_groups, mut target_groups)) = groups else {
        return Ok(None);
    };
    let mut output_storage = ctx.reserve_scoped(0, "catia_gauge_row_classes")?;
    let mut classes = Vec::new();
    for (key, sources) in ctx.admit_iter(source_groups, "catia_gauge_row_class_scan")? {
        let targets = if permutation.is_some() {
            let Some(targets) =
                ctx.remove_btree_map(&mut target_groups, &key, "catia_gauge_target_group_lookup")?
            else {
                return Ok(None);
            };
            if targets.len() != sources.len() {
                return Ok(None);
            }
            Some(targets)
        } else {
            None
        };
        output_storage.with_storage(|| {
            let sources = ctx.copy_slice(&sources, "catia_gauge_class_source_rows")?;
            let targets = targets
                .as_deref()
                .map(|rows| ctx.copy_slice(rows, "catia_gauge_class_target_rows"))
                .transpose()?;
            ctx.push_vec(&mut classes, (sources, targets), "catia_gauge_row_classes")
        })?;
    }
    if !target_groups.is_empty() {
        return Ok(None);
    }
    output_storage.commit()?;
    Ok(Some(classes))
}

fn gauge_rows_match(gauge: MeshCandidateGauge<'_>, edge_count: usize) -> bool {
    gauge.edge_rows.len() == edge_count
        && gauge.edge_faces.len() == edge_count
        && gauge.edge_geometry.len() == edge_count
        && gauge.edge_candidates.len() == edge_count
        && gauge.edge_identity_evidence.len() == edge_count
}

fn canonicalize_partial_endpoint_pair_gauge_with_permutation(
    ctx: &DecodeContext<'_>,
    pairs: &[Option<[usize; 2]>],
    gauge: MeshCandidateGauge<'_>,
    permutation: Option<&[usize]>,
) -> Result<Option<Vec<Option<[usize; 2]>>>, CodecError> {
    if !gauge_rows_match(gauge, pairs.len()) {
        return ctx.collect_options(
            pairs.iter().map(|pair| match *pair {
                Some(pair) => mapped_endpoint_pair(pair, permutation).map(Some),
                None => Some(None),
            }),
            "catia_gauge_canonical_pairs",
        );
    }
    let (mapped, _mapped_storage) = ctx.with_scoped_storage("catia_gauge_mapped_input", || {
        ctx.collect_options(
            pairs.iter().map(|pair| match *pair {
                Some(pair) => mapped_endpoint_pair(pair, permutation).map(Some),
                None => Some(None),
            }),
            "catia_gauge_canonical_pairs",
        )
    })?;
    let Some(mapped) = mapped else {
        return Ok(None);
    };
    let (classes, _class_storage) = ctx.with_scoped_storage("catia_gauge_row_classes", || {
        gauge_row_classes(ctx, gauge, permutation)
    })?;
    let Some(classes) = classes else {
        return Ok(None);
    };
    // Each class takes its rows' relabeled pairs in sorted order. Pairs are
    // read from the relabeled input, never from a slot an earlier class has
    // already written.
    let mut canonical = ctx.copy_slice(&mapped, "catia_gauge_canonical_pairs")?;
    for (sources, targets) in ctx.admit_iter(&classes, "catia_gauge_class_scan")? {
        let targets = targets.as_deref().unwrap_or(sources);
        let (mut ordered, _ordered_storage) =
            ctx.with_scoped_storage("catia_gauge_ordered_group", || {
                ctx.collect_vec(
                    sources.iter().map(|&edge| mapped[edge]),
                    "catia_gauge_ordered_group",
                )
            })?;
        ctx.sort_unstable_by(
            &mut ordered,
            |value| value,
            Ord::cmp,
            "catia mesh gauge ordered group sort",
        )?;
        for (&slot, pair) in ctx
            .admit_iter(targets, "catia_gauge_class_slots")?
            .zip(ordered)
        {
            canonical[slot] = pair;
        }
    }
    Ok(Some(canonical))
}

fn canonicalize_partial_endpoint_pair_gauge(
    ctx: &DecodeContext<'_>,
    pairs: &[Option<[usize; 2]>],
    gauge: MeshCandidateGauge<'_>,
) -> Result<Option<Vec<Option<[usize; 2]>>>, CodecError> {
    let (canonical, mut canonical_storage) = ctx
        .with_scoped_storage("catia_gauge_partial_candidate", || {
            canonicalize_partial_endpoint_pair_gauge_with_permutation(ctx, pairs, gauge, None)
        })?;
    let Some(mut canonical) = canonical else {
        return Ok(None);
    };
    if let Some(coordinate_gauge) = gauge.coordinate_gauge {
        for permutations in ctx.admit_iter(
            &coordinate_gauge.components,
            "catia_gauge_partial_components",
        )? {
            let mut best = None;
            for permutation in ctx.admit_iter(permutations, "catia_gauge_partial_permutations")? {
                let (candidate, storage) =
                    ctx.with_scoped_storage("catia_gauge_partial_candidate", || {
                        canonicalize_partial_endpoint_pair_gauge_with_permutation(
                            ctx,
                            &canonical,
                            gauge,
                            Some(permutation),
                        )
                    })?;
                let Some(candidate) = candidate else {
                    return Ok(None);
                };
                let incumbent = best.as_ref().map_or(&canonical, |(value, _)| value);
                if ctx
                    .compare(&candidate, incumbent, "catia_gauge_partial_pair_compare")?
                    .is_lt()
                {
                    best = Some((candidate, storage));
                }
            }
            if let Some((value, storage)) = best {
                canonical = value;
                canonical_storage = storage;
            }
        }
    }
    canonical_storage.commit()?;
    Ok(Some(canonical))
}

pub(super) fn canonicalize_complete_endpoint_pairs(
    ctx: &DecodeContext<'_>,
    pairs: &[[usize; 2]],
    gauge: MeshCandidateGauge<'_>,
) -> Result<Option<Vec<[usize; 2]>>, CodecError> {
    let (canonical, _scratch) = ctx.with_scoped_storage("catia_gauge_complete_scratch", || {
        let partial =
            ctx.collect_vec(pairs.iter().copied().map(Some), "catia_gauge_input_pairs")?;
        canonicalize_partial_endpoint_pair_gauge(ctx, &partial, gauge)
    })?;
    let Some(canonical) = canonical else {
        return Ok(None);
    };
    ctx.collect_options(canonical, "catia_gauge_complete_pairs")
}

/// Visits every coedge of every boundary, admitting each level's traversal.
/// Returns `false` as soon as `visit` rejects a coedge.
fn visit_coedges(
    ctx: &DecodeContext<'_>,
    topology: &StandardTopologyDraft,
    operation: &'static str,
    mut visit: impl FnMut(usize, usize, usize, &CoedgeUse) -> Result<bool, CodecError>,
) -> Result<bool, CodecError> {
    ctx.all_by(
        topology.faces.iter().enumerate(),
        |(face, face_topology)| {
            ctx.all_by(
                face_topology.boundaries.iter().enumerate(),
                |(boundary, boundary_topology)| {
                    ctx.all_by(
                        boundary_topology.coedges.iter().enumerate(),
                        |(position, coedge)| visit(face, boundary, position, coedge),
                        operation,
                    )
                },
                operation,
            )
        },
        operation,
    )
}

/// Rewrites every coedge of every boundary, admitting each level's traversal.
/// Returns `false` as soon as `rewrite` rejects a coedge.
fn rewrite_coedges(
    ctx: &DecodeContext<'_>,
    topology: &mut StandardTopologyDraft,
    operation: &'static str,
    mut rewrite: impl FnMut(&mut CoedgeUse) -> bool,
) -> Result<bool, CodecError> {
    ctx.all_by(
        &mut topology.faces,
        |face| {
            ctx.all_by(
                &mut face.boundaries,
                |boundary| {
                    ctx.all_by(
                        &mut *boundary.coedges,
                        |coedge| Ok(rewrite(coedge)),
                        operation,
                    )
                },
                operation,
            )
        },
        operation,
    )
}

fn canonicalize_mesh_edge_row_gauges(
    ctx: &DecodeContext<'_>,
    mut topology: StandardTopologyDraft,
    gauge: MeshCandidateGauge<'_>,
    permutation: Option<&[usize]>,
) -> Result<Option<StandardTopologyDraft>, CodecError> {
    let edge_count = topology.edge_rows.len();
    if gauge.edge_geometry.len() != edge_count
        || gauge.edge_candidates.len() != edge_count
        || gauge.edge_identity_evidence.len() != edge_count
    {
        return Ok(Some(topology));
    }
    let (mapping, index_storage) =
        ctx.with_scoped_storage("catia_mesh_edge_gauge_indices", || {
            let Some(edge_vertices) = topology.edge_vertices(ctx)? else {
                return Ok(None);
            };

            // Faces and uses arrive in face, boundary and position order, so each
            // edge's lists come out sorted and a repeated face is the last one seen.
            let mut incident_faces =
                ctx.collect_indexed_vec(edge_count, "catia_mesh_edge_gauge_faces", |_| {
                    Ok(Vec::<usize>::new())
                })?;
            let mut usage =
                ctx.collect_indexed_vec(edge_count, "catia_mesh_edge_gauge_usage", |_| {
                    Ok(Vec::<MeshEdgeUsage>::new())
                })?;
            let complete = visit_coedges(
                ctx,
                &topology,
                "catia_mesh_edge_gauge_coedge_scan",
                |face, boundary, position, coedge| {
                    let (Some(faces), Some(uses)) = (
                        incident_faces.get_mut(coedge.edge_row),
                        usage.get_mut(coedge.edge_row),
                    ) else {
                        return Ok(false);
                    };
                    if faces.last() != Some(&face) {
                        ctx.push_vec(faces, face, "catia_mesh_edge_gauge_incident_face_entries")?;
                    }
                    ctx.push_vec(
                        uses,
                        (
                            face,
                            boundary,
                            position,
                            coedge.reversed,
                            coedge.start_vertex,
                            coedge.end_vertex,
                        ),
                        "catia_mesh_edge_gauge_usage_entries",
                    )?;
                    Ok(true)
                },
            )?;
            if !complete {
                return Ok(None);
            }

            if gauge.edge_faces.len() == edge_count
                && !ctx.all_by(
                    incident_faces.iter().zip(gauge.edge_faces),
                    |(actual, &expected)| {
                        ctx.equal(
                            actual.as_slice(),
                            &ordered_pair(expected)[..],
                            "catia_mesh_edge_gauge_face_compare",
                        )
                    },
                    "catia_mesh_edge_gauge_face_check",
                )?
            {
                return Ok(None);
            }

            let endpoint_keys = ctx.collect_vec(
                edge_vertices.iter().map(|&pair| ordered_pair(pair)),
                "catia_mesh_edge_gauge_endpoint_keys",
            )?;
            let mut source_options = Vec::new();
            for options in ctx.admit_iter(gauge.edge_candidates, "catia_mesh_gauge_option_scan")? {
                let normalized = normalized_endpoint_options(ctx, options)?;
                ctx.push_vec(
                    &mut source_options,
                    normalized,
                    "catia_mesh_gauge_option_keys",
                )?;
            }
            let target_options = match permutation {
                Some(permutation) => {
                    let mut targets = Vec::new();
                    for (edge, options) in ctx
                        .admit_iter(&source_options, "catia_mesh_gauge_target_scan")?
                        .enumerate()
                    {
                        let mapped = if gauge.edge_identity_evidence[edge] {
                            Vec::new()
                        } else {
                            let Some(mapped) =
                                mapped_normalized_endpoint_options(ctx, options, permutation)?
                            else {
                                return Ok(None);
                            };
                            mapped
                        };
                        ctx.push_vec(&mut targets, mapped, "catia_mesh_gauge_target_options")?;
                    }
                    Some(targets)
                }
                None => None,
            };

            let mut source_groups = BTreeMap::<MeshTopologyEdgeClassKey<'_>, Vec<usize>>::new();
            let mut target_groups = BTreeMap::<MeshTopologyEdgeClassKey<'_>, Vec<usize>>::new();
            for edge in ctx.admit_iter(0..edge_count, "catia_mesh_gauge_class_edges")? {
                if gauge.edge_identity_evidence[edge] {
                    continue;
                }
                let row = &topology.edge_rows[edge];
                let key = |options| {
                    (
                        row.kind(),
                        row.boundary_layout(),
                        gauge.edge_geometry[edge],
                        row.handles().len(),
                        incident_faces[edge].as_slice(),
                        options,
                    )
                };
                match &target_options {
                    None => ctx.push_btree_group(
                        &mut source_groups,
                        key(source_options[edge].as_slice()),
                        edge,
                        "catia_mesh_gauge_source_keys",
                        "catia_mesh_gauge_source_edges",
                    )?,
                    Some(targets) => {
                        ctx.push_btree_group(
                            &mut source_groups,
                            key(targets[edge].as_slice()),
                            edge,
                            "catia_mesh_gauge_source_keys",
                            "catia_mesh_gauge_source_edges",
                        )?;
                        ctx.push_btree_group(
                            &mut target_groups,
                            key(source_options[edge].as_slice()),
                            edge,
                            "catia_mesh_gauge_target_keys",
                            "catia_mesh_gauge_target_edges",
                        )?;
                    }
                }
            }

            let (mut row_permutation, row_storage) = ctx
                .with_scoped_storage("catia_mesh_edge_gauge_row_permutation", || {
                    ctx.collect_indexed_vec(edge_count, "catia_mesh_edge_gauge_row_permutation", Ok)
                })?;
            let (mut normalize_rows, normalize_storage) = ctx
                .with_scoped_storage("catia_mesh_edge_gauge_normalize_rows", || {
                    ctx.alloc_filled(edge_count, false, "catia_mesh_edge_gauge_normalize_rows")
                })?;
            let mut normalizes = false;
            for (key, group) in ctx.admit_iter(source_groups, "catia_mesh_edge_gauge_class_scan")? {
                let slots = if target_options.is_some() {
                    let Some(slots) = ctx.remove_btree_map(
                        &mut target_groups,
                        &key,
                        "catia_mesh_edge_gauge_slots",
                    )?
                    else {
                        return Ok(None);
                    };
                    if slots.len() != group.len() {
                        return Ok(None);
                    }
                    Some(slots)
                } else {
                    None
                };
                let slots = slots.as_deref().unwrap_or(&group);
                let (mut ordered, _ordered_storage) =
                    ctx.with_scoped_storage("catia_mesh_gauge_ordered_group", || {
                        ctx.collect_vec(
                            group
                                .iter()
                                .map(|&edge| (endpoint_keys[edge], usage[edge].as_slice(), edge)),
                            "catia_mesh_gauge_ordered_group",
                        )
                    })?;
                ctx.sort_unstable_by(
                    &mut ordered,
                    |value| value,
                    Ord::cmp,
                    "catia_mesh_edge_gauge_ordered_sort",
                )?;
                for (&slot, &(_, _, old_edge)) in ctx
                    .admit_iter(slots, "catia_mesh_edge_gauge_slot_scan")?
                    .zip(&ordered)
                {
                    row_permutation[old_edge] = slot;
                    let normalize = group.len() > 1 || old_edge != slot;
                    normalize_rows[slot] = normalize;
                    normalizes |= normalize;
                }
            }
            if !target_groups.is_empty() {
                return Ok(None);
            }
            Ok::<_, CodecError>(Some((
                row_permutation,
                normalize_rows,
                normalizes,
                row_storage,
                normalize_storage,
            )))
        })?;
    drop(index_storage);
    let Some((row_permutation, normalize_rows, normalizes, _row_storage, _normalize_storage)) =
        mapping
    else {
        return Ok(None);
    };
    if !normalizes {
        return Ok(Some(topology));
    }

    // Each swap settles one row in its final slot, so a permutation needs
    // fewer swaps than rows.
    let (mut permuting, _permuting_storage) =
        ctx.copy_temporary_slice(&row_permutation, "catia_mesh_gauge_row_permutation_copy")?;
    let mut new_rows = std::mem::take(&mut topology.edge_rows);
    let mut swaps = 0..edge_count;
    for old_edge in ctx.admit_iter(0..edge_count, "catia_mesh_gauge_row_cycles")? {
        while permuting[old_edge] != old_edge {
            if ctx
                .next_charged(&mut swaps, "catia_mesh_gauge_row_swaps")?
                .is_none()
            {
                return Err(CodecError::malformed(
                    "edge row gauge order is not a permutation",
                ));
            }
            let new_edge = permuting[old_edge];
            new_rows.swap(old_edge, new_edge);
            permuting.swap(old_edge, new_edge);
        }
    }
    for (row, &normalize) in ctx
        .admit_iter(&mut new_rows, "catia_mesh_gauge_row_normalization")?
        .zip(&normalize_rows)
    {
        if normalize {
            row.normalize_handles(ctx)?;
        }
    }
    topology.edge_rows = new_rows;
    let complete = rewrite_coedges(
        ctx,
        &mut topology,
        "catia_mesh_gauge_row_relabel",
        |coedge| match row_permutation.get(coedge.edge_row) {
            Some(&row) => {
                coedge.edge_row = row;
                true
            }
            None => false,
        },
    )?;
    if !complete {
        return Ok(None);
    }
    canonicalize_topology_boundary_gauges(ctx, &mut topology)?;
    Ok(Some(topology))
}

#[test]
fn mesh_edge_gauge_ordered_sort_refuses_usage_tuple_bytes() {
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
    let result =
        crate::test_support::with_work_refusal("catia_mesh_edge_gauge_ordered_sort", |ctx| {
            canonicalize_mesh_edge_row_gauges(ctx, topology.clone(), gauge, None)
        });
    assert!(matches!(result, Err(CodecError::ResourceLimit(limit))
        if limit.dimension == cadmpeg_core::decode::ResourceDimension::WorkUnits
            && limit.operation == "catia_mesh_edge_gauge_ordered_sort"));
}

#[test]
fn mesh_edge_gauge_rows_refuse_each_collection_limit() {
    use crate::families::standard::topology::{BoundaryDraft, FaceTopologyDraft};
    use std::collections::HashSet;

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
    mut topology: StandardTopologyDraft,
    permutation: &[usize],
) -> Result<Option<StandardTopologyDraft>, CodecError> {
    if permutation.len() != topology.vertex_points.len() {
        return Ok(None);
    }
    let (mut seen, _seen_storage) = ctx
        .with_scoped_storage("catia_mesh_coordinate_gauge_seen", || {
            ctx.alloc_filled(permutation.len(), false, "catia_mesh_coordinate_gauge_seen")
        })?;
    for &target in ctx.admit_iter(permutation, "catia_mesh_coordinate_gauge_targets")? {
        let Some(seen_target) = seen.get_mut(target) else {
            return Ok(None);
        };
        if std::mem::replace(seen_target, true) {
            return Ok(None);
        }
    }
    let complete = rewrite_coedges(
        ctx,
        &mut topology,
        "catia_mesh_coordinate_gauge_relabel",
        |coedge| match (
            permutation.get(coedge.start_vertex),
            permutation.get(coedge.end_vertex),
        ) {
            (Some(&start), Some(&end)) => {
                coedge.start_vertex = start;
                coedge.end_vertex = end;
                true
            }
            _ => false,
        },
    )?;
    Ok(complete.then_some(topology))
}

/// Orders two topologies that share their vertex points: by logical vertex
/// count, then edge rows (kind, layout, handles), then faces (boundaries,
/// coedges), each sequence by length before its members.
fn compare_topology_structure(
    ctx: &DecodeContext<'_>,
    left: &StandardTopologyDraft,
    right: &StandardTopologyDraft,
    operation: &'static str,
) -> Result<Ordering, CodecError> {
    let counts = left.logical_vertex_count.cmp(&right.logical_vertex_count);
    if counts.is_ne() {
        return Ok(counts);
    }
    if left.edge_rows.len() != right.edge_rows.len() {
        return Ok(left.edge_rows.len().cmp(&right.edge_rows.len()));
    }
    let rows = ctx.find_map(
        left.edge_rows.iter().zip(&right.edge_rows),
        |(left, right)| {
            let header = (left.kind(), u64::from(left.boundary_layout()))
                .cmp(&(right.kind(), u64::from(right.boundary_layout())));
            if header.is_ne() {
                return Ok(Some(header));
            }
            if left.handles().len() != right.handles().len() {
                return Ok(Some(left.handles().len().cmp(&right.handles().len())));
            }
            ctx.find_map(
                left.handles().iter().zip(right.handles()),
                |(left, right)| Ok(Some(left.cmp(right)).filter(|order| order.is_ne())),
                operation,
            )
        },
        operation,
    )?;
    if let Some(rows) = rows {
        return Ok(rows);
    }
    if left.faces.len() != right.faces.len() {
        return Ok(left.faces.len().cmp(&right.faces.len()));
    }
    let faces = ctx.find_map(
        left.faces.iter().zip(&right.faces),
        |(left, right)| {
            if left.boundaries.len() != right.boundaries.len() {
                return Ok(Some(left.boundaries.len().cmp(&right.boundaries.len())));
            }
            ctx.find_map(
                left.boundaries.iter().zip(&right.boundaries),
                |(left, right)| {
                    if left.coedges.len() != right.coedges.len() {
                        return Ok(Some(left.coedges.len().cmp(&right.coedges.len())));
                    }
                    ctx.find_map(
                        left.coedges.iter().zip(right.coedges.iter()),
                        |(left, right)| {
                            Ok(Some(coedge_key(left).cmp(&coedge_key(right)))
                                .filter(|order| order.is_ne()))
                        },
                        operation,
                    )
                },
                operation,
            )
        },
        operation,
    )?;
    Ok(faces.unwrap_or(Ordering::Equal))
}

fn canonicalize_mesh_coordinate_gauges<'storage>(
    ctx: &'storage DecodeContext<'_>,
    mut topology: StandardTopologyDraft,
    mut topology_storage: cadmpeg_core::decode::ScopedReservation<'storage>,
    gauge: MeshCandidateGauge<'_>,
) -> Result<
    Option<(
        StandardTopologyDraft,
        cadmpeg_core::decode::ScopedReservation<'storage>,
    )>,
    CodecError,
> {
    let Some(coordinate_gauge) = gauge
        .coordinate_gauge
        .filter(|coordinate_gauge| !coordinate_gauge.components.is_empty())
    else {
        let topology = topology_storage
            .with_storage(|| canonicalize_mesh_edge_row_gauges(ctx, topology, gauge, None))?;
        return Ok(topology.map(|topology| (topology, topology_storage)));
    };
    for permutations in ctx.admit_iter(
        &coordinate_gauge.components,
        "catia_gauge_coordinate_components",
    )? {
        let mut best = None;
        for permutation in ctx.admit_iter(permutations, "catia_gauge_coordinate_permutations")? {
            let (candidate, storage) =
                ctx.with_scoped_storage("catia_gauge_topology_candidate", || {
                    let Some(mut candidate) = permute_mesh_coordinate_labels(
                        ctx,
                        topology.clone_charged(ctx)?,
                        permutation,
                    )?
                    else {
                        return Ok(None);
                    };
                    canonicalize_topology_boundary_gauges(ctx, &mut candidate)?;
                    canonicalize_mesh_edge_row_gauges(ctx, candidate, gauge, Some(permutation))
                })?;
            let Some(candidate) = candidate else {
                return Ok(None);
            };
            // Every candidate relabels the same topology, so their vertex
            // points agree and the structure decides the order.
            let improves = match &best {
                Some((best, _)) => compare_topology_structure(
                    ctx,
                    &candidate,
                    best,
                    "catia_gauge_coordinate_topology_compare",
                )?
                .is_lt(),
                None => true,
            };
            if improves {
                best = Some((candidate, storage));
            }
        }
        let Some((best, storage)) = best else {
            return Ok(None);
        };
        topology = best;
        topology_storage = storage;
    }
    Ok(Some((topology, topology_storage)))
}

fn canonicalize_mesh_candidate(
    ctx: &DecodeContext<'_>,
    source_topology: &StandardTopologyDraft,
    point_assignment: &[usize],
    gauge: Option<MeshCandidateGauge<'_>>,
) -> Result<Option<(StandardTopologyDraft, Vec<usize>)>, CodecError> {
    let (mut topology, mut topology_storage) = ctx
        .with_scoped_storage("catia_mesh_candidate_draft", || {
            source_topology.clone_charged(ctx)
        })?;
    if point_assignment.len() != topology.logical_vertex_count
        || point_assignment.len() != topology.vertex_points.len()
    {
        return Ok(None);
    }
    let (mut seen, _seen_storage) = ctx.with_scoped_storage("catia_mesh_vertex_seen", || {
        ctx.alloc_filled(point_assignment.len(), false, "catia_mesh_vertex_seen")
    })?;
    for &point in ctx.admit_iter(point_assignment, "catia_mesh_vertex_scan")? {
        let Some(entry) = seen.get_mut(point) else {
            return Ok(None);
        };
        if std::mem::replace(entry, true) {
            return Ok(None);
        }
    }
    let relabeled = rewrite_coedges(
        ctx,
        &mut topology,
        "catia_mesh_vertex_relabel",
        |coedge| match (
            point_assignment.get(coedge.start_vertex),
            point_assignment.get(coedge.end_vertex),
        ) {
            (Some(&start), Some(&end)) => {
                coedge.start_vertex = start;
                coedge.end_vertex = end;
                true
            }
            _ => false,
        },
    )?;
    if !relabeled {
        return Ok(None);
    }
    // Every use of an edge row must run between the same vertices; rows
    // whose first vertex exceeds the second are then flipped.
    let (mut edge_vertices, _edge_storage) = ctx
        .with_scoped_storage("catia_mesh_edge_vertices", || {
            ctx.alloc_filled(topology.edge_rows.len(), None, "catia_mesh_edge_vertices")
        })?;
    let consistent = visit_coedges(
        ctx,
        &topology,
        "catia_mesh_edge_vertex_scan",
        |_, _, _, coedge| {
            let vertices = if coedge.reversed {
                [coedge.end_vertex, coedge.start_vertex]
            } else {
                [coedge.start_vertex, coedge.end_vertex]
            };
            let Some(stored) = edge_vertices.get_mut(coedge.edge_row) else {
                return Ok(false);
            };
            Ok(match stored {
                Some(existing) => *existing == vertices,
                None => {
                    *stored = Some(vertices);
                    true
                }
            })
        },
    )?;
    if !consistent {
        return Ok(None);
    }
    rewrite_coedges(
        ctx,
        &mut topology,
        "catia_mesh_edge_orientation",
        |coedge| {
            if edge_vertices[coedge.edge_row].is_some_and(|vertices| vertices[0] > vertices[1]) {
                coedge.reversed = !coedge.reversed;
            }
            true
        },
    )?;
    canonicalize_topology_boundary_gauges(ctx, &mut topology)?;
    if let Some(gauge) = gauge {
        let Some((canonical, storage)) =
            canonicalize_mesh_coordinate_gauges(ctx, topology, topology_storage, gauge)?
        else {
            return Ok(None);
        };
        topology = canonical;
        topology_storage = storage;
    }
    let (identity, identity_storage) = ctx
        .with_scoped_storage("catia_mesh_candidate_identity", || {
            ctx.collect_indexed_vec(point_assignment.len(), "catia_mesh_candidate_identity", Ok)
        })?;
    topology_storage.commit()?;
    identity_storage.commit()?;
    Ok(Some((topology, identity)))
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
pub(crate) fn mesh_candidates_equivalent(
    ctx: &DecodeContext<'_>,
    left: &(StandardTopologyDraft, Vec<usize>),
    right: &(StandardTopologyDraft, Vec<usize>),
) -> Result<bool, CodecError> {
    mesh_candidates_equivalent_with_context(ctx, left, right, None)
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

/// Compares two search candidates for identity, charging each compared part
/// as it is read.
pub(super) fn mesh_candidates_identical_with_context(
    ctx: &DecodeContext<'_>,
    left: &(StandardTopologyDraft, Vec<usize>),
    right: &(StandardTopologyDraft, Vec<usize>),
) -> Result<bool, CodecError> {
    const OPERATION: &str = "catia_gauge_candidate_identity_compare";
    Ok(ctx.equal(&left.1, &right.1, OPERATION)?
        && ctx.equal(&left.0.vertex_points, &right.0.vertex_points, OPERATION)?
        && compare_topology_structure(ctx, &left.0, &right.0, OPERATION)?.is_eq())
}

pub(super) fn mesh_candidates_equivalent_with_context(
    ctx: &DecodeContext<'_>,
    left: &(StandardTopologyDraft, Vec<usize>),
    right: &(StandardTopologyDraft, Vec<usize>),
    gauge: Option<MeshCandidateGauge<'_>>,
) -> Result<bool, CodecError> {
    if !ctx.equal(
        &left.0.vertex_points,
        &right.0.vertex_points,
        "catia_gauge_candidate_point_compare",
    )? {
        return Ok(false);
    }
    let (equivalent, _scratch) = ctx.with_scoped_storage(
        "catia_gauge_equivalence_scratch",
        || -> Result<bool, CodecError> {
            let left = canonicalize_mesh_candidate(ctx, &left.0, &left.1, gauge)?;
            let right = canonicalize_mesh_candidate(ctx, &right.0, &right.1, gauge)?;
            let (Some(left), Some(right)) = (&left, &right) else {
                return Ok(false);
            };
            // Canonical candidates keep the shared vertex points and carry identity
            // assignments over them, so their structure decides equivalence.
            Ok(compare_topology_structure(
                ctx,
                &left.0,
                &right.0,
                "catia_gauge_candidate_topology_compare",
            )?
            .is_eq())
        },
    )?;
    Ok(equivalent)
}
#[test]
fn endpoint_pair_gauge_canonicalization_snapshots_row_values() {
    catia_test_context!(ctx);
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
        canonicalize_partial_endpoint_pair_gauge(&ctx, &left, gauge)
            .expect("service resource budget"),
        canonicalize_partial_endpoint_pair_gauge(&ctx, &right, gauge)
            .expect("service resource budget"),
    );
}

#[test]
fn complete_endpoint_pair_gauge_charges_options_and_result() {
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
        "catia_gauge_complete_pairs",
    ] {
        assert!(refused.contains(operation), "no refusal at {operation}");
    }
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
    boundary.coedges = NonEmptyMembers::<CoedgeUse>::one(boundary.coedges[0]);
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
    let (coordinate_gauge, _coordinate_storage) = build_mesh_coordinate_gauge(
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
    let (mismatched_coordinate_gauge, _mismatched_coordinate_storage) =
        build_mesh_coordinate_gauge(
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
    let (state, raw_storage) = ctx.with_scoped_storage("catia_relation_raw_state", || {
        raw_endpoint_relation_state_signature(ctx, domains, assigned)
    })?;

    let gauge_point_count = match gauge.coordinate_gauge {
        Some(coordinate_gauge) => ctx.find_map(
            &coordinate_gauge.components,
            |permutations| Ok(permutations.first().map(Vec::len)),
            "catia_relation_gauge_point_count",
        )?,
        None => None,
    };
    let point_count = match gauge_point_count {
        Some(count) => count,
        None => {
            let mut largest = None::<usize>;
            for options in ctx.admit_iter(gauge.edge_candidates, "catia_relation_point_rows")? {
                for &[left, right] in ctx.admit_iter(options, "catia_relation_point_scan")? {
                    largest = largest.max(Some(left.max(right)));
                }
            }
            match largest {
                Some(point) => point.checked_add(1).ok_or_else(|| {
                    ctx.refuse_codec_limit("catia_relation_point_count", u64::MAX, u64::MAX)
                })?,
                None => 0,
            }
        }
    };
    let (identity, identity_storage) = ctx
        .with_scoped_storage("catia_relation_identity", || {
            ctx.collect_indexed_vec(point_count, "catia_relation_identity", Ok)
        })?;
    let (mapped, mut state_storage) = ctx
        .with_scoped_storage("catia_relation_candidate", || {
            map_endpoint_relation_state(ctx, &state, gauge, &identity)
        })?;
    drop((state, raw_storage, identity, identity_storage));
    let Some(mut state) = mapped else {
        return Ok(None);
    };
    let Some(coordinate_gauge) = gauge.coordinate_gauge else {
        state_storage.commit()?;
        return Ok(Some(state));
    };
    for permutations in ctx.admit_iter(
        &coordinate_gauge.components,
        "catia_relation_gauge_components",
    )? {
        if permutations.len() <= 1 {
            continue;
        }
        let mut best = None;
        for permutation in ctx.admit_iter(permutations, "catia_relation_gauge_permutations")? {
            let (candidate, storage) = ctx
                .with_scoped_storage("catia_relation_candidate", || {
                    map_endpoint_relation_state(ctx, &state, gauge, permutation)
                })?;
            let Some(candidate) = candidate else {
                return Ok(None);
            };
            let incumbent = best.as_ref().map_or(&state, |(value, _)| value);
            if ctx
                .compare(&candidate, incumbent, "catia_relation_candidate_compare")?
                .is_lt()
            {
                best = Some((candidate, storage));
            }
        }
        if let Some((best, storage)) = best {
            state = best;
            state_storage = storage;
        }
    }
    state_storage.commit()?;
    Ok(Some(state))
}

fn map_endpoint_relation_state(
    ctx: &DecodeContext<'_>,
    state: &MeshEndpointRelationStateSignature,
    gauge: MeshCandidateGauge<'_>,
    permutation: &[usize],
) -> Result<Option<MeshEndpointRelationStateSignature>, CodecError> {
    let (row_mapping, _mapping_storage) = ctx
        .with_scoped_storage("catia_relation_row_mapping", || {
            relation_row_gauge_mapping(ctx, state, gauge, permutation)
        })?;
    let Some(row_mapping) = row_mapping else {
        return Ok(None);
    };
    let mut assigned = ctx.alloc_filled(state.0.len(), None, "catia_relation_mapped_assigned")?;
    for (edge, pair) in ctx
        .admit_iter(&state.0, "catia_relation_assigned_scan")?
        .enumerate()
    {
        let Some(&target) = row_mapping.get(edge) else {
            return Ok(None);
        };
        let mapped = match *pair {
            Some(pair) => {
                let Some(pair) = mapped_endpoint_pair(pair, Some(permutation)) else {
                    return Ok(None);
                };
                Some(pair)
            }
            None => None,
        };
        let Some(slot) = assigned.get_mut(target) else {
            return Ok(None);
        };
        *slot = mapped;
    }
    let mut domains = Vec::new();
    for row in ctx.admit_iter(&state.1, "catia_relation_domain_scan")? {
        let mut choices = Vec::new();
        for selection in ctx.admit_iter(row, "catia_relation_choice_scan")? {
            let mapped = match selection {
                MeshEndpointRelationSelection::Deferred => MeshEndpointRelationSelection::Deferred,
                MeshEndpointRelationSelection::Enumerated {
                    assignments,
                    edge_pairs,
                } => {
                    let Some(mut mapped_pairs) = ctx.collect_options(
                        edge_pairs.iter().map(|&(edge, pair)| {
                            Some((
                                *row_mapping.get(edge)?,
                                mapped_endpoint_pair(pair, Some(permutation))?,
                            ))
                        }),
                        "catia_relation_mapped_pairs",
                    )?
                    else {
                        return Ok(None);
                    };
                    ctx.sort_unstable_by(
                        &mut mapped_pairs,
                        |value| value,
                        Ord::cmp,
                        "catia_relation_mapped_pairs_sort",
                    )?;
                    // A choice that names one row twice after mapping is not
                    // a relabeling of a valid choice.
                    if ctx.any_by(
                        mapped_pairs.windows(2),
                        |adjacent| Ok(adjacent[0].0 == adjacent[1].0),
                        "catia_relation_mapped_pair_scan",
                    )? {
                        return Ok(None);
                    }
                    MeshEndpointRelationSelection::Enumerated {
                        assignments: ctx
                            .copy_slice(assignments, "catia_relation_mapped_assignments")?,
                        edge_pairs: mapped_pairs,
                    }
                }
            };
            ctx.push_vec(&mut choices, mapped, "catia_relation_mapped_choices")?;
        }
        ctx.sort_unstable_by(
            &mut choices,
            |value| value,
            Ord::cmp,
            "catia_relation_mapped_choices_sort",
        )?;
        ctx.push_vec(&mut domains, choices, "catia_relation_mapped_domains")?;
    }
    Ok(Some((assigned, domains)))
}

/// Maps each edge row onto its gauge-equivalent row under `permutation`.
/// Rows of one structural class are matched in the order of their relation
/// signatures: the relabeled assigned pair followed by the relabeled pair
/// each choice gives the row.
fn relation_row_gauge_mapping(
    ctx: &DecodeContext<'_>,
    state: &MeshEndpointRelationStateSignature,
    gauge: MeshCandidateGauge<'_>,
    permutation: &[usize],
) -> Result<Option<Vec<usize>>, CodecError> {
    let edge_count = state.0.len();
    let (mut row_mapping, mapping_storage) = ctx
        .with_scoped_storage("catia_relation_row_identity", || {
            ctx.collect_indexed_vec(edge_count, "catia_relation_row_identity", Ok)
        })?;
    if !gauge_rows_match(gauge, edge_count) {
        mapping_storage.commit()?;
        return Ok(Some(row_mapping));
    }
    let (classes, _classes_storage) = ctx.with_scoped_storage("catia_relation_classes", || {
        gauge_row_classes(ctx, gauge, Some(permutation))
    })?;
    let Some(classes) = classes else {
        return Ok(None);
    };

    // Signatures are needed only for rows that share a class with another row.
    let mut signature_storage = ctx.reserve_scoped(0, "catia_relation_signatures")?;
    let valid = signature_storage.with_storage(|| {
        let mut signature_slots =
            ctx.alloc_filled(edge_count, None::<usize>, "catia_relation_signature_slots")?;
        let mut signatures = Vec::<(Vec<Option<[usize; 2]>>, usize)>::new();
        for (sources, targets) in ctx.admit_iter(&classes, "catia_relation_class_scan")? {
            let targets = targets.as_deref().unwrap_or(sources);
            if let ([source], [target]) = (sources.as_slice(), targets) {
                // A one-row class has no row-order gauge.
                row_mapping[*source] = *target;
                continue;
            }
            for &edge in ctx.admit_iter(sources, "catia_relation_signature_rows")? {
                let assigned = match state.0[edge] {
                    Some(pair) => {
                        let Some(pair) = mapped_endpoint_pair(pair, Some(permutation)) else {
                            return Ok(false);
                        };
                        Some(pair)
                    }
                    None => None,
                };
                let mut signature = Vec::new();
                ctx.push_vec(&mut signature, assigned, "catia_relation_row_signature")?;
                signature_slots[edge] = Some(signatures.len());
                ctx.push_vec(
                    &mut signatures,
                    (signature, edge),
                    "catia_relation_ordered_rows",
                )?;
            }
        }
        if !signatures.is_empty() {
            for choices in ctx.admit_iter(&state.1, "catia_relation_signature_domains")? {
                for selection in ctx.admit_iter(choices, "catia_relation_signature_choices")? {
                    for (signature, _) in
                        ctx.admit_iter(&mut signatures, "catia_relation_signature_columns")?
                    {
                        ctx.push_vec(signature, None, "catia_relation_row_signature")?;
                    }
                    for &(edge, pair) in
                        ctx.admit_iter(selection.edge_pairs(), "catia_relation_signature_pairs")?
                    {
                        let Some(&Some(slot)) = signature_slots.get(edge) else {
                            continue;
                        };
                        let Some(value) = signatures[slot].0.last_mut() else {
                            return Err(CodecError::malformed(
                                "relation signature lost its choice column",
                            ));
                        };
                        if value.is_some() {
                            return Ok(false);
                        }
                        let Some(pair) = mapped_endpoint_pair(pair, Some(permutation)) else {
                            return Ok(false);
                        };
                        *value = Some(pair);
                    }
                }
            }
        }

        // Classes with several rows hold consecutive signature runs in class order.
        let mut start = 0;
        for (sources, targets) in ctx.admit_iter(&classes, "catia_relation_class_order")? {
            let targets = targets.as_deref().unwrap_or(sources);
            if sources.len() == 1 {
                continue;
            }
            let ordered = &mut signatures[start..start + sources.len()];
            start += sources.len();
            ctx.sort_unstable_by(
                ordered,
                |value| value,
                Ord::cmp,
                "catia_relation_ordered_rows_sort",
            )?;
            for (&target, &(_, source)) in ctx
                .admit_iter(targets, "catia_relation_class_targets")?
                .zip(ordered.iter())
            {
                row_mapping[source] = target;
            }
        }
        Ok::<_, CodecError>(true)
    })?;
    if !valid {
        return Ok(None);
    }
    mapping_storage.commit()?;
    Ok(Some(row_mapping))
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
    ] {
        assert!(refused.contains(operation), "no refusal at {operation}");
    }
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
            .expect("service resource budget")
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
    let (coordinate_gauge, _coordinate_storage) = build_mesh_coordinate_gauge(
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
    use cadmpeg_core::decode::ResourceDimension;

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
        .map(|(gauge, _storage)| gauge)
    };
    catia_test_context!(service_ctx);
    assert!(!run(&service_ctx)
        .expect("service resource budget")
        .components
        .is_empty());

    for operation in [
        "catia_coordinate_gauge_active",
        "catia_coordinate_gauge_components",
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
        let error = cadmpeg_test_support::refusal::resource_limit_at(
            ResourceDimension::CollectionItems,
            operation,
            |cap| crate::test_support::with_collection_limit(cap, &run),
        );
        assert!(matches!(error, CodecError::ResourceLimit(limit)
            if limit.dimension == ResourceDimension::CollectionItems && limit.operation == operation));
    }
}

#[test]
fn mesh_candidate_canonicalization_propagates_collection_refusals() {
    use cadmpeg_core::decode::{DecodeArena, DecodePolicy, ResourceDimension};

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
        "catia_mesh_candidate_identity",
    ] {
        assert!(refused.contains(operation), "no refusal at {operation}");
    }

    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    // Coedge, boundary, face, two handles, row and two points use eight slots.
    policy.limits.max_collection_items = 8;
    let (ctx, _) = DecodeContext::from_root_bytes(&[0], &arena, &policy)
        .expect("fixture fits the input limit");
    let error =
        canonicalize_mesh_candidate_for_output(&ctx, &candidate.0, &candidate.1, Some(gauge))
            .expect_err("canonicalization exceeds the collection limit");
    assert!(matches!(error, CodecError::ResourceLimit(limit)
        if limit.dimension == ResourceDimension::CollectionItems
            && limit.operation == "catia_mesh_vertex_seen"));
}

#[cfg(test)]
mod work_tests;
