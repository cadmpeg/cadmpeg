// SPDX-License-Identifier: Apache-2.0
use crate::sketches::SketchGeometry;
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};
use cadmpeg_core::text::NonBlankString;
use cadmpeg_core::CodecError;

fn copy_with_policy(
    geometry: &SketchGeometry,
    policy: &DecodePolicy,
) -> Result<SketchGeometry, CodecError> {
    let arena = DecodeArena::new();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, policy).expect("empty root");
    geometry.try_clone_for_decode(&ctx, "sketch geometry copy")
}

#[test]
fn sketch_native_copy_preserves_the_geometry() {
    let geometry = SketchGeometry::native(NonBlankString::new("native").expect("nonblank"));
    assert_eq!(
        copy_with_policy(&geometry, &DecodePolicy::service()).expect("service copy"),
        geometry
    );
}
