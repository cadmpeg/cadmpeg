// SPDX-License-Identifier: Apache-2.0
use cadmpeg_ir::up_axis::UpAxis;

use super::{apply, read_stream, UpAxisRead, Y_UP_PAIR, Z_UP_PAIR};
use crate::layout::base_feature_asset_settings as settings;

/// An `AssetSettings` record at stream offset `prefix` with the given pairs.
fn stream(prefix: usize, first: ([f64; 3], [f64; 3]), second: ([f64; 3], [f64; 3])) -> Vec<u8> {
    let mut record = settings::TEMPLATE.to_vec();
    record[..4].copy_from_slice(&3u32.to_le_bytes());
    record[4..7].copy_from_slice(b"001");
    record[7..11].copy_from_slice(&0u32.to_le_bytes());
    for (at, vector) in [
        (settings::FRONT_A, first.0),
        (settings::UP_A, first.1),
        (settings::FRONT_B, second.0),
        (settings::UP_B, second.1),
    ] {
        for (slot, value) in vector.iter().enumerate() {
            record[at + 8 * slot..at + 8 * slot + 8].copy_from_slice(&value.to_le_bytes());
        }
    }
    let mut out = vec![0xAA; prefix];
    out.extend_from_slice(&record);
    out
}

#[test]
fn the_z_up_form_reads_as_z() {
    let bytes = stream(0, Z_UP_PAIR, Z_UP_PAIR);
    assert_eq!(read_stream(&bytes), Some(UpAxisRead::Stated(UpAxis::Z)));
}

#[test]
fn the_y_up_form_reads_as_y_at_a_stream_offset() {
    let bytes = stream(37, Y_UP_PAIR, Y_UP_PAIR);
    assert_eq!(read_stream(&bytes), Some(UpAxisRead::Stated(UpAxis::Y)));
}

#[test]
fn a_negative_zero_component_is_a_zero_component() {
    let y_up = ([-0.0, 0.0, -1.0], [-0.0, 1.0, -0.0]);
    let bytes = stream(0, y_up, Y_UP_PAIR);
    assert_eq!(read_stream(&bytes), Some(UpAxisRead::Stated(UpAxis::Y)));
}

#[test]
fn disagreeing_copies_state_no_axis() {
    let bytes = stream(0, Z_UP_PAIR, Y_UP_PAIR);
    assert!(matches!(
        read_stream(&bytes),
        Some(UpAxisRead::Unrecognized {
            copies_agree: false,
            ..
        })
    ));
}

#[test]
fn a_pair_that_is_neither_form_states_no_axis() {
    let other = ([1.0, 0.0, 0.0], [0.0, 0.0, 1.0]);
    let bytes = stream(0, other, other);
    assert_eq!(
        read_stream(&bytes),
        Some(UpAxisRead::Unrecognized {
            front: other.0,
            up: other.1,
            copies_agree: true,
        })
    );
    let mixed = ([0.0, 1.0, 0.0], [0.0, 1.0, 0.0]);
    assert!(matches!(
        read_stream(&stream(0, mixed, mixed)),
        Some(UpAxisRead::Unrecognized { .. })
    ));
}

#[test]
fn a_stream_without_the_record_or_a_short_record_reads_nothing() {
    assert_eq!(read_stream(&[0u8; 600]), None);
    let mut bytes = stream(0, Z_UP_PAIR, Z_UP_PAIR);
    bytes.truncate(settings::UP_B + 8);
    assert_eq!(read_stream(&bytes), None);
}

#[test]
fn apply_sets_the_axis_or_charges_a_loss() {
    let mut axis = None;
    let mut losses = Vec::new();
    apply(UpAxisRead::Absent, &mut axis, &mut losses);
    assert_eq!((axis, losses.len()), (None, 0));
    apply(UpAxisRead::Stated(UpAxis::Y), &mut axis, &mut losses);
    assert_eq!((axis, losses.len()), (Some(UpAxis::Y), 0));

    let mut axis = None;
    let bytes = stream(0, Z_UP_PAIR, Y_UP_PAIR);
    apply(read_stream(&bytes).unwrap(), &mut axis, &mut losses);
    assert_eq!(axis, None);
    assert_eq!(losses.len(), 1);
    assert_eq!(
        losses[0].code,
        crate::loss::F3dLossCode::SettingsUpAxisUnrecognized.kind()
    );
}
