// SPDX-License-Identifier: Apache-2.0
//! Census identities and local references remain independent of source spelling.

use super::ApplicationRecordWire;
use cadmpeg_core::decode::DecodeContext;
use cadmpeg_core::CodecError;
use cadmpeg_ir::schema::rewrite::typed::prune::{
    DependencyPosition, DependencyReference, ReferencePruneEffect,
};
use cadmpeg_ir::schema::rewrite::typed::{IdentityMap, RewriteIdentities};
use std::borrow::Cow;
use std::collections::BTreeSet;

fn listed_links(kind: &str) -> bool {
    kind.contains("List")
        || crate::persistence::document_properties::value_parts::links::expression_container(kind)
}

impl RewriteIdentities for ApplicationRecordWire<'_> {
    fn visit_identity_references(
        &self,
        ctx: &DecodeContext<'_>,
        visitor: &mut dyn FnMut(&str) -> Result<(), CodecError>,
    ) -> Result<(), CodecError> {
        self.visit_dependency_references(ctx, &mut |reference| visitor(reference.target))
    }

    fn visit_dependency_references(
        &self,
        _ctx: &DecodeContext<'_>,
        visitor: &mut dyn FnMut(DependencyReference<'_>) -> Result<(), CodecError>,
    ) -> Result<(), CodecError> {
        visitor(DependencyReference {
            target: &self.object,
            position: DependencyPosition::Required,
            field: Some("object"),
        })?;
        for (index, target) in self.properties.iter().enumerate() {
            visitor(DependencyReference {
                target,
                position: DependencyPosition::List {
                    length: self.properties.len(),
                    index,
                },
                field: Some("properties"),
            })?;
        }
        for (index, target) in self.dependencies.iter().enumerate() {
            visitor(DependencyReference {
                target,
                position: DependencyPosition::List {
                    length: self.dependencies.len(),
                    index,
                },
                field: Some("dependencies"),
            })?;
        }
        for property in &self.property_records {
            for (field, target) in [
                ("property_records.object", property.object.as_ref()),
                ("property_records.property", property.property.as_ref()),
            ] {
                visitor(DependencyReference {
                    target,
                    position: DependencyPosition::Required,
                    field: Some(field),
                })?;
            }
            for (index, link) in property.links.iter().enumerate() {
                if let Some(link) = link.as_ref().filter(|link| link.document().is_none()) {
                    if let Some(target) = link.object() {
                        visitor(DependencyReference {
                            target,
                            position: if listed_links(&property.type_name) {
                                DependencyPosition::List {
                                    length: property.links.len(),
                                    index,
                                }
                            } else {
                                DependencyPosition::Required
                            },
                            field: Some("property_records.links"),
                        })?;
                    }
                }
            }
            for payload in &property.payloads {
                visitor(DependencyReference {
                    target: &payload.entry,
                    position: DependencyPosition::Required,
                    field: Some("property_records.payloads.entry"),
                })?;
            }
        }
        Ok(())
    }

    fn prune_reference_lists(
        &mut self,
        ctx: &DecodeContext<'_>,
        deleted: &BTreeSet<String>,
    ) -> Result<ReferencePruneEffect, CodecError> {
        let effect = cadmpeg_ir::schema::rewrite::typed::prune::analyze(self, ctx, deleted)?;
        if !effect.can_apply() || effect == ReferencePruneEffect::Unchanged {
            return Ok(effect);
        }
        self.properties
            .retain(|target| !deleted.contains(target.as_ref()));
        self.dependencies
            .to_mut()
            .retain(|target| !deleted.contains(target));
        for property in &mut self.property_records {
            if listed_links(&property.type_name) {
                for link in property.links.to_mut() {
                    if link
                        .as_ref()
                        .filter(|link| link.document().is_none())
                        .and_then(|link| link.object())
                        .is_some_and(|target| deleted.contains(target))
                    {
                        *link = None;
                    }
                }
            }
        }
        Ok(effect)
    }

    fn rewrite_identities<F: FnMut(&str) -> Result<String, CodecError>>(
        mut self,
        _ctx: &DecodeContext<'_>,
        map: &mut IdentityMap<'_, F>,
    ) -> Result<Self, CodecError> {
        self.id = map.identity(&self.id)?;
        self.object = Cow::Owned(map.identity(&self.object)?);
        for target in &mut self.properties {
            *target = Cow::Owned(map.identity(target)?);
        }
        for target in self.dependencies.to_mut() {
            *target = map.identity(target)?;
        }
        for property in &mut self.property_records {
            property.id = map.identity(&property.id)?;
            property.object = Cow::Owned(map.identity(&property.object)?);
            property.property = Cow::Owned(map.identity(&property.property)?);
            crate::native::document_value::rewrite_local_links(property.links.to_mut(), map)?;
            for payload in &mut property.payloads {
                payload.entry = Cow::Owned(map.identity(&payload.entry)?);
            }
        }
        Ok(self)
    }

    fn rewrite_native_value<F: FnMut(&str) -> Result<String, CodecError>>(
        _ctx: &DecodeContext<'_>,
        value: &mut serde_json::Value,
        map: &mut IdentityMap<'_, F>,
    ) -> Result<(), CodecError> {
        fn replace<F: FnMut(&str) -> Result<String, CodecError>>(
            value: &mut serde_json::Value,
            map: &mut IdentityMap<'_, F>,
        ) -> Result<(), CodecError> {
            let source = value
                .as_str()
                .ok_or_else(|| CodecError::malformed("application identity is not text"))?;
            *value = map.identity(source)?.into();
            Ok(())
        }
        for field in ["id", "object"] {
            replace(&mut value[field], map)?;
        }
        for field in ["properties", "dependencies"] {
            for target in value[field]
                .as_array_mut()
                .ok_or_else(|| CodecError::malformed("application references are not an array"))?
            {
                replace(target, map)?;
            }
        }
        for property in value["property_records"]
            .as_array_mut()
            .ok_or_else(|| CodecError::malformed("application properties are not an array"))?
        {
            for field in ["id", "object", "property"] {
                replace(&mut property[field], map)?;
            }
            for link in property["links"]
                .as_array_mut()
                .ok_or_else(|| CodecError::malformed("application links are not an array"))?
            {
                if link.is_null()
                    || link
                        .get("document")
                        .is_some_and(|document| !document.is_null())
                {
                    continue;
                }
                if let Some(target) = link.get_mut("object").filter(|target| !target.is_null()) {
                    replace(target, map)?;
                }
            }
            for payload in property["payloads"]
                .as_array_mut()
                .ok_or_else(|| CodecError::malformed("application payloads are not an array"))?
            {
                replace(&mut payload["entry"], map)?;
            }
        }
        Ok(())
    }
}
