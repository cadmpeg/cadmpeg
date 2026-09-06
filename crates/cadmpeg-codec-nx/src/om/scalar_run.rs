// SPDX-License-Identifier: Apache-2.0
//! Contiguous scalar runs with derived payload positions.

use super::nonempty::NonEmpty;
use super::scalar::ShiftedScalar;

pub(crate) trait AtomWidth {
    fn width(&self) -> u64;
}

impl AtomWidth for ShiftedScalar {
    fn width(&self) -> u64 { self.raw().len() as u64 }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ScalarRun<A, O> {
    start: u64,
    values: NonEmpty<(A, O)>,
}

impl<A: AtomWidth, O> ScalarRun<A, O> {
    pub(crate) fn new(start: u64, values: NonEmpty<(A, O)>) -> Result<Self, &'static str> {
        values.iter().try_fold(start, |at, (atom, _)| at.checked_add(atom.width()))
            .ok_or("value_payload_offsets overflow the scalar run")?;
        Ok(Self { start, values })
    }

    pub(crate) fn start(&self) -> u64 { self.start }

    pub(crate) fn end(&self) -> u64 {
        self.values.iter().fold(self.start, |at, (atom, _)| at + atom.width())
    }

    pub(crate) fn iter(&self) -> impl Iterator<Item = (u64, &A, &O)> {
        let mut at = self.start;
        self.values.iter().map(move |(atom, location)| {
            let offset = at;
            at += atom.width();
            (offset, atom, location)
        })
    }

    pub(crate) fn try_map_locations<P>(self, mut map: impl FnMut(u64, O) -> Option<P>) -> Option<ScalarRun<A, P>> {
        let mut at = self.start;
        let values = self.values.map(|(atom, location)| {
            let offset = at;
            at += atom.width();
            Some((atom, map(offset, location)?))
        }).transpose()?;
        Some(ScalarRun { start: self.start, values })
    }
}
