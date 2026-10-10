// SPDX-License-Identifier: Apache-2.0

use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
use cadmpeg_core::decode::refusal_probe::RefusalProbe;
use cadmpeg_core::CodecError;
use cadmpeg_ir::CadIr;
use crate::entities::geometry::SourceSequences;
use crate::test_support::test_owned::{owned_test_file, OwnedTestEntity};

#[test]
fn rejected_type128_carriers_release_candidate_storage() {
    let bytes = owned_test_file(&[OwnedTestEntity { entity_type: 128, form: 0,
        label: "CLOSED".into(), status: "00000000",
        parameters: "128,1,1,1,1,1,0,1,0,0,0,0,1,1,0,0,1,1,1,1,1,1,0,0,0,1,0,0,0,1,0,1,1,0,0,1,0,1;".into(),
    }]);
    let (directory, records, global) = crate::test_support::with_service_context(&bytes, |ctx| {
        let scan = crate::card::scan_with_context(&bytes, ctx).unwrap();
        let (global, _, _) = crate::global::parse(&scan, ctx).unwrap();
        let (directory, quarantined) = crate::directory::parse(&scan, global.global_table(), ctx).unwrap();
        assert!(quarantined.is_empty());
        let records = crate::parameter::assemble_with_context(&scan, &directory, &quarantined, &global, ctx).unwrap().records;
        (directory, records, global.length_context().unwrap())
    });
    let error = cadmpeg_test_support::refusal::resource_limit_at(
        ResourceDimension::MaterializedBytes, "iges NURBS surface source u knots", |cap| {
            let arena = DecodeArena::new();
            let mut policy = DecodePolicy::service();
            policy.limits.max_materialized_bytes = cap;
            let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
            let mut sequences = SourceSequences::new(&ctx)?;
            super::super::project(&mut CadIr::empty(), &directory, &records, &global, &ctx, &mut sequences).map(|_| ())
        });
    assert!(matches!(error, CodecError::ResourceLimit(limit)
        if limit.operation == "iges NURBS surface source u knots"
            && limit.additional == u64::try_from(4 * std::mem::size_of::<f64>()).unwrap()));
    let repeated_directory: Vec<_> = (0..16_u32).map(|index| {
        let mut entry = directory[0].clone(); entry.sequence = 2 * index + 1; entry
    }).collect();
    let repeated_records: Vec<_> = repeated_directory.iter().map(|entry| {
        let mut record = records[0].clone(); record.directory_sequence = entry.sequence; record
    }).collect();
    for operation in ["iges NURBS surface source u knots", "iges NURBS surface source v knots",
        "iges NURBS surface pole rows", "iges NURBS surface pole row controls"] {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_materialized_bytes = 64 * 1024;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        let mut sequences = SourceSequences::new(&ctx).unwrap();
        let mut ir = CadIr::empty();
        let _probe = RefusalProbe::arm(ResourceDimension::RetainedBytes, operation, None);
        let outcome = super::super::project(&mut ir, &repeated_directory, &repeated_records, &global, &ctx, &mut sequences).unwrap();
        assert!(outcome.decoded.is_empty());
        assert_eq!(outcome.losses.len(), 16);
        assert!(outcome.losses.iter().all(|loss| loss.message.ends_with("U-closed surface flag disagrees with boundary curves")));
        assert_eq!(ir, CadIr::empty());
        drop(outcome);
        drop(sequences);
        let free = ctx.reserve_scoped(policy.limits.max_materialized_bytes, "test rejected surface candidates released").unwrap();
        drop(free);
        ctx.finish_session().unwrap();
    }
}
