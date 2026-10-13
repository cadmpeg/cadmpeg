// SPDX-License-Identifier: Apache-2.0

use crate::scalar::ScalarCache;
use crate::surface::{decode_inline_selector_cylinder_envelope, SurfaceKind};

#[test]
fn selector_envelope_fixed_axes_are_free() {
    for (placeholders, accepted) in [
        (&[][..], true),
        (&[5][..], true),
        (&[1][..], false),
        (&[3, 5][..], false),
    ] {
        let mut body = vec![0x12];
        for (index, value) in [4.0_f64, -4.0, 2.0, 4.0, 2.0, -2.0, -4.0, -2.0]
            .into_iter()
            .enumerate()
        {
            if index == 1 {
                body.push(0x14);
            }
            if index >= 2 && placeholders.contains(&(index - 2)) {
                body.extend_from_slice(&[0xda, 0, 0, 0, 0, 0, 1]);
            } else {
                let raw = value.to_be_bytes();
                assert!(matches!(raw[0], 0x40 | 0xc0));
                body.push(if value > 0.0 { 0x2d } else { 0x46 });
                body.extend_from_slice(&raw[1..]);
            }
        }
        body.push(0xe3);
        for kind in [SurfaceKind::Cylinder, SurfaceKind::Plane] {
            let envelope =
                decode_inline_selector_cylinder_envelope(kind, &body, &ScalarCache::default());
            if kind == SurfaceKind::Cylinder && accepted {
                let envelope = envelope.expect("at most one absent radial span");
                assert_eq!(envelope.axial, [-4.0, 4.0]);
                assert_eq!(envelope.corners[0], [Some(-2.0), Some(-4.0), Some(-2.0)]);
                assert_eq!(
                    envelope.corners[1],
                    [
                        Some(2.0),
                        Some(4.0),
                        if placeholders.is_empty() {
                            Some(2.0)
                        } else {
                            None
                        }
                    ]
                );
                assert_eq!(envelope.close, body.len() - 1);
            } else {
                assert!(envelope.is_none());
            }
        }
    }
}
