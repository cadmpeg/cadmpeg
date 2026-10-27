// SPDX-License-Identifier: Apache-2.0
//! Sheet-metal attachment, relief, and bend property records.

use cadmpeg_core::decode::{DecodeContext, View};
use cadmpeg_core::CodecError;
use cadmpeg_ir::native::bytes::NativeBytes;
use serde::{Deserialize, Serialize};

use super::{
    property, PmDcFeatureProperty, PmDcFeaturePropertyKind, PmDcFeaturePropertyPayload,
    PmDcFeatureReferenceFamily, PropertyParser,
};
use crate::pmdc::{
    content_header, reference_list, state_content_header, type_id_string, Cursor, PmDcReference,
};

pub(crate) mod corner;
pub(crate) mod flange;
pub(super) mod inputs;
pub(crate) mod operands;
pub(crate) mod projection;
pub(crate) mod style;

const ATTACHMENT_TYPE: [u8; 16] = [
    0x92, 0x59, 0x6b, 0xa9, 0xd3, 0x11, 0xef, 0xa8, 0x60, 0x00, 0xb3, 0xa9, 0x9d, 0xcc, 0xef, 0xb0,
];
const CORNER_TYPE: [u8; 16] = [
    0x83, 0xce, 0x77, 0x86, 0xa9, 0x46, 0xcd, 0xe1, 0x1e, 0xbd, 0x99, 0xb4, 0xfe, 0x02, 0xaf, 0xb2,
];
const RELIEF_TYPE: [u8; 16] = [
    0x64, 0x88, 0x05, 0x96, 0xd3, 0x11, 0x9a, 0x76, 0x60, 0x00, 0x81, 0xa9, 0x9d, 0xcc, 0xef, 0xb0,
];
const TRANSITION_TYPE: [u8; 16] = [
    0xe9, 0x34, 0x50, 0x26, 0xd5, 0x11, 0x4c, 0x08, 0x10, 0x00, 0xe5, 0x96, 0xc2, 0x8b, 0x7a, 0xb5,
];
const ATTACHMENT_SELECTIONS_TYPE: [u8; 16] = [
    0xce, 0x2d, 0x9b, 0x29, 0xad, 0x43, 0xd8, 0x80, 0x2f, 0x13, 0x2c, 0x9a, 0x40, 0xc9, 0xba, 0x90,
];
const EDGE_SELECTION_TYPE: [u8; 16] = [
    0x79, 0x9c, 0x7c, 0xfa, 0x5a, 0x4e, 0xf5, 0xb2, 0x59, 0x7b, 0x64, 0x94, 0xfc, 0x79, 0x8d, 0xb5,
];
const PLATE_CONTROL_TYPE: [u8; 16] = [
    0x05, 0x17, 0x72, 0xe5, 0xa7, 0x4f, 0x64, 0x1a, 0x01, 0x38, 0x6e, 0x8f, 0x3c, 0xc0, 0x22, 0x18,
];
const BEND_CONTROL_TYPE: [u8; 16] = [
    0x34, 0x41, 0xb6, 0x90, 0xd5, 0x11, 0xd8, 0x02, 0x60, 0x00, 0xa2, 0x8d, 0x4a, 0xa6, 0xfb, 0xb0,
];
const SIDE_TYPE: [u8; 16] = [
    0x72, 0x67, 0xcc, 0x3b, 0xd3, 0x11, 0x04, 0xa9, 0x60, 0x00, 0xb3, 0xa9, 0x9d, 0xcc, 0xef, 0xb0,
];
const RELIEF_FORM_TYPE: [u8; 16] = [
    0x72, 0x0c, 0x3e, 0x2b, 0xd3, 0x11, 0x97, 0x60, 0x60, 0x00, 0x7a, 0xa9, 0x9d, 0xcc, 0xef, 0xb0,
];
const CORNER_FORM_TYPE: [u8; 16] = [
    0xe0, 0xa8, 0xd8, 0xaf, 0xd3, 0x11, 0x7d, 0x5a, 0x00, 0x08, 0x92, 0x8f, 0x8b, 0x1f, 0x0f, 0x69,
];
const TRANSITION_FORM_TYPE: [u8; 16] = [
    0x07, 0xa6, 0x61, 0x30, 0xd4, 0x11, 0x88, 0x95, 0x60, 0x00, 0x53, 0xaa, 0x9d, 0xcc, 0xef, 0xb0,
];

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "record", rename_all = "snake_case")]
pub(crate) enum SheetMetalProperty {
    AttachmentState {
        state: i32,
        thickness: PmDcReference,
        radius: PmDcReference,
        attachment: PmDcReference,
        relief_width: PmDcReference,
        minimum_remnant: PmDcReference,
        options: [PmDcReference; 2],
    },
    Attachment {
        control: PmDcReference,
        thickness: PmDcReference,
        first_mode: PmDcReference,
        directions: [PmDcReference; 2],
        radius: PmDcReference,
        second_mode: PmDcReference,
        relief_width: PmDcReference,
        minimum_remnant: PmDcReference,
        first_option: PmDcReference,
        states: NativeBytes<[u8; 3]>,
        second_option: PmDcReference,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        extension: Option<PmDcReference>,
    },
    Corner {
        control: PmDcReference,
        gap: PmDcReference,
        size: PmDcReference,
        relief_form: PmDcReference,
        corner_form: PmDcReference,
        secondary_form: PmDcReference,
        jacobi_radius: PmDcReference,
        value: u32,
        option: PmDcReference,
    },
    Relief {
        control: PmDcReference,
        width: PmDcReference,
        depth: PmDcReference,
        form: PmDcReference,
        minimum_remnant: PmDcReference,
        seam_gap: PmDcReference,
    },
    Transition {
        form: PmDcReference,
        control: PmDcReference,
        radius: PmDcReference,
    },
    EdgeSelection {
        edges: PmDcReference,
    },
    PlateControl {
        value: u32,
    },
    BendControl {
        value: u32,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        state: Option<u8>,
    },
    Enumeration {
        type_value: i16,
        value: u16,
    },
}

