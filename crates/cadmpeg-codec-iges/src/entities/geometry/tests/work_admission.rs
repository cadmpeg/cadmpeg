// SPDX-License-Identifier: Apache-2.0

fn assert_scan_work_refusal<T>(
    operation: &str,
    run: impl Fn(&cadmpeg_core::decode::DecodeContext<'_>) -> Result<T, cadmpeg_core::CodecError>,
) {
    cadmpeg_test_support::refusal::resource_limit_at(cadmpeg_core::decode::ResourceDimension::WorkUnits, operation, |cap| {
        let mut policy = cadmpeg_core::decode::DecodePolicy::service();
        policy.limits.max_work_units = cap;
        let arena = cadmpeg_core::decode::DecodeArena::new();
        let ctx = cadmpeg_core::decode::DecodeContext::new(&arena, &policy, false);
        run(&ctx)
    });
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

#[test]
fn geometry_driver_admits_directory_and_family_passes() {
    use cadmpeg_ir::codec::{Codec, DecodeFailure, DecodeOptions};
    let bytes = crate::test_support::test_curves_and_surfaces::line_file(0);
    for operation in ["iges geometry directory admission", "iges geometry parameter traversal", "iges geometry directory index traversal", "iges geometry family traversal", "iges composite directory traversal", "iges conic directory traversal", "iges analytic vertex traversal", "iges analytic point retention"] {
        cadmpeg_test_support::refusal::resource_limit_at(cadmpeg_core::decode::ResourceDimension::WorkUnits, operation, |cap| {
            let mut policy = cadmpeg_core::decode::DecodePolicy::service();
            policy.limits.max_work_units = cap;
            crate::IgesCodec.decode(&mut std::io::Cursor::new(&bytes), &DecodeOptions { policy, ..DecodeOptions::default() }).map_err(|failure| match failure {
                DecodeFailure::Codec(error) => error,
                other => panic!("unexpected decode failure: {other:?}"),
            })
        });
    }
}
