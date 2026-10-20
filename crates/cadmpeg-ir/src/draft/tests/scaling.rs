// SPDX-License-Identifier: Apache-2.0

use super::CommitSession;
use crate::{Annotations, CadIr};
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};

#[test]
fn successive_session_commits_accumulate_entities_and_exactness() {
    let ctx = cadmpeg_test_support::service_decode_context();
    let mut ir = CadIr::empty();
    let mut annotations = Annotations::default();
    let mut session = CommitSession::new(&mut ir, &ctx, None).unwrap();
    for index in 0..4096 {
        let id = format!("test:model:point#{index:08}");
        let mut draft = super::ModelDraft::new().with_accounting();
        draft.insert(super::point(&id)).unwrap();
        draft.exactness(&id, crate::provenance::Exactness::Derived);
        session.commit(draft, &mut annotations).unwrap().unwrap();
    }
    assert_eq!(session.document().model.points.len(), 4096);
    assert_eq!(annotations.exactness().len(), 4096);
}

#[test]
fn rejected_candidates_keep_the_reusable_admission_facts() {
    use crate::assets::{Asset, AssetContent, AssetData};
    use crate::schema::EntitySchema;
    fn asset(index: usize) -> CadIr {
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
    fn admitted_assets(session: &CommitSession<'_, CadIr>) -> Option<usize> {
        session
            .state
            .admission
            .as_ref()
            .map(|state| state.lengths[<Asset as EntitySchema>::KIND.index()])
    }
    let ctx = cadmpeg_test_support::service_decode_context();
    let annotations = Annotations::default();
    let mut session = CommitSession::new(CadIr::empty(), &ctx, Some("test")).unwrap();
    let mut append = |candidate, accept: bool| {
        session
            .try_admit_append(
                candidate,
                annotations.sparse_transaction(),
                crate::RHINO_DRAFT_CHECKS,
                |report, _| {
                    let valid = report.unwrap().is_ok();
                    Ok(if accept && valid { Ok(()) } else { Err(valid) })
                },
            )
            .unwrap()
    };
    assert_eq!(append(asset(0), true), Ok(()));
    let mut dangling = CadIr::empty();
    dangling.model.vertices.push(crate::topology::Vertex {
        id: "test:model:vertex#dangling".try_into().unwrap(),
        point: "test:model:point#missing".try_into().unwrap(),
        tolerance: None,
    });
    // An invalid candidate and a valid one its caller refuses both roll
    // back; the facts still describe the one admitted asset.
    assert_eq!(append(dangling, true), Err(false));
    assert_eq!(append(asset(1), false), Err(true));
    assert_eq!(admitted_assets(&session), Some(1));
    let mut append = |candidate| {
        session
            .try_admit_append(
                candidate,
                annotations.sparse_transaction(),
                crate::RHINO_DRAFT_CHECKS,
                |report, _| {
                    assert!(report.unwrap().is_ok());
                    Ok(Ok::<_, ()>(()))
                },
            )
            .unwrap()
            .unwrap();
    };
    append(asset(2));
    assert_eq!(admitted_assets(&session), Some(2));
    assert_eq!(session.document().model.assets.len(), 2);
}

#[test]
fn native_candidate_admission_does_not_build_an_unread_prefix_index() {
    use crate::assets::{Asset, AssetContent, AssetData};
    use crate::native::{NativeNamespace, NativeRecord};
    const COUNT: usize = 16384;
    let mut ir = CadIr::empty();
    for index in 0..COUNT {
        ir.model.assets.push(Asset {
            id: format!(
                "test:model:asset#existing-native-candidate-owner-with-a-long-prefix-{index:08}"
            )
            .try_into()
            .unwrap(),
            name: None,
            media_type: None,
            content: AssetContent::Embedded {
                data: AssetData::new(vec![1]).unwrap(),
            },
            native_ref: None,
        });
    }
    let arena = DecodeArena::new();
    let policy = DecodePolicy::service();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let mut session = CommitSession::new(ir, &ctx, Some("test")).unwrap();
    let mut candidate = CadIr::empty();
    let mut native = NativeNamespace::default();
    native.arenas_mut().insert(
        "records".into(),
        vec![NativeRecord::from_identity(
            crate::ids::Identity::new("test:native:record#last".to_owned()).unwrap(),
            None,
        )],
    );
    candidate.native.0.insert("fixture".into(), native);
    let annotations = Annotations::default();
    session
        .try_admit_append(
            candidate,
            annotations.sparse_transaction(),
            crate::RHINO_DRAFT_CHECKS,
            |report, _| {
                assert!(report.unwrap().is_ok());
                Ok(Ok::<_, ()>(()))
            },
        )
        .unwrap()
        .unwrap();
    assert_eq!(session.document().model.assets.len(), COUNT);
    assert_eq!(
        session.document().native.0["fixture"].arenas()["records"][0].id(),
        "test:native:record#last"
    );
    drop(session);
    ctx.finish_session().unwrap();
}
