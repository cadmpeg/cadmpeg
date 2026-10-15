// SPDX-License-Identifier: Apache-2.0
//! Association order, shared declarations, and storage ownership.

use std::collections::BTreeSet;
use std::fmt::Write as _;

use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
use cadmpeg_core::CodecError;
use cadmpeg_ir::ids::PcurveId;

use super::PcurveAssociations;

fn exchange(records: &str) -> crate::parse::Exchange {
    let mut defined = BTreeSet::new();
    let mut references = BTreeSet::new();
    for part in records.split('#').skip(1) {
        let end = part
            .find(|c: char| !c.is_ascii_digit())
            .unwrap_or(part.len());
        let id = part[..end].parse::<u64>().expect("fixture instance id");
        if part[end..].starts_with('=') {
            defined.insert(id);
        } else {
            references.insert(id);
        }
    }
    let mut source = String::from("ISO-10303-21;HEADER;FILE_DESCRIPTION(('test'),'4;2');FILE_NAME('','',(''),(''),'','','');FILE_SCHEMA(('AP242'));ENDSEC;REFERENCE;");
    for id in references.difference(&defined) {
        write!(source, "#{id}=<outside.step#root>;").expect("external fixture reference");
    }
    write!(source, "ENDSEC;DATA;{records}ENDSEC;END-ISO-10303-21;").expect("fixture records");
    let mut policy = DecodePolicy::desktop();
    policy.limits.max_work_units = 16_000_000_000;
    crate::test_support::with_policy_context(source.as_bytes(), &policy, crate::parse::parse_inner)
        .expect("association source parses")
        .0
}

#[test]
fn association_facts_preserve_surface_order_duplicates_and_missing_declarations() {
    let exchange = exchange("#1=SURFACE_CURVE('',#10,(#6,#2,#6,#4,#9,#7,#8),.PCURVE_S1.);#2=PCURVE('',#5,#11);#4=PCURVE('',#3,#11);#6=PCURVE('',#3,#11);#7=DUMMY();#8=PCURVE('',#3,#11);");
    let ctx = cadmpeg_test_support::service_decode_context();
    let decoded = BTreeSet::from([2, 6, 8, 9]);
    let mut facts = PcurveAssociations::new(&exchange, &decoded, &ctx).expect("empty facts");
    let first = facts.candidates(1, 3).expect("first surface");
    assert_eq!(
        first.iter().map(PcurveId::as_str).collect::<Vec<_>>(),
        [
            "step:data:pcurve#6",
            "step:data:pcurve#6",
            "step:data:pcurve#8"
        ]
    );
    let second = facts.candidates(1, 5).expect("second surface");
    assert_eq!(
        second.iter().map(PcurveId::as_str).collect::<Vec<_>>(),
        ["step:data:pcurve#2"]
    );
    assert!(facts.candidates(1, 77).expect("unused surface").is_empty());
    assert!(facts.candidates(99, 3).expect("missing curve").is_empty());
    assert_eq!(facts.curves.len(), 2);
    assert_eq!(facts.pcurve_surfaces.len(), 6);
    assert!(facts.contains(1, 3, 6).expect("valid seam membership"));
    assert!(!facts.contains(1, 5, 6).expect("wrong surface"));
    assert!(!facts.contains(1, 3, 4).expect("undecoded pcurve"));
    assert!(!facts.contains(1, 3, 7).expect("non-pcurve"));
    assert!(!facts.contains(1, 3, 9).expect("missing pcurve"));
    assert_eq!(ctx.resource_refusal(), None);
}

fn shared_declaration_work(count: usize) -> u64 {
    let mut records = String::from("#1=(REPRESENTATION_ITEM('') ");
    for index in 0..64 * count {
        write!(records, "UNRELATED{index}() ").expect("append partial");
    }
    records.push_str("PCURVE('',#3,#4));");
    for index in 0..count {
        write!(
            records,
            "#{}=SURFACE_CURVE('',#5,(#1),.PCURVE_S1.);",
            1000 + index
        )
        .expect("append curve");
    }
    let exchange = exchange(&records);
    let ctx = cadmpeg_test_support::service_decode_context();
    let decoded = BTreeSet::from([1]);
    let mut facts = PcurveAssociations::new(&exchange, &decoded, &ctx).expect("empty facts");
    for index in 0..count {
        let candidates = facts
            .candidates(u64::try_from(1000 + index).expect("curve id fits"), 3)
            .expect("shared declaration");
        assert_eq!(candidates.len(), 1);
        assert_eq!(candidates[0].as_str(), "step:data:pcurve#1");
    }
    assert_eq!(facts.pcurve_surfaces.len(), 1);
    assert_eq!(facts.curves.len(), count);
    let CodecError::ResourceLimit(limit) = ctx
        .charge_work(u64::MAX, "shared pcurve declaration work probe")
        .expect_err("work probe")
    else {
        panic!("work refusal");
    };
    assert_eq!(limit.dimension, ResourceDimension::WorkUnits);
    limit.used
}

