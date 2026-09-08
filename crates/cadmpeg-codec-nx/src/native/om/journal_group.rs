// SPDX-License-Identifier: Apache-2.0
//! Native state-journal group metadata and flat wire admission.

use crate::om::journal_group::JournalGroup;
use crate::om::state_journal::JournalRow;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "Wire", into = "Wire")]
pub(crate) struct OmOperationStateJournalGroup {
    pub(crate) id: String,
    pub(crate) section_link: String,
    pub(crate) ordinal: u32,
    pub(crate) frame: JournalGroup,
    pub(crate) source_entry: String,
}

#[derive(Serialize, Deserialize)]
struct Wire {
    id: String,
    section_link: String,
    ordinal: u32,
    selector: [u8; 2],
    rows: Vec<JournalRow>,
    source_entry: String,
    source_offset: u64,
    end_offset: u64,
}

impl From<OmOperationStateJournalGroup> for Wire {
    fn from(group: OmOperationStateJournalGroup) -> Self {
        Self {
            id: group.id,
            section_link: group.section_link,
            ordinal: group.ordinal,
            selector: group.frame.selector(),
            rows: group.frame.rows().iter().copied().collect(),
            source_entry: group.source_entry,
            source_offset: group.frame.offset(),
            end_offset: group.frame.end_offset(),
        }
    }
}

impl TryFrom<Wire> for OmOperationStateJournalGroup {
    type Error = String;
    fn try_from(wire: Wire) -> Result<Self, Self::Error> {
        let frame = JournalGroup::new(wire.selector, wire.source_offset, wire.rows)?;
        if wire.end_offset != frame.end_offset() {
            return Err("end_offset: disagrees with final journal row".into());
        }
        Ok(Self {
            id: wire.id,
            section_link: wire.section_link,
            ordinal: wire.ordinal,
            frame,
            source_entry: wire.source_entry,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::OmOperationStateJournalGroup;

    #[test]
    fn journal_group_wire_preserves_both_header_widths() {
        for start in [4u64, 5] {
            let end = start + 11;
            let json = format!(
                r#"{{"id":"group","section_link":"section","ordinal":0,"selector":[1,2],"rows":[{{"timestamp":0,"value_marker":160,"value":0,"raw_value":[160,0,0],"schema_id":0,"raw_schema_id":[0],"state_ordinal":0,"raw_state_ordinal":[0],"source_offset":{start},"end_offset":{end}}}],"source_entry":"om","source_offset":0,"end_offset":{end}}}"#
            );
            let group: OmOperationStateJournalGroup = serde_json::from_str(&json).unwrap();
            assert_eq!(serde_json::to_string(&group).unwrap(), json);
            let wire: serde_json::Value = serde_json::from_str(&json).unwrap();
            let mut empty = wire.clone();
            empty["rows"] = serde_json::json!([]);
            assert!(
                serde_json::from_value::<OmOperationStateJournalGroup>(empty)
                    .unwrap_err()
                    .to_string()
                    .contains("rows")
            );
            let mut header = wire.clone();
            header["source_offset"] = start.into();
            assert!(
                serde_json::from_value::<OmOperationStateJournalGroup>(header)
                    .unwrap_err()
                    .to_string()
                    .contains("source_offset")
            );
            let mut end_mismatch = wire.clone();
            end_mismatch["end_offset"] = (end + 1).into();
            assert!(
                serde_json::from_value::<OmOperationStateJournalGroup>(end_mismatch)
                    .unwrap_err()
                    .to_string()
                    .contains("end_offset")
            );
            let mut gap = wire;
            let mut second = gap["rows"][0].clone();
            second["source_offset"] = (end + 1).into();
            second["end_offset"] = (end + 12).into();
            gap["rows"].as_array_mut().unwrap().push(second);
            gap["end_offset"] = (end + 12).into();
            assert!(serde_json::from_value::<OmOperationStateJournalGroup>(gap)
                .unwrap_err()
                .to_string()
                .contains("rows.source_offset"));
        }
    }
}
