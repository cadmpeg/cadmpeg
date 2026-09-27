// SPDX-License-Identifier: Apache-2.0
//! Native roll-forward metadata and flat wire admission.

use super::state_index_wire;
use crate::om::roll_forward::{GroupTableFooter, OperationStateGroup, OperationStateGroupRow};
use crate::om::state_group::{
    OperationStateGroupCount, OperationStateGroupOpener, StateGroupMembers,
};
use serde::ser::SerializeSeq;
use serde::{Deserialize, Serialize};

/// One counted `m_rollForwardStates` group from a feature-history section.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(try_from = "OmRollForwardStateGroupWire")]
pub(in crate::native) struct OmRollForwardStateGroup {
    /// Globally unique group identity.
    pub(in crate::native) id: String,
    pub(in crate::native) frame: OperationStateGroup<u64>,
}

#[derive(Serialize)]
enum RollForwardRowRef<'a> {
    List {
        ordinal: u32,
        object_index: u32,
        raw_object_index: &'a [u8],
        position: u32,
        raw_position: &'a [u8],
        source_offset: u64,
    },
    Pair {
        ordinal: u32,
        tag: crate::om::discriminators::OperationStatePairTag,
        first: u32,
        raw_first: &'a [u8],
        second: u32,
        raw_second: &'a [u8],
        source_offset: u64,
    },
}

impl<'a> RollForwardRowRef<'a> {
    fn from_row(ordinal: u32, source_offset: u64, row: &'a OperationStateGroupRow) -> Self {
        match row {
            OperationStateGroupRow::List {
                object_index,
                position,
            } => Self::List {
                ordinal,
                object_index: object_index.value(),
                raw_object_index: object_index.raw(),
                position: position.value(),
                raw_position: position.raw(),
                source_offset,
            },
            OperationStateGroupRow::Pair { tag, first, second } => Self::Pair {
                ordinal,
                tag: *tag,
                first: first.value(),
                raw_first: first.raw(),
                second: second.value(),
                raw_second: second.raw(),
                source_offset,
            },
        }
    }
}

struct RollForwardRows<'a>(&'a OperationStateGroup<u64>);

impl Serialize for RollForwardRows<'_> {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let rows = self.0.members().rows();
        let mut sequence = serializer.serialize_seq(Some(rows.len()))?;
        let mut offset =
            self.0.offset() + 3 + u64::from(self.0.members().count().prefix().is_some());
        for (ordinal, row) in rows.iter().enumerate() {
            let ordinal = u32::try_from(ordinal).map_err(serde::ser::Error::custom)?;
            sequence.serialize_element(&RollForwardRowRef::from_row(ordinal, offset, row))?;
            offset += u64::from(row.byte_len());
        }
        sequence.end()
    }
}

impl Serialize for OmRollForwardStateGroup {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        RollForwardGroupWireRef {
            id: &self.id,
            opener: self.frame.opener().bytes(),
            count_prefix: self.frame.members().count().prefix(),
            declared_count: self.frame.members().count().declared_count(),
            rows: RollForwardRows(&self.frame),
            source_offset: self.frame.offset(),
        }
        .serialize(serializer)
    }
}

#[derive(Serialize)]
struct RollForwardGroupWireRef<'a> {
    id: &'a str,
    opener: [u8; 2],
    count_prefix: Option<u8>,
    declared_count: u8,
    rows: RollForwardRows<'a>,
    source_offset: u64,
}

#[derive(Serialize, Deserialize)]
struct OmRollForwardStateGroupWire {
    /// Globally unique group identity.
    id: String,
    /// Exact two-byte group opener.
    opener: [u8; 2],
    /// Whether the count used the nonempty `01 count` form.
    count_prefix: Option<u8>,
    /// Serialized member count including the implicit owner slot.
    declared_count: u8,
    /// Ordered typed rows in the group.
    rows: Vec<state_index_wire::OmRollForwardStateRowWire>,
    /// Absolute file offset of the group opener.
    source_offset: u64,
}

