// SPDX-License-Identifier: Apache-2.0
//! ACT registry records: tables, channels, entities and the root component and layout.

use cadmpeg_core::decode::u64_from_index;

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

use super::identity::{Located, NativeRecordId};
use super::mesh::DesignGuidText;
use super::references::DesignClassTag;

/// ACT component-link registry flag.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "u32", into = "u32")]
pub(crate) enum ActRegistryFlag {
    Off,
    On,
}

impl ActRegistryFlag {
    #[must_use]
    pub(crate) fn from_code(code: u32) -> Option<Self> {
        match code {
            0 => Some(Self::Off),
            1 => Some(Self::On),
            _ => None,
        }
    }

    #[must_use]
    pub(crate) fn code(self) -> u32 {
        match self {
            Self::Off => 0,
            Self::On => 1,
        }
    }
}

impl TryFrom<u32> for ActRegistryFlag {
    type Error = String;

    fn try_from(code: u32) -> Result<Self, Self::Error> {
        Self::from_code(code).ok_or_else(|| format!("act registry flag must be 0 or 1, not {code}"))
    }
}

impl From<ActRegistryFlag> for u32 {
    fn from(flag: ActRegistryFlag) -> Self {
        flag.code()
    }
}

/// Inline `ACTTable` membership, with optional transient parser location.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ActTableRow {
    record_index_offset: Option<u64>,
}
impl ActTableRow {
    pub(crate) fn new(record_index_offset: u64) -> Result<Self, String> {
        record_index_offset
            .checked_add(14)
            .ok_or("table_record_index_offset overflows table_entity_id_offset")?;
        Ok(Self {
            record_index_offset: Some(record_index_offset),
        })
    }
    fn entity_id_offset(&self) -> Option<u64> {
        self.record_index_offset
            .and_then(|offset| offset.checked_add(14))
    }
}

/// Opaque stream residue retained after the final ACT channel group. Actual
/// source bytes and location are transient.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct ActStreamResidue {
    frame: super::source_frame::SourceFrame,
    offset: Option<u64>,
}
impl ActStreamResidue {
    pub(crate) fn new(bytes: Vec<u8>, offset: u64) -> Result<Self, String> {
        if bytes.iter().all(|byte| *byte == 0) {
            return Err("channel_class_tail must contain non-padding bytes".into());
        }
        offset
            .checked_add(u64_from_index(bytes.len()))
            .ok_or("channel_class_tail_offset and channel_class_tail length overflow")?;
        Ok(Self {
            frame: bytes.into(),
            offset: Some(offset),
        })
    }
    #[cfg(test)]
    pub(crate) fn bytes(&self) -> Option<&[u8]> {
        self.frame.data()
    }
    pub(crate) fn offset(&self) -> Option<u64> {
        self.offset
    }
}

/// One typed channel value and its optional transient source location.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct ActChannel {
    pub(crate) value: DesignGuidText,
    pub(crate) offset: Option<u64>,
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct ActChannelGroup {
    record_index_offset: Option<u64>,
    entity_id_offset: Option<u64>,
    class_tag: DesignClassTag,
    channels: BTreeMap<String, ActChannel>,
    class_tail: Option<ActStreamResidue>,
}
fn validate_act_channel_name(name: &str) -> Result<(), String> {
    if name.is_empty() || name.len() > 128 || !name.is_ascii() {
        return Err("ACT channel name must contain 1 through 128 ASCII bytes".into());
    }
    Ok(())
}
impl ActChannelGroup {
    pub(crate) fn try_new(
        record_index_offset: u64,
        entity_id_offset: Option<u64>,
        class_tag: DesignClassTag,
        channels: BTreeMap<String, Located<DesignGuidText>>,
        class_tail: Option<ActStreamResidue>,
    ) -> Result<Self, String> {
        if channels.is_empty() || channels.len() > 8 {
            return Err("ACT channels must contain 1 through 8 entries".into());
        }
        if entity_id_offset.is_some_and(|offset| offset <= record_index_offset) {
            return Err("channel_entity_id_offset must follow channel_record_index_offset".into());
        }
        for (name, guid) in &channels {
            validate_act_channel_name(name)?;
            let end = guid
                .offset
                .checked_add(72)
                .ok_or("channel_guid_offsets overflow")?;
            if guid.offset <= record_index_offset
                || entity_id_offset.is_some_and(|offset| end > offset)
            {
                return Err(
                    "channel_guid_offsets must follow the record index and precede the entity key"
                        .into(),
                );
            }
            if class_tail
                .as_ref()
                .is_some_and(|tail| tail.offset().is_none_or(|offset| end > offset))
            {
                return Err("channel_guid_offsets must precede channel_class_tail_offset".into());
            }
        }
        if class_tail.as_ref().is_some_and(|tail| {
            tail.offset().is_none_or(|offset| {
                record_index_offset >= offset
                    || entity_id_offset.is_some_and(|entity| entity >= offset)
            })
        }) {
            return Err(
                "channel record and entity offsets must precede channel_class_tail_offset".into(),
            );
        }
        Ok(Self {
            record_index_offset: Some(record_index_offset),
            entity_id_offset,
            class_tag,
            channels: channels
                .into_iter()
                .map(|(name, value)| {
                    (
                        name,
                        ActChannel {
                            value: value.value,
                            offset: Some(value.offset),
                        },
                    )
                })
                .collect(),
            class_tail,
        })
    }
    pub(crate) fn channels(&self) -> &BTreeMap<String, ActChannel> {
        &self.channels
    }
}

