// SPDX-License-Identifier: Apache-2.0
//! Borrowed class and field registry metadata serialization.

use super::{ClassDefinition, FieldDefinition};
use serde::ser::SerializeMap;
use serde::Serialize;

impl Serialize for ClassDefinition {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let registry = self.registry_layout();
        let mut wire = serializer.serialize_map(None)?;
        wire.serialize_entry("id", &self.id)?;
        wire.serialize_entry("name", &self.name)?;
        wire.serialize_entry("ordinal", &self.ordinal)?;
        wire.serialize_entry("trailing_code", &self.trailing_code)?;
        if let Some(registry) = registry {
            wire.serialize_entry("registry_storage_code", &registry.storage_code.value())?;
            wire.serialize_entry(
                "registry_base_class",
                &registry.base_class.map_or(0, std::num::NonZeroU32::get),
            )?;
            wire.serialize_entry("registry_reference", &registry.reference.get())?;
        }
        if let Some(reference) = &self.registry_reference_declaration {
            wire.serialize_entry("registry_reference_declaration", reference)?;
        }
        if !self.registry_suffix.is_empty() {
            wire.serialize_entry("registry_suffix", &self.registry_suffix)?;
        }
        if let Some(fingerprint) = registry.map(|value| value.schema_fingerprint) {
            wire.serialize_entry(
                "schema_fingerprint",
                &cadmpeg_ir::native::bytes::NativeBytes::from(fingerprint),
            )?;
        }
        wire.serialize_entry("section_offset", &self.section_offset)?;
        wire.serialize_entry("source_entry", &self.source_entry)?;
        wire.serialize_entry("source_offset", &self.source_offset)?;
        wire.end()
    }
}

impl Serialize for FieldDefinition {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let registry = self.registry_layout();
        let mut wire = serializer.serialize_map(None)?;
        wire.serialize_entry("id", &self.id)?;
        wire.serialize_entry("name", &self.name)?;
        wire.serialize_entry("ordinal", &self.ordinal)?;
        wire.serialize_entry("trailing_code", &self.trailing_code)?;
        if let Some(registry) = registry {
            wire.serialize_entry("registry_storage_code", &registry.storage_code.value())?;
            wire.serialize_entry("registry_owner_class", &registry.owner_class.get())?;
        }
        if !self.registry_suffix.is_empty() {
            wire.serialize_entry("registry_suffix", &self.registry_suffix)?;
        }
        wire.serialize_entry("section_offset", &self.section_offset)?;
        wire.serialize_entry("source_entry", &self.source_entry)?;
        wire.serialize_entry("source_offset", &self.source_offset)?;
        wire.end()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use cadmpeg_test_support::native_serialization::assert_native_limit;

    #[test]
    fn class_registry_reference_roundtrip_and_source_bounds() {
        let reference = serde_json::from_value(serde_json::json!({
            "name": "UGS::OM::Meta", "code": 2, "source_offset": 1475
        }))
        .unwrap();
        let class = ClassDefinition {
            id: "nx:om-entry-5:class#643".to_owned(),
            name: "UGS::OM::RootObject".to_owned(),
            ordinal: 0,
            trailing_code: 0x38,
            registry_suffix: vec![0x16, 0xb0, 0x4b, 0xa5, 0xfe, 0x91, 0x1b, 0xf6, 0x79, 0x01]
                .into(),
            registry_reference_declaration: Some(reference),
            section_offset: 1364,
            source_entry: "/Root/FastLoad/RMFastLoad".to_owned(),
            source_offset: 2007,
        };
        let value = serde_json::to_value(&class).unwrap();
        assert_eq!(value["registry_reference"], 1);
        assert_eq!(
            value["registry_reference_declaration"]["name"],
            "UGS::OM::Meta"
        );
        assert_eq!(value["registry_reference_declaration"]["code"], 2);
        assert_eq!(value["schema_fingerprint"], "b04ba5fe911bf679");
        assert!(value.get("layout_prefix").is_none());
        assert!(value.get("layout_terminal").is_none());
        assert_eq!(
            serde_json::from_value::<ClassDefinition>(value.clone()).unwrap(),
            class
        );
        assert_eq!(
            serde_json::to_vec(&class).unwrap(),
            serde_json::to_vec(&super::super::ClassDefinitionWire::from(class.clone())).unwrap()
        );
        assert_native_limit(&class, value.clone());
        let mut invalid = value.clone();
        invalid["registry_reference_declaration"]["source_offset"] = 2007.into();
        assert!(serde_json::from_value::<ClassDefinition>(invalid).is_err());
        invalid = value.clone();
        invalid["registry_reference_declaration"]["name"] = "unframed name".into();
        assert!(serde_json::from_value::<ClassDefinition>(invalid).is_err());
        invalid = value.clone();
        invalid["registry_reference_declaration"]["unexpected"] = true.into();
        assert!(serde_json::from_value::<ClassDefinition>(invalid).is_err());
        invalid = value.clone();
        invalid["registry_reference_declaration"] = serde_json::Value::Null;
        assert!(serde_json::from_value::<ClassDefinition>(invalid).is_err());
        invalid = value;
        invalid["schema_fingerprint"] = "b14ba5fe911bf679".into();
        assert!(serde_json::from_value::<ClassDefinition>(invalid).is_err());
    }

    #[test]
    fn class_registry_borrowed_bytes_and_limit() {
        for suffix in [
            vec![0x05, 0x10, 0x20, 0x30, 0x40, 0x50, 0x60, 0x70, 0x80, 0x02],
            vec![
                0x81, 0x21, 0x01, 0x23, 0x45, 0x67, 0x89, 0xab, 0xcd, 0xef, 0x06,
            ],
            Vec::new(),
        ] {
            let class = ClassDefinition {
                id: "nx:om-class:class#0".to_owned(),
                name: "UGS::FEATURE_RECORD".to_owned(),
                ordinal: 0,
                trailing_code: 0x38,
                registry_suffix: suffix.into(),
                registry_reference_declaration: None,
                section_offset: 0,
                source_entry: "entry".to_owned(),
                source_offset: 1,
            };
            let borrowed = serde_json::to_vec(&class).unwrap();
            let owned = serde_json::to_vec(&super::super::ClassDefinitionWire::from(class.clone()))
                .unwrap();
            assert_eq!(borrowed, owned);
            assert_native_limit(&class, serde_json::to_value(&class).unwrap());
        }
    }

    #[test]
    fn field_registry_borrowed_bytes_and_limit() {
        for suffix in [
            vec![0x01, 0x02],
            vec![
                0x81, 0x21, 0x01, 0x23, 0x45, 0x67, 0x89, 0xab, 0xcd, 0xef, 0x06,
            ],
            Vec::new(),
        ] {
            let field = FieldDefinition {
                id: "nx:om-field:field#0".to_owned(),
                name: "m_target".to_owned(),
                ordinal: 0,
                trailing_code: 0x81,
                registry_suffix: suffix.into(),
                section_offset: 0,
                source_entry: "entry".to_owned(),
                source_offset: 1,
            };
            let borrowed = serde_json::to_vec(&field).unwrap();
            let owned = serde_json::to_vec(&super::super::FieldDefinitionWire::from(field.clone()))
                .unwrap();
            assert_eq!(borrowed, owned);
            assert_native_limit(&field, serde_json::to_value(&field).unwrap());
        }
    }
}
