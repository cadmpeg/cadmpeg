// SPDX-License-Identifier: Apache-2.0
//! Design parameters, their owners and the companion records that carry their expressions.

use super::identity::{Located, RecordedValue};
use super::references::DesignClassTag;
use cadmpeg_core::text::NonBlankString;
use cadmpeg_ir::scalar::FiniteReal;
use serde::{Deserialize, Deserializer, Serialize};
use std::num::NonZeroU64;
use std::sync::LazyLock;

cadmpeg_core::named_optional_field!(
    deserialize_family_discriminator,
    u64,
    "family_discriminator"
);
cadmpeg_core::named_optional_field!(deserialize_owner_record_index, u32, "owner_record_index");
cadmpeg_core::named_optional_field!(deserialize_unit, String, "unit");
cadmpeg_core::named_optional_field!(deserialize_parameter_state, String, "parameter_state");
cadmpeg_core::named_optional_field!(deserialize_variant, u8, "variant");
/// Semantic family of one Design parameter record.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum DesignParameterKind {
    /// A document-level named user parameter.
    User,
    /// A dimensional constraint parameter.
    Dimension,
    /// A parameter consumed by a construction feature.
    Feature,
}

/// Admitted Design parameter family discriminator values.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u64)]
pub(crate) enum DesignParameterDiscriminator {
    Code0 = 0,
    Code3 = 3,
    Code4 = 4,
    Code5 = 5,
    Code6 = 6,
}

impl DesignParameterDiscriminator {
    pub(crate) fn code(self) -> u64 {
        match self {
            Self::Code0 => 0,
            Self::Code3 => 3,
            Self::Code4 => 4,
            Self::Code5 => 5,
            Self::Code6 => 6,
        }
    }
}

impl TryFrom<u64> for DesignParameterDiscriminator {
    type Error = String;

    fn try_from(value: u64) -> Result<Self, Self::Error> {
        match value {
            0 => Ok(Self::Code0),
            3 => Ok(Self::Code3),
            4 => Ok(Self::Code4),
            5 => Ok(Self::Code5),
            6 => Ok(Self::Code6),
            _ => Err(format!(
                "invalid design parameter family_discriminator {value}"
            )),
        }
    }
}

/// Source family and ownership of one Design parameter.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum DesignParameterSource {
    /// A document parameter. The discriminated form stores a family
    /// discriminator; the typed compact form stores none.
    User {
        family_discriminator: Option<Located<DesignParameterDiscriminator, Option<u64>>>,
    },
    Owned(OwnedDesignParameter),
}

static USER_PARAMETER_SOURCE_KIND: LazyLock<NonBlankString> =
    LazyLock::new(|| cadmpeg_core::nonblank_literal!("User Parameter"));

/// An owned source family. Its nonempty name cannot identify a user parameter.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct OwnedDesignParameter {
    source_kind: NonBlankString,
    owner_record_index: u32,
    family_discriminator: Option<Located<DesignParameterDiscriminator, Option<u64>>>,
}

impl DesignParameterSource {
    pub(crate) fn new(
        source_kind: String,
        owner_record_index: Option<u32>,
        family_discriminator: Option<Located<DesignParameterDiscriminator>>,
    ) -> Result<Self, String> {
        Self::from_parts(
            source_kind,
            owner_record_index,
            family_discriminator.map(|value| Located {
                value: value.value,
                offset: Some(value.offset),
            }),
        )
    }
    pub(crate) fn authored(
        source_kind: String,
        owner_record_index: Option<u32>,
        family_discriminator: Option<DesignParameterDiscriminator>,
    ) -> Result<Self, String> {
        Self::from_parts(
            source_kind,
            owner_record_index,
            family_discriminator.map(|value| Located {
                value,
                offset: None,
            }),
        )
    }
    fn without_layout(mut self) -> Self {
        match &mut self {
            Self::User {
                family_discriminator,
            } => {
                if let Some(value) = family_discriminator {
                    value.offset = None;
                }
            }
            Self::Owned(source) => {
                if let Some(value) = &mut source.family_discriminator {
                    value.offset = None;
                }
            }
        }
        self
    }
    fn from_parts(
        source_kind: String,
        owner_record_index: Option<u32>,
        family_discriminator: Option<Located<DesignParameterDiscriminator, Option<u64>>>,
    ) -> Result<Self, String> {
        let Some(source_kind) = NonBlankString::new(source_kind) else {
            return Err("design parameter source_kind is empty".into());
        };
        match (
            source_kind == *USER_PARAMETER_SOURCE_KIND,
            owner_record_index,
        ) {
            (true, None) => Ok(Self::User {
                family_discriminator,
            }),
            (true, Some(_)) => {
                Err("design parameter owner_record_index is present for user source_kind".into())
            }
            (false, Some(owner_record_index)) => Ok(Self::Owned(OwnedDesignParameter {
                source_kind,
                owner_record_index,
                family_discriminator,
            })),
            (false, None) => {
                Err("design parameter owner_record_index is missing for owned source_kind".into())
            }
        }
    }
}

