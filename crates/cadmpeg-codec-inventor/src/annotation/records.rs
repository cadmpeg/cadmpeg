// SPDX-License-Identifier: Apache-2.0
//! Record grammars of part annotations.

use std::collections::HashMap;

use cadmpeg_core::decode::{DecodeContext, View};
use cadmpeg_core::CodecError;

use crate::layout::{
    pm_dc_751ade_label_owner_body_v25 as diameter_v25,
    pm_dc_751ade_label_owner_body_v28 as diameter_v28, pm_dc_annotation_value_block as value_block,
    pm_dc_content_header as content_header_v25, pm_dc_content_header_v28 as content_header_v28,
    pm_dc_frame_row_v25 as row_v25, pm_dc_frame_row_v28 as row_v28,
    pm_dc_hole_note_flags as hole_flags, pm_dc_naming_table_header as table_header,
};
use crate::pmdc::{
    content_header, state_content_header, u32_list_with_context, Cursor, PmDcContentHeader,
    PmDcReference,
};

pub(super) const DIAMETER_OWNER: [u8; 16] = [
    0x75, 0x1a, 0xde, 0x16, 0x74, 0x4f, 0xe9, 0x4f, 0xc4, 0x98, 0xa8, 0xa8, 0xe8, 0x9f, 0x29, 0xdd,
];
pub(super) const LINEAR_OWNER: [u8; 16] = [
    0xe1, 0xc3, 0x66, 0x65, 0xd2, 0x49, 0x68, 0xac, 0x24, 0xce, 0xa9, 0xb3, 0xcc, 0x60, 0x55, 0xcd,
];
pub(super) const ANGULAR_OWNER: [u8; 16] = [
    0x4f, 0x02, 0xd6, 0x9e, 0x38, 0x44, 0x25, 0x08, 0x2e, 0xa7, 0x33, 0xb6, 0xa1, 0x07, 0x0a, 0x20,
];
pub(super) const RADIAL_OWNER: [u8; 16] = [
    0x3c, 0xeb, 0x8c, 0xbe, 0x02, 0x44, 0x69, 0x07, 0xd3, 0xb5, 0x48, 0xa3, 0xbf, 0xc8, 0x7b, 0x32,
];
pub(super) const HOLE_NOTE_OWNER: [u8; 16] = [
    0x0c, 0x00, 0x74, 0x95, 0x8f, 0x4a, 0x8f, 0x36, 0x7f, 0x67, 0xfe, 0x82, 0x13, 0x29, 0xcd, 0xbd,
];
pub(super) const VALUE: [u8; 16] = [
    0x1f, 0x02, 0xe7, 0x8b, 0x32, 0x45, 0x48, 0xa6, 0xbd, 0x6d, 0x4c, 0x90, 0x71, 0x1d, 0xb6, 0xd0,
];
pub(super) const HOLE_VALUE: [u8; 16] = [
    0x1c, 0x0e, 0xcf, 0xdd, 0x56, 0x42, 0x1a, 0x02, 0xb3, 0x14, 0x56, 0x92, 0x15, 0xe8, 0x30, 0xb1,
];
pub(super) const PARAMETER: [u8; 16] = crate::pmdc::inventor_id(0x9087_4d26);
pub(super) const DATUM_OWNER: [u8; 16] = [
    0x47, 0x59, 0x6f, 0x34, 0x43, 0x4b, 0xd3, 0xef, 0x76, 0x8f, 0x59, 0x9a, 0x02, 0xd0, 0xef, 0xab,
];
pub(super) const DATUM_FEATURE: [u8; 16] = [
    0x7b, 0x02, 0x3e, 0x03, 0x30, 0x49, 0xcd, 0x9e, 0xf9, 0x1b, 0x87, 0xa2, 0x59, 0x82, 0x9c, 0x67,
];
pub(super) const FRAME_OWNER: [u8; 16] = [
    0x37, 0xe2, 0xb7, 0xca, 0xf5, 0x4e, 0xea, 0x9d, 0x7f, 0x98, 0x4b, 0xa8, 0x3f, 0x1c, 0x1a, 0xb1,
];
pub(super) const FRAME_ROW: [u8; 16] = [
    0x26, 0x1b, 0x9f, 0xa9, 0xbc, 0x4d, 0xaf, 0xc3, 0x8f, 0x05, 0x70, 0x97, 0x0a, 0x83, 0x13, 0xce,
];
pub(super) const DATUM_FRAME: [u8; 16] = [
    0x11, 0x84, 0x70, 0x5b, 0xe7, 0x40, 0xb0, 0xfa, 0x1b, 0x2f, 0xce, 0xb7, 0x09, 0x8a, 0xf8, 0x7d,
];
pub(super) const TOLERANCE_FEATURE: [u8; 16] = [
    0xcf, 0x2b, 0x17, 0xde, 0x9c, 0x46, 0xa8, 0x94, 0xe0, 0x64, 0x59, 0xa7, 0x05, 0x46, 0xaa, 0x94,
];
pub(super) const ATTACHMENT: [u8; 16] = [
    0x1f, 0x96, 0xe5, 0xbe, 0xde, 0x4c, 0x9c, 0xe1, 0xcf, 0xa3, 0x5a, 0x9b, 0xdf, 0xda, 0x09, 0x47,
];
pub(super) const DATUM_DEFINITION: [u8; 16] = [
    0x7a, 0x8f, 0x42, 0xa5, 0xdf, 0x4c, 0xac, 0xbd, 0xa0, 0xfb, 0xb5, 0x92, 0x2a, 0x57, 0xce, 0xc5,
];
pub(super) const FEATURE_FACE_SELECTOR: [u8; 16] = [
    0x2c, 0x81, 0xaa, 0x03, 0xd2, 0x11, 0xff, 0x26, 0x00, 0x08, 0x1f, 0xbb, 0x32, 0xa3, 0xdc, 0x09,
];
pub(super) const INTENT: [u8; 16] = [
    0xb4, 0x31, 0x79, 0x61, 0x69, 0x44, 0x93, 0x84, 0xe3, 0xad, 0x40, 0x9b, 0xde, 0xbf, 0x7e, 0x05,
];
pub(super) const STYLE: [u8; 16] = [
    0xae, 0x38, 0xb9, 0x99, 0xd2, 0x11, 0xc3, 0x5b, 0x60, 0x00, 0xb3, 0x85, 0x6c, 0x23, 0x87, 0xb0,
];
pub(super) const DIAMETER_LAYOUT: [u8; 16] = [
    0x30, 0x3e, 0x4a, 0x7f, 0xd2, 0x11, 0xd6, 0x5e, 0x60, 0x00, 0xb3, 0x85, 0x6c, 0x23, 0x87, 0xb0,
];
pub(super) const SIZE_RELATION: [u8; 16] = [
    0x3c, 0x31, 0x13, 0x3b, 0x47, 0x4a, 0x98, 0x6b, 0xd1, 0xea, 0x9b, 0xa7, 0x30, 0xc7, 0xe5, 0x7c,
];
pub(super) const FACE_RELATION: [u8; 16] = [
    0xc4, 0xf8, 0x1a, 0x38, 0x29, 0x4b, 0xd8, 0xee, 0x18, 0xe9, 0x0e, 0xb9, 0x1d, 0xf9, 0xb3, 0x76,
];
pub(super) const PAIR_RELATION: [u8; 16] = [
    0x8e, 0x92, 0xd9, 0x6f, 0x62, 0x48, 0x9d, 0xb9, 0xa2, 0xb2, 0x4d, 0xa6, 0x6e, 0x31, 0x22, 0x98,
];
pub(super) const EDGE_ITEM: [u8; 16] = [
    0x6a, 0x71, 0x43, 0x9e, 0xd2, 0x11, 0xa5, 0x0f, 0x60, 0x00, 0x84, 0xb7, 0xb0, 0x35, 0xc3, 0xb0,
];
pub(super) const WORK_AXIS: [u8; 16] = crate::feature::WORK_AXIS_TYPE;
pub(super) const GEOMETRY_LINK: [u8; 16] = [
    0x97, 0x11, 0x32, 0x7c, 0x5a, 0x4b, 0x08, 0xdd, 0x9a, 0x44, 0x21, 0xb6, 0x52, 0x00, 0xc2, 0x5e,
];
pub(super) const GEOMETRY: [u8; 16] = [
    0xa4, 0x26, 0x0c, 0xf1, 0x3b, 0x48, 0x84, 0x0a, 0xa3, 0xaf, 0x85, 0xaa, 0xad, 0xaf, 0xf6, 0x5d,
];
pub(super) const PLANE: [u8; 16] = [
    0x42, 0xdf, 0x52, 0xce, 0xd0, 0x11, 0xd0, 0xd2, 0x00, 0x08, 0xcc, 0xbc, 0x06, 0x63, 0xdc, 0x09,
];
pub(super) const FACE_SELECTOR: [u8; 16] = [
    0x68, 0x05, 0xba, 0x23, 0x32, 0x46, 0x11, 0xfa, 0xf6, 0x72, 0x5e, 0xbb, 0x87, 0xbd, 0xfd, 0x05,
];
pub(super) const EDGE_SELECTOR: [u8; 16] = [
    0x02, 0x2d, 0xa9, 0x6c, 0xd2, 0x11, 0x7c, 0x10, 0x60, 0x00, 0x84, 0xb7, 0xb0, 0x35, 0xc3, 0xb0,
];
pub(super) const NAMING_TABLE: [u8; 16] = [
    0x42, 0x20, 0xe9, 0xcc, 0xd1, 0x11, 0xc5, 0x71, 0x00, 0x08, 0xa7, 0xba, 0x32, 0xa3, 0xdc, 0x09,
];
pub(super) const BODY: [u8; 16] = crate::pmdc::inventor_id(0x9087_4d47);

