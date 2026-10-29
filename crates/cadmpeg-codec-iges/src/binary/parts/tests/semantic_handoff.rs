// SPDX-License-Identifier: Apache-2.0
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};

use crate::test_support::binary_parts::macro_then_point;

#[test]
fn actual_binary_callbacks_return_the_same_records_that_the_renderer_binds_and_reader_uses() {
    let original = macro_then_point(false);
    let mut bytes = b"prefix:".to_vec();
    bytes.extend_from_slice(&original);
    bytes.extend_from_slice(b":suffix");
    let arena = DecodeArena::new();
    let (ctx, root) = DecodeContext::from_root_bytes(&bytes, &arena, &DecodePolicy::service())
        .expect("actual Binary source session");
    let source = root
        .child(7, 7 + original.len())
        .expect("actual nonzero Binary window");
    let sections = crate::binary::parse_sections(source).expect("actual Binary sections");
    let (_, first_extent, _) = crate::binary::counted_record(sections.parameter, 0, "Parameter")
        .expect("counted Macro source");
    let (_, _, second_body) = crate::binary::counted_record(
        sections.parameter,
        first_extent.end() - sections.parameter.start(),
        "Parameter",
    )
    .expect("counted peer source");
    let (normalized, _) = super::super::normalize(source, &ctx).expect("actual callbacks");
    let scan = crate::card::scan(&normalized.bytes).expect("rendered grammar");
    let (directory, quarantined) = crate::directory::parse_with_keys(
        &scan,
        normalized.admission.global.global_table(),
        &ctx,
        |key| normalized.identities.semantic_sequence(key),
    )
    .expect("same Directory grammar");
    assert!(quarantined.is_empty());
    let expected = crate::parameter::assemble(&scan, &directory, [], &normalized.admission.global)
        .expect("existing Parameter grammar oracle");
    assert_eq!(&normalized.admission.parameters, &expected.records);
    assert_eq!(normalized.admission.parameters.len(), 2);
    let actual = crate::parameter::finish_admitted(
        &directory,
        normalized.admission.parameters,
        &normalized.admission.global,
    );
    assert_eq!(
        actual.trailing_pointer_analysis,
        expected.trailing_pointer_analysis
    );
    let mut point = crate::binary::parameter_prefix(second_body, sections.lengths)
        .expect("actual peer Point grammar");
    assert_eq!(point.entity_type, 116);
    for expected in [1, 2, 3] {
        assert_eq!(
            point.stream.next().expect("original peer coordinate"),
            Some(crate::binary::BinaryValue::Integer(expected))
        );
    }
}

