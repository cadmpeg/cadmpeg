// SPDX-License-Identifier: Apache-2.0
//! Decode-owned crate-root dump tests parked until the decode agent lands.
#![allow(unused_imports)]
#![allow(clippy::unwrap_used)]
#![allow(clippy::default_trait_access)]

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
mod selection;
