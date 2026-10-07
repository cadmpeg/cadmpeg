// SPDX-License-Identifier: Apache-2.0
//! Counted `THRU_CURVE` frames with positions derived from token widths.

use super::branch_items::BranchItems;
use super::operation_record::OperationPayload;
use super::reference_index::PayloadIndexToken;
use super::surface_envelope::thru_curve_payload_references;
use super::thru_curve_endings::{ThruCurveBranchSuffix, ThruCurveGroupTerminator};
use super::thru_curve_state::ThruCurveBranchItems;
use cadmpeg_core::decode::{u64_from_index, DecodeContext};
use cadmpeg_core::CodecError;
use std::num::NonZeroU8;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ThruCurveBranch<B> {
    pub(crate) mode: NonZeroU8,
    terminal_position: u64,
    pub(crate) members: ThruCurveBranchItems<(PayloadIndexToken, B)>,
    pub(crate) terminal: (PayloadIndexToken, B),
    pub(crate) suffix: ThruCurveBranchSuffix,
}

impl<B> ThruCurveBranch<B> {
    pub(crate) fn member_positions(&self) -> impl Iterator<Item = u64> + '_ {
        let mut at = 3;
        self.members.as_slice().iter().map(move |(token, _)| {
            let offset = at;
            at += cadmpeg_core::decode::u64_from_index(token.raw().len());
            offset
        })
    }

    pub(crate) fn new(
        mode: NonZeroU8,
        members: ThruCurveBranchItems<(PayloadIndexToken, B)>,
        terminal: (PayloadIndexToken, B),
        suffix: ThruCurveBranchSuffix,
    ) -> Result<Self, &'static str> {
        let terminal_position = match Self::extent(&members, |members| {
            Ok::<_, std::convert::Infallible>(members.iter())
        }) {
            Ok(position) => position?,
            Err(error) => match error {},
        };
        Ok(Self {
            mode,
            terminal_position,
            members,
            terminal,
            suffix,
        })
    }

    fn from_wire(
        ctx: &DecodeContext<'_>,
        mode: NonZeroU8,
        members: ThruCurveBranchItems<(PayloadIndexToken, B)>,
        terminal: (PayloadIndexToken, B),
        suffix: ThruCurveBranchSuffix,
    ) -> Result<Result<Self, &'static str>, CodecError> {
        Ok(Self::extent(&members, |members| {
            ctx.admit_iter(members, "NX thru-curve branch token widths")
        })?
        .map(|terminal_position| Self {
            mode,
            terminal_position,
            members,
            terminal,
            suffix,
        }))
    }

    fn extent<'a, E, I: Iterator<Item = &'a (PayloadIndexToken, B)>>(
        members: &'a ThruCurveBranchItems<(PayloadIndexToken, B)>,
        admit: impl FnOnce(&'a [(PayloadIndexToken, B)]) -> Result<I, E>,
    ) -> Result<Result<u64, &'static str>, E>
    where
        B: 'a,
    {
        let width = match members {
            ThruCurveBranchItems::Standard(members) => admit(members.as_slice())?
                .map(|(token, _)| u64_from_index(token.raw().len())).sum::<u64>(),
            ThruCurveBranchItems::Extended { members, .. } => members.iter()
                .map(|(token, _)| u64_from_index(token.raw().len())).sum::<u64>(),
        };
        let position = 3_u64.checked_add(width)
            .and_then(|at| at.checked_add(2))
            .and_then(|at| at.checked_add(u64_from_index(members.state_lane_len())))
            .and_then(|at| at.checked_add(3));
        Ok(position.ok_or("THRU_CURVE branch frame overflows"))
    }

    pub(crate) fn terminal_position(&self) -> u64 {
        self.terminal_position
    }

    fn byte_len(&self) -> u64 {
        self.terminal_position()
            + cadmpeg_core::decode::u64_from_index(self.terminal.0.raw().len())
            + 3
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ThruCurveGroup<B> {
    offset: u64,
    branches: BranchItems<ThruCurveBranch<B>>,
    terminator: ThruCurveGroupTerminator,
}

impl<B> ThruCurveGroup<B> {
    pub(crate) fn new(
        offset: u64,
        branches: BranchItems<ThruCurveBranch<B>>,
        terminator: ThruCurveGroupTerminator,
    ) -> Result<Self, &'static str> {
        match Self::validate(offset, branches.as_slice(), terminator, |branches| {
            Ok::<_, std::convert::Infallible>(branches.iter())
        }) {
            Ok(valid) => valid?,
            Err(error) => match error {},
        }
        Ok(Self {
            offset,
            branches,
            terminator,
        })
    }

    fn from_wire(
        ctx: &DecodeContext<'_>,
        offset: u64,
        branches: BranchItems<ThruCurveBranch<B>>,
        terminator: ThruCurveGroupTerminator,
    ) -> Result<Result<Self, &'static str>, CodecError> {
        Ok(
            Self::validate(offset, branches.as_slice(), terminator, |branches| {
                ctx.admit_iter(branches, "NX thru-curve group branch widths")
            })?
            .map(|()| Self {
                offset,
                branches,
                terminator,
            }),
        )
    }

    fn validate<'a, E, I: Iterator<Item = &'a ThruCurveBranch<B>>>(
        offset: u64,
        branches: &'a [ThruCurveBranch<B>],
        terminator: ThruCurveGroupTerminator,
        admit: impl FnOnce(&'a [ThruCurveBranch<B>]) -> Result<I, E>,
    ) -> Result<Result<(), &'static str>, E>
    where
        B: 'a,
    {
        let end = admit(branches)?
            .fold(Some(offset), |end, branch| end.and_then(|end| end.checked_add(branch.byte_len())))
            .and_then(|end| end.checked_add(1))
            .and_then(|end| end.checked_add(u64_from_index(terminator.bytes().len())));
        Ok(end
            .map(|_| ())
            .ok_or("source_offset: THRU_CURVE group frame overflows"))
    }

    pub(crate) fn offset(&self) -> u64 {
        self.offset
    }
    pub(crate) fn branches(&self) -> &BranchItems<ThruCurveBranch<B>> {
        &self.branches
    }
    pub(crate) fn terminator(&self) -> ThruCurveGroupTerminator {
        self.terminator
    }
    pub(crate) fn branch_offsets(&self) -> impl Iterator<Item = u64> + '_ {
        let mut at = self.offset + 1;
        self.branches.as_slice().iter().map(move |branch| {
            let offset = at;
            at += branch.byte_len();
            offset
        })
    }
}

