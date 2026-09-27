// SPDX-License-Identifier: Apache-2.0
//! Borrowed surface branch references and suffix.

use super::FeatureSurfaceConstructionBranch;
use serde::ser::{SerializeMap, SerializeSeq};
use serde::Serialize;

#[derive(Serialize)]
struct ReferenceView<'a> {
    ordinal: u32,
    #[serde(flatten)]
    token: &'a crate::om::reference_index::PayloadIndexToken,
    #[serde(skip_serializing_if = "Option::is_none")]
    data_block: Option<&'a str>,
    source_offset: u64,
}

struct MembersView<'a>(&'a FeatureSurfaceConstructionBranch);

impl Serialize for MembersView<'_> {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let branch = self.0;
        let members = branch.references.members().as_slice();
        let mut sequence = serializer.serialize_seq(Some(members.len()))?;
        for (ordinal, ((token, data_block), source_offset)) in members
            .iter()
            .zip(branch.references.member_offsets())
            .enumerate()
        {
            sequence.serialize_element(&ReferenceView {
                ordinal: ordinal as u32,
                token,
                data_block: data_block.as_deref(),
                source_offset,
            })?;
        }
        sequence.end()
    }
}

impl Serialize for FeatureSurfaceConstructionBranch {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut wire = serializer.serialize_map(None)?;
        let branch = &self.references;
        let terminal = branch.terminal();
        wire.serialize_entry("id", &self.id)?;
        wire.serialize_entry("operation_label", &self.operation_label)?;
        wire.serialize_entry("ordinal", &self.ordinal())?;
        wire.serialize_entry("family", &self.family)?;
        wire.serialize_entry("header_code", &self.header_code)?;
        wire.serialize_entry("mode", &branch.mode())?;
        wire.serialize_entry("declared_count", &branch.members().declared_count())?;
        wire.serialize_entry("witnessed", &branch.witnessed())?;
        wire.serialize_entry("members", &MembersView(self))?;
        wire.serialize_entry(
            "terminal",
            &ReferenceView {
                ordinal: branch.members().len() as u32,
                token: &terminal.0,
                data_block: terminal.1.as_deref(),
                source_offset: branch.terminal_offset(),
            },
        )?;
        wire.serialize_entry("suffix", branch.suffix())?;
        wire.serialize_entry("source_offset", &branch.offset())?;
        wire.end()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use cadmpeg_test_support::native_serialization::assert_native_limit;

    #[test]
    fn surface_branch_borrowed_bytes_and_limit() {
        let json = r#"{"id":"nx:feature:surface#0","operation_label":"operation","ordinal":254,"family":80,"header_code":255,"mode":22,"declared_count":3,"witnessed":false,"members":[{"ordinal":0,"object_index":0,"raw_object_index":[240,0],"data_block":"zero","source_offset":103},{"ordinal":1,"object_index":256,"raw_object_index":[241,1,0],"source_offset":105}],"terminal":{"ordinal":2,"object_index":1,"raw_object_index":[240,1],"source_offset":116},"suffix":[0,255],"source_offset":100}"#;
        let branch: FeatureSurfaceConstructionBranch = serde_json::from_str(json).unwrap();
        let borrowed = serde_json::to_vec(&branch).unwrap();
        let owned =
            serde_json::to_vec(&super::super::SurfaceBranchWire::from(branch.clone())).unwrap();
        assert_eq!(borrowed, owned);
        assert_eq!(borrowed, json.as_bytes());
        assert_native_limit(
            &branch,
            serde_json::from_str::<serde_json::Value>(json).unwrap(),
        );
    }
}
