// SPDX-License-Identifier: Apache-2.0
//! Sketch text, points, curves, surfaces and NURBS poles.

use super::references::DesignClassTag;
use cadmpeg_ir::math::{Point2, Point3, Vector3};
use cadmpeg_ir::scalar::Angle;
use cadmpeg_ir::sketches::TextPlacement;
use cadmpeg_ir::topology::Color;
use serde::{Deserialize, Serialize};

cadmpeg_core::named_optional_field!(deserialize_anchor, Point2, "anchor");
cadmpeg_core::named_optional_field!(deserialize_base_id, u64, "base_id");
cadmpeg_core::named_optional_field!(deserialize_closure, SketchPointClosureSerde, "closure");
cadmpeg_core::named_optional_field!(deserialize_entity_genesis, u64, "entity_genesis");
cadmpeg_core::named_optional_field!(deserialize_first_reference, u32, "first_reference");
cadmpeg_core::named_optional_field!(deserialize_geometry, SketchCurveGeometry, "geometry");
cadmpeg_core::named_optional_field!(
    deserialize_horizontal_alignment,
    u32,
    "horizontal_alignment"
);
cadmpeg_core::named_optional_field!(deserialize_owner_reference, u32, "owner_reference");
cadmpeg_core::named_optional_field!(deserialize_persistent_id, u64, "persistent_id");
cadmpeg_core::named_optional_field!(deserialize_rotation, f64, "rotation");
cadmpeg_core::named_optional_field!(deserialize_second_reference, u32, "second_reference");
cadmpeg_core::named_optional_field!(deserialize_vertical_alignment, u32, "vertical_alignment");
cadmpeg_core::named_optional_field!(deserialize_width_factor, f64, "width_factor");
/// One text entity in a Fusion sketch coordinate system.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(try_from = "SketchTextSerde", into = "SketchTextSerde")]
pub(crate) struct SketchText {
    /// Globally unique deterministic identifier for this native record.
    pub(crate) id: String,
    /// Index of this text record within the `BulkStream` tree.
    pub(crate) record_index: u32,
    /// Owning sketch record index.
    pub(crate) owner_reference: u32,
    /// Source per-file dynamic ASCII class tag naming this record's type.
    pub(crate) class_tag: DesignClassTag,
    /// Record version of this record's class, from its Design `MetaStream` type
    /// table. It selects the member sequence the record was written under.
    pub(crate) class_version: u32,
    /// Byte offset of this record within its Design `BulkStream`.
    pub(crate) byte_offset: u64,
    /// Optional `EntityGenesis` origin bitfield.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) entity_genesis: Option<u64>,
    /// Persistent identity of the text entity. A `txt_tag` record below class
    /// version 4 writes no identity key and stores none.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) persistent_id: Option<u64>,
    /// Persistent base identity, a property key absent from some records.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) base_id: Option<u64>,
    /// Unicode text content.
    pub(crate) text: String,
    /// Font-family name.
    pub(crate) font_family: String,
    /// Numeric font weight stored by the sketch-text class.
    pub(crate) font_weight: i32,
    /// Nominal text height in millimetres.
    pub(crate) height: f64,
    /// Display colour of the glyphs. Both identity forms store it, so it is
    /// never absent. `SketchGeometry` carries no display attribute on any
    /// variant, so the colour stays on the native record.
    pub(crate) color: Color,
    /// Identity-form layout: `txt_tag` placement or `textex_tag` width, alignment,
    /// and parameter references.
    pub(crate) layout: SketchTextLayout,
    /// Complete source record bytes for native replay and rewrite.
    #[serde(with = "cadmpeg_ir::bytes")]
    pub(crate) raw_bytes: Vec<u8>,
}

/// Horizontal and vertical alignment members of a sketch-text record.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct SketchTextAlignment {
    pub(crate) horizontal: u32,
    pub(crate) vertical: u32,
}

/// `txt_tag` versus `textex_tag` member layout of one sketch-text record.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum SketchTextLayout {
    TxtTag {
        placement: TextPlacement,
    },
    TextexTag {
        width_factor: f64,
        alignment: Option<SketchTextAlignment>,
        first_reference: Option<u32>,
        second_reference: Option<u32>,
        placement: Option<TextPlacement>,
    },
}

impl SketchText {
    pub(crate) fn width_factor(&self) -> Option<f64> {
        match self.layout {
            SketchTextLayout::TxtTag { .. } => None,
            SketchTextLayout::TextexTag { width_factor, .. } => Some(width_factor),
        }
    }

    pub(crate) fn placement(&self) -> Option<TextPlacement> {
        match self.layout {
            SketchTextLayout::TxtTag { placement } => Some(placement),
            SketchTextLayout::TextexTag { placement, .. } => placement,
        }
    }

    pub(crate) fn alignment(&self) -> Option<SketchTextAlignment> {
        match self.layout {
            SketchTextLayout::TxtTag { .. } => None,
            SketchTextLayout::TextexTag { alignment, .. } => alignment,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
struct SketchTextSerde {
    id: String,
    record_index: u32,
    owner_reference: u32,
    class_tag: String,
    class_version: u32,
    byte_offset: u64,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_entity_genesis"
    )]
    entity_genesis: Option<u64>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_persistent_id"
    )]
    persistent_id: Option<u64>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_base_id"
    )]
    base_id: Option<u64>,
    text: String,
    font_family: String,
    font_weight: i32,
    height: f64,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_width_factor"
    )]
    width_factor: Option<f64>,
    color: Color,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_anchor"
    )]
    anchor: Option<Point2>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_rotation"
    )]
    rotation: Option<f64>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_horizontal_alignment"
    )]
    horizontal_alignment: Option<u32>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_vertical_alignment"
    )]
    vertical_alignment: Option<u32>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_first_reference"
    )]
    first_reference: Option<u32>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_second_reference"
    )]
    second_reference: Option<u32>,
    #[serde(with = "cadmpeg_ir::bytes")]
    raw_bytes: Vec<u8>,
}

