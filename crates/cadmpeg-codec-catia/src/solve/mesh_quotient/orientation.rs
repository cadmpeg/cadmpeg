// SPDX-License-Identifier: Apache-2.0
//! Exact quotient and direction equivalence for orientation deduplication.

use cadmpeg_core::decode::{DecodeContext, ScopedReservation};
use cadmpeg_core::CodecError;
use std::collections::{hash_map::DefaultHasher, HashMap};
use std::hash::{Hash, Hasher};

use super::{MeshOrientationOption, MeshQuotient};

fn canonical_direction_bit(row: &[bool], index: usize) -> bool {
    row[index] ^ row.first().copied().unwrap_or(false)
}

/// The least node of each node's class, indexed by node. Ascending nodes
/// meet each class first at its least node.
fn quotient_class_firsts(
    ctx: &DecodeContext<'_>,
    quotient: &MeshQuotient<'_>,
    operation: &'static str,
) -> Result<Vec<usize>, CodecError> {
    let node_count = quotient.union.len();
    let (mut first_by_root, _root_storage) =
        ctx.with_scoped_storage(operation, || ctx.alloc_filled(node_count, None, operation))?;
    let mut firsts = ctx.collection_vec(node_count, operation)?;
    for node in ctx.admit_iter(0..node_count, operation)? {
        let root = quotient.union.find(ctx, node)?;
        firsts.push(*first_by_root[root].get_or_insert(node));
    }
    Ok(firsts)
}

/// Hashes the canonical boundary directions: each row relative to its first
/// direction.
fn hash_canonical_directions(
    ctx: &DecodeContext<'_>,
    directions: &[Vec<bool>],
    hasher: &mut DefaultHasher,
    operation: &'static str,
) -> Result<(), CodecError> {
    directions.len().hash(hasher);
    for row in ctx.admit_iter(directions, operation)? {
        row.len().hash(hasher);
        for index in ctx.admit_iter(0..row.len(), operation)? {
            canonical_direction_bit(row, index).hash(hasher);
        }
    }
    Ok(())
}

fn orientation_fingerprint(
    ctx: &DecodeContext<'_>,
    quotient: &MeshQuotient<'_>,
    directions: &[Vec<bool>],
) -> Result<u64, CodecError> {
    const OPERATION: &str = "catia_orientation_fingerprint";
    let mut storage = ctx.reserve_scoped(0, OPERATION)?;
    let firsts = storage.with_storage(|| quotient_class_firsts(ctx, quotient, OPERATION))?;
    let mut hasher = DefaultHasher::new();
    firsts.len().hash(&mut hasher);
    for (node, &first) in ctx.admit_iter(&firsts, OPERATION)?.enumerate() {
        first.hash(&mut hasher);
        if first == node {
            let root = quotient.union.find(ctx, node)?;
            ctx.hash_value(&quotient.domains[root][..], OPERATION)?
                .hash(&mut hasher);
        }
    }
    hash_canonical_directions(ctx, directions, &mut hasher, OPERATION)?;
    Ok(hasher.finish())
}

fn orientation_options_equivalent<'storage>(
    ctx: &DecodeContext<'_>,
    left_quotient: &MeshQuotient<'storage>,
    left_directions: &[Vec<bool>],
    right_quotient: &MeshQuotient<'storage>,
    right_directions: &[Vec<bool>],
) -> Result<bool, CodecError> {
    const OPERATION: &str = "catia_orientation_equivalence";
    if left_quotient.union.len() != right_quotient.union.len()
        || left_directions.len() != right_directions.len()
    {
        return Ok(false);
    }
    let mut storage = ctx.reserve_scoped(0, OPERATION)?;
    let (left_firsts, right_firsts) = storage.with_storage(|| {
        Ok::<_, CodecError>((
            quotient_class_firsts(ctx, left_quotient, OPERATION)?,
            quotient_class_firsts(ctx, right_quotient, OPERATION)?,
        ))
    })?;
    if !ctx.equal(&left_firsts, &right_firsts, OPERATION)? {
        return Ok(false);
    }
    // Equal class firsts give both quotients the same classes; each class
    // domain is compared once, at its least node.
    let mut admitted_visits = (left_firsts).iter().enumerate();
    while let Some((node, &first)) = ctx.next_charged(&mut admitted_visits, OPERATION)? {
        if first != node {
            continue;
        }
        let left = &left_quotient.domains[left_quotient.union.find(ctx, node)?];
        let right = &right_quotient.domains[right_quotient.union.find(ctx, node)?];
        if left.len() != right.len() || !ctx.equal(&left[..], &right[..], OPERATION)? {
            return Ok(false);
        }
    }
    ctx.all_by(
        left_directions.iter().zip(right_directions),
        |(left, right)| {
            Ok(left.len() == right.len()
                && ctx.all_by(
                    0..left.len(),
                    |index| {
                        Ok(canonical_direction_bit(left, index)
                            == canonical_direction_bit(right, index))
                    },
                    OPERATION,
                )?)
        },
        OPERATION,
    )
}

