// SPDX-License-Identifier: Apache-2.0
//! Decoded property carriers and their serialized representation.

use cadmpeg_ir::scalar::FiniteReal;
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
    Float(FiniteReal),
    /// Floating-point distance with its serialized unit code.
    Distance {
        /// Serialized Protein unit code.
        unit: u32,
        /// Distance value in the serialized unit.
        value: FiniteReal,
    },
    /// UTF-8 string.
    String(String),
    /// Four-channel floating-point color.
    Color([FiniteReal; 4]),
    /// Ordered texture URI values.
    TextureUri(Vec<String>),
    /// A member declared `allowmultiplevalues="true"` on a carrier other than
    /// `TextureURI`: a `u32` count followed by that many carrier values.
    Multiple(RepeatedValues),
}

/// A homogeneous sequence of non-recursive scalar carriers.
#[derive(Clone, Debug, Default, PartialEq, Deserialize)]
#[serde(try_from = "Vec<PropertyValue>")]
pub struct RepeatedValues(Vec<PropertyValue>);

impl RepeatedValues {
    /// Wraps members that one scalar carrier produced.
    ///
    /// The record reader builds every member of a repeated property with the
    /// same scalar carrier, which is the invariant the checked conversion
    /// tests, so its result needs no second pass.
    pub(crate) fn of_one_scalar_carrier(values: Vec<PropertyValue>) -> Self {
        Self(values)
    }

    /// Borrows the checked scalar sequence.
    pub fn values(&self) -> &[PropertyValue] {
        &self.0
    }

    /// Moves the checked sequence without copying it.
    pub fn into_values(self) -> Vec<PropertyValue> {
        self.0
    }
}

/// The sequence contains a recursive or mixed carrier layout.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RepeatedValueError {
    /// A member is a list or a texture URI carrier.
    NonScalar,
    /// Members do not have the same scalar carrier.
    Heterogeneous,
}

impl std::fmt::Display for RepeatedValueError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(match self {
            Self::NonScalar => "multiple values must contain scalar carriers",
            Self::Heterogeneous => "multiple values must have one scalar carrier",
        })
    }
}

impl std::error::Error for RepeatedValueError {}

impl TryFrom<Vec<PropertyValue>> for RepeatedValues {
    type Error = RepeatedValueError;

    fn try_from(values: Vec<PropertyValue>) -> Result<Self, Self::Error> {
        let carrier = values.first().map(std::mem::discriminant);
        for value in &values {
            if matches!(
                value,
                PropertyValue::Multiple(_) | PropertyValue::TextureUri(_)
            ) {
                return Err(RepeatedValueError::NonScalar);
            }
            if Some(std::mem::discriminant(value)) != carrier {
                return Err(RepeatedValueError::Heterogeneous);
            }
        }
        Ok(Self(values))
    }
}