#[cfg(test)]
impl From<OmRollForwardStateGroup> for OmRollForwardStateGroupWire {
    fn from(value: OmRollForwardStateGroup) -> Self {
        Self {
            opener: value.frame.opener().bytes(),
            count_prefix: value.frame.members().count().prefix(),
            declared_count: value.frame.members().count().declared_count(),
            id: value.id,
            source_offset: value.frame.offset(),
            rows: value
                .frame
                .map_rows(state_index_wire::OmRollForwardStateRowWire::from_row)
                .into_rows(),
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
        Ok(Self { id: wire.id, frame })
    }
}

/// Roll-forward groups with one set of table facts.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(try_from = "OmRollForwardStateTableWire")]
pub(in crate::native) struct OmRollForwardStateTable {
    section_link: String,
    source_entry: String,
    table_footer: GroupTableFooter,
    table_end_offset: u64,
    groups: Vec<OmRollForwardStateGroup>,
}

#[derive(Serialize)]
struct RollForwardTableRef<'a> {
    section_link: &'a str,
    source_entry: &'a str,
    table_trailing_bytes: &'static [u8],
    table_end_offset: u64,
    groups: &'a [OmRollForwardStateGroup],
}

impl Serialize for OmRollForwardStateTable {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        RollForwardTableRef {
            section_link: &self.section_link,
            source_entry: &self.source_entry,
            table_trailing_bytes: self.table_footer.bytes(),
            table_end_offset: self.table_end_offset,
            groups: &self.groups,
        }
        .serialize(serializer)
    }
}

#[derive(Serialize, Deserialize)]
struct OmRollForwardStateTableWire {
    /// Owning feature-history section link.
    section_link: String,
    /// Directory entry containing the feature-history section.
    source_entry: String,
    /// Exact bytes between the final group and the counter-map boundary.
    table_trailing_bytes: Vec<u8>,
    /// Absolute file offset of the counter-map boundary.
    table_end_offset: u64,
    /// Ordered groups of the table.
    groups: Vec<OmRollForwardStateGroup>,
}

impl OmRollForwardStateTable {
    pub(super) fn from_frames(
        section_ordinal: usize,
        section_link: &str,
        source_entry: &str,
        table_footer: GroupTableFooter,
        table_end_offset: u64,
        frames: Vec<OperationStateGroup<u64>>,
    ) -> Result<Self, &'static str> {
        let groups = frames
            .into_iter()
            .enumerate()
            .map(|(ordinal, frame)| {
                let ordinal = u32::try_from(ordinal)
                    .map_err(|_| "ordinal exceeds the roll-forward group range")?;
                Ok(OmRollForwardStateGroup {
                    id: format!(
                        "nx:feature-history:roll-forward-state-group#{section_ordinal:010}-{ordinal:010}"
                    ),
                    frame,
                })
            })
            .collect::<Result<Vec<_>, &'static str>>()?;
        Ok(Self {
            section_link: section_link.to_owned(),
            source_entry: source_entry.to_owned(),
            table_footer,
            table_end_offset,
            groups,
        })
    }

    pub(in crate::native) fn groups(&self) -> &[OmRollForwardStateGroup] {
        &self.groups
    }

    #[cfg(test)]
    pub(super) fn table_footer(&self) -> GroupTableFooter {
        self.table_footer
    }

    #[cfg(test)]
    pub(super) fn table_end_offset(&self) -> u64 {
        self.table_end_offset
    }
}

#[cfg(test)]
impl From<OmRollForwardStateTable> for OmRollForwardStateTableWire {
    fn from(table: OmRollForwardStateTable) -> Self {
        Self {
            section_link: table.section_link,
            source_entry: table.source_entry,
            table_trailing_bytes: table.table_footer.bytes().to_vec(),
            table_end_offset: table.table_end_offset,
            groups: table.groups,
        }
    }
}

impl TryFrom<OmRollForwardStateTableWire> for OmRollForwardStateTable {
    type Error = String;