/// One typed ACT change-version channel group and table membership.
#[derive(Debug, PartialEq, Deserialize)]
#[cfg_attr(not(test), derive(Clone))]
#[serde(try_from = "ActEntitySerde")]
pub(crate) struct ActEntity {
    id: NativeRecordId,
    record_index: u32,
    entity_id: String,
    table_row: Option<ActTableRow>,
    channel_group: ActChannelGroup,
}
#[cfg(test)]
std::thread_local! { static ACT_ENTITY_CLONE_COUNT:std::cell::Cell<usize>=const{std::cell::Cell::new(0)}; }
#[cfg(test)]
impl Clone for ActEntity {
    fn clone(&self) -> Self {
        ACT_ENTITY_CLONE_COUNT.with(|count| count.set(count.get() + 1));
        Self {
            id: self.id.clone(),
            record_index: self.record_index,
            entity_id: self.entity_id.clone(),
            table_row: self.table_row.clone(),
            channel_group: self.channel_group.clone(),
        }
    }
}
struct ChannelValues<'a>(&'a BTreeMap<String, ActChannel>);
impl Serialize for ChannelValues<'_> {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        use serde::ser::SerializeMap;
        let mut map = serializer.serialize_map(Some(self.0.len()))?;
        for (name, guid) in self.0 {
            map.serialize_entry(name, guid.value.as_str())?;
        }
        map.end()
    }
}
#[derive(Serialize)]
struct ActEntityWireRef<'a> {
    id: &'a str,
    record_index: u32,
    entity_id: &'a str,
    in_table: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    table_record_index_offset: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    channel_record_index_offset: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    channel_entity_id_offset: Option<u64>,
    channel_class_tag: &'a str,
    channels: ChannelValues<'a>,
    #[serde(skip_serializing_if = "BTreeMap::is_empty")]
    channel_guid_offsets: BTreeMap<&'a str, u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    class_tail_frame: Option<&'a super::source_frame::SourceFrame>,
}
impl Serialize for ActEntity {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        ActEntityWireRef {
            id: self.id.text(),
            record_index: self.record_index,
            entity_id: &self.entity_id,
            in_table: self.in_table(),
            table_record_index_offset: self.table_record_index_offset(),
            channel_record_index_offset: self.channel_record_index_offset(),
            channel_entity_id_offset: self.channel_entity_id_offset(),
            channel_class_tag: self.channel_class_tag(),
            channels: ChannelValues(&self.channel_group.channels),
            channel_guid_offsets: self
                .channel_group
                .channels
                .iter()
                .filter_map(|(name, channel)| Some((name.as_str(), channel.offset?)))
                .collect(),
            class_tail_frame: self
                .channel_group
                .class_tail
                .as_ref()
                .map(|tail| &tail.frame),
        }
        .serialize(serializer)
    }
}
impl ActEntity {
    pub(crate) fn id(&self) -> &String {
        self.id.text()
    }
    pub(crate) fn record_index(&self) -> u32 {
        self.record_index
    }
    pub(crate) fn stream(&self) -> &str {
        self.id.stream()
    }
    pub(crate) fn try_new(
        id: String,
        record_index: u32,
        entity_id: String,
        table_row: Option<ActTableRow>,
        channel_group: ActChannelGroup,
    ) -> Result<Self, String> {
        let id = NativeRecordId::try_new(id, "act-entity", record_index)?;
        if !crate::act::is_entity_key(&entity_id) {
            return Err("ACT entity_id must be a decimal segment_entity key".into());
        }
        if table_row.is_none() && channel_group.entity_id_offset.is_none() {
            return Err("channel_entity_id_offset is required without an ACTTable row".into());
        }
        Ok(Self {
            id,
            record_index,
            entity_id,
            table_row,
            channel_group,
        })
    }
    pub(crate) fn entity_id(&self) -> &str {
        &self.entity_id
    }
    pub(crate) fn try_set_entity_id(&mut self, entity_id: String) -> Result<(), String> {
        if !crate::act::is_entity_key(&entity_id) {
            return Err("ACT entity_id must be a decimal segment_entity key".into());
        }
        self.entity_id = entity_id;
        Ok(())
    }
    pub(crate) fn in_table(&self) -> bool {
        self.table_row.is_some()
    }
    pub(crate) fn channel_group(&self) -> &ActChannelGroup {
        &self.channel_group
    }
    pub(crate) fn set_channel_guid(
        &mut self,
        name: &str,
        value: DesignGuidText,
    ) -> Result<(), String> {
        self.channel_group
            .channels
            .get_mut(name)
            .ok_or("ACT channel does not exist")?
            .value = value;
        Ok(())
    }
    pub(crate) fn table_record_index_offset(&self) -> Option<u64> {
        self.table_row
            .as_ref()
            .and_then(|row| row.record_index_offset)
    }
    pub(crate) fn table_entity_id_offset(&self) -> Option<u64> {
        self.table_row
            .as_ref()
            .and_then(ActTableRow::entity_id_offset)
    }
    fn channel_record_index_offset(&self) -> Option<u64> {
        self.channel_group.record_index_offset
    }
    pub(crate) fn channel_entity_id_offset(&self) -> Option<u64> {
        self.channel_group.entity_id_offset
    }
    pub(crate) fn channel_class_tag(&self) -> &str {
        self.channel_group.class_tag.as_str()
    }
    pub(crate) fn retain_source(
        &self,
        fidelity: &mut cadmpeg_ir::SourceFidelity,
    ) -> Result<(), cadmpeg_core::CodecError> {
        for (name, offset) in [
            (
                "table_record_index_offset",
                self.table_record_index_offset(),
            ),
            ("table_entity_id_offset", self.table_entity_id_offset()),
            (
                "channel_record_index_offset",
                self.channel_record_index_offset(),
            ),
            ("channel_entity_id_offset", self.channel_entity_id_offset()),
        ] {
            if let Some(offset) = offset {
                fidelity
                    .metadata
                    .insert(format!("{}.{name}", self.id()), offset.to_string());
            }
        }
        for (name, guid) in &self.channel_group.channels {
            if let Some(offset) = guid.offset {
                fidelity.metadata.insert(
                    format!("{}.channel_guid_offsets.{name}", self.id()),
                    offset.to_string(),
                );
            }
        }
        if let Some(tail) = &self.channel_group.class_tail {
            if let Some(offset) = tail.offset() {
                fidelity.metadata.insert(
                    format!("{}.channel_class_tail_offset", self.id()),
                    offset.to_string(),
                );
            }
            tail.frame.retain(
                fidelity,
                &format!("{}:class-tail", self.id()),
                self.stream(),
                tail.offset(),
                [cadmpeg_ir::document::ArenaClass::Design].into(),
            )?;
        }
        Ok(())
    }
}
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[cfg_attr(test, derive(Serialize))]
#[serde(deny_unknown_fields)]
struct ActEntitySerde {
    id: String,
    record_index: u32,
    entity_id: String,
    in_table: bool,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_table_record_index_offset_u64"
    )]
    table_record_index_offset: Option<u64>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_channel_record_index_offset_u64"
    )]
    channel_record_index_offset: Option<u64>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_channel_entity_id_offset_u64"
    )]
    channel_entity_id_offset: Option<u64>,
    channel_class_tag: String,
    #[serde(deserialize_with = "cadmpeg_core::distinct_keys::btree_map")]
    channels: BTreeMap<String, String>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    channel_guid_offsets: BTreeMap<String, u64>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_class_tail_frame"
    )]
    class_tail_frame: Option<super::source_frame::SourceFrame>,
}
cadmpeg_core::named_optional_field!(
    deserialize_class_tail_frame,
    super::source_frame::SourceFrame,
    "class_tail_frame"
);
impl TryFrom<ActEntitySerde> for ActEntity {
    type Error = String;
    fn try_from(wire: ActEntitySerde) -> Result<Self, String> {
        if !crate::act::is_entity_key(&wire.entity_id) {
            return Err("ACT entity_id must be a decimal segment_entity key".into());
        }
        if wire.channels.is_empty() || wire.channels.len() > 8 {
            return Err("ACT channels must contain 1 through 8 entries".into());
        }
        if wire
            .channel_guid_offsets
            .keys()
            .any(|name| !wire.channels.contains_key(name))
        {
            return Err("channel_guid_offsets must name declared channels".into());
        }
        if wire.table_record_index_offset.is_some() && !wire.in_table {
            return Err("table_record_index_offset requires an ACTTable row".into());
        }
        let mut channels = BTreeMap::new();
        for (name, value) in wire.channels {
            validate_act_channel_name(&name)?;
            let offset = wire.channel_guid_offsets.get(&name).copied();
            channels.insert(
                name,
                ActChannel {
                    value: value.try_into()?,
                    offset,
                },
            );
        }
        if wire
            .class_tail_frame
            .as_ref()
            .is_some_and(super::source_frame::SourceFrame::is_empty)
        {
            return Err("ACT class tail evidence must describe nonempty bytes".into());
        }
        Ok(Self {
            id: NativeRecordId::try_new(wire.id, "act-entity", wire.record_index)?,
            record_index: wire.record_index,
            entity_id: wire.entity_id,
            table_row: wire.in_table.then_some(ActTableRow {
                record_index_offset: wire.table_record_index_offset,
            }),
            channel_group: ActChannelGroup {
                record_index_offset: wire.channel_record_index_offset,
                entity_id_offset: wire.channel_entity_id_offset,
                class_tag: wire.channel_class_tag.try_into()?,
                channels,
                class_tail: wire.class_tail_frame.map(|frame| ActStreamResidue {
                    frame,
                    offset: None,
                }),
            },
        })
    }
}
#[cfg(test)]
impl From<ActEntity> for ActEntitySerde {
    fn from(entity: ActEntity) -> Self {
        Self {
            id: entity.id.into_string(),
            record_index: entity.record_index,
            entity_id: entity.entity_id,
            in_table: entity.table_row.is_some(),
            table_record_index_offset: entity
                .table_row
                .as_ref()
                .and_then(|row| row.record_index_offset),
            channel_record_index_offset: entity.channel_group.record_index_offset,
            channel_entity_id_offset: entity.channel_group.entity_id_offset,
            channel_guid_offsets: entity
                .channel_group
                .channels
                .iter()
                .filter_map(|(name, channel)| Some((name.clone(), channel.offset?)))
                .collect(),
            channel_class_tag: entity.channel_group.class_tag.as_str().to_owned(),
            channels: entity
                .channel_group
                .channels
                .into_iter()
                .map(|(name, value)| (name, value.value.as_str().to_owned()))
                .collect(),
            class_tail_frame: entity.channel_group.class_tail.map(|tail| tail.frame),
        }
    }
}

