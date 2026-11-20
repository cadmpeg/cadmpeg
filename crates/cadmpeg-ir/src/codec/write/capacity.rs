// SPDX-License-Identifier: Apache-2.0
//! Text fields of a fixed width (§3.4, Content the target cannot hold).
//!
//! A writer declares one [`TextCapacity`] per text field in its
//! `CAPACITY_FIELDS`. A text wider than its field is cut at a character
//! boundary to the field's width, the writer writes the cut's length as the
//! count the format stores beside the text, and the writer charges
//! `export:value.unwritable` with [`FittedText::unwritable_entry`].
//!
//! A character boundary is a Unicode scalar value boundary.

/// The code unit a text field's stored count counts.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TextUnit {
    /// UTF-8 bytes.
    Utf8Byte,
    /// UTF-16 code units.
    Utf16CodeUnit,
}

impl TextUnit {
    const fn units(self, character: char) -> usize {
        match self {
            Self::Utf8Byte => character.len_utf8(),
            Self::Utf16CodeUnit => character.len_utf16(),
        }
    }

    fn count(self, text: &str) -> usize {
        match self {
            Self::Utf8Byte => text.len(),
            Self::Utf16CodeUnit => text.encode_utf16().count(),
        }
    }

    const fn name(self) -> &'static str {
        match self {
            Self::Utf8Byte => "UTF-8 bytes",
            Self::Utf16CodeUnit => "UTF-16 code units",
        }
    }
}

/// One text field of a writer and the most code units it stores.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TextCapacity {
    /// The field's name in the writer's format specification.
    pub field: &'static str,
    /// The unit the field's stored count counts.
    pub unit: TextUnit,
    /// The most units the field stores.
    pub width: usize,
}

impl TextCapacity {
    /// A field of `width` units.
    #[must_use]
    pub const fn new(field: &'static str, unit: TextUnit, width: usize) -> Self {
        Self { field, unit, width }
    }

    /// The longest prefix of `text` that ends on a character boundary and
    /// fits the field.
    #[must_use]
    pub fn fit<'a>(&self, text: &'a str) -> FittedText<'a> {
        let mut units = 0usize;
        let mut end = 0usize;
        for (start, character) in text.char_indices() {
            match units.checked_add(self.unit.units(character)) {
                Some(next) if next <= self.width => {
                    units = next;
                    end = start + character.len_utf8();
                }
                _ => break,
            }
        }
        FittedText {
            text: text.get(..end).unwrap_or_default(),
            units,
            stated_units: self.unit.count(text),
            capacity: *self,
        }
    }
}

/// A text as its field stores it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FittedText<'a> {
    /// The text the field holds.
    pub text: &'a str,
    /// The length of `text` in the field's unit: the count the writer stores.
    pub units: usize,
    /// The length of the stated text in the field's unit.
    pub stated_units: usize,
    capacity: TextCapacity,
}

impl FittedText<'_> {
    /// Whether the field holds less than the stated text.
    #[must_use]
    pub const fn was_cut(&self) -> bool {
        self.units != self.stated_units
    }

    /// The `export:value.unwritable` entry naming `record` and the field's
    /// width, when the text was cut.
    #[must_use]
    pub fn unwritable_entry(&self, record: &str) -> Option<String> {
        self.was_cut().then(|| {
            format!(
                "{record} {}: {} {} cut to {}; the field holds {}",
                self.capacity.field,
                self.stated_units,
                self.capacity.unit.name(),
                self.units,
                self.capacity.width
            )
        })
    }
}

#[cfg(test)]
mod tests;
