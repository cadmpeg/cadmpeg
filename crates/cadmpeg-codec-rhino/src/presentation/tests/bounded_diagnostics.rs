// SPDX-License-Identifier: Apache-2.0

use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};

use crate::chunks::{BoundedReader, FramingError};
use crate::presentation::scaled_length;
use crate::settings::StandardUnit;

#[test]
fn scaled_length_diagnostic_has_no_budget_fee() {
    let bytes = f64::MAX.to_le_bytes();
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = 0;
    policy.limits.max_retained_bytes = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&bytes, &arena, &policy).unwrap();
    let mut reader = BoundedReader::new(&bytes, 0, bytes.len()).unwrap();
    let error = scaled_length(&ctx, &mut reader, StandardUnit::Inches.into(), "arrow size").unwrap_err();
    assert!(matches!(error, FramingError::Structural { offset: 0, message } if message == "scaled arrow size is invalid"));
    assert_eq!(ctx.resource_refusal(), None);
    ctx.finish_session().unwrap();
}
