// SPDX-License-Identifier: Apache-2.0
#![recursion_limit = "4"]

pub fn too_many_states(ctx: &cadmpeg_core::DecodeContext) {
    cadmpeg_core::fanout::<u8>(ctx); // finding: unproven_decode_charge
}

pub fn deep_chain(ctx: &cadmpeg_core::DecodeContext) {
    cadmpeg_core::deep::<u8>(ctx); // finding: unproven_decode_charge
}

pub fn repeated_state(ctx: &cadmpeg_core::DecodeContext) {
    cadmpeg_core::repeated::<u8>(ctx);
}