/// One indexed Design parameter or expression record.
#[derive(Debug, PartialEq, Deserialize)]
#[cfg_attr(not(test), derive(Clone))]
#[serde(try_from = "DesignParameterSerde")]
pub(crate) struct DesignParameter {
    /// Globally unique deterministic identifier for this native record.
    pub(crate) id: String,
    /// Byte offset of the indexed record header in its Design `BulkStream`.
    byte_offset: Option<u64>,
    /// Source per-file dynamic three-digit ASCII class tag.
    pub(crate) class_tag: DesignClassTag,
    /// Source indexed-record identity.
    pub(crate) record_index: u32,
    /// Source ordering value stored by the parameter record.
    pub(crate) source_ordinal: u32,
    /// Indexed owner: user parameters have none; feature and dimension
    /// parameters name their owning record.
    source: DesignParameterSource,
    /// Typed state selected by the parameter tail reference.
    parameter_state: Option<String>,
    /// Literal or symbolic source expression.
    expression: NonBlankString,
    /// Byte offset of the expression's UTF-16LE code units.
    expression_offset: Option<u64>,
    /// Byte offset of the source-family UTF-16LE code units.
    source_kind_offset: Option<u64>,
    /// Declared unit token; absent for dimensionless and Boolean parameters.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    unit: Option<Located<NonBlankString, Option<u64>>>,
    /// Source parameter name or dimension identifier.
    name: NonBlankString,
    /// Byte offset of the name's UTF-16LE code units.
    name_offset: Option<u64>,
    /// Evaluated scalar in the record's native unit convention.
    evaluated_value: FiniteReal,
    /// Byte offset of `evaluated_value`.
    evaluated_value_offset: Option<u64>,
}

