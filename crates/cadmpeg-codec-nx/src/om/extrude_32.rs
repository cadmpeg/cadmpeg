// SPDX-License-Identifier: Apache-2.0
//! Structured extrusion branch with three byte-counted reference lanes.

use super::branch_items::BranchItems;
use super::compact::{CompactIndexAtom, WrappedCompactIndex};
use super::operation_record::OperationBodyInput;
use super::reference_index::FeatureReferenceToken;
use super::scalar::ShiftedBinary64;
use cadmpeg_core::decode::{u64_from_index, DecodeContext, View};
use cadmpeg_core::CodecError;

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

    pub(crate) fn atoms(&self) -> impl Iterator<Item = (WrappedCompactIndex, &B, u64)> + Clone {
        self.atoms
            .as_slice()
            .iter()
            .enumerate()
            .map(|(slot, (token, binding))| (*token, binding, self.origin + 13 + 4 * slot as u64))
    }
    pub(crate) fn first_indices(
        &self,
    ) -> impl Iterator<Item = (CompactIndexAtom, &B, u64)> + Clone {
        compact_positions(&self.first, self.origin + self.first_position())
    }
    pub(crate) fn second_indices(
        &self,
    ) -> impl Iterator<Item = (CompactIndexAtom, &B, u64)> + Clone {
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
    pub(crate) fn map_bindings<C>(
        self,
        ctx: &DecodeContext<'_>,
        mut map: impl FnMut(u32, B) -> Result<C, CodecError>,
    ) -> Result<Extrude32Frame<C>, CodecError> {
        Ok(Extrude32Frame {
            origin: self.origin,
            scalar: self.scalar,
            atoms: self
                .atoms
                .try_map_indexed_charged(ctx, |_, (token, binding)| {
                    Ok((token, map(token.value(), binding)?))
                })?,
            first: self
                .first
                .try_map_indexed_charged(ctx, |_, (token, binding)| {
                    Ok((token, map(token.value(), binding)?))
                })?,
            second: self
                .second
                .try_map_indexed_charged(ctx, |_, (token, binding)| {
                    Ok((token, map(token.value(), binding)?))
                })?,
            terminal: self.terminal,
        })
    }
}

fn compact_positions<B>(
    members: &BranchItems<(CompactIndexAtom, B)>,
    mut at: u64,
) -> impl Iterator<Item = (CompactIndexAtom, &B, u64)> + Clone {
    members.as_slice().iter().map(move |(token, binding)| {
        let offset = at;
        at += token.raw().len() as u64;
        (*token, binding, offset)
    })
}

pub(crate) fn extrude_payload_32_branch(
    ctx: &DecodeContext<'_>,
    record: OperationBodyInput<'_>,
) -> Result<Option<Extrude32Frame<()>>, CodecError> {
    ctx.charge_work(
        u64_from_index(record.bytes().len()),
        "scan NX extrude 32 branch",
    )?;
    if record.name() != "EXTRUDE" {
        return Ok(None);
    }
    let Some(reference) = super::operation_body_reference(record) else {
        return Ok(None);
    };
    let end = reference.offset - record.offset() + reference.object_index.raw().len();
    if record.bytes().get(end..end + 4) != Some(&[0xff, 0x32, 0x00, 0x00]) {
        return Ok(None);
    }
    let Some(scalar) = record
        .bytes()
        .get(end + 4..end + 12)
        .and_then(ShiftedBinary64::read)
    else {
        return Ok(None);
    };
    let mut at = end + 12;
    let Some(atoms) = counted_lane(ctx, record.bytes(), &mut at, |bytes| {
        Some((WrappedCompactIndex::read(View::u32_be_at(bytes, 0)?)?, 4))
    })?
    else {
        return Ok(None);
    };
    let mut compact = |bytes: &[u8]| {
        let token = CompactIndexAtom::read(bytes)?;
        Some((token, token.raw().len()))
    };
    let Some(first) = counted_lane(ctx, record.bytes(), &mut at, &mut compact)? else {
        return Ok(None);
    };
    let Some(second) = counted_lane(ctx, record.bytes(), &mut at, compact)? else {
        return Ok(None);
    };
    if record.bytes().get(at..at + 2) != Some(&[0x00, 0x01]) {
        return Ok(None);
    }
    let Some(terminal) = record
        .bytes()
        .get(at + 2..)
        .and_then(FeatureReferenceToken::read)
    else {
        return Ok(None);
    };
    let next = at + 2 + terminal.raw().len();
    if terminal.value() != reference.object_index.value()
        || record.bytes().get(next..next + 2) != Some(&[0x00, 0x00])
    {
        return Ok(None);
    }
    Ok(Extrude32Frame::new(
        (record.offset() + end + 1) as u64,
        scalar,
        atoms,
        first,
        second,
        terminal,
    )
    .ok())
}