pub(super) fn property_parser(type_id: [u8; 16]) -> Option<PropertyParser> {
    match type_id {
        ATTACHMENT_TYPE => Some(parse_attachment),
        ATTACHMENT_STATE_TYPE => Some(parse_attachment_state),
        CORNER_TYPE => Some(parse_corner),
        RELIEF_TYPE => Some(parse_relief),
        TRANSITION_TYPE => Some(parse_transition),
        EDGE_SELECTION_TYPE => Some(parse_edge_selection),
        ATTACHMENT_SELECTIONS_TYPE => Some(parse_attachment_selections),
        PLATE_CONTROL_TYPE => Some(parse_plate_control),
        BEND_CONTROL_TYPE => Some(parse_bend_control),
        SIDE_TYPE => Some(parse_mode),
        RELIEF_FORM_TYPE => Some(parse_relief_form),
        CORNER_FORM_TYPE => Some(parse_corner_form),
        TRANSITION_FORM_TYPE => Some(parse_transition_form),
        _ => None,
    }
}

fn parse_attachment_state(
    _: &DecodeContext<'_>,
    source: View<'_>,
    version: u8,
) -> Result<PmDcFeaturePropertyPayload, CodecError> {
    let mut cursor = Cursor::new(source);
    let header = state_content_header(&mut cursor, version)?;
    let state = cursor.i32("attachment state")?;
    let thickness = cursor.reference("attachment state thickness")?;
    let radius = cursor.reference("attachment state radius")?;
    let attachment = cursor.reference("attachment state control")?;
    let relief_width = cursor.reference("attachment state relief width")?;
    let minimum_remnant = cursor.reference("attachment state minimum remnant")?;
    let options = [
        cursor.reference("attachment state first option")?,
        cursor.reference("attachment state second option")?,
    ];
    cursor.finish("attachment state")?;
    Ok(property(
        version,
        header,
        PmDcFeaturePropertyKind::SheetMetal {
            definition: SheetMetalProperty::AttachmentState {
                state,
                thickness,
                radius,
                attachment,
                relief_width,
                minimum_remnant,
                options,
            },
        },
    ))
}

