// SPDX-License-Identifier: Apache-2.0
//! Surface construction branch framing domains.

use super::branch_items::BranchItems;
use super::discriminators::SurfaceBranchMode;
use super::operation_record::OperationPayload;
use super::reference_index::PayloadIndexToken;
use cadmpeg_core::decode::{u64_from_index, DecodeContext};
use cadmpeg_core::CodecError;
use serde::{Deserialize, Deserializer, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "u8", into = "u8")]
#[repr(u8)]
pub(crate) enum SurfaceFamily {
    Form14 = 0x14,
    Form50 = 0x50,
}

impl From<SurfaceFamily> for u8 {
    fn from(value: SurfaceFamily) -> Self {
        match value {
            SurfaceFamily::Form14 => 0x14,
            SurfaceFamily::Form50 => 0x50,
        }
    }
}

impl TryFrom<u8> for SurfaceFamily {
    type Error = &'static str;

    fn try_from(value: u8) -> Result<Self, Self::Error> {
        match value {
            0x14 => Ok(Self::Form14),
            0x50 => Ok(Self::Form50),
            _ => Err("surface family must be 0x14 or 0x50"),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(transparent)]
pub(crate) struct SurfaceSuffix(Vec<u8>);

impl SurfaceSuffix {
    pub(crate) fn new(bytes: Vec<u8>) -> Result<Self, &'static str> {
        if !(1..=5).contains(&bytes.len()) {
            return Err("surface suffix must contain 1 through 5 bytes");
        }
        Ok(Self(bytes))
    }

    #[cfg(test)]
    pub(crate) fn into_vec(self) -> Vec<u8> {
        self.0
    }
}

impl<'de> Deserialize<'de> for SurfaceSuffix {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        Self::new(Vec::deserialize(deserializer)?).map_err(serde::de::Error::custom)
    }
}

/// One branch whose reference positions follow from its frame and token widths.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct SurfaceBranch<B> {
    offset: u64,
    terminal_relative: u64,
    mode: SurfaceBranchMode,
    witnessed: bool,
    members: BranchItems<(PayloadIndexToken, B)>,
    terminal: (PayloadIndexToken, B),
    suffix: SurfaceSuffix,
}

impl<B> SurfaceBranch<B> {
    pub(crate) fn new(
        offset: u64,
        mode: SurfaceBranchMode,
        witnessed: bool,
        members: BranchItems<(PayloadIndexToken, B)>,
        terminal: (PayloadIndexToken, B),
        suffix: SurfaceSuffix,
    ) -> Result<Self, &'static str> {
        let terminal_relative = match Self::extent(offset, witnessed, &members, &terminal, &suffix, |members| {
            Ok::<_, std::convert::Infallible>(members.iter())
        }) {
            Ok(offset) => offset?,
            Err(error) => match error {},
        };
        Ok(Self { offset, terminal_relative, mode, witnessed, members, terminal, suffix })
    }

    fn from_wire(ctx: &DecodeContext<'_>, offset: u64, mode: SurfaceBranchMode, witnessed: bool, members: BranchItems<(PayloadIndexToken, B)>, terminal: (PayloadIndexToken, B), suffix: SurfaceSuffix) -> Result<Result<Self, &'static str>, CodecError> {
        Ok(Self::extent(offset, witnessed, &members, &terminal, &suffix, |members| {
            ctx.admit_iter(members, "NX surface branch token widths")
        })?.map(|terminal_relative| Self { offset, terminal_relative, mode, witnessed, members, terminal, suffix }))
    }

    fn extent<'a, E, I: Iterator<Item = &'a (PayloadIndexToken, B)>>(
        offset: u64, witnessed: bool, members: &'a BranchItems<(PayloadIndexToken, B)>, terminal: &(PayloadIndexToken, B), suffix: &SurfaceSuffix,
        admit: impl FnOnce(&'a [(PayloadIndexToken, B)]) -> Result<I, E>,
    ) -> Result<Result<u64, &'static str>, E> where B: 'a {
        let state_bytes = if witnessed { u64::from(members.declared_count()) + 5 } else { 5 };
        let relative = admit(members.as_slice())?.try_fold(3_u64, |length, (token, _)| {
            length.checked_add(u64_from_index(token.raw().len()))
        }).and_then(|length| length.checked_add(state_bytes))
            .and_then(|length| length.checked_add(3));
        let valid = relative.and_then(|relative| {
            offset.checked_add(relative)?
                .checked_add(u64_from_index(terminal.0.raw().len()))?
                .checked_add(1)?.checked_add(u64_from_index(suffix.0.len()))?;
            Some(relative)
        });
        Ok(valid.ok_or("source_offset: surface branch frame overflows"))
    }

    pub(crate) fn offset(&self) -> u64 {
        self.offset
    }
    pub(crate) fn mode(&self) -> SurfaceBranchMode {
        self.mode
    }
    pub(crate) fn witnessed(&self) -> bool {
        self.witnessed
    }
    pub(crate) fn members(&self) -> &BranchItems<(PayloadIndexToken, B)> {
        &self.members
    }
    pub(crate) fn terminal(&self) -> &(PayloadIndexToken, B) {
        &self.terminal
    }
    pub(crate) fn suffix(&self) -> &SurfaceSuffix {
        &self.suffix
    }

    pub(crate) fn member_offsets(&self) -> impl Iterator<Item = u64> + '_ {
        let mut at = self.offset + 3;
        self.members.as_slice().iter().map(move |(token, _)| {
            let offset = at;
            at += cadmpeg_core::decode::u64_from_index(token.raw().len());
            offset
        })
    }

    fn terminal_relative_offset(&self) -> u64 { self.terminal_relative }

    pub(crate) fn terminal_offset(&self) -> u64 {
        self.offset + self.terminal_relative_offset()
    }
}

