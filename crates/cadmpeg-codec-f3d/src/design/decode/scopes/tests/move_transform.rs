// SPDX-License-Identifier: Apache-2.0

use crate::design::decode::scopes::direct_face::exact_move_operation;
use crate::records::feature::scope::{DesignFeatureKind, DesignParameterScope};

fn fixture(class: &str, length: usize) -> (Vec<u8>, DesignParameterScope) {
    let mut bytes = vec![0; length];
    bytes[..4].copy_from_slice(&3u32.to_le_bytes());
    bytes[4..7].copy_from_slice(class.as_bytes());
    bytes[7..11].copy_from_slice(&71u32.to_le_bytes());
    let form: u32 = if class == "452" { 5 } else { 1 };
    bytes[43..47].copy_from_slice(&form.to_le_bytes());
    let transform: [[f64; 4]; 4] = [
        [0., -1., 0., 7.],
        [1., 0., 0., 11.],
        [0., 0., 1., 13.],
        [0., 0., 0., 1.],
    ];
    for (cell, value) in transform.into_iter().flatten().enumerate() {
        let at = 48 + cell * 8;
        bytes[at..at + 8].copy_from_slice(&value.to_le_bytes());
    }
    bytes.extend_from_slice(&3u32.to_le_bytes());
    bytes.extend_from_slice(b"262");
    bytes.extend_from_slice(&71u32.to_le_bytes());
    let mut scope = DesignParameterScope::empty("f3d:test:scope#17", DesignFeatureKind::Move, 17);
    scope
        .try_edit(|draft| {
            draft.reference_members = crate::records::identity::ReferenceRun::unlocated(vec![71]);
            draft.layout_fixture_references();
            draft.layout_fixture_tail();
        })
        .unwrap();
    (bytes, scope)
}

#[test]
fn move_transform_added_classes_decode_exact_matrix() {
    for (class, length) in [
        ("330", 273),
        ("273", 273),
        ("408", 273),
        ("372", 273),
        ("289", 273),
        ("394", 273),
        ("406", 273),
        ("282", 273),
        ("403", 273),
        ("391", 273),
        ("452", 253),
    ] {
        let (bytes, scope) = fixture(class, length);
        let decoded = exact_move_operation(
            &bytes,
            &crate::design::test_support::indexed_record_offsets_for_test(&bytes),
            &scope,
        )
        .unwrap();
        assert_eq!(
            decoded.transform.rows(),
            [
                [0., -1., 0., 7.],
                [1., 0., 0., 11.],
                [0., 0., 1., 13.],
                [0., 0., 0., 1.],
            ]
        );
        assert_eq!(u32::from(decoded.form), if class == "452" { 5 } else { 1 });
        assert_eq!(decoded.form_offset, Some(43));
        assert_eq!(decoded.transform_offset, Some(48));
        assert_eq!(decoded.transform_record_index, 71);
    }
}

#[test]
fn move_transform_refuses_wrong_length_controls_matrix_and_pair() {
    let (bytes, scope) = fixture("330", 273);
    let mut variants = Vec::new();
    for (at, value) in [(11, 1), (43, 2), (47, 1)] {
        let mut changed = bytes.clone();
        changed[at] = value;
        variants.push(changed);
    }
    let mut changed = bytes.clone();
    changed[168..176].copy_from_slice(&2.0f64.to_le_bytes());
    variants.push(changed);
    variants.push(fixture("330", 253).0);
    variants.push(fixture("330", 274).0);
    let mut changed = bytes.clone();
    changed[43..47].copy_from_slice(&5u32.to_le_bytes());
    variants.push(changed);
    let mut changed = fixture("452", 253).0;
    changed[43..47].copy_from_slice(&1u32.to_le_bytes());
    variants.push(changed);
    let mut changed = fixture("452", 253).0;
    changed[257..260].copy_from_slice(b"258");
    variants.push(changed);
    variants.push(fixture("999", 273).0);
    for bytes in variants {
        assert!(exact_move_operation(
            &bytes,
            &crate::design::test_support::indexed_record_offsets_for_test(&bytes),
            &scope,
        )
        .is_none());
    }
}
