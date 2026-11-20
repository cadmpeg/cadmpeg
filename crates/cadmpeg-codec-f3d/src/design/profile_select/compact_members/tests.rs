// SPDX-License-Identifier: Apache-2.0

use super::*;
use cadmpeg_ir::geometry::DirectedParameterRange;
use cadmpeg_ir::sketches::SketchEntityId;

fn edge(id: &str, range: [f64; 2], reversed: bool) -> SketchProfileBoundaryUse {
    SketchProfileBoundaryUse {
        entity: SketchEntityId::mint(format!("synthetic:test:edge#{id}")).unwrap(),
        parameter_range: DirectedParameterRange::new(range).unwrap(),
        reversed,
    }
}

fn region(edges: Vec<SketchProfileBoundaryUse>) -> SketchProfileRegion {
    SketchProfileRegion::trimmed(edges, Vec::new()).unwrap()
}

fn divided_rectangle() -> (Vec<SketchProfileRegion>, Vec<SketchProfileRegion>) {
    let whole = vec![region(
        ["a", "b", "c", "d"]
            .map(|id| edge(id, [0.0, 1.0], false))
            .into(),
    )];
    let divided = vec![
        region(vec![
            edge("a", [0.0, 0.5], false),
            edge("divider", [0.0, 1.0], false),
            edge("c", [0.5, 1.0], false),
            edge("d", [0.0, 1.0], false),
        ]),
        region(vec![
            edge("a", [0.5, 1.0], false),
            edge("b", [0.0, 1.0], false),
            edge("c", [0.0, 0.5], false),
            edge("divider", [0.0, 1.0], true),
        ]),
    ];
    (whole, divided)
}

#[test]
fn subdivision_cancels_internal_uses_and_preserves_complete_curve_intervals() {
    let (whole, divided) = divided_rectangle();
    assert!(boundaries_agree(&whole, &divided));
    assert!(!boundaries_agree(&whole, &divided[..1]));
}

#[test]
fn interval_gaps_overlaps_and_traversal_conflicts_refuse_the_certificate() {
    let (whole, divided) = divided_rectangle();
    for (range, reversed) in [([0.6, 1.0], false), ([0.4, 1.0], false), ([0.5, 1.0], true)] {
        let mut changed = divided.clone();
        let SketchProfileRegion::Trimmed { outer_boundary, .. } = &mut changed[1] else {
            panic!("trimmed region")
        };
        outer_boundary[0] = edge("a", range, reversed);
        assert!(!boundaries_agree(&whole, &changed));
    }
    let mut duplicate = divided.clone();
    duplicate.push(divided[0].clone());
    assert!(!boundaries_agree(&whole, &duplicate));
}

#[test]
fn a_hole_boundary_remains_part_of_the_membership_certificate() {
    let (mut whole, divided) = divided_rectangle();
    let SketchProfileRegion::Trimmed {
        hole_boundaries, ..
    } = &mut whole[0]
    else {
        panic!("trimmed region")
    };
    hole_boundaries.push(vec![edge("hole", [0.0, 1.0], true)].try_into().unwrap());
    assert!(!boundaries_agree(&whole, &divided));
}
