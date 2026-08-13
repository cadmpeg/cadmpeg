// SPDX-License-Identifier: Apache-2.0
//! Offset read of three big-endian `f64` values.
#![deny(clippy::disallowed_methods)]

use cadmpeg_core::decode::View;

pub(crate) fn vec3_be_at(bytes: &[u8], offset: usize) -> Option<[f64; 3]> {
    Some([
        View::f64_be_at(bytes, offset)?,
        View::f64_be_at(bytes, offset.checked_add(8)?)?,
        View::f64_be_at(bytes, offset.checked_add(16)?)?,
    ])
}
