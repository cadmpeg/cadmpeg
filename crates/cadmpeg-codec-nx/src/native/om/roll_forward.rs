// SPDX-License-Identifier: Apache-2.0
//! Native roll-forward metadata and flat wire admission.

use super::state_index_wire;
use crate::om::roll_forward::{GroupTableFooter, OperationStateGroup};
use crate::om::state_group::{
    OperationStateGroupCount, OperationStateGroupOpener, StateGroupMembers,
};
use serde::{Deserialize, Serialize};

/// One counted `m_rollForwardStates` group from a feature-history section.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(
    try_from = "OmRollForwardStateGroupWire",
    into = "OmRollForwardStateGroupWire"
)]
pub(crate) struct OmRollForwardStateGroup {
    /// Globally unique group identity.
    pub id: String,
    /// Owning feature-history section link.
    pub section_link: String,
    /// Zero-based group ordinal within the table.
    pub ordinal: u32,
    pub(crate) frame: OperationStateGroup<u64>,
    /// Exact bytes between the final group and the counter-map boundary.
    pub table_footer: GroupTableFooter,
    /// Directory entry containing the feature-history section.
    pub source_entry: String,
    /// Absolute file offset of the counter-map boundary.
    pub table_end_offset: u64,
}

#[derive(Serialize, Deserialize)]
struct OmRollForwardStateGroupWire {
    /// Globally unique group identity.
    id: String,
    /// Owning feature-history section link.
    section_link: String,
    /// Zero-based group ordinal within the table.
    ordinal: u32,
    /// Exact two-byte group opener.
    opener: [u8; 2],
    /// Whether the count used the nonempty `01 count` form.
    count_prefix: Option<u8>,
    /// Serialized member count including the implicit owner slot.
    declared_count: u8,
    /// Ordered typed rows in the group.
    rows: Vec<state_index_wire::OmRollForwardStateRowWire>,
    /// Exact bytes between the final group and the counter-map boundary.
    table_trailing_bytes: Vec<u8>,
    /// Directory entry containing the feature-history section.
    source_entry: String,
    /// Absolute file offset of the group opener.
    source_offset: u64,
    /// Absolute file offset of the counter-map boundary.
    table_end_offset: u64,
}

impl From<OmRollForwardStateGroup> for OmRollForwardStateGroupWire {
    fn from(value: OmRollForwardStateGroup) -> Self {
        Self {
            opener: value.frame.opener().bytes(),
            count_prefix: value.frame.members().count().prefix(),
            declared_count: value.frame.members().count().declared_count(),
            id: value.id,
            section_link: value.section_link,
            ordinal: value.ordinal,
            source_offset: value.frame.offset(),
            rows: value
                .frame
                .map_rows(state_index_wire::OmRollForwardStateRowWire::from_row)
                .into_rows(),
            table_trailing_bytes: value.table_footer.bytes().to_vec(),
            source_entry: value.source_entry,
            table_end_offset: value.table_end_offset,
        }
    }
}

impl TryFrom<OmRollForwardStateGroupWire> for OmRollForwardStateGroup {
    type Error = String;
    fn try_from(wire: OmRollForwardStateGroupWire) -> Result<Self, Self::Error> {
        let count = match (wire.count_prefix, wire.declared_count) {
            (None, 0) => OperationStateGroupCount::Empty,
            (Some(1), count) => OperationStateGroupCount::Counted(count),
            _ => return Err("invalid operation-state group count encoding".to_string()),
        };
        let mut offset = wire
            .source_offset
            .checked_add(3 + u64::from(count.prefix().is_some()))
            .ok_or("source_offset: roll-forward header extent overflows")?;
        let members = StateGroupMembers::new(count, wire.rows)?.try_map_rows(|ordinal, row| {
            let row = row.into_row(ordinal, offset)?;
            offset = offset
                .checked_add(u64::from(row.byte_len()))
                .ok_or("source_offset: roll-forward row extent overflows")?;
            Ok::<_, String>(row)
        })?;
        let frame = OperationStateGroup::new(
            wire.source_offset,
            OperationStateGroupOpener::try_from(wire.opener)?,
            members,
        )?;
        Ok(Self {
            id: wire.id,
            section_link: wire.section_link,
            ordinal: wire.ordinal,
            frame,
            table_footer: GroupTableFooter::try_from(wire.table_trailing_bytes.as_slice())?,
            source_entry: wire.source_entry,
            table_end_offset: wire.table_end_offset,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::OmRollForwardStateGroup;

    #[test]
    fn wire_rows_follow_group_header_and_preceding_tokens() {
        let json = r#"{"id":"group","section_link":"section","ordinal":0,"opener":[1,0],"count_prefix":1,"declared_count":3,"rows":[{"List":{"ordinal":0,"object_index":1,"raw_object_index":[1],"position":1,"raw_position":[1],"source_offset":4}},{"Pair":{"ordinal":1,"tag":79,"first":2,"raw_first":[2],"second":3,"raw_second":[3],"source_offset":8}}],"table_trailing_bytes":[1,1],"source_entry":"om","source_offset":0,"table_end_offset":15}"#;
        let group: OmRollForwardStateGroup = serde_json::from_str(json).unwrap();
        assert_eq!(group.frame.end_offset(), 13);
        assert_eq!(serde_json::to_string(&group).unwrap(), json);
        let wire: serde_json::Value = serde_json::from_str(json).unwrap();
        for (index, kind) in [(0, "List"), (1, "Pair")] {
            let mut invalid = wire.clone();
            invalid["rows"][index][kind]["source_offset"] = 9.into();
            assert!(serde_json::from_value::<OmRollForwardStateGroup>(invalid)
                .unwrap_err()
                .to_string()
                .contains("rows.source_offset"));
        }
        let mut invalid = wire.clone();
        invalid["table_trailing_bytes"] = serde_json::json!([1]);
        assert!(serde_json::from_value::<OmRollForwardStateGroup>(invalid)
            .unwrap_err()
            .to_string()
            .contains("table_trailing_bytes"));
        let mut overflow = wire;
        overflow["source_offset"] = u64::MAX.into();
        assert!(serde_json::from_value::<OmRollForwardStateGroup>(overflow)
            .unwrap_err()
            .to_string()
            .contains("source_offset"));
    }
}
