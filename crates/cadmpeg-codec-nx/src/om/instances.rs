// SPDX-License-Identifier: Apache-2.0
//! Complete multi-instance selector groups and their trailing references.

use std::collections::BTreeMap;
use super::compact::LocatedCompactIndex;
use super::reference_index::FeatureReferenceToken;
use super::PayloadObjectReference;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct MultiInstanceOutputs<O> {
    selectors: Vec<LocatedCompactIndex<O>>,
    references: Vec<PayloadObjectReference<FeatureReferenceToken, O>>,
}

impl<O> MultiInstanceOutputs<O> {
    pub(crate) fn new(
        rows: Vec<(LocatedCompactIndex<O>, u8)>,
        references: Vec<PayloadObjectReference<FeatureReferenceToken, O>>,
    ) -> Result<Self, &'static str> {
        if !(1..=254).contains(&rows.len()) {
            return Err("selectors: must contain 1 through 254 rows");
        }
        if !(1..=254).contains(&references.len()) {
            return Err("trailing_object_indices: must contain 1 through 254 references");
        }
        let mut ordinals = BTreeMap::<u32, usize>::new();
        for (selector, ordinal) in &rows {
            let expected = ordinals.entry(selector.atom.value()).or_insert(1);
            *expected += 1;
            if usize::from(*ordinal) != *expected {
                return Err("ordinals: each selector must enumerate instances from two");
            }
        }
        if ordinals.values().any(|ordinal| *ordinal != references.len() + 1) {
            return Err("trailing_object_indices: each selector must cover every instance");
        }
        Ok(Self {
            selectors: rows.into_iter().map(|(selector, _)| selector).collect(),
            references,
        })
    }

    pub(crate) fn selectors(&self) -> &[LocatedCompactIndex<O>] { &self.selectors }

    pub(crate) fn references(&self) -> &[PayloadObjectReference<FeatureReferenceToken, O>] {
        &self.references
    }

    pub(crate) fn ordinals(&self) -> impl ExactSizeIterator<Item = u8> + '_ {
        let mut ordinals = BTreeMap::<u32, u8>::new();
        self.selectors.iter().map(move |selector| {
            let ordinal = ordinals.entry(selector.atom.value()).or_insert(1);
            *ordinal += 1;
            *ordinal
        })
    }

    pub(crate) fn map_offsets<P>(self, mut map: impl FnMut(O) -> P) -> MultiInstanceOutputs<P> {
        MultiInstanceOutputs {
            selectors: self.selectors.into_iter().map(|selector| LocatedCompactIndex {
                atom: selector.atom, offset: map(selector.offset),
            }).collect(),
            references: self.references.into_iter().map(|reference| PayloadObjectReference {
                token: reference.token, offset: map(reference.offset),
            }).collect(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::om::compact::CompactIndexAtom;

    #[test]
    fn instance_ordinal_derivation_reaches_the_byte_limit() {
        let rows = (2..=u8::MAX).map(|ordinal| (
            LocatedCompactIndex { atom: CompactIndexAtom::from_wire(7, &[7]).unwrap(), offset: usize::from(ordinal) },
            ordinal,
        )).collect();
        let references = (2..=u8::MAX).map(|ordinal| PayloadObjectReference {
            token: FeatureReferenceToken::from_wire(9, &[9]).unwrap(), offset: usize::from(ordinal),
        }).collect();
        let outputs = MultiInstanceOutputs::new(rows, references).unwrap();
        assert_eq!(outputs.ordinals().collect::<Vec<_>>(), (2..=u8::MAX).collect::<Vec<_>>());
        let mapped = outputs.map_offsets(|offset| offset as u64 + 1000);
        assert_eq!(mapped.selectors().len(), 254);
        assert_eq!(mapped.references().len(), 254);
        assert_eq!(mapped.selectors()[253].offset, 1255);
        assert_eq!(mapped.references()[253].offset, 1255);
        assert_eq!(mapped.ordinals().last(), Some(255));
    }

    #[test]
    fn instance_groups_require_nonempty_byte_counted_rows_and_references() {
        let selector = LocatedCompactIndex { atom: CompactIndexAtom::from_wire(7, &[7]).unwrap(), offset: 0 };
        let reference = PayloadObjectReference { token: FeatureReferenceToken::from_wire(9, &[9]).unwrap(), offset: 0 };
        assert!(MultiInstanceOutputs::new(Vec::<(LocatedCompactIndex, u8)>::new(), vec![reference.clone()]).is_err());
        assert!(MultiInstanceOutputs::new(vec![(selector, 2)], Vec::new()).is_err());
        assert!(MultiInstanceOutputs::new(vec![(selector, 2); 255], vec![reference.clone()]).is_err());
        assert!(MultiInstanceOutputs::new(vec![(selector, 2)], vec![reference; 255]).is_err());
    }
}
