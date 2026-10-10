//! Endpoint-choice pruning driven by changed face points.

use super::{
    incidence_choice_points, incidence_point_degree, pair_fits_degrees, unique_incidence_faces,
};
use cadmpeg_core::decode::DecodeContext;
use cadmpeg_core::CodecError;
use std::collections::{BTreeMap, BTreeSet, HashSet, VecDeque};

type Supports = Vec<BTreeMap<usize, BTreeSet<usize>>>;

struct Pending {
    edges: VecDeque<usize>,
    queued: Vec<bool>,
    points: VecDeque<(usize, usize)>,
    point_keys: HashSet<(usize, usize)>,
}

impl Pending {
    fn edge(&mut self, ctx: &DecodeContext<'_>, edge: usize) -> Result<(), CodecError> {
        if !self.queued[edge] {
            ctx.push_back(&mut self.edges, edge, "catia incidence affected edges")?;
            self.queued[edge] = true;
        }
        Ok(())
    }

    /// A changed degree or support can change only rows supporting this point.
    fn point(
        &mut self,
        ctx: &DecodeContext<'_>,
        supports: &Supports,
        face: usize,
        point: usize,
    ) -> Result<(), CodecError> {
        const OPERATION: &str = "catia incidence affected points";
        if ctx.insert_hash_set(&mut self.point_keys, (face, point), OPERATION)? {
            ctx.push_back(&mut self.points, (face, point), OPERATION)?;
        }
        if let Some(edges) = ctx.get_btree_map(&supports[face], &point, OPERATION)? {
            for &edge in ctx.admit_iter(edges, OPERATION)? {
                self.edge(ctx, edge)?;
            }
        }
        Ok(())
    }
}

/// A pair cannot leave a degree-one point supported only by its own row.
fn preserves_new_degree_support(
    ctx: &DecodeContext<'_>,
    supports: &Supports,
    edge_support: &[usize],
    degrees: &[BTreeMap<usize, u8>],
    faces: [usize; 2],
    pair: [usize; 2],
) -> Result<bool, CodecError> {
    for face in unique_incidence_faces(faces) {
        for (rank, point) in pair.into_iter().enumerate() {
            if rank == 1 && point == pair[0] {
                continue;
            }
            let selected_degree = 1 + u8::from(pair[0] == pair[1]);
            if incidence_point_degree(ctx, &degrees[face], point)? + selected_degree != 1 {
                continue;
            }
            let support = ctx
                .get_btree_map(&supports[face], &point, "catia incidence support lookups")?
                .map_or(0, BTreeSet::len);
            let own = ctx
                .binary_search(edge_support, &point, "catia incidence support lookups")?
                .is_ok();
            if support <= usize::from(own) {
                return Ok(false);
            }
        }
    }
    Ok(true)
}

/// Remove support points that the retained pairs no longer contain.
fn retain_edge_support(
    ctx: &DecodeContext<'_>,
    supports: &mut Supports,
    edge_support: &mut Vec<usize>,
    faces: [usize; 2],
    edge: usize,
    retained_pairs: &[[usize; 2]],
    pending: &mut Pending,
) -> Result<Option<()>, CodecError> {
    const OPERATION: &str = "catia incidence removed support points";
    let (retained, _storage) =
        ctx.with_scoped_storage(OPERATION, || incidence_choice_points(ctx, retained_pairs))?;
    let mut consistent = true;
    ctx.retain_vec(
        edge_support,
        |point| {
            if !consistent || ctx.binary_search(&retained, point, OPERATION)?.is_ok() {
                return Ok(true);
            }
            for face in unique_incidence_faces(faces) {
                let Some(edges) = ctx.get_mut_btree_map(&mut supports[face], point, OPERATION)?
                else {
                    consistent = false;
                    return Ok(true);
                };
                if !ctx.remove_btree_set(edges, &edge, OPERATION)? {
                    consistent = false;
                    return Ok(true);
                }
                if edges.is_empty() {
                    ctx.remove_btree_map(&mut supports[face], point, OPERATION)?;
                }
                pending.point(ctx, supports, face, *point)?;
            }
            Ok(false)
        },
        OPERATION,
    )?;
    Ok(consistent.then_some(()))
}