impl TryFrom<SketchTextSerde> for SketchText {
    type Error = String;

    fn try_from(wire: SketchTextSerde) -> Result<Self, Self::Error> {
        let placement = match (wire.anchor, wire.rotation) {
            (None, None) => None,
            (Some(anchor), Some(rotation)) => Some(TextPlacement {
                anchor,
                rotation: Angle::new(rotation).ok_or("sketch text rotation must be finite")?,
            }),
            _ => return Err("sketch text anchor and rotation must occur together".into()),
        };
        let alignment =
            match (wire.horizontal_alignment, wire.vertical_alignment) {
                (None, None) => None,
                (Some(horizontal), Some(vertical)) => Some(SketchTextAlignment {
                    horizontal,
                    vertical,
                }),
                _ => return Err(
                    "sketch text horizontal_alignment and vertical_alignment must occur together"
                        .into(),
                ),
            };
        let layout = match (
            wire.width_factor,
            alignment,
            wire.first_reference,
            wire.second_reference,
            placement,
        ) {
            (None, None, None, None, Some(placement)) => SketchTextLayout::TxtTag { placement },
            (Some(width_factor), alignment, first_reference, second_reference, placement) => {
                SketchTextLayout::TextexTag {
                    width_factor,
                    alignment,
                    first_reference,
                    second_reference,
                    placement,
                }
            }
            _ => {
                return Err(
                    "sketch text layout disagrees with width_factor, alignment, and placement"
                        .into(),
                )
            }
        };
        Ok(Self {
            id: wire.id,
            record_index: wire.record_index,
            owner_reference: wire.owner_reference,
            class_tag: wire.class_tag.try_into()?,
            class_version: wire.class_version,
            byte_offset: wire.byte_offset,
            entity_genesis: wire.entity_genesis,
            persistent_id: wire.persistent_id,
            base_id: wire.base_id,
            text: wire.text,
            font_family: wire.font_family,
            font_weight: wire.font_weight,
            height: wire.height,
            color: wire.color,
            layout,
            raw_bytes: wire.raw_bytes,
        })
    }
}

impl From<SketchText> for SketchTextSerde {
    fn from(text: SketchText) -> Self {
        let (width_factor, alignment, first_reference, second_reference, placement) =
            match text.layout {
                SketchTextLayout::TxtTag { placement } => (None, None, None, None, Some(placement)),
                SketchTextLayout::TextexTag {
                    width_factor,
                    alignment,
                    first_reference,
                    second_reference,
                    placement,
                } => (
                    Some(width_factor),
                    alignment,
                    first_reference,
                    second_reference,
                    placement,
                ),
            };
        Self {
            id: text.id,
            record_index: text.record_index,
            owner_reference: text.owner_reference,
            class_tag: text.class_tag.into(),
            class_version: text.class_version,
            byte_offset: text.byte_offset,
            entity_genesis: text.entity_genesis,
            persistent_id: text.persistent_id,
            base_id: text.base_id,
            text: text.text,
            font_family: text.font_family,
            font_weight: text.font_weight,
            height: text.height,
            width_factor,
            color: text.color,
            anchor: placement.map(|value| value.anchor),
            rotation: placement.map(|value| value.rotation.get()),
            horizontal_alignment: alignment.map(|value| value.horizontal),
            vertical_alignment: alignment.map(|value| value.vertical),
            first_reference,
            second_reference,
            raw_bytes: text.raw_bytes,
        }
    }
}

/// Selector and state following a three-coordinate sketch point payload.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SketchPointClosure {
    Selector0State0,
    Selector0State1,
    Selector1State0,
    Selector2State1,
    Selector4State0,
}

impl SketchPointClosure {
    pub(crate) fn selector(self) -> u64 {
        match self {
            Self::Selector0State0 | Self::Selector0State1 => 0,
            Self::Selector1State0 => 1,
            Self::Selector2State1 => 2,
            Self::Selector4State0 => 4,
        }
    }

    pub(crate) fn state(self) -> u8 {
        match self {
            Self::Selector0State0 | Self::Selector1State0 | Self::Selector4State0 => 0,
            Self::Selector0State1 | Self::Selector2State1 => 1,
        }
    }

    pub(crate) fn from_pair(selector: u64, state: u8) -> Option<Self> {
        match (selector, state) {
            (0, 0) => Some(Self::Selector0State0),
            (0, 1) => Some(Self::Selector0State1),
            (1, 0) => Some(Self::Selector1State0),
            (2, 1) => Some(Self::Selector2State1),
            (4, 0) => Some(Self::Selector4State0),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
struct SketchPointClosureSerde {
    selector: u64,
    state: u8,
}

impl TryFrom<SketchPointClosureSerde> for SketchPointClosure {
    type Error = String;

    fn try_from(wire: SketchPointClosureSerde) -> Result<Self, Self::Error> {
        Self::from_pair(wire.selector, wire.state).ok_or_else(|| {
            format!(
                "sketch point closure selector {} state {} is not an admitted pair",
                wire.selector, wire.state
            )
        })
    }
}

impl From<SketchPointClosure> for SketchPointClosureSerde {
    fn from(closure: SketchPointClosure) -> Self {
        Self {
            selector: closure.selector(),
            state: closure.state(),
        }
    }
}

/// Version-10 same-segment closure: selector `0` and state `0` or `1`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SketchPointClosure10 {
    State0,
    State1,
}

impl SketchPointClosure10 {
    pub(crate) fn from_closure(closure: SketchPointClosure) -> Option<Self> {
        match closure {
            SketchPointClosure::Selector0State0 => Some(Self::State0),
            SketchPointClosure::Selector0State1 => Some(Self::State1),
            _ => None,
        }
    }

    fn to_closure(self) -> SketchPointClosure {
        match self {
            Self::State0 => SketchPointClosure::Selector0State0,
            Self::State1 => SketchPointClosure::Selector0State1,
        }
    }
}

/// Version-10 inline-typed closure: `(0, 0)`, `(0, 1)`, or `(2, 1)`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SketchPointClosure10Inline {
    Selector0State0,
    Selector0State1,
    Selector2State1,
}

impl SketchPointClosure10Inline {
    pub(crate) fn from_closure(closure: SketchPointClosure) -> Option<Self> {
        match closure {
            SketchPointClosure::Selector0State0 => Some(Self::Selector0State0),
            SketchPointClosure::Selector0State1 => Some(Self::Selector0State1),
            SketchPointClosure::Selector2State1 => Some(Self::Selector2State1),
            _ => None,
        }
    }

