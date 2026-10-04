// SPDX-License-Identifier: Apache-2.0

use super::NumericRun;
use cadmpeg_core::decode::cost::DecodeCost;
use cadmpeg_core::decode::DecodeContext;
use cadmpeg_core::decode::index_from_u32;
use cadmpeg_core::CodecError;
use serde::Serialize;

/// Numeric source runs whose total count equals the declared extent product.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub(crate) struct NumericArray<T> {
    dimensions: Vec<u32>,
    runs: Vec<NumericRun<T>>,
    #[serde(skip)]
    element_count: usize,
}

impl<T: DecodeCost> DecodeCost for NumericArray<T> {
    fn decode_cost(
        &self,
        ctx: &DecodeContext<'_>,
        operation: &'static str,
    ) -> Result<u64, CodecError> {
        (&self.dimensions, &self.runs, self.element_count).decode_cost(ctx, operation)
    }
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
        ctx.charge_work(
            cadmpeg_core::decode::u64_from_index(dimensions.len()),
            "creo numeric array extent validation",
        )?;
        let expected = dimensions.iter().try_fold(1usize, |count, dimension| {
            count.checked_mul(index_from_u32(*dimension))
        });
        let Some(expected) = expected else {
            return Ok(None);
        };
        ctx.charge_work(
            cadmpeg_core::decode::u64_from_index(runs.len()),
            "creo numeric array run validation",
        )?;
        let actual = runs.iter().try_fold(0usize, |count, run| {
            count.checked_add(index_from_u32(run.count))
        });
        let Some(actual) = actual else {
            return Ok(None);
        };
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
    use cadmpeg_core::decode::cost::DecodeCost;
    use cadmpeg_core::decode::u64_from_index;
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
    fn numeric_array_equality_reads_the_cached_element_count() {
        let left = super::NumericArray {
            dimensions: vec![1],
            runs: vec![NumericRun { count: 1, value: 7_u32 }],
            element_count: 1,
        };
        let right = super::NumericArray {
            dimensions: vec![1],
            runs: vec![NumericRun { count: 1, value: 7_u32 }],
            element_count: 2,
        };
        crate::decode::with_test_decode_ctx(|ctx| {
            let scalar_bytes = u64_from_index(std::mem::size_of::<u32>());
            let expected_cost = scalar_bytes
                .checked_mul(3)
                .and_then(|fields| fields.checked_add(u64_from_index(std::mem::size_of::<usize>())))
                .expect("derived array field cost fits");
            assert_eq!(
                left.decode_cost(ctx, "creo legacy numeric array equality")
                    .expect("array cost admitted"),
                expected_cost,
            );
            assert!(!ctx
                .equal(&left, &right, "creo legacy numeric array equality")
                .expect("array equality cost admitted"));
        });
    }
}
