// SPDX-License-Identifier: Apache-2.0
//! Thread constructions, forms, diameters and nominal sizes.

use crate::layout::thread_extended_scope_prefix as extended_prefix;
use crate::layout::thread_owner_marked_scope_prefix as owner_prefix;
use crate::layout::thread_standard_scope_prefix as direct_prefix;
use crate::records::identity::Located;
use crate::records::identity::Sourced;
use cadmpeg_ir::scalar::PositiveReal;
use serde::{ser::SerializeStruct, Deserialize, Serialize, Serializer};
use std::num::NonZeroU32;

cadmpeg_core::named_optional_field!(
    deserialize_trailing_reference_record_index,
    u32,
    "trailing_reference_record_index"
);
/// Thread construction form selected by the scope prefix and payload marker.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum DesignThreadForm {
    /// Standard prefix, construction marker, and trailer layout.
    Standard,
    /// Compact construction with an optional reference to its owning Hole scope.
    Compact(Option<Sourced<NonZeroU32>>),
    /// Direct standard prefix with the legacy compact scalar and trailer lanes.
    StandardLegacy,
    /// Compact prefix with the legacy scalar and no-reference trailer lanes.
    CompactLegacy,
    /// Standard prefix with the compact construction marker and a reference trailer.
    StandardCompact,
    /// Extended standard prefix with offset and length owner references.
    StandardExtended,
    /// Extended compact prefix with two owners before its face-selection pairs.
    CompactExtended(Located<NonZeroU32>),
}

impl DesignThreadForm {
    pub(crate) fn class_pair_holds(self, class: &str, paired: &str) -> bool {
        match self {
            Self::StandardLegacy => class == "334" && paired == "262",
            Self::CompactLegacy => class == "414" && paired == "263",
            Self::Standard
            | Self::Compact(_)
            | Self::CompactExtended(_)
            | Self::StandardCompact
            | Self::StandardExtended => true,
        }
    }

    pub(crate) fn designation_offset_holds(self, designation: u64, scope: u64) -> bool {
        let Some(delta) = designation
            .checked_sub(scope)
            .and_then(|delta| usize::try_from(delta).ok())
        else {
            return false;
        };
        match self {
            Self::StandardExtended | Self::CompactExtended(_) => delta == extended_prefix::LEN,
            _ => [direct_prefix::LEN, owner_prefix::LEN].contains(&delta),
        }
    }

    pub(crate) fn reference_count_holds(self, count: usize) -> bool {
        count.is_multiple_of(2)
            && match self {
                Self::StandardExtended => count == 4,
                Self::CompactExtended(_) => count >= 4,
                _ => count >= 2,
            }
    }

    pub(crate) fn owners_hold(
        self,
        scope: &super::scope::DesignParameterScope,
        owners: &[crate::records::parameters::DesignParameterOwner],
    ) -> bool {
        let (offset_ordinal, length_ordinal) = match self {
            Self::StandardExtended => (2, 3),
            Self::CompactExtended(_) => (0, 1),
            _ => return true,
        };
        let owner_at = |ordinal| {
            let index = scope.reference_members().values().nth(ordinal)?;
            let mut matching = owners.iter().filter(|owner| {
                crate::ids::native_stream(owner.id()) == crate::ids::native_stream(&scope.id)
                    && owner.scope_record_index() == Some(scope.record_index)
                    && owner.record_index() == *index
            });
            let owner = matching.next()?;
            matching.next().is_none().then_some(owner)
        };
        match (owner_at(offset_ordinal), owner_at(length_ordinal)) {
            (Some(offset), Some(length)) => {
                offset.local_ordinal() == Some(0)
                    && offset.evaluated_value().get() == 0.0
                    && length.local_ordinal() == Some(1)
                    && length.evaluated_value().get() > 0.0
            }
            _ => false,
        }
    }

    pub(crate) const fn face_reference_start(self) -> usize {
        match self {
            Self::CompactExtended(_) => 2,
            _ => 0,
        }
    }

    /// Whether the scope carries the standard one-group reference table.
    ///
    /// The other forms carry ordered `(face-selection group, sole member)` pairs.
    pub(crate) const fn has_standard_reference_table(&self) -> bool {
        match self {
            Self::Standard
            | Self::StandardLegacy
            | Self::StandardCompact
            | Self::StandardExtended => true,
            Self::Compact(_) | Self::CompactExtended(_) | Self::CompactLegacy => false,
        }
    }
}