#[cfg(test)]
thread_local! {
    static PARAMETER_CLONE_COUNT: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

#[cfg(test)]
impl Clone for DesignParameter {
    fn clone(&self) -> Self {
        PARAMETER_CLONE_COUNT.with(|count| count.set(count.get() + 1));
        Self {
            id: self.id.clone(),
            byte_offset: self.byte_offset,
            class_tag: self.class_tag.clone(),
            record_index: self.record_index,
            source_ordinal: self.source_ordinal,
            source: self.source.clone(),
            parameter_state: self.parameter_state.clone(),
            expression: self.expression.clone(),
            expression_offset: self.expression_offset,
            source_kind_offset: self.source_kind_offset,
            unit: self.unit.clone(),
            name: self.name.clone(),
            name_offset: self.name_offset,
            evaluated_value: self.evaluated_value,
            evaluated_value_offset: self.evaluated_value_offset,
        }
    }
}

/// Unchecked Design parameter input.
pub(crate) struct DesignParameterDraft {
    pub(crate) id: String,
    pub(crate) byte_offset: u64,
    pub(crate) class_tag: DesignClassTag,
    pub(crate) record_index: u32,
    pub(crate) source_ordinal: u32,
    pub(crate) source: DesignParameterSource,
    pub(crate) expression: String,
    pub(crate) expression_offset: u64,
    pub(crate) source_kind_offset: u64,
    pub(crate) unit: Option<RecordedValue<String>>,
    pub(crate) name: String,
    pub(crate) name_offset: u64,
    pub(crate) evaluated_value: f64,
    pub(crate) evaluated_value_offset: u64,
}

impl TryFrom<DesignParameterDraft> for DesignParameter {
    type Error = String;
    fn try_from(draft: DesignParameterDraft) -> Result<Self, Self::Error> {
        let evaluated_value =
            FiniteReal::new(draft.evaluated_value).ok_or("evaluated_value must be finite")?;
        let expression =
            NonBlankString::new(draft.expression).ok_or("expression must not be empty")?;
        let name = NonBlankString::new(draft.name).ok_or("name must not be empty")?;
        let unit = draft
            .unit
            .map(|unit| {
                Ok::<_, String>(Located {
                    value: NonBlankString::new(unit.value).ok_or("unit must not be empty")?,
                    offset: unit.offset,
                })
            })
            .transpose()?;
        check_parameter_discriminator_offset(
            &draft.source,
            draft.byte_offset,
            draft.expression_offset,
        )?;
        if !(draft.byte_offset < draft.expression_offset
            && draft.expression_offset < draft.source_kind_offset
            && unit
                .as_ref()
                .map_or(draft.source_kind_offset < draft.name_offset, |unit| {
                    draft.source_kind_offset < unit.offset && unit.offset < draft.name_offset
                })
            && draft.name_offset < draft.evaluated_value_offset)
        {
            return Err("byte_offset, expression_offset, source_kind_offset, unit_offset, name_offset, and evaluated_value_offset must be strictly ordered".into());
        }
        Ok(Self {
            id: draft.id,
            byte_offset: Some(draft.byte_offset),
            class_tag: draft.class_tag,
            record_index: draft.record_index,
            source_ordinal: draft.source_ordinal,
            source: draft.source,
            parameter_state: None,
            expression,
            expression_offset: Some(draft.expression_offset),
            source_kind_offset: Some(draft.source_kind_offset),
            unit: unit.map(|value| Located {
                value: value.value,
                offset: Some(value.offset),
            }),
            name,
            name_offset: Some(draft.name_offset),
            evaluated_value,
            evaluated_value_offset: Some(draft.evaluated_value_offset),
        })
    }
}

fn check_parameter_discriminator_offset(
    source: &DesignParameterSource,
    byte_offset: u64,
    expression_offset: u64,
) -> Result<(), String> {
    let family_discriminator = match source {
        DesignParameterSource::User {
            family_discriminator,
        } => *family_discriminator,
        DesignParameterSource::Owned(source) => source.family_discriminator,
    };
    // The wide discriminated form stores four further zero bytes before it.
    if family_discriminator.is_some_and(|field| {
        (byte_offset.checked_add(22) != field.offset && byte_offset.checked_add(26) != field.offset)
            || field
                .offset
                .is_none_or(|offset| offset >= expression_offset)
    }) {
        return Err("family_discriminator_offset must follow byte_offset by 22 or 26 bytes and precede expression_offset".into());
    }
    Ok(())
}

impl DesignParameter {
    pub(crate) fn set_parameter_state(&mut self, state: String) {
        self.parameter_state = Some(state);
    }
    pub(crate) fn authored(content: DesignParameterContent) -> Result<Self, String> {
        Ok(Self {
            id: content.id,
            class_tag: content.class_tag,
            record_index: content.record_index,
            source_ordinal: content.source_ordinal,
            source: content.source.without_layout(),
            parameter_state: None,
            expression: NonBlankString::new(content.expression)
                .ok_or("expression must not be empty")?,
            unit: content
                .unit
                .map(|value| {
                    NonBlankString::new(value)
                        .ok_or("unit must not be empty")
                        .map(|value| Located {
                            value,
                            offset: None,
                        })
                })
                .transpose()?,
            name: NonBlankString::new(content.name).ok_or("name must not be empty")?,
            evaluated_value: FiniteReal::new(content.evaluated_value)
                .ok_or("evaluated_value must be finite")?,
            byte_offset: None,
            expression_offset: None,
            source_kind_offset: None,
            name_offset: None,
            evaluated_value_offset: None,
        })
    }
    pub(crate) fn retain_source(&self, fidelity: &mut cadmpeg_ir::SourceFidelity) {
        for (name, offset) in [
            ("byte_offset", self.byte_offset),
            ("expression_offset", self.expression_offset),
            ("source_kind_offset", self.source_kind_offset),
            ("name_offset", self.name_offset),
            ("evaluated_value_offset", self.evaluated_value_offset),
            (
                "unit_offset",
                self.unit.as_ref().and_then(|unit| unit.offset),
            ),
            (
                "family_discriminator_offset",
                self.family_discriminator().and_then(|value| value.offset),
            ),
        ] {
            if let Some(offset) = offset {
                fidelity
                    .metadata
                    .insert(format!("{}.{name}", self.id), offset.to_string());
            }
        }
    }

    #[cfg(test)]
    pub(crate) fn source(&self) -> &DesignParameterSource {
        &self.source
    }

    #[cfg(test)]
    /// Checked replacement of a present unit token.
    pub(crate) fn try_set_unit_value(&mut self, value: String) -> Result<(), String> {
        let value = NonBlankString::new(value).ok_or("unit must not be empty")?;
        let unit = self.unit.as_mut().ok_or("unit is absent")?;
        unit.value = value;
        Ok(())
    }

    /// Indexed record header offset.
    pub(crate) fn byte_offset(&self) -> Option<u64> {
        self.byte_offset
    }
    /// Expression code-unit offset.
    #[cfg(test)]
    pub(crate) fn expression_offset(&self) -> Option<u64> {
        self.expression_offset
    }
    /// Evaluated scalar offset.
    pub(crate) fn evaluated_value_offset(&self) -> Option<u64> {
        self.evaluated_value_offset
    }
    /// Evaluated scalar.
    pub(crate) fn evaluated_value(&self) -> FiniteReal {
        self.evaluated_value
    }
    /// Nonempty parameter name.
    pub(crate) fn name(&self) -> &str {
        self.name.as_str()
    }
    /// Nonempty source expression.
    pub(crate) fn expression(&self) -> &str {
        self.expression.as_str()
    }
    /// Located nonempty unit token, when present.
    pub(crate) fn unit(&self) -> Option<&Located<NonBlankString, Option<u64>>> {
        self.unit.as_ref()
    }

    #[cfg(test)]
    /// Checked replacement of the evaluated scalar.
    pub(crate) fn try_set_evaluated_value(&mut self, value: f64) -> Result<(), String> {
        self.evaluated_value = FiniteReal::new(value).ok_or("evaluated_value must be finite")?;
        Ok(())
    }
    #[cfg(test)]
    /// Checked replacement of source family and ownership.
    pub(crate) fn try_set_source(&mut self, source: DesignParameterSource) -> Result<(), String> {
        self.source = match (self.byte_offset, self.expression_offset) {
            (Some(base), Some(expression)) => {
                check_parameter_discriminator_offset(&source, base, expression)?;
                source
            }
            _ => source.without_layout(),
        };
        Ok(())
    }

    pub(crate) fn family_discriminator(
        &self,
    ) -> Option<Located<DesignParameterDiscriminator, Option<u64>>> {
        match &self.source {
            DesignParameterSource::User {
                family_discriminator,
            } => *family_discriminator,
            DesignParameterSource::Owned(source) => source.family_discriminator,
        }
    }

    pub(crate) fn source_kind(&self) -> &NonBlankString {
        match &self.source {
            DesignParameterSource::User { .. } => &USER_PARAMETER_SOURCE_KIND,
            DesignParameterSource::Owned(source) => &source.source_kind,
        }
    }

    /// The source kind as text.
    pub(crate) fn source_kind_str(&self) -> &str {
        self.source_kind().as_str()
    }

    pub(crate) fn kind(&self) -> DesignParameterKind {
        design_parameter_kind_from_source(self.source_kind_str())
    }

    pub(crate) fn owner_record_index(&self) -> Option<u32> {
        match &self.source {
            DesignParameterSource::User { .. } => None,
            DesignParameterSource::Owned(source) => Some(source.owner_record_index),
        }
    }
}

impl Serialize for DesignParameter {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        #[derive(Serialize)]
        struct WireRef<'a> {
            id: &'a str,
            class_tag: &'a str,
            record_index: u32,
            source_ordinal: u32,
            #[serde(skip_serializing_if = "Option::is_none")]
            family_discriminator: Option<u64>,
            #[serde(skip_serializing_if = "Option::is_none")]
            owner_record_index: Option<u32>,
            expression: &'a str,
            source_kind: &'a str,
            #[serde(skip_serializing_if = "Option::is_none")]
            parameter_state: Option<&'a str>,
            #[serde(skip_serializing_if = "Option::is_none")]
            unit: Option<&'a str>,
            name: &'a str,
            evaluated_value: f64,
        }
        WireRef {
            id: &self.id,
            class_tag: self.class_tag.as_str(),
            record_index: self.record_index,
            source_ordinal: self.source_ordinal,
            family_discriminator: self.family_discriminator().map(|value| value.value.code()),
            owner_record_index: self.owner_record_index(),
            expression: self.expression(),
            source_kind: self.source_kind_str(),
            parameter_state: self.parameter_state.as_deref(),
            unit: self.unit().map(|value| value.value.as_str()),
            name: self.name(),
            evaluated_value: self.evaluated_value().get(),
        }
        .serialize(serializer)
    }
}
fn design_parameter_kind_from_source(source_kind: &str) -> DesignParameterKind {
    if source_kind == "User Parameter" {
        DesignParameterKind::User
    } else if source_kind.contains("Dimension") {
        DesignParameterKind::Dimension
    } else {
        DesignParameterKind::Feature
    }
}
/// Semantic input for an authored native parameter. It has no source frame.
pub(crate) struct DesignParameterContent {
    pub(crate) id: String,
    pub(crate) class_tag: DesignClassTag,
    pub(crate) record_index: u32,
    pub(crate) source_ordinal: u32,
    pub(crate) source: DesignParameterSource,
    pub(crate) expression: String,
    pub(crate) unit: Option<String>,
    pub(crate) name: String,
    pub(crate) evaluated_value: f64,
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct DesignParameterSerde {
    id: String,
    class_tag: String,
    record_index: u32,
    source_ordinal: u32,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_family_discriminator"
    )]
    family_discriminator: Option<u64>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_owner_record_index"
    )]
    owner_record_index: Option<u32>,
    expression: String,
    source_kind: String,
    /// Typed state selected by the parameter tail reference.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_parameter_state"
    )]
    parameter_state: Option<String>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_unit"
    )]
    unit: Option<String>,
    name: String,
    evaluated_value: f64,
}
impl TryFrom<DesignParameterSerde> for DesignParameter {
    type Error = String;
    fn try_from(wire: DesignParameterSerde) -> Result<Self, String> {
        let source = DesignParameterSource::authored(
            wire.source_kind,
            wire.owner_record_index,
            wire.family_discriminator
                .map(DesignParameterDiscriminator::try_from)
                .transpose()?,
        )?;
        let parameter_state = wire.parameter_state;
        if parameter_state
            .as_ref()
            .is_some_and(|state| state.trim().is_empty())
        {
            return Err("parameter_state must not be blank".into());
        }
        let mut value = Self::authored(DesignParameterContent {
            id: wire.id,
            class_tag: wire.class_tag.try_into()?,
            record_index: wire.record_index,
            source_ordinal: wire.source_ordinal,
            source,
            expression: wire.expression,
            unit: wire.unit,
            name: wire.name,
            evaluated_value: wire.evaluated_value,
        })?;
        value.parameter_state = parameter_state;
        Ok(value)
    }
}
#[cfg(test)]
impl From<DesignParameter> for DesignParameterSerde {
    fn from(parameter: DesignParameter) -> Self {
        let source_kind = parameter.source_kind_str().to_owned();
        let family_discriminator = parameter
            .family_discriminator()
            .map(|value| value.value.code());
        let owner_record_index = parameter.owner_record_index();
        Self {
            id: parameter.id,
            class_tag: parameter.class_tag.into(),
            record_index: parameter.record_index,
            source_ordinal: parameter.source_ordinal,
            family_discriminator,
            owner_record_index,
            expression: parameter.expression.to_string(),
            source_kind,
            parameter_state: parameter.parameter_state,
            unit: parameter.unit.map(|value| value.value.to_string()),
            name: parameter.name.to_string(),
            evaluated_value: parameter.evaluated_value.get(),
        }
    }
}

