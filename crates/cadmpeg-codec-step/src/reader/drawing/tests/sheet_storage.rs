// SPDX-License-Identifier: Apache-2.0
//! Sheet selections remain temporary until a drawing consumes them.

use std::cell::RefCell;
use std::collections::{BTreeMap, BTreeSet, HashSet};
use std::fmt::Write as _;

use cadmpeg_core::decode::DecodePolicy;

use super::super::{TargetContext, WrapperCache};

#[test]
fn discarded_sheet_usage_selections_do_not_retain_storage() {
    for indexed in [false, true] {
        let mut records = String::from("#1=ITEM();#2=ITEM();");
        for id in 100..228 {
            write!(records, "#{id}=DRAWING_SHEET_REVISION_USAGE(#1,#2,$);").expect("usage");
        }
        let source = format!("ISO-10303-21;HEADER;FILE_DESCRIPTION(('test'),'2;1');FILE_NAME('','',(''),(''),'','','');FILE_SCHEMA(('AP242'));ENDSEC;DATA;{records}ENDSEC;END-ISO-10303-21;");
        let (exchange, _) =
            crate::test_support::with_service_context(source.as_bytes(), crate::parse::parse_inner)
                .expect("sheet usage exchange");
        let identities = if indexed {
            BTreeMap::from([
                (1, BTreeSet::from(["A".repeat(4096)])),
                (2, BTreeSet::from(["B".repeat(4096)])),
            ])
        } else {
            BTreeMap::new()
        };
        let typed = HashSet::new();
        let documents = BTreeMap::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_retained_bytes = 0;
        policy.limits.max_materialized_bytes = 64 * 1024;
        crate::test_support::with_policy_context(b"", &policy, |_, ctx| {
            let target_context = TargetContext {
                target_identities: &identities,
                known_typed: &typed,
                exchange: &exchange,
                external_documents: &documents,
                wrappers: WrapperCache::new(ctx).expect("wrapper cache"),
            };
            let reports = RefCell::new(ctx.reserve_scoped(0, "sheet reports").expect("scope"));
            let mut losses = Vec::new();
            let mut drawings = BTreeMap::new();
            super::super::add_sheet_revision_usages(
                &exchange,
                &mut drawings,
                &target_context,
                (&mut losses, &reports),
            )
            .expect("discarded selections fit zero retained bytes");
            assert!(drawings.is_empty());
            assert!(losses.is_empty());
            assert_eq!(ctx.resource_refusal(), None);
            drop(target_context);
            drop(drawings);
            drop(losses);
            drop(reports);
            let reservation = ctx
                .reserve_scoped(
                    policy.limits.max_materialized_bytes,
                    "released sheet selections",
                )
                .expect("all temporary selection storage is released");
            drop(reservation);
        });
    }
}

#[test]
fn sheet_usage_refuses_retained_promotion_before_relationship_insertion() {
    use cadmpeg_core::decode::ResourceDimension;
    use cadmpeg_core::CodecError;
    use cadmpeg_ir::drawings::{Drawing, DrawingId, DrawingKind};

    let source = b"ISO-10303-21;HEADER;FILE_DESCRIPTION(('test'),'2;1');FILE_NAME('','',(''),(''),'','','');FILE_SCHEMA(('AP242'));ENDSEC;DATA;#1=ITEM();#2=ITEM();#3=DRAWING_SHEET_REVISION_USAGE(#1,#2,$);ENDSEC;END-ISO-10303-21;";
    let (exchange, _) =
        crate::test_support::with_service_context(source, crate::parse::parse_inner)
            .expect("sheet usage exchange");
    let target = "revision-target";
    let identities = BTreeMap::from([
        (1, BTreeSet::from(["sheet-target".to_owned()])),
        (2, BTreeSet::from([target.to_owned()])),
    ]);
    let typed = HashSet::new();
    let documents = BTreeMap::new();
    let drawing = Drawing {
        id: DrawingId::from(crate::ids::drawing(
            crate::ids::kind!("drawing_sheet_revision"),
            1,
        )),
        object: "sheet".to_owned(),
        kind: DrawingKind::Page,
        runtime_type: "sheet".to_owned(),
        order: 0,
        visible: None,
        relationships: BTreeMap::new(),
        template: None,
        position: None,
        scale: None,
        direction: None,
        rotation_degrees: None,
        parameters: BTreeMap::new(),
        assets: Vec::new(),
        native_ref: "sheet".to_owned(),
    };
    let mut drawings = BTreeMap::from([(1, drawing)]);
    let mut policy = DecodePolicy::service();
    policy.limits.max_retained_bytes = 0;
    crate::test_support::with_policy_context(b"", &policy, |_, ctx| {
        let targets = TargetContext {
            target_identities: &identities,
            known_typed: &typed,
            exchange: &exchange,
            external_documents: &documents,
            wrappers: WrapperCache::new(ctx).expect("wrapper cache"),
        };
        let reports = RefCell::new(ctx.reserve_scoped(0, "promotion reports").expect("scope"));
        let mut losses = Vec::new();
        let CodecError::ResourceLimit(limit) = super::super::add_sheet_revision_usages(
            &exchange,
            &mut drawings,
            &targets,
            (&mut losses, &reports),
        )
        .expect_err("used selection needs retained storage") else {
            panic!("promotion resource refusal")
        };
        assert_eq!(limit.dimension, ResourceDimension::RetainedBytes);
        assert_eq!(limit.operation, "STEP sheet usage target");
        assert_eq!(limit.used, 0);
        assert_eq!(
            limit.additional,
            u64::try_from(target.len()).expect("target length")
        );
        assert_eq!(ctx.resource_refusal(), Some(limit));
        assert!(drawings[&1].relationships.is_empty());
        assert!(losses.is_empty());
    });
}
