// SPDX-License-Identifier: Apache-2.0
//! Thread constructions, forms, diameters and nominal sizes.

use crate::layout::thread_compact_owned_scope_prefix as compact_owned;
use crate::layout::thread_extended_scope_prefix as extended_prefix;
use crate::layout::thread_owner_marked_scope_prefix as owner_prefix;
use crate::layout::thread_standard_scope_prefix as direct_prefix;
use crate::records::identity::Located;
use cadmpeg_ir::scalar::PositiveReal;
use serde::{ser::SerializeStruct, Deserialize, Serialize, Serializer};
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
cadmpeg_core::named_optional_field!(deserialize_extended, DesignThreadExtended, "extended");
cadmpeg_core::named_optional_field!(deserialize_full_length, bool, "full_length");
cadmpeg_core::named_optional_field!(deserialize_right_handed, bool, "right_handed");

/// Fields that the extended Thread prefix and the extended standard
/// construction marker state.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct DesignThreadExtended {
    /// Thread class, such as `6g` or `6H`.
    pub(crate) class: String,
    /// Whether the thread is external rather than internal.
    pub(crate) external: bool,
    /// Whether an extended standard thread runs along the full face length.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_full_length"
    )]
    pub(crate) full_length: Option<bool>,
    /// Whether an extended standard thread is right-handed.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_right_handed"
    )]
    pub(crate) right_handed: Option<bool>,
}
/// Bytes of a one-unit LP-UTF16 thread class and an empty LP-UTF16 designation
/// copy between the extended prefix and the designation.
const SHORTEST_EXTENDED_CLASS_AND_COPY: usize = 10;

/// Thread construction form selected by the scope prefix and payload marker.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum DesignThreadForm {
    /// Standard prefix, construction marker, and trailer layout.
    Standard,
    /// Compact construction with an optional reference to its owning Hole scope.
    Compact(Option<Located<NonZeroU32>>),
    /// Direct standard prefix with the legacy compact scalar and trailer lanes.
    StandardLegacy,
    /// Compact prefix with the legacy scalar and no-reference trailer lanes.
    CompactLegacy,
    /// Standard prefix with the compact construction marker and a reference trailer.
    StandardCompact,
    /// Extended standard prefix with offset and length owner references.
    StandardExtended,
    CompactOwnersLeading,
    CompactOwnersTrailing,
    /// Compact Hole companion with two owners before its face-selection pairs.
    CompactExtended(Located<NonZeroU32>),
}

impl DesignThreadForm {
    pub(crate) const fn hole_reference(self) -> Option<Located<NonZeroU32>> {
        match self {
            Self::Compact(Some(reference)) | Self::CompactExtended(reference) => Some(reference),
            _ => None,
        }
    }

    pub(crate) fn class_pair_holds(self, class: &str, paired: &str) -> bool {
        match self {
            Self::StandardLegacy => class == "334" && paired == "262",
            Self::CompactLegacy => class == "414" && paired == "263",
            Self::CompactOwnersLeading
            | Self::CompactOwnersTrailing
            | Self::Standard
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
            Self::CompactOwnersLeading | Self::CompactOwnersTrailing => delta == compact_owned::LEN,
            Self::CompactExtended(_)
                if [direct_prefix::LEN, owner_prefix::LEN].contains(&delta) =>
            {
                true
            }
            Self::StandardExtended | Self::CompactExtended(_) => {
                delta >= extended_prefix::LEN + SHORTEST_EXTENDED_CLASS_AND_COPY
            }
            _ => [direct_prefix::LEN, owner_prefix::LEN].contains(&delta),
        }
    }

    pub(crate) fn reference_count_holds(self, count: usize) -> bool {
        count.is_multiple_of(2)
            && match self {
                Self::StandardExtended
                | Self::CompactOwnersLeading
                | Self::CompactOwnersTrailing => count >= 4,
                Self::CompactExtended(_) => count >= 4,
                _ => count >= 2,
            }
    }

