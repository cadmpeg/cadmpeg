// SPDX-License-Identifier: Apache-2.0
//! Admitted delta record layouts and their native-wire projections.

use super::group::{GroupReferenceStatus, GroupSelector};
use super::record_kind::RecordKind;
use crate::framing::xmt_reference::NonNullXmt;
use crate::nurbs::curve_references::CurveDescriptorReferences;
use crate::parasolid::entity_references::EntityReferences;

/// Semantic family of one admitted deltas record.
#[derive(Debug, Clone, PartialEq)]
pub enum RecordFamily {
    Body {
        references: Vec<u32>,
        node_id: u32,
    },
    Shell {
        references: [u32; 1],
        node_id: u32,
    },
    Face {
        references: [u32; 11],
        node_id: u32,
    },
    Loop {
        references: [u32; 1],
        node_id: u32,
    },
    Edge {
        references: [u32; 8],
        node_id: u32,
    },
    Fin { references: [u32; 9] },
    Vertex {
        references: [u32; 6],
        node_id: u32,
    },
    Region {
        references: [u32; 1],
        node_id: u32,
    },
    Point {
        references: [u32; 4],
        node_id: u32,
        position: [f64; 3],
    },
    Line {
        references: [u32; 5],
        node_id: u32,
    },
    Circle {
        references: [u32; 5],
        node_id: u32,
    },
    Ellipse {
        references: [u32; 5],
        node_id: u32,
    },
    Intersection {
        references: [u32; 11],
        node_id: u32,
    },
    Chart,
    TermUse,
    Type45,
    Plane {
        references: [u32; 5],
        node_id: u32,
    },
    Cylinder {
        references: [u32; 5],
        node_id: u32,
    },
    Cone {
        references: [u32; 5],
        node_id: u32,
    },
    Sphere {
        references: [u32; 5],
        node_id: u32,
    },
    Torus {
        references: [u32; 5],
        node_id: u32,
    },
    BlendSurf {
        references: [u32; 12],
        node_id: u32,
    },
    BlendBound { references: [u32; 7] },
    OffsetSurf {
        references: [u32; 6],
        node_id: u32,
    },
    Type67 {
        references: [u32; 6],
        node_id: u32,
    },
    Type70 {
        references: [u32; 4],
        trailing_reference: NonNullXmt,
        node_id: u32,
    },
    AttdefList { references: Vec<u32> },
    Entity51 { leading_references: [u32; 5], trailing_references: EntityReferences },
    Entity52,
    Entity53,
    Entity54,
    Entity55,
    Entity56,
    Entity57,
    Entity58,
    Entity59,
    Entity62,
    Group {
        references: [u32; 5],
        node_id: u32,
        selector: GroupSelector,
        linked_reference_status: GroupReferenceStatus,
    },
    IntersectionData { references: [u32; 11] },
    Type91 { references: [u32; 6] },
    Type101 { references: [u32; 15] },
    BSurface {
        references: [u32; 7],
        node_id: u32,
    },
    BSurfaceData,
    BSurfaceDescriptor,
    Multiplicities,
    Knots,
    TrimmedCurve {
        references: [u32; 6],
        node_id: u32,
    },
    BCurve {
        references: [u32; 7],
        node_id: u32,
    },
    BCurveData,
    BCurveDescriptor { references: CurveDescriptorReferences },
    SpCurve {
        references: [u32; 8],
        node_id: u32,
    },
    Type141 { references: [u32; 4] },
    SupportUv,
}

impl RecordFamily {
    /// Numeric Parasolid node type for this family.
    pub const fn kind(&self) -> u16 {
        self.record_kind().code() as u16
    }

