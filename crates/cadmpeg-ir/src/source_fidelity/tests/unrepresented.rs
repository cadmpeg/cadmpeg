// SPDX-License-Identifier: Apache-2.0
use std::collections::BTreeSet;

use super::{fidelity, id, record, report};
use crate::hash::digest::Sha256Digest;
use crate::source_fidelity::{DecodeSidecar, SourceFidelity};
use cadmpeg_core::decode::ArenaClass;

#[test]
fn unrepresented_is_the_union_of_the_retained_records_untyped_classes() {
    let mut fidelity = fidelity();
    assert!(fidelity.unrepresented().is_empty());
    fidelity
        .insert_retained_record(id("decoded"), record(b"decoded image"))
        .unwrap();
    assert!(
        fidelity.unrepresented().is_empty(),
        "a record the decoder decoded marks no class"
    );
    fidelity
        .insert_retained_record(
            id("unknown"),
            record(b"unknown record type").with_untyped([ArenaClass::Design]),
        )
        .unwrap();
    fidelity
        .insert_retained_record(id("skipped"), record(b"skipped region"))
        .unwrap();
    fidelity
        .mark_untyped(id("skipped").as_str(), [ArenaClass::Model])
        .unwrap();
    assert_eq!(
        fidelity.unrepresented(),
        BTreeSet::from([ArenaClass::Model, ArenaClass::Design])
    );
    assert_eq!(
        fidelity
            .retained_record(id("skipped").as_str())
            .unwrap()
            .untyped(),
        &BTreeSet::from([ArenaClass::Model])
    );

    fidelity.remove_retained_record(id("unknown").as_str());
    assert_eq!(
        fidelity.unrepresented(),
        BTreeSet::from([ArenaClass::Model])
    );
}

#[test]
fn marks_join_the_classes_already_marked() {
    let marked = record(b"part")
        .with_untyped([ArenaClass::Model])
        .with_untyped([ArenaClass::Design]);
    assert_eq!(
        marked.untyped(),
        &BTreeSet::from([ArenaClass::Model, ArenaClass::Design])
    );

    let mut fidelity = fidelity();
    fidelity
        .insert_retained_record(
            id("part"),
            record(b"part").with_untyped([ArenaClass::Design]),
        )
        .unwrap();
    fidelity
        .mark_untyped(id("part").as_str(), [ArenaClass::Model])
        .unwrap();
    assert_eq!(
        fidelity
            .retained_record(id("part").as_str())
            .unwrap()
            .untyped(),
        &BTreeSet::from([ArenaClass::Model, ArenaClass::Design])
    );
}

#[test]
fn a_mark_names_a_retained_record() {
    let mut fidelity = fidelity();
    let original = fidelity.clone();
    let error = fidelity
        .mark_untyped("synthetic:source:record#absent", [ArenaClass::Model])
        .expect_err("a mark is set on a record the sidecar keeps");
    assert!(
        error.to_string().contains("synthetic:source:record#absent"),
        "{error}"
    );
    assert_eq!(fidelity, original);
}

#[test]
fn appended_fidelity_carries_its_marks_into_the_union() {
    let mut left = fidelity();
    left.insert_retained_record(id("left"), record(b"l").with_untyped([ArenaClass::Model]))
        .unwrap();
    let mut right = fidelity();
    right
        .insert_retained_record(id("right"), record(b"r").with_untyped([ArenaClass::Design]))
        .unwrap();
    left.append(right).unwrap();
    assert_eq!(
        left.unrepresented(),
        BTreeSet::from([ArenaClass::Model, ArenaClass::Design])
    );
}

#[test]
fn the_sidecar_states_unrepresented_and_each_records_untyped_classes() {
    let mut fidelity = fidelity();
    fidelity
        .insert_retained_record(
            id("given-up"),
            record(b"given-up part").with_untyped([ArenaClass::Design, ArenaClass::Model]),
        )
        .unwrap();
    fidelity
        .insert_retained_record(id("image"), record(b"source image"))
        .unwrap();
    let sidecar =
        DecodeSidecar::bind_sha256(Sha256Digest::digest(b"cad-ir"), report(), fidelity.clone());
    let wire = cadmpeg_core::serde_erase::json::to_value(&sidecar).unwrap();
    assert_eq!(
        wire["fidelity"]["unrepresented"],
        serde_json::json!(["model", "design"])
    );
    let records = &wire["fidelity"]["retained_records"];
    assert_eq!(
        records["synthetic:source:record#given-up"]["untyped"],
        serde_json::json!(["model", "design"])
    );
    assert_eq!(
        records["synthetic:source:record#image"]["untyped"],
        serde_json::json!([])
    );
    let restored: DecodeSidecar = cadmpeg_core::serde_erase::json::from_value(wire).unwrap();
    assert_eq!(restored.fidelity, fidelity);
}

