// SPDX-License-Identifier: Apache-2.0

use crate::assets::{Asset, AssetContent, AssetData};
use crate::draft::CommitSession;
use crate::{Annotations, CadIr};
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};

fn asset_candidate(index: usize) -> CadIr {
    let mut candidate = CadIr::empty();
    candidate.model.assets.push(Asset {
        id: format!("test:model:asset#{index:08}").try_into().unwrap(),
        name: None,
        media_type: None,
        content: AssetContent::Embedded {
            data: AssetData::new(vec![1]).unwrap(),
        },
        native_ref: None,
    });
    candidate
}

fn source_records() -> Vec<crate::UnknownRecord> {
    (0..1024)
        .map(|index| {
            crate::UnknownRecord::retained(
                format!("test:source:unknown#{index:08}")
                    .try_into()
                    .unwrap(),
                0,
                vec![1],
                Vec::new(),
            )
        })
        .collect()
}

#[test]
fn incremental_admission_admits_thousands_at_a_budget_that_refuses_combined_scans() {
    const COUNT: usize = 4096;
    const WORK: u64 = 512_000_000;
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = WORK;
    // Isolate work from cumulative collection charges in the comparison loop.
    policy.limits.max_collection_items = WORK;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let mut session = CommitSession::new(CadIr::empty(), &ctx, Some("test")).unwrap();
    session.replace_unknowns(source_records()).unwrap();
    let annotations = Annotations::default();
    for index in 0..COUNT {
        let changes = annotations
            .sparse_transaction(&ctx, "test admission annotations")
            .unwrap();
        session
            .try_admit_append(
                asset_candidate(index),
                changes,
                crate::RHINO_DRAFT_CHECKS,
                |report, _| {
                    assert!(report.unwrap().is_ok());
                    Ok(Ok::<_, ()>(()))
                },
            )
            .unwrap()
            .unwrap();
        let (report, _report_storage) = ctx
            .with_scoped_storage("test instance admission report", || {
                session.admit_appended(&annotations, crate::RHINO_INSTANCE_CHECKS)
            })
            .unwrap();
        assert!(report.unwrap().is_ok());
    }
    assert_eq!(session.document().model.assets.len(), COUNT);
    drop(session);
    ctx.finish_session().unwrap();

    let arena = DecodeArena::new();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let mut ir = CadIr::empty();
    let records = source_records();
    let mut refused = false;
    for index in 0..COUNT {
        ir.model.assets.extend(asset_candidate(index).model.assets);
        match ctx.with_scoped_storage("test combined admission report", || {
            super::super::admit_with_native_unknowns(
                &ctx,
                &ir,
                ("test", &records),
                Some(&annotations),
                crate::RHINO_DRAFT_CHECKS,
                Vec::new(),
            )
        }) {
            Ok((Ok(report), _report_storage)) => assert!(report.is_ok()),
            Err(cadmpeg_core::CodecError::ResourceLimit(limit)) => {
                assert_eq!(
                    limit.dimension,
                    cadmpeg_core::decode::ResourceDimension::WorkUnits
                );
                refused = true;
                break;
            }
            other => panic!("unexpected combined admission result: {other:?}"),
        }
    }
    assert!(
        refused,
        "the former combined scan exceeds this unchanged budget"
    );
}

fn sourced_point(id: &str) -> crate::topology::Point {
    let mut point = crate::topology::Point::new(
        id.try_into().unwrap(),
        crate::features::FinitePoint3::new(crate::math::Point3::new(0.0, 0.0, 0.0)).unwrap(),
        None,
    );
    point.source_object = Some(crate::provenance::SourceObjectAssociation {
        format: crate::CodecFormat::Rhino,
        geometry_role: None,
        object_id: cadmpeg_core::nonblank_literal!("synthetic"),
        name: None,
        color: None,
        visible: None,
        layer: None,
        instance_path: Vec::new(),
    });
    point
}