    /// Ordinals of the offset owner and the length owner in the scope
    /// reference table.
    ///
    /// The extended standard table is in record order, so its owners are the
    /// two references whose owner records hold local ordinals 0 and 1.
    pub(crate) fn owner_ordinals(
        self,
        scope: &super::scope::DesignParameterScope,
        owners: &[crate::records::parameters::DesignParameterOwner],
    ) -> Option<(usize, usize)> {
        let count = scope.reference_members().len();
        match self {
            Self::CompactOwnersTrailing => Some((count.checked_sub(2)?, count.checked_sub(1)?)),
            Self::CompactExtended(_) | Self::CompactOwnersLeading => Some((0, 1)),
            Self::StandardExtended => {
                let ordinal_of = |local_ordinal: u32| {
                    let mut ordinals = (0..count).filter(|ordinal| {
                        thread_owner_at(scope, owners, *ordinal)
                            .is_some_and(|owner| owner.local_ordinal() == local_ordinal)
                    });
                    let ordinal = ordinals.next()?;
                    ordinals.next().is_none().then_some(ordinal)
                };
                Some((ordinal_of(0)?, ordinal_of(1)?))
            }
            _ => None,
        }
    }

    pub(crate) fn owners_hold(
        self,
        scope: &super::scope::DesignParameterScope,
        owners: &[crate::records::parameters::DesignParameterOwner],
    ) -> bool {
        if !matches!(
            self,
            Self::StandardExtended
                | Self::CompactOwnersTrailing
                | Self::CompactExtended(_)
                | Self::CompactOwnersLeading
        ) {
            return true;
        }
        let Some((offset_ordinal, length_ordinal)) = self.owner_ordinals(scope, owners) else {
            return false;
        };
        match (
            thread_owner_at(scope, owners, offset_ordinal),
            thread_owner_at(scope, owners, length_ordinal),
        ) {
            (Some(offset), Some(length)) => {
                let offset_value = offset.evaluated_value().get();
                offset.local_ordinal() == 0
                    && if matches!(self, Self::StandardExtended | Self::CompactExtended(_)) {
                        offset_value >= 0.0
                    } else {
                        offset_value == 0.0
                    }
                    && length.local_ordinal() == 1
                    && length.evaluated_value().get() > 0.0
            }
            _ => false,
        }
    }

    /// The face-selection groups of an extended standard scope: the first of
    /// each reference pair that remains without the two owner references.
    pub(crate) fn extended_standard_face_groups(
        references: &[u32],
        owner_ordinals: (usize, usize),
    ) -> Vec<u32> {
        references
            .iter()
            .enumerate()
            .filter(|(ordinal, _)| *ordinal != owner_ordinals.0 && *ordinal != owner_ordinals.1)
            .map(|(_, reference)| *reference)
            .step_by(2)
            .collect()
    }

    pub(crate) const fn face_reference_start(self) -> usize {
        match self {
            Self::CompactExtended(_) | Self::CompactOwnersLeading => 2,
            _ => 0,
        }
    }

    /// Whether the scope carries the standard one-group reference table.
    ///
    /// The other forms carry ordered `(face-selection group, sole member)` pairs.
    pub(crate) const fn has_standard_reference_table(&self) -> bool {
        match self {
            Self::Standard | Self::StandardLegacy | Self::StandardCompact => true,
            Self::Compact(_)
            | Self::CompactExtended(_)
            | Self::CompactLegacy
            | Self::CompactOwnersLeading
            | Self::CompactOwnersTrailing
            | Self::StandardExtended => false,
        }
    }

    /// The face-selection group of each face in the ordered scope references.
    pub(crate) fn face_groups(self, references: &[u32]) -> Vec<u32> {
        match self {
            Self::StandardExtended | Self::CompactOwnersTrailing => references
                .len()
                .checked_sub(2)
                .and_then(|pairs| references.get(..pairs))
                .unwrap_or_default()
                .iter()
                .step_by(2)
                .copied()
                .collect(),
            _ if self.has_standard_reference_table() => {
                references.first().copied().into_iter().collect()
            }
            _ => references
                .iter()
                .skip(self.face_reference_start())
                .step_by(2)
                .copied()
                .collect(),
        }
    }
}

/// The one owner record of `scope` that its reference at `ordinal` names.
fn thread_owner_at<'a>(
    scope: &super::scope::DesignParameterScope,
    owners: &'a [crate::records::parameters::DesignParameterOwner],
    ordinal: usize,
) -> Option<&'a crate::records::parameters::DesignParameterOwner> {
    let index = scope.reference_members().values().nth(ordinal)?;
    let mut matching = owners.iter().filter(|owner| {
        crate::ids::native_stream(owner.id()) == crate::ids::native_stream(&scope.id)
            && owner.scope_record_index() == scope.record_index
            && owner.record_index() == *index
    });
    let owner = matching.next()?;
    matching.next().is_none().then_some(owner)
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
    /// Whether the body geometry contains the helical form.
    pub(crate) modeled: bool,
    /// Thread profile angle between the flanks.
    pub(crate) angle: DesignThreadAngle,
    /// Ordered counted face-selection groups referenced by the scope.
    pub(crate) face_group_record_indices: Vec<u32>,
    /// Class, sense, and length flag of an extended prefix.
    pub(crate) extended: Option<DesignThreadExtended>,
}