/// One GUID in the ordered ACT stream-wide asset/change-version pool.
#[derive(Debug, PartialEq, Deserialize)]
#[cfg_attr(not(test), derive(Clone))]
#[serde(try_from = "ActGuidWire")]
pub(crate) struct ActGuid {
    /// Globally unique deterministic identifier for this native record.
    id: NativeRecordId,
    /// Byte offset of the UTF-16 length prefix in the ACT `BulkStream`.
    byte_offset: Option<u64>,
    /// Position in the pool in source order; does not assign a GUID to one table entry.
    pub(crate) ordinal: u32,
    /// The pooled GUID string.
    pub(crate) guid: DesignGuidText,
}

#[cfg(test)]
std::thread_local! {
    static ACT_GUID_CLONE_COUNT: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

#[cfg(test)]
impl Clone for ActGuid {
    fn clone(&self) -> Self {
        ACT_GUID_CLONE_COUNT.with(|count| count.set(count.get() + 1));
        Self {
            id: self.id.clone(),
            byte_offset: self.byte_offset,
            ordinal: self.ordinal,
            guid: self.guid.clone(),
        }
    }
}

#[derive(Serialize)]
struct ActGuidWireRef<'a> {
    id: &'a str,
    #[serde(skip_serializing_if = "Option::is_none")]
    byte_offset: Option<u64>,
    ordinal: u32,
    guid: &'a str,
}

