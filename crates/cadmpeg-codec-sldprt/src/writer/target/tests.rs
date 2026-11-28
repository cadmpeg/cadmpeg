// SPDX-License-Identifier: Apache-2.0
#![allow(clippy::unwrap_used)]

use std::io::Cursor;

use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodeDepth, DecodePolicy, ResourceLimits};
use cadmpeg_core::CodecError;
use cadmpeg_ir::codec::{Codec, DecodeOptions};
use cadmpeg_ir::features::geometry::FinitePoint3;
use cadmpeg_ir::math::Point3;

use super::stored_losses;
use crate::test_support::container::sldprt_with_body;
use crate::test_support::parasolid::triangle_body;
use crate::writer::SourceInput;
use crate::SldprtCodec;

/// The losses of a part written as stored name the same records whether
/// they compare the document with the source the write held or with the
/// source decoded again, and a decode the session refuses refuses the write.
#[test]
fn stored_losses_read_the_held_source_as_a_full_decode_does() {
    let image = sldprt_with_body(&triangle_body());
    let decoded = SldprtCodec
        .decode(&mut Cursor::new(&image), &DecodeOptions::default())
        .unwrap();
    let mut edited = decoded.ir().clone();
    let at = edited.geometry.points[0].position().get();
    edited.geometry.points[0]
        .set_position(FinitePoint3::new(Point3::new(at.x + 1.0, at.y, at.z)).unwrap());
    let moved = edited.geometry.points[0].id.as_str().to_owned();

    let arena = DecodeArena::new();
    let policy = DecodePolicy {
        limits: ResourceLimits::desktop(),
    };
    let ctx = DecodeContext::new(&arena, &policy, DecodeDepth::Full);
    let input = SourceInput::new(&image);
    input.baseline(&ctx).unwrap();
    let Err(CodecError::ResourceLimit(count)) =
        ctx.charge_entities(u64::MAX, "measure source baseline")
    else {
        panic!("the measurement charge must exceed the entity ceiling")
    };
    let policy = DecodePolicy {
        limits: ResourceLimits {
            max_entities: count.used,
            max_input_bytes: cadmpeg_core::decode::u64_from_index(image.len()),
            ..ResourceLimits::desktop()
        },
    };
    let ctx = DecodeContext::new(&arena, &policy, DecodeDepth::Full);
    let input = SourceInput::new(&image);
    let held = input.baseline(&ctx).unwrap();
    let losses = stored_losses(&ctx, &edited, &input, None, "the test").unwrap();
    assert!(
        losses.iter().any(|loss| loss.message.contains(&moved)),
        "{losses:?}"
    );
    assert_eq!(
        stored_losses(&ctx, &edited, &input, Some(held), "the test").unwrap(),
        losses
    );

    let policy = DecodePolicy {
        limits: ResourceLimits {
            max_entities: 1,
            ..ResourceLimits::desktop()
        },
    };
    let ctx = DecodeContext::new(&arena, &policy, DecodeDepth::Full);
    assert!(matches!(
        stored_losses(&ctx, &edited, &SourceInput::new(&image), None, "the test"),
        Err(CodecError::ResourceLimit(_))
    ));
}
