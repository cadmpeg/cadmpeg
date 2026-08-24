// SPDX-License-Identifier: Apache-2.0
#![allow(
    unused_imports,
    clippy::cloned_ref_to_slice_refs,
    clippy::default_trait_access,
    clippy::trivially_copy_pass_by_ref,
    clippy::uninlined_format_args,
    clippy::wildcard_imports
)]
use super::prelude::*;

const EPS_SCALE_VALUE: f64 = 1e-12;

#[test]
fn legacy_scale_resolves_explicit_point_data_center() {
    for extra_reference in [false, true] {
        let (bytes, scope, position_at) = legacy_scale_fixture(extra_reference);
        let records = IndexedRecordOffsets::build(&bytes);
        let operation = exact_scale_operation(&bytes, &records, &scope, &HashMap::new())
            .expect("legacy Scale operation");

        assert_eq!(operation.body_group_record_index, 102);
        assert_eq!(operation.center_record_index, 105);
        assert_eq!(operation.center_position_offset, Some(position_at as u64));
        assert_eq!(operation.uniform_factor_offset, 21);
        assert!((operation.uniform_factor - 2.5).abs() < EPS_SCALE_VALUE);

        let position = operation.center_position.expect("point-data center");
        for (actual, expected) in position.into_iter().zip([1.25, -2.5, 3.75]) {
            assert!((actual - expected).abs() < EPS_SCALE_VALUE);
        }
    }
}

#[test]
fn modern_localized_scale_resolves_explicit_point_data_center() {
    let (bytes, scope, position_at) = modern_scale_fixture();
    let records = IndexedRecordOffsets::build(&bytes);
    let operation = exact_scale_operation(&bytes, &records, &scope, &HashMap::new())
        .expect("modern localized Scale operation");

    assert_eq!(operation.body_group_record_index, 102);
    assert_eq!(operation.center_record_index, 105);
    assert_eq!(operation.center_position_offset, Some(position_at as u64));
    assert_eq!(operation.uniform_factor_offset, 25);
    assert!((operation.uniform_factor - 2.5).abs() < EPS_SCALE_VALUE);

    let position = operation.center_position.expect("point-data center");
    for (actual, expected) in position.into_iter().zip([1.25, -2.5, 3.75]) {
        assert!((actual - expected).abs() < EPS_SCALE_VALUE);
    }
}

fn modern_scale_fixture() -> (Vec<u8>, DesignParameterScope, usize) {
    let mut bytes = vec![0; 317];
    bytes[20..24].copy_from_slice(&1u32.to_le_bytes());
    bytes[25..33].copy_from_slice(&2.5f64.to_le_bytes());
    for (offset, record_index) in [(33, 105u32), (44, 101), (68, 102)] {
        bytes[offset] = 1;
        bytes[offset + 1..offset + 5].copy_from_slice(&record_index.to_le_bytes());
    }
    bytes[55..59].copy_from_slice(&1u32.to_le_bytes());
    bytes[60..64].copy_from_slice(&1u32.to_le_bytes());
    bytes[64..68].copy_from_slice(&1u32.to_le_bytes());

    let position_at = append_point_data(&mut bytes, 105, [104, 103]);
    let mut scope = DesignParameterScope::empty("generated:scale#100", "Maßstab", 100);
    scope.frame_length = 317;
    scope.reference_members = vec![101, 102, 103, 104, 105];
    (bytes, scope, position_at)
}

fn legacy_scale_fixture(extra_reference: bool) -> (Vec<u8>, DesignParameterScope, usize) {
    let frame_length = if extra_reference { 318 } else { 307 };
    let reference_members = if extra_reference {
        vec![101, 102, 103, 104, 106, 105]
    } else {
        vec![101, 102, 103, 104, 105]
    };
    let mut bytes = vec![0; frame_length];
    bytes[21..29].copy_from_slice(&2.5f64.to_le_bytes());
    for (offset, record_index) in [(29, 105u32), (40, 101), (64, 102)] {
        bytes[offset] = 1;
        bytes[offset + 1..offset + 5].copy_from_slice(&record_index.to_le_bytes());
    }
    bytes[51..55].copy_from_slice(&1u32.to_le_bytes());
    bytes[56..60].copy_from_slice(&1u32.to_le_bytes());
    bytes[60..64].copy_from_slice(&1u32.to_le_bytes());

    let position_at = append_point_data(&mut bytes, 105, [104, 103]);
    let mut scope = DesignParameterScope::empty("generated:scale#100", "Scale", 100);
    scope.frame_length = frame_length as u64;
    scope.reference_members = reference_members;
    (bytes, scope, position_at)
}

fn append_point_data(bytes: &mut Vec<u8>, record_index: u32, input_records: [u32; 2]) -> usize {
    let point_at = bytes.len();
    bytes.extend_from_slice(&3u32.to_le_bytes());
    bytes.extend_from_slice(b"282");
    bytes.extend_from_slice(&record_index.to_le_bytes());
    bytes.extend_from_slice(&0u32.to_le_bytes());
    bytes.extend_from_slice(&[0; 27]);
    let position_at = bytes.len();
    for value in [1.25f64, -2.5, 3.75] {
        bytes.extend_from_slice(&value.to_le_bytes());
    }
    bytes.extend_from_slice(&7u32.to_le_bytes());
    for _ in 0..3 {
        bytes.extend_from_slice(&(-1.0f64).to_le_bytes());
    }
    bytes.extend_from_slice(&2u32.to_le_bytes());
    for target in input_records {
        bytes.push(1);
        bytes.extend_from_slice(&u64::from(target).to_le_bytes());
        bytes.extend_from_slice(&[0, 0]);
    }
    bytes.resize(point_at + 208, 0);
    bytes.extend_from_slice(&3u32.to_le_bytes());
    bytes.extend_from_slice(b"259");
    bytes.extend_from_slice(&record_index.to_le_bytes());
    position_at
}
