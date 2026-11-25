// SPDX-License-Identifier: Apache-2.0
//! Reads of the write path run under the caller's resource limits.

use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceLimits};
use cadmpeg_core::CodecError;

use crate::test_support::container::sldprt_with_body;
use crate::test_support::parasolid::triangle_body;

/// The retained source baseline of `image` decoded under `limits`, with no
/// baseline carried in.
fn baseline_under(image: &[u8], limits: ResourceLimits) -> Result<bool, CodecError> {
    let arena = DecodeArena::new();
    let source = DecodeContext::from_root_bytes(image, &arena, &DecodePolicy { limits })?;
    let mut baseline = None;
    crate::writer::retained_source_baseline(&mut baseline, Some(&source))
        .map(|decoded| decoded.is_some())
}

#[test]
fn a_baseline_decoded_under_looser_limits_is_not_reused_under_tighter_ones() {
    let image = sldprt_with_body(&triangle_body());
    assert!(baseline_under(&image, ResourceLimits::desktop()).unwrap());
    let mut tight = ResourceLimits::desktop();
    tight.max_recursion_depth = 0;
    assert!(matches!(
        baseline_under(&image, tight),
        Err(CodecError::ResourceLimit(_))
    ));
}
