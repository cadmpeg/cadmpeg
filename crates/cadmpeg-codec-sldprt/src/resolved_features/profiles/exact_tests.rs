// SPDX-License-Identifier: Apache-2.0
//! Profile bodies of a whole archive walk bind sketch features (spec §2.3).

use super::{holders, profile_position};
use crate::carchive::SketchProfile;

/// Lane sketches at stream offsets 20, 30 and 40, positions 0, 1 and 2.
const STREAMS: [(u64, usize); 3] = [(20, 0), (30, 1), (40, 2)];

fn profile(stream: usize, owner_start: usize, owner_end: usize) -> SketchProfile {
    SketchProfile {
        stream,
        owner_start,
        owner_end,
    }
}

#[test]
fn a_whole_walk_binds_the_profile_body_among_support_snapshots() {
    // The profile feature at 10..50 holds the name at 12 and writes the body
    // at 40; the streams at 20 and 30 are support snapshots.
    let held = holders(&STREAMS, &[profile(40, 10, 50)]);
    assert_eq!(profile_position(&STREAMS, Some(&held), 12, None), Some(2));
    assert_eq!(profile_position(&STREAMS, None, 12, None), None);
}

#[test]
fn the_innermost_holder_of_the_name_binds_its_body() {
    let streams = [(50, 0), (90, 1)];
    let held = holders(&streams, &[profile(90, 10, 100), profile(50, 30, 60)]);
    assert_eq!(profile_position(&streams, Some(&held), 12, None), Some(1));
    assert_eq!(profile_position(&streams, Some(&held), 32, None), Some(0));
}

#[test]
fn a_stopped_walk_or_a_name_outside_every_holder_falls_back_to_the_only_enclosed_stream() {
    assert_eq!(profile_position(&STREAMS, None, 35, None), Some(2));
    let held = holders(&STREAMS, &[profile(20, 10, 30)]);
    assert_eq!(profile_position(&STREAMS, Some(&held), 35, None), Some(2));
}

#[test]
fn a_holder_whose_body_is_no_lane_sketch_or_that_holds_two_bodies_binds_none() {
    let held = holders(&STREAMS, &[profile(99, 10, 50)]);
    assert_eq!(profile_position(&STREAMS, Some(&held), 35, None), None);
    let held = holders(&STREAMS, &[profile(30, 10, 50), profile(40, 10, 50)]);
    assert_eq!(profile_position(&STREAMS, Some(&held), 35, None), None);
}