fn parse_attachment(
    _: &DecodeContext<'_>,
    source: View<'_>,
    version: u8,
) -> Result<PmDcFeaturePropertyPayload, CodecError> {
    let mut cursor = Cursor::new(source);
    let header = content_header(&mut cursor, version)?;
    let control = cursor.reference("attachment control")?;
    let thickness = cursor.reference("attachment thickness")?;
    let first_mode = cursor.reference("attachment first mode")?;
    let directions = [
        cursor.reference("attachment first direction")?,
        cursor.reference("attachment second direction")?,
    ];
    let radius = cursor.reference("attachment radius")?;
    let second_mode = cursor.reference("attachment second mode")?;
    let relief_width = cursor.reference("attachment relief width")?;
    let minimum_remnant = cursor.reference("attachment minimum remnant")?;
    let first_option = cursor.reference("attachment first option")?;
    let states = NativeBytes::new([
        cursor.u8("attachment first state")?,
        cursor.u8("attachment second state")?,
        cursor.u8("attachment third state")?,
    ]);
    let second_option = cursor.reference("attachment second option")?;
    let extension = if cursor.remaining() == 4 {
        Some(cursor.reference("attachment extension")?)
    } else {
        None
    };
    cursor.finish("sheet-metal attachment")?;
    Ok(property(
        version,
        header,
        PmDcFeaturePropertyKind::SheetMetal {
            definition: SheetMetalProperty::Attachment {
                control,
                thickness,
                first_mode,
                directions,
                radius,
                second_mode,
                relief_width,
                minimum_remnant,
                first_option,
                states,
                second_option,
                extension,
            },
        },
    ))
}

fn parse_corner(
    _: &DecodeContext<'_>,
    source: View<'_>,
    version: u8,
) -> Result<PmDcFeaturePropertyPayload, CodecError> {
    let mut cursor = Cursor::new(source);
    let header = content_header(&mut cursor, version)?;
    let control = cursor.reference("sheet-metal corner control")?;
    let gap = cursor.reference("sheet-metal corner gap")?;
    let size = cursor.reference("sheet-metal corner size")?;
    let relief_form = cursor.reference("sheet-metal corner relief_form")?;
    let corner_form = cursor.reference("sheet-metal corner corner_form")?;
    let secondary_form = cursor.reference("sheet-metal corner secondary_form")?;
    let jacobi_radius = cursor.reference("sheet-metal corner jacobi_radius")?;
    let value = cursor.u32("sheet-metal corner value")?;
    let option = cursor.reference("sheet-metal corner option")?;
    cursor.finish("sheet-metal corner")?;
    Ok(property(
        version,
        header,
        PmDcFeaturePropertyKind::SheetMetal {
            definition: SheetMetalProperty::Corner {
                control,
                gap,
                size,
                relief_form,
                corner_form,
                secondary_form,
                jacobi_radius,
                value,
                option,
            },
        },
    ))
}

fn parse_relief(
    _: &DecodeContext<'_>,
    source: View<'_>,
    version: u8,
) -> Result<PmDcFeaturePropertyPayload, CodecError> {
    let mut cursor = Cursor::new(source);
    let header = content_header(&mut cursor, version)?;
    let control = cursor.reference("sheet-metal relief control")?;
    let width = cursor.reference("sheet-metal relief width")?;
    let depth = cursor.reference("sheet-metal relief depth")?;
    let form = cursor.reference("sheet-metal relief form")?;
    let minimum_remnant = cursor.reference("sheet-metal relief minimum_remnant")?;
    let seam_gap = cursor.reference("sheet-metal relief seam_gap")?;
    cursor.finish("sheet-metal relief")?;
    Ok(property(
        version,
        header,
        PmDcFeaturePropertyKind::SheetMetal {
            definition: SheetMetalProperty::Relief {
                control,
                width,
                depth,
                form,
                minimum_remnant,
                seam_gap,
            },
        },
    ))
}

