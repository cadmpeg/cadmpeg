// SPDX-License-Identifier: Apache-2.0
//! Source bytes with their decode address space.

use cadmpeg_core::decode::View;

#[derive(Debug, Clone)]
pub(crate) enum SourceImage<'a> {
    Borrowed(View<'a>),
    Owned(Vec<u8>),
}

impl SourceImage<'_> {
    pub(crate) fn view(&self) -> View<'_> {
        match self {
            Self::Borrowed(view) => *view,
            Self::Owned(bytes) => View::over_retained(bytes),
        }
    }

    #[cfg(test)]
    pub(crate) fn to_mut(&mut self) -> &mut Vec<u8> {
        if let Self::Borrowed(view) = self {
            *self = Self::Owned(view.window().to_vec());
        }
        match self {
            Self::Owned(bytes) => bytes,
            Self::Borrowed(_) => unreachable!("borrowed image converted to owned storage"),
        }
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
        Self::Borrowed(View::over_retained(bytes))
    }
}

impl<'a> From<&'a Vec<u8>> for SourceImage<'a> {
    fn from(bytes: &'a Vec<u8>) -> Self {
        Self::from(bytes.as_slice())
    }
}

impl From<Vec<u8>> for SourceImage<'_> {
    fn from(bytes: Vec<u8>) -> Self {
        Self::Owned(bytes)
    }
}
