// SPDX-License-Identifier: Apache-2.0
//! Typed identities for the source-owned assembly stage.

use cadmpeg_core::decode::DecodeContext;
use cadmpeg_core::CodecError;
use cadmpeg_ir::schema::rewrite::typed::{IdentityMap, RewriteIdentities};

// The source reference numbers in these records are format-local integers, not
// CADIR identity strings. Their only string identity is the record's own id.
macro_rules! assembly_owner {
    ($record:ty) => {
        impl RewriteIdentities for $record {
            fn visit_identity_references(
                &self,
                _ctx: &DecodeContext<'_>,
                _visitor: &mut dyn FnMut(&str) -> Result<(), CodecError>,
            ) -> Result<(), CodecError> {
                Ok(())
            }
            fn rewrite_identities<F>(
                mut self,
                _ctx: &DecodeContext<'_>,
                map: &mut IdentityMap<'_, F>,
            ) -> Result<Self, CodecError>
            where
                F: FnMut(&str) -> Result<String, CodecError>,
            {
                self.id = map.identity(&self.id)?;
                Ok(self)
            }
            fn rewrite_native_value<F: FnMut(&str) -> Result<String, CodecError>>(
                _ctx: &DecodeContext<'_>,
                value: &mut serde_json::Value,
                map: &mut IdentityMap<'_, F>,
            ) -> Result<(), CodecError> {
                if let Some(serde_json::Value::String(id)) = value.get_mut("id") {
                    *id = map.identity(id)?;
                }
                Ok(())
            }
        }
    };
}
assembly_owner!(super::AssemblyOccurrenceRecord);
assembly_owner!(super::RseRecordRecord);
