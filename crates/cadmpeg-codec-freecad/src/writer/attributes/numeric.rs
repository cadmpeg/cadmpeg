// SPDX-License-Identifier: Apache-2.0
//! Current nullable numeric attributes own their emitted member and XML pointer.

use super::*;
use cadmpeg_core::CodecError;
use cadmpeg_ir::codec::write::{ExportLossCode, NativeDisposition, NativeDispositions};
use std::collections::BTreeSet;
mod encoding;
mod mesh_material;

#[derive(Default)]
pub(in crate::writer) struct Prepared {
    pub(in crate::writer) xml: BTreeMap<String, Option<String>>,
    pub(in crate::writer) claimed: BTreeSet<String>,
    emitted_entries: BTreeMap<String, Vec<String>>,
    members: BTreeMap<String, String>,
}

pub(in crate::writer) fn prepare(
    ir: &CadIr,
    properties: &[crate::native::PropertyRecord],
    entries: &mut BTreeMap<String, Vec<u8>>,
    losses: &mut Vec<LossNote>,
) -> Result<Prepared, CodecError> {
    let native_entries = ir
        .native
        .namespace("fcstd")
        .map(|namespace| namespace.arena_as::<crate::native::EntryRecord>("entries"))
        .transpose()?
        .unwrap_or_default();
    let mut reserved = entries.keys().cloned().collect::<BTreeSet<_>>();
    if let Some(namespace) = ir.native.namespace("fcstd") {
        reserved.extend(
            namespace
                .arena_as::<crate::native::EntryRecord>("entries")?
                .iter()
                .map(|entry| entry.name().to_owned()),
        );
    }
    let mut prepared = Prepared::default();
    for (ordinal, attribute) in ir.model.attributes.iter().enumerate() {
        let original = properties
            .iter()
            .find(|property| attribute.native_ref.as_deref() == Some(property.id.as_str()));
        if let Some(original) =
            original.filter(|property| property.type_name == "Mesh::PropertyMaterial")
        {
            prepared.claimed.insert(original.id.clone());
        }
        let encoded = if mesh_material::matches(&attribute.values) {
            mesh_material::encode(&attribute.values, &attribute.id.to_string(), losses)?
        } else if let [AttributeValue::NumericList(list)] = attribute.values.as_slice() {
            encoding::encode(list, original, &attribute.id.to_string(), losses)?
        } else {
            continue;
        };
        if let Some(original) = original {
            prepared.claimed.insert(original.id.clone());
        }
        let encoded = match encoded {
            Ok(encoded) => Some(encoded),
            Err(encoding::Omission::Missing) => {
                losses.push(ExportLossCode::SourceUndecodedOmitted.note(format!(
                    "numeric attribute {} ({}) is omitted: its arity or a positional row is not held; the binary list grammar has no null-row representation",attribute.id,attribute.name)));
                None
            }
            Err(encoding::Omission::OutOfRange) => None,
        };
        if let Some(original) = original {
            let emitted = native_entries
                .iter()
                .filter(|entry| {
                    original
                        .side_entries()
                        .iter()
                        .any(|name| name == entry.name())
                })
                .map(|entry| entry.id().to_owned())
                .collect();
            prepared
                .emitted_entries
                .insert(attribute.id.to_string(), emitted);
        }
        let xml = if let Some(encoded) = encoded {
            if let Some(inline) = encoded.inline {
                let generated = xml::property(&attribute.name, &encoded.runtime, &inline);
                let xml = if let Some(original) = original {
                    super::structured::encode_generated(
                        attribute,
                        original,
                        &HashMap::new(),
                        &generated,
                    )?
                } else {
                    Some(generated)
                };
                prepared.xml.insert(attribute.id.to_string(), xml);
                continue;
            }
            let kind = encoded.tag;
            let bytes = encoded.bytes;
            // A fresh member prevents another property's old shared pointer from
            // overwriting this attribute's current values.
            let mut suffix = 0_u64;
            let file = loop {
                let candidate = format!("Attribute_{ordinal}_{suffix}.bin");
                if reserved.insert(candidate.clone()) {
                    break candidate;
                }
                suffix = suffix.checked_add(1).ok_or_else(|| {
                    CodecError::malformed("numeric attribute member-name alternatives exceed u64")
                })?;
            };
            entries.insert(file.clone(), bytes);
            prepared
                .members
                .insert(attribute.id.to_string(), file.clone());
            let generated = xml::property(
                &attribute.name,
                &encoded.runtime,
                &xml::element(kind, &[("file", file.clone())]),
            );
            if let Some(original) = original {
                let mut property = original.clone();
                property.name.clone_from(&attribute.name);
                property.type_name = encoded.runtime;
                property.family = crate::persistence::property_family(&property.type_name);
                let mut root = original
                    .values()
                    .iter()
                    .find(|value| value.tag == kind && value.parent.is_none())
                    .cloned()
                    .unwrap_or(crate::native::ValueRecord {
                        tag: kind.to_owned(),
                        order: 0,
                        parent: None,
                        attributes: BTreeMap::new(),
                        decoded_json: None,
                        text: None,
                        item_index: None,
                        link_index: None,
                    });
                root.attributes.insert("file".into(), file.clone());
                property.body = Some(crate::native::PropertyBody::Persisted {
                    values: vec![root].into(),
                    links: Vec::new(),
                    side_entries: Vec::new(),
                });
                xml::native_named_payloads(&property, &HashMap::new(), &BTreeSet::from([file]))?
            } else {
                Some(generated)
            }
        } else {
            None
        };
        prepared.xml.insert(attribute.id.to_string(), xml);
    }
    Ok(prepared)
}

impl Prepared {
    pub(in crate::writer) fn emitted(&self, id: &str, outcomes: &mut NativeDispositions) {
        if let Some(rows) = self.emitted_entries.get(id) {
            for row in rows {
                super::super::dispositions::record(
                    outcomes,
                    "entries",
                    row,
                    NativeDisposition::Regenerated,
                );
            }
        }
    }
    pub(in crate::writer) fn omitted(&self, id: &str, entries: &mut BTreeMap<String, Vec<u8>>) {
        if let Some(member) = self.members.get(id) {
            entries.remove(member);
        }
    }
}
