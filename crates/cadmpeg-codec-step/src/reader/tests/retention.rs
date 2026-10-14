// SPDX-License-Identifier: Apache-2.0
//! Allocation and transfer claims in the retention reference index.

use std::collections::HashSet;
use std::fmt::Write;

#[test]
fn claim_handoffs_preserve_the_union() {
    let source = (0..1000).collect::<HashSet<_>>();
    let mut target = HashSet::new();
    crate::reader::merge_claims(&mut target, source);
    assert_eq!(target.len(), 1000);

    let mut target = HashSet::from([2]);
    crate::reader::merge_claims(&mut target, HashSet::from([1, 2, 3]));
    assert_eq!(target, HashSet::from([1, 2, 3]));
}

#[test]
fn retention_reference_index_stores_no_unrelated_record_ids() {
    let data = (1..=1000).fold(String::new(), |mut data, id| {
        write!(data, "#{id}=ITEM(#{});", id % 1000 + 1).expect("fixture string");
        data
    });
    let source = format!(
        "ISO-10303-21;HEADER;FILE_SCHEMA(('AP242'));ENDSEC;DATA;{data}ENDSEC;END-ISO-10303-21;"
    );
    let (exchange, _) =
        crate::test_support::with_service_context(source.as_bytes(), crate::parse::parse_inner)
            .expect("bounded reference cycle");
    let policy = cadmpeg_core::decode::DecodePolicy::service();
    crate::test_support::with_policy_context(source.as_bytes(), &policy, |_, ctx| {
        let carriers = crate::reader::index::CarrierIndex::from_ir(&cadmpeg_ir::CadIr::empty());
        let references = crate::reader::referenced_record_ids(
            &exchange,
            &cadmpeg_ir::CadIr::empty(),
            &HashSet::new(),
            &carriers,
            ctx,
        )
        .expect("unrelated reference targets consume no storage");
        assert!(references.is_empty());
    });
}

#[test]
fn retention_reference_index_keeps_point_curve_and_surface_uses() {
    let source = b"ISO-10303-21;HEADER;FILE_SCHEMA(('AP242'));ENDSEC;DATA;#1=CARTESIAN_POINT('',(0.,0.,0.));#2=LINE('',#1,#3);#3=VECTOR('',#4,1.);#4=DIRECTION('',(1.,0.,0.));#5=PLANE('',#6);#6=AXIS2_PLACEMENT_3D('',#1,$,$);#7=ITEM(#1,#2,#5,#3);ENDSEC;END-ISO-10303-21;";
    let (exchange, _) =
        crate::test_support::with_service_context(source, crate::parse::parse_inner)
            .expect("carrier references");
    let (ir, carriers, geometry) = crate::test_support::with_service_context(source, |_, ctx| {
        let mut ir = cadmpeg_ir::CadIr::empty();
        let (geometry, carriers) = crate::reader::geometry::decode(&exchange, &mut ir, ctx)?;
        Ok::<_, cadmpeg_core::CodecError>((ir, carriers, geometry))
    })
    .expect("decoded carriers");
    let policy = cadmpeg_core::decode::DecodePolicy::service();
    crate::test_support::with_policy_context(source, &policy, |_, ctx| {
        let references =
            crate::reader::referenced_record_ids(&exchange, &ir, &geometry.claims, &carriers, ctx)
                .expect("only three relevant targets need storage");
        assert_eq!(references.keys().copied().collect::<Vec<_>>(), [1, 2, 5]);
        assert!(references[&1], "typed geometry uses the coordinate");
    });
}

#[test]
fn retention_reference_index_reuses_transferred_carrier_ownership() {
    use cadmpeg_ir::{Codec, DecodeOptions};
    let source = b"ISO-10303-21;HEADER;FILE_SCHEMA(('AP242'));ENDSEC;DATA;#1=CARTESIAN_POINT('',(0.,0.,0.));#2=GEOMETRIC_SET('',(#1));#3=ITEM(#1);ENDSEC;END-ISO-10303-21;";
    let decoded = crate::StepCodec::default()
        .decode(&mut std::io::Cursor::new(source), &DecodeOptions::default())
        .expect("owned coordinate");
    let ir = decoded.ir();
    assert_eq!(ir.model.points.len(), 1);
    assert!(ir.model.points[0].source_object.is_some());
    let (exchange, _) =
        crate::test_support::with_service_context(source, crate::parse::parse_inner)
            .expect("carrier references");
    let carriers = { crate::reader::index::CarrierIndex::from_ir(ir) };
    let policy = cadmpeg_core::decode::DecodePolicy::service();
    crate::test_support::with_policy_context(source, &policy, |_, ctx| {
        let references = crate::reader::referenced_record_ids(
            &exchange,
            ir,
            &HashSet::from([1, 2]),
            &carriers,
            ctx,
        )
        .expect("transferred coordinate needs no reference storage");
        assert!(references.is_empty());
    });
}

#[test]
fn record_closure_streams_repeated_roots_and_a_reference_cycle() {
    let data = (1..=1000).fold(String::new(), |mut data, id| {
        let next = id % 1000 + 1;
        write!(data, "#{id}=ITEM(#{next},#{next});").expect("fixture string");
        data
    });
    let source = format!(
        "ISO-10303-21;HEADER;FILE_SCHEMA(('AP242'));ENDSEC;DATA;{data}ENDSEC;END-ISO-10303-21;"
    );
    let (exchange, _) =
        crate::test_support::with_service_context(source.as_bytes(), crate::parse::parse_inner)
            .expect("bounded cycle");
    let policy = cadmpeg_core::decode::DecodePolicy::service();
    // Sixteen allocated membership blocks and one reusable pending slot.
    crate::test_support::with_policy_context(source.as_bytes(), &policy, |_, ctx| {
        let closure = crate::reader::record_closure(1..=1000, &exchange, ctx)
            .expect("each source record is queued once");
        for id in 1..=1000 {
            assert!(closure.contains(id));
        }
        assert!(!closure.contains(0));
        assert!(!closure.contains(1001));
    });
}

#[test]
fn record_closure_keeps_nested_references_and_reuses_admitted_pending_slots() {
    let source = b"ISO-10303-21;HEADER;FILE_SCHEMA(('AP242'));ENDSEC;DATA;#1=ITEM((#2,#3,#4,#5,#2,#3),WRAPPED(#1));#2=ITEM(#3);#3=ITEM(#4);#4=ITEM(#5);#5=ITEM(#1);ENDSEC;END-ISO-10303-21;";
    let (exchange, _) =
        crate::test_support::with_service_context(source, crate::parse::parse_inner)
            .expect("nested reference graph");
    let policy = cadmpeg_core::decode::DecodePolicy::service();
    // One membership block and four maximum-live pending slots.
    crate::test_support::with_policy_context(source, &policy, |_, ctx| {
        let closure = crate::reader::record_closure([1, 1, 5], &exchange, ctx)
            .expect("nested graph closes with reused storage");
        for id in 1..=5 {
            assert!(closure.contains(id));
        }
        assert!(!closure.contains(6));
    });
}
