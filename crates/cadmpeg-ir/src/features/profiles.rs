// SPDX-License-Identifier: Apache-2.0
//! Sketch profile, planar profile, loft section and path references.

use super::geometry::FinitePoint3;
use super::selection_overlap;
use super::selections::{
    BodySelectionError, DistinctMembers, NativeSelections, NonEmptyMembers, SelectionMembers,
    SelectionReference, VertexSelection,
};
use super::{
    deserialize_local_value, deserialize_profile_parameter_range, deserialize_selection_curves,
    deserialize_selection_edges, deserialize_selection_entities, deserialize_selection_faces,
    deserialize_selection_local_id, deserialize_selection_native, deserialize_selection_profiles,
    deserialize_selection_selections, FeatureCollectionError, FeatureId,
};
use crate::ids::{
    CurveId, EdgeId, FaceId, FeatureInputTopologyId, HistoricalEdgeId, HistoricalFaceId, VertexId,
};
use cadmpeg_core::text::NonBlankString;
#[cfg(feature = "schema")]
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// One directed use of a solved sketch curve in an arrangement boundary.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct SketchProfileBoundaryUse {
    /// Sketch entity supplying the curve geometry.
    pub entity: crate::sketches::SketchEntityId,
    /// Parameter endpoints on the source curve, ordered in the entity's stored direction.
    /// A line is parameterized from `0` at its start point to `1` at its end point; an arc
    /// or circle by its angle in radians; an ellipse by its eccentric angle in radians; a
    /// NURBS curve by its knot-domain coordinate.
    #[serde(deserialize_with = "deserialize_profile_parameter_range")]
    pub parameter_range: crate::geometry::procedural_curves::DirectedParameterRange,
    /// Whether boundary traversal opposes the interval's stored direction.
    pub reversed: bool,
}

/// Whole-loop region with distinct holes that exclude its outer loop.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(try_from = "SketchProfileLoopsWire")]
pub struct SketchProfileLoops {
    pub(super) outer: u32,
    pub(super) holes: DistinctMembers<u32>,
}

#[derive(Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
struct SketchProfileLoopsWire {
    /// The exterior-loop index.
    outer: u32,
    /// The hole-loop indices in source order.
    holes: Vec<u32>,
}

impl SketchProfileLoops {
    /// Admit a whole-loop region with distinct holes that exclude its outer loop.
    pub fn new(outer: u32, holes: Vec<u32>) -> Result<Self, &'static str> {
        let holes = DistinctMembers::try_from(holes).map_err(|_| "holes must be distinct")?;
        if holes.contains(&outer) {
            return Err("holes must not contain outer");
        }
        Ok(Self { outer, holes })
    }

    /// The exterior-loop index.
    pub fn outer(&self) -> u32 {
        self.outer
    }

    /// The hole-loop indices in source order.
    pub fn holes(&self) -> &[u32] {
        self.holes.as_slice()
    }
}

impl TryFrom<SketchProfileLoopsWire> for SketchProfileLoops {
    type Error = FeatureCollectionError;
    fn try_from(wire: SketchProfileLoopsWire) -> Result<Self, Self::Error> {
        let mut seen = std::collections::HashSet::new();
        seen.try_reserve(wire.holes.len())
            .map_err(|_| FeatureCollectionError::Invalid("hole member allocation failed"))?;
        for hole in &wire.holes {
            if !seen.insert(hole) {
                return Err(FeatureCollectionError::Invalid("holes must be distinct"));
            }
        }
        if seen.contains(&wire.outer) {
            return Err(FeatureCollectionError::Invalid(
                "holes must not contain outer",
            ));
        }
        Ok(Self {
            outer: wire.outer,
            holes: DistinctMembers(wire.holes),
        })
    }
}

/// One connected planar region bounded by solved sketch curves.
#[derive(Debug, Clone, PartialEq, Serialize, cadmpeg_derive::DeserializeTagged)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum SketchProfileRegion {
    /// Exterior and holes are complete entries in the sketch profile table.
    Loops {
        /// Whole-loop exterior and hole indices.
        #[serde(flatten)]
        loops: SketchProfileLoops,
    },
    /// Boundary rings switch source curves at arrangement intersections.
    Trimmed {
        /// Directed exterior boundary ring.
        outer_boundary: NonEmptyMembers<SketchProfileBoundaryUse>,
        /// Directed hole boundary rings.
        hole_boundaries: Vec<NonEmptyMembers<SketchProfileBoundaryUse>>,
    },
}

