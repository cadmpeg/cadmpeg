// SPDX-License-Identifier: Apache-2.0

fn assert_scan_work_refusal<T>(
    operation: &str,
    run: impl Fn(&cadmpeg_core::decode::DecodeContext<'_>) -> Result<T, cadmpeg_core::CodecError>,
) {
    let mut cap = 0_u64;
    for _ in 0..128 {
        let mut policy = cadmpeg_core::decode::DecodePolicy::service();
        policy.limits.max_work_units = cap;
        let arena = cadmpeg_core::decode::DecodeArena::new();
        let ctx = cadmpeg_core::decode::DecodeContext::new(&arena, &policy, false);
        match run(&ctx) {
            Err(cadmpeg_core::CodecError::ResourceLimit(limit)) => {
                assert_eq!(limit.dimension, cadmpeg_core::decode::ResourceDimension::WorkUnits);
                if limit.operation == operation {
                    return;
                }
                cap = limit.used.checked_add(limit.additional).unwrap();
            }
            Err(error) => panic!("unexpected error before {operation}: {error}"),
            Ok(_) => panic!("operation {operation} was not admitted"),
        }
    }
    panic!("operation {operation} was not reached");
}

#[test]
fn transform_preflight_refuses_work_before_chain_step() {
    let directory = [super::transform_entry(1, 0), super::transform_entry(3, 1)];
    assert_scan_work_refusal("iges transform preflight walk", |ctx| {
        super::super::enforce_transform_depth(&directory, ctx)
    });
}

#[test]
fn consumed_support_closure_refuses_work_before_advance() {
    let directory = [super::transform_entry(1, 0), super::transform_entry(3, 1)];
    let records = std::collections::BTreeMap::new();
    assert_scan_work_refusal("iges consumed-support closure traversal", |ctx| {
        super::super::consumed_support_sequences(&directory, &records, ctx)
    });
}
