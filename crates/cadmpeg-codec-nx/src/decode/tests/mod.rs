// SPDX-License-Identifier: Apache-2.0
//! Decode-owner unit tests.

pub(crate) use super::*;

use cadmpeg_core::decode::DecodeMode;
use cadmpeg_ir::codec::DecodeOptions;

pub(crate) fn options_in(mode: DecodeMode, container_only: bool) -> DecodeOptions {
    DecodeOptions {
        container_only,
        policy: cadmpeg_core::decode::DecodePolicy {
            mode,
            ..Default::default()
        },
    }
}

mod blend_contact;
mod emission;
mod parameterization;
mod pcurves;
mod selection;