impl Serialize for ActGuid {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        ActGuidWireRef {
            id: self.id.text(),
            byte_offset: self.byte_offset,
            ordinal: self.ordinal,
            guid: self.guid.as_str(),
        }
        .serialize(serializer)
    }
}

impl ActGuid {
    /// Returns the admitted native identity.
    pub(crate) fn id(&self) -> &String {
        self.id.text()
    }
    /// Returns the native stream encoded in the identity.
    pub(crate) fn stream(&self) -> &str {
        self.id.stream()
    }
    pub(crate) fn new(
        id: String,
        byte_offset: u64,
        ordinal: u32,
        guid: String,
    ) -> Result<Self, String> {
        let id = NativeRecordId::try_new(id, "act-guid", byte_offset)?;
        byte_offset
            .checked_add(4)
            .ok_or("ACT GUID byte_offset overflows guid_offset")?;
        Ok(Self {
            id,
            byte_offset: Some(byte_offset),
            ordinal,
            guid: guid.try_into()?,
        })
    }

    pub(crate) fn byte_offset(&self) -> Option<u64> {
        self.byte_offset
    }

    pub(crate) fn guid_offset(&self) -> Option<u64> {
        self.byte_offset.and_then(|offset| offset.checked_add(4))
    }
}

#[derive(Deserialize)]
#[cfg_attr(test, derive(Serialize))]
#[serde(deny_unknown_fields)]
struct ActGuidWire {
    id: String,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_byte_offset_u64"
    )]
    byte_offset: Option<u64>,
    ordinal: u32,
    guid: String,
}

impl TryFrom<ActGuidWire> for ActGuid {
    type Error = String;

    fn try_from(wire: ActGuidWire) -> Result<Self, Self::Error> {
        Ok(Self {
            id: NativeRecordId::from_content(wire.id, "act-guid")?,
            byte_offset: wire.byte_offset,
            ordinal: wire.ordinal,
            guid: wire.guid.try_into()?,
        })
    }
}