#[test]
fn shared_references_and_rejected_identities_keep_combined_admission_guarantees() {
    let ctx = cadmpeg_test_support::service_decode_context();
    let annotations = Annotations::default();
    let mut session = CommitSession::new(CadIr::empty(), &ctx, Some("test")).unwrap();
    let point_id = "test:model:point#earlier";
    let mut first = CadIr::empty();
    first.model.points.push(sourced_point(point_id));
    session
        .try_admit_append(
            first,
            annotations
                .sparse_transaction(&ctx, "first annotations")
                .unwrap(),
            crate::RHINO_DRAFT_CHECKS,
            |report, _| {
                assert!(report.unwrap().is_ok());
                Ok(Ok::<_, ()>(()))
            },
        )
        .unwrap()
        .unwrap();
    for target in [point_id, "test:model:point#missing"] {
        let mut candidate = CadIr::empty();
        let id = if target == point_id {
            "test:model:vertex#accepted"
        } else {
            "test:model:vertex#rejected"
        };
        candidate.model.vertices.push(crate::topology::Vertex {
            id: id.try_into().unwrap(),
            point: target.try_into().unwrap(),
            tolerance: None,
        });
        let suffix = if target == point_id {
            "accepted"
        } else {
            "rejected"
        };
        let shell_id: crate::ids::ShellId =
            format!("test:model:shell#{suffix}").try_into().unwrap();
        let region_id: crate::ids::RegionId =
            format!("test:model:region#{suffix}").try_into().unwrap();
        let body_id: crate::ids::BodyId = format!("test:model:body#{suffix}").try_into().unwrap();
        candidate
            .model
            .shells
            .push(crate::topology::Shell::with_free_vertex(
                shell_id.clone(),
                region_id.clone(),
                id.try_into().unwrap(),
            ));
        candidate.model.regions.push(crate::topology::Region {
            id: region_id.clone(),
            body: body_id.clone(),
            shells: vec![shell_id],
        });
        candidate.model.bodies.push(crate::topology::Body {
            id: body_id,
            kind: crate::topology::BodyKind::General,
            regions: vec![region_id],
            transform: None,
            name: None,
            color: None,
            visible: None,
        });
        let before = session.document().clone();
        let result = session
            .try_admit_append(
                candidate,
                annotations
                    .sparse_transaction(&ctx, "vertex annotations")
                    .unwrap(),
                crate::RHINO_DRAFT_CHECKS,
                |report, _| {
                    let report = report.unwrap();
                    if target == point_id {
                        assert!(report.is_ok(), "{report:?}");
                    } else {
                        assert!(report.findings.iter().any(|finding| finding.check
                            == crate::report::check::Check::ReferentialIntegrity));
                    }
                    if report.is_ok() {
                        Ok(Ok(()))
                    } else {
                        Ok(Err(()))
                    }
                },
            )
            .unwrap();
        if target == point_id {
            assert!(result.is_ok());
        } else {
            assert!(result.is_err());
            assert_eq!(session.document(), &before);
            assert!(!session.contains(id).unwrap());
        }
    }
}

#[test]
fn source_link_mutation_is_checked_after_reusing_admitted_state() {
    let ctx = cadmpeg_test_support::service_decode_context();
    let annotations = Annotations::default();
    let mut session = CommitSession::new(CadIr::empty(), &ctx, Some("test")).unwrap();
    session.replace_unknowns(source_records()).unwrap();
    session
        .try_admit_append(
            asset_candidate(0),
            annotations
                .sparse_transaction(&ctx, "first annotations")
                .unwrap(),
            crate::RHINO_DRAFT_CHECKS,
            |report, _| {
                assert!(report.unwrap().is_ok());
                Ok(Ok::<_, ()>(()))
            },
        )
        .unwrap()
        .unwrap();
    session
        .unknown_links_mut(0)
        .unwrap()
        .unwrap()
        .1
        .push("invalid".into());
    let before = session.document().clone();
    let result = session
        .try_admit_append(
            asset_candidate(1),
            annotations
                .sparse_transaction(&ctx, "next annotations")
                .unwrap(),
            crate::RHINO_DRAFT_CHECKS,
            |report, _| {
                assert!(report.is_err());
                Ok(Err::<(), _>(()))
            },
        )
        .unwrap();
    assert!(result.is_err());
    assert_eq!(session.document(), &before);
}

#[test]
fn native_annotation_field_paths_use_complete_source_records() {
    let ctx = cadmpeg_test_support::service_decode_context();
    let annotations = Annotations::default();
    let source = "test:source:unknown#annotated";
    let mut session = CommitSession::new(CadIr::empty(), &ctx, Some("test")).unwrap();
    session
        .replace_unknowns(vec![crate::UnknownRecord::retained(
            source.try_into().unwrap(),
            0,
            Vec::new(),
            vec!["test:model:asset#00000000".into()],
        )])
        .unwrap();
    for index in 0..2 {
        let mut candidate = asset_candidate(index);
        candidate.model.assets[0].native_ref = Some(String::from(source));
        let mut changes = annotations
            .sparse_transaction(&ctx, "source field annotations")
            .unwrap();
        changes
            .field_exactness(source, "links.0", crate::Exactness::Derived)
            .unwrap();
        session
            .try_admit_append(
                candidate,
                changes,
                crate::RHINO_DRAFT_CHECKS,
                |report, _| {
                    let report = report.unwrap();
                    assert!(report.is_ok(), "{report:?}");
                    assert!(!report
                        .findings
                        .iter()
                        .any(|finding| finding.check == crate::report::check::Check::Annotations));
                    Ok(Ok::<_, ()>(()))
                },
            )
            .unwrap()
            .unwrap();
    }
}