impl DesignThreadConstruction {
    pub(crate) fn class(&self) -> Option<&str> {
        self.extended
            .as_ref()
            .map(|value| value.class.as_str())
            .or(match self.form {
                DesignThreadForm::Standard
                | DesignThreadForm::StandardLegacy
                | DesignThreadForm::StandardCompact => Some("6g"),
                DesignThreadForm::Compact(_)
                | DesignThreadForm::CompactLegacy
                | DesignThreadForm::CompactOwnersLeading
                | DesignThreadForm::CompactOwnersTrailing => Some("6H"),
                _ => None,
            })
    }

    pub(crate) fn external(&self) -> Option<bool> {
        self.extended.as_ref().map(|value| value.external)
    }
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
        if nominal_size_value(&text).is_none() {
            return Err(
                "nominal_size_text must encode a finite positive decimal, fraction, or numbered inch size"
                    .into(),
            );
        }
        Ok(Self(text))
    }
}

/// The value of a positive finite decimal, or of a fraction `N/D` of two
/// positive integers.
fn nominal_size_value(text: &str) -> Option<f64> {
    let text = numbered_size_decimal(text).unwrap_or(text);
    let value = match text.split_once('/') {
        None => text.parse::<f64>().ok()?,
        Some((numerator, denominator)) => {
            let numerator = numerator.parse::<u32>().ok().filter(|value| *value > 0)?;
            let denominator = denominator.parse::<u32>().ok().filter(|value| *value > 0)?;
            f64::from(numerator) / f64::from(denominator)
        }
    };
    (value.is_finite() && value > 0.0).then_some(value)
}

/// The decimal of a numbered inch size `D(#N)`.
fn numbered_size_decimal(text: &str) -> Option<&str> {
    let (decimal, number) = text.strip_suffix(')')?.split_once("(#")?;
    if number.is_empty() || !number.bytes().all(|byte| byte.is_ascii_digit()) {
        return None;
    }
    if decimal.is_empty()
        || !decimal
            .bytes()
            .all(|byte| byte.is_ascii_digit() || byte == b'.')
        || decimal.bytes().filter(|byte| *byte == b'.').count() > 1
    {
        return None;
    }
    Some(decimal)
}

impl DesignThreadNominalSize {
    #[must_use]
    #[cfg(test)]
    pub(crate) fn text(&self) -> &str {
        &self.0
    }

    pub(crate) fn value(&self) -> Option<f64> {
        nominal_size_value(&self.0)
    }

    /// The nominal diameter in inches of a numbered size `D(#N)`.
    pub(crate) fn numbered_inches(&self) -> Option<f64> {
        numbered_size_decimal(&self.0)?.parse().ok()
    }

    /// Whether the size is a plain decimal: a fraction names a size in the
    /// units of its profile, and a numbered size names inches.
    pub(crate) fn is_decimal(&self) -> bool {
        !self.0.contains('/') && self.numbered_inches().is_none()
    }
}

/// Thread profile angle between the flanks, in degrees.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct DesignThreadAngle(f64);

impl DesignThreadAngle {
    /// The 60-degree profile angle.
    pub(crate) const SIXTY: Self = Self(60.0);

    /// Admit a finite angle strictly between zero and 180 degrees.
    pub(crate) fn new(degrees: f64) -> Option<Self> {
        (degrees.is_finite() && degrees > 0.0 && degrees < 180.0).then_some(Self(degrees))
    }

    /// The angle in degrees.
    pub(crate) fn degrees(self) -> f64 {
        self.0
    }

    pub(crate) fn is_sixty(self) -> bool {
        self.0.to_bits() == Self::SIXTY.0.to_bits()
    }
}

fn default_thread_angle() -> f64 {
    DesignThreadAngle::SIXTY.degrees()
}

