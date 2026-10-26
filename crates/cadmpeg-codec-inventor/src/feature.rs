// SPDX-License-Identifier: Apache-2.0
//! Typed `PmDc` feature records and feature-list terminators.
use cadmpeg_core::decode::unique_index;
use cadmpeg_ir::native::bytes::NativeBytes;

use cadmpeg_ir::features::{PlanarProfileRef, ProfileRef, RevolveConstruction};

use std::collections::{BTreeMap, HashMap, HashSet};

use cadmpeg_core::decode::{DecodeContext, View};
use cadmpeg_core::CodecError;
use cadmpeg_ir::ids::FeatureResultTopologyId;
use cadmpeg_ir::math::{Point3, Vector3};
use cadmpeg_ir::sketches::Sketch;
use cadmpeg_ir::{
    features::{
        edge_treatments::{ChamferForm, ChamferGroup, ChamferSpec, FilletGroup, RadiusSpec},
        holes::HoleKind,
        BooleanOp, DesignParameter, DistinctMembers, EdgeSelection, ExtrudeDirection,
        ExtrudeExtent, ExtrudeSide, ExtrudeStart, ExtrusionDirectionSource, FaceSelection, Feature,
        FeatureContent, FeatureDefinition, FeatureId, FeatureOperation, FeatureResultTopology,
        LinearTermination, ParameterValue,
    },
    scalar::{Angle, Length},
};
use serde::{Deserialize, Serialize};

use crate::pmdc::{
    content_header, inventor_id, reference_list, state_content_header, type_id_string, u32_list,
    Cursor, PmDcContentHeader, PmDcReferenceList, PmDcU32List,
};
use crate::record_identity::{push_record, Located, RecordPayload};
use crate::record_issue::{RecordIssue, RecordIssueFamily};
use crate::rse::{RecordFrameState, RseInventory, SegmentBulkState, SegmentKind};
use crate::{design::DesignInventory, sketch::SketchInventory};

pub(crate) mod base;
mod edge_selection;
mod history;
mod hole;
pub(crate) mod loft_sweep;
pub(crate) mod profile;
pub(crate) mod revolve;
pub(crate) mod sheet_metal;
mod split;

const EPS_FEATURE_PROJECT_HOLE_E10: f64 = 1.0e-10;

const FEATURE_TYPE: [u8; 16] = [
    0x91, 0x4d, 0x87, 0x90, 0xd0, 0x11, 0xf8, 0xd1, 0x00, 0x08, 0xca, 0xbc, 0x06, 0x63, 0xdc, 0x09,
];
const END_OF_FEATURES_TYPE: [u8; 16] = [
    0x24, 0xfd, 0x41, 0x8f, 0xd2, 0x11, 0xac, 0x6e, 0x00, 0x08, 0x2a, 0xab, 0x32, 0xa3, 0xdc, 0x09,
];
pub(crate) const BOOLEAN_TYPE: [u8; 16] = inventor_id(0x9087_4d28);
const PLATE_SIDE_TYPE: [u8; 16] = [
    0x69, 0xb9, 0x50, 0x5e, 0xfb, 0x4b, 0xba, 0xa9, 0x25, 0xc3, 0xdf, 0xa8, 0x70, 0x81, 0x78, 0x10,
];
const SURFACE_BODY_TYPE: [u8; 16] = inventor_id(0x9087_4d47);
const ENTITY_STYLE_LINK_TYPE: [u8; 16] = inventor_id(0x9087_4d15);
const PART_OPERATION_TYPE: [u8; 16] = [
    0x28, 0xbe, 0x9a, 0x72, 0xd1, 0x11, 0x44, 0x09, 0x00, 0x08, 0x4e, 0xba, 0x32, 0xa3, 0xdc, 0x09,
];
const BOUNDARY_PATCH_TYPE: [u8; 16] = [
    0x91, 0x73, 0x94, 0x22, 0xd1, 0x11, 0x07, 0xcf, 0x00, 0x08, 0x35, 0xbd, 0x06, 0x63, 0xdc, 0x09,
];
pub(crate) const SELECTION_LIST_TYPE: [u8; 16] = [
    0x7a, 0x25, 0xf2, 0xef, 0xd1, 0x11, 0x05, 0xaf, 0x00, 0x08, 0x47, 0xb0, 0x05, 0x24, 0xdc, 0x09,
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
pub(crate) const FACE_COLLECTION_TYPE: [u8; 16] = [
    0xf1, 0xb2, 0xbc, 0x24, 0xd0, 0x11, 0x21, 0xdd, 0x00, 0x08, 0xd0, 0xbc, 0x06, 0x63, 0xdc, 0x09,
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
pub(crate) const FACE_ITEM_TYPE: [u8; 16] = [
    0xcc, 0xb5, 0x9f, 0x50, 0xd1, 0x11, 0x4f, 0x6b, 0x00, 0x08, 0xa1, 0xba, 0x32, 0xa3, 0xdc, 0x09,
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
/// Profile selection of a sheet-metal plate; it uses the profile-selection layout.
const PLATE_PROFILE_SELECTION_TYPE: [u8; 16] = [
    0x47, 0x3f, 0x20, 0xfc, 0xd1, 0x11, 0x01, 0xcf, 0x00, 0x08, 0x35, 0xbd, 0x06, 0x63, 0xdc, 0x09,
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
const WORK_AXIS_TYPE: [u8; 16] = [
    0x89, 0x6c, 0xf0, 0x8e, 0xd1, 0x11, 0x3c, 0x04, 0x60, 0x00, 0x7c, 0xb8, 0x01, 0xf3, 0x1b, 0xb0,
];
/// Revolve extent value of a full revolution.
const REVOLVE_EXTENT_FULL: u16 = 3;
const FEATURE_LABEL_TYPE: [u8; 16] = [
    0x2b, 0xa4, 0x48, 0x2b, 0xd2, 0x11, 0x58, 0x64, 0x60, 0x00, 0x74, 0xb7, 0x9b, 0x49, 0xeb, 0xb0,
];

#[derive(Debug, Default)]
pub(crate) struct FeatureInventory {
    edge_inputs: edge_selection::EdgeInventory,
    record_types: HashMap<cadmpeg_ir::ids::IdentityKey, HashMap<u32, Option<String>>>,
    pub(crate) features: Vec<PmDcFeature>,
    pub(crate) pattern_features: Vec<PmDcPatternFeature>,
    pub(crate) terminators: Vec<PmDcFeatureTerminator>,
    pub(crate) properties: Vec<PmDcFeatureProperty>,
    pub(crate) sheet_metal_styles: Vec<sheet_metal::style::Style>,
    pub(crate) sheet_metal_flanges: Vec<sheet_metal::flange::Flange>,
    pub(crate) dimensions: Vec<revolve::Dimension>,
    pub(crate) labels: Vec<PmDcFeatureLabel>,
    pub(crate) entity_style_links: Vec<PmDcEntityStyleLink>,
    pub(crate) kernel_entity_links: Vec<PmDcKernelEntityLink>,
    pub(crate) work_planes: Vec<PmDcWorkPlane>,
    /// Wire bodies of `PmDc` kernel-body records, which profile selections name.
    pub(crate) issues: Vec<RecordIssue>,
}

pub(crate) struct FeatureProjection {
    pub(crate) features: Vec<Feature>,
    pub(crate) result_topologies: Vec<FeatureResultTopology>,
    pub(crate) input_topologies: Vec<cadmpeg_ir::features::FeatureInputTopology>,
    pub(crate) unresolved_features: usize,
    pub(crate) unresolved_states: usize,
    pub(crate) revolve_losses: Vec<cadmpeg_ir::report::loss::LossNote>,
    /// Closed loops that resolved extrusion profiles append to sketch
    /// profile tables, by sketch id, in table order.
    pub(crate) added_profiles: HashMap<String, Vec<Vec<cadmpeg_ir::sketches::SketchEntityUse>>>,
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
    SheetMetal {
        definition: sheet_metal::SheetMetalProperty,
    },
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
    LoftSection {
        entity_link: crate::pmdc::PmDcReference,
        value: u8,
        condition: crate::pmdc::PmDcReference,
        parameters: [crate::pmdc::PmDcReference; 2],
        map_value: u32,
        source_value: u32,
        option: crate::pmdc::PmDcReference,
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
        /// The fifth reference that segment major version 23 and later store:
        /// null or a Boolean record.
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            deserialize_with = "deserialize_edge_set_option"
        )]
        option: Option<crate::pmdc::PmDcReference>,
    },
    EdgeItem {
        index_references: PmDcU32List,
        index_reference_value: i32,
        value: u32,
    },
    FaceItem {
        index_references: PmDcU32List,
    },
    TerminationFlag {
        value: u8,
    },
    /// A work axis: the line through `origin` along `span`, in internal
    /// centimetres.
    WorkAxis {
        entity_flags: u32,
        values: Vec<u32>,
        references: PmDcReferenceList,
        marker: bool,
        origin: [cadmpeg_ir::scalar::FiniteReal; 3],
        span: [cadmpeg_ir::scalar::FiniteReal; 3],
        state: u8,
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
    PlateSide,
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
    SelectionList,
    HoleCenters,
    FeatureDimensions,
    ObjectCollection,
    FilletEdgeSets,
    EdgeCollection,
    FaceCollection,
    LoftSections,
    SheetMetalAttachmentSelections,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct PmDcLinkedHeader {
    header_value: u32,
    header_id: u16,
    values: [u32; 2],
    /// The u32 that segment major versions from 28 store after `values`.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_linked_flags_extension"
    )]
    flags_extension: Option<u32>,
    pub(crate) owner: crate::pmdc::PmDcReference,
    pub(crate) parent: crate::pmdc::PmDcReference,
    pub(crate) next: crate::pmdc::PmDcReference,
}

cadmpeg_core::named_optional_field!(deserialize_linked_flags_extension, u32, "flags_extension");
cadmpeg_core::named_optional_field!(
    deserialize_edge_set_option,
    crate::pmdc::PmDcReference,
    "option"
);

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
    /// The stored code units, present only when they hold an unpaired
    /// surrogate that `name` replaces with U+FFFD.
    name_units: Option<Vec<u16>>,
    class_id: ClassId,
}

#[derive(Serialize)]
struct PmDcFeatureLabelPayloadRef<'a> {
    save_version_major: u8,
    header: &'a PmDcLinkedHeader,
    index: u32,
    participants: &'a PmDcReferenceList,
    name: &'a str,
    #[serde(skip_serializing_if = "Option::is_none")]
    name_units: Option<&'a [u16]>,
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
            name_units: self.name_units.as_deref(),
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
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_name_units"
    )]
    name_units: Option<Vec<u16>>,
    class_id: String,
}

cadmpeg_core::named_optional_field!(deserialize_name_units, Vec<u16>, "name_units");