    const fn record_kind(&self) -> RecordKind {
        match self {
            Self::Body { .. } => RecordKind::Body,
            Self::Shell { .. } => RecordKind::Shell,
            Self::Face { .. } => RecordKind::Face,
            Self::Loop { .. } => RecordKind::Loop,
            Self::Edge { .. } => RecordKind::Edge,
            Self::Fin { .. } => RecordKind::Fin,
            Self::Vertex { .. } => RecordKind::Vertex,
            Self::Region { .. } => RecordKind::Region,
            Self::Point { .. } => RecordKind::Point,
            Self::Line { .. } => RecordKind::Line,
            Self::Circle { .. } => RecordKind::Circle,
            Self::Ellipse { .. } => RecordKind::Ellipse,
            Self::Intersection { .. } => RecordKind::Intersection,
            Self::Chart => RecordKind::Chart,
            Self::TermUse => RecordKind::TermUse,
            Self::Type45 => RecordKind::Type45,
            Self::Plane { .. } => RecordKind::Plane,
            Self::Cylinder { .. } => RecordKind::Cylinder,
            Self::Cone { .. } => RecordKind::Cone,
            Self::Sphere { .. } => RecordKind::Sphere,
            Self::Torus { .. } => RecordKind::Torus,
            Self::BlendSurf { .. } => RecordKind::BlendSurf,
            Self::BlendBound { .. } => RecordKind::BlendBound,
            Self::OffsetSurf { .. } => RecordKind::OffsetSurf,
            Self::Type67 { .. } => RecordKind::Type67,
            Self::Type70 { .. } => RecordKind::Type70,
            Self::AttdefList { .. } => RecordKind::AttdefList,
            Self::Entity51 { .. } => RecordKind::Entity51,
            Self::Entity52 => RecordKind::Entity52,
            Self::Entity53 => RecordKind::Entity53,
            Self::Entity54 => RecordKind::Entity54,
            Self::Entity55 => RecordKind::Entity55,
            Self::Entity56 => RecordKind::Entity56,
            Self::Entity57 => RecordKind::Entity57,
            Self::Entity58 => RecordKind::Entity58,
            Self::Entity59 => RecordKind::Entity59,
            Self::Entity62 => RecordKind::Entity62,
            Self::Group { .. } | Self::IntersectionData { .. } => RecordKind::Group,
            Self::Type91 { .. } => RecordKind::Type91,
            Self::Type101 { .. } => RecordKind::Type101,
            Self::BSurface { .. } => RecordKind::BSurface,
            Self::BSurfaceData => RecordKind::BSurfaceData,
            Self::BSurfaceDescriptor => RecordKind::BSurfaceDescriptor,
            Self::Multiplicities => RecordKind::Multiplicities,
            Self::Knots => RecordKind::Knots,
            Self::TrimmedCurve { .. } => RecordKind::TrimmedCurve,
            Self::BCurve { .. } => RecordKind::BCurve,
            Self::BCurveData => RecordKind::BCurveData,
            Self::BCurveDescriptor { .. } => RecordKind::BCurveDescriptor,
            Self::SpCurve { .. } => RecordKind::SpCurve,
            Self::Type141 { .. } => RecordKind::Type141,
            Self::SupportUv => RecordKind::SupportUv,
        }
    }

    /// Kernel node identifier when this family serializes one.
    pub const fn node_id(&self) -> Option<u32> {
        match self {
            Self::Body { node_id, .. }
            | Self::Shell { node_id, .. }
            | Self::Face { node_id, .. }
            | Self::Loop { node_id, .. }
            | Self::Edge { node_id, .. }
            | Self::Vertex { node_id, .. }
            | Self::Region { node_id, .. }
            | Self::Point { node_id, .. }
            | Self::Line { node_id, .. }
            | Self::Circle { node_id, .. }
            | Self::Ellipse { node_id, .. }
            | Self::Intersection { node_id, .. }
            | Self::Plane { node_id, .. }
            | Self::Cylinder { node_id, .. }
            | Self::Cone { node_id, .. }
            | Self::Sphere { node_id, .. }
            | Self::Torus { node_id, .. }
            | Self::BlendSurf { node_id, .. }
            | Self::OffsetSurf { node_id, .. }
            | Self::Type67 { node_id, .. }
            | Self::Type70 { node_id, .. }
            | Self::Group { node_id, .. }
            | Self::BSurface { node_id, .. }
            | Self::TrimmedCurve { node_id, .. }
            | Self::BCurve { node_id, .. }
            | Self::SpCurve { node_id, .. } => Some(*node_id),
            Self::Fin { .. }
            | Self::Chart
            | Self::TermUse
            | Self::Type45
            | Self::BlendBound { .. }
            | Self::AttdefList { .. }
            | Self::Entity51 { .. }
            | Self::Entity52
            | Self::Entity53
            | Self::Entity54
            | Self::Entity55
            | Self::Entity56
            | Self::Entity57
            | Self::Entity58
            | Self::Entity59
            | Self::Entity62
            | Self::IntersectionData { .. }
            | Self::Type91 { .. }
            | Self::Type101 { .. }
            | Self::BSurfaceData
            | Self::BSurfaceDescriptor
            | Self::Multiplicities
            | Self::Knots
            | Self::BCurveData
            | Self::BCurveDescriptor { .. }
            | Self::Type141 { .. }
            | Self::SupportUv => None,
        }
    }