impl SketchProfileRegion {
    /// Admit a whole-loop region.
    pub fn loops(outer: u32, holes: Vec<u32>) -> Result<Self, &'static str> {
        SketchProfileLoops::new(outer, holes).map(|loops| Self::Loops { loops })
    }

    /// Admits a trimmed region whose exterior and hole rings are nonempty.
    pub fn trimmed(
        outer_boundary: Vec<SketchProfileBoundaryUse>,
        hole_boundaries: Vec<Vec<SketchProfileBoundaryUse>>,
    ) -> Result<Self, &'static str> {
        Ok(Self::Trimmed {
            outer_boundary: outer_boundary
                .try_into()
                .map_err(|_| "outer_boundary must not be empty")?,
            hole_boundaries: hole_boundaries
                .into_iter()
                .map(|ring| {
                    ring.try_into()
                        .map_err(|_| "hole_boundaries must not contain an empty ring")
                })
                .collect::<Result<_, _>>()?,
        })
    }
}

/// Nonempty distinct profile regions in source selection order.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(transparent)]
pub struct SketchProfileRegions(pub(super) Vec<SketchProfileRegion>);

impl TryFrom<Vec<SketchProfileRegion>> for SketchProfileRegions {
    type Error = &'static str;
    fn try_from(regions: Vec<SketchProfileRegion>) -> Result<Self, Self::Error> {
        if regions.is_empty() {
            return Err("regions must not be empty");
        }
        if regions
            .iter()
            .enumerate()
            .any(|(index, region)| regions[..index].contains(region))
        {
            return Err("regions must be distinct");
        }
        Ok(Self(regions))
    }
}

impl SketchProfileRegions {
    /// The selected regions in source order.
    pub fn as_slice(&self) -> &[SketchProfileRegion] {
        &self.0
    }
}

impl std::ops::Deref for SketchProfileRegions {
    type Target = [SketchProfileRegion];
    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl<'de> Deserialize<'de> for SketchProfileRegions {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        Self::try_from(Vec::<SketchProfileRegion>::deserialize(deserializer)?)
            .map_err(serde::de::Error::custom)
    }
}

/// Profile consumed by a profile-driven feature that admits no spatial-sketch
/// form.
#[derive(Debug, Clone, PartialEq, Serialize, cadmpeg_derive::DeserializeTagged)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(
    tag = "kind",
    content = "value",
    rename_all = "snake_case",
    deny_unknown_fields
)]
pub enum PlanarProfileRef {
    /// A profile is required by the identified native owner but its carrier is unresolved.
    Unresolved(String),
    /// Opaque reference into a native feature-input record; no neutral geometry given.
    Native(String),
    /// Solved neutral sketch profile.
    Sketch(crate::sketches::SketchId),
    /// Specific solved profile loops within one neutral sketch.
    SketchProfiles {
        /// Sketch containing the selected loops.
        sketch: crate::sketches::SketchId,
        /// Zero-based indices into [`crate::sketches::Sketch::profiles`].
        #[serde(deserialize_with = "deserialize_selection_profiles")]
        profiles: SelectionMembers<u32>,
    },
    /// Exact union of bounded atomic regions within one neutral sketch.
    SketchRegions {
        /// Sketch containing every referenced boundary loop.
        sketch: crate::sketches::SketchId,
        /// Connected regions in source selection order.
        regions: SketchProfileRegions,
    },
    /// Exact ordered sketch entities forming an open or closed profile.
    SketchEntities {
        /// Sketch containing every selected entity.
        sketch: crate::sketches::SketchId,
        /// Selected entities in source order.
        #[serde(deserialize_with = "deserialize_selection_entities")]
        entities: SelectionMembers<crate::sketches::SketchEntityId>,
    },
    /// Source-native selection within a known neutral sketch.
    SketchSelection {
        /// Sketch containing the unresolved selected geometry.
        sketch: crate::sketches::SketchId,
        /// Full-fidelity native selection records in source order.
        #[serde(deserialize_with = "deserialize_selection_selections")]
        selections: NativeSelections,
    },
    /// Profile given by faces in the consuming feature's input topology.
    HistoricalFaces {
        /// Input topology containing every selected face.
        state: FeatureInputTopologyId,
        /// State-local face identities in source selection order.
        #[serde(deserialize_with = "deserialize_selection_faces")]
        faces: SelectionMembers<HistoricalFaceId>,
        /// Full-fidelity source selection groups in source order.
        #[serde(deserialize_with = "deserialize_selection_native")]
        native: NativeSelections,
    },
    /// Complete curve result of an earlier construction-history feature.
    Feature(FeatureId),
    /// Curves in an intermediate regenerated feature result, paired with the
    /// format-native persistent reference required for rewrite.
    Generated {
        /// Persistent feature-local curve identities.
        #[serde(deserialize_with = "deserialize_selection_curves")]
        curves: NonEmptyMembers<GeneratedCurveRef>,
        /// Format-native persistent profile reference.
        #[serde(deserialize_with = "deserialize_selection_native")]
        native: SelectionReference,
    },
    /// Profile given directly as a set of solved B-rep faces.
    Faces(Vec<FaceId>),
}

