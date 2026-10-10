// SPDX-License-Identifier: Apache-2.0


use crate::scalar::ScalarCache;
use crate::surface::{scalar_tokens, SurfaceKind};

#[test]
fn surface_scalar_dispatch_admits_only_visited_unknown_offsets_and_no_terminal_probe() {
    for code in [0x22, 0x24, 0x25, 0x26, 0x28, 0x29, 0x2a, 0x2c] {
        let kind = SurfaceKind::from_byte(code).expect("surface family");
        for count in [0, 1, 7, 17, 257] {
            let body = vec![0xed; count];
            assert!(super::work_output(|ctx| scalar_tokens(ctx, kind, &body, &ScalarCache::default())).is_empty());
        }
    }
}

#[test]
fn surface_scalar_dispatch_skips_scalar_interior_bytes_and_keeps_raw_identity() {
    // Head46 replaces the high IEEE byte 0x40. The low seven bytes belong
    // to this scalar even when they look like scalar heads or delimiters.
    let raw = [0x46, 0x00, 0xe3, 0xe0, 0xf7, 0xe4, 0x0f, 0x18];
    let expected = f64::from_bits(0x4000_e3e0_f7e4_0f18);
    for prefix in [0, 1, 7, 17, 257] {
        for suffix in [0, 1, 7, 17, 257] {
            let mut body = vec![0xed; prefix];
            body.extend_from_slice(&raw);
            body.extend(std::iter::repeat_n(0xed, suffix));
            let tokens = super::work_output(|ctx| scalar_tokens(ctx, SurfaceKind::Cylinder, &body, &ScalarCache::default()));
            assert_eq!(tokens.len(), 1);
            assert_eq!(tokens[0].value, Some(expected));
            assert_eq!(tokens[0].raw, raw);
            assert_eq!(tokens[0].offset, prefix);

        }
    }
}

#[test]
fn surface_scalar_dispatch_refuses_before_copy_or_token_storage() {
    let raw = [0x46, 0, 0, 0, 0, 0, 0, 0];
    for prefix in [0, 1, 7, 17, 257] {
        let mut body = vec![0xed; prefix];
        body.extend_from_slice(&raw);
        crate::test_support::assert_work_boundaries(&[
            "creo surface scalar token dispatch", "creo surface scalar token bytes",
        ], |ctx| scalar_tokens(ctx, SurfaceKind::Cylinder, &body, &ScalarCache::default()));
    }
}
