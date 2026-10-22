// SPDX-License-Identifier: Apache-2.0
//! Typed `PmDc` feature records and feature-list terminators.
use cadmpeg_core::decode::unique_index;
use cadmpeg_ir::native::bytes::NativeBytes;

use cadmpeg_ir::features::{PlanarProfileRef, ProfileRef};

use std::collections::{BTreeMap, HashMap, HashSet};

use cadmpeg_core::decode::{DecodeContext, View};
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

mod history;

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
const KERNEL_ENTITY_LINK_TYPE: [u8; 16] = [
    0x21, 0x75, 0x0f, 0xcc, 0xd1, 0x11, 0x27, 0x80, 0xe3, 0x86, 0x19, 0x96, 0x22, 0x59, 0x01, 0x7a,
];
const WORK_PLANE_TYPE: [u8; 16] = [
    0x42, 0xdf, 0x52, 0xce, 0xd0, 0x11, 0xd0, 0xd2, 0x00, 0x08, 0xcc, 0xbc, 0x06, 0x63, 0xdc, 0x09,
];
const TERMINATION_FLAG_TYPE: [u8; 16] = [
    0xfd, 0x24, 0x89, 0x3d, 0xd2, 0x11, 0xfa, 0x99, 0x00, 0x08, 0x40, 0xab, 0x06, 0x63, 0xdc, 0x09,
];
const SURFACE_TYPE: [u8; 16] = [
    0xf0, 0x80, 0x12, 0x15, 0xd4, 0x11, 0x9e, 0x8e, 0xef, 0x90, 0xa8, 0xac, 0x7f, 0x9a, 0xc3, 0xff,
];
/// Surface-form value of a plane in a surface record.
const PLANE_SURFACE_FORM: u32 = 25;
/// Work-plane value that precedes the explicit plane frame in its extended form.
const WORK_PLANE_FRAME_MARKER: u32 = 0x203;
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
    pub(crate) kernel_entity_links: Vec<PmDcKernelEntityLink>,
    pub(crate) work_planes: Vec<PmDcWorkPlane>,
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
    TerminationFlag {
        value: u8,
    },
    PlaneSurface {
        references: [crate::pmdc::PmDcReference; 3],
        state: u8,
        surface_form: u32,
        value: u32,
        origin: [cadmpeg_ir::scalar::FiniteReal; 3],
        u_axis: [cadmpeg_ir::scalar::FiniteReal; 3],
        v_axis: [cadmpeg_ir::scalar::FiniteReal; 3],
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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(try_from = "String")]
pub(crate) struct ClassId(NativeBytes<[u8; 16]>);

impl std::fmt::Display for ClassId {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.0.fmt(formatter)
    }
}

impl Serialize for ClassId {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        self.0.serialize(serializer)
    }
}

impl TryFrom<String> for ClassId {
    type Error = String;

    fn try_from(value: String) -> Result<Self, Self::Error> {
        if value.len() != 32
            || !value
                .bytes()
                .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
        {
            return Err("class_id must contain 32 lowercase hexadecimal digits".into());
        }
        let mut bytes = [0; 16];
        for (byte, digits) in bytes.iter_mut().zip(value.as_bytes().chunks_exact(2)) {
            let digit = |value: u8| match value {
                b'0'..=b'9' => value - b'0',
                _ => value - b'a' + 10,
            };
            *byte = digit(digits[0]) * 16 + digit(digits[1]);
        }
        Ok(Self(bytes.into()))
    }
}

