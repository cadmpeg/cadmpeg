// SPDX-License-Identifier: Apache-2.0
//! Shared diamond paths keep annotation selection and charged cursor visits.

use std::collections::{BTreeSet, HashSet};
use std::cell::RefCell;
use std::fmt::Write as _;
use cadmpeg_core::{decode::{DecodePolicy, ResourceDimension}, CodecError};
use cadmpeg_ir::CadIr;

fn diamond() -> crate::parse::Exchange {
    let mut records = String::new();
    for layer in 0..8 {
        let a = 3*layer+1;
        write!(records, "#{a}=ITEM((#{b},#{c}));#{b}=ITEM(#{next});#{c}=ITEM(#{next});", b=a+1, c=a+2, next=a+3).expect("diamond");
    }
    records.push_str("#25=ITEM(#1000,#1001);#1000=TEXT_LITERAL('\\X2\\D83D\\X0\\',$,.LEFT.);#1001=TEXT_LITERAL('shared',$,.LEFT.);#2000=ANNOTATION_TEXT('',#1);");
    super::exchange(&records)
}

#[test]
fn annotation_diamond_preserves_text_claims_and_first_visit_warnings() {
    let exchange = diamond();
    let setup = cadmpeg_test_support::service_decode_context();
    let geometry = crate::reader::geometry::decode(&exchange, &mut CadIr::empty(), &setup).expect("geometry");
    super::with_session(&DecodePolicy::service(), |ctx| {
        let mut index = super::super::AnnotationDiscoveryIndex::new(ctx).expect("index");
        let reports = RefCell::new(ctx.reserve_scoped(0, "test reports").expect("scope"));
        let mut claims = ctx.reserve_scoped(0, "test claims").expect("scope");
        let mut expected = Vec::new();
        let mut expected_used = BTreeSet::new();
        let text = super::super::super::find_annotation_text(2000, &exchange, &mut BTreeSet::new(), (&mut expected_used, &mut claims), (&mut expected, &reports), 0, ctx).expect("reference DFS");
        assert_eq!(text.as_deref(), Some("shared"));
        assert_eq!(expected.len(), 1);
        let mut actual = Vec::new();
        let mut used = BTreeSet::new();
        for _ in 0..2 {
            assert_eq!(index.text(2000, &exchange, &geometry.value, (&mut used, &mut claims), (&mut actual, &reports)).expect("indexed DFS"), text);
        }
        assert_eq!(used, expected_used);
        assert_eq!(&actual[..1], expected);
        assert_eq!(&actual[1..], expected);
        let mut cutoff_expected = Vec::new();
        assert!(super::super::super::find_annotation_text(1, &exchange, &mut BTreeSet::new(), (&mut BTreeSet::new(), &mut claims), (&mut cutoff_expected, &reports), 240, ctx).expect("reference cutoff").is_none());
        let mut summary = super::super::TextSummary::default();
        let mut summary_storage = ctx.reserve_scoped(0, "test summary").expect("scope");
        let mut visited_storage = ctx.reserve_scoped(0, "test visited").expect("scope");
        index.collect_text_summary((1, 240), &exchange, &geometry.value, (&mut HashSet::new(), &mut visited_storage), (&mut summary, &mut summary_storage)).expect("indexed cutoff");
        assert!(matches!(summary.selection, super::super::TextSelection::Absent));
        assert!(summary.warnings.is_empty());
        assert!(cutoff_expected.is_empty());
    });
}

fn diamond_refusal(operation: &str, cutoff: bool) {
    let exchange = diamond();
    let setup = cadmpeg_test_support::service_decode_context();
    let geometry = crate::reader::geometry::decode(&exchange, &mut CadIr::empty(), &setup).expect("geometry");
    cadmpeg_test_support::refusal::resource_limit_at(ResourceDimension::WorkUnits, operation, |limit| {
        let mut policy = DecodePolicy::service();
        policy.limits.max_work_units = limit;
        super::with_session(&policy, |ctx| {
            let result = (|| {
                let mut index = super::super::AnnotationDiscoveryIndex::new(ctx)?;
                if cutoff {
                    let mut summary = super::super::TextSummary::default();
                    let mut summary_storage = ctx.reserve_scoped(0, "test summary")?;
                    let mut visited_storage = ctx.reserve_scoped(0, "test visited")?;
                    index.collect_text_summary((1, 240), &exchange, &geometry.value, (&mut HashSet::new(), &mut visited_storage), (&mut summary, &mut summary_storage))
                } else {
                    let reports = RefCell::new(ctx.reserve_scoped(0, "test reports")?);
                    let mut claims = ctx.reserve_scoped(0, "test claims")?;
                    index.text(2000, &exchange, &geometry.value, (&mut BTreeSet::new(), &mut claims), (&mut Vec::new(), &reports)).map(|_| ())
                }
            })();
            if let Err(CodecError::ResourceLimit(ref refusal)) = result {
                assert_eq!(refusal.operation, operation);
                assert!(refusal.used < 50_000);
                assert_eq!(ctx.resource_refusal(), Some(*refusal));
            }
            result
        })
    });
}

#[test]
fn annotation_diamond_refuses_alias_walk_and_keeps_refusal() {
    diamond_refusal("STEP annotation alias record", false);
}

#[test]
fn annotation_diamond_refuses_diagnostic_cursor_and_keeps_refusal() {
    diamond_refusal("STEP annotation diagnostic cursor traversal", false);
}

#[test]
fn annotation_diamond_refuses_warning_ranges_and_keeps_refusal() {
    diamond_refusal("STEP annotation warning range construction", false);
}

#[test]
fn annotation_diamond_refuses_cutoff_walk_and_keeps_refusal() {
    diamond_refusal("STEP annotation cutoff edge traversal", true);
}

#[test]
fn annotation_diamond_refuses_alias_installation_walk_and_keeps_refusal() {
    diamond_refusal("STEP annotation alias installation", false);
}

#[test]
fn annotation_diamond_refuses_graph_walk_and_keeps_refusal() {
    diamond_refusal("STEP annotation graph parameter traversal", false);
}

#[test]
fn annotation_diamond_refuses_diagnostic_replay_walk_and_keeps_refusal() {
    diamond_refusal("STEP annotation diagnostic replay", false);
}

#[test]
fn annotation_diamond_refuses_loss_replay_walk_and_keeps_refusal() {
    diamond_refusal("STEP annotation loss replay", false);
}

#[test]
fn annotation_diamond_refuses_cutoff_frame_walk_and_keeps_refusal() {
    diamond_refusal("STEP annotation cutoff frame traversal", true);
}
