// SPDX-License-Identifier: Apache-2.0
//! Thread constructions, forms, diameters and nominal sizes.

use crate::records::identity::Located;
use cadmpeg_ir::scalar::PositiveReal;
use serde::{Deserialize, Serialize, Serializer};
use std::num::NonZeroU32;

cadmpeg_core::named_optional_field!(
    deserialize_trailing_reference_offset,
    u64,
    "trailing_reference_offset"
);
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
    /// Compact prefix, construction marker, and trailer layout.
    Compact(Option<Located<NonZeroU32>>),
    /// Direct standard prefix with the legacy compact scalar and trailer lanes.
    StandardLegacy,
    /// Compact prefix with the legacy scalar and no-reference trailer lanes.
    CompactLegacy,
}

/// Exact form and size construction carried by a `Thread` scope.
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(try_from = "DesignThreadConstructionWire")]
pub(crate) struct DesignThreadConstruction {
    /// Standard, compact, or class-specific legacy construction form.
    pub(crate) form: DesignThreadForm,
    /// Byte offset of the designation LP-UTF16 field.
    pub(crate) designation_offset: u64,
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
        DesignThreadConstructionWire::try_from(self.clone())
            .map_err(serde::ser::Error::custom)?
            .serialize(serializer)
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
}

#[derive(Serialize, Deserialize)]
struct DesignThreadConstructionWire {
    /// Standard, compact, or class-specific legacy construction form.
    form: DesignThreadFormWire,
    /// Byte offset of the designation LP-UTF16 field.
    designation_offset: u64,
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
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_trailing_reference_offset"
    )]
    trailing_reference_offset: Option<u64>,
    /// Ordered counted face-selection groups referenced by the scope.
    face_group_record_indices: Vec<u32>,
}

impl TryFrom<DesignThreadConstruction> for DesignThreadConstructionWire {
    type Error = String;
    fn try_from(value: DesignThreadConstruction) -> Result<Self, Self::Error> {
        let nominal_size = value
            .nominal_size
            .value()
            .map_err(|error| format!("nominal_size_text: {error}"))?;
        let (form, trailing_reference) = match value.form {
            DesignThreadForm::Standard => (DesignThreadFormWire::Standard, None),
            DesignThreadForm::Compact(reference) => (DesignThreadFormWire::Compact, reference),
            DesignThreadForm::StandardLegacy => (DesignThreadFormWire::StandardLegacy, None),
            DesignThreadForm::CompactLegacy => (DesignThreadFormWire::CompactLegacy, None),
        };
        Ok(Self {
            form,
            designation_offset: value.designation_offset,
            designation: value.designation.as_str().to_owned(),
            nominal_size_text: value.nominal_size.0,
            nominal_size,
            profile: value.profile.as_str().to_owned(),
            major_diameter: value.diameters.major(),
            minor_diameter: value.diameters.minor(),
            pitch: value.pitch.get(),
            pitch_diameter: value.diameters.pitch(),
            trailing_reference_record_index: trailing_reference.map(|located| located.value.get()),
            trailing_reference_offset: trailing_reference.map(|located| located.offset),
            face_group_record_indices: value.face_group_record_indices,
        })
    }
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
        let reference = match (
            value.trailing_reference_record_index,
            value.trailing_reference_offset,
        ) {
            (None, None) => None,
            (Some(value), Some(offset)) => Some(Located {
                value: NonZeroU32::new(value)
                    .ok_or("trailing_reference_record_index must be nonzero")?,
                offset,
            }),
            _ => return Err(
                "trailing_reference_record_index and trailing_reference_offset must occur together"
                    .into(),
            ),
        };
        let form = match (value.form, reference) {
            (DesignThreadFormWire::Compact, reference) => DesignThreadForm::Compact(reference),
            (DesignThreadFormWire::Standard, None) => DesignThreadForm::Standard,
            (DesignThreadFormWire::StandardLegacy, None) => DesignThreadForm::StandardLegacy,
            (DesignThreadFormWire::CompactLegacy, None) => DesignThreadForm::CompactLegacy,
            _ => {
                return Err("trailing_reference_record_index is only valid for compact form".into())
            }
        };
        Ok(Self {
            form,
            designation_offset: value.designation_offset,
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
