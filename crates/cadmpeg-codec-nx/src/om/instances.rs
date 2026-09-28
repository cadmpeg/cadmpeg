// SPDX-License-Identifier: Apache-2.0
//! Complete multi-instance selector groups and their trailing references.

use super::compact::LocatedCompactIndex;
use super::reference_index::FeatureReferenceToken;
use super::PayloadObjectReference;
use cadmpeg_core::decode::DecodeContext;
use cadmpeg_core::CodecError;

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
        Self::validate(&rows, &references)?;
        Ok(Self {
            selectors: rows.into_iter().map(|(selector, _)| selector).collect(),
            references,
        })
    }

    fn validate(
        rows: &[(LocatedCompactIndex<O>, u8)],
        references: &[PayloadObjectReference<FeatureReferenceToken, O>],
    ) -> Result<(), &'static str> {
        if !(1..=254).contains(&rows.len()) {
            return Err("selectors: must contain 1 through 254 rows");
        }
        if !(1..=254).contains(&references.len()) {
            return Err("trailing_object_indices: must contain 1 through 254 references");
        }
        for (position, (selector, ordinal)) in rows.iter().enumerate() {
            let preceding = rows[..position].iter().filter(|(prior, _)| prior.atom.value() == selector.atom.value()).count();
            if usize::from(*ordinal) != preceding + 2 {
                return Err("ordinals: each selector must enumerate instances from two");
            }
        }
        if rows.iter().any(|(selector, _)| {
            rows.iter().filter(|(other, _)| other.atom.value() == selector.atom.value()).count() != references.len()
        }) {
            return Err("trailing_object_indices: each selector must cover every instance");
        }
        Ok(())
    }

    pub(crate) fn new_charged(
        ctx: &DecodeContext<'_>,
        rows: Vec<(LocatedCompactIndex<O>, u8)>,
        references: Vec<PayloadObjectReference<FeatureReferenceToken, O>>,
    ) -> Result<Option<Self>, CodecError> {
        ctx.charge_work(
            cadmpeg_core::decode::u64_from_index(rows.len().checked_mul(rows.len()).ok_or_else(|| ctx.refuse_codec_limit("nx instance selector validation", u64::MAX, u64::MAX))?),
            "nx instance selector validation",
        )?;
        if Self::validate(&rows, &references).is_err() { return Ok(None); }
        let mut selectors = Vec::new();
        for (selector, _) in rows {
            super::reserve_om_retained_item(ctx, &mut selectors, "nx instance selectors")?;
            selectors.push(selector);
        }
        Ok(Some(Self { selectors, references }))
    }

    pub(crate) fn selectors(&self) -> &[LocatedCompactIndex<O>] {
        &self.selectors
    }

    pub(crate) fn references(&self) -> &[PayloadObjectReference<FeatureReferenceToken, O>] {
        &self.references
    }

    #[cfg(test)]
    pub(crate) fn ordinals(&self) -> impl ExactSizeIterator<Item = u8> + '_ {
        self.selectors.iter().enumerate().map(move |(position, selector)| {
            let preceding = self.selectors[..position].iter().filter(|prior| prior.atom.value() == selector.atom.value()).count();
            u8::try_from(preceding + 2).expect("validated selector count fits u8")
        })
    }

    pub(crate) fn map_offsets<P>(self, ctx: &DecodeContext<'_>, mut map: impl FnMut(O) -> P) -> Result<MultiInstanceOutputs<P>, CodecError> {
        let mut selectors = Vec::new();
        for selector in self.selectors {
            super::reserve_om_retained_item(ctx, &mut selectors, "nx mapped instance selectors")?;
            selectors.push(LocatedCompactIndex {
                    atom: selector.atom,
                    offset: map(selector.offset),
                });
        }
        let mut references = Vec::new();
        for reference in self.references {
            super::reserve_om_retained_item(ctx, &mut references, "nx mapped instance references")?;
            references.push(PayloadObjectReference {
                    token: reference.token,
                    offset: map(reference.offset),
                });
        }
        Ok(MultiInstanceOutputs { selectors, references })
    }
}

#[cfg(test)]
mod tests {
    use super::super::compact::LocatedCompactIndex;
    use super::super::reference_index::FeatureReferenceToken;
    use super::super::PayloadObjectReference;
    use super::MultiInstanceOutputs;
    use crate::om::compact::CompactIndexAtom;

    #[test]
    fn instance_ordinal_derivation_reaches_the_byte_limit() {
        let rows = (2..=u8::MAX)
            .map(|ordinal| {
                (
                    LocatedCompactIndex {
                        atom: CompactIndexAtom::from_wire(7, &[7]).unwrap(),
                        offset: usize::from(ordinal),
                    },
                    ordinal,
                )
            })
            .collect();
        let references = (2..=u8::MAX)
            .map(|ordinal| PayloadObjectReference {
                token: FeatureReferenceToken::from_wire(9, &[9]).unwrap(),
                offset: usize::from(ordinal),
            })
            .collect();
        let outputs = MultiInstanceOutputs::new(rows, references).unwrap();
        assert_eq!(
            outputs.ordinals().collect::<Vec<_>>(),
            (2..=u8::MAX).collect::<Vec<_>>()
        );
        let mapped = crate::test_support::with_decode_context(|ctx| outputs.map_offsets(ctx, |offset| offset as u64 + 1000)).unwrap();
        assert_eq!(mapped.selectors().len(), 254);
        assert_eq!(mapped.references().len(), 254);
        assert_eq!(mapped.selectors()[253].offset, 1255);
        assert_eq!(mapped.references()[253].offset, 1255);
        assert_eq!(mapped.ordinals().last(), Some(255));
    }

    #[test]
    fn instance_groups_require_nonempty_byte_counted_rows_and_references() {
        let selector = LocatedCompactIndex {
            atom: CompactIndexAtom::from_wire(7, &[7]).unwrap(),
            offset: 0,
        };
        let reference = PayloadObjectReference {
            token: FeatureReferenceToken::from_wire(9, &[9]).unwrap(),
            offset: 0,
        };
        assert!(MultiInstanceOutputs::new(
            Vec::<(LocatedCompactIndex, u8)>::new(),
            vec![reference.clone()]
        )
        .is_err());
        assert!(MultiInstanceOutputs::new(vec![(selector, 2)], Vec::new()).is_err());
        assert!(
            MultiInstanceOutputs::new(vec![(selector, 2); 255], vec![reference.clone()]).is_err()
        );
        assert!(MultiInstanceOutputs::new(vec![(selector, 2)], vec![reference; 255]).is_err());
    }
}
