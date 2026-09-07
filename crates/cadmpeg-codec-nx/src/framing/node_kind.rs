// SPDX-License-Identifier: Apache-2.0
//! Supported fixed-record node kinds.

use crate::layout::token;

/// Supported fixed-record node kind.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
#[repr(u8)]
pub enum NodeKind {
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
    /// BlendSurface record.
    BlendSurface = token::BLEND_SURF,
    /// OffsetSurface record.
    OffsetSurface = token::OFFSET_SURF,
    /// BSurface record.
    BSurface = token::B_SURFACE,
    /// TrimmedCurve record.
    TrimmedCurve = token::TRIMMED_CURVE,
    /// BCurve record.
    BCurve = token::B_CURVE,
    /// SpCurve record.
    SpCurve = token::SP_CURVE,
}

impl NodeKind {
    /// Parasolid record tag byte.
    pub const fn code(self) -> u8 {
        self as u8
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
