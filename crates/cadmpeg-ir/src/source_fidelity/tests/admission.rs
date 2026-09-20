// SPDX-License-Identifier: Apache-2.0
use super::{id, record, report};
use crate::hash::digest::Sha256Digest;
use crate::provenance::SourceOwner;
use crate::source_fidelity::{DecodeSidecar, RetainedSourceRecord, SourceFidelity};
use crate::{CadIr, UnknownRecord};

fn sidecar_wire() -> serde_json::Value {
    let mut fidelity = SourceFidelity::default();
    fidelity
        .insert_retained_record(id("a"), record(b"abc"))
        .unwrap();
    serde_json::to_value(DecodeSidecar::bind_sha256(
        crate::hash::digest::Sha256Digest::digest(b"cad-ir"),
        report(),
        fidelity,
    ))
    .unwrap()
}

#[test]
fn invalid_product_link_cannot_partially_attach_but_source_retention_accepts_evidence() {
    let invalid =
        UnknownRecord::retained(id("bad-link"), 0, vec![1], vec!["raw target text".into()]);
    let mut fidelity = SourceFidelity::default();
    let mut ir = CadIr::empty();
    let incoming = [
        UnknownRecord::retained(id("good"), 0, vec![2], vec![]),
        invalid.clone(),
    ];
    let error = fidelity
        .attach_native_unknown_records(&mut ir, "synthetic", incoming)
        .unwrap_err();
    assert!(error.to_string().contains(invalid.id().as_str()), "{error}");
    assert_eq!(fidelity, SourceFidelity::default());
    assert_eq!(ir, CadIr::empty());
    fidelity
        .retain_unknown_records(SourceOwner::Root, [invalid.clone()])
        .unwrap();
    assert_eq!(
        fidelity
            .retained_record(invalid.id().as_str())
            .unwrap()
            .data(),
        Some(&[1][..])
    );
}

#[test]
fn attachment_refuses_an_identity_already_owned_by_another_native_namespace() {
    let mut ir = CadIr::empty();
    ir.set_native_unknowns(
        "other",
        &[crate::NativeUnknownRecord {
            id: id("occupied"),
            links: vec![],
        }],
    )
    .unwrap();
    let before = ir.clone();
    let mut fidelity = SourceFidelity::default();
    let error = fidelity
        .attach_native_unknown_records(
            &mut ir,
            "synthetic",
            [
                UnknownRecord::retained(id("first"), 0, vec![1], vec![]),
                UnknownRecord::retained(id("occupied"), 0, vec![2], vec![]),
            ],
        )
        .unwrap_err();
    assert!(
        error.to_string().contains(id("occupied").as_str()),
        "{error}"
    );
    assert_eq!(ir, before);
    assert_eq!(fidelity, SourceFidelity::default());
}

#[test]
fn complete_sidecar_requires_the_current_ir_version_on_both_read_routes() {
    let valid = sidecar_wire();
    assert_eq!(valid["ir_version"], crate::IR_VERSION);
    assert!(DecodeSidecar::from_json(&valid.to_string()).is_ok());
    assert!(serde_json::from_value::<DecodeSidecar>(valid.clone()).is_ok());
    for version in [
        None,
        Some(serde_json::Value::Null),
        Some(serde_json::json!(0)),
        Some(serde_json::json!(false)),
        Some(serde_json::json!("unsupported")),
        Some(serde_json::json!({})),
    ] {
        let mut wire = valid.clone();
        match version {
            Some(version) => {
                wire["ir_version"] = version;
            }
            None => {
                wire.as_object_mut().unwrap().remove("ir_version");
            }
        }
        let error = DecodeSidecar::from_json(&wire.to_string()).unwrap_err();
        assert!(error.to_string().contains("ir_version"), "{error}");
        let error = serde_json::from_value::<DecodeSidecar>(wire).unwrap_err();
        assert!(error.to_string().contains("ir_version"), "{error}");
    }
}

