// SPDX-License-Identifier: Apache-2.0

use super::NumericRun;
use cadmpeg_core::decode::index_from_u32;
use serde::Serialize;

/// Numeric source runs whose total count equals the declared extent product.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub(crate) struct NumericArray<T> {
    dimensions: Vec<u32>,
    runs: Vec<NumericRun<T>>,
    #[serde(skip)]
    element_count: usize,
}

impl<T> NumericArray<T> {
    /// Admits an array whose runs state the extent product.
    ///
    /// The extent product and the run-count sum are both taken as indices: an
    /// array with more elements than the address space can index is refused
    /// here, so [`NumericArray::element_count`] states an index and no later
    /// conversion can fail.
    pub(super) fn try_new(
        ctx: &cadmpeg_core::decode::DecodeContext<'_>,
        dimensions: Vec<u32>,
        runs: Vec<NumericRun<T>>,
    ) -> Result<Option<Self>, cadmpeg_core::CodecError> {
        if let Some(refusal) = ctx.resource_refusal() {
            return Err(refusal.into());
        }
        let mut expected = 1usize;
        let mut extents = dimensions.iter();
        while extents.len() != 0 {
            let Some(dimension) =
                ctx.next_charged(&mut extents, "creo numeric array extent validation")?
            else {
                break;
            };
            let Some(product) = expected.checked_mul(index_from_u32(*dimension)) else {
                return Ok(None);
            };
            expected = product;
        }
        let mut actual = 0usize;
        let mut source_runs = runs.iter();
        while source_runs.len() != 0 {
            let Some(run) =
                ctx.next_charged(&mut source_runs, "creo numeric array run validation")?
            else {
                break;
            };
            let Some(total) = actual.checked_add(index_from_u32(run.count)) else {
                return Ok(None);
            };
            actual = total;
        }
        Ok((expected == actual).then_some(Self {
            dimensions,
            runs,
            element_count: actual,
        }))
    }

    /// Array extents from outermost to innermost dimension.
    pub(crate) fn dimensions(&self) -> &[u32] {
        &self.dimensions
    }
    /// Source runs in element order.
    pub(crate) fn runs(&self) -> &[NumericRun<T>] {
        &self.runs
    }
    /// Number of logical scalar elements.
    ///
    /// The constructor checks and retains this count during run admission.
    pub(super) fn element_count(&self) -> usize {
        self.element_count
    }
}

#[cfg(test)]
mod tests {
    use crate::legacy::{NumericPayload, NumericRun};

    fn array<T>(dimensions: Vec<u32>, runs: Vec<NumericRun<T>>) -> Option<NumericPayload<T>> {
        crate::decode::with_test_decode_ctx(|ctx| NumericPayload::array(ctx, dimensions, runs))
            .expect("numeric array work admission")
    }

    #[test]
    fn rejects_incomplete_or_overflowing_extent_products() {
        assert!(array(vec![2, 2], vec![NumericRun { count: 3, value: 0 }]).is_none());
        assert!(array(vec![2, 2], vec![NumericRun { count: 5, value: 0 }]).is_none());
        assert!(array(vec![u32::MAX; 3], vec![NumericRun { count: 0, value: 0 }]).is_none());
    }

    #[test]
    fn preserves_array_wire_shape_and_zero_extents() {
        let payload = array(
            vec![2, 2],
            vec![NumericRun {
                count: 4,
                value: -1,
            }],
        )
        .expect("complete array");
        assert_eq!(
            serde_json::to_value(payload).expect("array JSON"),
            serde_json::json!({"form": "array", "dimensions": [2, 2], "runs": [{"count": 4, "value": -1}]})
        );
        assert!(array::<i32>(vec![0], vec![]).is_some());
    }
    #[test]
    fn numeric_array_extent_and_run_validation_refuse_work() {
        let payload = crate::test_support::assert_work_boundaries(
            &[
                "creo numeric array extent validation",
                "creo numeric array run validation",
            ],
            |ctx| NumericPayload::array(ctx, vec![2, 2], vec![NumericRun { count: 4, value: 0 }]),
        );
        assert_eq!(payload.expect("array").element_count(), 4);
    }

    #[test]
    fn numeric_array_admits_only_present_extents_and_runs() {
        let payload = crate::test_support::assert_work_boundaries(
            &[
                "creo numeric array extent validation",
                "creo numeric array run validation",
            ],
            |ctx| {
                NumericPayload::array(
                    ctx,
                    vec![2, 2],
                    vec![
                        NumericRun { count: 2, value: 7 },
                        NumericRun { count: 2, value: 9 },
                    ],
                )
            },
        )
        .expect("complete array");
        assert_eq!(payload.element_count(), 4);
        let NumericPayload::Array(array) = payload else {
            panic!("expected numeric array");
        };
        assert_eq!(array.dimensions(), &[2, 2]);
        assert_eq!(
            array.runs(),
            &[
                NumericRun { count: 2, value: 7 },
                NumericRun { count: 2, value: 9 }
            ]
        );
    }

    #[test]
    fn empty_numeric_array_lanes_are_free_and_zero_extent_visits_once() {
        use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};
        use cadmpeg_core::CodecError;

        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_work_units = 0;
        policy.limits.max_materialized_bytes = 0;
        policy.limits.max_retained_bytes = 0;
        policy.limits.max_collection_items = 0;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("empty root");
        assert!(NumericPayload::<u32>::array(&ctx, Vec::new(), Vec::new())
            .expect("empty lanes do no input-sized work")
            .is_none());
        let original = ctx
            .charge_work_limit(1, "seed empty numeric array refusal")
            .expect_err("zero work cap");
        assert_eq!((original.used, original.additional), (0, 1));
        assert!(
            matches!(NumericPayload::<u32>::array(&ctx, Vec::new(), Vec::new()),
            Err(CodecError::ResourceLimit(refusal)) if refusal == original)
        );
        let payload = crate::test_support::assert_work_boundaries(
            &["creo numeric array extent validation"],
            |ctx| NumericPayload::<u32>::array(ctx, vec![0], Vec::new()),
        )
        .expect("complete zero extent");
        assert_eq!(payload.element_count(), 0);
    }
}