impl SurfaceBranch<()> {
    pub(crate) fn resolve<B>(
        self,
        ctx: &DecodeContext<'_>,
        file_base: u64,
        mut target: impl FnMut(PayloadIndexToken) -> Result<B, CodecError>,
    ) -> Result<Option<SurfaceBranch<B>>, CodecError> {
        let Some(offset) = self.offset.checked_add(file_base) else {
            return Ok(None);
        };
        let members = self
            .members
            .try_map_indexed_charged(ctx, |_, (token, ())| Ok((token, target(token)?)))?;
        let terminal = (self.terminal.0, target(self.terminal.0)?);
        Ok(SurfaceBranch::from_wire(
            ctx, offset,
            self.mode,
            self.witnessed,
            members,
            terminal,
            self.suffix,
        )?.ok())
    }
}

/// A uniquely framed group containing the declared 1 through 255 branches.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct SurfaceFeaturePayloadBranches {
    pub(crate) family: SurfaceFamily,
    pub(crate) header_code: u8,
    branches: Vec<SurfaceBranch<()>>,
}

impl SurfaceFeaturePayloadBranches {
    pub(crate) fn into_branches(self) -> Vec<SurfaceBranch<()>> {
        self.branches
    }
}

fn surface_feature_branch_paths(
    ctx: &DecodeContext<'_>,
    payload: &[u8],
    payload_offset: usize,
    at: usize,
    remaining: u8,
    terminator: &[u8],
) -> Result<Vec<Vec<SurfaceBranch<()>>>, CodecError> {
    let _depth = ctx.enter_nested("NX surface branch path")?;
    ctx.charge_work(1, "scan NX surface branch path")?;
    if remaining == 0 {
        return Ok(Vec::new());
    }
    let Some(mode) = payload
        .get(at)
        .copied()
        .and_then(|value| SurfaceBranchMode::try_from(value).ok())
    else {
        return Ok(Vec::new());
    };
    if payload.get(at + 1) != Some(&0x01) {
        return Ok(Vec::new());
    }
    let Some(declared_count @ 2..) = payload.get(at + 2).copied() else {
        return Ok(Vec::new());
    };
    let mut cursor = at + 3;
    let mut members = Vec::new();
    for _ in 1..declared_count {
        let Some(token) = payload.get(cursor..).and_then(PayloadIndexToken::read) else {
            return Ok(Vec::new());
        };
        cursor += token.raw().len();
        ctx.reserve_vec(&mut members, 1, "NX surface branch members")?;
        members.push((token, ()));
    }
    let Ok(members) = BranchItems::new(members) else {
        return Ok(Vec::new());
    };
    let witnessed = payload.get(cursor..cursor + 2) == Some(&[0x01, declared_count]);
    if witnessed {
        cursor += 2;
    }
    let zero_count = if witnessed {
        usize::from(declared_count) + 3
    } else {
        5
    };
    let Some(zero_lane) = payload.get(cursor..cursor + zero_count) else {
        return Ok(Vec::new());
    };
    if !ctx.admit_iter(zero_lane, "NX surface branch zero lane")?.all(|&byte| byte == 0) {
        return Ok(Vec::new());
    }
    cursor += zero_count;
    if payload.get(cursor..cursor + 3) != Some(&[0xff, 0x01, 0x02]) {
        return Ok(Vec::new());
    }
    cursor += 3;
    let Some(terminal) = payload.get(cursor..).and_then(PayloadIndexToken::read) else {
        return Ok(Vec::new());
    };
    cursor += terminal.raw().len();
    if payload.get(cursor) != Some(&0x00) {
        return Ok(Vec::new());
    }
    cursor += 1;

    let mut paths = Vec::new();
    for suffix_len in 1..=5 {
        ctx.charge_work(1, "scan NX surface branch suffix")?;
        let Some(suffix) = payload.get(cursor..cursor + suffix_len) else {
            continue;
        };
        let next = cursor + suffix_len;
        let continuations = if remaining == 1 {
            let mut continuations = Vec::new();
            if payload.get(next..next + terminator.len()) == Some(terminator) {
                ctx.reserve_vec(&mut continuations, 1, "NX surface terminal paths")?;
                continuations.push(Vec::new());
            }
            continuations
        } else {
            surface_feature_branch_paths(
                ctx,
                payload,
                payload_offset,
                next,
                remaining - 1,
                terminator,
            )?
        };
        if continuations.is_empty() {
            continue;
        }
        let Some(offset) = u64_from_index(payload_offset).checked_add(u64_from_index(at)) else {
            continue;
        };
        for mut continuation in continuations {
            let mut member_copy = Vec::new();
            for member in ctx.admit_iter(members.as_slice(), "NX surface branch copied members")?.copied() {
                ctx.reserve_vec(&mut member_copy, 1, "NX surface branch member copy")?;
                member_copy.push(member);
            }
            let Ok(member_copy) = BranchItems::new(member_copy) else {
                continue;
            };
            ctx.charge_collection_items(u64_from_index(suffix.len()), "NX surface suffix bytes")?;
            let suffix_copy = ctx.copy_retained(suffix, "NX surface suffix bytes")?;
            let Ok(suffix_copy) = SurfaceSuffix::new(suffix_copy) else {
                continue;
            };
            let Ok(branch) = SurfaceBranch::from_wire(
                ctx,
                offset,
                mode,
                witnessed,
                member_copy,
                (terminal, ()),
                suffix_copy,
            )? else {
                continue;
            };
            ctx.reserve_vec(&mut continuation, 1, "NX surface branch path entries")?;
            continuation.insert(0, branch);
            ctx.reserve_vec(&mut paths, 1, "NX surface branch paths")?;
            paths.push(continuation);
            if paths.len() == 2 {
                return Ok(paths);
            }
        }
    }
    Ok(paths)
}

