// SPDX-License-Identifier: Apache-2.0
//! Reader session, carrier and opaque-record resource boundaries.

use crate::loss::StepLossCode;
use crate::reader::Packaging;
use std::collections::HashSet;

#[test]
fn reference_walk_refuses_collection_limit() {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
    use cadmpeg_core::CodecError;
    use std::collections::BTreeSet;

    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_collection_items = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(b"reference", &arena, &policy)
        .expect("root fits collection policy");
    let error = crate::reader::collect_references(
        &crate::parse::Value::Reference(7),
        &mut BTreeSet::new(),
        &ctx,
    )
    .expect_err("reference needs one set item");
    assert!(matches!(error, CodecError::ResourceLimit(limit)
        if limit.dimension == ResourceDimension::CollectionItems
            && limit.operation == "step_reference_walk_ids"));
}

#[test]
fn reference_walk_refuses_depth_limit() {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
    use cadmpeg_core::CodecError;
    use std::collections::BTreeSet;

    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_recursion_depth = 1;
    let (ctx, _) = DecodeContext::from_root_bytes(b"reference", &arena, &policy)
        .expect("root fits depth policy");
    let error = crate::reader::collect_references(
        &crate::parse::Value::List(vec![crate::parse::Value::Reference(7)]),
        &mut BTreeSet::new(),
        &ctx,
    )
    .expect_err("nested reference needs another depth level");
    assert!(matches!(error, CodecError::ResourceLimit(limit)
        if limit.dimension == ResourceDimension::RecursionDepth
            && limit.operation == "step_reference_walk"));
}

#[test]
fn record_closure_pending_refuses_collection_limit() {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
    use cadmpeg_core::CodecError;
    use std::collections::BTreeSet;

    let source = b"ISO-10303-21;HEADER;FILE_DESCRIPTION(('test'),'2;1');FILE_NAME('','',(''),(''),'','','');FILE_SCHEMA(('AP242'));ENDSEC;DATA;#1=ITEM();ENDSEC;END-ISO-10303-21;";
    let (exchange, _) =
        crate::test_support::with_service_context(source, crate::parse::parse_inner)
            .expect("valid exchange");
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_collection_items = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(source, &arena, &policy)
        .expect("root fits collection policy");
    let error = crate::reader::record_closure(&BTreeSet::from([1]), &exchange, &ctx)
        .expect_err("pending root needs one item");
    assert!(matches!(error, CodecError::ResourceLimit(limit)
        if limit.dimension == ResourceDimension::CollectionItems
            && limit.operation == "step_record_closure_pending"));
}

#[test]
fn record_closure_ids_refuse_collection_limit() {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
    use cadmpeg_core::CodecError;
    use std::collections::BTreeSet;

    let source = b"ISO-10303-21;HEADER;FILE_DESCRIPTION(('test'),'2;1');FILE_NAME('','',(''),(''),'','','');FILE_SCHEMA(('AP242'));ENDSEC;DATA;#1=ITEM();ENDSEC;END-ISO-10303-21;";
    let (exchange, _) =
        crate::test_support::with_service_context(source, crate::parse::parse_inner)
            .expect("valid exchange");
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_collection_items = 1;
    let (ctx, _) = DecodeContext::from_root_bytes(source, &arena, &policy)
        .expect("root fits collection policy");
    let error = crate::reader::record_closure(&BTreeSet::from([1]), &exchange, &ctx)
        .expect_err("closure needs one item after the pending root");
    assert!(matches!(error, CodecError::ResourceLimit(limit)
        if limit.dimension == ResourceDimension::CollectionItems
            && limit.operation == "step_record_closure_ids"));
}

#[test]
fn opaque_kind_name_refuses_materialized_limit() {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
    use cadmpeg_core::CodecError;

    let source = b"ISO-10303-21;HEADER;FILE_DESCRIPTION(('test'),'2;1');FILE_NAME('','',(''),(''),'','','');FILE_SCHEMA(('AP242'));ENDSEC;DATA;#1=ITEM();ENDSEC;END-ISO-10303-21;";
    let (exchange, _) =
        crate::test_support::with_service_context(source, crate::parse::parse_inner)
            .expect("valid exchange");
    // The four normalized kind bytes are scratch; the final identity is retained.
    let error = cadmpeg_test_support::refusal::resource_limit_at(
        ResourceDimension::MaterializedBytes,
        "step_opaque_kind_name",
        |limit| {
            let arena = DecodeArena::new();
            let mut policy = DecodePolicy::service();
            policy.limits.max_materialized_bytes = limit;
            let (ctx, _) = DecodeContext::from_root_bytes(source, &arena, &policy).expect("root");
            crate::reader::opaque_record_id(1, &exchange.records()[&1], &ctx)
        },
    );
    assert!(matches!(error, CodecError::ResourceLimit(limit)
        if limit.dimension == ResourceDimension::MaterializedBytes
            && limit.operation == "step_opaque_kind_name"));
}

