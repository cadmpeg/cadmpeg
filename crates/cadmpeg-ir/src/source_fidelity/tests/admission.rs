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
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let policy = cadmpeg_core::decode::DecodePolicy::service();
    let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(&[], &arena, &policy)
        .expect("test context");
    let invalid =
        UnknownRecord::retained(id("bad-link"), 0, vec![1], vec!["raw target text".into()]);
    let mut fidelity = SourceFidelity::default();
    let mut ir = CadIr::empty();
    let incoming = [
        UnknownRecord::retained(id("good"), 0, vec![2], vec![]),
        invalid.clone(),
    ];
    let error = fidelity
        .attach_native_unknown_records(&mut ir, "synthetic", incoming.into(), &ctx)
        .unwrap_err();
    assert!(error.to_string().contains("raw target text"), "{error}");
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
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let policy = cadmpeg_core::decode::DecodePolicy::service();
    let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(&[], &arena, &policy)
        .expect("test context");
    let mut ir = CadIr::empty();
    ir.set_native_unknowns(
        &cadmpeg_test_support::service_decode_context(),
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
            ]
            .into(),
            &ctx,
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
fn charged_native_unknown_attachment_preserves_product_and_retained_wire() {
    let mut prior = CadIr::empty();
    prior
        .set_native_unknowns(
            &cadmpeg_test_support::service_decode_context(),
            "synthetic",
            &[crate::NativeUnknownRecord {
                id: id("prior"),
                links: vec![],
            }],
        )
        .expect("prior native unknown");
    let incoming = vec![
        UnknownRecord::retained(id("a"), 2, vec![1, 2], vec![id("prior").to_string()]),
        UnknownRecord::retained(id("b"), 5, vec![3], vec![]),
    ];
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let policy = cadmpeg_core::decode::DecodePolicy::service();
    let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(&[], &arena, &policy)
        .expect("test context");
    let mut expected_ir = prior.clone();
    let mut expected_fidelity = SourceFidelity::default();
    expected_fidelity
        .attach_native_unknown_records(&mut expected_ir, "synthetic", incoming.clone(), &ctx)
        .expect("plain attachment");

    let mut actual_ir = prior;
    let mut actual_fidelity = SourceFidelity::default();
    actual_fidelity
        .attach_native_unknown_records(&mut actual_ir, "synthetic", incoming, &ctx)
        .expect("charged attachment");
    assert_eq!(actual_ir, expected_ir);
    assert_eq!(actual_fidelity, expected_fidelity);
}

#[test]
fn charged_native_unknown_attachment_refuses_retained_limit_atomically() {
    let mut ir = CadIr::empty();
    let mut fidelity = SourceFidelity::default();
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let mut policy = cadmpeg_core::decode::DecodePolicy::service();
    policy.limits.max_retained_bytes = 0;
    let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(&[], &arena, &policy)
        .expect("test context");
    let error = fidelity
        .attach_native_unknown_records(
            &mut ir,
            "synthetic",
            vec![UnknownRecord::retained(id("a"), 0, vec![1], vec![])],
            &ctx,
        )
        .expect_err("retained identity refusal");
    assert!(matches!(error, cadmpeg_core::CodecError::ResourceLimit(_)));
    assert_eq!(ir, CadIr::empty());
    assert_eq!(fidelity, SourceFidelity::default());
}

#[test]
fn native_unknown_identity_copy_refuses_one_byte_below_its_length() {
    let identity = id("identity");
    let mut policy = cadmpeg_core::decode::DecodePolicy::service();
    policy.limits.max_retained_bytes =
        u64::try_from(identity.as_str().len()).expect("identity length") - 1;
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(&[], &arena, &policy)
        .expect("test context");
    let mut ir = CadIr::empty();
    let mut fidelity = SourceFidelity::default();
    let error = fidelity
        .attach_native_unknown_records(
            &mut ir,
            "synthetic",
            vec![UnknownRecord::retained(identity, 0, vec![1], vec![])],
            &ctx,
        )
        .expect_err("identity copy must be charged before allocation");
    assert!(matches!(error, cadmpeg_core::CodecError::ResourceLimit(_)));
    assert_eq!(ir, CadIr::empty());
    assert_eq!(fidelity, SourceFidelity::default());
}