impl ThruCurveGroup<()> {
    pub(crate) fn resolve<B>(
        self,
        ctx: &DecodeContext<'_>,
        file_base: u64,
        mut target: impl FnMut(PayloadIndexToken) -> Result<B, CodecError>,
    ) -> Result<Option<ThruCurveGroup<B>>, CodecError> {
        let Some(offset) = self.offset.checked_add(file_base) else {
            return Ok(None);
        };
        let mut branches = Vec::new();
        let mut input = self.branches.into_vec().into_iter();
        while let Some(branch) = ctx.next_charged(&mut input, "NX resolved thru-curve branch visits")? {
            let members = branch
                .members
                .try_map_indexed_charged(ctx, |_, (token, ())| Ok((token, target(token)?)))?;
            let terminal = (branch.terminal.0, target(branch.terminal.0)?);
            ctx.reserve_vec(&mut branches, 1, "NX resolved thru-curve branches")?;
            branches.push(ThruCurveBranch {
                mode: branch.mode,
                terminal_position: branch.terminal_position,
                members,
                terminal,
                suffix: branch.suffix,
            });
        }
        let Ok(branches) = BranchItems::new(branches) else {
            return Ok(None);
        };
        Ok(ThruCurveGroup::from_wire(ctx, offset, branches, self.terminator)?.ok())
    }
}