#[cfg(test)]
impl From<ActGuid> for ActGuidWire {
    fn from(guid: ActGuid) -> Self {
        Self {
            id: guid.id.into_string(),
            byte_offset: guid.byte_offset,
            ordinal: guid.ordinal,
            guid: guid.guid.into(),
        }
    }
}

/// One reference in the ACT table run between the GUID pool and channel registry.
#[derive(Debug, PartialEq, Deserialize)]
#[cfg_attr(not(test), derive(Clone))]
#[serde(try_from = "ActTableReferenceWire")]
pub(crate) struct ActTableReference {
    /// Globally unique deterministic identifier for this native record.
    id: NativeRecordId,
    /// Position in the counted reference run, in source order.
    pub(crate) ordinal: u32,
    /// Byte offset of the reference-presence marker in the ACT `BulkStream`.
    byte_offset: Option<u64>,
    /// Target ACT record index.
    pub(crate) target_record: u32,
}

#[cfg(test)]
std::thread_local! {
    static ACT_TABLE_REFERENCE_CLONE_COUNT: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

#[cfg(test)]
impl Clone for ActTableReference {
    fn clone(&self) -> Self {
        ACT_TABLE_REFERENCE_CLONE_COUNT.with(|count| count.set(count.get() + 1));
        Self {
            id: self.id.clone(),
            ordinal: self.ordinal,
            byte_offset: self.byte_offset,
            target_record: self.target_record,
        }
    }
}

#[derive(Serialize)]
struct ActTableReferenceWireRef<'a> {
    id: &'a str,
    ordinal: u32,
    target_record: u32,
}

impl Serialize for ActTableReference {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        ActTableReferenceWireRef {
            id: self.id.text(),
            ordinal: self.ordinal,
            target_record: self.target_record,
        }
        .serialize(serializer)
    }
}

impl ActTableReference {
    /// Returns the admitted native identity.
    pub(crate) fn id(&self) -> &String {
        self.id.text()
    }
    /// Returns the native stream encoded in the identity.
    pub(crate) fn stream(&self) -> &str {
        self.id.stream()
    }
    pub(crate) fn new(
        id: String,
        ordinal: u32,
        byte_offset: u64,
        target_record: u32,
    ) -> Result<Self, String> {
        let id = NativeRecordId::try_new(id, "act-table-reference", byte_offset)?;
        byte_offset
            .checked_add(1)
            .ok_or("ACT table reference byte_offset overflows target_record_offset")?;
        Ok(Self {
            id,
            ordinal,
            byte_offset: Some(byte_offset),
            target_record,
        })
    }

    pub(crate) fn byte_offset(&self) -> Option<u64> {
        self.byte_offset
    }

    pub(crate) fn target_record_offset(&self) -> Option<u64> {
        self.byte_offset.and_then(|offset| offset.checked_add(1))
    }

    /// Whether two references hold the same document content.
    pub(crate) fn content_eq(&self, other: &Self) -> bool {
        self.id == other.id
            && self.ordinal == other.ordinal
            && self.target_record == other.target_record
    }
}

#[derive(Deserialize)]
#[cfg_attr(test, derive(Serialize))]
#[serde(deny_unknown_fields)]
struct ActTableReferenceWire {
    id: String,
    ordinal: u32,
    target_record: u32,
}

impl TryFrom<ActTableReferenceWire> for ActTableReference {
    type Error = String;

    fn try_from(wire: ActTableReferenceWire) -> Result<Self, Self::Error> {
        Ok(Self {
            id: NativeRecordId::from_content(wire.id, "act-table-reference")?,
            ordinal: wire.ordinal,
            byte_offset: None,
            target_record: wire.target_record,
        })
    }
}

#[cfg(test)]
impl From<ActTableReference> for ActTableReferenceWire {
    fn from(reference: ActTableReference) -> Self {
        Self {
            id: reference.id.into_string(),
            ordinal: reference.ordinal,
            target_record: reference.target_record,
        }
    }
}

/// One named entry in the ACT table's stream-wide channel registry.
#[derive(Debug, PartialEq, Deserialize)]
#[cfg_attr(not(test), derive(Clone))]
#[serde(try_from = "ActRegistryChannelWire")]
pub(crate) struct ActRegistryChannel {
    id: NativeRecordId,
    pub(crate) ordinal: u32,
    byte_offset: Option<u64>,
    name: String,
    pub(crate) guid: DesignGuidText,
}

#[cfg(test)]
std::thread_local! {
    static ACT_REGISTRY_CHANNEL_CLONE_COUNT: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

#[cfg(test)]
impl Clone for ActRegistryChannel {
    fn clone(&self) -> Self {
        ACT_REGISTRY_CHANNEL_CLONE_COUNT.with(|count| count.set(count.get() + 1));
        Self {
            id: self.id.clone(),
            ordinal: self.ordinal,
            byte_offset: self.byte_offset,
            name: self.name.clone(),
            guid: self.guid.clone(),
        }
    }
}

#[derive(Serialize)]
struct ActRegistryChannelWireRef<'a> {
    id: &'a str,
    ordinal: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    byte_offset: Option<u64>,
    name: &'a str,
    guid: &'a str,
}

