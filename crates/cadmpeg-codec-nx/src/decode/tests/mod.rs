// SPDX-License-Identifier: Apache-2.0
//! Decode-owner unit tests.

use cadmpeg_core::decode::{DecodeDepth, DecodeMode};
use cadmpeg_ir::codec::DecodeOptions;

fn options_in(mode: DecodeMode, depth: DecodeDepth) -> DecodeOptions {
    DecodeOptions {
        depth,
        policy: cadmpeg_core::decode::DecodePolicy {
            mode,
            ..Default::default()
        },
    }
}

mod blend_contact;
mod emission;
mod metadata;
mod parameterization;
mod pcurves;
mod selection;
mod support_uv;
