// SPDX-License-Identifier: Apache-2.0
//! Decoded property carriers and their serialized representation.

use serde::{Deserialize, Serialize};
use std::num::NonZeroUsize;

/// One typed property decoded according to its packaged schema.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(into = "DecodedPropertyWire", try_from = "DecodedPropertyWire")]
pub struct DecodedProperty {
    /// Byte offset of the value payload, after a scalar unit tag or at a count prefix.
    pub value_offset: usize,
    /// Value and the connections owned by its carrier.
    pub content: PropertyContent,
}

/// A reference owns one connection list, including for multiple-value carriers.
#[derive(Clone, Debug, PartialEq)]
pub enum PropertyContent {
    /// A non-reference value, optionally declared connectable by its schema.
    Value {
        /// Decoded scalar or multiple-value payload.
        value: PropertyValue,
        /// Empty for a carrier the schema does not declare connectable.
        connections: Vec<String>,
    },
    /// A single reference carrier.
    Reference(Vec<String>),
    /// Repeated reference carriers share one connection block.
    MultipleReferences {
        /// Number of zero-byte reference values preceding the connection block.
        count: NonZeroUsize,
        /// Connected asset identifiers in serialized order.
        targets: Vec<String>,
    },
}

impl DecodedProperty {
    /// Non-reference payload, when this property has one.
    #[must_use]
    pub fn value(&self) -> Option<&PropertyValue> {
        match &self.content {
            PropertyContent::Value { value, .. } => Some(value),
            PropertyContent::Reference(_) | PropertyContent::MultipleReferences { .. } => None,
        }
    }

    /// Connected asset identifiers in serialized order.
    #[must_use]
    pub fn connections(&self) -> &[String] {
        match &self.content {
            PropertyContent::Value { connections, .. } => connections,
            PropertyContent::Reference(targets)
            | PropertyContent::MultipleReferences { targets, .. } => targets,
        }
    }
}

/// A schema-defined Protein property value.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", content = "value", rename_all = "snake_case")]
pub enum PropertyValue {
    /// Boolean scalar.
    Boolean(bool),
    /// Unsigned integer scalar.
    Integer(u32),
    /// Unitless floating-point scalar.
    Float(f64),
    /// Floating-point distance with its serialized unit code.
    Distance {
        /// Serialized Protein unit code.
        unit: u32,
        /// Distance value in the serialized unit.
        value: f64,
    },
    /// UTF-8 string.
    String(String),
    /// Four-channel floating-point color.
    Color([f64; 4]),
    /// Ordered texture URI values.
    TextureUri(Vec<String>),
    /// A member declared `allowmultiplevalues="true"` on a carrier other than
    /// `TextureURI`: a `u32` count followed by that many carrier values.
    Multiple(Vec<PropertyValue>),
}

/// A schema-defined Protein property value.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", content = "value", rename_all = "snake_case")]
enum PropertyValueWire {
    /// Boolean scalar.
    Boolean(bool),
    /// Unsigned integer scalar.
    Integer(u32),
    /// Unitless floating-point scalar.
    Float(f64),
    /// Floating-point distance with its serialized unit code.
    Distance {
        /// Serialized Protein unit code.
        unit: u32,
        /// Distance value in the serialized unit.
        value: f64,
    },
    /// UTF-8 string.
    String(String),
    /// Four-channel floating-point color.
    Color([f64; 4]),
    /// Reference carrier whose target is held by its connection list.
    Reference,
    /// Ordered texture URI values.
    TextureUri(Vec<String>),
    /// A member declared `allowmultiplevalues="true"` on a carrier other than
    /// `TextureURI`: a `u32` count followed by that many carrier values.
    Multiple(Vec<PropertyValueWire>),
}

#[derive(Serialize, Deserialize)]
struct DecodedPropertyWire {
    value_offset: usize,
    value: PropertyValueWire,
    connections: Vec<String>,
}

impl From<DecodedProperty> for DecodedPropertyWire {
    fn from(property: DecodedProperty) -> Self {
        let (value, connections) = match property.content {
            PropertyContent::Value { value, connections } => (value.into(), connections),
            PropertyContent::Reference(targets) => (PropertyValueWire::Reference, targets),
            PropertyContent::MultipleReferences { count, targets } => (
                PropertyValueWire::Multiple(
                    (0..count.get())
                        .map(|_| PropertyValueWire::Reference)
                        .collect(),
                ),
                targets,
            ),
        };
        Self {
            value_offset: property.value_offset,
            value,
            connections,
        }
    }
}

impl TryFrom<DecodedPropertyWire> for DecodedProperty {
    type Error = String;

    fn try_from(wire: DecodedPropertyWire) -> Result<Self, Self::Error> {
        let content = match wire.value {
            PropertyValueWire::Reference => PropertyContent::Reference(wire.connections),
            PropertyValueWire::Multiple(values) => {
                if let Some(count) = NonZeroUsize::new(values.len()).filter(|_| {
                    values
                        .iter()
                        .all(|value| matches!(value, PropertyValueWire::Reference))
                }) {
                    PropertyContent::MultipleReferences {
                        count,
                        targets: wire.connections,
                    }
                } else {
                    PropertyContent::Value {
                        value: PropertyValueWire::Multiple(values).try_into()?,
                        connections: wire.connections,
                    }
                }
            }
            value => PropertyContent::Value {
                value: value.try_into()?,
                connections: wire.connections,
            },
        };
        Ok(Self {
            value_offset: wire.value_offset,
            content,
        })
    }
}

