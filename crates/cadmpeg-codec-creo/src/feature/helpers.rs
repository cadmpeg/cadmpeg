// SPDX-License-Identifier: Apache-2.0
//! Shared feature-byte helpers used by definition and row decoders.

use cadmpeg_core::decode::{bounded_len, DecodeContext};
use cadmpeg_core::CodecError;

use crate::psb;
use crate::scalar;

pub(super) fn decode_exact_scalars(
    ctx: &DecodeContext<'_>,
    payload: &[u8],
    slot_count: usize,
    cache: &scalar::ScalarCache,
) -> Result<Option<Vec<f64>>, CodecError> {
    // Each slot decodes at least one payload byte and the whole payload must be
    // consumed, so a valid slot count cannot exceed the payload length.
    if bounded_len(
        cadmpeg_core::decode::u64_from_index(slot_count),
        1,
        payload.len(),
    )
    .is_none()
    {
        return Ok(None);
    }
    let mut storage = ctx.reserve_scoped(0, "creo feature scalar values")?;
    let mut values = Vec::new();
    let mut cursor = psb::Cursor::new(payload);
    let mut slots = 0..slot_count;
    while !slots.is_empty()
        && cursor.pos() < payload.len()
        && ctx
            .next_charged(&mut slots, "creo exact scalar scan")?
            .is_some()
    {
        let Some(value) = cursor.take_with(|data, pos| scalar::decode_in_lane(data, pos, cache))
        else {
            return Ok(None);
        };
        storage.with_storage(|| ctx.push_vec(&mut values, value, "creo feature scalar values"))?;
    }
    if !slots.is_empty() || cursor.pos() != payload.len() {
        return Ok(None);
    }
    storage.commit_value(values).map(Some)
}

#[cfg(test)]
mod tests {
    use super::decode_exact_scalars;
    use crate::scalar::ScalarCache;
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
    use cadmpeg_core::CodecError;

    // Core amortized growth starts a nonempty f64 vector at four slots.
    const SCALAR_VECTOR_BYTES: u64 = 4 * 8;