#[test]
fn opaque_identity_text_refuses_retained_limit() {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
    use cadmpeg_core::CodecError;

    let source = b"ISO-10303-21;HEADER;FILE_DESCRIPTION(('test'),'2;1');FILE_NAME('','',(''),(''),'','','');FILE_SCHEMA(('AP242'));ENDSEC;DATA;#1=ITEM();ENDSEC;END-ISO-10303-21;";
    let (exchange, _) =
        crate::test_support::with_service_context(source, crate::parse::parse_inner)
            .expect("valid exchange");
    // The final identity has sixteen bytes; its four kind bytes are scratch.
    let error = cadmpeg_test_support::refusal::resource_limit_at(
        ResourceDimension::RetainedBytes,
        "step_opaque_identity_text",
        |limit| {
            let arena = DecodeArena::new();
            let mut policy = DecodePolicy::service();
            policy.limits.max_retained_bytes = limit;
            let (ctx, _) = DecodeContext::from_root_bytes(source, &arena, &policy).expect("root");
            crate::reader::opaque_record_id(1, &exchange.records()[&1], &ctx)
        },
    );
    assert!(matches!(error, CodecError::ResourceLimit(limit)
        if limit.dimension == ResourceDimension::RetainedBytes
            && limit.operation == "step_opaque_identity_text"));
}

#[test]
fn opaque_target_map_refuses_collection_limit() {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
    use cadmpeg_core::CodecError;

    let decoded = crate::test_support::exchange::decode_inline(
        "#1=EXAMPLE_RECORD('',#2);#2=LINE('target',#3,#5);#3=CARTESIAN_POINT('',(0.,0.,0.));#4=DIRECTION('',(1.,0.,0.));#5=VECTOR('',#4,1.);",
    );
    let error = cadmpeg_test_support::refusal::resource_limit_at(
        ResourceDimension::CollectionItems,
        "step_opaque_target_records",
        |limit| {
            let arena = DecodeArena::new();
            let mut policy = DecodePolicy::service();
            policy.limits.max_collection_items = limit;
            let (ctx, _) =
                DecodeContext::from_root_bytes(b"target", &arena, &policy).expect("root");
            crate::reader::record_targets(decoded.ir(), |id| Ok(id == 2), &ctx)
        },
    );
    assert!(matches!(error, CodecError::ResourceLimit(limit)
        if limit.dimension == ResourceDimension::CollectionItems
            && limit.operation == "step_opaque_target_records"));
}

#[test]
fn opaque_kind_count_refuses_collection_limit() {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
    use cadmpeg_core::CodecError;
    use std::collections::BTreeMap;

    let source = b"ISO-10303-21;HEADER;FILE_DESCRIPTION(('test'),'2;1');FILE_NAME('','',(''),(''),'','','');FILE_SCHEMA(('AP242'));ENDSEC;DATA;#1=ITEM();ENDSEC;END-ISO-10303-21;";
    let (exchange, _) =
        crate::test_support::with_service_context(source, crate::parse::parse_inner)
            .expect("valid exchange");
    // Admit the borrowed-name slots before refusing the kind map entry.
    let error = cadmpeg_test_support::refusal::resource_limit_at(
        ResourceDimension::CollectionItems,
        "step_opaque_kind_counts",
        |cap| {
            let arena = DecodeArena::new();
            let mut policy = DecodePolicy::service();
            policy.limits.max_collection_items = cap;
            let (ctx, _) = DecodeContext::from_root_bytes(source, &arena, &policy).expect("root");
            crate::reader::count_unknown_kind(&mut BTreeMap::new(), &exchange.records()[&1], &ctx)
        },
    );
    assert!(matches!(error, CodecError::ResourceLimit(limit)
        if limit.dimension == ResourceDimension::CollectionItems
            && limit.operation == "step_opaque_kind_counts"));
}

