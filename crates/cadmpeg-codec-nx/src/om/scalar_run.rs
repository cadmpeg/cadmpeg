// SPDX-License-Identifier: Apache-2.0
//! Typed scalar frames with contiguous, derived payload positions.

use super::fixed::Q155Atom;
use super::nonempty::NonEmpty;
use super::scalar::{ShiftedBinary32, ShiftedScalar};

pub(crate) trait AtomWidth {
    fn width(&self) -> u64;
}

impl AtomWidth for ShiftedScalar {
    fn width(&self) -> u64 { self.raw().len() as u64 }
}

impl AtomWidth for ShiftedBinary32 {
    fn width(&self) -> u64 { 4 }
}

impl AtomWidth for Q155Atom {
    fn width(&self) -> u64 { 8 }
}

pub(crate) trait ScalarFrame: Copy + std::fmt::Debug + Eq {
    type Atom: AtomWidth + Clone + std::fmt::Debug + Eq;
    fn prefix_len(self) -> u64;
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct FramedScalarRun<F: ScalarFrame, O> {
    form: F,
    offset: u64,
    values: NonEmpty<(F::Atom, O)>,
}

impl<F: ScalarFrame, O> FramedScalarRun<F, O> {
    pub(crate) fn new(form: F, offset: u64, values: NonEmpty<(F::Atom, O)>) -> Result<Self, &'static str> {
        let start = offset.checked_add(form.prefix_len())
            .ok_or("value_payload_offsets overflow the discriminator")?;
        values.iter().try_fold(start, |at, (atom, _)| at.checked_add(atom.width()))
            .ok_or("value_payload_offsets overflow the scalar run")?;
        Ok(Self { form, offset, values })
    }

    pub(crate) fn offset(&self) -> u64 { self.offset }

    pub(crate) fn form(&self) -> F { self.form }

    pub(crate) fn end(&self) -> u64 {
        self.values.iter().fold(self.offset + self.form.prefix_len(), |at, (atom, _)| at + atom.width())
    }

    pub(crate) fn iter(&self) -> impl Iterator<Item = (u64, &F::Atom, &O)> {
        let mut at = self.offset + self.form.prefix_len();
        self.values.iter().map(move |(atom, location)| {
            let offset = at;
            at += atom.width();
            (offset, atom, location)
        })
    }

    pub(crate) fn try_map_locations<P>(self, mut map: impl FnMut(u64, O) -> Option<P>) -> Option<FramedScalarRun<F, P>> {
        let mut at = self.offset + self.form.prefix_len();
        let values = self.values.map(|(atom, location)| {
            let offset = at;
            at += atom.width();
            Some((atom, map(offset, location)?))
        }).transpose()?;
        Some(FramedScalarRun { form: self.form, offset: self.offset, values })
    }
}
