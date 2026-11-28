// SPDX-License-Identifier: Apache-2.0

use crate::test_support::local_reference;

#[test]
fn plain_local_path_closure_preserves_a_nested_four_element_path() {
    let elements = [(10_u64, 0_u32), (20, 0), (30, 1), (40, 0)];
    let matrix = [
        [1.0_f64, 0.0, 0.0, 0.5],
        [0.0, 1.0, 0.0, 0.25],
        [0.0, 0.0, 1.0, 2.0],
        [0.0, 0.0, 0.0, 1.0],
    ];
    let mut bytes = Vec::new();
    bytes.extend_from_slice(&3_u32.to_le_bytes());
    bytes.extend_from_slice(b"315");
    bytes.extend_from_slice(&50_u64.to_le_bytes());
    bytes.extend_from_slice(&0_u32.to_le_bytes());
    bytes.push(1);
    bytes.extend_from_slice(&4_u32.to_le_bytes());
    for (record, discriminator) in elements {
        bytes.extend(local_reference(record));
        bytes.extend_from_slice(&discriminator.to_le_bytes());
    }
    bytes.extend([0, 0]);
    for value in matrix.into_iter().flatten() {
        bytes.extend_from_slice(&value.to_le_bytes());
    }
    bytes.extend_from_slice(&3_u32.to_le_bytes());
    for record in [20, 10, 40, 60, 70] {
        bytes.extend(local_reference(record));
    }
    let magic = Some(crate::metastream::MODERN_SERIALIZER_MAGIC);
    let parsed = super::super::component_path_carrier(&bytes, magic).unwrap();
    assert_eq!(parsed.elements, elements);
    assert_eq!(parsed.transform, Some(matrix));
    assert!(!super::super::local_component_path_unadmitted(
        &bytes, magic
    ));
    bytes.push(0);
    assert!(super::super::component_path_carrier(&bytes, magic).is_none());
}