/// Profile consumed by a profile-driven feature.
#[derive(Debug, Clone, PartialEq, Serialize, cadmpeg_derive::DeserializeTagged)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(
    tag = "kind",
    content = "value",
    rename_all = "snake_case",
    deny_unknown_fields
)]
pub enum ProfileRef {
    /// Specific solved profile loops within one neutral spatial sketch.
    SpatialSketchProfiles {
        /// Spatial sketch containing the selected loops.
        sketch: crate::sketches::SpatialSketchId,
        /// Zero-based indices into [`crate::sketches::SpatialSketch::profiles`].
        #[serde(deserialize_with = "deserialize_selection_profiles")]
        profiles: SelectionMembers<u32>,
    },
    /// Source-native selection within a known neutral spatial sketch.
    SpatialSketchSelection {
        /// Spatial sketch containing the unresolved selected geometry.
        sketch: crate::sketches::SpatialSketchId,
        /// Full-fidelity native selection records in source order.
        #[serde(deserialize_with = "deserialize_selection_selections")]
        selections: NativeSelections,
    },
    /// Any profile form that does not select spatial-sketch geometry.
    Planar(PlanarProfileRef),
}

impl From<crate::sketches::SketchId> for PlanarProfileRef {
    fn from(sketch: crate::sketches::SketchId) -> Self {
        Self::Sketch(sketch)
    }
}

impl PlanarProfileRef {
    /// Retain a native profile reference.
    #[must_use]
    pub const fn native(reference: String) -> Self {
        Self::Native(reference)
    }
}

impl ProfileRef {
    /// Planar form of this profile, when it selects no spatial-sketch geometry.
    #[must_use]
    pub const fn planar(&self) -> Option<&PlanarProfileRef> {
        match self {
            Self::Planar(profile) => Some(profile),
            _ => None,
        }
    }
}

/// One ordered cross-section consumed by a loft operation.
#[derive(Debug, Clone, PartialEq, Serialize, cadmpeg_derive::DeserializeTagged)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(
    tag = "kind",
    content = "value",
    rename_all = "snake_case",
    deny_unknown_fields
)]
pub enum LoftSection {
    /// Planar or face-backed section profile.
    Profile(ProfileRef),
    /// Point-like terminal section.
    Point(LoftPointSection),
}

/// Point-like cross-section consumed by a loft operation.
#[derive(Debug, Clone, PartialEq, Serialize, cadmpeg_derive::DeserializeTagged)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(tag = "kind", content = "value", rename_all = "snake_case")]
#[serde(deny_unknown_fields)]
pub enum LoftPointSection {
    /// Source-native point-selection record whose position is not resolved.
    #[serde(rename = "native_point")]
    Native(#[serde(deserialize_with = "deserialize_local_value")] NonBlankString),
    /// Solved model-space point section.
    Point(FinitePoint3),
    /// Solved B-rep vertex section.
    Vertex(VertexId),
}

/// Persistent identity of a curve in one regenerated feature result.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct GeneratedCurveRef {
    /// Feature whose regenerated result owns the curve.
    pub feature: FeatureId,
    /// Complete ordered feature-local component identity.
    #[serde(deserialize_with = "deserialize_selection_local_id")]
    pub local_id: SelectionReference,
}