#[test]
fn opaque_record_collections_refuse_caller_limits() {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
    use cadmpeg_core::CodecError;
    use std::collections::BTreeSet;

    let source = b"ISO-10303-21;HEADER;FILE_DESCRIPTION(('test'),'2;1');FILE_NAME('','',(''),(''),'','','');FILE_SCHEMA(('AP242'));ENDSEC;DATA;#1=EXAMPLE_RECORD(#2);#2=EXAMPLE_RECORD();ENDSEC;END-ISO-10303-21;";
    let (exchange, diagnostics) =
        crate::test_support::with_service_context(source, crate::parse::parse_inner)
            .expect("valid opaque exchange");
    let arena = DecodeArena::new();
    let mut observed = BTreeSet::new();
    for limit in 0..1024 {
        let mut policy = DecodePolicy::service();
        policy.limits.max_collection_items = limit;
        let (ctx, _) = DecodeContext::from_root_bytes(source, &arena, &policy)
            .expect("root fits collection policy");
        if let Err(CodecError::ResourceLimit(refusal)) = crate::reader::decode_exchange_mode(
            source,
            &mut exchange.clone(),
            &diagnostics,
            crate::reader::DecodeMode::Decode(Packaging::Bare),
            &ctx,
        ) {
            if refusal.dimension == ResourceDimension::CollectionItems {
                observed.insert(refusal.operation);
            }
        }
        if [
            "step_opaque_ids",
            "step_opaque_sources",
            "step_opaque_records",
            "step_opaque_links",
        ]
        .iter()
        .all(|operation| observed.contains(operation))
        {
            break;
        }
    }
    for operation in [
        "step_opaque_ids",
        "step_opaque_sources",
        "step_opaque_records",
        "step_opaque_links",
    ] {
        assert!(
            observed.contains(operation),
            "missing refusal at {operation}; observed {observed:?}"
        );
    }
}

fn stage_refuses_at_collection_limit(
    operation: &str,
    make_stage: impl Fn() -> crate::reader::StageOutcome<()>,
) -> bool {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
    use cadmpeg_core::CodecError;

    let source = b"ISO-10303-21;HEADER;FILE_DESCRIPTION(('test'),'2;1');FILE_NAME('','',(''),(''),'','','');FILE_SCHEMA(('AP242'));ENDSEC;DATA;#1=ITEM();ENDSEC;END-ISO-10303-21;";
    let (exchange, diagnostics) =
        crate::test_support::with_service_context(source, crate::parse::parse_inner)
            .expect("valid exchange");
    let arena = DecodeArena::new();
    for limit in 0..128 {
        let mut policy = DecodePolicy::service();
        policy.limits.max_collection_items = limit;
        let (ctx, _) = DecodeContext::from_root_bytes(source, &arena, &policy)
            .expect("root fits collection policy");
        let Ok(mut session) = crate::reader::StepDecodeSession::new(
            &exchange,
            &diagnostics,
            &ctx,
            crate::reader::DecodeMode::Inspect,
        ) else {
            continue;
        };
        let mut stage = make_stage();
        if matches!(
            session.absorb(&mut stage),
            Err(CodecError::ResourceLimit(refusal))
                if refusal.dimension == ResourceDimension::CollectionItems
                    && refusal.operation == operation
        ) {
            return true;
        }
    }
    false
}

#[test]
fn stage_claims_refuse_collection_limit() {
    assert!(stage_refuses_at_collection_limit(
        "step_stage_claims",
        || crate::reader::StageOutcome {
            value: (),
            claims: std::collections::BTreeSet::from([1]),
            losses: Vec::new(),
            notes: Vec::new(),
        }
    ));
}

#[test]
fn stage_losses_refuse_collection_limit() {
    assert!(stage_refuses_at_collection_limit(
        "step_stage_losses",
        || crate::reader::StageOutcome {
            value: (),
            claims: std::collections::BTreeSet::new(),
            losses: vec![StepLossCode::DecodeWarning.note("test")],
            notes: Vec::new(),
        }
    ));
}

#[test]
fn stage_notes_refuse_collection_limit() {
    assert!(stage_refuses_at_collection_limit(
        "step_stage_notes",
        || crate::reader::StageOutcome {
            value: (),
            claims: std::collections::BTreeSet::new(),
            losses: Vec::new(),
            notes: vec!["stage note".into()],
        }
    ));
}