fn charged_unknown_limit_error(
    policy: &cadmpeg_core::decode::DecodePolicy,
) -> cadmpeg_core::CodecError {
    let mut ir = CadIr::empty();
    ir.set_native_unknowns(
        &cadmpeg_test_support::service_decode_context(),
        "synthetic",
        &[crate::NativeUnknownRecord {
            id: id("prior"),
            links: vec![
                crate::ids::Identity::new(id("target").to_string()).expect("identity grammar")
            ],
        }],
    )
    .expect("prior native unknown");
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(&[], &arena, policy)
        .expect("test context");
    SourceFidelity::default()
        .attach_native_unknown_records(
            &mut ir,
            "synthetic",
            vec![UnknownRecord::retained(id("a"), 0, vec![1], vec![])],
            &ctx,
        )
        .expect_err("native unknown budget refusal")
}

#[test]
fn charged_native_unknown_attachment_refuses_collection_limit() {
    let mut policy = cadmpeg_core::decode::DecodePolicy::service();
    policy.limits.max_collection_items = 0;
    assert!(matches!(
        charged_unknown_limit_error(&policy),
        cadmpeg_core::CodecError::ResourceLimit(_)
    ));
}

#[test]
fn charged_native_unknown_attachment_refuses_nesting_limit() {
    let mut policy = cadmpeg_core::decode::DecodePolicy::service();
    policy.limits.max_recursion_depth = 0;
    assert!(matches!(
        charged_unknown_limit_error(&policy),
        cadmpeg_core::CodecError::ResourceLimit(_)
    ));
}

#[test]
fn charged_native_unknown_attachment_refuses_work_limit() {
    let mut policy = cadmpeg_core::decode::DecodePolicy::service();
    policy.limits.max_work_units = 0;
    assert!(matches!(
        charged_unknown_limit_error(&policy),
        cadmpeg_core::CodecError::ResourceLimit(_)
    ));
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
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let policy = cadmpeg_core::decode::DecodePolicy::service();
    let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(&[], &arena, &policy)
        .expect("test context");
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
                            [UnknownRecord::retained(id("existing"), 0, vec![3], vec![])].into(),
                            &ctx,
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
                    fidelity
                        .attach_native_unknown_records(&mut ir, "synthetic", incoming.into(), &ctx)
                        .map_err(|error| error.to_string())
                } else {
                    fidelity
                        .retain_unknown_records(SourceOwner::Root, incoming)
                        .map_err(|error| error.to_string())
                }
                .unwrap_err();
                assert!(error.contains(invalid.id().as_str()), "{error}");
                assert_eq!(ir, before_ir);
                assert_eq!(fidelity, before_fidelity);
            }
        }
    }
}

#[test]
fn retention_and_attachment_refuse_duplicate_batches_without_mutation() {
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let policy = cadmpeg_core::decode::DecodePolicy::service();
    let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(&[], &arena, &policy)
        .expect("test context");
    for attach in [false, true] {
        for collide_with_existing in [false, true] {
            let mut ir = CadIr::empty();
            let mut fidelity = SourceFidelity::default();
            fidelity
                .attach_native_unknown_records(
                    &mut ir,
                    "synthetic",
                    [UnknownRecord::retained(id("existing"), 0, vec![3], vec![])].into(),
                    &ctx,
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
                fidelity
                    .attach_native_unknown_records(&mut ir, "synthetic", incoming.into(), &ctx)
                    .map_err(|error| error.to_string())
            } else {
                fidelity
                    .retain_unknown_records(SourceOwner::Root, incoming)
                    .map_err(|error| error.to_string())
            }
            .unwrap_err();
            assert!(error.contains(duplicate_id.as_str()), "{error}");
            assert_eq!(fidelity, before_fidelity);
            assert_eq!(ir, before_ir);
        }
    }
}

