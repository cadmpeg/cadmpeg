// SPDX-License-Identifier: Apache-2.0

use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
use cadmpeg_core::CodecError;
use cadmpeg_ir::codec::CodecBackend;
use cadmpeg_ir::geometry::ProceduralCurveDefinition;

use crate::test_support::test_owned::{owned_test_file, OwnedTestEntity};
use crate::IgesCodec;

fn work(count: usize) -> u64 {
    let mut inputs = vec![OwnedTestEntity {
        entity_type: 110, form: 0, label: "SOURCE".into(), status: "00010000",
        parameters: "110,0,0,0,2,0,0;".into(),
    }];
    inputs.extend((0..count).map(|_| OwnedTestEntity {
        entity_type: 130, form: 0, label: "OFFSET".into(), status: "00000000",
        parameters: "130,1,1,0,,,0.5,,,,0,0,1,0,1;".into(),
    }));
    let bytes = owned_test_file(&inputs);
    let run = |cap| {
            let arena = DecodeArena::new();
            let mut policy = DecodePolicy::default();
            policy.limits.max_work_units = cap;
            let (ctx, root) = DecodeContext::from_root_bytes(&bytes, &arena, &policy)
                .expect("fixture fits the default input budget");
            let decoded = IgesCodec.decode_impl(&ctx, root)
                .expect("thousands of offsets fit the decode budget");
            assert_eq!(decoded.ir.model.curves.len(), count + 1);
            assert_eq!(decoded.ir.model.procedural_curves.len(), count);
            assert_eq!(decoded.ir.model.edges.len(), count + 1);
            for construction in &decoded.ir.model.procedural_curves {
                let ProceduralCurveDefinition::Offset(payload) = construction.definition() else {
                    panic!("expected offset construction");
                };
                assert_eq!(payload.source().as_str(), "iges:model:curve#D1");
                let owner = decoded.ir.model.procedural_curve_owner(&construction.id)
                    .expect("one carrier owns each construction");
                assert_ne!(owner.as_str(), "iges:model:curve#D1");
            }
            let result = ctx.charge_work(1, "test completed offset batch work");
            ctx.finish(result)
    };
    run(DecodePolicy::default().limits.max_work_units).expect("default-budget decode");
    let error = cadmpeg_test_support::refusal::resource_limit_at(
        ResourceDimension::WorkUnits, "test completed offset batch work", run,
    );
    let CodecError::ResourceLimit(limit) = error else { panic!("expected work measurement"); };
    assert_eq!(limit.additional, 1);
    limit.used
}

#[test]
fn offset_curve_batch_decodes_thousands_with_linear_work() {
    let small = work(2048);
    let large = work(4096);
    assert!(large <= small * 5 / 2, "work grows faster than the indexed route: {small} -> {large}");
}

#[test]
fn offset_curve_batch_preserves_a_chain_of_solved_carriers() {
    let inputs = [
        OwnedTestEntity { entity_type: 110, form: 0, label: "SOURCE".into(), status: "00010000",
            parameters: "110,0,0,0,2,0,0;".into() },
        OwnedTestEntity { entity_type: 130, form: 0, label: "FIRST".into(), status: "00010000",
            parameters: "130,1,1,0,,,0.5,,,,0,0,1,0,1;".into() },
        OwnedTestEntity { entity_type: 130, form: 0, label: "SECOND".into(), status: "00000000",
            parameters: "130,3,1,0,,,0.5,,,,0,0,1,0,1;".into() },
    ];
    let bytes = owned_test_file(&inputs);
    let arena = DecodeArena::new();
    let (ctx, root) = DecodeContext::from_root_bytes(&bytes, &arena, &DecodePolicy::default())
        .expect("default context");
    let decoded = IgesCodec.decode_impl(&ctx, root).expect("offset chain decodes");
    ctx.finish_session().expect("unfused decode");
    assert_eq!(decoded.ir.model.curves.len(), 3);
    assert_eq!(decoded.ir.model.procedural_curves.len(), 2);
    let sources: Vec<_> = decoded.ir.model.procedural_curves.iter().map(|construction| {
        let ProceduralCurveDefinition::Offset(payload) = construction.definition() else {
            panic!("expected offset construction");
        };
        payload.source().as_str()
    }).collect();
    assert_eq!(sources, ["iges:model:curve#D1", "iges:model:curve#D3"]);
    assert!(decoded.ir.model.curves.iter().all(|curve| curve.geometry.solved().is_some()));
}
