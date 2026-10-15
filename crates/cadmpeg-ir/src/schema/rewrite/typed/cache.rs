// SPDX-License-Identifier: Apache-2.0
//! Borrowed replacement lookup over keys in byte order.

use cadmpeg_core::CodecError;

/// Borrowed replacements require unique keys in byte order.
#[derive(Debug)]
pub(super) struct ReplacementIndex<'ctx> {
    values: Vec<(&'ctx str, &'ctx str)>,
}

impl<'ctx> ReplacementIndex<'ctx> {
    pub(super) fn build<K: AsRef<str> + 'ctx>(
        replacements: impl ExactSizeIterator<Item = (&'ctx K, &'ctx String)>,
    ) -> Result<Self, CodecError> {
        let mut values: Vec<(&'ctx str, &'ctx str)> = Vec::new();
        for (source, target) in replacements {
            let source = source.as_ref();
            if let Some((previous, _)) = values.last() {
                if *previous >= source {
                    return Err(CodecError::malformed(
                        "text replacements must have unique keys in byte order",
                    ));
                }
            }
            values.push((source, target.as_str()));
        }
        Ok(Self { values })
    }

    pub(super) fn get(&self, source: &str) -> Option<&'ctx str> {
        self.values
            .binary_search_by(|(candidate, _)| (*candidate).cmp(source))
            .ok()
            .map(|position| self.values[position].1)
    }
}

#[cfg(test)]
mod tests;