#[test]
fn malformed_binary_macro_gives_up_only_its_parameter_record_and_keeps_an_exact_peer() {
    let original = macro_then_point(false);
    let bytes = macro_then_point(true);
    assert_eq!(original.len(), bytes.len());
    assert_eq!(
        original.iter().zip(&bytes).filter(|(a, b)| a != b).count(),
        3
    );
    let baseline = crate::test_support::decode(original);
    let decoded = crate::test_support::decode(bytes.clone());
    assert_eq!(decoded.ir().model.points.len(), 1);
    assert_eq!(decoded.ir().model.vertices.len(), 1);
    assert_eq!(
        serde_json::to_value(&decoded.ir().model.points).expect("points"),
        serde_json::to_value(&baseline.ir().model.points).expect("baseline points")
    );
    assert_eq!(
        serde_json::to_value(&decoded.ir().model.vertices).expect("vertices"),
        serde_json::to_value(&baseline.ir().model.vertices).expect("baseline vertices")
    );
    let native = decoded.ir().native.namespace("iges").expect("IGES native");
    let old = baseline
        .ir()
        .native
        .namespace("iges")
        .expect("baseline native");
    assert_eq!(old.arenas()["macro_definitions"].len(), 1);
    assert_eq!(
        old.arenas()["macro_definitions"][0].id(),
        "iges:native:macro-definition#D1"
    );
    assert_eq!(
        native.arenas().get("macro_definitions").map_or(0, Vec::len),
        0
    );
    assert!(!native
        .arenas()
        .values()
        .flatten()
        .any(|record| record.id() == "iges:native:macro-definition#D1"));
    // #215 rule 1: the Macro's Directory record keeps its fields and its
    // Directory-owned display attributes; only its Parameter Data record is
    // given up.
    assert_eq!(native.arenas()["entities"].len(), 2);
    assert_eq!(
        serde_json::to_value(&native.arenas()["entities"][0]).expect("damaged Macro entity"),
        crate::test_support::binary_parts::entity_without_parameter_data(
            &old.arenas()["entities"][0]
        )
    );
    let peer = old.arenas()["entities"]
        .iter()
        .find(|record| record.id() == "iges:entity:directory#3")
        .expect("original peer entity");
    assert_eq!(&native.arenas()["entities"][1], peer);
    assert_eq!(
        native.arenas()["display_attributes"],
        old.arenas()["display_attributes"]
    );
    assert!(native.arenas()["quarantined_directory_records"].is_empty());
    assert!(native.arenas()["quarantined_parameter_records"].is_empty());
    let arena = DecodeArena::new();
    let (_ctx, root) = DecodeContext::from_root_bytes(&bytes, &arena, &DecodePolicy::service())
        .expect("independent source window");
    let sections = crate::binary::parse_sections(root).expect("original source sections");
    let (_, parameter, _) = crate::binary::counted_record(sections.parameter, 0, "Parameter")
        .expect("original Parameter count");
    let fidelity = decoded.source_fidelity();
    assert_eq!(
        crate::test_support::given_up_count(decoded.report(), "iges.binary_directory_record"),
        0
    );
    for (level, extent) in [("iges.binary_parameter_record", parameter)] {
        assert_eq!(
            crate::test_support::given_up_count(decoded.report(), level),
            1
        );
        let parts = crate::test_support::given_up_parts(fidelity);
        assert_eq!(parts.len(), 1);
        let part = parts[0];
        assert!(!part.untyped().is_empty());
        assert_eq!(
            part.offset(),
            u64::try_from(extent.start()).expect("source offset")
        );
        assert_eq!(
            part.byte_len(),
            u64::try_from(extent.window().len()).expect("source length")
        );
        assert_eq!(part.data(), Some(extent.window()));
    }
    let unreadable: Vec<_> = decoded
        .report()
        .losses
        .iter()
        .filter(|loss| loss.code.local_code() == "record.unreadable")
        .collect();
    assert_eq!(unreadable.len(), 1);
    assert!(unreadable
        .iter()
        .all(|loss| loss.severity == cadmpeg_ir::report::Severity::Error));
    assert!(cadmpeg_ir::validate_neutral(decoded.ir(), Vec::new())
        .expect("neutral validation")
        .is_ok());
}

#[test]
fn a_group_keeps_an_encoded_member_whose_parameter_record_is_given_up() {
    let original = crate::test_support::binary_parts::macro_group_then_point(false);
    let bytes = crate::test_support::binary_parts::macro_group_then_point(true);
    assert_eq!(original.len(), bytes.len());
    assert_eq!(
        original.iter().zip(&bytes).filter(|(a, b)| a != b).count(),
        3
    );
    let baseline = crate::test_support::decode(original);
    let native = baseline
        .ir()
        .native
        .namespace("iges")
        .expect("original native");
    assert_eq!(native.arenas()["groups"].len(), 1);
    assert_eq!(
        native.arenas()["groups"][0].field("declared_member_count"),
        Some(serde_json::json!(1))
    );
    assert_eq!(
        native.arenas()["groups"][0].field("members"),
        Some(serde_json::json!(["iges:entity:directory#1"]))
    );
    assert_eq!(native.arenas()["macro_definitions"].len(), 1);
    let decoded = crate::test_support::decode(bytes.clone());
    assert_eq!(decoded.ir().model.points.len(), 1);
    assert_eq!(decoded.ir().model.vertices.len(), 1);
    assert_eq!(
        decoded.ir().model.points[0].id,
        crate::ids::point(&crate::ids::Stem::directory(5_u32))
    );
    assert_eq!(
        serde_json::to_value(&decoded.ir().model.points).expect("points"),
        serde_json::to_value(&baseline.ir().model.points).expect("baseline points")
    );
    assert_eq!(
        serde_json::to_value(&decoded.ir().model.vertices).expect("vertices"),
        serde_json::to_value(&baseline.ir().model.vertices).expect("baseline vertices")
    );
    let current = decoded
        .ir()
        .native
        .namespace("iges")
        .expect("current native");
    // #215 rules 1-2: the Macro's Directory record stays, so the Group that
    // lists it keeps that list entry and its declared count.
    assert_eq!(current.arenas()["groups"].len(), 1);
    let group = &current.arenas()["groups"][0];
    assert_eq!(group.id(), "iges:product:group#D3");
    assert_eq!(
        group.field("source_entity"),
        Some(serde_json::json!("iges:entity:directory#3"))
    );
    assert_eq!(group, &native.arenas()["groups"][0]);
    assert_eq!(
        current
            .arenas()
            .get("macro_definitions")
            .map_or(0, Vec::len),
        0
    );
    assert!(!current
        .arenas()
        .values()
        .flatten()
        .any(|record| record.id() == "iges:native:macro-definition#D1"));
    assert_eq!(current.arenas()["entities"].len(), 3);
    assert_eq!(
        serde_json::to_value(&current.arenas()["entities"][0]).expect("damaged Macro entity"),
        crate::test_support::binary_parts::entity_without_parameter_data(
            &native.arenas()["entities"][0]
        )
    );
    assert_eq!(
        current.arenas()["entities"][1..],
        native.arenas()["entities"][1..]
    );
    for (level, given_up) in [
        ("iges.binary_directory_record", 0),
        ("iges.binary_parameter_record", 1),
    ] {
        assert_eq!(
            crate::test_support::given_up_count(decoded.report(), level),
            given_up
        );
    }
    assert_eq!(
        crate::test_support::given_up_parts(decoded.source_fidelity()).len(),
        1
    );
    let unreadable: Vec<_> = decoded
        .report()
        .losses
        .iter()
        .filter(|loss| loss.code.local_code() == "record.unreadable")
        .collect();
    assert_eq!(unreadable.len(), 1);
    assert!(unreadable
        .iter()
        .all(|loss| loss.severity == cadmpeg_ir::report::Severity::Error));
    assert!(cadmpeg_ir::validate_neutral(decoded.ir(), Vec::new())
        .expect("neutral validation")
        .is_ok());
}

