// SPDX-License-Identifier: Apache-2.0

use super::deltas_event_id;
use crate::framing::xmt_reference::NonNullXmt;
use crate::records::deltas::{Definition, Event, EventContent};
use crate::records::instance::FieldValue;
use crate::records::{positive, SchemaPlan};
use cadmpeg_core::decode::{u64_from_index, DecodeContext, View};
use cadmpeg_core::CodecError;
use cadmpeg_ir::hash::digest::Sha256Digest;
use cadmpeg_ir::native::{NativeConvertError, NativeNamespace};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::rc::Rc;

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(try_from = "SchemaWire")]
pub(in crate::native) struct ParasolidDeltasSchema {
    pub(in crate::native) id: String,
    pub(in crate::native) stream_ordinal: u32,
    pub(in crate::native) inflated_offset: u64,
    byte_len: u64,
    definition: Rc<SchemaPlan>,
    sha256: Sha256Digest,
}

#[derive(Serialize)]
struct SchemaRef<'a> {
    id: &'a str,
    stream_ordinal: u32,
    inflated_offset: u64,
    byte_len: u64,
    definition: &'a SchemaPlan,
    custom_name: Option<&'a str>,
    custom_description: Option<&'a str>,
    sha256: &'a Sha256Digest,
}

impl Serialize for ParasolidDeltasSchema {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        SchemaRef {
            id: &self.id,
            stream_ordinal: self.stream_ordinal,
            inflated_offset: self.inflated_offset,
            byte_len: self.byte_len,
            definition: &self.definition,
            custom_name: self.definition.custom_name.as_deref(),
            custom_description: self.definition.custom_description.as_deref(),
            sha256: &self.sha256,
        }
        .serialize(serializer)
    }
}

#[derive(Deserialize)]
struct SchemaWire {
    id: String,
    stream_ordinal: u32,
    inflated_offset: u64,
    byte_len: u64,
    definition: SchemaPlan,
    custom_name: Option<String>,
    custom_description: Option<String>,
    sha256: Sha256Digest,
}