#[test]
fn complete_sidecar_admission_rejects_malformed_digests_and_record_keys() {
    let valid = sidecar_wire();
    assert!(DecodeSidecar::from_json(&valid.to_string()).is_ok());
    for digest in [
        String::new(),
        "a".repeat(63),
        "a".repeat(65),
        "A".repeat(64),
        "z".repeat(64),
    ] {
        let mut wire = valid.clone();
        wire["ir_sha256"] = digest.into();
        assert!(DecodeSidecar::from_json(&wire.to_string()).is_err());
    }
    for key in ["", "record", "a:b:c#", "a:b:c#with space"] {
        let mut wire = valid.clone();
        let records = wire["fidelity"]["retained_records"]
            .as_object_mut()
            .unwrap();
        let record = records.remove(id("a").as_str()).unwrap();
        records.insert(key.into(), record);
        assert!(
            DecodeSidecar::from_json(&wire.to_string()).is_err(),
            "{key:?}"
        );
    }
    let mut wire = valid;
    wire["fidelity"]["retained_records"][id("a").as_str()]["bytes"] = serde_json::json!({
        "retention": "digest", "byte_len": 3, "sha256": "wire-value"
    });
    let error = DecodeSidecar::from_json(&wire.to_string()).unwrap_err();
    assert!(error.to_string().contains(id("a").as_str()), "{error}");
}

#[test]
fn complete_sidecar_admission_checks_inline_and_digest_extents() {
    for (image, admitted) in [
        (serde_json::json!({"retention": "inline", "data": ""}), true),
        (
            serde_json::json!({"retention": "inline", "data": "AQ=="}),
            false,
        ),
        (
            serde_json::json!({"retention": "digest", "byte_len": 0, "sha256": Sha256Digest::digest(b"")}),
            true,
        ),
        (
            serde_json::json!({"retention": "digest", "byte_len": 1, "sha256": Sha256Digest::digest(b"a")}),
            false,
        ),
    ] {
        let mut wire = sidecar_wire();
        let record = &mut wire["fidelity"]["retained_records"][id("a").as_str()];
        record["offset"] = u64::MAX.into();
        record["bytes"] = image;
        let parsed = DecodeSidecar::from_json(&wire.to_string());
        if admitted {
            let parsed = parsed.unwrap();
            assert_eq!(
                parsed
                    .fidelity
                    .retained_record(id("a").as_str())
                    .unwrap()
                    .end_offset(),
                u64::MAX
            );
        } else {
            let error = parsed.unwrap_err();
            assert!(error.to_string().contains(id("a").as_str()), "{error}");
        }
    }
    assert!(RetainedSourceRecord::from_bytes(
        SourceOwner::Root,
        u64::MAX,
        crate::source_fidelity::RetainedBytes::Inline { data: vec![1] }
    )
    .is_err());
    assert!(RetainedSourceRecord::from_bytes(
        SourceOwner::Root,
        u64::MAX,
        crate::source_fidelity::RetainedBytes::Digest {
            byte_len: 1,
            sha256: Sha256Digest::digest(b"a")
        }
    )
    .is_err());
}

#[test]
fn invalid_raw_evidence_cannot_partially_enter_authoritative_retention() {
    let invalid_records = [
        UnknownRecord::retained(id("inline-overflow"), u64::MAX, vec![1], vec![]),
        UnknownRecord::unavailable(
            id("digest-overflow"),
            u64::MAX,
            1,
            Sha256Digest::digest(b"a").as_str(),
            vec![],
        ),
        UnknownRecord::unavailable(id("digest-text"), 0, 1, "wire-value", vec![]),
    ];
    for attach in [false, true] {
        for invalid in &invalid_records {
            for existing in [false, true] {
                let mut ir = CadIr::empty();
                let mut fidelity = SourceFidelity::default();
                if existing {
                    fidelity
                        .attach_native_unknown_records(
                            &mut ir,
                            "synthetic",
                            [UnknownRecord::retained(id("existing"), 0, vec![3], vec![])],
                        )
                        .unwrap();
                }
                let before_ir = ir.clone();
                let before_fidelity = fidelity.clone();
                let incoming = [
                    UnknownRecord::retained(id("new"), 0, vec![2], vec![]),
                    invalid.clone(),
                ];
                let error = if attach {
                    fidelity.attach_native_unknown_records(&mut ir, "synthetic", incoming)
                } else {
                    fidelity.retain_unknown_records(SourceOwner::Root, incoming)
                }
                .unwrap_err();
                assert!(error.to_string().contains(invalid.id().as_str()), "{error}");
                assert_eq!(ir, before_ir);
                assert_eq!(fidelity, before_fidelity);
            }
        }
    }
}