#[test]
fn a_pointer_tagged_discriminator_cannot_change_the_actual_parameter_entity_type() {
    let original = crate::test_support::binary_parts::discriminator_source_offset_pair(false);
    let bytes = crate::test_support::binary_parts::discriminator_source_offset_pair(true);
    assert_eq!(original.len(), bytes.len());
    assert_eq!(
        original.iter().zip(&bytes).filter(|(a, b)| a != b).count(),
        1
    );
    let baseline = crate::test_support::decode(original);
    assert_eq!(baseline.ir().model.points.len(), 2);
    let decoded = crate::test_support::decode(bytes);
    assert_eq!(decoded.ir().model.points.len(), 1);
    let point_id = crate::ids::point(&crate::ids::Stem::directory(3_u32));
    let expected = baseline
        .ir()
        .model
        .points
        .iter()
        .find(|point| point.id == point_id)
        .expect("the actual second source owner");
    assert_eq!(
        serde_json::to_value(&decoded.ir().model.points[0]).expect("point"),
        serde_json::to_value(expected).expect("baseline peer")
    );
    let native = decoded.ir().native.namespace("iges").expect("native");
    let old = baseline
        .ir()
        .native
        .namespace("iges")
        .expect("baseline native");
    assert_eq!(
        native.arenas()["entities"],
        [old.arenas()["entities"][1].clone()]
    );
    assert!(native.arenas()["quarantined_directory_records"].is_empty());
    assert!(native.arenas()["quarantined_parameter_records"].is_empty());
    assert_eq!(
        crate::test_support::given_up_parts(decoded.source_fidelity()).len(),
        2
    );
    for level in [
        "iges.binary_directory_record",
        "iges.binary_parameter_record",
    ] {
        assert_eq!(
            crate::test_support::given_up_count(decoded.report(), level),
            1
        );
    }
}

#[test]
fn a_non_null_binary_owner_without_parameter_data_keeps_its_directory_entry() {
    let bytes = crate::test_support::binary_parts::non_null_without_parameter_record();
    let decoded = crate::test_support::decode(bytes);
    // A zero Parameter Data pointer states no Parameter Data record. No byte
    // is unreadable, so no part is given up; the Directory record keeps its
    // fields with no parameters (#215 §4.5, rule 1).
    assert!(decoded.ir().model.points.is_empty());
    let native = decoded.ir().native.namespace("iges").expect("native");
    let entities = &native.arenas()["entities"];
    assert_eq!(entities.len(), 1);
    assert_eq!(entities[0].id(), "iges:entity:directory#1");
    assert_eq!(
        entities[0].field("entity_type"),
        Some(serde_json::json!(116))
    );
    assert_eq!(entities[0].field("parameters"), Some(serde_json::json!([])));
    assert_eq!(
        entities[0].field("parameter_line_count"),
        Some(serde_json::json!(0))
    );
    assert!(native.arenas()["quarantined_parameter_records"].is_empty());
    assert!(native.arenas().get("unknowns").is_none_or(Vec::is_empty));
    assert!(crate::test_support::given_up_parts(decoded.source_fidelity()).is_empty());
    assert!(!decoded
        .report()
        .losses
        .iter()
        .any(|loss| loss.code.local_code() == "record.unreadable"));
}