/// Trajectory consumed by a sweep or path-driven operation.
#[derive(Debug, Clone, PartialEq, Serialize, cadmpeg_derive::DeserializeTagged)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(
    tag = "kind",
    content = "value",
    rename_all = "snake_case",
    deny_unknown_fields
)]
pub enum PathRef {
    /// Source path exists but its neutral members remain unresolved.
    Unresolved(String),
    /// Opaque reference into a native path record.
    Native(String),
    /// Ordered geometry from a neutral sketch.
    Sketch(crate::sketches::SketchId),
    /// Ordered selected curves from one neutral planar sketch.
    SketchCurves {
        /// Sketch containing every selected curve.
        sketch: crate::sketches::SketchId,
        /// Selected curve identities in source order.
        #[serde(deserialize_with = "deserialize_selection_curves")]
        curves: SelectionMembers<crate::sketches::SketchEntityId>,
    },
    /// Source-native curve selection within a known neutral spatial sketch.
    SpatialSketchSelection {
        /// Spatial sketch containing the selected curves.
        sketch: crate::sketches::SpatialSketchId,
        /// Full-fidelity native selection records in path order.
        #[serde(deserialize_with = "deserialize_selection_selections")]
        selections: NativeSelections,
    },
    /// Ordered selected curves from one neutral spatial sketch.
    SpatialSketchCurves {
        /// Spatial sketch containing every selected curve.
        sketch: crate::sketches::SpatialSketchId,
        /// Selected curve identities in source order.
        #[serde(deserialize_with = "deserialize_selection_curves")]
        curves: SelectionMembers<crate::sketches::SpatialSketchEntityId>,
    },
    /// Path resolved as ordered topological edges.
    Edges(Vec<EdgeId>),
    /// Path resolved as ordered geometric curves.
    Curves(Vec<CurveId>),
    /// Path resolved as ordered edges in the consuming feature's input topology.
    HistoricalEdges {
        /// Input topology containing every path edge.
        state: FeatureInputTopologyId,
        /// State-local edge identities in path order.
        #[serde(deserialize_with = "deserialize_selection_edges")]
        edges: SelectionMembers<HistoricalEdgeId>,
        /// Full-fidelity source path selection.
        #[serde(deserialize_with = "deserialize_selection_native")]
        native: NonBlankString,
    },
}

impl PlanarProfileRef {
    /// Admits nonempty distinct profile regions in one sketch.
    pub fn sketch_regions(
        sketch: crate::sketches::SketchId,
        regions: Vec<SketchProfileRegion>,
    ) -> Result<Self, &'static str> {
        Ok(Self::SketchRegions {
            sketch,
            regions: regions.try_into()?,
        })
    }

    /// Admit nonempty distinct members.
    pub fn sketch_profiles(
        sketch: crate::sketches::SketchId,
        profiles: Vec<u32>,
    ) -> Result<Self, BodySelectionError> {
        let profiles = SelectionMembers::try_from(profiles)?;
        Ok(Self::SketchProfiles { sketch, profiles })
    }

    /// Admit nonempty distinct members.
    pub fn sketch_entities(
        sketch: crate::sketches::SketchId,
        entities: Vec<crate::sketches::SketchEntityId>,
    ) -> Result<Self, BodySelectionError> {
        let entities = SelectionMembers::try_from(entities)?;
        Ok(Self::SketchEntities { sketch, entities })
    }

    /// Admit nonempty distinct members.
    pub fn sketch_selection(
        sketch: crate::sketches::SketchId,
        selections: Vec<String>,
    ) -> Result<Self, BodySelectionError> {
        let selections = NativeSelections::try_from(selections)?;
        Ok(Self::SketchSelection { sketch, selections })
    }

    /// Admit nonempty distinct members.
    pub fn historical_faces(
        state: FeatureInputTopologyId,
        faces: Vec<HistoricalFaceId>,
        native: Vec<String>,
    ) -> Result<Self, BodySelectionError> {
        let faces = SelectionMembers::try_from(faces)?;
        let native = NativeSelections::try_from(native)?;
        Ok(Self::HistoricalFaces {
            state,
            faces,
            native,
        })
    }
    /// Admits generated profile curves and their native reference.
    pub fn generated(
        curves: Vec<GeneratedCurveRef>,
        native: String,
    ) -> Result<Self, BodySelectionError> {
        let curves = curves.try_into()?;
        SelectionReference::new(native).map(|native| Self::Generated { curves, native })
    }
}

