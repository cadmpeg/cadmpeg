// SPDX-License-Identifier: Apache-2.0
//! Contiguous scalar triples anchored to ordered operation body references.

use super::operation_record::OperationBodyInput;
use super::scalar::PayloadScalarAtom;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ScalarTriple {
    origin: u64,
    atoms: [PayloadScalarAtom; 3],
}

impl ScalarTriple {
    pub fn new(origin: u64, atoms: [PayloadScalarAtom; 3]) -> Option<Self> {
        atoms
            .iter()
            .try_fold(origin, |at, atom| at.checked_add(atom.raw().len() as u64))?;
        Some(Self { origin, atoms })
    }

    pub fn atoms(&self) -> &[PayloadScalarAtom; 3] {
        &self.atoms
    }

    pub fn source_offsets(self) -> [u64; 3] {
        let second = self.origin + self.atoms[0].raw().len() as u64;
        [
            self.origin,
            second,
            second + self.atoms[1].raw().len() as u64,
        ]
    }

    pub fn relocate(self, base: u64) -> Option<Self> {
        Self::new(self.origin.checked_add(base)?, self.atoms)
    }
}

/// One three-scalar clause anchored to an ordered operation body reference.
#[derive(Debug, Clone, PartialEq)]
pub struct OperationBodyScalarTriple {
    /// Zero-based body-reference occurrence order.
    pub body_reference_ordinal: u32,
    /// Serialized body object index.
    pub body_object_index: u32,
    /// Branch discriminator following the body-reference terminator.
    pub branch: u8,
    /// Three scalar atoms in byte order.
    pub scalars: ScalarTriple,
}

/// Decode complete three-scalar clauses following ordered operation body fields.
pub fn operation_body_scalar_triples(
    record: OperationBodyInput<'_>,
) -> Vec<OperationBodyScalarTriple> {
    super::operation_body_references(record)
        .into_iter()
        .enumerate()
        .filter_map(|(ordinal, reference)| {
            let token = reference.offset - record.offset();
            let end = token + reference.object_index.raw().len();
            let branch = *record.bytes().get(end + 1)?;
            let mut at = end + 2;
            let origin = record.offset().checked_add(at)? as u64;
            let mut read = || {
                let atom = PayloadScalarAtom::read(record.bytes().get(at..)?)?;
                at += atom.raw().len();
                Some(atom)
            };
            let scalars = ScalarTriple::new(origin, [read()?, read()?, read()?])?;
            Some(OperationBodyScalarTriple {
                body_reference_ordinal: ordinal as u32,
                body_object_index: reference.object_index.value(),
                branch,
                scalars,
            })
        })
        .collect()
}
