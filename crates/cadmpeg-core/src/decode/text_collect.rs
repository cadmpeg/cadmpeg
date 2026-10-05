// SPDX-License-Identifier: Apache-2.0
//! Charged collection of character and text fragments.

use super::{DecodeContext, ScopedReservation};
use crate::CodecError;
use std::borrow::Cow;

mod sealed {
    pub trait Fragment {}
}

/// A character or borrowed fragment projected without scanning or copying.
pub enum Fragment<'a> {
    /// Existing UTF-8 bytes.
    Text(&'a str),
    /// One Unicode scalar value.
    Character(char),
}

/// Closed fragment sources for String collection.
pub trait TextFragment: sealed::Fragment {
    /// Borrows text or copies one scalar without allocating.
    fn fragment(&self) -> Fragment<'_>;
}

impl sealed::Fragment for str {}
impl TextFragment for str {
    fn fragment(&self) -> Fragment<'_> {
        Fragment::Text(self)
    }
}
impl sealed::Fragment for String {}
impl TextFragment for String {
    fn fragment(&self) -> Fragment<'_> {
        Fragment::Text(self.as_str())
    }
}
impl sealed::Fragment for char {}
impl TextFragment for char {
    fn fragment(&self) -> Fragment<'_> {
        Fragment::Character(*self)
    }
}
impl sealed::Fragment for Cow<'_, str> {}
impl TextFragment for Cow<'_, str> {
    fn fragment(&self) -> Fragment<'_> {
        Fragment::Text(self.as_ref())
    }
}
impl sealed::Fragment for Box<str> {}
impl TextFragment for Box<str> {
    fn fragment(&self) -> Fragment<'_> {
        Fragment::Text(self.as_ref())
    }
}
impl<T: TextFragment + ?Sized> sealed::Fragment for &T {}
impl<T: TextFragment + ?Sized> TextFragment for &T {
    fn fragment(&self) -> Fragment<'_> {
        T::fragment(*self)
    }
}

impl DecodeContext<'_> {
    fn append_fragment<S: TextFragment>(
        &self,
        output: &mut String,
        value: S,
        operation: &'static str,
    ) -> Result<(), CodecError> {
        match value.fragment() {
            Fragment::Text(text) => self.append_retained(output, text, operation),
            Fragment::Character(character) => self.push_retained_char(output, character, operation),
        }
    }

    fn collect_text_with<S: TextFragment>(
        &self,
        values: impl IntoIterator<Item = S>,
        operation: &'static str,
        mut append: impl FnMut(&mut String, S) -> Result<(), CodecError>,
    ) -> Result<String, CodecError> {
        let mut output = String::new();
        let mut input = values.into_iter();
        while let Some(value) = self.next_charged(&mut input, operation)? {
            append(&mut output, value)?;
        }
        Ok(output)
    }

    /// Collects chars or fragments into retained UTF-8 text.
    /// Adapted sources require admitted bases; each source step precedes append.
    pub fn collect_text<S: TextFragment>(
        &self,
        values: impl IntoIterator<Item = S>,
        operation: &'static str,
    ) -> Result<String, CodecError> {
        self.collect_text_with(values, operation, |output, value| {
            self.append_fragment(output, value, operation)
        })
    }

    /// Collects scoped UTF-8 text while source-produced children remain retained.
    /// Keep the returned reservation alive with the text.
    pub fn collect_scoped_text<'ctx, S: TextFragment>(
        &'ctx self,
        values: impl IntoIterator<Item = S>,
        operation: &'static str,
    ) -> Result<(String, ScopedReservation<'ctx>), CodecError> {
        let mut reservation = self.reserve_scoped(0, operation)?;
        let output = self.collect_text_with(values, operation, |output, value| {
            reservation.with_storage(|| self.append_fragment(output, value, operation))
        })?;
        Ok((output, reservation))
    }
}

#[cfg(test)]
mod tests;
