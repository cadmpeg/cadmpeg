// SPDX-License-Identifier: Apache-2.0

use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
use cadmpeg_core::CodecError;

fn boundary(source: &[u8], operation: &str) -> cadmpeg_core::decode::ResourceLimit {
    let error = cadmpeg_test_support::refusal::resource_limit_at(
        ResourceDimension::WorkUnits, operation, |cap| {
            let arena = DecodeArena::new();
            let mut policy = DecodePolicy::service();
            policy.limits.max_work_units = cap;
            let (ctx, _) = DecodeContext::from_root_bytes(source, &arena, &policy)?;
            super::super::parse(&ctx, source)
                .map_err(|error| error.into_codec_error(&ctx, CodecError::malformed))
        });
    let CodecError::ResourceLimit(limit) = error else { panic!("frame step refusal"); };
    assert_eq!(limit.operation, operation);
    assert_eq!(limit.additional, 1);
    limit
}

#[test]
fn sat_record_step_precedes_the_name_scan() {
    let short = b"0 0 0 0\n0 0 0 \n1 0 0\nx #\nEnd-of-ASM-data \n";
    let long = format!("0 0 0 0\n0 0 0 \n1 0 0\n{} #\nEnd-of-ASM-data \n", "x".repeat(4096));
    assert_eq!(boundary(short, "frame SAT record").used,
        boundary(long.as_bytes(), "frame SAT record").used);
}

#[test]
fn sat_field_step_precedes_the_payload_scan() {
    let short = b"0 0 0 0\n0 0 0 \n1 0 0\nx a #\nEnd-of-ASM-data \n";
    let long = format!("0 0 0 0\n0 0 0 \n1 0 0\nx {} #\nEnd-of-ASM-data \n", "a".repeat(4096));
    assert_eq!(boundary(short, "frame SAT field").used,
        boundary(long.as_bytes(), "frame SAT field").used);
}