#[test]
fn attachment_preserves_existing_records_and_the_root_owner() {
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let policy = cadmpeg_core::decode::DecodePolicy::service();
    let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(&[], &arena, &policy)
        .expect("test context");
    let mut ir = CadIr::empty();
    let mut fidelity = SourceFidelity::default();
    for name in ["a", "b"] {
        fidelity
            .attach_native_unknown_records(
                &mut ir,
                "synthetic",
                [UnknownRecord::retained(id(name), 0, vec![1], vec![])].into(),
                &ctx,
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
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let policy = cadmpeg_core::decode::DecodePolicy::service();
    let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(&[], &arena, &policy)
        .expect("test context");
    let mut ir = CadIr::empty();
    ir.native
        .namespace_mut("synthetic")
        .set_arena(
            &crate::native::test_ctx(),
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
            [UnknownRecord::retained(id("new"), 0, vec![1], vec![])].into(),
            &ctx,
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
        existing
            .note(
                &cadmpeg_test_support::service_decode_context(),
                id("annotated"),
                &StreamHandle::new(
                    &cadmpeg_test_support::service_decode_context(),
                    crate::stream_name!("original"),
                    "fixture stream handle",
                )
                .unwrap(),
                7,
                None,
            )
            .unwrap();
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
        annotations
            .note(
                &cadmpeg_test_support::service_decode_context(),
                annotation_id,
                &StreamHandle::new(
                    &cadmpeg_test_support::service_decode_context(),
                    crate::stream_name!("incoming"),
                    "fixture stream handle",
                )
                .unwrap(),
                9,
                None,
            )
            .unwrap();
        incoming.annotations = annotations.build();
        let error =
            crate::test_support::with_service_decode_context(|ctx| target.append(ctx, incoming))
                .unwrap_err();
        let conflicting = id(if record_collision {
            "existing"
        } else {
            "annotated"
        });
        assert!(error.to_string().contains(conflicting.as_str()), "{error}");
        assert_eq!(target, before);
    }
}

#[test]
fn decode_retention_preserves_wire_and_refuses_atomically() {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
    use cadmpeg_core::CodecError;
    for (dimension, operation) in [
        (ResourceDimension::WorkUnits, "retain source record batch"),
        (
            ResourceDimension::RetainedBytes,
            "retain source record owner",
        ),
        (ResourceDimension::MaterializedBytes, "stage source records"),
        (ResourceDimension::CollectionItems, "stage source records"),
    ] {
        let run = |cap| {
            let arena = DecodeArena::new();
            let mut policy = DecodePolicy::service();
            policy.limits.max_materialized_bytes = 4096;
            match dimension {
                ResourceDimension::WorkUnits => policy.limits.max_work_units = cap,
                ResourceDimension::RetainedBytes => policy.limits.max_retained_bytes = cap,
                ResourceDimension::MaterializedBytes => policy.limits.max_materialized_bytes = cap,
                ResourceDimension::CollectionItems => policy.limits.max_collection_items = cap,
                _ => unreachable!(),
            }
            let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy)?;
            let records = vec![
                UnknownRecord::retained(id("first"), 2, vec![1, 2], vec![]),
                UnknownRecord::unavailable(
                    id("second"),
                    5,
                    1,
                    Sha256Digest::digest(b"a").as_str(),
                    vec![],
                ),
            ];
            let mut expected = SourceFidelity::default();
            expected
                .retain_unknown_records("owner", records.clone())
                .unwrap();
            let mut actual = SourceFidelity::default();
            let result = actual.retain_unknown_records_for_decode(&ctx, "owner", records);
            match &result {
                Err(CodecError::ResourceLimit(limit)) => {
                    assert_eq!(actual, SourceFidelity::default());
                    assert!(
                        matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(original)) if original == *limit)
                    );
                }
                Ok(()) => {
                    assert_eq!(actual, expected);
                    drop(ctx.reserve_scoped_limit(
                        policy.limits.max_materialized_bytes,
                        "source staging released",
                    )?);
                    ctx.finish_session()?;
                }
                Err(error) => panic!("unexpected refusal: {error}"),
            }
            result
        };
        cadmpeg_test_support::refusal::resource_limit_at(dimension, operation, run);
        run(if dimension == ResourceDimension::MaterializedBytes {
            4096
        } else {
            u64::MAX
        })
        .unwrap();
    }
}