#[test]
fn retention_and_attachment_refuse_duplicate_batches_without_mutation() {
    for attach in [false, true] {
        for collide_with_existing in [false, true] {
            let mut ir = CadIr::empty();
            let mut fidelity = SourceFidelity::default();
            fidelity
                .attach_native_unknown_records(
                    &mut ir,
                    "synthetic",
                    [UnknownRecord::retained(id("existing"), 0, vec![3], vec![])],
                )
                .unwrap();
            let before_ir = ir.clone();
            let before_fidelity = fidelity.clone();
            let duplicate_id = if collide_with_existing {
                id("existing")
            } else {
                id("new")
            };
            let incoming = [
                UnknownRecord::retained(id("new"), 0, vec![1], vec![]),
                UnknownRecord::retained(duplicate_id.clone(), 0, vec![2], vec![]),
            ];
            let error = if attach {
                fidelity.attach_native_unknown_records(&mut ir, "synthetic", incoming)
            } else {
                fidelity.retain_unknown_records(SourceOwner::Root, incoming)
            }
            .unwrap_err();
            assert!(error.to_string().contains(duplicate_id.as_str()), "{error}");
            assert_eq!(fidelity, before_fidelity);
            assert_eq!(ir, before_ir);
        }
    }
}

#[test]
fn attachment_preserves_existing_records_and_the_root_owner() {
    let mut ir = CadIr::empty();
    let mut fidelity = SourceFidelity::default();
    for name in ["a", "b"] {
        fidelity
            .attach_native_unknown_records(
                &mut ir,
                "synthetic",
                [UnknownRecord::retained(id(name), 0, vec![1], vec![])],
            )
            .unwrap();
    }
    assert_eq!(
        ir.native_unknowns("synthetic")
            .unwrap()
            .into_iter()
            .map(|record| record.id)
            .collect::<Vec<_>>(),
        [id("a"), id("b")]
    );
    assert_eq!(fidelity.retained_records().len(), 2);
    assert_eq!(
        fidelity.retained_record(id("a").as_str()).unwrap().stream(),
        ""
    );
    let before = fidelity.clone();
    assert!(fidelity
        .insert_retained_record(id("a"), record(b"replacement"))
        .is_err());
    assert_eq!(fidelity, before);
    fidelity
        .retain_unknown_records(
            " \t",
            [UnknownRecord::retained(id("space"), 0, vec![], vec![])],
        )
        .unwrap();
    assert_eq!(
        fidelity
            .retained_record(id("space").as_str())
            .unwrap()
            .stream(),
        " \t"
    );
}

#[test]
fn failed_existing_native_admission_leaves_both_destinations_unchanged() {
    let mut ir = CadIr::empty();
    ir.native
        .namespace_mut("synthetic")
        .set_arena(
            "unknowns",
            &[serde_json::json!({"id": id("bad"), "links": [1]})],
        )
        .unwrap();
    let mut fidelity = SourceFidelity::default();
    let before = ir.clone();
    let error = fidelity
        .attach_native_unknown_records(
            &mut ir,
            "synthetic",
            [UnknownRecord::retained(id("new"), 0, vec![1], vec![])],
        )
        .unwrap_err();
    assert!(error.to_string().contains("string"), "{error}");
    assert_eq!(ir, before);
    assert_eq!(fidelity, SourceFidelity::default());
}

#[test]
fn appending_source_metadata_is_atomic_across_annotations_and_bytes() {
    use crate::annotations::{AnnotationBuilder, StreamHandle};
    for record_collision in [false, true] {
        let mut target = SourceFidelity::default();
        target
            .insert_retained_record(id("existing"), record(b"original"))
            .unwrap();
        let mut existing = AnnotationBuilder::new();
        existing.note(
            id("annotated"),
            &StreamHandle::new(crate::stream_name!("original")),
            7,
        );
        target.annotations = existing.build();
        let before = target.clone();
        let mut incoming = SourceFidelity::default();
        incoming
            .insert_retained_record(
                id(if record_collision { "existing" } else { "new" }),
                record(b"incoming"),
            )
            .unwrap();
        let mut annotations = AnnotationBuilder::new();
        let annotation_id = id(if record_collision {
            "new-annotation"
        } else {
            "annotated"
        });
        annotations.note(
            annotation_id,
            &StreamHandle::new(crate::stream_name!("incoming")),
            9,
        );
        incoming.annotations = annotations.build();
        let error = target.append(incoming).unwrap_err();
        let conflicting = id(if record_collision {
            "existing"
        } else {
            "annotated"
        });
        assert!(error.to_string().contains(conflicting.as_str()), "{error}");
        assert_eq!(target, before);
    }
}