#[test]
fn a_sidecar_whose_unrepresented_differs_from_its_marks_is_refused() {
    let mut fidelity = fidelity();
    fidelity
        .insert_retained_record(
            id("part"),
            record(b"part").with_untyped([ArenaClass::Design]),
        )
        .unwrap();
    let wire = cadmpeg_core::serde_erase::json::to_value(&fidelity).unwrap();
    for stated in [
        serde_json::json!([]),
        serde_json::json!(["model", "design"]),
    ] {
        let mut tampered = wire.clone();
        tampered["unrepresented"] = stated.clone();
        let error = cadmpeg_core::serde_erase::json::from_value::<SourceFidelity>(tampered)
            .expect_err("unrepresented restates the records' marks")
            .to_string();
        assert!(error.contains("untyped classes"), "{stated}: {error}");
    }
}

#[test]
fn every_untyped_and_unrepresented_set_is_written_and_required() {
    let mut fidelity = fidelity();
    fidelity
        .insert_retained_record(id("image"), record(b"source image"))
        .unwrap();
    let wire = cadmpeg_core::serde_erase::json::to_value(&fidelity).unwrap();
    assert_eq!(wire["unrepresented"], serde_json::json!([]));

    let mut without_set = wire.clone();
    without_set.as_object_mut().unwrap().remove("unrepresented");
    let error = cadmpeg_core::serde_erase::json::from_value::<SourceFidelity>(without_set)
        .expect_err("an empty set is written, not omitted")
        .to_string();
    assert!(error.contains("unrepresented"), "{error}");

    let mut without_marks = wire;
    without_marks["retained_records"]["synthetic:source:record#image"]
        .as_object_mut()
        .unwrap()
        .remove("untyped");
    let error = cadmpeg_core::serde_erase::json::from_value::<SourceFidelity>(without_marks)
        .expect_err("a record's empty mark is written, not omitted")
        .to_string();
    assert!(error.contains("untyped"), "{error}");
}

#[test]
fn a_class_set_has_one_spelling() {
    let mut fidelity = fidelity();
    fidelity
        .insert_retained_record(
            id("part"),
            record(b"part").with_untyped([ArenaClass::Model, ArenaClass::Design]),
        )
        .unwrap();
    let wire = cadmpeg_core::serde_erase::json::to_value(&fidelity).unwrap();
    assert_eq!(
        wire["unrepresented"],
        serde_json::json!(["model", "design"])
    );
    for (stated, refusal) in [
        (serde_json::json!(["design", "model"]), "out of order"),
        (serde_json::json!(["model", "model", "design"]), "twice"),
        (serde_json::json!(["model", "geometry"]), "geometry"),
    ] {
        let mut set = wire.clone();
        set["unrepresented"] = stated.clone();
        let error = cadmpeg_core::serde_erase::json::from_value::<SourceFidelity>(set)
            .expect_err("unrepresented is written once, in class order")
            .to_string();
        assert!(error.contains(refusal), "{stated}: {error}");
        let mut marks = wire.clone();
        marks["retained_records"]["synthetic:source:record#part"]["untyped"] = stated.clone();
        let error = cadmpeg_core::serde_erase::json::from_value::<SourceFidelity>(marks)
            .expect_err("untyped is written once, in class order")
            .to_string();
        assert!(error.contains(refusal), "{stated}: {error}");
    }
}

#[test]
fn a_record_read_back_keeps_its_marks() {
    let marked = record(b"part").with_untyped([ArenaClass::Model]);
    let wire = cadmpeg_core::serde_erase::json::to_value(&marked).unwrap();
    assert_eq!(wire["untyped"], serde_json::json!(["model"]));
    assert_eq!(
        cadmpeg_core::serde_erase::json::from_value::<crate::source_fidelity::RetainedSourceRecord>(wire).unwrap(),
        marked
    );
}