fn parse_transition(
    _: &DecodeContext<'_>,
    source: View<'_>,
    version: u8,
) -> Result<PmDcFeaturePropertyPayload, CodecError> {
    let mut cursor = Cursor::new(source);
    let header = content_header(&mut cursor, version)?;
    let form = cursor.reference("sheet-metal transition form")?;
    let control = cursor.reference("sheet-metal transition control")?;
    let radius = cursor.reference("sheet-metal transition radius")?;
    cursor.finish("sheet-metal transition")?;
    Ok(property(
        version,
        header,
        PmDcFeaturePropertyKind::SheetMetal {
            definition: SheetMetalProperty::Transition {
                form,
                control,
                radius,
            },
        },
    ))
}

fn parse_edge_selection(
    _: &DecodeContext<'_>,
    source: View<'_>,
    version: u8,
) -> Result<PmDcFeaturePropertyPayload, CodecError> {
    let mut cursor = Cursor::new(source);
    let header = content_header(&mut cursor, version)?;
    let edges = cursor.reference("sheet-metal edge_selection edges")?;
    cursor.finish("sheet-metal edge_selection")?;
    Ok(property(
        version,
        header,
        PmDcFeaturePropertyKind::SheetMetal {
            definition: SheetMetalProperty::EdgeSelection { edges },
        },
    ))
}

fn parse_plate_control(
    _: &DecodeContext<'_>,
    source: View<'_>,
    version: u8,
) -> Result<PmDcFeaturePropertyPayload, CodecError> {
    let mut cursor = Cursor::new(source);
    let header = content_header(&mut cursor, version)?;
    let value = cursor.u32("sheet-metal plate_control value")?;
    cursor.finish("sheet-metal plate_control")?;
    Ok(property(
        version,
        header,
        PmDcFeaturePropertyKind::SheetMetal {
            definition: SheetMetalProperty::PlateControl { value },
        },
    ))
}

fn parse_bend_control(
    _: &DecodeContext<'_>,
    source: View<'_>,
    version: u8,
) -> Result<PmDcFeaturePropertyPayload, CodecError> {
    let mut cursor = Cursor::new(source);
    let header = content_header(&mut cursor, version)?;
    let value = cursor.u32("sheet-metal bend_control value")?;
    let state = if cursor.remaining() == 1 {
        Some(cursor.u8("sheet-metal bend_control state")?)
    } else {
        None
    };
    cursor.finish("sheet-metal bend_control")?;
    Ok(property(
        version,
        header,
        PmDcFeaturePropertyKind::SheetMetal {
            definition: SheetMetalProperty::BendControl { value, state },
        },
    ))
}

fn parse_enumeration(
    source: View<'_>,
    version: u8,
    expected: &[i16],
) -> Result<PmDcFeaturePropertyPayload, CodecError> {
    let mut cursor = Cursor::new(source);
    let header = content_header(&mut cursor, version)?;
    let type_value = cursor.i16("sheet-metal enumeration type_value")?;
    if !expected.contains(&type_value) {
        return Err(CodecError::malformed(format_args!(
            "Inventor sheet-metal enumeration type {type_value} is outside {expected:?}"
        )));
    }
    let value = cursor.u16("sheet-metal enumeration value")?;
    cursor.finish("sheet-metal enumeration")?;
    Ok(property(
        version,
        header,
        PmDcFeaturePropertyKind::SheetMetal {
            definition: SheetMetalProperty::Enumeration { type_value, value },
        },
    ))
}