    fn to_closure(self) -> SketchPointClosure {
        match self {
            Self::Selector0State0 => SketchPointClosure::Selector0State0,
            Self::Selector0State1 => SketchPointClosure::Selector0State1,
            Self::Selector2State1 => SketchPointClosure::Selector2State1,
        }
    }
}

/// Serialized member sequence of one sketch-point record.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum SketchPointRecordForm {
    /// Class version 0: one flag, two coordinates, and no persistent identity.
    Version0 { flag: bool },
    /// Class version 8: seven flags and an eight-zero closure lane.
    Version8 {
        persistent_id: std::num::NonZeroU64,
        flags: [bool; 7],
        /// Third sketch coordinate in millimetres.
        depth: f64,
    },
    /// Class version 10 with same-segment references and seven flags.
    Version10 {
        /// Third sketch coordinate in millimetres.
        depth: f64,
        persistent_id: std::num::NonZeroU64,
        flags: [bool; 7],
        closure: SketchPointClosure10,
    },
    /// Class version 10 with inline target-type GUIDs on its references.
    Version10InlineTyped {
        /// Third sketch coordinate in millimetres.
        depth: f64,
        /// Final inline-typed reference following the repeated companion reference.
        trailing_reference: u32,
        persistent_id: std::num::NonZeroU64,
        flags: [bool; 7],
        closure: SketchPointClosure10Inline,
    },
    /// Class version 11 with same-segment references and eight flags.
    Version11 {
        /// Third sketch coordinate in millimetres.
        depth: f64,
        /// Optional origin bitfield preceding the persistent identity.
        entity_genesis: Option<u64>,
        /// Whether four fixed zero bytes follow the repeated companion reference.
        padded_paired_reference: bool,
        /// Whether the paired companion record carries the fixed present-zero prefix.
        companion_prefix_present_zero: bool,
        persistent_id: std::num::NonZeroU64,
        flags: [bool; 8],
        closure: SketchPointClosure,
    },
    /// Class version 11 with inline target-type GUIDs on its references and eight flags.
    Version11InlineTyped {
        /// Third sketch coordinate in millimetres.
        depth: f64,
        /// Optional origin bitfield preceding the persistent identity.
        entity_genesis: Option<u64>,
        /// Final inline-typed reference following the repeated companion reference.
        trailing_reference: u32,
        /// Whether the paired companion record carries the fixed present-zero prefix.
        companion_prefix_present_zero: bool,
        persistent_id: std::num::NonZeroU64,
        flags: [bool; 8],
        closure: SketchPointClosure,
    },
}

impl SketchPointRecordForm {
    #[cfg(test)]
    pub(crate) fn version11(
        persistent_id: u64,
        closure: SketchPointClosure,
        entity_genesis: Option<u64>,
        depth: f64,
    ) -> Self {
        Self::Version11 {
            depth,
            entity_genesis,
            padded_paired_reference: false,
            companion_prefix_present_zero: false,
            persistent_id: std::num::NonZeroU64::new(persistent_id).unwrap(),
            flags: [false; 8],
            closure,
        }
    }

    pub(crate) fn depth(&self) -> f64 {
        match *self {
            Self::Version0 { .. } => 0.0,
            Self::Version8 { depth, .. }
            | Self::Version10 { depth, .. }
            | Self::Version10InlineTyped { depth, .. }
            | Self::Version11 { depth, .. }
            | Self::Version11InlineTyped { depth, .. } => depth,
        }
    }

    pub(crate) fn class_version(&self) -> u32 {
        match self {
            Self::Version0 { .. } => 0,
            Self::Version8 { .. } => 8,
            Self::Version10 { .. } | Self::Version10InlineTyped { .. } => 10,
            Self::Version11 { .. } | Self::Version11InlineTyped { .. } => 11,
        }
    }

    fn companion_prefix_present_zero(&self) -> bool {
        match *self {
            Self::Version11 {
                companion_prefix_present_zero,
                ..
            }
            | Self::Version11InlineTyped {
                companion_prefix_present_zero,
                ..
            } => companion_prefix_present_zero,
            Self::Version0 { .. }
            | Self::Version8 { .. }
            | Self::Version10 { .. }
            | Self::Version10InlineTyped { .. } => false,
        }
    }

    /// Return this form with the companion prefix observed on the paired record,
    /// or `None` when the form has no member for a present-zero prefix.
    pub(crate) fn with_companion_prefix_present_zero(self, present_zero: bool) -> Option<Self> {
        match self {
            Self::Version11 {
                depth,
                entity_genesis,
                padded_paired_reference,
                persistent_id,
                flags,
                closure,
                ..
            } => Some(Self::Version11 {
                depth,
                entity_genesis,
                padded_paired_reference,
                companion_prefix_present_zero: present_zero,
                persistent_id,
                flags,
                closure,
            }),
            Self::Version11InlineTyped {
                depth,
                entity_genesis,
                trailing_reference,
                persistent_id,
                flags,
                closure,
                ..
            } => Some(Self::Version11InlineTyped {
                depth,
                entity_genesis,
                trailing_reference,
                companion_prefix_present_zero: present_zero,
                persistent_id,
                flags,
                closure,
            }),
            other => (!present_zero).then_some(other),
        }
    }

