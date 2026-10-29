// SPDX-License-Identifier: Apache-2.0
use super::*;
use crate::test_support::test_fixtures::{content_at, parse as with_view};

#[test]
fn flange_children_follow_the_state_header_without_stamps() {
    for version in [19, 20, 23, 29, 31] {
        let mut bytes = content_at(73, version, &[]);
        bytes.extend_from_slice(&(-1_i32).to_le_bytes());
        bytes.extend_from_slice(&2_u16.to_le_bytes());
        bytes.extend_from_slice(&0x3000_u16.to_le_bytes());
        for value in [3_u32, 1, 0, 0x8000_0005, 6, 0x8000_0007, 0] {
            bytes.extend_from_slice(&value.to_le_bytes());
        }
        let flange = with_view(&bytes, |ctx, view| parse(ctx, view, version)).expect("flange");
        assert_eq!(flange.header.source_index, 73);
        assert!(flange.header.stamps.is_empty());
        assert_eq!(flange.state, -1);
        assert_eq!(flange.value, 0);
        assert_eq!(
            flange
                .children
                .references()
                .iter()
                .map(|r| (r.index(), r.qualified()))
                .collect::<Vec<_>>(),
            [(5, true), (6, false), (7, true)]
        );
        for end in 0..bytes.len() {
            assert!(with_view(&bytes[..end], |ctx, view| parse(ctx, view, version)).is_err());
        }
        bytes.push(0);
        assert!(with_view(&bytes, |ctx, view| parse(ctx, view, version)).is_err());
    }
}
