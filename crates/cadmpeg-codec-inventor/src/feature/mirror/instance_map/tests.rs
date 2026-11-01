// SPDX-License-Identifier: Apache-2.0

use super::*;
use crate::layout::pm_dc_mirror_instance_map_single_tail_v19 as layout;
use crate::test_support::test_fixtures::{content_at, parse as with_context};

fn fixture() -> Vec<u8> {
    let mut bytes = content_at(1, 19, &[]);
    for value in [
        u32::MAX,
        0x3000_0006,
        1,
        0,
        0x3000_0002,
        0x8000_0011,
        0x3000_0002,
        1,
        1,
        0,
        0x8000_0012,
        2,
        1,
        0x8000_0013,
    ] {
        bytes.extend_from_slice(&value.to_le_bytes());
    }
    bytes
}

#[test]
fn single_entry_map_keeps_the_source_generated_node_and_owner() {
    let parsed = with_context(&fixture(), |ctx, source| parse(ctx, source, 19))
        .expect("map envelope")
        .expect("single-entry map");
    assert_eq!(
        parsed.kind,
        PmDcFeaturePropertyKind::Mirror {
            definition: super::super::Property::InstanceMap {
                source: PmDcReference::from_packed(0x8000_0011),
                result: PmDcReference::from_packed(0x8000_0012),
                owner: PmDcReference::from_packed(0x8000_0013),
            }
        }
    );
}

#[test]
fn single_entry_map_checks_every_fixed_control() {
    let original = fixture();
    let start = content_at(1, 19, &[]).len();
    for offset in [
        layout::STATE,
        layout::MAP_MARKER,
        layout::MAP_SIGNATURE,
        layout::MAP_COUNT,
        layout::MAP_VALUE,
        layout::SOURCE_TAG,
        layout::RESULT_MARKER,
        layout::RESULT_SIGNATURE,
        layout::RESULT_COUNT,
        layout::RESULT_METADATA0,
        layout::RESULT_METADATA1,
        layout::TERMINAL_VALUE,
        layout::OWNER_COUNT,
    ] {
        let mut changed = original.clone();
        changed[start + offset] ^= 1;
        assert!(with_context(&changed, |ctx, source| parse(ctx, source, 19))
            .expect("map envelope")
            .is_none());
    }
}

#[test]
fn single_entry_map_requires_distinct_nonnull_qualified_references() {
    let original = fixture();
    let start = content_at(1, 19, &[]).len();
    for offset in [layout::SOURCE, layout::RESULT, layout::OWNER] {
        for value in [0_u32, 0x8000_0000, 0x11] {
            let mut changed = original.clone();
            changed[start + offset..start + offset + 4].copy_from_slice(&value.to_le_bytes());
            assert!(with_context(&changed, |ctx, source| parse(ctx, source, 19))
                .expect("map envelope")
                .is_none());
        }
    }
    for (left, right) in [
        (layout::SOURCE, layout::RESULT),
        (layout::SOURCE, layout::OWNER),
        (layout::RESULT, layout::OWNER),
    ] {
        let mut changed = original.clone();
        changed.copy_within(start + left..start + left + 4, start + right);
        assert!(with_context(&changed, |ctx, source| parse(ctx, source, 19))
            .expect("map envelope")
            .is_none());
    }
}

#[test]
fn single_entry_map_requires_the_measured_version_and_exact_length() {
    let original = fixture();
    for len in 0..original.len() {
        assert!(!matches!(
            with_context(&original[..len], |ctx, source| parse(ctx, source, 19)),
            Ok(Some(_))
        ));
    }
    let mut extended = original.clone();
    extended.push(0);
    assert!(
        with_context(&extended, |ctx, source| parse(ctx, source, 19))
            .expect("map envelope")
            .is_none()
    );
    for version in [15, 18, 20, 21, 23, 25, 28, 29, 30, 31] {
        assert!(
            with_context(&original, |ctx, source| parse(ctx, source, version))
                .expect("version gate")
                .is_none()
        );
    }
}
