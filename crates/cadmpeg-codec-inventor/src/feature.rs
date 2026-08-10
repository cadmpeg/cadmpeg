// SPDX-License-Identifier: Apache-2.0
//! Typed `PmDc` feature records and feature-list terminators.

use cadmpeg_core::decode::{DecodeContext, View};
use cadmpeg_core::CodecError;
use serde::{Deserialize, Serialize};

use crate::pmdc::{
    content_header, reference_list, type_id_string, Cursor, PmDcContentHeader, PmDcReferenceList,
};
use crate::rse::{RecordFrameState, RseInventory, SegmentBulkState, SegmentKind};

const FEATURE_TYPE: [u8; 16] = [
    0x91, 0x4d, 0x87, 0x90, 0xd0, 0x11, 0xf8, 0xd1, 0x00, 0x08, 0xca, 0xbc, 0x06, 0x63, 0xdc, 0x09,
];
const END_OF_FEATURES_TYPE: [u8; 16] = [
    0x24, 0xfd, 0x41, 0x8f, 0xd2, 0x11, 0xac, 0x6e, 0x00, 0x08, 0x2a, 0xab, 0x32, 0xa3, 0xdc, 0x09,
];
const BOOLEAN_TYPE: [u8; 16] = inventor_id(0x9087_4d28);
const SURFACE_BODY_TYPE: [u8; 16] = inventor_id(0x9087_4d47);
const ENTITY_STYLE_LINK_TYPE: [u8; 16] = inventor_id(0x9087_4d15);
const PART_OPERATION_TYPE: [u8; 16] = [
    0x28, 0xbe, 0x9a, 0x72, 0xd1, 0x11, 0x44, 0x09, 0x00, 0x08, 0x4e, 0xba, 0x32, 0xa3, 0xdc, 0x09,
];
const BOUNDARY_PATCH_TYPE: [u8; 16] = [
    0x91, 0x73, 0x94, 0x22, 0xd1, 0x11, 0x07, 0xcf, 0x00, 0x08, 0x35, 0xbd, 0x06, 0x63, 0xdc, 0x09,
];
const EXTENT_TYPE: [u8; 16] = [
    0x29, 0x7d, 0x63, 0x92, 0xd1, 0x11, 0x3c, 0xb9, 0x00, 0x08, 0x31, 0xbd, 0x06, 0x63, 0xdc, 0x09,
];
const FEATURE_DIMENSIONS_TYPE: [u8; 16] = [
    0x71, 0xf2, 0x3e, 0xd8, 0xd2, 0x11, 0x50, 0x94, 0xa0, 0x00, 0x49, 0x80, 0x36, 0x03, 0xc8, 0xc9,
];
const RDX_VARIABLE_TYPE: [u8; 16] = [
    0xdf, 0xd5, 0x1d, 0xbb, 0xd1, 0x11, 0x6e, 0x72, 0x00, 0x08, 0x17, 0xbd, 0x06, 0x63, 0xdc, 0x09,
];
const AUXILIARY_ENUM_TYPE: [u8; 16] = [
    0x73, 0x39, 0xfd, 0xce, 0x7a, 0x4e, 0x40, 0x11, 0xbe, 0xe3, 0x43, 0x89, 0x79, 0x08, 0xba, 0x92,
];
const OBJECT_COLLECTION_TYPE: [u8; 16] = [
    0xae, 0x70, 0x68, 0x0e, 0xd1, 0x4a, 0x1e, 0x86, 0xd7, 0x62, 0x48, 0xb0, 0x3c, 0x2a, 0x96, 0xe1,
];
const HOLE_TYPE: [u8; 16] = [
    0x11, 0x7c, 0xcd, 0x43, 0xd2, 0x11, 0x96, 0x58, 0xa0, 0x00, 0x21, 0x80, 0x36, 0x03, 0xc8, 0xc9,
];
const PLACEMENT_TYPE: [u8; 16] = [
    0x2c, 0x92, 0x56, 0x72, 0x4d, 0x4d, 0x6d, 0x70, 0x94, 0x27, 0xfd, 0x96, 0x4d, 0x84, 0xdf, 0x16,
];
const FILLET_EDGE_SETS_TYPE: [u8; 16] = [
    0xda, 0xe9, 0x48, 0x1b, 0xd2, 0x11, 0xdc, 0x2c, 0x00, 0x08, 0x3e, 0xab, 0x1b, 0x14, 0xdc, 0x09,
];
const FILLET_TYPE: [u8; 16] = [
    0x27, 0x88, 0xf2, 0x78, 0xc5, 0x4d, 0xd7, 0xbe, 0x43, 0x13, 0xb3, 0x98, 0x60, 0x39, 0xb5, 0x2e,
];
const PROFILE_SELECTION_TYPE: [u8; 16] = [
    0x3b, 0x24, 0x77, 0xa4, 0xd1, 0x11, 0x8f, 0x96, 0x00, 0x08, 0x26, 0xbd, 0x06, 0x63, 0xdc, 0x09,
];
const FEATURE_LABEL_TYPE: [u8; 16] = [
    0x2b, 0xa4, 0x48, 0x2b, 0xd2, 0x11, 0x58, 0x64, 0x60, 0x00, 0x74, 0xb7, 0x9b, 0x49, 0xeb, 0xb0,
];