#[test]
fn attachment_moves_incoming_link_buffers_and_admits_their_grammar() {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
    use cadmpeg_core::CodecError;
    let run = |cap| {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_work_units = cap;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy)?;
        let text = id("target").to_string();
        let pointer = text.as_ptr();
        let mut ir = CadIr::empty();
        let mut fidelity = SourceFidelity::default();
        let result = fidelity.attach_native_unknown_records(
            &mut ir,
            "synthetic",
            vec![UnknownRecord::retained(
                id("source"),
                2,
                vec![1, 2],
                vec![text],
            )],
            &ctx,
        );
        match &result {
            Err(CodecError::ResourceLimit(limit)) => {
                assert_eq!(ir, CadIr::empty());
                assert_eq!(fidelity, SourceFidelity::default());
                assert!(
                    matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(original)) if original == *limit)
                );
            }
            Ok(()) => {
                let fields =
                    ir.native.namespace("synthetic").unwrap().arenas()["unknowns"][0].fields();
                let text = fields["links"][0].as_str().unwrap();
                assert_eq!(text, id("target").as_str());
                assert_eq!(text.as_ptr(), pointer);
                assert_eq!(
                    fidelity
                        .retained_record(id("source").as_str())
                        .unwrap()
                        .data(),
                    Some(&[1, 2][..])
                );
                ctx.finish_session()?;
            }
            Err(error) => panic!("unexpected refusal: {error}"),
        }
        result
    };
    cadmpeg_test_support::refusal::resource_limit_at(
        ResourceDimension::WorkUnits,
        "native unknown product link text",
        run,
    );
    run(u64::MAX).unwrap();
}

#[test]
fn invalid_digest_does_not_copy_an_unused_source_owner() {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};
    use cadmpeg_core::CodecError;
    let owner = "owner".repeat(16_384);
    let records = vec![UnknownRecord::unavailable(
        id("bad-digest"),
        0,
        1,
        "invalid",
        vec![],
    )];
    let expected = SourceFidelity::default()
        .retain_unknown_records(owner.as_str(), records.clone())
        .unwrap_err()
        .to_string();
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_retained_bytes = 1024;
    policy.limits.max_work_units = 1024;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let mut fidelity = SourceFidelity::default();
    let CodecError::Malformed(message) = fidelity
        .retain_unknown_records_for_decode(&ctx, &owner, records)
        .unwrap_err()
    else {
        panic!("invalid digest must report its evidence without copying the owner");
    };
    assert_eq!(message, expected);
    assert_eq!(fidelity, SourceFidelity::default());
    ctx.finish_session().unwrap();
}

#[test]
fn source_annotation_collision_admits_its_final_diagnostic() {
    use crate::annotations::{AnnotationBuilder, StreamHandle};
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
    use cadmpeg_core::CodecError;
    let fixture = cadmpeg_test_support::service_decode_context();
    let mut annotations = AnnotationBuilder::new();
    annotations
        .note(
            &fixture,
            id("conflict"),
            &StreamHandle::new(&fixture, crate::stream_name!("fixture"), "fixture stream").unwrap(),
            0,
            None,
        )
        .unwrap();
    let annotations = annotations.build();
    let run = |cap, dimension| {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        match dimension {
            ResourceDimension::WorkUnits => policy.limits.max_work_units = cap,
            ResourceDimension::RetainedBytes => policy.limits.max_retained_bytes = cap,
            _ => unreachable!(),
        }
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy)?;
        let mut target = SourceFidelity::with_annotations(annotations.clone());
        let before = target.clone();
        let incoming = SourceFidelity::with_annotations(annotations.clone());
        let result = target.append(&ctx, incoming);
        assert_eq!(target, before);
        match &result {
            Err(CodecError::ResourceLimit(limit)) => assert!(matches!(ctx.finish_session(),
                Err(CodecError::ResourceLimit(original)) if original == *limit)),
            Err(CodecError::Malformed(message)) => {
                assert_eq!(
                    message,
                    &format!("annotation identity collision at {}", id("conflict"))
                );
                ctx.finish_session()?;
            }
            result => panic!("collision must refuse: {result:?}"),
        }
        result
    };
    for dimension in [
        ResourceDimension::WorkUnits,
        ResourceDimension::RetainedBytes,
    ] {
        cadmpeg_test_support::refusal::resource_limit_at(
            dimension,
            "report source annotation collision",
            |cap| run(cap, dimension),
        );
        assert!(matches!(
            run(u64::MAX, dimension),
            Err(CodecError::Malformed(_))
        ));
    }
}
