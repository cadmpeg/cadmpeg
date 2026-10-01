// SPDX-License-Identifier: Apache-2.0
//! Detection assertions using a complete caller-owned session.

use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};
use cadmpeg_ir::codec::{Codec, Confidence};

/// Returns the confidence under the default test policy and checks the session.
pub fn confidence(codec: &dyn Codec, bytes: &[u8]) -> Confidence {
    let arena = DecodeArena::new();
    let (ctx, root) = DecodeContext::from_root_bytes(bytes, &arena, &DecodePolicy::default()).expect("test input");
    let result = codec.detect(&ctx, root);
    ctx.finish_session().expect("test detection session");
    result.expect("test detection result")
}
