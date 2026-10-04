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
        match Self::validate(&rows, &references, |rows| Ok::<_, std::convert::Infallible>(rows.iter())) {
            Ok(validation) => validation?,
            Err(error) => match error {},
        };
        Ok(Self {
            selectors: rows.into_iter().map(|(selector, _)| selector).collect(),
            references,
        })
    }

    fn validate<'a, E, I: Iterator<Item = &'a (LocatedCompactIndex<O>, u8)>>(
        rows: &'a [(LocatedCompactIndex<O>, u8)],
        references: &[PayloadObjectReference<FeatureReferenceToken, O>],
        mut admit: impl FnMut(&'a [(LocatedCompactIndex<O>, u8)]) -> Result<I, E>,
    ) -> Result<Result<(), &'static str>, E>
    where O: 'a {
        if !(1..=254).contains(&rows.len()) {
            return Ok(Err("selectors: must contain 1 through 254 rows"));
        }
        if !(1..=254).contains(&references.len()) {
            return Ok(Err("trailing_object_indices: must contain 1 through 254 references"));
        }
        for (position, (selector, ordinal)) in admit(rows)?.enumerate() {
            let preceding = admit(&rows[..position])?
                .filter(|(prior, _)| prior.atom.value() == selector.atom.value()).count();
            if usize::from(*ordinal) != preceding + 2 {
                return Ok(Err("ordinals: each selector must enumerate instances from two"));
            }
        }
        for (selector, _) in admit(rows)? {
            if admit(rows)?.filter(|(other, _)| other.atom.value() == selector.atom.value()).count() != references.len() {
                return Ok(Err("trailing_object_indices: each selector must cover every instance"));
            }
        }
        Ok(Ok(()))
    }

    pub(crate) fn new_charged(
        ctx: &DecodeContext<'_>,
        rows: Vec<(LocatedCompactIndex<O>, u8)>,
        references: Vec<PayloadObjectReference<FeatureReferenceToken, O>>,
    ) -> Result<Option<Self>, CodecError>
    where O: Copy {
        if Self::validate(&rows, &references, |rows| ctx.admit_iter(rows, "nx instance selector validation"))?.is_err() {
            return Ok(None);
        }
        let mut selectors = Vec::new();
        for (selector, _) in ctx.admit_iter(&rows, "nx instance selector projection")?.copied() {
            ctx.reserve_vec(&mut selectors, 1, "nx instance selectors")?;
            selectors.push(selector);
        }
        Ok(Some(Self {
            selectors,
            references,
        }))
    }

    pub(crate) fn selectors(&self) -> &[LocatedCompactIndex<O>] {
        &self.selectors
    }

    pub(crate) fn references(&self) -> &[PayloadObjectReference<FeatureReferenceToken, O>] {
        &self.references
    }

    #[cfg(test)]
    pub(crate) fn ordinals(&self) -> impl ExactSizeIterator<Item = u8> + '_ {
        self.selectors
            .iter()
            .enumerate()
            .map(move |(position, selector)| {
                let preceding = self.selectors[..position]
                    .iter()
                    .filter(|prior| prior.atom.value() == selector.atom.value())
                    .count();
                u8::try_from(preceding + 2).expect("validated selector count fits u8")
            })
    }

    pub(crate) fn map_offsets<P>(
        self,
        ctx: &DecodeContext<'_>,
        mut map: impl FnMut(O) -> P,
    ) -> Result<MultiInstanceOutputs<P>, CodecError>
    where O: Copy {
        let mut selectors = Vec::new();
        for selector in ctx.admit_iter(&self.selectors, "nx mapped instance selectors")? {
            ctx.reserve_vec(&mut selectors, 1, "nx mapped instance selectors")?;
            selectors.push(LocatedCompactIndex {
                atom: selector.atom,
                offset: map(selector.offset),
            });
        }
        let mut references = Vec::new();
        for reference in ctx.admit_iter(&self.references, "nx mapped instance references")? {
            ctx.reserve_vec(&mut references, 1, "nx mapped instance references")?;
            references.push(PayloadObjectReference {
                token: reference.token,
                offset: map(reference.offset),
            });
        }
        Ok(MultiInstanceOutputs {
            selectors,
            references,
        })
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
        let mapped = crate::test_support::with_decode_context(|ctx| {
            outputs.map_offsets(ctx, |offset| {
                cadmpeg_core::decode::u64_from_index(offset) + 1000
            })
        })
        .unwrap();
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
    #[test]
    fn instance_selector_iteration_refusal_propagates() {
        use cadmpeg_core::decode::ResourceDimension;
        use cadmpeg_core::CodecError;
        let selector = LocatedCompactIndex {
            atom: CompactIndexAtom::from_wire(7, &[7]).unwrap(), offset: 0_usize,
        };
        let reference = PayloadObjectReference {
            token: FeatureReferenceToken::from_wire(9, &[9]).unwrap(), offset: 0_usize,
        };
        for ordinal in [2, 3] {
            let error = crate::test_support::resource_refusal_at(
                &[], ResourceDimension::WorkUnits, "nx instance selector validation",
                |ctx| MultiInstanceOutputs::new_charged(ctx, vec![(selector, ordinal)], vec![reference.clone()]),
            );
            assert!(matches!(error, CodecError::ResourceLimit(limit)
                if limit.dimension == ResourceDimension::WorkUnits && limit.operation == "nx instance selector validation"));
        }
    }

}
