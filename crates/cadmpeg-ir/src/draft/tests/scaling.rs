// SPDX-License-Identifier: Apache-2.0

use super::{point, CommitSession, ModelDraft};
use crate::{Annotations, CadIr, Exactness};
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};

#[test]
fn indexed_draft_exactness_admits_thousands_with_linearithmic_work() {
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = 128_000_000;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let mut draft = ModelDraft::new().with_accounting();
    for index in 0..4096 {
        let id = format!("test:model:point#{index:08}");
        draft.exactness(&ctx, &id, Exactness::Derived).unwrap();
    }
    assert_eq!(draft.accounting.exactness.len(), 4096);
    // The removed estimate charges two complete key scans for each lookup.
    let old_charge = (1_u64..=4096).sum::<u64>() * 25 * 2;
    assert!(old_charge > policy.limits.max_work_units);
    ctx.finish_session().unwrap();
}

#[test]
fn sparse_draft_commits_admit_thousands_without_copying_prior_annotations() {
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = 512_000_000;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let mut ir = CadIr::empty();
    let mut annotations = Annotations::default();
    let mut session = CommitSession::new(&mut ir, &ctx, None).unwrap();
    for index in 0..4096 {
        let id = format!("test:model:point#{index:08}");
        let mut draft = ModelDraft::new().with_accounting();
        draft.insert(point(&id), &ctx).unwrap();
        draft.exactness(&ctx, &id, Exactness::Derived).unwrap();
        session.commit(draft, &mut annotations).unwrap().unwrap();
    }
    assert_eq!(session.document().model.points.len(), 4096);
    assert_eq!(annotations.exactness().len(), 4096);
    // Each old transaction copies every prior key and bills tree comparisons.
    // Even one comparison per level is a lower bound for that old copy loop.
    let old_copy_charge: u64 = (0_u32..4096)
        .map(|count| {
            (0_u32..count)
                .map(|position| u64::from(32 - position.leading_zeros() + 1) * 32 * 25)
                .sum::<u64>()
        })
        .sum();
    assert!(old_copy_charge > policy.limits.max_work_units);
    drop(session);
    ctx.finish_session().unwrap();
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
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = 256_000_000;
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
            annotations
                .sparse_transaction(&ctx, "native candidate annotations")
                .unwrap(),
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