    pub(crate) fn uses_inline_typed_references(&self) -> bool {
        matches!(
            self,
            Self::Version10InlineTyped { .. } | Self::Version11InlineTyped { .. }
        )
    }

    pub(crate) fn persistent_id(&self) -> Option<u64> {
        match *self {
            Self::Version0 { .. } => None,
            Self::Version8 { persistent_id, .. }
            | Self::Version10 { persistent_id, .. }
            | Self::Version10InlineTyped { persistent_id, .. }
            | Self::Version11 { persistent_id, .. }
            | Self::Version11InlineTyped { persistent_id, .. } => Some(persistent_id.get()),
        }
    }

    pub(crate) fn flags(&self) -> [u8; 8] {
        let mut flags = [0; 8];
        match self {
            Self::Version0 { flag, .. } => flags[0] = u8::from(*flag),
            Self::Version8 { flags: source, .. }
            | Self::Version10 { flags: source, .. }
            | Self::Version10InlineTyped { flags: source, .. } => {
                flags[..7].copy_from_slice(&source.map(u8::from));
            }
            Self::Version11 { flags: source, .. }
            | Self::Version11InlineTyped { flags: source, .. } => flags = source.map(u8::from),
        }
        flags
    }

    pub(crate) fn closure(&self) -> Option<SketchPointClosure> {
        match *self {
            Self::Version0 { .. } => None,
            Self::Version8 { .. } => Some(SketchPointClosure::Selector0State0),
            Self::Version10 { closure, .. } => Some(closure.to_closure()),
            Self::Version10InlineTyped { closure, .. } => Some(closure.to_closure()),
            Self::Version11 { closure, .. } | Self::Version11InlineTyped { closure, .. } => {
                Some(closure)
            }
        }
    }
}

/// Encoding of every reference owned by a point companion.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SketchPointCompanionReferenceEncoding {
    /// Target entity ID followed directly by the same-segment flags.
    SameSegment,
    /// Target entity ID followed by the target type GUID and same-segment flags.
    InlineTyped,
}

impl SketchPointCompanionReferenceEncoding {
    /// Return the reference encoding the point record form dictates.
    pub(crate) fn for_form(form: &SketchPointRecordForm) -> Self {
        if form.uses_inline_typed_references() {
            Self::InlineTyped
        } else {
            Self::SameSegment
        }
    }
}

/// Reverse curve-incidence record paired with a version-11 sketch point.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct SketchPointCompanion {
    /// Incident sketch-curve record indexes in serialized order.
    pub(crate) incident_curves: Vec<u32>,
}

impl SketchPointCompanion {
    fn validate(&self) -> Result<(), String> {
        let unique: std::collections::HashSet<_> = self.incident_curves.iter().collect();
        if unique.len() != self.incident_curves.len() {
            return Err("sketch point companion.incident_curves must be distinct".into());
        }
        Ok(())
    }
}

/// Borrowed companion payload with the prefix derived for older point forms.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct SketchPointCompanionRef<'a> {
    pub(crate) prefix_present_zero: bool,
    pub(crate) incident_curves: &'a [u32],
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct SketchPointCompanionWire {
    #[serde(default)]
    incident_curves: Vec<u32>,
}

// Serde requires `skip_serializing_if` predicates to borrow the field.
#[allow(clippy::trivially_copy_pass_by_ref)]
fn sketch_point_flags_are_zero(flags: &[u8; 8]) -> bool {
    flags.iter().all(|flag| *flag == 0)
}

/// One point in a Fusion sketch coordinate system.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(try_from = "SketchPointSerde", into = "SketchPointSerde")]
pub(crate) struct SketchPoint {
    /// Globally unique deterministic identifier for this native record.
    pub(crate) id: String,
    /// Index of this point record within the `BulkStream` tree.
    pub(crate) record_index: u32,
    /// Resolved owning-sketch reference from a direct backlink, typed relation,
    /// or sketch-container member run.
    pub(crate) owner_reference: Option<u32>,
    /// Source per-file dynamic three-digit ASCII class tag naming this point's record type.
    pub(crate) class_tag: DesignClassTag,
    /// Byte offset of this record within its Design `BulkStream`.
    pub(crate) byte_offset: u64,
    /// Byte offset of the first coordinate relative to the record start.
    pub(crate) coordinate_offset: u32,
    /// Serialized point-record member sequence, identity, flags, and closure.
    record_form: SketchPointRecordForm,
    companion: SketchPointCompanion,
    /// Record index of the paired reverse curve-incidence companion.
    pub(crate) paired_reference: u32,
    /// First two sketch coordinates in millimetres.
    coordinates: Point2,
}

#[derive(Debug, Clone)]
pub(crate) struct SketchPointDraft {
    /// Globally unique deterministic identifier for this native record.
    pub(crate) id: String,
    /// Index of this point record within the `BulkStream` tree.
    pub(crate) record_index: u32,
    /// Resolved owning-sketch reference from a direct backlink, typed relation,
    /// or sketch-container member run.
    pub(crate) owner_reference: Option<u32>,
    /// Source per-file dynamic three-digit ASCII class tag naming this point's record type.
    pub(crate) class_tag: DesignClassTag,
    /// Byte offset of this record within its Design `BulkStream`.
    pub(crate) byte_offset: u64,
    /// Byte offset of the first coordinate relative to the record start.
    pub(crate) coordinate_offset: u32,
    /// Serialized point-record member sequence, identity, flags, and closure.
    pub(crate) record_form: SketchPointRecordForm,
    pub(crate) companion: SketchPointCompanion,
    /// Record index of the paired reverse curve-incidence companion.
    pub(crate) paired_reference: u32,
    /// First two sketch coordinates in millimetres.
    pub(crate) coordinates: Point2,
}

