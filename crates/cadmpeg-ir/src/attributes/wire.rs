// SPDX-License-Identifier: Apache-2.0
//! Attribute read grammar and linked-object validation.

use super::{AttributeExpression, AttributeJson, AttributeMapEntry, AttributeValue};
use crate::scalar::FiniteReal;
use serde::Deserialize;

#[derive(Deserialize)]
#[serde(
    tag = "kind",
    content = "value",
    rename_all = "snake_case",
    deny_unknown_fields
)]
pub(super) enum AttributeValueWire {
    /// A signed integer value.
    Integer(i64),
    /// A floating-point value.
    Float(FiniteReal),
    /// A text value.
    String(String),
    /// A boolean value.
    Boolean(bool),
    /// A string-encoded reference to another entity, opaque to this crate.
    Reference(#[serde(deserialize_with = "super::deserialize_local_object_reference")] String),
    /// A fixed- or variable-length numeric vector value.
    Vector(Vec<FiniteReal>),
    /// Ordered source map entries, including repeated keys.
    Map(Vec<AttributeMapEntry>),
    /// A property expression not consumed by a held parameter.
    Expression(AttributeExpression),
    /// A local object link with source subelement names.
    SubReference {
        /// Native object identity, absent when the source states no object.
        #[serde(deserialize_with = "cadmpeg_core::absent_key::nullable")]
        object: Option<String>,
        /// Ordered subelement names.
        subelements: Vec<String>,
    },
    /// A link into another document.
    ExternalReference {
        /// Source document name or path.
        document: String,
        /// External object name, absent when the source states no object.
        #[serde(deserialize_with = "cadmpeg_core::absent_key::nullable")]
        object: Option<String>,
        /// Ordered subelement names.
        subelements: Vec<String>,
    },
    /// A readable local link slot with no object and no subelement name.
    EmptyReference,
    /// A closed, typed JSON tree.
    Json(AttributeJson),
}

impl TryFrom<AttributeValueWire> for AttributeValue {
    type Error = &'static str;

    fn try_from(value: AttributeValueWire) -> Result<Self, Self::Error> {
        Ok(match value {
            AttributeValueWire::Integer(value) => Self::Integer(value),
            AttributeValueWire::Float(value) => Self::Float(value),
            AttributeValueWire::String(value) => Self::String(value),
            AttributeValueWire::Boolean(value) => Self::Boolean(value),
            AttributeValueWire::Reference(value) => Self::Reference(value),
            AttributeValueWire::Vector(value) => Self::Vector(value),
            AttributeValueWire::Map(value) => Self::Map(value),
            AttributeValueWire::Expression(value) => Self::Expression(value),
            AttributeValueWire::SubReference {
                object,
                subelements,
            } => {
                if object.as_ref().is_some_and(String::is_empty) {
                    return Err("an absent linked object uses null");
                }
                Self::SubReference {
                    object,
                    subelements,
                }
            }
            AttributeValueWire::ExternalReference {
                document,
                object,
                subelements,
            } => {
                if object.as_ref().is_some_and(String::is_empty) {
                    return Err("an absent linked object uses null");
                }
                Self::ExternalReference {
                    document,
                    object,
                    subelements,
                }
            }
            AttributeValueWire::EmptyReference => Self::EmptyReference,
            AttributeValueWire::Json(value) => Self::Json(value),
        })
    }
}
