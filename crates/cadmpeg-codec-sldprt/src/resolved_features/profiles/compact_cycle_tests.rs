// SPDX-License-Identifier: Apache-2.0

use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
use cadmpeg_core::CodecError;

fn assert_cycle_refusal(dimension: ResourceDimension) {
    let (history, mut lane, feature) = super::detached_legacy_sketch_tests::compact_profile_projection_fixture();
    lane.native_payload.extend(b"moSketchRegion_c");
    lane.native_payload.extend(0x8060u16.to_le_bytes());
    lane.native_payload.extend(4u16.to_le_bytes());
    // These paired region and chain addresses form four edges in a closed cycle.
    for address in [2u16, 1, 4, 3] {
        lane.native_payload.extend(0x80e1u16.to_le_bytes());
        lane.native_payload.extend(address.to_le_bytes());
        lane.native_payload.extend([0xff; 4]);
        lane.native_payload.extend([0; 4]);
    }
    lane.classes.push(crate::records::FeatureInputClass {
        id: "line-class".into(), parent: lane.id.clone(), ordinal: 0,
        offset: 0, name: "sgLineHandle".into(),
    });
    let arena = DecodeArena::new();
    let run = |policy: &DecodePolicy| {
        let (ctx, _) = DecodeContext::from_root_bytes(&lane.native_payload, &arena, policy).unwrap();
        let mut features = [feature.clone()];
        let mut sketches = Vec::new();
        let mut entities = Vec::new();
        let mut losses = Vec::new();
        super::project_compact_sketch_profiles(&ctx, &mut features, &mut sketches, &mut entities,
            std::slice::from_ref(&history), std::slice::from_ref(&lane), &mut losses)
            .map(|()| (features, sketches, entities, losses))
    };
    let expected = run(&DecodePolicy::service()).unwrap();
    assert_eq!(expected.1.len(), 1);
    assert_eq!(expected.2.len(), 4);
    assert_eq!(expected.1[0].profiles[0].len(), 4);
    assert!(expected.3.is_empty());
    assert_eq!(expected.1[0].profiles[0].iter().map(|use_| use_.entity.as_str()).collect::<Vec<_>>(), [
        "sldprt:model:sketch-entity#compact:1:30:0",
        "sldprt:model:sketch-entity#compact:1:30:3",
        "sldprt:model:sketch-entity#compact:1:30:2",
        "sldprt:model:sketch-entity#compact:1:30:1",
    ]);
    let set_limit = |policy: &mut DecodePolicy, limit| match dimension {
        ResourceDimension::RetainedBytes => policy.limits.max_retained_bytes = limit,
        ResourceDimension::WorkUnits => policy.limits.max_work_units = limit,
        _ => panic!("unsupported cycle limit"),
    };
    let mut policy = DecodePolicy::service();
    let admitted = |policy: &DecodePolicy| match run(policy) {
        Ok(actual) => { assert_eq!(actual, expected); true }
        Err(CodecError::ResourceLimit(limit)) => { assert_eq!(limit.dimension, dimension); false }
        Err(error) => panic!("unexpected cycle projection error: {error}"),
    };
    let mut lower = 0;
    let mut upper = 1_u64;
    loop {
        set_limit(&mut policy, upper);
        if admitted(&policy) { break; }
        upper = upper.checked_mul(2).unwrap();
    }
    while lower < upper {
        let middle = lower + (upper - lower) / 2;
        set_limit(&mut policy, middle);
        if admitted(&policy) { upper = middle; } else { lower = middle + 1; }
    }
    assert!(upper > 0);
    set_limit(&mut policy, upper); assert!(admitted(&policy));
    set_limit(&mut policy, upper - 1); assert!(!admitted(&policy));
}

#[test]
fn compact_cycle_projection_refuses_retained_limit() { assert_cycle_refusal(ResourceDimension::RetainedBytes); }
#[test]
fn compact_cycle_projection_refuses_work_limit() { assert_cycle_refusal(ResourceDimension::WorkUnits); }
