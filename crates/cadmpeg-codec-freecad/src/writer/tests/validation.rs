// SPDX-License-Identifier: Apache-2.0
//! Property graph identity, refusal precedence and deterministic error selection.

use cadmpeg_core::CodecError;

use crate::native::{PropertyBody, PropertyFamily, PropertyRecord, RetainedXml};
use crate::writer::validate_properties;

fn property(id: &str) -> PropertyRecord {
    PropertyRecord {
        id: id.into(),
        owner: "test:object#owner".into(),
        name: "Value".into(),
        type_name: "App::PropertyString".into(),
        family: PropertyFamily::String,
        status: Some(8),
        body: PropertyBody::Transient,
        order: 0,
        xml: RetainedXml::from_text(
            r#"<_Property name="Value" type="App::PropertyString" status="8"/>"#.into(),
            0,
        )
        .expect("retained declaration"),
    }
}

#[test]
fn writer_property_missing_identity_follows_expected_order() {
    let first = property("test:property#z");
    let second = property("test:property#a");
    let written = [
        property("test:property#other-a"),
        property("test:property#other-z"),
    ];
    for expected in [[first.clone(), second.clone()], [second, first]] {
        let error = validate_properties(&expected, &written).expect_err("missing identities");
        assert!(matches!(error, CodecError::NotImplemented(message)
            if message == format!("edited FCStd property {} is missing from the written graph", expected[0].id)));
    }
}

#[test]
fn writer_property_semantic_mismatch_follows_expected_order() {
    let first = property("test:property#z");
    let second = property("test:property#a");
    let mut written = [second.clone(), first.clone()];
    for candidate in &mut written {
        candidate.status = Some(16);
    }
    for expected in [[first.clone(), second.clone()], [second, first]] {
        let error = validate_properties(&expected, &written).expect_err("status mismatch");
        assert!(matches!(error, CodecError::NotImplemented(message)
            if message == format!("edited FCStd property {} changes its admitted value or link graph", expected[0].id)));
    }
}

#[test]
fn writer_property_expected_duplicate_precedes_written_duplicate_and_count() {
    let duplicate = property("test:property#expected");
    let written_duplicate = property("test:property#written");
    let error = validate_properties(
        &[duplicate.clone(), duplicate],
        &[
            written_duplicate.clone(),
            written_duplicate,
            property("test:property#extra"),
        ],
    )
    .expect_err("expected duplicates precede later gates");
    assert!(matches!(error, CodecError::NotImplemented(message)
        if message == "edited FCStd property graph has duplicate property test:property#expected"));
}

#[test]
fn writer_property_written_duplicate_precedes_count_and_missing_identity() {
    let duplicate = property("test:property#written");
    let error = validate_properties(
        &[property("test:property#expected")],
        &[duplicate.clone(), duplicate],
    )
    .expect_err("written duplicates precede count and correspondence");
    assert!(matches!(error, CodecError::NotImplemented(message)
        if message == "written FCStd property graph has duplicate property test:property#written"));
}

#[test]
fn writer_property_count_precedes_missing_identity() {
    let error = validate_properties(
        &[property("test:property#expected")],
        &[
            property("test:property#other"),
            property("test:property#extra"),
        ],
    )
    .expect_err("record count precedes identity correspondence");
    assert!(matches!(error, CodecError::NotImplemented(message)
        if message == "edited FCStd property graph changes its record count"));
}

#[test]
fn writer_property_correspondence_uses_identity_and_preserves_semantic_order() {
    let first = property("test:property#z");
    let mut second = property("test:property#a");
    second.order = 1;
    validate_properties(&[first.clone(), second.clone()], &[second, first])
        .expect("arena order does not replace property identity or its source ordinal");
    validate_properties(&[], &[]).expect("empty property graph");
}
