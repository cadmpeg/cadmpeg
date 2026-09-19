// SPDX-License-Identifier: Apache-2.0
//! Design body members, bounds, bindings and visibility.

use super::mesh::DesignMeshSceneBounds;
use cadmpeg_ir::ids::BodyId;
use cadmpeg_ir::math::Point3;
use serde::{Deserialize, Serialize};

cadmpeg_core::named_optional_field!(deserialize_body, BodyId, "body");
/// One member of the Design `BulkStream` `BodiesRoot` list.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub(crate) struct DesignBodyMember {
    /// Globally unique deterministic identifier for this native record.
    pub(crate) id: String,
    /// Byte offset of this member's leading presence byte in its Design `BulkStream`.
    pub(crate) byte_offset: u64,
    /// Numeric suffix of this body's design-entity id.
    pub(crate) entity_suffix: u64,
    /// Source per-member flag word from the `BodiesRoot` list entry.
    pub(crate) flags: u16,
}

/// Triplicated axis-aligned body bounds cached in the Design stream.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(try_from = "DesignBodyBoundsWire", into = "DesignBodyBoundsWire")]
pub(crate) struct DesignBodyBounds {
    /// Globally unique deterministic identifier for this native record set.
    pub(crate) id: String,
    /// Numeric suffix of the owning Design body entity.
    entity_suffix: u32,
    /// Byte offset of the owning Design entity header.
    pub(crate) entity_byte_offset: u64,
    /// Indexed-header byte offsets parallel to `record_indices`.
    record_byte_offsets: [u64; 3],
    /// First f64 byte of each repeated sextuple.
    value_byte_offsets: [u64; 3],
    /// Design BREP body-map pairs carrying this entity suffix, in stream order.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub(crate) body_binding_ids: Vec<String>,
    corners: DesignMeshSceneBounds,
}

#[derive(Serialize, Deserialize)]
pub(crate) struct DesignBodyBoundsWire {
    /// Globally unique deterministic identifier for this native record set.
    pub(crate) id: String,
    /// Numeric suffix of the owning Design body entity.
    pub(crate) entity_suffix: u64,
    /// Byte offset of the owning Design entity header.
    pub(crate) entity_byte_offset: u64,
    /// Three consecutive indexed record identities carrying the cache.
    pub(crate) record_indices: [u32; 3],
    /// Indexed-header byte offsets parallel to `record_indices`.
    pub(crate) record_byte_offsets: [u64; 3],
    /// First f64 byte of each repeated sextuple.
    pub(crate) value_byte_offsets: [u64; 3],
    /// Design BREP body-map pairs carrying this entity suffix, in stream order.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub(crate) body_binding_ids: Vec<String>,
    /// Maximum model-space corner in millimetres.
    pub(crate) maximum: Point3,
    /// Minimum model-space corner in millimetres.
    pub(crate) minimum: Point3,
}

impl TryFrom<DesignBodyBoundsWire> for DesignBodyBounds {
    type Error = String;
    fn try_from(wire: DesignBodyBoundsWire) -> Result<Self, Self::Error> {
        let entity_suffix = u32::try_from(wire.entity_suffix)
            .map_err(|_| "entity_suffix exceeds indexed record range")?;
        let last = entity_suffix
            .checked_add(3)
            .ok_or("entity_suffix leaves no room for three cache records")?;
        if wire.record_indices != [last - 2, last - 1, last] {
            return Err("record_indices must follow entity_suffix consecutively".into());
        }
        if !wire
            .record_byte_offsets
            .windows(2)
            .all(|pair| pair[0] < pair[1])
        {
            return Err("record_byte_offsets must be strictly increasing".into());
        }
        if !wire
            .value_byte_offsets
            .iter()
            .zip(wire.record_byte_offsets)
            .all(|(value, record)| *value > record)
        {
            return Err("value_byte_offsets must follow record_byte_offsets".into());
        }
        let maximum = [wire.maximum.x, wire.maximum.y, wire.maximum.z];
        let minimum = [wire.minimum.x, wire.minimum.y, wire.minimum.z];
        let corners = DesignMeshSceneBounds::new(maximum, minimum)?;
        if maximum == minimum {
            return Err("maximum and minimum must not define a degenerate box".into());
        }
        Ok(Self {
            id: wire.id,
            entity_suffix,
            entity_byte_offset: wire.entity_byte_offset,
            record_byte_offsets: wire.record_byte_offsets,
            value_byte_offsets: wire.value_byte_offsets,
            body_binding_ids: wire.body_binding_ids,
            corners,
        })
    }
}

