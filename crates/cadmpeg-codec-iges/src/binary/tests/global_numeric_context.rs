// SPDX-License-Identifier: Apache-2.0
//! The Binary Flag Section supersedes Global fields 7 through 11 (IGES 5.3
//! Appendix H.2.3): their values do not change a Binary decode.

use std::io::Cursor;

use cadmpeg_ir::codec::{Codec, DecodeOptions};

use super::{binary_file, NEUTRAL_NUMERIC_FIELDS};

/// Global fields 7 through 11 that contradict the Binary primitive widths:
/// a 4-bit integer, a single-precision magnitude below the coordinates, a
/// zero single-precision significance, a negative double-precision
/// magnitude, and a negative double-precision significance.
const CONTRADICTORY_NUMERIC_FIELDS: [i64; 5] = [4, -5, 0, -9, -1];

fn decode(numeric_fields: [i64; 5]) -> cadmpeg_ir::codec::DecodeResult {
    crate::IgesCodec
        .decode(
            &mut Cursor::new(binary_file(b"binary fixture", numeric_fields)),
            &DecodeOptions::default(),
        )
        .expect("Binary point decodes")
}

#[test]
fn contradictory_binary_global_numeric_fields_decode_as_neutral_ones() {
    let neutral = decode(NEUTRAL_NUMERIC_FIELDS);
    let contradictory = decode(CONTRADICTORY_NUMERIC_FIELDS);
    assert_eq!(contradictory.ir().model.points.len(), 1);
    assert_eq!(
        serde_json::to_value(&contradictory.ir().model).expect("model JSON"),
        serde_json::to_value(&neutral.ir().model).expect("model JSON")
    );
    assert_eq!(
        serde_json::to_value(contradictory.ir().tolerances).expect("tolerance JSON"),
        serde_json::to_value(neutral.ir().tolerances).expect("tolerance JSON")
    );
    assert_eq!(
        serde_json::to_value(&contradictory.report().losses).expect("loss JSON"),
        serde_json::to_value(&neutral.report().losses).expect("loss JSON")
    );
}

#[test]
fn no_binary_loss_names_global_fields_7_through_11() {
    for numeric_fields in [NEUTRAL_NUMERIC_FIELDS, CONTRADICTORY_NUMERIC_FIELDS] {
        let decoded = decode(numeric_fields);
        for loss in &decoded.report().losses {
            for field in 7..=11 {
                assert!(
                    !loss.message.contains(&format!("Global field {field} ")),
                    "{}",
                    loss.message
                );
            }
        }
    }
}