/// Selector `state` of the edge selector.
const EDGE_SELECTOR_STATE: i32 = -1;
/// Selector `enabled` and `form` bytes.
const SELECTOR_ENABLED: u8 = 1;
const SELECTOR_FORM: u8 = 5;
/// Face-selector states of an annotation or tolerance-feature face.
pub(super) const ANNOTATION_STATES: [i32; 2] = [1, 2];

/// One framed record.
pub(super) struct Record<'a> {
    pub(super) type_id: [u8; 16],
    pub(super) payload: View<'a>,
}

/// The records of one `PmDc` segment by ordinal.
pub(super) struct Segment<'a> {
    pub(super) token: String,
    pub(super) version: u8,
    pub(super) records: HashMap<u32, Record<'a>>,
}

impl<'a> Segment<'a> {
    pub(super) fn record(&self, ordinal: u32, type_id: [u8; 16]) -> Option<View<'a>> {
        self.records
            .get(&ordinal)
            .filter(|record| record.type_id == type_id)
            .map(|record| record.payload)
    }

    pub(super) fn target_type(&self, reference: PmDcReference) -> Option<[u8; 16]> {
        self.records
            .get(&reference.record_ordinal()?)
            .map(|record| record.type_id)
    }

    /// The byte length of a state content header in this segment.
    pub(super) const fn state_header_len(&self) -> usize {
        if self.version >= 28 {
            content_header_v28::LEN
        } else {
            content_header_v25::LEN
        }
    }

    /// The content-header source index of a record.
    pub(super) fn source_index(&self, ordinal: u32) -> Option<u32> {
        let payload = self.records.get(&ordinal)?.payload;
        state_content_header(&mut Cursor::new(payload), self.version)
            .ok()
            .map(|header| header.source_index)
    }
}