/// Decode the unique exactly framed counted branch group in a bounded `SKIN`
/// or `Studio Surface` payload.
pub(crate) fn surface_feature_payload_branches(
    ctx: &DecodeContext<'_>,
    record: OperationPayload<'_>,
) -> Result<Option<SurfaceFeaturePayloadBranches>, CodecError> {
    const SKIN_TERMINATOR: [u8; 11] = [
        0x00, 0x00, 0x00, 0x01, 0x03, 0x00, 0x00, 0x00, 0xff, 0xff, 0x01,
    ];
    const STUDIO_TERMINATOR: [u8; 8] = [0x00, 0x00, 0x00, 0x00, 0x00, 0xff, 0xff, 0x01];
    let terminator = match record.name() {
        "SKIN" => &SKIN_TERMINATOR[..],
        "Studio Surface" => &STUDIO_TERMINATOR[..],
        _ => return Ok(None),
    };
    ctx.charge_work(
        u64_from_index(record.payload().len()),
        "scan NX surface branch groups",
    )?;
    let mut candidate = None;
    for start in record
        .payload()
        .len()
        .checked_sub(6)
        .into_iter()
        .flat_map(|last| 0..last)
    {
        if record.payload().get(start..start + 2) != Some(&[0xa0, 0x5a]) {
            continue;
        }
        let Some(family) = record
            .payload()
            .get(start + 2)
            .copied()
            .and_then(|byte| SurfaceFamily::try_from(byte).ok())
        else {
            continue;
        };
        let Some(header_code) = record.payload().get(start + 3).copied() else {
            continue;
        };
        if record.payload().get(start + 4) != Some(&0x01) {
            continue;
        }
        let Some(declared_group_count @ 1..) = record.payload().get(start + 5).copied() else {
            continue;
        };
        let paths = surface_feature_branch_paths(
            ctx,
            record.payload(),
            record.payload_offset(),
            start + 6,
            declared_group_count,
            terminator,
        )?;
        if paths.len() != 1 {
            continue;
        }
        let Some(branches) = paths.into_iter().next() else {
            continue;
        };
        let group = SurfaceFeaturePayloadBranches {
            family,
            header_code,
            branches,
        };
        if candidate.is_some() {
            return Ok(None);
        }
        candidate = Some(group);
    }
    Ok(candidate)
}

#[cfg(test)]
mod tests {
    use super::{SurfaceFamily, SurfaceSuffix};

    #[test]
    fn surface_family_preserves_exact_numeric_domain() {
        for byte in u8::MIN..=u8::MAX {
            let wire = byte.to_string();
            let decoded = serde_json::from_str::<SurfaceFamily>(&wire);
            if matches!(byte, 0x14 | 0x50) {
                assert_eq!(serde_json::to_string(&decoded.unwrap()).unwrap(), wire);
            } else {
                assert!(decoded.unwrap_err().to_string().contains("family"));
            }
        }
    }

    #[test]
    fn surface_suffix_preserves_opaque_bytes_and_rejects_invalid_lengths() {
        for wire in [
            "[255]",
            "[0,255]",
            "[1,2,3]",
            "[0,1,2,255]",
            "[255,0,1,2,3]",
        ] {
            let decoded: SurfaceSuffix = serde_json::from_str(wire).unwrap();
            assert_eq!(serde_json::to_string(&decoded).unwrap(), wire);
            assert!((1..=5).contains(&decoded.into_vec().len()));
        }
        for wire in ["[]", "[0,1,2,3,4,5]"] {
            assert!(serde_json::from_str::<SurfaceSuffix>(wire)
                .unwrap_err()
                .to_string()
                .contains("suffix"));
        }
    }
}
