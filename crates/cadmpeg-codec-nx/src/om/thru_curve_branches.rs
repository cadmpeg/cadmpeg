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

fn reserve_thru_item<T>(
    ctx: &DecodeContext<'_>,
    values: &mut Vec<T>,
    operation: &'static str,
) -> Result<(), CodecError> {
    ctx.charge_collection_items(1, operation)?;
    ctx.charge_retained(u64_from_index(std::mem::size_of::<T>()), operation)?;
    values
        .try_reserve(1)
        .map_err(|_| ctx.refuse_codec_limit(operation, 0, 1))
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ThruCurveBranch<B> {
    pub(crate) mode: NonZeroU8,
    pub(crate) members: ThruCurveBranchItems<(PayloadIndexToken, B)>,
    pub(crate) terminal: (PayloadIndexToken, B),
    pub(crate) suffix: ThruCurveBranchSuffix,
}

impl<B> ThruCurveBranch<B> {
    pub(crate) fn member_positions(&self) -> impl Iterator<Item = u64> + '_ {
        let mut at = 3;
        self.members.as_slice().iter().map(move |(token, _)| {
            let offset = at;
            at += token.raw().len() as u64;
            offset
        })
    }

    pub(crate) fn terminal_position(&self) -> u64 {
        3 + self
            .members
            .as_slice()
            .iter()
            .map(|(token, _)| token.raw().len() as u64)
            .sum::<u64>()
            + 2
            + self.members.state_lane_len() as u64
            + 3
    }

    fn byte_len(&self) -> u64 {
        self.terminal_position() + self.terminal.0.raw().len() as u64 + 3
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
        let byte_len = 1
            + branches
                .as_slice()
                .iter()
                .map(ThruCurveBranch::byte_len)
                .sum::<u64>()
            + terminator.bytes().len() as u64;
        offset
            .checked_add(byte_len)
            .ok_or("source_offset: THRU_CURVE group frame overflows")?;
        Ok(Self {
            offset,
            branches,
            terminator,
        })
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
        mut target: impl FnMut(PayloadIndexToken) -> B,
    ) -> Result<Option<ThruCurveGroup<B>>, CodecError> {
        let Some(offset) = self.offset.checked_add(file_base) else {
            return Ok(None);
        };
        let mut branches = Vec::new();
        for branch in self.branches.into_vec() {
            let members = branch
                .members
                .map_indexed_charged(ctx, |_, (token, ())| (token, target(token)))?;
            let terminal = (branch.terminal.0, target(branch.terminal.0));
            reserve_thru_item(ctx, &mut branches, "NX resolved thru-curve branches")?;
            branches.push(ThruCurveBranch {
                mode: branch.mode,
                members,
                terminal,
                suffix: branch.suffix,
            });
        }
        let Ok(branches) = BranchItems::new(branches) else {
            return Ok(None);
        };
        Ok(ThruCurveGroup::new(offset, branches, self.terminator).ok())
    }
}

fn thru_curve_payload_branch(
    ctx: &DecodeContext<'_>,
    record: OperationPayload<'_>,
    at: usize,
) -> Result<Option<(ThruCurveBranch<()>, usize)>, CodecError> {
    let mut failure = None;
    let parsed = (|| {
        let mode = NonZeroU8::new(*record.payload().get(at)?)?;
        (*record.payload().get(at + 1)? == 0x01).then_some(())?;
        let declared_count @ 2.. = *record.payload().get(at + 2)? else {
            return None;
        };
        let mut cursor = at + 3;
        let mut members = Vec::new();
        for _ in 1..declared_count {
            let token = PayloadIndexToken::read(record.payload().get(cursor..)?)?;
            cursor += token.raw().len();
            if let Err(error) = reserve_thru_item(ctx, &mut members, "NX thru-curve branch members")
            {
                failure = Some(error);
                return None;
            }
            members.push((token, ()));
        }
        (record.payload().get(cursor..cursor + 2) == Some(&[0x01, declared_count])).then_some(())?;
        cursor += 2;

        let standard_len = usize::from(declared_count) + 3;
        let lane = record.payload().get(cursor..cursor + standard_len)?;
        let lane = if lane.iter().all(|&byte| byte == 0) {
            lane
        } else {
            record.payload().get(cursor..cursor + 18)?
        };
        let lane_len = lane.len();
        let members = ThruCurveBranchItems::from_parts(members, lane).ok()?;
        cursor += lane_len;
        (record.payload().get(cursor..cursor + 3) == Some(&[0xff, 0x01, 0x02])).then_some(())?;
        cursor += 3;
        let token = PayloadIndexToken::read(record.payload().get(cursor..)?)?;
        cursor += token.raw().len();
        let terminal = (token, ());
        (*record.payload().get(cursor)? == 0x00).then_some(())?;
        cursor += 1;
        let suffix: [u8; 2] = record.payload().get(cursor..cursor + 2)?.try_into().ok()?;
        let suffix = ThruCurveBranchSuffix::try_from(suffix).ok()?;
        cursor += 2;

        Some((
            ThruCurveBranch {
                mode,
                members,
                terminal,
                suffix,
            },
            cursor,
        ))
    })();
    if let Some(error) = failure {
        return Err(error);
    }
    Ok(parsed)
}

/// Decode the exact counted branch group after a bounded `THRU_CURVE`
/// reference envelope.
pub(crate) fn thru_curve_payload_branch_group(
    ctx: &DecodeContext<'_>,
    record: OperationPayload<'_>,
) -> Result<Option<ThruCurveGroup<()>>, CodecError> {
    let Some(envelope) = thru_curve_payload_references(record) else {
        return Ok(None);
    };
    ctx.charge_work(
        u64_from_index(record.payload().len()),
        "scan NX thru-curve branches",
    )?;
    let mut at = envelope.byte_len();
    let group_offset = at;
    let Some(declared_count @ 2..) = record.payload().get(at).copied() else {
        return Ok(None);
    };
    at += 1;
    let mut branches = Vec::new();
    for _ in 1..declared_count {
        let Some((branch, next)) = thru_curve_payload_branch(ctx, record, at)? else {
            return Ok(None);
        };
        reserve_thru_item(ctx, &mut branches, "NX thru-curve branches")?;
        branches.push(branch);
        at = next;
    }
    let Some(terminator) = ThruCurveGroupTerminator::ALL
        .into_iter()
        .find(|terminator| {
            record.payload().get(at..at + terminator.bytes().len()) == Some(terminator.bytes())
        })
    else {
        return Ok(None);
    };
    let Some(offset) = record.payload_offset().checked_add(group_offset) else {
        return Ok(None);
    };
    let Ok(branches) = BranchItems::new(branches) else {
        return Ok(None);
    };
    Ok(ThruCurveGroup::new(u64_from_index(offset), branches, terminator).ok())
}