impl Serialize for ActRegistryChannel {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        ActRegistryChannelWireRef {
            id: self.id.text(),
            ordinal: self.ordinal,
            byte_offset: self.byte_offset,
            name: &self.name,
            guid: self.guid.as_str(),
        }
        .serialize(serializer)
    }
}

impl ActRegistryChannel {
    /// Returns the admitted native identity.
    pub(crate) fn id(&self) -> &String {
        self.id.text()
    }
    /// Returns the native stream encoded in the identity.
    pub(crate) fn stream(&self) -> &str {
        self.id.stream()
    }
    pub(crate) fn new(
        id: String,
        ordinal: u32,
        byte_offset: u64,
        name: String,
        guid: String,
    ) -> Result<Self, String> {
        let id = NativeRecordId::try_new(id, "act-registry-channel", byte_offset)?;
        validate_act_channel_name(&name)?;
        byte_offset
            .checked_add(8 + u64_from_index(name.len()))
            .ok_or("ACT registry offset overflow")?;
        Ok(Self {
            id,
            ordinal,
            byte_offset: Some(byte_offset),
            name,
            guid: guid.try_into()?,
        })
    }
    pub(crate) fn byte_offset(&self) -> Option<u64> {
        self.byte_offset
    }
    pub(crate) fn name(&self) -> &str {
        &self.name
    }
    pub(crate) fn name_offset(&self) -> Option<u64> {
        self.byte_offset.and_then(|offset| offset.checked_add(4))
    }
    pub(crate) fn guid_offset(&self) -> Option<u64> {
        self.byte_offset
            .and_then(|offset| offset.checked_add(8 + u64_from_index(self.name.len())))
    }
}

#[derive(Deserialize)]
#[cfg_attr(test, derive(Serialize))]
#[serde(deny_unknown_fields)]
struct ActRegistryChannelWire {
    id: String,
    ordinal: u32,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_byte_offset_u64"
    )]
    byte_offset: Option<u64>,
    name: String,
    guid: String,
}

impl TryFrom<ActRegistryChannelWire> for ActRegistryChannel {
    type Error = String;
    fn try_from(wire: ActRegistryChannelWire) -> Result<Self, Self::Error> {
        validate_act_channel_name(&wire.name)?;
        Ok(Self {
            id: NativeRecordId::from_content(wire.id, "act-registry-channel")?,
            ordinal: wire.ordinal,
            byte_offset: wire.byte_offset,
            name: wire.name,
            guid: wire.guid.try_into()?,
        })
    }
}