impl TryFrom<SketchPointDraft> for SketchPoint {
    type Error = String;
    fn try_from(draft: SketchPointDraft) -> Result<Self, Self::Error> {
        if !draft.coordinates.is_finite() {
            return Err("sketch point coordinates must be finite".into());
        }
        if !draft.record_form.depth().is_finite() {
            return Err("sketch point depth must be finite".into());
        }
        draft.companion.validate()?;
        Ok(Self {
            id: draft.id,
            record_index: draft.record_index,
            owner_reference: draft.owner_reference,
            class_tag: draft.class_tag,
            byte_offset: draft.byte_offset,
            coordinate_offset: draft.coordinate_offset,
            record_form: draft.record_form,
            companion: draft.companion,
            paired_reference: draft.paired_reference,
            coordinates: draft.coordinates,
        })
    }
}

impl SketchPoint {
    pub(crate) fn coordinates(&self) -> Point2 {
        self.coordinates
    }
    pub(crate) fn record_form(&self) -> &SketchPointRecordForm {
        &self.record_form
    }
    pub(crate) fn try_set_coordinates(&mut self, coordinates: Point2) -> Result<(), String> {
        if !coordinates.is_finite() {
            return Err("sketch point coordinates must be finite".into());
        }
        self.coordinates = coordinates;
        Ok(())
    }
    #[cfg(test)]
    pub(crate) fn try_set_record_form(
        &mut self,
        record_form: SketchPointRecordForm,
    ) -> Result<(), String> {
        if !record_form.depth().is_finite() {
            return Err("sketch point depth must be finite".into());
        }
        self.record_form = record_form;
        Ok(())
    }

    #[cfg(test)]
    pub(crate) fn try_set_companion(
        &mut self,
        companion: SketchPointCompanion,
    ) -> Result<(), String> {
        companion.validate()?;
        self.companion = companion;
        Ok(())
    }
    pub(crate) fn companion(&self) -> SketchPointCompanionRef<'_> {
        SketchPointCompanionRef {
            prefix_present_zero: self.record_form.companion_prefix_present_zero(),
            incident_curves: &self.companion.incident_curves,
        }
    }

    pub(crate) fn depth(&self) -> f64 {
        self.record_form.depth()
    }

    pub(crate) fn entity_genesis(&self) -> Option<u64> {
        match self.record_form {
            SketchPointRecordForm::Version11 { entity_genesis, .. }
            | SketchPointRecordForm::Version11InlineTyped { entity_genesis, .. } => entity_genesis,
            SketchPointRecordForm::Version0 { .. }
            | SketchPointRecordForm::Version8 { .. }
            | SketchPointRecordForm::Version10 { .. }
            | SketchPointRecordForm::Version10InlineTyped { .. } => None,
        }
    }

    pub(crate) fn persistent_id(&self) -> Option<u64> {
        self.record_form.persistent_id()
    }

    pub(crate) fn flags(&self) -> [u8; 8] {
        self.record_form.flags()
    }

    pub(crate) fn closure(&self) -> Option<SketchPointClosure> {
        self.record_form.closure()
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
enum SketchPointRecordFormSerde {
    Version0,
    Version8,
    Version10,
    Version10InlineTyped {
        trailing_reference: u32,
    },
    Version11 {
        padded_paired_reference: bool,
        #[serde(default)]
        companion_prefix_present_zero: bool,
    },
    Version11InlineTyped {
        trailing_reference: u32,
        #[serde(default)]
        companion_prefix_present_zero: bool,
    },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub(super) struct SketchPointSerde {
    id: String,
    record_index: u32,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_owner_reference"
    )]
    owner_reference: Option<u32>,
    class_tag: String,
    byte_offset: u64,
    coordinate_offset: u32,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_entity_genesis"
    )]
    entity_genesis: Option<u64>,
    #[serde(default)]
    record_form: SketchPointRecordFormSerde,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_persistent_id"
    )]
    persistent_id: Option<u64>,
    paired_reference: u32,
    #[serde(default, skip_serializing_if = "sketch_point_flags_are_zero")]
    flags: [u8; 8],
    coordinates: Point2,
    #[serde(default)]
    pub(super) depth: f64,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_closure"
    )]
    closure: Option<SketchPointClosureSerde>,
    companion: SketchPointCompanionWire,
}

impl Default for SketchPointRecordFormSerde {
    fn default() -> Self {
        Self::Version11 {
            padded_paired_reference: false,
            companion_prefix_present_zero: false,
        }
    }
}

fn seven_flags(flags: [bool; 8]) -> Result<[bool; 7], String> {
    if flags[7] {
        return Err("sketch point flags beyond the form width must be zero".into());
    }
    let mut dest = [false; 7];
    dest.copy_from_slice(&flags[..7]);
    Ok(dest)
}

impl TryFrom<SketchPointSerde> for SketchPoint {
    type Error = String;

