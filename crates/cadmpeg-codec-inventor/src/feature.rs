// SPDX-License-Identifier: Apache-2.0
//! Typed `PmDc` feature records and feature-list terminators.
use cadmpeg_ir::features::{PlanarProfileRef, ProfileRef};

use std::collections::{BTreeMap, HashMap, HashSet};

use cadmpeg_core::decode::cost::DecodeCost;
use cadmpeg_core::decode::{DecodeContext, View};
use cadmpeg_core::text::NonBlankString;
use cadmpeg_core::CodecError;
use cadmpeg_ir::ids::FeatureResultTopologyId;
use cadmpeg_ir::math::{Point3, Vector3};
use cadmpeg_ir::sketches::Sketch;
use cadmpeg_ir::{
    features::{
        edge_treatments::{ChamferGroup, ChamferSpec, FilletGroup, RadiusSpec},
        holes::{HoleKind, HolePlacement},
        BooleanOp, DesignParameter, DistinctMembers, EdgeSelection, ExtrudeDirection,
        ExtrudeExtent, ExtrudeSide, ExtrudeStart, ExtrusionDirectionSource, Feature,
        FeatureContent, FeatureDefinition, FeatureId, FeatureOperation, FeatureResultTopology,
        LinearTermination, ParameterValue,
    },
    scalar::{Angle, Length},
};
use serde::{Deserialize, Serialize};

use crate::pmdc::{
    content_header, inventor_id, reference_list, type_id_string, u32_list, Cursor,
    PmDcContentHeader, PmDcReferenceList, PmDcU32List,
};
use crate::record_identity::{push_record, Located, RecordPayload};
use crate::record_issue::{RecordIssue, RecordIssueFamily};
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

#[derive(Debug)]
pub(crate) struct FeatureInventory {
    pub(crate) features: Vec<PmDcFeature>,
    pub(crate) pattern_features: Vec<PmDcPatternFeature>,
    pub(crate) terminators: Vec<PmDcFeatureTerminator>,
    pub(crate) properties: Vec<PmDcFeatureProperty>,
    pub(crate) labels: Vec<PmDcFeatureLabel>,
    pub(crate) entity_style_links: Vec<PmDcEntityStyleLink>,
    pub(crate) issues: Vec<RecordIssue>,
}