#[cfg(test)]
impl From<ActRegistryChannel> for ActRegistryChannelWire {
    fn from(channel: ActRegistryChannel) -> Self {
        Self {
            id: channel.id.into_string(),
            ordinal: channel.ordinal,
            byte_offset: channel.byte_offset,
            name: channel.name,
            guid: channel.guid.as_str().into(),
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct ActComponentReferences {
    pub(crate) instance_root: u32,
    pub(crate) tracked_entity: u32,
    pub(crate) components_root: u32,
}

/// ACT link from a tracked Design entity to the instance/component registries.
#[derive(Debug, PartialEq, Deserialize)]
#[cfg_attr(not(test), derive(Clone))]
#[serde(try_from = "ActComponentLinkWire")]
pub(crate) struct ActComponentLink {
    /// Globally unique deterministic identifier for this native record.
    id: NativeRecordId,
    /// Index of this record within the ACT `BulkStream`.
    pub(crate) record_index: u32,
    /// Source per-file dynamic three-digit ASCII class tag naming this record's type.
    class_tag: DesignClassTag,
    pub(crate) references: ActComponentReferences,
    /// Source counter/registry flag; 0 and 1 are both valid.
    pub(crate) registry_flag: ActRegistryFlag,
    /// Checked source layout and the two variable-length strings.
    layout: ActComponentLayout,
}

#[cfg(test)]
std::thread_local! {
    static ACT_COMPONENT_LINK_CLONE_COUNT: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

#[cfg(test)]
impl Clone for ActComponentLink {
    fn clone(&self) -> Self {
        ACT_COMPONENT_LINK_CLONE_COUNT.with(|count| count.set(count.get() + 1));
        Self {
            id: self.id.clone(),
            record_index: self.record_index,
            class_tag: self.class_tag.clone(),
            references: self.references.clone(),
            registry_flag: self.registry_flag,
            layout: self.layout.clone(),
        }
    }
}

#[derive(Serialize)]
struct ActComponentLinkWireRef<'a> {
    id: &'a str,
    #[serde(skip_serializing_if = "Option::is_none")]
    byte_offset: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    padding: Option<u64>,
    record_index: u32,
    class_tag: &'a str,
    instance_root_record: u32,
    tracked_entity_record: u32,
    components_root_record: u32,
    registry_flag: ActRegistryFlag,
    entity_id: &'a str,
    display_name: &'a str,
}
impl Serialize for ActComponentLink {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        ActComponentLinkWireRef {
            id: self.id.text(),
            byte_offset: self.layout.byte_offset(),
            padding: self.layout.source.map(|source| source.padding),
            record_index: self.record_index,
            class_tag: self.class_tag.as_str(),
            instance_root_record: self.references.instance_root,
            tracked_entity_record: self.references.tracked_entity,
            components_root_record: self.references.components_root,
            registry_flag: self.registry_flag,
            entity_id: self.layout.entity_id(),
            display_name: self.layout.display_name(),
        }
        .serialize(serializer)
    }
}

impl ActComponentLink {
    /// Returns the admitted native identity.
    pub(crate) fn id(&self) -> &String {
        self.id.text()
    }
    /// Whether the link tracks the document root entity, record 3.
    pub(crate) fn tracks_document_root(&self) -> bool {
        self.references.tracked_entity == 3
    }
    /// Returns the ACT component-link source layout.
    pub(crate) fn layout(&self) -> &ActComponentLayout {
        &self.layout
    }
    /// Returns the native stream encoded in the identity.
    pub(crate) fn stream(&self) -> &str {
        self.id.stream()
    }
    /// Admits a record whose identity matches its source location.
    pub(crate) fn try_new(
        id: String,
        record_index: u32,
        class_tag: DesignClassTag,
        references: ActComponentReferences,
        registry_flag: ActRegistryFlag,
        layout: ActComponentLayout,
    ) -> Result<Self, String> {
        let id = NativeRecordId::try_new(
            id,
            "act-component-link",
            layout
                .byte_offset()
                .ok_or("source ACT component link requires an actual byte_offset")?,
        )?;
        Ok(Self {
            id,
            record_index,
            class_tag,
            references,
            registry_flag,
            layout,
        })
    }
    /// Changes component-link strings without changing the identity offset.
    pub(crate) fn try_set_strings(
        &mut self,
        entity_id: String,
        display_name: String,
    ) -> Result<(), String> {
        self.layout = self.layout.with_strings(entity_id, display_name)?;
        Ok(())
    }
}

#[derive(Deserialize)]
#[cfg_attr(test, derive(Serialize))]
#[serde(deny_unknown_fields)]
struct ActComponentLinkWire {
    id: String,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_byte_offset_u64"
    )]
    byte_offset: Option<u64>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_padding_u64"
    )]
    padding: Option<u64>,
    record_index: u32,
    class_tag: String,
    instance_root_record: u32,
    tracked_entity_record: u32,
    components_root_record: u32,
    registry_flag: ActRegistryFlag,
    entity_id: String,
    display_name: String,
}
impl TryFrom<ActComponentLinkWire> for ActComponentLink {
    type Error = String;
    fn try_from(wire: ActComponentLinkWire) -> Result<Self, String> {
        Ok(Self {
            id: NativeRecordId::from_content(wire.id, "act-component-link")?,
            record_index: wire.record_index,
            class_tag: wire
                .class_tag
                .try_into()
                .map_err(|error| format!("class_tag: {error}"))?,
            references: ActComponentReferences {
                instance_root: wire.instance_root_record,
                tracked_entity: wire.tracked_entity_record,
                components_root: wire.components_root_record,
            },
            registry_flag: wire.registry_flag,
            layout: match (wire.byte_offset, wire.padding) {
                (Some(byte_offset), Some(padding)) => ActComponentLayout::new(
                    byte_offset,
                    wire.entity_id,
                    wire.display_name,
                    padding,
                )?,
                (None, None) => {
                    ActComponentLayout::from_content(wire.entity_id, wire.display_name)?
                }
                _ => return Err("byte_offset and padding must occur together".into()),
            },
        })
    }
}
#[cfg(test)]
impl From<ActComponentLink> for ActComponentLinkWire {
    fn from(root: ActComponentLink) -> Self {
        Self {
            id: root.id.into_string(),
            byte_offset: root.layout.byte_offset(),
            padding: root.layout.source.map(|source| source.padding),
            record_index: root.record_index,
            class_tag: root.class_tag.into(),
            instance_root_record: root.references.instance_root,
            tracked_entity_record: root.references.tracked_entity,
            components_root_record: root.references.components_root,
            registry_flag: root.registry_flag,
            entity_id: root.layout.entity_id,
            display_name: root.layout.display_name,
        }
    }
}

/// Source extent of an ACT component link. Offsets follow its fixed grammar.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct ActComponentLayout {
    source: Option<ActRootSource>,
    entity_id: String,
    display_name: String,
}
#[derive(Debug, Clone, Copy, PartialEq)]
struct ActRootSource {
    byte_offset: u64,
    padding: u64,
}