impl Serialize for DesignThreadConstruction {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let nominal_size = self
            .nominal_size
            .value()
            .ok_or_else(|| serde::ser::Error::custom("nominal_size_text is not a positive size"))?;
        let (form, trailing_reference) = match &self.form {
            DesignThreadForm::Standard => (DesignThreadFormWire::Standard, None),
            DesignThreadForm::Compact(reference) => {
                (DesignThreadFormWire::Compact, reference.as_ref())
            }
            DesignThreadForm::CompactExtended(reference) => {
                (DesignThreadFormWire::CompactExtended, Some(reference))
            }
            DesignThreadForm::StandardLegacy => (DesignThreadFormWire::StandardLegacy, None),
            DesignThreadForm::CompactLegacy => (DesignThreadFormWire::CompactLegacy, None),
            DesignThreadForm::StandardCompact => (DesignThreadFormWire::StandardCompact, None),
            DesignThreadForm::StandardExtended => (DesignThreadFormWire::StandardExtended, None),
            DesignThreadForm::CompactOwnersLeading => {
                (DesignThreadFormWire::CompactOwnersLeading, None)
            }
            DesignThreadForm::CompactOwnersTrailing => {
                (DesignThreadFormWire::CompactOwnersTrailing, None)
            }
        };
        let mut fields = serializer.serialize_struct(
            "DesignThreadConstructionWire",
            12 + if trailing_reference.is_some() { 2 } else { 0 }
                + usize::from(self.extended.is_some())
                + usize::from(!self.angle.is_sixty()),
        )?;
        fields.serialize_field("form", &form)?;
        fields.serialize_field("designation_offset", &self.designation_offset)?;
        fields.serialize_field("designation", self.designation.as_str())?;
        fields.serialize_field("nominal_size_text", &self.nominal_size.0)?;
        fields.serialize_field("nominal_size", &nominal_size)?;
        fields.serialize_field("profile", self.profile.as_str())?;
        fields.serialize_field("major_diameter", &self.diameters.major())?;
        fields.serialize_field("minor_diameter", &self.diameters.minor())?;
        fields.serialize_field("pitch", &self.pitch.get())?;
        fields.serialize_field("pitch_diameter", &self.diameters.pitch())?;
        fields.serialize_field("modeled", &self.modeled)?;
        if !self.angle.is_sixty() {
            fields.serialize_field("angle", &self.angle.degrees())?;
        }
        if let Some(reference) = trailing_reference {
            fields.serialize_field("trailing_reference_record_index", &reference.value.get())?;
            fields.serialize_field("trailing_reference_offset", &reference.offset)?;
        }
        fields.serialize_field("face_group_record_indices", &self.face_group_record_indices)?;
        if let Some(extended) = &self.extended {
            fields.serialize_field("extended", extended)?;
        }
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
    CompactOwnersLeading,
    CompactOwnersTrailing,
    CompactExtended,
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
    /// Whether the body geometry contains the helical form.
    modeled: bool,
    /// Thread profile angle between the flanks, in degrees.
    #[serde(default = "default_thread_angle")]
    angle: f64,
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
    /// Class, sense, and length flag of an extended prefix.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_extended"
    )]
    extended: Option<DesignThreadExtended>,
}

impl TryFrom<DesignThreadConstructionWire> for DesignThreadConstruction {
    type Error = String;
    fn try_from(value: DesignThreadConstructionWire) -> Result<Self, Self::Error> {
        let nominal_size = DesignThreadNominalSize::try_from(value.nominal_size_text)?;
        if nominal_size
            .value()
            .ok_or("nominal_size_text must encode a positive size")?
            != value.nominal_size
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
            (DesignThreadFormWire::CompactExtended, Some(reference)) => {
                DesignThreadForm::CompactExtended(reference)
            }
            (DesignThreadFormWire::CompactExtended, None) => {
                return Err("compact_extended requires a trailing Hole reference".into());
            }
            (DesignThreadFormWire::Standard, None) => DesignThreadForm::Standard,
            (DesignThreadFormWire::StandardLegacy, None) => DesignThreadForm::StandardLegacy,
            (DesignThreadFormWire::CompactLegacy, None) => DesignThreadForm::CompactLegacy,
            (DesignThreadFormWire::StandardCompact, None) => DesignThreadForm::StandardCompact,
            (DesignThreadFormWire::StandardExtended, None) => DesignThreadForm::StandardExtended,
            (DesignThreadFormWire::CompactOwnersLeading, None) => {
                DesignThreadForm::CompactOwnersLeading
            }
            (DesignThreadFormWire::CompactOwnersTrailing, None) => {
                DesignThreadForm::CompactOwnersTrailing
            }
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
            modeled: value.modeled,
            angle: DesignThreadAngle::new(value.angle)
                .ok_or("angle must be finite and strictly between 0 and 180 degrees")?,
            face_group_record_indices: value.face_group_record_indices,
            extended: value.extended,
        })
    }
}

#[cfg(test)]
mod tests;

mod identity_rewrite;