pub(crate) struct FeatureProjection {
    pub(crate) features: Vec<Feature>,
    pub(crate) result_topologies: Vec<FeatureResultTopology>,
    pub(crate) unresolved_features: usize,
    pub(crate) unresolved_states: usize,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub(crate) struct PmDcFeaturePropertyPayload {
    save_version_major: u8,
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
enum PmDcPatternFamily {
    Rectangular,
    Mirror,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct PmDcPatternFeaturePayload {
    save_version_major: u8,
    pub(crate) header: PmDcContentHeader,
    state: i32,
    outline_value: u32,
    pub(crate) properties: PmDcReferenceList,
    value: u32,
    pub(crate) participants: PmDcReferenceList,
    family: PmDcPatternFamily,
    pub(crate) property_slots: Vec<crate::pmdc::PmDcReference>,
    control: u8,
    extension_values: Vec<u32>,
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
    header_value: u32,
    header_id: u16,
    values: [u32; 2],
    pub(crate) owner: crate::pmdc::PmDcReference,
    pub(crate) parent: crate::pmdc::PmDcReference,
    pub(crate) next: crate::pmdc::PmDcReference,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct ClassId([u8; 16]);

impl DecodeCost for ClassId {
    const FIXED_BYTES: Option<u64> = Some(16);

    fn decode_cost(
        &self,
        ctx: &DecodeContext<'_>,
        operation: &'static str,
    ) -> Result<u64, CodecError> {
        self.0.decode_cost(ctx, operation)
    }
}

impl std::fmt::Display for ClassId {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        cadmpeg_ir::hash::LowerHex(&self.0).fmt(formatter)
    }
}

impl Serialize for ClassId {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.collect_str(self)
    }
}

impl ClassId {
    fn from_text(ctx: &DecodeContext<'_>, value: &str) -> Result<Self, CodecError> {
        let valid = value.len() == 32
            && ctx
                .admit_iter(value.as_bytes(), "validate Inventor feature class identity")?
                .all(|digit| digit.is_ascii_digit() || (b'a'..=b'f').contains(digit));
        if !valid {
            return Err(CodecError::Malformed(ctx.copy_retained_text(
                "class_id must contain 32 lowercase hexadecimal digits",
                "retain Inventor invalid feature class identity",
            )?));
        }
        let mut bytes = [0; 16];
        for (index, digit) in ctx
            .admit_iter(value.as_bytes(), "decode Inventor feature class identity")?
            .enumerate()
        {
            let nibble = if digit.is_ascii_digit() {
                *digit - b'0'
            } else {
                *digit - b'a' + 10
            };
            if index % 2 == 0 {
                bytes[index / 2] = nibble << 4;
            } else {
                bytes[index / 2] |= nibble;
            }
        }
        Ok(Self(bytes))
    }

    #[cfg(test)]
    fn into_text(
        self,
        ctx: &DecodeContext<'_>,
        operation: &'static str,
    ) -> Result<String, CodecError> {
        type_id_string(ctx, self.0, operation)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct PmDcFeatureLabelPayload {
    save_version_major: u8,
    pub(crate) header: PmDcLinkedHeader,
    index: u32,
    pub(crate) participants: PmDcReferenceList,
    name: NonBlankString,
    class_id: ClassId,
}

#[derive(Serialize)]
struct PmDcFeatureLabelPayloadRef<'a> {
    save_version_major: u8,
    header: &'a PmDcLinkedHeader,
    index: u32,
    participants: &'a PmDcReferenceList,
    name: &'a str,
    class_id: &'a ClassId,
}

impl Serialize for PmDcFeatureLabelPayload {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        PmDcFeatureLabelPayloadRef {
            save_version_major: self.save_version_major,
            header: &self.header,
            index: self.index,
            participants: &self.participants,
            name: self.name.as_str(),
            class_id: &self.class_id,
        }
        .serialize(serializer)
    }
}

#[derive(Serialize, Deserialize)]
pub(crate) struct PmDcFeatureLabelPayloadWire<T = String> {
    save_version_major: u8,
    header: PmDcLinkedHeader,
    index: u32,
    participants: PmDcReferenceList,
    name: T,
    class_id: String,
}

impl PmDcFeatureLabelPayloadWire {
    pub(crate) fn into_record(
        self,
        ctx: &DecodeContext<'_>,
    ) -> Result<PmDcFeatureLabelPayload, CodecError> {
        let name = ctx.validate_nonblank_text(self.name, "validate Inventor feature label name")?;
        let Ok(name) = NonBlankString::try_from(name) else {
            return Err(CodecError::Malformed(ctx.copy_retained_text(
                "name must not be empty",
                "retain Inventor invalid feature label name",
            )?));
        };
        let class_id = ClassId::from_text(ctx, &self.class_id)?;
        Ok(PmDcFeatureLabelPayload {
            save_version_major: self.save_version_major,
            header: self.header,
            index: self.index,
            participants: self.participants,
            name,
            class_id,
        })
    }
}

#[cfg(test)]
impl PmDcFeatureLabelPayload {
    fn into_wire(self, ctx: &DecodeContext<'_>) -> Result<PmDcFeatureLabelPayloadWire, CodecError> {
        let name = ctx.copy_retained_text(
            self.name.as_str(),
            "copy Inventor feature label name for test wire",
        )?;
        let class_id = self
            .class_id
            .into_text(ctx, "retain Inventor feature label class id")?;
        Ok(PmDcFeatureLabelPayloadWire {
            save_version_major: self.save_version_major,
            header: self.header,
            index: self.index,
            participants: self.participants,
            name,
            class_id,
        })
    }
}

impl PmDcFeatureLabelPayload {
    pub(crate) fn class_id(&self) -> ClassId {
        self.class_id
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct PmDcEntityStyleLinkPayload {
    save_version_major: u8,
    pub(crate) header: PmDcLinkedHeader,
    value: u32,
    associative_id: u32,
    entity_type: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct PmDcFeaturePayload {
    save_version_major: u8,
    pub(crate) header: PmDcContentHeader,
    state: i32,
    outline_value: u32,
    pub(crate) properties: PmDcReferenceList,
    value: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct PmDcFeatureTerminatorPayload {
    save_version_major: u8,
    pub(crate) header: PmDcContentHeader,
    state: i32,
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
    for segment in ctx.admit_iter(&document.segments, "visit Inventor feature items")? {
        if !ctx.equal(
            &segment.kind,
            &SegmentKind::PmDc,
            "filter Inventor PmDc feature segments",
        )? {
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
        let RecordFrameState::Framed(table) = &bulk.records else {
            continue;
        };
        for record in ctx.admit_iter(&table.records, "visit Inventor feature items")? {
            let parsed = match record.type_id {
                FEATURE_TYPE => parse_feature(ctx, record.payload, version).and_then(|feature| {
                    push_record(
                        ctx,
                        &mut inventory.features,
                        feature,
                        record.type_id,
                        segment.pair.token.key(),
                        record.ordinal,
                        "admit Inventor PmDc feature record",
                    )
                }),
                RECTANGULAR_PATTERN_FEATURE_TYPE | MIRROR_FEATURE_TYPE => {
                    let family = if record.type_id == RECTANGULAR_PATTERN_FEATURE_TYPE {
                        PmDcPatternFamily::Rectangular
                    } else {
                        PmDcPatternFamily::Mirror
                    };
                    parse_pattern_feature(ctx, record.payload, version, family).and_then(
                        |feature| {
                            push_record(
                                ctx,
                                &mut inventory.pattern_features,
                                feature,
                                record.type_id,
                                segment.pair.token.key(),
                                record.ordinal,
                                "admit Inventor PmDc pattern feature record",
                            )
                        },
                    )
                }
                END_OF_FEATURES_TYPE => {
                    parse_terminator(record.payload, version).and_then(|terminator| {
                        push_record(
                            ctx,
                            &mut inventory.terminators,
                            terminator,
                            record.type_id,
                            segment.pair.token.key(),
                            record.ordinal,
                            "admit Inventor PmDc feature terminator record",
                        )
                    })
                }
                FEATURE_LABEL_TYPE => parse_label(ctx, record.payload, version).and_then(|label| {
                    push_record(
                        ctx,
                        &mut inventory.labels,
                        label,
                        record.type_id,
                        segment.pair.token.key(),
                        record.ordinal,
                        "admit Inventor PmDc feature label record",
                    )
                }),
                ENTITY_STYLE_LINK_TYPE => parse_entity_style_link(record.payload, version)
                    .and_then(|link| {
                        push_record(
                            ctx,
                            &mut inventory.entity_style_links,
                            link,
                            record.type_id,
                            segment.pair.token.key(),
                            record.ordinal,
                            "admit Inventor PmDc entity style link record",
                        )
                    }),
                type_id => match feature_property_parser(ctx, type_id, record.payload, version) {
                    Ok(Some(property)) => push_record(
                        ctx,
                        &mut inventory.properties,
                        property,
                        record.type_id,
                        segment.pair.token.key(),
                        record.ordinal,
                        "admit Inventor PmDc feature property record",
                    ),
                    Ok(None) => Ok(()),
                    Err(error) => Err(error),
                },
            };
            if let Err(error) = parsed {
                if matches!(error, CodecError::ResourceLimit(_)) {
                    return Err(error);
                }
                ctx.charge_entities(1, "admit Inventor PmDc feature issue")?;
                ctx.push_vec(
                    &mut inventory.issues,
                    RecordIssue {
                        family: RecordIssueFamily::Feature {
                            type_id: crate::record_identity::RecordTypeId::from_bytes(
                                ctx,
                                record.type_id,
                                "retain Inventor PmDc feature issue type id",
                            )?,
                        },
                        segment_token: segment.pair.token.key().try_clone_for_decode(
                            ctx,
                            "retain Inventor PmDc feature issue segment token",
                        )?,
                        record_ordinal: record.ordinal,
                        detail: crate::issue_detail(
                            ctx,
                            error,
                            "retain Inventor PmDc feature issue detail",
                        )?,
                    },
                    "admit Inventor PmDc feature issue",
                )?;
            }
        }
    }
    Ok(inventory)
}

fn parse_pattern_feature(
    ctx: &DecodeContext<'_>,
    source: View<'_>,
    version: u8,
    family: PmDcPatternFamily,
) -> Result<PmDcPatternFeaturePayload, CodecError> {
    let mut cursor = Cursor::new(source);
    let header = content_header(&mut cursor)?;
    let state = cursor.u32("pattern-feature state")?.cast_signed();
    let outline_value = cursor.u32("pattern-feature outline value")?;
    let properties = reference_list(ctx, &mut cursor, 2, "pattern-feature properties")?;
    let value = cursor.u32("pattern-feature value")?;
    let participants = reference_list(ctx, &mut cursor, 2, "pattern-feature participants")?;
    let mut property_slots = Vec::new();
    for _ in 0..6 {
        ctx.push_vec(
            &mut property_slots,
            cursor.reference("pattern-feature property slot")?,
            "admit Inventor pattern feature property slots",
        )?;
    }
    let control = cursor.u8("pattern-feature control")?;
    let mut extension_values = Vec::new();
    match family {
        PmDcPatternFamily::Rectangular => {
            let remaining = if version > 20 { 26 } else { 20 };
            for _ in 0..remaining {
                ctx.push_vec(
                    &mut property_slots,
                    cursor.reference("pattern-feature property slot")?,
                    "admit Inventor pattern feature property slots",
                )?;
            }
        }
        PmDcPatternFamily::Mirror => {
            for _ in 0..5 {
                ctx.push_vec(
                    &mut property_slots,
                    cursor.reference("pattern-feature property slot")?,
                    "admit Inventor pattern feature property slots",
                )?;
            }
            if version > 20 {
                ctx.charge_collection_items(6, "admit Inventor pattern feature extension values")?;
                ctx.reserve_capacity(
                    &mut extension_values,
                    6,
                    "admit Inventor pattern feature extension values",
                )?;
                for _ in 0..6 {
                    extension_values.push(cursor.u32("pattern-feature extension value")?);
                }
            }
            for _ in 0..2 {
                ctx.push_vec(
                    &mut property_slots,
                    cursor.reference("pattern-feature property slot")?,
                    "admit Inventor pattern feature property slots",
                )?;
            }
        }
    }
    cursor.finish("pattern feature")?;
    Ok(PmDcPatternFeaturePayload {
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
) -> Result<PmDcFeaturePayload, CodecError> {
    let mut cursor = Cursor::new(source);
    let header = content_header(&mut cursor)?;
    let state = cursor.u32("feature state")?.cast_signed();
    let outline_value = cursor.u32("feature outline value")?;
    let properties = reference_list(ctx, &mut cursor, 2, "feature property list")?;
    let value = cursor.u32("feature value")?;
    cursor.finish("feature")?;
    Ok(PmDcFeaturePayload {
        save_version_major: version,
        header,
        state,
        outline_value,
        properties,
        value,
    })
}

fn parse_terminator(
    source: View<'_>,
    version: u8,
) -> Result<PmDcFeatureTerminatorPayload, CodecError> {
    let mut cursor = Cursor::new(source);
    let header = content_header(&mut cursor)?;
    let state = cursor.u32("feature terminator state")?.cast_signed();
    cursor.finish("feature terminator")?;
    Ok(PmDcFeatureTerminatorPayload {
        save_version_major: version,
        header,
        state,
    })
}

fn feature_property_parser(
    ctx: &DecodeContext<'_>,
    type_id: [u8; 16],
    source: View<'_>,
    version: u8,
) -> Result<Option<PmDcFeaturePropertyPayload>, CodecError> {
    match type_id {
        PART_OPERATION_TYPE => parse_part_operation(ctx, source, version).map(Some),
        EXTENT_TYPE => parse_extent(ctx, source, version).map(Some),
        HOLE_TYPE => parse_hole(ctx, source, version).map(Some),
        FILLET_TYPE => parse_fillet(ctx, source, version).map(Some),
        CHAMFER_TYPE => parse_chamfer(ctx, source, version).map(Some),
        FILLET_EDGE_SELECTION_TYPE => parse_fillet_edge_selection(ctx, source, version).map(Some),
        AUXILIARY_ENUM_TYPE => parse_auxiliary_enum(ctx, source, version).map(Some),
        BOOLEAN_TYPE => parse_boolean(ctx, source, version).map(Some),
        BOUNDARY_PATCH_TYPE => parse_boundary_patch(ctx, source, version).map(Some),
        FEATURE_DIMENSIONS_TYPE => parse_feature_dimensions(ctx, source, version).map(Some),
        OBJECT_COLLECTION_TYPE => parse_object_collection(ctx, source, version).map(Some),
        FILLET_EDGE_SETS_TYPE => parse_fillet_edge_sets(ctx, source, version).map(Some),
        RDX_VARIABLE_TYPE => parse_rdx_variable(ctx, source, version).map(Some),
        SURFACE_BODY_TYPE => parse_surface_body(ctx, source, version).map(Some),
        PROFILE_SELECTION_TYPE => parse_profile_selection(ctx, source, version).map(Some),
        PLACEMENT_TYPE => parse_placement(ctx, source, version).map(Some),
        FILLET_EDGE_SET_TYPE => parse_fillet_edge_set(ctx, source, version).map(Some),
        EDGE_COLLECTION_TYPE => parse_edge_collection(ctx, source, version).map(Some),
        EDGE_ITEM_TYPE => parse_edge_item(ctx, source, version).map(Some),
        _ => Ok(None),
    }
}

fn property(
    version: u8,
    header: PmDcContentHeader,
    kind: PmDcFeaturePropertyKind,
) -> PmDcFeaturePropertyPayload {
    PmDcFeaturePropertyPayload {
        save_version_major: version,
        header,
        kind,
    }
}

fn parse_enumeration(
    source: View<'_>,
    version: u8,
    family: PmDcFeatureEnumFamily,
) -> Result<PmDcFeaturePropertyPayload, CodecError> {
    let mut cursor = Cursor::new(source);
    let header = content_header(&mut cursor)?;
    let type_value = cursor.i16("feature enumeration type value")?;
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
        ) -> Result<PmDcFeaturePropertyPayload, CodecError> {
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
) -> Result<PmDcFeaturePropertyPayload, CodecError> {
    let mut cursor = Cursor::new(source);
    let header = content_header(&mut cursor)?;
    let type_value = cursor.i16("chamfer enumeration type value")?;
    let value = cursor.u16("chamfer enumeration value")?;
    let terminal = cursor.u32("chamfer enumeration terminal")?;
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
) -> Result<PmDcFeaturePropertyPayload, CodecError> {
    let mut cursor = Cursor::new(source);
    let header = content_header(&mut cursor)?;
    let type_value = cursor.u32("fillet edge-selection type value")?;
    let value = cursor.u32("fillet edge-selection value")?;
    cursor.finish("fillet edge-selection enumeration")?;
    Ok(property(
        version,
        header,
        PmDcFeaturePropertyKind::WideEnumeration { type_value, value },
    ))
}

fn parse_boolean(
    ctx: &DecodeContext<'_>,
    source: View<'_>,
    version: u8,
) -> Result<PmDcFeaturePropertyPayload, CodecError> {
    let mut cursor = Cursor::new(source);
    let header = content_header(&mut cursor)?;
    let name = cursor.utf16(ctx, "feature Boolean name")?;
    let name_value = cursor.u32("feature Boolean name value")?;
    let raw = cursor.u8("feature Boolean raw value")?;
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
) -> Result<PmDcFeaturePropertyPayload, CodecError> {
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
        ) -> Result<PmDcFeaturePropertyPayload, CodecError> {
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
) -> Result<PmDcFeaturePropertyPayload, CodecError> {
    let mut cursor = Cursor::new(source);
    let header = content_header(&mut cursor)?;
    let name = cursor.utf16(ctx, "feature RDx variable name")?;
    let name_value = cursor.u32("feature RDx variable name value")?;
    let nominal_value = cursor.u32("feature RDx variable nominal value")?;
    let model_value = cursor.u32("feature RDx variable model value")?;
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
) -> Result<PmDcFeaturePropertyPayload, CodecError> {
    let mut cursor = Cursor::new(source);
    let header = content_header(&mut cursor)?;
    let body = cursor.reference("feature surface body reference")?;
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
) -> Result<PmDcFeaturePropertyPayload, CodecError> {
    let mut cursor = Cursor::new(source);
    let header = content_header(&mut cursor)?;
    let entity_link = cursor.reference("profile selection entity link")?;
    let value = cursor.u8("profile selection value")?;
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
) -> Result<PmDcEntityStyleLinkPayload, CodecError> {
    let mut cursor = Cursor::new(source);
    let header = linked_header(&mut cursor)?;
    let value = cursor.u32("entity-style link value")?;
    let associative_id = cursor.u32("entity-style link associative id")?;
    let entity_type = cursor.u32("entity-style link entity type")?;
    cursor.finish("entity-style link")?;
    Ok(PmDcEntityStyleLinkPayload {
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
) -> Result<PmDcFeaturePropertyPayload, CodecError> {
    let mut cursor = Cursor::new(source);
    let header = content_header(&mut cursor)?;
    let transform = cursor.reference("feature placement transform reference")?;
    let point = cursor.reference("feature placement point reference")?;
    let value = cursor.reference("feature placement value reference")?;
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
) -> Result<PmDcFeaturePropertyPayload, CodecError> {
    let mut cursor = Cursor::new(source);
    let header = content_header(&mut cursor)?;
    let edges = cursor.reference("fillet edge-set edges reference")?;
    let radius = cursor.reference("fillet edge-set radius reference")?;
    let selection = cursor.reference("fillet edge-set selection reference")?;
    let continuity = cursor.reference("fillet edge-set continuity reference")?;
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
) -> Result<PmDcFeaturePropertyPayload, CodecError> {
    let mut cursor = Cursor::new(source);
    let header = content_header(&mut cursor)?;
    let index_references = u32_list(ctx, &mut cursor, 2, "edge-item index references")?;
    let index_reference_value = if index_references.values().is_empty() {
        -1
    } else {
        cursor.i32("edge-item index reference value")?
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
        owner: cursor.reference("linked header owner reference")?,
        parent: cursor.reference("linked header parent reference")?,
        next: cursor.reference("linked header next reference")?,
    })
}

fn parse_label(
    ctx: &DecodeContext<'_>,
    source: View<'_>,
    version: u8,
) -> Result<PmDcFeatureLabelPayload, CodecError> {
    let mut cursor = Cursor::new(source);
    let header = linked_header(&mut cursor)?;
    let index = cursor.u32("feature label index")?;
    let participants = reference_list(ctx, &mut cursor, 2, "feature-label participants")?;
    let name = cursor.utf16(ctx, "feature label")?;
    let class_id_bytes = cursor.take_array("feature-label class id")?;
    let mut class_id_storage =
        ctx.reserve_scoped(0, "materialize Inventor feature label class id")?;
    let class_id = class_id_storage.with_storage(|| {
        type_id_string(
            ctx,
            class_id_bytes,
            "retain Inventor feature label class id",
        )
    })?;
    cursor.finish("feature label")?;
    PmDcFeatureLabelPayloadWire {
        save_version_major: version,
        header,
        index,
        participants,
        name,
        class_id,
    }
    .into_record(ctx)
}

const EXTRUSION_CLASS_ID: ClassId = ClassId([
    0x31, 0x11, 0xa9, 0x0c, 0xd0, 0x11, 0x8b, 0x83, 0x00, 0x08, 0x19, 0xb0, 0x05, 0x24, 0xdc, 0x09,
]);
const FILLET_CLASS_ID: ClassId = ClassId([
    0xdc, 0x15, 0xf7, 0xf1, 0xd1, 0x11, 0x42, 0x05, 0x00, 0x08, 0x30, 0xb0, 0x05, 0x24, 0xdc, 0x09,
]);
const CHAMFER_CLASS_ID: ClassId = ClassId([
    0x3f, 0x71, 0x00, 0xf9, 0xd2, 0x11, 0x8b, 0x6f, 0x60, 0x00, 0xf0, 0xa8, 0x9d, 0xcc, 0xef, 0xb0,
]);
const HOLE_CLASS_ID: ClassId = ClassId([
    0x1a, 0x7d, 0x75, 0x1f, 0xd2, 0x11, 0x9c, 0x54, 0xa0, 0x00, 0x20, 0x80, 0x36, 0x03, 0xc8, 0xc9,
]);

#[derive(Clone, Copy)]
pub(crate) enum FeatureFamily {
    Extrusion,
    Fillet,
    Chamfer,
    Hole,
}

impl FeatureFamily {
    pub(crate) fn class_id(self) -> ClassId {
        match self {
            Self::Extrusion => EXTRUSION_CLASS_ID,
            Self::Fillet => FILLET_CLASS_ID,
            Self::Chamfer => CHAMFER_CLASS_ID,
            Self::Hole => HOLE_CLASS_ID,
        }
    }

    pub(crate) fn output_slot(self) -> usize {
        match self {
            Self::Extrusion => 26,
            Self::Fillet => 15,
            Self::Chamfer => 11,
            Self::Hole => 24,
        }
    }

    fn from_class_id(class_id: ClassId) -> Option<Self> {
        [Self::Extrusion, Self::Fillet, Self::Chamfer, Self::Hole]
            .into_iter()
            .find(|family| family.class_id() == class_id)
    }

    pub(crate) fn from_definition(definition: &FeatureDefinition) -> Option<Self> {
        match definition {
            FeatureDefinition::Operation(FeatureOperation::Extrude { .. }) => Some(Self::Extrusion),
            FeatureDefinition::Operation(FeatureOperation::Fillet { .. }) => Some(Self::Fillet),
            FeatureDefinition::Operation(FeatureOperation::Chamfer { .. }) => Some(Self::Chamfer),
            FeatureDefinition::Operation(FeatureOperation::Hole { .. }) => Some(Self::Hole),
            _ => None,
        }
    }
}

struct ProjectionIndex<'a> {
    properties: HashMap<(&'a str, u32), Option<&'a PmDcFeatureProperty>>,
    parameters: HashMap<(&'a str, u32), Option<&'a crate::design::PmDcParameter>>,
    parameter_values: HashMap<&'a str, &'a ParameterValue>,
    sketches: HashMap<(&'a str, u32), Option<&'a crate::sketch::PmDcSketch>>,
    sketch_ids: HashMap<&'a str, &'a cadmpeg_ir::sketches::SketchId>,
    directions: HashMap<(&'a str, u32), Option<&'a crate::sketch::PmDcDirection>>,
    transforms: HashMap<(&'a str, u32), Option<&'a crate::sketch::PmDcTransform>>,
    entity_style_links: HashSet<(&'a str, u32)>,
}

pub(crate) fn project(
    ctx: &DecodeContext<'_>,
    inventory: &FeatureInventory,
    design: &DesignInventory,
    sketch: &SketchInventory,
    parameters: &[DesignParameter],
    sketches: &[Sketch],
) -> Result<FeatureProjection, CodecError> {
    let total = inventory
        .features
        .len()
        .checked_add(inventory.pattern_features.len())
        .ok_or_else(|| ctx.refuse_codec_limit("Inventor feature count", u64::MAX, u64::MAX))?;
    let (feature_tokens, _feature_tokens_storage) =
        ctx.with_scoped_storage("index Inventor feature token", || {
            let mut feature_tokens = HashSet::new();
            for token in ctx
                .admit_iter(&inventory.features, "index Inventor feature token")?
                .map(|feature| feature.identity.segment_token.as_str())
                .chain(
                    ctx.admit_iter(&inventory.pattern_features, "index Inventor feature token")?
                        .map(|feature| feature.identity.segment_token.as_str()),
                )
            {
                ctx.insert_hash_set(&mut feature_tokens, token, "index Inventor feature token")?;
            }
            Ok::<_, CodecError>(feature_tokens)
        })?;
    if feature_tokens.len() > 1 {
        return Ok(FeatureProjection {
            features: Vec::new(),
            result_topologies: Vec::new(),
            unresolved_features: total,
            unresolved_states: 0,
        });
    }

    let (unique_properties, _properties_storage) = ctx.unique_index(
        inventory.properties.iter().map(|record| {
            (
                {
                    (
                        record.identity.segment_token.as_str(),
                        record.identity.record_ordinal,
                    )
                },
                record,
            )
        }),
        "index Inventor feature properties",
    )?;
    let (unique_parameters, _parameters_storage) = ctx.unique_index(
        design.parameters.iter().map(|record| {
            (
                {
                    (
                        record.identity.segment_token.as_str(),
                        record.identity.record_ordinal,
                    )
                },
                record,
            )
        }),
        "index Inventor feature parameters",
    )?;
    let (unique_sketches, _sketches_storage) = ctx.unique_index(
        sketch.sketches.iter().map(|record| {
            (
                {
                    (
                        record.identity.segment_token.as_str(),
                        record.identity.record_ordinal,
                    )
                },
                record,
            )
        }),
        "index Inventor feature sketches",
    )?;
    let (unique_directions, _directions_storage) = ctx.unique_index(
        sketch.directions.iter().map(|record| {
            (
                {
                    (
                        record.identity.segment_token.as_str(),
                        record.identity.record_ordinal,
                    )
                },
                record,
            )
        }),
        "index Inventor feature directions",
    )?;
    let (unique_transforms, _transforms_storage) = ctx.unique_index(
        sketch.transforms.iter().map(|record| {
            (
                {
                    (
                        record.identity.segment_token.as_str(),
                        record.identity.record_ordinal,
                    )
                },
                record,
            )
        }),
        "index Inventor feature transforms",
    )?;
    let (parameter_values, _parameter_values_storage) =
        ctx.with_scoped_storage("index Inventor feature parameter values", || {
            ctx.collect_hash_map(
                ctx.admit_iter(parameters, "index Inventor feature parameter values")?
                    .filter_map(|parameter| {
                        Some((parameter.native_ref.as_deref()?, parameter.value.as_ref()?))
                    }),
                "index Inventor feature parameter values",
            )
        })?;
    let (sketch_ids, _sketch_ids_storage) =
        ctx.with_scoped_storage("index Inventor feature sketch ids", || {
            let mut ids = HashMap::new();
            for sketch in ctx.admit_iter(sketches, "visit Inventor feature items")? {
                if let Some(native) = sketch.native_ref.as_deref() {
                    ctx.insert_hash_map(
                        &mut ids,
                        native,
                        &sketch.id,
                        "index Inventor feature sketch ids",
                    )?;
                }
            }
            Ok::<_, CodecError>(ids)
        })?;
    let (entity_style_links, _entity_style_links_storage) =
        ctx.with_scoped_storage("index Inventor entity style links", || {
            let mut links = HashSet::new();
            for record in ctx.admit_iter(
                &inventory.entity_style_links,
                "visit Inventor feature items",
            )? {
                ctx.insert_hash_set(
                    &mut links,
                    (
                        record.identity.segment_token.as_str(),
                        record.identity.record_ordinal,
                    ),
                    "index Inventor entity style link",
                )?;
            }
            Ok::<_, CodecError>(links)
        })?;
    let index = ProjectionIndex {
        properties: unique_properties,
        parameters: unique_parameters,
        parameter_values,
        sketches: unique_sketches,
        sketch_ids,
        directions: unique_directions,
        transforms: unique_transforms,
        entity_style_links,
    };
    // The label owner is a one-based reference. Keying on the optional record
    // ordinal keeps the null reference out of the ordinal space, so a label
    // with no owner never claims the feature at ordinal 0.
    let (labels, _labels_storage) = ctx.unique_index(
        inventory.labels.iter().map(|label| {
            (
                {
                    (
                        label.identity.segment_token.as_str(),
                        label.header.owner.record_ordinal(),
                    )
                },
                label,
            )
        }),
        "index Inventor feature labels",
    )?;
    let mut projected = Vec::new();
    let mut projected_storage = ctx.reserve_scoped(0, "collect Inventor feature items")?;
    for feature in ctx.admit_iter(&inventory.features, "visit Inventor feature items")? {
        let Some(label) = ctx
            .get_hash_map(
                &labels,
                &(
                    feature.identity.segment_token.as_str(),
                    Some(feature.identity.record_ordinal),
                ),
                "access Inventor feature records",
            )?
            .and_then(Option::as_ref)
        else {
            continue;
        };
        let Some(family) = FeatureFamily::from_class_id(label.class_id()) else {
            continue;
        };
        let value = match family {
            FeatureFamily::Extrusion => project_extrusion(ctx, feature, label, &index),
            FeatureFamily::Fillet => project_fillet(ctx, feature, label, &index),
            FeatureFamily::Chamfer => project_chamfer(ctx, feature, label, &index),
            FeatureFamily::Hole => project_hole(ctx, feature, label, &index),
        }
        .transpose()?;
        if let Some(value) = value {
            projected_storage.with_storage(|| {
                ctx.push_vec(&mut projected, value, "collect Inventor feature items")
            })?;
        }
    }
    let (ordinal_counts, _ordinal_counts_storage) =
        ctx.with_scoped_storage("count Inventor feature ordinals", || {
            let mut counts = BTreeMap::<u64, usize>::new();
            for (feature, _) in ctx.admit_iter(&projected, "count Inventor feature ordinals")? {
                if let Some(count) = ctx.get_mut_btree_map(
                    &mut counts,
                    &feature.ordinal,
                    "count Inventor feature ordinals",
                )? {
                    *count = 2;
                } else {
                    ctx.insert_btree_map(
                        &mut counts,
                        feature.ordinal,
                        1,
                        "count Inventor feature ordinals",
                    )?;
                }
            }
            Ok::<_, CodecError>(counts)
        })?;
    let (duplicate_ordinals, _duplicate_ordinals_storage) =
        ctx.with_scoped_storage("collect duplicate Inventor feature ordinals", || {
            ctx.collect_hash_set(
                ctx.admit_iter(
                    &ordinal_counts,
                    "collect duplicate Inventor feature ordinals",
                )?
                .filter_map(|(ordinal, count)| (*count > 1).then_some(*ordinal)),
                "collect duplicate Inventor feature ordinals",
            )
        })?;
    ctx.retain_vec(
        &mut projected,
        |(feature, _)| {
            Ok(!ctx.contains_hash_set(
                &duplicate_ordinals,
                &feature.ordinal,
                "remove duplicate Inventor feature ordinal",
            )?)
        },
        "remove duplicate Inventor feature ordinals",
    )?;
    ctx.sort_unstable_by(
        &mut projected,
        |value| &value.0.ordinal,
        Ord::cmp,
        "Inventor projected features sort",
    )?;
    let (features, result_topologies): (Vec<_>, Vec<_>) = ctx.unzip_vec(
        projected,
        "collect Inventor projected features and topologies",
    )?;
    drop(projected_storage);
    Ok(FeatureProjection {
        unresolved_features: total.checked_sub(features.len()).ok_or_else(|| {
            CodecError::malformed("Inventor feature projection exceeds inventory")
        })?,
        unresolved_states: features.len(),
        features,
        result_topologies,
    })
}

macro_rules! option_result_value {
    ($result:expr) => {
        match $result {
            Ok(value) => value,
            Err(error) => return Some(Err(CodecError::from(error))),
        }
    };
}

fn project_extrusion(
    ctx: &DecodeContext<'_>,
    source: &PmDcFeature,
    label: &PmDcFeatureLabel,
    index: &ProjectionIndex<'_>,
) -> Option<Result<(Feature, FeatureResultTopology), CodecError>> {
    let operation = option_result_value!(enum16(
        ctx,
        source,
        0,
        PmDcFeatureEnumFamily::PartOperation,
        index
    ))?;
    let op = match operation {
        1 => BooleanOp::NewBody,
        2 => BooleanOp::Cut,
        3 => BooleanOp::Join,
        4 => BooleanOp::Intersect,
        _ => return None,
    };
    let boundary = option_result_value!(references(
        ctx,
        source,
        1,
        PmDcFeatureReferenceFamily::BoundaryPatch,
        index
    ))?;
    if source.properties.references().get(23)? != source.properties.references().get(1)? {
        return None;
    }
    for (position, selection) in
        option_result_value!(ctx.admit_iter(boundary.references(), "visit Inventor feature items"))
            .enumerate()
    {
        let mut duplicate = false;
        for prior in option_result_value!(ctx.admit_iter(
            &boundary.references()[..position],
            "check distinct Inventor extrusion selections",
        )) {
            if option_result_value!(ctx.equal(
                &prior.index(),
                &selection.index(),
                "compare Inventor extrusion selection references",
            )) {
                duplicate = true;
                break;
            }
        }
        if duplicate {
            return None;
        }
    }
    let mut selections = Vec::new();
    for reference in
        option_result_value!(ctx.admit_iter(boundary.references(), "visit Inventor feature items"))
    {
        let property = option_result_value!(resolve_property(
            ctx,
            source.identity.segment_token.as_str(),
            reference.index(),
            index,
        ))?;
        let PmDcFeaturePropertyKind::ProfileSelection { entity_link, .. } = &property.kind else {
            return None;
        };
        let ordinal = entity_link.index().checked_sub(1)?;
        if !option_result_value!(ctx.contains_hash_set(
            &index.entity_style_links,
            &(source.identity.segment_token.as_str(), ordinal),
            "resolve Inventor feature entity style link",
        )) {
            return None;
        }
        option_result_value!(ctx.push_vec(
            &mut selections,
            option_result_value!(property.id(ctx)),
            "collect Inventor feature items"
        ));
    }
    if selections.is_empty() || label.participants.references().len() != 1 {
        return None;
    }
    let sketch_reference = label.participants.references().first()?;
    let sketch = option_result_value!(ctx.get_hash_map(
        &index.sketches,
        &(
            source.identity.segment_token.as_str(),
            sketch_reference.index().checked_sub(1)?,
        ),
        "resolve Inventor feature sketch",
    ))
    .and_then(Option::as_ref)?;
    let (sketch_native_id, sketch_native_id_storage) = option_result_value!(ctx
        .with_scoped_storage("resolve Inventor extrusion sketch native id", || sketch
            .id(ctx),));
    let sketch_id = option_result_value!(ctx.get_hash_map(
        &index.sketch_ids,
        sketch_native_id.as_str(),
        "access Inventor feature records",
    ))?;
    drop(sketch_native_id_storage);
    let sketch_id = match sketch_id.try_clone_for_decode(ctx, "retain Inventor extrusion sketch id")
    {
        Ok(value) => value,
        Err(error) => return Some(Err(error)),
    };

    let direction_record = option_result_value!(resolve_direction(ctx, source, 2, index))?;
    let mut direction = cadmpeg_ir::units::UnitVector3::normalized(Vector3::new(
        direction_record.direction[0].get(),
        direction_record.direction[1].get(),
        direction_record.direction[2].get(),
    ))?;
    if option_result_value!(boolean(ctx, source, 3, index))? {
        direction = direction.reversed();
    }
    let length = option_result_value!(length_parameter(ctx, source, 4, index))?;
    let taper = cadmpeg_ir::scalar::SlopeAngle::try_from(option_result_value!(angle_parameter(
        ctx, source, 5, index
    ))?)
    .ok()?;
    let termination =
        match option_result_value!(enum16(ctx, source, 6, PmDcFeatureEnumFamily::Extent, index))? {
            1 => LinearTermination::Blind {
                length: cadmpeg_ir::scalar::NonZeroLength::from(
                    cadmpeg_ir::scalar::PositiveLength::try_from(length).ok()?,
                ),
            },
            4 => LinearTermination::ThroughNext {},
            5 => LinearTermination::ThroughAll {},
            _ => return None,
        };
    let side = ExtrudeSide {
        termination,
        draft: (taper.get() != 0.0).then_some(taper),
    };
    let extent = if option_result_value!(boolean(ctx, source, 7, index))? {
        ExtrudeExtent::Symmetric { side }
    } else {
        ExtrudeExtent::OneSided { side }
    };
    let (feature_id, result) = match feature_result(ctx, source, 26, index)? {
        Ok(value) => value,
        Err(error) => return Some(Err(error)),
    };
    let source_properties = match boolean_properties(ctx, source, &[20, 22], index) {
        Ok(value) => value,
        Err(error) => return Some(Err(error)),
    };
    if let Err(error) = admit_projected_feature(ctx, source, label, "extrude") {
        return Some(Err(error));
    }
    let profile = match PlanarProfileRef::sketch_selection(sketch_id, selections, ctx) {
        Ok(Ok(profile)) => profile,
        Ok(Err(_)) => return None,
        Err(limit) => return Some(Err(CodecError::ResourceLimit(limit))),
    };
    let feature = Feature {
        id: feature_id,
        ordinal: u64::from(label.index),
        name: Some(option_result_value!(ctx.copy_retained_text(
            label.name.as_str(),
            "retain Inventor projected feature name"
        ))),
        suppressed: None,
        dependencies: DistinctMembers::default(),
        source_properties,
        source_tag: Some("extrude".into()),
        source_text: None,
        source_content: FeatureContent::default(),

        evaluation: cadmpeg_ir::features::FeatureEvaluation::from_definition(
            FeatureDefinition::Operation(FeatureOperation::Extrude {
                profile: ProfileRef::Planar(profile),
                direction: ExtrudeDirection::Explicit {
                    vector: cadmpeg_ir::features::FeatureDirection3::from(direction),
                    source: Some(ExtrusionDirectionSource::Custom {}),
                },
                start: ExtrudeStart::ProfilePlane {},
                extent,
                op,
                solid: Some(true),
                face_maker: None,
                inner_wire_taper: None,
                length_along_profile_normal: None,
                allow_multi_profile_faces: None,
            }),
        ),
        native_ref: Some(option_result_value!(source.id(ctx))),
    };
    Some(Ok((feature, result)))
}

fn admit_projected_feature(
    ctx: &DecodeContext<'_>,
    source: &PmDcFeature,
    label: &PmDcFeatureLabel,
    tag: &'static str,
) -> Result<(), CodecError> {
    ctx.charge_collection_items(1, "project Inventor feature")?;
    ctx.charge_entities(1, "project Inventor feature")?;
    ctx.charge_retained(
        cadmpeg_core::decode::u64_from_index(label.name.as_str().len()),
        "retain Inventor projected feature name",
    )?;
    ctx.charge_retained(
        cadmpeg_core::decode::u64_from_index(tag.len()),
        "retain Inventor projected feature tag",
    )?;
    ctx.charge_retained(
        cadmpeg_core::decode::u64_from_index(source.id_len().ok_or_else(|| {
            CodecError::Malformed("Inventor identifier length exceeds address space".into())
        })?),
        "retain Inventor projected feature native id",
    )?;
    Ok(())
}

fn project_fillet(
    ctx: &DecodeContext<'_>,
    source: &PmDcFeature,
    label: &PmDcFeatureLabel,
    index: &ProjectionIndex<'_>,
) -> Option<Result<(Feature, FeatureResultTopology), CodecError>> {
    if option_result_value!(enum16(
        ctx,
        source,
        11,
        PmDcFeatureEnumFamily::Fillet,
        index
    ))? != 0
        || source.properties.references().get(1)?.index() != 0
        || source.properties.references().get(10)?.index() != 0
    {
        return None;
    }
    let sets = option_result_value!(references(
        ctx,
        source,
        0,
        PmDcFeatureReferenceFamily::FilletEdgeSets,
        index
    ))?;
    let mut groups = Vec::new();
    for reference in
        option_result_value!(ctx.admit_iter(sets.references(), "visit Inventor feature items"))
    {
        let set = option_result_value!(resolve_property(
            ctx,
            source.identity.segment_token.as_str(),
            reference.index(),
            index,
        ))?;
        let PmDcFeaturePropertyKind::FilletEdgeSet {
            edges,
            radius,
            selection,
            continuity,
        } = &set.kind
        else {
            return None;
        };
        let selection = option_result_value!(resolve_property(
            ctx,
            source.identity.segment_token.as_str(),
            selection.index(),
            index,
        ))?;
        if !matches!(
            selection.kind,
            PmDcFeaturePropertyKind::WideEnumeration {
                type_value: 4,
                value: 0
            }
        ) || !matches!(
            option_result_value!(resolve_property(
                ctx,
                source.identity.segment_token.as_str(),
                continuity.index(),
                index
            ))?
            .kind,
            PmDcFeaturePropertyKind::Boolean { value: false, .. }
        ) {
            return None;
        }
        let edge_collection = option_result_value!(resolve_property(
            ctx,
            source.identity.segment_token.as_str(),
            edges.index(),
            index
        ))?;
        let PmDcFeaturePropertyKind::References {
            family: PmDcFeatureReferenceFamily::EdgeCollection,
            items,
        } = &edge_collection.kind
        else {
            return None;
        };
        if !option_result_value!(closed_edge_items(
            ctx,
            source.identity.segment_token.as_str(),
            items,
            index
        )) {
            return None;
        }
        let radius = cadmpeg_ir::scalar::PositiveLength::new(
            option_result_value!(length_reference(
                ctx,
                source.identity.segment_token.as_str(),
                radius.index(),
                index,
            ))?
            .get(),
        )?;
        option_result_value!(ctx.push_vec(
            &mut groups,
            FilletGroup {
                edges: EdgeSelection::Native(option_result_value!(edge_collection.id(ctx))),
                radius: RadiusSpec::Constant { radius },
                tangency_weight: None,
            },
            "collect Inventor feature items"
        ));
    }
    if groups.is_empty() {
        return None;
    }
    let (feature_id, result) = match feature_result(ctx, source, 15, index)? {
        Ok(value) => value,
        Err(error) => return Some(Err(error)),
    };
    let source_properties = match boolean_properties(ctx, source, &[2, 3, 4, 5, 8], index) {
        Ok(value) => value,
        Err(error) => return Some(Err(error)),
    };
    if let Err(error) = admit_projected_feature(ctx, source, label, "fillet") {
        return Some(Err(error));
    }
    Some(Ok((
        Feature {
            id: feature_id,
            ordinal: u64::from(label.index),
            name: Some(option_result_value!(ctx.copy_retained_text(
                label.name.as_str(),
                "retain Inventor projected feature name"
            ))),
            suppressed: None,
            dependencies: DistinctMembers::default(),
            source_properties,
            source_tag: Some("fillet".into()),
            source_text: None,
            source_content: FeatureContent::default(),

            evaluation: cadmpeg_ir::features::FeatureEvaluation::from_definition(
                FeatureDefinition::Operation(FeatureOperation::Fillet {
                    groups: groups.try_into().ok()?,
                }),
            ),
            native_ref: Some(option_result_value!(source.id(ctx))),
        },
        result,
    )))
}

fn project_chamfer(
    ctx: &DecodeContext<'_>,
    source: &PmDcFeature,
    label: &PmDcFeatureLabel,
    index: &ProjectionIndex<'_>,
) -> Option<Result<(Feature, FeatureResultTopology), CodecError>> {
    if option_result_value!(enum16(
        ctx,
        source,
        4,
        PmDcFeatureEnumFamily::Chamfer,
        index
    ))? != 0
    {
        return None;
    }
    let edges = option_result_value!(slot_property(ctx, source, 0, index))?;
    let PmDcFeaturePropertyKind::References {
        family: PmDcFeatureReferenceFamily::EdgeCollection,
        items,
    } = &edges.kind
    else {
        return None;
    };
    if !option_result_value!(closed_edge_items(
        ctx,
        source.identity.segment_token.as_str(),
        items,
        index
    )) {
        return None;
    }
    let distance = cadmpeg_ir::scalar::PositiveLength::try_from(option_result_value!(
        length_parameter(ctx, source, 2, index)
    )?)
    .ok()?;
    let flip_direction = option_result_value!(boolean(ctx, source, 5, index))?;
    let (feature_id, result) = match feature_result(ctx, source, 11, index)? {
        Ok(value) => value,
        Err(error) => return Some(Err(error)),
    };
    let source_properties = match boolean_properties(ctx, source, &[6, 9], index) {
        Ok(value) => value,
        Err(error) => return Some(Err(error)),
    };
    if let Err(error) = admit_projected_feature(ctx, source, label, "chamfer") {
        return Some(Err(error));
    }
    if let Err(error) = ctx.charge_collection_items(1, "collect Inventor chamfer group") {
        return Some(Err(error));
    }
    let groups = cadmpeg_ir::features::NonEmptyMembers::one(ChamferGroup {
        edges: EdgeSelection::Native(option_result_value!(edges.id(ctx))),
        spec: ChamferSpec::Distance { distance },
    });
    Some(Ok((
        Feature {
            id: feature_id,
            ordinal: u64::from(label.index),
            name: Some(option_result_value!(ctx.copy_retained_text(
                label.name.as_str(),
                "retain Inventor projected feature name"
            ))),
            suppressed: None,
            dependencies: DistinctMembers::default(),
            source_properties,
            source_tag: Some("chamfer".into()),
            source_text: None,
            source_content: FeatureContent::default(),

            evaluation: cadmpeg_ir::features::FeatureEvaluation::from_definition(
                FeatureDefinition::Operation(FeatureOperation::Chamfer {
                    groups,
                    flip_direction,
                }),
            ),
            native_ref: Some(option_result_value!(source.id(ctx))),
        },
        result,
    )))
}

fn project_hole(
    ctx: &DecodeContext<'_>,
    source: &PmDcFeature,
    label: &PmDcFeatureLabel,
    index: &ProjectionIndex<'_>,
) -> Option<Result<(Feature, FeatureResultTopology), CodecError>> {
    let hole_form =
        option_result_value!(enum16(ctx, source, 0, PmDcFeatureEnumFamily::Hole, index))?;
    if option_result_value!(boolean(ctx, source, 17, index))? {
        return None;
    }
    let diameter = option_result_value!(length_parameter(ctx, source, 1, index))?;
    let depth = option_result_value!(length_parameter(ctx, source, 2, index))?;
    let head_diameter = option_result_value!(length_parameter(ctx, source, 3, index))?;
    let head_depth = option_result_value!(length_parameter(ctx, source, 4, index))?;
    let head_angle = option_result_value!(angle_parameter(ctx, source, 5, index))?;
    let point_angle = option_result_value!(angle_parameter(ctx, source, 6, index))?;
    let kind = match hole_form {
        0 if point_angle.get() == 0.0 => HoleKind::Simple,
        0 => HoleKind::SimpleDrilled {
            drill_point_angle: cadmpeg_ir::scalar::InteriorAngle::try_from(point_angle).ok()?,
        },
        1 => HoleKind::Countersink {
            diameter: cadmpeg_ir::scalar::PositiveLength::try_from(head_diameter).ok()?,
            angle: cadmpeg_ir::scalar::InteriorAngle::try_from(head_angle).ok()?,
        },
        2 if point_angle.get() == 0.0 => HoleKind::Counterbore {
            diameter: cadmpeg_ir::scalar::PositiveLength::try_from(head_diameter).ok()?,
            depth: cadmpeg_ir::scalar::PositiveLength::try_from(head_depth).ok()?,
        },
        2 => HoleKind::CounterboreDrilled {
            diameter: cadmpeg_ir::scalar::PositiveLength::try_from(head_diameter).ok()?,
            depth: cadmpeg_ir::scalar::PositiveLength::try_from(head_depth).ok()?,
            drill_point_angle: cadmpeg_ir::scalar::InteriorAngle::try_from(point_angle).ok()?,
        },
        _ => return None,
    };
    let extent =
        match option_result_value!(enum16(ctx, source, 9, PmDcFeatureEnumFamily::Extent, index))? {
            1 => LinearTermination::Blind {
                length: cadmpeg_ir::scalar::NonZeroLength::from(
                    cadmpeg_ir::scalar::PositiveLength::try_from(depth).ok()?,
                ),
            },
            4 => LinearTermination::ThroughNext {},
            5 => LinearTermination::ThroughAll {},
            _ => return None,
        };
    let transform_reference = source.properties.references().get(8)?;
    let transform = option_result_value!(ctx.get_hash_map(
        &index.transforms,
        &(
            source.identity.segment_token.as_str(),
            transform_reference.index().checked_sub(1)?,
        ),
        "resolve Inventor feature transform",
    ))
    .and_then(Option::as_ref)?;
    if transform.matrix.rows()[3]
        .iter()
        .zip([0.0, 0.0, 0.0, 1.0])
        .any(|(actual, expected)| (actual - expected).abs() > EPS_FEATURE_PROJECT_HOLE_E10)
    {
        return None;
    }
    let direction_record = option_result_value!(resolve_direction(ctx, source, 16, index))?;
    let direction = cadmpeg_ir::units::UnitVector3::normalized(Vector3::new(
        direction_record.direction[0].get(),
        direction_record.direction[1].get(),
        direction_record.direction[2].get(),
    ))?;
    let placement = option_result_value!(slot_property(ctx, source, 21, index))?;
    let PmDcFeaturePropertyKind::Placement {
        transform: placement_transform,
        point,
        value,
    } = &placement.kind
    else {
        return None;
    };
    if placement_transform.index() != transform_reference.index()
        || point.index() == 0
        || value.index() == 0
    {
        return None;
    }
    let position = cadmpeg_ir::features::FinitePoint3::new(Point3::new(
        transform.matrix.rows()[0][3] * 10.0,
        transform.matrix.rows()[1][3] * 10.0,
        transform.matrix.rows()[2][3] * 10.0,
    ))?;
    let shape = cadmpeg_ir::features::holes::HoleShape::new(
        cadmpeg_ir::features::holes::HoleConstruction::Form {
            kind,
            specification: None,
        },
        None,
        Some(cadmpeg_ir::scalar::PositiveLength::try_from(diameter).ok()?),
    )
    .ok()?;
    let (feature_id, result) = match feature_result(ctx, source, 24, index)? {
        Ok(value) => value,
        Err(error) => return Some(Err(error)),
    };
    if let Err(error) = admit_projected_feature(ctx, source, label, "hole") {
        return Some(Err(error));
    }
    if let Err(error) = ctx.charge_collection_items(1, "project Inventor hole placement") {
        return Some(Err(error));
    }
    Some(Ok((
        Feature {
            id: feature_id,
            ordinal: u64::from(label.index),
            name: Some(option_result_value!(ctx.copy_retained_text(
                label.name.as_str(),
                "retain Inventor projected feature name"
            ))),
            suppressed: None,
            dependencies: DistinctMembers::default(),
            source_properties: BTreeMap::new(),
            source_tag: Some("hole".into()),
            source_text: None,
            source_content: FeatureContent::default(),

            evaluation: cadmpeg_ir::features::FeatureEvaluation::from_definition(
                FeatureDefinition::Operation(FeatureOperation::Hole {
                    profile: None,
                    profile_filter: None,
                    face: None,
                    direction: None,
                    placements: Some(vec![HolePlacement::Directed {
                        position,
                        direction: cadmpeg_ir::features::FeatureDirection3::from(direction),
                    }]),
                    shape,

                    extent: Some(extent),
                    bottom: None,
                    taper_angle: None,
                    allow_multi_profile_faces: None,
                }),
            ),
            native_ref: Some(option_result_value!(source.id(ctx))),
        },
        result,
    )))
}

fn feature_result(
    ctx: &DecodeContext<'_>,
    source: &PmDcFeature,
    slot: usize,
    index: &ProjectionIndex<'_>,
) -> Option<Result<(FeatureId, FeatureResultTopology), CodecError>> {
    const FEATURE_ID_PREFIX: &str = "inventor:design:feature#";
    const RESULT_ID_PREFIX: &str = "inventor:design:feature-result#";
    let collection = option_result_value!(slot_property(ctx, source, slot, index))?;
    let PmDcFeaturePropertyKind::References {
        family: PmDcFeatureReferenceFamily::ObjectCollection,
        items,
    } = &collection.kind
    else {
        return None;
    };
    let mut bodies = Vec::new();
    for reference in
        option_result_value!(ctx.admit_iter(items.references(), "visit Inventor feature items"))
    {
        let body = option_result_value!(resolve_property(
            ctx,
            source.identity.segment_token.as_str(),
            reference.index(),
            index,
        ))?;
        if !matches!(body.kind, PmDcFeaturePropertyKind::SurfaceBody { .. }) {
            return None;
        }
        let body_id = option_result_value!(body.id(ctx));
        // Located::id uses the fixed nonblank `inventor:pmdc:` prefix.
        let body = NonBlankString::from_ascii_leading(body_id)?;
        option_result_value!(ctx.push_vec(&mut bodies, body, "collect Inventor feature items"));
    }
    if bodies.is_empty() {
        return None;
    }
    let members = match cadmpeg_ir::features::FeatureResultMembers::new(
        bodies,
        Vec::new(),
        Vec::new(),
        Vec::new(),
        ctx,
        "precheck distinct Inventor feature result bodies",
    ) {
        Ok(Ok(members)) => members,
        Ok(Err(_)) => return None,
        Err(limit) => return Some(Err(CodecError::ResourceLimit(limit))),
    };
    let mut key_storage = match ctx.reserve_scoped(0, "compose Inventor feature result key") {
        Ok(storage) => storage,
        Err(error) => return Some(Err(error)),
    };
    let key = match key_storage.with_storage(|| source.identity.key(ctx)) {
        Ok(value) => value,
        Err(error) => return Some(Err(error)),
    };
    let key_len = key.as_str().len();
    let Some(feature_id_len) = FEATURE_ID_PREFIX.len().checked_add(key_len) else {
        return Some(Err(ctx.refuse_codec_limit(
            "retain Inventor feature result identities",
            u64::MAX,
            u64::MAX,
        )));
    };
    let Some(result_id_len) = RESULT_ID_PREFIX.len().checked_add(key_len) else {
        return Some(Err(ctx.refuse_codec_limit(
            "retain Inventor feature result identities",
            u64::MAX,
            u64::MAX,
        )));
    };
    let feature_id_text = match ctx.format_retained(
        format_args!("{FEATURE_ID_PREFIX}{}", key.as_str()),
        "retain Inventor feature result identities",
    ) {
        Ok(value) => value,
        Err(error) => return Some(Err(error)),
    };
    let result_id_text = match ctx.format_retained(
        format_args!("{RESULT_ID_PREFIX}{}", key.as_str()),
        "retain Inventor feature result identities",
    ) {
        Ok(value) => value,
        Err(error) => return Some(Err(error)),
    };
    debug_assert_eq!(feature_id_text.len(), feature_id_len);
    debug_assert_eq!(result_id_text.len(), result_id_len);
    let Some(feature_id_scan) = cadmpeg_core::decode::u64_from_index(feature_id_len)
        .checked_add(cadmpeg_core::decode::u64_from_index(key_len))
    else {
        return Some(Err(ctx.refuse_codec_limit(
            "validate Inventor feature result identity",
            u64::MAX,
            u64::MAX,
        )));
    };
    let Some(result_id_scan) = cadmpeg_core::decode::u64_from_index(result_id_len)
        .checked_add(cadmpeg_core::decode::u64_from_index(key_len))
    else {
        return Some(Err(ctx.refuse_codec_limit(
            "validate Inventor feature result identity",
            u64::MAX,
            u64::MAX,
        )));
    };
    let Some(identity_scan_work) = feature_id_scan.checked_add(result_id_scan) else {
        return Some(Err(ctx.refuse_codec_limit(
            "validate Inventor feature result identity",
            u64::MAX,
            u64::MAX,
        )));
    };
    if let Err(error) = ctx.charge_work(
        identity_scan_work,
        "validate Inventor feature result identity",
    ) {
        return Some(Err(error));
    }
    let Ok(feature_id) = FeatureId::mint(feature_id_text) else {
        return Some(Err(CodecError::malformed(
            "generated Inventor feature identity is invalid",
        )));
    };
    let Ok(result_id) = FeatureResultTopologyId::mint(result_id_text) else {
        return Some(Err(CodecError::malformed(
            "generated Inventor feature result identity is invalid",
        )));
    };
    if let Err(error) = ctx.charge_collection_items(1, "project Inventor feature result topology") {
        return Some(Err(error));
    }
    if let Err(error) = ctx.charge_entities(1, "project Inventor feature result topology") {
        return Some(Err(error));
    }
    let result = FeatureResultTopology::new(
        result_id,
        match feature_id.try_clone_for_decode(ctx, "retain Inventor feature result identities") {
            Ok(value) => value,
            Err(error) => return Some(Err(error)),
        },
        members,
        Some(option_result_value!(collection.id(ctx))),
    );
    Some(Ok((feature_id, result)))
}

fn closed_edge_items(
    ctx: &DecodeContext<'_>,
    token: &str,
    items: &PmDcReferenceList,
    index: &ProjectionIndex<'_>,
) -> Result<bool, CodecError> {
    if items.references().is_empty() {
        return Ok(false);
    }
    for reference in ctx.admit_iter(items.references(), "check Inventor closed edge items")? {
        let Some(property) = resolve_property(ctx, token, reference.index(), index)? else {
            return Ok(false);
        };
        if !matches!(&property.kind, PmDcFeaturePropertyKind::EdgeItem { index_references, .. } if !index_references.values().is_empty())
        {
            return Ok(false);
        }
    }
    Ok(true)
}

fn slot_property<'a>(
    ctx: &DecodeContext<'_>,
    source: &PmDcFeature,
    slot: usize,
    index: &'a ProjectionIndex<'a>,
) -> Result<Option<&'a PmDcFeatureProperty>, CodecError> {
    let Some(reference) = source.properties.references().get(slot) else {
        return Ok(None);
    };
    resolve_property(
        ctx,
        source.identity.segment_token.as_str(),
        reference.index(),
        index,
    )
}

fn resolve_property<'a>(
    ctx: &DecodeContext<'_>,
    token: &str,
    reference: u32,
    index: &'a ProjectionIndex<'a>,
) -> Result<Option<&'a PmDcFeatureProperty>, CodecError> {
    let Some(ordinal) = reference.checked_sub(1) else {
        return Ok(None);
    };
    Ok(ctx
        .get_hash_map(
            &index.properties,
            &(token, ordinal),
            "resolve Inventor feature property",
        )?
        .and_then(Option::as_ref)
        .copied())
}

fn references<'a>(
    ctx: &DecodeContext<'_>,
    source: &PmDcFeature,
    slot: usize,
    family: PmDcFeatureReferenceFamily,
    index: &'a ProjectionIndex<'a>,
) -> Result<Option<&'a PmDcReferenceList>, CodecError> {
    Ok(
        slot_property(ctx, source, slot, index)?.and_then(|property| match &property.kind {
            PmDcFeaturePropertyKind::References {
                family: actual,
                items,
            } if *actual == family => Some(items),
            _ => None,
        }),
    )
}

fn enum16(
    ctx: &DecodeContext<'_>,
    source: &PmDcFeature,
    slot: usize,
    family: PmDcFeatureEnumFamily,
    index: &ProjectionIndex<'_>,
) -> Result<Option<u16>, CodecError> {
    let expected_type = match family {
        PmDcFeatureEnumFamily::PartOperation => 5,
        PmDcFeatureEnumFamily::Extent => 11,
        PmDcFeatureEnumFamily::Hole => 3,
        PmDcFeatureEnumFamily::Fillet | PmDcFeatureEnumFamily::Chamfer => 2,
        PmDcFeatureEnumFamily::Auxiliary => return Ok(None),
    };
    Ok(
        slot_property(ctx, source, slot, index)?.and_then(|property| match property.kind {
            PmDcFeaturePropertyKind::Enumeration {
                family: actual,
                type_value,
                value,
            } if actual == family && type_value == expected_type => Some(value),
            _ => None,
        }),
    )
}

fn boolean(
    ctx: &DecodeContext<'_>,
    source: &PmDcFeature,
    slot: usize,
    index: &ProjectionIndex<'_>,
) -> Result<Option<bool>, CodecError> {
    Ok(
        slot_property(ctx, source, slot, index)?.and_then(|property| match property.kind {
            PmDcFeaturePropertyKind::Boolean { value, .. } => Some(value),
            _ => None,
        }),
    )
}

fn resolve_direction<'a>(
    ctx: &DecodeContext<'_>,
    source: &PmDcFeature,
    slot: usize,
    index: &'a ProjectionIndex<'a>,
) -> Result<Option<&'a crate::sketch::PmDcDirection>, CodecError> {
    let Some(reference) = source.properties.references().get(slot) else {
        return Ok(None);
    };
    let Some(ordinal) = reference.index().checked_sub(1) else {
        return Ok(None);
    };
    Ok(ctx
        .get_hash_map(
            &index.directions,
            &(source.identity.segment_token.as_str(), ordinal),
            "resolve Inventor feature direction",
        )?
        .and_then(Option::as_ref)
        .copied())
}

fn length_parameter(
    ctx: &DecodeContext<'_>,
    source: &PmDcFeature,
    slot: usize,
    index: &ProjectionIndex<'_>,
) -> Result<Option<Length>, CodecError> {
    let Some(reference) = source.properties.references().get(slot) else {
        return Ok(None);
    };
    length_reference(
        ctx,
        source.identity.segment_token.as_str(),
        reference.index(),
        index,
    )
}

fn length_reference(
    ctx: &DecodeContext<'_>,
    token: &str,
    reference: u32,
    index: &ProjectionIndex<'_>,
) -> Result<Option<Length>, CodecError> {
    let Some(ordinal) = reference.checked_sub(1) else {
        return Ok(None);
    };
    let Some(parameter) = ctx
        .get_hash_map(
            &index.parameters,
            &(token, ordinal),
            "resolve Inventor feature length parameter",
        )?
        .and_then(Option::as_ref)
    else {
        return Ok(None);
    };
    let mut storage = ctx.reserve_scoped(0, "resolve Inventor feature parameter identity")?;
    let native = storage.with_storage(|| parameter.id(ctx))?;
    Ok(
        match ctx.get_hash_map(
            &index.parameter_values,
            native.as_str(),
            "resolve Inventor feature parameter value",
        )? {
            Some(ParameterValue::Length(value)) if value.get() >= 0.0 => Some(*value),
            _ => None,
        },
    )
}

fn angle_parameter(
    ctx: &DecodeContext<'_>,
    source: &PmDcFeature,
    slot: usize,
    index: &ProjectionIndex<'_>,
) -> Result<Option<Angle>, CodecError> {
    let Some(reference) = source.properties.references().get(slot) else {
        return Ok(None);
    };
    let Some(ordinal) = reference.index().checked_sub(1) else {
        return Ok(None);
    };
    let Some(parameter) = ctx
        .get_hash_map(
            &index.parameters,
            &(source.identity.segment_token.as_str(), ordinal),
            "resolve Inventor feature angle parameter",
        )?
        .and_then(Option::as_ref)
    else {
        return Ok(None);
    };
    let mut storage = ctx.reserve_scoped(0, "resolve Inventor feature parameter identity")?;
    let native = storage.with_storage(|| parameter.id(ctx))?;
    Ok(
        match ctx.get_hash_map(
            &index.parameter_values,
            native.as_str(),
            "resolve Inventor feature parameter value",
        )? {
            Some(ParameterValue::Angle(value)) => Some(*value),
            _ => None,
        },
    )
}

fn boolean_properties(
    ctx: &DecodeContext<'_>,
    source: &PmDcFeature,
    slots: &[usize],
    index: &ProjectionIndex<'_>,
) -> Result<BTreeMap<cadmpeg_core::text::NonBlankString, String>, CodecError> {
    let mut properties = BTreeMap::new();
    for slot in ctx.admit_iter(slots, "visit Inventor feature items")? {
        if let Some(value) = boolean(ctx, source, *slot, index)? {
            ctx.charge_retained(
                if value { 4 } else { 5 },
                "retain Inventor feature property value",
            )?;
            ctx.insert_btree_map(
                &mut properties,
                cadmpeg_core::nonblank_literal!(ctx, "property_{slot}_boolean")?,
                value.to_string(),
                "project Inventor feature boolean property",
            )?;
        }
    }
    Ok(properties)
}

pub(crate) type PmDcFeatureProperty = Located<PmDcFeaturePropertyPayload>;

impl RecordPayload for PmDcFeaturePropertyPayload {
    const KIND: &'static str = "feature-property";
}

pub(crate) type PmDcPatternFeature = Located<PmDcPatternFeaturePayload>;

impl RecordPayload for PmDcPatternFeaturePayload {
    const KIND: &'static str = "pattern-feature";
}

pub(crate) type PmDcFeatureLabel = Located<PmDcFeatureLabelPayload>;

impl RecordPayload for PmDcFeatureLabelPayload {
    const KIND: &'static str = "feature-label";
}

pub(crate) type PmDcEntityStyleLink = Located<PmDcEntityStyleLinkPayload>;

impl RecordPayload for PmDcEntityStyleLinkPayload {
    const KIND: &'static str = "entity-style-link";
}

pub(crate) type PmDcFeature = Located<PmDcFeaturePayload>;

impl RecordPayload for PmDcFeaturePayload {
    const KIND: &'static str = "feature";
}

pub(crate) type PmDcFeatureTerminator = Located<PmDcFeatureTerminatorPayload>;

impl RecordPayload for PmDcFeatureTerminatorPayload {
    const KIND: &'static str = "feature-terminator";
}

#[cfg(test)]
mod tests {
    use super::{
        inventory, parse_boolean, parse_boundary_patch, parse_chamfer, parse_edge_item,
        parse_entity_style_link, parse_feature, parse_fillet_edge_selection, parse_fillet_edge_set,
        parse_label, parse_part_operation, parse_pattern_feature, parse_placement,
        parse_profile_selection, parse_rdx_variable, parse_surface_body, parse_terminator,
        project_chamfer, project_extrusion, project_fillet, project_hole, ClassId,
        PmDcEntityStyleLink, PmDcEntityStyleLinkPayload, PmDcFeature, PmDcFeatureEnumFamily,
        PmDcFeatureLabel, PmDcFeatureLabelPayload, PmDcFeatureLabelPayloadWire, PmDcFeaturePayload,
        PmDcFeatureProperty, PmDcFeaturePropertyKind, PmDcFeaturePropertyPayload,
        PmDcFeatureReferenceFamily, PmDcLinkedHeader, PmDcPatternFamily, ProjectionIndex,
        BOOLEAN_TYPE, CHAMFER_CLASS_ID, END_OF_FEATURES_TYPE, ENTITY_STYLE_LINK_TYPE,
        EXTRUSION_CLASS_ID, FEATURE_LABEL_TYPE, FEATURE_TYPE, FILLET_CLASS_ID, HOLE_CLASS_ID,
        MIRROR_FEATURE_TYPE, RECTANGULAR_PATTERN_FEATURE_TYPE,
    };
    use crate::container::InventorContainer;
    use crate::pmdc::{PmDcContentHeader, PmDcReferenceList, PmDcU32List};
    use crate::record_identity::Located;
    use crate::rse::{RecordFrameState, SegmentBulkState, SegmentKind};
    use crate::test_support::test_fixtures::{content, parse, primary_envelope_fixture};
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension, View};
    use cadmpeg_core::CodecError;
    use cadmpeg_ir::features::{
        edge_treatments::{ChamferSpec, RadiusSpec},
        holes::{HoleKind, HolePlacement},
        BooleanOp, DesignParameter, ExtrudeDirection, ExtrudeExtent, ExtrudeSide, Feature,
        FeatureDefinition, FeatureOperation, FeatureResultTopology, LinearTermination,
    };
    use cadmpeg_ir::features::{ParameterId, ParameterValue};
    use cadmpeg_ir::math::{Point3, Vector3};
    use cadmpeg_ir::scalar::{Angle, Length};
    use cadmpeg_ir::sketches::Sketch;
    use cadmpeg_ir::sketches::{SketchId, SketchPlacement};
    use std::collections::BTreeMap;

    #[test]
    fn feature_projection_refuses_collection_limit_before_token_index() {
        let inventory = super::FeatureInventory {
            features: vec![test_feature(0, 0, &[])],
            pattern_features: Vec::new(),
            terminators: Vec::new(),
            properties: Vec::new(),
            labels: Vec::new(),
            entity_style_links: Vec::new(),
            issues: Vec::new(),
        };
        let design = crate::design::DesignInventory {
            parameters: Vec::new(),
            expressions: Vec::new(),
            units: Vec::new(),
            issues: Vec::new(),
        };
        let sketch = crate::sketch::SketchInventory {
            sketches: Vec::new(),
            entities: Vec::new(),
            transforms: Vec::new(),
            directions: Vec::new(),
            constraints: Vec::new(),
            issues: Vec::new(),
        };
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_collection_items = 0;
        let (ctx, _) =
            DecodeContext::from_root_bytes(&[], &arena, &policy).expect("projection context");
        assert!(matches!(
            super::project(&ctx, &inventory, &design, &sketch, &[], &[]),
            Err(CodecError::ResourceLimit(limit))
                if limit.dimension == ResourceDimension::CollectionItems
                    && limit.operation == "index Inventor feature token"
        ));
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &DecodePolicy::service())
            .expect("service projection context");
        assert_eq!(
            super::project(&ctx, &inventory, &design, &sketch, &[], &[])
                .expect("feature projection")
                .unresolved_features,
            1
        );
    }

    #[test]
    fn feature_result_refuses_entity_limit_before_creation() {
        let source = test_feature(0, 1, &[(0, 1)]);
        let properties = [
            test_property(
                1,
                PmDcFeaturePropertyKind::References {
                    family: PmDcFeatureReferenceFamily::ObjectCollection,
                    items: reference_list(&[3]),
                },
            ),
            test_property(
                2,
                PmDcFeaturePropertyKind::SurfaceBody { body: reference(0) },
            ),
        ];
        let index = test_projection_index(&properties, &[], &[], &[], &[], &[], &[], &[]);
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_entities = 0;
        let (ctx, _) =
            DecodeContext::from_root_bytes(&[], &arena, &policy).expect("projection context");
        assert!(matches!(
            super::feature_result(&ctx, &source, 0, &index),
            Some(Err(CodecError::ResourceLimit(limit)))
                if limit.dimension == ResourceDimension::Entities
                    && limit.operation == "project Inventor feature result topology"
        ));
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &DecodePolicy::service())
            .expect("service projection context");
        assert!(matches!(
            super::feature_result(&ctx, &source, 0, &index),
            Some(Ok(_))
        ));
    }

    #[test]
    fn duplicate_feature_result_members_skip_without_entity_charge() {
        let source = test_feature(0, 1, &[(0, 1)]);
        let properties = [
            test_property(
                1,
                PmDcFeaturePropertyKind::References {
                    family: PmDcFeatureReferenceFamily::ObjectCollection,
                    items: reference_list(&[3, 3]),
                },
            ),
            test_property(
                2,
                PmDcFeaturePropertyKind::SurfaceBody { body: reference(0) },
            ),
        ];
        let index = test_projection_index(&properties, &[], &[], &[], &[], &[], &[], &[]);
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_entities = 0;
        let (ctx, _) =
            DecodeContext::from_root_bytes(&[], &arena, &policy).expect("projection context");
        assert!(super::feature_result(&ctx, &source, 0, &index).is_none());
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &DecodePolicy::service())
            .expect("service projection context");
        assert!(super::feature_result(&ctx, &source, 0, &index).is_none());
    }

    #[test]
    fn feature_result_refuses_collection_limit_before_distinct_precheck() {
        let source = test_feature(0, 1, &[(0, 1)]);
        let properties = [
            test_property(
                1,
                PmDcFeaturePropertyKind::References {
                    family: PmDcFeatureReferenceFamily::ObjectCollection,
                    items: reference_list(&[3, 3]),
                },
            ),
            test_property(
                2,
                PmDcFeaturePropertyKind::SurfaceBody { body: reference(0) },
            ),
        ];
        let index = test_projection_index(&properties, &[], &[], &[], &[], &[], &[], &[]);
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_collection_items = 2;
        let (ctx, _) =
            DecodeContext::from_root_bytes(&[], &arena, &policy).expect("projection context");
        assert!(matches!(
            super::feature_result(&ctx, &source, 0, &index),
            Some(Err(CodecError::ResourceLimit(limit)))
                if limit.dimension == ResourceDimension::CollectionItems
                    && limit.operation == "precheck distinct Inventor feature result bodies"
        ));
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &DecodePolicy::service())
            .expect("service projection context");
        assert!(super::feature_result(&ctx, &source, 0, &index).is_none());
    }

    #[test]
    fn feature_projection_refuses_entity_limit_before_creation() {
        let source = test_feature(0, 0, &[]);
        let label = test_label(0, 1, EXTRUSION_CLASS_ID, &[]);
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_entities = 0;
        let (ctx, _) =
            DecodeContext::from_root_bytes(&[], &arena, &policy).expect("projection context");
        assert!(matches!(
            super::admit_projected_feature(&ctx, &source, &label, "extrude"),
            Err(CodecError::ResourceLimit(limit))
                if limit.dimension == ResourceDimension::Entities
                    && limit.operation == "project Inventor feature"
        ));
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &DecodePolicy::service())
            .expect("service projection context");
        assert!(super::admit_projected_feature(&ctx, &source, &label, "extrude").is_ok());
    }

    fn inventory_with_record(
        type_id: [u8; 16],
        payload: &[u8],
        policy: DecodePolicy,
    ) -> Result<super::FeatureInventory, CodecError> {
        let bytes = primary_envelope_fixture();
        let payload = payload.to_vec();
        let arena = DecodeArena::new();
        let (setup_ctx, source) =
            DecodeContext::from_root_bytes(&bytes, &arena, &DecodePolicy::service())
                .expect("envelope view");
        let mut container = InventorContainer::open(&setup_ctx, source).expect("framed envelope");
        let segment = &mut container.rse.segments[0];
        segment.kind = SegmentKind::PmDc;
        let SegmentBulkState::Framed(bulk) = &mut segment.bulk else {
            panic!("framed bulk fixture");
        };
        let RecordFrameState::Framed(table) = &mut bulk.records else {
            panic!("framed record fixture");
        };
        table.records[0].type_id = type_id;
        table.records[0].payload = View::over_retained(&payload);
        let (ctx, _) = DecodeContext::from_root_bytes(&bytes, &arena, &policy).expect("input view");
        inventory(&ctx, &container.rse)
    }

    #[test]
    fn feature_terminator_refuses_collection_limit_before_push() {
        let mut payload = content(0);
        payload.extend_from_slice(&0u32.to_le_bytes());
        assert_eq!(
            inventory_with_record(END_OF_FEATURES_TYPE, &payload, DecodePolicy::service())
                .expect("terminator is admitted")
                .terminators
                .len(),
            1
        );
        let mut policy = DecodePolicy::service();
        policy.limits.max_collection_items = 0;
        assert!(matches!(
            inventory_with_record(END_OF_FEATURES_TYPE, &payload, policy),
            Err(CodecError::ResourceLimit(limit))
                if limit.dimension == ResourceDimension::CollectionItems
                    && limit.operation == "admit Inventor PmDc feature terminator record"
                    && limit.used == 0
        ));
    }

    #[test]
    fn feature_terminator_refuses_retained_limit_before_identity_copy() {
        let mut payload = content(0);
        payload.extend_from_slice(&0u32.to_le_bytes());
        let mut policy = DecodePolicy::service();
        policy.limits.max_retained_bytes = 31;
        assert!(matches!(
            inventory_with_record(END_OF_FEATURES_TYPE, &payload, policy),
            Err(CodecError::ResourceLimit(limit))
                if limit.dimension == ResourceDimension::RetainedBytes
                    && limit.operation == "retain Inventor PmDc record type id"
                    && limit.used == 0
        ));
        policy.limits.max_retained_bytes = 32;
        assert!(matches!(
            inventory_with_record(END_OF_FEATURES_TYPE, &payload, policy),
            Err(CodecError::ResourceLimit(limit))
                if limit.dimension == ResourceDimension::RetainedBytes
                    && limit.operation == "retain Inventor PmDc record segment token"
                    && limit.used == 32
        ));
    }

    #[test]
    fn feature_parse_issue_refuses_collection_limit_before_push() {
        assert_eq!(
            inventory_with_record(END_OF_FEATURES_TYPE, &[], DecodePolicy::service())
                .expect("truncated terminator becomes an issue")
                .issues
                .len(),
            1
        );
        let mut policy = DecodePolicy::service();
        policy.limits.max_collection_items = 0;
        assert!(matches!(
            inventory_with_record(END_OF_FEATURES_TYPE, &[], policy),
            Err(CodecError::ResourceLimit(limit))
                if limit.dimension == ResourceDimension::CollectionItems
                    && limit.operation == "admit Inventor PmDc feature issue"
                    && limit.used == 0
        ));
    }

    #[test]
    fn feature_parse_issue_refuses_entity_limit_before_push() {
        let mut policy = DecodePolicy::service();
        policy.limits.max_entities = 0;
        assert!(matches!(
            inventory_with_record(END_OF_FEATURES_TYPE, &[], policy),
            Err(CodecError::ResourceLimit(limit))
                if limit.dimension == ResourceDimension::Entities
                    && limit.operation == "admit Inventor PmDc feature issue"
        ));
        assert_eq!(
            inventory_with_record(END_OF_FEATURES_TYPE, &[], DecodePolicy::service())
                .expect("service issue")
                .issues
                .len(),
            1
        );
    }

    #[test]
    fn feature_parse_issue_copies_refuse_retained_limits_before_creation() {
        let admitted = inventory_with_record(END_OF_FEATURES_TYPE, &[], DecodePolicy::service())
            .expect("truncated terminator becomes an issue");
        let issue = &admitted.issues[0];
        let detail_len = issue.detail.len();
        let segment_token_len = issue.segment_token.as_str().len();
        // Construction retains the 32-byte type id, token, then detail.
        for (limit_bytes, operation, used) in [
            (32 - 1, "retain Inventor PmDc feature issue type id", 0),
            (
                32 + segment_token_len - 1,
                "retain Inventor PmDc feature issue segment token",
                32,
            ),
            (
                32 + segment_token_len + detail_len - 1,
                "retain Inventor PmDc feature issue detail",
                32 + segment_token_len,
            ),
        ] {
            let mut policy = DecodePolicy::service();
            policy.limits.max_retained_bytes = cadmpeg_core::decode::u64_from_index(limit_bytes);
            assert!(matches!(
                inventory_with_record(END_OF_FEATURES_TYPE, &[], policy),
                Err(CodecError::ResourceLimit(limit))
                    if limit.dimension == ResourceDimension::RetainedBytes
                        && limit.operation == operation
                        && limit.used == cadmpeg_core::decode::u64_from_index(used)
            ));
        }
    }

    fn segment() -> cadmpeg_ir::ids::IdentityKey {
        cadmpeg_ir::identity_key!("generated")
    }

    fn references(values: &[u32]) -> Vec<u8> {
        let mut bytes = Vec::new();
        bytes.extend_from_slice(&2u16.to_le_bytes());
        bytes.extend_from_slice(&0x3000u16.to_le_bytes());
        bytes.extend_from_slice(
            &(u32::try_from(values.len()).expect("fixture value fits u32")).to_le_bytes(),
        );
        if !values.is_empty() {
            bytes.extend_from_slice(
                &(u32::try_from(values.len()).expect("fixture value fits u32")).to_le_bytes(),
            );
            bytes.extend_from_slice(&0u32.to_le_bytes());
        }
        for value in values {
            bytes.extend_from_slice(&value.to_le_bytes());
        }
        bytes
    }

    fn utf16(value: &str) -> Vec<u8> {
        let units = value.encode_utf16().collect::<Vec<_>>();
        let mut bytes = (u32::try_from(units.len()).expect("fixture value fits u32"))
            .to_le_bytes()
            .to_vec();
        for unit in units {
            bytes.extend_from_slice(&unit.to_le_bytes());
        }
        bytes
    }

    fn reference(index: u32) -> crate::pmdc::PmDcReference {
        crate::pmdc::PmDcReference::new(index, index != 0)
            .expect("test reference index fits 31 bits")
    }

    fn reference_list(values: &[u32]) -> PmDcReferenceList {
        PmDcReferenceList::new(
            2,
            (!values.is_empty()).then_some(crate::pmdc::PmDcListMetadata::U32([
                u32::try_from(values.len()).expect("fixture value fits u32"),
                0,
            ])),
            values.iter().copied().map(reference).collect(),
        )
        .expect("test list metadata matches length")
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
        Located::new(
            PmDcFeaturePropertyPayload {
                save_version_major: 16,
                header: test_header(),
                kind,
            },
            crate::record_identity::RecordTypeId::try_from(format!("{ordinal:032x}"))
                .expect("test GUID"),
            segment()
                .try_clone_for_decode(
                    &cadmpeg_test_support::service_decode_context(),
                    "Inventor located fixture token",
                )
                .expect("service fixture token"),
            ordinal,
        )
    }

    fn test_feature(ordinal: u32, slot_count: usize, slots: &[(usize, u32)]) -> PmDcFeature {
        let mut references = vec![reference(0); slot_count];
        for (slot, record_ordinal) in slots {
            references[*slot] = reference(record_ordinal + 1);
        }
        Located::new(
            PmDcFeaturePayload {
                save_version_major: 16,
                header: test_header(),
                state: 69,
                outline_value: 0,
                properties: PmDcReferenceList::new(
                    2,
                    (!references.is_empty()).then_some(crate::pmdc::PmDcListMetadata::U32([
                        u32::try_from(slot_count).expect("fixture value fits u32"),
                        0,
                    ])),
                    references,
                )
                .expect("test list metadata matches length"),
                value: 0,
            },
            test_type_id(FEATURE_TYPE),
            segment()
                .try_clone_for_decode(
                    &cadmpeg_test_support::service_decode_context(),
                    "Inventor located fixture token",
                )
                .expect("service fixture token"),
            ordinal,
        )
    }

    fn test_type_id(value: [u8; 16]) -> crate::record_identity::RecordTypeId {
        let ctx = cadmpeg_test_support::service_decode_context();
        crate::record_identity::RecordTypeId::from_bytes(
            &ctx,
            value,
            "retain Inventor PmDc test record type id",
        )
        .expect("service fixture type id")
    }

    fn decode_label(value: serde_json::Value) -> Result<PmDcFeatureLabel, String> {
        let ctx = cadmpeg_test_support::service_decode_context();
        let wire = serde_json::from_value::<
            crate::record_identity::LocatedWire<PmDcFeatureLabelPayloadWire>,
        >(value)
        .map_err(|error| error.to_string())?;
        crate::record_identity::LocatedWire {
            id: wire.id,
            type_id: wire.type_id,
            segment_token: wire.segment_token,
            record_ordinal: wire.record_ordinal,
            value: wire
                .value
                .into_record(&ctx)
                .map_err(|error| error.to_string())?,
        }
        .into_record(&ctx)
        .map_err(|error| error.to_string())
    }

    #[test]
    fn located_label_admission_rejects_empty_name_and_wrong_class_width() {
        let label = test_label(0, 1, EXTRUSION_CLASS_ID, &[]);
        let valid = serde_json::to_value(&label).expect("valid label fixture");
        let admitted = decode_label(valid.clone()).expect("valid label fixture");
        assert_eq!(
            serde_json::to_value(admitted).expect("valid label fixture"),
            valid
        );
        for (field, value) in [
            ("name", String::new()),
            ("class_id", "a".repeat(31)),
            ("class_id", "a".repeat(33)),
        ] {
            let mut wire = valid.clone();
            wire[field] = serde_json::json!(value);
            assert!(decode_label(wire)
                .expect_err("invalid label")
                .contains(field));
        }
        let mut wire = valid;
        wire["class_id"] = serde_json::json!("z".repeat(32));
        assert!(decode_label(wire).is_err());
    }

    fn test_label(
        owner_ordinal: u32,
        index: u32,
        class_id: ClassId,
        participants: &[u32],
    ) -> PmDcFeatureLabel {
        let ctx = cadmpeg_test_support::service_decode_context();
        let payload = PmDcFeatureLabelPayloadWire::<String> {
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
            participants: reference_list(participants),
            name: format!("Feature {index}"),
            class_id: class_id
                .into_text(&ctx, "retain Inventor feature label fixture class id")
                .expect("service fixture class id"),
        }
        .into_record(&ctx)
        .expect("valid label fixture");
        Located::new(
            payload,
            test_type_id(FEATURE_LABEL_TYPE),
            segment()
                .try_clone_for_decode(&ctx, "Inventor located fixture token")
                .expect("service fixture token"),
            owner_ordinal + 1000,
        )
    }

    fn raw_parameter(ordinal: u32) -> crate::design::PmDcParameter {
        Located::new(
            crate::design::PmDcParameterPayload {
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
                nominal_value: cadmpeg_ir::scalar::FiniteReal::ZERO,
                model_value: cadmpeg_ir::scalar::FiniteReal::ZERO,
                tolerance: 0,
                terminal_value: 0,
            },
            crate::record_identity::RecordTypeId::try_from(
                "264d8790d011f8d10008cabc0663dc09".to_owned(),
            )
            .expect("test GUID"),
            segment()
                .try_clone_for_decode(
                    &cadmpeg_test_support::service_decode_context(),
                    "Inventor located fixture token",
                )
                .expect("service fixture token"),
            ordinal,
        )
    }

    fn neutral_parameter(
        raw: &crate::design::PmDcParameter,
        value: ParameterValue,
    ) -> DesignParameter {
        let ctx = cadmpeg_test_support::service_decode_context();
        DesignParameter {
            id: ParameterId::mint(format!(
                "inventor:design:parameter#{}",
                raw.identity.record_ordinal
            ))
            .expect("identity grammar"),
            owner: None,
            ordinal: raw.identity.record_ordinal,
            name: raw.name.clone(),
            expression: String::new(),
            display: None,
            value: Some(value),
            dependencies: cadmpeg_ir::features::DistinctMembers::default(),
            properties: BTreeMap::new(),
            pmi: None,
            native_ref: Some(raw.id(&ctx).expect("service fixture record identity")),
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
                        (
                            record.identity.segment_token.as_str(),
                            record.identity.record_ordinal,
                        ),
                        Some(record),
                    )
                })
                .collect(),
            parameters: parameters
                .iter()
                .map(|record| {
                    (
                        (
                            record.identity.segment_token.as_str(),
                            record.identity.record_ordinal,
                        ),
                        Some(record),
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
                        (
                            record.identity.segment_token.as_str(),
                            record.identity.record_ordinal,
                        ),
                        Some(record),
                    )
                })
                .collect(),
            sketch_ids: neutral_sketches
                .iter()
                .filter_map(|sketch| {
                    sketch
                        .native_ref
                        .as_deref()
                        .map(|native| (native, &sketch.id))
                })
                .collect(),
            directions: directions
                .iter()
                .map(|record| {
                    (
                        (
                            record.identity.segment_token.as_str(),
                            record.identity.record_ordinal,
                        ),
                        Some(record),
                    )
                })
                .collect(),
            transforms: transforms
                .iter()
                .map(|record| {
                    (
                        (
                            record.identity.segment_token.as_str(),
                            record.identity.record_ordinal,
                        ),
                        Some(record),
                    )
                })
                .collect(),
            entity_style_links: entity_style_links
                .iter()
                .map(|record| {
                    (
                        record.identity.segment_token.as_str(),
                        record.identity.record_ordinal,
                    )
                })
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
        assert_eq!(parsed.properties.references().len(), 2);
        assert!(parsed.properties.references()[0].qualified());
        assert_eq!(parsed.value, 9);

