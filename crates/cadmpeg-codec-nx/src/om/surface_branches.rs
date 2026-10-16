// SPDX-License-Identifier: Apache-2.0
//! Surface construction branch framing domains.

use super::branch_items::BranchItems;
use super::discriminators::SurfaceBranchMode;
use super::operation_record::OperationPayload;
use super::reference_index::PayloadIndexToken;
use cadmpeg_core::decode::{u64_from_index, DecodeContext};
use cadmpeg_core::CodecError;
use serde::{Deserialize, Deserializer, Serialize};
use std::collections::BTreeMap;

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
pub(crate) struct SurfaceSuffix(cadmpeg_ir::native::bytes::NativeBytes);

impl SurfaceSuffix {
    pub(crate) fn new(bytes: Vec<u8>) -> Result<Self, &'static str> {
        if !(1..=5).contains(&bytes.len()) {
            return Err("surface suffix must contain 1 through 5 bytes");
        }
        Ok(Self(bytes.into()))
    }

    #[cfg(test)]
    pub(crate) fn into_vec(self) -> Vec<u8> {
        self.0.into_inner()
    }
}

impl<'de> Deserialize<'de> for SurfaceSuffix {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        Self::new(
            cadmpeg_ir::native::bytes::NativeBytes::<Vec<u8>>::deserialize(deserializer)?
                .into_inner(),
        )
        .map_err(serde::de::Error::custom)
    }
}

/// One branch whose reference positions follow from its frame and token widths.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct SurfaceBranch<B> {
    offset: u64,
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
        let branch = Self {
            offset,
            mode,
            witnessed,
            members,
            terminal,
            suffix,
        };
        branch
            .offset
            .checked_add(
                branch.terminal_relative_offset()
                    + cadmpeg_core::decode::u64_from_index(branch.terminal.0.raw().len())
                    + 1
                    + cadmpeg_core::decode::u64_from_index(branch.suffix.0.len()),
            )
            .ok_or("source_offset: surface branch frame overflows")?;
        Ok(branch)
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

    fn terminal_relative_offset(&self) -> u64 {
        let token_bytes = self
            .members
            .as_slice()
            .iter()
            .map(|(token, _)| cadmpeg_core::decode::u64_from_index(token.raw().len()))
            .sum::<u64>();
        let state_bytes = if self.witnessed {
            u64::from(self.members.declared_count()) + 5
        } else {
            5
        };
        3 + token_bytes + state_bytes + 3
    }

    pub(crate) fn terminal_offset(&self) -> u64 {
        self.offset + self.terminal_relative_offset()
    }
}

impl SurfaceBranch<()> {
    pub(crate) fn resolve<B>(
        self,
        file_base: u64,
        mut target: impl FnMut(PayloadIndexToken) -> B,
    ) -> Option<SurfaceBranch<B>> {
        let offset = self.offset.checked_add(file_base)?;
        let members = self
            .members
            .map_indexed(|_, (token, ())| (token, target(token)));
        let terminal = (self.terminal.0, target(self.terminal.0));
        SurfaceBranch::new(
            offset,
            self.mode,
            self.witnessed,
            members,
            terminal,
            self.suffix,
        )
        .ok()
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

/// One branch group framed at a payload offset, before its suffix.
struct BranchGroupFrame {
    mode: SurfaceBranchMode,
    witnessed: bool,
    members: BranchItems<(PayloadIndexToken, ())>,
    terminal: PayloadIndexToken,
    end: usize,
}

fn branch_group_frame(payload: &[u8], at: usize) -> Option<BranchGroupFrame> {
    let mode = payload
        .get(at)
        .copied()
        .and_then(|value| SurfaceBranchMode::try_from(value).ok())?;
    if payload.get(at + 1) != Some(&0x01) {
        return None;
    }
    let declared_count @ 2.. = payload.get(at + 2).copied()? else {
        return None;
    };
    let mut cursor = at + 3;
    let mut members = Vec::new();
    for _ in 1..declared_count {
        let token = payload.get(cursor..).and_then(PayloadIndexToken::read)?;
        cursor += token.raw().len();
        members.push((token, ()));
    }
    let members = BranchItems::new(members).ok()?;
    let witnessed = payload.get(cursor..cursor + 2) == Some(&[0x01, declared_count]);
    if witnessed {
        cursor += 2;
    }
    let zero_count = if witnessed {
        usize::from(declared_count) + 3
    } else {
        5
    };
    if !payload
        .get(cursor..cursor + zero_count)?
        .iter()
        .all(|&byte| byte == 0)
    {
        return None;
    }
    cursor += zero_count;
    if payload.get(cursor..cursor + 3) != Some(&[0xff, 0x01, 0x02]) {
        return None;
    }
    cursor += 3;
    let terminal = payload.get(cursor..).and_then(PayloadIndexToken::read)?;
    cursor += terminal.raw().len();
    if payload.get(cursor) != Some(&0x00) {
        return None;
    }
    Some(BranchGroupFrame {
        mode,
        witnessed,
        members,
        terminal,
        end: cursor + 1,
    })
}

/// Search for complete branch paths, memoised on `(offset, remaining groups)`.
///
/// Distinct suffix lengths can frame overlapping groups that rejoin, so the
/// path tree can grow exponentially while the distinct search states stay
/// bounded by the payload length times the declared group count.
struct BranchPathSearch<'a> {
    payload: &'a [u8],
    terminator: &'a [u8],
    group_ends: BTreeMap<usize, Option<usize>>,
    path_counts: BTreeMap<(usize, u8), u8>,
}

impl<'a> BranchPathSearch<'a> {
    fn new(payload: &'a [u8], terminator: &'a [u8]) -> Self {
        Self {
            payload,
            terminator,
            group_ends: BTreeMap::new(),
            path_counts: BTreeMap::new(),
        }
    }