impl Serialize for RepeatedValues {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        self.0.serialize(serializer)
    }
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
    /// Byte offset of the value payload, after a scalar unit tag or at a count prefix.
    value_offset: usize,
    /// Decoded carrier payload; a reference carrier states `reference`, and
    /// repeated reference carriers state `multiple` of them.
    value: PropertyValueWire,
    /// Connected asset identifiers in serialized order; empty for a carrier
    /// the schema does not declare connectable.
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
            PropertyValue::Float(value) => Self::Float(value.get()),
            PropertyValue::String(value) => Self::String(value),
            PropertyValue::Color(value) => Self::Color(value.map(FiniteReal::get)),
            PropertyValue::TextureUri(value) => Self::TextureUri(value),
            PropertyValue::Distance { unit, value } => Self::Distance {
                unit,
                value: value.get(),
            },
            PropertyValue::Multiple(values) => {
                Self::Multiple(values.into_values().into_iter().map(Into::into).collect())
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
            PropertyValueWire::Float(value) => {
                Self::Float(FiniteReal::new(value).ok_or("float must be finite")?)
            }
            PropertyValueWire::String(value) => Self::String(value),
            PropertyValueWire::Color(value) => {
                let [r, g, b, a] = value.map(FiniteReal::new);
                Self::Color([
                    r.ok_or("color must be finite")?,
                    g.ok_or("color must be finite")?,
                    b.ok_or("color must be finite")?,
                    a.ok_or("color must be finite")?,
                ])
            }
            PropertyValueWire::TextureUri(value) => Self::TextureUri(value),
            PropertyValueWire::Distance { unit, value } => Self::Distance {
                unit,
                value: FiniteReal::new(value).ok_or("distance must be finite")?,
            },
            PropertyValueWire::Multiple(values) => Self::Multiple(
                values
                    .into_iter()
                    .map(TryInto::try_into)
                    .collect::<Result<Vec<_>, _>>()?
                    .try_into()
                    .map_err(|error: RepeatedValueError| error.to_string())?,
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
    use std::num::NonZeroUsize;

    use super::{DecodedProperty, PropertyContent, PropertyValue};

    #[test]
    fn empty_references_preserve_the_flat_wire() {
        let property = DecodedProperty {
            value_offset: 4,
            content: PropertyContent::Value {
                value: PropertyValue::Multiple(super::RepeatedValues::default()),
                connections: vec!["target".into()],
            },
        };
        let encoded = serde_json::to_string(&property).expect("serialize empty reference carrier");
        assert_eq!(
            serde_json::from_str::<DecodedProperty>(&encoded)
                .expect("deserialize empty reference carrier"),
            property
        );
        assert_eq!(
            property.value(),
            Some(&PropertyValue::Multiple(super::RepeatedValues::default()))
        );
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
                    value: PropertyValue::Multiple(super::RepeatedValues::default()),
                    connections: vec!["target".into()],
                },
                r#"{"value_offset":4,"value":{"kind":"multiple","value":[]},"connections":["target"]}"#,
            ),
            (
                PropertyContent::Value {
                    value: PropertyValue::Float(
                        cadmpeg_ir::scalar::FiniteReal::new(1.5).expect("finite"),
                    ),
                    connections: Vec::new(),
                },
                r#"{"value_offset":4,"value":{"kind":"float","value":1.5},"connections":[]}"#,
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
    fn property_wire_rejects_nonfinite_scalars_and_colors() {
        for value in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
            for wire in [
                super::PropertyValueWire::Float(value),
                super::PropertyValueWire::Distance {
                    unit: 0x200e,
                    value,
                },
                super::PropertyValueWire::Color([value, 0.25, 1.0, 1.0]),
            ] {
                assert!(PropertyValue::try_from(wire).is_err());
            }
        }
        for wire in [
            r#"{"kind":"float","value":1e400}"#,
            r#"{"kind":"distance","value":{"unit":8206,"value":1e400}}"#,
            r#"{"kind":"color","value":[1e400,0.25,1.0,1.0]}"#,
        ] {
            assert!(serde_json::from_str::<PropertyValue>(wire).is_err());
        }
    }

    #[test]
    fn repeated_carriers_reject_mixed_nested_and_texture_members() {
        use super::{RepeatedValueError, RepeatedValues};
        assert_eq!(
            RepeatedValues::try_from(vec![
                PropertyValue::Float(cadmpeg_ir::scalar::FiniteReal::new(1.5).expect("finite")),
                PropertyValue::Boolean(true)
            ]),
            Err(RepeatedValueError::Heterogeneous)
        );
        assert_eq!(
            RepeatedValues::try_from(vec![PropertyValue::Multiple(RepeatedValues::default())]),
            Err(RepeatedValueError::NonScalar)
        );
        assert_eq!(
            RepeatedValues::try_from(vec![PropertyValue::TextureUri(Vec::new())]),
            Err(RepeatedValueError::NonScalar)
        );
        for value in [
            r#"{"kind":"multiple","value":[{"kind":"float","value":1.5},{"kind":"boolean","value":true}]}"#,
            r#"{"kind":"multiple","value":[{"kind":"multiple","value":[]}]}"#,
            r#"{"kind":"multiple","value":[{"kind":"texture_uri","value":[]}]}"#,
        ] {
            assert!(serde_json::from_str::<PropertyValue>(value).is_err());
            let property =
                format!("{{\"value_offset\":4,\"value\":{value},\"connections\":[\"target\"]}}");
            assert!(serde_json::from_str::<DecodedProperty>(&property).is_err());
        }
        let values =
            RepeatedValues::try_from(vec![PropertyValue::Integer(1), PropertyValue::Integer(2)])
                .expect("same carrier");
        let value = PropertyValue::Multiple(values);
        let wire = r#"{"kind":"multiple","value":[{"kind":"integer","value":1},{"kind":"integer","value":2}]}"#;
        assert_eq!(serde_json::to_string(&value).expect("serialize"), wire);
        assert_eq!(
            serde_json::from_str::<PropertyValue>(wire).expect("homogeneous wire"),
            value
        );
    }

    #[test]
    fn mixed_reference_and_scalar_elements_are_rejected_at_deserialization() {
        let wire = r#"{"value_offset":0,"value":{"kind":"multiple","value":[{"kind":"reference"},{"kind":"float","value":1.5}]},"connections":[]}"#;
        let error = serde_json::from_str::<DecodedProperty>(wire)
            .expect_err("reject contradictory property payload");
        assert!(error.to_string().contains("value: reference elements"));
    }
}