impl ProfileRef {
    /// Admit nonempty distinct members.
    pub fn spatial_sketch_profiles(
        sketch: crate::sketches::SpatialSketchId,
        profiles: Vec<u32>,
    ) -> Result<Self, BodySelectionError> {
        let profiles = SelectionMembers::try_from(profiles)?;
        Ok(Self::SpatialSketchProfiles { sketch, profiles })
    }

    /// Admit nonempty distinct members.
    pub fn spatial_sketch_selection(
        sketch: crate::sketches::SpatialSketchId,
        selections: Vec<String>,
    ) -> Result<Self, BodySelectionError> {
        let selections = NativeSelections::try_from(selections)?;
        Ok(Self::SpatialSketchSelection { sketch, selections })
    }
}

impl PathRef {
    /// Admit nonempty distinct members.
    pub fn sketch_curves(
        sketch: crate::sketches::SketchId,
        curves: Vec<crate::sketches::SketchEntityId>,
    ) -> Result<Self, BodySelectionError> {
        let curves = SelectionMembers::try_from(curves)?;
        Ok(Self::SketchCurves { sketch, curves })
    }

    /// Admit nonempty distinct members.
    pub fn spatial_sketch_curves(
        sketch: crate::sketches::SpatialSketchId,
        curves: Vec<crate::sketches::SpatialSketchEntityId>,
    ) -> Result<Self, BodySelectionError> {
        let curves = SelectionMembers::try_from(curves)?;
        Ok(Self::SpatialSketchCurves { sketch, curves })
    }

    /// Admit nonempty distinct members.
    pub fn historical_edges(
        state: FeatureInputTopologyId,
        edges: Vec<HistoricalEdgeId>,
        native: String,
    ) -> Result<Self, BodySelectionError> {
        let edges = SelectionMembers::try_from(edges)?;

        let native = NonBlankString::new(native).ok_or(BodySelectionError::BlankNativeMember)?;
        Ok(Self::HistoricalEdges {
            state,
            edges,
            native,
        })
    }
}

impl GeneratedCurveRef {
    /// Admit a feature-local persistent identity.
    pub fn new(feature: FeatureId, local_id: String) -> Result<Self, BodySelectionError> {
        SelectionReference::new(local_id).map(|local_id| Self { feature, local_id })
    }
}

/// Three distinct vertex targets with a common historical input state.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(transparent)]
pub struct ThreePointSelection(pub(super) Box<[VertexSelection; 3]>);

impl TryFrom<Box<[VertexSelection; 3]>> for ThreePointSelection {
    type Error = &'static str;
    /// Admit three distinct vertex targets that use one historical input state.
    fn try_from(points: Box<[VertexSelection; 3]>) -> Result<Self, Self::Error> {
        for (first, second) in [(0, 1), (0, 2), (1, 2)] {
            if selection_overlap::vertex_targets_equal(&points[first], &points[second]) {
                return Err("points must select three distinct vertex targets");
            }
        }
        let mut state: Option<&FeatureInputTopologyId> = None;
        for point in points.iter() {
            if let VertexSelection::Historical {
                state: candidate, ..
            } = point
            {
                match state {
                    Some(state) if state.as_str() != candidate.as_str() => {
                        return Err("points must use the same input topology");
                    }
                    Some(_) => {}
                    None => state = Some(candidate),
                }
            }
        }
        Ok(Self(points))
    }
}

impl std::ops::Deref for ThreePointSelection {
    type Target = [VertexSelection; 3];
    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl<'de> Deserialize<'de> for ThreePointSelection {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        Self::try_from(Box::<[VertexSelection; 3]>::deserialize(deserializer)?)
            .map_err(serde::de::Error::custom)
    }
}

/// A nonempty source path without NUL characters.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(transparent)]
pub struct GeometryImportPath(pub(super) String);

impl TryFrom<String> for GeometryImportPath {
    type Error = &'static str;
    fn try_from(path: String) -> Result<Self, Self::Error> {
        if path.is_empty() || path.contains('\0') {
            return Err("path must be nonempty and contain no NUL");
        }
        Ok(Self(path))
    }
}

impl std::ops::Deref for GeometryImportPath {
    type Target = str;
    fn deref(&self) -> &str {
        &self.0
    }
}

impl<'de> Deserialize<'de> for GeometryImportPath {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        Self::try_from(String::deserialize(deserializer)?).map_err(serde::de::Error::custom)
    }
}
