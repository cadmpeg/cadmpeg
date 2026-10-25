// SPDX-License-Identifier: Apache-2.0
//! GUI identities are distinct from literal renderer values.

use crate::native::{GuiDocumentRecord, GuiPropertyRecord, GuiViewProviderRecord};
use cadmpeg_core::decode::DecodeContext;
use cadmpeg_core::CodecError;
use cadmpeg_ir::schema::rewrite::typed::prune::{
    DependencyPosition, DependencyReference, ReferencePruneEffect,
};
use cadmpeg_ir::schema::rewrite::typed::{IdentityMap, RewriteIdentities};

fn rewrite_fields<F: FnMut(&str) -> Result<String, CodecError>>(
    value: &mut serde_json::Value,
    fields: &[&str],
    map: &mut IdentityMap<'_, F>,
) -> Result<(), CodecError> {
    for field in fields {
        if let Some(value) = value.get_mut(*field).filter(|value| !value.is_null()) {
            let id = value
                .as_str()
                .ok_or_else(|| CodecError::malformed("GUI identity is not text"))?;
            *value = map.identity(id)?.into();
        }
    }
    Ok(())
}

impl RewriteIdentities for GuiDocumentRecord {
    fn visit_identity_references(
        &self,
        _ctx: &DecodeContext<'_>,
        _visitor: &mut dyn FnMut(&str) -> Result<(), CodecError>,
    ) -> Result<(), CodecError> {
        Ok(())
    }
    fn rewrite_identities<F: FnMut(&str) -> Result<String, CodecError>>(
        mut self,
        _ctx: &DecodeContext<'_>,
        map: &mut IdentityMap<'_, F>,
    ) -> Result<Self, CodecError> {
        self.id = map.identity(&self.id)?;
        for state in &mut self.states {
            state.id = map.identity(&state.id)?;
        }
        Ok(self)
    }
    fn rewrite_native_value<F: FnMut(&str) -> Result<String, CodecError>>(
        _ctx: &DecodeContext<'_>,
        value: &mut serde_json::Value,
        map: &mut IdentityMap<'_, F>,
    ) -> Result<(), CodecError> {
        rewrite_fields(value, &["id"], map)?;
        if let Some(states) = value
            .get_mut("states")
            .and_then(|value| value.as_array_mut())
        {
            for state in states {
                rewrite_fields(state, &["id"], map)?;
            }
        }
        Ok(())
    }
}

impl RewriteIdentities for GuiViewProviderRecord {
    fn visit_identity_references(
        &self,
        _ctx: &DecodeContext<'_>,
        visitor: &mut dyn FnMut(&str) -> Result<(), CodecError>,
    ) -> Result<(), CodecError> {
        if let Some(object) = &self.object {
            visitor(object.as_str())?;
        }
        Ok(())
    }
    fn visit_dependency_references(
        &self,
        _ctx: &DecodeContext<'_>,
        visitor: &mut dyn FnMut(DependencyReference<'_>) -> Result<(), CodecError>,
    ) -> Result<(), CodecError> {
        if let Some(object) = &self.object {
            visitor(DependencyReference {
                target: object.as_str(),
                position: DependencyPosition::Required,
                field: Some("object"),
            })?;
        }
        Ok(())
    }
    fn rewrite_identities<F: FnMut(&str) -> Result<String, CodecError>>(
        mut self,
        _ctx: &DecodeContext<'_>,
        map: &mut IdentityMap<'_, F>,
    ) -> Result<Self, CodecError> {
        self.id = map.identity(&self.id)?;
        self.object = self
            .object
            .map(|object| {
                map.identity(object.as_str()).and_then(|id| {
                    cadmpeg_core::text::NonBlankString::new(id)
                        .ok_or_else(|| CodecError::malformed("GUI object identity is blank"))
                })
            })
            .transpose()?;
        Ok(self)
    }
    fn rewrite_native_value<F: FnMut(&str) -> Result<String, CodecError>>(
        _ctx: &DecodeContext<'_>,
        value: &mut serde_json::Value,
        map: &mut IdentityMap<'_, F>,
    ) -> Result<(), CodecError> {
        rewrite_fields(value, &["id", "object"], map)
    }
}

impl RewriteIdentities for GuiPropertyRecord {
    fn visit_identity_references(
        &self,
        _ctx: &DecodeContext<'_>,
        visitor: &mut dyn FnMut(&str) -> Result<(), CodecError>,
    ) -> Result<(), CodecError> {
        visitor(&self.owner)?;
        for link in self
            .links
            .iter()
            .flatten()
            .filter(|link| link.document().is_none())
        {
            if let Some(target) = link.object() {
                visitor(target)?;
            }
        }
        Ok(())
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
        })?;
        for (index, link) in self.links.iter().enumerate() {
            if let Some(target) = link
                .as_ref()
                .filter(|link| link.document().is_none())
                .and_then(|link| link.object())
            {
                visitor(DependencyReference {
                    target,
                    position: if self.type_name.contains("List") || crate::persistence::document_properties::value_parts::links::expression_container(&self.type_name) {
                        DependencyPosition::List {
                            length: self.links.len(),
                            index,
                        }
                    } else {
                        DependencyPosition::Required
                    },
                    field: Some("links"),
                })?;
            }
        }
        Ok(())
    }
    fn prune_reference_lists(
        &mut self,
        ctx: &DecodeContext<'_>,
        deleted: &std::collections::BTreeSet<String>,
    ) -> Result<ReferencePruneEffect, CodecError> {
        let effect = cadmpeg_ir::schema::rewrite::typed::prune::analyze(self, ctx, deleted)?;
        if effect.can_apply() && effect != ReferencePruneEffect::Unchanged {
            crate::native::document_value::prune_local_values(
                &self.type_name,
                &mut self.links,
                &mut self.values,
                deleted,
            )?;
        }
        Ok(effect)
    }
    fn rewrite_identities<F: FnMut(&str) -> Result<String, CodecError>>(
        mut self,
        _ctx: &DecodeContext<'_>,
        map: &mut IdentityMap<'_, F>,
    ) -> Result<Self, CodecError> {
        self.id = map.identity(&self.id)?;
        self.owner = map.identity(&self.owner)?;
        crate::native::document_value::rewrite_local_links(&mut self.links, map)?;
        Ok(self)
    }
    fn rewrite_native_value<F: FnMut(&str) -> Result<String, CodecError>>(
        ctx: &DecodeContext<'_>,
        value: &mut serde_json::Value,
        map: &mut IdentityMap<'_, F>,
    ) -> Result<(), CodecError> {
        crate::native::document_value::DocumentValue::rewrite_native_value(ctx, value, map)
    }
}