#[test]
fn a_group_keeps_every_member_and_its_count_after_a_macro_parameter_record_fails() {
    for encoded_members in [true, false] {
        let original = crate::test_support::binary_parts::macro_and_point_group_then_point(
            false,
            encoded_members,
        );
        let bytes = crate::test_support::binary_parts::macro_and_point_group_then_point(
            true,
            encoded_members,
        );
        assert_eq!(original.len(), bytes.len());
        assert_eq!(
            original.iter().zip(&bytes).filter(|(a, b)| a != b).count(),
            3
        );
        let baseline = crate::test_support::decode(original);
        let decoded = crate::test_support::decode(bytes.clone());
        assert_eq!(decoded.ir().model.points.len(), 1);
        assert_eq!(decoded.ir().model.vertices.len(), 1);
        assert_eq!(
            serde_json::to_value(&decoded.ir().model.points).expect("actual peer points"),
            serde_json::to_value(&baseline.ir().model.points).expect("baseline peer points"),
        );
        assert_eq!(
            serde_json::to_value(&decoded.ir().model.vertices).expect("actual peer vertices"),
            serde_json::to_value(&baseline.ir().model.vertices).expect("baseline peer vertices"),
        );
        let native = decoded
            .ir()
            .native
            .namespace("iges")
            .expect("current native");
        let original_native = baseline
            .ir()
            .native
            .namespace("iges")
            .expect("baseline native");
        assert_eq!(original_native.arenas()["macro_definitions"].len(), 1);
        assert_eq!(original_native.arenas()["groups"].len(), 1);
        assert_eq!(
            original_native.arenas()["groups"][0].field("declared_member_count"),
            Some(serde_json::json!(2))
        );
        assert_eq!(
            original_native.arenas()["groups"][0].field("members"),
            Some(serde_json::json!([
                "iges:entity:directory#1",
                "iges:entity:directory#5"
            ]))
        );
        let groups = &native.arenas()["groups"];
        assert_eq!(groups.len(), 1);
        let group = &groups[0];
        assert_eq!(group.id(), "iges:product:group#D3");
        assert_eq!(
            group.field("source_entity"),
            Some(serde_json::json!("iges:entity:directory#3"))
        );
        // #215 rules 1-2: the Macro's Directory record stays, so the Group
        // keeps both list entries and its declared count.
        assert_eq!(
            group.field("declared_member_count"),
            Some(serde_json::json!(2))
        );
        assert_eq!(
            group.field("members"),
            Some(serde_json::json!([
                "iges:entity:directory#1",
                "iges:entity:directory#5"
            ]))
        );
        assert_eq!(group.field("ordered"), Some(serde_json::json!(false)));
        assert_eq!(
            group.field("back_pointers_required"),
            Some(serde_json::json!(false))
        );
        assert_eq!(
            native.arenas().get("macro_definitions").map_or(0, Vec::len),
            0
        );
        assert!(!native
            .arenas()
            .values()
            .flatten()
            .any(|record| record.id() == "iges:native:macro-definition#D1"));
        let entities = &native.arenas()["entities"];
        assert_eq!(entities.len(), 3);
        assert_eq!(
            serde_json::to_value(&entities[0]).expect("damaged Macro entity"),
            crate::test_support::binary_parts::entity_without_parameter_data(
                &original_native.arenas()["entities"][0]
            )
        );
        assert_eq!(entities[1..], original_native.arenas()["entities"][1..]);
        assert_eq!(
            crate::test_support::given_up_parts(decoded.source_fidelity()).len(),
            1
        );
        let errors: Vec<_> = decoded
            .report()
            .losses
            .iter()
            .filter(|loss| loss.code.local_code() == "record.unreadable")
            .collect();
        assert_eq!(errors.len(), 1);
        assert!(errors
            .iter()
            .all(|loss| loss.severity == cadmpeg_ir::report::Severity::Error));
        assert!(cadmpeg_ir::validate_neutral(decoded.ir(), Vec::new())
            .expect("neutral validation")
            .is_ok());
    }
}