        let mut terminator = content(8);
        terminator.extend_from_slice(&(-1i32).to_le_bytes());
        let parsed = parse(&terminator, |_, source| {
            parse_terminator(source, 16).expect("terminator")
        });
        assert_eq!(parsed.state, -1);
    }

    fn pattern_feature_bytes(version: u8, family: PmDcPatternFamily) -> Vec<u8> {
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
    }

    fn feature_label_bytes() -> Vec<u8> {
        let mut label = Vec::new();
        label.extend_from_slice(&0u32.to_le_bytes());
        label.extend_from_slice(&18u16.to_le_bytes());
        label.extend_from_slice(&[0; 20]);
        label.extend_from_slice(&0u32.to_le_bytes());
        label.extend(references(&[]));
        label.extend(utf16("a"));
        label.extend_from_slice(&[0xab; 16]);
        label
    }

    #[test]
    fn feature_label_class_id_refuses_materialized_limit_before_hex_copy() {
        let bytes = feature_label_bytes();
        let arena = DecodeArena::new();
        let (ctx, source) =
            DecodeContext::from_root_bytes(&bytes, &arena, &DecodePolicy::service())
                .expect("label view");
        assert_eq!(
            parse_label(&ctx, source, 22)
                .expect("label is admitted")
                .name
                .as_str(),
            "a"
        );
        let mut policy = DecodePolicy::service();
        policy.limits.max_materialized_bytes = 31;
        let (ctx, source) =
            DecodeContext::from_root_bytes(&bytes, &arena, &policy).expect("label view");
        assert!(matches!(
            parse_label(&ctx, source, 22),
            Err(CodecError::ResourceLimit(limit))
                if limit.dimension == ResourceDimension::MaterializedBytes
                    && limit.operation == "retain Inventor feature label class id"
                    && limit.used == 0
                    && limit.additional == 32
        ));
    }

    #[test]
    fn feature_record_forms_refuse_collection_limit_before_push() {
        let mut feature = content(0);
        feature.extend_from_slice(&[0; 8]);
        feature.extend(references(&[]));
        feature.extend_from_slice(&0u32.to_le_bytes());
        let mut property = content(0);
        property.extend(utf16("a"));
        property.extend_from_slice(&0u32.to_le_bytes());
        property.push(1);
        let mut entity_link = Vec::new();
        entity_link.extend_from_slice(&0u32.to_le_bytes());
        entity_link.extend_from_slice(&16u16.to_le_bytes());
        entity_link.extend_from_slice(&[0; 32]);
        let cases = [
            (
                FEATURE_TYPE,
                feature,
                0,
                "admit Inventor PmDc feature record",
            ),
            (
                RECTANGULAR_PATTERN_FEATURE_TYPE,
                pattern_feature_bytes(16, PmDcPatternFamily::Rectangular),
                28,
                "admit Inventor PmDc pattern feature record",
            ),
            (
                MIRROR_FEATURE_TYPE,
                pattern_feature_bytes(16, PmDcPatternFamily::Mirror),
                15,
                "admit Inventor PmDc pattern feature record",
            ),
            (
                FEATURE_LABEL_TYPE,
                feature_label_bytes(),
                0,
                "admit Inventor PmDc feature label record",
            ),
            (
                ENTITY_STYLE_LINK_TYPE,
                entity_link,
                0,
                "admit Inventor PmDc entity style link record",
            ),
            (
                BOOLEAN_TYPE,
                property,
                0,
                "admit Inventor PmDc feature property record",
            ),
        ];
        for (type_id, payload, parser_items, operation) in cases {
            let admitted = inventory_with_record(type_id, &payload, DecodePolicy::service())
                .expect("feature record is admitted");
            assert_eq!(
                admitted.features.len()
                    + admitted.pattern_features.len()
                    + admitted.labels.len()
                    + admitted.entity_style_links.len()
                    + admitted.properties.len(),
                1,
                "{operation}"
            );
            assert!(admitted.issues.is_empty());
            let mut policy = DecodePolicy::service();
            policy.limits.max_collection_items = parser_items;
            assert!(matches!(
                inventory_with_record(type_id, &payload, policy),
                Err(CodecError::ResourceLimit(limit))
                    if limit.dimension == ResourceDimension::CollectionItems
                        && limit.operation == operation
                        && limit.used == parser_items
            ));
        }
    }

    #[test]
    fn parses_generated_pattern_feature_branches() {
        for (version, family, slots, extensions) in [
            (16, PmDcPatternFamily::Rectangular, 26, 0),
            (21, PmDcPatternFamily::Rectangular, 32, 0),
            (16, PmDcPatternFamily::Mirror, 13, 0),
            (21, PmDcPatternFamily::Mirror, 13, 6),
        ] {
            let bytes = pattern_feature_bytes(version, family);
            let parsed = parse(&bytes, |ctx, source| {
                parse_pattern_feature(ctx, source, version, family).expect("pattern feature")
            });
            assert_eq!(parsed.family, family);
            assert_eq!(parsed.participants.references().len(), 2);
            assert_eq!(parsed.property_slots.len(), slots);
            assert_eq!(parsed.extension_values.len(), extensions);
        }
    }

    #[test]
    fn pattern_feature_property_slots_refuse_collection_limit_before_allocation() {
        for (version, family, slots, extension_slots) in [
            (16, PmDcPatternFamily::Rectangular, 26, 0),
            (21, PmDcPatternFamily::Rectangular, 32, 0),
            (16, PmDcPatternFamily::Mirror, 13, 0),
            (21, PmDcPatternFamily::Mirror, 13, 6),
        ] {
            let bytes = pattern_feature_bytes(version, family);
            let mut policy = DecodePolicy::service();
            let collection_limit = 2 + slots + extension_slots - 1;
            // Two participants, property slots, and extension slots precede the final property push.
            policy.limits.max_collection_items = collection_limit;
            let arena = DecodeArena::new();
            let (ctx, source) = DecodeContext::from_root_bytes(&bytes, &arena, &policy)
                .expect("pattern feature view");
            assert!(matches!(
                parse_pattern_feature(&ctx, source, version, family),
                Err(CodecError::ResourceLimit(limit))
                    if limit.dimension == ResourceDimension::CollectionItems
                        && limit.operation == "admit Inventor pattern feature property slots"
                        && limit.used == collection_limit
            ));
        }
    }

    #[test]
    fn pattern_feature_extension_values_refuse_collection_limit_before_allocation() {
        let bytes = pattern_feature_bytes(21, PmDcPatternFamily::Mirror);
        let mut policy = DecodePolicy::service();
        // Two participants and eleven property slots precede the six-slot extension admission.
        policy.limits.max_collection_items = 2 + 11 + 6 - 1;
        let arena = DecodeArena::new();
        let (ctx, source) =
            DecodeContext::from_root_bytes(&bytes, &arena, &policy).expect("pattern feature view");
        assert!(matches!(
            parse_pattern_feature(&ctx, source, 21, PmDcPatternFamily::Mirror),
            Err(CodecError::ResourceLimit(limit))
                    if limit.dimension == ResourceDimension::CollectionItems
                    && limit.operation == "admit Inventor pattern feature extension values"
                    && limit.used == 13
        ));
        let (ctx, source) =
            DecodeContext::from_root_bytes(&bytes, &arena, &DecodePolicy::service())
                .expect("pattern feature view");
        assert_eq!(
            parse_pattern_feature(&ctx, source, 21, PmDcPatternFamily::Mirror)
                .expect("pattern feature is admitted")
                .extension_values
                .len(),
            6
        );
    }

    #[test]
    fn projects_generated_fillet_and_chamfer() {
        let raw_radius = raw_parameter(20);
        let neutral_radius = neutral_parameter(
            &raw_radius,
            ParameterValue::Length(Length::new(2.5).expect("finite length fixture")),
        );
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
                    index_references: PmDcU32List::new(
                        2,
                        Some(crate::pmdc::PmDcListMetadata::U32([1, 0])),
                        vec![42],
                    )
                    .expect("test list metadata matches length"),
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
        let label = test_label(100, 7, FILLET_CLASS_ID, &[]);
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
        let arena = DecodeArena::new();
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &DecodePolicy::service())
            .expect("projection context");
        let (projected, result) = project_fillet(&ctx, &fillet, &label, &index)
            .expect("fillet candidate")
            .expect("fillet projection");
        assert!(matches!(
            projected.evaluation.definition(),
            FeatureDefinition::Operation(FeatureOperation::Fillet { groups })
                if matches!(groups[0].radius, RadiusSpec::Constant { radius: actual_radius } if actual_radius.get() == 2.5)
        ));
        assert_eq!(
            result
                .bodies()
                .iter()
                .map(|id| id.as_str().to_owned())
                .collect::<Vec<_>>(),
            vec![fillet_properties[8]
                .id(&ctx)
                .expect("service fixture record identity")]
        );

        let raw_distance = raw_parameter(40);
        let neutral_distance = neutral_parameter(
            &raw_distance,
            ParameterValue::Length(Length::new(1.25).expect("finite length fixture")),
        );
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
                    index_references: PmDcU32List::new(
                        2,
                        Some(crate::pmdc::PmDcListMetadata::U32([1, 0])),
                        vec![17],
                    )
                    .expect("test list metadata matches length"),
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
        let label = test_label(101, 8, CHAMFER_CLASS_ID, &[]);
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
        let arena = DecodeArena::new();
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &DecodePolicy::service())
            .expect("projection context");
        let (projected, _) = project_chamfer(&ctx, &chamfer, &label, &index)
            .expect("chamfer candidate")
            .expect("chamfer projection");
        assert!(matches!(
            projected.evaluation.definition(),
            FeatureDefinition::Operation(FeatureOperation::Chamfer {
                groups,
                flip_direction: true
            }) if matches!(groups[0].spec, ChamferSpec::Distance { distance: actual_distance } if actual_distance.get() == 1.25)
        ));
    }

    fn generated_extrusion(
        selections: &[u32],
        policy: DecodePolicy,
    ) -> Option<Result<(Feature, FeatureResultTopology), CodecError>> {
        let raw_length = raw_parameter(70);
        let raw_taper = raw_parameter(71);
        let neutral_parameters = vec![
            neutral_parameter(
                &raw_length,
                ParameterValue::Length(Length::new(12.0).expect("finite length fixture")),
            ),
            neutral_parameter(
                &raw_taper,
                ParameterValue::Angle(Angle::new(0.1).expect("finite angle fixture")),
            ),
        ];
        let raw_sketch = Located::new(
            crate::sketch::PmDcSketchPayload {
                save_version_major: 16,
                header: test_header(),
                state: 0,
                count_value: 0,
                entities: PmDcReferenceList::new(8, None, Vec::new()).expect("empty entity list"),
                transform: reference(0),
                direction: reference(0),
                values: [0; 2],
                auxiliary: None,
            },
            crate::record_identity::RecordTypeId::try_from(
                "114d8790d011f8d10008cabc0663dc09".to_owned(),
            )
            .expect("test GUID"),
            segment()
                .try_clone_for_decode(
                    &cadmpeg_test_support::service_decode_context(),
                    "Inventor located fixture token",
                )
                .expect("service fixture token"),
            50,
        );
        let neutral_sketch = Sketch {
            id: SketchId::mint(format!("inventor:design:sketch#{}-50", segment()))
                .expect("valid test fixture"),
            name: None,
            configuration: None,
            visible: None,
            placement: SketchPlacement::try_resolved(
                Point3::new(0.0, 0.0, 0.0),
                Vector3::new(0.0, 0.0, 1.0),
                Vector3::new(1.0, 0.0, 0.0),
            )
            .expect("valid test fixture"),
            profiles: cadmpeg_ir::sketches::SketchProfiles::default(),
            native_ref: Some(
                raw_sketch
                    .id(&cadmpeg_test_support::service_decode_context())
                    .expect("service fixture record identity"),
            ),
        };
        let direction = Located::new(
            crate::sketch::PmDcDirectionPayload {
                save_version_major: 16,
                header: test_header(),
                entity_flags: 0,
                parameter: cadmpeg_ir::scalar::FiniteReal::ZERO,
                extension: None,
                direction: [
                    cadmpeg_ir::scalar::FiniteReal::ZERO,
                    cadmpeg_ir::scalar::FiniteReal::ZERO,
                    cadmpeg_ir::scalar::FiniteReal::ONE,
                ],
            },
            crate::record_identity::RecordTypeId::try_from(
                "40df52ced011d0d20008ccbc0663dc09".to_owned(),
            )
            .expect("test GUID"),
            segment()
                .try_clone_for_decode(
                    &cadmpeg_test_support::service_decode_context(),
                    "Inventor located fixture token",
                )
                .expect("service fixture token"),
            60,
        );
        let entity_link = Located::new(
            PmDcEntityStyleLinkPayload {
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
            },
            test_type_id(ENTITY_STYLE_LINK_TYPE),
            segment()
                .try_clone_for_decode(
                    &cadmpeg_test_support::service_decode_context(),
                    "Inventor located fixture token",
                )
                .expect("service fixture token"),
            51,
        );
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
                    items: reference_list(selections),
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
        let label = test_label(100, 5, EXTRUSION_CLASS_ID, &[51]);
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
        let arena = DecodeArena::new();
        let (ctx, _) =
            DecodeContext::from_root_bytes(&[], &arena, &policy).expect("projection context");
        project_extrusion(&ctx, &feature, &label, &index)
    }

    #[test]
    fn duplicate_extrusion_selections_skip_before_feature_entity_charge() {
        let mut policy = DecodePolicy::service();
        policy.limits.max_entities = 0;
        assert!(generated_extrusion(&[4, 4], policy).is_none());
    }

    #[test]
    fn projects_generated_extrusion() {
        let (projected, _) = generated_extrusion(&[4], DecodePolicy::service())
            .expect("extrusion candidate")
            .expect("extrusion projection");
        assert!(matches!(
            projected.evaluation.definition(),
            FeatureDefinition::Operation(FeatureOperation::Extrude {
                direction: ExtrudeDirection::Explicit {
                    vector: geometry_1,
                    ..
                },
                extent: ExtrudeExtent::OneSided {
                    side: ExtrudeSide {
                        termination: LinearTermination::Blind {
                            length: actual_length
                        },
                        draft: Some(actual_draft),
                        ..
                    }
                },
                op: BooleanOp::NewBody,
                ..
            }) if ( actual_length.get() == 12.0 && actual_draft.get() == 0.1) && matches!(geometry_1.get(), Vector3 { z: -1.0, .. })
        ));
    }

    #[test]
    fn projects_generated_hole() {
        let raw_parameters = (70..76).map(raw_parameter).collect::<Vec<_>>();
        let neutral_parameters = [
            ParameterValue::Length(Length::new(5.0).expect("finite length fixture")),
            ParameterValue::Length(Length::new(20.0).expect("finite length fixture")),
            ParameterValue::Length(Length::new(9.0).expect("finite length fixture")),
            ParameterValue::Length(Length::new(3.0).expect("finite length fixture")),
            ParameterValue::Angle(Angle::new(1.5).expect("finite angle fixture")),
            ParameterValue::Angle(Angle::new(2.0).expect("finite angle fixture")),
        ]
        .into_iter()
        .zip(&raw_parameters)
        .map(|(value, raw)| neutral_parameter(raw, value))
        .collect::<Vec<_>>();
        let transform = Located::new(
            crate::sketch::PmDcTransformPayload {
                save_version_major: 16,
                header: test_header(),
                prefix_present: false,
                matrix: crate::compact_matrix::CompactMatrix::try_from_rows(
                    &cadmpeg_test_support::service_decode_context(),
                    0,
                    0,
                    [
                        [1.0, 0.0, 0.0, 1.0],
                        [0.0, 1.0, 0.0, 2.0],
                        [0.0, 0.0, 1.0, 3.0],
                        [0.0, 0.0, 0.0, 1.0],
                    ],
                )
                .expect("finite explicit matrix fixture"),
            },
            crate::record_identity::RecordTypeId::try_from(
                "184d8790d011f8d10008cabc0663dc09".to_owned(),
            )
            .expect("test GUID"),
            segment()
                .try_clone_for_decode(
                    &cadmpeg_test_support::service_decode_context(),
                    "Inventor located fixture token",
                )
                .expect("service fixture token"),
            60,
        );
        let direction = Located::new(
            crate::sketch::PmDcDirectionPayload {
                save_version_major: 16,
                header: test_header(),
                entity_flags: 0,
                parameter: cadmpeg_ir::scalar::FiniteReal::ZERO,
                extension: None,
                direction: [
                    cadmpeg_ir::scalar::FiniteReal::ZERO,
                    cadmpeg_ir::scalar::FiniteReal::ZERO,
                    cadmpeg_ir::scalar::FiniteReal::ONE.negated(),
                ],
            },
            crate::record_identity::RecordTypeId::try_from(
                "40df52ced011d0d20008ccbc0663dc09".to_owned(),
            )
            .expect("test GUID"),
            segment()
                .try_clone_for_decode(
                    &cadmpeg_test_support::service_decode_context(),
                    "Inventor located fixture token",
                )
                .expect("service fixture token"),
            61,
        );
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
        let label = test_label(100, 9, HOLE_CLASS_ID, &[]);
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
        let arena = DecodeArena::new();
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &DecodePolicy::service())
            .expect("projection context");
        let (projected, _) = project_hole(&ctx, &feature, &label, &index)
            .expect("hole candidate")
            .expect("hole projection");
        assert!(matches!(
            projected.evaluation.definition(), FeatureDefinition::Operation(FeatureOperation::Hole {
                placements,
                shape,

                extent: Some(LinearTermination::ThroughAll {}),
                ..
            }) if matches!((shape.construction(), &shape.diameter(),), (cadmpeg_ir::features::holes::HoleConstruction::Form {
                    kind: HoleKind::CounterboreDrilled {
                        diameter: actual_diameter,
                        depth: actual_depth,
                        drill_point_angle: actual_drill_point_angle
                    },
                    ..
                }, Some(actual_diameter_2),) if (matches!(
                placements.as_deref(),
                Some([HolePlacement::Directed {
                    position: geometry_1,
                    direction: geometry_2
                }])
             if matches!(geometry_1.get(), Point3 { x: 10.0, y: 20.0, z: 30.0 }) && matches!(geometry_2.get(), Vector3 { z: -1.0, .. }))) && actual_diameter.get() == 9.0 && actual_depth.get() == 3.0 && actual_drill_point_angle.get() == 2.0 && actual_diameter_2.get() == 5.0)));
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
                if items.references().len() == 2
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
        assert_eq!(parsed.header.owner.index(), 8);
        assert_eq!(parsed.header.next.index(), 9);
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
            PmDcFeaturePropertyKind::FilletEdgeSet { radius, .. } if radius.index() == 17
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
        assert_eq!(parsed.name.as_str(), "Extrude1");
        assert_eq!(parsed.participants.references().len(), 1);
        assert_eq!(parsed.class_id, ClassId([0xab; 16]));
    }

    #[test]
    fn class_id_admits_only_the_canonical_lowercase_spelling() {
        let lower = "ab".repeat(16);
        let ctx = cadmpeg_test_support::service_decode_context();
        let class_id = ClassId::from_text(&ctx, &lower).expect("lowercase class id");
        assert_eq!(
            class_id
                .into_text(&ctx, "retain Inventor class id test text")
                .expect("service class id text"),
            lower
        );
        let upper = "AB".repeat(16);
        assert!(ClassId::from_text(&ctx, &upper).is_err());
    }

    #[test]
    fn class_id_native_writer_streams_the_existing_hex_bytes() {
        #[derive(serde::Serialize)]
        struct Record<'a> {
            id: &'static str,
            value: &'a ClassId,
        }
        let class_id = ClassId([0xab; 16]);
        let owned = class_id
            .into_text(
                &cadmpeg_test_support::service_decode_context(),
                "retain Inventor class id native writer test text",
            )
            .expect("service class id text");
        assert_eq!(
            serde_json::to_vec(&class_id).expect("borrowed class id"),
            serde_json::to_vec(&owned).expect("owned class id")
        );
        let record = Record {
            id: "inventor:pmdc:class-id#1",
            value: &class_id,
        };
        cadmpeg_test_support::native_serialization::assert_native_limit(
            &record,
            serde_json::json!({"id": record.id, "value": owned}),
        );
    }

    #[test]
    fn feature_label_native_writer_refuses_retained_limit_before_clone() {
        #[derive(serde::Serialize)]
        struct Record<'a> {
            id: &'static str,
            value: &'a PmDcFeatureLabelPayload,
        }
        let reference =
            crate::pmdc::PmDcReference::new(1, false).expect("test reference index fits 31 bits");
        let ctx = cadmpeg_test_support::service_decode_context();
        let label = PmDcFeatureLabelPayloadWire::<String> {
            save_version_major: 16,
            header: PmDcLinkedHeader {
                header_value: 0,
                header_id: 18,
                values: [0, 0],
                owner: reference,
                parent: reference,
                next: reference,
            },
            index: 3,
            participants: crate::pmdc::PmDcReferenceList::new(
                8,
                Some(crate::pmdc::PmDcListMetadata::U16([1, 2])),
                vec![reference],
            )
            .expect("paired participants"),
            name: "Extrude1".to_owned(),
            class_id: "ab".repeat(16),
        }
        .into_record(&ctx)
        .expect("feature label");
        let owned = label
            .clone()
            .into_wire(&ctx)
            .expect("contextful owned feature-label wire");
        assert_eq!(
            serde_json::to_vec(&label).expect("borrowed feature label"),
            serde_json::to_vec(&owned).expect("owned feature label")
        );
        let record = Record {
            id: "inventor:pmdc:feature-label#1",
            value: &label,
        };
        crate::pmdc::PMDC_LIST_CLONE_COUNT.with(|count| count.set(0));
        cadmpeg_test_support::native_serialization::assert_native_limit(
            &record,
            serde_json::json!({"id": record.id, "value": owned}),
        );
        crate::pmdc::PMDC_LIST_CLONE_COUNT.with(|count| assert_eq!(count.get(), 0));
    }
}