    fn group_end(&mut self, at: usize) -> Option<usize> {
        *self
            .group_ends
            .entry(at)
            .or_insert_with(|| branch_group_frame(self.payload, at).map(|frame| frame.end))
    }

    fn terminates_at(&self, at: usize) -> bool {
        self.payload.get(at..at + self.terminator.len()) == Some(self.terminator)
    }

    /// Number of complete paths of `remaining` groups from `at`, capped at two.
    fn path_count(
        &mut self,
        ctx: &DecodeContext<'_>,
        at: usize,
        remaining: u8,
    ) -> Result<u8, CodecError> {
        let _depth = ctx.enter_nested("NX surface branch path")?;
        if let Some(&count) = self.path_counts.get(&(at, remaining)) {
            return Ok(count);
        }
        let count = self.uncached_path_count(ctx, at, remaining)?;
        self.path_counts.insert((at, remaining), count);
        Ok(count)
    }

    fn uncached_path_count(
        &mut self,
        ctx: &DecodeContext<'_>,
        at: usize,
        remaining: u8,
    ) -> Result<u8, CodecError> {
        if remaining == 0 {
            return Ok(0);
        }
        let Some(end) = self.group_end(at) else {
            return Ok(0);
        };
        let mut count = 0;
        for suffix_len in 1..=5 {
            let next = end + suffix_len;
            if next > self.payload.len() {
                continue;
            }
            let continuations = if remaining == 1 {
                u8::from(self.terminates_at(next))
            } else {
                self.path_count(ctx, next, remaining - 1)?
            };
            count = (count + continuations).min(2);
            if count == 2 {
                break;
            }
        }
        Ok(count)
    }

