// SPDX-License-Identifier: Apache-2.0
//! Paired field names and value-record identities on the unchanged wire.

use super::ParasolidAttributeFieldNames;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct NamedField {
    pub(crate) value_record: String,
    pub(crate) name: String,
}

#[derive(Clone, Serialize, Deserialize)]
pub(super) struct FieldNamesWire {
    id: String,
    stream_ordinal: u32,
    attribute_definition: String,
    field_names_record: String,
    value_records: Vec<String>,
    names: Vec<String>,
}
impl From<ParasolidAttributeFieldNames> for FieldNamesWire {
    fn from(value: ParasolidAttributeFieldNames) -> Self {
        let (value_records, names) = value
            .fields
            .into_iter()
            .map(|field| (field.value_record, field.name))
            .unzip();
        Self {
            id: value.id,
            stream_ordinal: value.stream_ordinal,
            attribute_definition: value.attribute_definition,
            field_names_record: value.field_names_record,
            value_records,
            names,
        }
    }
}
impl TryFrom<FieldNamesWire> for ParasolidAttributeFieldNames {
    type Error = &'static str;
    fn try_from(wire: FieldNamesWire) -> Result<Self, Self::Error> {
        if wire.value_records.len() != wire.names.len() {
            return Err("value_records/names: must have equal lengths");
        }
        Ok(Self {
            id: wire.id,
            stream_ordinal: wire.stream_ordinal,
            attribute_definition: wire.attribute_definition,
            field_names_record: wire.field_names_record,
            fields: wire
                .value_records
                .into_iter()
                .zip(wire.names)
                .map(|(value_record, name)| NamedField { value_record, name })
                .collect(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::ParasolidAttributeFieldNames;

    #[test]
    fn field_name_wire_preserves_pairs_empty_names_and_empty_lists() {
        for (records, names) in [("[]", "[]"), (r#"["record-a","record-b"]"#, r#"["","μ"]"#)] {
            let json = format!(
                r#"{{"id":"relation","stream_ordinal":3,"attribute_definition":"definition","field_names_record":"list","value_records":{records},"names":{names}}}"#
            );
            let relation: ParasolidAttributeFieldNames = serde_json::from_str(&json).unwrap();
            assert_eq!(serde_json::to_string(&relation).unwrap(), json);
            for field in ["value_records", "names"] {
                let mut wire = serde_json::to_value(&relation).unwrap();
                wire[field].as_array_mut().unwrap().push("extra".into());
                let error =
                    serde_json::from_value::<ParasolidAttributeFieldNames>(wire).unwrap_err();
                assert!(error.to_string().contains("value_records/names"));
            }
        }
    }
}
