// SPDX-License-Identifier: Apache-2.0

use super::*;

#[test]
fn owned_surface_suffix_reads_independent_axis_continuations() {
    for suffix in [
        vec![0x0d, 0x00, 0x05, 0x00, 0x07, 0x07],
        vec![0x05, 0x01, 0x09, 0x05, 0x01, 0x05, 0x07, 0x07],
        vec![0x04, 0x40, 0x01, 0x04, 0x41, 0x09, 0x00, 0x07, 0x07],
    ] {
        let mut tail = a5_surface_tail();
        tail.truncate(crate::layout::surface_terminal_span_tail::SUFFIX);
        tail.extend(&suffix);
        assert_eq!(
            super::super::parse_surface_tail(&tail, 0, tail.len(), true),
            Some(tail.len())
        );
        tail.pop();
        assert_eq!(
            super::super::parse_surface_tail(&tail, 0, tail.len(), true),
            None
        );
    }
    for suffix in [
        vec![0x05, 0x02, 0x09, 0x05, 0x01, 0x05, 0x07, 0x07],
        vec![0x05, 0x01, 0x09, 0x05, 0x01, 0x04, 0x07, 0x07],
        vec![0x05, 0x00, 0x09, 0x00, 0x07, 0x07, 0x00],
    ] {
        assert!(super::super::owned_surface_suffix(&suffix).is_none());
    }
}
