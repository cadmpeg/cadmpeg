// SPDX-License-Identifier: Apache-2.0
//! Typed identity ownership for admitted document value properties.

use super::{PropertyBody, PropertyRecord};
use cadmpeg_core::decode::DecodeContext;
use cadmpeg_core::CodecError;
use cadmpeg_ir::schema::rewrite::typed::prune::{DependencyPosition, DependencyReference};
use cadmpeg_ir::schema::rewrite::typed::{IdentityMap, RewriteIdentities};
use serde::{Deserialize, Serialize};

pub(crate) struct DocumentValue(PropertyRecord);

impl DocumentValue {
    pub(crate) fn new(property: PropertyRecord) -> Result<Self, CodecError> {
        if !matches!(
            property.type_name.as_str(),
            "App::PropertyInteger"
                | "App::PropertyIntegerConstraint"
                | "App::PropertyPercent"
                | "App::PropertyBool"
                | "App::PropertyFloat"
                | "App::PropertyFloatConstraint"
                | "App::PropertyLength"
                | "App::PropertyAngle"
                | "App::PropertyDistance"
                | "App::PropertyQuantity"
                | "App::PropertyQuantityConstraint"
                | "App::PropertyVector"
        ) || !(matches!(&property.body, Some(PropertyBody::Persisted { links, side_entries, .. }) if links.is_empty() && side_entries.is_empty())
            || property.body.is_none())
        {
            return Err(CodecError::malformed(
                "document value has a different property grammar",
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
        visitor(&self.0.owner)
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
        })
    }
    fn rewrite_identities<F: FnMut(&str) -> Result<String, CodecError>>(
        mut self,
        _ctx: &DecodeContext<'_>,
        map: &mut IdentityMap<'_, F>,
    ) -> Result<Self, CodecError> {
        self.0.id = map.identity(&self.0.id)?;
        self.0.owner = map.identity(&self.0.owner)?;
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
        Ok(())
    }
}
