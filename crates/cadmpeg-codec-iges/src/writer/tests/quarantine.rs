// SPDX-License-Identifier: Apache-2.0
//! The semantic writer admits both quarantine arenas as native passthrough.
#![allow(clippy::unwrap_used)]

use crate::IgesVersion;
use cadmpeg_ir::codec::TargetRequest;
use std::io::Cursor;

use cadmpeg_ir::codec::{Codec, DecodeOptions, EncodeInput, Encoder};
use cadmpeg_ir::report::WritePath;

use crate::loss::IgesLossCode;
use crate::test_support::{owned_test_file, OwnedTestEntity};
use crate::IgesCodec;

/// A file whose second Directory Entry pair carries a non-integer level field.
fn quarantined_directory_file() -> Vec<u8> {
    let mut bytes = owned_test_file(&[
        OwnedTestEntity {
            entity_type: 116,
            form: 0,
            label: "POINT".into(),
            status: "00000000",
            parameters: "116,1,2,3,0;".into(),
        },
        OwnedTestEntity {
            entity_type: 116,
            form: 0,
            label: "BROKEN".into(),
            status: "00000000",
            parameters: "116,4,5,6,0;".into(),
        },
    ]);
    let level = bytes
        .chunks_exact(81)
        .position(|line| line[72] == b'D')
        .map(|first| (first + 2) * 81 + 4 * 8)
        .expect("Directory card");
    bytes[level..level + 8].copy_from_slice(b"     abc");
    bytes
}

#[test]
fn a_quarantine_arena_is_written_as_an_omitted_passthrough_arena() {
    let decoded = IgesCodec
        .decode(
            &mut Cursor::new(quarantined_directory_file()),
            &DecodeOptions::default(),
        )
        .unwrap();
    assert_eq!(
        decoded.ir().native.namespace("iges").unwrap().arenas["quarantined_directory_records"]
            .len(),
        1
    );

    let plan = IgesCodec
        .plan(
            EncodeInput::new(decoded.ir(), None),
            TargetRequest::Explicit(IgesVersion::V5_3.descriptor().id.as_str()),
        )
        .unwrap();
    assert_eq!(plan.write_path(), WritePath::Synthesized);
    let mut written = Vec::new();
    let report = plan.write_to(&mut written).unwrap();

    let passthrough = report
        .losses
        .iter()
        .filter(|loss| loss.code == IgesLossCode::PassthroughRecordOmitted.kind())
        .collect::<Vec<_>>();
    assert!(passthrough
        .iter()
        .any(|loss| loss.message.contains("quarantined_directory_records")));
    assert!(!written.is_empty());

    let round_trip = IgesCodec
        .decode(&mut Cursor::new(written), &DecodeOptions::default())
        .unwrap();
    assert_eq!(round_trip.ir().model.points.len(), 1);
    assert!(round_trip.ir().native.namespace("iges").unwrap().arenas
        ["quarantined_directory_records"]
        .is_empty());
}
