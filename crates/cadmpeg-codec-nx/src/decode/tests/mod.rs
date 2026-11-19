// SPDX-License-Identifier: Apache-2.0
//! Decode-owner unit tests.

use cadmpeg_core::decode::DecodeDepth;
use cadmpeg_ir::codec::DecodeOptions;

fn options_at(depth: DecodeDepth) -> DecodeOptions {
    DecodeOptions {
        depth,
        ..DecodeOptions::default()
    }
}

mod attribute_lists;
mod blend_contact;
mod emission;
mod metadata;
mod parameterization;
mod pcurves;
mod selection;
mod support_uv;