/// Exact form and size construction carried by a `Thread` scope.
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(try_from = "DesignThreadConstructionWire")]
pub(crate) struct DesignThreadConstruction {
    /// Standard, compact, or class-specific legacy construction form.
    pub(crate) form: DesignThreadForm,
    /// Byte offset of the designation LP-UTF16 field of a located scope.
    pub(crate) designation_offset: Option<u64>,
    /// Standard thread designation.
    pub(crate) designation: cadmpeg_core::text::NonBlankString,
    /// Validated nominal-size spelling; its numeric value is derived on read.
    pub(crate) nominal_size: DesignThreadNominalSize,
    /// Thread profile name.
    pub(crate) profile: cadmpeg_core::text::NonBlankString,
    /// Ordered physical thread diameters in Design length units.
    pub(crate) diameters: DesignThreadDiameters,
    /// Thread pitch in Design length units.
    pub(crate) pitch: PositiveReal,
    /// Ordered counted face-selection groups referenced by the scope.
    pub(crate) face_group_record_indices: Vec<u32>,
}

/// Positive finite thread diameters ordered from minor through pitch to major.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct DesignThreadDiameters {
    major: PositiveReal,
    minor: PositiveReal,
    pitch: PositiveReal,
}

impl DesignThreadDiameters {
    /// Admit strictly ordered positive finite thread diameters.
    pub(crate) fn new(major: f64, minor: f64, pitch: f64) -> Option<Self> {
        let major = PositiveReal::new(major)?;
        let minor = PositiveReal::new(minor)?;
        let pitch = PositiveReal::new(pitch)?;
        (minor.get() < pitch.get() && pitch.get() < major.get()).then_some(Self {
            major,
            minor,
            pitch,
        })
    }
    /// Physical major diameter in Design length units.
    pub(crate) fn major(self) -> f64 {
        self.major.get()
    }
    /// Physical minor diameter in Design length units.
    pub(crate) fn minor(self) -> f64 {
        self.minor.get()
    }
    /// Physical pitch diameter in Design length units.
    pub(crate) fn pitch(self) -> f64 {
        self.pitch.get()
    }
}

/// Original spelling of a finite positive nominal thread size.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct DesignThreadNominalSize(String);

impl TryFrom<String> for DesignThreadNominalSize {
    type Error = String;
    fn try_from(text: String) -> Result<Self, Self::Error> {
        if !text
            .parse::<f64>()
            .is_ok_and(|value| value.is_finite() && value > 0.0)
        {
            return Err("nominal_size_text must encode a finite positive number".into());
        }
        Ok(Self(text))
    }
}

impl DesignThreadNominalSize {
    #[must_use]
    #[cfg(test)]
    pub(crate) fn text(&self) -> &str {
        &self.0
    }

    pub(crate) fn value(&self) -> Result<f64, std::num::ParseFloatError> {
        self.0.parse()
    }
}

impl Serialize for DesignThreadConstruction {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let nominal_size = self.nominal_size.value().map_err(|error| {
            serde::ser::Error::custom(format_args!("nominal_size_text: {error}"))
        })?;
        let (form, trailing_reference) = match &self.form {
            DesignThreadForm::Standard => (DesignThreadFormWire::Standard, None),
            DesignThreadForm::Compact(reference) => (
                DesignThreadFormWire::Compact,
                reference
                    .as_ref()
                    .map(|reference| (reference.value.get(), reference.offset)),
            ),
            DesignThreadForm::CompactExtended(reference) => (
                DesignThreadFormWire::CompactExtended,
                Some((reference.value.get(), Some(reference.offset))),
            ),
            DesignThreadForm::StandardLegacy => (DesignThreadFormWire::StandardLegacy, None),
            DesignThreadForm::CompactLegacy => (DesignThreadFormWire::CompactLegacy, None),
            DesignThreadForm::StandardCompact => (DesignThreadFormWire::StandardCompact, None),
            DesignThreadForm::StandardExtended => (DesignThreadFormWire::StandardExtended, None),
        };
        let mut fields = serializer.serialize_struct(
            "DesignThreadConstructionWire",
            10 + usize::from(trailing_reference.is_some())
                + usize::from(
                    trailing_reference.is_some_and(|(_, offset)| offset.is_some()),
                ),
        )?;
        fields.serialize_field("form", &form)?;
        fields.serialize_field("designation", self.designation.as_str())?;
        fields.serialize_field("nominal_size_text", &self.nominal_size.0)?;
        fields.serialize_field("nominal_size", &nominal_size)?;
        fields.serialize_field("profile", self.profile.as_str())?;
        fields.serialize_field("major_diameter", &self.diameters.major())?;
        fields.serialize_field("minor_diameter", &self.diameters.minor())?;
        fields.serialize_field("pitch", &self.pitch.get())?;
        fields.serialize_field("pitch_diameter", &self.diameters.pitch())?;
        if let Some((record_index, offset)) = trailing_reference {
            fields.serialize_field("trailing_reference_record_index", &record_index)?;
            if let Some(offset) = offset {
                fields.serialize_field("trailing_reference_offset", &offset)?;
            }
        }
        fields.serialize_field("face_group_record_indices", &self.face_group_record_indices)?;
        fields.end()
    }
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum DesignThreadFormWire {
    /// Standard prefix, construction marker, and trailer layout.
    Standard,
    /// Compact prefix, construction marker, and trailer layout.
    Compact,
    /// Direct standard prefix with the legacy compact scalar and trailer lanes.
    StandardLegacy,
    /// Compact prefix with the legacy scalar and no-reference trailer lanes.
    CompactLegacy,
    /// Standard prefix with the compact construction marker and a reference trailer.
    StandardCompact,
    StandardExtended,
    CompactExtended,
}

