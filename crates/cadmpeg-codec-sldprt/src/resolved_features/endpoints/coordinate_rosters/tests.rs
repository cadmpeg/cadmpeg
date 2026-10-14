// SPDX-License-Identifier: Apache-2.0

use super::*;

fn marker(id: &str, owner: Option<&str>, offset: u64, coordinate: bool) -> SketchInputEntity {
    let mut marker = SketchInputEntity::new(id, "lane", 0, offset, SketchInputKind::Point);
    marker.feature_ref = owner.map(str::to_owned);
    marker.coordinates_m =
        coordinate.then(|| cadmpeg_ir::units::FiniteVector::new([0.0, 0.0]).unwrap());
    marker
}

#[test]
fn owner_and_grammar_rosters_keep_offset_order() {
    let markers = [
        marker("a2", Some("a"), 20, true),
        marker("b1", Some("b"), 10, true),
        marker("a0", Some("a"), 0, false),
        marker("a1", Some("a"), 10, true),
        marker("none", None, 5, true),
        marker("empty", Some(""), 5, true),
    ];
    let roster = CoordinateRosters::new(markers.iter().collect());
    for _ in 0..500 {
        for (owner, complete, expected) in [
            (Some("a"), false, vec!["a1", "a2"]),
            (Some("a"), true, vec!["a0", "a1", "a2"]),
            (Some("b"), false, vec!["b1"]),
            (None, true, vec!["none"]),
            (Some(""), true, vec!["empty"]),
            (Some("missing"), true, vec![]),
        ] {
            let owner = marker("query", owner, 0, false);
            roster.with_roster(&owner, complete, |rows| {
                assert_eq!(
                    rows.iter().map(|row| row.id()).collect::<Vec<_>>(),
                    expected
                );
            });
        }
    }
}

#[test]
fn repeated_endpoint_queries_read_the_coordinate_roster() {
    use crate::layout::current_extended_zero_tail_92_profile_curve as layout;
    let mut payload = vec![0; layout::LEN];
    payload[layout::HEADER..layout::NATIVE_KIND].copy_from_slice(&layout::HEADER_VALUE);
    payload[layout::NATIVE_KIND..layout::NATIVE_KIND + 4].copy_from_slice(&2u32.to_le_bytes());
    payload[layout::PROFILE_LOCUS..layout::ROLE].copy_from_slice(&layout::PROFILE_LOCUS_VALUE);
    payload[layout::ROLE..layout::STATE].copy_from_slice(&layout::ROLE_VALUE.to_le_bytes());
    payload[layout::STATE..layout::SELECTOR].copy_from_slice(&layout::STATE_VALUE.to_le_bytes());
    payload[layout::SELECTOR..layout::SELECTOR + 8].copy_from_slice(&layout::SELECTOR_VALUE);
    payload[layout::STATE_SCALAR..layout::ZERO_ENDPOINT_PREFIX]
        .copy_from_slice(&layout::STATE_SCALAR_VALUE.to_le_bytes());
    payload[layout::ENDPOINT_FIRST..layout::ENDPOINT_SECOND].copy_from_slice(&2u16.to_le_bytes());
    payload[layout::ENDPOINT_SELECTOR..layout::SIGNED_SELECTOR]
        .copy_from_slice(&layout::ENDPOINT_SELECTOR_VALUE.to_le_bytes());
    payload[layout::SIGNED_SELECTOR..layout::ZERO_TAIL]
        .copy_from_slice(&layout::SIGNED_SELECTOR_VALUE.to_le_bytes());
    let curve = SketchInputEntity::new("curve", "lane", 0, 0, SketchInputKind::LineOrCircle);
    let markers = [
        marker("third", None, 30, true),
        marker("first", None, 10, true),
        marker("second", None, 20, true),
    ];
    let roster = CoordinateRosters::new(markers.iter().collect());
    for _ in 0..50 {
        let endpoints = super::super::coordinate_roster_curve_endpoint_markers_at(
            &payload,
            &curve,
            &roster,
            Some(layout::ENDPOINT_FIRST),
        );
        assert_eq!(
            endpoints
                .iter()
                .map(|marker| marker.id())
                .collect::<Vec<_>>(),
            ["third", "first"]
        );
    }
}