    /// POINT coordinates in Parasolid metres.
    pub const fn position(&self) -> Option<[f64; 3]> {
        match self {
            Self::Point { position, .. } => Some(*position),
            _ => None,
        }
    }

    /// Stable family name used by the deltas census and native records.
    pub const fn family_name(&self) -> &'static str {
        match self {
            Self::IntersectionData { .. } => "INTERSECTION_DATA",
            _ => self.record_kind().name(),
        }
    }

    /// Ordered references retained by this record layout.
    pub fn references(&self) -> Vec<u32> {
        match self {
            Self::Body { references, .. } => references.to_vec(),
            Self::Shell { references, .. } => references.to_vec(),
            Self::Face { references, .. } => references.to_vec(),
            Self::Loop { references, .. } => references.to_vec(),
            Self::Edge { references, .. } => references.to_vec(),
            Self::Fin { references, .. } => references.to_vec(),
            Self::Vertex { references, .. } => references.to_vec(),
            Self::Region { references, .. } => references.to_vec(),
            Self::Point { references, .. } => references.to_vec(),
            Self::Line { references, .. } => references.to_vec(),
            Self::Circle { references, .. } => references.to_vec(),
            Self::Ellipse { references, .. } => references.to_vec(),
            Self::Intersection { references, .. } => references.to_vec(),
            Self::Plane { references, .. } => references.to_vec(),
            Self::Cylinder { references, .. } => references.to_vec(),
            Self::Cone { references, .. } => references.to_vec(),
            Self::Sphere { references, .. } => references.to_vec(),
            Self::Torus { references, .. } => references.to_vec(),
            Self::BlendSurf { references, .. } => references.to_vec(),
            Self::BlendBound { references, .. } => references.to_vec(),
            Self::OffsetSurf { references, .. } => references.to_vec(),
            Self::Type67 { references, .. } => references.to_vec(),
            Self::Type70 { references, trailing_reference, .. } => references.iter().copied()
                .chain([u32::from(*trailing_reference); 2]).collect(),
            Self::AttdefList { references, .. } => references.to_vec(),
            Self::Entity51 { leading_references, trailing_references } => leading_references.iter().copied()
                .chain(trailing_references.values().iter().copied()).collect(),
            Self::Group { references, .. } => references.to_vec(),
            Self::IntersectionData { references, .. } => references.to_vec(),
            Self::Type91 { references, .. } => references.to_vec(),
            Self::Type101 { references, .. } => references.to_vec(),
            Self::BSurface { references, .. } => references.to_vec(),
            Self::TrimmedCurve { references, .. } => references.to_vec(),
            Self::BCurve { references, .. } => references.to_vec(),
            Self::BCurveDescriptor { references, .. } => references.values(),
            Self::SpCurve { references, .. } => references.to_vec(),
            Self::Type141 { references, .. } => references.to_vec(),
            Self::Chart | Self::TermUse | Self::Type45
            | Self::Entity52 | Self::Entity53 | Self::Entity54 | Self::Entity55
            | Self::Entity56 | Self::Entity57 | Self::Entity58 | Self::Entity59
            | Self::Entity62 | Self::BSurfaceData | Self::BSurfaceDescriptor
            | Self::Multiplicities | Self::Knots | Self::BCurveData | Self::SupportUv => Vec::new(),
        }
    }

    pub(super) fn from_fixed(kind: u16, node_id: Option<u32>, position: Option<[f64; 3]>, references: Vec<u32>) -> Option<Self> {
        Some(match kind {
            13 => Self::Shell { references: references.try_into().ok()?, node_id: node_id? },
            14 => Self::Face { references: references.try_into().ok()?, node_id: node_id? },
            15 => Self::Loop { references: references.try_into().ok()?, node_id: node_id? },
            16 => Self::Edge { references: references.try_into().ok()?, node_id: node_id? },
            17 => Self::Fin { references: references.try_into().ok()? },
            18 => Self::Vertex { references: references.try_into().ok()?, node_id: node_id? },
            19 => Self::Region { references: references.try_into().ok()?, node_id: node_id? },
            29 => Self::Point { references: references.try_into().ok()?,
                node_id: node_id?,
                position: position?,
            },
            30 => Self::Line { references: references.try_into().ok()?, node_id: node_id? },
            31 => Self::Circle { references: references.try_into().ok()?, node_id: node_id? },
            32 => Self::Ellipse { references: references.try_into().ok()?, node_id: node_id? },
            38 => Self::Intersection { references: references.try_into().ok()?, node_id: node_id? },
            50 => Self::Plane { references: references.try_into().ok()?, node_id: node_id? },
            51 => Self::Cylinder { references: references.try_into().ok()?, node_id: node_id? },
            52 => Self::Cone { references: references.try_into().ok()?, node_id: node_id? },
            53 => Self::Sphere { references: references.try_into().ok()?, node_id: node_id? },
            54 => Self::Torus { references: references.try_into().ok()?, node_id: node_id? },
            56 => Self::BlendSurf { references: references.try_into().ok()?, node_id: node_id? },
            60 => Self::OffsetSurf { references: references.try_into().ok()?, node_id: node_id? },
            124 => Self::BSurface { references: references.try_into().ok()?, node_id: node_id? },
            133 => Self::TrimmedCurve { references: references.try_into().ok()?, node_id: node_id? },
            134 => Self::BCurve { references: references.try_into().ok()?, node_id: node_id? },
            137 => Self::SpCurve { references: references.try_into().ok()?, node_id: node_id? },
            _ => return None,
        })
    }

    pub(crate) fn from_wire(
        name: &str,
        kind: u16,
        node_id: Option<u32>,
        position: Option<[f64; 3]>,
        group_selector: Option<GroupSelector>,
        group_linked_reference_status: Option<GroupReferenceStatus>,
        references: Vec<u32>,
    ) -> Option<Self> {
        let family = match name {
            "BODY" => Self::Body { references, node_id: node_id? },
            "SHELL" => Self::Shell { references: references.try_into().ok()?, node_id: node_id? },
            "FACE" => Self::Face { references: references.try_into().ok()?, node_id: node_id? },
            "LOOP" => Self::Loop { references: references.try_into().ok()?, node_id: node_id? },
            "EDGE" => Self::Edge { references: references.try_into().ok()?, node_id: node_id? },
            "FIN" => Self::Fin { references: references.try_into().ok()? },
            "VERTEX" => Self::Vertex { references: references.try_into().ok()?, node_id: node_id? },
            "REGION" => Self::Region { references: references.try_into().ok()?, node_id: node_id? },
            "POINT" => Self::Point { references: references.try_into().ok()?,
                node_id: node_id?,
                position: position?,
            },
            "LINE" => Self::Line { references: references.try_into().ok()?, node_id: node_id? },
            "CIRCLE" => Self::Circle { references: references.try_into().ok()?, node_id: node_id? },
            "ELLIPSE" => Self::Ellipse { references: references.try_into().ok()?, node_id: node_id? },
            "INTERSECTION" => Self::Intersection { references: references.try_into().ok()?, node_id: node_id? },
            "CHART" => { references.is_empty().then_some(())?; Self::Chart },
            "TERM_USE" => { references.is_empty().then_some(())?; Self::TermUse },
            "TYPE_45" => { references.is_empty().then_some(())?; Self::Type45 },
            "PLANE" => Self::Plane { references: references.try_into().ok()?, node_id: node_id? },
            "CYLINDER" => Self::Cylinder { references: references.try_into().ok()?, node_id: node_id? },
            "CONE" => Self::Cone { references: references.try_into().ok()?, node_id: node_id? },
            "SPHERE" => Self::Sphere { references: references.try_into().ok()?, node_id: node_id? },
            "TORUS" => Self::Torus { references: references.try_into().ok()?, node_id: node_id? },
            "BLEND_SURF" => Self::BlendSurf { references: references.try_into().ok()?, node_id: node_id? },
            "BLEND_BOUND" => Self::BlendBound { references: references.try_into().ok()? },
            "OFFSET_SURF" => Self::OffsetSurf { references: references.try_into().ok()?, node_id: node_id? },
            "TYPE_67" => Self::Type67 { references: references.try_into().ok()?, node_id: node_id? },
            "TYPE_70" => {
                let [a, b, c, d, trailing, repeated] = <[u32; 6]>::try_from(references).ok()?;
                (trailing == repeated).then_some(())?;
                Self::Type70 {
                    references: [a, b, c, d],
                    trailing_reference: NonNullXmt::try_from(trailing).ok()?,
                    node_id: node_id?,
                }
            },
            "ATTDEF_LIST" => Self::AttdefList { references },
            "ENTITY_51" => {
                let leading_references = references.get(..5)?.try_into().ok()?;
                let trailing_references = EntityReferences::new(references.into_iter().skip(5).collect()).ok()?;
                Self::Entity51 { leading_references, trailing_references }
            },
            "ENTITY_52" => { references.is_empty().then_some(())?; Self::Entity52 },
            "ENTITY_53" => { references.is_empty().then_some(())?; Self::Entity53 },
            "ENTITY_54" => { references.is_empty().then_some(())?; Self::Entity54 },
            "ENTITY_55" => { references.is_empty().then_some(())?; Self::Entity55 },
            "ENTITY_56" => { references.is_empty().then_some(())?; Self::Entity56 },
            "ENTITY_57" => { references.is_empty().then_some(())?; Self::Entity57 },
            "ENTITY_58" => { references.is_empty().then_some(())?; Self::Entity58 },
            "ENTITY_59" => { references.is_empty().then_some(())?; Self::Entity59 },
            "ENTITY_62" => { references.is_empty().then_some(())?; Self::Entity62 },
            "GROUP" => Self::Group { references: references.try_into().ok()?,
                node_id: node_id?,
                selector: group_selector?,
                linked_reference_status: group_linked_reference_status?,
            },
            "INTERSECTION_DATA" => Self::IntersectionData { references: references.try_into().ok()? },
            "TYPE_91" => Self::Type91 { references: references.try_into().ok()? },
            "TYPE_101" => Self::Type101 { references: references.try_into().ok()? },
            "B_SURFACE" => Self::BSurface { references: references.try_into().ok()?, node_id: node_id? },
            "B_SURFACE_DATA" => { references.is_empty().then_some(())?; Self::BSurfaceData },
            "B_SURFACE_DESCRIPTOR" => { references.is_empty().then_some(())?; Self::BSurfaceDescriptor },
            "MULTIPLICITIES" => { references.is_empty().then_some(())?; Self::Multiplicities },
            "KNOTS" => { references.is_empty().then_some(())?; Self::Knots },
            "TRIMMED_CURVE" => Self::TrimmedCurve { references: references.try_into().ok()?, node_id: node_id? },
            "B_CURVE" => Self::BCurve { references: references.try_into().ok()?, node_id: node_id? },
            "B_CURVE_DATA" => { references.is_empty().then_some(())?; Self::BCurveData },
            "B_CURVE_DESCRIPTOR" => Self::BCurveDescriptor { references: references.try_into().ok()? },
            "SP_CURVE" => Self::SpCurve { references: references.try_into().ok()?, node_id: node_id? },
            "TYPE_141" => Self::Type141 { references: references.try_into().ok()? },
            "SUPPORT_UV" => { references.is_empty().then_some(())?; Self::SupportUv },
            _ => return None,
        };
        let group_ok = match family {
            Self::Group { .. } => true,
            _ => group_selector.is_none() && group_linked_reference_status.is_none(),
        };
        let position_ok = match family {
            Self::Point { .. } => true,
            _ => position.is_none(),
        };
        (family.kind() == kind
            && family.node_id() == node_id
            && group_ok
            && position_ok)
            .then_some(family)
    }


}

