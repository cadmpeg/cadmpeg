// SPDX-License-Identifier: Apache-2.0
//! Identity references of dynamic object extensions.

use super::ExtensionRecord;
use cadmpeg_core::decode::DecodeContext;
use cadmpeg_core::CodecError;
use cadmpeg_ir::schema::rewrite::typed::prune::{DependencyPosition, DependencyReference};
use cadmpeg_ir::schema::rewrite::typed::{IdentityMap, RewriteIdentities};

impl RewriteIdentities for ExtensionRecord {
    fn visit_identity_references(
        &self,
        _ctx: &DecodeContext<'_>,
        visitor: &mut dyn FnMut(&str) -> Result<(), CodecError>,
    ) -> Result<(), CodecError> {
        visitor(&self.owner)
    }
    fn visit_dependency_references(
        &self,
        _ctx: &DecodeContext<'_>,
        visitor: &mut dyn FnMut(DependencyReference<'_>) -> Result<(), CodecError>,
    ) -> Result<(), CodecError> {
        visitor(DependencyReference {
            target: &self.owner,
            position: DependencyPosition::Required,
            field: Some("owner"),
        })
    }
    fn rewrite_identities<F: FnMut(&str) -> Result<String, CodecError>>(
        mut self,
        _ctx: &DecodeContext<'_>,
        map: &mut IdentityMap<'_, F>,
    ) -> Result<Self, CodecError> {
        self.id = map.identity(&self.id)?;
        self.owner = map.identity(&self.owner)?;
        Ok(self)
    }
    fn rewrite_native_value<F: FnMut(&str) -> Result<String, CodecError>>(
        _ctx: &DecodeContext<'_>,
        value: &mut serde_json::Value,
        map: &mut IdentityMap<'_, F>,
    ) -> Result<(), CodecError> {
        for name in ["id", "owner"] {
            let field = value
                .get_mut(name)
                .ok_or_else(|| CodecError::malformed("extension identity is absent"))?;
            let id = field
                .as_str()
                .ok_or_else(|| CodecError::malformed("extension identity is not text"))?;
            *field = map.identity(id)?.into();
        }
        Ok(())
    }
}
