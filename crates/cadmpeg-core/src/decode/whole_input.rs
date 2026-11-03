// SPDX-License-Identifier: Apache-2.0
//! A detection prefix proven to hold every byte of its source.

use std::io::SeekFrom;

use crate::decode::View;
use crate::{CodecError, ReadSeek};

/// A detection prefix that holds every byte of its source.
///
/// [`WholeInput::of_source`] is the only constructor, so a holder knows the
/// source has no byte beyond the view.
#[derive(Debug, Clone, Copy)]
pub struct WholeInput<'a> {
    view: View<'a>,
}

impl<'a> WholeInput<'a> {
    /// Proves that `prefix` is the whole of `source`.
    ///
    /// `prefix` must start at offset 0 and its length must equal the source
    /// length. The source position is restored on every path that returns.
    pub fn of_source(
        prefix: View<'a>,
        source: &mut dyn ReadSeek,
    ) -> Result<Option<Self>, CodecError> {
        let entry = source.stream_position()?;
        let length = source.seek(SeekFrom::End(0));
        source.seek(SeekFrom::Start(entry))?;
        let length = length?;
        let whole =
            prefix.start() == 0 && crate::decode::u64_from_index(prefix.window().len()) == length;
        Ok(whole.then_some(Self { view: prefix }))
    }

    /// The bytes of the whole source.
    pub fn view(self) -> View<'a> {
        self.view
    }
}

#[cfg(test)]
mod tests;