    fn try_from(wire: SketchPointSerde) -> Result<Self, Self::Error> {
        if wire.flags.iter().any(|flag| *flag > 1) {
            return Err("sketch point flags must be zero or one".into());
        }
        if wire.entity_genesis.is_some()
            && !matches!(
                wire.record_form,
                SketchPointRecordFormSerde::Version11 { .. }
                    | SketchPointRecordFormSerde::Version11InlineTyped { .. }
            )
        {
            return Err("sketch point entity_genesis requires version 11".into());
        }
        if matches!(wire.record_form, SketchPointRecordFormSerde::Version0) && wire.depth != 0.0 {
            return Err("sketch point depth must be zero for version 0".into());
        }
        let flags = wire.flags.map(|flag| flag == 1);
        let closure = wire.closure.map(SketchPointClosure::try_from).transpose()?;
        let persistent_id = wire
            .persistent_id
            .map(|value| std::num::NonZeroU64::new(value).ok_or("persistent_id must be nonzero"))
            .transpose()?;
        let record_form = match (wire.record_form, persistent_id, closure) {
            (SketchPointRecordFormSerde::Version0, None, None) => {
                SketchPointRecordForm::Version0 { flag: flags[0] }
            }
            (
                SketchPointRecordFormSerde::Version8,
                Some(persistent_id),
                Some(SketchPointClosure::Selector0State0),
            ) => SketchPointRecordForm::Version8 {
                depth: wire.depth,
                persistent_id,
                flags: seven_flags(flags)?,
            },
            (SketchPointRecordFormSerde::Version10, Some(persistent_id), Some(closure)) => {
                SketchPointRecordForm::Version10 {
                    depth: wire.depth,
                    persistent_id,
                    flags: seven_flags(flags)?,
                    closure: SketchPointClosure10::from_closure(closure).ok_or_else(|| {
                        "sketch point version-10 closure must be selector 0 with state 0 or 1"
                            .to_string()
                    })?,
                }
            }
            (
                SketchPointRecordFormSerde::Version10InlineTyped { trailing_reference },
                Some(persistent_id),
                Some(closure),
            ) => SketchPointRecordForm::Version10InlineTyped {
                depth: wire.depth,
                trailing_reference,
                persistent_id,
                flags: seven_flags(flags)?,
                closure: SketchPointClosure10Inline::from_closure(closure).ok_or_else(|| {
                    "sketch point version-10 inline closure must be (0,0), (0,1), or (2,1)"
                        .to_string()
                })?,
            },
            (
                SketchPointRecordFormSerde::Version11 {
                    padded_paired_reference,
                    companion_prefix_present_zero,
                },
                Some(persistent_id),
                Some(closure),
            ) => SketchPointRecordForm::Version11 {
                depth: wire.depth,
                entity_genesis: wire.entity_genesis,
                padded_paired_reference,
                companion_prefix_present_zero,
                persistent_id,
                flags,
                closure,
            },
            (
                SketchPointRecordFormSerde::Version11InlineTyped {
                    trailing_reference,
                    companion_prefix_present_zero,
                },
                Some(persistent_id),
                Some(closure),
            ) => SketchPointRecordForm::Version11InlineTyped {
                depth: wire.depth,
                entity_genesis: wire.entity_genesis,
                trailing_reference,
                companion_prefix_present_zero,
                persistent_id,
                flags,
                closure,
            },
            _ => {
                return Err(
                    "sketch point record_form disagrees with persistent_id or closure".into(),
                );
            }
        };
        if matches!(record_form, SketchPointRecordForm::Version0 { .. })
            && wire.flags[1..].iter().any(|flag| *flag != 0)
        {
            return Err("sketch point flags beyond the form width must be zero".into());
        }
        let companion = SketchPointCompanion {
            incident_curves: wire.companion.incident_curves,
        };
        Self::try_from(SketchPointDraft {
            id: wire.id,
            record_index: wire.record_index,
            owner_reference: wire.owner_reference,
            class_tag: wire.class_tag.try_into()?,
            byte_offset: wire.byte_offset,
            coordinate_offset: wire.coordinate_offset,
            record_form,
            companion,
            paired_reference: wire.paired_reference,
            coordinates: wire.coordinates,
        })
    }
}

impl From<SketchPoint> for SketchPointSerde {
    fn from(point: SketchPoint) -> Self {
        let depth = point.depth();
        let entity_genesis = point.entity_genesis();
        let persistent_id = point.persistent_id();
        let flags = point.flags();
        let closure = point.closure().map(SketchPointClosureSerde::from);
        let record_form = match point.record_form {
            SketchPointRecordForm::Version0 { .. } => SketchPointRecordFormSerde::Version0,
            SketchPointRecordForm::Version8 { .. } => SketchPointRecordFormSerde::Version8,
            SketchPointRecordForm::Version10 { .. } => SketchPointRecordFormSerde::Version10,
            SketchPointRecordForm::Version10InlineTyped {
                trailing_reference, ..
            } => SketchPointRecordFormSerde::Version10InlineTyped { trailing_reference },
            SketchPointRecordForm::Version11 {
                padded_paired_reference,
                companion_prefix_present_zero,
                ..
            } => SketchPointRecordFormSerde::Version11 {
                padded_paired_reference,
                companion_prefix_present_zero,
            },
            SketchPointRecordForm::Version11InlineTyped {
                trailing_reference,
                companion_prefix_present_zero,
                ..
            } => SketchPointRecordFormSerde::Version11InlineTyped {
                trailing_reference,
                companion_prefix_present_zero,
            },
        };
        let companion = SketchPointCompanionWire {
            incident_curves: point.companion.incident_curves,
        };
        Self {
            id: point.id,
            record_index: point.record_index,
            owner_reference: point.owner_reference,
            class_tag: point.class_tag.into(),
            byte_offset: point.byte_offset,
            coordinate_offset: point.coordinate_offset,
            entity_genesis,
            record_form,
            persistent_id,
            paired_reference: point.paired_reference,
            flags,
            coordinates: point.coordinates,
            depth,
            closure,
            companion,
        }
    }
}

