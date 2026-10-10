// SPDX-License-Identifier: Apache-2.0

use super::super::{named_record_boundary, named_record_length, SurfaceKind};
use crate::scalar::ScalarCache;
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
use cadmpeg_core::CodecError;

fn check_name(payload: &[u8], offset: usize, expected: Option<usize>) {
    assert_eq!(named_record_length(payload, offset), expected);
}

#[test]
fn surface_name_prefix_uses_bounded_grammar() {
    for length in [0, 1, 2, 17, 95, 96, 120] {
        let mut payload = b"\xe0\x24".to_vec();
        payload.extend(std::iter::repeat_n(b'A', length));
        check_name(&payload, 0, None);
        payload.push(0);
        check_name(&payload, 0, (1..96).contains(&length).then_some(length + 3));
    }
    check_name(b"\xe0\0_ignored\0", 0, None);
    check_name(b"\xe0\0A-invalid\0", 0, None);
    check_name(b"\xe0\0A_2(x)\0ignored", 0, Some(9));
    check_name(b"\xe0\x25A\0", 0, None);
    check_name(b"\xe0", 0, None);
    check_name(b"", usize::MAX, None);
}

#[test]
fn surface_named_boundary_admits_visited_positions_and_skips_scalar_interiors() {
    let cache = ScalarCache::from_section(&[]);
    for (payload, offset) in [
        (b"".as_slice(), None),
        (b"\xff\xff\xff".as_slice(), None),
        (b"\xe0\0A\0ignored".as_slice(), Some(0)),
        (b"\xff\xff\xe0\0A\0ignored".as_slice(), Some(2)),
        // One eight-byte scalar, followed by the named field. Its interior is not visited.
        (b"\x46\0\0\0\0\0\0\0\xe0\0A\0ignored".as_slice(), Some(8)),
    ] {
        let operations = if payload.is_empty() {
            &[][..]
        } else {
            &["creo surface named boundary traversal"][..]
        };
        let actual = crate::test_support::assert_refusal_order(
            ResourceDimension::WorkUnits,
            operations,
            |cap| {
                let arena = DecodeArena::new();
                let mut policy = DecodePolicy::service();
                policy.limits.max_work_units = cap;
                policy.limits.max_materialized_bytes = 0;
                policy.limits.max_retained_bytes = 0;
                policy.limits.max_collection_items = 0;
                let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("root");
                let result = named_record_boundary(&ctx, SurfaceKind::Plane, payload, &cache);
                if let Err(CodecError::ResourceLimit(resource)) = &result {
                    assert_eq!(
                        (
                            resource.dimension,
                            resource.used,
                            resource.additional,
                            resource.operation
                        ),
                        (
                            ResourceDimension::WorkUnits,
                            cap,
                            1,
                            "creo surface named boundary traversal"
                        )
                    );
                }
                result
            },
        );
        assert_eq!(actual, offset);
    }
}