    fn with_limits<T>(
        work: u64,
        materialized: u64,
        retained: u64,
        items: u64,
        test: impl FnOnce(&DecodeContext<'_>) -> T,
    ) -> T {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_work_units = work;
        policy.limits.max_materialized_bytes = materialized;
        policy.limits.max_retained_bytes = retained;
        policy.limits.max_collection_items = items;
        policy.limits.max_entities = 0;
        policy.limits.max_recursion_depth = 0;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("empty root");
        test(&ctx)
    }

    #[test]
    fn exact_scalar_values_visit_only_present_slots_and_retain_one_vector() {
        let cache = ScalarCache::from_section(&[]);
        for count in 1..=4 {
            let visits = u64::try_from(count).expect("four slots fit u64");
            with_limits(
                visits,
                SCALAR_VECTOR_BYTES,
                SCALAR_VECTOR_BYTES,
                visits,
                |ctx| {
                    let values = decode_exact_scalars(ctx, &[0x0f; 4][..count], count, &cache)
                        .expect("one visit per zero token and one four-slot vector")
                        .expect("complete scalar lane");
                    assert_eq!(values.as_slice(), &[0.0; 4][..count]);
                    let limit = ctx
                        .charge_work_limit(1, "after exact scalar visits")
                        .expect_err("all permitted visits were used");
                    assert_eq!(limit.dimension, ResourceDimension::WorkUnits);
                    assert_eq!((limit.used, limit.additional), (visits, 1));
                },
            );
        }
    }

    #[test]
    fn exact_scalar_variable_width_eof_does_not_visit_an_absent_slot() {
        let cache = ScalarCache::from_section(&[]);
        crate::test_support::assert_refusal_order(
            ResourceDimension::WorkUnits,
            &["creo exact scalar scan"],
            |work| {
                with_limits(work, SCALAR_VECTOR_BYTES, 0, 2, |ctx| {
                    assert_eq!(decode_exact_scalars(ctx, &[0x29, 0, 0], 2, &cache)?, None);
                    let limit = ctx
                        .charge_work_limit(u64::MAX, "measure exact scalar visits")
                        .expect_err("measure completed traversal");
                    assert_eq!(limit.used, 1);
                    Ok::<_, CodecError>(())
                })
            },
        );
    }

    #[test]
    fn exact_scalar_values_refuse_before_the_next_present_slot() {
        let cache = ScalarCache::from_section(&[]);
        for count in 1..=4 {
            let visits = u64::try_from(count).expect("four slots fit u64");
            crate::test_support::assert_refusal_order(
                ResourceDimension::WorkUnits,
                &vec!["creo exact scalar scan"; count],
                |cap| with_limits(cap, SCALAR_VECTOR_BYTES, SCALAR_VECTOR_BYTES, 4, |ctx| {
                    match decode_exact_scalars(ctx, &[0x0f; 4][..count], count, &cache) {
                        Ok(values) => {
                            assert_eq!(values.as_deref(), Some(&[0.0; 4][..count]));
                            let limit = ctx.charge_work_limit(1, "after walked scalar visits")
                                .expect_err("all present slots were visited");
                            assert_eq!((limit.used, limit.additional), (visits, 1));
                            Ok(())
                        }
                        Err(CodecError::ResourceLimit(limit)) => {
                            assert_eq!((limit.dimension, limit.used, limit.additional, limit.operation),
                                (ResourceDimension::WorkUnits, cap, 1, "creo exact scalar scan"));
                            Err(CodecError::ResourceLimit(limit))
                        }
                        Err(error) => Err(error),
                    }
                }),
            );
        }
    }

    #[test]
    fn exact_scalar_values_refuse_each_storage_boundary() {
        let cache = ScalarCache::from_section(&[]);
        for dimension in [ResourceDimension::MaterializedBytes,
            ResourceDimension::RetainedBytes, ResourceDimension::CollectionItems] {
            let operations = if dimension == ResourceDimension::CollectionItems {
                vec!["creo feature scalar values"; 3]
            } else {
                vec!["creo feature scalar values"]
            };
            crate::test_support::assert_refusal_order(dimension, &operations, |cap| {
                let materialized = if dimension == ResourceDimension::MaterializedBytes {
                    cap
                } else { SCALAR_VECTOR_BYTES };
                let retained = if dimension == ResourceDimension::RetainedBytes {
                    cap
                } else { SCALAR_VECTOR_BYTES };
                let items = if dimension == ResourceDimension::CollectionItems { cap } else { 3 };
                with_limits(3, materialized, retained, items, |ctx| {
                    match decode_exact_scalars(ctx, &[0x0f; 3], 3, &cache) {
                        Ok(values) => {
                            assert_eq!(values.as_deref(), Some(&[0.0; 3][..]));
                            assert_eq!(cap, if dimension == ResourceDimension::CollectionItems {
                                3
                            } else { SCALAR_VECTOR_BYTES });
                            Ok(())
                        }
                        Err(CodecError::ResourceLimit(limit)) => {
                            let (used, additional) = if dimension == ResourceDimension::CollectionItems {
                                (cap, 1)
                            } else { (0, SCALAR_VECTOR_BYTES) };
                            assert_eq!((limit.dimension, limit.used, limit.additional, limit.operation, limit.limit),
                                (dimension, used, additional, "creo feature scalar values", cap));
                            Err(CodecError::ResourceLimit(limit))
                        }
                        Err(error) => Err(error),
                    }
                })
            });
        }
    }

    #[test]
    fn exact_scalar_empty_and_impossible_counts_are_free() {
        let cache = ScalarCache::from_section(&[]);
        with_limits(0, 0, 0, 0, |ctx| {
            assert_eq!(decode_exact_scalars(ctx, &[], 0, &cache).expect("empty lane"),
                Some(Vec::new()));
            assert_eq!(decode_exact_scalars(ctx, &[0x0f], 2, &cache).expect("impossible count"), None);
            assert_eq!(decode_exact_scalars(ctx, &[0x0f], 0, &cache).expect("unread payload"), None);
            assert!(ctx.resource_refusal().is_none());
        });
    }

    #[test]
    fn rejected_exact_scalar_lanes_release_their_vector_backing() {
        let cache = ScalarCache::from_section(&[]);
        // Undefined second token, then a valid prefix with an unread tail.
        for (payload, count) in [([0x0f, 0xff], 2), ([0x0f, 0xe4], 1)] {
            with_limits(2, SCALAR_VECTOR_BYTES, 0, 2, |ctx| {
                assert_eq!(
                    decode_exact_scalars(ctx, &payload, count, &cache)
                        .expect("invalid lane does not retain output"),
                    None
                );
                let probe = ctx
                    .reserve_scoped(SCALAR_VECTOR_BYTES, "released scalar vector")
                    .expect("discarded backing releases its complete live reservation");
                drop(probe);
                assert!(ctx.resource_refusal().is_none());
            });
        }
    }

    #[test]
    fn exact_scalar_vector_promotion_transfers_only_surviving_backing_to_parent() {
        let cache = ScalarCache::from_section(&[]);
        with_limits(1, SCALAR_VECTOR_BYTES, 0, 1, |ctx| {
            let mut parent = ctx
                .reserve_scoped(0, "scalar parent scope")
                .expect("parent lease");
            let values = parent
                .with_storage(|| decode_exact_scalars(ctx, &[0x0f], 1, &cache))
                .expect("existing backing transfers without a second live charge")
                .expect("complete scalar lane");
            assert_eq!(values, [0.0]);
            assert!(ctx.resource_refusal().is_none());
            drop(values);
            drop(parent);
            let probe = ctx
                .reserve_scoped(SCALAR_VECTOR_BYTES, "released scalar parent")
                .expect("parent releases the transferred backing after value drop");
            drop(probe);
        });
    }
}