/// The dimension kinds of the owner types.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Measure {
    Linear,
    Angular,
    Diameter,
    Radial,
    HoleNote,
}

impl Measure {
    pub(super) fn of(type_id: [u8; 16]) -> Option<Self> {
        match type_id {
            LINEAR_OWNER => Some(Self::Linear),
            ANGULAR_OWNER => Some(Self::Angular),
            DIAMETER_OWNER => Some(Self::Diameter),
            RADIAL_OWNER => Some(Self::Radial),
            HOLE_NOTE_OWNER => Some(Self::HoleNote),
            _ => None,
        }
    }

    pub(super) const fn value_type(self) -> [u8; 16] {
        match self {
            Self::HoleNote => HOLE_VALUE,
            Self::Linear | Self::Angular | Self::Diameter | Self::Radial => VALUE,
        }
    }

    /// Value-record measure code of the owner kind.
    pub(super) const fn code(self) -> u32 {
        match self {
            Self::Linear | Self::Angular => 0,
            Self::Diameter | Self::HoleNote => 1,
            Self::Radial => 2,
        }
    }
}

/// The content header of an annotation record and its `state = -1`.
pub(super) fn header(
    cursor: &mut Cursor<'_>,
    version: u8,
) -> Result<PmDcContentHeader, CodecError> {
    let header = state_content_header(cursor, version)?;
    match cursor.i32("annotation state")? {
        -1 => Ok(header),
        state => Err(CodecError::malformed(format_args!(
            "Inventor annotation state is {state}"
        ))),
    }
}

pub(super) fn ordinal(reference: PmDcReference, field: &str) -> Result<u32, CodecError> {
    reference
        .record_ordinal()
        .ok_or_else(|| CodecError::malformed(format_args!("Inventor annotation {field} is null")))
}

fn expect_type(
    segment: &Segment<'_>,
    reference: PmDcReference,
    types: &[[u8; 16]],
    field: &str,
) -> Result<u32, CodecError> {
    let ordinal = ordinal(reference, field)?;
    if reference.qualified()
        && segment
            .target_type(reference)
            .is_some_and(|type_id| types.contains(&type_id))
    {
        Ok(ordinal)
    } else {
        Err(CodecError::malformed(format_args!(
            "Inventor annotation {field} names another record type"
        )))
    }
}

fn expect_optional(
    segment: &Segment<'_>,
    reference: PmDcReference,
    types: &[[u8; 16]],
    field: &str,
) -> Result<Option<u32>, CodecError> {
    if reference.record_ordinal().is_none() {
        Ok(None)
    } else {
        expect_type(segment, reference, types, field).map(Some)
    }
}