#[test]
fn curves_share_one_completed_pcurve_declaration() {
    let small = shared_declaration_work(128);
    let large = shared_declaration_work(256);
    assert!(
        large < 3 * small,
        "shared declaration work grew from {small} to {large}"
    );
}

#[test]
fn association_facts_release_storage_after_use() {
    let exchange = exchange("#1=SURFACE_CURVE('',#4,(#2),.PCURVE_S1.);#2=PCURVE('',#3,#5);");
    let ctx = cadmpeg_test_support::service_decode_context();
    let decoded = BTreeSet::from([2]);
    {
        let mut facts = PcurveAssociations::new(&exchange, &decoded, &ctx).expect("empty facts");
        assert_eq!(facts.candidates(1, 3).expect("candidate").len(), 1);
    }
    let CodecError::ResourceLimit(limit) = ctx
        .reserve_scoped(u64::MAX, "association storage probe")
        .expect_err("storage probe")
    else {
        panic!("materialized refusal");
    };
    assert_eq!(limit.dimension, ResourceDimension::MaterializedBytes);
    assert_eq!(limit.used, 0);
}

#[test]
fn association_candidate_slots_propagate_collection_refusal() {
    let exchange = exchange("#1=SURFACE_CURVE('',#4,(#2,#2,#2),.PCURVE_S1.);#2=PCURVE('',#3,#5);");
    let error = cadmpeg_test_support::refusal::resource_limit_at(
        ResourceDimension::CollectionItems,
        "step_associated_pcurves",
        |cap| {
            let arena = DecodeArena::new();
            let mut policy = DecodePolicy::service();
            policy.limits.max_collection_items = cap;
            let (ctx, _) =
                DecodeContext::from_root_bytes(b"", &arena, &policy).expect("empty root fits");
            let decoded = BTreeSet::from([2]);
            let mut facts =
                PcurveAssociations::new(&exchange, &decoded, &ctx).expect("empty facts");
            let result = facts.candidates(1, 3).map(|_| ());
            if let Err(CodecError::ResourceLimit(limit)) = &result {
                assert_eq!(ctx.resource_refusal(), Some(*limit));
                assert!(facts.curves.is_empty());
            }
            result
        },
    );
    assert!(
        matches!(error,CodecError::ResourceLimit(limit) if limit.operation=="step_associated_pcurves" && limit.dimension==ResourceDimension::CollectionItems)
    );
}

#[test]
fn explicit_association_accepts_the_original_simple_carrier_list() {
    let exchange = exchange("#1=DUMMY('',#4,(#2));#2=PCURVE('',#3,#5);");
    let ctx = cadmpeg_test_support::service_decode_context();
    let decoded = BTreeSet::from([2]);
    let mut facts = PcurveAssociations::new(&exchange, &decoded, &ctx).expect("empty facts");
    assert!(facts
        .candidates(1, 3)
        .expect("non-surface curve")
        .is_empty());
    assert!(facts.contains(1, 3, 2).expect("explicit seam source list"));
}

fn repeated_membership_work(count: u64) -> u64 {
    let mut records = String::from("#1=SURFACE_CURVE('',#4,(");
    for index in 0..count {
        if index != 0 {
            records.push(',');
        }
        write!(records, "#{}", 1000 + index).expect("append reference");
    }
    records.push_str("),.PCURVE_S1.);");
    for index in 0..count {
        write!(records, "#{}=PCURVE('',#3,#5);", 1000 + index).expect("append declaration");
    }
    let exchange = exchange(&records);
    let decoded = (1000..1000 + count).collect();
    let ctx = cadmpeg_test_support::service_decode_context();
    let mut facts = PcurveAssociations::new(&exchange, &decoded, &ctx).expect("empty facts");
    for index in 0..count {
        assert!(facts.contains(1, 3, 1000 + index).expect("member"));
    }
    let CodecError::ResourceLimit(limit) = ctx
        .charge_work(u64::MAX, "seam membership work probe")
        .expect_err("work probe")
    else {
        panic!("work refusal");
    };
    assert_eq!(limit.dimension, ResourceDimension::WorkUnits);
    limit.used
}

#[test]
fn explicit_seam_membership_uses_keys_after_one_source_walk() {
    let small = repeated_membership_work(128);
    let large = repeated_membership_work(256);
    assert!(
        large < 3 * small,
        "seam membership work grew from {small} to {large}"
    );
}
