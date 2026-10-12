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
        let mut counts = std::collections::BTreeMap::new();
        match Self::validate(
            &rows,
            &references,
            &mut counts,
            |rows| Ok::<_, std::convert::Infallible>(rows.next()),
            |counts, key| {
                let count = counts.entry(key).or_insert(0);
                let preceding = *count;
                *count += 1;
                Ok::<_, std::convert::Infallible>(preceding)
            },
            |counts, expected| {
                Ok::<_, std::convert::Infallible>(counts.values().all(|count| *count == expected))
            },
        ) {
            Ok(validation) => validation?,
            Err(error) => match error {},
        }
        Ok(Self {
            selectors: rows.into_iter().map(|(selector, _)| selector).collect(),
            references,
        })
    }

    fn validate<'a, E, C>(
        rows: &'a [(LocatedCompactIndex<O>, u8)],
        references: &[PayloadObjectReference<FeatureReferenceToken, O>],
        counts: &mut C,
        mut next: impl FnMut(
            &mut std::slice::Iter<'a, (LocatedCompactIndex<O>, u8)>,
        ) -> Result<Option<&'a (LocatedCompactIndex<O>, u8)>, E>,
        mut preceding: impl FnMut(&mut C, u32) -> Result<usize, E>,
        complete: impl FnOnce(&C, usize) -> Result<bool, E>,
    ) -> Result<Result<(), &'static str>, E>
    where
        O: 'a,
    {
        if !(1..=254).contains(&rows.len()) {
            return Ok(Err("selectors: must contain 1 through 254 rows"));
        }
        if !(1..=254).contains(&references.len()) {
            return Ok(Err(
                "trailing_object_indices: must contain 1 through 254 references",
            ));
        }
        let mut rows = rows.iter();
        while rows.len() > 0 {
            let Some((selector, ordinal)) = next(&mut rows)? else {
                break;
            };
            if usize::from(*ordinal) != preceding(counts, selector.atom.value())? + 2 {
                return Ok(Err(
                    "ordinals: each selector must enumerate instances from two",
                ));
            }
        }
        if !complete(counts, references.len())? {
            return Ok(Err(
                "trailing_object_indices: each selector must cover every instance",
            ));
        }
        Ok(Ok(()))
    }

    pub(crate) fn new_charged(
        ctx: &DecodeContext<'_>,
        rows: Vec<(LocatedCompactIndex<O>, u8)>,
        references: Vec<PayloadObjectReference<FeatureReferenceToken, O>>,
    ) -> Result<Option<Self>, CodecError>
    where
        O: Copy,
    {
        let mut counts = std::collections::BTreeMap::new();
        let mut storage = ctx.reserve_scoped(0, "NX instance occurrence workspace")?;
        if Self::validate(
            &rows,
            &references,
            &mut counts,
            |rows| ctx.next_charged(rows, "nx instance selector validation"),
            |counts, key| {
                storage.with_storage(|| {
                    let count = ctx
                        .entry_btree_map(counts, key, "NX instance occurrence index")?
                        .or_insert(0);
                    let preceding = *count;
                    *count += 1;
                    Ok::<_, CodecError>(preceding)
                })
            },
            |counts, expected| {
                ctx.all_by(
                    counts.values(),
                    |count| Ok(*count == expected),
                    "NX instance reference coverage",
                )
            },
        )?
        .is_err()
        {
            return Ok(None);
        }
        let mut selectors = ctx.collection_vec(rows.len(), "nx instance selectors")?;
        for (selector, _) in ctx.admit_iter(rows, "nx instance selector projection")? {
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
    where
        O: Copy,
    {
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
            atom: CompactIndexAtom::from_wire(7, &[7]).unwrap(),
            offset: 0_usize,
        };
        let reference = PayloadObjectReference {
            token: FeatureReferenceToken::from_wire(9, &[9]).unwrap(),
            offset: 0_usize,
        };
        for ordinal in [2, 3] {
            let error = crate::test_support::resource_refusal_at(
                &[],
                ResourceDimension::WorkUnits,
                "nx instance selector validation",
                |ctx| {
                    MultiInstanceOutputs::new_charged(
                        ctx,
                        vec![(selector, ordinal)],
                        vec![reference.clone()],
                    )
                },
            );
            assert!(matches!(error, CodecError::ResourceLimit(limit)
                if limit.dimension == ResourceDimension::WorkUnits && limit.operation == "nx instance selector validation"));
        }
    }
}
