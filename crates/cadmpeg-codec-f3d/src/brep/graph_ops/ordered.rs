// SPDX-License-Identifier: Apache-2.0
//! Ordered graph keys admit the bytes inspected by B-tree callbacks.

use std::borrow::Cow;
use std::cmp::Ordering;

use cadmpeg_core::decode::DecodeContext;
use cadmpeg_ir::ids::comparison::compare;

#[derive(Debug)]
pub(in crate::brep) struct Key<'ctx> {
    text: Cow<'ctx, str>,
    ctx: &'ctx DecodeContext<'ctx>,
}

impl<'ctx> Key<'ctx> {
    pub(in crate::brep) fn owned(ctx: &'ctx DecodeContext<'_>, text: String) -> Self {
        Self { text: Cow::Owned(text), ctx }
    }

    pub(super) fn borrowed(ctx: &'ctx DecodeContext<'_>, text: &'ctx str) -> Self {
        Self { text: Cow::Borrowed(text), ctx }
    }
}

impl AsRef<str> for Key<'_> {
    fn as_ref(&self) -> &str { &self.text }
}

impl PartialEq for Key<'_> {
    fn eq(&self, other: &Self) -> bool { self.cmp(other) == Ordering::Equal }
}
impl Eq for Key<'_> {}
impl PartialOrd for Key<'_> {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> { Some(self.cmp(other)) }
}
impl Ord for Key<'_> {
    fn cmp(&self, other: &Self) -> Ordering {
        match compare(self.ctx, self.as_ref(), other.as_ref(), "compare F3D BREP graph ID") {
            Ok(order) => order,
            // Each tree operation observes the fused session before exposing its result.
            Err(_) => Ordering::Equal,
        }
    }
}

#[cfg(test)]
mod tests;
