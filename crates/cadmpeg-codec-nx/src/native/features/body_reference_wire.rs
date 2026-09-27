// SPDX-License-Identifier: Apache-2.0
//! Borrowed serialization of operation body-reference lanes.

use super::{
    FeatureOperationBodyReferenceLane, FeatureOperationBodyReferenceLaneEncoding,
    FeatureOperationBodyReferences,
};
use crate::iter_wire::IterWire;
use serde::ser::SerializeMap;
use serde::Serialize;

impl Serialize for FeatureOperationBodyReferenceLane {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut wire = serializer.serialize_map(None)?;
        wire.serialize_entry("id", &self.id)?;
        wire.serialize_entry("operation_label", &self.operation_label)?;
        wire.serialize_entry("body_reference_ordinal", &self.body_reference_ordinal)?;
        wire.serialize_entry("body_object_index", &self.body_object_index)?;
        wire.serialize_entry("branch", &self.branch)?;
        macro_rules! columns {
            ($references:expr, $encoding:expr) => {{
                wire.serialize_entry("encoding", &$encoding)?;
                wire.serialize_entry(
                    "object_indices",
                    &IterWire($references.iter().map(|reference| reference.token.value())),
                )?;
                wire.serialize_entry(
                    "raw_object_indices",
                    &IterWire($references.iter().map(|reference| reference.token.raw())),
                )?;
                wire.serialize_entry(
                    "data_blocks",
                    &IterWire(
                        $references
                            .iter()
                            .map(|reference| reference.data_block.as_deref()),
                    ),
                )?;
                wire.serialize_entry(
                    "source_offsets",
                    &IterWire($references.iter().map(|reference| reference.source_offset)),
                )?;
            }};
        }
        match &self.references {
            FeatureOperationBodyReferences::CompactIndex(references) => {
                columns!(
                    references,
                    FeatureOperationBodyReferenceLaneEncoding::CompactIndex
                );
            }
            FeatureOperationBodyReferences::PayloadObjectIndex(references) => {
                columns!(
                    references,
                    FeatureOperationBodyReferenceLaneEncoding::PayloadObjectIndex
                );
            }
        }
        wire.end()
    }
}

#[cfg(test)]
mod tests {
    use super::super::{FeatureOperationBodyReferenceLane, FeatureOperationBodyReferenceLaneWire};

    #[test]
    fn operation_body_reference_borrowed_wires_match_owned_bytes_and_retained_limit() {
        for json in [
            r#"{"id":"nx:feature:body-reference-lane#0","operation_label":"operation","body_reference_ordinal":0,"body_object_index":110,"branch":28,"encoding":"compact_index","object_indices":[4096,28673],"raw_object_indices":[[144,0],[240,1]],"data_blocks":[null,"block"],"source_offsets":[111,113]}"#,
            r#"{"id":"nx:feature:body-reference-lane#1","operation_label":"operation","body_reference_ordinal":0,"body_object_index":110,"branch":17,"encoding":"payload_object_index","object_indices":[1,256],"raw_object_indices":[[240,1],[241,1,0]],"data_blocks":[null,"block"],"source_offsets":[111,113]}"#,
        ] {
            let record: FeatureOperationBodyReferenceLane = serde_json::from_str(json).unwrap();
            assert_eq!(serde_json::to_vec(&record).unwrap(), json.as_bytes());
            assert_eq!(
                serde_json::to_vec(&record).unwrap(),
                serde_json::to_vec(&FeatureOperationBodyReferenceLaneWire::from(record.clone()))
                    .unwrap()
            );
            cadmpeg_test_support::native_serialization::assert_native_limit(
                &record,
                serde_json::from_str::<serde_json::Value>(json).unwrap(),
            );
        }
    }
}
