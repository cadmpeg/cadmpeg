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
    content_header, inventor_id, reference_list, u32_list, Cursor, PmDcContentHeader,
    PmDcReferenceList, PmDcU32List,
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
    fn from_text(value: &str) -> Result<Self, CodecError> {
        let invalid =
            || CodecError::malformed("class_id must contain 32 lowercase hexadecimal digits");
        if value.len() != 32 {
            return Err(invalid());
        }
        // One pass over the 32 digits validates and decodes them.
        let mut bytes = [0; 16];
        for (index, digit) in value.as_bytes().iter().enumerate() {
            let nibble = match digit {
                b'0'..=b'9' => *digit - b'0',
                b'a'..=b'f' => *digit - b'a' + 10,
                _ => return Err(invalid()),
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
        crate::pmdc::type_id_string(ctx, self.0, operation)
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
        let class_id = ClassId::from_text(&self.class_id)?;
        PmDcFeatureLabelPayload::new(
            ctx,
            self.save_version_major,
            self.header,
            self.index,
            self.participants,
            self.name,
            class_id,
        )
    }
}

impl PmDcFeatureLabelPayload {
    fn new(
        ctx: &DecodeContext<'_>,
        save_version_major: u8,
        header: PmDcLinkedHeader,
        index: u32,
        participants: PmDcReferenceList,
        name: String,
        class_id: ClassId,
    ) -> Result<Self, CodecError> {
        let name = ctx.validate_nonblank_text(name, "validate Inventor feature label name")?;
        let Ok(name) = NonBlankString::try_from(name) else {
            return Err(CodecError::malformed("name must not be empty"));
        };
        Ok(Self {
            save_version_major,
            header,
            index,
            participants,
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
    let mut segments = document.segments.iter();
    while let Some(segment) = ctx.next_charged(&mut segments, "visit Inventor feature items")? {
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
        let mut records = table.records.iter();
        while let Some(record) = ctx.next_charged(&mut records, "visit Inventor feature items")? {
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
                for _ in 0..6 {
                    ctx.push_vec(
                        &mut extension_values,
                        cursor.u32("pattern-feature extension value")?,
                        "admit Inventor pattern feature extension values",
                    )?;
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
    // The class id stays in its sixteen bytes: decode never renders it.
    let class_id = ClassId(cursor.take_array("feature-label class id")?);
    cursor.finish("feature label")?;
    PmDcFeatureLabelPayload::new(ctx, version, header, index, participants, name, class_id)
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
    let Some((first, feature_tail)) = inventory
        .features
        .split_first()
        .filter(|_| !inventory.labels.is_empty())
    else {
        return Ok(FeatureProjection {
            features: Vec::new(),
            result_topologies: Vec::new(),
            unresolved_features: total,
            unresolved_states: 0,
        });
    };
    let token = first.identity.segment_token.as_str();
    let pattern_tail = inventory.pattern_features.as_slice();
    let multiple_tokens = (!feature_tail.is_empty()
        && ctx.any_by(
            feature_tail,
            |feature| {
                Ok(!ctx.equal(
                    token,
                    feature.identity.segment_token.as_str(),
                    "check Inventor feature tokens",
                )?)
            },
            "check Inventor feature tokens",
        )?)
        || (!pattern_tail.is_empty()
            && ctx.any_by(
                pattern_tail,
                |feature| {
                    Ok(!ctx.equal(
                        token,
                        feature.identity.segment_token.as_str(),
                        "check Inventor feature tokens",
                    )?)
                },
                "check Inventor feature tokens",
            )?);
    if multiple_tokens {
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
            let mut values = HashMap::new();
            let mut source = parameters.iter();
            while let Some(parameter) =
                ctx.next_charged(&mut source, "index Inventor feature parameter values")?
            {
                let Some(native) = parameter.native_ref.as_deref() else {
                    continue;
                };
                let Some(value) = parameter.value.as_ref() else {
                    continue;
                };
                ctx.insert_hash_map(
                    &mut values,
                    native,
                    value,
                    "index Inventor feature parameter values",
                )?;
            }
            Ok::<_, CodecError>(values)
        })?;
    let (sketch_ids, _sketch_ids_storage) =
        ctx.with_scoped_storage("index Inventor feature sketch ids", || {
            let mut ids = HashMap::new();
            let mut source = sketches.iter();
            while let Some(sketch) =
                ctx.next_charged(&mut source, "visit Inventor feature items")?
            {
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
            let mut source = inventory.entity_style_links.iter();
            while let Some(record) =
                ctx.next_charged(&mut source, "visit Inventor feature items")?
            {
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
    let mut source = inventory.features.iter();
    while let Some(feature) = ctx.next_charged(&mut source, "visit Inventor feature items")? {
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
    // The feature loop is the last reader of both indexes. Release their maps
    // and reservations before ordinal counting. Keep projected_storage live
    // while the projected vector is counted, sorted, and consumed by unzip_vec.
    drop((
        index,
        labels,
        _properties_storage,
        _parameters_storage,
        _sketches_storage,
        _directions_storage,
        _transforms_storage,
        _parameter_values_storage,
        _sketch_ids_storage,
        _entity_style_links_storage,
        _labels_storage,
    ));
    let (ordinal_counts, _ordinal_counts_storage) =
        ctx.with_scoped_storage("count Inventor feature ordinals", || {
            let mut counts = BTreeMap::<u64, usize>::new();
            let mut source = projected.iter();
            while let Some((feature, _)) =
                ctx.next_charged(&mut source, "count Inventor feature ordinals")?
            {
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
    drop((ordinal_counts, _ordinal_counts_storage));
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
    drop((duplicate_ordinals, _duplicate_ordinals_storage));
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
    // Selections must be distinct; one set of the indices seen so far
    // answers each selection instead of a scan of the ones before it.
    let mut seen_storage =
        option_result_value!(ctx.reserve_scoped(0, "check distinct Inventor extrusion selections"));
    let mut seen = std::collections::HashSet::new();
    let mut references = boundary.references().iter();
    while let Some(reference) = option_result_value!(
        ctx.next_charged(&mut references, "visit Inventor feature items")
    ) {
        let first = seen_storage.with_storage(|| {
            ctx.insert_hash_set(
                &mut seen,
                reference.index(),
                "check distinct Inventor extrusion selections",
            )
        });
        if !option_result_value!(first) {
            return None;
        }
    }
    drop(seen);
    drop(seen_storage);
    let mut selections = Vec::new();
    let mut references = boundary.references().iter();
    while let Some(reference) = option_result_value!(
        ctx.next_charged(&mut references, "resolve Inventor extrusion selections")
    ) {
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
    let source_tag = match admit_projected_feature(ctx, "extrude") {
        Ok(value) => value,
        Err(error) => return Some(Err(error)),
    };
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
        source_tag: Some(source_tag),
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
    tag: &'static str,
) -> Result<String, CodecError> {
    ctx.charge_entities(1, "project Inventor feature")?;
    let mut value = ctx.retained_string(tag.len(), "retain Inventor projected feature tag")?;
    value.push_str(tag);
    Ok(value)
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
    let mut references = sets.references().iter();
    while let Some(reference) =
        option_result_value!(ctx.next_charged(&mut references, "visit Inventor feature items"))
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
    let source_tag = match admit_projected_feature(ctx, "fillet") {
        Ok(value) => value,
        Err(error) => return Some(Err(error)),
    };
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
            source_tag: Some(source_tag),
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
    let source_tag = match admit_projected_feature(ctx, "chamfer") {
        Ok(value) => value,
        Err(error) => return Some(Err(error)),
    };
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
            source_tag: Some(source_tag),
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
    let source_tag = match admit_projected_feature(ctx, "hole") {
        Ok(value) => value,
        Err(error) => return Some(Err(error)),
    };
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
            source_tag: Some(source_tag),
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
    let mut references = items.references().iter();
    while let Some(reference) =
        option_result_value!(ctx.next_charged(&mut references, "visit Inventor feature items"))
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
    ctx.all_by(
        items.references(),
        |reference| {
            let Some(property) = resolve_property(ctx, token, reference.index(), index)? else {
                return Ok(false);
            };
            Ok(matches!(&property.kind, PmDcFeaturePropertyKind::EdgeItem { index_references, .. } if !index_references.values().is_empty()))
        },
        "check Inventor closed edge items",
    )
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
    // Callers pass fixed property-slot lists.
    for slot in slots {
        if let Some(value) = boolean(ctx, source, *slot, index)? {
            ctx.insert_btree_map(
                &mut properties,
                cadmpeg_core::nonblank_literal!(ctx, "property_{slot}_boolean")?,
                ctx.copy_retained_text(
                    if value { "true" } else { "false" },
                    "retain Inventor feature property value",
                )?,
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
mod tests;