/// Keeps an orientation option unless an equivalent one is already in
/// `output`; options are bucketed by fingerprint.
pub(super) fn admit_orientation_option<'storage>(
    ctx: &'storage DecodeContext<'_>,
    seen: &mut HashMap<u64, Vec<usize>>,
    seen_storage: &mut ScopedReservation<'_>,
    output: &[MeshOrientationOption<'storage>],
    directions: &[Vec<bool>],
    quotient: &MeshQuotient<'storage>,
) -> Result<bool, CodecError> {
    let fingerprint = orientation_fingerprint(ctx, quotient, directions)?;
    if let Some(indices) =
        ctx.get_hash_map(seen, &fingerprint, "catia_orientation_fingerprint_keys")?
    {
        let equivalent = ctx.any_by(
            indices,
            |&index| {
                let (prior_directions, prior_quotient) = &output[index];
                orientation_options_equivalent(
                    ctx,
                    quotient,
                    directions,
                    prior_quotient,
                    prior_directions,
                )
            },
            "catia_orientation_dedup_compare",
        )?;
        if equivalent {
            return Ok(false);
        }
    }
    seen_storage.with_storage(|| {
        ctx.push_vec(
            ctx.entry_hash_map(seen, fingerprint, "catia_orientation_fingerprint_keys")?
                .or_default(),
            output.len(),
            "catia_orientation_fingerprint_indices",
        )?;
        Ok::<_, CodecError>(())
    })?;
    Ok(true)
}

#[cfg(test)]
mod tests {
    use super::{
        admit_orientation_option, orientation_fingerprint, orientation_options_equivalent,
    };
    use crate::solve::mesh_quotient::MeshQuotient;
    use std::collections::{BTreeSet, HashMap, HashSet};
    use std::sync::Arc;

    #[test]
    fn orientation_fingerprint_preserves_exact_quotient_and_direction_equality() {
        let domains = vec![
            Arc::new(HashSet::from([0, 1])),
            Arc::new(HashSet::from([1, 2])),
        ];
        let mut left = MeshQuotient::new(domains.clone());
        let mut right = MeshQuotient::new(domains);
        assert!(
            crate::test_support::with_service_context(|ctx| left.merge_charged(ctx, 0, 1))
                .expect("service merge")
                .is_some()
        );
        assert!(
            crate::test_support::with_service_context(|ctx| right.merge_charged(ctx, 1, 0))
                .expect("service merge")
                .is_some()
        );
        let first = vec![vec![false, true]];
        let complement = vec![vec![true, false]];
        assert_eq!(
            crate::test_support::with_service_context(|ctx| orientation_fingerprint(
                ctx, &left, &first
            ))
            .expect("service orientation"),
            crate::test_support::with_service_context(|ctx| orientation_fingerprint(
                ctx,
                &right,
                &complement
            ))
            .expect("service orientation")
        );
        assert!(
            crate::test_support::with_service_context(|ctx| orientation_options_equivalent(
                ctx,
                &left,
                &first,
                &right,
                &complement
            ))
            .expect("service orientation")
        );
        assert!(
            !crate::test_support::with_service_context(|ctx| orientation_options_equivalent(
                ctx,
                &left,
                &first,
                &right,
                &[vec![false, false]]
            ))
            .expect("service orientation")
        );

        let run = |ctx: &cadmpeg_core::decode::DecodeContext<'_>| {
            let mut seen_storage = ctx.reserve_scoped(0, "fixture orientation index")?;
            let mut seen = HashMap::new();
            let mut output = Vec::new();
            assert!(admit_orientation_option(
                ctx,
                &mut seen,
                &mut seen_storage,
                &output,
                &first,
                &left
            )?);
            output.push((first.clone(), left.clone()));
            assert!(!admit_orientation_option(
                ctx,
                &mut seen,
                &mut seen_storage,
                &output,
                &complement,
                &right
            )?);
            Ok::<_, cadmpeg_core::CodecError>(())
        };
        crate::test_support::with_service_context(run).expect("service resource budget");
        let mut refused = BTreeSet::new();
        for cap in 0..=256 {
            match crate::test_support::with_collection_limit(cap, run) {
                Err(cadmpeg_core::CodecError::ResourceLimit(limit)) => {
                    refused.insert(limit.operation);
                }
                Ok(()) => break,
                Err(error) => panic!("unexpected orientation refusal: {error}"),
            }
        }
        for operation in [
            "catia_orientation_fingerprint",
            "catia_orientation_fingerprint_keys",
            "catia_orientation_fingerprint_indices",
        ] {
            assert!(refused.contains(operation), "no refusal at {operation}");
        }
    }
}