impl DesignBodyBounds {
    pub(crate) fn entity_suffix(&self) -> u64 {
        u64::from(self.entity_suffix)
    }
    fn record_indices(&self) -> [u32; 3] {
        [
            self.entity_suffix + 1,
            self.entity_suffix + 2,
            self.entity_suffix + 3,
        ]
    }
}

impl From<DesignBodyBounds> for DesignBodyBoundsWire {
    fn from(value: DesignBodyBounds) -> Self {
        let [x, y, z] = value.corners.maximum();
        let maximum = Point3::new(x, y, z);
        let [x, y, z] = value.corners.minimum();
        Self {
            entity_suffix: value.entity_suffix(),
            record_indices: value.record_indices(),
            id: value.id,
            entity_byte_offset: value.entity_byte_offset,
            record_byte_offsets: value.record_byte_offsets,
            value_byte_offsets: value.value_byte_offsets,
            body_binding_ids: value.body_binding_ids,
            maximum,
            minimum: Point3::new(x, y, z),
        }
    }
}

/// One ordered pair in a Design `BulkStream` BREP body-map record.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(try_from = "DesignBodyBindingWire", into = "DesignBodyBindingWire")]
pub(crate) struct DesignBodyBinding {
    /// Globally unique deterministic identifier for this native map entry.
    pub(crate) id: String,
    /// Design `BulkStream` ZIP entry containing the map.
    pub(crate) stream: String,
    /// Number of pairs in the enclosing body map.
    pair_count: std::num::NonZeroU32,
    /// Zero-based position in the enclosing body map.
    pair_ordinal: u32,
    /// BREP body selector stored by this pair.
    pub(crate) asm_body_key: u64,
    /// Byte offset of `asm_body_key` within `stream`.
    asm_body_key_offset: u64,
    /// Numeric Design entity suffix stored by this pair.
    pub(crate) entity_suffix: u64,
    /// Basename of the BREP blob whose body namespace contains the key.
    blob_name: String,
    /// Byte offset of the UTF-16LE `blob_name` code units within `stream`.
    blob_name_offset: u64,
    /// Solved body in the BREP blob named by this pair.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) body: Option<BodyId>,
}

#[derive(Serialize, Deserialize)]
pub(crate) struct DesignBodyBindingWire {
    /// Globally unique deterministic identifier for this native map entry.
    pub(crate) id: String,
    /// Design `BulkStream` ZIP entry containing the map.
    pub(crate) stream: String,
    /// Number of pairs in the enclosing body map.
    pub(crate) pair_count: u32,
    /// Zero-based position in the enclosing body map.
    pub(crate) pair_ordinal: u32,
    /// BREP body selector stored by this pair.
    pub(crate) asm_body_key: u64,
    /// Byte offset of `asm_body_key` within `stream`.
    pub(crate) asm_body_key_offset: u64,
    /// Numeric Design entity suffix stored by this pair.
    pub(crate) entity_suffix: u64,
    /// Byte offset of `entity_suffix` within `stream`.
    pub(crate) entity_suffix_offset: u64,
    /// Basename of the BREP blob whose body namespace contains the key.
    pub(crate) blob_name: String,
    /// Byte offset of the UTF-16LE `blob_name` code units within `stream`.
    pub(crate) blob_name_offset: u64,
    /// Solved body in the BREP blob named by this pair.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_body"
    )]
    pub(crate) body: Option<BodyId>,
}