#[test]
fn byte_accounting_note_refuses_collection_limit() {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
    use cadmpeg_core::CodecError;

    let source = b"ISO-10303-21;HEADER;FILE_DESCRIPTION(('test'),'2;1');FILE_NAME('','',(''),(''),'','','');FILE_SCHEMA(('AP242'));ENDSEC;DATA;#1=EXAMPLE_RECORD();ENDSEC;END-ISO-10303-21;";
    let (exchange, diagnostics) =
        crate::test_support::with_service_context(source, crate::parse::parse_inner)
            .expect("valid exchange");
    let arena = DecodeArena::new();
    let refused = (0..1024).any(|limit| {
        let mut policy = DecodePolicy::service();
        policy.limits.max_collection_items = limit;
        let (ctx, _) = DecodeContext::from_root_bytes(source, &arena, &policy)
            .expect("root fits collection policy");
        let refused = matches!(
            crate::reader::decode_exchange_mode(
                source,
                &mut exchange.clone(),
                &diagnostics,
                crate::reader::DecodeMode::Inspect,
                &ctx,
            ),
            Err(CodecError::ResourceLimit(refusal))
                if refusal.dimension == ResourceDimension::CollectionItems
                    && refusal.operation == "step_byte_accounting_note"
        );
        refused
    });
    assert!(refused, "byte accounting note must charge its vector item");
}

#[test]
fn opaque_preservation_loss_text_refuses_retained_limit() {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
    use cadmpeg_core::CodecError;

    let source = b"ISO-10303-21;HEADER;FILE_DESCRIPTION(('test'),'2;1');FILE_NAME('','',(''),(''),'','','');FILE_SCHEMA(('AP242'));ENDSEC;DATA;#1=EXAMPLE_RECORD();ENDSEC;END-ISO-10303-21;";
    let (exchange, diagnostics) =
        crate::test_support::with_service_context(source, crate::parse::parse_inner)
            .expect("valid exchange");
    let arena = DecodeArena::new();
    let refused = {
        let error = cadmpeg_test_support::refusal::resource_limit_at(
            ResourceDimension::RetainedBytes,
            "step_opaque_preservation_loss_text",
            |limit| {
                let mut policy = DecodePolicy::service();
                policy.limits.max_retained_bytes = limit;
                let (ctx, _) = DecodeContext::from_root_bytes(source, &arena, &policy)
                    .expect("root fits retained policy");

                (crate::reader::decode_exchange_mode(
                    source,
                    &mut exchange.clone(),
                    &diagnostics,
                    crate::reader::DecodeMode::Inspect,
                    &ctx,
                ))
                .map(|_| ())
            },
        );
        matches!(Err::<(), CodecError>(error), Err(CodecError::ResourceLimit(refusal))
                if refusal.dimension == ResourceDimension::RetainedBytes
                    && refusal.operation == "step_opaque_preservation_loss_text")
    };
    assert!(refused, "opaque loss text must charge retained bytes");
}

#[test]
fn dialect_match_copy_refuses_collection_limit() {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
    use cadmpeg_core::CodecError;
    use std::collections::BTreeSet;

    let source = b"ISO-10303-21;HEADER;FILE_DESCRIPTION(('test'),'2;1');FILE_NAME('','',(''),(''),'','','');FILE_SCHEMA(('AP242'));ENDSEC;DATA;#1=ITEM();ENDSEC;END-ISO-10303-21;";
    let (exchange, diagnostics) =
        crate::test_support::with_service_context(source, crate::parse::parse_inner)
            .expect("valid exchange");
    let arena = DecodeArena::new();
    let refused = (0..256).any(|limit| {
        let mut policy = DecodePolicy::service();
        policy.limits.max_collection_items = limit;
        let (ctx, _) = DecodeContext::from_root_bytes(source, &arena, &policy)
            .expect("root fits collection policy");
        let Ok(session) = crate::reader::StepDecodeSession::new(
            &exchange,
            &diagnostics,
            &ctx,
            crate::reader::DecodeMode::Inspect,
        ) else {
            return false;
        };
        let refused = matches!(
            session.into_result(cadmpeg_ir::SourceFidelity::default(), BTreeSet::new()),
            Err(CodecError::ResourceLimit(refusal))
                if refusal.dimension == ResourceDimension::CollectionItems
                    && refusal.operation == "copy STEP dialect layer"
        );
        refused
    });
    assert!(refused, "dialect declaration copy must charge each item");
}