macro_rules! enumeration_parser {
    ($name:ident, $type_value:literal) => {
        fn $name(
            _: &DecodeContext<'_>,
            source: View<'_>,
            version: u8,
        ) -> Result<PmDcFeaturePropertyPayload, CodecError> {
            parse_enumeration(source, version, &[$type_value])
        }
    };
}
enumeration_parser!(parse_mode, 9);
enumeration_parser!(parse_corner_form, 8);
enumeration_parser!(parse_transition_form, 4);

fn parse_relief_form(
    _: &DecodeContext<'_>,
    source: View<'_>,
    version: u8,
) -> Result<PmDcFeaturePropertyPayload, CodecError> {
    parse_enumeration(source, version, &[304, 501])
}

fn parse_attachment_selections(
    _: &DecodeContext<'_>,
    source: View<'_>,
    version: u8,
) -> Result<PmDcFeaturePropertyPayload, CodecError> {
    let mut cursor = Cursor::new(source);
    let header = content_header(&mut cursor, version)?;
    let items = reference_list(&mut cursor, 2, "sheet-metal attachment selections")?;
    cursor.finish("sheet-metal attachment selections")?;
    Ok(property(
        version,
        header,
        PmDcFeaturePropertyKind::References {
            family: PmDcFeatureReferenceFamily::SheetMetalAttachmentSelections,
            items,
        },
    ))
}

impl SheetMetalProperty {
    pub(crate) fn references(&self) -> Vec<PmDcReference> {
        match self {
            Self::AttachmentState {
                thickness,
                radius,
                attachment,
                relief_width,
                minimum_remnant,
                options,
                ..
            } => vec![
                *thickness,
                *radius,
                *attachment,
                *relief_width,
                *minimum_remnant,
                options[0],
                options[1],
            ],
            Self::Attachment {
                control,
                thickness,
                first_mode,
                directions,
                radius,
                second_mode,
                relief_width,
                minimum_remnant,
                first_option,
                second_option,
                extension,
                ..
            } => {
                let mut refs = vec![
                    *control,
                    *thickness,
                    *first_mode,
                    directions[0],
                    directions[1],
                    *radius,
                    *second_mode,
                    *relief_width,
                    *minimum_remnant,
                    *first_option,
                    *second_option,
                ];
                refs.extend(extension);
                refs
            }
            Self::Corner {
                control,
                gap,
                size,
                relief_form,
                corner_form,
                secondary_form,
                jacobi_radius,
                option,
                ..
            } => vec![
                *control,
                *gap,
                *size,
                *relief_form,
                *corner_form,
                *secondary_form,
                *jacobi_radius,
                *option,
            ],
            Self::Relief {
                control,
                width,
                depth,
                form,
                minimum_remnant,
                seam_gap,
            } => vec![*control, *width, *depth, *form, *minimum_remnant, *seam_gap],
            Self::Transition {
                form,
                control,
                radius,
            } => vec![*form, *control, *radius],
            Self::EdgeSelection { edges } => vec![*edges],
            Self::PlateControl { .. } | Self::BendControl { .. } | Self::Enumeration { .. } => {
                vec![]
            }
        }
    }
}