fn counted_lane<T>(
    ctx: &DecodeContext<'_>,
    bytes: &[u8],
    at: &mut usize,
    mut read: impl FnMut(&[u8]) -> Option<(T, usize)>,
) -> Result<Option<BranchItems<(T, ())>>, CodecError> {
    if bytes.get(*at) != Some(&0x01) {
        return Ok(None);
    }
    let Some(&count) = bytes.get(*at + 1) else {
        return Ok(None);
    };
    if count < 2 {
        return Ok(None);
    }
    *at += 2;
    let len = usize::from(count - 1);
    let count_u64 = u64_from_index(len);
    let operation = "NX extrude 32 counted lane";
    ctx.charge_collection_items(count_u64, operation)?;
    let retained_bytes = count_u64
        .checked_mul(u64_from_index(std::mem::size_of::<(T, ())>()))
        .ok_or_else(|| ctx.refuse_codec_limit(operation, 0, count_u64))?;
    ctx.charge_retained(retained_bytes, operation)?;
    let mut values = Vec::new();
    values
        .try_reserve_exact(len)
        .map_err(|_| ctx.refuse_codec_limit(operation, 0, count_u64))?;
    for _ in 1..count {
        let Some((token, width)) = bytes.get(*at..).and_then(&mut read) else {
            return Ok(None);
        };
        *at += width;
        values.push((token, ()));
    }
    Ok(BranchItems::new(values).ok())
}

#[cfg(test)]
mod tests {
    use super::super::branch_items::BranchItems;
    use super::super::compact::CompactIndexAtom;
    use super::super::compact::WrappedCompactIndex;
    use super::super::reference_index::FeatureReferenceToken;
    use super::super::scalar::ShiftedBinary64;
    use super::Extrude32Frame;
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
    use cadmpeg_core::CodecError;

    fn refusal(configure: impl FnOnce(&mut DecodePolicy)) -> CodecError {
        let bytes = b"\x01\x02\x10\x73\xff\x32\x00\x00\x30\x77\x7e\x14\x7a\xe1\x47\xb3\x01\x03\x3d\x82\x56\x00\x3d\x82\x57\x00\x01\x04\x80\x2b\x80\x2d\x80\x2c\x01\x03\x80\x2e\x80\x77\x00\x01\x73\x00\x00";
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::default();
        configure(&mut policy);
        let (ctx, _) = DecodeContext::from_root_bytes(bytes, &arena, &policy).unwrap();
        let record = super::OperationBodyInput::new(bytes, 100, 0, "EXTRUDE").unwrap();
        super::extrude_payload_32_branch(&ctx, record).unwrap_err()
    }

    #[test]
    fn extrude_32_branch_refuses_collection_limit() {
        let error = refusal(|policy| policy.limits.max_collection_items = 0);
        assert!(
            matches!(error, CodecError::ResourceLimit(limit) if limit.dimension == ResourceDimension::CollectionItems)
        );
    }

    #[test]
    fn extrude_32_branch_refuses_retained_limit() {
        let error = refusal(|policy| policy.limits.max_retained_bytes = 0);
        assert!(
            matches!(error, CodecError::ResourceLimit(limit) if limit.dimension == ResourceDimension::RetainedBytes)
        );
    }

    #[test]
    fn extrude_32_branch_refuses_work_limit() {
        let error = refusal(|policy| policy.limits.max_work_units = 0);
        assert!(
            matches!(error, CodecError::ResourceLimit(limit) if limit.dimension == ResourceDimension::WorkUnits)
        );
    }

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
        let mapped = crate::test_support::with_decode_context(|ctx| {
            frame
                .relocate(1000)
                .unwrap()
                .map_bindings(ctx, |index, ()| Ok((index != 4096).then_some(index)))
        })
        .unwrap();
        assert_eq!(mapped.terminal_offset(), 1128);
        assert_eq!(mapped.first_members().declared_count(), 3);
        assert_eq!(mapped.first_members().as_slice()[1].1, None);
        assert_eq!(mapped.atoms().next().unwrap().1, &Some(0));
    }
}