#[test]
fn dialect_match_copy_refuses_retained_limit() {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
    use cadmpeg_core::CodecError;
    use std::collections::BTreeSet;

    let source = b"ISO-10303-21;HEADER;FILE_DESCRIPTION(('test'),'2;1');FILE_NAME('','',(''),(''),'','','');FILE_SCHEMA(('AP242'));ENDSEC;DATA;#1=ITEM();ENDSEC;END-ISO-10303-21;";
    let (exchange, diagnostics) =
        crate::test_support::with_service_context(source, crate::parse::parse_inner)
            .expect("valid exchange");
    let arena = DecodeArena::new();
    let refused = {
        let error = cadmpeg_test_support::refusal::resource_limit_at(
            ResourceDimension::RetainedBytes,
            "copy STEP dialect layer",
            |limit| {
                let mut policy = DecodePolicy::service();
                policy.limits.max_retained_bytes = limit;
                let (ctx, _) = DecodeContext::from_root_bytes(source, &arena, &policy)
                    .expect("root fits retained policy");
                let session = crate::reader::StepDecodeSession::new(
                    &exchange,
                    &diagnostics,
                    &ctx,
                    crate::reader::DecodeMode::Inspect,
                )?;

                (session.into_result(cadmpeg_ir::SourceFidelity::default(), BTreeSet::new()))
                    .map(|_| ())
            },
        );
        matches!(Err::<(), CodecError>(error), Err(CodecError::ResourceLimit(refusal))
                if refusal.dimension == ResourceDimension::RetainedBytes
                    && refusal.operation == "copy STEP dialect layer")
    };
    assert!(
        refused,
        "dialect declaration copy must charge retained text"
    );
}

#[test]
fn owned_pcurve_identity_refuses_collection_limit() {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
    use cadmpeg_core::CodecError;
    use std::collections::BTreeSet;

    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_collection_items = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(b"pcurve", &arena, &policy)
        .expect("root fits collection policy");
    let error =
        crate::reader::insert_retained_identity(&mut BTreeSet::new(), "step:data:pcurve#1", &ctx)
            .expect_err("owned ID needs one set item");
    assert!(matches!(error, CodecError::ResourceLimit(limit)
        if limit.dimension == ResourceDimension::CollectionItems
            && limit.operation == "step_owned_pcurve_ids"));
}

#[test]
fn owned_pcurve_identity_refuses_retained_limit() {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
    use cadmpeg_core::CodecError;
    use std::collections::BTreeSet;

    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_retained_bytes = 3;
    let (ctx, _) = DecodeContext::from_root_bytes(b"pcurve", &arena, &policy)
        .expect("root fits retained policy");
    let error =
        crate::reader::insert_retained_identity(&mut BTreeSet::new(), "step:data:pcurve#1", &ctx)
            .expect_err("owned identity text exceeds three bytes");
    assert!(matches!(error, CodecError::ResourceLimit(limit)
        if limit.dimension == ResourceDimension::RetainedBytes
            && limit.operation == "step_owned_pcurve_identity"));
}

#[test]
fn unowned_pcurve_set_refuses_collection_limit() {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
    use cadmpeg_core::CodecError;

    let source = b"ISO-10303-21;HEADER;FILE_DESCRIPTION(('test'),'2;1');FILE_NAME('','',(''),(''),'','','');FILE_SCHEMA(('AP242'));ENDSEC;DATA;#1=PCURVE('',#2,#3);#2=ITEM();#3=ITEM();ENDSEC;END-ISO-10303-21;";
    let (exchange, _) =
        crate::test_support::with_service_context(source, crate::parse::parse_inner)
            .expect("valid exchange");
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_collection_items = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(source, &arena, &policy)
        .expect("root fits collection policy");
    let error = crate::reader::retain_unowned_carriers(
        &exchange,
        &mut cadmpeg_ir::CadIr::empty(),
        &mut HashSet::new(),
        (
            &mut Vec::new(),
            &std::cell::RefCell::new(ctx.reserve_scoped(0, "report fixture").expect("scope")),
        ),
        &ctx,
    )
    .expect_err("unowned pcurve needs one set item");
    assert!(matches!(error, CodecError::ResourceLimit(limit)
        if limit.dimension == ResourceDimension::CollectionItems
            && limit.operation == "step_unowned_pcurves"));
}

