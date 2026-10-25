// SPDX-License-Identifier: Apache-2.0
//! Typed identity ownership for admitted document value properties.

use super::{PropertyBody, PropertyRecord};
use cadmpeg_core::decode::DecodeContext;
use cadmpeg_core::CodecError;
use cadmpeg_ir::schema::rewrite::typed::prune::{
    DependencyPosition, DependencyReference, ReferencePruneEffect,
};
use cadmpeg_ir::schema::rewrite::typed::{IdentityMap, RewriteIdentities};
use serde::{Deserialize, Serialize};

mod link_slots;
pub(crate) use link_slots::link_carrier_slots;

/// Resolves source object names in temporary census fields; retained XML fields stay literal.
pub(crate) fn canonicalize_local_value(
    tag: &str,
    attributes: &mut std::collections::BTreeMap<String, String>,
) {
    if tag == "XLink" && attributes.get("file").is_some_and(|file| !file.is_empty()) {
        return;
    }
    let key = match tag {
        "Link" => {
            if attributes.contains_key("obj") {
                "obj"
            } else {
                "value"
            }
        }
        "LinkSub" => "value",
        "XLink" => "name",
        _ => return,
    };
    if let Some(value) = attributes.get_mut(key).filter(|value| !value.is_empty()) {
        *value = super::native_id("object", value.as_str());
    }
}

/// Drops only missing local reference carriers while preserving source value positions.
pub(crate) fn prune_local_values(
    kind: &str,
    links: &mut [Option<super::LinkTarget>],
    values: &mut super::ValueSlots,
    deleted: &std::collections::BTreeSet<String>,
) -> Result<(), CodecError> {
    let slots = link_carrier_slots(kind, values, links.len())?;
    let mut removed = std::collections::BTreeSet::new();
    for (index, link) in links.iter_mut().enumerate() {
        let remove = link
            .as_ref()
            .filter(|link| link.document().is_none())
            .and_then(|link| link.object())
            .is_some_and(|target| deleted.contains(target));
        if remove {
            removed.insert(index);
        }
        if remove {
            *link = None;
        }
    }
    let mut kept = std::collections::BTreeSet::new();
    values.retain_mut(|value| {
        if let Some((_, slot)) = slots.iter().find(|(order, _)| *order == value.order) {
            if removed.contains(slot) {
                return false;
            }
            // Preserve an authored dense association before removal introduces
            // holes, so a later pruning pass retains the same slot authority.
            if value.parent.is_some() {
                value.link_index = Some(*slot);
            }
        }
        if let Some(parent) = value.parent {
            if !kept.contains(&parent) {
                return false;
            }
        }
        kept.insert(value.order);
        true
    });
    Ok(())
}

/// Rewrites typed target identities while source XML attributes remain literal names.
pub(crate) fn rewrite_local_links<F: FnMut(&str) -> Result<String, CodecError>>(
    links: &mut [Option<super::LinkTarget>],
    map: &mut IdentityMap<'_, F>,
) -> Result<(), CodecError> {
    for link in links
        .iter_mut()
        .flatten()
        .filter(|link| link.document().is_none())
    {
        if let Some(target) = link.object() {
            link.set_object(
                cadmpeg_core::text::NonBlankString::new(map.identity(target)?)
                    .ok_or_else(|| CodecError::malformed("rewritten link identity is empty"))?,
            );
        }
    }
    Ok(())
}

pub(crate) struct DocumentValue(PropertyRecord);

impl DocumentValue {
    pub(crate) fn new(property: PropertyRecord) -> Result<Self, CodecError> {
        if property.type_name == "App::PropertyString" {
            return Err(CodecError::malformed(
                "string property has a different typed owner",
            ));
        }
        Ok(Self(property))
    }
}

impl Serialize for DocumentValue {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        self.0.serialize(serializer)
    }
}
impl<'de> Deserialize<'de> for DocumentValue {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        Self::new(PropertyRecord::deserialize(deserializer)?).map_err(serde::de::Error::custom)
    }
}
impl RewriteIdentities for DocumentValue {
    fn visit_identity_references(
        &self,
        _ctx: &DecodeContext<'_>,
        visitor: &mut dyn FnMut(&str) -> Result<(), CodecError>,
    ) -> Result<(), CodecError> {
        visitor(&self.0.owner)?;
        for link in self.0.links().iter().flatten() {
            if link.document().is_none() {
                if let Some(target) = link.object() {
                    visitor(target)?;
                }
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
            target: &self.0.owner,
            position: DependencyPosition::Required,
            field: Some("owner"),
        })?;
        for (index, link) in self.0.links().iter().enumerate() {
            if let Some(link) = link.as_ref().filter(|link| link.document().is_none()) {
                if let Some(target) = link.object() {
                    visitor(DependencyReference {
                        target,
                        position: if self.0.type_name.contains("List")
                            || crate::persistence::document_properties::value_parts::links::expression_container(&self.0.type_name)
                        {
                            DependencyPosition::List {
                                length: self.0.links().len(),
                                index,
                            }
                        } else {
                            DependencyPosition::Required
                        },
                        field: Some("links"),
                    })?;
                }
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
        if !effect.can_apply() || effect == ReferencePruneEffect::Unchanged {
            return Ok(effect);
        }
        if let Some(PropertyBody::Persisted { links, values, .. }) = &mut self.0.body {
            prune_local_values(&self.0.type_name, links, values, deleted)?;
        }
        Ok(effect)
    }
    fn rewrite_identities<F: FnMut(&str) -> Result<String, CodecError>>(
        mut self,
        _ctx: &DecodeContext<'_>,
        map: &mut IdentityMap<'_, F>,
    ) -> Result<Self, CodecError> {
        self.0.id = map.identity(&self.0.id)?;
        self.0.owner = map.identity(&self.0.owner)?;
        if let Some(PropertyBody::Persisted { links, .. }) = &mut self.0.body {
            rewrite_local_links(links, map)?;
        }
        Ok(self)
    }
    fn rewrite_native_value<F: FnMut(&str) -> Result<String, CodecError>>(
        _ctx: &DecodeContext<'_>,
        value: &mut serde_json::Value,
        map: &mut IdentityMap<'_, F>,
    ) -> Result<(), CodecError> {
        for field in ["id", "owner"] {
            let target = value
                .get_mut(field)
                .ok_or_else(|| CodecError::malformed("document property identity is absent"))?;
            let original = target
                .as_str()
                .ok_or_else(|| CodecError::malformed("document property identity is not text"))?;
            *target = map.identity(original)?.into();
        }
        if let Some(links) = value
            .get_mut("links")
            .and_then(serde_json::Value::as_array_mut)
        {
            for link in links {
                if link
                    .get("document")
                    .is_some_and(|document| !document.is_null())
                {
                    continue;
                }
                if let Some(target) = link.get_mut("object") {
                    if let Some(original) = target.as_str().filter(|text| !text.is_empty()) {
                        *target = map.identity(original)?.into();
                    }
                }
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests;
