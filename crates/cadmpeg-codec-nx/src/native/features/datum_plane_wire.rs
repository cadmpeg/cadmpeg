// SPDX-License-Identifier: Apache-2.0
//! Borrowed serialization of datum-plane payloads and descriptors.

use super::{
    FeatureDatumPlaneDescriptor, FeatureDatumPlanePayload, FeaturePayloadBlock,
    FeaturePayloadContent,
};
use crate::iter_wire::IterWire;
use crate::om::compact::RawCompactIndex;
use crate::om::datum_index::DatumIndexLane;
use serde::ser::SerializeMap;
use serde::Serialize;

#[derive(Serialize)]
struct PayloadRef<'a> {
    id: &'a str,
    operation_label: &'a str,
    datum_plane_header: &'a str,
    #[serde(flatten)]
    content: &'a FeaturePayloadContent<Vec<FeaturePayloadBlock>>,
    #[serde(flatten)]
    index: IndexColumns<'a>,
}

struct IndexColumns<'a>(Option<&'a DatumIndexLane<u64>>);

impl Serialize for IndexColumns<'_> {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut wire = serializer.serialize_map(None)?;
        if let Some(lane) = self.0 {
            wire.serialize_entry("index_lane_offset", &lane.offset())?;
            wire.serialize_entry(
                "index_lane_declared_count",
                &usize::from(lane.declared_count()),
            )?;
            wire.serialize_entry(
                "index_lane_values",
                &IterWire(lane.indices().map(|entry| entry.atom.value())),
            )?;
            wire.serialize_entry(
                "index_lane_raw_indices",
                &IterWire(lane.indices().map(|entry| RawCompactIndex(entry.atom))),
            )?;
            wire.serialize_entry(
                "index_lane_value_offsets",
                &IterWire(lane.indices().map(|entry| entry.offset)),
            )?;
            wire.serialize_entry("index_lane_trailer", &lane.trailer())?;
        }
        wire.end()
    }
}

impl Serialize for FeatureDatumPlanePayload {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        PayloadRef {
            id: &self.id,
            operation_label: &self.operation_label,
            datum_plane_header: &self.datum_plane_header,
            content: &self.content,
            index: IndexColumns(self.index_lane.as_ref()),
        }
        .serialize(serializer)
    }
}

impl Serialize for FeatureDatumPlaneDescriptor {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut wire = serializer.serialize_map(None)?;
        wire.serialize_entry("id", &self.id)?;
        wire.serialize_entry("operation_label", &self.operation_label)?;
        wire.serialize_entry("datum_plane_header", &self.datum_plane_header)?;
        wire.serialize_entry("ordinal", &self.ordinal)?;
        wire.serialize_entry("data_block", &self.data_block)?;
        wire.serialize_entry("identity", self.descriptor.identity())?;
        wire.serialize_entry("suffix", &IterWire(self.descriptor.suffix_bytes()))?;
        wire.serialize_entry("schema_index", &self.descriptor.schema_index())?;
        wire.serialize_entry("label", self.descriptor.label())?;
        wire.serialize_entry("source_offset", &self.source_offset)?;
        wire.end()
    }
}

#[cfg(test)]
mod tests {
    use super::super::{FeatureDatumPlaneDescriptorWire, FeatureDatumPlanePayloadWire};
    use super::{FeatureDatumPlaneDescriptor, FeatureDatumPlanePayload};

    #[test]
    fn datum_plane_payload_borrowed_wire_matches_owned_bytes_and_retained_limit() {
        let json = r#"{"id":"nx:feature:datum-plane-payload#0","operation_label":"operation","datum_plane_header":"header","data_blocks":["block"],"byte_len":8,"sha256":"d04b98f48e8f8bcc15c6ae5ac050801cd6dcfd428fb5f9e65c4e16e7807340fa","block_payload_offsets":[0],"block_byte_lengths":[8],"block_source_offsets":[10],"index_lane_offset":2,"index_lane_declared_count":3,"index_lane_values":[4096,1],"index_lane_raw_indices":[[144,0],[128,1]],"index_lane_value_offsets":[4,6],"index_lane_trailer":0}"#;
        let record: FeatureDatumPlanePayload = serde_json::from_str(json).unwrap();
        assert_eq!(serde_json::to_vec(&record).unwrap(), json.as_bytes());
        assert_eq!(
            serde_json::to_vec(&record).unwrap(),
            serde_json::to_vec(&FeatureDatumPlanePayloadWire::from(record.clone())).unwrap()
        );
        cadmpeg_test_support::native_serialization::assert_native_limit(
            &record,
            serde_json::from_str::<serde_json::Value>(json).unwrap(),
        );
    }

    #[test]
    fn datum_plane_descriptor_borrowed_wire_matches_owned_bytes_and_retained_limit() {
        let json = r#"{"id":"nx:feature:datum-plane-descriptor#0","operation_label":"operation","datum_plane_header":"header","ordinal":0,"data_block":"block","identity":"012345678901234567890123456789","suffix":[63,65,1,255,2,1,97,98,99,100],"schema_index":1,"label":"abcd","source_offset":10}"#;
        let record: FeatureDatumPlaneDescriptor = serde_json::from_str(json).unwrap();
        assert_eq!(serde_json::to_vec(&record).unwrap(), json.as_bytes());
        assert_eq!(
            serde_json::to_vec(&record).unwrap(),
            serde_json::to_vec(&FeatureDatumPlaneDescriptorWire::from(record.clone())).unwrap()
        );
        cadmpeg_test_support::native_serialization::assert_native_limit(
            &record,
            serde_json::from_str::<serde_json::Value>(json).unwrap(),
        );
    }
}
