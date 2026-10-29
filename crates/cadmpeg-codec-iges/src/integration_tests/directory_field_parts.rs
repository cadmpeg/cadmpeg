// SPDX-License-Identifier: Apache-2.0

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

fn first_directory(bytes: &[u8]) -> usize {
    bytes
        .chunks_exact(81)
        .position(|card| card[72] == b'D')
        .expect("real Directory card")
        * 81
}

#[test]
fn each_optional_directory_field_has_one_exact_failure_home_and_keeps_both_real_points() {
    let original = points();
    let first = first_directory(&original);
    for (relative, field) in [
        (24, "line_font"),
        (32, "level"),
        (40, "view"),
        (56, "label_display"),
        (81 + 8, "line_weight"),
        (81 + 16, "color"),
        (81 + 64, "subscript"),
    ] {
        let start = first + relative;
        let mut good = original.clone();
        good[start..start + 8].copy_from_slice(b"       0");
        let baseline = crate::test_support::decode(good.clone());
        let mut bad = good.clone();
        bad[start + 7] = b'x';
        assert_eq!(good.len(), bad.len());
        assert_eq!(good.iter().zip(&bad).filter(|(a, b)| a != b).count(), 1);
        let decoded = crate::test_support::decode(bad.clone());
        assert_eq!(decoded.ir().model.points.len(), 2);
        assert_eq!(
            serde_json::to_value(&decoded.ir().model.points).expect("point values"),
            serde_json::to_value(&baseline.ir().model.points).expect("baseline points")
        );
        let native = decoded.ir().native.namespace("iges").expect("native IGES");
        let baseline_native = baseline
            .ir()
            .native
            .namespace("iges")
            .expect("baseline IGES");
        assert_eq!(native.arenas()["entities"].len(), 2);
        let id = "iges:entity:directory#1";
        let entity = native.arenas()["entities"]
            .iter()
            .find(|record| record.id() == id)
            .expect("field owner");
        let expected = baseline_native.arenas()["entities"]
            .iter()
            .find(|record| record.id() == id)
            .expect("original owner");
        let actual = serde_json::to_value(entity).expect("native entity");
        let mut expected = serde_json::to_value(expected).expect("original native entity");
        assert_eq!(expected[field], 0);
        expected[field] = serde_json::Value::Null;
        assert_eq!(actual, expected);
        let peer = |namespace: &cadmpeg_ir::NativeNamespace| {
            serde_json::to_value(
                namespace.arenas()["entities"]
                    .iter()
                    .find(|record| record.id() == "iges:entity:directory#3")
                    .expect("independent peer"),
            )
            .expect("peer value")
        };
        assert_eq!(peer(native), peer(baseline_native));
        let fidelity = decoded.source_fidelity();
        let parts = crate::test_support::given_up_parts(fidelity);
        assert_eq!(parts.len(), 1);
        let part = parts[0];
        assert!(!part.untyped().is_empty());
        assert_eq!(part.stream(), "");
        assert_eq!(part.offset(), u64::try_from(start).expect("source offset"));
        assert_eq!(part.byte_len(), 8);
        assert_eq!(part.data(), Some(&bad[start..start + 8]));
        assert_eq!(
            crate::test_support::given_up_count(decoded.report(), "iges.directory_entry"),
            0
        );
        let losses: Vec<_> = decoded
            .report()
            .losses
            .iter()
            .filter(|loss| loss.code.local_code() == "record.unreadable")
            .collect();
        assert_eq!(losses.len(), 1);
        assert_eq!(losses[0].severity, cadmpeg_ir::report::Severity::Error);
        assert!(losses[0].message.contains("iges.directory_field"));
        assert!(native.arenas()["quarantined_directory_records"].is_empty());
        assert!(native.arenas()["quarantined_parameter_records"].is_empty());
        assert!(cadmpeg_ir::validate_neutral(decoded.ir(), Vec::new())
            .expect("neutral validity")
            .is_ok());
        assert_eq!(good[start..start + 8], *b"       0");
        assert_eq!(bad[start..start + 8], *b"       x");
    }
}

#[test]
fn actual_optional_directory_scalar_source_flip_changes_only_its_current_native_field() {
    let original = points();
    let offset = first_directory(&original) + 81 + 8;
    let mut outputs = Vec::new();
    for value in [2, 3] {
        let mut source = original.clone();
        source[offset..offset + 8].copy_from_slice(if value == 2 {
            b"       2"
        } else {
            b"       3"
        });
        let decoded = crate::test_support::decode(source.clone());
        let native = decoded.ir().native.namespace("iges").expect("native IGES");
        let owner = native.arenas()["entities"]
            .iter()
            .find(|record| record.id() == "iges:entity:directory#1")
            .expect("actual owner");
        let mut owner = serde_json::to_value(owner).expect("native owner");
        assert_eq!(owner["line_weight"], value);
        owner["line_weight"] = serde_json::Value::Null;
        assert_eq!(decoded.ir().model.points.len(), 2);
        assert!(!decoded
            .report()
            .losses
            .iter()
            .any(|loss| loss.code.local_code() == "record.unreadable"));
        assert!(crate::test_support::given_up_parts(decoded.source_fidelity()).is_empty());
        outputs.push(owner);
        assert_eq!(source.len(), original.len());
    }
    assert_eq!(outputs[0], outputs[1]);
}