/// Indexed record that owns one Design parameter.
#[derive(Debug, PartialEq, Deserialize)]
#[cfg_attr(not(test), derive(Clone))]
#[serde(try_from = "DesignParameterOwnerContent")]
pub(crate) struct DesignParameterOwner {
    id: String,
    byte_offset: Option<u64>,
    frame_length: Option<u64>,
    class_tag: DesignClassTag,
    scope_record_index: Option<u32>,
    local_ordinal: Option<u32>,
    evaluated_value: FiniteReal,
    evaluated_value_offset: Option<u64>,
    owned_ordinal: u32,
    variant: Option<u8>,
    base_index: u32,
    order: ParameterFrameOrder,
}

#[cfg(test)]
thread_local! {
    static PARAMETER_OWNER_CLONE_COUNT: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

#[cfg(test)]
impl Clone for DesignParameterOwner {
    fn clone(&self) -> Self {
        PARAMETER_OWNER_CLONE_COUNT.with(|count| count.set(count.get() + 1));
        Self {
            id: self.id.clone(),
            byte_offset: self.byte_offset,
            frame_length: self.frame_length,
            class_tag: self.class_tag.clone(),
            scope_record_index: self.scope_record_index,
            local_ordinal: self.local_ordinal,
            evaluated_value: self.evaluated_value,
            evaluated_value_offset: self.evaluated_value_offset,
            owned_ordinal: self.owned_ordinal,
            variant: self.variant,
            base_index: self.base_index,
            order: self.order,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ParameterFrameOrder {
    OwnerParameterCompanion,
    ParameterOwnerCompanion,
    OwnerCompanionParameter,
}

impl DesignParameterOwner {
    /// Native order among values owned by this record.
    pub(crate) fn owned_ordinal(&self) -> u32 {
        self.owned_ordinal
    }
    /// Admitted optional owner variant.
    pub(crate) fn variant(&self) -> Option<u8> {
        self.variant
    }

    /// The id value.
    pub(crate) fn id(&self) -> &String {
        &self.id
    }
    /// The byte offset value.
    pub(crate) fn byte_offset(&self) -> Option<u64> {
        self.byte_offset
    }
    /// The frame length value.
    pub(crate) fn frame_length(&self) -> Option<u64> {
        self.frame_length
    }
    /// The class tag value.
    pub(crate) fn class_tag(&self) -> &DesignClassTag {
        &self.class_tag
    }
    /// The record index value.
    pub(crate) fn record_index(&self) -> u32 {
        self.base_index
            + match self.order {
                ParameterFrameOrder::OwnerParameterCompanion => 0,
                ParameterFrameOrder::ParameterOwnerCompanion => 1,
                ParameterFrameOrder::OwnerCompanionParameter => 0,
            }
    }
    /// The scope record index value.
    pub(crate) fn scope_record_index(&self) -> Option<u32> {
        self.scope_record_index
    }
    /// The local ordinal value.
    pub(crate) fn local_ordinal(&self) -> Option<u32> {
        self.local_ordinal
    }
    /// The evaluated value value.
    pub(crate) fn evaluated_value(&self) -> FiniteReal {
        self.evaluated_value
    }
    /// The evaluated value offset value.
    pub(crate) fn evaluated_value_offset(&self) -> Option<u64> {
        self.evaluated_value_offset
    }
    /// The parameter record index value.
    pub(crate) fn parameter_record_index(&self) -> u32 {
        self.base_index
            + match self.order {
                ParameterFrameOrder::OwnerParameterCompanion => 1,
                ParameterFrameOrder::ParameterOwnerCompanion => 0,
                ParameterFrameOrder::OwnerCompanionParameter => 2,
            }
    }
    /// The companion record index value.
    pub(crate) fn companion_record_index(&self) -> u32 {
        self.base_index
            + match self.order {
                ParameterFrameOrder::OwnerParameterCompanion => 2,
                ParameterFrameOrder::ParameterOwnerCompanion => 2,
                ParameterFrameOrder::OwnerCompanionParameter => 1,
            }
    }
}

impl Serialize for DesignParameterOwner {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        #[derive(Serialize)]
        struct WireRef<'a> {
            id: &'a str,
            class_tag: &'a str,
            record_index: u32,
            #[serde(skip_serializing_if = "Option::is_none")]
            scope_record_index: Option<u32>,
            #[serde(skip_serializing_if = "Option::is_none")]
            local_ordinal: Option<u32>,
            evaluated_value: f64,
            parameter_record_index: u32,
            owned_ordinal: u32,
            #[serde(skip_serializing_if = "Option::is_none")]
            variant: Option<u8>,
            companion_record_index: u32,
        }
        WireRef {
            id: &self.id,
            class_tag: self.class_tag.as_str(),
            record_index: self.record_index(),
            scope_record_index: self.scope_record_index,
            local_ordinal: self.local_ordinal,
            evaluated_value: self.evaluated_value.get(),
            parameter_record_index: self.parameter_record_index(),
            owned_ordinal: self.owned_ordinal,
            variant: self.variant,
            companion_record_index: self.companion_record_index(),
        }
        .serialize(serializer)
    }
}

impl TryFrom<DesignParameterOwnerWire> for DesignParameterOwner {
    type Error = String;
    fn try_from(wire: DesignParameterOwnerWire) -> Result<Self, Self::Error> {
        let evaluated_value =
            FiniteReal::new(wire.evaluated_value).ok_or("evaluated_value must be finite")?;
        Self::from_parts(DesignParameterOwnerWire {
            id: wire.id,
            byte_offset: wire.byte_offset,
            frame_length: wire.frame_length,
            class_tag: wire.class_tag,
            record_index: wire.record_index,
            scope_record_index: wire.scope_record_index,
            local_ordinal: wire.local_ordinal,
            evaluated_value,
            evaluated_value_offset: wire.evaluated_value_offset,
            parameter_record_index: wire.parameter_record_index,
            owned_ordinal: wire.owned_ordinal,
            variant: wire.variant,
            companion_record_index: wire.companion_record_index,
        })
    }
}

impl DesignParameterOwner {
    pub(crate) fn from_parts(wire: DesignParameterOwnerWire<FiniteReal>) -> Result<Self, String> {
        let evaluated_value = wire.evaluated_value;
        let (base_index, order) = if wire.record_index.checked_add(1)
            == Some(wire.parameter_record_index)
            && wire.record_index.checked_add(2) == Some(wire.companion_record_index)
        {
            (
                wire.record_index,
                ParameterFrameOrder::OwnerParameterCompanion,
            )
        } else if wire.parameter_record_index.checked_add(1) == Some(wire.record_index)
            && wire.parameter_record_index.checked_add(2) == Some(wire.companion_record_index)
        {
            (
                wire.parameter_record_index,
                ParameterFrameOrder::ParameterOwnerCompanion,
            )
        } else if wire.record_index.checked_add(1) == Some(wire.companion_record_index)
            && wire.record_index.checked_add(2) == Some(wire.parameter_record_index)
        {
            (
                wire.record_index,
                ParameterFrameOrder::OwnerCompanionParameter,
            )
        } else {
            return Err("record_index, parameter_record_index, and companion_record_index must follow a parameter frame order".into());
        };
        // A variant byte either ends its scope lane, keeping the variant-less
        // length, or is followed by one zero byte.
        let modern = matches!(
            (
                wire.frame_length,
                wire.evaluated_value_offset.checked_sub(wire.byte_offset),
                wire.variant
            ),
            (99 | 103, Some(40), None)
                | (100, Some(41), None)
                | (107, Some(44), None)
                | (100 | 101, Some(41), Some(0..=1))
                | (103 | 104, Some(40), Some(0..=1))
                | (107 | 108, Some(44), Some(0..=1))
        );
        // The typed-reference spelling widens each of the five reference
        // fields by forty bytes; one of them precedes the scalar envelope.
        let typed = matches!(
            (
                wire.frame_length,
                wire.evaluated_value_offset.checked_sub(wire.byte_offset),
                wire.variant
            ),
            (299 | 303, Some(80), None)
                | (300, Some(81), None)
                | (307, Some(84), None)
                | (300 | 301, Some(81), Some(0..=1))
                | (303 | 304, Some(80), Some(0..=1))
                | (307 | 308, Some(84), Some(0..=1))
        );
        let legacy = wire.local_ordinal == 0
            && match wire.frame_length {
                68 => {
                    crate::design::decode::parameters::is_legacy_parameter_owner_68_class(
                        wire.class_tag.as_str(),
                    ) && wire.scope_record_index == 0
                }
                88 => {
                    crate::design::decode::parameters::is_legacy_parameter_owner_88_class(
                        wire.class_tag.as_str(),
                    ) && wire.scope_record_index != 0
                }
                _ => false,
            };
        if !modern && !typed && !legacy {
            return Err("frame_length, evaluated_value_offset, and variant must describe a parameter owner frame".into());
        }
        Ok(Self {
            base_index,
            order,
            id: wire.id,
            byte_offset: Some(wire.byte_offset),
            frame_length: Some(wire.frame_length),
            class_tag: wire.class_tag,
            scope_record_index: (wire.frame_length != 68).then_some(wire.scope_record_index),
            local_ordinal: modern.then_some(wire.local_ordinal),
            evaluated_value,
            evaluated_value_offset: Some(wire.evaluated_value_offset),
            owned_ordinal: wire.owned_ordinal,
            variant: wire.variant,
        })
    }
}

#[cfg(test)]
impl From<DesignParameterOwner> for DesignParameterOwnerContent {
    fn from(owner: DesignParameterOwner) -> Self {
        Self {
            id: owner.id.clone(),
            class_tag: owner.class_tag.clone(),
            record_index: owner.record_index(),
            scope_record_index: owner.scope_record_index,
            local_ordinal: owner.local_ordinal,
            evaluated_value: owner.evaluated_value.get(),
            parameter_record_index: owner.parameter_record_index(),
            owned_ordinal: owner.owned_ordinal,
            variant: owner.variant,
            companion_record_index: owner.companion_record_index(),
        }
    }
}

/// Parameter owner fields before aggregate validation.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub(crate) struct DesignParameterOwnerWire<T = f64> {
    /// Globally unique deterministic identifier for this native record.
    pub(crate) id: String,
    /// Byte offset of the indexed record header in its Design `BulkStream`.
    pub(crate) byte_offset: u64,
    /// Byte length from the primary header to its same-index paired header.
    #[serde(default)]
    pub(crate) frame_length: u64,
    /// Source per-file dynamic three-digit ASCII class tag.
    pub(crate) class_tag: DesignClassTag,
    /// Source indexed-record identity.
    pub(crate) record_index: u32,
    /// Feature or sketch record that scopes this parameter.
    pub(crate) scope_record_index: u32,
    /// Position among parameters in the same scope.
    pub(crate) local_ordinal: u32,
    /// Evaluated scalar duplicated from the parameter record.
    pub(crate) evaluated_value: T,
    /// Byte offset of `evaluated_value`.
    pub(crate) evaluated_value_offset: u64,
    /// Indexed parameter record owned by this frame.
    pub(crate) parameter_record_index: u32,
    /// Native owner ordering value.
    pub(crate) owned_ordinal: u32,
    /// Source owner-frame variant flag when the frame carries one.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_variant"
    )]
    pub(crate) variant: Option<u8>,
    /// Paired indexed record following the parameter record.
    pub(crate) companion_record_index: u32,
}

