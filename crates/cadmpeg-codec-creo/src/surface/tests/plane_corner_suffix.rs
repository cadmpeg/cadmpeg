// SPDX-License-Identifier: Apache-2.0

use crate::scalar::ScalarCache;
use crate::surface::first_coordinate_plane_corner_tokens;

#[test]
fn terminal_plane_corner_candidates_preserve_one_and_eight_byte_scalar_widths() {
    let cache = ScalarCache::default();
    for width in [1, 8] {
        for prefix in [0, 1, 47, 48, 49, 4096] {
            for reference in [false, true] {
                let mut body = vec![0xed; prefix];
                body.extend_from_slice(&[0, 0x0c, 0x9a]);
                let start = body.len();
                if width == 1 {
                    // First-coordinate 0d is -1. Its two X values are
                    // mirrored; the four row coordinates retain their signs.
                    body.extend_from_slice(&[0x0d, 0x0f, 0xe4, 0x0d, 0x0f, 0xe4]);
                } else {
                    // Head46 with zero tail states -2 in the first-coordinate
                    // lane and +2 in the row lane. All six occupy eight bytes.
                    for _ in 0..6 {
                        body.extend_from_slice(&[0x46, 0, 0, 0, 0, 0, 0, 0]);
                    }
                }
                let frame_end = body.len();
                if reference {
                    body.extend_from_slice(&[0xf7, 0x0c]);
                }
                let slots = first_coordinate_plane_corner_tokens(&body, &cache)
                    .expect("complete six-scalar corner suffix");
                let expected = if width == 1 { [1.0, 0.0, 1.0, 1.0, 0.0, 1.0] } else { [2.0; 6] };
                for (index, (value, first, end)) in slots.into_iter().enumerate() {
                    assert_eq!(value, expected[index]);
                    assert_eq!((first, end), (start + index * width, start + (index + 1) * width));
                }
                assert_eq!(slots[5].2, frame_end);
                body[prefix + 2] = 0x9b;
                assert!(first_coordinate_plane_corner_tokens(&body, &cache).is_none());
            }
        }
    }
}

#[test]
fn terminal_plane_corner_suffix_requires_exact_end_and_negative_stored_x_values() {
    let cache = ScalarCache::default();
    let body = [0, 0x0c, 0x9a, 0x0d, 0x0f, 0xe4, 0x0d, 0x0f, 0xe4];
    for end in 0..body.len() {
        assert!(first_coordinate_plane_corner_tokens(&body[..end], &cache).is_none());
    }
    for x in [3, 6] {
        let mut positive_x = body;
        positive_x[x] = 0xe4;
        assert!(first_coordinate_plane_corner_tokens(&positive_x, &cache).is_none());
    }
    for trailer in [&[0xe3][..], &[0xf7, 0x0d][..]] {
        let mut trailing = body.to_vec();
        trailing.extend_from_slice(trailer);
        assert!(first_coordinate_plane_corner_tokens(&trailing, &cache).is_none());
    }
}