fn thru_curve_payload_branch(
    ctx: &DecodeContext<'_>,
    record: OperationPayload<'_>,
    at: usize,
) -> Result<Option<(ThruCurveBranch<()>, usize)>, CodecError> {
    let Some(mode) = record.payload().get(at).copied().and_then(NonZeroU8::new) else { return Ok(None); };
    if record.payload().get(at + 1) != Some(&1) { return Ok(None); }
    let Some(&declared_count @ 2..) = record.payload().get(at + 2) else { return Ok(None); };
    let mut cursor = at + 3;
    let first_at = cursor;
    let mut rows = 1..declared_count;
    while ctx.next_charged(&mut rows, "NX thru curve payload branch row traversal")?.is_some() {
        let Some(token) = record.payload().get(cursor..).and_then(PayloadIndexToken::read) else { return Ok(None); };
        cursor += token.raw().len();
    }
    if record.payload().get(cursor..cursor + 2) != Some(&[1, declared_count]) { return Ok(None); }
    cursor += 2;
    let standard_len = usize::from(declared_count) + 3;
    let Some(standard_lane) = record.payload().get(cursor..cursor + standard_len) else { return Ok(None); };
    let lane = if standard_lane.get(..5) == Some(&[0; 5]) {
        standard_lane
    } else {
        let Some(lane) = record.payload().get(cursor..cursor + 18) else { return Ok(None); };
        lane
    };
    cursor += lane.len();
    if record.payload().get(cursor..cursor + 3) != Some(&[0xff, 1, 2]) { return Ok(None); }
    cursor += 3;
    let Some(token) = record.payload().get(cursor..).and_then(PayloadIndexToken::read) else { return Ok(None); };
    cursor += token.raw().len();
    let terminal = (token, ());
    if record.payload().get(cursor) != Some(&0) { return Ok(None); }
    cursor += 1;
    let Some(suffix) = record.payload().get(cursor..cursor + 2).and_then(|bytes| <[u8; 2]>::try_from(bytes).ok())
        .and_then(|bytes| ThruCurveBranchSuffix::try_from(bytes).ok()) else { return Ok(None); };
    cursor += 2;
    let mut members = ctx.collection_vec(usize::from(declared_count - 1), "NX thru-curve branch members")?;
    let mut member_at = first_at;
    for _ in ctx.admit_iter(&(1..declared_count), "NX thru-curve member materialization")? {
        let Some(token) = record.payload().get(member_at..).and_then(PayloadIndexToken::read) else { return Ok(None); };
        member_at += token.raw().len();
        members.push((token, ()));
    }
    let Ok(members) = ThruCurveBranchItems::from_wire(ctx, members, lane)? else { return Ok(None); };
    let Ok(branch) = ThruCurveBranch::from_wire(ctx, mode, members, terminal, suffix)? else { return Ok(None); };
    Ok(Some((branch, cursor)))
}

/// Decode the exact counted branch group after a bounded `THRU_CURVE`
/// reference envelope.
pub(crate) fn thru_curve_payload_branch_group(
    ctx: &DecodeContext<'_>,
    record: OperationPayload<'_>,
) -> Result<Option<ThruCurveGroup<()>>, CodecError> {
    let mut storage = ctx.reserve_scoped(0, "NX thru-curve candidate storage")?;
    let candidate = storage.with_storage(|| read_thru_curve_payload_branch_group(ctx, record))?;
    let Some(candidate) = candidate else { return Ok(None); };
    let mut branches = ctx.collection_vec(candidate.branches.len(), "NX retained thru-curve branches")?;
    for branch in ctx.admit_iter(candidate.branches.into_vec(), "NX retained thru-curve branch visits")? {
        let members = match branch.members {
            ThruCurveBranchItems::Standard(members) => {
                let mut retained = ctx.collection_vec(members.len(), "NX retained thru-curve members")?;
                for &member in ctx.admit_iter(members.as_slice(), "NX retained thru-curve member copies")? {
                    retained.push(member);
                }
                let members = retained;
                let Ok(members) = BranchItems::new(members) else { return Ok(None); };
                ThruCurveBranchItems::Standard(members)
            }
            ThruCurveBranchItems::Extended { members, values } => ThruCurveBranchItems::Extended { members, values },
        };
        branches.push(ThruCurveBranch { members, ..branch });
    }
    let Ok(branches) = BranchItems::new(branches) else { return Ok(None); };
    Ok(Some(ThruCurveGroup { offset: candidate.offset, branches, terminator: candidate.terminator }))
}

