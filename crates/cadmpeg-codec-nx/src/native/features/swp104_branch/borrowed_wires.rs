// SPDX-License-Identifier: Apache-2.0
//! Borrowed SWP104 branch and reference serialization.

use super::FeatureSwp104LeadingBranch;
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

struct MembersView<'a>(&'a FeatureSwp104LeadingBranch);

impl Serialize for MembersView<'_> {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let branch = self.0;
        let mut sequence = serializer.serialize_seq(Some(branch.members.len()))?;
        let mut at = branch.source_offset + branch.members_offset();
        for (ordinal, reference) in branch.members.as_slice().iter().enumerate() {
            sequence.serialize_element(&ReferenceView {
                ordinal: u32::try_from(ordinal).map_err(serde::ser::Error::custom)?,
                token: &reference.token,
                data_block: reference.data_block.as_deref(),
                source_offset: at,
            })?;
            at += cadmpeg_core::decode::u64_from_index(reference.token.raw().len());
        }
        sequence.end()
    }
}

impl Serialize for FeatureSwp104LeadingBranch {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut wire = serializer.serialize_map(None)?;
        wire.serialize_entry("id", &self.id)?;
        wire.serialize_entry("operation_label", &self.operation_label)?;
        wire.serialize_entry("discriminator", &self.discriminator)?;
        wire.serialize_entry("scalars", &self.scalars.map(|scalar| scalar.value().get()))?;
        wire.serialize_entry(
            "raw_scalars",
            &self.scalars.map(crate::om::scalar::ShiftedBinary64::raw),
        )?;
        wire.serialize_entry("leading_zero", &self.leading_zero)?;
        wire.serialize_entry("mode", &self.mode)?;
        wire.serialize_entry("declared_count", &self.members.declared_count())?;
        if let Some(count) = self.state_lane.witnessed_count() {
            wire.serialize_entry("witnessed_count", &count)?;
        }
        wire.serialize_entry("state_lane", self.state_lane.bytes())?;
        wire.serialize_entry("members", &MembersView(self))?;
        let terminal_offset = self.source_offset
            + self.members_offset()
            + self
                .members
                .as_slice()
                .iter()
                .map(|reference| cadmpeg_core::decode::u64_from_index(reference.token.raw().len()))
                .sum::<u64>()
            + self.state_len()
            + 3;
        wire.serialize_entry(
            "terminal",
            &ReferenceView {
                ordinal: u32::from(self.members.declared_count() - 1),
                token: &self.terminal.token,
                data_block: self.terminal.data_block.as_deref(),
                source_offset: terminal_offset,
            },
        )?;
        wire.serialize_entry("byte_len", &self.byte_len())?;
        wire.serialize_entry("source_offset", &self.source_offset)?;
        wire.end()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use cadmpeg_core::decode::{DecodePolicy, ResourceDimension};
    use cadmpeg_core::CodecError;
    use cadmpeg_test_support::native_serialization::assert_native_limit;

    #[test]
    fn swp104_branch_borrowed_bytes_and_limit() {
        let mut payload = vec![33, 0, 0, 1, 0];
        for _ in 0..4 {
            payload.extend([47, 164, 122, 225, 71, 174, 20, 123]);
        }
        payload.extend([35, 1, 2, 240, 1]);
        payload.extend([0; 5]);
        payload.extend([255, 1, 2, 241, 1, 0, 0]);
        let record =
            crate::om::operation_record::OperationPayload::new(&payload, 200, "SWP104").unwrap();
        let source = crate::test_support::with_decode_context(|ctx| {
            crate::om::swp104_payload_leading_branch(ctx, record)
        })
        .unwrap()
        .unwrap();
        let branch = crate::test_support::with_decode_context(|ctx| {
            FeatureSwp104LeadingBranch::from_source(
                ctx,
                "nx:feature:swp104#0".to_owned(),
                "operation".to_owned(),
                1200,
                source,
                |token| Ok(Some(format!("block#{}", token.value()))),
            )
        })
        .unwrap()
        .unwrap();
        let borrowed = serde_json::to_vec(&branch).unwrap();
        let owned = serde_json::to_vec(&super::super::FeatureSwp104LeadingBranchWire::from(
            branch.clone(),
        ))
        .unwrap();
        assert_eq!(borrowed, owned);
        assert_native_limit(&branch, serde_json::to_value(&branch).unwrap());
    }

    fn mapped_branch_refusal(configure: impl FnOnce(&mut DecodePolicy)) -> CodecError {
        let mut payload = vec![33, 0, 0, 1, 0];
        for _ in 0..4 {
            payload.extend([47, 164, 122, 225, 71, 174, 20, 123]);
        }
        payload.extend([35, 1, 2, 240, 1]);
        payload.extend([0; 5]);
        payload.extend([255, 1, 2, 241, 1, 0, 0]);
        let record =
            crate::om::operation_record::OperationPayload::new(&payload, 200, "SWP104").unwrap();
        let source = crate::test_support::with_decode_context(|ctx| {
            crate::om::swp104_payload_leading_branch(ctx, record)
        })
        .unwrap()
        .unwrap();
        
        
        
        crate::test_support::with_decode_context_over(&payload, |policy| { configure(policy); }, |ctx| {

        FeatureSwp104LeadingBranch::from_source(
            ctx,
            "branch".to_owned(),
            "operation".to_owned(),
            1200,
            source,
            |_| Ok(None),
        )
        .unwrap_err()
    
})
}

    #[test]
    fn swp104_mapped_branch_refuses_collection_limit() {
        let error = mapped_branch_refusal(|policy| policy.limits.max_collection_items = 0);
        assert!(
            matches!(error, CodecError::ResourceLimit(limit) if limit.dimension == ResourceDimension::CollectionItems)
        );
    }

    #[test]
    fn swp104_mapped_branch_refuses_retained_limit() {
        let error = mapped_branch_refusal(|policy| policy.limits.max_retained_bytes = 0);
        assert!(
            matches!(error, CodecError::ResourceLimit(limit) if limit.dimension == ResourceDimension::RetainedBytes)
        );
    }
}
