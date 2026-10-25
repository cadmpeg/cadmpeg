// SPDX-License-Identifier: Apache-2.0
//! Typed application declarations and their ordered dependency references.

use super::ObjectRecord;
use cadmpeg_core::decode::DecodeContext;
use cadmpeg_core::CodecError;
use cadmpeg_ir::schema::rewrite::typed::prune::{
    DependencyPosition, DependencyReference, ReferencePruneEffect,
};
use cadmpeg_ir::schema::rewrite::typed::{IdentityMap, RewriteIdentities};
use serde::{Deserialize, Serialize};

pub(crate) struct ObjectDeclaration(ObjectRecord);
impl ObjectDeclaration {
    pub(crate) fn new(record: ObjectRecord) -> Result<Self, CodecError> {
        Ok(Self(record))
    }
}
impl Serialize for ObjectDeclaration {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        self.0.serialize(serializer)
    }
}
impl<'de> Deserialize<'de> for ObjectDeclaration {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        Self::new(ObjectRecord::deserialize(deserializer)?).map_err(serde::de::Error::custom)
    }
}
impl RewriteIdentities for ObjectDeclaration {
    fn visit_identity_references(
        &self,
        _ctx: &DecodeContext<'_>,
        visitor: &mut dyn FnMut(&str) -> Result<(), CodecError>,
    ) -> Result<(), CodecError> {
        for dependency in &self.0.dependencies {
            visitor(dependency)?;
        }
        Ok(())
    }
    fn visit_dependency_references(
        &self,
        _ctx: &DecodeContext<'_>,
        visitor: &mut dyn FnMut(DependencyReference<'_>) -> Result<(), CodecError>,
    ) -> Result<(), CodecError> {
        for (index, target) in self.0.dependencies.iter().enumerate() {
            visitor(DependencyReference {
                target,
                position: DependencyPosition::List {
                    length: self.0.dependencies.len(),
                    index,
                },
                field: Some("dependencies"),
            })?;
        }
        Ok(())
    }
    fn prune_reference_lists(
        &mut self,
        ctx: &DecodeContext<'_>,
        deleted: &std::collections::BTreeSet<String>,
    ) -> Result<ReferencePruneEffect, CodecError> {
        let effect = cadmpeg_ir::schema::rewrite::typed::prune::analyze(self, ctx, deleted)?;
        self.0.dependencies.retain(|id| !deleted.contains(id));
        Ok(effect)
    }
    fn rewrite_identities<F: FnMut(&str) -> Result<String, CodecError>>(
        mut self,
        _ctx: &DecodeContext<'_>,
        map: &mut IdentityMap<'_, F>,
    ) -> Result<Self, CodecError> {
        self.0.identity = super::object_identity::ObjectIdentity::try_new(
            map.identity(self.0.id())?,
            self.0.name().clone(),
        )
        .map_err(CodecError::Malformed)?;
        for dependency in &mut self.0.dependencies {
            *dependency = map.identity(dependency)?;
        }
        Ok(self)
    }
    fn rewrite_native_value<F: FnMut(&str) -> Result<String, CodecError>>(
        _ctx: &DecodeContext<'_>,
        value: &mut serde_json::Value,
        map: &mut IdentityMap<'_, F>,
    ) -> Result<(), CodecError> {
        let id = value
            .get("id")
            .and_then(serde_json::Value::as_str)
            .ok_or_else(|| CodecError::malformed("object declaration identity is absent"))?;
        let name = value
            .get("name")
            .and_then(serde_json::Value::as_str)
            .ok_or_else(|| CodecError::malformed("object declaration name is absent"))?;
        let identity =
            super::object_identity::ObjectIdentity::try_new(map.identity(id)?, name.to_owned())
                .map_err(CodecError::Malformed)?;
        value["id"] = identity.id().clone().into();
        let dependencies = value
            .get_mut("dependencies")
            .and_then(serde_json::Value::as_array_mut)
            .ok_or_else(|| CodecError::malformed("object dependencies are absent"))?;
        for dependency in dependencies {
            let id = dependency
                .as_str()
                .ok_or_else(|| CodecError::malformed("object dependency is not text"))?;
            *dependency = map.identity(id)?.into();
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn declaration_owner_preserves_name_identity_and_literal_type_without_reference_guesses() {
        let xml = roxmltree::Document::parse("<Document><Objects Count='1'><Object name='A' type='test:native:object#literal'/></Objects><ObjectData Count='1'><Object name='A'><Properties Count='0'/></Object></ObjectData></Document>").unwrap();
        let graph =
            crate::persistence::parse_document(&xml, crate::persistence::Vocabulary::Objects)
                .unwrap();
        let original = graph.objects[0].clone();
        let ctx = cadmpeg_test_support::service_decode_context();
        let value = ObjectDeclaration::new(original.clone()).unwrap();
        let mut refs = Vec::new();
        value
            .visit_identity_references(&ctx, &mut |id| {
                refs.push(id.to_owned());
                Ok(())
            })
            .unwrap();
        assert!(refs.is_empty());
        let mut unchanged = IdentityMap::new(|id: &str| Ok(id.to_owned()));
        let kept = value.rewrite_identities(&ctx, &mut unchanged).unwrap();
        assert_eq!(
            serde_json::to_value(kept).unwrap(),
            serde_json::to_value(&original).unwrap()
        );
        let mut changed = IdentityMap::new(|id: &str| Ok(id.replace("#", "#changed-")));
        let changed = ObjectDeclaration::new(original.clone())
            .unwrap()
            .rewrite_identities(&ctx, &mut changed)
            .unwrap();
        let mut expected = serde_json::to_value(&original).unwrap();
        expected["id"] = original.id().replace("#", "#changed-").into();
        assert_eq!(serde_json::to_value(changed).unwrap(), expected);
        let mut with_dependency = original;
        with_dependency
            .dependencies
            .push("test:native:object#dependency".into());
        let with_dependency = ObjectDeclaration::new(with_dependency).unwrap();
        let mut refs = Vec::new();
        with_dependency
            .visit_identity_references(&ctx, &mut |id| {
                refs.push(id.to_owned());
                Ok(())
            })
            .unwrap();
        assert_eq!(refs, ["test:native:object#dependency"]);
    }
}
