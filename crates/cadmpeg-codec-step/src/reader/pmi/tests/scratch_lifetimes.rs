// SPDX-License-Identifier: Apache-2.0
//! Per-record scratch is released before the next record.

use std::fmt::Write as _;

use cadmpeg_core::decode::DecodePolicy;
use cadmpeg_ir::CadIr;

fn decode_with_scratch_cap(records: &str, cap: u64) -> CadIr {
    let source = format!("ISO-10303-21;HEADER;FILE_DESCRIPTION(('test'),'2;1');FILE_NAME('','',(''),(''),'','','');FILE_SCHEMA(('AP242'));ENDSEC;DATA;{records}ENDSEC;END-ISO-10303-21;");
    let (exchange, _) = crate::test_support::with_service_context(source.as_bytes(), crate::parse::parse_inner).expect("exchange");
    let setup = cadmpeg_test_support::service_decode_context();
    let mut ir = CadIr::empty();
    let geometry = crate::reader::geometry::decode(&exchange, &mut ir, &setup).expect("geometry");
    let index = crate::reader::index::CarrierIndex::from_ir(&ir, &setup).expect("carriers");
    let topology = crate::reader::topology::decode(&exchange, &mut ir, &index, &setup).expect("topology");
    let mut policy = DecodePolicy::service();
    policy.limits.max_materialized_bytes = cap;
    crate::test_support::with_policy_context(b"", &policy, |_, ctx| {
        super::super::decode(&exchange, &geometry.value, &topology.value, &mut ir, ctx).expect("only live scratch fits");
    });
    ir
}

#[test]
fn geometric_usage_releases_each_discarded_annotation_set() {
    let mut records = String::from("#1=SHAPE_ASPECT('','',#99,.T.);#2=DIMENSIONAL_SIZE(#1,'');#99=ITEM();");
    for id in 100..356 {
        write!(records, "#{id}=GEOMETRIC_ITEM_SPECIFIC_USAGE('','',#1,$,#99);").expect("usage record");
    }
    // Long-lived maps and one single-entry set fit in 8 KiB. Keeping 256
    // discarded sets would add 256 * (11*8 + 16*8 + 2*8) = 59,392 bytes.
    let ir = decode_with_scratch_cap(&records, 8192);
    assert_eq!(ir.model.pmi.len(), 1);
    assert_eq!(ir.model.pmi[0].targets.len(), 1);
}

#[test]
fn dimensions_release_each_aspect_deduplication_set() {
    let count = 256;
    let mut records = String::from("#1=SHAPE_ASPECT('','',#99,.T.);#99=ITEM();");
    for id in 100..100 + count {
        write!(records, "#{id}=DIMENSIONAL_SIZE(#1,'');").expect("dimension record");
    }
    // The live annotation, claim and aspect-group trees fit in 200 bytes
    // per dimension plus 10 KiB. Dead singleton sets would add 59,392 bytes.
    let ir = decode_with_scratch_cap(&records, 200 * count + 10_000);
    assert_eq!(ir.model.pmi.len(), usize::try_from(count).expect("dimension count"));
    assert!(ir.model.pmi.iter().all(|annotation| annotation.targets.len() == 1));
}
