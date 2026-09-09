// SPDX-License-Identifier: Apache-2.0

/// One fixed byte or wildcard position.
pub(super) type PatternByte = Option<u8>;

/// A nonempty byte pattern with optional wildcard positions.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Pattern(Vec<PatternByte>);

impl Pattern {
    pub(super) fn new(bytes: Vec<PatternByte>) -> Result<Self, String> {
        if bytes.is_empty() {
            return Err("empty pattern; expected hexadecimal byte pairs such as `4d5a??00`".into());
        }
        Ok(Self(bytes))
    }

    /// Returns the number of byte positions.
    pub(crate) fn len(&self) -> usize {
        self.0.len()
    }

    pub(super) fn bytes(&self) -> &[PatternByte] {
        &self.0
    }
}

#[cfg(test)]
mod tests {
    use super::Pattern;

    #[test]
    fn empty_pattern_is_refused_at_construction() {
        assert!(Pattern::new(Vec::new()).is_err());
    }
}
