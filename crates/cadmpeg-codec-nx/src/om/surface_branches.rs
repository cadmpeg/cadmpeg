// SPDX-License-Identifier: Apache-2.0
//! Surface construction branch framing domains.

use super::branch_items::BranchItems;
use super::discriminators::SurfaceBranchMode;
use super::operation_record::OperationPayload;
use super::reference_index::PayloadIndexToken;
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
        value as Self
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
                    + branch.terminal.0.raw().len() as u64
                    + 1
                    + branch.suffix.0.len() as u64,
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
            at += token.raw().len() as u64;
            offset
        })
    }

    fn terminal_relative_offset(&self) -> u64 {
        let token_bytes = self
            .members
            .as_slice()
            .iter()
            .map(|(token, _)| token.raw().len() as u64)
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
    ) -> Result<SurfaceBranch<B>, &'static str> {
        let offset = self
            .offset
            .checked_add(file_base)
            .ok_or("source_offset: surface branch frame overflows")?;
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
    payload: &[u8],
    payload_offset: usize,
    at: usize,
    remaining: u8,
    terminator: &[u8],
) -> Vec<Vec<SurfaceBranch<()>>> {
    if remaining == 0 {
        return Vec::new();
    }
    let Some(mode) = payload
        .get(at)
        .copied()
        .and_then(|value| SurfaceBranchMode::try_from(value).ok())
    else {
        return Vec::new();
    };
    if payload.get(at + 1) != Some(&0x01) {
        return Vec::new();
    }
    let Some(declared_count @ 2..) = payload.get(at + 2).copied() else {
        return Vec::new();
    };
    let mut cursor = at + 3;
    let mut members = Vec::with_capacity(usize::from(declared_count) - 1);
    for _ in 1..declared_count {
        let Some(token) = payload.get(cursor..).and_then(PayloadIndexToken::read) else {
            return Vec::new();
        };
        cursor += token.raw().len();
        members.push((token, ()));
    }
    let Ok(members) = BranchItems::new(members) else {
        return Vec::new();
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
        return Vec::new();
    };
    if !zero_lane.iter().all(|&byte| byte == 0) {
        return Vec::new();
    }
    cursor += zero_count;
    if payload.get(cursor..cursor + 3) != Some(&[0xff, 0x01, 0x02]) {
        return Vec::new();
    }
    cursor += 3;
    let Some(terminal) = payload.get(cursor..).and_then(PayloadIndexToken::read) else {
        return Vec::new();
    };
    cursor += terminal.raw().len();
    if payload.get(cursor) != Some(&0x00) {
        return Vec::new();
    }
    cursor += 1;

    let mut paths = Vec::new();
    for suffix_len in 1..=5 {
        let Some(suffix) = payload.get(cursor..cursor + suffix_len) else {
            continue;
        };
        let next = cursor + suffix_len;
        let continuations = if remaining == 1 {
            (payload.get(next..next + terminator.len()) == Some(terminator))
                .then_some(Vec::new())
                .into_iter()
                .collect::<Vec<_>>()
        } else {
            surface_feature_branch_paths(payload, payload_offset, next, remaining - 1, terminator)
        };
        if continuations.is_empty() {
            continue;
        }
        let Ok(suffix) = SurfaceSuffix::new(suffix.to_vec()) else {
            continue;
        };
        let Some(offset) = (payload_offset as u64).checked_add(at as u64) else {
            continue;
        };
        let Ok(branch) = SurfaceBranch::new(
            offset,
            mode,
            witnessed,
            members.clone(),
            (terminal, ()),
            suffix,
        ) else {
            continue;
        };
        for mut continuation in continuations {
            continuation.insert(0, branch.clone());
            paths.push(continuation);
            if paths.len() == 2 {
                return paths;
            }
        }
    }
    paths
}

/// Decode the unique exactly framed counted branch group in a bounded `SKIN`
/// or `Studio Surface` payload.
pub(crate) fn surface_feature_payload_branches(
    record: OperationPayload<'_>,
) -> Option<SurfaceFeaturePayloadBranches> {
    const SKIN_TERMINATOR: [u8; 11] = [
        0x00, 0x00, 0x00, 0x01, 0x03, 0x00, 0x00, 0x00, 0xff, 0xff, 0x01,
    ];
    const STUDIO_TERMINATOR: [u8; 8] = [0x00, 0x00, 0x00, 0x00, 0x00, 0xff, 0xff, 0x01];
    let terminator = match record.name() {
        "SKIN" => &SKIN_TERMINATOR[..],
        "Studio Surface" => &STUDIO_TERMINATOR[..],
        _ => return None,
    };
    let mut candidate = None;
    for start in 0..record.payload().len().saturating_sub(6) {
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
            record.payload(),
            record.payload_offset(),
            start + 6,
            declared_group_count,
            terminator,
        );
        let [branches] = paths.as_slice() else {
            continue;
        };
        let group = SurfaceFeaturePayloadBranches {
            family,
            header_code,
            branches: branches.clone(),
        };
        if candidate.is_some() {
            return None;
        }
        candidate = Some(group);
    }
    candidate
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