fn empty_list(cursor: &mut Cursor<'_>, field: &str) -> Result<(), CodecError> {
    if crate::pmdc::reference_list(cursor, 2, field)?
        .references()
        .is_empty()
    {
        Ok(())
    } else {
        Err(CodecError::malformed(format_args!(
            "Inventor annotation {field} is not empty"
        )))
    }
}

/// The owner fields that the projection reads.
#[derive(Debug, PartialEq, Eq)]
pub(super) struct Owner {
    pub(super) value: u32,
    pub(super) intents: Vec<u32>,
}

pub(super) fn owner(
    segment: &Segment<'_>,
    payload: View<'_>,
    measure: Measure,
) -> Result<Owner, CodecError> {
    if measure == Measure::Diameter {
        return diameter_owner(segment, payload);
    }
    let mut cursor = Cursor::new(payload);
    header(&mut cursor, segment.version)?;
    cursor.u32("annotation first reference")?;
    let plane = cursor.reference("annotation work plane")?;
    expect_type(segment, plane, &[PLANE], "work plane")?;
    cursor.u32("annotation value")?;
    if segment.version >= 28 {
        empty_list(&mut cursor, "annotation list")?;
    }
    if measure == Measure::HoleNote {
        cursor.reference("hole-note definition")?;
        cursor.reference("hole-note feature")?;
    }
    cursor.reference("annotation layout")?;
    let value = cursor.reference("annotation value record")?;
    let value = expect_type(segment, value, &[measure.value_type()], "value record")?;
    let mut intents = Vec::new();
    let mut styles = 0usize;
    while cursor.remaining() >= 4 {
        let reference = cursor.reference("annotation operand")?;
        match segment.target_type(reference) {
            Some(INTENT) if styles == 0 && reference.qualified() => {
                intents.push(ordinal(reference, "intent")?);
            }
            Some(STYLE) if reference.qualified() => styles += 1,
            _ if measure == Measure::HoleNote && styles > 0 => break,
            _ => {
                return Err(CodecError::malformed(
                    "Inventor annotation operand is out of the intent and style order",
                ))
            }
        }
    }
    if measure != Measure::HoleNote && cursor.remaining() != 0 {
        return Err(CodecError::malformed(
            "Inventor annotation owner has trailing bytes",
        ));
    }
    Ok(Owner { value, intents })
}

/// A diameter owner, read through the section-13 table record of its version.
fn diameter_owner(segment: &Segment<'_>, payload: View<'_>) -> Result<Owner, CodecError> {
    let bytes = payload.window();
    let body = bytes.get(segment.state_header_len()..).unwrap_or_default();
    let (len, state, work_plane, references) = if segment.version >= 28 {
        if View::u32_le_at(body, diameter_v28::EMPTY_LIST_MARKER)
            != Some(diameter_v28::EMPTY_LIST_MARKER_VALUE)
            || View::u32_le_at(body, diameter_v28::EMPTY_LIST_COUNT)
                != Some(diameter_v28::EMPTY_LIST_COUNT_VALUE)
        {
            return Err(CodecError::malformed(
                "Inventor diameter owner list is not empty",
            ));
        }
        (
            diameter_v28::LEN,
            View::i32_le_at(body, diameter_v28::STATE),
            diameter_v28::WORK_PLANE,
            diameter_v28::REFERENCES,
        )
    } else {
        (
            diameter_v25::LEN,
            View::i32_le_at(body, diameter_v25::STATE),
            diameter_v25::WORK_PLANE,
            diameter_v25::REFERENCES,
        )
    };
    if body.len() != len || state != Some(diameter_v25::STATE_VALUE) {
        return Err(CodecError::malformed(
            "Inventor diameter owner is outside the section-13 form",
        ));
    }
    let reference = |offset: usize| {
        View::u32_le_at(body, offset)
            .map(PmDcReference::from_packed)
            .ok_or_else(|| CodecError::malformed("truncated Inventor diameter owner"))
    };
    let first = if segment.version >= 28 {
        diameter_v28::FIRST_REFERENCE
    } else {
        diameter_v25::FIRST_REFERENCE
    };
    expect_optional(segment, reference(first)?, &[ATTACHMENT], "first reference")?;
    expect_type(segment, reference(work_plane)?, &[PLANE], "work plane")?;
    let expected = [DIAMETER_LAYOUT, VALUE, INTENT, STYLE, STYLE];
    let mut ordinals = [0; 5];
    for ((slot, type_id), ordinal) in expected.iter().enumerate().zip(&mut ordinals) {
        *ordinal = expect_type(
            segment,
            reference(references + 4 * slot)?,
            &[*type_id],
            "diameter owner reference",
        )?;
    }
    Ok(Owner {
        value: ordinals[1],
        intents: vec![ordinals[2]],
    })
}

