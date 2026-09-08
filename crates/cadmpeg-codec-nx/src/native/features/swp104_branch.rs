// SPDX-License-Identifier: Apache-2.0
//! Leading SWP104 frame with positions derived from its serialized fields.

use crate::om::{
    branch_items::BranchItems, reference_index::PayloadIndexToken, scalar::ShiftedBinary64,
    swp104_state::Swp104StateLane, Swp104PayloadLeadingBranch,
};
use serde::{Deserialize, Serialize};
use std::num::NonZeroU8;
#[derive(Serialize, Deserialize)]
struct ReferenceWire {
    ordinal: u32,
    #[serde(flatten)]
    token: PayloadIndexToken,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    data_block: Option<String>,
    source_offset: u64,
}

/// Exact leading construction branch in a `SWP104` payload.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(
    try_from = "FeatureSwp104LeadingBranchWire",
    into = "FeatureSwp104LeadingBranchWire"
)]
pub(crate) struct FeatureSwp104LeadingBranch {
    /// Globally unique leading-branch identity.
    id: String,
    /// Owning `SWP104` operation label.
    operation_label: String,
    /// Nonzero construction discriminator.
    discriminator: NonZeroU8,
    /// Four finite shifted-binary64 values in serialized order.
    scalars: [ShiftedBinary64; 4],
    /// Whether one zero byte precedes the branch mode.
    leading_zero: bool,
    /// Serialized nonzero branch mode.
    mode: NonZeroU8,
    /// Exact state lane preceding the terminal marker.
    state_lane: Swp104StateLane,
    /// Ordered nonterminal references.
    members: BranchItems<Reference>,
    /// Terminal reference.
    terminal: Reference,
    /// Absolute source offset of the discriminator.
    source_offset: u64,
}

#[derive(Debug, Clone, PartialEq)]
struct Reference {
    token: PayloadIndexToken,
    data_block: Option<String>,
}

impl FeatureSwp104LeadingBranch {
    pub(crate) fn from_source(
        id: String,
        operation_label: String,
        source_offset: u64,
        branch: Swp104PayloadLeadingBranch,
        resolve: impl Fn(PayloadIndexToken) -> Option<String>,
    ) -> Option<Self> {
        source_offset.checked_add(branch.byte_len() as u64)?;
        let reference = |token| Reference {
            token,
            data_block: resolve(token),
        };
        Some(Self {
            id,
            operation_label,
            source_offset,
            discriminator: branch.discriminator,
            scalars: branch.scalars,
            leading_zero: branch.leading_zero,
            mode: branch.mode,
            state_lane: branch.state_lane,
            members: branch.members.map_indexed(|_, item| reference(item)),
            terminal: reference(branch.terminal),
        })
    }

    fn members_offset(&self) -> u64 {
        40 + u64::from(self.leading_zero)
    }
    fn state_len(&self) -> u64 {
        self.state_lane.byte_len() as u64
    }
    fn byte_len(&self) -> u64 {
        self.members_offset()
            + self
                .members
                .as_slice()
                .iter()
                .map(|item| item.token.raw().len() as u64)
                .sum::<u64>()
            + self.state_len()
            + 3
            + self.terminal.token.raw().len() as u64
            + 1
    }
}

#[derive(Serialize, Deserialize)]
struct FeatureSwp104LeadingBranchWire {
    id: String,
    operation_label: String,
    discriminator: NonZeroU8,
    scalars: [f64; 4],
    raw_scalars: [[u8; 8]; 4],
    leading_zero: bool,
    mode: NonZeroU8,
    declared_count: u8,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    witnessed_count: Option<u8>,
    state_lane: Vec<u8>,
    members: BranchItems<ReferenceWire>,
    terminal: ReferenceWire,
    byte_len: u64,
    source_offset: u64,
}

impl From<FeatureSwp104LeadingBranch> for FeatureSwp104LeadingBranchWire {
    fn from(value: FeatureSwp104LeadingBranch) -> Self {
        let byte_len = value.byte_len();
        let mut at = value.source_offset + value.members_offset();
        let state_len = value.state_len();
        let terminal_ordinal = value.members.len() as u32;
        let members = value.members.map_indexed(|ordinal, reference| {
            let source_offset = at;
            at += reference.token.raw().len() as u64;
            ReferenceWire {
                ordinal: ordinal as u32,
                token: reference.token,
                data_block: reference.data_block,
                source_offset,
            }
        });
        let terminal = ReferenceWire {
            ordinal: terminal_ordinal,
            token: value.terminal.token,
            data_block: value.terminal.data_block,
            source_offset: at + state_len + 3,
        };
        Self {
            id: value.id,
            operation_label: value.operation_label,
            discriminator: value.discriminator,
            scalars: value.scalars.map(ShiftedBinary64::value),
            raw_scalars: value.scalars.map(ShiftedBinary64::raw),
            leading_zero: value.leading_zero,
            mode: value.mode,
            declared_count: members.declared_count(),
            witnessed_count: value.state_lane.witnessed_count(),
            state_lane: value.state_lane.bytes().to_vec(),
            members,
            terminal,
            byte_len,
            source_offset: value.source_offset,
        }
    }
}

