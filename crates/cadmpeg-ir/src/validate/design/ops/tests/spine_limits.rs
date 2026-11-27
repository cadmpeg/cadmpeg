// SPDX-License-Identifier: Apache-2.0
//! The kind of a sweep's spine limits: a point or a plane (§6.6).

use super::{assert_finding, assert_no_finding_containing, entity, named, sweep, with};
use crate::design::feature::FeatureOperation;
use crate::design::names::NameKind;
use crate::design::ops::swept::PathExtent;

#[test]
fn a_spine_limit_picks_a_point_or_a_plane() {
    let limited = |kind: NameKind| {
        let mut operation = sweep(PathExtent {
            along: None,
            against: None,
        });
        if let FeatureOperation::Sweep(sweep) = &mut operation {
            sweep.limits.start = Some(named(vec![entity(kind, 0)]));
        }
        with(operation)
    };
    let fragment = "a spine limit is a point or a plane";
    assert_no_finding_containing(&limited(NameKind::Vertex), fragment);
    assert_no_finding_containing(&limited(NameKind::Face), fragment);
    assert_finding(
        &limited(NameKind::Edge),
        "Sweep.limits.start picks an edge; a spine limit is a point or a plane",
    );
}