/// Persistent identity pair attached to one source sketch-curve record.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub(crate) struct SketchCurveIdentity {
    /// Globally unique deterministic identifier for this native record.
    pub(crate) id: String,
    /// Index of this identity record within the `BulkStream` tree.
    pub(crate) record_index: u32,
    /// Direct owning-sketch backlink when the curve record form carries one.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_owner_reference"
    )]
    pub(crate) owner_reference: Option<u32>,
    /// Source per-file dynamic three-digit ASCII class tag naming this record's type.
    pub(crate) class_tag: DesignClassTag,
    /// Byte offset of this record within its Design `BulkStream`.
    pub(crate) byte_offset: u64,
    /// Byte offset of the fixed analytic geometry payload relative to the record start.
    pub(crate) geometry_offset: u32,
    /// Optional `EntityGenesis` origin bitfield carried ahead of the curve identities.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_entity_genesis"
    )]
    pub(crate) entity_genesis: Option<u64>,
    /// Primary persistent identifier of the source sketch curve.
    pub(crate) primary_id: std::num::NonZeroU64,
    /// Secondary persistent identifier of the source sketch curve (e.g. its
    /// complementary endpoint or paired-curve identity).
    pub(crate) secondary_id: u64,
    /// Exact analytic geometry carried by this sketch-curve record, when the
    /// decoder recovered one; `None` when the geometry subtype was not decoded.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_geometry"
    )]
    pub(crate) geometry: Option<SketchCurveGeometry>,
}

/// One persistent tensor-product surface owned by a spatial Fusion sketch.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub(crate) struct SketchSurface {
    /// Globally unique deterministic identifier for this native record.
    pub(crate) id: String,
    /// Index of this surface record within the `BulkStream` tree.
    pub(crate) record_index: u32,
    /// Owning sketch entity derived from relations using this surface.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_owner_reference"
    )]
    pub(crate) owner_reference: Option<u32>,
    /// Source per-file dynamic three-digit ASCII class tag.
    pub(crate) class_tag: DesignClassTag,
    /// Byte offset of this record within its Design `BulkStream`.
    pub(crate) byte_offset: u64,
    /// Optional `EntityGenesis` origin bitfield carried ahead of the surface identity.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_entity_genesis"
    )]
    pub(crate) entity_genesis: Option<u64>,
    /// Persistent Fusion identifier for the sketch surface.
    pub(crate) persistent_id: std::num::NonZeroU64,
    /// Degree in the first surface parameter.
    pub(crate) u_degree: u32,
    /// Degree in the second surface parameter.
    pub(crate) v_degree: u32,
    /// Full knot vector in the first parameter.
    pub(crate) u_knots: Vec<f64>,
    /// Full knot vector in the second parameter.
    pub(crate) v_knots: Vec<f64>,
    /// Rectangular control grid in first-parameter-major order, in millimetres.
    pub(crate) control_points: Vec<Vec<Point3>>,
}