#[test]
fn inspect_and_decode_report_the_same_unreadable_optional_directory_field() {
    use cadmpeg_core::decode::InspectOptions;
    use cadmpeg_ir::codec::Codec as _;

    let original = points();
    let start = first_directory(&original) + 81 + 16;
    let mut bad = original.clone();
    bad[start..start + 8].copy_from_slice(b"       x");
    let decoded = crate::test_support::decode(bad.clone());
    let summary = crate::IgesCodec
        .inspect(&mut std::io::Cursor::new(bad), &InspectOptions::default())
        .expect("inspection");
    let unreadable = |losses: &[cadmpeg_ir::report::loss::LossNote]| {
        losses
            .iter()
            .filter(|loss| loss.code.local_code() == "record.unreadable")
            .map(|loss| loss.message.clone())
            .collect::<Vec<_>>()
    };
    let decode_losses = unreadable(&decoded.report().losses);
    assert_eq!(decode_losses.len(), 1);
    assert!(decode_losses[0].contains("iges.directory_field"));
    // #222: `inspect` runs the Directory parse that `decode` runs.
    assert_eq!(unreadable(&summary.losses), decode_losses);
}

#[test]
fn an_unreadable_binary_optional_directory_value_gives_up_only_its_value_part() {
    let baseline = crate::test_support::decode(
        crate::test_support::binary_parts::directory_optional_field(false),
    );
    let decoded = crate::test_support::decode(
        crate::test_support::binary_parts::directory_optional_field(true),
    );
    // #215 rule 1 and #222: the Directory record and both points stay; the
    // color value is one given-up `iges.binary_directory_value` part.
    assert_eq!(decoded.ir().model.points.len(), 2);
    assert_eq!(
        serde_json::to_value(&decoded.ir().model.points).expect("points"),
        serde_json::to_value(&baseline.ir().model.points).expect("baseline points")
    );
    let given_up = crate::test_support::given_up_parts(decoded.source_fidelity());
    assert_eq!(given_up.len(), 1);
    assert_eq!(
        crate::test_support::given_up_count(decoded.report(), "iges.binary_directory_value"),
        1
    );
    let unreadable: Vec<_> = decoded
        .report()
        .losses
        .iter()
        .filter(|loss| loss.code.local_code() == "record.unreadable")
        .collect();
    assert_eq!(unreadable.len(), 1);
    assert!(unreadable[0]
        .message
        .contains("iges.binary_directory_value"));
    let entities = |result: &cadmpeg_ir::codec::DecodeResult| {
        result
            .ir()
            .native
            .namespace("iges")
            .expect("native")
            .arenas()["entities"]
            .iter()
            .map(|record| serde_json::to_value(record).expect("entity"))
            .collect::<Vec<_>>()
    };
    let (actual, expected) = (entities(&decoded), entities(&baseline));
    assert_eq!(actual.len(), 2);
    let mut expected_first = expected[0].clone();
    expected_first["color"] = serde_json::Value::Null;
    assert_eq!(actual[0], expected_first);
    assert_eq!(actual[1], expected[1]);
}

#[test]
fn an_unreadable_compressed_optional_specifier_gives_up_only_its_specifier_part() {
    let original = crate::test_support::compressed_parts::points(false, false);
    let at = original
        .windows(5)
        .position(|window| window == b"@13_0")
        .expect("explicit color specifier");
    let mut bad = original.clone();
    bad[at + 4] = b'x';
    let baseline = crate::test_support::decode(original);
    let decoded = crate::test_support::decode(bad);
    // #215 rule 1 and #222: the Data record and both points stay; the color
    // specifier is one given-up `iges.compressed_directory_specifier` part.
    assert_eq!(decoded.ir().model.points.len(), 2);
    assert_eq!(
        serde_json::to_value(&decoded.ir().model.points).expect("points"),
        serde_json::to_value(&baseline.ir().model.points).expect("baseline points")
    );
    let given_up = crate::test_support::given_up_parts(decoded.source_fidelity());
    assert_eq!(given_up.len(), 1);
    assert_eq!(
        crate::test_support::given_up_count(
            decoded.report(),
            "iges.compressed_directory_specifier"
        ),
        1
    );
    assert_eq!(given_up[0].offset(), u64::try_from(at).expect("offset"));
    assert_eq!(given_up[0].byte_len(), 5);
    let unreadable = decoded
        .report()
        .losses
        .iter()
        .filter(|loss| loss.code.local_code() == "record.unreadable")
        .count();
    assert_eq!(unreadable, 1);
    let first = |result: &cadmpeg_ir::codec::DecodeResult| {
        serde_json::to_value(
            &result
                .ir()
                .native
                .namespace("iges")
                .expect("native")
                .arenas()["entities"][0],
        )
        .expect("entity")
    };
    let mut expected = first(&baseline);
    expected["color"] = serde_json::Value::Null;
    assert_eq!(first(&decoded), expected);
}