impl TryFrom<SchemaWire> for ParasolidDeltasSchema {
    type Error = &'static str;
    fn try_from(mut wire: SchemaWire) -> Result<Self, Self::Error> {
        if wire.definition.kind < 2
            || wire.definition.fields.len() >= 255
            || wire.byte_len < 2
            || wire.inflated_offset.checked_add(wire.byte_len).is_none()
            || wire.custom_name.is_some() != wire.custom_description.is_some()
            || wire.id
                != format!(
                    "nx:s{}:deltas-schema#{}",
                    wire.stream_ordinal, wire.inflated_offset
                )
        {
            return Err("invalid Deltas schema identity, metadata, or range");
        }
        wire.definition.custom_name = wire.custom_name;
        wire.definition.custom_description = wire.custom_description;
        Ok(Self {
            id: wire.id,
            stream_ordinal: wire.stream_ordinal,
            inflated_offset: wire.inflated_offset,
            byte_len: wire.byte_len,
            definition: Rc::new(wire.definition),
            sha256: wire.sha256,
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(in crate::native) struct FieldWire {
    inflated_offset: u64,
    byte_len: u64,
    values: Vec<FieldValue>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(in crate::native) struct ParasolidDeltasInstance {
    pub(in crate::native) id: String,
    pub(in crate::native) stream_ordinal: u32,
    schema: String,
    pub(in crate::native) inflated_offset: u64,
    byte_len: u64,
    identity_offset: u64,
    xmt: NonNullXmt,
    variable_length: Option<u32>,
    fields: Vec<FieldWire>,
    sha256: Sha256Digest,
}

impl ParasolidDeltasSchema {
    pub(super) fn from_owned(source: &[u8], ordinal: u32, definition: &Definition) -> Self {
        let range = definition.declaration();
        Self {
            id: deltas_event_id(
                cadmpeg_core::decode::index_from_u32(ordinal),
                "deltas-schema",
                range.start,
                None,
            ),
            stream_ordinal: ordinal,
            inflated_offset: u64_from_index(range.start),
            byte_len: u64_from_index(range.len()),
            definition: definition.shared_plan(),
            sha256: Sha256Digest::digest(&source[range]),
        }
    }
}

impl ParasolidDeltasInstance {
    pub(super) fn from_owned(
        source: &[u8],
        ordinal: u32,
        event: &Event,
        schema: &ParasolidDeltasSchema,
    ) -> Result<Self, CodecError> {
        let EventContent::Instance(instance) = event.content() else {
            return Err(CodecError::Malformed("expected NX Deltas instance".into()));
        };
        let mut fields = Vec::new();
        for (field, range) in instance
            .definition()
            .plan()
            .fields
            .iter()
            .zip(instance.fields())
        {
            let mut view = View::over_retained(&source[range.clone()]);
            let mut values = Vec::new();
            while !view.is_empty() {
                let value = FieldValue::read(field.data_type, &mut view)
                    .ok_or_else(|| CodecError::Malformed("invalid owned NX Deltas field".into()))?;
                values.push(value);
            }
            fields.push(FieldWire {
                inflated_offset: u64_from_index(range.start),
                byte_len: u64_from_index(range.len()),
                values,
            });
        }
        let range = event.range();
        let value = Self {
            id: deltas_event_id(
                cadmpeg_core::decode::index_from_u32(ordinal),
                "deltas-instance",
                range.start,
                None,
            ),
            stream_ordinal: ordinal,
            schema: schema.id.clone(),
            inflated_offset: u64_from_index(range.start),
            byte_len: u64_from_index(range.len()),
            identity_offset: u64_from_index(instance.identity_at()),
            xmt: instance.xmt(),
            variable_length: instance.variable_length(),
            fields,
            sha256: Sha256Digest::digest(&source[range]),
        };
        value.validate_source(schema, source)?;
        Ok(value)
    }

    /// The source projections with downstream readers use declared field names.
    /// BODY has no role projection: its values remain generic physical fields.
    pub(super) fn project_record(
        &self,
        schema: &ParasolidDeltasSchema,
    ) -> Option<crate::deltas::record_family::RecordFamily> {
        use crate::deltas::record_family::RecordFamily;
        let plan = &schema.definition;
        let region =
            plan.kind == 19 && crate::deltas::region::RegionLayout::from_schema(plan).is_some();
        let group = plan.matches_base_with_pointer_tail(90, &[]);
        if !region && !group {
            return None;
        }
        let mut references = Vec::new();
        let mut node_id = None;
        let mut selector = None;
        let mut region_type = None;
        let mut linked_status = None;
        for (descriptor, field) in plan.fields.iter().zip(&self.fields) {
            let [value] = field.values.as_slice() else {
                return None;
            };
            match value {
                FieldValue::Pointer { target, status } => {
                    references.push(target.get());
                    if descriptor.name.as_str() == "first_member" {
                        linked_status =
                            crate::deltas::group::GroupReferenceStatus::try_from(u8::from(*status))
                                .ok();
                    }
                }
                FieldValue::Integer(value) if descriptor.name.as_str() == "node_id" => {
                    node_id = Some(*value);
                }
                FieldValue::Byte(value) if group && descriptor.name.as_str() == "type" => {
                    selector = crate::records::group::GroupSelector::try_from(*value).ok();
                }
                FieldValue::Char(value) if region && descriptor.name.as_str() == "type" => {
                    region_type = crate::deltas::region::RegionType::try_from(*value).ok();
                }
                _ => return None,
            }
        }
        (|| {
            if group {
                Some(RecordFamily::Group {
                    node_id: node_id?,
                    selector: selector?,
                    linked_reference_status: linked_status?,
                    references: references.try_into().ok()?,
                })
            } else {
                Some(RecordFamily::Region {
                    fields: crate::deltas::region::RegionFields::from_wire(
                        node_id?,
                        references,
                        region_type?,
                    )?,
                })
            }
        })()
    }

    fn validate(&self, schema: &ParasolidDeltasSchema) -> Result<(), &'static str> {
        let end = self
            .inflated_offset
            .checked_add(self.byte_len)
            .ok_or("Deltas instance range overflow")?;
        let schema_end = schema
            .inflated_offset
            .checked_add(schema.byte_len)
            .ok_or("Deltas schema range overflow")?;
        let first = self.inflated_offset == schema.inflated_offset;
        let prefix_end = if first {
            schema_end
        } else {
            if schema_end > self.inflated_offset {
                return Err("Deltas schema does not precede instance");
            }
            self.inflated_offset
                .checked_add(2)
                .ok_or("Deltas kind range overflow")?
        };
        let variable = schema
            .definition
            .fields
            .iter()
            .any(|field| field.n_elements == 1);
        if !(schema.definition.kind == 2 || schema.definition.kind > 4)
            || self.schema != schema.id
            || self.stream_ordinal != schema.stream_ordinal
            || self.id
                != format!(
                    "nx:s{}:deltas-instance#{}",
                    self.stream_ordinal, self.inflated_offset
                )
            || self.variable_length.is_some() != variable
            || self.fields.len() != schema.definition.fields.len()
            || prefix_end.checked_add(if variable { 4 } else { 0 }) != Some(self.identity_offset)
        {
            return Err("Deltas schema join, field cardinality, or identity range differs");
        }
        let mut at = self
            .fields
            .first()
            .map_or(end, |field| field.inflated_offset);
        let identity_width = if u32::from(self.xmt) <= 32_767 { 2 } else { 4 };
        if at.checked_sub(self.identity_offset) != Some(identity_width) {
            return Err("invalid Deltas identity width");
        }
        for (descriptor, field) in schema.definition.fields.iter().zip(&self.fields) {
            let count = if !descriptor.transmit {
                0
            } else if descriptor.n_elements == 1 {
                self.variable_length
                    .ok_or("missing Deltas variable count")?
            } else {
                descriptor.n_elements.max(1)
            };
            if field.inflated_offset != at
                || usize::try_from(count).ok() != Some(field.values.len())
                || field
                    .values
                    .iter()
                    .any(|value| value.data_type() != descriptor.data_type)
                || (field.values.is_empty() != (field.byte_len == 0))
                || field
                    .values
                    .iter()
                    .try_fold(0u64, |length, value| {
                        length.checked_add(value.encoded_len())
                    })
                    .is_none_or(|length| length != field.byte_len)
            {
                return Err("Deltas field range, cardinality, or type differs from schema");
            }
            at = at
                .checked_add(field.byte_len)
                .ok_or("Deltas field range overflow")?;
        }
        if at != end {
            return Err("Deltas fields do not end at instance endpoint");
        }
        Ok(())
    }

    // The schema is shared directly with the owner at extraction. Reopened source
    // validation must re-read and compare every schema, including prefix declarations;
    // a matching declaration hash alone does not authenticate a serialized plan.
    fn validate_source(
        &self,
        schema: &ParasolidDeltasSchema,
        source: &[u8],
    ) -> Result<(), CodecError> {
        self.validate(schema)
            .map_err(|message| CodecError::Malformed(message.into()))?;
        let bad =
            || CodecError::Malformed("NX Deltas native values differ from retained source".into());
        let range = |offset: u64, length: u64| -> Option<std::ops::Range<usize>> {
            Some(usize::try_from(offset).ok()?..usize::try_from(offset.checked_add(length)?).ok()?)
        };
        let event = source
            .get(range(self.inflated_offset, self.byte_len).ok_or_else(bad)?)
            .ok_or_else(bad)?;
        let declaration = source
            .get(range(schema.inflated_offset, schema.byte_len).ok_or_else(bad)?)
            .ok_or_else(bad)?;
        if View::u16_be_at(event, 0) != Some(schema.definition.kind)
            || Sha256Digest::digest(event) != self.sha256
            || Sha256Digest::digest(declaration) != schema.sha256
        {
            return Err(bad());
        }
        let mut identity = View::over_retained(source);
        identity
            .seek(usize::try_from(self.identity_offset).map_err(|_| bad())?)
            .ok_or_else(bad)?;
        if positive(&mut identity).and_then(|value| value.checked_add(1)) != Some(self.xmt.into()) {
            return Err(bad());
        }
        let first_field = self
            .fields
            .first()
            .map_or(self.inflated_offset + self.byte_len, |field| {
                field.inflated_offset
            });
        if u64_from_index(identity.position()) != first_field {
            return Err(bad());
        }
        if let Some(count) = self.variable_length {
            let at = usize::try_from(self.identity_offset.checked_sub(4).ok_or_else(bad)?)
                .map_err(|_| bad())?;
            if View::u32_be_at(source, at) != Some(count) {
                return Err(bad());
            }
        }
        for (descriptor, field) in schema.definition.fields.iter().zip(&self.fields) {
            let mut view = View::over_retained(
                source
                    .get(range(field.inflated_offset, field.byte_len).ok_or_else(bad)?)
                    .ok_or_else(bad)?,
            );
            for value in &field.values {
                if FieldValue::read(descriptor.data_type, &mut view).as_ref() != Some(value) {
                    return Err(bad());
                }
            }
            if !view.is_empty() {
                return Err(bad());
            }
        }
        Ok(())
    }
}

pub(in crate::native) fn admit(
    ctx: &DecodeContext<'_>,
    namespace: &NativeNamespace,
) -> Result<(), NativeConvertError> {
    let mut declarations =
        namespace.arena_as_for_decode::<ParasolidDeltasSchema>(ctx, "parasolid_deltas_schemas")?;
    declarations.sort_unstable_by_key(|schema| (schema.stream_ordinal, schema.inflated_offset));
    let mut schemas = BTreeMap::new();
    let mut kinds = std::collections::BTreeSet::new();
    let mut declaration_ends = BTreeMap::<u32, u64>::new();
    for schema in declarations {
        if schemas.contains_key(&schema.id) {
            return Err(NativeConvertError::InvalidCollection(
                "duplicate Deltas schema".into(),
            ));
        }
        let kind = (schema.stream_ordinal, schema.definition.kind);
        if kinds.contains(&kind)
            || declaration_ends
                .get(&schema.stream_ordinal)
                .is_some_and(|end| *end > schema.inflated_offset)
        {
            return Err(NativeConvertError::InvalidCollection(
                "duplicate Deltas kind or overlapping declaration".into(),
            ));
        }
        kinds.insert(kind);
        declaration_ends.insert(
            schema.stream_ordinal,
            schema.inflated_offset + schema.byte_len,
        );
        schemas.insert(schema.id.clone(), schema);
    }
    let mut previous = BTreeMap::<u32, u64>::new();
    let mut first_uses = std::collections::BTreeSet::new();
    let mut instances = namespace
        .arena_as_for_decode::<ParasolidDeltasInstance>(ctx, "parasolid_deltas_instances")?;
    instances.sort_unstable_by_key(|instance| (instance.stream_ordinal, instance.inflated_offset));
    for instance in instances {
        let schema = schemas
            .get(&instance.schema)
            .ok_or_else(|| NativeConvertError::InvalidOwner(instance.schema.clone()))?;
        if instance.inflated_offset == schema.inflated_offset {
            first_uses.insert(schema.id.clone());
        }
        instance
            .validate(schema)
            .map_err(|message| NativeConvertError::InvalidCollection(message.into()))?;
        if previous
            .get(&instance.stream_ordinal)
            .is_some_and(|end| *end > instance.inflated_offset)
        {
            return Err(NativeConvertError::InvalidCollection(
                "overlapping Deltas instances".into(),
            ));
        }
        previous.insert(
            instance.stream_ordinal,
            instance.inflated_offset + instance.byte_len,
        );
    }
    if schemas.values().any(|schema| {
        (schema.definition.kind == 2 || schema.definition.kind > 4)
            && !first_uses.contains(&schema.id)
    }) {
        return Err(NativeConvertError::InvalidCollection(
            "Deltas schema has no transactional first instance".into(),
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::{owned_deltas, with_decode_context};

    #[test]
    fn native_admission_rejects_missing_schema_wrong_cardinality_ranges_and_source_bytes() {
        let bytes = crate::test_support::test_pmarks::complete(owned_deltas::body_stream(36));
        with_decode_context(|_ctx| {
            let owner = crate::records::DeltasRecords::read(&bytes).unwrap();
            let schema = ParasolidDeltasSchema::from_owned(&bytes, 0, &owner.definitions()[2]);
            let instance =
                ParasolidDeltasInstance::from_owned(&bytes, 0, &owner.events()[2], &schema)
                    .unwrap();
            instance.validate_source(&schema, &bytes).unwrap();
            assert!(cadmpeg_ir::ids::is_valid_identity(&schema.id));
            assert!(cadmpeg_ir::ids::is_valid_identity(&instance.id));
            let wire = serde_json::to_value(&instance).unwrap();
            let round_trip: ParasolidDeltasInstance = serde_json::from_value(wire.clone()).unwrap();
            assert_eq!(round_trip, instance);
            for (key, value) in [
                ("schema", serde_json::json!("missing")),
                ("identity_offset", serde_json::json!(0)),
                ("byte_len", serde_json::json!(1)),
                ("stream_ordinal", serde_json::json!(2)),
            ] {
                let mut bad = wire.clone();
                bad[key] = value;
                let value: ParasolidDeltasInstance = serde_json::from_value(bad).unwrap();
                assert!(value.validate(&schema).is_err());
            }
            for field_key in ["byte_len", "inflated_offset"] {
                let mut bad = wire.clone();
                bad["fields"][0][field_key] = 0.into();
                let value: ParasolidDeltasInstance = serde_json::from_value(bad).unwrap();
                assert!(value.validate(&schema).is_err());
            }
            let mut bad = wire.clone();
            bad["fields"][0]["values"] = serde_json::json!([]);
            assert!(serde_json::from_value::<ParasolidDeltasInstance>(bad)
                .unwrap()
                .validate(&schema)
                .is_err());
            let mut changed = bytes.clone();
            let last = owner.events()[2].range().end - 1;
            changed[last] ^= 1;
            assert!(instance.validate_source(&schema, &changed).is_err());
            let mut changed_value = instance.clone();
            changed_value.fields[0].values[0] = FieldValue::Integer(0);
            assert!(changed_value.validate_source(&schema, &bytes).is_err());
        });
    }

    #[test]
    fn native_body_reals_preserve_nonfinite_bits_and_share_the_owner_plan() {
        for bits in [0x7ff8_1234_5678_9abcu64, 0x7ff0_0000_0000_0000, 1] {
            let mut bytes =
                crate::test_support::test_pmarks::complete(owned_deltas::body_stream(36));
            let span = with_decode_context(|_ctx| {
                let owner = crate::records::DeltasRecords::read(&bytes).unwrap();
                let EventContent::Instance(instance) = owner.events()[2].content() else {
                    panic!("BODY");
                };
                instance.fields()[10].clone()
            });
            assert_eq!(span.len(), 8);
            bytes[span].copy_from_slice(&bits.to_be_bytes());
            with_decode_context(|_ctx| {
                let owner = crate::records::DeltasRecords::read(&bytes).unwrap();
                let definition = &owner.definitions()[2];
                let schema = ParasolidDeltasSchema::from_owned(&bytes, 0, definition);
                assert!(Rc::ptr_eq(&schema.definition, &definition.shared_plan()));
                let instance =
                    ParasolidDeltasInstance::from_owned(&bytes, 0, &owner.events()[2], &schema)
                        .unwrap();
                let wire = serde_json::to_value(&instance).unwrap();
                assert_eq!(
                    wire["fields"][10]["values"][0]["value"],
                    format!("{bits:016x}")
                );
                let restored: ParasolidDeltasInstance = serde_json::from_value(wire).unwrap();
                restored.validate_source(&schema, &bytes).unwrap();
            });
        }
    }

    #[test]
    fn custom_metadata_is_serialized_once_and_round_trips_with_its_plan() {
        let mut bytes = crate::test_support::test_pmarks::prefix(1);
        let maximum = crate::test_support::test_pmarks::header().len() - 6;
        bytes[maximum..maximum + 2].copy_from_slice(&500u16.to_be_bytes());
        bytes.extend(b"\0\xf0\x01\x07MY_KIND\x06custom\x05value\0\0\0\x01\x01d\0\x02\0\0\0\x07");
        let bytes = crate::test_support::test_pmarks::complete(bytes);
        with_decode_context(|_ctx| {
            let owner = crate::records::DeltasRecords::read(&bytes).unwrap();
            let schema = ParasolidDeltasSchema::from_owned(&bytes, 0, &owner.definitions()[2]);
            let wire = serde_json::to_value(&schema).unwrap();
            assert_eq!(wire["custom_name"], "MY_KIND");
            assert_eq!(wire["custom_description"], "custom");
            assert!(wire["definition"].get("custom_name").is_none());
            let restored: ParasolidDeltasSchema = serde_json::from_value(wire).unwrap();
            assert_eq!(restored, schema);
        });
    }
}