/// Semantic parameter-owner record, without source-layout fields.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct DesignParameterOwnerContent<T = f64> {
    pub(crate) id: String,
    pub(crate) class_tag: DesignClassTag,
    pub(crate) record_index: u32,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_owner_scope_record_index"
    )]
    pub(crate) scope_record_index: Option<u32>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_owner_local_ordinal"
    )]
    pub(crate) local_ordinal: Option<u32>,
    pub(crate) evaluated_value: T,
    pub(crate) parameter_record_index: u32,
    pub(crate) owned_ordinal: u32,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_variant"
    )]
    pub(crate) variant: Option<u8>,
    pub(crate) companion_record_index: u32,
}
impl TryFrom<DesignParameterOwnerContent> for DesignParameterOwner {
    type Error = String;
    fn try_from(wire: DesignParameterOwnerContent) -> Result<Self, String> {
        let (base_index, order) = if wire.record_index.checked_add(1)
            == Some(wire.parameter_record_index)
            && wire.record_index.checked_add(2) == Some(wire.companion_record_index)
        {
            (
                wire.record_index,
                ParameterFrameOrder::OwnerParameterCompanion,
            )
        } else if wire.parameter_record_index.checked_add(1) == Some(wire.record_index)
            && wire.parameter_record_index.checked_add(2) == Some(wire.companion_record_index)
        {
            (
                wire.parameter_record_index,
                ParameterFrameOrder::ParameterOwnerCompanion,
            )
        } else if wire.record_index.checked_add(1) == Some(wire.companion_record_index)
            && wire.record_index.checked_add(2) == Some(wire.parameter_record_index)
        {
            (
                wire.record_index,
                ParameterFrameOrder::OwnerCompanionParameter,
            )
        } else {
            return Err("record_index, parameter_record_index, and companion_record_index must follow a parameter frame order".into());
        };
        if wire.local_ordinal.is_some() && wire.scope_record_index.is_none() {
            return Err("a parameter local ordinal requires a scope record".into());
        }
        if wire.variant.is_some_and(|value| value > 1) {
            return Err("parameter owner variant must be 0 or 1".into());
        }
        Ok(Self {
            id: wire.id,
            class_tag: wire.class_tag,
            base_index,
            order,
            scope_record_index: wire.scope_record_index,
            local_ordinal: wire.local_ordinal,
            evaluated_value: FiniteReal::new(wire.evaluated_value)
                .ok_or("evaluated_value must be finite")?,
            owned_ordinal: wire.owned_ordinal,
            variant: wire.variant,
            byte_offset: None,
            frame_length: None,
            evaluated_value_offset: None,
        })
    }
}
impl DesignParameterOwner {
    pub(crate) fn retain_source(&self, fidelity: &mut cadmpeg_ir::SourceFidelity) {
        for (name, offset) in [
            ("byte_offset", self.byte_offset),
            ("frame_length", self.frame_length),
            ("evaluated_value_offset", self.evaluated_value_offset),
        ] {
            if let Some(offset) = offset {
                fidelity
                    .metadata
                    .insert(format!("{}.{name}", self.id), offset.to_string());
            }
        }
    }
}
/// Owned payload bound to a Design parameter companion.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct DesignCompanionPayload {
    byte_offset: Option<u64>,
    byte_length: Option<u64>,
    owned_recipe_ids: Vec<String>,
}

