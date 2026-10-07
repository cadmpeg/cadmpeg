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
        let position = admit(members.as_slice())?
            .try_fold(3_u64, |at, (token, _)| {
                at.checked_add(u64_from_index(token.raw().len()))
            })
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
            .try_fold(offset, |end, branch| end.checked_add(branch.byte_len()))
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
        for branch in self.branches.into_vec() {
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
    let mut failure = None;
    let parsed = (|| {
        let mode = NonZeroU8::new(*record.payload().get(at)?)?;
        (*record.payload().get(at + 1)? == 0x01).then_some(())?;
        let declared_count @ 2.. = *record.payload().get(at + 2)? else {
            return None;
        };
        let mut cursor = at + 3;
        let mut members = Vec::new();
        for _ in match ctx.admit_iter(
            &(1..declared_count),
            "NX thru curve payload branch row traversal",
        ) {
            Ok(rows) => rows,
            Err(error) => {
                failure = Some(error.into());
                return None;
            }
        } {
            let token = PayloadIndexToken::read(record.payload().get(cursor..)?)?;
            cursor += token.raw().len();
            if let Err(error) = ctx.reserve_vec(&mut members, 1, "NX thru-curve branch members") {
                failure = Some(error);
                return None;
            }
            members.push((token, ()));
        }
        (record.payload().get(cursor..cursor + 2) == Some(&[0x01, declared_count])).then_some(())?;
        cursor += 2;

        let standard_len = usize::from(declared_count) + 3;
        let lane = record.payload().get(cursor..cursor + standard_len)?;
        let mut lane_bytes = match ctx.admit_iter(lane, "NX thru-curve state lane selection") {
            Ok(bytes) => bytes,
            Err(error) => {
                failure = Some(error.into());
                return None;
            }
        };
        let lane = if lane_bytes.all(|&byte| byte == 0) {
            lane
        } else {
            record.payload().get(cursor..cursor + 18)?
        };
        let lane_len = lane.len();
        let members = match ThruCurveBranchItems::from_wire(ctx, members, lane) {
            Ok(members) => members.ok()?,
            Err(error) => {
                failure = Some(error);
                return None;
            }
        };
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

        let branch = match ThruCurveBranch::from_wire(ctx, mode, members, terminal, suffix) {
            Ok(branch) => branch.ok()?,
            Err(error) => {
                failure = Some(error);
                return None;
            }
        };
        Some((branch, cursor))
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
    let mut at = envelope.byte_len();
    let group_offset = at;
    let Some(declared_count @ 2..) = record.payload().get(at).copied() else {
        return Ok(None);
    };
    at += 1;
    let mut branches = Vec::new();
    for _ in ctx.admit_iter(&(1..declared_count), "scan NX thru-curve branches")? {
        let Some((branch, next)) = thru_curve_payload_branch(ctx, record, at)? else {
            return Ok(None);
        };
        ctx.reserve_vec(&mut branches, 1, "NX thru-curve branches")?;
        branches.push(branch);
        at = next;
    }
    let Some(terminator) = ctx
        .find_by(
            &ThruCurveGroupTerminator::ALL,
            |terminator| {
                Ok(ctx.equal(
                    &record.payload().get(at..at + terminator.bytes().len()),
                    &Some(terminator.bytes()),
                    "NX thru curve payload branch group equality",
                )?)
            },
            "NX thru curve group terminator lookup",
        )?
        .copied()
    else {
        return Ok(None);
    };
    let Some(offset) = record.payload_offset().checked_add(group_offset) else {
        return Ok(None);
    };
    let Ok(branches) = BranchItems::new(branches) else {
        return Ok(None);
    };
    Ok(ThruCurveGroup::from_wire(ctx, u64_from_index(offset), branches, terminator)?.ok())
}