impl From<PropertyValue> for PropertyValueWire {
    fn from(value: PropertyValue) -> Self {
        match value {
            PropertyValue::Boolean(value) => Self::Boolean(value),
            PropertyValue::Integer(value) => Self::Integer(value),
            PropertyValue::Float(value) => Self::Float(value),
            PropertyValue::String(value) => Self::String(value),
            PropertyValue::Color(value) => Self::Color(value),
            PropertyValue::TextureUri(value) => Self::TextureUri(value),
            PropertyValue::Distance { unit, value } => Self::Distance { unit, value },
            PropertyValue::Multiple(values) => {
                Self::Multiple(values.into_iter().map(Into::into).collect())
            }
        }
    }
}

impl TryFrom<PropertyValueWire> for PropertyValue {
    type Error = String;

    fn try_from(value: PropertyValueWire) -> Result<Self, Self::Error> {
        Ok(match value {
            PropertyValueWire::Boolean(value) => Self::Boolean(value),
            PropertyValueWire::Integer(value) => Self::Integer(value),
            PropertyValueWire::Float(value) => Self::Float(value),
            PropertyValueWire::String(value) => Self::String(value),
            PropertyValueWire::Color(value) => Self::Color(value),
            PropertyValueWire::TextureUri(value) => Self::TextureUri(value),
            PropertyValueWire::Distance { unit, value } => Self::Distance { unit, value },
            PropertyValueWire::Multiple(values) => Self::Multiple(
                values
                    .into_iter()
                    .map(TryInto::try_into)
                    .collect::<Result<_, _>>()?,
            ),
            PropertyValueWire::Reference => {
                return Err(
                    "value: reference elements must share one property connection block".into(),
                )
            }
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_references_preserve_the_flat_wire() {
        let property = DecodedProperty {
            value_offset: 4,
            content: PropertyContent::Value {
                value: PropertyValue::Multiple(Vec::new()),
                connections: vec!["target".into()],
            },
        };
        let encoded = serde_json::to_string(&property).expect("serialize empty reference carrier");
        assert_eq!(
            serde_json::from_str::<DecodedProperty>(&encoded)
                .expect("deserialize empty reference carrier"),
            property
        );
        assert_eq!(property.value(), Some(&PropertyValue::Multiple(Vec::new())));
        assert_eq!(
            serde_json::to_string(&property).expect("serialize empty references"),
            r#"{"value_offset":4,"value":{"kind":"multiple","value":[]},"connections":["target"]}"#
        );
    }

    #[test]
    fn carrier_payloads_preserve_the_flat_wire() {
        let cases = [
            (
                PropertyContent::Reference(vec!["target".into()]),
                r#"{"value_offset":4,"value":{"kind":"reference"},"connections":["target"]}"#,
            ),
            (
                PropertyContent::MultipleReferences {
                    count: NonZeroUsize::new(2).expect("two reference carriers"),
                    targets: vec!["target".into()],
                },
                r#"{"value_offset":4,"value":{"kind":"multiple","value":[{"kind":"reference"},{"kind":"reference"}]},"connections":["target"]}"#,
            ),
            (
                PropertyContent::Value {
                    value: PropertyValue::Multiple(Vec::new()),
                    connections: vec!["target".into()],
                },
                r#"{"value_offset":4,"value":{"kind":"multiple","value":[]},"connections":["target"]}"#,
            ),
            (
                PropertyContent::Value {
                    value: PropertyValue::Float(1.5),
                    connections: Vec::new(),
                },
                r#"{"value_offset":4,"value":{"kind":"float","value":1.5},"connections":[]}"#,
            ),
            (
                PropertyContent::Value {
                    value: PropertyValue::Multiple(vec![
                        PropertyValue::Float(1.5),
                        PropertyValue::Boolean(true),
                    ]),
                    connections: vec!["target".into()],
                },
                r#"{"value_offset":4,"value":{"kind":"multiple","value":[{"kind":"float","value":1.5},{"kind":"boolean","value":true}]},"connections":["target"]}"#,
            ),
        ];
        for (content, expected) in cases {
            let property = DecodedProperty {
                value_offset: 4,
                content,
            };
            assert_eq!(
                serde_json::to_string(&property).expect("serialize decoded property"),
                expected
            );
            let decoded: DecodedProperty =
                serde_json::from_str(expected).expect("decode property fixture");
            assert_eq!(
                serde_json::to_string(&decoded).expect("serialize round-trip property"),
                expected
            );
            assert_eq!(decoded.connections(), property.connections());
            assert_eq!(decoded, property);
        }
    }

    #[test]
    fn mixed_reference_and_scalar_elements_are_rejected_at_deserialization() {
        let wire = r#"{"value_offset":0,"value":{"kind":"multiple","value":[{"kind":"reference"},{"kind":"float","value":1.5}]},"connections":[]}"#;
        let error = serde_json::from_str::<DecodedProperty>(wire)
            .expect_err("reject contradictory property payload");
        assert!(error.to_string().contains("value: reference elements"));
    }
}