#[test]
fn instance_admission_does_not_certify_annotations_for_the_draft_route() {
    let ctx = cadmpeg_test_support::service_decode_context();
    let mut session = CommitSession::new(CadIr::empty(), &ctx, Some("test")).unwrap();
    session
        .append_model_mut()
        .unwrap()
        .assets
        .extend(asset_candidate(0).model.assets);
    let mut builder = crate::AnnotationBuilder::new();
    builder
        .exactness(&ctx, "test:model:asset#missing", crate::Exactness::Derived)
        .unwrap();
    let annotations = builder.build();
    assert!(session
        .admit_appended(&annotations, crate::RHINO_INSTANCE_CHECKS)
        .unwrap()
        .unwrap()
        .is_ok());
    let report = session
        .admit_appended(&annotations, crate::RHINO_DRAFT_CHECKS)
        .unwrap()
        .unwrap();
    assert!(report
        .findings
        .iter()
        .any(|finding| finding.check == crate::report::check::Check::Annotations));
    let before = session.document().clone();
    let result = session
        .try_admit_append(
            asset_candidate(1),
            annotations
                .sparse_transaction(&ctx, "draft route annotations")
                .unwrap(),
            crate::RHINO_DRAFT_CHECKS,
            |report, _| {
                let report = report.unwrap();
                assert!(report
                    .findings
                    .iter()
                    .any(|finding| finding.check == crate::report::check::Check::Annotations));
                Ok(Err::<(), _>(()))
            },
        )
        .unwrap();
    assert!(result.is_err());
    assert_eq!(session.document(), &before);
}

#[test]
fn sparse_deletion_replaces_invalid_base_annotations_in_combined_admission() {
    let ctx = cadmpeg_test_support::service_decode_context();
    let mut builder = crate::AnnotationBuilder::new();
    let missing = "test:model:asset#missing";
    builder
        .exactness(&ctx, missing, crate::Exactness::Derived)
        .unwrap();
    let mut annotations = builder.build();
    let mut changes = annotations
        .sparse_transaction(&ctx, "annotation deletion")
        .unwrap();
    changes.remove_entity(missing).unwrap();
    let mut session = CommitSession::new(CadIr::empty(), &ctx, Some("test")).unwrap();
    let prepared = session
        .try_admit_append(
            asset_candidate(0),
            changes,
            crate::RHINO_DRAFT_CHECKS,
            |report, changes| {
                assert!(report.unwrap().is_ok());
                Ok(Ok::<_, ()>(changes.prepare()?))
            },
        )
        .unwrap()
        .unwrap();
    prepared.apply(&mut annotations);
    assert!(annotations.exactness().is_empty());
    assert_eq!(session.document().model.assets.len(), 1);
}

#[test]
fn accounted_commit_does_not_hide_annotation_errors_from_the_next_candidate() {
    let ctx = cadmpeg_test_support::service_decode_context();
    let mut annotations = Annotations::default();
    let mut session = CommitSession::new(CadIr::empty(), &ctx, Some("test")).unwrap();
    session
        .try_admit_append(
            asset_candidate(0),
            annotations
                .sparse_transaction(&ctx, "first annotations")
                .unwrap(),
            crate::RHINO_DRAFT_CHECKS,
            |report, _| {
                assert!(report.unwrap().is_ok());
                Ok(Ok::<_, ()>(()))
            },
        )
        .unwrap()
        .unwrap();
    let missing = "test:model:asset#missing";
    let mut draft = crate::draft::ModelDraft::new().with_accounting();
    draft
        .exactness(&ctx, missing, crate::Exactness::Derived)
        .unwrap();
    session.commit(draft, &mut annotations).unwrap().unwrap();
    let before = session.document().clone();
    let result = session
        .try_admit_append(
            asset_candidate(1),
            annotations
                .sparse_transaction(&ctx, "next annotations")
                .unwrap(),
            crate::RHINO_DRAFT_CHECKS,
            |report, _| {
                let report = report.unwrap();
                assert!(report.findings.iter().any(|finding| finding.check
                    == crate::report::check::Check::Annotations
                    && finding.entity.as_deref() == Some(missing)));
                Ok(Err::<(), _>(()))
            },
        )
        .unwrap();
    assert!(result.is_err());
    assert_eq!(session.document(), &before);
    assert_eq!(
        annotations.exactness()[missing].entity(),
        crate::Exactness::Derived
    );
}

#[test]
fn source_grammar_admission_releases_changed_position_storage_after_each_check() {
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_materialized_bytes = 64 * 1024;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let mut session = CommitSession::new(CadIr::empty(), &ctx, Some("test")).unwrap();
    session
        .replace_unknowns(vec![crate::UnknownRecord::retained(
            "test:source:unknown#only".try_into().unwrap(),
            0,
            Vec::new(),
            Vec::new(),
        )])
        .unwrap();
    session.validate_source_records().unwrap().unwrap();
    for _ in 0..1024 {
        session.unknown_links_mut(0).unwrap().unwrap().1.clear();
        session.validate_source_records().unwrap().unwrap();
        let storage = ctx
            .reserve_scoped(64 * 1024, "source grammar workspace released")
            .unwrap();
        drop(storage);
    }
    drop(session);
    ctx.finish_session().unwrap();
}
