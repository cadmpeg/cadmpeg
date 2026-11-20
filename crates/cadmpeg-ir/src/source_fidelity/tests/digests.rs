// SPDX-License-Identifier: Apache-2.0
use std::collections::BTreeMap;

use super::{fidelity, id, record, root_digest};
use crate::hash::digest::Sha256Digest;
use crate::ids::ProductDefinitionId;
use crate::source_fidelity::SourceFidelity;

fn definition(key: &str) -> ProductDefinitionId {
    ProductDefinitionId::mint(format!("synthetic:document:definition#{key}"))
        .expect("fixture identity")
}

#[test]
fn the_decode_digest_is_the_digest_of_the_root_files_bytes() {
    let bytes = b"root file bytes";
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let policy = cadmpeg_core::decode::DecodePolicy::service();
    let (_ctx, root) = cadmpeg_core::decode::DecodeContext::from_root_bytes(bytes, &arena, &policy)
        .expect("test context");
    let fidelity = SourceFidelity::for_root(root);
    assert_eq!(fidelity.decode_digest, Sha256Digest::digest(bytes));
    assert!(fidelity.file_digests.is_empty());
    assert!(fidelity.retained_records().is_empty());
}

#[test]
fn the_sidecar_writes_one_digest_per_file_before_its_records() {
    let mut fidelity = fidelity();
    let component = Sha256Digest::digest(b"component file");
    fidelity
        .file_digests
        .insert(definition("pin"), component.clone());
    let text = serde_json::to_string(&fidelity).unwrap();
    let keys: Vec<&str> = [
        "decode_digest",
        "file_digests",
        "unrepresented",
        "annotations",
        "retained_records",
    ]
    .into_iter()
    .collect();
    let positions: Vec<usize> = keys
        .iter()
        .map(|key| {
            text.find(&format!("\"{key}\""))
                .expect("every field is written")
        })
        .collect();
    assert!(positions.windows(2).all(|pair| pair[0] < pair[1]), "{text}");

    let wire: serde_json::Value = serde_json::from_str(&text).unwrap();
    assert_eq!(wire["decode_digest"], root_digest().as_str());
    assert_eq!(
        wire["file_digests"],
        serde_json::json!({"synthetic:document:definition#pin": component.as_str()})
    );
    assert_eq!(wire["retained_records"], serde_json::json!({}));
    assert_eq!(
        serde_json::from_str::<SourceFidelity>(&text).unwrap(),
        fidelity
    );
}

#[test]
fn every_digest_field_is_required() {
    let wire = serde_json::to_value(fidelity()).unwrap();
    for field in [
        "decode_digest",
        "file_digests",
        "annotations",
        "retained_records",
    ] {
        let mut without = wire.clone();
        without.as_object_mut().unwrap().remove(field);
        let error = serde_json::from_value::<SourceFidelity>(without)
            .expect_err("a sidecar states every field")
            .to_string();
        assert!(error.contains(field), "{field}: {error}");
    }
}

#[test]
fn a_component_file_is_keyed_once() {
    let digest = Sha256Digest::digest(b"component");
    let text = format!(
        r#"{{"decode_digest":"{root}","file_digests":{{
            "synthetic:document:definition#pin":"{digest}",
            "synthetic:document:definition#pin":"{digest}"}},
            "unrepresented":[],"annotations":{{"provenance":{{}},"exactness":{{}}}},"retained_records":{{}}}}"#,
        root = root_digest(),
    );
    let error = serde_json::from_str::<SourceFidelity>(&text)
        .expect_err("a definition states one digest")
        .to_string();
    assert!(
        error.contains("duplicate key synthetic:document:definition#pin"),
        "{error}"
    );
}

#[test]
fn appending_takes_only_fidelity_of_the_same_root_file() {
    let mut left = fidelity();
    left.insert_retained_record(id("left"), record(b"l"))
        .unwrap();
    let original = left.clone();
    let mut other_root = SourceFidelity::new(Sha256Digest::digest(b"another root"));
    other_root
        .insert_retained_record(id("right"), record(b"r"))
        .unwrap();
    let error = left
        .append(other_root)
        .expect_err("two root files are two decodes")
        .to_string();
    assert!(error.contains(root_digest().as_str()), "{error}");
    assert_eq!(left, original);

    let mut same_root = fidelity();
    same_root
        .insert_retained_record(id("right"), record(b"r"))
        .unwrap();
    left.append(same_root).unwrap();
    assert_eq!(left.retained_records().len(), 2);
    assert_eq!(left.decode_digest, root_digest());
}

#[test]
fn appending_keeps_one_digest_per_component_file() {
    let pin = Sha256Digest::digest(b"pin");
    let bolt = Sha256Digest::digest(b"bolt");
    let mut left = fidelity();
    left.file_digests.insert(definition("pin"), pin.clone());

    let mut agreeing = fidelity();
    agreeing.file_digests.insert(definition("pin"), pin.clone());
    agreeing
        .file_digests
        .insert(definition("bolt"), bolt.clone());
    left.append(agreeing).unwrap();
    assert_eq!(
        left.file_digests,
        BTreeMap::from([(definition("bolt"), bolt), (definition("pin"), pin)])
    );

    let original = left.clone();
    let mut conflicting = fidelity();
    conflicting
        .file_digests
        .insert(definition("pin"), Sha256Digest::digest(b"edited pin"));
    conflicting
        .insert_retained_record(id("conflict"), record(b"c"))
        .unwrap();
    let error = left
        .append(conflicting)
        .expect_err("one file has one digest")
        .to_string();
    assert!(
        error.contains("synthetic:document:definition#pin"),
        "{error}"
    );
    assert_eq!(left, original);
}
