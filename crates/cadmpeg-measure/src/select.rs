// SPDX-License-Identifier: Apache-2.0
//! What one measurement pass measures: body scope, row levels and face
//! filters.

use cadmpeg_ir::ids::{BodyId, EdgeId, FaceId, SurfaceId};
use cadmpeg_ir::scalar::NonNegativeLength;
use serde::Serialize;

/// The rows a measurement pass emits.
///
/// Without a face, edge or surface level, the pass emits one row per body in
/// scope. With one, it emits those rows and no body rows.
#[derive(Debug, Clone, Default)]
pub struct Selection {
    /// Body scope; every body when empty.
    pub bodies: Vec<BodyId>,
    /// Faces to measure.
    pub faces: Vec<FaceId>,
    /// Edges to measure.
    pub edges: Vec<EdgeId>,
    /// Surfaces whose carrier rows to report.
    pub surfaces: Vec<SurfaceId>,
    /// Measure every face in scope.
    pub all_faces: bool,
    /// Measure every edge in scope.
    pub all_edges: bool,
    /// Filters on face rows. A non-empty filter selects the face level.
    pub face_filter: FaceFilter,
}

impl Selection {
    /// Whether the face level is selected.
    #[must_use]
    pub fn face_level(&self) -> bool {
        !self.faces.is_empty() || self.all_faces || !self.face_filter.is_empty()
    }

    /// Whether the edge level is selected.
    #[must_use]
    pub fn edge_level(&self) -> bool {
        !self.edges.is_empty() || self.all_edges
    }

    /// Whether the surface level is selected.
    #[must_use]
    pub fn surface_level(&self) -> bool {
        !self.surfaces.is_empty()
    }
}

/// Filters on face rows.
#[derive(Debug, Clone, Default)]
pub struct FaceFilter {
    /// Canonical surface kinds a face row must have; any kind when empty.
    pub kinds: Vec<SurfaceKind>,
    /// Range the face's radius must lie in.
    pub radius: Option<RadiusRange>,
}

impl FaceFilter {
    /// Whether the filter selects every face.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.kinds.is_empty() && self.radius.is_none()
    }
}

/// The canonical kind of a face's carrier.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum SurfaceKind {
    /// A plane.
    Plane,
    /// A circular cylinder.
    Cylinder,
    /// A cone, circular or elliptic.
    Cone,
    /// A sphere.
    Sphere,
    /// A torus.
    Torus,
    /// A NURBS surface.
    Nurbs,
    /// A polygonal surface.
    Polygonal,
    /// A procedural surface.
    Procedural,
    /// An unknown or unresolved surface.
    Unknown,
}

impl SurfaceKind {
    /// Every kind, in declaration order.
    pub const ALL: [Self; 9] = [
        Self::Plane,
        Self::Cylinder,
        Self::Cone,
        Self::Sphere,
        Self::Torus,
        Self::Nurbs,
        Self::Polygonal,
        Self::Procedural,
        Self::Unknown,
    ];

    /// The serialized name.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Plane => "plane",
            Self::Cylinder => "cylinder",
            Self::Cone => "cone",
            Self::Sphere => "sphere",
            Self::Torus => "torus",
            Self::Nurbs => "nurbs",
            Self::Polygonal => "polygonal",
            Self::Procedural => "procedural",
            Self::Unknown => "unknown",
        }
    }

    /// The kind with this serialized name.
    #[must_use]
    pub fn parse(text: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|kind| kind.as_str() == text)
    }

    /// Whether the kind is an analytic quadric or torus.
    #[must_use]
    pub const fn analytic(self) -> bool {
        matches!(
            self,
            Self::Plane | Self::Cylinder | Self::Cone | Self::Sphere | Self::Torus
        )
    }
}

/// A closed range of radii; either end may be open.
#[derive(Debug, Clone, Copy)]
pub struct RadiusRange {
    min: Option<NonNegativeLength>,
    max: Option<NonNegativeLength>,
}

impl RadiusRange {
    /// Absent when `min > max`.
    #[must_use]
    pub fn new(min: Option<NonNegativeLength>, max: Option<NonNegativeLength>) -> Option<Self> {
        match (min, max) {
            (Some(min), Some(max)) if min.get() > max.get() => None,
            _ => Some(Self { min, max }),
        }
    }

    /// Whether `radius` lies in the range widened by `tolerance` at both
    /// ends.
    #[must_use]
    pub fn contains(&self, radius: f64, tolerance: f64) -> bool {
        self.min.is_none_or(|min| radius >= min.get() - tolerance)
            && self.max.is_none_or(|max| radius <= max.get() + tolerance)
    }
}

#[cfg(test)]
mod tests {
    use super::{RadiusRange, Selection, SurfaceKind};
    use cadmpeg_ir::scalar::NonNegativeLength;

    #[test]
    fn surface_kinds_round_trip_their_names() {
        for kind in SurfaceKind::ALL {
            assert_eq!(SurfaceKind::parse(kind.as_str()), Some(kind));
            assert_eq!(
                serde_json::to_value(kind).unwrap(),
                serde_json::Value::from(kind.as_str())
            );
        }
        assert_eq!(SurfaceKind::parse("cylindrical"), None);
    }

    #[test]
    fn radius_ranges_refuse_inverted_ends_and_widen_by_tolerance() {
        let length = |value| NonNegativeLength::new(value);
        assert!(RadiusRange::new(length(2.0), length(1.0)).is_none());
        let range = RadiusRange::new(length(9.0), length(11.0)).unwrap();
        assert!(range.contains(10.0, 0.0));
        assert!(!range.contains(11.5, 0.0));
        assert!(range.contains(11.0 + 1.0e-8, 1.0e-7));
        let open = RadiusRange::new(length(20.0), None).unwrap();
        assert!(!open.contains(10.0, 1.0e-7));
        assert!(open.contains(1.0e6, 0.0));
    }

    #[test]
    fn a_filter_selects_the_face_level() {
        let mut selection = Selection::default();
        assert!(!selection.face_level());
        selection.face_filter.kinds.push(SurfaceKind::Torus);
        assert!(selection.face_level());
        assert!(!selection.edge_level());
    }
}