/// The value-record fields that the projection reads.
#[derive(Debug, PartialEq)]
pub(super) struct Value {
    pub(super) measure: u32,
    pub(super) tolerance: u32,
    pub(super) display: u32,
    pub(super) upper: f64,
    pub(super) lower: f64,
    pub(super) hole_fit: String,
    pub(super) shaft_fit: String,
    pub(super) nominal_parameter: u32,
}

/// The hole tolerance form code that maps to an equal-bilateral value-record
/// tolerance, and the one that maps to deviations.
const HOLE_SYMMETRIC: u32 = 3;
const HOLE_DEVIATION: u32 = 4;
/// Value-record tolerance codes of an equal-bilateral tolerance and of
/// deviations.
pub(super) const TOLERANCE_SYMMETRIC: u32 = 6;
pub(super) const TOLERANCE_DEVIATION: u32 = 7;
/// A tolerance code outside the established codes.
pub(super) const TOLERANCE_UNKNOWN: u32 = u32::MAX;

pub(super) fn value(
    ctx: &DecodeContext<'_>,
    segment: &Segment<'_>,
    payload: View<'_>,
    kind: Measure,
) -> Result<Value, CodecError> {
    let mut cursor = Cursor::new(payload);
    content_header(&mut cursor, segment.version)?;
    cursor.u32_array::<3>("value prefix")?;
    let measure = cursor.u32("value measure")?;
    let tolerance = cursor.u32("value tolerance form")?;
    cursor.u32_array::<6>("value controls")?;
    let display = cursor.u32("value display")?;
    cursor.u32_array::<2>("value display controls")?;
    let upper = cursor.f64("value upper tolerance")?.get();
    let lower = cursor.f64("value lower tolerance")?.get();
    cursor.u32_array::<2>("value tolerance controls")?;
    let hole_fit = cursor.utf16(ctx, "value hole fit")?;
    let shaft_fit = cursor.utf16(ctx, "value shaft fit")?;
    cursor.take_array::<{ value_block::LEN }>("value block")?;
    let nominal = cursor.reference("value nominal parameter")?;
    let nominal_parameter = expect_type(segment, nominal, &[PARAMETER], "nominal parameter")?;
    let reference = cursor.reference("value reference parameter")?;
    expect_type(segment, reference, &[PARAMETER], "reference parameter")?;
    let (tolerance, upper, lower) = if kind == Measure::HoleNote {
        if tolerance != 0 {
            return Err(CodecError::malformed(
                "Inventor hole-note value stores a dimension tolerance form",
            ));
        }
        hole_tolerance(ctx, segment, cursor.view().unread())?
    } else {
        (tolerance, upper, lower)
    };
    Ok(Value {
        measure,
        tolerance,
        display,
        upper,
        lower,
        hole_fit,
        shaft_fit,
        nominal_parameter,
    })
}

/// The diameter tolerance of a plain hole note, as a value-record tolerance
/// code with its deviations.
pub(super) fn hole_tolerance(
    ctx: &DecodeContext<'_>,
    segment: &Segment<'_>,
    tail: &[u8],
) -> Result<(u32, f64, f64), CodecError> {
    let parameter_at = |offset: usize| {
        View::u32_le_at(tail, offset)
            .map(PmDcReference::from_packed)
            .is_some_and(|reference| {
                reference.qualified() && segment.target_type(reference) == Some(PARAMETER)
            })
    };
    let start = (0..tail.len())
        .find(|offset| (0..4).all(|index| parameter_at(offset + 4 * index)))
        .ok_or_else(|| CodecError::malformed("Inventor hole note has no hole parameters"))?;
    let mut hole = Cursor::new(View::over_retained(tail.get(start..).unwrap_or_default()));
    for _ in 0..4 {
        hole.reference("hole-note parameter")?;
    }
    let flags = [
        hole.u32("hole-note flag word a")?,
        hole.u32("hole-note flag word b")?,
        hole.u32("hole-note flag word c")?,
        hole.u32("hole-note flag word d")?,
    ];
    if flags
        != [
            hole_flags::WORD_A_VALUE,
            hole_flags::WORD_B_VALUE,
            hole_flags::WORD_C_VALUE,
            hole_flags::WORD_D_VALUE,
        ]
    {
        return Err(CodecError::malformed(
            "Inventor hole note is outside the plain form",
        ));
    }
    let text = hole.utf16(ctx, "hole-note text")?;
    if !text.contains("<HDIA>") {
        return Err(CodecError::malformed(
            "Inventor hole note shows no hole diameter",
        ));
    }
    let form = hole.u32("hole-note tolerance form")?;
    hole.u32_array::<3>("hole-note tolerance controls")?;
    let upper = hole.f64("hole-note upper tolerance")?.get();
    let lower = hole.f64("hole-note lower tolerance")?.get();
    let form = match form {
        0 => 0,
        HOLE_SYMMETRIC => TOLERANCE_SYMMETRIC,
        HOLE_DEVIATION => TOLERANCE_DEVIATION,
        _ => TOLERANCE_UNKNOWN,
    };
    Ok((form, upper, lower))
}