impl TryFrom<PmDcFeatureLabelPayloadWire> for PmDcFeatureLabelPayload {
    type Error = String;
    fn try_from(wire: PmDcFeatureLabelPayloadWire) -> Result<Self, Self::Error> {
        // Exact code units accompany a name only when they are not valid
        // UTF-16 and the name is their replacement decoding.
        if let Some(units) = &wire.name_units {
            if String::from_utf16(units).is_ok() || String::from_utf16_lossy(units) != wire.name {
                return Err(
                    "Inventor feature label code units are valid UTF-16 or differ from its name"
                        .into(),
                );
            }
        }
        Ok(Self {
            save_version_major: wire.save_version_major,
            header: wire.header,
            index: wire.index,
            participants: wire.participants,
            name: wire.name,
            name_units: wire.name_units,
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
            name_units: value.name_units,
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

    /// Whether the neutral name replaces unpaired surrogates of the stored name.
    pub(crate) fn name_replaced(&self) -> bool {
        self.name_units.is_some()
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

/// Link from a selection to one entity of its segment's kernel body.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct PmDcKernelEntityLinkPayload {
    save_version_major: u8,
    header_value: u32,
    header_id: u16,
    pub(crate) selection_source: u32,
    pub(crate) state: u8,
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
        edge_inputs: edge_selection::inventory(ctx, document)?,
        record_types: HashMap::new(),
        features: Vec::new(),
        pattern_features: Vec::new(),
        terminators: Vec::new(),
        properties: Vec::new(),
        dimensions: Vec::new(),
        labels: Vec::new(),
        entity_style_links: Vec::new(),
        kernel_entity_links: Vec::new(),
        work_planes: Vec::new(),
        issues: Vec::new(),
        ..Default::default()
    };
    for segment in &document.segments {
        if !matches!(segment.kind, SegmentKind::PmDc | SegmentKind::PmBRep) {
            continue;
        }
        let Some(version) = segment.registry.map(|join| join.version_major) else {
            continue;
        };
        if !crate::pmdc::record_version_supported(version) {
            continue;
        }
        let SegmentBulkState::Framed(bulk) = &segment.bulk else {
            continue;
        };
        let RecordFrameState::Framed(table) = &bulk.records else {
            continue;
        };
        let record_types = inventory
            .record_types
            .entry(segment.pair.token.key().clone())
            .or_default();
        for record in &table.records {
            record_types
                .entry(record.ordinal)
                .and_modify(|previous| *previous = None)
                .or_insert_with(|| Some(type_id_string(record.type_id)));
            if segment.kind == SegmentKind::PmBRep && record.type_id != KERNEL_ENTITY_LINK_TYPE {
                continue;
            }
            let parsed = match record.type_id {
                sheet_metal::style::TYPE if sheet_metal::style::prefix_len(version).is_some() => {
                    sheet_metal::style::parse(ctx, record.payload, version).and_then(|style| {
                        push_record(
                            ctx,
                            &mut inventory.sheet_metal_styles,
                            style,
                            record.type_id,
                            segment.pair.token.key(),
                            record.ordinal,
                            "admit Inventor sheet-metal style",
                        )
                    })
                }
                sheet_metal::flange::TYPE => {
                    sheet_metal::flange::parse(ctx, record.payload, version).and_then(|flange| {
                        push_record(
                            ctx,
                            &mut inventory.sheet_metal_flanges,
                            flange,
                            record.type_id,
                            segment.pair.token.key(),
                            record.ordinal,
                            "admit Inventor sheet-metal flange",
                        )
                    })
                }
                revolve::DIMENSION_TYPE => {
                    revolve::parse_dimension(ctx, record.payload).and_then(|dimension| {
                        push_record(
                            ctx,
                            &mut inventory.dimensions,
                            dimension,
                            record.type_id,
                            segment.pair.token.key(),
                            record.ordinal,
                            "admit Inventor feature dimension",
                        )
                    })
                }
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

fn pattern_slots(
    cursor: &mut Cursor<'_>,
    count: usize,
    slots: &mut Vec<crate::pmdc::PmDcReference>,
) -> Result<(), CodecError> {
    for _ in 0..count {
        slots.push(cursor.reference("pattern-feature property slot")?);
    }
    Ok(())
}

fn pattern_values(
    cursor: &mut Cursor<'_>,
    count: usize,
    values: &mut Vec<u32>,
) -> Result<(), CodecError> {
    for _ in 0..count {
        values.push(cursor.u32("pattern-feature extension value")?);
    }
    Ok(())
}

fn parse_pattern_feature(
    source: View<'_>,
    version: u8,
    family: PmDcPatternFamily,
) -> Result<PmDcPatternFeaturePayload, CodecError> {
    let mut cursor = Cursor::new(source);
    let header = state_content_header(&mut cursor, version)?;
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
    // Read the version-specific suffix in stored order, keeping its reference
    // words with the slots and its u32 values as extension values.
    match family {
        PmDcPatternFamily::Rectangular if version >= 29 => {
            pattern_slots(&mut cursor, 12, &mut property_slots)?;
            pattern_values(&mut cursor, 8, &mut extension_values)?;
            pattern_slots(&mut cursor, 14, &mut property_slots)?;
            if version >= 30 {
                pattern_values(&mut cursor, 6, &mut extension_values)?;
            }
        }
        PmDcPatternFamily::Rectangular => {
            let remaining = if version > 20 { 26 } else { 20 };
            pattern_slots(&mut cursor, remaining, &mut property_slots)?;
        }
        PmDcPatternFamily::Mirror => {
            pattern_slots(&mut cursor, 5, &mut property_slots)?;
            if version > 20 {
                pattern_values(&mut cursor, 6, &mut extension_values)?;
            }
            if version >= 29 {
                pattern_slots(&mut cursor, 1, &mut property_slots)?;
                pattern_values(&mut cursor, 8, &mut extension_values)?;
                pattern_slots(&mut cursor, 1, &mut property_slots)?;
            } else {
                pattern_slots(&mut cursor, 2, &mut property_slots)?;
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
    let header = state_content_header(&mut cursor, version)?;
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
    let header = state_content_header(&mut cursor, version)?;
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
        loft_sweep::SECTION_LIST_TYPE => Some(loft_sweep::parse_sections),
        loft_sweep::SECTION_TYPE => Some(loft_sweep::parse_section),
        loft_sweep::LOFT_MODE_TYPE
        | loft_sweep::SECTION_CONDITION_TYPE
        | loft_sweep::SWEEP_MODE_TYPE
        | loft_sweep::SWEEP_ORIENTATION_TYPE
        | loft_sweep::SWEEP_SCALING_TYPE => Some(parse_auxiliary_enum),
        PART_OPERATION_TYPE => Some(parse_part_operation),
        EXTENT_TYPE => Some(parse_extent),
        HOLE_TYPE => Some(parse_hole),
        FILLET_TYPE => Some(parse_fillet),
        CHAMFER_TYPE => Some(parse_chamfer),
        FILLET_EDGE_SELECTION_TYPE => Some(parse_fillet_edge_selection),
        AUXILIARY_ENUM_TYPE => Some(parse_auxiliary_enum),
        PLATE_SIDE_TYPE => Some(parse_plate_side),
        BOOLEAN_TYPE => Some(parse_boolean),
        BOUNDARY_PATCH_TYPE => Some(parse_boundary_patch),
        SELECTION_LIST_TYPE => Some(parse_selection_list),
        hole::CENTERS_TYPE => Some(parse_hole_centers),
        FEATURE_DIMENSIONS_TYPE => Some(parse_feature_dimensions),
        OBJECT_COLLECTION_TYPE => Some(parse_object_collection),
        FILLET_EDGE_SETS_TYPE => Some(parse_fillet_edge_sets),
        RDX_VARIABLE_TYPE => Some(parse_rdx_variable),
        SURFACE_BODY_TYPE => Some(parse_surface_body),
        PROFILE_SELECTION_TYPE | PLATE_PROFILE_SELECTION_TYPE => Some(parse_profile_selection),
        PLACEMENT_TYPE => Some(parse_placement),
        FILLET_EDGE_SET_TYPE => Some(parse_fillet_edge_set),
        EDGE_COLLECTION_TYPE => Some(parse_edge_collection),
        FACE_COLLECTION_TYPE => Some(parse_face_collection),
        EDGE_ITEM_TYPE => Some(parse_edge_item),
        FACE_ITEM_TYPE => Some(parse_face_item),
        TERMINATION_FLAG_TYPE => Some(parse_termination_flag),
        WORK_AXIS_TYPE => Some(parse_work_axis),
        revolve::CONTROL_TYPE => Some(parse_auxiliary_enum),
        _ => sheet_metal::property_parser(type_id).or_else(|| split::property_parser(type_id)),
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
    let header = content_header(&mut cursor, version)?;
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
enum_parser!(parse_plate_side, PlateSide);

fn parse_chamfer(
    _: &DecodeContext<'_>,
    source: View<'_>,
    version: u8,
) -> Result<PmDcFeaturePropertyPayload, CodecError> {
    let mut cursor = Cursor::new(source);
    let header = content_header(&mut cursor, version)?;
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
    let header = content_header(&mut cursor, version)?;
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
    let header = content_header(&mut cursor, version)?;
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
    let header = content_header(&mut cursor, version)?;
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
reference_parser!(parse_selection_list, SelectionList);
reference_parser!(parse_hole_centers, HoleCenters);
reference_parser!(parse_feature_dimensions, FeatureDimensions);
reference_parser!(parse_object_collection, ObjectCollection);
reference_parser!(parse_fillet_edge_sets, FilletEdgeSets);
reference_parser!(parse_edge_collection, EdgeCollection);
reference_parser!(parse_face_collection, FaceCollection);

fn parse_rdx_variable(
    ctx: &DecodeContext<'_>,
    source: View<'_>,
    version: u8,
) -> Result<PmDcFeaturePropertyPayload, CodecError> {
    let mut cursor = Cursor::new(source);
    let header = content_header(&mut cursor, version)?;
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
    let header = content_header(&mut cursor, version)?;
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
    let header = content_header(&mut cursor, version)?;
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
    let header = content_header(&mut cursor, version)?;
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

fn parse_work_axis(
    _: &DecodeContext<'_>,
    source: View<'_>,
    version: u8,
) -> Result<PmDcFeaturePropertyPayload, CodecError> {
    let mut cursor = Cursor::new(source);
    let header = content_header(&mut cursor, version)?;
    let entity_flags = cursor.u32("work axis entity flags")?;
    let mut values = vec![
        cursor.u32("work axis value")?,
        cursor.u32("work axis value")?,
    ];
    if version > 21 {
        values.push(cursor.u32("work axis value")?);
    }
    let references = reference_list(&mut cursor, 2, "work axis references")?;
    let marker = cursor.peek_u32("work axis frame marker")? == WORK_PLANE_FRAME_MARKER;
    if marker {
        cursor.u32("work axis frame marker")?;
    }
    let origin = f64_triple(&mut cursor, "work axis origin")?;
    let span = f64_triple(&mut cursor, "work axis span")?;
    let state = cursor.u8("work axis state")?;
    cursor.finish("work axis")?;
    Ok(property(
        version,
        header,
        PmDcFeaturePropertyKind::WorkAxis {
            entity_flags,
            values,
            references,
            marker,
            origin,
            span,
            state,
        },
    ))
}

fn parse_termination_flag(
    _: &DecodeContext<'_>,
    source: View<'_>,
    version: u8,
) -> Result<PmDcFeaturePropertyPayload, CodecError> {
    let mut cursor = Cursor::new(source);
    let header = content_header(&mut cursor, version)?;
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
    let header = content_header(&mut cursor, version)?;
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
    let header = linked_header(&mut cursor, version)?;
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
    let header = content_header(&mut cursor, version)?;
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
    let header = content_header(&mut cursor, version)?;
    let edges = cursor.reference("fillet edge-set edges reference")?;
    let radius = cursor.reference("fillet edge-set radius reference")?;
    let selection = cursor.reference("fillet edge-set selection reference")?;
    let continuity = cursor.reference("fillet edge-set continuity reference")?;
    let option = if version >= 23 {
        Some(cursor.reference("fillet edge-set option reference")?)
    } else {
        None
    };
    cursor.finish("fillet edge set")?;
    Ok(property(
        version,
        header,
        PmDcFeaturePropertyKind::FilletEdgeSet {
            edges,
            radius,
            selection,
            continuity,
            option,
        },
    ))
}

fn parse_edge_item(
    _: &DecodeContext<'_>,
    source: View<'_>,
    version: u8,
) -> Result<PmDcFeaturePropertyPayload, CodecError> {
    let mut cursor = Cursor::new(source);
    let header = content_header(&mut cursor, version)?;
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

fn parse_face_item(
    _: &DecodeContext<'_>,
    source: View<'_>,
    version: u8,
) -> Result<PmDcFeaturePropertyPayload, CodecError> {
    let mut cursor = Cursor::new(source);
    let header = content_header(&mut cursor, version)?;
    let index_references = u32_list(&mut cursor, 2, "face-item index references")?;
    if !index_references.values().is_empty() && cursor.i32("face-item terminal value")? != -1 {
        return Err(CodecError::malformed(
            "Inventor PmDc face item has an invalid terminal value",
        ));
    }
    cursor.finish("face item")?;
    Ok(property(
        version,
        header,
        PmDcFeaturePropertyKind::FaceItem { index_references },
    ))
}

fn linked_header(cursor: &mut Cursor<'_>, version: u8) -> Result<PmDcLinkedHeader, CodecError> {
    Ok(PmDcLinkedHeader {
        header_value: cursor.u32("linked header value")?,
        header_id: cursor.u16("linked header id")?,
        values: [
            cursor.u32("linked header value 0")?,
            cursor.u32("linked header value 1")?,
        ],
        flags_extension: crate::pmdc::flags_extension(cursor, version)?,
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
    let header = linked_header(&mut cursor, version)?;
    let index = cursor.u32("feature label index")?;
    let participants = reference_list(&mut cursor, 2, "feature-label participants")?;
    let (name, name_units) = cursor.utf16_units(ctx, "feature label")?;
    let class_id_bytes = cursor.take_array("feature-label class id")?;
    let class_id = type_id_string(class_id_bytes);
    cursor.finish("feature label")?;
    PmDcFeatureLabelPayload::try_from(PmDcFeatureLabelPayloadWire {
        save_version_major: version,
        header,
        index,
        participants,
        name,
        name_units,
        class_id,
    })
    .map_err(CodecError::malformed)
}

const BASE_CLASS_ID: ClassId = ClassId(NativeBytes::new([
    0x2d, 0x11, 0xa9, 0x0c, 0xd0, 0x11, 0x8b, 0x83, 0x00, 0x08, 0x19, 0xb0, 0x05, 0x24, 0xdc, 0x09,
]));
const EXTRUSION_CLASS_ID: ClassId = ClassId(NativeBytes::new([
    0x31, 0x11, 0xa9, 0x0c, 0xd0, 0x11, 0x8b, 0x83, 0x00, 0x08, 0x19, 0xb0, 0x05, 0x24, 0xdc, 0x09,
]));
const FILLET_CLASS_ID: ClassId = ClassId(NativeBytes::new([
    0xdc, 0x15, 0xf7, 0xf1, 0xd1, 0x11, 0x42, 0x05, 0x00, 0x08, 0x30, 0xb0, 0x05, 0x24, 0xdc, 0x09,
]));
/// Sheet-metal corner round: the constant-radius fillet record layout.
const CORNER_ROUND_CLASS_ID: ClassId = ClassId(NativeBytes::new([
    0xb5, 0x4d, 0xd6, 0xf6, 0xd3, 0x11, 0xc4, 0x96, 0x60, 0x00, 0xf2, 0xb2, 0x1d, 0x6e, 0xef, 0xb0,
]));
const REVOLVE_CLASS_ID: ClassId = ClassId(NativeBytes::new([
    0x5f, 0x01, 0x54, 0x00, 0xd2, 0x11, 0x25, 0x1c, 0x60, 0x00, 0x67, 0xb7, 0x9b, 0x49, 0xeb, 0xb0,
]));
/// Sheet-metal plate: the solid of one sheet-metal face.
const PLATE_CLASS_ID: ClassId = ClassId(NativeBytes::new([
    0xc6, 0x5f, 0x58, 0x55, 0xd3, 0x11, 0x73, 0x0c, 0x60, 0x00, 0x3a, 0xa9, 0x9d, 0xcc, 0xef, 0xb0,
]));
const CHAMFER_CLASS_ID: ClassId = ClassId(NativeBytes::new([
    0x3f, 0x71, 0x00, 0xf9, 0xd2, 0x11, 0x8b, 0x6f, 0x60, 0x00, 0xf0, 0xa8, 0x9d, 0xcc, 0xef, 0xb0,
]));
const HOLE_CLASS_ID: ClassId = ClassId(NativeBytes::new([
    0x1a, 0x7d, 0x75, 0x1f, 0xd2, 0x11, 0x9c, 0x54, 0xa0, 0x00, 0x20, 0x80, 0x36, 0x03, 0xc8, 0xc9,
]));
pub(crate) const THREAD_CLASS_ID: ClassId = ClassId(NativeBytes::new([
    0x06, 0x19, 0xaa, 0x28, 0xd4, 0x11, 0xc7, 0x7a, 0x60, 0x00, 0xc4, 0xa4, 0xca, 0xe1, 0xfb, 0xb0,
]));
const THICKEN_CLASS_ID: ClassId = ClassId(NativeBytes::new([
    0xec, 0xf4, 0x81, 0x72, 0x9c, 0x4d, 0x5f, 0x92, 0x5f, 0x21, 0x92, 0xb4, 0xa8, 0x30, 0x96, 0xac,
]));
const SHELL_CLASS_ID: ClassId = ClassId(NativeBytes::new([
    0x12, 0x96, 0x3c, 0xb8, 0xd1, 0x11, 0x8a, 0x28, 0x60, 0x00, 0x8b, 0xb8, 0x01, 0xf3, 0x1b, 0xb0,
]));

/// Native family names of feature-label classes outside the neutral families.
const NATIVE_CLASSES: &[([u8; 16], &str)] = &[
    (
        [
            0x06, 0x19, 0xaa, 0x28, 0xd4, 0x11, 0xc7, 0x7a, 0x60, 0x00, 0xc4, 0xa4, 0xca, 0xe1,
            0xfb, 0xb0,
        ],
        "Thread",
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
            0x12, 0x96, 0x3c, 0xb8, 0xd1, 0x11, 0x8a, 0x28, 0x60, 0x00, 0x8b, 0xb8, 0x01, 0xf3,
            0x1b, 0xb0,
        ],
        "Shell",
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
    Base,
    Extrusion,
    Fillet,
    Chamfer,
    Hole,
    Plate,
    Bend,
    Thicken,
    Shell,
    Sweep,
    Loft,
    Revolve,
}

impl FeatureFamily {
    /// The feature-label classes whose records use this family's layout.
    pub(crate) fn class_ids(self) -> &'static [ClassId] {
        match self {
            Self::Base => &[BASE_CLASS_ID],
            Self::Extrusion => &[EXTRUSION_CLASS_ID],
            Self::Fillet => &[FILLET_CLASS_ID, CORNER_ROUND_CLASS_ID],
            Self::Chamfer => &[CHAMFER_CLASS_ID],
            Self::Hole => &[HOLE_CLASS_ID],
            Self::Plate => &[PLATE_CLASS_ID],
            Self::Bend => &[sheet_metal::BEND_CLASS],
            Self::Thicken => &[THICKEN_CLASS_ID],
            Self::Shell => &[SHELL_CLASS_ID],
            Self::Sweep => &[loft_sweep::slots::SWEEP_CLASS],
            Self::Loft => &[loft_sweep::slots::LOFT_CLASS],
            Self::Revolve => &[REVOLVE_CLASS_ID],
        }
    }

    pub(crate) fn output_slot(self, source: &PmDcFeature) -> usize {
        match self {
            Self::Base => 1,
            Self::Extrusion => 26,
            Self::Fillet => 15,
            Self::Chamfer => 11,
            Self::Hole => 24,
            Self::Plate
                if source.properties.references().len() == 17
                    && source.properties.references()[6].index() == 0 =>
            {
                15
            }
            Self::Plate => 6,
            Self::Bend => 9,
            Self::Thicken => 15,
            Self::Shell => 9,
            Self::Sweep => 15,
            Self::Loft => 13,
            Self::Revolve => 17,
        }
    }

    pub(crate) fn from_class_id(class_id: ClassId) -> Option<Self> {
        [
            Self::Base,
            Self::Extrusion,
            Self::Fillet,
            Self::Chamfer,
            Self::Hole,
            Self::Plate,
            Self::Bend,
            Self::Thicken,
            Self::Shell,
            Self::Sweep,
            Self::Loft,
            Self::Revolve,
        ]
        .into_iter()
        .find(|family| family.class_ids().contains(&class_id))
    }

    /// Whether a neutral definition is the operation this family projects.
    pub(crate) fn admits_definition(self, definition: &FeatureDefinition) -> bool {
        matches!(
            (self, definition),
            (
                Self::Base,
                FeatureDefinition::Operation(FeatureOperation::StoredGeometry {})
            ) | (
                Self::Extrusion,
                FeatureDefinition::Operation(FeatureOperation::Extrude { .. })
            ) | (
                Self::Plate,
                FeatureDefinition::Operation(
                    FeatureOperation::SheetMetalBaseFlange { .. }
                        | FeatureOperation::SheetMetalAttachedPlate { .. }
                )
            ) | (
                Self::Bend,
                FeatureDefinition::Operation(FeatureOperation::SheetMetalBend { .. })
            ) | (
                Self::Fillet,
                FeatureDefinition::Operation(FeatureOperation::Fillet { .. })
            ) | (
                Self::Chamfer,
                FeatureDefinition::Operation(FeatureOperation::Chamfer { .. })
            ) | (
                Self::Hole,
                FeatureDefinition::Operation(FeatureOperation::Hole { .. })
            ) | (
                Self::Thicken,
                FeatureDefinition::Operation(FeatureOperation::Thicken { .. })
            ) | (
                Self::Shell,
                FeatureDefinition::Operation(FeatureOperation::Shell { .. })
            ) | (
                Self::Sweep,
                FeatureDefinition::Operation(FeatureOperation::Sweep { .. })
            ) | (
                Self::Loft,
                FeatureDefinition::Operation(FeatureOperation::Loft { solid: true, .. })
            ) | (
                Self::Revolve,
                FeatureDefinition::Operation(FeatureOperation::Revolve { .. })
            )
        )
    }

    pub(crate) fn from_definition(definition: &FeatureDefinition) -> Option<Self> {
        match definition {
            FeatureDefinition::Operation(FeatureOperation::StoredGeometry {}) => Some(Self::Base),
            FeatureDefinition::Operation(FeatureOperation::Extrude { .. }) => Some(Self::Extrusion),
            FeatureDefinition::Operation(
                FeatureOperation::SheetMetalBaseFlange { .. }
                | FeatureOperation::SheetMetalAttachedPlate { .. },
            ) => Some(Self::Plate),
            FeatureDefinition::Operation(FeatureOperation::SheetMetalBend { .. }) => {
                Some(Self::Bend)
            }
            FeatureDefinition::Operation(FeatureOperation::Fillet { .. }) => Some(Self::Fillet),
            FeatureDefinition::Operation(FeatureOperation::Chamfer { .. }) => Some(Self::Chamfer),
            FeatureDefinition::Operation(FeatureOperation::Hole { .. }) => Some(Self::Hole),
            FeatureDefinition::Operation(FeatureOperation::Thicken { .. }) => Some(Self::Thicken),
            FeatureDefinition::Operation(FeatureOperation::Shell { .. }) => Some(Self::Shell),
            FeatureDefinition::Operation(FeatureOperation::Sweep { .. }) => Some(Self::Sweep),
            FeatureDefinition::Operation(FeatureOperation::Loft { solid: true, .. }) => {
                Some(Self::Loft)
            }
            FeatureDefinition::Operation(FeatureOperation::Revolve { .. }) => Some(Self::Revolve),
            _ => None,
        }
    }
}

struct ProjectionIndex<'a> {
    result_model: Option<&'a cadmpeg_ir::document::Model>,
    edge_names: HashMap<(String, u32), Option<cadmpeg_ir::features::GeneratedEdgeRef>>,
    dimensions: HashMap<(&'a str, u32), Option<&'a revolve::Dimension>>,
    sketch_entities: HashMap<(&'a str, u32), Option<&'a crate::sketch::PmDcSketchEntity>>,
    properties: HashMap<(&'a str, u32), Option<&'a PmDcFeatureProperty>>,
    parameters: HashMap<(&'a str, u32), Option<&'a crate::design::PmDcParameter>>,
    parameter_values: HashMap<&'a str, Option<&'a DesignParameter>>,
    sketches: HashMap<(&'a str, u32), Option<&'a crate::sketch::PmDcSketch>>,
    sketch_ids: HashMap<&'a str, cadmpeg_ir::sketches::SketchId>,
    sketches_by_id: HashMap<&'a str, &'a Sketch>,
    directions: HashMap<(&'a str, u32), Option<&'a crate::sketch::PmDcDirection>>,
    transforms: HashMap<(&'a str, u32), Option<&'a crate::sketch::PmDcTransform>>,
    entity_style_links: HashSet<(&'a str, u32)>,
    work_planes: HashMap<(&'a str, u32), Option<&'a PmDcWorkPlane>>,
    profiles: profile::ProfileIndex<'a>,
    /// Profile sketch ordinal of each sheet-metal plate, from the face label
    /// whose participants are that sketch and that plate.
    plate_sketches: HashMap<(&'a str, u32), u32>,
}

/// The projected sketch model that features consume.
pub(crate) struct ProjectedSketches<'a> {
    pub(crate) sketches: &'a [Sketch],
    pub(crate) entities: &'a [cadmpeg_ir::sketches::SketchEntity],
    /// The decoded profile bodies that extrusion profiles resolve through.
    pub(crate) profile_bodies: &'a [profile::ProfileBody],
    pub(crate) result_model: Option<&'a cadmpeg_ir::document::Model>,
}

pub(crate) fn project(
    ctx: &DecodeContext<'_>,
    inventory: &FeatureInventory,
    design: &DesignInventory,
    sketch: &SketchInventory,
    parameters: &[DesignParameter],
    projected: &ProjectedSketches<'_>,
) -> Result<FeatureProjection, CodecError> {
    let ProjectedSketches {
        sketches,
        entities: sketch_entities,
        profile_bodies,
        result_model,
    } = *projected;
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
            input_topologies: Vec::new(),
            unresolved_features: total,
            unresolved_states: 0,
            revolve_losses: Vec::new(),
            added_profiles: HashMap::new(),
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
    let mut index = ProjectionIndex {
        result_model,
        edge_names: HashMap::new(),
        sketch_entities: if inventory.properties.iter().any(|property| {
            matches!(
                property.kind,
                PmDcFeaturePropertyKind::References {
                    family: PmDcFeatureReferenceFamily::HoleCenters,
                    ..
                }
            )
        }) {
            unique_index(
                sketch
                    .entities
                    .iter()
                    .filter(|record| {
                        matches!(
                            record.kind,
                            crate::sketch::PmDcSketchEntityKind::Point { .. }
                        )
                    })
                    .map(|record| {
                        (
                            (
                                record.identity.segment_token.as_str(),
                                record.identity.record_ordinal,
                            ),
                            record,
                        )
                    }),
            )
        } else {
            HashMap::new()
        },
        dimensions: unique_index(inventory.dimensions.iter().map(|record| {
            (
                (
                    record.identity.segment_token.as_str(),
                    record.identity.record_ordinal,
                ),
                record,
            )
        })),
        properties: unique_properties,
        parameters: unique_parameters,
        parameter_values: unique_index(
            parameters
                .iter()
                .filter_map(|parameter| Some((parameter.native_ref.as_deref()?, parameter))),
        ),
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
        sketches_by_id: sketches
            .iter()
            .map(|sketch| (sketch.id.as_str(), sketch))
            .collect(),
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
        profiles: profile::ProfileIndex::new(
            profile_bodies,
            &inventory.kernel_entity_links,
            &inventory.entity_style_links,
            &sketch.entities,
            sketch_entities,
        ),
        plate_sketches: plate_sketches(inventory, sketch),
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
    index.edge_names = edge_selection::generated_names(inventory, &history, sketch);
    let label_of = |ordinal: u32| {
        token.and_then(|token| {
            labels
                .get(&(token, Some(ordinal)))
                .and_then(Option::as_ref)
                .copied()
        })
    };
    let mut projected = Vec::new();
    let mut revolve_losses = Vec::new();
    let mut neutral = 0_usize;
    for feature in &inventory.features {
        let label = label_of(feature.identity.record_ordinal);
        let value = match label.and_then(|label| {
            FeatureFamily::from_class_id(label.class_id()).map(|family| (label, family))
        }) {
            Some((label, family)) => match family {
                FeatureFamily::Base => None,
                FeatureFamily::Extrusion => project_extrusion(ctx, feature, label, &index),
                FeatureFamily::Fillet => project_fillet(ctx, feature, label, &index),
                FeatureFamily::Chamfer => project_chamfer(ctx, feature, label, &index),
                FeatureFamily::Hole => project_hole(ctx, feature, label, &index),
                FeatureFamily::Plate => project_plate(ctx, feature, label, &index).or_else(|| {
                    sheet_metal::projection::project_plate(ctx, feature, label, &index, inventory)
                }),
                FeatureFamily::Bend => None,
                FeatureFamily::Thicken => project_thicken(ctx, feature, label, &index, inventory),
                FeatureFamily::Shell => project_shell(ctx, feature, label, &index),
                FeatureFamily::Sweep | FeatureFamily::Loft => {
                    loft_sweep::projection::project(ctx, feature, label, &index, family)
                }
                FeatureFamily::Revolve => {
                    project_revolve(ctx, feature, label, &index, &mut revolve_losses)
                }
            }
            .map(|result| result.map(|(feature, result)| (feature, Some(result))))
            .transpose()?,
            None => None,
        };
        let value = match value {
            Some(value) => Some(value),
            None => label
                .filter(|label| label.class_id() == THREAD_CLASS_ID)
                .and_then(|label| project_thread(ctx, feature, label, &index))
                .transpose()?
                .map(|feature| (feature, None)),
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
                    result,
                    neutral: true,
                });
            }
            None if history.placement(feature.identity.record_ordinal).is_some() => {
                if label.is_some_and(|label| label.class_id() == REVOLVE_CLASS_ID) {
                    revolve_losses.push(
                        revolve::indexed_operands(feature, &index)
                            .err()
                            .unwrap_or(revolve::NativeReason::ProfileOrResult)
                            .note(),
                    );
                }
                projected.push(HistoryItem {
                    record: feature.identity.record_ordinal,
                    kind: history::HistoryNodeKind::Feature,
                    flags: feature.header.flags,
                    label,
                    feature: native_feature(ctx, &feature.identity, label, feature.id())?,
                    result: None,
                    neutral: false,
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
            neutral: false,
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
            neutral: false,
        });
    }
    apply_history(&mut projected, &history, inventory, token)?;
    let unresolved_states = projected
        .iter()
        .filter(|item| item.neutral && history.placement(item.record).is_none())
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
        if !keep && item.neutral {
            neutral -= 1;
        }
        keep
    });
    neutral -= sheet_metal::projection::reject_unresolved_inputs(ctx, &mut projected, inventory)?;
    neutral += sheet_metal::projection::project_bends(ctx, &mut projected, &index, inventory)?;
    projected.sort_unstable_by_key(|item| item.feature.ordinal);
    let mut features = Vec::new();
    let mut result_topologies = Vec::new();
    for item in projected {
        features.push(item.feature);
        if let Some(result) = item.result {
            result_topologies.push(result);
        }
    }
    edge_selection::bind_generated(&mut features, &mut result_topologies)?;
    let input_topologies = sheet_metal::inputs::project(ctx, &mut features, inventory)?;
    let unresolved_chamfers = features
        .iter()
        .filter(|feature| {
            matches!(feature.evaluation.definition(),
                FeatureDefinition::Operation(FeatureOperation::Chamfer { groups, .. })
                    if groups.iter().any(|group| group.spec.is_unresolved()))
        })
        .count();
    Ok(FeatureProjection {
        unresolved_features: total
            .checked_sub(neutral)
            .and_then(|unresolved| unresolved.checked_add(unresolved_chamfers))
            .ok_or_else(|| {
                CodecError::malformed("Inventor feature projection exceeds inventory")
            })?,
        unresolved_states: unresolved_states.min(neutral),
        revolve_losses,
        features,
        result_topologies,
        input_topologies,
        added_profiles: index.profiles.into_added_profiles(),
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
    neutral: bool,
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
            FeatureFamily::Base => NativeFeatureKind::Other("Base".into()),
            FeatureFamily::Extrusion => NativeFeatureKind::Extrude,
            FeatureFamily::Fillet => NativeFeatureKind::Fillet,
            FeatureFamily::Chamfer => NativeFeatureKind::Chamfer,
            FeatureFamily::Hole => NativeFeatureKind::Other("Hole".into()),
            FeatureFamily::Plate => NativeFeatureKind::Other("Plate".into()),
            FeatureFamily::Bend => NativeFeatureKind::Other("SheetMetalBend".into()),
            FeatureFamily::Thicken => NativeFeatureKind::Other("Thicken".into()),
            FeatureFamily::Shell => NativeFeatureKind::Other("Shell".into()),
            FeatureFamily::Sweep => NativeFeatureKind::Other("Sweep".into()),
            FeatureFamily::Loft => NativeFeatureKind::Other("Loft".into()),
            FeatureFamily::Revolve => NativeFeatureKind::Other("Revolve".into()),
        };
    }
    if let Some(family) = loft_sweep::slots::Family::from_class_id(class_id) {
        return NativeFeatureKind::Other(family.native_name().into());
    }
    NATIVE_CLASSES
        .iter()
        .find(|(bytes, _)| *bytes == class_id.0.into_inner())
        .map_or_else(
            || NativeFeatureKind::Other(format!("InventorClass:{class_id}")),
            |(_, name)| NativeFeatureKind::Other((*name).to_owned()),
        )
}

/// Admit Thread annotations with nine typed slots and a closed, nonempty face selection.
/// Edge members must resolve to edge items; both numeric parameters must be decoded.
pub(crate) fn thread_face_collection<'a>(
    source: &PmDcFeature,
    property: impl Fn(u32) -> Option<&'a PmDcFeatureProperty>,
    parameter: impl Fn(u32) -> bool,
) -> Option<&'a PmDcFeatureProperty> {
    let slots = source.properties.references();
    if source.properties.marker() != 2 || slots.len() != 9 {
        return None;
    }
    let faces = property(slots[0].index())?;
    let PmDcFeaturePropertyKind::References {
        family: PmDcFeatureReferenceFamily::FaceCollection,
        items,
    } = &faces.kind
    else {
        return None;
    };
    if items.marker() != 2
        || items.references().is_empty()
        || !items.references().iter().all(|reference| {
            property(reference.index()).is_some_and(|item| {
                matches!(
                    &item.kind,
                    PmDcFeaturePropertyKind::FaceItem { index_references }
                        if index_references.marker() == 2 && !index_references.values().is_empty()
                )
            })
        })
        || [1, 2, 6].into_iter().any(|slot| {
            !property(slots[slot].index())
                .is_some_and(|value| matches!(value.kind, PmDcFeaturePropertyKind::Boolean { .. }))
        })
        || [3, 4]
            .into_iter()
            .any(|slot| !parameter(slots[slot].index()))
    {
        return None;
    }
    for (slot, family) in [
        (5, PmDcFeatureReferenceFamily::EdgeCollection),
        (8, PmDcFeatureReferenceFamily::FeatureDimensions),
    ] {
        let PmDcFeaturePropertyKind::References {
            family: actual,
            items,
        } = &property(slots[slot].index())?.kind
        else {
            return None;
        };
        if *actual != family || items.marker() != 2 {
            return None;
        }
        if family == PmDcFeatureReferenceFamily::EdgeCollection
            && !items.references().iter().all(|reference| {
                property(reference.index()).is_some_and(|item| {
                    matches!(&item.kind, PmDcFeaturePropertyKind::EdgeItem { .. })
                })
            })
        {
            return None;
        }
    }
    Some(faces)
}

fn project_thread(
    ctx: &DecodeContext<'_>,
    source: &PmDcFeature,
    label: &PmDcFeatureLabel,
    index: &ProjectionIndex<'_>,
) -> Option<Result<Feature, CodecError>> {
    let token = source.identity.segment_token.as_str();
    let faces = thread_face_collection(
        source,
        |reference| resolve_property(token, reference, index),
        |reference| {
            reference
                .checked_sub(1)
                .and_then(|ordinal| index.parameters.get(&(token, ordinal)))
                .is_some_and(Option::is_some)
        },
    )?;
    if let Err(error) = ctx.charge_entities(1, "project Inventor thread annotation") {
        return Some(Err(error));
    }
    let key = match source.identity.key() {
        Ok(key) => key,
        Err(error) => return Some(Err(error)),
    };
    Some(Ok(Feature {
        id: FeatureId::compose(
            &cadmpeg_ir::identity_namespace!("inventor", "design", "feature"),
            key,
        ),
        ordinal: u64::from(label.index),
        name: label.name().map(str::to_owned),
        suppressed: None,
        dependencies: DistinctMembers::default(),
        source_properties: boolean_properties(source, &[1, 2, 6], index),
        source_tag: Some("thread".into()),
        source_text: None,
        source_content: FeatureContent::default(),
        evaluation: cadmpeg_ir::features::FeatureEvaluation::from_definition(
            FeatureDefinition::Operation(FeatureOperation::CosmeticThread {
                face: cadmpeg_ir::features::FaceSelection::Native(faces.id()),
                diameter: None,
                extent: None,
            }),
        ),
        native_ref: Some(source.id()),
    }))
}

/// The profile selections of a boundary patch and the one planar sketch that
/// the feature label names.
struct BoundaryProfile<'a> {
    selections: Vec<String>,
    links: Vec<u32>,
    sketch: &'a crate::sketch::PmDcSketch,
    sketch_id: cadmpeg_ir::sketches::SketchId,
}

impl<'a> BoundaryProfile<'a> {
    fn new(
        source: &PmDcFeature,
        label: &PmDcFeatureLabel,
        slot: usize,
        index: &ProjectionIndex<'a>,
    ) -> Option<Self> {
        let boundary = references(
            source,
            slot,
            PmDcFeatureReferenceFamily::BoundaryPatch,
            index,
        )?;
        let mut distinct = HashSet::new();
        if !boundary
            .references()
            .iter()
            .all(|selection| distinct.insert(selection.index()))
        {
            return None;
        }
        let mut selections = Vec::new();
        let mut links = Vec::new();
        for reference in boundary.references() {
            let property = resolve_property(
                source.identity.segment_token.as_str(),
                reference.index(),
                index,
            )?;
            let PmDcFeaturePropertyKind::ProfileSelection { entity_link, .. } = &property.kind
            else {
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
            links.push(ordinal);
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
            .and_then(Option::as_ref)
            .copied()?;
        let sketch_id = index.sketch_ids.get(sketch.id().as_str())?.clone();
        Some(Self {
            selections,
            links,
            sketch,
            sketch_id,
        })
    }

    /// The selected sketch regions, or the native selection when they do not
    /// resolve. Resolution can append loops to the sketch profile table, so a
    /// projection resolves its profile after every other check.
    fn resolve(self, index: &ProjectionIndex<'_>) -> Option<PlanarProfileRef> {
        let regions = index
            .sketches_by_id
            .get(self.sketch_id.as_str())
            .and_then(|neutral| {
                index.profiles.regions(
                    self.sketch.identity.segment_token.as_str(),
                    self.sketch.identity.record_ordinal,
                    neutral,
                    &self.links,
                )
            });
        match regions {
            Some(profile) => Some(profile),
            None => PlanarProfileRef::sketch_selection(self.sketch_id, self.selections).ok(),
        }
    }
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
    if source.properties.references().get(23)? != source.properties.references().get(1)? {
        return None;
    }
    let boundary = BoundaryProfile::new(source, label, 1, index)?;

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
        let (plane, native) = extent_target_plane(source, index, 8, 10, 17)?;
        ExtrudeStart::FromPlane { plane, native }
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
    let profile = boundary.resolve(index)?;
    let feature = Feature {
        id: feature_id,
        ordinal: u64::from(label.index),
        name: label.name().map(str::to_owned),
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

fn project_revolve(
    ctx: &DecodeContext<'_>,
    source: &PmDcFeature,
    label: &PmDcFeatureLabel,
    index: &ProjectionIndex<'_>,
    losses: &mut Vec<cadmpeg_ir::report::loss::LossNote>,
) -> Option<Result<(Feature, FeatureResultTopology), CodecError>> {
    let revolve::Operands {
        axis,
        extent,
        op,
        unresolved_control,
    } = revolve::indexed_operands(source, index).ok()?;
    let boundary = BoundaryProfile::new(source, label, 1, index)?;
    let (feature_id, result) = match feature_result(ctx, source, 17, index)? {
        Ok(value) => value,
        Err(error) => return Some(Err(error)),
    };
    if let Err(error) = ctx.charge_entities(1, "project Inventor feature") {
        return Some(Err(error));
    }
    let profile = boundary.resolve(index)?;
    let feature = Feature {
        id: feature_id,
        ordinal: u64::from(label.index),
        name: label.name().map(str::to_owned),
        suppressed: None,
        dependencies: DistinctMembers::default(),
        source_properties: BTreeMap::new(),
        source_tag: Some("revolve".into()),
        source_text: None,
        source_content: FeatureContent::default(),
        evaluation: cadmpeg_ir::features::FeatureEvaluation::from_definition(
            FeatureDefinition::Operation(FeatureOperation::Revolve {
                construction: RevolveConstruction::Resolved {
                    profile,
                    axis,
                    extent,
                    solid: Some(true),
                    face_maker: None,
                    fuse_order: None,
                    allow_multi_profile_faces: None,
                },
                op,
            }),
        ),
        native_ref: Some(source.id()),
    };
    if unresolved_control {
        losses.push(crate::loss::InventorLossCode::FeatureRevolveControlUnresolved.note(
            "Projected Inventor revolve with its true slot-16 control retained natively with unresolved meaning.",
        ));
    }
    Some(Ok((feature, result)))
}

/// The profile sketch of each plate: a face label's participants are one
/// planar sketch and one plate feature record.
fn plate_sketches<'a>(
    inventory: &'a FeatureInventory,
    sketch: &'a SketchInventory,
) -> HashMap<(&'a str, u32), u32> {
    let sketches = sketch
        .sketches
        .iter()
        .map(|record| {
            (
                record.identity.segment_token.as_str(),
                record.identity.record_ordinal,
            )
        })
        .collect::<HashSet<_>>();
    let features = inventory
        .features
        .iter()
        .map(|record| {
            (
                record.identity.segment_token.as_str(),
                record.identity.record_ordinal,
            )
        })
        .collect::<HashSet<_>>();
    let mut plates = sheet_metal::projection::profiles(inventory, &sketches);
    for label in &inventory.labels {
        let token = label.identity.segment_token.as_str();
        let [first, second] = label.participants.references() else {
            continue;
        };
        let (Some(sketch_ordinal), Some(plate_ordinal)) =
            (first.index().checked_sub(1), second.index().checked_sub(1))
        else {
            continue;
        };
        if sketches.contains(&(token, sketch_ordinal)) && features.contains(&(token, plate_ordinal))
        {
            plates.insert((token, plate_ordinal), sketch_ordinal);
        }
    }
    plates
}

/// Plate slots that hold a bend to an existing face or relief options.
const PLATE_BEND_SLOTS: [usize; 6] = [4, 9, 10, 11, 12, 13];

/// A sheet-metal plate without a bend: the profile of its face sketch,
/// thickened by the slot-3 parameter from the sketch plane along the sketch
/// normal, or against it when the slot-2 Boolean is true.
fn project_plate(
    ctx: &DecodeContext<'_>,
    source: &PmDcFeature,
    label: &PmDcFeatureLabel,
    index: &ProjectionIndex<'_>,
) -> Option<Result<(Feature, FeatureResultTopology), CodecError>> {
    let references = source.properties.references();
    if PLATE_BEND_SLOTS.iter().any(|slot| {
        references
            .get(*slot)
            .is_some_and(|reference| reference.index() != 0)
    }) {
        return None;
    }
    let (profile, normal) = plate_profile(source, index)?;
    let direction_record = resolve_direction(source, 1, index)?;
    let stored = Vector3::new(
        direction_record.direction[0].get(),
        direction_record.direction[1].get(),
        direction_record.direction[2].get(),
    );
    let direction = cadmpeg_ir::units::UnitVector3::normalized(stored)?;
    if direction.as_raw().dot(*normal.as_raw()) < 1.0 - EPS_FEATURE_PROJECT_HOLE_E10 {
        return None;
    }
    let side = plate_side(source, index)?;
    let thickness =
        cadmpeg_ir::scalar::PositiveLength::try_from(length_parameter(source, 3, index)?).ok()?;
    let result = plate_result(ctx, source, index)?;
    let (feature_id, result) = match result {
        Ok(value) => value,
        Err(error) => return Some(Err(error)),
    };
    if let Err(error) = ctx.charge_entities(1, "project Inventor feature") {
        return Some(Err(error));
    }
    let feature = Feature {
        id: feature_id,
        ordinal: u64::from(label.index),
        name: label.name().map(str::to_owned),
        suppressed: None,
        dependencies: DistinctMembers::default(),
        source_properties: BTreeMap::new(),
        source_tag: Some("sheet_metal_face".into()),
        source_text: None,
        source_content: FeatureContent::default(),
        evaluation: cadmpeg_ir::features::FeatureEvaluation::from_definition(
            FeatureDefinition::Operation(FeatureOperation::SheetMetalBaseFlange {
                profile,
                thickness,
                side,
            }),
        ),
        native_ref: Some(source.id()),
    };
    Some(Ok((feature, result)))
}

fn plate_profile(
    source: &PmDcFeature,
    index: &ProjectionIndex<'_>,
) -> Option<(
    cadmpeg_ir::features::PlanarProfileRef,
    cadmpeg_ir::features::FiniteVector3,
)> {
    let token = source.identity.segment_token.as_str();
    let selection = slot_property(source, 0, index)?;
    let PmDcFeaturePropertyKind::ProfileSelection { entity_link, .. } = &selection.kind else {
        return None;
    };
    let link = entity_link.index().checked_sub(1)?;
    let sketch_ordinal = *index
        .plate_sketches
        .get(&(token, source.identity.record_ordinal))?;
    let sketch = index
        .sketches
        .get(&(token, sketch_ordinal))
        .and_then(Option::as_ref)?;
    let sketch_id = index.sketch_ids.get(sketch.id().as_str())?.clone();
    let neutral_sketch = index.sketches_by_id.get(sketch_id.as_str())?;
    let profile = index
        .profiles
        .regions(token, sketch_ordinal, neutral_sketch, &[link])?;
    let (_, normal, _) = neutral_sketch.resolved_placement()?;
    Some((profile, normal))
}

fn plate_side(
    source: &PmDcFeature,
    index: &ProjectionIndex<'_>,
) -> Option<cadmpeg_ir::features::SheetMetalThicknessSide> {
    Some(match slot_property(source, 2, index)?.kind {
        PmDcFeaturePropertyKind::Boolean { value, .. } => {
            if value {
                cadmpeg_ir::features::SheetMetalThicknessSide::Reverse
            } else {
                cadmpeg_ir::features::SheetMetalThicknessSide::Forward
            }
        }
        PmDcFeaturePropertyKind::Enumeration {
            family: PmDcFeatureEnumFamily::PlateSide,
            type_value: 2,
            value: 0,
        } => cadmpeg_ir::features::SheetMetalThicknessSide::Forward,
        _ => return None,
    })
}

fn plate_result(
    ctx: &DecodeContext<'_>,
    source: &PmDcFeature,
    index: &ProjectionIndex<'_>,
) -> Option<Result<(FeatureId, FeatureResultTopology), CodecError>> {
    let output_slot = FeatureFamily::Plate.output_slot(source);
    if output_slot == 15 {
        if source.properties.references().get(6)?.index() != 0
            || enum16(source, 16, PmDcFeatureEnumFamily::PartOperation, index)? != 1
        {
            return None;
        }
        feature_result(ctx, source, output_slot, index)
    } else {
        if [15, 16].iter().any(|slot| {
            source
                .properties
                .references()
                .get(*slot)
                .is_some_and(|reference| reference.index() != 0)
        }) {
            return None;
        }
        body_result(ctx, source, output_slot, index)
    }
}

/// The result of a feature whose output slot holds one surface-body record.
fn body_result(
    ctx: &DecodeContext<'_>,
    source: &PmDcFeature,
    slot: usize,
    index: &ProjectionIndex<'_>,
) -> Option<Result<(FeatureId, FeatureResultTopology), CodecError>> {
    let body = slot_property(source, slot, index)?;
    if !matches!(body.kind, PmDcFeaturePropertyKind::SurfaceBody { .. }) {
        return None;
    }
    let members = cadmpeg_ir::features::FeatureResultMembers::new(
        vec![cadmpeg_core::text::NonBlankString::new(body.id())?],
        Vec::new(),
        Vec::new(),
        Vec::new(),
    )
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
        Some(body.id()),
    );
    Some(Ok((feature_id, result)))
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
            length: cadmpeg_ir::scalar::NonZeroLength::from(
                cadmpeg_ir::scalar::PositiveLength::try_from(length_parameter(
                    source,
                    slots.length,
                    index,
                )?)
                .ok()?,
            ),
        },
        EXTENT_THROUGH_NEXT => LinearTermination::ThroughNext {},
        EXTENT_THROUGH_ALL => LinearTermination::ThroughAll {},
        EXTENT_FROM_TO | EXTENT_TO => {
            let (plane, native) = extent_target_plane(source, index, 11, 13, 18)?;
            LinearTermination::ToPlane { plane, native }
        }
        _ => return None,
    };
    Some(ExtrudeSide {
        termination,
        draft: (taper.get() != 0.0).then_some(taper),
    })
}

/// Resolve an extent target: a work plane, or a model face recorded as a
/// termination flag with value 1 and its plane surface. Both store a plane
/// in centimetres and no face identity.
fn extent_target_plane(
    source: &PmDcFeature,
    index: &ProjectionIndex<'_>,
    plane_slot: usize,
    flag_slot: usize,
    surface_slot: usize,
) -> Option<(
    cadmpeg_ir::features::FeatureSupportPlaneFrame,
    cadmpeg_core::text::NonBlankString,
)> {
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
        return target_plane(plane.origin, plane.u_axis, plane.v_axis, plane.id());
    }
    if !matches!(
        resolve_property(token, flag, index)?.kind,
        PmDcFeaturePropertyKind::TerminationFlag { value: 1 }
    ) {
        return None;
    }
    let surface = resolve_property(token, surface, index)?;
    let PmDcFeaturePropertyKind::PlaneSurface {
        origin,
        u_axis,
        v_axis,
        ..
    } = surface.kind
    else {
        return None;
    };
    target_plane(origin, u_axis, v_axis, surface.id())
}

/// Build a model-space plane frame from an origin in centimetres and two
/// in-plane axes. The normal is the cross product of the axes.
fn target_plane(
    origin: [cadmpeg_ir::scalar::FiniteReal; 3],
    u_axis: [cadmpeg_ir::scalar::FiniteReal; 3],
    v_axis: [cadmpeg_ir::scalar::FiniteReal; 3],
    native: String,
) -> Option<(
    cadmpeg_ir::features::FeatureSupportPlaneFrame,
    cadmpeg_core::text::NonBlankString,
)> {
    let vector = |value: [cadmpeg_ir::scalar::FiniteReal; 3]| {
        Vector3::new(value[0].get(), value[1].get(), value[2].get())
    };
    let u_axis = vector(u_axis);
    let plane = cadmpeg_ir::features::FeatureSupportPlaneFrame::new(
        Point3::new(
            origin[0].get() * 10.0,
            origin[1].get() * 10.0,
            origin[2].get() * 10.0,
        ),
        u_axis.cross(vector(v_axis)),
        u_axis,
    )?;
    Some((plane, cadmpeg_core::text::NonBlankString::new(native)?))
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
            ..
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
                type_value: 4 | 0x0002_0004,
                value: 0
            }
        ) {
            return None;
        }
        if continuity.index() == 0 {
            if label.class_id() != CORNER_ROUND_CLASS_ID {
                return None;
            }
        } else if !matches!(
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
        let radius = cadmpeg_ir::scalar::PositiveLength::new(
            length_reference(
                source.identity.segment_token.as_str(),
                radius.index(),
                index,
            )?
            .get(),
        )?;
        groups.push(FilletGroup {
            edges: match selection.kind {
                PmDcFeaturePropertyKind::WideEnumeration { type_value: 4, .. } => {
                    edge_selection::resolve(edge_collection, index)
                }
                _ => EdgeSelection::Native(set.id()),
            },
            radius: RadiusSpec::Constant { radius },
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
    let form = enum16(source, 4, PmDcFeatureEnumFamily::Chamfer, index)?;
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
    let distance =
        cadmpeg_ir::scalar::PositiveLength::try_from(length_parameter(source, 2, index)?).ok()?;
    let spec = match form {
        0 => ChamferSpec::Distance { distance },
        1 | 2 => {
            let faces = references(source, 1, PmDcFeatureReferenceFamily::FaceCollection, index)?;
            if faces.references().is_empty()
                || !faces.references().iter().all(|reference| {
                    resolve_property(
                        source.identity.segment_token.as_str(),
                        reference.index(),
                        index,
                    )
                    .is_some_and(|property| {
                        matches!(&property.kind, PmDcFeaturePropertyKind::FaceItem {
                            index_references, ..
                        } if !index_references.values().is_empty())
                    })
                })
            {
                return None;
            }
            if form == 1 {
                cadmpeg_ir::scalar::InteriorAngle::try_from(angle_parameter(source, 10, index)?)
                    .ok()?;
                ChamferSpec::Unresolved {
                    form: Some(ChamferForm::DistanceAngle),
                }
            } else {
                let second = cadmpeg_ir::scalar::PositiveLength::try_from(length_parameter(
                    source, 3, index,
                )?)
                .ok()?;
                if distance == second {
                    ChamferSpec::TwoDistances {
                        first: distance,
                        second,
                    }
                } else {
                    ChamferSpec::Unresolved {
                        form: Some(ChamferForm::TwoDistances),
                    }
                }
            }
        }
        _ => return None,
    };
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
            dependencies: DistinctMembers::default(),
            source_properties,
            source_tag: Some("chamfer".into()),
            source_text: None,
            source_content: FeatureContent::default(),

            evaluation: cadmpeg_ir::features::FeatureEvaluation::from_definition(
                FeatureDefinition::Operation(FeatureOperation::Chamfer {
                    groups: cadmpeg_ir::features::NonEmptyMembers::one(ChamferGroup {
                        edges: if form == 0 {
                            edge_selection::resolve(edges, index)
                        } else {
                            EdgeSelection::Native(source.id())
                        },
                        spec,
                    }),
                    flip_direction,
                }),
            ),
            native_ref: Some(source.id()),
        },
        result,
    )))
}

/// Project the ten-slot Shell branch with an empty selection list in slot 2 and null slot 4.
/// Require distinct non-null removed faces, a positive length, and a resolved body result.
fn project_shell(
    ctx: &DecodeContext<'_>,
    source: &PmDcFeature,
    label: &PmDcFeatureLabel,
    index: &ProjectionIndex<'_>,
) -> Option<Result<(Feature, FeatureResultTopology), CodecError>> {
    let slots = source.properties.references();
    if slots.len() != 10 || slots.get(4)?.index() != 0 {
        return None;
    }
    let faces = slot_property(source, 1, index)?;
    let PmDcFeaturePropertyKind::References {
        family: PmDcFeatureReferenceFamily::FaceCollection,
        items,
    } = &faces.kind
    else {
        return None;
    };
    let mut distinct = HashSet::new();
    if !items
        .references()
        .iter()
        .all(|reference| reference.index() != 0 && distinct.insert(reference.index()))
        || !references(source, 2, PmDcFeatureReferenceFamily::SelectionList, index)?
            .references()
            .is_empty()
    {
        return None;
    }
    let thickness =
        cadmpeg_ir::scalar::PositiveLength::try_from(length_parameter(source, 3, index)?).ok()?;
    let removed_faces = if items.references().is_empty() {
        cadmpeg_ir::features::FaceSelection::Faces(Vec::new())
    } else {
        cadmpeg_ir::features::FaceSelection::Native(faces.id())
    };
    let (feature_id, result) = match feature_result(ctx, source, 9, index)? {
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
            dependencies: DistinctMembers::default(),
            source_properties: BTreeMap::new(),
            source_tag: Some("shell".into()),
            source_text: None,
            source_content: FeatureContent::default(),
            evaluation: cadmpeg_ir::features::FeatureEvaluation::from_definition(
                FeatureDefinition::Operation(FeatureOperation::Shell {
                    bodies: None,
                    removed_faces,
                    thickness: Some(thickness),
                    outward: None,
                    mode: None,
                    join: None,
                    resolve_intersections: None,
                    allow_self_intersections: None,
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
            drill_point_angle: cadmpeg_ir::scalar::InteriorAngle::try_from(point_angle).ok()?,
        },
        1 => HoleKind::Countersink {
            diameter: cadmpeg_ir::scalar::PositiveLength::try_from(head_diameter).ok()?,
            angle: cadmpeg_ir::scalar::InteriorAngle::try_from(head_angle).ok()?,
        },
        2 | 3 if point_angle.get() == 0.0 => HoleKind::Counterbore {
            diameter: cadmpeg_ir::scalar::PositiveLength::try_from(head_diameter).ok()?,
            depth: cadmpeg_ir::scalar::PositiveLength::try_from(head_depth).ok()?,
        },
        2 | 3 => HoleKind::CounterboreDrilled {
            diameter: cadmpeg_ir::scalar::PositiveLength::try_from(head_diameter).ok()?,
            depth: cadmpeg_ir::scalar::PositiveLength::try_from(head_depth).ok()?,
            drill_point_angle: cadmpeg_ir::scalar::InteriorAngle::try_from(point_angle).ok()?,
        },
        _ => return None,
    };
    let extent = hole::termination(source, index, depth)?;
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
    let placements = hole::placements(source, label, index, transform, direction)?;
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
    if let Err(error) = ctx.charge_entities(1, "project Inventor feature") {
        return Some(Err(error));
    }
    Some(Ok((
        Feature {
            id: feature_id,
            ordinal: u64::from(label.index),
            name: label.name().map(str::to_owned),
            suppressed: None,
            dependencies: DistinctMembers::default(),
            source_properties: if hole_form == 3 {
                BTreeMap::from([(
                    cadmpeg_core::nonblank_literal!("inventor.hole_form"),
                    "spotface".into(),
                )])
            } else {
                BTreeMap::new()
            },
            source_tag: Some("hole".into()),
            source_text: None,
            source_content: FeatureContent::default(),

            evaluation: cadmpeg_ir::features::FeatureEvaluation::from_definition(
                FeatureDefinition::Operation(FeatureOperation::Hole {
                    profile: None,
                    profile_filter: None,
                    face: None,
                    direction: None,
                    placements: Some(placements),
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

/// Admit the join form with Boolean values `1, 0, 0, 0, 0, 1, 0` in slots
/// 2, 3, 4, 7, 10, 16, and 17, and a positive thickness in slot 6.
pub(crate) fn thicken_operation<'a>(
    source: &PmDcFeature,
    property_at: impl Fn(usize) -> Option<&'a PmDcFeatureProperty>,
    parameter_at: impl Fn(usize) -> Option<(&'a crate::design::PmDcParameter, &'a ParameterValue)>,
    record_type_at: impl Fn(u32) -> Option<&'a str>,
) -> Option<FeatureOperation> {
    if source.properties.references().len() != 18
        || [5, 8, 11]
            .into_iter()
            .any(|slot| source.properties.references()[slot].index() != 0)
        || !matches!(
            property_at(1)?.kind,
            PmDcFeaturePropertyKind::Enumeration {
                family: PmDcFeatureEnumFamily::PartOperation,
                type_value: 5,
                value: 3,
            }
        )
    {
        return None;
    }
    for (slot, expected) in [
        (2, true),
        (3, false),
        (4, false),
        (7, false),
        (10, false),
        (16, true),
        (17, false),
    ] {
        if !matches!(
            property_at(slot)?.kind,
            PmDcFeaturePropertyKind::Boolean { value, .. } if value == expected
        ) {
            return None;
        }
    }
    if !matches!(
        property_at(9)?.kind,
        PmDcFeaturePropertyKind::References {
            family: PmDcFeatureReferenceFamily::FeatureDimensions,
            ..
        }
    ) {
        return None;
    }
    for (slot, expected) in [
        (13, "4c40b0a3ac4b9504e719b28a70e0e818"),
        (14, "c11c7b63ea4d840b68b76e8c67bcbc48"),
    ] {
        if record_type_at(source.properties.references()[slot].record_ordinal()?)? != expected {
            return None;
        }
    }
    let (raw_thickness, value) = parameter_at(6)?;
    let ParameterValue::Length(thickness) = value else {
        return None;
    };
    if Length::new(raw_thickness.model_value.get() * 10.0)? != *thickness {
        return None;
    }
    let (raw_control, control) = parameter_at(12)?;
    if !matches!(control, ParameterValue::Real(value) if *value == raw_control.model_value) {
        return None;
    }
    let selections = property_at(0)?;
    let PmDcFeaturePropertyKind::References {
        family: PmDcFeatureReferenceFamily::SelectionList,
        items,
    } = &selections.kind
    else {
        return None;
    };
    let mut distinct = HashSet::new();
    if items.references().is_empty()
        || items.references().iter().any(|reference| {
            reference
                .record_ordinal()
                .and_then(&record_type_at)
                .is_none()
                || !distinct.insert(reference.index())
        })
    {
        return None;
    }
    Some(FeatureOperation::Thicken {
        faces: FaceSelection::Native(selections.id()),
        thickness: Some(cadmpeg_ir::scalar::PositiveLength::try_from(*thickness).ok()?),
        side: None,
    })
}

fn project_thicken(
    ctx: &DecodeContext<'_>,
    source: &PmDcFeature,
    label: &PmDcFeatureLabel,
    index: &ProjectionIndex<'_>,
    inventory: &FeatureInventory,
) -> Option<Result<(Feature, FeatureResultTopology), CodecError>> {
    let operation = thicken_operation(
        source,
        |slot| slot_property(source, slot, index),
        |slot| {
            let ordinal = source.properties.references().get(slot)?.record_ordinal()?;
            let parameter = index
                .parameters
                .get(&(source.identity.segment_token.as_str(), ordinal))?
                .as_ref()?;
            let value = index
                .parameter_values
                .get(parameter.id().as_str())?
                .as_ref()?
                .value
                .as_ref()?;
            Some((*parameter, value))
        },
        |ordinal| {
            inventory
                .record_types
                .get(&source.identity.segment_token)?
                .get(&ordinal)?
                .as_deref()
        },
    )?;
    let (feature_id, result) = match feature_result(ctx, source, 15, index)? {
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
            dependencies: DistinctMembers::default(),
            source_properties: boolean_properties(source, &[2, 3, 4, 7, 10, 16, 17], index),
            source_tag: Some("thicken".into()),
            source_text: None,
            source_content: FeatureContent::default(),
            evaluation: cadmpeg_ir::features::FeatureEvaluation::from_definition(
                FeatureDefinition::Operation(operation),
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

/// First segment major version whose extent enumerations use type value 12.
const EXTENT_TYPE_12_FIRST_MAJOR: u8 = 22;

fn enum16(
    source: &PmDcFeature,
    slot: usize,
    family: PmDcFeatureEnumFamily,
    index: &ProjectionIndex<'_>,
) -> Option<u16> {
    let property = slot_property(source, slot, index)?;
    let expected_type = match family {
        PmDcFeatureEnumFamily::PartOperation => 5,
        PmDcFeatureEnumFamily::Extent
            if property.save_version_major >= EXTENT_TYPE_12_FIRST_MAJOR =>
        {
            12
        }
        PmDcFeatureEnumFamily::Extent => 11,
        PmDcFeatureEnumFamily::Hole => 3,
        PmDcFeatureEnumFamily::Fillet
        | PmDcFeatureEnumFamily::Chamfer
        | PmDcFeatureEnumFamily::PlateSide => 2,
        PmDcFeatureEnumFamily::Auxiliary => return None,
    };
    match property.kind {
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
    match index
        .parameter_values
        .get(parameter.id().as_str())?
        .as_ref()?
        .value
        .as_ref()?
    {
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
    match index
        .parameter_values
        .get(parameter.id().as_str())?
        .as_ref()?
        .value
        .as_ref()?
    {
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
    mod holes;
    mod sheet_metal;
    mod thicken;

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
    use crate::test_support::test_fixtures::{
        content, content_at, parse, primary_envelope_fixture,
    };
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
    fn a_sheet_metal_corner_round_uses_the_fillet_family() {
        assert!(matches!(
            super::FeatureFamily::from_class_id(super::CORNER_ROUND_CLASS_ID),
            Some(super::FeatureFamily::Fillet)
        ));
        assert!(super::FeatureFamily::Fillet
            .class_ids()
            .contains(&super::CORNER_ROUND_CLASS_ID));
    }

    #[test]
    fn a_sheet_metal_plate_with_a_bend_stays_native() {
        let family = super::FeatureFamily::from_class_id(super::PLATE_CLASS_ID);
        assert!(matches!(family, Some(super::FeatureFamily::Plate)));
        assert_eq!(
            super::FeatureFamily::Plate.output_slot(&test_feature(100, 15, &[])),
            6
        );
        // A plate with a bend to an existing face stays native.
        let bent = test_feature(100, 15, &[(4, 7)]);
        let label = test_label(100, 1, super::PLATE_CLASS_ID, &[]);
        let index = test_projection_index(&[], &[], &[], &[], &[], &[], &[], &[]);
        let arena = DecodeArena::new();
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &DecodePolicy::service())
            .expect("projection context");
        assert!(super::project_plate(&ctx, &bent, &label, &index).is_none());
    }

    #[test]
    fn feature_projection_reports_an_unlabelled_feature() {
        let inventory = super::FeatureInventory {
            edge_inputs: crate::feature::edge_selection::EdgeInventory::default(),
            record_types: std::collections::HashMap::new(),
            features: vec![test_feature(0, 0, &[])],
            pattern_features: Vec::new(),
            terminators: Vec::new(),
            properties: Vec::new(),
            dimensions: Vec::new(),
            labels: Vec::new(),
            entity_style_links: Vec::new(),
            kernel_entity_links: Vec::new(),
            work_planes: Vec::new(),
            issues: Vec::new(),
            ..Default::default()
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
            super::project(
                &ctx,
                &inventory,
                &design,
                &sketch,
                &[],
                &super::ProjectedSketches {
                    sketches: &[],
                    entities: &[],
                    profile_bodies: &[],
                    result_model: None,
                },
            )
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
        with_record(type_id, payload, policy, inventory)
    }

    /// Run `read` over a `PmDc` segment whose first record has `type_id` and
    /// `payload`.
    fn with_record<T>(
        type_id: [u8; 16],
        payload: &[u8],
        policy: DecodePolicy,
        read: impl FnOnce(&DecodeContext<'_>, &crate::rse::RseInventory<'_>) -> Result<T, CodecError>,
    ) -> Result<T, CodecError> {
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
        read(&ctx, &container.rse)
    }

    #[test]
    fn an_undecodable_profile_body_is_counted_and_is_no_record_issue() {
        let payload = [0u8; 8];
        let kernel = crate::kernel::KERNEL_RECORD_TYPE_ID;
        let inventory = inventory_with_record(kernel, &payload, DecodePolicy::service())
            .expect("feature inventory");
        assert!(inventory.issues.is_empty());
        let bodies = with_record(kernel, &payload, DecodePolicy::service(), |ctx, rse| {
            super::profile::decode_profile_bodies(ctx, rse)
        })
        .expect("profile bodies");
        assert_eq!((bodies.bodies.len(), bodies.undecoded), (0, 1));
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
            flags_extension: None,
            context: reference(0),
            source_index: 0,
            stamps: Vec::new(),
        }
    }

    #[test]
    fn a_fillet_edge_set_option_is_null_or_a_boolean_record() {
        let edge_set = |option| PmDcFeaturePropertyKind::FilletEdgeSet {
            edges: reference(5),
            radius: reference(21),
            selection: reference(6),
            continuity: reference(7),
            option,
        };
        let boolean = crate::pmdc::type_id_string(BOOLEAN_TYPE);
        let types = |ordinal: u32| match ordinal {
            8 => Some(boolean.as_str()),
            9 => Some("00000000000000000000000000000009"),
            _ => None,
        };
        let admitted = crate::validate::fillet_option_is_admitted;
        assert!(admitted(&edge_set(None), types));
        assert!(admitted(&edge_set(Some(reference(0))), types));
        assert!(admitted(&edge_set(Some(reference(9))), types));
        assert!(!admitted(&edge_set(Some(reference(10))), types));
        // An unresolved reference is the reference check's single finding.
        assert!(admitted(&edge_set(Some(reference(11))), types));
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
                    flags_extension: None,
                    owner: reference(owner_ordinal + 1),
                    parent: reference(0),
                    next: reference(0),
                },
                index,
                participants: reference_list(participants),
                name: format!("Feature {index}"),
                name_units: None,
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
                    flags_extension: None,
                    context: reference(0),
                    source_index: ordinal,
                    stamps: Vec::new(),
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
            result_model: None,
            edge_names: std::collections::HashMap::new(),
            dimensions: std::collections::HashMap::new(),
            sketch_entities: std::collections::HashMap::new(),
            properties: super::unique_index(properties.iter().map(|record| {
                (
                    (
                        record.identity.segment_token.as_str(),
                        record.identity.record_ordinal,
                    ),
                    record,
                )
            })),
            parameters: super::unique_index(parameters.iter().map(|record| {
                (
                    (
                        record.identity.segment_token.as_str(),
                        record.identity.record_ordinal,
                    ),
                    record,
                )
            })),
            parameter_values: super::unique_index(
                neutral_parameters
                    .iter()
                    .filter_map(|parameter| Some((parameter.native_ref.as_deref()?, parameter))),
            ),
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
            sketches_by_id: neutral_sketches
                .iter()
                .map(|sketch| (sketch.id.as_str(), sketch))
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
            profiles: super::profile::ProfileIndex::new(&[], &[], entity_style_links, &[], &[]),
            plate_sketches: std::collections::HashMap::new(),
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
        let mut bytes = if version >= 23 {
            content_at(21, version, &[])
        } else {
            content(21)
        };
        bytes.extend_from_slice(&69u32.to_le_bytes());
        bytes.extend_from_slice(&3u32.to_le_bytes());
        bytes.extend_from_slice(&references(&[]));
        bytes.extend_from_slice(&7u32.to_le_bytes());
        bytes.extend_from_slice(&references(&[0x8000_0010, 0x8000_0011]));
        for index in 0..6 {
            bytes.extend_from_slice(&(0x8000_0020u32 + index).to_le_bytes());
        }
        bytes.push(1);
        let slots = |bytes: &mut Vec<u8>, base: u32, count: u32| {
            for index in 0..count {
                bytes.extend_from_slice(&(base + index).to_le_bytes());
            }
        };
        let values = |bytes: &mut Vec<u8>, count: u32| {
            for index in 0..count {
                bytes.extend_from_slice(&(0x100u32 + index).to_le_bytes());
            }
        };
        match family {
            PmDcPatternFamily::Rectangular if version >= 29 => {
                slots(&mut bytes, 0x8000_0040, 12);
                values(&mut bytes, 8);
                slots(&mut bytes, 0x8000_004c, 14);
                if version >= 30 {
                    values(&mut bytes, 6);
                }
            }
            PmDcPatternFamily::Rectangular => {
                slots(&mut bytes, 0x8000_0040, if version > 20 { 26 } else { 20 });
            }
            PmDcPatternFamily::Mirror => {
                slots(&mut bytes, 0x8000_0040, 5);
                if version > 20 {
                    values(&mut bytes, 6);
                }
                if version >= 29 {
                    slots(&mut bytes, 0x8000_0050, 1);
                    values(&mut bytes, 8);
                    slots(&mut bytes, 0x8000_0051, 1);
                } else {
                    slots(&mut bytes, 0x8000_0050, 2);
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
            (29, PmDcPatternFamily::Rectangular, 32, 8),
            (30, PmDcPatternFamily::Rectangular, 32, 14),
            (29, PmDcPatternFamily::Mirror, 13, 14),
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
                    option: None,
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
            vec![fillet_properties[8].id()]
        );

        for (type_value, value, continuity, admitted) in [
            (0x0002_0004, 0, Some(false), true),
            (0x0002_0004, 0, Some(true), false),
            (0x0002_0004, 0, None, false),
            (0x0002_0004, 1, Some(false), false),
            (4, 1, Some(false), false),
            (5, 0, Some(false), false),
        ] {
            let mut properties = fillet_properties.clone();
            properties[4] = test_property(
                5,
                PmDcFeaturePropertyKind::WideEnumeration { type_value, value },
            );
            properties[2] = test_property(
                3,
                PmDcFeaturePropertyKind::FilletEdgeSet {
                    edges: reference(5),
                    radius: reference(21),
                    selection: reference(6),
                    continuity: reference(if continuity.is_some() { 7 } else { 0 }),
                    option: None,
                },
            );
            properties[5] = test_property(
                6,
                PmDcFeaturePropertyKind::Boolean {
                    name: String::new(),
                    name_value: 0,
                    value: continuity.unwrap_or(false),
                },
            );
            let index = test_projection_index(
                &properties,
                std::slice::from_ref(&raw_radius),
                std::slice::from_ref(&neutral_radius),
                &[],
                &[],
                &[],
                &[],
                &[],
            );
            let projected = project_fillet(&ctx, &fillet, &label, &index);
            if admitted {
                let (feature, _) = projected
                    .expect("native selector")
                    .expect("fillet projection");
                assert!(matches!(feature.evaluation.definition(),
                    FeatureDefinition::Operation(FeatureOperation::Fillet { groups })
                        if matches!(&groups[0].edges, cadmpeg_ir::features::EdgeSelection::Native(id)
                            if id == &properties[2].id())
                        && matches!(groups[0].radius, RadiusSpec::Constant { radius } if radius.get() == 2.5)
                ));
            } else {
                assert!(projected.is_none());
            }
        }

        let mut named_properties = fillet_properties.clone();
        named_properties[6] = test_property(
            7,
            PmDcFeaturePropertyKind::EdgeItem {
                index_references: PmDcU32List::new(
                    2,
                    Some(crate::pmdc::PmDcListMetadata::U32([1, 0])),
                    vec![42],
                )
                .expect("valid named feature fixture"),
                index_reference_value: -1,
                value: 0,
            },
        );
        let mut named_index = test_projection_index(
            &named_properties,
            std::slice::from_ref(&raw_radius),
            std::slice::from_ref(&neutral_radius),
            &[],
            &[],
            &[],
            &[],
            &[],
        );
        let generated_edge = cadmpeg_ir::features::GeneratedEdgeRef {
            feature: super::history_feature_id(&test_feature(90, 0, &[]).identity)
                .expect("valid named feature fixture"),
            local_id: cadmpeg_ir::features::SelectionReference::new(
                "inventor:nmx:INV_NMX_SWEEPGENERATED_TAG:3:0:18".into(),
            )
            .expect("valid named feature fixture"),
        };
        named_index.edge_names.insert(
            (
                named_properties[3]
                    .identity
                    .segment_token
                    .as_str()
                    .to_owned(),
                42,
            ),
            Some(generated_edge.clone()),
        );
        let (named, _) = project_fillet(&ctx, &fillet, &label, &named_index)
            .expect("named fillet candidate")
            .expect("named fillet projection");
        assert!(
            matches!(named.evaluation.definition(), FeatureDefinition::Operation(FeatureOperation::Fillet { groups })
            if matches!(&groups[0].edges, cadmpeg_ir::features::EdgeSelection::Generated { edges, native }
                if edges.as_slice() == [generated_edge.clone()] && native.as_str() == named_properties[3].id()))
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
        let mut named_index = index;
        named_index.edge_names.insert(
            (
                chamfer_properties[0]
                    .identity
                    .segment_token
                    .as_str()
                    .to_owned(),
                17,
            ),
            Some(generated_edge.clone()),
        );
        let (named, _) = project_chamfer(&ctx, &chamfer, &label, &named_index)
            .expect("named chamfer candidate")
            .expect("named chamfer projection");
        assert!(
            matches!(named.evaluation.definition(), FeatureDefinition::Operation(FeatureOperation::Chamfer { groups, .. })
            if matches!(&groups[0].edges, cadmpeg_ir::features::EdgeSelection::Generated { edges, native }
                if edges.as_slice() == [generated_edge] && native.as_str() == chamfer_properties[0].id()))
        );
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
                    flags_extension: None,
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
        assert_eq!(parsed.name, "Extrude1");
        assert_eq!(parsed.participants.references().len(), 1);
        assert_eq!(
            parsed.class_id,
            ClassId(cadmpeg_ir::native::bytes::NativeBytes::new([0xab; 16]))
        );
    }

    #[test]
    fn a_label_name_with_an_unpaired_surrogate_keeps_its_code_units() {
        let stored = [0x0059u16, 0xdca5, 0x006b];
        let mut label = Vec::new();
        label.extend_from_slice(&0u32.to_le_bytes());
        label.extend_from_slice(&18u16.to_le_bytes());
        for value in [0u32, 0, 0x8000_000d, 0, 0, 4] {
            label.extend_from_slice(&value.to_le_bytes());
        }
        label.extend_from_slice(&references(&[0x8000_0013]));
        label.extend_from_slice(&3u32.to_le_bytes());
        for unit in stored {
            label.extend_from_slice(&unit.to_le_bytes());
        }
        label.extend_from_slice(&[0xabu8; 16]);
        let parsed = parse(&label, |ctx, source| {
            parse_label(ctx, source, 16).expect("label")
        });
        assert_eq!(parsed.name, "Y\u{fffd}k");
        assert!(parsed.name_replaced());
        assert_eq!(parsed.name_units.as_deref(), Some(&stored[..]));

        // The wire admits code units only beside their replacement decoding.
        let wire = |name: &str, units: &[u16]| PmDcFeatureLabelPayloadWire {
            save_version_major: 16,
            header: parsed.header.clone(),
            index: 4,
            participants: parsed.participants.clone(),
            name: name.to_owned(),
            name_units: Some(units.to_vec()),
            class_id: "ab".repeat(16),
        };
        assert!(PmDcFeatureLabelPayload::try_from(wire("Y\u{fffd}k", &stored)).is_ok());
        assert!(PmDcFeatureLabelPayload::try_from(wire("Yk", &stored)).is_err());
        assert!(PmDcFeatureLabelPayload::try_from(wire("Yk", &[0x59, 0x6b])).is_err());
    }

    #[test]
    fn parses_newer_segment_version_record_forms() {
        // A feature body starts with its state, so its header has no stamps.
        let mut feature = content_at(7, 29, &[]);
        feature.extend_from_slice(&2908i32.to_le_bytes());
        feature.extend_from_slice(&0u32.to_le_bytes());
        feature.extend_from_slice(&references(&[0x8000_0004]));
        feature.extend_from_slice(&9u32.to_le_bytes());
        let parsed = parse(&feature, |_, source| {
            parse_feature(source, 29).expect("feature")
        });
        assert_eq!(parsed.header.flags_extension, Some(0));
        assert!(parsed.header.stamps.is_empty());
        assert_eq!(parsed.state, 2908);
        assert_eq!(parsed.properties.references().len(), 1);

        // Other content records store one stamp in version 23, two from 25,
        // and a fillet edge set adds a fifth reference from version 23.
        let edge_set = |version: u8, stamps: &[i32], option: u32| {
            let mut bytes = content_at(18, version, stamps);
            for reference in [
                0x8000_0010u32,
                0x8000_0011,
                0x8000_0012,
                0x8000_0013,
                option,
            ] {
                bytes.extend_from_slice(&reference.to_le_bytes());
            }
            bytes
        };
        let parsed = parse(&edge_set(23, &[-1], 0x8000_0014), |ctx, source| {
            parse_fillet_edge_set(ctx, source, 23).expect("fillet edge set")
        });
        assert_eq!(parsed.header.stamps, [-1]);
        assert!(matches!(
            parsed.kind,
            PmDcFeaturePropertyKind::FilletEdgeSet { option: Some(option), .. } if option.index() == 20
        ));
        let parsed = parse(&edge_set(25, &[-1, 2502], 0), |ctx, source| {
            parse_fillet_edge_set(ctx, source, 25).expect("fillet edge set")
        });
        assert_eq!(parsed.header.stamps, [-1, 2502]);
        assert!(matches!(
            parsed.kind,
            PmDcFeaturePropertyKind::FilletEdgeSet { option: Some(option), .. } if option.index() == 0
        ));
        // Without its stamp, a version 23 record is short.
        parse(&edge_set(23, &[], 0x8000_0014), |ctx, source| {
            assert!(parse_fillet_edge_set(ctx, source, 23).is_err());
        });

        // Version 29 adds a zero u32 to the linked header after its values.
        let mut entity_link = Vec::new();
        entity_link.extend_from_slice(&0u32.to_le_bytes());
        entity_link.extend_from_slice(&16u16.to_le_bytes());
        for value in [0u32, 0, 0, 0x8000_0008, 0, 0x8000_0009, 1, 2, 3] {
            entity_link.extend_from_slice(&value.to_le_bytes());
        }
        let parsed = parse(&entity_link, |_, source| {
            parse_entity_style_link(source, 29).expect("entity-style link")
        });
        assert_eq!(parsed.header.owner.index(), 8);
        assert_eq!(parsed.header.next.index(), 9);
        assert_eq!(parsed.associative_id, 2);
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
                flags_extension: None,
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
            name_units: None,
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

    mod edge_treatments;
    mod extents;
    mod revolve;
    mod shell;
    mod thread;
}
