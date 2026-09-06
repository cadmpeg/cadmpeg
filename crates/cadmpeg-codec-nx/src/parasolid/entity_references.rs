// SPDX-License-Identifier: Apache-2.0
//! The bounded trailing reference collection of an Entity 51 record.

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct EntityReferences(Vec<u32>);

impl EntityReferences {
    pub(crate) fn new(values: Vec<u32>) -> Result<Self, &'static str> {
        if !(1..=32).contains(&values.len()) {
            return Err("trailing_references: must contain 1 through 32 references");
        }
        Ok(Self(values))
    }
    pub(crate) fn values(&self) -> &[u32] { &self.0 }
    #[cfg(test)]
    pub(crate) fn values_mut(&mut self) -> &mut [u32] { &mut self.0 }
    pub(crate) fn into_values(self) -> Vec<u32> { self.0 }
}