    /// Rebuild the single path that [`Self::path_count`] counted from `at`.
    fn unique_path(
        &self,
        payload_offset: usize,
        mut at: usize,
        mut remaining: u8,
    ) -> Option<Vec<SurfaceBranch<()>>> {
        let mut path = Vec::with_capacity(usize::from(remaining));
        while remaining > 0 {
            let frame = branch_group_frame(self.payload, at)?;
            let next = (1..=5)
                .map(|suffix_len| frame.end + suffix_len)
                .find(|&next| {
                    next <= self.payload.len()
                        && if remaining == 1 {
                            self.terminates_at(next)
                        } else {
                            self.path_counts.get(&(next, remaining - 1)) == Some(&1)
                        }
                })?;
            let offset = u64_from_index(payload_offset).checked_add(u64_from_index(at))?;
            let suffix = SurfaceSuffix::new(self.payload.get(frame.end..next)?.to_vec()).ok()?;
            path.push(
                SurfaceBranch::new(
                    offset,
                    frame.mode,
                    frame.witnessed,
                    frame.members,
                    (frame.terminal, ()),
                    suffix,
                )
                .ok()?,
            );
            at = next;
            remaining -= 1;
        }
        Some(path)
    }
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
    let mut search = BranchPathSearch::new(record.payload(), terminator);
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
        if search.path_count(ctx, start + 6, declared_group_count)? != 1 {
            continue;
        }
        let Some(branches) =
            search.unique_path(record.payload_offset(), start + 6, declared_group_count)
        else {
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
    use super::{surface_feature_payload_branches, BranchPathSearch, SurfaceFamily, SurfaceSuffix};
    use crate::om::operation_record::OperationPayload;

    const SKIN_TERMINATOR: [u8; 11] = [
        0x00, 0x00, 0x00, 0x01, 0x03, 0x00, 0x00, 0x00, 0xff, 0xff, 0x01,
    ];

    /// A root group, then `levels` pairs of groups at suffix lengths 1 and 5
    /// that frame the same tail, so the path tree doubles at every level.
    fn rejoining_branch_payload(levels: u8, terminated: bool) -> Vec<u8> {
        let mut payload = vec![0xa0, 0x5a, 0x14, 0x13, 0x01, levels + 1];
        payload.extend([0x40, 0x01, 0x02, 0xf0, 0x10, 0, 0, 0, 0, 0]);
        payload.extend([0xff, 0x01, 0x02, 0xf0, 0x20, 0x00]);
        for _ in 0..levels {
            // Suffix byte, then the first group: mode, 0x01, 242 members.
            payload.extend([0x00, 0x40, 0x01, 0xf2]);
            // Its first token `f1 40 01` also opens the second group, whose
            // count byte 0xf0 is the first byte of the token after it.
            payload.extend([0xf1, 0x40, 0x01, 0xf0, 0xf1, 0xf0, 0x11]);
            for value in 0..238_u8 {
                payload.extend([0xf0, value]);
            }
            payload.extend([0, 0, 0, 0, 0, 0xff, 0x01, 0x02, 0xf0, 0x22, 0x00]);
        }
        payload.push(0x00);
        if terminated {
            payload.extend(SKIN_TERMINATOR);
        } else {
            payload.extend([0; SKIN_TERMINATOR.len()]);
        }
        payload
    }

    #[test]
    fn rejoining_branch_groups_are_searched_once_per_offset() {
        const LEVELS: u8 = 20;
        let payload = rejoining_branch_payload(LEVELS, false);
        crate::test_support::with_decode_context(|ctx| {
            let mut search = BranchPathSearch::new(&payload, &SKIN_TERMINATOR);
            assert_eq!(search.path_count(ctx, 6, LEVELS + 1).unwrap(), 0);
            assert!(search.path_counts.len() <= 5 * usize::from(LEVELS + 1));

            let record = OperationPayload::new(&payload, 0, "SKIN").unwrap();
            assert!(surface_feature_payload_branches(ctx, record)
                .unwrap()
                .is_none());
        });
    }

    #[test]
    fn rejoining_branch_groups_keep_path_count_semantics() {
        crate::test_support::with_decode_context(|ctx| {
            let single = rejoining_branch_payload(0, true);
            let record = OperationPayload::new(&single, 100, "SKIN").unwrap();
            let group = surface_feature_payload_branches(ctx, record)
                .unwrap()
                .expect("one root group");
            let branches = group.into_branches();
            assert_eq!(branches.len(), 1);
            assert_eq!(branches[0].offset(), 106);
            assert_eq!(branches[0].terminal().0.value(), 0x20);
            assert_eq!(branches[0].suffix().clone().into_vec(), [0x00]);

            for levels in [1, 20] {
                let payload = rejoining_branch_payload(levels, true);
                let mut search = BranchPathSearch::new(&payload, &SKIN_TERMINATOR);
                assert_eq!(search.path_count(ctx, 6, levels + 1).unwrap(), 2);
                let record = OperationPayload::new(&payload, 0, "SKIN").unwrap();
                assert!(surface_feature_payload_branches(ctx, record)
                    .unwrap()
                    .is_none());
            }
        });
    }

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
            "\"ff\"",
            "\"00ff\"",
            "\"010203\"",
            "\"000102ff\"",
            "\"ff00010203\"",
        ] {
            let decoded: SurfaceSuffix = serde_json::from_str(wire).unwrap();
            assert_eq!(serde_json::to_string(&decoded).unwrap(), wire);
            assert!((1..=5).contains(&decoded.into_vec().len()));
        }
        for wire in ["\"\"", "\"000102030405\""] {
            assert!(serde_json::from_str::<SurfaceSuffix>(wire)
                .unwrap_err()
                .to_string()
                .contains("suffix"));
        }
    }
}
