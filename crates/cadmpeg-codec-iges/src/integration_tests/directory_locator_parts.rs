// SPDX-License-Identifier: Apache-2.0
use cadmpeg_ir::NativeUnknownRecord;

use crate::test_support::test_owned::{owned_test_file, OwnedTestEntity};

fn points() -> Vec<u8> {
    owned_test_file(&[
        OwnedTestEntity {
            entity_type: 116,
            form: 0,
            label: "FIRST".into(),
            status: "00000000",
            parameters: "116,1,2,3,0;".into(),
        },
        OwnedTestEntity {
            entity_type: 116,
            form: 0,
            label: "SECOND".into(),
            status: "00000000",
            parameters: "116,4,5,6,0;".into(),
        },
    ])
}

fn section(bytes: &[u8], code: u8) -> usize {
    bytes
        .chunks_exact(81)
        .position(|card| card[72] == code)
        .expect("fixture contains the actual section")
        * 81
}

#[test]
fn unlocatable_parameter_data_keeps_its_readable_directory_entry_and_the_peer() {
    let original = points();
    let baseline = crate::test_support::decode(original.clone());
    let directory = section(&original, b'D') + 162;
    let parameter = section(&original, b'P');
    let terminate = section(&original, b'T');
    let mut bytes = original;
    bytes[directory + 8..directory + 16].copy_from_slice(b"       0");
    bytes[parameter + 81 + 64..parameter + 81 + 72].copy_from_slice(b"      99");
    let decoded = crate::test_support::decode(bytes.clone());
    assert_eq!(decoded.ir().model.points.len(), 1);
    assert_eq!(decoded.ir().model.vertices.len(), 1);
    assert_eq!(
        serde_json::to_value(&decoded.ir().model.points[0]).expect("serialize current point"),
        serde_json::to_value(&baseline.ir().model.points[0]).expect("serialize baseline point")
    );
    assert_eq!(
        serde_json::to_value(&decoded.ir().model.vertices[0]).expect("serialize current vertex"),
        serde_json::to_value(&baseline.ir().model.vertices[0]).expect("serialize baseline vertex")
    );
    assert_eq!(
        decoded.ir().source.as_ref().expect("source").attributes,
        baseline
            .ir()
            .source
            .as_ref()
            .expect("baseline source")
            .attributes
    );
    let native = decoded.ir().native.namespace("iges").expect("IGES native");
    let old = baseline
        .ir()
        .native
        .namespace("iges")
        .expect("baseline native");
    // #215 rule 1: the Directory Entry keeps the fields it read; only its
    // Parameter Data is absent.
    let mut expected = serde_json::to_value(&old.arenas()["entities"]).expect("baseline entities");
    expected[1]["parameters"] = serde_json::json!([]);
    expected[1]["comment"] = serde_json::to_value(cadmpeg_ir::native::bytes::NativeBytes::from(
        Vec::<u8>::new(),
    ))
    .expect("empty comment");
    assert_eq!(
        serde_json::to_value(&native.arenas()["entities"]).expect("current entities"),
        expected
    );
    assert!(native
        .arenas()
        .get("quarantined_directory_records")
        .is_none_or(Vec::is_empty));
    let fidelity = decoded.source_fidelity();
    // The unlocated Parameter Data is a Parameter-section failure: every
    // given-up part lies inside the Parameter section.
    let parameter_section =
        u64::try_from(parameter).expect("offset")..u64::try_from(terminate).expect("offset");
    assert_eq!(
        crate::test_support::given_up_count(decoded.report(), "iges.directory_entry"),
        0
    );
    for part in crate::test_support::given_up_parts(fidelity) {
        assert!(!part.untyped().is_empty());
        assert!(parameter_section.start <= part.offset());
        assert!(part.offset() + part.byte_len() <= parameter_section.end);
    }
    assert!(!decoded
        .report()
        .losses
        .iter()
        .any(|loss| loss.message.contains("cannot locate Parameter Data")));
    assert!(cadmpeg_ir::validate_neutral(decoded.ir(), Vec::new())
        .expect("neutral validation")
        .is_ok());
}

#[test]
fn a_real_back_pointer_run_admits_a_directory_with_an_unusable_declaration() {
    let original = points();
    let baseline = crate::test_support::decode(original.clone());
    let directory = section(&original, b'D') + 162;
    let mut bytes = original;
    bytes[directory + 8..directory + 16].copy_from_slice(b"       0");
    let decoded = crate::test_support::decode(bytes);
    assert_eq!(
        serde_json::to_value(&decoded.ir().model.points).expect("current points"),
        serde_json::to_value(&baseline.ir().model.points).expect("baseline points")
    );
    let native = decoded.ir().native.namespace("iges").expect("IGES native");
    assert_eq!(native.arenas()["entities"].len(), 2);
    assert!(native
        .arena_as::<NativeUnknownRecord>("unknowns")
        .expect("typed unknowns")
        .is_empty());
    assert!(!decoded
        .report()
        .losses
        .iter()
        .any(|loss| loss.code.local_code() == "record.unreadable"));
}