impl DesignCompanionPayload {
    /// The byte interval a companion owns, with the recipes nested in it.
    #[must_use]
    pub(crate) fn new(byte_offset: u64, byte_length: u64, owned_recipe_ids: Vec<String>) -> Self {
        Self {
            byte_offset: Some(byte_offset),
            byte_length: Some(byte_length),
            owned_recipe_ids,
        }
    }

    /// First byte owned after the fixed companion prefix.
    #[must_use]
    pub(crate) fn byte_offset(&self) -> Option<u64> {
        self.byte_offset
    }

    /// Number of bytes owned before the next sibling Design record.
    #[must_use]
    pub(crate) fn byte_length(&self) -> Option<u64> {
        self.byte_length
    }

    /// Construction recipes contained by the owned payload, in byte order.
    #[must_use]
    pub(crate) fn owned_recipe_ids(&self) -> &[String] {
        &self.owned_recipe_ids
    }
}

/// Fixed prefix of the indexed record paired with a Design parameter owner.
#[derive(Debug, PartialEq, Deserialize)]
#[cfg_attr(not(test), derive(Clone))]
#[serde(try_from = "DesignParameterCompanionWire")]
pub(crate) struct DesignParameterCompanion {
    id: String,
    byte_offset: Option<u64>,
    class_tag: DesignClassTag,
    record_index: u32,
    owner_record_index: u32,
    timestamp_micros: NonZeroU64,
    timestamp_micros_offset: Option<u64>,
    payload: Option<DesignCompanionPayload>,
}

