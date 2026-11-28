// SPDX-License-Identifier: Apache-2.0
//! The semantic writer omits the part stub of a quarantined record and names it.

use crate::IgesVersion;
use cadmpeg_ir::codec::write::target::TargetRequest;
use std::io::Cursor;

use cadmpeg_ir::codec::write::{EncodeInput, Encoder};
use cadmpeg_ir::codec::{Codec, DecodeOptions};
use cadmpeg_ir::report::export::WritePath;

use crate::test_support::test_owned::{owned_test_file, OwnedTestEntity};
use crate::IgesCodec;

/// A file whose second Directory Entry pair carries a non-integer Parameter Data pointer.
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
    let parameter = bytes
        .chunks_exact(81)
        .position(|line| line[72] == b'D')
        .map(|first| (first + 2) * 81 + 8)
        .expect("Directory card");
    bytes[parameter..parameter + 8].copy_from_slice(b"     abc");
    bytes
}

#[test]
fn a_quarantined_record_stub_is_written_as_an_omitted_native_record() {
    let decoded = IgesCodec
        .decode(
            &mut Cursor::new(quarantined_directory_file()),
            &DecodeOptions::default(),
        )
        .unwrap();
    crate::test_support::given_up_part_bytes(&decoded, ("directory-entry", 3));
    let stub = decoded.ir().native.namespace("iges").unwrap().arenas()["unknowns"][0]
        .id()
        .to_owned();

    let plan = IgesCodec
        .plan(
            EncodeInput::new(decoded.ir(), None).with_target(TargetRequest::Explicit(
                IgesVersion::V5_3.descriptor().id.as_str(),
            )),
        )
        .unwrap();
    assert!(matches!(
        plan.report().write_path(),
        WritePath::Synthesized { .. }
    ));
    let mut written = Vec::new();
    let report = plan.write_to(&mut written).unwrap();

    assert!(
        report.losses.iter().any(
            |loss| loss.code.to_string() == "export/native.records-omitted"
                && loss.message.contains("of `iges` arena `cards`")
                && loss.message.contains("cards of no typed entity")
        ),
        "{:?}",
        report.losses
    );
    // The decode charged the part, so no write loss names its stub.
    assert!(!report
        .losses
        .iter()
        .any(|loss| loss.message.contains(&stub)));
    assert!(!written.is_empty());

    let round_trip = IgesCodec
        .decode(&mut Cursor::new(written), &DecodeOptions::default())
        .unwrap();
    assert_eq!(round_trip.ir().geometry.points.len(), 1);
    assert!(!round_trip
        .ir()
        .native
        .namespace("iges")
        .unwrap()
        .arenas()
        .contains_key("unknowns"));
}
