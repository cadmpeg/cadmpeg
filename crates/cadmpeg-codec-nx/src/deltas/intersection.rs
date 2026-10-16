// SPDX-License-Identifier: Apache-2.0
//! INTERSECTION storage selected by its effective transmitted field array.

use super::{Token, COMPOSITE_CURVE};
use crate::records::EmbeddedSchema;

#[derive(Debug, Clone, Copy)]
pub(super) enum IntersectionLayout {
    Base,
    WithData,
    Unsupported,
}

impl IntersectionLayout {
    pub(super) fn from_schema(schema: &EmbeddedSchema) -> Self {
        if schema.matches_base_with_pointer_tail(38, &[]) {
            Self::Base
        } else if schema.matches_base_with_pointer_tail(38, &[("intersection_data", 204)]) {
            Self::WithData
        } else {
            Self::Unsupported
        }
    }

    pub(super) fn signature(self) -> Option<&'static [Token]> {
        match self {
            Self::Base => Some(&COMPOSITE_CURVE[..COMPOSITE_CURVE.len() - 1]),
            Self::WithData => Some(COMPOSITE_CURVE),
            Self::Unsupported => None,
        }
    }
}

/// Five common-header pointers followed by five or six construction pointers.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum IntersectionReferences {
    Base([u32; 10]),
    WithData([u32; 11]),
}

impl IntersectionReferences {
    pub(super) fn values(&self) -> &[u32] {
        match self {
            Self::Base(values) => values,
            Self::WithData(values) => values,
        }
    }
}

impl TryFrom<Vec<u32>> for IntersectionReferences {
    type Error = &'static str;

    fn try_from(values: Vec<u32>) -> Result<Self, Self::Error> {
        if values.contains(&0) {
            return Err("INTERSECTION references: zero target");
        }
        match values.len() {
            10 => Ok(Self::Base(values.try_into().map_err(|_| "references")?)),
            11 => Ok(Self::WithData(values.try_into().map_err(|_| "references")?)),
            _ => Err("references: INTERSECTION requires ten or eleven pointers"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{IntersectionLayout, IntersectionReferences};
    use crate::records::EmbeddedSchema;
    use crate::test_support::with_decode_context;

    #[test]
    fn primary_base_and_declared_data_pointer_select_different_cardinalities() {
        with_decode_context(|ctx| {
            let (base, _) = crate::records::read(ctx, &[0, 38, 255], 0)
                .unwrap()
                .unwrap();
            assert!(matches!(
                IntersectionLayout::from_schema(&base),
                IntersectionLayout::Base
            ));
            let (modern, _) = crate::records::read(ctx, crate::topology::TYPE_38_SCHEMA_HEADER, 0)
                .unwrap()
                .unwrap();
            assert!(matches!(
                IntersectionLayout::from_schema(&modern),
                IntersectionLayout::WithData
            ));
            let wrong_kind: EmbeddedSchema = crate::records::read(ctx, &[0, 19, 255], 0)
                .unwrap()
                .unwrap()
                .0;
            assert!(matches!(
                IntersectionLayout::from_schema(&wrong_kind),
                IntersectionLayout::Unsupported
            ));
        });
    }

    #[test]
    fn wire_references_admit_only_the_two_declared_cardinalities() {
        for count in 0..14 {
            assert_eq!(
                IntersectionReferences::try_from(vec![1; count]).is_ok(),
                matches!(count, 10 | 11)
            );
        }
        for count in [10, 11] {
            let mut values = vec![1; count];
            values[count - 1] = 0;
            assert!(IntersectionReferences::try_from(values).is_err());
        }
    }
}