fn point_ir(with_source: bool) -> cadmpeg_ir::CadIr {
    let mut ir = cadmpeg_ir::CadIr::empty();
    let identity =
        cadmpeg_ir::ids::Identity::new("step:data:point#1").expect("valid point identity");
    let point = cadmpeg_ir::topology::Point::new(
        cadmpeg_ir::ids::PointId::from(identity),
        cadmpeg_ir::features::FinitePoint3::new(cadmpeg_ir::math::Point3::new(0.0, 0.0, 0.0))
            .expect("finite point"),
        with_source.then(|| {
            crate::reader::step_source_association(
                &cadmpeg_test_support::service_decode_context(),
                1,
                None,
            )
            .unwrap()
        }),
    );
    ir.model.points.push(point);
    ir
}

#[test]
fn unowned_direct_carriers_refuse_collection_limit() {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
    use cadmpeg_core::CodecError;

    let source = b"ISO-10303-21;HEADER;FILE_DESCRIPTION(('test'),'2;1');FILE_NAME('','',(''),(''),'','','');FILE_SCHEMA(('AP242'));ENDSEC;DATA;#1=CARTESIAN_POINT('',(0.,0.,0.));ENDSEC;END-ISO-10303-21;";
    let (exchange, _) =
        crate::test_support::with_service_context(source, crate::parse::parse_inner)
            .expect("valid exchange");
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_collection_items = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(source, &arena, &policy)
        .expect("root fits collection policy");
    let error = crate::reader::retain_unowned_carriers(
        &exchange,
        &mut point_ir(false),
        &mut HashSet::new(),
        (
            &mut Vec::new(),
            &std::cell::RefCell::new(ctx.reserve_scoped(0, "report fixture").expect("scope")),
        ),
        &ctx,
    )
    .expect_err("free point needs one carrier set item");
    assert!(matches!(error, CodecError::ResourceLimit(limit)
        if limit.dimension == ResourceDimension::CollectionItems
            && limit.operation == "step_unowned_direct_carriers"));
}

#[test]
fn protected_roots_refuse_collection_limit() {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
    use cadmpeg_core::CodecError;

    let source = b"ISO-10303-21;HEADER;FILE_DESCRIPTION(('test'),'2;1');FILE_NAME('','',(''),(''),'','','');FILE_SCHEMA(('AP242'));ENDSEC;DATA;#1=CARTESIAN_POINT('',(0.,0.,0.));#2=PCURVE('',#3,#4);#3=ITEM();#4=ITEM();ENDSEC;END-ISO-10303-21;";
    let (exchange, _) =
        crate::test_support::with_service_context(source, crate::parse::parse_inner)
            .expect("valid exchange");
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_collection_items = 3;
    let (ctx, _) = DecodeContext::from_root_bytes(source, &arena, &policy)
        .expect("root fits collection policy");
    let error = crate::reader::retain_unowned_carriers(
        &exchange,
        &mut point_ir(true),
        &mut HashSet::new(),
        (
            &mut Vec::new(),
            &std::cell::RefCell::new(ctx.reserve_scoped(0, "report fixture").expect("scope")),
        ),
        &ctx,
    )
    .expect_err("protected root needs an additional set item");
    assert!(matches!(error, CodecError::ResourceLimit(limit)
        if limit.dimension == ResourceDimension::CollectionItems
            && limit.operation == "step_unowned_protected_roots"));
}

#[test]
fn protected_root_copy_refuses_collection_limit() {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
    use cadmpeg_core::CodecError;

    let source = b"ISO-10303-21;HEADER;FILE_DESCRIPTION(('test'),'2;1');FILE_NAME('','',(''),(''),'','','');FILE_SCHEMA(('AP242'));ENDSEC;DATA;#1=CARTESIAN_POINT('',(0.,0.,0.));#2=PCURVE('',#3,#4);#3=ITEM();#4=ITEM();ENDSEC;END-ISO-10303-21;";
    let (exchange, _) =
        crate::test_support::with_service_context(source, crate::parse::parse_inner)
            .expect("valid exchange");
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_collection_items = 4;
    let (ctx, _) = DecodeContext::from_root_bytes(source, &arena, &policy)
        .expect("root fits collection policy");
    let error = crate::reader::retain_unowned_carriers(
        &exchange,
        &mut point_ir(true),
        &mut HashSet::new(),
        (
            &mut Vec::new(),
            &std::cell::RefCell::new(ctx.reserve_scoped(0, "report fixture").expect("scope")),
        ),
        &ctx,
    )
    .expect_err("protected root copy needs an additional set item");
    assert!(matches!(error, CodecError::ResourceLimit(limit)
        if limit.dimension == ResourceDimension::CollectionItems
            && limit.operation == "step_unowned_protected_root_copy"));
}
