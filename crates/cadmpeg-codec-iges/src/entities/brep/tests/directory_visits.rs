// SPDX-License-Identifier: Apache-2.0

use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
use cadmpeg_core::CodecError;
use std::collections::BTreeMap;

#[test]
fn brep_directory_does_not_admit_an_unvisited_tail_before_its_first_loss_refusal() {
    let mut vertex = crate::test_support::directory_target(1, 502);
    vertex.form = 1;
    let directory = [
        vertex,
        crate::test_support::directory_target(3, 110),
        crate::test_support::directory_target(5, 110),
    ];
    let entries: BTreeMap<_, _> = directory.iter().map(|entry| (entry.sequence, entry)).collect();
    let records = BTreeMap::new();
    let bytes = crate::test_support::test_owned::owned_test_file(&[]);
    let global = crate::test_support::with_service_context(&bytes, |setup| {
        let scan = crate::card::scan_with_context(&bytes, setup).unwrap();
        let (global, _, _) = crate::global::parse(&scan, setup).unwrap();
        global.length_context().unwrap()
    });
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    // One original Directory step. The empty parameter map has no key comparisons.
    policy.limits.max_work_units = 1;
    policy.limits.max_collection_items = 0;
    policy.limits.max_retained_bytes = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let mut ir = cadmpeg_ir::CadIr::empty();
    let mut sequences = super::super::super::geometry::SourceSequences::default();
    let result = super::super::project(
        &mut ir, &directory, (&entries, &records), &global, &ctx, &mut sequences,
    );
    let first = match result.as_ref() {
        Err(CodecError::ResourceLimit(first)) => *first,
        _ => panic!("expected first visited entity loss-slot refusal"),
    };
    drop(result);
    assert_eq!(first.dimension, ResourceDimension::CollectionItems);
    assert_eq!(first.operation, "iges entity loss slots");
    assert_eq!((first.limit, first.used, first.additional), (0, 0, 1));
    assert!(ir.model.bodies.is_empty());
    assert!(matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(last)) if last == first));
}
