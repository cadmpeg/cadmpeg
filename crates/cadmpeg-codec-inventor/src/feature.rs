// SPDX-License-Identifier: Apache-2.0
//! Typed `PmDc` feature records and feature-list terminators.

use std::collections::{BTreeMap, HashMap, HashSet};

use cadmpeg_core::decode::{DecodeContext, View};
use cadmpeg_core::CodecError;
use cadmpeg_ir::features::{
    Angle, BooleanOp, ChamferGroup, ChamferSpec, DesignParameter, EdgeSelection, ExtrudeDirection,
    ExtrudeExtent, ExtrudeSide, ExtrudeStart, ExtrusionDirectionSource, Feature, FeatureDefinition,
    FeatureId, FeatureResultTopology, FilletGroup, HoleKind, HolePlacement, Length,
    LinearTermination, ParameterValue, ProfileRef, RadiusSpec,
};
use cadmpeg_ir::ids::FeatureResultTopologyId;
use cadmpeg_ir::math::{Point3, Vector3};
use cadmpeg_ir::sketches::Sketch;
use serde::{Deserialize, Serialize};

use crate::pmdc::{
    content_header, reference_list, type_id_string, u32_list, Cursor, PmDcContentHeader,
    PmDcReferenceList, PmDcU32List,
};
use crate::rse::{RecordFrameState, RseInventory, SegmentBulkState, SegmentKind};
use crate::{design::DesignInventory, sketch::SketchInventory};

const EPS_FEATURE_PROJECT_HOLE_E10: f64 = 1.0e-10;

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
const FILLET_EDGE_SET_TYPE: [u8; 16] = [
    0x16, 0x41, 0xd6, 0xaa, 0xd2, 0x11, 0xdb, 0x2c, 0x00, 0x08, 0x3e, 0xab, 0x1b, 0x14, 0xdc, 0x09,
];
const EDGE_COLLECTION_TYPE: [u8; 16] = inventor_id(0x9087_4d51);
const EDGE_ITEM_TYPE: [u8; 16] = [
    0x82, 0x69, 0x5c, 0x37, 0xd1, 0x11, 0x51, 0x6b, 0x00, 0x08, 0xa1, 0xba, 0x32, 0xa3, 0xdc, 0x09,
];
const FILLET_TYPE: [u8; 16] = [
    0x27, 0x88, 0xf2, 0x78, 0xc5, 0x4d, 0xd7, 0xbe, 0x43, 0x13, 0xb3, 0x98, 0x60, 0x39, 0xb5, 0x2e,
];
const CHAMFER_TYPE: [u8; 16] = [
    0x32, 0x00, 0xaa, 0x7d, 0xd2, 0x11, 0x2b, 0x83, 0x60, 0x00, 0xf3, 0xa8, 0x9d, 0xcc, 0xef, 0xb0,
];
const FILLET_EDGE_SELECTION_TYPE: [u8; 16] = [
    0x4a, 0x37, 0x49, 0x49, 0xd2, 0x11, 0x00, 0x1d, 0x00, 0x08, 0x3b, 0xab, 0x1b, 0x14, 0xdc, 0x09,
];
const RECTANGULAR_PATTERN_FEATURE_TYPE: [u8; 16] = [
    0x44, 0x32, 0x67, 0x20, 0xd2, 0x11, 0xc5, 0x1d, 0x60, 0x00, 0x2a, 0xab, 0x01, 0xf3, 0x1b, 0xb0,
];
const MIRROR_FEATURE_TYPE: [u8; 16] = [
    0xb5, 0xa9, 0xd9, 0xfa, 0xd2, 0x11, 0x05, 0x33, 0x60, 0x00, 0x2c, 0xab, 0x01, 0xf3, 0x1b, 0xb0,
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
    pub(crate) pattern_features: Vec<PmDcPatternFeature>,
    pub(crate) terminators: Vec<PmDcFeatureTerminator>,
    pub(crate) properties: Vec<PmDcFeatureProperty>,
    pub(crate) labels: Vec<PmDcFeatureLabel>,
    pub(crate) entity_style_links: Vec<PmDcEntityStyleLink>,
    pub(crate) issues: Vec<FeatureRecordIssue>,
}

