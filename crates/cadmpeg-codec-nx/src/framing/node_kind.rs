// SPDX-License-Identifier: Apache-2.0
//! Supported fixed-record node kinds.

use crate::layout::token;

/// Supported fixed-record node kind.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
#[repr(u8)]
pub(crate) enum NodeKind {
    /// Body record.
    Body = token::BODY,
    /// Shell record.
    Shell = token::SHELL,
    /// Face record.
    Face = token::FACE,
    /// Loop record.
    Loop = token::LOOP,
    /// Edge record.
    Edge = token::EDGE,
    /// Fin record.
    Fin = token::FIN,
    /// Vertex record.
    Vertex = token::VERTEX,
    /// Region record.
    Region = token::REGION,
    /// Point record.
    Point = token::POINT,
    /// Line record.
    Line = token::LINE,
    /// Circle record.
    Circle = token::CIRCLE,
    /// Ellipse record.
    Ellipse = token::ELLIPSE,
    /// Intersection record.
    Intersection = 38,
    /// Plane record.
    Plane = token::PLANE,
    /// Cylinder record.
    Cylinder = token::CYLINDER,
    /// Cone record.
    Cone = token::CONE,
    /// Sphere record.
    Sphere = token::SPHERE,
    /// Torus record.
    Torus = token::TORUS,
    /// `BlendSurface` record.
    BlendSurface = token::BLEND_SURF,
    /// `OffsetSurface` record.
    OffsetSurface = token::OFFSET_SURF,
    /// `BSurface` record.
    BSurface = token::B_SURFACE,
    /// `TrimmedCurve` record.
    TrimmedCurve = token::TRIMMED_CURVE,
    /// `BCurve` record.
    BCurve = token::B_CURVE,
    /// `SpCurve` record.
    SpCurve = token::SP_CURVE,
}

impl cadmpeg_core::decode::cost::DecodeCost for NodeKind {
    const FIXED_BYTES: Option<u64> =
        Some(cadmpeg_core::decode::u64_from_index(std::mem::size_of::<
            Self,
        >()));
    fn decode_cost(
        &self,
        _ctx: &cadmpeg_core::decode::DecodeContext<'_>,
        _operation: &'static str,
    ) -> Result<u64, cadmpeg_core::CodecError> {
        Ok(cadmpeg_core::decode::u64_from_index(std::mem::size_of::<
            Self,
        >()))
    }
}

impl NodeKind {
    /// Number of supported fixed-record kinds.
    pub(crate) const COUNT: usize = 24;

    /// Dense index of this kind in `0..Self::COUNT`.
    pub(crate) const fn ordinal(self) -> usize {
        match self {
            NodeKind::Body => 0,
            NodeKind::Shell => 1,
            NodeKind::Face => 2,
            NodeKind::Loop => 3,
            NodeKind::Edge => 4,
            NodeKind::Fin => 5,
            NodeKind::Vertex => 6,
            NodeKind::Region => 7,
            NodeKind::Point => 8,
            NodeKind::Line => 9,
            NodeKind::Circle => 10,
            NodeKind::Ellipse => 11,
            NodeKind::Intersection => 12,
            NodeKind::Plane => 13,
            NodeKind::Cylinder => 14,
            NodeKind::Cone => 15,
            NodeKind::Sphere => 16,
            NodeKind::Torus => 17,
            NodeKind::BlendSurface => 18,
            NodeKind::OffsetSurface => 19,
            NodeKind::BSurface => 20,
            NodeKind::TrimmedCurve => 21,
            NodeKind::BCurve => 22,
            NodeKind::SpCurve => 23,
        }
    }

    /// Parasolid record tag byte.
    pub(crate) const fn code(self) -> u8 {
        match self {
            NodeKind::Body => token::BODY,
            NodeKind::Shell => token::SHELL,
            NodeKind::Face => token::FACE,
            NodeKind::Loop => token::LOOP,
            NodeKind::Edge => token::EDGE,
            NodeKind::Fin => token::FIN,
            NodeKind::Vertex => token::VERTEX,
            NodeKind::Region => token::REGION,
            NodeKind::Point => token::POINT,
            NodeKind::Line => token::LINE,
            NodeKind::Circle => token::CIRCLE,
            NodeKind::Ellipse => token::ELLIPSE,
            NodeKind::Intersection => 38,
            NodeKind::Plane => token::PLANE,
            NodeKind::Cylinder => token::CYLINDER,
            NodeKind::Cone => token::CONE,
            NodeKind::Sphere => token::SPHERE,
            NodeKind::Torus => token::TORUS,
            NodeKind::BlendSurface => token::BLEND_SURF,
            NodeKind::OffsetSurface => token::OFFSET_SURF,
            NodeKind::BSurface => token::B_SURFACE,
            NodeKind::TrimmedCurve => token::TRIMMED_CURVE,
            NodeKind::BCurve => token::B_CURVE,
            NodeKind::SpCurve => token::SP_CURVE,
        }
    }
}

impl TryFrom<u8> for NodeKind {
    type Error = ();
    fn try_from(value: u8) -> Result<Self, Self::Error> {
        Ok(match value {
            token::BODY => Self::Body,
            token::SHELL => Self::Shell,
            token::FACE => Self::Face,
            token::LOOP => Self::Loop,
            token::EDGE => Self::Edge,
            token::FIN => Self::Fin,
            token::VERTEX => Self::Vertex,
            token::REGION => Self::Region,
            token::POINT => Self::Point,
            token::LINE => Self::Line,
            token::CIRCLE => Self::Circle,
            token::ELLIPSE => Self::Ellipse,
            38 => Self::Intersection,
            token::PLANE => Self::Plane,
            token::CYLINDER => Self::Cylinder,
            token::CONE => Self::Cone,
            token::SPHERE => Self::Sphere,
            token::TORUS => Self::Torus,
            token::BLEND_SURF => Self::BlendSurface,
            token::OFFSET_SURF => Self::OffsetSurface,
            token::B_SURFACE => Self::BSurface,
            token::TRIMMED_CURVE => Self::TrimmedCurve,
            token::B_CURVE => Self::BCurve,
            token::SP_CURVE => Self::SpCurve,
            _ => return Err(()),
        })
    }
}