    fn try_from(wire: OmRollForwardStateTableWire) -> Result<Self, Self::Error> {
        Ok(Self {
            section_link: wire.section_link,
            source_entry: wire.source_entry,
            table_footer: GroupTableFooter::try_from(wire.table_trailing_bytes.as_slice())?,
            table_end_offset: wire.table_end_offset,
            groups: wire.groups,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::OmRollForwardStateGroup;

    #[test]
    fn table_wire_carries_the_table_facts_once() {
        let group = serde_json::json!({
            "id": "group", "opener": [1, 0], "count_prefix": null, "declared_count": 0,
            "rows": [], "source_offset": 0
        });
        let wire = serde_json::json!({
            "section_link": "section",
            "source_entry": "om",
            "table_trailing_bytes": [],
            "table_end_offset": 8,
            "groups": [group.clone(), group],
        });
        let table: super::OmRollForwardStateTable = serde_json::from_value(wire.clone()).unwrap();
        assert_eq!(table.groups().len(), 2);
        assert_eq!(serde_json::to_value(&table).unwrap(), wire);
        assert_eq!(
            serde_json::to_vec(&table).unwrap(),
            serde_json::to_vec(&super::OmRollForwardStateTableWire::from(table.clone())).unwrap()
        );

        let mut invalid = wire;
        invalid["table_trailing_bytes"] = serde_json::json!([1]);
        assert!(
            serde_json::from_value::<super::OmRollForwardStateTable>(invalid)
                .unwrap_err()
                .to_string()
                .contains("table_trailing_bytes")
        );
    }

    #[test]
    fn wire_rows_follow_group_header_and_preceding_tokens() {
        let json = r#"{"id":"group","opener":[1,0],"count_prefix":1,"declared_count":3,"rows":[{"List":{"ordinal":0,"object_index":1,"raw_object_index":[1],"position":1,"raw_position":[1],"source_offset":4}},{"Pair":{"ordinal":1,"tag":79,"first":2,"raw_first":[2],"second":3,"raw_second":[3],"source_offset":8}}],"source_offset":0}"#;
        let group: OmRollForwardStateGroup = serde_json::from_str(json).unwrap();
        assert_eq!(group.frame.end_offset(), 13);
        assert_eq!(serde_json::to_string(&group).unwrap(), json);
        assert_eq!(
            serde_json::to_vec(&group).unwrap(),
            serde_json::to_vec(&super::OmRollForwardStateGroupWire::from(group.clone())).unwrap()
        );
        let wire: serde_json::Value = serde_json::from_str(json).unwrap();
        for (index, kind) in [(0, "List"), (1, "Pair")] {
            let mut invalid = wire.clone();
            invalid["rows"][index][kind]["source_offset"] = 9.into();
            assert!(serde_json::from_value::<OmRollForwardStateGroup>(invalid)
                .unwrap_err()
                .to_string()
                .contains("rows.source_offset"));
        }
        let mut overflow = wire;
        overflow["source_offset"] = u64::MAX.into();
        assert!(serde_json::from_value::<OmRollForwardStateGroup>(overflow)
            .unwrap_err()
            .to_string()
            .contains("source_offset"));
    }

    #[test]
    fn roll_forward_group_native_limit_refuses_before_row_copy() {
        let json = r#"{"id":"nx:om:roll-forward-state-group#0","opener":[1,0],"count_prefix":1,"declared_count":2,"rows":[{"List":{"ordinal":0,"object_index":1,"raw_object_index":[1],"position":1,"raw_position":[1],"source_offset":4}}],"source_offset":0}"#;
        let group: OmRollForwardStateGroup = serde_json::from_str(json).unwrap();
        cadmpeg_test_support::native_serialization::assert_native_limit(
            &group,
            serde_json::from_str::<serde_json::Value>(json).unwrap(),
        );
    }

    #[test]
    fn roll_forward_table_native_limit_refuses_before_group_copy() {
        #[derive(serde::Serialize)]
        struct Record<'a> {
            id: &'static str,
            #[serde(flatten)]
            table: &'a super::OmRollForwardStateTable,
        }
        let wire = serde_json::json!({
            "id": "nx:om:roll-forward-table#0",
            "section_link": "section", "source_entry": "om",
            "table_trailing_bytes": [], "table_end_offset": 8,
            "groups": [{"id": "nx:om:roll-forward-state-group#0", "opener": [1, 0],
                "count_prefix": null, "declared_count": 0,
                "rows": [], "source_offset": 0}]
        });
        let table: super::OmRollForwardStateTable = serde_json::from_value(wire.clone()).unwrap();
        cadmpeg_test_support::native_serialization::assert_native_limit(
            &Record {
                id: "nx:om:roll-forward-table#0",
                table: &table,
            },
            wire,
        );
    }
}