const fn inventor_id(time_low: u32) -> [u8; 16] {
    let first = time_low.to_le_bytes();
    [
        first[0], first[1], first[2], first[3], 0xd0, 0x11, 0xf8, 0xd1, 0x00, 0x08, 0xca, 0xbc,
        0x06, 0x63, 0xdc, 0x09,
    ]
}

#[derive(Debug)]
pub(crate) struct FeatureInventory {
    pub(crate) features: Vec<PmDcFeature>,
    pub(crate) terminators: Vec<PmDcFeatureTerminator>,
    pub(crate) properties: Vec<PmDcFeatureProperty>,
    pub(crate) labels: Vec<PmDcFeatureLabel>,
    pub(crate) entity_style_links: Vec<PmDcEntityStyleLink>,
    pub(crate) issues: Vec<FeatureRecordIssue>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub(crate) struct PmDcFeatureProperty {
    pub(crate) id: String,
    pub(crate) type_id: String,
    pub(crate) segment_token: String,
    pub(crate) record_ordinal: u32,
    pub(crate) save_version_major: u8,
    pub(crate) header: PmDcContentHeader,
    pub(crate) kind: PmDcFeaturePropertyKind,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "form", rename_all = "snake_case")]
pub(crate) enum PmDcFeaturePropertyKind {
    Enumeration {
        family: PmDcFeatureEnumFamily,
        type_value: i16,
        value: u16,
    },
    Boolean {
        name: String,
        name_value: u32,
        value: bool,
    },
    References {
        family: PmDcFeatureReferenceFamily,
        items: PmDcReferenceList,
    },
    RdxVariable {
        name: String,
        name_value: u32,
        nominal_value: u32,
        model_value: u32,
    },
    SurfaceBody {
        body: crate::pmdc::PmDcReference,
    },
    ProfileSelection {
        entity_link: crate::pmdc::PmDcReference,
        value: u8,
    },
    Placement {
        transform: crate::pmdc::PmDcReference,
        point: crate::pmdc::PmDcReference,
        value: crate::pmdc::PmDcReference,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum PmDcFeatureEnumFamily {
    PartOperation,
    Extent,
    Hole,
    Fillet,
    Auxiliary,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum PmDcFeatureReferenceFamily {
    BoundaryPatch,
    FeatureDimensions,
    ObjectCollection,
    FilletEdgeSets,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct PmDcLinkedHeader {
    pub(crate) header_value: u32,
    pub(crate) header_id: u16,
    pub(crate) values: [u32; 2],
    pub(crate) owner: crate::pmdc::PmDcReference,
    pub(crate) parent: crate::pmdc::PmDcReference,
    pub(crate) next: crate::pmdc::PmDcReference,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct PmDcFeatureLabel {
    pub(crate) id: String,
    pub(crate) type_id: String,
    pub(crate) segment_token: String,
    pub(crate) record_ordinal: u32,
    pub(crate) save_version_major: u8,
    pub(crate) header: PmDcLinkedHeader,
    pub(crate) index: u32,
    pub(crate) participants: PmDcReferenceList,
    pub(crate) name: String,
    pub(crate) class_id: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct PmDcEntityStyleLink {
    pub(crate) id: String,
    pub(crate) type_id: String,
    pub(crate) segment_token: String,
    pub(crate) record_ordinal: u32,
    pub(crate) save_version_major: u8,
    pub(crate) header: PmDcLinkedHeader,
    pub(crate) value: u32,
    pub(crate) associative_id: u32,
    pub(crate) entity_type: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct PmDcFeature {
    pub(crate) id: String,
    pub(crate) type_id: String,
    pub(crate) segment_token: String,
    pub(crate) record_ordinal: u32,
    pub(crate) save_version_major: u8,
    pub(crate) header: PmDcContentHeader,
    pub(crate) state: i32,
    pub(crate) outline_value: u32,
    pub(crate) properties: PmDcReferenceList,
    pub(crate) value: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct PmDcFeatureTerminator {
    pub(crate) id: String,
    pub(crate) type_id: String,
    pub(crate) segment_token: String,
    pub(crate) record_ordinal: u32,
    pub(crate) save_version_major: u8,
    pub(crate) header: PmDcContentHeader,
    pub(crate) state: i32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct FeatureRecordIssue {
    pub(crate) id: String,
    pub(crate) type_id: String,
    pub(crate) segment_token: String,
    pub(crate) record_ordinal: u32,
    pub(crate) detail: String,
}

pub(crate) fn inventory(
    ctx: &DecodeContext<'_>,
    document: &RseInventory<'_>,
) -> Result<FeatureInventory, CodecError> {
    let mut inventory = FeatureInventory {
        features: Vec::new(),
        terminators: Vec::new(),
        properties: Vec::new(),
        labels: Vec::new(),
        entity_style_links: Vec::new(),
        issues: Vec::new(),
    };
    for segment in &document.segments {
        if segment.kind != SegmentKind::PmDc {
            continue;
        }
        let Some(version) = segment.registry_version_major else {
            continue;
        };
        if !(15..=22).contains(&version) {
            continue;
        }
        let SegmentBulkState::Framed(bulk) = &segment.bulk else {
            continue;
        };
        let RecordFrameState::Framed(table) = &bulk.records else {
            continue;
        };
        for record in &table.records {
            let parsed = match record.type_id {
                FEATURE_TYPE => parse_feature(ctx, record.payload, version).map(|mut feature| {
                    feature.id = format!(
                        "inventor:pmdc:feature#{}-{}",
                        segment.pair.token.as_str(),
                        record.ordinal
                    );
                    feature.type_id = type_id_string(record.type_id);
                    feature.segment_token = segment.pair.token.as_str().into();
                    feature.record_ordinal = record.ordinal;
                    inventory.features.push(feature);
                }),
                END_OF_FEATURES_TYPE => {
                    parse_terminator(record.payload, version).map(|mut terminator| {
                        terminator.id = format!(
                            "inventor:pmdc:feature-terminator#{}-{}",
                            segment.pair.token.as_str(),
                            record.ordinal
                        );
                        terminator.type_id = type_id_string(record.type_id);
                        terminator.segment_token = segment.pair.token.as_str().into();
                        terminator.record_ordinal = record.ordinal;
                        inventory.terminators.push(terminator);
                    })
                }
                FEATURE_LABEL_TYPE => parse_label(ctx, record.payload, version).map(|mut label| {
                    if label.name.is_empty() {
                        return;
                    }
                    (
                        label.id,
                        label.type_id,
                        label.segment_token,
                        label.record_ordinal,
                    ) = identity(
                        "feature-label",
                        segment.pair.token.as_str(),
                        record.ordinal,
                        record.type_id,
                    );
                    inventory.labels.push(label);
                }),
                ENTITY_STYLE_LINK_TYPE => {
                    parse_entity_style_link(record.payload, version).map(|mut link| {
                        (
                            link.id,
                            link.type_id,
                            link.segment_token,
                            link.record_ordinal,
                        ) = identity(
                            "entity-style-link",
                            segment.pair.token.as_str(),
                            record.ordinal,
                            record.type_id,
                        );
                        inventory.entity_style_links.push(link);
                    })
                }
                type_id => feature_property_parser(type_id).map_or_else(
                    || Ok(()),
                    |parser| {
                        parser(ctx, record.payload, version).map(|mut property| {
                            (
                                property.id,
                                property.type_id,
                                property.segment_token,
                                property.record_ordinal,
                            ) = identity(
                                "feature-property",
                                segment.pair.token.as_str(),
                                record.ordinal,
                                record.type_id,
                            );
                            inventory.properties.push(property);
                        })
                    },
                ),
            };
            if let Err(error) = parsed {
                inventory.issues.push(FeatureRecordIssue {
                    id: format!(
                        "inventor:pmdc:feature-record-issue#{}-{}",
                        segment.pair.token.as_str(),
                        record.ordinal
                    ),
                    type_id: type_id_string(record.type_id),
                    segment_token: segment.pair.token.as_str().into(),
                    record_ordinal: record.ordinal,
                    detail: error.to_string(),
                });
            }
        }
    }
    ctx.charge_collection_items(
        inventory
            .features
            .len()
            .saturating_add(inventory.terminators.len())
            .saturating_add(inventory.properties.len())
            .saturating_add(inventory.labels.len())
            .saturating_add(inventory.entity_style_links.len())
            .saturating_add(inventory.issues.len()) as u64,
        "admit Inventor feature records",
    )?;
    Ok(inventory)
}

fn parse_feature(
    ctx: &DecodeContext<'_>,
    source: View<'_>,
    version: u8,
) -> Result<PmDcFeature, CodecError> {
    let mut cursor = Cursor::new(source);
    let header = content_header(&mut cursor)?;
    let state = cursor.u32("feature state")? as i32;
    let outline_value = cursor.u32("feature outline value")?;
    let properties = reference_list(ctx, &mut cursor, 2, "feature property list")?;
    let value = cursor.u32("feature value")?;
    cursor.finish("feature")?;
    Ok(PmDcFeature {
        id: String::new(),
        type_id: String::new(),
        segment_token: String::new(),
        record_ordinal: 0,
        save_version_major: version,
        header,
        state,
        outline_value,
        properties,
        value,
    })
}

fn parse_terminator(source: View<'_>, version: u8) -> Result<PmDcFeatureTerminator, CodecError> {
    let mut cursor = Cursor::new(source);
    let header = content_header(&mut cursor)?;
    let state = cursor.u32("feature-terminator state")? as i32;
    cursor.finish("feature terminator")?;
    Ok(PmDcFeatureTerminator {
        id: String::new(),
        type_id: String::new(),
        segment_token: String::new(),
        record_ordinal: 0,
        save_version_major: version,
        header,
        state,
    })
}

type PropertyParser =
    for<'a> fn(&DecodeContext<'a>, View<'a>, u8) -> Result<PmDcFeatureProperty, CodecError>;

fn feature_property_parser(type_id: [u8; 16]) -> Option<PropertyParser> {
    match type_id {
        PART_OPERATION_TYPE => Some(parse_part_operation),
        EXTENT_TYPE => Some(parse_extent),
        HOLE_TYPE => Some(parse_hole),
        FILLET_TYPE => Some(parse_fillet),
        AUXILIARY_ENUM_TYPE => Some(parse_auxiliary_enum),
        BOOLEAN_TYPE => Some(parse_boolean),
        BOUNDARY_PATCH_TYPE => Some(parse_boundary_patch),
        FEATURE_DIMENSIONS_TYPE => Some(parse_feature_dimensions),
        OBJECT_COLLECTION_TYPE => Some(parse_object_collection),
        FILLET_EDGE_SETS_TYPE => Some(parse_fillet_edge_sets),
        RDX_VARIABLE_TYPE => Some(parse_rdx_variable),
        SURFACE_BODY_TYPE => Some(parse_surface_body),
        PROFILE_SELECTION_TYPE => Some(parse_profile_selection),
        PLACEMENT_TYPE => Some(parse_placement),
        _ => None,
    }
}

fn identity(
    family: &str,
    segment_token: &str,
    record_ordinal: u32,
    type_id: [u8; 16],
) -> (String, String, String, u32) {
    (
        format!("inventor:pmdc:{family}#{segment_token}-{record_ordinal}"),
        type_id_string(type_id),
        segment_token.into(),
        record_ordinal,
    )
}

fn property(
    version: u8,
    header: PmDcContentHeader,
    kind: PmDcFeaturePropertyKind,
) -> PmDcFeatureProperty {
    PmDcFeatureProperty {
        id: String::new(),
        type_id: String::new(),
        segment_token: String::new(),
        record_ordinal: 0,
        save_version_major: version,
        header,
        kind,
    }
}

fn parse_enumeration(
    source: View<'_>,
    version: u8,
    family: PmDcFeatureEnumFamily,
) -> Result<PmDcFeatureProperty, CodecError> {
    let mut cursor = Cursor::new(source);
    let header = content_header(&mut cursor)?;
    let type_value = cursor.i16("feature enumeration type")?;
    let value = cursor.u16("feature enumeration value")?;
    cursor.finish("feature enumeration")?;
    Ok(property(
        version,
        header,
        PmDcFeaturePropertyKind::Enumeration {
            family,
            type_value,
            value,
        },
    ))
}

macro_rules! enum_parser {
    ($name:ident, $family:ident) => {
        fn $name(
            _: &DecodeContext<'_>,
            source: View<'_>,
            version: u8,
        ) -> Result<PmDcFeatureProperty, CodecError> {
            parse_enumeration(source, version, PmDcFeatureEnumFamily::$family)
        }
    };
}

enum_parser!(parse_part_operation, PartOperation);
enum_parser!(parse_extent, Extent);
enum_parser!(parse_hole, Hole);
enum_parser!(parse_fillet, Fillet);
enum_parser!(parse_auxiliary_enum, Auxiliary);

fn parse_boolean(
    ctx: &DecodeContext<'_>,
    source: View<'_>,
    version: u8,
) -> Result<PmDcFeatureProperty, CodecError> {
    let mut cursor = Cursor::new(source);
    let header = content_header(&mut cursor)?;
    let name = cursor.utf16(ctx, "feature Boolean name")?;
    let name_value = cursor.u32("feature Boolean name value")?;
    let raw = cursor.u8("feature Boolean value")?;
    if raw > 1 {
        return Err(CodecError::Malformed(format!(
            "Inventor PmDc feature Boolean value is {raw}"
        )));
    }
    cursor.finish("feature Boolean")?;
    Ok(property(
        version,
        header,
        PmDcFeaturePropertyKind::Boolean {
            name,
            name_value,
            value: raw != 0,
        },
    ))
}

fn parse_references(
    ctx: &DecodeContext<'_>,
    source: View<'_>,
    version: u8,
    family: PmDcFeatureReferenceFamily,
) -> Result<PmDcFeatureProperty, CodecError> {
    let mut cursor = Cursor::new(source);
    let header = content_header(&mut cursor)?;
    let items = reference_list(ctx, &mut cursor, 2, "feature-property references")?;
    cursor.finish("feature-property references")?;
    Ok(property(
        version,
        header,
        PmDcFeaturePropertyKind::References { family, items },
    ))
}

macro_rules! reference_parser {
    ($name:ident, $family:ident) => {
        fn $name(
            ctx: &DecodeContext<'_>,
            source: View<'_>,
            version: u8,
        ) -> Result<PmDcFeatureProperty, CodecError> {
            parse_references(ctx, source, version, PmDcFeatureReferenceFamily::$family)
        }
    };
}

reference_parser!(parse_boundary_patch, BoundaryPatch);
reference_parser!(parse_feature_dimensions, FeatureDimensions);
reference_parser!(parse_object_collection, ObjectCollection);
reference_parser!(parse_fillet_edge_sets, FilletEdgeSets);

fn parse_rdx_variable(
    ctx: &DecodeContext<'_>,
    source: View<'_>,
    version: u8,
) -> Result<PmDcFeatureProperty, CodecError> {
    let mut cursor = Cursor::new(source);
    let header = content_header(&mut cursor)?;
    let name = cursor.utf16(ctx, "feature RDx variable name")?;
    let name_value = cursor.u32("feature RDx variable name value")?;
    let nominal_value = cursor.u32("feature RDx nominal value")?;
    let model_value = cursor.u32("feature RDx model value")?;
    cursor.finish("feature RDx variable")?;
    Ok(property(
        version,
        header,
        PmDcFeaturePropertyKind::RdxVariable {
            name,
            name_value,
            nominal_value,
            model_value,
        },
    ))
}

fn parse_surface_body(
    _: &DecodeContext<'_>,
    source: View<'_>,
    version: u8,
) -> Result<PmDcFeatureProperty, CodecError> {
    let mut cursor = Cursor::new(source);
    let header = content_header(&mut cursor)?;
    let body = cursor.reference("feature surface-body reference")?;
    cursor.finish("feature surface body")?;
    Ok(property(
        version,
        header,
        PmDcFeaturePropertyKind::SurfaceBody { body },
    ))
}

fn parse_profile_selection(
    _: &DecodeContext<'_>,
    source: View<'_>,
    version: u8,
) -> Result<PmDcFeatureProperty, CodecError> {
    let mut cursor = Cursor::new(source);
    let header = content_header(&mut cursor)?;
    let entity_link = cursor.reference("profile-selection entity link")?;
    let value = cursor.u8("profile-selection value")?;
    cursor.finish("profile selection")?;
    Ok(property(
        version,
        header,
        PmDcFeaturePropertyKind::ProfileSelection { entity_link, value },
    ))
}

fn parse_entity_style_link(
    source: View<'_>,
    version: u8,
) -> Result<PmDcEntityStyleLink, CodecError> {
    let mut cursor = Cursor::new(source);
    let header = linked_header(&mut cursor)?;
    let value = cursor.u32("entity-style-link value")?;
    let associative_id = cursor.u32("entity-style-link associative id")?;
    let entity_type = cursor.u32("entity-style-link entity type")?;
    cursor.finish("entity-style link")?;
    Ok(PmDcEntityStyleLink {
        id: String::new(),
        type_id: String::new(),
        segment_token: String::new(),
        record_ordinal: 0,
        save_version_major: version,
        header,
        value,
        associative_id,
        entity_type,
    })
}

fn parse_placement(
    _: &DecodeContext<'_>,
    source: View<'_>,
    version: u8,
) -> Result<PmDcFeatureProperty, CodecError> {
    let mut cursor = Cursor::new(source);
    let header = content_header(&mut cursor)?;
    let transform = cursor.reference("feature placement transform")?;
    let point = cursor.reference("feature placement point")?;
    let value = cursor.reference("feature placement value")?;
    cursor.finish("feature placement")?;
    Ok(property(
        version,
        header,
        PmDcFeaturePropertyKind::Placement {
            transform,
            point,
            value,
        },
    ))
}

fn linked_header(cursor: &mut Cursor<'_>) -> Result<PmDcLinkedHeader, CodecError> {
    Ok(PmDcLinkedHeader {
        header_value: cursor.u32("linked header value")?,
        header_id: cursor.u16("linked header id")?,
        values: [
            cursor.u32("linked header value 0")?,
            cursor.u32("linked header value 1")?,
        ],
        owner: cursor.reference("linked owner reference")?,
        parent: cursor.reference("linked parent reference")?,
        next: cursor.reference("linked next reference")?,
    })
}

fn parse_label(
    ctx: &DecodeContext<'_>,
    source: View<'_>,
    version: u8,
) -> Result<PmDcFeatureLabel, CodecError> {
    let mut cursor = Cursor::new(source);
    let header = linked_header(&mut cursor)?;
    let index = cursor.u32("feature-label index")?;
    let participants = reference_list(ctx, &mut cursor, 2, "feature-label participants")?;
    let name = cursor.utf16(ctx, "feature label")?;
    let class_id = type_id_string(
        cursor
            .take(16, "feature-label class id")?
            .try_into()
            .expect("sixteen-byte class id"),
    );
    cursor.finish("feature label")?;
    Ok(PmDcFeatureLabel {
        id: String::new(),
        type_id: String::new(),
        segment_token: String::new(),
        record_ordinal: 0,
        save_version_major: version,
        header,
        index,
        participants,
        name,
        class_id,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use cadmpeg_core::decode::{DecodeArena, DecodePolicy};

    fn content(index: u32) -> Vec<u8> {
        let mut bytes = Vec::new();
        bytes.extend_from_slice(&0u32.to_le_bytes());
        bytes.extend_from_slice(&index.to_le_bytes()[..2]);
        bytes.extend_from_slice(&0u32.to_le_bytes());
        bytes.extend_from_slice(&0x0002_0200u32.to_le_bytes());
        bytes.extend_from_slice(&0x8000_0003u32.to_le_bytes());
        bytes.extend_from_slice(&index.to_le_bytes());
        bytes
    }

    fn references(values: &[u32]) -> Vec<u8> {
        let mut bytes = Vec::new();
        bytes.extend_from_slice(&2u16.to_le_bytes());
        bytes.extend_from_slice(&0x3000u16.to_le_bytes());
        bytes.extend_from_slice(&(values.len() as u32).to_le_bytes());
        if !values.is_empty() {
            bytes.extend_from_slice(&(values.len() as u32).to_le_bytes());
            bytes.extend_from_slice(&0u32.to_le_bytes());
        }
        for value in values {
            bytes.extend_from_slice(&value.to_le_bytes());
        }
        bytes
    }

    fn utf16(value: &str) -> Vec<u8> {
        let units = value.encode_utf16().collect::<Vec<_>>();
        let mut bytes = (units.len() as u32).to_le_bytes().to_vec();
        for unit in units {
            bytes.extend_from_slice(&unit.to_le_bytes());
        }
        bytes
    }

    fn parse<T>(bytes: &[u8], parser: impl FnOnce(&DecodeContext<'_>, View<'_>) -> T) -> T {
        let arena = DecodeArena::new();
        let policy = DecodePolicy::default();
        let (ctx, source) = DecodeContext::from_root_bytes(bytes, &arena, &policy).expect("view");
        parser(&ctx, source)
    }

    #[test]
    fn parses_generated_feature_and_terminator() {
        let mut feature = content(7);
        feature.extend_from_slice(&(-1i32).to_le_bytes());
        feature.extend_from_slice(&42u32.to_le_bytes());
        feature.extend_from_slice(&2u16.to_le_bytes());
        feature.extend_from_slice(&0x3000u16.to_le_bytes());
        feature.extend_from_slice(&2u32.to_le_bytes());
        feature.extend_from_slice(&[0; 8]);
        feature.extend_from_slice(&0x8000_0004u32.to_le_bytes());
        feature.extend_from_slice(&5u32.to_le_bytes());
        feature.extend_from_slice(&9u32.to_le_bytes());
        let parsed = parse(&feature, |ctx, source| {
            parse_feature(ctx, source, 16).expect("feature")
        });
        assert_eq!(parsed.state, -1);
        assert_eq!(parsed.outline_value, 42);
        assert_eq!(parsed.properties.references.len(), 2);
        assert!(parsed.properties.references[0].qualified);
        assert_eq!(parsed.value, 9);

        let mut terminator = content(8);
        terminator.extend_from_slice(&(-1i32).to_le_bytes());
        let parsed = parse(&terminator, |_, source| {
            parse_terminator(source, 16).expect("terminator")
        });
        assert_eq!(parsed.state, -1);
    }

    #[test]
    fn parses_generated_feature_properties_and_label() {
        let mut enumeration = content(10);
        enumeration.extend_from_slice(&5i16.to_le_bytes());
        enumeration.extend_from_slice(&3u16.to_le_bytes());
        let parsed = parse(&enumeration, |ctx, source| {
            parse_part_operation(ctx, source, 16).expect("enumeration")
        });
        assert!(matches!(
            parsed.kind,
            PmDcFeaturePropertyKind::Enumeration {
                family: PmDcFeatureEnumFamily::PartOperation,
                type_value: 5,
                value: 3
            }
        ));

        let mut boolean = content(11);
        boolean.extend_from_slice(&utf16("solid"));
        boolean.extend_from_slice(&7u32.to_le_bytes());
        boolean.push(1);
        let parsed = parse(&boolean, |ctx, source| {
            parse_boolean(ctx, source, 16).expect("Boolean")
        });
        assert!(matches!(
            parsed.kind,
            PmDcFeaturePropertyKind::Boolean { value: true, .. }
        ));

        let mut collection = content(12);
        collection.extend_from_slice(&references(&[0x8000_0004, 0x8000_0005]));
        let parsed = parse(&collection, |ctx, source| {
            parse_boundary_patch(ctx, source, 16).expect("boundary patch")
        });
        assert!(matches!(
            parsed.kind,
            PmDcFeaturePropertyKind::References { items, .. }
                if items.references.len() == 2
        ));

        let mut rdx = content(13);
        rdx.extend_from_slice(&utf16("RDxVar1"));
        rdx.extend_from_slice(&0u32.to_le_bytes());
        rdx.extend_from_slice(&2u32.to_le_bytes());
        rdx.extend_from_slice(&3u32.to_le_bytes());
        parse(&rdx, |ctx, source| {
            parse_rdx_variable(ctx, source, 16).expect("RDx variable")
        });

        let mut surface = content(14);
        surface.extend_from_slice(&0x8000_0006u32.to_le_bytes());
        parse(&surface, |ctx, source| {
            parse_surface_body(ctx, source, 16).expect("surface body")
        });

        let mut selection = content(15);
        selection.extend_from_slice(&0x8000_0007u32.to_le_bytes());
        selection.push(0);
        parse(&selection, |ctx, source| {
            parse_profile_selection(ctx, source, 16).expect("profile selection")
        });

        let mut entity_link = Vec::new();
        entity_link.extend_from_slice(&0u32.to_le_bytes());
        entity_link.extend_from_slice(&16u16.to_le_bytes());
        entity_link.extend_from_slice(&0u32.to_le_bytes());
        entity_link.extend_from_slice(&0u32.to_le_bytes());
        entity_link.extend_from_slice(&0x8000_0008u32.to_le_bytes());
        entity_link.extend_from_slice(&0u32.to_le_bytes());
        entity_link.extend_from_slice(&0x8000_0009u32.to_le_bytes());
        entity_link.extend_from_slice(&1u32.to_le_bytes());
        entity_link.extend_from_slice(&2u32.to_le_bytes());
        entity_link.extend_from_slice(&3u32.to_le_bytes());
        let parsed = parse(&entity_link, |_, source| {
            parse_entity_style_link(source, 16).expect("entity-style link")
        });
        assert_eq!(parsed.header.owner.index, 8);
        assert_eq!(parsed.header.next.index, 9);
        assert_eq!(parsed.associative_id, 2);

        let mut placement = content(17);
        placement.extend_from_slice(&0x8000_0009u32.to_le_bytes());
        placement.extend_from_slice(&0x8000_000au32.to_le_bytes());
        placement.extend_from_slice(&0x8000_000bu32.to_le_bytes());
        parse(&placement, |ctx, source| {
            parse_placement(ctx, source, 16).expect("placement")
        });

        let mut label = Vec::new();
        label.extend_from_slice(&0u32.to_le_bytes());
        label.extend_from_slice(&18u16.to_le_bytes());
        label.extend_from_slice(&0u32.to_le_bytes());
        label.extend_from_slice(&0u32.to_le_bytes());
        label.extend_from_slice(&0x8000_000du32.to_le_bytes());
        label.extend_from_slice(&0u32.to_le_bytes());
        label.extend_from_slice(&0u32.to_le_bytes());
        label.extend_from_slice(&4u32.to_le_bytes());
        label.extend_from_slice(&references(&[0x8000_0013]));
        label.extend_from_slice(&utf16("Extrude1"));
        label.extend_from_slice(&[0xabu8; 16]);
        let parsed = parse(&label, |ctx, source| {
            parse_label(ctx, source, 16).expect("label")
        });
        assert_eq!(parsed.name, "Extrude1");
        assert_eq!(parsed.participants.references.len(), 1);
        assert_eq!(parsed.class_id, "abababababababababababababababab");
    }
}