#[derive(Serialize, Deserialize)]
struct DesignThreadConstructionWire {
    /// Standard, compact, or class-specific legacy construction form.
    form: DesignThreadFormWire,
    /// Standard thread designation.
    designation: String,
    /// Exact nominal-size text interpreted into `nominal_size`.
    nominal_size_text: String,
    /// Numeric nominal size interpreted by `profile`.
    nominal_size: f64,
    /// Thread profile name.
    profile: String,
    /// Physical major diameter in Design length units.
    pub(super) major_diameter: f64,
    /// Physical minor diameter in Design length units.
    pub(super) minor_diameter: f64,
    /// Thread pitch in Design length units.
    pub(super) pitch: f64,
    /// Pitch diameter in Design length units.
    pub(super) pitch_diameter: f64,
    /// Record named by the reference-bearing compact trailer.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_trailing_reference_record_index"
    )]
    trailing_reference_record_index: Option<u32>,
    /// Byte offset of `trailing_reference_record_index`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    trailing_reference_offset: Option<u64>,
    /// Ordered counted face-selection groups referenced by the scope.
    face_group_record_indices: Vec<u32>,
}

impl TryFrom<DesignThreadConstructionWire> for DesignThreadConstruction {
    type Error = String;
    fn try_from(value: DesignThreadConstructionWire) -> Result<Self, Self::Error> {
        let nominal_size = DesignThreadNominalSize::try_from(value.nominal_size_text)?;
        if nominal_size
            .value()
            .map_err(|error| format!("nominal_size_text: {error}"))?
            .to_bits()
            != value.nominal_size.to_bits()
        {
            return Err("nominal_size must match nominal_size_text".into());
        }
        let record_index = match value.trailing_reference_record_index {
            None => None,
            Some(value) => Some(
                NonZeroU32::new(value)
                    .ok_or("trailing_reference_record_index must be nonzero")?,
            ),
        };
        let form = match (value.form, record_index) {
            (DesignThreadFormWire::Compact, record_index) => DesignThreadForm::Compact(
                record_index.map(|value| Sourced { value, offset: None }),
            ),
            (
                DesignThreadFormWire::CompactExtended,
                Some(record_index),
            ) => DesignThreadForm::CompactExtended(Located {
                value: record_index,
                offset: value.trailing_reference_offset.ok_or(
                    "compact_extended requires a trailing Hole reference offset",
                )?,
            }),
            (DesignThreadFormWire::CompactExtended, None) => {
                return Err("compact_extended requires a trailing Hole reference".into());
            }
            (DesignThreadFormWire::Standard, None) => DesignThreadForm::Standard,
            (DesignThreadFormWire::StandardLegacy, None) => DesignThreadForm::StandardLegacy,
            (DesignThreadFormWire::CompactLegacy, None) => DesignThreadForm::CompactLegacy,
            (DesignThreadFormWire::StandardCompact, None) => DesignThreadForm::StandardCompact,
            (DesignThreadFormWire::StandardExtended, None) => DesignThreadForm::StandardExtended,
            _ => {
                return Err("trailing_reference_record_index is only valid for compact form".into())
            }
        };
        Ok(Self {
            form,
            designation_offset: None,
            designation: cadmpeg_core::text::NonBlankString::new(value.designation).ok_or("designation must not be empty")?,
            nominal_size,
            profile: cadmpeg_core::text::NonBlankString::new(value.profile).ok_or("profile must not be empty")?,
            diameters: DesignThreadDiameters::new(value.major_diameter, value.minor_diameter, value.pitch_diameter)
                .ok_or("major_diameter, minor_diameter, and pitch_diameter must be positive finite and strictly ordered")?,
            pitch: PositiveReal::new(value.pitch).ok_or("pitch must be positive finite")?,
            face_group_record_indices: value.face_group_record_indices,
        })
    }
}

#[cfg(test)]
mod tests;

mod identity_rewrite;
