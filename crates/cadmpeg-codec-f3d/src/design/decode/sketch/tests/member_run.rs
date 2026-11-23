// SPDX-License-Identifier: Apache-2.0

use super::visibility::{member, ENTITY_SUFFIX};
use crate::design::decode::sketch::parse_sketch_member_run;

const MEMBERS: [u32; 3] = [210, 211, 212];

fn geometry_member(name: &str, contained: bool, after_name: u8) -> Vec<u8> {
    let mut bytes = member(1, 1);
    if !contained {
        bytes.truncate(36);
        bytes.push(0);
    }
    crate::test_support::lp_utf16(&mut bytes, name);
    bytes.push(after_name);
    bytes.extend_from_slice(&3u32.to_le_bytes());
    for record_index in MEMBERS {
        bytes.push(1);
        bytes.extend_from_slice(&record_index.to_le_bytes());
        bytes.extend_from_slice(&[0; 6]);
    }
    bytes.push(0);
    bytes
}

fn run(bytes: &[u8]) -> Vec<(u32, u64)> {
    parse_sketch_member_run(bytes, 0, ENTITY_SUFFIX)
        .into_iter()
        .map(|member| (member.value, member.offset))
        .collect()
}

fn expected(first_entry: u64) -> Vec<(u32, u64)> {
    MEMBERS
        .into_iter()
        .zip([first_entry + 1, first_entry + 12, first_entry + 23])
        .collect()
}

#[test]
fn unnamed_contained_sketch_member_run_starts_at_offset_56() {
    assert_eq!(run(&geometry_member("", true, 0)), expected(56));
}

#[test]
fn named_contained_sketch_member_run_follows_the_display_name() {
    assert_eq!(run(&geometry_member("Profile", true, 0)), expected(70));
}

#[test]
fn named_uncontained_sketch_member_run_follows_the_display_name() {
    assert_eq!(run(&geometry_member("Profile", false, 0)), expected(60));
}

#[test]
fn sketch_member_run_requires_the_zero_byte_after_the_display_name() {
    assert!(run(&geometry_member("Profile", true, 1)).is_empty());
}
