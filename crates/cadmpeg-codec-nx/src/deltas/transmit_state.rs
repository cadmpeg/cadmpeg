// SPDX-License-Identifier: Apache-2.0
//! Valid transmit-header text and consecutive identities.

use cadmpeg_core::decode::DecodeContext;
use cadmpeg_core::CodecError;
use serde::{Deserialize, Serialize};
use std::convert::Infallible;

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(try_from = "TransmitWire")]
pub(crate) struct TransmitState {
    description: String,
    schema: String,
    first_reference: u32,
}

#[derive(Serialize)]
struct TransmitRef<'a> {
    description: &'a str,
    schema: &'a str,
    references: [u32; 2],
}

impl Serialize for TransmitState {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        TransmitRef {
            description: &self.description,
            schema: &self.schema,
            references: self.references(),
        }
        .serialize(serializer)
    }
}

impl TransmitState {
    pub(super) fn new(
        description: String,
        schema: String,
        references: [u32; 2],
    ) -> Result<Self, &'static str> {
        match validate_text(
            &description,
            &schema,
            references,
            |value, valid| Ok::<_, Infallible>(value.chars().all(valid)),
            |value| Ok::<_, Infallible>(value.contains("(deltas)")),
        ) {
            Ok(validation) => validation?,
            Err(never) => match never {},
        }
        let first_reference = references[0];
        Ok(Self {
            description,
            schema,
            first_reference,
        })
    }
    pub(super) fn from_wire(
        ctx: &DecodeContext<'_>,
        description: &str,
        schema: &str,
        references: [u32; 2],
    ) -> Result<Result<Self, &'static str>, CodecError> {
        let validation = validate_text(
            description,
            schema,
            references,
            |value, valid| {
                ctx.all_by(
                    value.chars(),
                    |character| Ok(valid(character)),
                    "NX transmit text validation",
                )
            },
            |value| {
                ctx.any_by(
                    value.as_bytes().windows(8),
                    |window| Ok(window == b"(deltas)"),
                    "NX transmit deltas marker",
                )
            },
        )?;
        if let Err(error) = validation {
            return Ok(Err(error));
        }
        Ok(Ok(Self {
            description: ctx.copy_retained_text(description, "NX deltas description")?,
            schema: ctx.copy_retained_text(schema, "NX deltas schema")?,
            first_reference: references[0],
        }))
    }
    #[cfg(test)]
    pub(crate) fn description(&self) -> &str {
        &self.description
    }
    #[cfg(test)]
    pub(crate) fn schema(&self) -> &str {
        &self.schema
    }
    pub(crate) fn references(&self) -> [u32; 2] {
        [self.first_reference, self.first_reference + 1]
    }
}

fn validate_text<E>(
    description: &str,
    schema: &str,
    references: [u32; 2],
    mut all_valid: impl FnMut(&str, fn(char) -> bool) -> Result<bool, E>,
    contains_marker: impl FnOnce(&str) -> Result<bool, E>,
) -> Result<Result<(), &'static str>, E> {
    if !contains_marker(description)?
        || !all_valid(description, |byte| byte.is_ascii_graphic() || byte == ' ')?
    {
        return Ok(Err(
            "description: require printable ASCII containing (deltas)",
        ));
    }
    if schema.len() <= 4
        || !schema.starts_with("SCH_")
        || !all_valid(schema, |byte| byte.is_ascii_alphanumeric() || byte == '_')?
    {
        return Ok(Err(
            "schema: require SCH_ followed by ASCII letters, digits, or underscores",
        ));
    }
    let [first_reference, second] = references;
    if first_reference <= 1 || first_reference.checked_add(1) != Some(second) {
        return Ok(Err(
            "references: require two consecutive non-null identities",
        ));
    }
    Ok(Ok(()))
}

#[derive(Clone, Serialize, Deserialize)]
struct TransmitWire {
    description: String,
    schema: String,
    references: [u32; 2],
}

#[cfg(test)]
impl From<TransmitState> for TransmitWire {
    fn from(state: TransmitState) -> Self {
        Self {
            references: state.references(),
            description: state.description,
            schema: state.schema,
        }
    }
}
impl TryFrom<TransmitWire> for TransmitState {
    type Error = &'static str;
    fn try_from(wire: TransmitWire) -> Result<Self, Self::Error> {
        Self::new(wire.description, wire.schema, wire.references)
    }
}

#[cfg(test)]
mod tests {
    use super::TransmitState;

    #[test]
    // Keep the source-word spelling in this wire fixture.
    #[allow(clippy::unreadable_literal)]
    fn transmit_wire_preserves_fields_and_rejects_invalid_header_payload() {
        let json =
            r#"{"description":"Transmit (deltas)","schema":"SCH_1","references":[1063,1064]}"#;
        let state: TransmitState = serde_json::from_str(json).unwrap();
        assert_eq!(serde_json::to_string(&state).unwrap(), json);
        assert_eq!(
            serde_json::to_vec(&state).unwrap(),
            serde_json::to_vec(&super::TransmitWire::from(state.clone())).unwrap()
        );
        let valid = serde_json::to_value(&state).unwrap();
        for (field, value) in [
            ("description", serde_json::json!("Transmit")),
            ("description", serde_json::json!("(deltas)\n")),
            ("schema", serde_json::json!("SCH_")),
            ("schema", serde_json::json!("SCH_1!")),
            ("references", serde_json::json!([1, 2])),
            ("references", serde_json::json!([1063, 1065])),
            ("references", serde_json::json!([4294967295u32, 0])),
        ] {
            let mut wire = valid.clone();
            wire[field] = value;
            assert!(serde_json::from_value::<TransmitState>(wire)
                .unwrap_err()
                .to_string()
                .contains(field));
        }
    }

    #[test]
    fn transmit_state_native_limit_refuses_before_text_copy() {
        #[derive(serde::Serialize)]
        struct Record<'a> {
            id: &'static str,
            #[serde(flatten)]
            state: &'a TransmitState,
        }
        let state: TransmitState = serde_json::from_str(
            r#"{"description":"Transmit (deltas)","schema":"SCH_1","references":[2,3]}"#,
        )
        .unwrap();
        cadmpeg_test_support::native_serialization::assert_native_limit(
            &Record {
                id: "nx:parasolid:transmit-state#0",
                state: &state,
            },
            serde_json::json!({"id": "nx:parasolid:transmit-state#0",
                "description": "Transmit (deltas)", "schema": "SCH_1", "references": [2,3]}),
        );
    }
    #[test]
    fn transmit_text_iteration_refusal_propagates() {
        use cadmpeg_core::decode::ResourceDimension;
        use cadmpeg_core::CodecError;
        let error = crate::test_support::resource_refusal_at(
            &[],
            ResourceDimension::WorkUnits,
            "NX transmit text validation",
            |ctx| super::TransmitState::from_wire(ctx, "header (deltas)", "SCH_A", [2, 3]),
        );
        assert!(matches!(error, CodecError::ResourceLimit(limit)
            if limit.dimension == ResourceDimension::WorkUnits && limit.operation == "NX transmit text validation"));
    }
}