/// The intent of a frame or datum-identifier owner.
pub(super) fn tolerance_owner(
    segment: &Segment<'_>,
    payload: View<'_>,
) -> Result<Option<u32>, CodecError> {
    let mut cursor = Cursor::new(payload);
    header(&mut cursor, segment.version)?;
    cursor.u32("tolerance-owner first reference")?;
    let plane = cursor.reference("tolerance-owner work plane")?;
    expect_type(segment, plane, &[PLANE], "work plane")?;
    cursor.u32("tolerance-owner value")?;
    let intent = cursor.reference("tolerance-owner intent")?;
    expect_optional(segment, intent, &[INTENT], "intent")
}

/// The frame row that the last u32 of a frame owner names as an
/// unqualified, non-null reference.
pub(super) fn trailing_reference(payload: View<'_>) -> Option<u32> {
    let bytes = payload.window();
    let reference =
        PmDcReference::from_packed(View::u32_le_at(bytes, bytes.len().checked_sub(4)?)?);
    if reference.qualified() {
        return None;
    }
    reference.record_ordinal()
}

/// The datum identifier owner and label of a datum feature.
pub(super) fn datum_feature(
    ctx: &DecodeContext<'_>,
    segment: &Segment<'_>,
    payload: View<'_>,
) -> Result<(u32, String), CodecError> {
    let mut cursor = Cursor::new(payload);
    content_header(&mut cursor, segment.version)?;
    let owners = crate::pmdc::reference_list(&mut cursor, 2, "datum-feature owner list")?;
    let [owner] = owners.references() else {
        return Err(CodecError::malformed(
            "Inventor datum feature names other than one owner",
        ));
    };
    let owner = expect_type(segment, *owner, &[DATUM_OWNER], "datum-feature owner")?;
    if segment.version < 28 {
        crate::pmdc::reference_list(&mut cursor, 2, "datum-feature member list")?;
    }
    cursor.reference("datum-feature style")?;
    cursor.reference("datum-feature definition")?;
    let label = cursor.utf16(ctx, "datum label")?;
    if segment.version >= 28 {
        crate::pmdc::reference_list(&mut cursor, 2, "datum-feature member list")?;
    }
    cursor.finish("datum feature")?;
    Ok((owner, label))
}

/// The fields of a frame row that the projection reads.
#[derive(Debug, PartialEq, Eq)]
pub(super) struct FrameRow {
    pub(super) characteristic: u32,
    pub(super) magnitude: u32,
    pub(super) secondary: [u32; 2],
    pub(super) datums: Vec<u32>,
    pub(super) plain: bool,
    /// A version-28 datum reference frame.
    pub(super) frame_reference: Option<u32>,
}

pub(super) fn frame_row(segment: &Segment<'_>, payload: View<'_>) -> Result<FrameRow, CodecError> {
    let bytes = payload.window();
    let expected_len = if segment.version >= 28 {
        row_v28::LEN
    } else {
        row_v25::LEN
    };
    if bytes.len() != expected_len {
        return Err(CodecError::malformed(
            "Inventor frame row length differs from its version",
        ));
    }
    let word = |offset: usize| {
        View::u32_le_at(bytes, offset).ok_or_else(|| CodecError::malformed("truncated frame row"))
    };
    let reference = |offset: usize| word(offset).map(PmDcReference::from_packed);
    let magnitude = expect_type(
        segment,
        reference(row_v25::MAGNITUDE)?,
        &[PARAMETER],
        "frame magnitude",
    )?;
    let secondary = [
        expect_type(
            segment,
            reference(row_v25::SECOND_VALUE)?,
            &[PARAMETER],
            "frame second value",
        )?,
        expect_type(
            segment,
            reference(row_v25::THIRD_VALUE)?,
            &[PARAMETER],
            "frame third value",
        )?,
    ];
    let mut datums = Vec::new();
    for slot in 0..3 {
        if let Some(datum) = expect_optional(
            segment,
            reference(row_v25::DATUMS + 4 * slot)?,
            &[DATUM_FEATURE],
            "frame datum",
        )? {
            datums.push(datum);
        }
    }
    let zone_modifiers = [
        word(row_v25::ZONE_MODIFIERS)?,
        word(row_v25::ZONE_MODIFIERS + 4)?,
        word(row_v25::ZONE_MODIFIERS + 8)?,
    ];
    let datum_modifiers = bytes
        .get(row_v25::DATUM_MODIFIERS..row_v25::LEN)
        .unwrap_or_default();
    let frame_reference = if segment.version >= 28 {
        expect_optional(
            segment,
            reference(row_v28::FRAME_REFERENCE)?,
            &[DATUM_FRAME],
            "frame datum reference frame",
        )?
    } else {
        None
    };
    Ok(FrameRow {
        characteristic: word(row_v25::CHARACTERISTIC)?,
        magnitude,
        secondary,
        datums,
        plain: zone_modifiers == [0; 3] && datum_modifiers.iter().all(|byte| *byte == 0),
        frame_reference,
    })
}