fn read_thru_curve_payload_branch_group(
    ctx: &DecodeContext<'_>,
    record: OperationPayload<'_>,
) -> Result<Option<ThruCurveGroup<()>>, CodecError> {
    let Some(envelope) = thru_curve_payload_references(record) else {
        return Ok(None);
    };
    let mut at = envelope.byte_len();
    let group_offset = at;
    let Some(declared_count @ 2..) = record.payload().get(at).copied() else {
        return Ok(None);
    };
    at += 1;
    let mut branches = Vec::new();
    let mut rows = 1..declared_count;
    while ctx.next_charged(&mut rows, "scan NX thru-curve branches")?.is_some() {
        let Some((branch, next)) = thru_curve_payload_branch(ctx, record, at)? else {
            return Ok(None);
        };
        ctx.reserve_vec(&mut branches, 1, "NX thru-curve branches")?;
        branches.push(branch);
        at = next;
    }
    let Some(terminator) = ThruCurveGroupTerminator::ALL.into_iter().find(|terminator| {
        record.payload().get(at..at + terminator.bytes().len()) == Some(terminator.bytes())
    }) else { return Ok(None); };
    let Some(offset) = record.payload_offset().checked_add(group_offset) else {
        return Ok(None);
    };
    let Ok(branches) = BranchItems::new(branches) else {
        return Ok(None);
    };
    Ok(ThruCurveGroup::from_wire(ctx, u64_from_index(offset), branches, terminator)?.ok())
}

#[cfg(test)]
mod tests {
    #[test]
    fn thru_curve_candidate_storage_is_scoped_until_the_group_is_complete() {
        let mut payload = vec![1, 0, 0, 1, 0, 0xf0, 0, 0xf1, 1, 0, 0xf0, 1];
        payload.extend([1, 8, 0, 0, 0, 0, 0, 0, 0, 0, 7]);
        for value in 0..6 { payload.extend([0xf0, value]); }
        payload.extend([4, 1, 0xa0, 0, 0, 0x13, 1]);
        payload.extend([2, 1, 1, 2, 0xf0, 1, 1, 2]);
        payload.extend([0; 5]);
        payload.extend([0xff, 1, 2, 0xf0, 2, 0, 0x81, 0x58]);
        payload.extend(super::ThruCurveGroupTerminator::Adjacent.bytes());
        let decode = |ctx: &cadmpeg_core::decode::DecodeContext<'_>| {
            super::thru_curve_payload_branch_group(ctx,
                super::OperationPayload::new(&payload, 100, "THRU_CURVE").unwrap())
        };
        let group = crate::test_support::with_decode_context(decode).unwrap().unwrap();
        assert_eq!(group.offset(), 142);
        assert_eq!(group.branches().len(), 1);
        assert_eq!(group.branches().as_slice()[0].members.as_slice()[0].0.value(), 1);
        assert_eq!(group.branches().as_slice()[0].terminal.0.value(), 2);
        crate::test_support::resource_refusal_at(&[], cadmpeg_core::decode::ResourceDimension::MaterializedBytes,
            "NX thru-curve branch members", decode);
        crate::test_support::resource_refusal_at(&[], cadmpeg_core::decode::ResourceDimension::RetainedBytes,
            "NX retained thru-curve members", decode);
        payload.pop();
        crate::test_support::with_decode_context_over(&[],
            |policy| policy.limits.max_retained_bytes = 0,
            |ctx| assert!(super::thru_curve_payload_branch_group(ctx,
                super::OperationPayload::new(&payload, 100, "THRU_CURVE").unwrap()).unwrap().is_none()));
    }
}
