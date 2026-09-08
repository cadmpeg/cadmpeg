// SPDX-License-Identifier: Apache-2.0
//! Structured extrusion branch with three byte-counted reference lanes.

use super::branch_items::BranchItems;
use super::compact::{CompactIndexAtom, WrappedCompactIndex};
use super::operation_record::OperationBodyInput;
use super::reference_index::FeatureReferenceToken;
use super::scalar::ShiftedBinary64;
use cadmpeg_core::decode::View;

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Extrude32Frame<B> {
    origin: u64,
    scalar: ShiftedBinary64,
    atoms: BranchItems<(WrappedCompactIndex, B)>,
    first: BranchItems<(CompactIndexAtom, B)>,
    second: BranchItems<(CompactIndexAtom, B)>,
    terminal: FeatureReferenceToken,
}

impl<B> Extrude32Frame<B> {
    pub(crate) fn new(
        origin: u64,
        scalar: ShiftedBinary64,
        atoms: BranchItems<(WrappedCompactIndex, B)>,
        first: BranchItems<(CompactIndexAtom, B)>,
        second: BranchItems<(CompactIndexAtom, B)>,
        terminal: FeatureReferenceToken,
    ) -> Result<Self, &'static str> {
        let frame = Self {
            origin,
            scalar,
            atoms,
            first,
            second,
            terminal,
        };
        origin
            .checked_add(frame.terminal_position() + frame.terminal.raw().len() as u64 + 2)
            .ok_or("source_offset: extrusion branch end overflows")?;
        Ok(frame)
    }
    pub(crate) fn origin(&self) -> u64 {
        self.origin
    }
    pub(crate) fn scalar(&self) -> ShiftedBinary64 {
        self.scalar
    }
    pub(crate) fn terminal(&self) -> FeatureReferenceToken {
        self.terminal
    }
    pub(crate) fn terminal_offset(&self) -> u64 {
        self.origin + self.terminal_position()
    }
    pub(crate) fn atom_members(&self) -> &BranchItems<(WrappedCompactIndex, B)> {
        &self.atoms
    }
    pub(crate) fn first_members(&self) -> &BranchItems<(CompactIndexAtom, B)> {
        &self.first
    }
    pub(crate) fn second_members(&self) -> &BranchItems<(CompactIndexAtom, B)> {
        &self.second
    }

    fn first_position(&self) -> u64 {
        15 + 4 * self.atoms.len() as u64
    }
    fn second_position(&self) -> u64 {
        self.first_position()
            + self
                .first
                .as_slice()
                .iter()
                .map(|(token, _)| token.raw().len() as u64)
                .sum::<u64>()
            + 2
    }
    fn terminal_position(&self) -> u64 {
        self.second_position()
            + self
                .second
                .as_slice()
                .iter()
                .map(|(token, _)| token.raw().len() as u64)
                .sum::<u64>()
            + 2
    }

    pub(crate) fn atoms(&self) -> impl Iterator<Item = (WrappedCompactIndex, &B, u64)> {
        self.atoms
            .as_slice()
            .iter()
            .enumerate()
            .map(|(slot, (token, binding))| (*token, binding, self.origin + 13 + 4 * slot as u64))
    }
    pub(crate) fn first_indices(&self) -> impl Iterator<Item = (CompactIndexAtom, &B, u64)> {
        compact_positions(&self.first, self.origin + self.first_position())
    }
    pub(crate) fn second_indices(&self) -> impl Iterator<Item = (CompactIndexAtom, &B, u64)> {
        compact_positions(&self.second, self.origin + self.second_position())
    }
    pub(crate) fn relocate(self, base: u64) -> Option<Self> {
        Self::new(
            base.checked_add(self.origin)?,
            self.scalar,
            self.atoms,
            self.first,
            self.second,
            self.terminal,
        )
        .ok()
    }
    pub(crate) fn map_bindings<C>(self, mut map: impl FnMut(u32, B) -> C) -> Extrude32Frame<C> {
        Extrude32Frame {
            origin: self.origin,
            scalar: self.scalar,
            atoms: self
                .atoms
                .map_indexed(|_, (token, binding)| (token, map(token.value(), binding))),
            first: self
                .first
                .map_indexed(|_, (token, binding)| (token, map(token.value(), binding))),
            second: self
                .second
                .map_indexed(|_, (token, binding)| (token, map(token.value(), binding))),
            terminal: self.terminal,
        }
    }
}

