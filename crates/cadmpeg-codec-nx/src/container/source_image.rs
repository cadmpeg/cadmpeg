// SPDX-License-Identifier: Apache-2.0
//! Borrowed source bytes with their decode address space.

use cadmpeg_core::decode::View;

#[derive(Debug, Clone)]
pub(crate) struct SourceImage<'a>(pub(crate) View<'a>);

impl<'a> SourceImage<'a> {
    pub(crate) fn view(&self) -> View<'a> {
        self.0
    }
}

impl std::ops::Deref for SourceImage<'_> {
    type Target = [u8];
    fn deref(&self) -> &[u8] {
        self.view().window()
    }
}

impl AsRef<[u8]> for SourceImage<'_> {
    fn as_ref(&self) -> &[u8] {
        self
    }
}

impl<'a> From<&'a [u8]> for SourceImage<'a> {
    fn from(bytes: &'a [u8]) -> Self {
        Self(View::over_retained(bytes))
    }
}

impl<'a> From<&'a Vec<u8>> for SourceImage<'a> {
    fn from(bytes: &'a Vec<u8>) -> Self {
        Self::from(bytes.as_slice())
    }
}