/// Exact analytic geometry carried by a source sketch-curve record.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(try_from = "SketchCurveGeometryWire", into = "SketchCurveGeometryWire")]
pub(crate) enum SketchCurveGeometry {
    /// A straight line segment.
    Line {
        /// Start point in sketch space, millimetres.
        start: Point3,
        /// End point in sketch space, millimetres.
        end: Point3,
        /// Direction from `start` to `end`; the decoder stores it at unit length.
        direction: Vector3,
        /// Normal of the sketch plane the line lies in; the decoder stores it at
        /// unit length.
        normal: Vector3,
    },
    /// A circular arc.
    Arc {
        /// Arc center in sketch space, millimetres.
        center: Point3,
        /// Normal of the sketch plane the arc lies in; the decoder admits a norm
        /// within `1e-9` of one.
        normal: Vector3,
        /// Zero-angle direction for `start_angle`/`end_angle`; the decoder admits a
        /// norm within `1e-9` of one.
        reference_direction: Vector3,
        /// Arc radius in millimetres.
        radius: f64,
        /// Start angle in radians, measured from `reference_direction`.
        start_angle: f64,
        /// End angle in radians, measured from `reference_direction`.
        end_angle: f64,
    },
    /// A NURBS (procedural spline) curve.
    Nurbs {
        /// Record index of the underlying carrier geometry, when the NURBS record
        /// references one; `None` when the control data is self-contained.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        carrier_reference: Option<u64>,
        /// Source per-file dynamic three-digit ASCII class tag naming the NURBS subtype.
        subtype_class_tag: DesignClassTag,
        /// Record index of the NURBS subtype record.
        subtype_record_index: u32,
        /// Polynomial degree of the curve.
        degree: u32,
        /// Source fit tolerance used when the curve was fitted, in millimetres.
        fit_tolerance: f64,
        /// Width in scalars of each control-point record as stored in the source
        /// (control point components plus weight, before decoding into `poles`).
        scalar_width: u32,
        /// Knot vector, non-decreasing, length `poles.point_count() + degree + 1`.
        knots: Vec<f64>,
        /// Polynomial control points or rational point/weight pairs.
        poles: SketchNurbsPoles,
    },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
enum SketchCurveGeometryWire {
    /// A straight line segment.
    Line {
        /// Start point in sketch space, millimetres.
        start: Point3,
        /// End point in sketch space, millimetres.
        end: Point3,
        /// Direction from `start` to `end`; the decoder stores it at unit length.
        direction: Vector3,
        /// Normal of the sketch plane the line lies in; the decoder stores it at
        /// unit length.
        normal: Vector3,
    },
    /// A circular arc.
    Arc {
        /// Arc center in sketch space, millimetres.
        center: Point3,
        /// Normal of the sketch plane the arc lies in; the decoder admits a norm
        /// within `1e-9` of one.
        normal: Vector3,
        /// Zero-angle direction for `start_angle`/`end_angle`; the decoder admits a
        /// norm within `1e-9` of one.
        reference_direction: Vector3,
        /// Arc radius in millimetres.
        radius: f64,
        /// Start angle in radians, measured from `reference_direction`.
        start_angle: f64,
        /// End angle in radians, measured from `reference_direction`.
        end_angle: f64,
    },
    /// A NURBS (procedural spline) curve.
    Nurbs {
        /// Record index of the underlying carrier geometry, when the NURBS record
        /// references one; `None` when the control data is self-contained.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        carrier_reference: Option<u64>,
        /// Source per-file dynamic three-digit ASCII class tag naming the NURBS subtype.
        subtype_class_tag: String,
        /// Record index of the NURBS subtype record.
        subtype_record_index: u32,
        /// Polynomial degree of the curve.
        degree: u32,
        /// Source fit tolerance used when the curve was fitted, in millimetres.
        fit_tolerance: f64,
        /// Width in scalars of each control-point record as stored in the source
        /// (control point components plus weight, before decoding into `control_points`/`weights`).
        scalar_width: u32,
        /// Knot vector, non-decreasing, length `control_points.len() + degree + 1`.
        knots: Vec<f64>,
        /// Per-control-point rational weights, parallel to `control_points`.
        weights: Vec<f64>,
        /// Control points in sketch space, millimetres, parallel to `weights`.
        control_points: Vec<Point3>,
    },
}

impl TryFrom<SketchCurveGeometryWire> for SketchCurveGeometry {
    type Error = String;
    fn try_from(wire: SketchCurveGeometryWire) -> Result<Self, Self::Error> {
        Ok(match wire {
            SketchCurveGeometryWire::Line {
                start,
                end,
                direction,
                normal,
            } => Self::Line {
                start,
                end,
                direction,
                normal,
            },
            SketchCurveGeometryWire::Arc {
                center,
                normal,
                reference_direction,
                radius,
                start_angle,
                end_angle,
            } => Self::Arc {
                center,
                normal,
                reference_direction,
                radius,
                start_angle,
                end_angle,
            },
            SketchCurveGeometryWire::Nurbs {
                carrier_reference,
                subtype_class_tag,
                subtype_record_index,
                degree,
                fit_tolerance,
                scalar_width,
                knots,
                weights,
                control_points,
            } => Self::Nurbs {
                carrier_reference,
                subtype_class_tag: DesignClassTag::try_from(subtype_class_tag)
                    .map_err(|error| format!("subtype_class_tag: {error}"))?,
                subtype_record_index,
                degree,
                fit_tolerance,
                scalar_width,
                knots,
                poles: SketchNurbsPoles::from_wire(control_points, weights)?,
            },
        })
    }
}

impl From<SketchCurveGeometry> for SketchCurveGeometryWire {
    fn from(geometry: SketchCurveGeometry) -> Self {
        match geometry {
            SketchCurveGeometry::Line {
                start,
                end,
                direction,
                normal,
            } => Self::Line {
                start,
                end,
                direction,
                normal,
            },
            SketchCurveGeometry::Arc {
                center,
                normal,
                reference_direction,
                radius,
                start_angle,
                end_angle,
            } => Self::Arc {
                center,
                normal,
                reference_direction,
                radius,
                start_angle,
                end_angle,
            },
            SketchCurveGeometry::Nurbs {
                carrier_reference,
                subtype_class_tag,
                subtype_record_index,
                degree,
                fit_tolerance,
                scalar_width,
                knots,
                poles,
            } => {
                let (control_points, weights) = match poles {
                    SketchNurbsPoles::Polynomial(points) => (points, Vec::new()),
                    SketchNurbsPoles::Rational(poles) => poles
                        .into_iter()
                        .map(|pole| (pole.point, pole.weight))
                        .unzip(),
                };
                Self::Nurbs {
                    carrier_reference,
                    subtype_class_tag: subtype_class_tag.into(),
                    subtype_record_index,
                    degree,
                    fit_tolerance,
                    scalar_width,
                    knots,
                    weights,
                    control_points,
                }
            }
        }
    }
}

/// Control data for a polynomial or rational sketch spline.
#[derive(Debug, Clone)]
pub(crate) enum SketchNurbsPoles {
    Polynomial(Vec<Point3>),
    Rational(Vec<SketchNurbsPole>),
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct SketchNurbsPole {
    pub(crate) point: Point3,
    pub(crate) weight: f64,
}

impl PartialEq for SketchNurbsPoles {
    fn eq(&self, other: &Self) -> bool {
        self.points().eq(other.points()) && self.weights().eq(other.weights())
    }
}

impl SketchNurbsPoles {
    pub(crate) fn from_wire(points: Vec<Point3>, weights: Vec<f64>) -> Result<Self, String> {
        if weights.is_empty() {
            return Ok(Self::Polynomial(points));
        }
        if points.len() != weights.len() {
            return Err("weights must be absent or match every control_points entry".into());
        }
        Ok(Self::Rational(
            points
                .into_iter()
                .zip(weights)
                .map(|(point, weight)| SketchNurbsPole { point, weight })
                .collect(),
        ))
    }

    pub(crate) fn point_count(&self) -> usize {
        match self {
            Self::Polynomial(points) => points.len(),
            Self::Rational(poles) => poles.len(),
        }
    }

    pub(crate) fn points(&self) -> impl DoubleEndedIterator<Item = &Point3> {
        let (points, poles): (&[Point3], &[SketchNurbsPole]) = match self {
            Self::Polynomial(points) => (points, &[]),
            Self::Rational(poles) => (&[], poles),
        };
        points.iter().chain(poles.iter().map(|pole| &pole.point))
    }

    pub(crate) fn weights(&self) -> impl ExactSizeIterator<Item = &f64> {
        let poles: &[SketchNurbsPole] = match self {
            Self::Polynomial(_) => &[],
            Self::Rational(poles) => poles,
        };
        poles.iter().map(|pole| &pole.weight)
    }

    #[cfg(test)]
    pub(crate) fn points_mut(&mut self) -> impl Iterator<Item = &mut Point3> {
        let (points, poles): (&mut [Point3], &mut [SketchNurbsPole]) = match self {
            Self::Polynomial(points) => (points, &mut []),
            Self::Rational(poles) => (&mut [], poles),
        };
        points
            .iter_mut()
            .chain(poles.iter_mut().map(|pole| &mut pole.point))
    }
}
