// SPDX-License-Identifier: Apache-2.0

use super::NumericRun;
use serde::Serialize;

/// Numeric source runs whose total count equals the declared extent product.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub(crate) struct NumericArray<T> {
    dimensions: Vec<u32>,
    runs: Vec<NumericRun<T>>,
}

impl<T> NumericArray<T> {
    pub(super) fn try_new(dimensions: Vec<u32>, runs: Vec<NumericRun<T>>) -> Option<Self> {
        let expected = dimensions.iter().try_fold(1u64, |count, dimension| {
            count.checked_mul(u64::from(*dimension))
        })?;
        let actual = runs
            .iter()
            .try_fold(0u64, |count, run| count.checked_add(u64::from(run.count)))?;
        (expected == actual).then_some(Self { dimensions, runs })
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
    pub(crate) fn element_count(&self) -> u64 {
        self.runs.iter().map(|run| u64::from(run.count)).sum()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::legacy::NumericPayload;

    #[test]
    fn rejects_incomplete_or_overflowing_extent_products() {
        assert!(
            NumericPayload::array(vec![2, 2], vec![NumericRun { count: 3, value: 0 }]).is_none()
        );
        assert!(
            NumericPayload::array(vec![2, 2], vec![NumericRun { count: 5, value: 0 }]).is_none()
        );
        assert!(
            NumericPayload::array(vec![u32::MAX; 3], vec![NumericRun { count: 0, value: 0 }])
                .is_none()
        );
    }

    #[test]
    fn preserves_array_wire_shape_and_zero_extents() {
        let payload = NumericPayload::array(
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
        assert!(NumericPayload::<i32>::array(vec![0], vec![]).is_some());
    }
}
