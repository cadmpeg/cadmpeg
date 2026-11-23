// SPDX-License-Identifier: Apache-2.0

use super::{push_marked_reference, rectangular_pattern_direction_frame};

fn frame() -> Vec<u8> {
    let mut bytes = rectangular_pattern_direction_frame(8, [0.0, 0.0, 2.075]);
    bytes[89..93].copy_from_slice(&7u32.to_le_bytes());
    bytes.drain(93..117);
    let mut links = Vec::new();
    push_marked_reference(&mut links, 27);
    push_marked_reference(&mut links, 30);
    bytes.splice(108..110, links);
    bytes
}

#[test]
fn linked_spacing_direction_keeps_its_displacement_and_adjacent_distance_form() {
    let bytes = frame();
    assert_eq!(bytes.len(), 227);
    let direction = super::super::exact_rectangular_pattern_direction(&bytes, 0, 216, 20, 23, 10)
        .expect("linked spacing direction");
    assert_eq!(direction.distance_type, 7);
    assert_eq!(direction.selection_record_index, 23);
    assert_eq!(direction.origin_offset, 25);
    assert_eq!(direction.displacement_offset, 49);
    assert_eq!(
        direction.displacement,
        crate::test_support::reals([0.0, 0.0, 2.075])
    );
    assert_eq!(
        *direction.direction().unwrap().as_raw(),
        cadmpeg_ir::math::Vector3::new(0.0, 0.0, 1.0)
    );
}

#[test]
fn linked_spacing_direction_checks_both_links_and_the_shifted_tail() {
    let bytes = frame();
    let parse =
        |bytes: &[u8]| super::super::exact_rectangular_pattern_direction(bytes, 0, 216, 20, 23, 10);
    for offset in [
        11, 73, 89, 93, 97, 98, 108, 119, 130, 134, 145, 153, 163, 175, 179, 180, 190, 191, 193,
        194, 204, 205, 206, 216, 223,
    ] {
        let mut corrupt = bytes.clone();
        corrupt[offset] ^= 1;
        assert!(parse(&corrupt).is_none(), "offset {offset}");
    }
    for len in 0..bytes.len() {
        assert!(parse(&bytes[..len]).is_none());
    }
    assert!(
        super::super::exact_rectangular_pattern_direction(&bytes, 0, 216, 20, 24, 10).is_none()
    );
}

#[test]
fn linked_direction_selection_links_stay_native_with_a_pinned_loss() {
    let bytes = frame();
    let direction =
        super::super::exact_rectangular_pattern_direction(&bytes, 0, 216, 20, 23, 10).unwrap();
    let records = crate::design::decode::sketch::IndexedRecordOffsets::build(&bytes);
    let losses =
        super::super::linked_direction_losses(&bytes, &records, "f3d:scope#10", &[direction]);
    assert_eq!(losses.len(), 1);
    assert_eq!(
        losses[0].code,
        crate::loss::F3dLossCode::FeatureRectangularPatternDirectionLinksNative.kind()
    );
    assert!(losses[0].message.contains("27 and 30"));
}

#[test]
fn compact_unflagged_extent_direction_keeps_the_unit_axis() {
    let mut bytes = rectangular_pattern_direction_frame(16, [0.0, 0.0, 1.0]);
    bytes.drain(93..117);
    bytes.remove(170);
    let direction = super::super::exact_rectangular_pattern_direction(&bytes, 0, 195, 20, 23, 10)
        .expect("unflagged compact extent");
    assert_eq!(direction.distance_type, 8);
    assert_eq!(
        *direction.direction().unwrap().as_raw(),
        cadmpeg_ir::math::Vector3::new(0.0, 0.0, 1.0)
    );
}

#[test]
fn linked_extent_direction_reads_both_selection_kinds_and_the_unflagged_tail() {
    for selection_kind in [8_u32, 16] {
        let mut bytes = frame();
        bytes[21..25].copy_from_slice(&selection_kind.to_le_bytes());
        bytes[49..73].fill(0);
        bytes[49..57].copy_from_slice(&(-1.0_f64).to_le_bytes());
        bytes[89..93].copy_from_slice(&8_u32.to_le_bytes());
        bytes.remove(190);
        let parse = |bytes: &[u8]| {
            super::super::exact_rectangular_pattern_direction(bytes, 0, 215, 20, 23, 10)
        };
        let direction = parse(&bytes).expect("linked extent direction");
        assert_eq!(direction.distance_type, 8);
        assert_eq!(
            *direction.direction().unwrap().as_raw(),
            cadmpeg_ir::math::Vector3::new(-1.0, 0.0, 0.0)
        );
        for offset in [
            11, 73, 89, 93, 97, 98, 108, 119, 130, 134, 145, 153, 163, 175, 179, 180, 190, 191,
            192, 193, 203, 204, 205, 215, 222,
        ] {
            let mut corrupt = bytes.clone();
            corrupt[offset] ^= 1;
            assert!(
                parse(&corrupt).is_none(),
                "kind {selection_kind}, offset {offset}"
            );
        }
        for len in 0..bytes.len() {
            assert!(parse(&bytes[..len]).is_none());
        }
        let records = crate::design::decode::sketch::IndexedRecordOffsets::build(&bytes);
        let losses =
            super::super::linked_direction_losses(&bytes, &records, "f3d:scope#10", &[direction]);
        assert_eq!(losses.len(), 1);
        assert_eq!(
            losses[0].code,
            crate::loss::F3dLossCode::FeatureRectangularPatternDirectionLinksNative.kind()
        );
    }
}