#[cfg(test)]
thread_local! {
    static PARAMETER_COMPANION_CLONE_COUNT: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

#[cfg(test)]
impl Clone for DesignParameterCompanion {
    fn clone(&self) -> Self {
        PARAMETER_COMPANION_CLONE_COUNT.with(|count| count.set(count.get() + 1));
        Self {
            id: self.id.clone(),
            byte_offset: self.byte_offset,
            class_tag: self.class_tag.clone(),
            record_index: self.record_index,
            owner_record_index: self.owner_record_index,
            timestamp_micros: self.timestamp_micros,
            timestamp_micros_offset: self.timestamp_micros_offset,
            payload: self.payload.clone(),
        }
    }
}

impl DesignParameterCompanion {
    /// Dynamic type identity of this companion record.
    pub(crate) fn class_tag(&self) -> &DesignClassTag {
        &self.class_tag
    }
    /// Native timestamp value, independent of its original encoded position.
    pub(crate) fn timestamp_micros(&self) -> NonZeroU64 {
        self.timestamp_micros
    }

    /// A companion prefix whose owned payload has not been bound.
    #[must_use]
    pub(crate) fn unbound(
        id: String,
        byte_offset: u64,
        class_tag: DesignClassTag,
        record_index: u32,
        owner_record_index: u32,
        timestamp_micros: NonZeroU64,
        timestamp_micros_offset: u64,
    ) -> Self {
        Self {
            id,
            byte_offset: Some(byte_offset),
            class_tag,
            record_index,
            owner_record_index,
            timestamp_micros,
            timestamp_micros_offset: Some(timestamp_micros_offset),
            payload: None,
        }
    }

    /// The same companion with its owned payload bound.
    #[must_use]
    pub(crate) fn bound(self, payload: DesignCompanionPayload) -> Self {
        Self {
            payload: Some(payload),
            ..self
        }
    }

    /// Globally unique deterministic identifier for this native record.
    #[must_use]
    pub(crate) fn id(&self) -> &str {
        &self.id
    }

    /// Byte offset of the indexed record header in its Design `BulkStream`.
    #[must_use]
    pub(crate) fn byte_offset(&self) -> Option<u64> {
        self.byte_offset
    }

    /// Source indexed-record identity.
    #[must_use]
    pub(crate) fn record_index(&self) -> u32 {
        self.record_index
    }

    /// Indexed parameter-owner record referenced by this prefix.
    #[must_use]
    pub(crate) fn owner_record_index(&self) -> u32 {
        self.owner_record_index
    }

    /// Byte offset of `timestamp_micros`.
    #[must_use]
    pub(crate) fn timestamp_micros_offset(&self) -> Option<u64> {
        self.timestamp_micros_offset
    }

    /// Owned payload, when the binding pass reached this companion.
    #[must_use]
    pub(crate) fn payload(&self) -> Option<&DesignCompanionPayload> {
        self.payload.as_ref()
    }
}

impl Serialize for DesignParameterCompanion {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        #[derive(Serialize)]
        struct WireRef<'a> {
            id: &'a str,
            class_tag: &'a str,
            record_index: u32,
            owner_record_index: u32,
            timestamp_micros: NonZeroU64,
            #[serde(skip_serializing_if = "Option::is_none")]
            owned_recipe_ids: Option<&'a [String]>,
        }
        WireRef {
            id: &self.id,
            class_tag: self.class_tag.as_str(),
            record_index: self.record_index,
            owner_record_index: self.owner_record_index,
            timestamp_micros: self.timestamp_micros,
            owned_recipe_ids: self
                .payload
                .as_ref()
                .map(|payload| payload.owned_recipe_ids.as_slice()),
        }
        .serialize(serializer)
    }
}