impl TryFrom<FeatureSwp104LeadingBranchWire> for FeatureSwp104LeadingBranch {
    type Error = String;
    fn try_from(wire: FeatureSwp104LeadingBranchWire) -> Result<Self, Self::Error> {
        if wire.declared_count != wire.members.declared_count() {
            return Err("declared_count must equal members length plus one".to_owned());
        }
        let [a, b, c, d] = std::array::from_fn::<_, 4, _>(|i| {
            ShiftedBinary64::from_wire(wire.scalars[i], wire.raw_scalars[i])
        });
        let scalars = [a?, b?, c?, d?];
        let state_lane = Swp104StateLane::from_parts(wire.witnessed_count, wire.state_lane)?;
        let mut at = wire
            .source_offset
            .checked_add(40 + u64::from(wire.leading_zero))
            .ok_or("source_offset overflow")?;
        for (ordinal, item) in wire.members.as_slice().iter().enumerate() {
            if item.ordinal != ordinal as u32 {
                return Err("members ordinal does not match serialized order".to_owned());
            }
            if item.source_offset != at {
                return Err("members source_offset does not match serialized position".to_owned());
            }
            at = at
                .checked_add(item.token.raw().len() as u64)
                .ok_or("source_offset overflow")?;
        }
        at = at
            .checked_add(state_lane.byte_len() as u64 + 3)
            .ok_or("source_offset overflow")?;
        if wire.terminal.ordinal != wire.members.len() as u32 {
            return Err("terminal ordinal does not match serialized order".to_owned());
        }
        if wire.terminal.source_offset != at {
            return Err("terminal source_offset does not match serialized position".to_owned());
        }
        let end = at
            .checked_add(wire.terminal.token.raw().len() as u64 + 1)
            .ok_or("source_offset overflow")?;
        if wire.byte_len != end - wire.source_offset {
            return Err("byte_len does not match serialized frame length".to_owned());
        }
        let reference = |item: ReferenceWire| Reference {
            token: item.token,
            data_block: item.data_block,
        };
        Ok(Self {
            id: wire.id,
            operation_label: wire.operation_label,
            discriminator: wire.discriminator,
            scalars,
            leading_zero: wire.leading_zero,
            mode: wire.mode,
            state_lane,
            members: wire.members.map_indexed(|_, item| reference(item)),
            terminal: reference(wire.terminal),
            source_offset: wire.source_offset,
        })
    }
}

#[cfg(test)]
mod tests {
    // Source frames are explicit; wire assertions exercise derived positions.
    #![allow(clippy::unwrap_used)]
    use super::FeatureSwp104LeadingBranch;

    #[test]
    fn source_frames_derive_native_positions_for_all_optional_lanes() {
        for leading_zero in [false, true] {
            for witnessed_count in [None, Some(2_u8), Some(255)] {
                for member_count in [1_u8, 254] {
                    let mut payload = vec![33, 0, 0, 1, 0];
                    for _ in 0..4 {
                        payload.extend([47, 164, 122, 225, 71, 174, 20, 123]);
                    }
                    if leading_zero {
                        payload.push(0);
                    }
                    payload.extend([35, 1, member_count + 1]);
                    for _ in 0..member_count {
                        payload.extend([240, 1]);
                    }
                    if let Some(count) = witnessed_count {
                        payload.extend([1, count]);
                        payload.extend(std::iter::repeat_n(0, usize::from(count) + 3));
                    } else {
                        payload.extend([0; 5]);
                    }
                    payload.extend([255, 1, 2, 241, 1, 0, 0]);
                    let record =
                        crate::om::operation_record::OperationPayload::new(&payload, 200, "SWP104")
                            .unwrap();
                    let source = crate::om::swp104_payload_leading_branch(record).unwrap();
                    let branch = FeatureSwp104LeadingBranch::from_source(
                        "branch".to_owned(),
                        "operation".to_owned(),
                        1200,
                        source.clone(),
                        |token| Some(format!("block#{}", token.value())),
                    )
                    .unwrap();
                    let wire = serde_json::to_value(&branch).unwrap();
                    assert_eq!(wire["byte_len"], payload.len());
                    assert_eq!(
                        wire["members"][0]["source_offset"],
                        1240 + u64::from(leading_zero)
                    );
                    assert_eq!(wire["terminal"]["source_offset"], 1200 + payload.len() - 4);
                    assert_eq!(wire["terminal"]["ordinal"], member_count);
                    assert_eq!(wire["terminal"]["data_block"], "block#256");
                    assert_eq!(
                        serde_json::from_value::<FeatureSwp104LeadingBranch>(wire.clone()).unwrap(),
                        branch
                    );
                    let mut invalid = wire;
                    invalid["members"][0]["raw_object_index"] = serde_json::json!([1]);
                    assert!(serde_json::from_value::<FeatureSwp104LeadingBranch>(invalid).is_err());
                    assert!(FeatureSwp104LeadingBranch::from_source(
                        "branch".to_owned(),
                        "operation".to_owned(),
                        u64::MAX,
                        source,
                        |_| None
                    )
                    .is_none());
                }
            }
        }
    }
}