impl TryFrom<DesignBodyBindingWire> for DesignBodyBinding {
    type Error = String;
    fn try_from(wire: DesignBodyBindingWire) -> Result<Self, Self::Error> {
        let pair_count =
            std::num::NonZeroU32::new(wire.pair_count).ok_or("pair_count must be nonzero")?;
        if wire.pair_ordinal >= pair_count.get() {
            return Err("pair_ordinal must be less than pair_count".into());
        }
        let entity_suffix_offset = wire
            .asm_body_key_offset
            .checked_add(8)
            .ok_or("asm_body_key_offset overflows entity_suffix_offset")?;
        if wire.entity_suffix_offset != entity_suffix_offset {
            return Err(
                "entity_suffix_offset must follow asm_body_key_offset by eight bytes".into(),
            );
        }
        if !wire.blob_name.starts_with("BREP.") {
            return Err("blob_name must start with BREP.".into());
        }
        if wire.blob_name_offset <= entity_suffix_offset {
            return Err("blob_name_offset must follow entity_suffix_offset".into());
        }
        Ok(Self {
            pair_count,
            id: wire.id,
            stream: wire.stream,
            pair_ordinal: wire.pair_ordinal,
            asm_body_key: wire.asm_body_key,
            asm_body_key_offset: wire.asm_body_key_offset,
            entity_suffix: wire.entity_suffix,
            blob_name: wire.blob_name,
            blob_name_offset: wire.blob_name_offset,
            body: wire.body,
        })
    }
}

impl DesignBodyBinding {
    pub(crate) fn pair_count(&self) -> u32 {
        self.pair_count.get()
    }
    pub(crate) fn pair_ordinal(&self) -> u32 {
        self.pair_ordinal
    }
    pub(crate) fn asm_body_key_offset(&self) -> u64 {
        self.asm_body_key_offset
    }
    pub(crate) fn entity_suffix_offset(&self) -> u64 {
        self.asm_body_key_offset + 8
    }
    pub(crate) fn blob_name(&self) -> &str {
        &self.blob_name
    }
    pub(crate) fn blob_name_offset(&self) -> u64 {
        self.blob_name_offset
    }
}

impl From<DesignBodyBinding> for DesignBodyBindingWire {
    fn from(value: DesignBodyBinding) -> Self {
        Self {
            pair_count: value.pair_count(),
            entity_suffix_offset: value.entity_suffix_offset(),
            id: value.id,
            stream: value.stream,
            pair_ordinal: value.pair_ordinal,
            asm_body_key: value.asm_body_key,
            asm_body_key_offset: value.asm_body_key_offset,
            entity_suffix: value.entity_suffix,
            blob_name: value.blob_name,
            blob_name_offset: value.blob_name_offset,
            body: value.body,
        }
    }
}

/// Design browser-node visibility joined to one solved ASM body.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub(crate) struct BodyVisibility {
    /// Globally unique deterministic identifier for this native record.
    pub(crate) id: String,
    /// Solved B-rep body controlled by the browser node.
    pub(crate) body: BodyId,
    /// Design `BulkStream` ZIP entry containing the browser node.
    pub(crate) stream: String,
    /// Byte offset of the browser node's hidden flag within `stream`.
    pub(crate) byte_offset: u64,
    /// Byte offset of the joined body-map ASM key within `stream`.
    pub(crate) asm_body_key_offset: u64,
    /// ASM body key used by the BREP body-map join.
    pub(crate) asm_body_key: u64,
    /// Numeric Design entity suffix stored by both joined records.
    pub(crate) entity_suffix: u64,
    /// Display visibility after inverting the native hidden flag.
    pub(crate) visible: bool,
}