fn compact_positions<B>(
    members: &BranchItems<(CompactIndexAtom, B)>,
    mut at: u64,
) -> impl Iterator<Item = (CompactIndexAtom, &B, u64)> {
    members.as_slice().iter().map(move |(token, binding)| {
        let offset = at;
        at += token.raw().len() as u64;
        (*token, binding, offset)
    })
}

pub(crate) fn extrude_payload_32_branch(
    record: OperationBodyInput<'_>,
) -> Option<Extrude32Frame<()>> {
    if record.name() != "EXTRUDE" {
        return None;
    }
    let reference = super::operation_body_reference(record)?;
    let end = reference.offset - record.offset() + reference.object_index.raw().len();
    if record.bytes().get(end..end + 4) != Some(&[0xff, 0x32, 0x00, 0x00]) {
        return None;
    }
    let scalar = ShiftedBinary64::read(record.bytes().get(end + 4..end + 12)?)?;
    let mut at = end + 12;
    let atoms = counted_lane(record.bytes(), &mut at, |bytes| {
        Some((WrappedCompactIndex::read(View::u32_be_at(bytes, 0)?)?, 4))
    })?;
    let mut compact = |bytes: &[u8]| {
        let token = CompactIndexAtom::read(bytes)?;
        Some((token, token.raw().len()))
    };
    let first = counted_lane(record.bytes(), &mut at, &mut compact)?;
    let second = counted_lane(record.bytes(), &mut at, compact)?;
    if record.bytes().get(at..at + 2) != Some(&[0x00, 0x01]) {
        return None;
    }
    let terminal = FeatureReferenceToken::read(record.bytes().get(at + 2..)?)?;
    let next = at + 2 + terminal.raw().len();
    if terminal.value() != reference.object_index.value()
        || record.bytes().get(next..next + 2) != Some(&[0x00, 0x00])
    {
        return None;
    }
    Extrude32Frame::new(
        (record.offset() + end + 1) as u64,
        scalar,
        atoms,
        first,
        second,
        terminal,
    )
    .ok()
}

fn counted_lane<T>(
    bytes: &[u8],
    at: &mut usize,
    mut read: impl FnMut(&[u8]) -> Option<(T, usize)>,
) -> Option<BranchItems<(T, ())>> {
    if bytes.get(*at) != Some(&0x01) {
        return None;
    }
    let count = *bytes.get(*at + 1)?;
    if count < 2 {
        return None;
    }
    *at += 2;
    let mut values = Vec::with_capacity(usize::from(count - 1));
    for _ in 1..count {
        let (token, width) = read(bytes.get(*at..)?)?;
        *at += width;
        values.push((token, ()));
    }
    BranchItems::new(values).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn frame_derives_mixed_width_lane_positions_and_the_complete_end() {
        let frame = Extrude32Frame::new(
            100,
            ShiftedBinary64::read(&[0x2f, 0xf0, 0, 0, 0, 0, 0, 0]).unwrap(),
            BranchItems::new(vec![(
                WrappedCompactIndex::from_wire(0, 0x3d80_0000).unwrap(),
                (),
            )])
            .unwrap(),
            BranchItems::new(vec![
                (CompactIndexAtom::read(&[0]).unwrap(), ()),
                (CompactIndexAtom::read(&[0x90, 0]).unwrap(), ()),
            ])
            .unwrap(),
            BranchItems::new(vec![(CompactIndexAtom::read(&[0x80, 0]).unwrap(), ())]).unwrap(),
            FeatureReferenceToken::from_wire(0, &[0x90, 0, 0]).unwrap(),
        )
        .unwrap();
        assert_eq!(
            frame
                .atoms()
                .map(|(_, (), offset)| offset)
                .collect::<Vec<_>>(),
            [113]
        );
        assert_eq!(
            frame
                .first_indices()
                .map(|(_, (), offset)| offset)
                .collect::<Vec<_>>(),
            [119, 120]
        );
        assert_eq!(
            frame
                .second_indices()
                .map(|(_, (), offset)| offset)
                .collect::<Vec<_>>(),
            [124]
        );
        assert_eq!(frame.terminal_offset(), 128);
        assert!(frame.clone().relocate(u64::MAX - 133).is_some());
        assert!(frame.clone().relocate(u64::MAX - 132).is_none());
        let mapped = frame
            .relocate(1000)
            .unwrap()
            .map_bindings(|index, ()| (index != 4096).then_some(index));
        assert_eq!(mapped.terminal_offset(), 1128);
        assert_eq!(mapped.first_members().declared_count(), 3);
        assert_eq!(mapped.first_members().as_slice()[1].1, None);
        assert_eq!(mapped.atoms().next().unwrap().1, &Some(0));
    }
}
