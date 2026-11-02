// SPDX-License-Identifier: Apache-2.0
//! Record extents the topology scope pass skips (spec §4, §5).

use super::{face_body, scan};
use crate::test_support::parasolid::{bridge, loop_head, triangle_body};

#[test]
fn a_face_front_ref_read_as_a_record_header_does_not_hide_the_next_loop() {
    // The last front ref of FACE 10 reads as a `00 12` vertex-use or a
    // `00 0d` shell header whose extent spans the loop after the face.
    for phantom in [0x0012_u16, 0x000d] {
        let mut bytes = triangle_body();
        let face = bridge(10, 20, 100);
        let at = bytes
            .windows(face.len())
            .position(|window| window == face.as_slice())
            .unwrap();
        let front = at + 2 + face_body::SURFACE_FRONT_REFS;
        for (index, reference) in [1, 1, 0x5f, 0x16, phantom].into_iter().enumerate() {
            let cell = front + 2 * index;
            bytes[cell..cell + 2].copy_from_slice(&u16::to_be_bytes(reference));
        }
        assert!(
            scan(&bytes).loops.contains_key(&20),
            "phantom {phantom:#06x}"
        );
    }
}

#[test]
fn a_string_node_length_read_as_a_record_header_does_not_hide_the_next_loop() {
    // The length 13 of a KEY or CHAR_VALUES node reads as a `00 0d` shell
    // header whose extent spans the loop after the node.
    for tag in [0x4f_u8, 0x54] {
        let mut bytes = triangle_body();
        let head = loop_head(20, 30, 10);
        let at = bytes
            .windows(head.len())
            .position(|window| window == head.as_slice())
            .unwrap();
        let mut node = vec![0x00, tag, 0x00, 0x00, 0x00, 0x0d, 0x00, 0x05];
        node.extend_from_slice(b"SDL/TYSA_NAME");
        bytes.splice(at..at, node);
        assert!(scan(&bytes).loops.contains_key(&20), "tag {tag:#04x}");
    }
}
