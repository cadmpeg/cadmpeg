// SPDX-License-Identifier: Apache-2.0

use crate::math::Point2;
use crate::sketches::{SketchGeometry, SketchGeometryDefinition};
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};

#[test]
fn fixed_line_geometry_clone_uses_no_retained_storage() {
    let geometry = SketchGeometry::try_from(SketchGeometryDefinition::Line {
        start: Point2::new(0.0, 0.0),
        end: Point2::new(1.0, 0.0),
    })
    .expect("finite line geometry");
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_retained_bytes = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy)
        .expect("empty root admitted");
    assert_eq!(
        geometry
            .try_clone_for_decode(&ctx, "IR fixed line geometry clone")
            .expect("fixed line clone has no retained child storage"),
        geometry
    );
}