/// Semantic companion content. Physical frame positions belong to fidelity.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct DesignParameterCompanionWire {
    id: String,
    class_tag: DesignClassTag,
    record_index: u32,
    owner_record_index: u32,
    #[serde(deserialize_with = "deserialize_companion_timestamp")]
    timestamp_micros: NonZeroU64,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_owned_recipe_ids"
    )]
    owned_recipe_ids: Option<Vec<String>>,
}
impl TryFrom<DesignParameterCompanionWire> for DesignParameterCompanion {
    type Error = String;
    fn try_from(wire: DesignParameterCompanionWire) -> Result<Self, String> {
        Ok(Self {
            id: wire.id,
            class_tag: wire.class_tag,
            record_index: wire.record_index,
            owner_record_index: wire.owner_record_index,
            timestamp_micros: wire.timestamp_micros,
            byte_offset: None,
            timestamp_micros_offset: None,
            payload: wire
                .owned_recipe_ids
                .map(|owned_recipe_ids| DesignCompanionPayload {
                    byte_offset: None,
                    byte_length: None,
                    owned_recipe_ids,
                }),
        })
    }
}
#[cfg(test)]
impl From<DesignParameterCompanion> for DesignParameterCompanionWire {
    fn from(record: DesignParameterCompanion) -> Self {
        Self {
            id: record.id,
            class_tag: record.class_tag,
            record_index: record.record_index,
            owner_record_index: record.owner_record_index,
            timestamp_micros: record.timestamp_micros,
            owned_recipe_ids: record.payload.map(|payload| payload.owned_recipe_ids),
        }
    }
}
impl DesignParameterCompanion {
    pub(crate) fn retain_source(&self, fidelity: &mut cadmpeg_ir::SourceFidelity) {
        for (name, value) in [
            ("byte_offset", self.byte_offset),
            ("timestamp_micros_offset", self.timestamp_micros_offset),
            (
                "payload_byte_offset",
                self.payload.as_ref().and_then(|p| p.byte_offset),
            ),
            (
                "payload_byte_length",
                self.payload.as_ref().and_then(|p| p.byte_length),
            ),
        ] {
            if let Some(value) = value {
                fidelity
                    .metadata
                    .insert(format!("{}.{name}", self.id), value.to_string());
            }
        }
    }
}
cadmpeg_core::named_optional_field!(
    deserialize_owned_recipe_ids,
    Vec<String>,
    "owned_recipe_ids"
);

fn deserialize_companion_timestamp<'de, D: Deserializer<'de>>(
    deserializer: D,
) -> Result<NonZeroU64, D::Error> {
    NonZeroU64::new(u64::deserialize(deserializer)?)
        .ok_or_else(|| serde::de::Error::custom("timestamp_micros must be nonzero"))
}

#[cfg(test)]
mod tests;

cadmpeg_core::named_optional_field!(
    deserialize_owner_scope_record_index,
    u32,
    "scope_record_index"
);
cadmpeg_core::named_optional_field!(deserialize_owner_local_ordinal, u32, "local_ordinal");
