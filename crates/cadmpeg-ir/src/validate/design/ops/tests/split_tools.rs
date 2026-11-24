// SPDX-License-Identifier: Apache-2.0
//! The tools of a `ByTools` split.

use super::{
    assert_finding, assert_no_finding_containing, entity, feature, named, pick, split_by_tools,
    with,
};
use crate::design::feature::FeatureOperation;
use crate::design::names::NameKind;
use crate::design::ops::body::{SplitMode, SplitTool};
use crate::design::selection::{PickTarget, Picked};

fn split_by(tools: Vec<Picked>) -> FeatureOperation {
    let FeatureOperation::Split(mut split) =
        split_by_tools(Some(named(vec![entity(NameKind::Body, 1)])))
    else {
        unreachable!("split_by_tools builds a split");
    };
    if let SplitMode::ByTools {
        tools: SplitTool::Geometry { tools: picks, .. },
        ..
    } = &mut split.mode
    {
        *picks = named(tools);
    }
    FeatureOperation::Split(split)
}

#[test]
fn a_geometry_split_tool_is_not_one_datum_pick_alone() {
    let message = "SplitTool::Geometry picks one datum or principal entity alone";
    let datum = || {
        pick(PickTarget::Datum {
            feature: feature("plane"),
            part: None,
        })
    };
    assert_finding(&with(split_by(vec![datum()])), message);
    assert_no_finding_containing(
        &with(split_by(vec![datum(), entity(NameKind::Face, 2)])),
        message,
    );
    assert_no_finding_containing(&with(split_by(vec![entity(NameKind::Face, 2)])), message);
}