pub(crate) fn valid_references<'a>(
    property: &PmDcFeatureProperty,
    type_at: impl Fn(u32) -> Option<&'a str>,
) -> bool {
    if let PmDcFeaturePropertyKind::References {
        family: PmDcFeatureReferenceFamily::SheetMetalAttachmentSelections,
        items,
    } = &property.kind
    {
        return property.identity.type_id.as_str() == type_id_string(ATTACHMENT_SELECTIONS_TYPE)
            && items.marker() == 2
            && items.references().iter().all(|reference| {
                reference.record_ordinal().is_none_or(|ordinal| {
                    type_at(ordinal)
                        .is_some_and(|actual| actual == type_id_string(EDGE_SELECTION_TYPE))
                })
            });
    }
    let PmDcFeaturePropertyKind::SheetMetal { definition } = &property.kind else {
        return true;
    };
    let (record_type, expected): (_, &[[u8; 16]]) = match definition {
        SheetMetalProperty::AttachmentState { .. } => {
            (ATTACHMENT_STATE_TYPE, &ATTACHMENT_STATE_REFERENCES)
        }
        SheetMetalProperty::Attachment { extension, .. } => (
            ATTACHMENT_TYPE,
            if extension.is_some() {
                &ATTACHMENT_EXTENDED_REFERENCES
            } else {
                &ATTACHMENT_REFERENCES
            },
        ),
        SheetMetalProperty::Corner { .. } => (CORNER_TYPE, &CORNER_REFERENCES),
        SheetMetalProperty::Relief { .. } => (RELIEF_TYPE, &RELIEF_REFERENCES),
        SheetMetalProperty::Transition { .. } => (TRANSITION_TYPE, &TRANSITION_REFERENCES),
        SheetMetalProperty::EdgeSelection { .. } => {
            (EDGE_SELECTION_TYPE, &[super::EDGE_COLLECTION_TYPE])
        }
        SheetMetalProperty::PlateControl { .. } => (PLATE_CONTROL_TYPE, &[]),
        SheetMetalProperty::BendControl { .. } => (BEND_CONTROL_TYPE, &[]),
        SheetMetalProperty::Enumeration { type_value, .. } => {
            return [
                (SIDE_TYPE, 9),
                (RELIEF_FORM_TYPE, 304),
                (RELIEF_FORM_TYPE, 501),
                (CORNER_FORM_TYPE, 8),
                (TRANSITION_FORM_TYPE, 4),
            ]
            .iter()
            .any(|(kind, expected)| {
                type_id_string(*kind) == property.identity.type_id.as_str()
                    && type_value == expected
            });
        }
    };
    let references = definition.references();
    type_id_string(record_type) == property.identity.type_id.as_str()
        && references.len() == expected.len()
        && references
            .iter()
            .zip(expected)
            .all(|(reference, expected)| {
                reference.record_ordinal().is_none_or(|ordinal| {
                    type_at(ordinal).is_some_and(|actual| {
                        actual == type_id_string(*expected)
                            || (record_type == ATTACHMENT_STATE_TYPE
                                && *expected == ATTACHMENT_TYPE
                                && actual == type_id_string(HEM_CONTROL_TYPE))
                    })
                })
            })
}

const PARAMETER_TYPE: [u8; 16] = crate::pmdc::inventor_id(0x9087_4d26);
const DIRECTION_TYPE: [u8; 16] = [
    0x40, 0xdf, 0x52, 0xce, 0xd0, 0x11, 0xd0, 0xd2, 0x00, 0x08, 0xcc, 0xbc, 0x06, 0x63, 0xdc, 0x09,
];
const ATTACHMENT_STATE_TYPE: [u8; 16] = [
    0x93, 0x59, 0x6b, 0xa9, 0xd3, 0x11, 0xef, 0xa8, 0x60, 0x00, 0xb3, 0xa9, 0x9d, 0xcc, 0xef, 0xb0,
];
const HEM_CONTROL_TYPE: [u8; 16] = [
    0xe5, 0xb8, 0x10, 0xb3, 0xd4, 0x11, 0xa3, 0x7a, 0x60, 0x00, 0x50, 0xaa, 0x9d, 0xcc, 0xef, 0xb0,
];
const ATTACHMENT_REFERENCES: [[u8; 16]; 11] = [
    super::BOOLEAN_TYPE,
    PARAMETER_TYPE,
    SIDE_TYPE,
    DIRECTION_TYPE,
    DIRECTION_TYPE,
    PARAMETER_TYPE,
    SIDE_TYPE,
    PARAMETER_TYPE,
    PARAMETER_TYPE,
    super::BOOLEAN_TYPE,
    super::BOOLEAN_TYPE,
];
const ATTACHMENT_EXTENDED_REFERENCES: [[u8; 16]; 12] = [
    super::BOOLEAN_TYPE,
    PARAMETER_TYPE,
    SIDE_TYPE,
    DIRECTION_TYPE,
    DIRECTION_TYPE,
    PARAMETER_TYPE,
    SIDE_TYPE,
    PARAMETER_TYPE,
    PARAMETER_TYPE,
    super::BOOLEAN_TYPE,
    super::BOOLEAN_TYPE,
    ATTACHMENT_STATE_TYPE,
];
const CORNER_REFERENCES: [[u8; 16]; 8] = [
    super::BOOLEAN_TYPE,
    PARAMETER_TYPE,
    PARAMETER_TYPE,
    RELIEF_FORM_TYPE,
    CORNER_FORM_TYPE,
    RELIEF_FORM_TYPE,
    PARAMETER_TYPE,
    super::BOOLEAN_TYPE,
];
const RELIEF_REFERENCES: [[u8; 16]; 6] = [
    super::BOOLEAN_TYPE,
    PARAMETER_TYPE,
    PARAMETER_TYPE,
    RELIEF_FORM_TYPE,
    PARAMETER_TYPE,
    PARAMETER_TYPE,
];
const TRANSITION_REFERENCES: [[u8; 16]; 3] =
    [TRANSITION_FORM_TYPE, super::BOOLEAN_TYPE, PARAMETER_TYPE];