/// The member tolerance features and the face collection of a tolerance
/// feature.
#[derive(Debug, PartialEq, Eq)]
pub(super) struct ToleranceFeature {
    pub(super) members: Vec<u32>,
    pub(super) attachments: usize,
    pub(super) face_collection: Option<u32>,
}

/// Header flags of a tolerance feature.
const TOLERANCE_FEATURE_FLAGS: u32 = 0x200;

pub(super) fn tolerance_feature(
    ctx: &DecodeContext<'_>,
    segment: &Segment<'_>,
    payload: View<'_>,
) -> Result<ToleranceFeature, CodecError> {
    let mut cursor = Cursor::new(payload);
    let content = header(&mut cursor, segment.version)?;
    if content.flags != TOLERANCE_FEATURE_FLAGS {
        return Err(CodecError::malformed(
            "Inventor tolerance-feature flags differ",
        ));
    }
    expect_type(
        segment,
        content.next,
        &[crate::feature::FEATURE_LABEL_TYPE],
        "tolerance-feature label",
    )?;
    expect_type(
        segment,
        content.context,
        &[crate::design::scope::PARAMETER_TABLE_TYPE],
        "tolerance-feature parameter table",
    )?;
    let list = crate::pmdc::reference_list(&mut cursor, 2, "tolerance-feature members")?;
    let mut members = Vec::new();
    let mut attachments = 0usize;
    for reference in list.references() {
        let member = expect_type(
            segment,
            *reference,
            &[TOLERANCE_FEATURE, ATTACHMENT],
            "tolerance-feature member",
        )?;
        if segment.target_type(*reference) == Some(TOLERANCE_FEATURE) {
            members.push(member);
        } else {
            attachments += 1;
        }
    }
    cursor.u32_array::<2>("tolerance-feature values")?;
    cursor.utf16(ctx, "tolerance-feature identifier")?;
    let collection = cursor.reference("tolerance-feature face collection")?;
    let face_collection = expect_optional(
        segment,
        collection,
        &[crate::feature::FACE_COLLECTION_TYPE],
        "face collection",
    )?;
    let definition = cursor.reference("tolerance-feature definition")?;
    expect_optional(segment, definition, &[DATUM_DEFINITION], "datum definition")?;
    cursor.u32_array::<2>("tolerance-feature controls")?;
    cursor.finish("tolerance feature")?;
    Ok(ToleranceFeature {
        members,
        attachments,
        face_collection,
    })
}

/// The selection items that a relation joins to an intent.
pub(super) fn relation(
    segment: &Segment<'_>,
    payload: View<'_>,
) -> Result<(u32, Vec<u32>), CodecError> {
    let mut cursor = Cursor::new(payload);
    header(&mut cursor, segment.version)?;
    if cursor.u32("relation link")? != 0 {
        return Err(CodecError::malformed("Inventor relation link is not null"));
    }
    let intent = cursor.reference("relation intent")?;
    let intent = expect_type(segment, intent, &[INTENT], "relation intent")?;
    let mut items = Vec::new();
    while cursor.remaining() >= 4 {
        let reference = cursor.reference("relation operand")?;
        if reference.qualified()
            && matches!(segment.target_type(reference), Some(EDGE_ITEM | WORK_AXIS))
        {
            items.push(ordinal(reference, "relation item")?);
        }
    }
    cursor.finish("relation")?;
    Ok((intent, items))
}

/// The byte offset of the intent reference of a relation.
pub(super) const fn relation_intent_offset(segment: &Segment<'_>) -> usize {
    segment.state_header_len() + 8
}

/// The selection item and stored geometry that a geometry link joins.
pub(super) fn geometry_link(
    segment: &Segment<'_>,
    payload: View<'_>,
) -> Result<(u32, u32), CodecError> {
    let mut cursor = Cursor::new(payload);
    header(&mut cursor, segment.version)?;
    let geometry = cursor.reference("geometry-link geometry")?;
    let plane = cursor.reference("geometry-link plane")?;
    let item = cursor.reference("geometry-link item")?;
    cursor.finish("geometry link")?;
    let geometry = expect_type(segment, geometry, &[GEOMETRY], "geometry")?;
    expect_type(segment, plane, &[PLANE], "geometry-link plane")?;
    let item = expect_type(segment, item, &[EDGE_ITEM, WORK_AXIS], "geometry-link item")?;
    Ok((item, geometry))
}