pub(crate) struct FeatureProjection {
    pub(crate) features: Vec<Feature>,
    pub(crate) result_topologies: Vec<FeatureResultTopology>,
    pub(crate) unresolved_features: usize,
    pub(crate) unresolved_states: usize,
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
    WideEnumeration {
        family: PmDcFeatureEnum32Family,
        type_value: u32,
        value: u32,
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
    FilletEdgeSet {
        edges: crate::pmdc::PmDcReference,
        radius: crate::pmdc::PmDcReference,
        selection: crate::pmdc::PmDcReference,
        continuity: crate::pmdc::PmDcReference,
    },
    EdgeItem {
        index_references: PmDcU32List,
        index_reference_value: i32,
        value: u32,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum PmDcFeatureEnumFamily {
    PartOperation,
    Extent,
    Hole,
    Fillet,
    Chamfer,
    Auxiliary,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum PmDcFeatureEnum32Family {
    FilletEdgeSelection,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum PmDcPatternFamily {
    Rectangular,
    Mirror,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct PmDcPatternFeature {
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
    pub(crate) participants: PmDcReferenceList,
    pub(crate) family: PmDcPatternFamily,
    pub(crate) property_slots: Vec<crate::pmdc::PmDcReference>,
    pub(crate) control: u8,
    pub(crate) extension_values: Vec<u32>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum PmDcFeatureReferenceFamily {
    BoundaryPatch,
    FeatureDimensions,
    ObjectCollection,
    FilletEdgeSets,
    EdgeCollection,
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
        pattern_features: Vec::new(),
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
        let Some(version) = segment.registry.map(|join| join.version_major) else {
            continue;
        };
        if !(15..=22).contains(&version) {
            continue;
        }
        let SegmentBulkState::Framed(bulk) = &segment.bulk else {
            continue;
        };
        let Some(RecordFrameState::Framed(table)) = &bulk.records else {
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
                RECTANGULAR_PATTERN_FEATURE_TYPE | MIRROR_FEATURE_TYPE => {
                    let family = if record.type_id == RECTANGULAR_PATTERN_FEATURE_TYPE {
                        PmDcPatternFamily::Rectangular
                    } else {
                        PmDcPatternFamily::Mirror
                    };
                    parse_pattern_feature(ctx, record.payload, version, family).map(
                        |mut feature| {
                            feature.id = format!(
                                "inventor:pmdc:pattern-feature#{}-{}",
                                segment.pair.token.as_str(),
                                record.ordinal
                            );
                            feature.type_id = type_id_string(record.type_id);
                            feature.segment_token = segment.pair.token.as_str().into();
                            feature.record_ordinal = record.ordinal;
                            inventory.pattern_features.push(feature);
                        },
                    )
                }
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
                    detail: crate::issue_detail(error)?,
                });
            }
        }
    }
    ctx.charge_collection_items(
        inventory
            .features
            .len()
            .saturating_add(inventory.pattern_features.len())
            .saturating_add(inventory.terminators.len())
            .saturating_add(inventory.properties.len())
            .saturating_add(inventory.labels.len())
            .saturating_add(inventory.entity_style_links.len())
            .saturating_add(inventory.issues.len()) as u64,
        "admit Inventor feature records",
    )?;
    Ok(inventory)
}

fn parse_pattern_feature(
    ctx: &DecodeContext<'_>,
    source: View<'_>,
    version: u8,
    family: PmDcPatternFamily,
) -> Result<PmDcPatternFeature, CodecError> {
    let mut cursor = Cursor::new(source);
    let header = content_header(&mut cursor)?;
    let state = cursor.u32("pattern-feature state")? as i32;
    let outline_value = cursor.u32("pattern-feature outline value")?;
    let properties = reference_list(ctx, &mut cursor, 2, "pattern-feature properties")?;
    let value = cursor.u32("pattern-feature value")?;
    let participants = reference_list(ctx, &mut cursor, 2, "pattern-feature participants")?;
    let mut property_slots = Vec::new();
    for index in 0..6 {
        property_slots.push(cursor.reference(&format!("pattern-feature property {index}"))?);
    }
    let control = cursor.u8("pattern-feature control")?;
    let mut extension_values = Vec::new();
    match family {
        PmDcPatternFamily::Rectangular => {
            let remaining = if version > 20 { 26 } else { 20 };
            for index in 0..remaining {
                property_slots.push(
                    cursor.reference(&format!("rectangular-pattern property {}", index + 6))?,
                );
            }
        }
        PmDcPatternFamily::Mirror => {
            for index in 0..5 {
                property_slots
                    .push(cursor.reference(&format!("mirror-feature property {}", index + 6))?);
            }
            if version > 20 {
                extension_values.reserve(6);
                for index in 0..6 {
                    extension_values
                        .push(cursor.u32(&format!("mirror-feature extension {index}"))?);
                }
            }
            for index in 0..2 {
                property_slots
                    .push(cursor.reference(&format!("mirror-feature property {}", index + 11))?);
            }
        }
    }
    cursor.finish("pattern feature")?;
    Ok(PmDcPatternFeature {
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
        participants,
        family,
        property_slots,
        control,
        extension_values,
    })
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
        CHAMFER_TYPE => Some(parse_chamfer),
        FILLET_EDGE_SELECTION_TYPE => Some(parse_fillet_edge_selection),
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
        FILLET_EDGE_SET_TYPE => Some(parse_fillet_edge_set),
        EDGE_COLLECTION_TYPE => Some(parse_edge_collection),
        EDGE_ITEM_TYPE => Some(parse_edge_item),
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

fn parse_chamfer(
    _: &DecodeContext<'_>,
    source: View<'_>,
    version: u8,
) -> Result<PmDcFeatureProperty, CodecError> {
    let mut cursor = Cursor::new(source);
    let header = content_header(&mut cursor)?;
    let type_value = cursor.i16("chamfer enumeration type")?;
    let value = cursor.u16("chamfer enumeration value")?;
    let terminal = cursor.u32("chamfer enumeration terminal value")?;
    if terminal != 0 {
        return Err(CodecError::malformed(format_args!(
            "Inventor PmDc chamfer enumeration terminal value is {terminal}"
        )));
    }
    cursor.finish("chamfer enumeration")?;
    Ok(property(
        version,
        header,
        PmDcFeaturePropertyKind::Enumeration {
            family: PmDcFeatureEnumFamily::Chamfer,
            type_value,
            value,
        },
    ))
}

fn parse_fillet_edge_selection(
    _: &DecodeContext<'_>,
    source: View<'_>,
    version: u8,
) -> Result<PmDcFeatureProperty, CodecError> {
    let mut cursor = Cursor::new(source);
    let header = content_header(&mut cursor)?;
    let type_value = cursor.u32("fillet edge-selection enumeration type")?;
    let value = cursor.u32("fillet edge-selection enumeration value")?;
    cursor.finish("fillet edge-selection enumeration")?;
    Ok(property(
        version,
        header,
        PmDcFeaturePropertyKind::WideEnumeration {
            family: PmDcFeatureEnum32Family::FilletEdgeSelection,
            type_value,
            value,
        },
    ))
}

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
        return Err(CodecError::malformed(format_args!(
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
reference_parser!(parse_edge_collection, EdgeCollection);

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

fn parse_fillet_edge_set(
    _: &DecodeContext<'_>,
    source: View<'_>,
    version: u8,
) -> Result<PmDcFeatureProperty, CodecError> {
    let mut cursor = Cursor::new(source);
    let header = content_header(&mut cursor)?;
    let edges = cursor.reference("fillet edge-set collection")?;
    let radius = cursor.reference("fillet edge-set radius")?;
    let selection = cursor.reference("fillet edge-set selection")?;
    let continuity = cursor.reference("fillet edge-set continuity")?;
    cursor.finish("fillet edge set")?;
    Ok(property(
        version,
        header,
        PmDcFeaturePropertyKind::FilletEdgeSet {
            edges,
            radius,
            selection,
            continuity,
        },
    ))
}

fn parse_edge_item(
    ctx: &DecodeContext<'_>,
    source: View<'_>,
    version: u8,
) -> Result<PmDcFeatureProperty, CodecError> {
    let mut cursor = Cursor::new(source);
    let header = content_header(&mut cursor)?;
    let index_references = u32_list(ctx, &mut cursor, 2, "edge-item index references")?;
    let index_reference_value = if index_references.values.is_empty() {
        -1
    } else {
        cursor.i32("edge-item selected index")?
    };
    let value = cursor.u32("edge-item value")?;
    cursor.finish("edge item")?;
    Ok(property(
        version,
        header,
        PmDcFeaturePropertyKind::EdgeItem {
            index_references,
            index_reference_value,
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

const EXTRUSION_CLASS_ID: &str = "3111a90cd0118b83000819b00524dc09";
const FILLET_CLASS_ID: &str = "dc15f7f1d1114205000830b00524dc09";
const CHAMFER_CLASS_ID: &str = "3f7100f9d2118b6f6000f0a89dccefb0";
const HOLE_CLASS_ID: &str = "1a7d751fd2119c54a00020803603c8c9";

struct ProjectionIndex<'a> {
    properties: HashMap<(&'a str, u32), &'a PmDcFeatureProperty>,
    parameters: HashMap<(&'a str, u32), &'a crate::design::PmDcParameter>,
    parameter_values: HashMap<&'a str, &'a ParameterValue>,
    sketches: HashMap<(&'a str, u32), &'a crate::sketch::PmDcSketch>,
    sketch_ids: HashMap<&'a str, cadmpeg_ir::sketches::SketchId>,
    directions: HashMap<(&'a str, u32), &'a crate::sketch::PmDcDirection>,
    transforms: HashMap<(&'a str, u32), &'a crate::sketch::PmDcTransform>,
    entity_style_links: HashSet<(&'a str, u32)>,
}

pub(crate) fn project(
    inventory: &FeatureInventory,
    design: &DesignInventory,
    sketch: &SketchInventory,
    parameters: &[DesignParameter],
    sketches: &[Sketch],
) -> FeatureProjection {
    let total = inventory
        .features
        .len()
        .saturating_add(inventory.pattern_features.len());
    let feature_tokens = inventory
        .features
        .iter()
        .map(|feature| feature.segment_token.as_str())
        .chain(
            inventory
                .pattern_features
                .iter()
                .map(|feature| feature.segment_token.as_str()),
        )
        .collect::<HashSet<_>>();
    if feature_tokens.len() > 1 {
        return FeatureProjection {
            features: Vec::new(),
            result_topologies: Vec::new(),
            unresolved_features: total,
            unresolved_states: 0,
        };
    }

    let index = ProjectionIndex {
        properties: unique_by_key(&inventory.properties, |record| {
            (record.segment_token.as_str(), record.record_ordinal)
        }),
        parameters: unique_by_key(&design.parameters, |record| {
            (record.segment_token.as_str(), record.record_ordinal)
        }),
        parameter_values: parameters
            .iter()
            .filter_map(|parameter| {
                Some((parameter.native_ref.as_deref()?, parameter.value.as_ref()?))
            })
            .collect(),
        sketches: unique_by_key(&sketch.sketches, |record| {
            (record.segment_token.as_str(), record.record_ordinal)
        }),
        sketch_ids: sketches
            .iter()
            .filter_map(|sketch| {
                sketch
                    .native_ref
                    .as_deref()
                    .map(|native| (native, sketch.id.clone()))
            })
            .collect(),
        directions: unique_by_key(&sketch.directions, |record| {
            (record.segment_token.as_str(), record.record_ordinal)
        }),
        transforms: unique_by_key(&sketch.transforms, |record| {
            (record.segment_token.as_str(), record.record_ordinal)
        }),
        entity_style_links: inventory
            .entity_style_links
            .iter()
            .map(|record| (record.segment_token.as_str(), record.record_ordinal))
            .collect(),
    };
    let labels = unique_by_key(&inventory.labels, |label| {
        (
            label.segment_token.as_str(),
            label.header.owner.index.saturating_sub(1),
        )
    });
    let mut projected = Vec::new();
    for feature in &inventory.features {
        let Some(label) = labels.get(&(feature.segment_token.as_str(), feature.record_ordinal))
        else {
            continue;
        };
        let value = match label.class_id.as_str() {
            EXTRUSION_CLASS_ID => project_extrusion(feature, label, &index),
            FILLET_CLASS_ID => project_fillet(feature, label, &index),
            CHAMFER_CLASS_ID => project_chamfer(feature, label, &index),
            HOLE_CLASS_ID => project_hole(feature, label, &index),
            _ => None,
        };
        if let Some(value) = value {
            projected.push(value);
        }
    }
    let duplicate_ordinals = projected
        .iter()
        .map(|(feature, _)| feature.ordinal)
        .fold(HashMap::<u64, usize>::new(), |mut counts, ordinal| {
            *counts.entry(ordinal).or_default() += 1;
            counts
        })
        .into_iter()
        .filter_map(|(ordinal, count)| (count > 1).then_some(ordinal))
        .collect::<HashSet<_>>();
    projected.retain(|(feature, _)| !duplicate_ordinals.contains(&feature.ordinal));
    projected.sort_by_key(|(feature, _)| feature.ordinal);
    let (features, result_topologies): (Vec<_>, Vec<_>) = projected.into_iter().unzip();
    FeatureProjection {
        unresolved_features: total.saturating_sub(features.len()),
        unresolved_states: features.len(),
        features,
        result_topologies,
    }
}

fn project_extrusion(
    source: &PmDcFeature,
    label: &PmDcFeatureLabel,
    index: &ProjectionIndex<'_>,
) -> Option<(Feature, FeatureResultTopology)> {
    let operation = enum16(source, 0, PmDcFeatureEnumFamily::PartOperation, index)?;
    let op = match operation {
        1 => BooleanOp::NewBody,
        2 => BooleanOp::Cut,
        3 => BooleanOp::Join,
        4 => BooleanOp::Intersect,
        _ => return None,
    };
    let boundary = references(source, 1, PmDcFeatureReferenceFamily::BoundaryPatch, index)?;
    if source.properties.references.get(23)? != source.properties.references.get(1)? {
        return None;
    }
    let selections = boundary
        .references
        .iter()
        .map(|reference| {
            let property = resolve_property(&source.segment_token, reference.index, index)?;
            let PmDcFeaturePropertyKind::ProfileSelection { entity_link, .. } = &property.kind
            else {
                return None;
            };
            let ordinal = entity_link.index.checked_sub(1)?;
            index
                .entity_style_links
                .contains(&(source.segment_token.as_str(), ordinal))
                .then(|| property.id.clone())
        })
        .collect::<Option<Vec<_>>>()?;
    if selections.is_empty() || label.participants.references.len() != 1 {
        return None;
    }
    let sketch_reference = label.participants.references.first()?;
    let sketch = index.sketches.get(&(
        source.segment_token.as_str(),
        sketch_reference.index.checked_sub(1)?,
    ))?;
    let sketch_id = index.sketch_ids.get(sketch.id.as_str())?.clone();

    let direction_record = resolve_direction(source, 2, index)?;
    let mut direction = Vector3::new(
        direction_record.direction[0],
        direction_record.direction[1],
        direction_record.direction[2],
    )
    .unit()?;
    if boolean(source, 3, index)? {
        direction = direction.scale(-1.0);
    }
    let length = length_parameter(source, 4, index)?;
    let taper = angle_parameter(source, 5, index)?;
    let termination = match enum16(source, 6, PmDcFeatureEnumFamily::Extent, index)? {
        1 if length.0 > 0.0 => LinearTermination::Blind { length },
        4 => LinearTermination::ThroughNext,
        5 => LinearTermination::ThroughAll,
        _ => return None,
    };
    let side = ExtrudeSide {
        termination,
        draft: (taper.0 != 0.0).then_some(taper),
    };
    let extent = if boolean(source, 7, index)? {
        ExtrudeExtent::Symmetric { side }
    } else {
        ExtrudeExtent::OneSided { side }
    };
    let (feature_id, result) = feature_result(source, 26, index)?;
    let feature = Feature {
        id: feature_id,
        ordinal: u64::from(label.index),
        name: Some(label.name.clone()),
        suppressed: None,
        dependencies: Vec::new(),
        source_properties: boolean_properties(source, &[20, 22], index),
        source_tag: Some("extrude".into()),
        source_text: None,
        source_content: Vec::new(),
        outputs: Vec::new(),
        definition: FeatureDefinition::Extrude {
            profile: ProfileRef::SketchSelection {
                sketch: sketch_id,
                selections,
            },
            direction: ExtrudeDirection::Explicit {
                vector: direction,
                source: Some(ExtrusionDirectionSource::Custom),
            },
            start: ExtrudeStart::ProfilePlane,
            extent,
            op,
            solid: Some(true),
            face_maker: None,
            inner_wire_taper: None,
            length_along_profile_normal: None,
            allow_multi_profile_faces: None,
        },
        native_ref: Some(source.id.clone()),
    };
    Some((feature, result))
}

fn project_fillet(
    source: &PmDcFeature,
    label: &PmDcFeatureLabel,
    index: &ProjectionIndex<'_>,
) -> Option<(Feature, FeatureResultTopology)> {
    if enum16(source, 11, PmDcFeatureEnumFamily::Fillet, index)? != 0
        || source.properties.references.get(1)?.index != 0
        || source.properties.references.get(10)?.index != 0
    {
        return None;
    }
    let sets = references(source, 0, PmDcFeatureReferenceFamily::FilletEdgeSets, index)?;
    let groups = sets
        .references
        .iter()
        .map(|reference| {
            let set = resolve_property(&source.segment_token, reference.index, index)?;
            let PmDcFeaturePropertyKind::FilletEdgeSet {
                edges,
                radius,
                selection,
                continuity,
            } = &set.kind
            else {
                return None;
            };
            let selection = resolve_property(&source.segment_token, selection.index, index)?;
            if !matches!(
                selection.kind,
                PmDcFeaturePropertyKind::WideEnumeration {
                    family: PmDcFeatureEnum32Family::FilletEdgeSelection,
                    type_value: 4,
                    value: 0
                }
            ) || !matches!(
                resolve_property(&source.segment_token, continuity.index, index)?.kind,
                PmDcFeaturePropertyKind::Boolean { value: false, .. }
            ) {
                return None;
            }
            let edge_collection = resolve_property(&source.segment_token, edges.index, index)?;
            let PmDcFeaturePropertyKind::References {
                family: PmDcFeatureReferenceFamily::EdgeCollection,
                items,
            } = &edge_collection.kind
            else {
                return None;
            };
            if !closed_edge_items(&source.segment_token, items, index) {
                return None;
            }
            Some(FilletGroup {
                edges: EdgeSelection::Native(edge_collection.id.clone()),
                radius: RadiusSpec::Constant {
                    radius: length_reference(&source.segment_token, radius.index, index)?,
                },
                tangency_weight: None,
            })
        })
        .collect::<Option<Vec<_>>>()?;
    if groups.is_empty() {
        return None;
    }
    let (feature_id, result) = feature_result(source, 15, index)?;
    Some((
        Feature {
            id: feature_id,
            ordinal: u64::from(label.index),
            name: Some(label.name.clone()),
            suppressed: None,
            dependencies: Vec::new(),
            source_properties: boolean_properties(source, &[2, 3, 4, 5, 8], index),
            source_tag: Some("fillet".into()),
            source_text: None,
            source_content: Vec::new(),
            outputs: Vec::new(),
            definition: FeatureDefinition::Fillet { groups },
            native_ref: Some(source.id.clone()),
        },
        result,
    ))
}

fn project_chamfer(
    source: &PmDcFeature,
    label: &PmDcFeatureLabel,
    index: &ProjectionIndex<'_>,
) -> Option<(Feature, FeatureResultTopology)> {
    if enum16(source, 4, PmDcFeatureEnumFamily::Chamfer, index)? != 0 {
        return None;
    }
    let edges = slot_property(source, 0, index)?;
    let PmDcFeaturePropertyKind::References {
        family: PmDcFeatureReferenceFamily::EdgeCollection,
        items,
    } = &edges.kind
    else {
        return None;
    };
    if !closed_edge_items(&source.segment_token, items, index) {
        return None;
    }
    let (feature_id, result) = feature_result(source, 11, index)?;
    Some((
        Feature {
            id: feature_id,
            ordinal: u64::from(label.index),
            name: Some(label.name.clone()),
            suppressed: None,
            dependencies: Vec::new(),
            source_properties: boolean_properties(source, &[6, 9], index),
            source_tag: Some("chamfer".into()),
            source_text: None,
            source_content: Vec::new(),
            outputs: Vec::new(),
            definition: FeatureDefinition::Chamfer {
                groups: vec![ChamferGroup {
                    edges: EdgeSelection::Native(edges.id.clone()),
                    spec: ChamferSpec::Distance {
                        distance: length_parameter(source, 2, index)?,
                    },
                }],
                flip_direction: boolean(source, 5, index)?,
            },
            native_ref: Some(source.id.clone()),
        },
        result,
    ))
}

fn project_hole(
    source: &PmDcFeature,
    label: &PmDcFeatureLabel,
    index: &ProjectionIndex<'_>,
) -> Option<(Feature, FeatureResultTopology)> {
    let hole_form = enum16(source, 0, PmDcFeatureEnumFamily::Hole, index)?;
    if boolean(source, 17, index)? {
        return None;
    }
    let diameter = length_parameter(source, 1, index)?;
    let depth = length_parameter(source, 2, index)?;
    let head_diameter = length_parameter(source, 3, index)?;
    let head_depth = length_parameter(source, 4, index)?;
    let head_angle = angle_parameter(source, 5, index)?;
    let point_angle = angle_parameter(source, 6, index)?;
    let kind = match hole_form {
        0 if point_angle.0 == 0.0 => HoleKind::Simple,
        0 => HoleKind::SimpleDrilled {
            drill_point_angle: point_angle,
        },
        1 => HoleKind::Countersink {
            diameter: head_diameter,
            angle: head_angle,
        },
        2 if point_angle.0 == 0.0 => HoleKind::Counterbore {
            diameter: head_diameter,
            depth: head_depth,
        },
        2 => HoleKind::CounterboreDrilled {
            diameter: head_diameter,
            depth: head_depth,
            drill_point_angle: point_angle,
        },
        _ => return None,
    };
    let extent = match enum16(source, 9, PmDcFeatureEnumFamily::Extent, index)? {
        1 if depth.0 > 0.0 => LinearTermination::Blind { length: depth },
        4 => LinearTermination::ThroughNext,
        5 => LinearTermination::ThroughAll,
        _ => return None,
    };
    let transform_reference = source.properties.references.get(8)?;
    let transform = index.transforms.get(&(
        source.segment_token.as_str(),
        transform_reference.index.checked_sub(1)?,
    ))?;
    if transform.matrix[3]
        .iter()
        .zip([0.0, 0.0, 0.0, 1.0])
        .any(|(actual, expected)| (actual - expected).abs() > EPS_FEATURE_PROJECT_HOLE_E10)
    {
        return None;
    }
    let direction_record = resolve_direction(source, 16, index)?;
    let direction = Vector3::new(
        direction_record.direction[0],
        direction_record.direction[1],
        direction_record.direction[2],
    )
    .unit()?;
    let placement = slot_property(source, 21, index)?;
    let PmDcFeaturePropertyKind::Placement {
        transform: placement_transform,
        point,
        value,
    } = &placement.kind
    else {
        return None;
    };
    if placement_transform.index != transform_reference.index
        || point.index == 0
        || value.index == 0
    {
        return None;
    }
    let (feature_id, result) = feature_result(source, 24, index)?;
    Some((
        Feature {
            id: feature_id,
            ordinal: u64::from(label.index),
            name: Some(label.name.clone()),
            suppressed: None,
            dependencies: Vec::new(),
            source_properties: BTreeMap::new(),
            source_tag: Some("hole".into()),
            source_text: None,
            source_content: Vec::new(),
            outputs: Vec::new(),
            definition: FeatureDefinition::Hole {
                profile: None,
                profile_filter: None,
                face: None,
                placements: Some(vec![HolePlacement::Directed {
                    position: Point3::new(
                        transform.matrix[0][3] * 10.0,
                        transform.matrix[1][3] * 10.0,
                        transform.matrix[2][3] * 10.0,
                    ),
                    direction,
                }]),
                construction: cadmpeg_ir::features::HoleConstruction::Form {
                    kind,
                    specification: None,
                },
                exit_kind: None,
                diameter: Some(diameter),
                extent: Some(extent),
                bottom: None,
                taper_angle: None,
                allow_multi_profile_faces: None,
            },
            native_ref: Some(source.id.clone()),
        },
        result,
    ))
}

fn feature_result(
    source: &PmDcFeature,
    slot: usize,
    index: &ProjectionIndex<'_>,
) -> Option<(FeatureId, FeatureResultTopology)> {
    let collection = slot_property(source, slot, index)?;
    let PmDcFeaturePropertyKind::References {
        family: PmDcFeatureReferenceFamily::ObjectCollection,
        items,
    } = &collection.kind
    else {
        return None;
    };
    let bodies = items
        .references
        .iter()
        .map(|reference| {
            let body = resolve_property(&source.segment_token, reference.index, index)?;
            matches!(body.kind, PmDcFeaturePropertyKind::SurfaceBody { .. })
                .then(|| body.id.clone())
        })
        .collect::<Option<Vec<_>>>()?;
    if bodies.is_empty() {
        return None;
    }
    let feature_id = FeatureId(format!(
        "inventor:design:feature#{}-{}",
        source.segment_token, source.record_ordinal
    ));
    let result = FeatureResultTopology {
        id: FeatureResultTopologyId::mint(format!(
            "inventor:design:feature-result#{}-{}",
            source.segment_token, source.record_ordinal
        ))
        .expect("identity grammar"),
        output_of: feature_id.clone(),
        bodies,
        faces: Vec::new(),
        edges: Vec::new(),
        vertices: Vec::new(),
        native_ref: Some(collection.id.clone()),
    };
    Some((feature_id, result))
}

fn closed_edge_items(token: &str, items: &PmDcReferenceList, index: &ProjectionIndex<'_>) -> bool {
    !items.references.is_empty()
        && items.references.iter().all(|reference| {
            resolve_property(token, reference.index, index).is_some_and(|property| {
                matches!(
                    &property.kind,
                    PmDcFeaturePropertyKind::EdgeItem {
                        index_references,
                        ..
                    } if !index_references.values.is_empty()
                )
            })
        })
}

fn slot_property<'a>(
    source: &PmDcFeature,
    slot: usize,
    index: &'a ProjectionIndex<'a>,
) -> Option<&'a PmDcFeatureProperty> {
    resolve_property(
        &source.segment_token,
        source.properties.references.get(slot)?.index,
        index,
    )
}

fn resolve_property<'a>(
    token: &str,
    reference: u32,
    index: &'a ProjectionIndex<'a>,
) -> Option<&'a PmDcFeatureProperty> {
    index
        .properties
        .get(&(token, reference.checked_sub(1)?))
        .copied()
}

fn references<'a>(
    source: &PmDcFeature,
    slot: usize,
    family: PmDcFeatureReferenceFamily,
    index: &'a ProjectionIndex<'a>,
) -> Option<&'a PmDcReferenceList> {
    match &slot_property(source, slot, index)?.kind {
        PmDcFeaturePropertyKind::References {
            family: actual,
            items,
        } if *actual == family => Some(items),
        _ => None,
    }
}

fn enum16(
    source: &PmDcFeature,
    slot: usize,
    family: PmDcFeatureEnumFamily,
    index: &ProjectionIndex<'_>,
) -> Option<u16> {
    let expected_type = match family {
        PmDcFeatureEnumFamily::PartOperation => 5,
        PmDcFeatureEnumFamily::Extent => 11,
        PmDcFeatureEnumFamily::Hole => 3,
        PmDcFeatureEnumFamily::Fillet | PmDcFeatureEnumFamily::Chamfer => 2,
        PmDcFeatureEnumFamily::Auxiliary => return None,
    };
    match slot_property(source, slot, index)?.kind {
        PmDcFeaturePropertyKind::Enumeration {
            family: actual,
            type_value,
            value,
        } if actual == family && type_value == expected_type => Some(value),
        _ => None,
    }
}

fn boolean(source: &PmDcFeature, slot: usize, index: &ProjectionIndex<'_>) -> Option<bool> {
    match slot_property(source, slot, index)?.kind {
        PmDcFeaturePropertyKind::Boolean { value, .. } => Some(value),
        _ => None,
    }
}

fn resolve_direction<'a>(
    source: &PmDcFeature,
    slot: usize,
    index: &'a ProjectionIndex<'a>,
) -> Option<&'a crate::sketch::PmDcDirection> {
    let reference = source.properties.references.get(slot)?;
    index
        .directions
        .get(&(
            source.segment_token.as_str(),
            reference.index.checked_sub(1)?,
        ))
        .copied()
}

fn length_parameter(
    source: &PmDcFeature,
    slot: usize,
    index: &ProjectionIndex<'_>,
) -> Option<Length> {
    length_reference(
        &source.segment_token,
        source.properties.references.get(slot)?.index,
        index,
    )
}

fn length_reference(token: &str, reference: u32, index: &ProjectionIndex<'_>) -> Option<Length> {
    let parameter = index.parameters.get(&(token, reference.checked_sub(1)?))?;
    match index.parameter_values.get(parameter.id.as_str())? {
        ParameterValue::Length(value) if value.0.is_finite() && value.0 >= 0.0 => Some(*value),
        _ => None,
    }
}

fn angle_parameter(
    source: &PmDcFeature,
    slot: usize,
    index: &ProjectionIndex<'_>,
) -> Option<Angle> {
    let reference = source.properties.references.get(slot)?;
    let parameter = index.parameters.get(&(
        source.segment_token.as_str(),
        reference.index.checked_sub(1)?,
    ))?;
    match index.parameter_values.get(parameter.id.as_str())? {
        ParameterValue::Angle(value) if value.0.is_finite() => Some(*value),
        _ => None,
    }
}

fn boolean_properties(
    source: &PmDcFeature,
    slots: &[usize],
    index: &ProjectionIndex<'_>,
) -> BTreeMap<String, String> {
    slots
        .iter()
        .filter_map(|slot| {
            boolean(source, *slot, index)
                .map(|value| (format!("property_{slot}_boolean"), value.to_string()))
        })
        .collect()
}

fn unique_by_key<'a, T, K: Eq + std::hash::Hash + Copy>(
    records: &'a [T],
    key: impl Fn(&'a T) -> K,
) -> HashMap<K, &'a T> {
    let mut unique = HashMap::new();
    let mut duplicate = HashSet::new();
    for record in records {
        let key = key(record);
        if unique.insert(key, record).is_some() {
            duplicate.insert(key);
        }
    }
    for key in duplicate {
        unique.remove(&key);
    }
    unique
}

#[cfg(test)]
mod tests {
    use super::*;
    use cadmpeg_core::decode::{DecodeArena, DecodePolicy};
    use cadmpeg_ir::features::{ParameterId, ParameterValue};
    use cadmpeg_ir::sketches::{SketchId, SketchPlacement};

    const SEGMENT: &str = "generated";

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

    fn reference(index: u32) -> crate::pmdc::PmDcReference {
        crate::pmdc::PmDcReference {
            index,
            qualified: index != 0,
        }
    }

    fn reference_list(values: &[u32]) -> PmDcReferenceList {
        PmDcReferenceList {
            marker: 2,
            metadata: (!values.is_empty())
                .then_some(crate::pmdc::PmDcListMetadata::U32([values.len() as u32, 0])),
            references: values.iter().copied().map(reference).collect(),
        }
    }

    fn test_header() -> PmDcContentHeader {
        PmDcContentHeader {
            header_value: 0,
            header_id: 0,
            next: reference(0),
            flags: 0,
            context: reference(0),
            source_index: 0,
        }
    }

    fn test_property(ordinal: u32, kind: PmDcFeaturePropertyKind) -> PmDcFeatureProperty {
        PmDcFeatureProperty {
            id: format!("inventor:pmdc:feature-property#{SEGMENT}-{ordinal}"),
            type_id: format!("{ordinal:032x}"),
            segment_token: SEGMENT.into(),
            record_ordinal: ordinal,
            save_version_major: 16,
            header: test_header(),
            kind,
        }
    }

    fn test_feature(ordinal: u32, slot_count: usize, slots: &[(usize, u32)]) -> PmDcFeature {
        let mut references = vec![reference(0); slot_count];
        for (slot, record_ordinal) in slots {
            references[*slot] = reference(record_ordinal + 1);
        }
        PmDcFeature {
            id: format!("inventor:pmdc:feature#{SEGMENT}-{ordinal}"),
            type_id: type_id_string(FEATURE_TYPE),
            segment_token: SEGMENT.into(),
            record_ordinal: ordinal,
            save_version_major: 16,
            header: test_header(),
            state: 69,
            outline_value: 0,
            properties: PmDcReferenceList {
                marker: 2,
                metadata: Some(crate::pmdc::PmDcListMetadata::U32([slot_count as u32, 0])),
                references,
            },
            value: 0,
        }
    }

    fn test_label(owner_ordinal: u32, index: u32, class_id: &str) -> PmDcFeatureLabel {
        PmDcFeatureLabel {
            id: format!("inventor:pmdc:feature-label#{SEGMENT}-{owner_ordinal}"),
            type_id: type_id_string(FEATURE_LABEL_TYPE),
            segment_token: SEGMENT.into(),
            record_ordinal: owner_ordinal + 1000,
            save_version_major: 16,
            header: PmDcLinkedHeader {
                header_value: 0,
                header_id: 0,
                values: [0; 2],
                owner: reference(owner_ordinal + 1),
                parent: reference(0),
                next: reference(0),
            },
            index,
            participants: reference_list(&[]),
            name: format!("Feature {index}"),
            class_id: class_id.into(),
        }
    }

    fn raw_parameter(ordinal: u32) -> crate::design::PmDcParameter {
        crate::design::PmDcParameter {
            id: format!("inventor:pmdc:parameter#{SEGMENT}-{ordinal}"),
            type_id: "264d8790d011f8d10008cabc0663dc09".into(),
            segment_token: SEGMENT.into(),
            record_ordinal: ordinal,
            save_version_major: 16,
            header: crate::pmdc::PmDcContentHeader {
                header_value: 0,
                header_id: 0,
                next: reference(0),
                flags: 0,
                context: reference(0),
                source_index: ordinal,
            },
            name: format!("p{ordinal}"),
            name_value: 0,
            unit: reference(0),
            formula: reference(0),
            nominal_value: 0.0,
            model_value: 0.0,
            tolerance: 0,
            terminal_value: 0,
        }
    }

    fn neutral_parameter(
        raw: &crate::design::PmDcParameter,
        value: ParameterValue,
    ) -> DesignParameter {
        DesignParameter {
            id: ParameterId(format!("inventor:design:parameter#{}", raw.record_ordinal)),
            owner: None,
            ordinal: raw.record_ordinal,
            name: raw.name.clone(),
            expression: String::new(),
            display: None,
            value: Some(value),
            dependencies: Vec::new(),
            properties: BTreeMap::new(),
            pmi: None,
            native_ref: Some(raw.id.clone()),
        }
    }

    // The fixture builder keeps each independently indexed record family explicit.
    #[allow(clippy::too_many_arguments)]
    fn test_projection_index<'a>(
        properties: &'a [PmDcFeatureProperty],
        parameters: &'a [crate::design::PmDcParameter],
        neutral_parameters: &'a [DesignParameter],
        sketches: &'a [crate::sketch::PmDcSketch],
        neutral_sketches: &'a [Sketch],
        directions: &'a [crate::sketch::PmDcDirection],
        transforms: &'a [crate::sketch::PmDcTransform],
        entity_style_links: &'a [PmDcEntityStyleLink],
    ) -> ProjectionIndex<'a> {
        ProjectionIndex {
            properties: properties
                .iter()
                .map(|record| {
                    (
                        (record.segment_token.as_str(), record.record_ordinal),
                        record,
                    )
                })
                .collect(),
            parameters: parameters
                .iter()
                .map(|record| {
                    (
                        (record.segment_token.as_str(), record.record_ordinal),
                        record,
                    )
                })
                .collect(),
            parameter_values: neutral_parameters
                .iter()
                .filter_map(|parameter| {
                    Some((parameter.native_ref.as_deref()?, parameter.value.as_ref()?))
                })
                .collect(),
            sketches: sketches
                .iter()
                .map(|record| {
                    (
                        (record.segment_token.as_str(), record.record_ordinal),
                        record,
                    )
                })
                .collect(),
            sketch_ids: neutral_sketches
                .iter()
                .filter_map(|sketch| {
                    sketch
                        .native_ref
                        .as_deref()
                        .map(|native| (native, sketch.id.clone()))
                })
                .collect(),
            directions: directions
                .iter()
                .map(|record| {
                    (
                        (record.segment_token.as_str(), record.record_ordinal),
                        record,
                    )
                })
                .collect(),
            transforms: transforms
                .iter()
                .map(|record| {
                    (
                        (record.segment_token.as_str(), record.record_ordinal),
                        record,
                    )
                })
                .collect(),
            entity_style_links: entity_style_links
                .iter()
                .map(|record| (record.segment_token.as_str(), record.record_ordinal))
                .collect(),
        }
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
    fn parses_generated_pattern_feature_branches() {
        let build = |version: u8, family: PmDcPatternFamily| {
            let mut bytes = content(21);
            bytes.extend_from_slice(&69u32.to_le_bytes());
            bytes.extend_from_slice(&3u32.to_le_bytes());
            bytes.extend_from_slice(&references(&[]));
            bytes.extend_from_slice(&7u32.to_le_bytes());
            bytes.extend_from_slice(&references(&[0x8000_0010, 0x8000_0011]));
            for index in 0..6 {
                bytes.extend_from_slice(&(0x8000_0020u32 + index).to_le_bytes());
            }
            bytes.push(1);
            match family {
                PmDcPatternFamily::Rectangular => {
                    let remaining = if version > 20 { 26 } else { 20 };
                    for index in 0..remaining {
                        bytes.extend_from_slice(&(0x8000_0040u32 + index).to_le_bytes());
                    }
                }
                PmDcPatternFamily::Mirror => {
                    for index in 0..5 {
                        bytes.extend_from_slice(&(0x8000_0040u32 + index).to_le_bytes());
                    }
                    if version > 20 {
                        for index in 0..6 {
                            bytes.extend_from_slice(&(0x100u32 + index).to_le_bytes());
                        }
                    }
                    for index in 0..2 {
                        bytes.extend_from_slice(&(0x8000_0050u32 + index).to_le_bytes());
                    }
                }
            }
            bytes
        };

        for (version, family, slots, extensions) in [
            (16, PmDcPatternFamily::Rectangular, 26, 0),
            (21, PmDcPatternFamily::Rectangular, 32, 0),
            (16, PmDcPatternFamily::Mirror, 13, 0),
            (21, PmDcPatternFamily::Mirror, 13, 6),
        ] {
            let bytes = build(version, family);
            let parsed = parse(&bytes, |ctx, source| {
                parse_pattern_feature(ctx, source, version, family).expect("pattern feature")
            });
            assert_eq!(parsed.family, family);
            assert_eq!(parsed.participants.references.len(), 2);
            assert_eq!(parsed.property_slots.len(), slots);
            assert_eq!(parsed.extension_values.len(), extensions);
        }
    }

    #[test]
    fn projects_generated_fillet_and_chamfer() {
        let raw_radius = raw_parameter(20);
        let neutral_radius = neutral_parameter(&raw_radius, ParameterValue::Length(Length(2.5)));
        let fillet_properties = vec![
            test_property(
                1,
                PmDcFeaturePropertyKind::Enumeration {
                    family: PmDcFeatureEnumFamily::Fillet,
                    type_value: 2,
                    value: 0,
                },
            ),
            test_property(
                2,
                PmDcFeaturePropertyKind::References {
                    family: PmDcFeatureReferenceFamily::FilletEdgeSets,
                    items: reference_list(&[4]),
                },
            ),
            test_property(
                3,
                PmDcFeaturePropertyKind::FilletEdgeSet {
                    edges: reference(5),
                    radius: reference(21),
                    selection: reference(6),
                    continuity: reference(7),
                },
            ),
            test_property(
                4,
                PmDcFeaturePropertyKind::References {
                    family: PmDcFeatureReferenceFamily::EdgeCollection,
                    items: reference_list(&[8]),
                },
            ),
            test_property(
                5,
                PmDcFeaturePropertyKind::WideEnumeration {
                    family: PmDcFeatureEnum32Family::FilletEdgeSelection,
                    type_value: 4,
                    value: 0,
                },
            ),
            test_property(
                6,
                PmDcFeaturePropertyKind::Boolean {
                    name: String::new(),
                    name_value: 0,
                    value: false,
                },
            ),
            test_property(
                7,
                PmDcFeaturePropertyKind::EdgeItem {
                    index_references: PmDcU32List {
                        marker: 2,
                        metadata: Some(crate::pmdc::PmDcListMetadata::U32([1, 0])),
                        values: vec![42],
                    },
                    index_reference_value: 0,
                    value: 0,
                },
            ),
            test_property(
                8,
                PmDcFeaturePropertyKind::References {
                    family: PmDcFeatureReferenceFamily::ObjectCollection,
                    items: reference_list(&[10]),
                },
            ),
            test_property(
                9,
                PmDcFeaturePropertyKind::SurfaceBody {
                    body: reference(30),
                },
            ),
        ];
        let fillet = test_feature(100, 16, &[(0, 2), (11, 1), (15, 8)]);
        let label = test_label(100, 7, FILLET_CLASS_ID);
        let index = test_projection_index(
            &fillet_properties,
            std::slice::from_ref(&raw_radius),
            std::slice::from_ref(&neutral_radius),
            &[],
            &[],
            &[],
            &[],
            &[],
        );
        let (projected, result) = project_fillet(&fillet, &label, &index).expect("fillet");
        assert!(matches!(
            projected.definition,
            FeatureDefinition::Fillet { groups }
                if matches!(groups[0].radius, RadiusSpec::Constant { radius: Length(2.5) })
        ));
        assert_eq!(result.bodies, vec![fillet_properties[8].id.clone()]);

        let raw_distance = raw_parameter(40);
        let neutral_distance =
            neutral_parameter(&raw_distance, ParameterValue::Length(Length(1.25)));
        let chamfer_properties = vec![
            test_property(
                31,
                PmDcFeaturePropertyKind::References {
                    family: PmDcFeatureReferenceFamily::EdgeCollection,
                    items: reference_list(&[34]),
                },
            ),
            test_property(
                32,
                PmDcFeaturePropertyKind::Enumeration {
                    family: PmDcFeatureEnumFamily::Chamfer,
                    type_value: 2,
                    value: 0,
                },
            ),
            test_property(
                33,
                PmDcFeaturePropertyKind::EdgeItem {
                    index_references: PmDcU32List {
                        marker: 2,
                        metadata: Some(crate::pmdc::PmDcListMetadata::U32([1, 0])),
                        values: vec![17],
                    },
                    index_reference_value: -1,
                    value: 0,
                },
            ),
            test_property(
                34,
                PmDcFeaturePropertyKind::Boolean {
                    name: String::new(),
                    name_value: 0,
                    value: true,
                },
            ),
            test_property(
                35,
                PmDcFeaturePropertyKind::References {
                    family: PmDcFeatureReferenceFamily::ObjectCollection,
                    items: reference_list(&[37]),
                },
            ),
            test_property(
                36,
                PmDcFeaturePropertyKind::SurfaceBody {
                    body: reference(30),
                },
            ),
        ];
        let chamfer = test_feature(101, 12, &[(0, 31), (2, 40), (4, 32), (5, 34), (11, 35)]);
        let label = test_label(101, 8, CHAMFER_CLASS_ID);
        let index = test_projection_index(
            &chamfer_properties,
            std::slice::from_ref(&raw_distance),
            std::slice::from_ref(&neutral_distance),
            &[],
            &[],
            &[],
            &[],
            &[],
        );
        let (projected, _) = project_chamfer(&chamfer, &label, &index).expect("chamfer");
        assert!(matches!(
            projected.definition,
            FeatureDefinition::Chamfer {
                groups,
                flip_direction: true
            } if matches!(groups[0].spec, ChamferSpec::Distance { distance: Length(1.25) })
        ));
    }

    #[test]
    fn projects_generated_extrusion() {
        let raw_length = raw_parameter(70);
        let raw_taper = raw_parameter(71);
        let neutral_parameters = vec![
            neutral_parameter(&raw_length, ParameterValue::Length(Length(12.0))),
            neutral_parameter(&raw_taper, ParameterValue::Angle(Angle(0.1))),
        ];
        let raw_sketch = crate::sketch::PmDcSketch {
            id: format!("inventor:pmdc:sketch#{SEGMENT}-50"),
            type_id: "114d8790d011f8d10008cabc0663dc09".into(),
            segment_token: SEGMENT.into(),
            record_ordinal: 50,
            save_version_major: 16,
            header: test_header(),
            state: 0,
            count_value: 0,
            entities: PmDcReferenceList {
                marker: 8,
                metadata: None,
                references: Vec::new(),
            },
            transform: reference(0),
            direction: reference(0),
            values: [0; 2],
            auxiliary: None,
        };
        let neutral_sketch = Sketch {
            id: SketchId(format!("inventor:design:sketch#{SEGMENT}-50")),
            name: None,
            configuration: None,
            visible: None,
            placement: SketchPlacement::Resolved {
                origin: Point3::new(0.0, 0.0, 0.0),
                normal: Vector3::new(0.0, 0.0, 1.0),
                u_axis: Vector3::new(1.0, 0.0, 0.0),
            },
            profiles: Vec::new(),
            native_ref: Some(raw_sketch.id.clone()),
        };
        let direction = crate::sketch::PmDcDirection {
            id: format!("inventor:pmdc:direction#{SEGMENT}-60"),
            type_id: "40df52ced011d0d20008ccbc0663dc09".into(),
            segment_token: SEGMENT.into(),
            record_ordinal: 60,
            save_version_major: 16,
            header: test_header(),
            entity_flags: 0,
            parameter: 0.0,
            extension: None,
            direction: [0.0, 0.0, 1.0],
        };
        let entity_link = PmDcEntityStyleLink {
            id: format!("inventor:pmdc:entity-style-link#{SEGMENT}-51"),
            type_id: type_id_string(ENTITY_STYLE_LINK_TYPE),
            segment_token: SEGMENT.into(),
            record_ordinal: 51,
            save_version_major: 16,
            header: PmDcLinkedHeader {
                header_value: 0,
                header_id: 0,
                values: [0; 2],
                owner: reference(0),
                parent: reference(0),
                next: reference(0),
            },
            value: 0,
            associative_id: 1,
            entity_type: 1,
        };
        let properties = vec![
            test_property(
                1,
                PmDcFeaturePropertyKind::Enumeration {
                    family: PmDcFeatureEnumFamily::PartOperation,
                    type_value: 5,
                    value: 1,
                },
            ),
            test_property(
                2,
                PmDcFeaturePropertyKind::References {
                    family: PmDcFeatureReferenceFamily::BoundaryPatch,
                    items: reference_list(&[4]),
                },
            ),
            test_property(
                3,
                PmDcFeaturePropertyKind::ProfileSelection {
                    entity_link: reference(52),
                    value: 0,
                },
            ),
            test_property(
                4,
                PmDcFeaturePropertyKind::Boolean {
                    name: String::new(),
                    name_value: 0,
                    value: true,
                },
            ),
            test_property(
                5,
                PmDcFeaturePropertyKind::Enumeration {
                    family: PmDcFeatureEnumFamily::Extent,
                    type_value: 11,
                    value: 1,
                },
            ),
            test_property(
                6,
                PmDcFeaturePropertyKind::Boolean {
                    name: String::new(),
                    name_value: 0,
                    value: false,
                },
            ),
            test_property(
                7,
                PmDcFeaturePropertyKind::References {
                    family: PmDcFeatureReferenceFamily::ObjectCollection,
                    items: reference_list(&[9]),
                },
            ),
            test_property(
                8,
                PmDcFeaturePropertyKind::SurfaceBody {
                    body: reference(30),
                },
            ),
        ];
        let feature = test_feature(
            100,
            27,
            &[
                (0, 1),
                (1, 2),
                (2, 60),
                (3, 4),
                (4, 70),
                (5, 71),
                (6, 5),
                (7, 6),
                (23, 2),
                (26, 7),
            ],
        );
        let mut label = test_label(100, 5, EXTRUSION_CLASS_ID);
        label.participants = reference_list(&[51]);
        let raw_parameters = vec![raw_length, raw_taper];
        let index = test_projection_index(
            &properties,
            &raw_parameters,
            &neutral_parameters,
            std::slice::from_ref(&raw_sketch),
            std::slice::from_ref(&neutral_sketch),
            std::slice::from_ref(&direction),
            &[],
            std::slice::from_ref(&entity_link),
        );
        let (projected, _) = project_extrusion(&feature, &label, &index).expect("extrusion");
        assert!(matches!(
            projected.definition,
            FeatureDefinition::Extrude {
                direction: ExtrudeDirection::Explicit {
                    vector: Vector3 { z: -1.0, .. },
                    ..
                },
                extent: ExtrudeExtent::OneSided {
                    side: ExtrudeSide {
                        termination: LinearTermination::Blind {
                            length: Length(12.0)
                        },
                        draft: Some(Angle(0.1)),
                        ..
                    }
                },
                op: BooleanOp::NewBody,
                ..
            }
        ));
    }

    #[test]
    fn projects_generated_hole() {
        let raw_parameters = (70..76).map(raw_parameter).collect::<Vec<_>>();
        let neutral_parameters = [
            ParameterValue::Length(Length(5.0)),
            ParameterValue::Length(Length(20.0)),
            ParameterValue::Length(Length(9.0)),
            ParameterValue::Length(Length(3.0)),
            ParameterValue::Angle(Angle(1.5)),
            ParameterValue::Angle(Angle(2.0)),
        ]
        .into_iter()
        .zip(&raw_parameters)
        .map(|(value, raw)| neutral_parameter(raw, value))
        .collect::<Vec<_>>();
        let transform = crate::sketch::PmDcTransform {
            id: format!("inventor:pmdc:transform#{SEGMENT}-60"),
            type_id: "184d8790d011f8d10008cabc0663dc09".into(),
            segment_token: SEGMENT.into(),
            record_ordinal: 60,
            save_version_major: 16,
            header: test_header(),
            prefix: None,
            value_mask: 0,
            zero_mask: 0,
            matrix: [
                [1.0, 0.0, 0.0, 1.0],
                [0.0, 1.0, 0.0, 2.0],
                [0.0, 0.0, 1.0, 3.0],
                [0.0, 0.0, 0.0, 1.0],
            ],
        };
        let direction = crate::sketch::PmDcDirection {
            id: format!("inventor:pmdc:direction#{SEGMENT}-61"),
            type_id: "40df52ced011d0d20008ccbc0663dc09".into(),
            segment_token: SEGMENT.into(),
            record_ordinal: 61,
            save_version_major: 16,
            header: test_header(),
            entity_flags: 0,
            parameter: 0.0,
            extension: None,
            direction: [0.0, 0.0, -1.0],
        };
        let properties = vec![
            test_property(
                1,
                PmDcFeaturePropertyKind::Enumeration {
                    family: PmDcFeatureEnumFamily::Hole,
                    type_value: 3,
                    value: 2,
                },
            ),
            test_property(
                2,
                PmDcFeaturePropertyKind::Enumeration {
                    family: PmDcFeatureEnumFamily::Extent,
                    type_value: 11,
                    value: 5,
                },
            ),
            test_property(
                3,
                PmDcFeaturePropertyKind::Boolean {
                    name: String::new(),
                    name_value: 0,
                    value: false,
                },
            ),
            test_property(
                4,
                PmDcFeaturePropertyKind::Placement {
                    transform: reference(61),
                    point: reference(90),
                    value: reference(91),
                },
            ),
            test_property(
                5,
                PmDcFeaturePropertyKind::References {
                    family: PmDcFeatureReferenceFamily::ObjectCollection,
                    items: reference_list(&[7]),
                },
            ),
            test_property(
                6,
                PmDcFeaturePropertyKind::SurfaceBody {
                    body: reference(30),
                },
            ),
        ];
        let feature = test_feature(
            100,
            25,
            &[
                (0, 1),
                (1, 70),
                (2, 71),
                (3, 72),
                (4, 73),
                (5, 74),
                (6, 75),
                (8, 60),
                (9, 2),
                (16, 61),
                (17, 3),
                (21, 4),
                (24, 5),
            ],
        );
        let label = test_label(100, 9, HOLE_CLASS_ID);
        let index = test_projection_index(
            &properties,
            &raw_parameters,
            &neutral_parameters,
            &[],
            &[],
            std::slice::from_ref(&direction),
            std::slice::from_ref(&transform),
            &[],
        );
        let (projected, _) = project_hole(&feature, &label, &index).expect("hole");
        assert!(matches!(
            projected.definition,
            FeatureDefinition::Hole {
                placements,
                construction: cadmpeg_ir::features::HoleConstruction::Form {
                    kind: HoleKind::CounterboreDrilled {
                        diameter: Length(9.0),
                        depth: Length(3.0),
                        drill_point_angle: Angle(2.0)
                    },
                    ..
                },
                diameter: Some(Length(5.0)),
                extent: Some(LinearTermination::ThroughAll),
                ..
            } if matches!(
                placements.as_deref(),
                Some([HolePlacement::Directed {
                    position: Point3 { x: 10.0, y: 20.0, z: 30.0 },
                    direction: Vector3 { z: -1.0, .. }
                }])
            )
        ));
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

        let mut chamfer = content(10);
        chamfer.extend_from_slice(&2i16.to_le_bytes());
        chamfer.extend_from_slice(&0u16.to_le_bytes());
        chamfer.extend_from_slice(&0u32.to_le_bytes());
        let parsed = parse(&chamfer, |ctx, source| {
            parse_chamfer(ctx, source, 16).expect("chamfer enumeration")
        });
        assert!(matches!(
            parsed.kind,
            PmDcFeaturePropertyKind::Enumeration {
                family: PmDcFeatureEnumFamily::Chamfer,
                type_value: 2,
                value: 0
            }
        ));

        let mut fillet_selection = content(10);
        fillet_selection.extend_from_slice(&4u32.to_le_bytes());
        fillet_selection.extend_from_slice(&0u32.to_le_bytes());
        let parsed = parse(&fillet_selection, |ctx, source| {
            parse_fillet_edge_selection(ctx, source, 16).expect("fillet edge selection")
        });
        assert!(matches!(
            parsed.kind,
            PmDcFeaturePropertyKind::WideEnumeration {
                family: PmDcFeatureEnum32Family::FilletEdgeSelection,
                type_value: 4,
                value: 0
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

        let mut fillet_set = content(18);
        fillet_set.extend_from_slice(&0x8000_0010u32.to_le_bytes());
        fillet_set.extend_from_slice(&0x8000_0011u32.to_le_bytes());
        fillet_set.extend_from_slice(&0x8000_0012u32.to_le_bytes());
        fillet_set.extend_from_slice(&0x8000_0013u32.to_le_bytes());
        let parsed = parse(&fillet_set, |ctx, source| {
            parse_fillet_edge_set(ctx, source, 16).expect("fillet edge set")
        });
        assert!(matches!(
            parsed.kind,
            PmDcFeaturePropertyKind::FilletEdgeSet { radius, .. } if radius.index == 17
        ));

        let mut edge_item = content(18);
        edge_item.extend_from_slice(&2u16.to_le_bytes());
        edge_item.extend_from_slice(&0x3000u16.to_le_bytes());
        edge_item.extend_from_slice(&1u32.to_le_bytes());
        edge_item.extend_from_slice(&[1u32.to_le_bytes(), 0u32.to_le_bytes()].concat());
        edge_item.extend_from_slice(&42u32.to_le_bytes());
        edge_item.extend_from_slice(&0i32.to_le_bytes());
        edge_item.extend_from_slice(&7u32.to_le_bytes());
        let parsed = parse(&edge_item, |ctx, source| {
            parse_edge_item(ctx, source, 16).expect("edge item")
        });
        assert!(matches!(
            parsed.kind,
            PmDcFeaturePropertyKind::EdgeItem {
                index_reference_value: 0,
                value: 7,
                ..
            }
        ));

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