const ATTACHMENT_STATE_REFERENCES: [[u8; 16]; 7] = [
    PARAMETER_TYPE,
    PARAMETER_TYPE,
    ATTACHMENT_TYPE,
    PARAMETER_TYPE,
    PARAMETER_TYPE,
    super::BOOLEAN_TYPE,
    super::BOOLEAN_TYPE,
];

pub(crate) fn native_kind_losses(
    inventory: &super::FeatureInventory,
    features: &[cadmpeg_ir::features::Feature],
) -> Vec<cadmpeg_ir::report::loss::LossNote> {
    use crate::loss::InventorLossCode;
    let (attached_plates, bends) = native_kind_counts(inventory, features);
    let mut losses = Vec::new();
    if attached_plates != 0 {
        losses.push(InventorLossCode::FeatureSheetMetalAttachmentControlsNative.note(format!(
            "Retained {attached_plates} attached plate(s) as native history features: the operands, controls, style defaults, or attachment selections do not close the neutral construction."
        )));
    }
    if bends != 0 {
        losses.push(InventorLossCode::FeatureSheetMetalBendControlsNative.note(format!(
            "Retained {bends} separate bend record(s) as native history features: the attached plate or repeated bend operands do not close the neutral construction."
        )));
    }
    for feature in features {
        if matches!(feature.evaluation.definition(),
            cadmpeg_ir::features::FeatureDefinition::Operation(cadmpeg_ir::features::FeatureOperation::Native { kind, .. })
                if kind.as_str() == "SheetMetalCorner")
        {
            losses.push(InventorLossCode::FeatureSheetMetalCornerEffectNative.note(format!(
                "Retained {} as native: the corner treatment, paired bend, controls, or complete generated-name evidence do not close the unchanged branch.", feature.id)));
        }
    }
    losses
}

pub(crate) fn native_kind_counts(
    inventory: &super::FeatureInventory,
    features: &[cadmpeg_ir::features::Feature],
) -> (usize, usize) {
    use cadmpeg_ir::features::{FeatureDefinition, FeatureOperation};
    let native: std::collections::HashSet<_> = features
        .iter()
        .filter_map(|feature| {
            matches!(
                feature.evaluation.definition(),
                FeatureDefinition::Operation(FeatureOperation::Native { .. })
            )
            .then_some(feature.native_ref.as_deref())
            .flatten()
        })
        .collect();
    let labels = cadmpeg_core::decode::unique_index(inventory.labels.iter().map(|label| {
        (
            (
                label.identity.segment_token.as_str(),
                label.header.owner.record_ordinal(),
            ),
            label,
        )
    }));
    let mut plates = 0;
    let mut bends = 0;
    for feature in &inventory.features {
        if !native.contains(feature.id().as_str()) {
            continue;
        }
        let Some(Some(label)) = labels.get(&(
            feature.identity.segment_token.as_str(),
            Some(feature.identity.record_ordinal),
        )) else {
            continue;
        };
        if label.class_id() == super::PLATE_CLASS_ID
            && super::PLATE_BEND_SLOTS.iter().any(|slot| {
                feature
                    .properties
                    .references()
                    .get(*slot)
                    .is_some_and(|reference| reference.index() != 0)
            })
        {
            plates += 1;
        }
        if label.class_id().0.into_inner() == BEND_CLASS_ID {
            bends += 1;
        }
    }
    (plates, bends)
}

