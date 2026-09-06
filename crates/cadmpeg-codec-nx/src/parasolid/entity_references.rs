// SPDX-License-Identifier: Apache-2.0
//! The bounded trailing reference collection of an Entity 51 record.

use serde::{Deserialize, Serialize};

const LEADING_REFERENCE_COUNT: u32 = 5;
const MAX_TRAILING_REFERENCE_COUNT: usize = 32;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(try_from = "u32", into = "u32")]
pub(crate) struct FieldPosition(u32);

impl FieldPosition {
    pub(crate) fn reference_ordinal(self) -> u32 { self.0 }
    pub(crate) fn field_ordinal(self) -> u32 { self.0 - LEADING_REFERENCE_COUNT }
}

impl From<FieldPosition> for u32 {
    fn from(position: FieldPosition) -> Self { position.0 }
}

impl TryFrom<u32> for FieldPosition {
    type Error = &'static str;

    fn try_from(reference_ordinal: u32) -> Result<Self, Self::Error> {
        let end = LEADING_REFERENCE_COUNT + MAX_TRAILING_REFERENCE_COUNT as u32;
        if !(LEADING_REFERENCE_COUNT..end).contains(&reference_ordinal) {
            return Err("reference_ordinal must address a trailing field in slots 5 through 36");
        }
        Ok(Self(reference_ordinal))
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct EntityReferences(Vec<u32>);

impl EntityReferences {
    pub(crate) fn new(values: Vec<u32>) -> Result<Self, &'static str> {
        if !(1..=MAX_TRAILING_REFERENCE_COUNT).contains(&values.len()) {
            return Err("trailing_references: must contain 1 through 32 references");
        }
        Ok(Self(values))
    }
    pub(crate) fn fields(&self) -> impl Iterator<Item = (FieldPosition, &u32)> {
        self.0.iter().enumerate().map(|(ordinal, value)| {
            (FieldPosition(ordinal as u32 + LEADING_REFERENCE_COUNT), value)
        })
    }
    pub(crate) fn values(&self) -> &[u32] { &self.0 }
    #[cfg(test)]
    pub(crate) fn values_mut(&mut self) -> &mut [u32] { &mut self.0 }
    pub(crate) fn into_values(self) -> Vec<u32> { self.0 }
}

#[cfg(test)]
mod tests {
    use super::{EntityReferences, FieldPosition};

    #[test]
    fn field_positions_cover_only_the_bounded_trailing_lane() {
        for reference in [5, 36] {
            let position = FieldPosition::try_from(reference).unwrap();
            let wire = reference.to_string();
            assert_eq!(position.field_ordinal(), reference - 5);
            assert_eq!(serde_json::to_string(&position).unwrap(), wire);
            assert_eq!(serde_json::from_str::<FieldPosition>(&wire).unwrap(), position);
        }
        for reference in [0, 4, 37, u32::MAX] {
            assert!(FieldPosition::try_from(reference).is_err());
            assert!(serde_json::from_str::<FieldPosition>(&reference.to_string()).is_err());
        }
        let references = EntityReferences::new(vec![10; 32]).unwrap();
        let positions = references.fields().map(|(position, _)| position.reference_ordinal());
        assert!(positions.eq(5..=36));
    }
}
