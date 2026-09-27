// SPDX-License-Identifier: Apache-2.0
//! Borrowed class and field registry metadata serialization.

use super::{registry_layout, ClassDefinition, FieldDefinition};
use serde::ser::SerializeMap;
use serde::Serialize;

impl Serialize for ClassDefinition {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut tail = [0_u8; 17];
        tail[0] = self.trailing_code;
        let registry = if self.registry_suffix.len() <= 16 {
            let end = self.registry_suffix.len() + 1;
            tail[1..end].copy_from_slice(&self.registry_suffix);
            crate::om::registry::class_registry_layout(&tail[..end])
        } else {
            None
        };
        let layout = registry_layout(&self.registry_suffix);
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
        if !self.registry_suffix.is_empty() {
            wire.serialize_entry("registry_suffix", &self.registry_suffix)?;
        }
        if let Some(layout) = &layout {
            if !layout.prefix.is_empty() {
                wire.serialize_entry("layout_prefix", layout.prefix)?;
            }
        }
        if let Some(fingerprint) = registry
            .map(|value| value.schema_fingerprint)
            .or_else(|| layout.as_ref().map(|value| value.fingerprint))
        {
            wire.serialize_entry("schema_fingerprint", &fingerprint)?;
        }
        if let Some(layout) = &layout {
            wire.serialize_entry("layout_terminal", &layout.terminal)?;
        }
        wire.serialize_entry("section_offset", &self.section_offset)?;
        wire.serialize_entry("source_entry", &self.source_entry)?;
        wire.serialize_entry("source_offset", &self.source_offset)?;
        wire.end()
    }
}

impl Serialize for FieldDefinition {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut tail = [0_u8; 6];
        tail[0] = self.trailing_code;
        let suffix_len = self.registry_suffix.len().min(5);
        tail[1..=suffix_len].copy_from_slice(&self.registry_suffix[..suffix_len]);
        let registry = crate::om::registry::field_registry_layout(&tail[..=suffix_len]);
        let layout = registry_layout(&self.registry_suffix);
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
        if let Some(layout) = &layout {
            if !layout.prefix.is_empty() {
                wire.serialize_entry("layout_prefix", layout.prefix)?;
            }
            wire.serialize_entry("schema_fingerprint", &layout.fingerprint)?;
            wire.serialize_entry("layout_terminal", &layout.terminal)?;
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
                registry_suffix: suffix,
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
                registry_suffix: suffix,
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
