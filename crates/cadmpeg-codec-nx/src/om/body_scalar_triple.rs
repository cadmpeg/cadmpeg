// SPDX-License-Identifier: Apache-2.0
//! Contiguous scalar triples anchored to ordered operation body references.

use super::operation_record::OperationBodyInput;
use super::scalar::PayloadScalarAtom;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct ScalarTriple {
    origin: u64,
    atoms: [PayloadScalarAtom; 3],
}

impl ScalarTriple {
    pub(crate) fn new(origin: u64, atoms: [PayloadScalarAtom; 3]) -> Option<Self> {
        atoms.iter().try_fold(origin, |at, atom| {
            at.checked_add(cadmpeg_core::decode::u64_from_index(atom.raw().len()))
        })?;
        Some(Self { origin, atoms })
    }

    pub(crate) fn atoms(&self) -> &[PayloadScalarAtom; 3] {
        &self.atoms
    }

    pub(crate) fn source_offsets(self) -> [u64; 3] {
        let second = self.origin + cadmpeg_core::decode::u64_from_index(self.atoms[0].raw().len());
        [
            self.origin,
            second,
            second + cadmpeg_core::decode::u64_from_index(self.atoms[1].raw().len()),
        ]
    }

    pub(crate) fn relocate(self, base: u64) -> Option<Self> {
        Self::new(self.origin.checked_add(base)?, self.atoms)
    }
}

/// One three-scalar clause anchored to an ordered operation body reference.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct OperationBodyScalarTriple {
    /// Zero-based body-reference occurrence order.
    pub(crate) body_reference_ordinal: u32,
    /// Serialized body object index.
    pub(crate) body_object_index: u32,
    /// Branch discriminator following the body-reference terminator.
    pub(crate) branch: u8,
    /// Three scalar atoms in byte order.
    pub(crate) scalars: ScalarTriple,
}

/// Decode complete three-scalar clauses following ordered operation body fields.
pub(crate) fn operation_body_scalar_triples(
    record: OperationBodyInput<'_>,
) -> Vec<OperationBodyScalarTriple> {
    let mut triples = Vec::new();
    for (ordinal, reference) in super::operation_body_reference_candidates(record).enumerate() {
        let parsed = (|| {
            let token = reference.offset - record.offset();
            let end = token + reference.object_index.raw().len();
            let branch = *record.bytes().get(end + 1)?;
            let mut at = end + 2;
            let origin = cadmpeg_core::decode::u64_from_index(record.offset().checked_add(at)?);
            let mut read = || {
                let atom = PayloadScalarAtom::read(record.bytes().get(at..)?)?;
                at += atom.raw().len();
                Some(atom)
            };
            let scalars = ScalarTriple::new(origin, [read()?, read()?, read()?])?;
            Some(OperationBodyScalarTriple {
                body_reference_ordinal: u32::try_from(ordinal).ok()?,
                body_object_index: reference.object_index.value(),
                branch,
                scalars,
            })
        })();
        if let Some(triple) = parsed {
            triples.push(triple);
        }
    }
    triples
}