impl ActComponentLayout {
    pub(crate) fn new(
        byte_offset: u64,
        entity_id: String,
        display_name: String,
        padding: u64,
    ) -> Result<Self, String> {
        if !crate::act::is_entity_key(&entity_id) {
            return Err("entity_id must be an ACT entity key".into());
        }
        if padding > 8 {
            return Err(
                "components_root_record_offset requires zero through eight padding bytes".into(),
            );
        }
        let string_bytes = entity_id
            .encode_utf16()
            .count()
            .checked_add(display_name.encode_utf16().count())
            .and_then(|length| u64::try_from(length).ok())
            .and_then(|length| length.checked_mul(2))
            .ok_or("ACT component-link string lengths overflow")?;
        byte_offset
            .checked_add(56)
            .and_then(|offset| offset.checked_add(string_bytes))
            .and_then(|offset| offset.checked_add(padding))
            .ok_or("components_root_record_offset overflows ACT component-link layout")?;
        Ok(Self {
            source: Some(ActRootSource {
                byte_offset,
                padding,
            }),
            entity_id,
            display_name,
        })
    }

    fn from_content(entity_id: String, display_name: String) -> Result<Self, String> {
        if !crate::act::is_entity_key(&entity_id) {
            return Err("entity_id must be an ACT entity key".into());
        }
        Ok(Self {
            source: None,
            entity_id,
            display_name,
        })
    }
    fn with_strings(&self, entity_id: String, display_name: String) -> Result<Self, String> {
        match self.source {
            Some(source) => Self::new(source.byte_offset, entity_id, display_name, source.padding),
            None => Self::from_content(entity_id, display_name),
        }
    }
    fn byte_offset(&self) -> Option<u64> {
        self.source.map(|source| source.byte_offset)
    }
    pub(crate) fn entity_id(&self) -> &str {
        &self.entity_id
    }
    pub(crate) fn display_name(&self) -> &str {
        &self.display_name
    }
    fn record_index_offset(&self) -> Option<u64> {
        self.byte_offset()?.checked_add(7)
    }
    pub(crate) fn instance_root_record_offset(&self) -> Option<u64> {
        self.byte_offset()?.checked_add(22)
    }
    pub(crate) fn entity_id_offset(&self) -> Option<u64> {
        self.byte_offset()?.checked_add(36)
    }
    pub(crate) fn tracked_entity_record_offset(&self) -> Option<u64> {
        self.entity_id_offset()?
            .checked_add(u64_from_index(self.entity_id.encode_utf16().count()).checked_mul(2)?)?
            .checked_add(1)
    }
    pub(crate) fn registry_flag_offset(&self) -> Option<u64> {
        self.tracked_entity_record_offset()?.checked_add(10)
    }
    pub(crate) fn display_name_offset(&self) -> Option<u64> {
        self.registry_flag_offset()?.checked_add(8)
    }
    pub(crate) fn components_root_record_offset(&self) -> Option<u64> {
        self.display_name_offset()?
            .checked_add(u64_from_index(self.display_name.encode_utf16().count()).checked_mul(2)?)?
            .checked_add(self.source?.padding)?
            .checked_add(1)
    }
}
impl ActComponentLink {
    pub(crate) fn content_eq(&self, other: &Self) -> bool {
        self.id == other.id
            && self.record_index == other.record_index
            && self.class_tag == other.class_tag
            && self.references == other.references
            && self.registry_flag == other.registry_flag
            && self.layout.entity_id == other.layout.entity_id
            && self.layout.display_name == other.layout.display_name
    }
    pub(crate) fn retain_source(&self, fidelity: &mut cadmpeg_ir::SourceFidelity) {
        let layout = &self.layout;
        for (name, value) in [
            ("byte_offset", layout.byte_offset()),
            ("record_index_offset", layout.record_index_offset()),
            (
                "instance_root_record_offset",
                layout.instance_root_record_offset(),
            ),
            (
                "tracked_entity_record_offset",
                layout.tracked_entity_record_offset(),
            ),
            (
                "components_root_record_offset",
                layout.components_root_record_offset(),
            ),
            ("registry_flag_offset", layout.registry_flag_offset()),
            ("entity_id_offset", layout.entity_id_offset()),
            ("display_name_offset", layout.display_name_offset()),
            ("padding", layout.source.map(|source| source.padding)),
        ] {
            if let Some(value) = value {
                fidelity
                    .metadata
                    .insert(format!("{}.{name}", self.id()), value.to_string());
            }
        }
    }
}

#[cfg(test)]
mod tests;

// Each optional source offset below names itself in whatever it refuses.
cadmpeg_core::named_optional_field!(deserialize_byte_offset_u64, u64, "byte_offset");
cadmpeg_core::named_optional_field!(
    deserialize_channel_entity_id_offset_u64,
    u64,
    "channel_entity_id_offset"
);
cadmpeg_core::named_optional_field!(
    deserialize_channel_record_index_offset_u64,
    u64,
    "channel_record_index_offset"
);
cadmpeg_core::named_optional_field!(deserialize_padding_u64, u64, "padding");
cadmpeg_core::named_optional_field!(
    deserialize_table_record_index_offset_u64,
    u64,
    "table_record_index_offset"
);