pub(super) const BEND_CLASS: super::ClassId = super::ClassId(NativeBytes::new(BEND_CLASS_ID));
pub(super) const CORNER_CLASS: super::ClassId = super::ClassId(NativeBytes::new([
    0x7f, 0x58, 0x80, 0x68, 0xb7, 0x43, 0x78, 0xa0, 0x55, 0x95, 0x15, 0x94, 0x6f, 0x7a, 0x1c, 0x12,
]));

const BEND_CLASS_ID: [u8; 16] = [
    0xf2, 0x58, 0xf7, 0x4c, 0xd3, 0x11, 0x4f, 0x24, 0x60, 0x00, 0x47, 0xa9, 0x9d, 0xcc, 0xef, 0xb0,
];

#[cfg(test)]
mod tests;

pub(crate) fn retained_control_losses<'a>(
    features: impl IntoIterator<Item = &'a cadmpeg_ir::features::Feature>,
) -> Vec<cadmpeg_ir::report::loss::LossNote> {
    use crate::loss::InventorLossCode;
    use cadmpeg_ir::features::{FeatureDefinition, FeatureOperation};
    features.into_iter().flat_map(|feature| {
        let (code, controls) = match feature.evaluation.definition() {
            FeatureDefinition::Operation(FeatureOperation::SheetMetalAttachedPlate { .. }) => (
                InventorLossCode::FeatureSheetMetalAttachmentControlsRetained,
                "plate slot-0 construction wire, plate slot-1 direction, attachment control, type-9 first and second modes, direction references, first and second options, state bytes, optional extension state, plate scalar, relief Boolean, and transition Boolean",
            ),
            FeatureDefinition::Operation(FeatureOperation::SheetMetalBend { .. }) => (
                InventorLossCode::FeatureSheetMetalBendControlsRetained,
                "plate slot-0 construction wire, plate slot-1 direction, attachment control, type-9 first and second modes, direction references, first and second options, state bytes, optional extension state, bend scalar and state, slot-10 Boolean, relief Boolean, and transition Boolean",
            ),
            FeatureDefinition::Operation(FeatureOperation::SheetMetalCorner { .. }) => (
                InventorLossCode::FeatureSheetMetalCornerControlsRetained,
                "corner gap, size, Jacobi radius, relief-form and secondary-form enumerations, type-8 corner form, control and option Booleans, raw word, feature dimensions, and profile, attachment, and result references",
            ),
            _ => return Vec::new(),
        };
        let mut losses = vec![code.note(format!("Projected {} with {controls} retained in its native records.", feature.id))];
        if let FeatureDefinition::Operation(FeatureOperation::SheetMetalBend { relief, transition, .. }) = feature.evaluation.definition() {
            if matches!(relief.form, cadmpeg_ir::features::SheetMetalReliefForm::StyleDefault)
                || matches!(transition.form, cadmpeg_ir::features::SheetMetalTransitionForm::StyleDefault) {
                losses.push(InventorLossCode::FeatureSheetMetalStyleFormsNative.note(format!(
                    "Projected {} with relief or transition set to StyleDefault; the form definitions remain in the native document style.", feature.id
                )));
            }
        }
        losses
    }).collect()
}
