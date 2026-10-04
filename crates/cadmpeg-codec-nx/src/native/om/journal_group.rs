// SPDX-License-Identifier: Apache-2.0
//! Native state-journal group metadata and flat wire admission.

use crate::om::journal_group::JournalGroup;
use crate::om::state_journal::JournalRow;
use serde::ser::SerializeSeq;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(try_from = "Wire")]
pub(in crate::native) struct OmOperationStateJournalGroup {
    pub(in crate::native) id: String,
    pub(in crate::native) section_link: String,
    pub(in crate::native) ordinal: u32,
    pub(in crate::native) frame: JournalGroup,
    pub(in crate::native) source_entry: String,
}

struct JournalRows<'a>(&'a JournalGroup);

impl Serialize for JournalRows<'_> {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut rows = serializer.serialize_seq(Some(self.0.rows().len()))?;
        for row in self.0.rows().iter() {
            rows.serialize_element(row)?;
        }
        rows.end()
    }
}

#[derive(Serialize)]
struct JournalGroupRef<'a> {
    id: &'a str,
    section_link: &'a str,
    ordinal: u32,
    selector: [u8; 2],
    rows: JournalRows<'a>,
    source_entry: &'a str,
    source_offset: u64,
    end_offset: u64,
}

impl Serialize for OmOperationStateJournalGroup {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        JournalGroupRef {
            id: &self.id,
            section_link: &self.section_link,
            ordinal: self.ordinal,
            selector: self.frame.selector(),
            rows: JournalRows(&self.frame),
            source_entry: &self.source_entry,
            source_offset: self.frame.offset(),
            end_offset: self.frame.end_offset(),
        }
        .serialize(serializer)
    }
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

#[cfg(test)]
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
            assert_eq!(
                serde_json::to_vec(&group).unwrap(),
                serde_json::to_vec(&super::Wire::from(group.clone())).unwrap()
            );
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

    #[test]
    fn journal_group_native_limit_refuses_before_row_copy() {
        let json = r#"{"id":"nx:om:state-journal-group#0","section_link":"section","ordinal":0,"selector":[1,2],"rows":[{"timestamp":0,"value_marker":160,"value":0,"raw_value":[160,0,0],"schema_id":0,"raw_schema_id":[0],"state_ordinal":0,"raw_state_ordinal":[0],"source_offset":4,"end_offset":15}],"source_entry":"om","source_offset":0,"end_offset":15}"#;
        let group: OmOperationStateJournalGroup = serde_json::from_str(json).unwrap();
        cadmpeg_test_support::native_serialization::assert_native_limit(
            &group,
            serde_json::from_str::<serde_json::Value>(json).unwrap(),
        );
    }
}
