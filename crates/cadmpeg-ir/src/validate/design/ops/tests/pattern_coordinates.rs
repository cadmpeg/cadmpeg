// SPDX-License-Identifier: Apache-2.0
//! The coordinates an instance index of a `Scale` layout or stage has.

use super::{assert_finding, assert_no_finding_containing, linear_stage, pattern, skipped, with};
use crate::design::dimension::Dimension;
use crate::design::ops::shared::{CompositeStage, Locus, PatternLayout, ScaleLayout};
use crate::scalar::PositiveReal;

/// §6.5: a `Scale` layout's instance i and a `Composite` `Scale` stage's
/// slice are one coordinate each, so a composite with a `Scale` stage and no
/// `Native` stage has its index counted.
#[test]
fn a_scale_layout_or_stage_has_one_coordinate() {
    let scale = || ScaleLayout {
        factor: Dimension::Literal(PositiveReal::new(2.0).unwrap()),
        count: Dimension::Literal(2),
        center: Locus::Centroid,
    };
    let composite =
        || PatternLayout::Composite(vec![linear_stage(2), CompositeStage::Scale(scale())]);
    let counted = "other than one coordinate per run";
    assert_finding(&with(pattern(composite(), vec![skipped(vec![1])])), counted);
    assert_finding(
        &with(pattern(composite(), vec![skipped(vec![1, 1, 1])])),
        counted,
    );
    assert_no_finding_containing(
        &with(pattern(composite(), vec![skipped(vec![1, 0])])),
        counted,
    );
    assert_finding(
        &with(pattern(
            PatternLayout::Scale(scale()),
            vec![skipped(vec![1, 1])],
        )),
        counted,
    );
    assert_no_finding_containing(
        &with(pattern(
            PatternLayout::Scale(scale()),
            vec![skipped(vec![1])],
        )),
        counted,
    );
}
