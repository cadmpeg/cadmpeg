// SPDX-License-Identifier: Apache-2.0
//! Offset reads over retained bytes through [`View`].
#![deny(clippy::disallowed_methods)]

use cadmpeg_core::decode::View;

pub(crate) fn u16_be_at(bytes: &[u8], offset: usize) -> Option<u16> {
    let mut view = View::over_retained(bytes);
    view.seek(offset)?;
    view.u16_be()
}

pub(crate) fn u32_be_at(bytes: &[u8], offset: usize) -> Option<u32> {
    let mut view = View::over_retained(bytes);
    view.seek(offset)?;
    view.u32_be()
}

pub(crate) fn u64_be_at(bytes: &[u8], offset: usize) -> Option<u64> {
    let mut view = View::over_retained(bytes);
    view.seek(offset)?;
    view.u64_be()
}

pub(crate) fn f64_be_at(bytes: &[u8], offset: usize) -> Option<f64> {
    let mut view = View::over_retained(bytes);
    view.seek(offset)?;
    view.f64_be()
}

pub(crate) fn vec3_be_at(bytes: &[u8], offset: usize) -> Option<[f64; 3]> {
    Some([
        f64_be_at(bytes, offset)?,
        f64_be_at(bytes, offset.checked_add(8)?)?,
        f64_be_at(bytes, offset.checked_add(16)?)?,
    ])
}

pub(crate) fn u32_le_at(bytes: &[u8], offset: usize) -> Option<u32> {
    let mut view = View::over_retained(bytes);
    view.seek(offset)?;
    view.u32_le()
}