/// A naming selector: its source index, key, the naming table that its
/// context names, and the item that it names.
#[derive(Debug, PartialEq, Eq)]
pub(super) struct Selector {
    pub(super) face: bool,
    pub(super) source: u32,
    pub(super) table: u32,
    pub(super) key: u32,
    pub(super) item: u32,
}

/// A face selector with `state` 1 or 2. `item_types` are the target types of
/// its face-item reference.
pub(super) fn face_selector(
    ctx: &DecodeContext<'_>,
    segment: &Segment<'_>,
    payload: View<'_>,
    item_types: &[[u8; 16]],
) -> Result<Selector, CodecError> {
    let parsed = crate::feature::face_selection::parse_with_states(
        ctx,
        payload,
        segment.version,
        &ANNOTATION_STATES,
    )?
    .ok_or_else(|| CodecError::malformed("Inventor face selector is outside its grammar"))?;
    let body = expect_type(segment, parsed.body(), &[BODY], "selector body")?;
    if segment.source_index(body) != Some(parsed.topology()) {
        return Err(CodecError::malformed(
            "Inventor face selector topology differs from its body source index",
        ));
    }
    Ok(Selector {
        face: true,
        source: parsed.source_index(),
        table: expect_type(
            segment,
            parsed.context(),
            &[NAMING_TABLE],
            "selector context",
        )?,
        key: parsed.key(),
        item: expect_type(segment, parsed.item(), item_types, "selector item")?,
    })
}

pub(super) fn edge_selector(
    ctx: &DecodeContext<'_>,
    segment: &Segment<'_>,
    payload: View<'_>,
) -> Result<Selector, CodecError> {
    let mut cursor = Cursor::new(payload);
    let header = state_content_header(&mut cursor, segment.version)?;
    let body = if cursor.i32("selector state")? == EDGE_SELECTOR_STATE
        && cursor.u8("selector enabled")? == SELECTOR_ENABLED
        && cursor.u8("selector form")? == SELECTOR_FORM
    {
        cursor.reference("selector body")?
    } else {
        return Err(CodecError::malformed(
            "Inventor edge selector prefix differs",
        ));
    };
    let body = expect_type(segment, body, &[BODY], "selector body")?;
    let topology = cursor.u32("selector topology")?;
    if cursor.u32("selector qualifier")? != 0
        || cursor.u32("selector naming form")? != topology
        || segment.source_index(body) != Some(topology)
    {
        return Err(CodecError::malformed(
            "Inventor edge selector naming words differ",
        ));
    }
    u32_list_with_context(ctx, &mut cursor, 8, "selector generators")?;
    u32_list_with_context(ctx, &mut cursor, 8, "selector secondary names")?;
    if cursor.u8("selector separator")? != 0 {
        return Err(CodecError::malformed(
            "Inventor selector separator is not 0",
        ));
    }
    let item = cursor.reference("selector item")?;
    let table = cursor.reference("selector context")?;
    let key = cursor.u32("selector key")?;
    if cursor.u8("selector flag")? > 1 {
        return Err(CodecError::malformed(
            "Inventor edge selector flag exceeds 1",
        ));
    }
    cursor.finish("edge selector")?;
    Ok(Selector {
        face: false,
        source: header.source_index,
        table: expect_type(segment, table, &[NAMING_TABLE], "selector context")?,
        key,
        item: expect_type(segment, item, &[EDGE_ITEM, WORK_AXIS], "selector item")?,
    })
}

/// The entries of a naming table: name identifier to record ordinal.
pub(super) fn naming_table(payload: View<'_>) -> Result<HashMap<u32, u32>, CodecError> {
    let bytes = payload.window();
    let count = View::u32_le_at(bytes, table_header::COUNT)
        .ok_or_else(|| CodecError::malformed("truncated Inventor naming table"))?;
    let mut cursor = Cursor::new(View::over_retained(
        bytes.get(table_header::LEN..).unwrap_or_default(),
    ));
    let mut entries = HashMap::new();
    for _ in 0..count {
        let id = cursor.u32("naming-table identifier")?;
        let record = cursor.reference("naming-table record")?;
        if entries
            .insert(id, ordinal(record, "naming-table record")?)
            .is_some()
        {
            return Err(CodecError::malformed(
                "Inventor naming table repeats an identifier",
            ));
        }
    }
    cursor.finish("naming table")?;
    Ok(entries)
}
