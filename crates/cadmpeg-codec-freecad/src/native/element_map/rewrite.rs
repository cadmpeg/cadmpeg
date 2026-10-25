// SPDX-License-Identifier: Apache-2.0
//! Naming records distinguish semantic references from literal encodings.

use super::ElementMapRecord;
use crate::native::StringTableRecord;
use cadmpeg_core::decode::DecodeContext;
use cadmpeg_core::CodecError;
use cadmpeg_ir::schema::rewrite::typed::prune::{
    DependencyPosition, DependencyReference, ReferencePruneEffect,
};
use cadmpeg_ir::schema::rewrite::typed::{IdentityMap, RewriteIdentities};

impl RewriteIdentities for StringTableRecord {
    fn visit_identity_references(
        &self,
        _ctx: &DecodeContext<'_>,
        visit: &mut dyn FnMut(&str) -> Result<(), CodecError>,
    ) -> Result<(), CodecError> {
        if let Some(owner) = &self.owner_property {
            visit(owner)?;
        }
        Ok(())
    }
    fn visit_dependency_references(
        &self,
        _ctx: &DecodeContext<'_>,
        visit: &mut dyn FnMut(DependencyReference<'_>) -> Result<(), CodecError>,
    ) -> Result<(), CodecError> {
        if let Some(owner) = &self.owner_property {
            visit(DependencyReference {
                target: owner,
                position: DependencyPosition::Required,
                field: Some("owner_property"),
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
        if let Some(owner) = &mut self.owner_property {
            *owner = map.identity(owner)?;
        }
        Ok(self)
    }
    fn rewrite_native_value<F: FnMut(&str) -> Result<String, CodecError>>(
        _ctx: &DecodeContext<'_>,
        value: &mut serde_json::Value,
        map: &mut IdentityMap<'_, F>,
    ) -> Result<(), CodecError> {
        for field in ["id", "owner_property"] {
            rewrite_field(value, field, map)?;
        }
        Ok(())
    }
}

impl RewriteIdentities for ElementMapRecord {
    fn visit_identity_references(
        &self,
        _ctx: &DecodeContext<'_>,
        visit: &mut dyn FnMut(&str) -> Result<(), CodecError>,
    ) -> Result<(), CodecError> {
        visit(&self.property)?;
        if let Some(table) = &self.string_table {
            visit(table)?;
        }
        Ok(())
    }
    fn visit_dependency_references(
        &self,
        _ctx: &DecodeContext<'_>,
        visit: &mut dyn FnMut(DependencyReference<'_>) -> Result<(), CodecError>,
    ) -> Result<(), CodecError> {
        visit(DependencyReference {
            target: &self.property,
            position: DependencyPosition::Required,
            field: Some("property"),
        })?;
        if let Some(table) = &self.string_table {
            visit(DependencyReference {
                target: table,
                position: DependencyPosition::Required,
                field: Some("string_table"),
            })?;
        }
        Ok(())
    }
    fn prune_reference_lists(
        &mut self,
        _ctx: &DecodeContext<'_>,
        deleted: &std::collections::BTreeSet<String>,
    ) -> Result<ReferencePruneEffect, CodecError> {
        if deleted.contains(&self.property)
            || self
                .string_table
                .as_ref()
                .is_some_and(|table| deleted.contains(table))
        {
            return Ok(ReferencePruneEffect::DropRequired);
        }
        Ok(ReferencePruneEffect::Unchanged)
    }

    fn rewrite_identities<F: FnMut(&str) -> Result<String, CodecError>>(
        mut self,
        _ctx: &DecodeContext<'_>,
        map: &mut IdentityMap<'_, F>,
    ) -> Result<Self, CodecError> {
        self.id = map.identity(&self.id)?;
        self.property = map.identity(&self.property)?;
        if let Some(table) = &mut self.string_table {
            *table = map.identity(table)?;
        }
        Ok(self)
    }
    fn rewrite_native_value<F: FnMut(&str) -> Result<String, CodecError>>(
        _ctx: &DecodeContext<'_>,
        value: &mut serde_json::Value,
        map: &mut IdentityMap<'_, F>,
    ) -> Result<(), CodecError> {
        for field in ["id", "property", "string_table"] {
            rewrite_field(value, field, map)?;
        }
        Ok(())
    }
}

fn rewrite_field<F: FnMut(&str) -> Result<String, CodecError>>(
    value: &mut serde_json::Value,
    field: &str,
    map: &mut IdentityMap<'_, F>,
) -> Result<(), CodecError> {
    let target = value
        .get_mut(field)
        .ok_or_else(|| CodecError::malformed("naming identity field is absent"))?;
    if target.is_null() {
        return Ok(());
    }
    let original = target
        .as_str()
        .ok_or_else(|| CodecError::malformed("naming identity is not text"))?;
    *target = map.identity(original)?.into();
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn string_table_renaming_preserves_numeric_indices_literal_payloads_and_null_positions() {
        let literal = "fcstd:native:property#literal";
        let table = StringTableRecord::try_sparse(
            7,
            Some("fcstd:native:property#owner".into()),
            Some(true),
            Some(3),
            Some(literal.into()),
            Some(vec![
                Some(crate::native::StringTableEntry {
                    string_id: 1,
                    flags: 12,
                    components: Vec::new(),
                    payload: literal.into(),
                }),
                None,
            ]),
        )
        .unwrap();
        let ctx = cadmpeg_test_support::service_decode_context();
        let mut wire = serde_json::to_value(&table).unwrap();
        let mut map = IdentityMap::new(|id: &str| Ok(id.replace('#', "#changed-")));
        let changed = table.rewrite_identities(&ctx, &mut map).unwrap();
        StringTableRecord::rewrite_native_value(&ctx, &mut wire, &mut map).unwrap();
        assert_eq!(serde_json::to_value(&changed).unwrap(), wire);
        assert_eq!(changed.index, 7);
        assert_eq!(changed.id(), "fcstd:native:string-table#changed-7");
        assert_eq!(
            changed.owner_property.as_deref(),
            Some("fcstd:native:property#changed-owner")
        );
        assert_eq!(changed.source_entry.as_deref(), Some(literal));
        assert_eq!(
            changed.entries().unwrap()[0].as_ref().unwrap().payload,
            literal
        );
        assert!(changed.entries().unwrap()[1].is_none());
        let restored = serde_json::from_value::<StringTableRecord>(wire).unwrap();
        assert_eq!(restored, changed);
    }
    #[test]
    fn element_map_string_table_is_a_required_rewritten_native_dependency() {
        let mut record = ElementMapRecord {
            id: "fcstd:native:element-map#map".into(),
            property: "fcstd:native:property#Shape".into(),
            version: "1".into(),
            hasher_index: Some(7),
            string_table: Some("fcstd:native:string-table#7".into()),
            source_entry: None,
            map_id: 1,
            declared_count: None,
            legacy_records: None,
            postfixes: Vec::new(),
            maps: super::super::ElementMapNodes::try_from(vec![super::super::ElementMapNode {
                map_id: 1,
                groups: Vec::new(),
            }])
            .unwrap(),
        };
        let ctx = cadmpeg_test_support::service_decode_context();
        let mut references = Vec::new();
        record
            .visit_dependency_references(&ctx, &mut |reference| {
                references.push((reference.target.to_owned(), reference.position));
                Ok(())
            })
            .unwrap();
        assert!(references.contains(&(
            "fcstd:native:string-table#7".into(),
            DependencyPosition::Required
        )));
        let mut wire = serde_json::to_value(&record).unwrap();
        let mut map = IdentityMap::new(|id: &str| Ok(id.replace('#', "#changed-")));
        let changed = record.clone().rewrite_identities(&ctx, &mut map).unwrap();
        ElementMapRecord::rewrite_native_value(&ctx, &mut wire, &mut map).unwrap();
        assert_eq!(serde_json::to_value(changed).unwrap(), wire);
        assert_eq!(
            record
                .prune_reference_lists(
                    &ctx,
                    &std::collections::BTreeSet::from(["fcstd:native:string-table#7".into()])
                )
                .unwrap(),
            ReferencePruneEffect::DropRequired
        );
    }
}