pub(super) fn prune_incidence_choices(
    ctx: &DecodeContext<'_>,
    choices: &mut [Vec<[usize; 2]>],
    edge_faces: &[[usize; 2]],
    face_count: usize,
    explicit_support_complete: bool,
) -> Result<Option<()>, CodecError> {
    let mut fixed = ctx.alloc_filled(choices.len(), false, "catia_incidence_fixed_edges")?;
    let mut degrees = ctx.collect_indexed_vec(face_count, "catia_incidence_degrees", |_| {
        Ok(BTreeMap::<usize, u8>::new())
    })?;
    let mut edge_supports = ctx.collection_vec(choices.len(), "catia incidence edge supports")?;
    for pairs in ctx.admit_iter(&*choices, "catia incidence edge supports")? {
        edge_supports.push(incidence_choice_points(ctx, pairs)?);
    }
    let mut supports = ctx.collect_indexed_vec(face_count, "catia_incidence_supports", |_| {
        Ok(BTreeMap::<usize, BTreeSet<usize>>::new())
    })?;
    for (edge, points) in ctx
        .admit_iter(&edge_supports, "catia incidence point support counts")?
        .enumerate()
    {
        for face in unique_incidence_faces(edge_faces[edge]) {
            for &point in ctx.admit_iter(points, "catia incidence point support counts")? {
                ctx.insert_btree_group_set(
                    &mut supports[face],
                    point,
                    edge,
                    "catia incidence point support counts",
                    "catia incidence point support counts",
                )?;
            }
        }
    }
    let mut pending = Pending {
        edges: VecDeque::new(),
        queued: ctx.alloc_filled(choices.len(), false, "catia incidence affected edges")?,
        points: VecDeque::new(),
        point_keys: HashSet::new(),
    };
    for edge in ctx.admit_iter(0..choices.len(), "catia incidence affected edges")? {
        pending.edge(ctx, edge)?;
    }
    loop {
        while let Some((face, point)) = ctx.next_charged(
            &mut std::iter::from_fn(|| pending.points.pop_front()),
            "catia incidence degree-one points",
        )? {
            ctx.remove_hash_set(
                &mut pending.point_keys,
                &(face, point),
                "catia incidence affected points",
            )?;
            if !explicit_support_complete
                || incidence_point_degree(ctx, &degrees[face], point)? != 1
            {
                continue;
            }
            let edges =
                ctx.get_btree_map(&supports[face], &point, "catia incidence support lookups")?;
            match edges.map_or(0, BTreeSet::len) {
                0 => return Ok(None),
                1 => {
                    let Some(&edge) = edges.and_then(BTreeSet::first) else {
                        return Ok(None);
                    };
                    let before = choices[edge].len();
                    ctx.retain_vec(
                        &mut choices[edge],
                        |pair| Ok(pair.contains(&point)),
                        "catia incidence sole support choices",
                    )?;
                    if choices[edge].is_empty() {
                        return Ok(None);
                    }
                    if before != choices[edge].len() {
                        if retain_edge_support(
                            ctx,
                            &mut supports,
                            &mut edge_supports[edge],
                            edge_faces[edge],
                            edge,
                            &choices[edge],
                            &mut pending,
                        )?
                        .is_none()
                        {
                            return Ok(None);
                        }
                        pending.edge(ctx, edge)?;
                    }
                }
                _ => {}
            }
        }
        let Some(edge) = ctx.next_charged(
            &mut std::iter::from_fn(|| pending.edges.pop_front()),
            "catia_incidence_iteration",
        )?
        else {
            return Ok(Some(()));
        };
        pending.queued[edge] = false;
        if fixed[edge] {
            continue;
        }
        let faces = edge_faces[edge];
        let before = choices[edge].len();
        ctx.retain_vec(
            &mut choices[edge],
            |pair| {
                Ok(pair_fits_degrees(ctx, &degrees, faces, *pair)?
                    && (!explicit_support_complete
                        || preserves_new_degree_support(
                            ctx,
                            &supports,
                            &edge_supports[edge],
                            &degrees,
                            faces,
                            *pair,
                        )?))
            },
            "catia incidence retained choices",
        )?;
        if before != choices[edge].len()
            && retain_edge_support(
                ctx,
                &mut supports,
                &mut edge_supports[edge],
                faces,
                edge,
                &choices[edge],
                &mut pending,
            )?
            .is_none()
        {
            return Ok(None);
        }
        let [pair] = choices[edge].as_slice() else {
            if choices[edge].is_empty() {
                return Ok(None);
            }
            continue;
        };
        let pair = *pair;
        for face in unique_incidence_faces(faces) {
            for point in pair {
                let degree = ctx
                    .entry_btree_map(
                        &mut degrees[face],
                        point,
                        "catia incidence endpoint degrees",
                    )?
                    .or_default();
                let Some(next) = degree.checked_add(1) else {
                    return Ok(None);
                };
                *degree = next;
                pending.point(ctx, &supports, face, point)?;
            }
        }
        fixed[edge] = true;
        if retain_edge_support(
            ctx,
            &mut supports,
            &mut edge_supports[edge],
            faces,
            edge,
            &[],
            &mut pending,
        )?
        .is_none()
        {
            return Ok(None);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn chain(count: usize) -> Vec<Vec<[usize; 2]>> {
        let mut rows = vec![vec![[0, count]]];
        rows.extend((0..count).map(|point| vec![[point, point + 1], [point + 1, point + 2]]));
        rows
    }

    #[test]
    fn incidence_point_worklist_closes_two_fronts_without_global_rounds() {
        let measure = |count| {
            crate::test_support::with_service_context(|ctx| {
                let mut choices = chain(count);
                let faces = vec![[0, 0]; choices.len()];
                let (result, _storage) = ctx
                    .with_scoped_storage("fixture pruning", || {
                        prune_incidence_choices(ctx, &mut choices, &faces, 1, true)
                    })
                    .expect("worklist");
                assert_eq!(result, Some(()));
                assert_eq!(choices[0], vec![[0, count]]);
                for (point, row) in choices[1..].iter().enumerate() {
                    assert_eq!(row, &vec![[point, point + 1]]);
                }
                let CodecError::ResourceLimit(limit) = ctx
                    .charge_work(u64::MAX, "measure performed work")
                    .expect_err("measure")
                else {
                    panic!("work refusal");
                };
                limit.used
            })
        };
        let small = measure(128);
        let large = measure(256);
        assert!(
            large <= 3 * small,
            "work must grow below quadratic: {small}, {large}"
        );
    }

    #[test]
    fn incidence_point_worklist_refuses_before_queue_growth() {
        let result =
            crate::test_support::with_work_refusal("catia incidence affected edges", |ctx| {
                let mut choices = chain(16);
                let faces = vec![[0, 0]; choices.len()];
                ctx.with_scoped_storage("fixture pruning", || {
                    prune_incidence_choices(ctx, &mut choices, &faces, 1, true)
                })
                .map(|(result, _storage)| result)
            });
        assert!(matches!(result, Err(CodecError::ResourceLimit(_))));
    }
}
