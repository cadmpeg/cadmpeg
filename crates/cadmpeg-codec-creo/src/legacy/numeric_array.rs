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
        let mut expected = 1usize;
        let mut extents = dimensions.iter();
        while let Some(dimension) = ctx.next_charged(&mut extents, "creo numeric array extent validation")? {
            let Some(product) = expected.checked_mul(index_from_u32(*dimension)) else { return Ok(None); };
            expected = product;
        }
        let mut actual = 0usize;
        let mut source_runs = runs.iter();
        while let Some(run) = ctx.next_charged(&mut source_runs, "creo numeric array run validation")? {
            let Some(total) = actual.checked_add(index_from_u32(run.count)) else { return Ok(None); };
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
}