impl From<ClassId> for String {
    fn from(value: ClassId) -> Self {
        type_id_string(value.0.into_inner())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(try_from = "PmDcFeatureLabelPayloadWire")]
pub(crate) struct PmDcFeatureLabelPayload {
    save_version_major: u8,
    pub(crate) header: PmDcLinkedHeader,
    index: u32,
    pub(crate) participants: PmDcReferenceList,
    name: String,
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
struct PmDcFeatureLabelPayloadWire {
    save_version_major: u8,
    header: PmDcLinkedHeader,
    index: u32,
    participants: PmDcReferenceList,
    name: String,
    class_id: String,
}

impl TryFrom<PmDcFeatureLabelPayloadWire> for PmDcFeatureLabelPayload {
    type Error = String;
    fn try_from(wire: PmDcFeatureLabelPayloadWire) -> Result<Self, Self::Error> {
        Ok(Self {
            save_version_major: wire.save_version_major,
            header: wire.header,
            index: wire.index,
            participants: wire.participants,
            name: wire.name,
            class_id: ClassId::try_from(wire.class_id)?,
        })
    }
}

impl From<PmDcFeatureLabelPayload> for PmDcFeatureLabelPayloadWire {
    fn from(value: PmDcFeatureLabelPayload) -> Self {
        Self {
            save_version_major: value.save_version_major,
            header: value.header,
            index: value.index,
            participants: value.participants,
            name: value.name.as_str().to_owned(),
            class_id: value.class_id.into(),
        }
    }
}

impl PmDcFeatureLabelPayload {
    pub(crate) fn name(&self) -> Option<&str> {
        (!self.name.trim().is_empty()).then_some(self.name.as_str())
    }

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

/// Link from a selection to one entity of a `PmDc` kernel body.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct PmDcKernelEntityLinkPayload {
    save_version_major: u8,
    header_value: u32,
    header_id: u16,
    pub(crate) selection_source: u32,
    state: u8,
    pub(crate) body: crate::pmdc::PmDcReference,
    entity_id: u32,
    entity_type: u32,
}

/// A planar work feature with its resolved frame in internal centimetres.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub(crate) struct PmDcWorkPlanePayload {
    save_version_major: u8,
    pub(crate) header: PmDcContentHeader,
    entity_flags: u32,
    values: Vec<u32>,
    marker: bool,
    pub(crate) origin: [cadmpeg_ir::scalar::FiniteReal; 3],
    pub(crate) u_axis: [cadmpeg_ir::scalar::FiniteReal; 3],
    pub(crate) v_axis: [cadmpeg_ir::scalar::FiniteReal; 3],
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
        kernel_entity_links: Vec::new(),
        work_planes: Vec::new(),
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
        let RecordFrameState::Framed(table) = &bulk.records else {
            continue;
        };
        for record in &table.records {
            let parsed = match record.type_id {
                FEATURE_TYPE => parse_feature(record.payload, version).and_then(|feature| {
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
                    parse_pattern_feature(record.payload, version, family).and_then(|feature| {
                        push_record(
                            ctx,
                            &mut inventory.pattern_features,
                            feature,
                            record.type_id,
                            segment.pair.token.key(),
                            record.ordinal,
                            "admit Inventor PmDc pattern feature record",
                        )
                    })
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
                KERNEL_ENTITY_LINK_TYPE => parse_kernel_entity_link(record.payload, version)
                    .and_then(|link| {
                        push_record(
                            ctx,
                            &mut inventory.kernel_entity_links,
                            link,
                            record.type_id,
                            segment.pair.token.key(),
                            record.ordinal,
                            "admit Inventor PmDc kernel entity link record",
                        )
                    }),
                WORK_PLANE_TYPE => parse_work_plane(record.payload, version).and_then(|plane| {
                    push_record(
                        ctx,
                        &mut inventory.work_planes,
                        plane,
                        record.type_id,
                        segment.pair.token.key(),
                        record.ordinal,
                        "admit Inventor PmDc work plane record",
                    )
                }),
                SURFACE_TYPE => parse_surface(record.payload, version).and_then(|surface| {
                    surface.map_or(Ok(()), |surface| {
                        push_record(
                            ctx,
                            &mut inventory.properties,
                            surface,
                            record.type_id,
                            segment.pair.token.key(),
                            record.ordinal,
                            "admit Inventor PmDc surface record",
                        )
                    })
                }),
                type_id => feature_property_parser(type_id).map_or_else(
                    || Ok(()),
                    |parser| {
                        parser(ctx, record.payload, version).and_then(|property| {
                            push_record(
                                ctx,
                                &mut inventory.properties,
                                property,
                                record.type_id,
                                segment.pair.token.key(),
                                record.ordinal,
                                "admit Inventor PmDc feature property record",
                            )
                        })
                    },
                ),
            };
            if let Err(error) = parsed {
                if matches!(error, CodecError::ResourceLimit(_)) {
                    return Err(error);
                }
                ctx.charge_entities(1, "admit Inventor PmDc feature issue")?;
                inventory.issues.push(RecordIssue {
                    family: RecordIssueFamily::Feature {
                        type_id: crate::record_identity::RecordTypeId::from_bytes(record.type_id),
                    },
                    segment_token: segment.pair.token.key().clone(),
                    record_ordinal: record.ordinal,
                    detail: crate::issue_detail(error)?,
                });
            }
        }
    }
    Ok(inventory)
}

fn parse_pattern_feature(
    source: View<'_>,
    version: u8,
    family: PmDcPatternFamily,
) -> Result<PmDcPatternFeaturePayload, CodecError> {
    let mut cursor = Cursor::new(source);
    let header = content_header(&mut cursor)?;
    let state = cursor.u32("pattern-feature state")?.cast_signed();
    let outline_value = cursor.u32("pattern-feature outline value")?;
    let properties = reference_list(&mut cursor, 2, "pattern-feature properties")?;
    let value = cursor.u32("pattern-feature value")?;
    let participants = reference_list(&mut cursor, 2, "pattern-feature participants")?;
    let mut property_slots = Vec::new();
    for _ in 0..6 {
        property_slots.push(cursor.reference("pattern-feature property slot")?);
    }
    let control = cursor.u8("pattern-feature control")?;
    let mut extension_values = Vec::new();
    match family {
        PmDcPatternFamily::Rectangular => {
            let remaining = if version > 20 { 26 } else { 20 };
            for _ in 0..remaining {
                property_slots.push(cursor.reference("pattern-feature property slot")?);
            }
        }
        PmDcPatternFamily::Mirror => {
            for _ in 0..5 {
                property_slots.push(cursor.reference("pattern-feature property slot")?);
            }
            if version > 20 {
                for _ in 0..6 {
                    extension_values.push(cursor.u32("pattern-feature extension value")?);
                }
            }
            for _ in 0..2 {
                property_slots.push(cursor.reference("pattern-feature property slot")?);
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

fn parse_feature(source: View<'_>, version: u8) -> Result<PmDcFeaturePayload, CodecError> {
    let mut cursor = Cursor::new(source);
    let header = content_header(&mut cursor)?;
    let state = cursor.u32("feature state")?.cast_signed();
    let outline_value = cursor.u32("feature outline value")?;
    let properties = reference_list(&mut cursor, 2, "feature property list")?;
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

type PropertyParser =
    for<'a> fn(&DecodeContext<'a>, View<'a>, u8) -> Result<PmDcFeaturePropertyPayload, CodecError>;

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
        TERMINATION_FLAG_TYPE => Some(parse_termination_flag),
        _ => None,
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
    source: View<'_>,
    version: u8,
    family: PmDcFeatureReferenceFamily,
) -> Result<PmDcFeaturePropertyPayload, CodecError> {
    let mut cursor = Cursor::new(source);
    let header = content_header(&mut cursor)?;
    let items = reference_list(&mut cursor, 2, "feature-property references")?;
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
            _: &DecodeContext<'_>,
            source: View<'_>,
            version: u8,
        ) -> Result<PmDcFeaturePropertyPayload, CodecError> {
            parse_references(source, version, PmDcFeatureReferenceFamily::$family)
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

fn parse_kernel_entity_link(
    source: View<'_>,
    version: u8,
) -> Result<PmDcKernelEntityLinkPayload, CodecError> {
    let mut cursor = Cursor::new(source);
    let header_value = cursor.u32("kernel entity link header value")?;
    let header_id = cursor.u16("kernel entity link header id")?;
    let selection_source = cursor.u32("kernel entity link selection source")?;
    let state = cursor.u8("kernel entity link state")?;
    let body = cursor.reference("kernel entity link body reference")?;
    let entity_id = cursor.u32("kernel entity link entity id")?;
    let entity_type = cursor.u32("kernel entity link entity type")?;
    cursor.finish("kernel entity link")?;
    Ok(PmDcKernelEntityLinkPayload {
        save_version_major: version,
        header_value,
        header_id,
        selection_source,
        state,
        body,
        entity_id,
        entity_type,
    })
}

fn f64_triple(
    cursor: &mut Cursor<'_>,
    field: &'static str,
) -> Result<[cadmpeg_ir::scalar::FiniteReal; 3], CodecError> {
    Ok([cursor.f64(field)?, cursor.f64(field)?, cursor.f64(field)?])
}

fn parse_work_plane(source: View<'_>, version: u8) -> Result<PmDcWorkPlanePayload, CodecError> {
    let mut cursor = Cursor::new(source);
    let header = content_header(&mut cursor)?;
    let entity_flags = cursor.u32("work plane entity flags")?;
    let mut values = vec![
        cursor.u32("work plane value")?,
        cursor.u32("work plane value")?,
    ];
    if version > 21 {
        values.push(cursor.u32("work plane value")?);
    }
    let marker = cursor.peek_u32("work plane frame marker")? == WORK_PLANE_FRAME_MARKER;
    if marker {
        cursor.u32("work plane frame marker")?;
    }
    values.push(cursor.u32("work plane value")?);
    let origin = f64_triple(&mut cursor, "work plane origin")?;
    let u_axis = f64_triple(&mut cursor, "work plane u axis")?;
    let v_axis = f64_triple(&mut cursor, "work plane v axis")?;
    cursor.finish("work plane")?;
    Ok(PmDcWorkPlanePayload {
        save_version_major: version,
        header,
        entity_flags,
        values,
        marker,
        origin,
        u_axis,
        v_axis,
    })
}

fn parse_termination_flag(
    _: &DecodeContext<'_>,
    source: View<'_>,
    version: u8,
) -> Result<PmDcFeaturePropertyPayload, CodecError> {
    let mut cursor = Cursor::new(source);
    let header = content_header(&mut cursor)?;
    let value = cursor.u8("termination flag value")?;
    cursor.finish("termination flag")?;
    Ok(property(
        version,
        header,
        PmDcFeaturePropertyKind::TerminationFlag { value },
    ))
}

/// Parse a surface record. Only the plane form is typed; other forms are not
/// admitted.
fn parse_surface(
    source: View<'_>,
    version: u8,
) -> Result<Option<PmDcFeaturePropertyPayload>, CodecError> {
    let mut cursor = Cursor::new(source);
    let header = content_header(&mut cursor)?;
    let references = [
        cursor.reference("surface reference")?,
        cursor.reference("surface reference")?,
        cursor.reference("surface reference")?,
    ];
    let state = cursor.u8("surface state")?;
    let form = cursor.u32("surface form")?;
    if form != PLANE_SURFACE_FORM {
        return Ok(None);
    }
    let value = cursor.u32("surface value")?;
    let origin = f64_triple(&mut cursor, "surface plane origin")?;
    let u_axis = f64_triple(&mut cursor, "surface plane u axis")?;
    let v_axis = f64_triple(&mut cursor, "surface plane v axis")?;
    cursor.finish("plane surface")?;
    Ok(Some(property(
        version,
        header,
        PmDcFeaturePropertyKind::PlaneSurface {
            references,
            state,
            surface_form: form,
            value,
            origin,
            u_axis,
            v_axis,
        },
    )))
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
    _: &DecodeContext<'_>,
    source: View<'_>,
    version: u8,
) -> Result<PmDcFeaturePropertyPayload, CodecError> {
    let mut cursor = Cursor::new(source);
    let header = content_header(&mut cursor)?;
    let index_references = u32_list(&mut cursor, 2, "edge-item index references")?;
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
    let participants = reference_list(&mut cursor, 2, "feature-label participants")?;
    let name = cursor.utf16(ctx, "feature label")?;
    let class_id_bytes = cursor.take_array("feature-label class id")?;
    let class_id = type_id_string(class_id_bytes);
    cursor.finish("feature label")?;
    PmDcFeatureLabelPayload::try_from(PmDcFeatureLabelPayloadWire {
        save_version_major: version,
        header,
        index,
        participants,
        name,
        class_id,
    })
    .map_err(CodecError::malformed)
}

const EXTRUSION_CLASS_ID: ClassId = ClassId(NativeBytes::new([
    0x31, 0x11, 0xa9, 0x0c, 0xd0, 0x11, 0x8b, 0x83, 0x00, 0x08, 0x19, 0xb0, 0x05, 0x24, 0xdc, 0x09,
]));
const FILLET_CLASS_ID: ClassId = ClassId(NativeBytes::new([
    0xdc, 0x15, 0xf7, 0xf1, 0xd1, 0x11, 0x42, 0x05, 0x00, 0x08, 0x30, 0xb0, 0x05, 0x24, 0xdc, 0x09,
]));
const CHAMFER_CLASS_ID: ClassId = ClassId(NativeBytes::new([
    0x3f, 0x71, 0x00, 0xf9, 0xd2, 0x11, 0x8b, 0x6f, 0x60, 0x00, 0xf0, 0xa8, 0x9d, 0xcc, 0xef, 0xb0,
]));
const HOLE_CLASS_ID: ClassId = ClassId(NativeBytes::new([
    0x1a, 0x7d, 0x75, 0x1f, 0xd2, 0x11, 0x9c, 0x54, 0xa0, 0x00, 0x20, 0x80, 0x36, 0x03, 0xc8, 0xc9,
]));

/// Native family names of feature-label classes outside the neutral families.
const NATIVE_CLASSES: &[([u8; 16], &str)] = &[
    (
        [
            0x05, 0xda, 0xba, 0x90, 0xd1, 0x11, 0x38, 0x61, 0x00, 0x08, 0x0b, 0xbd, 0x06, 0x63,
            0xdc, 0x09,
        ],
        "Sweep",
    ),
    (
        [
            0x06, 0x19, 0xaa, 0x28, 0xd4, 0x11, 0xc7, 0x7a, 0x60, 0x00, 0xc4, 0xa4, 0xca, 0xe1,
            0xfb, 0xb0,
        ],
        "Thread",
    ),
    (
        [
            0x12, 0x96, 0x3c, 0xb8, 0xd1, 0x11, 0x8a, 0x28, 0x60, 0x00, 0x8b, 0xb8, 0x01, 0xf3,
            0x1b, 0xb0,
        ],
        "Shell",
    ),
    (
        [
            0x2d, 0x11, 0xa9, 0x0c, 0xd0, 0x11, 0x8b, 0x83, 0x00, 0x08, 0x19, 0xb0, 0x05, 0x24,
            0xdc, 0x09,
        ],
        "Base",
    ),
    (
        [
            0x33, 0x67, 0x23, 0xc0, 0x7e, 0x40, 0xea, 0xcb, 0x2e, 0xff, 0x66, 0x9e, 0x9a, 0xdd,
            0x87, 0xf8,
        ],
        "Combine",
    ),
    (
        [
            0x3a, 0x6f, 0xed, 0x36, 0xd4, 0x11, 0x42, 0x88, 0x10, 0x00, 0xfc, 0xa5, 0xbb, 0xa6,
            0x47, 0xb5,
        ],
        "RevolveSurface",
    ),
    (
        [
            0x5f, 0x01, 0x54, 0x00, 0xd2, 0x11, 0x25, 0x1c, 0x60, 0x00, 0x67, 0xb7, 0x9b, 0x49,
            0xeb, 0xb0,
        ],
        "Revolve",
    ),
    (
        [
            0x68, 0x6e, 0x16, 0xd3, 0x33, 0x4f, 0x63, 0x71, 0x61, 0xa6, 0xa1, 0x9b, 0xdd, 0x3d,
            0x2b, 0xe8,
        ],
        "DeleteFace",
    ),
    (
        [
            0x7f, 0x58, 0x80, 0x68, 0xb7, 0x43, 0x78, 0xa0, 0x55, 0x95, 0x15, 0x94, 0x6f, 0x7a,
            0x1c, 0x12,
        ],
        "SheetMetalCorner",
    ),
    (
        [
            0x80, 0x8b, 0x00, 0xaf, 0xd4, 0x11, 0x40, 0x88, 0x10, 0x00, 0xfc, 0xa5, 0xbb, 0xa6,
            0x47, 0xb5,
        ],
        "LoftSurface",
    ),
    (
        [
            0x85, 0x5b, 0x33, 0xe7, 0x04, 0x45, 0xdf, 0x5c, 0x25, 0xb4, 0x8c, 0xb8, 0xa8, 0xb2,
            0x69, 0x22,
        ],
        "Decal",
    ),
    (
        [
            0x8c, 0x39, 0x5e, 0x28, 0x4c, 0x4f, 0xeb, 0x8b, 0xc9, 0x81, 0x4c, 0xab, 0xad, 0xec,
            0x94, 0x39,
        ],
        "Emboss",
    ),
    (
        [
            0x94, 0x26, 0xc2, 0xb6, 0xd2, 0x11, 0x26, 0x36, 0x60, 0x00, 0x2c, 0xab, 0x01, 0xf3,
            0x1b, 0xb0,
        ],
        "CircularPattern",
    ),
    (
        [
            0xa3, 0x26, 0xc2, 0xb6, 0xd2, 0x11, 0x26, 0x36, 0x60, 0x00, 0x2c, 0xab, 0x01, 0xf3,
            0x1b, 0xb0,
        ],
        "Mirror",
    ),
    (
        [
            0xaa, 0xf2, 0x10, 0x93, 0xd3, 0x44, 0x20, 0x9a, 0x04, 0xcc, 0x79, 0x96, 0x56, 0xcd,
            0x7e, 0x09,
        ],
        "FlatPatternDefinition",
    ),
    (
        [
            0xb5, 0x4d, 0xd6, 0xf6, 0xd3, 0x11, 0xc4, 0x96, 0x60, 0x00, 0xf2, 0xb2, 0x1d, 0x6e,
            0xef, 0xb0,
        ],
        "CornerRound",
    ),
    (
        [
            0xba, 0xa8, 0x71, 0x83, 0xd3, 0x11, 0x34, 0x7b, 0xc0, 0x00, 0xe3, 0x95, 0x45, 0xdf,
            0x72, 0x4f,
        ],
        "Split",
    ),
    (
        [
            0xc6, 0x5f, 0x58, 0x55, 0xd3, 0x11, 0x73, 0x0c, 0x60, 0x00, 0x3a, 0xa9, 0x9d, 0xcc,
            0xef, 0xb0,
        ],
        "SheetMetalPlate",
    ),
    (
        [
            0xda, 0x14, 0x19, 0xb4, 0xd2, 0x11, 0xda, 0xd0, 0x60, 0x00, 0x12, 0xa9, 0x9d, 0xcc,
            0xef, 0xb0,
        ],
        "Coil",
    ),
    (
        [
            0xda, 0xc2, 0x7a, 0x76, 0xd2, 0x11, 0x0d, 0x2e, 0x60, 0x00, 0x2a, 0xab, 0x01, 0xf3,
            0x1b, 0xb0,
        ],
        "RectangularPattern",
    ),
    (
        [
            0xde, 0xa6, 0xa6, 0x8f, 0x4c, 0x4b, 0x52, 0x91, 0x88, 0xca, 0x31, 0xb5, 0x63, 0x01,
            0x98, 0xc8,
        ],
        "Move",
    ),
    (
        [
            0xe0, 0xef, 0x54, 0x2f, 0xd1, 0x11, 0x34, 0x09, 0x00, 0x08, 0x4e, 0xba, 0x32, 0xa3,
            0xdc, 0x09,
        ],
        "Loft",
    ),
    (
        [
            0xe6, 0xda, 0x3f, 0x26, 0x92, 0x4b, 0x5d, 0xab, 0x8e, 0x1b, 0xc5, 0xa9, 0x3f, 0xea,
            0xd7, 0xde,
        ],
        "Rotate",
    ),
    (
        [
            0xe8, 0x99, 0xdb, 0xdf, 0xd3, 0x11, 0x65, 0xaf, 0xc0, 0x00, 0x01, 0x85, 0xe8, 0x9c,
            0x6b, 0x4f,
        ],
        "Tagging",
    ),
    (
        [
            0xe9, 0x9f, 0x83, 0x2b, 0x7a, 0x42, 0x95, 0xa1, 0xd9, 0x76, 0x33, 0x83, 0xbe, 0xa1,
            0x7c, 0xce,
        ],
        "MoveBody",
    ),
    (
        [
            0xec, 0xf4, 0x81, 0x72, 0x9c, 0x4d, 0x5f, 0x92, 0x5f, 0x21, 0x92, 0xb4, 0xa8, 0x30,
            0x96, 0xac,
        ],
        "Thicken",
    ),
    (
        [
            0xf0, 0xa4, 0x97, 0xe5, 0xd4, 0x11, 0xd8, 0x84, 0x10, 0x00, 0xfa, 0xa5, 0xbb, 0xa6,
            0x47, 0xb5,
        ],
        "ExtrudeSurface",
    ),
    (
        [
            0xf2, 0x58, 0xf7, 0x4c, 0xd3, 0x11, 0x4f, 0x24, 0x60, 0x00, 0x47, 0xa9, 0x9d, 0xcc,
            0xef, 0xb0,
        ],
        "SheetMetalBend",
    ),
];

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
    parameter_ids: HashMap<&'a str, &'a cadmpeg_ir::features::ParameterId>,
    sketches: HashMap<(&'a str, u32), Option<&'a crate::sketch::PmDcSketch>>,
    sketch_ids: HashMap<&'a str, cadmpeg_ir::sketches::SketchId>,
    directions: HashMap<(&'a str, u32), Option<&'a crate::sketch::PmDcDirection>>,
    transforms: HashMap<(&'a str, u32), Option<&'a crate::sketch::PmDcTransform>>,
    entity_style_links: HashSet<(&'a str, u32)>,
    work_planes: HashMap<(&'a str, u32), Option<&'a PmDcWorkPlane>>,
}

pub(crate) fn project(
    ctx: &DecodeContext<'_>,
    inventory: &FeatureInventory,
    design: &DesignInventory,
    sketch: &SketchInventory,
    parameters: &[DesignParameter],
    sketches: &[Sketch],
) -> Result<FeatureProjection, CodecError> {
    let total = inventory.features.len() + inventory.pattern_features.len();
    let mut feature_tokens = HashSet::new();
    for token in inventory
        .features
        .iter()
        .map(|feature| feature.identity.segment_token.as_str())
        .chain(
            inventory
                .pattern_features
                .iter()
                .map(|feature| feature.identity.segment_token.as_str()),
        )
    {
        feature_tokens.insert(token);
    }
    if feature_tokens.len() > 1 {
        return Ok(FeatureProjection {
            features: Vec::new(),
            result_topologies: Vec::new(),
            unresolved_features: total,
            unresolved_states: 0,
        });
    }

    let unique_properties = unique_index(inventory.properties.iter().map(|record| {
        (
            (
                record.identity.segment_token.as_str(),
                record.identity.record_ordinal,
            ),
            record,
        )
    }));
    let unique_parameters = unique_index(design.parameters.iter().map(|record| {
        (
            (
                record.identity.segment_token.as_str(),
                record.identity.record_ordinal,
            ),
            record,
        )
    }));
    let unique_sketches = unique_index(sketch.sketches.iter().map(|record| {
        (
            (
                record.identity.segment_token.as_str(),
                record.identity.record_ordinal,
            ),
            record,
        )
    }));
    let unique_directions = unique_index(sketch.directions.iter().map(|record| {
        (
            (
                record.identity.segment_token.as_str(),
                record.identity.record_ordinal,
            ),
            record,
        )
    }));
    let unique_work_planes = unique_index(inventory.work_planes.iter().map(|record| {
        (
            (
                record.identity.segment_token.as_str(),
                record.identity.record_ordinal,
            ),
            record,
        )
    }));
    let unique_transforms = unique_index(sketch.transforms.iter().map(|record| {
        (
            (
                record.identity.segment_token.as_str(),
                record.identity.record_ordinal,
            ),
            record,
        )
    }));
    let index = ProjectionIndex {
        properties: unique_properties,
        parameters: unique_parameters,
        parameter_ids: parameters
            .iter()
            .filter_map(|parameter| Some((parameter.native_ref.as_deref()?, &parameter.id)))
            .collect(),
        parameter_values: {
            parameters
                .iter()
                .filter_map(|parameter| {
                    Some((parameter.native_ref.as_deref()?, parameter.value.as_ref()?))
                })
                .collect()
        },
        sketches: unique_sketches,
        sketch_ids: {
            let mut ids = HashMap::new();
            for sketch in sketches {
                if let Some(native) = sketch.native_ref.as_deref() {
                    let id = sketch.id.clone();
                    ids.insert(native, id);
                }
            }
            ids
        },
        directions: unique_directions,
        transforms: unique_transforms,
        entity_style_links: {
            let mut links = HashSet::new();
            for record in &inventory.entity_style_links {
                let key = (
                    record.identity.segment_token.as_str(),
                    record.identity.record_ordinal,
                );
                links.insert(key);
            }
            for record in &inventory.kernel_entity_links {
                let key = (
                    record.identity.segment_token.as_str(),
                    record.identity.record_ordinal,
                );
                links.insert(key);
            }
            links
        },
        work_planes: unique_work_planes,
    };
    // The label owner is a one-based reference. Keying on the optional record
    // ordinal keeps the null reference out of the ordinal space, so a label
    // with no owner never claims the feature at ordinal 0.
    let labels = unique_index(inventory.labels.iter().map(|label| {
        (
            (
                label.identity.segment_token.as_str(),
                label.header.owner.record_ordinal(),
            ),
            label,
        )
    }));
    let token = feature_tokens.iter().next().copied();
    let history = match token {
        Some(token) => history::build(ctx, token, inventory, design, &sketch.sketches)?,
        None => history::History::default(),
    };
    let label_of = |ordinal: u32| {
        token.and_then(|token| {
            labels
                .get(&(token, Some(ordinal)))
                .and_then(Option::as_ref)
                .copied()
        })
    };
    let mut projected = Vec::new();
    let mut neutral = 0_usize;
    for feature in &inventory.features {
        let label = label_of(feature.identity.record_ordinal);
        let value = match label.and_then(|label| {
            FeatureFamily::from_class_id(label.class_id()).map(|family| (label, family))
        }) {
            Some((label, family)) => match family {
                FeatureFamily::Extrusion => project_extrusion(ctx, feature, label, &index),
                FeatureFamily::Fillet => project_fillet(ctx, feature, label, &index),
                FeatureFamily::Chamfer => project_chamfer(ctx, feature, label, &index),
                FeatureFamily::Hole => project_hole(ctx, feature, label, &index),
            }
            .transpose()?,
            None => None,
        };
        match value {
            Some((projected_feature, result)) => {
                neutral += 1;
                projected.push(HistoryItem {
                    record: feature.identity.record_ordinal,
                    kind: history::HistoryNodeKind::Feature,
                    flags: feature.header.flags,
                    label,
                    feature: projected_feature,
                    result: Some(result),
                });
            }
            None if history.placement(feature.identity.record_ordinal).is_some() => {
                projected.push(HistoryItem {
                    record: feature.identity.record_ordinal,
                    kind: history::HistoryNodeKind::Feature,
                    flags: feature.header.flags,
                    label,
                    feature: native_feature(ctx, &feature.identity, label, feature.id())?,
                    result: None,
                });
            }
            None => {}
        }
    }
    for pattern in &inventory.pattern_features {
        if history.placement(pattern.identity.record_ordinal).is_none() {
            continue;
        }
        let label = label_of(pattern.identity.record_ordinal);
        projected.push(HistoryItem {
            record: pattern.identity.record_ordinal,
            kind: history::HistoryNodeKind::Pattern,
            flags: pattern.header.flags,
            label,
            feature: native_feature(ctx, &pattern.identity, label, pattern.id())?,
            result: None,
        });
    }
    for record in &sketch.sketches {
        let Some(placement) = history.placement(record.identity.record_ordinal) else {
            continue;
        };
        if placement.kind != history::HistoryNodeKind::Sketch {
            continue;
        }
        let label = label_of(record.identity.record_ordinal);
        projected.push(HistoryItem {
            record: record.identity.record_ordinal,
            kind: history::HistoryNodeKind::Sketch,
            flags: record.header.flags,
            label,
            feature: sketch_feature(ctx, record, label, &index)?,
            result: None,
        });
    }
    apply_history(&mut projected, &history, inventory, token)?;
    let unresolved_states = projected
        .iter()
        .filter(|item| item.result.is_some() && history.placement(item.record).is_none())
        .count();
    let ordinal_counts = projected.iter().map(|item| item.feature.ordinal).fold(
        HashMap::<u64, usize>::new(),
        |mut counts, ordinal| {
            *counts.entry(ordinal).or_default() += 1;
            counts
        },
    );
    let duplicate_ordinals = ordinal_counts
        .into_iter()
        .filter_map(|(ordinal, count)| (count > 1).then_some(ordinal))
        .collect::<HashSet<_>>();
    projected.retain(|item| {
        let keep = !duplicate_ordinals.contains(&item.feature.ordinal);
        if !keep && item.result.is_some() {
            neutral -= 1;
        }
        keep
    });
    projected.sort_unstable_by_key(|item| item.feature.ordinal);
    let mut features = Vec::new();
    let mut result_topologies = Vec::new();
    for item in projected {
        features.push(item.feature);
        if let Some(result) = item.result {
            result_topologies.push(result);
        }
    }
    Ok(FeatureProjection {
        unresolved_features: total.checked_sub(neutral).ok_or_else(|| {
            CodecError::malformed("Inventor feature projection exceeds inventory")
        })?,
        unresolved_states: unresolved_states.min(neutral),
        features,
        result_topologies,
    })
}

/// One projected history node and the record that supplies it.
struct HistoryItem<'a> {
    record: u32,
    kind: history::HistoryNodeKind,
    flags: u32,
    label: Option<&'a PmDcFeatureLabel>,
    feature: Feature,
    result: Option<FeatureResultTopology>,
}

/// Apply browser order, suppression, end-of-part state, and consumed-node
/// dependencies to the projected history nodes.
fn apply_history(
    projected: &mut [HistoryItem<'_>],
    history: &history::History,
    inventory: &FeatureInventory,
    token: Option<&str>,
) -> Result<(), CodecError> {
    if history.placements.is_empty() {
        return Ok(());
    }
    let mut unplaced = projected
        .iter()
        .enumerate()
        .filter(|(_, item)| history.placement(item.record).is_none())
        .map(|(position, item)| (item.label.map(|label| label.index), item.record, position))
        .collect::<Vec<_>>();
    unplaced.sort_unstable_by_key(|&(label, record, _)| (label.is_none(), label, record));
    let mut next = history.next_ordinal;
    for (_, _, position) in unplaced {
        if let Some(item) = projected.get_mut(position) {
            item.feature.ordinal = next;
            next = next
                .checked_add(1)
                .ok_or_else(|| CodecError::malformed("Inventor feature ordinal overflows"))?;
        }
    }
    let ids = projected
        .iter()
        .map(|item| (item.record, item.feature.id.clone()))
        .collect::<HashMap<_, _>>();
    let patterns = inventory
        .pattern_features
        .iter()
        .filter(|pattern| Some(pattern.identity.segment_token.as_str()) == token)
        .map(|pattern| (pattern.identity.record_ordinal, pattern))
        .collect::<HashMap<_, _>>();
    for item in projected.iter_mut() {
        let Some(placement) = history.placement(item.record) else {
            continue;
        };
        item.feature.ordinal = placement.ordinal;
        let flagged = item.flags & history::SUPPRESSED_FLAG != 0;
        item.feature.suppressed = Some(flagged || placement.below_end_of_part);
        if flagged {
            insert_history_property(&mut item.feature, "suppressed", "true")?;
        }
        if placement.below_end_of_part {
            insert_history_property(&mut item.feature, "end_of_part", "below")?;
        }
        if !placement.model_history {
            insert_history_property(&mut item.feature, "history", "flat_pattern")?;
        }
        let consumed: Vec<u32> = match item.kind {
            history::HistoryNodeKind::Sketch => Vec::new(),
            history::HistoryNodeKind::Feature => item
                .label
                .map(|label| {
                    label
                        .participants
                        .references()
                        .iter()
                        .filter_map(|reference| reference.record_ordinal())
                        .collect()
                })
                .unwrap_or_default(),
            history::HistoryNodeKind::Pattern => patterns
                .get(&item.record)
                .map(|pattern| {
                    pattern
                        .participants
                        .references()
                        .iter()
                        .filter_map(|reference| reference.record_ordinal())
                        .collect()
                })
                .unwrap_or_default(),
        };
        for ordinal in consumed {
            let Some(dependency) = ids.get(&ordinal) else {
                continue;
            };
            if history
                .placement(ordinal)
                .is_none_or(|earlier| earlier.ordinal >= placement.ordinal)
            {
                continue;
            }
            item.feature.dependencies.insert(dependency.clone());
        }
    }
    Ok(())
}

fn insert_history_property(
    feature: &mut Feature,
    key: &'static str,
    value: &'static str,
) -> Result<(), CodecError> {
    feature.source_properties.insert(
        cadmpeg_core::text::NonBlankString::new(key.to_owned()).ok_or_else(|| {
            CodecError::malformed("Inventor feature history property key is blank")
        })?,
        value.to_owned(),
    );
    Ok(())
}

fn history_feature_id(
    identity: &crate::record_identity::RecordIdentity,
) -> Result<FeatureId, CodecError> {
    Ok(FeatureId::compose(
        &cadmpeg_ir::identity_namespace!("inventor", "design", "feature"),
        identity.key()?,
    ))
}

/// A feature record whose operation graph is not closed, retained at its
/// history position with its native family.
fn native_feature(
    ctx: &DecodeContext<'_>,
    identity: &crate::record_identity::RecordIdentity,
    label: Option<&PmDcFeatureLabel>,
    native_id: String,
) -> Result<Feature, CodecError> {
    ctx.charge_entities(1, "project Inventor history feature")?;
    let kind = native_kind(label.map(|label| label.class_id()));
    Ok(Feature {
        id: history_feature_id(identity)?,
        needs_recompute: false,
        ordinal: label.map_or(0, |label| u64::from(label.index)),
        name: label.and_then(|label| label.name()).map(str::to_owned),
        suppressed: None,
        dependencies: DistinctMembers::default(),
        source_properties: BTreeMap::new(),
        source_tag: Some("native".into()),
        source_text: None,
        source_content: FeatureContent::default(),
        evaluation: cadmpeg_ir::features::FeatureEvaluation::from_definition(
            FeatureDefinition::Operation(FeatureOperation::Native {
                kind,
                parameters: BTreeMap::new(),
            }),
        ),
        native_ref: Some(native_id),
    })
}

/// A planar sketch node of the history.
fn sketch_feature(
    ctx: &DecodeContext<'_>,
    record: &crate::sketch::PmDcSketch,
    label: Option<&PmDcFeatureLabel>,
    index: &ProjectionIndex<'_>,
) -> Result<Feature, CodecError> {
    let native_id = record.id();
    ctx.charge_entities(1, "project Inventor history feature")?;
    let sketch = index.sketch_ids.get(native_id.as_str()).cloned();
    Ok(Feature {
        id: history_feature_id(&record.identity)?,
        needs_recompute: false,
        ordinal: label.map_or(0, |label| u64::from(label.index)),
        name: label.and_then(|label| label.name()).map(str::to_owned),
        suppressed: None,
        dependencies: DistinctMembers::default(),
        source_properties: BTreeMap::new(),
        source_tag: Some("sketch".into()),
        source_text: None,
        source_content: FeatureContent::default(),
        evaluation: cadmpeg_ir::features::FeatureEvaluation::from_definition(
            FeatureDefinition::Operation(FeatureOperation::Sketch {
                sketch: cadmpeg_ir::features::SketchFeatureBinding::Planar(sketch),
            }),
        ),
        native_ref: Some(native_id),
    })
}

/// Native family of a feature-label class identifier.
fn native_kind(class_id: Option<ClassId>) -> cadmpeg_ir::features::NativeFeatureKind {
    use cadmpeg_ir::features::NativeFeatureKind;
    let Some(class_id) = class_id else {
        return NativeFeatureKind::Other("InventorFeature".into());
    };
    if let Some(family) = FeatureFamily::from_class_id(class_id) {
        return match family {
            FeatureFamily::Extrusion => NativeFeatureKind::Extrude,
            FeatureFamily::Fillet => NativeFeatureKind::Fillet,
            FeatureFamily::Chamfer => NativeFeatureKind::Chamfer,
            FeatureFamily::Hole => NativeFeatureKind::Other("Hole".into()),
        };
    }
    NATIVE_CLASSES
        .iter()
        .find(|(bytes, _)| *bytes == class_id.0.into_inner())
        .map_or_else(
            || NativeFeatureKind::Other(format!("InventorClass:{class_id}")),
            |(_, name)| NativeFeatureKind::Other((*name).to_owned()),
        )
}

fn project_extrusion(
    ctx: &DecodeContext<'_>,
    source: &PmDcFeature,
    label: &PmDcFeatureLabel,
    index: &ProjectionIndex<'_>,
) -> Option<Result<(Feature, FeatureResultTopology), CodecError>> {
    let operation = enum16(source, 0, PmDcFeatureEnumFamily::PartOperation, index)?;
    let op = match operation {
        1 => BooleanOp::NewBody,
        2 => BooleanOp::Cut,
        3 => BooleanOp::Join,
        4 => BooleanOp::Intersect,
        _ => return None,
    };
    let boundary = references(source, 1, PmDcFeatureReferenceFamily::BoundaryPatch, index)?;
    if source.properties.references().get(23)? != source.properties.references().get(1)? {
        return None;
    }
    let mut distinct = HashSet::new();
    if !boundary
        .references()
        .iter()
        .all(|selection| distinct.insert(selection.index()))
    {
        return None;
    }
    let mut selections = Vec::new();
    for reference in boundary.references() {
        let property = resolve_property(
            source.identity.segment_token.as_str(),
            reference.index(),
            index,
        )?;
        let PmDcFeaturePropertyKind::ProfileSelection { entity_link, .. } = &property.kind else {
            return None;
        };
        let ordinal = entity_link.index().checked_sub(1)?;
        if !index
            .entity_style_links
            .contains(&(source.identity.segment_token.as_str(), ordinal))
        {
            return None;
        }
        selections.push(property.id());
    }
    if selections.is_empty() || label.participants.references().len() != 1 {
        return None;
    }
    let sketch_reference = label.participants.references().first()?;
    let sketch = index
        .sketches
        .get(&(
            source.identity.segment_token.as_str(),
            sketch_reference.index().checked_sub(1)?,
        ))
        .and_then(Option::as_ref)?;
    let sketch_id = index.sketch_ids.get(sketch.id().as_str())?.clone();

    let direction_record = resolve_direction(source, 2, index)?;
    let mut direction = cadmpeg_ir::units::UnitVector3::normalized(Vector3::new(
        direction_record.direction[0].get(),
        direction_record.direction[1].get(),
        direction_record.direction[2].get(),
    ))?;
    if boolean(source, 3, index)? {
        direction = direction.reversed();
    }
    let extent_value = enum16(source, 6, PmDcFeatureEnumFamily::Extent, index)?;
    let symmetric = boolean(source, 7, index)?;
    let second_extent = source
        .properties
        .references()
        .get(29)
        .is_some_and(|reference| reference.index() != 0);
    let first = extrusion_side(source, index, ExtrusionSideSlots::FIRST, extent_value)?;
    let start = if extent_value == EXTENT_FROM_TO {
        ExtrudeStart::FromFace {
            face: termination_target(source, index, 8, 10, 17)?,
            offset: None,
        }
    } else {
        ExtrudeStart::ProfilePlane {}
    };
    let extent = match (extent_value, symmetric, second_extent) {
        (EXTENT_SYMMETRIC_DISTANCE | EXTENT_THROUGH_ALL, true, false) => {
            ExtrudeExtent::Symmetric { side: first }
        }
        (EXTENT_SYMMETRIC_DISTANCE, false, _) | (_, true, true) => return None,
        (_, true, false) => return None,
        (_, false, true) => {
            let second_value = enum16(source, 29, PmDcFeatureEnumFamily::Extent, index)?;
            if !matches!(
                second_value,
                EXTENT_DISTANCE | EXTENT_THROUGH_NEXT | EXTENT_THROUGH_ALL
            ) || extent_value == EXTENT_FROM_TO
            {
                return None;
            }
            ExtrudeExtent::TwoSided {
                first,
                second: extrusion_side(source, index, ExtrusionSideSlots::SECOND, second_value)?,
            }
        }
        (_, false, false) => ExtrudeExtent::OneSided { side: first },
    };
    let (feature_id, result) = match feature_result(ctx, source, 26, index)? {
        Ok(value) => value,
        Err(error) => return Some(Err(error)),
    };
    let source_properties = boolean_properties(source, &[20, 22], index);
    if let Err(error) = ctx.charge_entities(1, "project Inventor feature") {
        return Some(Err(error));
    }
    let profile = PlanarProfileRef::sketch_selection(sketch_id, selections).ok()?;
    let feature = Feature {
        id: feature_id,
        ordinal: u64::from(label.index),
        name: label.name().map(str::to_owned),
        suppressed: None,
        needs_recompute: false,
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
                start,
                extent,
                op,
                solid: Some(true),
                face_maker: None,
                inner_wire_taper: None,
                length_along_profile_normal: None,
                allow_multi_profile_faces: None,
            }),
        ),
        native_ref: Some(source.id()),
    };
    Some(Ok((feature, result)))
}

const EXTENT_DISTANCE: u16 = 1;
const EXTENT_SYMMETRIC_DISTANCE: u16 = 2;
const EXTENT_THROUGH_NEXT: u16 = 4;
const EXTENT_THROUGH_ALL: u16 = 5;
const EXTENT_FROM_TO: u16 = 6;
const EXTENT_TO: u16 = 7;

/// Property slots of one extrusion side.
#[derive(Clone, Copy)]
struct ExtrusionSideSlots {
    length: usize,
    taper: usize,
}

impl ExtrusionSideSlots {
    const FIRST: Self = Self {
        length: 4,
        taper: 5,
    };
    const SECOND: Self = Self {
        length: 27,
        taper: 28,
    };
}

fn extrusion_side(
    source: &PmDcFeature,
    index: &ProjectionIndex<'_>,
    slots: ExtrusionSideSlots,
    extent: u16,
) -> Option<ExtrudeSide> {
    let taper =
        cadmpeg_ir::scalar::SlopeAngle::try_from(angle_parameter(source, slots.taper, index)?)
            .ok()?;
    let termination = match extent {
        EXTENT_DISTANCE | EXTENT_SYMMETRIC_DISTANCE => LinearTermination::Blind {
            length: {
                cadmpeg_ir::scalar::PositiveLength::try_from(length_parameter(
                    source,
                    slots.length,
                    index,
                )?)
                .ok()?;
                parameter_binding(source, slots.length, index)?
            },
        },
        EXTENT_THROUGH_NEXT => LinearTermination::ThroughNext {},
        EXTENT_THROUGH_ALL => LinearTermination::ThroughAll {},
        EXTENT_FROM_TO | EXTENT_TO => LinearTermination::ToFace {
            face: termination_target(source, index, 11, 13, 18)?,
            offset: None,
        },
        _ => return None,
    };
    Some(ExtrudeSide {
        termination,
        draft: (taper.get() != 0.0)
            .then(|| parameter_binding(source, slots.taper, index))
            .flatten(),
    })
}

/// Resolve an extent target: a work plane, or a model face recorded as a
/// termination flag with value 1 and its plane surface.
fn termination_target(
    source: &PmDcFeature,
    index: &ProjectionIndex<'_>,
    plane_slot: usize,
    flag_slot: usize,
    surface_slot: usize,
) -> Option<cadmpeg_ir::features::FaceSelection> {
    let references = source.properties.references();
    let plane = references.get(plane_slot)?.index();
    let flag = references.get(flag_slot)?.index();
    let surface = references.get(surface_slot)?.index();
    let token = source.identity.segment_token.as_str();
    if plane != 0 {
        if flag != 0 || surface != 0 {
            return None;
        }
        let plane = index
            .work_planes
            .get(&(token, plane.checked_sub(1)?))
            .and_then(Option::as_ref)?;
        return Some(cadmpeg_ir::features::FaceSelection::Native(plane.id()));
    }
    if !matches!(
        resolve_property(token, flag, index)?.kind,
        PmDcFeaturePropertyKind::TerminationFlag { value: 1 }
    ) {
        return None;
    }
    let surface = resolve_property(token, surface, index)?;
    if !matches!(surface.kind, PmDcFeaturePropertyKind::PlaneSurface { .. }) {
        return None;
    }
    Some(cadmpeg_ir::features::FaceSelection::Native(surface.id()))
}

fn project_fillet(
    ctx: &DecodeContext<'_>,
    source: &PmDcFeature,
    label: &PmDcFeatureLabel,
    index: &ProjectionIndex<'_>,
) -> Option<Result<(Feature, FeatureResultTopology), CodecError>> {
    if enum16(source, 11, PmDcFeatureEnumFamily::Fillet, index)? != 0
        || source.properties.references().get(1)?.index() != 0
        || source.properties.references().get(10)?.index() != 0
    {
        return None;
    }
    let sets = references(source, 0, PmDcFeatureReferenceFamily::FilletEdgeSets, index)?;
    let mut groups = Vec::new();
    for reference in sets.references() {
        let set = resolve_property(
            source.identity.segment_token.as_str(),
            reference.index(),
            index,
        )?;
        let PmDcFeaturePropertyKind::FilletEdgeSet {
            edges,
            radius,
            selection,
            continuity,
        } = &set.kind
        else {
            return None;
        };
        let selection = resolve_property(
            source.identity.segment_token.as_str(),
            selection.index(),
            index,
        )?;
        if !matches!(
            selection.kind,
            PmDcFeaturePropertyKind::WideEnumeration {
                type_value: 4,
                value: 0
            }
        ) || !matches!(
            resolve_property(
                source.identity.segment_token.as_str(),
                continuity.index(),
                index
            )?
            .kind,
            PmDcFeaturePropertyKind::Boolean { value: false, .. }
        ) {
            return None;
        }
        let edge_collection =
            resolve_property(source.identity.segment_token.as_str(), edges.index(), index)?;
        let PmDcFeaturePropertyKind::References {
            family: PmDcFeatureReferenceFamily::EdgeCollection,
            items,
        } = &edge_collection.kind
        else {
            return None;
        };
        if !closed_edge_items(source.identity.segment_token.as_str(), items, index) {
            return None;
        }
        let radius_reference = radius.index();
        cadmpeg_ir::scalar::PositiveLength::new(
            length_reference(
                source.identity.segment_token.as_str(),
                radius.index(),
                index,
            )?
            .get(),
        )?;
        groups.push(FilletGroup {
            edges: EdgeSelection::Native(edge_collection.id()),
            radius: RadiusSpec::Constant {
                radius: reference_binding(
                    source.identity.segment_token.as_str(),
                    radius_reference,
                    index,
                )?,
            },
            tangency_weight: None,
        });
    }
    if groups.is_empty() {
        return None;
    }
    let (feature_id, result) = match feature_result(ctx, source, 15, index)? {
        Ok(value) => value,
        Err(error) => return Some(Err(error)),
    };
    let source_properties = boolean_properties(source, &[2, 3, 4, 5, 8], index);
    if let Err(error) = ctx.charge_entities(1, "project Inventor feature") {
        return Some(Err(error));
    }
    Some(Ok((
        Feature {
            id: feature_id,
            ordinal: u64::from(label.index),
            name: label.name().map(str::to_owned),
            suppressed: None,
            needs_recompute: false,
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
            native_ref: Some(source.id()),
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
    if !closed_edge_items(source.identity.segment_token.as_str(), items, index) {
        return None;
    }
    cadmpeg_ir::scalar::PositiveLength::try_from(length_parameter(source, 2, index)?).ok()?;
    let distance = parameter_binding(source, 2, index)?;
    let flip_direction = boolean(source, 5, index)?;
    let (feature_id, result) = match feature_result(ctx, source, 11, index)? {
        Ok(value) => value,
        Err(error) => return Some(Err(error)),
    };
    let source_properties = boolean_properties(source, &[6, 9], index);
    if let Err(error) = ctx.charge_entities(1, "project Inventor feature") {
        return Some(Err(error));
    }
    Some(Ok((
        Feature {
            id: feature_id,
            ordinal: u64::from(label.index),
            name: label.name().map(str::to_owned),
            suppressed: None,
            needs_recompute: false,
            dependencies: DistinctMembers::default(),
            source_properties,
            source_tag: Some("chamfer".into()),
            source_text: None,
            source_content: FeatureContent::default(),

            evaluation: cadmpeg_ir::features::FeatureEvaluation::from_definition(
                FeatureDefinition::Operation(FeatureOperation::Chamfer {
                    groups: cadmpeg_ir::features::NonEmptyMembers::one(ChamferGroup {
                        edges: EdgeSelection::Native(edges.id()),
                        spec: ChamferSpec::Distance { distance },
                    }),
                    flip_direction,
                }),
            ),
            native_ref: Some(source.id()),
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
        0 if point_angle.get() == 0.0 => HoleKind::Simple,
        0 => HoleKind::SimpleDrilled {
            drill_point_angle: {
                cadmpeg_ir::scalar::InteriorAngle::try_from(point_angle).ok()?;
                parameter_binding(source, 6, index)?
            },
        },
        1 => HoleKind::Countersink {
            diameter: {
                cadmpeg_ir::scalar::PositiveLength::try_from(head_diameter).ok()?;
                parameter_binding(source, 3, index)?
            },
            angle: {
                cadmpeg_ir::scalar::InteriorAngle::try_from(head_angle).ok()?;
                parameter_binding(source, 5, index)?
            },
        },
        2 if point_angle.get() == 0.0 => HoleKind::Counterbore {
            diameter: {
                cadmpeg_ir::scalar::PositiveLength::try_from(head_diameter).ok()?;
                parameter_binding(source, 3, index)?
            },
            depth: {
                cadmpeg_ir::scalar::PositiveLength::try_from(head_depth).ok()?;
                parameter_binding(source, 4, index)?
            },
        },
        2 => HoleKind::CounterboreDrilled {
            diameter: {
                cadmpeg_ir::scalar::PositiveLength::try_from(head_diameter).ok()?;
                parameter_binding(source, 3, index)?
            },
            depth: {
                cadmpeg_ir::scalar::PositiveLength::try_from(head_depth).ok()?;
                parameter_binding(source, 4, index)?
            },
            drill_point_angle: {
                cadmpeg_ir::scalar::InteriorAngle::try_from(point_angle).ok()?;
                parameter_binding(source, 6, index)?
            },
        },
        _ => return None,
    };
    let extent = match enum16(source, 9, PmDcFeatureEnumFamily::Extent, index)? {
        1 => LinearTermination::Blind {
            length: {
                cadmpeg_ir::scalar::PositiveLength::try_from(depth).ok()?;
                parameter_binding(source, 2, index)?
            },
        },
        4 => LinearTermination::ThroughNext {},
        5 => LinearTermination::ThroughAll {},
        _ => return None,
    };
    let transform_reference = source.properties.references().get(8)?;
    let transform = index
        .transforms
        .get(&(
            source.identity.segment_token.as_str(),
            transform_reference.index().checked_sub(1)?,
        ))
        .and_then(Option::as_ref)?;
    if transform.matrix.rows()[3]
        .iter()
        .zip([0.0, 0.0, 0.0, 1.0])
        .any(|(actual, expected)| (actual - expected).abs() > EPS_FEATURE_PROJECT_HOLE_E10)
    {
        return None;
    }
    let direction_record = resolve_direction(source, 16, index)?;
    let direction = cadmpeg_ir::units::UnitVector3::normalized(Vector3::new(
        direction_record.direction[0].get(),
        direction_record.direction[1].get(),
        direction_record.direction[2].get(),
    ))?;
    let placement = slot_property(source, 21, index)?;
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
        Some({
            cadmpeg_ir::scalar::PositiveLength::try_from(diameter).ok()?;
            parameter_binding(source, 1, index)?
        }),
    )
    .ok()?;
    let (feature_id, result) = match feature_result(ctx, source, 24, index)? {
        Ok(value) => value,
        Err(error) => return Some(Err(error)),
    };
    if let Err(error) = ctx.charge_entities(1, "project Inventor feature") {
        return Some(Err(error));
    }
    Some(Ok((
        Feature {
            id: feature_id,
            ordinal: u64::from(label.index),
            name: label.name().map(str::to_owned),
            suppressed: None,
            needs_recompute: false,
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
            native_ref: Some(source.id()),
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
    let collection = slot_property(source, slot, index)?;
    let PmDcFeaturePropertyKind::References {
        family: PmDcFeatureReferenceFamily::ObjectCollection,
        items,
    } = &collection.kind
    else {
        return None;
    };
    let mut bodies = Vec::new();
    for reference in items.references() {
        let body = resolve_property(
            source.identity.segment_token.as_str(),
            reference.index(),
            index,
        )?;
        if !matches!(body.kind, PmDcFeaturePropertyKind::SurfaceBody { .. }) {
            return None;
        }
        bodies.push(cadmpeg_core::text::NonBlankString::new(body.id())?);
    }
    if bodies.is_empty() {
        return None;
    }
    let members =
        cadmpeg_ir::features::FeatureResultMembers::new(bodies, Vec::new(), Vec::new(), Vec::new())
            .ok()?;
    if let Err(error) = ctx.charge_entities(1, "project Inventor feature result topology") {
        return Some(Err(error));
    }
    let key = match source.identity.key() {
        Ok(key) => key,
        Err(error) => return Some(Err(error)),
    };
    let feature_id = FeatureId::compose(
        &cadmpeg_ir::identity_namespace!("inventor", "design", "feature"),
        key.clone(),
    );
    let result = FeatureResultTopology::new(
        FeatureResultTopologyId::compose(
            &cadmpeg_ir::identity_namespace!("inventor", "design", "feature-result"),
            key,
        ),
        feature_id.clone(),
        members,
        Some(collection.id()),
    );
    Some(Ok((feature_id, result)))
}

fn closed_edge_items(token: &str, items: &PmDcReferenceList, index: &ProjectionIndex<'_>) -> bool {
    !items.references().is_empty()
        && items.references().iter().all(|reference| {
            resolve_property(token, reference.index(), index).is_some_and(|property| {
                matches!(
                    &property.kind,
                    PmDcFeaturePropertyKind::EdgeItem {
                        index_references,
                        ..
                    } if !index_references.values().is_empty()
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
        source.identity.segment_token.as_str(),
        source.properties.references().get(slot)?.index(),
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
        .and_then(Option::as_ref)
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
    let reference = source.properties.references().get(slot)?;
    index
        .directions
        .get(&(
            source.identity.segment_token.as_str(),
            reference.index().checked_sub(1)?,
        ))
        .and_then(Option::as_ref)
        .copied()
}

fn parameter_binding<Q>(
    source: &PmDcFeature,
    slot: usize,
    index: &ProjectionIndex<'_>,
) -> Option<cadmpeg_ir::features::Dimension<Q>> {
    reference_binding(
        source.identity.segment_token.as_str(),
        source.properties.references().get(slot)?.index(),
        index,
    )
}

fn reference_binding<Q>(
    token: &str,
    reference: u32,
    index: &ProjectionIndex<'_>,
) -> Option<cadmpeg_ir::features::Dimension<Q>> {
    let parameter = index
        .parameters
        .get(&(token, reference.checked_sub(1)?))
        .and_then(Option::as_ref)?;
    Some(cadmpeg_ir::features::Dimension::Parameter(
        (*index.parameter_ids.get(parameter.id().as_str())?).clone(),
    ))
}

fn length_parameter(
    source: &PmDcFeature,
    slot: usize,
    index: &ProjectionIndex<'_>,
) -> Option<Length> {
    length_reference(
        source.identity.segment_token.as_str(),
        source.properties.references().get(slot)?.index(),
        index,
    )
}

fn length_reference(token: &str, reference: u32, index: &ProjectionIndex<'_>) -> Option<Length> {
    let parameter = index
        .parameters
        .get(&(token, reference.checked_sub(1)?))
        .and_then(Option::as_ref)?;
    match index.parameter_values.get(parameter.id().as_str())? {
        ParameterValue::Length(value) if value.get() >= 0.0 => Some(*value),
        _ => None,
    }
}

fn angle_parameter(
    source: &PmDcFeature,
    slot: usize,
    index: &ProjectionIndex<'_>,
) -> Option<Angle> {
    let reference = source.properties.references().get(slot)?;
    let parameter = index
        .parameters
        .get(&(
            source.identity.segment_token.as_str(),
            reference.index().checked_sub(1)?,
        ))
        .and_then(Option::as_ref)?;
    match index.parameter_values.get(parameter.id().as_str())? {
        ParameterValue::Angle(value) => Some(*value),
        _ => None,
    }
}

fn boolean_properties(
    source: &PmDcFeature,
    slots: &[usize],
    index: &ProjectionIndex<'_>,
) -> BTreeMap<cadmpeg_core::text::NonBlankString, String> {
    let mut properties = BTreeMap::new();
    for slot in slots {
        if let Some(value) = boolean(source, *slot, index) {
            properties.insert(
                cadmpeg_core::nonblank_literal!("property_{slot}_boolean"),
                value.to_string(),
            );
        }
    }
    properties
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

pub(crate) type PmDcKernelEntityLink = Located<PmDcKernelEntityLinkPayload>;

impl RecordPayload for PmDcKernelEntityLinkPayload {
    const KIND: &'static str = "kernel-entity-link";
}

pub(crate) type PmDcWorkPlane = Located<PmDcWorkPlanePayload>;

impl RecordPayload for PmDcWorkPlanePayload {
    const KIND: &'static str = "work-plane";
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
        PmDcFeatureReferenceFamily, PmDcLinkedHeader, PmDcPatternFamily, PmDcWorkPlane,
        PmDcWorkPlanePayload, ProjectionIndex, BOOLEAN_TYPE, CHAMFER_CLASS_ID,
        END_OF_FEATURES_TYPE, ENTITY_STYLE_LINK_TYPE, EXTRUSION_CLASS_ID, FEATURE_LABEL_TYPE,
        FEATURE_TYPE, FILLET_CLASS_ID, HOLE_CLASS_ID, MIRROR_FEATURE_TYPE, PLANE_SURFACE_FORM,
        RECTANGULAR_PATTERN_FEATURE_TYPE, WORK_PLANE_TYPE,
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
    fn feature_projection_reports_an_unlabelled_feature() {
        let inventory = super::FeatureInventory {
            features: vec![test_feature(0, 0, &[])],
            pattern_features: Vec::new(),
            terminators: Vec::new(),
            properties: Vec::new(),
            labels: Vec::new(),
            entity_style_links: Vec::new(),
            kernel_entity_links: Vec::new(),
            work_planes: Vec::new(),
            issues: Vec::new(),
        };
        let design = crate::design::DesignInventory {
            parameters: Vec::new(),
            expressions: Vec::new(),
            units: Vec::new(),
            parameter_tables: Vec::new(),
            model_roots: Vec::new(),
            brep_segments: Vec::new(),
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
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &DecodePolicy::service())
            .expect("build the decode context");
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
            .expect("build the decode context");
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
            .expect("build the decode context");
        assert!(super::feature_result(&ctx, &source, 0, &index).is_none());
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
    fn feature_terminator_is_admitted() {
        let mut payload = content(0);
        payload.extend_from_slice(&0u32.to_le_bytes());
        assert_eq!(
            inventory_with_record(END_OF_FEATURES_TYPE, &payload, DecodePolicy::service())
                .expect("terminator is admitted")
                .terminators
                .len(),
            1
        );
    }

    #[test]
    fn feature_parse_issue_is_recorded() {
        assert_eq!(
            inventory_with_record(END_OF_FEATURES_TYPE, &[], DecodePolicy::service())
                .expect("truncated terminator becomes an issue")
                .issues
                .len(),
            1
        );
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
                .expect("build the record inventory")
                .issues
                .len(),
            1
        );
    }

    pub(super) fn segment() -> cadmpeg_ir::ids::IdentityKey {
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

    pub(super) fn reference(index: u32) -> crate::pmdc::PmDcReference {
        crate::pmdc::PmDcReference::new(index, index != 0)
            .expect("test reference index fits 31 bits")
    }

    pub(super) fn reference_list(values: &[u32]) -> PmDcReferenceList {
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

    pub(super) fn test_header() -> PmDcContentHeader {
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
            segment().clone(),
            ordinal,
        )
    }

    pub(super) fn test_feature(
        ordinal: u32,
        slot_count: usize,
        slots: &[(usize, u32)],
    ) -> PmDcFeature {
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
            crate::record_identity::RecordTypeId::from_bytes(FEATURE_TYPE),
            segment().clone(),
            ordinal,
        )
    }

    #[test]
    fn located_label_retains_empty_name_and_rejects_wrong_class_width() {
        let label = test_label(0, 1, EXTRUSION_CLASS_ID, &[]);
        let valid = serde_json::to_value(&label).expect("valid label fixture");
        let admitted: PmDcFeatureLabel =
            serde_json::from_value(valid.clone()).expect("valid label fixture");
        assert_eq!(
            serde_json::to_value(admitted).expect("valid label fixture"),
            valid
        );
        for (field, value) in [("class_id", "a".repeat(31)), ("class_id", "a".repeat(33))] {
            let mut wire = valid.clone();
            wire[field] = serde_json::json!(value);
            assert!(serde_json::from_value::<PmDcFeatureLabel>(wire)
                .expect_err("invalid label")
                .to_string()
                .contains(field));
        }
        let mut unnamed = valid.clone();
        unnamed["name"] = serde_json::json!("");
        let label: PmDcFeatureLabel =
            serde_json::from_value(unnamed.clone()).expect("optional label");
        assert!(label.name().is_none());
        assert_eq!(serde_json::to_value(label).expect("label JSON"), unnamed);
        let mut wire = valid;
        wire["class_id"] = serde_json::json!("z".repeat(32));
        assert!(serde_json::from_value::<PmDcFeatureLabel>(wire).is_err());
    }

    pub(super) fn test_label(
        owner_ordinal: u32,
        index: u32,
        class_id: ClassId,
        participants: &[u32],
    ) -> PmDcFeatureLabel {
        Located::new(
            PmDcFeatureLabelPayload::try_from(PmDcFeatureLabelPayloadWire {
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
                class_id: class_id.into(),
            })
            .expect("valid label fixture"),
            crate::record_identity::RecordTypeId::from_bytes(FEATURE_LABEL_TYPE),
            segment().clone(),
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
            segment().clone(),
            ordinal,
        )
    }

    fn neutral_parameter(
        raw: &crate::design::PmDcParameter,
        value: ParameterValue,
    ) -> DesignParameter {
        DesignParameter {
            driving: true,
            id: ParameterId::mint(format!(
                "inventor:design:parameter#{}",
                raw.identity.record_ordinal
            ))
            .expect("identity grammar"),
            owner: None,
            ordinal: raw.identity.record_ordinal,
            name: raw.name.clone(),
            expression: cadmpeg_ir::features::Expression::text(String::new()),
            display: None,
            value: Some(value),

            properties: BTreeMap::new(),
            pmi: None,
            native_ref: Some(raw.id()),
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
            parameter_ids: neutral_parameters
                .iter()
                .filter_map(|parameter| Some((parameter.native_ref.as_deref()?, &parameter.id)))
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
                        .map(|native| (native, sketch.id.clone()))
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
            work_planes: std::collections::HashMap::new(),
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
        let parsed = parse(&feature, |_, source| {
            parse_feature(source, 16).expect("feature")
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
    fn feature_label_parses_its_name() {
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
    }

    #[test]
    fn feature_record_forms_are_admitted() {
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
            (FEATURE_TYPE, feature, "admit Inventor PmDc feature record"),
            (
                RECTANGULAR_PATTERN_FEATURE_TYPE,
                pattern_feature_bytes(16, PmDcPatternFamily::Rectangular),
                "admit Inventor PmDc pattern feature record",
            ),
            (
                MIRROR_FEATURE_TYPE,
                pattern_feature_bytes(16, PmDcPatternFamily::Mirror),
                "admit Inventor PmDc pattern feature record",
            ),
            (
                FEATURE_LABEL_TYPE,
                feature_label_bytes(),
                "admit Inventor PmDc feature label record",
            ),
            (
                ENTITY_STYLE_LINK_TYPE,
                entity_link,
                "admit Inventor PmDc entity style link record",
            ),
            (
                BOOLEAN_TYPE,
                property,
                "admit Inventor PmDc feature property record",
            ),
        ];
        for (type_id, payload, operation) in cases {
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
            let parsed = parse(&bytes, |_, source| {
                parse_pattern_feature(source, version, family).expect("pattern feature")
            });
            assert_eq!(parsed.family, family);
            assert_eq!(parsed.participants.references().len(), 2);
            assert_eq!(parsed.property_slots.len(), slots);
            assert_eq!(parsed.extension_values.len(), extensions);
        }
    }

    #[test]
    fn pattern_feature_extension_values_are_read() {
        let bytes = pattern_feature_bytes(21, PmDcPatternFamily::Mirror);
        let arena = DecodeArena::new();
        let (_, source) = DecodeContext::from_root_bytes(&bytes, &arena, &DecodePolicy::service())
            .expect("pattern feature view");
        assert_eq!(
            parse_pattern_feature(source, 21, PmDcPatternFamily::Mirror)
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
                if matches!(&groups[0].radius, RadiusSpec::Constant { radius: actual_radius } if actual_radius.value_from(std::slice::from_ref(&neutral_radius)).expect("bound radius").get() == 2.5)
        ));
        assert_eq!(
            result
                .bodies()
                .iter()
                .map(|id| id.as_str().to_owned())
                .collect::<Vec<_>>(),
            vec![fillet_properties[8].id()]
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
            }) if matches!(&groups[0].spec, ChamferSpec::Distance { distance: actual_distance } if actual_distance.value_from(std::slice::from_ref(&neutral_distance)).expect("bound distance").get() == 1.25)
        ));
    }

    type GeneratedExtrusion = (Feature, FeatureResultTopology, Vec<DesignParameter>);

    fn generated_extrusion(
        selections: &[u32],
        policy: DecodePolicy,
    ) -> Option<Result<GeneratedExtrusion, CodecError>> {
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
            segment().clone(),
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
            native_ref: Some(raw_sketch.id()),
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
            segment().clone(),
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
            crate::record_identity::RecordTypeId::from_bytes(ENTITY_STYLE_LINK_TYPE),
            segment().clone(),
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
            .map(|result| result.map(|(feature, topology)| (feature, topology, neutral_parameters)))
    }

    #[test]
    fn duplicate_extrusion_selections_skip_before_feature_entity_charge() {
        let mut policy = DecodePolicy::service();
        policy.limits.max_entities = 0;
        assert!(generated_extrusion(&[4, 4], policy).is_none());
    }

    #[test]
    fn projects_generated_extrusion() {
        let (projected, _, neutral_parameters) = generated_extrusion(&[4], DecodePolicy::service())
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
            }) if ( actual_length.value_from(&neutral_parameters).expect("bound extrusion length").get() == 12.0 && actual_draft.value_from(&neutral_parameters).expect("bound draft").get() == 0.1) && matches!(geometry_1.get(), Vector3 { z: -1.0, .. })
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
            segment().clone(),
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
            segment().clone(),
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
             if matches!(geometry_1.get(), Point3 { x: 10.0, y: 20.0, z: 30.0 }) && matches!(geometry_2.get(), Vector3 { z: -1.0, .. }))) && actual_diameter.value_from(&neutral_parameters).expect("bound hole dimension").get() == 9.0 && actual_depth.value_from(&neutral_parameters).expect("bound hole dimension").get() == 3.0 && actual_drill_point_angle.value_from(&neutral_parameters).expect("bound hole dimension").get() == 2.0 && actual_diameter_2.value_from(&neutral_parameters).expect("bound hole dimension").get() == 5.0)));
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
        assert_eq!(parsed.name, "Extrude1");
        assert_eq!(parsed.participants.references().len(), 1);
        assert_eq!(
            parsed.class_id,
            ClassId(cadmpeg_ir::native::bytes::NativeBytes::new([0xab; 16]))
        );
    }

    #[test]
    fn class_id_admits_only_the_canonical_lowercase_spelling() {
        let lower = "ab".repeat(16);
        assert_eq!(
            String::from(ClassId::try_from(lower.clone()).expect("lowercase class id")),
            lower
        );
        assert!(ClassId::try_from("AB".repeat(16)).is_err());
    }

    #[test]
    fn class_id_native_writer_streams_the_existing_hex_bytes() {
        #[derive(serde::Serialize)]
        struct Record<'a> {
            id: &'static str,
            value: &'a ClassId,
        }
        let class_id = ClassId(cadmpeg_ir::native::bytes::NativeBytes::new([0xab; 16]));
        let owned = String::from(class_id);
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
        let label = PmDcFeatureLabelPayload::try_from(PmDcFeatureLabelPayloadWire {
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
        })
        .expect("feature label");
        let owned = PmDcFeatureLabelPayloadWire::from(label.clone());
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

    mod extents;
}
