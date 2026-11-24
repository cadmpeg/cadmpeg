// SPDX-License-Identifier: Apache-2.0
//! Neutral construction-feature taxonomy.

use std::collections::BTreeMap;

use crate::assets::AssetId;
use crate::ids::ProductDefinitionId;
use crate::ids::{BodyId, OccurrenceId};

use crate::products::JointId;
use crate::scalar::{
    Angle, FiniteReal, Fraction, InteriorAngle, Length, NonNegativeLength, NonZeroLength,
    PositiveLength, PositiveReal, SlopeAngle,
};
use crate::transform::Transform;
use crate::units::UnitVector3;
use cadmpeg_core::text::NonBlankString;
use cadmpeg_derive::DeserializeName;
#[cfg(feature = "schema")]
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

macro_rules! selection_field_deserializer {
    ($name:ident, $field:literal) => {
        fn $name<'de, D, T>(deserializer: D) -> Result<T, D::Error>
        where
            D: serde::Deserializer<'de>,
            T: Deserialize<'de>,
        {
            T::deserialize(deserializer)
                .map_err(|error| serde::de::Error::custom(format!("{}: {error}", $field)))
        }
    };
}

pub mod binders;
pub mod direct_edit;
pub mod evaluation;
pub mod extents;
pub mod geometry;
pub mod operation_settings;
pub mod parameters;
pub mod primitives;
pub mod profiles;
pub mod revolve;
pub mod selections;
pub mod sheet_metal_forms;
pub mod surface_inputs;
pub mod sweeps;
pub mod tree;

use binders::{BinderConstruction, BinderOffset, BinderSource, BinderTarget};
use datums::{DatumPlaneReference, DatumPointConstruction, DecalMapping};
use direct_edit::{
    AxisAngle, DraftAnchor, DraftPull, DraftSecondSide, FaceMotion, FlexForm, FlexMode,
    ScaleCenter, ScaleFactors, SplitBodyTool, SplitFaceTool, ThickenSide,
};
use evaluation::FeatureEvaluation;
use extents::{
    ExtrudeDirection, ExtrudeExtent, ExtrudeStart, ExtrusionDirectionSource, LinearTermination,
    PlaneTargetSource,
};
use geometry::{
    FeatureCircularArc, FeatureCoordinateFrame, FeatureDatumPlaneFrame, FeatureDirection3,
    FeatureEllipticArc, FeatureEquationCurve, FeatureImageBounds, FeatureLineSegment,
    FeaturePolyline, FeatureRigidPlacement, FeatureSupportPlaneFrame, FeatureUnitPlaneFrame,
    FinitePoint3, FiniteVector3,
};
use operation_settings::{
    BodyRetentionMode, BodyTrimSide, BooleanKind, BooleanOp, CombineDestination, FuzzyTolerance,
    GeometryImportFormat, RuledCurveOrientation, ShellJoin, ShellMode, SolidFeatureResult,
};
use parameters::{ConfigurationEvaluation, ConfigurationFeatureState, PolygonSideCount};
use primitives::PrimitiveSolid;
use profiles::{
    GeometryImportPath, LoftSection, PathRef, PlanarProfileRef, ProfileRef, ThreePointSelection,
};
use revolve::{
    RevolutionAxis, RevolutionFuseOrder, RevolveConstruction, RevolveExtent, RibConstruction,
};
use selections::{
    BodySelection, DistinctMembers, EdgeSelection, FaceSelection, InsertedBodies,
    InsertedComponents, NonEmptyMembers, SelectionReference,
};
use sheet_metal_forms::{
    SheetMetalCornerEffect, SheetMetalHemBendPosition, SheetMetalHemDirection, SheetMetalHemForm,
    SheetMetalRelief, SheetMetalThicknessSide, SheetMetalTransition,
};
use surface_inputs::{
    deserialize_continuity, CurveProjectionDirection, FilledSurfaceContinuity, RuledSurfaceCorner,
    RuledSurfaceMode, SurfaceBoundary, SurfaceExtension, SurfaceProjectionMode, TrimRegion,
    WrapMode,
};
use sweeps::{
    CoilConstruction, FaceMaker, HelicalSweepConstruction, HelixConstructionStyle, HelixShape,
    InnerWireTaper, SweepGuideRail, SweepOrientation, SweepPathExtent, SweepShape,
    SweepTransformation, SweepTransition,
};
use tree::{CapturedComponentPosition, ComponentTransferMode, FeatureTreeNodeRole, TreeChildren};

mod body_selection;
mod decode_clone;
mod membership;
mod operation;
mod selection_overlap;
mod source_literals;

/// Boundary fill cell selections.
pub mod boundary_fill;
use boundary_fill::BoundaryFillCell;

#[doc = "Checked datum geometry and construction inputs."]
pub mod datums;
pub mod edge_treatments;
use edge_treatments::{ChamferGroup, FilletGroup, FullRoundFilletGroup, RadiusSpec};

#[doc = include_str!("features/form-api.md")]
pub mod form;

pub mod holes;
use holes::{HoleBottom, HolePlacement, HoleProfileFilter, HoleShape};

#[doc = "Mesh source selections."]
pub mod mesh;

#[doc = "Sheet-metal construction inputs and controls."]
pub mod sheet_metal;
use sheet_metal::SheetMetalBendLinesConstruction;

pub mod patterns;
use patterns::{PatternKind, PatternSeed};

#[doc = "Face replacement operands and tangent-chain settings."]
pub mod replace_face;
use replace_face::ReplaceFaceOperands;

#[doc = "Surface operation settings and constructors."]
pub mod surfaces;
use surfaces::{SilhouetteSplitMode, SplitFaceMethod, UntrimLoops};

const EPS_FEATURE_UNIT_FRAME: f64 = 1.0e-9;

const EPS_FEATURE_PLANE_ORTHOGONAL: f64 = 1.0e-9;

const EPS_FEATURE_ELLIPSE_AXES_ORTHO: f64 = 1.0e-9;

/// Relative bound on the sine between a draft pull direction and its plane-frame normal.
const EPS_DRAFT_PULL_FRAME_PARALLEL: f64 = 1.0e-9;

crate::ids::id_type!(
    /// Identifies a neutral construction feature.
    FeatureId, compose, into_string, key
);

crate::ids::id_type!(
    /// Identifies a neutral design configuration.
    ConfigurationId, compose
);

crate::ids::id_type!(
    /// Identifies a neutral design parameter.
    ParameterId, compose, key
);

crate::units::named_field!(deserialize_parameter_real, FiniteReal, "value");

fn deserialize_dependencies<'de, D, T>(deserializer: D) -> Result<DistinctMembers<T>, D::Error>
where
    D: serde::Deserializer<'de>,
    T: Deserialize<'de> + Eq + std::hash::Hash,
{
    crate::units::deserialize_named(deserializer, "dependencies")
}

/// An ordered neutral construction feature and its resulting bodies.
///
/// Construction requires an admitted identity and feature evaluation. There is
/// no public [`Default`]: an empty id with an arbitrary definition is illegal.
#[derive(Debug, Clone, PartialEq)]
pub struct Feature {
    /// Product definition whose frame contains this feature.
    pub product_definition: Option<ProductDefinitionId>,
    /// Globally unique feature id.
    pub id: FeatureId,
    /// Stable construction order within the source history.
    pub ordinal: u64,
    /// Source display name.
    pub name: Option<String>,
    /// Whether evaluation of this feature is disabled.
    pub suppressed: Option<bool>,
    /// Earlier features consumed during regeneration, in source operand order.
    pub dependencies: DistinctMembers<FeatureId>,
    /// Source operation attributes not consumed by the neutral definition.
    pub source_properties: BTreeMap<NonBlankString, String>,
    /// Source XML element name for the operation record.
    pub source_tag: Option<String>,
    /// Text payload of a source leaf operation.
    pub source_text: Option<String>,
    /// Ordered source text, parameter, and child-feature content.
    pub source_content: FeatureContent,
    /// Construction semantics and their admitted resulting bodies.
    pub evaluation: FeatureEvaluation,
    /// Identifier of the full-fidelity record in a native namespace.
    pub native_ref: Option<String>,
}

/// One row of the model route's feature list.
///
/// `regeneration_parent` is a model-route datum: the read hands it to
/// [`crate::document::Model`] with the feature, which is the only owner that
/// can place the edge. The standalone route reads [`FeatureReadWire`], which
/// declares no such field, so the difference between the two routes is a
/// field, not a check.
#[derive(Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
pub(crate) struct FeatureRowWire {
    #[serde(default, deserialize_with = "deserialize_product_definition")]
    product_definition: Option<ProductDefinitionId>,
    id: FeatureId,
    ordinal: u64,
    #[serde(default, deserialize_with = "deserialize_name")]
    name: Option<String>,
    #[serde(default, deserialize_with = "deserialize_feature_row_wire_suppressed")]
    suppressed: Option<bool>,
    #[serde(default, deserialize_with = "deserialize_regeneration_parent")]
    regeneration_parent: Option<FeatureId>,
    #[serde(deserialize_with = "deserialize_dependencies")]
    dependencies: DistinctMembers<FeatureId>,
    #[serde(deserialize_with = "cadmpeg_core::distinct_keys::btree_map")]
    source_properties: BTreeMap<NonBlankString, String>,
    #[serde(default, deserialize_with = "deserialize_source_tag")]
    source_tag: Option<String>,
    #[serde(default, deserialize_with = "deserialize_source_text")]
    source_text: Option<String>,
    source_content: FeatureContent,
    #[cfg_attr(feature = "schema", schemars(with = "Vec<BodyId>"))]
    outputs: DistinctMembers<BodyId>,
    definition: FeatureDefinition,
    #[serde(default, deserialize_with = "deserialize_native_ref")]
    native_ref: Option<String>,
}

impl FeatureRowWire {
    pub(crate) fn into_parts(self) -> (Feature, Option<FeatureId>) {
        let regeneration_parent = self.regeneration_parent;
        let feature = FeatureReadWire {
            product_definition: self.product_definition,
            id: self.id,
            ordinal: self.ordinal,
            name: self.name,
            suppressed: self.suppressed,
            dependencies: self.dependencies,
            source_properties: self.source_properties,
            source_tag: self.source_tag,
            source_text: self.source_text,
            source_content: self.source_content,
            outputs: self.outputs,
            definition: self.definition,
            native_ref: self.native_ref,
        }
        .into_feature();
        (feature, regeneration_parent)
    }
}

/// The standalone feature wire: one feature outside any model.
///
/// It declares no `regeneration_parent`, so a row of the model route does not
/// read back as a standalone feature and the parent edge cannot arrive without
/// the model that owns it. `deny_unknown_fields` states the refusal.
#[derive(Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
struct FeatureReadWire {
    #[serde(default, deserialize_with = "deserialize_product_definition")]
    product_definition: Option<ProductDefinitionId>,
    id: FeatureId,
    ordinal: u64,
    #[serde(default, deserialize_with = "deserialize_name")]
    name: Option<String>,
    #[serde(default, deserialize_with = "deserialize_suppressed")]
    suppressed: Option<bool>,
    #[serde(deserialize_with = "deserialize_dependencies")]
    dependencies: DistinctMembers<FeatureId>,
    #[serde(deserialize_with = "cadmpeg_core::distinct_keys::btree_map")]
    source_properties: BTreeMap<NonBlankString, String>,
    #[serde(default, deserialize_with = "deserialize_source_tag")]
    source_tag: Option<String>,
    #[serde(default, deserialize_with = "deserialize_source_text")]
    source_text: Option<String>,
    source_content: FeatureContent,
    #[cfg_attr(feature = "schema", schemars(with = "Vec<BodyId>"))]
    outputs: DistinctMembers<BodyId>,
    definition: FeatureDefinition,
    #[serde(default, deserialize_with = "deserialize_native_ref")]
    native_ref: Option<String>,
}

impl FeatureReadWire {
    fn into_feature(self) -> Feature {
        Feature {
            product_definition: self.product_definition,
            id: self.id,
            ordinal: self.ordinal,
            name: self.name,
            suppressed: self.suppressed,
            dependencies: self.dependencies,
            source_properties: self.source_properties,
            source_tag: self.source_tag,
            source_text: self.source_text,
            source_content: self.source_content,
            evaluation: FeatureEvaluation::new(self.definition, self.outputs),
            native_ref: self.native_ref,
        }
    }
}

/// Read one feature outside any model.
///
/// The model route reads `FeatureRowWire` and splits the row with `into_parts`.
/// Standalone reconstruction reads the same feature fields without a parent edge.
impl<'de> Deserialize<'de> for Feature {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        Ok(FeatureReadWire::deserialize(deserializer)?.into_feature())
    }
}

#[cfg(feature = "schema")]
impl JsonSchema for Feature {
    fn schema_name() -> std::borrow::Cow<'static, str> {
        "Feature".into()
    }

    fn json_schema(generator: &mut schemars::SchemaGenerator) -> schemars::Schema {
        FeatureReadWire::json_schema(generator)
    }
}

/// Ordered source content with distinct parameter and child-feature references.
#[derive(Debug, Clone, Default, PartialEq, Serialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(transparent)]
pub struct FeatureContent(Vec<FeatureSourceContent>);

impl TryFrom<Vec<FeatureSourceContent>> for FeatureContent {
    type Error = FeatureCollectionError;
    fn try_from(value: Vec<FeatureSourceContent>) -> Result<Self, Self::Error> {
        Self::build(value).map_err(FeatureCollectionError::Invalid)
    }
}
impl FeatureContent {
    fn build(value: Vec<FeatureSourceContent>) -> Result<Self, &'static str> {
        if !membership::distinct(&value, |item| {
            !matches!(item, FeatureSourceContent::Text(_))
        }) {
            return Err("source_content repeats a parameter or child-feature reference");
        }
        Ok(Self(value))
    }

    /// Constructs text-only content in source order, retaining repeated text.
    pub fn text(values: impl IntoIterator<Item = String>) -> Self {
        Self(values.into_iter().map(FeatureSourceContent::Text).collect())
    }

    /// Append decoded content without repeating a parameter or child-feature reference.
    pub fn push(&mut self, value: FeatureSourceContent) -> Result<(), FeatureCollectionError> {
        if !matches!(value, FeatureSourceContent::Text(_)) {
            for member in &self.0 {
                if member == &value {
                    return Err(FeatureCollectionError::Invalid(
                        "source_content repeats a parameter or child-feature reference",
                    ));
                }
            }
        }

        self.0.push(value);
        Ok(())
    }

    /// Whether the sequence has no content.
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
}

impl std::ops::Deref for FeatureContent {
    type Target = [FeatureSourceContent];
    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl<'a> IntoIterator for &'a FeatureContent {
    type Item = &'a FeatureSourceContent;
    type IntoIter = std::slice::Iter<'a, FeatureSourceContent>;
    fn into_iter(self) -> Self::IntoIter {
        self.0.iter()
    }
}

impl<'de> Deserialize<'de> for FeatureContent {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let values = Vec::<FeatureSourceContent>::deserialize(deserializer)?;
        Self::build(values).map_err(serde::de::Error::custom)
    }
}
/// One item in a source feature's mixed-content sequence.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, cadmpeg_derive::DeserializeTagged)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(tag = "kind", content = "value", rename_all = "snake_case")]
#[serde(deny_unknown_fields)]
pub enum FeatureSourceContent {
    /// Literal text between child records.
    Text(String),
    /// Dimension or equation parameter at this position.
    Parameter(ParameterId),
    /// Nested feature record at this position.
    Feature(FeatureId),
}

/// Source-native feature family retained without neutral construction semantics.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum NativeFeatureKind {
    /// Fusion reference-image operation.
    Canvas,
    /// Fusion decal operation.
    Decal,
    /// Fusion draft operation.
    Draft,
    /// Fusion edge-fillet operation.
    Fillet,
    /// Fusion chamfer operation.
    Chamfer,
    /// Fusion extrusion operation.
    Extrude,
    /// Fusion direct face deletion.
    DeleteFace,
    /// Fusion surface face deletion.
    SurfaceDeleteFace,
    /// Source-native family without typed neutral handling.
    Other(String),
}

impl NativeFeatureKind {
    /// Stable source spelling carried on the CADIR wire.
    pub fn as_str(&self) -> &str {
        match self {
            Self::Canvas => "Canvas",
            Self::Decal => "Decal",
            Self::Draft => "Draft",
            Self::Fillet => "Fillet",
            Self::Chamfer => "Chamfer",
            Self::Extrude => "Extrude",
            Self::DeleteFace => "DeleteFace",
            Self::SurfaceDeleteFace => "SurfaceDeleteFace",
            Self::Other(value) => value,
        }
    }
}

impl From<String> for NativeFeatureKind {
    fn from(value: String) -> Self {
        match value.as_str() {
            "Canvas" => Self::Canvas,
            "Decal" => Self::Decal,
            "Draft" => Self::Draft,
            "Fillet" => Self::Fillet,
            "Chamfer" => Self::Chamfer,
            "Extrude" => Self::Extrude,
            "DeleteFace" => Self::DeleteFace,
            "SurfaceDeleteFace" => Self::SurfaceDeleteFace,
            _ => Self::Other(value),
        }
    }
}

impl From<&str> for NativeFeatureKind {
    fn from(value: &str) -> Self {
        value.to_owned().into()
    }
}

impl std::fmt::Display for NativeFeatureKind {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(self.as_str())
    }
}

impl Serialize for NativeFeatureKind {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        serializer.serialize_str(self.as_str())
    }
}

impl<'de> Deserialize<'de> for NativeFeatureKind {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        Ok(String::deserialize(deserializer)?.into())
    }
}

#[cfg(feature = "schema")]
impl JsonSchema for NativeFeatureKind {
    fn schema_name() -> std::borrow::Cow<'static, str> {
        "NativeFeatureKind".into()
    }

    fn json_schema(generator: &mut schemars::SchemaGenerator) -> schemars::Schema {
        String::json_schema(generator)
    }
}

/// Guide semantics of a loft operation.
#[derive(Debug, Clone, PartialEq, Serialize, cadmpeg_derive::DeserializeTagged)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(
    tag = "kind",
    content = "value",
    rename_all = "snake_case",
    deny_unknown_fields
)]
pub enum LoftGuidance {
    /// Ordered guide trajectories; an empty vector means unguided.
    Guides(Vec<PathRef>),
    /// Centerline to which loft sections remain normal.
    Centerline(PathRef),
}

impl Default for LoftGuidance {
    fn default() -> Self {
        Self::Guides(Vec::new())
    }
}

/// A body selection with at least two members when membership is resolved.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(transparent)]
pub struct SewBodySelection(BodySelection);

impl std::ops::Deref for SewBodySelection {
    type Target = BodySelection;
    fn deref(&self) -> &BodySelection {
        &self.0
    }
}

impl AsRef<BodySelection> for SewBodySelection {
    fn as_ref(&self) -> &BodySelection {
        &self.0
    }
}

impl<'de> Deserialize<'de> for SewBodySelection {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        Self::try_from(BodySelection::deserialize(deserializer)?).map_err(serde::de::Error::custom)
    }
}

macro_rules! selection_operands {
    ($name:ident, $wire:ident, $selection:ty, $first:ident, $second:ident, $arity:expr, $overlap:ident $(, $edit:ident)?) => {
        /// Two admitted operand selections for one feature operation.
        #[derive(Debug, Clone, PartialEq, Serialize)]
        #[cfg_attr(feature = "schema", derive(JsonSchema))]
        pub struct $name {
            $first: $selection,
            $second: $selection,
        }

        #[derive(Deserialize)]
        #[serde(deny_unknown_fields)]
        struct $wire {
            $first: $selection,
            $second: $selection,
        }

        impl $name {
            /// Admit the operand arity and disjoint membership.
            pub fn new(
                $first: $selection,
                $second: $selection,
            ) -> Result<Self, &'static str> {
                if !($arity)(&$first) || selection_overlap::$overlap(&$first, &$second) {
                    return Err(concat!(
                        stringify!($first),
                        " and ",
                        stringify!($second),
                        " must have valid arity and disjoint membership"
                    ));
                }
                Ok(Self { $first, $second })
            }

            /// Return the first operand selection.
            pub fn $first(&self) -> &$selection {
                &self.$first
            }

            /// Return the second operand selection.
            pub fn $second(&self) -> &$selection {
                &self.$second
            }

            $(
            /// Admit both edited selections before replacing either operand.
            pub fn $edit(
                &mut self,
                edit: impl FnOnce(&mut $selection, &mut $selection),
            ) -> Result<(), &'static str> {
                let mut first = self.$first.clone();
                let mut second = self.$second.clone();
                edit(&mut first, &mut second);
                *self = Self::new(first, second)?;
                Ok(())
            }
            )?
        }

        impl<'de> Deserialize<'de> for $name {
            fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
                let wire = $wire::deserialize(deserializer)?;
                Self::new(wire.$first, wire.$second).map_err(serde::de::Error::custom)
            }
        }
    };
}

selection_operands!(
    FaceBlendOperands,
    FaceBlendOperandsWire,
    FaceSelection,
    first_faces,
    second_faces,
    |_: &FaceSelection| true,
    face_selections_overlap
);

selection_operands!(
    SectionOperands,
    SectionOperandsWire,
    BodySelection,
    first,
    second,
    |_: &BodySelection| true,
    body_selections_overlap
);
selection_operands!(
    CombineOperands,
    CombineOperandsWire,
    BodySelection,
    target,
    tools,
    |first| body_selection::known_body_count(first).is_none_or(|count| count == 1),
    body_selections_overlap,
    try_edit
);
selection_operands!(
    TrimBodyOperands,
    TrimBodyOperandsWire,
    BodySelection,
    targets,
    tools,
    |_: &BodySelection| true,
    body_selections_overlap
);

impl CombineOperands {
    /// Move both selections out of this admitted pair.
    pub fn into_parts(self) -> (BodySelection, BodySelection) {
        (self.target, self.tools)
    }
}

/// A feature operation: neutral construction semantics with an explicit native escape hatch, and no post-processing layer of its own.
#[derive(Debug, Clone, PartialEq, Serialize, cadmpeg_derive::DeserializeTagged)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(
    tag = "kind",
    content = "value",
    rename_all = "snake_case",
    deny_unknown_fields
)]
pub enum FeatureOperation {
    /// Non-modeling node retained in the ordered feature tree.
    TreeNode {
        /// Structural or presentation role of the node.
        role: FeatureTreeNodeRole,
        /// Ordered child membership and its optional active child.
        children: TreeChildren,
    },
    /// Direct-modeling session represented by its captured result bodies.
    BaseFeature {
        /// Bodies copied into the parametric timeline when the session closed.
        bodies: BodySelection,
    },
    /// Mesh geometry imported into the parametric timeline.
    MeshImport {
        #[doc = "Resolved tessellations or a retained mesh source."]
        #[serde(deserialize_with = "deserialize_local_tessellations")]
        tessellations: mesh::MeshSelection,
    },
    #[doc = "Generate a mesh face-group partition from independently retained controls."]
    MeshGenerateFaceGroups {
        #[doc = "Mesh and optional group selection in its source history state."]
        #[serde(deserialize_with = "deserialize_mesh_source")]
        source: mesh::MeshSelection,
        #[doc = "Decoded controls, including stored inactive values."]
        settings: mesh::MeshFaceGroupGenerationSettings,
    },
    #[doc = "Combine selected groups of a mesh face-group partition."]
    MeshCombineFaceGroups {
        #[doc = "Mesh and ordered groups in their source history state."]
        #[serde(deserialize_with = "deserialize_mesh_source")]
        source: mesh::MeshSelection,
    },
    #[doc = "Planar section of a mesh whose curves belong to a sketch."]
    MeshSection {
        #[doc = "Sectioned mesh."]
        source: mesh::MeshSelection,
        #[doc = "Model-space section plane, when resolved."]
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            deserialize_with = "deserialize_section_plane"
        )]
        plane: Option<FeatureSupportPlaneFrame>,
        #[doc = "Sketch feature that receives the section curves, when resolved."]
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            deserialize_with = "deserialize_section_sketch"
        )]
        sketch: Option<FeatureId>,
    },
    #[doc = "Change applied to mesh geometry."]
    MeshModify {
        #[doc = "Mesh that the operation changes."]
        source: mesh::MeshSelection,
        #[doc = "Applied change."]
        modification: mesh::MeshModification,
    },
    #[doc = "Conversion of meshes to solid or surface bodies."]
    MeshConvert(mesh::MeshConversion),
    /// Independent bodies introduced by a copy-and-paste operation. The copies
    /// are the feature's outputs, so the selection carries only its native
    /// expression.
    InsertBodies {
        /// Native selection expression of the copied bodies.
        bodies: InsertedBodies,
    },
    /// External component occurrence introduced into an assembly timeline.
    InsertComponent {
        /// Placed occurrence created by this history operation.
        occurrence: OccurrenceId,
    },
    #[doc = "Component occurrences introduced by one construction feature."]
    InsertComponents {
        #[doc = "Resolved membership and its retained native selection."]
        components: InsertedComponents,
    },
    #[doc = "Component occurrence copied or transferred between local owners."]
    ComponentTransfer {
        #[doc = "Whether the feature copies an occurrence or changes its owner."]
        mode: ComponentTransferMode,
        #[doc = "Transferred occurrence when its persistent identity resolves."]
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            deserialize_with = "deserialize_transfer_occurrence"
        )]
        occurrence: Option<OccurrenceId>,
        #[doc = "Reusable component definition when its identity resolves."]
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            deserialize_with = "deserialize_transfer_component"
        )]
        component: Option<ProductDefinitionId>,
        #[doc = "Source containing definition when it resolves."]
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            deserialize_with = "deserialize_transfer_source_owner"
        )]
        source_owner: Option<ProductDefinitionId>,
        #[doc = "Destination containing definition when it resolves."]
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            deserialize_with = "deserialize_transfer_target_owner"
        )]
        target_owner: Option<ProductDefinitionId>,
        #[doc = "Source local placement when the operation stores one."]
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            deserialize_with = "deserialize_transfer_source_transform"
        )]
        source_transform: Option<Transform>,
        #[doc = "Destination local placement when the operation stores one."]
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            deserialize_with = "deserialize_transfer_target_transform"
        )]
        target_transform: Option<Transform>,
    },
    #[doc = "Independent captured occurrence poses in source order, including repeated selectors."]
    CapturedComponentPositions {
        #[doc = "Exactly the captured rows, with an empty capture represented by an empty sequence."]
        positions: Vec<CapturedComponentPosition>,
    },
    /// Assembly constraint introduced into the product structure.
    AssemblyJoint {
        /// Joint created by this history operation.
        joint: JointId,
    },
    /// Freeform modeling session represented by its final subdivision cages.
    Form(form::FormConstruction),
    /// Thread attached to a cylindrical face.
    ///
    /// The source may also model the helical form in the body geometry;
    /// `modeled` states whether it does, and absent means unproven. A field
    /// that the source encoding does not prove stays absent. F3D threads set
    /// `standard`, `designation`, `pitch` and `modeled`;
    /// the `nx`, `sldprt`, and `inventor` codecs do not set the four new fields.
    CosmeticThread {
        /// Cylindrical face carrying the thread.
        face: FaceSelection,
        /// Thread standard family or profile name, as the source spells it.
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            deserialize_with = "deserialize_thread_standard"
        )]
        standard: Option<NonBlankString>,
        /// Size designation within the standard, as the source spells it.
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            deserialize_with = "deserialize_thread_designation"
        )]
        designation: Option<NonBlankString>,
        /// Nominal thread diameter, when resolved.
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            deserialize_with = "deserialize_diameter"
        )]
        diameter: Option<PositiveLength>,
        /// Thread pitch along the axis, when resolved.
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            deserialize_with = "deserialize_thread_pitch"
        )]
        pitch: Option<PositiveLength>,
        /// Whether the body geometry already contains the helical form.
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            deserialize_with = "deserialize_thread_modeled"
        )]
        modeled: Option<bool>,
        /// Axial extent of the thread, when resolved.
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            deserialize_with = "deserialize_extent"
        )]
        extent: Option<CosmeticThreadExtent>,
    },
    /// Raster reference image placed in model space.
    ReferenceImage {
        /// Embedded or external raster resource.
        asset: AssetId,
        /// Whether the raster is visible in the source presentation.
        visible: bool,
        /// Whether image u increases toward decreasing plane-local u.
        mirror_u: bool,
        /// Whether image v increases toward decreasing plane-local v.
        mirror_v: bool,
        /// Finite image plane with perpendicular unit directions.
        frame: FeatureUnitPlaneFrame,
        /// Opposite corners with nonzero extent in both coordinates.
        bounds: FeatureImageBounds,
        /// Normalized image opacity.
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            deserialize_with = "deserialize_opacity"
        )]
        opacity: Option<Fraction>,
    },
    /// Raster image applied to model faces.
    Decal {
        /// Embedded or external raster resource.
        asset: AssetId,
        /// Faces receiving the raster image.
        faces: FaceSelection,
        /// Rule relating image coordinates to the selected faces.
        mapping: DecalMapping,
        /// Normalized image opacity, or the source format's default when absent.
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            deserialize_with = "deserialize_opacity"
        )]
        opacity: Option<Fraction>,
    },
    /// Built-in world-origin reference plane.
    DatumPrincipalPlane {
        /// Canonical principal-plane role.
        plane: PrincipalPlane,
    },
    /// Constructed reference plane.
    DatumPlane {
        /// Finite plane with nonzero perpendicular directions.
        frame: FeatureDatumPlaneFrame,
    },
    /// Reference plane constructed through three selected vertices.
    DatumThreePointPlane {
        /// Finite plane with nonzero perpendicular directions.
        frame: FeatureDatumPlaneFrame,
        /// Construction vertices in source order.
        points: ThreePointSelection,
    },
    /// Reference plane offset from another datum plane.
    DatumOffsetPlane {
        /// Source plane or planar face, when its identity is available.
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            deserialize_with = "deserialize_reference"
        )]
        reference: Option<DatumPlaneReference>,
        /// Signed normal offset from the source plane.
        distance: Length,
    },
    /// Constructed reference axis.
    DatumAxis {
        #[doc = "Readable geometry, construction inputs and source options."]
        axis: datums::DatumAxis,
    },
    /// Constructed reference point.
    DatumPoint {
        /// Point position in model space.
        position: FinitePoint3,
        /// Rule that derives the point from preceding construction geometry.
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            deserialize_with = "deserialize_construction"
        )]
        construction: Option<Box<DatumPointConstruction>>,
    },
    /// Standalone model vertex constructed at one point.
    PointGeometry {
        /// Vertex position in the feature's local construction frame.
        position: FinitePoint3,
    },
    /// Straight edge between two finite points.
    LineSegment {
        /// Finite distinct endpoints.
        segment: FeatureLineSegment,
    },
    /// Circular edge over an angular interval.
    CircularArc {
        /// Finite arc geometry and directed angular span.
        arc: FeatureCircularArc,
    },
    /// Elliptic edge over an angular interval.
    EllipticArc {
        /// Finite arc geometry and directed angular span.
        arc: FeatureEllipticArc,
    },
    /// Ordered straight-edge chain.
    Polyline {
        /// Finite ordered chain and closure.
        chain: FeaturePolyline,
    },
    /// Regular planar polygon centered at the local origin.
    RegularPolygonCurve {
        /// Number of polygon sides.
        sides: PolygonSideCount,
        /// Center-to-vertex distance.
        circumradius: PositiveLength,
    },
    /// Rectangular bounded planar face in the local XY plane.
    PlanarPatch {
        /// Length along the local x-axis.
        length: PositiveLength,
        /// Width along the local y-axis.
        width: PositiveLength,
    },
    /// Faces built from an ordered set of source shapes.
    FaceFromShapes {
        /// Complete ordered source-shape selection.
        sources: BodySelection,
        /// Native face-building algorithm.
        #[serde(rename = "face_maker_class")]
        #[cfg_attr(feature = "schema", schemars(with = "String"))]
        face_maker: FaceMaker,
    },
    /// Constructed model-space coordinate system.
    DatumCoordinateSystem {
        /// Finite right-handed coordinate frame.
        frame: FeatureCoordinateFrame,
    },
    /// Rectangular solid primitive. The solid is the set of points of the
    /// placement origin plus `a`, `b`, and `c` times the first, second, and
    /// third dimension along the local x, y, and z axes, for `a`, `b`, and `c`
    /// in the closed unit interval. The placement origin is the box corner,
    /// not its centre.
    Block {
        /// Ordered local x, y, and z dimensions, when resolved.
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            deserialize_with = "deserialize_dimensions"
        )]
        dimensions: Option<[PositiveLength; 3]>,
        /// Local-to-model placement, when resolved.
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            deserialize_with = "deserialize_feature_operation_placement"
        )]
        placement: Option<FeatureRigidPlacement>,
        /// Whether the primitive creates or combines material.
        op: BooleanOp,
    },
    /// Parametric model-space curve defined by coordinate expressions.
    EquationCurve {
        /// Coordinate expressions and parameter domain.
        curve: FeatureEquationCurve,
    },
    /// Curve produced by projecting a source path onto target faces.
    ProjectedCurve {
        /// Sketch or model-space path being projected.
        source: PathRef,
        /// Faces receiving the projected curve.
        target_faces: FaceSelection,
        /// Direction law used by the projection.
        direction: CurveProjectionDirection,
        /// Whether projection proceeds in both directions, when resolved.
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            deserialize_with = "deserialize_bidirectional"
        )]
        bidirectional: Option<bool>,
    },
    /// Shapes projected along a direction onto a support face selection.
    ProjectOnSurface {
        /// Ordered shapes and subelements projected onto the support.
        sources: PathRef,
        /// Support face selection receiving the projection. The selection
        /// admits any number of faces.
        support_face: FaceSelection,
        /// Unit projection direction.
        direction: UnitVector3,
        /// Result topology retained from the projected shapes.
        mode: SurfaceProjectionMode,
        /// Normal extrusion height used to turn projected faces into solids.
        height: NonNegativeLength,
        /// Normal offset applied to the projected result.
        offset: Length,
    },
    /// Ordered chain of source paths exposed as one construction curve.
    CompositeCurve {
        /// Source segments in traversal order.
        #[serde(deserialize_with = "deserialize_local_segments")]
        segments: NonEmptyMembers<PathRef>,
        /// Whether the final segment joins the first.
        closed: bool,
    },
    /// Circular helix or planar spiral constructed around an axis.
    Helix {
        /// Point on the construction axis at the curve start.
        axis_origin: FinitePoint3,
        /// Construction-axis direction.
        axis_direction: FeatureDirection3,
        /// Initial radial distance from the axis.
        radius: PositiveLength,
        /// Axial or radial construction law.
        shape: HelixShape,
        /// Positive number of revolutions.
        revolutions: PositiveReal,
        /// Angular position at the curve start.
        start_angle: Angle,
        /// Whether angular travel is clockwise when viewed along the axis.
        clockwise: bool,
        /// Number of turns per generated curve subdivision, when requested.
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            deserialize_with = "deserialize_segment_turns"
        )]
        segment_turns: Option<PositiveReal>,
        /// Persisted construction algorithm generation, when selectable.
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            deserialize_with = "deserialize_construction_style"
        )]
        construction_style: Option<HelixConstructionStyle>,
    },
    /// Circular helix with retained native axis placement.
    HelixNativeAxis {
        /// Source-native record carrying the unresolved construction axis.
        #[serde(deserialize_with = "deserialize_local_axis_native_ref")]
        axis_native_ref: NonBlankString,
        /// Signed total rise along the axis.
        #[serde(alias = "radius")]
        axial_rise: Length,
        /// Signed axial rise per revolution.
        #[serde(alias = "height")]
        pitch: Length,
        /// Positive number of revolutions.
        revolutions: PositiveReal,
        /// Angular position at the curve start.
        start_angle: Angle,
        /// Whether angular travel is clockwise when viewed along the axis.
        clockwise: bool,
    },
    /// Solid primitive formed by sweeping a generated section along a helix or spiral.
    Coil {
        /// Complete geometric and parametric construction definition.
        construction: CoilConstruction,
        /// Result-body semantics.
        result: SolidFeatureResult,
    },
    /// Solid sphere primitive.
    Sphere {
        /// Sphere center in model space.
        center: FinitePoint3,
        /// Positive sphere radius.
        radius: PositiveLength,
        /// Boolean combination with existing bodies.
        op: BooleanOp,
    },
    /// Solid torus primitive.
    Torus {
        /// Torus center in model space.
        center: FinitePoint3,
        /// Unit normal of the torus center plane.
        axis: UnitVector3,
        /// Positive distance from the center to the tube centerline.
        major_radius: PositiveLength,
        /// Positive tube radius.
        minor_radius: PositiveLength,
        /// Boolean combination with existing bodies.
        op: BooleanOp,
    },
    /// Profile mapped onto a target face.
    Wrap {
        /// Sketch or face profile mapped onto the target.
        profile: PlanarProfileRef,
        /// Face receiving the mapped profile.
        face: FaceSelection,
        /// Material or imprint operation performed by the mapping.
        mode: WrapMode,
    },
    /// Solved sketch node in the construction history.
    Sketch {
        /// Source-declared sketch space and optional decoded geometry.
        sketch: SketchFeatureBinding,
    },
    /// Solved spatial-sketch node in the construction history.
    SpatialSketch {
        /// Neutral model-space sketch geometry owned by this history node, when resolved.
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            deserialize_with = "deserialize_sketch"
        )]
        sketch: Option<crate::sketches::SpatialSketchId>,
    },
    /// Reusable planar sketch geometry.
    SketchBlockDefinition {
        /// Neutral sketch geometry owned by the block, when resolved.
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            deserialize_with = "deserialize_feature_operation_sketch"
        )]
        sketch: Option<crate::sketches::SketchId>,
    },
    /// Placement of one reusable sketch-block definition.
    SketchBlockInstance {
        /// Referenced block definition, when resolved.
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            deserialize_with = "deserialize_block"
        )]
        block: Option<FeatureId>,
        /// Affine placement in the owning sketch space, when resolved.
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            deserialize_with = "deserialize_placement"
        )]
        placement: Option<crate::transform::Transform>,
    },
    /// Directly stored geometry with no replayable parametric construction.
    ///
    /// The feature's `outputs` identify the retained bodies when geometry is present.
    StoredGeometry {},
    /// Body geometry copied from existing bodies.
    ExtractBody {
        /// Bodies supplying the copied geometry.
        source: BodySelection,
    },
    /// Geometry copied from an earlier feature without an additional modeling operation.
    DerivedGeometry {
        /// Feature supplying the copied geometry.
        source: FeatureId,
    },
    /// Geometry imported from an external model file.
    ImportedGeometry {
        /// External source path exactly as persisted by the design.
        path: GeometryImportPath,
        /// Model format read from the external file.
        format: GeometryImportFormat,
    },
    /// Parametric analytic solid primitive.
    Primitive {
        /// Primitive dimensions and angular bounds.
        solid: PrimitiveSolid,
        /// Local-to-model placement of the primitive's feature frame, when
        /// resolved. Without it the feature frame is unknown.
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            deserialize_with = "deserialize_primitive_placement"
        )]
        placement: Option<FeatureRigidPlacement>,
        /// Boolean combination with an existing `PartDesign` body.
        op: BooleanOp,
    },
    /// Revolution of a profile around an axis.
    Revolve {
        /// Independently resolved construction inputs.
        construction: RevolveConstruction,
        /// Boolean combination with existing bodies.
        op: BooleanOp,
    },
    /// Sweep of a referenced or generated cross-section along a path.
    Sweep {
        /// Cross-sections and their compatible result mode.
        shape: SweepShape,
        /// Trajectory followed by the profile, when resolved.
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            deserialize_with = "deserialize_path"
        )]
        path: Option<PathRef>,
        /// Rule used to orient cross-sections along the path.
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            deserialize_with = "deserialize_orientation"
        )]
        orientation: Option<SweepOrientation>,
        /// Corner continuation used where path segments meet.
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            deserialize_with = "deserialize_transition"
        )]
        transition: Option<SweepTransition>,
        /// Interpolation law used between multiple cross-sections.
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            deserialize_with = "deserialize_transformation"
        )]
        transformation: Option<SweepTransformation>,
        /// Whether tangent-connected edges are included in the primary path.
        path_tangent: bool,
        /// Whether linear edges and planar faces are simplified after construction.
        linearize: bool,
        /// Total profile twist along the path, when specified.
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            deserialize_with = "deserialize_twist"
        )]
        twist: Option<Angle>,
        /// Fractions of the selected path swept on either side of the profile.
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            deserialize_with = "deserialize_path_extent"
        )]
        path_extent: Option<SweepPathExtent>,
        /// Guide rail and its independently consumed extent, when present.
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            deserialize_with = "deserialize_guide_rail"
        )]
        guide_rail: Option<SweepGuideRail>,
        /// Profile taper angle over the swept extent, when specified.
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            deserialize_with = "deserialize_taper"
        )]
        taper: Option<Angle>,
        /// End-to-start profile scale ratio, when specified.
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            deserialize_with = "deserialize_scale"
        )]
        scale: Option<PositiveReal>,
        /// Whether a profile containing multiple faces is accepted as one operation.
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            deserialize_with = "deserialize_allow_multi_profile_faces"
        )]
        allow_multi_profile_faces: Option<bool>,
    },
    /// Solid sweep of a profile along a parametrically defined helix or spiral.
    HelicalSweep {
        /// Complete helix path and profile construction.
        construction: HelicalSweepConstruction,
        /// Boolean combination with the existing body.
        op: BooleanOp,
    },
    /// Live or frozen reference geometry imported from other design features.
    Binder {
        /// Ordered source objects and selected subelements.
        sources: Vec<BinderSource>,
        /// Binding and derived-shape construction semantics.
        construction: BinderConstruction,
    },
    /// Thin rib grown from a profile.
    Rib {
        /// Independently resolved construction inputs.
        construction: RibConstruction,
        /// Boolean combination with existing bodies.
        op: BooleanOp,
    },
    /// Planar sheet-metal body created from a closed profile.
    SheetMetalBaseFlange {
        /// Closed profile defining the planar sheet boundary.
        profile: PlanarProfileRef,
        /// Finished sheet thickness.
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            deserialize_with = "deserialize_thickness"
        )]
        #[cfg_attr(feature = "schema", schemars(with = "PositiveLength"))]
        thickness: Option<PositiveLength>,
        /// Distribution of thickness relative to the profile plane.
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            deserialize_with = "deserialize_base_flange_side"
        )]
        #[cfg_attr(feature = "schema", schemars(with = "SheetMetalThicknessSide"))]
        side: Option<SheetMetalThicknessSide>,
    },
    /// Flat plate attached to an existing sheet edge.
    /// Replay the plate before its paired `SheetMetalBend`. The bend builds the joining region.
    SheetMetalAttachedPlate {
        /// Closed profile in the attached plate's sketch plane.
        profile: PlanarProfileRef,
        /// Finished sheet thickness.
        thickness: PositiveLength,
        /// Distribution of thickness relative to the profile plane.
        side: SheetMetalThicknessSide,
        /// Selected edges of the existing sheet.
        attachment: EdgeSelection,
    },
    /// Bend region joining a separate flat plate to the selected sheet edge.
    /// Replay after the paired `SheetMetalAttachedPlate`. Bend dimensions belong to this operation.
    SheetMetalBend {
        /// Preceding attached plate that supplies the planar profile and thickness.
        plate: FeatureId,
        /// Selected edges of the existing sheet, in this bend's input topology.
        attachment: EdgeSelection,
        /// Inside bend radius.
        inside_radius: PositiveLength,
        /// Relief form and evaluated length operands.
        relief: SheetMetalRelief,
        /// Transition form and evaluated radius.
        transition: SheetMetalTransition,
    },
    #[doc = "Creates a sheet by extruding open profiles with rounded bends."]
    SheetMetalContourFlange {
        #[doc = "Independently decoded profile, travel and sheet-rule inputs."]
        construction: sheet_metal::SheetMetalContourFlangeConstruction,
    },
    #[doc = "Flattens selected bends of an existing sheet."]
    SheetMetalUnfold {
        #[doc = "Independently decoded unfolding inputs."]
        construction: sheet_metal::SheetMetalUnfoldConstruction,
    },
    #[doc = "Restores the bends flattened by a preceding Unfold."]
    SheetMetalRefold {
        #[doc = "Associated unfolding operation."]
        construction: sheet_metal::SheetMetalRefoldConstruction,
    },
    #[doc = "Bends an existing sheet along selected sketch lines."]
    SheetMetalBendLines {
        #[doc = "Independently decoded bend inputs."]
        construction: SheetMetalBendLinesConstruction,
    },
    /// Corner evaluation on a bend with an explicit geometric effect.
    SheetMetalCorner {
        /// Bend on which the corner is evaluated.
        bend: FeatureId,
        /// Effect on the input body.
        effect: SheetMetalCornerEffect,
    },
    /// Sheet-metal wall grown from selected edges of an existing sheet body.
    SheetMetalEdgeFlange(sheet_metal::SheetMetalEdgeFlangeConstruction),
    /// Sheet-metal hem grown from one or more selected edges.
    SheetMetalHem {
        /// Edges the hem is grown from.
        edges: EdgeSelection,
        /// Placement of the hem bend against its selected edge.
        bend_position: SheetMetalHemBendPosition,
        /// Dimensional owner layout carried by the source hem.
        form: SheetMetalHemForm,
        /// Direction of the hem fold, when the source carries a proven value.
        direction: SheetMetalHemDirection,
        /// Inside radius of the bend joining the hem to its source face.
        bend_radius: PositiveLength,
    },
    /// Edge fillet.
    Fillet {
        /// Ordered edge groups and their radius laws.
        #[serde(deserialize_with = "deserialize_local_groups")]
        groups: NonEmptyMembers<FilletGroup>,
    },
    /// Full-round fillet built from a center-face selection and two side-face sets.
    FullRoundFillet {
        /// Ordered full-round face groups.
        #[serde(deserialize_with = "deserialize_local_groups")]
        groups: NonEmptyMembers<FullRoundFilletGroup>,
    },
    /// Blend constructed between two face sets.
    FaceBlend {
        /// Disjoint operand selections and their operation arity.
        operands: FaceBlendOperands,
        /// Radius law along the face intersection.
        radius: RadiusSpec,
    },
    /// Edge chamfer.
    Chamfer {
        /// Ordered edge groups and their dimensional specifications.
        #[serde(deserialize_with = "deserialize_local_groups")]
        groups: NonEmptyMembers<ChamferGroup>,
        /// Whether the dimensional reference side is reversed.
        flip_direction: bool,
    },
    /// Thin-wall shell operation.
    Shell {
        /// Bodies explicitly selected as shell inputs, when the source names them.
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            deserialize_with = "deserialize_feature_operation_bodies"
        )]
        bodies: Option<BodySelection>,
        /// Faces removed to open the shell.
        removed_faces: FaceSelection,
        /// Wall thickness left after shelling, when resolved.
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            deserialize_with = "deserialize_thickness"
        )]
        thickness: Option<PositiveLength>,
        /// Whether the wall is grown outward from the original boundary,
        /// as opposed to inward, when resolved.
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            deserialize_with = "deserialize_outward"
        )]
        outward: Option<bool>,
        /// Offset construction used to generate the wall.
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            deserialize_with = "deserialize_mode"
        )]
        mode: Option<ShellMode>,
        /// Corner continuation law used between offset faces.
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            deserialize_with = "deserialize_join"
        )]
        join: Option<ShellJoin>,
        /// Whether intersecting offset regions are resolved during construction.
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            deserialize_with = "deserialize_resolve_intersections"
        )]
        resolve_intersections: Option<bool>,
        /// Whether self-intersecting offset regions may be retained.
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            deserialize_with = "deserialize_allow_self_intersections"
        )]
        allow_self_intersections: Option<bool>,
    },
    /// Offsets an entire source shape without removing opening faces.
    OffsetShape {
        /// Source shape or body to offset.
        source: BodySelection,
        /// Signed normal offset in canonical millimeters.
        distance: NonZeroLength,
        /// Offset construction mode.
        mode: ShellMode,
        /// Corner continuation law.
        join: ShellJoin,
        /// Whether intersecting regions are resolved.
        resolve_intersections: bool,
        /// Whether self-intersecting regions may be retained.
        allow_self_intersections: bool,
        /// Whether open offset boundaries are filled.
        fill: bool,
        /// Whether planar two-dimensional offset rules are used.
        planar: bool,
    },
    /// Builds one compound topology node from ordered source shapes.
    Compound {
        /// Ordered source members retained as a native or resolved selection.
        members: BodySelection,
    },
    /// Removes redundant splitter topology from a source shape.
    RefineShape {
        /// Source shape whose coincident boundaries are simplified.
        source: BodySelection,
    },
    /// Reverses the topological orientation of a source shape.
    ReverseShape {
        /// Source shape whose complete orientation is reversed.
        source: BodySelection,
    },
    /// Ruled sheet connecting two ordered boundary curves.
    RuledBetweenCurves {
        /// First source boundary.
        first: PathRef,
        /// Second source boundary.
        second: PathRef,
        /// Traversal relationship between the two boundaries.
        orientation: RuledCurveOrientation,
    },
    /// Intersection curves produced where two source shapes meet.
    SectionShape {
        /// Disjoint operand selections and their operation arity.
        operands: SectionOperands,
        /// Whether the resulting section edges are approximated, when resolved.
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            deserialize_with = "deserialize_approximate"
        )]
        approximate: Option<bool>,
    },
    /// Reflects one source shape across a model-space plane.
    MirrorShape {
        /// Shape transformed into the mirrored result.
        source: BodySelection,
        /// Point on the persisted resolved mirror plane.
        plane_origin: FinitePoint3,
        /// Unit normal of the persisted resolved mirror plane.
        plane_normal: UnitVector3,
        /// Native plane, face, or circle reference that supplied the resolved plane.
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            deserialize_with = "deserialize_plane_reference"
        )]
        plane_reference: Option<FaceSelection>,
    },
    /// Adds material normal to selected faces.
    Thicken {
        /// Faces offset by the operation.
        faces: FaceSelection,
        /// Finished added thickness, when resolved.
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            deserialize_with = "deserialize_thickness"
        )]
        thickness: Option<PositiveLength>,
        /// Distribution of thickness relative to the selected faces, when resolved.
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            deserialize_with = "deserialize_side"
        )]
        side: Option<ThickenSide>,
        #[doc = "Result-body creation or Boolean combination, when resolved."]
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            deserialize_with = "deserialize_thicken_result"
        )]
        result: Option<SolidFeatureResult>,
        #[doc = "Whether the face selection extends along tangent faces, when resolved."]
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            deserialize_with = "deserialize_tangent_chain"
        )]
        tangent_chain: Option<bool>,
    },
    /// Surface copied at a signed normal offset from selected support faces.
    OffsetSurface {
        /// Faces supplying the source surface geometry.
        faces: FaceSelection,
        /// Signed normal offset in canonical millimeters.
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            deserialize_with = "deserialize_feature_operation_distance"
        )]
        distance: Option<Length>,
    },
    /// Joins selected surface bodies along coincident or near-coincident boundaries.
    KnitSurface {
        /// Faces participating in the knit operation.
        faces: FaceSelection,
        /// Whether coincident face and edge entities are merged.
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            deserialize_with = "deserialize_merge_entities"
        )]
        merge_entities: Option<bool>,
        /// Whether a closed result is converted to a solid body.
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            deserialize_with = "deserialize_create_solid"
        )]
        create_solid: Option<bool>,
        /// Maximum boundary gap accepted by the operation.
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            deserialize_with = "deserialize_gap_tolerance"
        )]
        gap_tolerance: Option<NonNegativeLength>,
    },
    /// Joins sheet or solid bodies along coincident boundaries.
    SewBodies {
        /// Bodies participating in the sew operation.
        bodies: SewBodySelection,
        /// Maximum accepted boundary gap, when resolved.
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            deserialize_with = "deserialize_feature_operation_gap_tolerance"
        )]
        gap_tolerance: Option<PositiveLength>,
    },
    /// Separates selected faces from their bodies into individual surface bodies.
    UnstitchFaces {
        /// Faces separated by the operation.
        faces: FaceSelection,
    },
    /// Surface patch spanning a selected edge boundary.
    FilledSurface {
        /// Closed boundary of the generated patch.
        boundary: SurfaceBoundary,
        /// Adjacent faces supplying tangent or curvature conditions.
        support_faces: FaceSelection,
        /// Uniform, component-specific, or unresolved boundary continuity.
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            deserialize_with = "deserialize_continuity"
        )]
        continuity: Option<FilledSurfaceContinuity>,
        /// Whether the generated patch is merged into adjacent surface bodies,
        /// when resolved.
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            deserialize_with = "deserialize_merge_result"
        )]
        merge_result: Option<bool>,
        #[doc = "Curves and points inside the boundary that constrain the patch, in source order."]
        interior_constraints: Vec<PathRef>,
        #[doc = "Whether the generated patch normal is reversed, when resolved."]
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            deserialize_with = "deserialize_reverse_normal"
        )]
        reverse_normal: Option<bool>,
    },
    /// Restricts selected surface faces to one side of a trimming path.
    TrimSurface {
        /// Surface faces modified by the operation.
        faces: FaceSelection,
        /// Sketch or model-space path defining the trim boundary.
        tool: PathRef,
        /// Region or explicit partition-cell set retained after trimming.
        keep: TrimRegion,
    },
    /// Extends selected surface boundaries by a fixed distance.
    ExtendSurface {
        /// Surface faces whose boundaries are extended.
        faces: FaceSelection,
        /// Positive extension distance in canonical millimeters.
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            deserialize_with = "deserialize_distance"
        )]
        distance: Option<PositiveLength>,
        /// Geometric continuation law.
        method: SurfaceExtension,
    },
    #[doc = "Removes trim loops from selected surface faces."]
    UntrimSurface {
        #[doc = "Surface faces whose trim loops are removed."]
        faces: FaceSelection,
        #[doc = "Trim loops removed from the selected faces."]
        loops: UntrimLoops,
        #[doc = "Signed exterior-boundary extension in canonical millimeters."]
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            deserialize_with = "deserialize_extension"
        )]
        extension: Option<Length>,
    },
    /// Ruled surface grown from selected boundary edges.
    RuledSurface {
        /// Boundary edges from which the surface is generated.
        edges: EdgeSelection,
        /// Adjacent faces supplying normal or tangent context.
        support_faces: FaceSelection,
        /// Direction law and extension distance.
        mode: RuledSurfaceMode,
        /// Signed rotation from the selected ruled-surface law.
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            deserialize_with = "deserialize_angle"
        )]
        angle: Option<Angle>,
        /// Whether the opposite incident face supplies the angle reference.
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            deserialize_with = "deserialize_alternate_face"
        )]
        alternate_face: Option<bool>,
        /// Boundary-corner construction law.
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            deserialize_with = "deserialize_corner"
        )]
        corner: Option<RuledSurfaceCorner>,
    },
    /// Taper applied to selected faces about a neutral plane.
    Draft {
        /// Faces whose angle is modified.
        faces: FaceSelection,
        /// Structurally selected anchor and pull frame.
        anchor: DraftAnchor,
        /// Signed draft angle.
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            deserialize_with = "deserialize_feature_operation_angle"
        )]
        angle: Option<SlopeAngle>,
        /// Whether material is added away from the pull direction.
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            deserialize_with = "deserialize_feature_operation_outward"
        )]
        outward: Option<bool>,
        /// Draft of the face portions on the far side of the neutral plane.
        ///
        /// When present, the anchor is a neutral plane with a pull direction,
        /// and the neutral plane splits each selected face. The portion on the
        /// side into which the pull direction points drafts by `angle`. The
        /// other portion drafts by the second-side angle against the reversed
        /// pull direction, with the sign convention of `angle`.
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            deserialize_with = "deserialize_draft_second_side"
        )]
        second_side: Option<DraftSecondSide>,
    },
    /// Boolean operation between existing bodies.
    Combine {
        /// Disjoint operand selections and their operation arity.
        operands: CombineOperands,
        /// Join, cut, or intersection operation.
        op: BooleanKind,
        /// Whether tool bodies remain present after the Boolean result is created.
        keep_tools: bool,
        #[doc = "Component creation requested for the Boolean result."]
        destination: CombineDestination,
    },
    /// Creates or combines solids from cells enclosed by boundary tools.
    BoundaryFill {
        /// Bodies whose faces partition space into candidate cells.
        tools: BodySelection,
        /// Selected cells, in source order.
        #[serde(deserialize_with = "deserialize_local_cells")]
        cells: NonEmptyMembers<BoundaryFillCell>,
        /// Construction-plane selectors in source order, including empty source slots.
        plane_tools: Vec<Option<SelectionReference>>,
        /// Boolean effect of the selected cells.
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            deserialize_with = "deserialize_boundary_fill_operation"
        )]
        operation: Option<BooleanOp>,
        /// Whether to remove tool bodies after the fill.
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            deserialize_with = "deserialize_boundary_fill_remove_tools"
        )]
        remove_tools: Option<bool>,
    },
    /// Removes one side of selected bodies using selected surface faces.
    CutWithSurface {
        /// Bodies cut by the operation.
        targets: BodySelection,
        /// Oriented surface faces defining the cut.
        tools: FaceSelection,
        /// Whether the side opposite the default tool orientation is removed,
        /// when the native side flag is present.
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            deserialize_with = "deserialize_reverse"
        )]
        reverse: Option<bool>,
    },
    /// Removes one side of target bodies using ordered tool bodies.
    TrimBodies {
        /// Disjoint operand selections and their operation arity.
        operands: TrimBodyOperands,
        /// Side retained by the trim.
        keep: BodyTrimSide,
    },
    /// Removes one side of selected bodies at a datum plane. The selected
    /// bodies keep their identity and retain the side named by `keep`.
    TrimBodiesAtPlane {
        /// Bodies trimmed by the operation.
        targets: BodySelection,
        /// Earlier datum-plane feature supplying the trimming plane.
        plane: FeatureId,
        /// Side retained: `Forward` keeps the side that the plane normal
        /// points into, `Reverse` keeps the opposite side.
        keep: BodyTrimSide,
    },
    /// Partitions selected bodies with surface faces or a datum plane while
    /// retaining every resulting side.
    SplitBody {
        /// Bodies partitioned by the operation.
        targets: BodySelection,
        /// Geometry that partitions the target bodies.
        tool: SplitBodyTool,
        /// Whether the operation extends the splitting geometry through the targets.
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            deserialize_with = "deserialize_split_body_extend_tool"
        )]
        extend_tool: Option<bool>,
    },
    /// Partitions selected faces along selected sketch curves while retaining
    /// every resulting face region.
    SplitFace {
        /// Faces partitioned by the operation.
        targets: FaceSelection,
        /// Geometry or datum plane that partitions the target faces.
        tool: SplitFaceTool,
        #[doc = "Method used to apply the splitting tool to the target faces."]
        method: SplitFaceMethod,
    },
    /// Partitions a body's faces or volume along its silhouette in a given direction.
    SilhouetteSplit {
        /// Body selected before the split.
        target: BodySelection,
        /// Saved model-space silhouette direction.
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            deserialize_with = "deserialize_direction"
        )]
        direction: Option<FeatureDirection3>,
        /// Result of the partition.
        mode: SilhouetteSplitMode,
    },
    /// Deletes bodies directly or retains only the selected bodies.
    DeleteBody {
        /// Bodies selected by the operation.
        bodies: BodySelection,
        /// Whether selected bodies are deleted or retained.
        mode: BodyRetentionMode,
    },
    /// Removal of selected faces from an existing body.
    DeleteFace {
        /// Faces removed by the operation.
        faces: FaceSelection,
        /// Whether adjacent faces extend to heal the resulting boundary.
        heal: bool,
    },
    /// Replaces selected faces with another face set.
    ReplaceFace {
        /// Disjoint operand selections and their operation arity.
        operands: ReplaceFaceOperands,
    },
    /// Direct motion of selected faces.
    MoveFace {
        /// Faces modified by the operation.
        faces: FaceSelection,
        /// Motion applied to the selected faces.
        motion: FaceMotion,
    },
    /// Rigid translation or rotation of selected bodies, optionally creating copies.
    MoveBody {
        /// Bodies transformed by the operation.
        bodies: BodySelection,
        /// Model-space translation vector in canonical millimeters.
        translation: FiniteVector3,
        /// Axis-angle rotation applied with the translation.
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            deserialize_with = "deserialize_rotation"
        )]
        rotation: Option<AxisAngle>,
        /// Number of transformed copies; zero moves the selected bodies.
        copies: u32,
    },
    /// Dome grown from selected planar faces.
    Dome {
        /// Faces that bound the dome base.
        faces: FaceSelection,
        /// Dome height measured normal to the base, when resolved.
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            deserialize_with = "deserialize_height"
        )]
        height: Option<PositiveLength>,
        /// Whether the profile is elliptical rather than spherical, when resolved.
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            deserialize_with = "deserialize_elliptical"
        )]
        elliptical: Option<bool>,
        /// Whether growth opposes the selected-face normal, when resolved.
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            deserialize_with = "deserialize_reverse"
        )]
        reverse: Option<bool>,
    },
    /// Deformation of existing geometry about a feature axis.
    Flex {
        /// Flex axis direction in model space, when resolved.
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            deserialize_with = "deserialize_axis"
        )]
        axis: Option<FeatureDirection3>,
        /// Applied deformation mode and magnitude.
        mode: FlexMode,
    },
    /// Scales selected bodies about a model-space point.
    Scale {
        /// Bodies transformed by the operation.
        bodies: BodySelection,
        /// Fixed locus of the scale transform.
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            deserialize_with = "deserialize_center"
        )]
        center: Option<ScaleCenter>,
        /// Uniform, per-axis, or unresolved scale factors.
        factors: ScaleFactors,
    },
    /// Drilled or machined hole.
    Hole {
        /// Sketch or profile supplying one or more hole locations.
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            deserialize_with = "deserialize_profile"
        )]
        profile: Option<PlanarProfileRef>,
        /// Geometry families in the profile that generate hole locations.
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            deserialize_with = "deserialize_profile_filter"
        )]
        profile_filter: Option<HoleProfileFilter>,
        /// Face the hole is placed on, when known.
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            deserialize_with = "deserialize_face"
        )]
        face: Option<FaceSelection>,
        /// Drilling direction carried independently of complete placements.
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            deserialize_with = "deserialize_direction"
        )]
        direction: Option<FeatureDirection3>,
        /// Complete one-or-many hole placements, when resolved.
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            deserialize_with = "deserialize_placements"
        )]
        placements: Option<Vec<HolePlacement>>,
        /// Bore diameter and compatible entry, exit, and thread construction.
        shape: HoleShape,
        /// How deep the hole extends, when resolved. Holes travel on one side
        /// only, so the termination law needs no sidedness wrapper.
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            deserialize_with = "deserialize_feature_operation_extent"
        )]
        extent: Option<LinearTermination>,
        /// Shape and depth convention at the blind end of the hole.
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            deserialize_with = "deserialize_bottom"
        )]
        bottom: Option<HoleBottom>,
        /// Included taper angle for a conical hole, when enabled.
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            deserialize_with = "deserialize_taper_angle"
        )]
        taper_angle: Option<InteriorAngle>,
        /// Whether a profile containing multiple faces is accepted as one operation.
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            deserialize_with = "deserialize_allow_multi_profile_faces"
        )]
        allow_multi_profile_faces: Option<bool>,
    },
    /// Repetition or reflection of existing features.
    Pattern {
        /// Geometry being repeated or reflected; empty when the source selection is unresolved.
        seeds: Vec<PatternSeed>,
        /// Spatial transform defining the repetition or reflection.
        pattern: PatternKind,
    },
    /// Operation family established without its construction operands.
    Unresolved {
        /// Operation family of the unresolved feature.
        family: UnresolvedFamily,
    },
    /// Source-native operation without neutral semantics.
    Native {
        /// Native feature-type tag (e.g. `"Extrude"`, `"Fillet"`).
        kind: NativeFeatureKind,
        /// Source parametric input values keyed by parameter name.
        #[serde(deserialize_with = "cadmpeg_core::distinct_keys::btree_map")]
        parameters: BTreeMap<NonBlankString, String>,
    },
    /// Linear extrusion of a profile.
    Extrude {
        /// Profile swept along `direction`.
        profile: ProfileRef,
        /// Direction in which the profile is swept and its optional persisted source.
        direction: ExtrudeDirection,
        /// Plane or face from which the extrusion begins.
        start: ExtrudeStart,
        /// How far the extrusion travels on each of its sides.
        extent: ExtrudeExtent,
        /// Boolean combination with existing bodies.
        op: BooleanOp,
        /// Whether the result is a solid (`true`) or sheet (`false`), when selectable.
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            deserialize_with = "deserialize_solid"
        )]
        solid: Option<bool>,
        /// Native face-building policy used to turn closed wires into faces.
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            deserialize_with = "deserialize_face_maker"
        )]
        face_maker: Option<FaceMaker>,
        /// Taper orientation used for inner wires, when selectable.
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            deserialize_with = "deserialize_inner_wire_taper"
        )]
        inner_wire_taper: Option<InnerWireTaper>,
        /// Whether stored lengths are measured along the profile normal instead of the sweep axis.
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            deserialize_with = "deserialize_length_along_profile_normal"
        )]
        length_along_profile_normal: Option<bool>,
        /// Whether a profile containing multiple faces is accepted as one operation.
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            deserialize_with = "deserialize_allow_multi_profile_faces"
        )]
        allow_multi_profile_faces: Option<bool>,
    },
    /// Loft through an ordered sequence of profile or point sections.
    Loft {
        /// Ordered cross-sections from the loft start to end.
        sections: Vec<LoftSection>,
        /// Mutually exclusive guide trajectories or centerline.
        guidance: LoftGuidance,
        /// Boolean combination with existing bodies.
        op: BooleanOp,
        /// Whether the loft closes from the last section to the first.
        closed: bool,
        /// Whether the sections bound a solid instead of a sheet body.
        solid: bool,
        /// Whether adjacent sections are connected by straight ruled spans.
        ruled: bool,
        /// Whether linear edges and planar faces are simplified after construction.
        linearize: bool,
        /// Maximum polynomial degree used to interpolate the sections, when constrained.
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            deserialize_with = "deserialize_max_degree"
        )]
        max_degree: Option<std::num::NonZeroU32>,
        /// Whether profiles containing multiple faces are accepted as one operation.
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            deserialize_with = "deserialize_allow_multi_profile_faces"
        )]
        allow_multi_profile_faces: Option<bool>,
    },
}

/// Neutral construction semantics: one operation, optionally under a single
/// post-processing layer.
///
/// A post-processed operation carries a [`FeatureOperation`], which has no
/// post-processing arm, so a post-process inside a post-process has no
/// spelling.
#[derive(Debug, Clone, PartialEq, Serialize, cadmpeg_derive::DeserializeTagged)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(
    tag = "kind",
    content = "value",
    rename_all = "snake_case",
    deny_unknown_fields
)]
pub enum FeatureDefinition {
    /// Operation followed by source-requested topology cleanup.
    PostProcess {
        /// Underlying construction whose result is post-processed.
        operation: FeatureOperation,
        /// Whether redundant splitter boundaries are removed.
        refine: bool,
        /// Boolean-operation tolerance selection carried by the feature family.
        fuzzy_tolerance: FuzzyTolerance,
    },
    /// Any operation that a post-processing layer also admits.
    Operation(FeatureOperation),
}

/// Operation family of a feature whose construction operands are unresolved.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, DeserializeName)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(rename_all = "snake_case")]
#[serde(deny_unknown_fields)]
pub enum UnresolvedFamily {
    /// The `datum_plane` operation family.
    DatumPlane,
    /// The `datum_axis` operation family.
    DatumAxis,
    /// The `datum_point` operation family.
    DatumPoint,
    /// The `datum_coordinate_system` operation family.
    DatumCoordinateSystem,
    /// The `bridge_curve` operation family.
    BridgeCurve,
    /// The `brep` operation family.
    Brep,
    /// The `cylinder` operation family.
    Cylinder,
    /// The `cone` operation family.
    Cone,
    /// The `sphere` operation family.
    Sphere,
    /// The `thread` operation family.
    Thread,
    /// The `detailed_thread` operation family.
    DetailedThread,
    /// The `loft` operation family.
    Loft,
    /// The `through_curve_mesh` operation family.
    ThroughCurveMesh,
    /// The `freeform_surface` operation family.
    FreeformSurface,
    /// The `extrude` operation family.
    Extrude,
    /// The `revolve` operation family.
    Revolve,
    #[doc = "The `combine` operation family."]
    Combine,
    /// The `fillet` operation family.
    Fillet,
    /// The `extract_face` operation family.
    ExtractFace,
    /// The `copy_face` operation family.
    CopyFace,
    /// The `linked_face` operation family.
    LinkedFace,
    /// The `fill_hole` operation family.
    FillHole,
    /// The `boundary_surface` operation family.
    BoundarySurface,
    /// The `draft` operation family.
    Draft,
    /// The `delete_face` operation family.
    DeleteFace,
    /// The `move_face` operation family.
    MoveFace,
    /// The `mirror_face` operation family.
    MirrorFace,
    /// The `subdivision_body` operation family.
    SubdivisionBody,
    /// The `topology_optimization` operation family.
    TopologyOptimization,
    /// The `move_object` operation family.
    MoveObject,
}

impl FeatureDefinition {
    /// The operation this definition performs, its post-processing layer aside.
    #[must_use]
    pub const fn operation(&self) -> &FeatureOperation {
        match self {
            Self::PostProcess { operation, .. } | Self::Operation(operation) => operation,
        }
    }

    /// Family name of a definition whose replay produces body geometry, and
    /// `None` for definitions that do not.
    ///
    /// A feature of one of these families carries current result bodies in its
    /// [`FeatureEvaluation::outputs`] list or intermediate result identities in a
    /// [`FeatureResultTopology`]. The returned name is the stable lowercase
    /// label for the family, suitable for grouping such features in a report.
    #[must_use]
    pub fn body_output_family(&self) -> Option<&'static str> {
        match self {
            Self::PostProcess { operation, .. } => operation.body_output_family(),
            Self::Operation(operation) => operation.body_output_family(),
        }
    }
}

/// The refusal of primitive dimensions that define no solid.
const INVALID_PRIMITIVE_DIMENSIONS: &str = "primitive dimensions are invalid";

/// Axial termination of a cosmetic-thread annotation.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, cadmpeg_derive::DeserializeTagged)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(tag = "kind", rename_all = "snake_case")]
#[serde(deny_unknown_fields)]
pub enum CosmeticThreadExtent {
    /// Fixed thread length along the cylindrical face.
    Blind {
        /// Positive axial thread length.
        length: PositiveLength,
    },
    /// Thread annotation spans the complete cylindrical face.
    Through {},
}

/// Canonical role of a built-in reference plane.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, DeserializeName)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(rename_all = "snake_case")]
#[serde(deny_unknown_fields)]
pub enum PrincipalPlane {
    /// Front plane through the model origin.
    Front,
    /// Top plane through the model origin.
    Top,
    /// Right plane through the model origin.
    Right,
}

/// Known sketch space and its optional resolved planar geometry.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(from = "SketchFeatureBindingWire", into = "SketchFeatureBindingWire")]
#[serde(deny_unknown_fields)]
pub enum SketchFeatureBinding {
    /// The feature's sketch space is unresolved.
    Unresolved,
    /// The source declares a planar sketch, with optional decoded geometry.
    Planar(Option<crate::sketches::SketchId>),
}

impl SketchFeatureBinding {
    /// Resolved planar sketch identity, when available.
    pub fn id(&self) -> Option<&crate::sketches::SketchId> {
        match self {
            Self::Unresolved => None,
            Self::Planar(sketch) => sketch.as_ref(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, cadmpeg_derive::DeserializeTagged)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
enum SketchFeatureBindingWire {
    /// The feature's sketch space is unresolved.
    Unresolved {},
    /// The source declares a planar sketch, with optional decoded geometry.
    Planar {
        /// Resolved planar sketch identity, when available.
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            deserialize_with = "deserialize_feature_operation_sketch"
        )]
        sketch: Option<crate::sketches::SketchId>,
    },
}

impl From<SketchFeatureBindingWire> for SketchFeatureBinding {
    fn from(value: SketchFeatureBindingWire) -> Self {
        match value {
            SketchFeatureBindingWire::Unresolved {} => Self::Unresolved,
            SketchFeatureBindingWire::Planar { sketch } => Self::Planar(sketch),
        }
    }
}

cadmpeg_core::named_optional_field!(deserialize_filled_surface_weight, PositiveReal, "weight");

selection_field_deserializer!(deserialize_selection_native, "native");
selection_field_deserializer!(deserialize_selection_local_id, "local_id");
selection_field_deserializer!(deserialize_selection_edges, "edges");
selection_field_deserializer!(deserialize_selection_faces, "faces");
selection_field_deserializer!(deserialize_selection_unresolved, "unresolved");

selection_field_deserializer!(deserialize_selection_profiles, "profiles");
selection_field_deserializer!(deserialize_selection_entities, "entities");
selection_field_deserializer!(deserialize_selection_selections, "selections");
selection_field_deserializer!(deserialize_selection_curves, "curves");
selection_field_deserializer!(deserialize_profile_parameter_range, "parameter_range");
selection_field_deserializer!(deserialize_local_tessellations, "tessellations");
selection_field_deserializer!(deserialize_mesh_source, "source");
selection_field_deserializer!(deserialize_local_segments, "segments");
selection_field_deserializer!(deserialize_local_axis_native_ref, "axis_native_ref");
selection_field_deserializer!(deserialize_local_groups, "groups");
selection_field_deserializer!(deserialize_local_cells, "cells");
selection_field_deserializer!(deserialize_local_document, "document");
selection_field_deserializer!(deserialize_local_object, "object");
selection_field_deserializer!(deserialize_local_reference, "reference");
selection_field_deserializer!(deserialize_local_subelements, "subelements");
selection_field_deserializer!(deserialize_local_value, "value");

/// Invalid collection membership.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum FeatureCollectionError {
    /// The collection violates its member constraint.
    #[error("{0}")]
    Invalid(&'static str),
}

impl From<FeatureCollectionError> for cadmpeg_core::CodecError {
    fn from(error: FeatureCollectionError) -> Self {
        match error {
            FeatureCollectionError::Invalid(message) => Self::malformed(message),
        }
    }
}

rewrite_scalar!(PlaneTargetSource);

const EPS_FEATURE_FULL_TURN: f64 = 1.0e-12;

cadmpeg_core::named_optional_field!(deserialize_boundary_fill_operation, BooleanOp, "operation");
cadmpeg_core::named_optional_field!(deserialize_boundary_fill_remove_tools, bool, "remove_tools");

#[cfg(test)]
mod tests;

// Each optional key below names itself in whatever it refuses.

cadmpeg_core::named_optional_field!(deserialize_section_sketch, FeatureId, "sketch");
cadmpeg_core::named_optional_field!(deserialize_section_plane, FeatureSupportPlaneFrame, "plane");
cadmpeg_core::named_optional_field!(deserialize_pull, DraftPull, "pull");

cadmpeg_core::named_optional_field!(
    deserialize_draft_second_side,
    DraftSecondSide,
    "second_side"
);

cadmpeg_core::named_optional_field!(deserialize_name, String, "name");

cadmpeg_core::named_optional_field!(deserialize_native_ref, String, "native_ref");

cadmpeg_core::named_optional_field!(
    deserialize_regeneration_parent,
    FeatureId,
    "regeneration_parent"
);
cadmpeg_core::named_optional_field!(deserialize_source_tag, String, "source_tag");
cadmpeg_core::named_optional_field!(deserialize_transfer_occurrence, OccurrenceId, "occurrence");
cadmpeg_core::named_optional_field!(
    deserialize_transfer_component,
    ProductDefinitionId,
    "component"
);
cadmpeg_core::named_optional_field!(
    deserialize_transfer_source_owner,
    ProductDefinitionId,
    "source_owner"
);
cadmpeg_core::named_optional_field!(
    deserialize_transfer_target_owner,
    ProductDefinitionId,
    "target_owner"
);
cadmpeg_core::named_optional_field!(
    deserialize_transfer_source_transform,
    Transform,
    "source_transform"
);
cadmpeg_core::named_optional_field!(
    deserialize_transfer_target_transform,
    Transform,
    "target_transform"
);
cadmpeg_core::named_optional_field!(deserialize_source_text, String, "source_text");

cadmpeg_core::named_optional_field!(deserialize_diameter, PositiveLength, "diameter");
cadmpeg_core::named_optional_field!(deserialize_extent, CosmeticThreadExtent, "extent");
cadmpeg_core::named_optional_field!(deserialize_thread_standard, NonBlankString, "standard");
cadmpeg_core::named_optional_field!(
    deserialize_thread_designation,
    NonBlankString,
    "designation"
);
cadmpeg_core::named_optional_field!(deserialize_thread_pitch, PositiveLength, "pitch");
cadmpeg_core::named_optional_field!(deserialize_thread_modeled, bool, "modeled");
cadmpeg_core::named_optional_field!(deserialize_split_body_extend_tool, bool, "extend_tool");
cadmpeg_core::named_optional_field!(deserialize_opacity, Fraction, "opacity");
cadmpeg_core::named_optional_field!(deserialize_reference, DatumPlaneReference, "reference");
cadmpeg_core::named_optional_field!(
    deserialize_construction,
    Box<DatumPointConstruction>,
    "construction"
);
cadmpeg_core::named_optional_field!(deserialize_bidirectional, bool, "bidirectional");
cadmpeg_core::named_optional_field!(
    deserialize_primitive_placement,
    FeatureRigidPlacement,
    "placement"
);
cadmpeg_core::named_optional_field!(deserialize_segment_turns, PositiveReal, "segment_turns");
cadmpeg_core::named_optional_field!(
    deserialize_construction_style,
    HelixConstructionStyle,
    "construction_style"
);
cadmpeg_core::named_optional_field!(
    deserialize_sketch,
    crate::sketches::SpatialSketchId,
    "sketch"
);
cadmpeg_core::named_optional_field!(
    deserialize_feature_operation_sketch,
    crate::sketches::SketchId,
    "sketch"
);
cadmpeg_core::named_optional_field!(deserialize_block, FeatureId, "block");
cadmpeg_core::named_optional_field!(
    deserialize_placement,
    crate::transform::Transform,
    "placement"
);
cadmpeg_core::named_optional_field!(deserialize_path, PathRef, "path");
cadmpeg_core::named_optional_field!(deserialize_orientation, SweepOrientation, "orientation");
cadmpeg_core::named_optional_field!(deserialize_transition, SweepTransition, "transition");
cadmpeg_core::named_optional_field!(
    deserialize_transformation,
    SweepTransformation,
    "transformation"
);
cadmpeg_core::named_optional_field!(deserialize_twist, Angle, "twist");
cadmpeg_core::named_optional_field!(deserialize_path_extent, SweepPathExtent, "path_extent");
cadmpeg_core::named_optional_field!(deserialize_guide_rail, SweepGuideRail, "guide_rail");
cadmpeg_core::named_optional_field!(deserialize_taper, Angle, "taper");
cadmpeg_core::named_optional_field!(deserialize_scale, PositiveReal, "scale");
cadmpeg_core::named_optional_field!(
    deserialize_allow_multi_profile_faces,
    bool,
    "allow_multi_profile_faces"
);
cadmpeg_core::named_optional_field!(
    deserialize_feature_operation_bodies,
    BodySelection,
    "bodies"
);
cadmpeg_core::named_optional_field!(deserialize_thickness, PositiveLength, "thickness");
cadmpeg_core::named_optional_field!(
    deserialize_base_flange_side,
    SheetMetalThicknessSide,
    "side"
);
cadmpeg_core::named_optional_field!(deserialize_outward, bool, "outward");
cadmpeg_core::named_optional_field!(deserialize_mode, ShellMode, "mode");
cadmpeg_core::named_optional_field!(deserialize_join, ShellJoin, "join");
cadmpeg_core::named_optional_field!(
    deserialize_resolve_intersections,
    bool,
    "resolve_intersections"
);
cadmpeg_core::named_optional_field!(
    deserialize_allow_self_intersections,
    bool,
    "allow_self_intersections"
);
cadmpeg_core::named_optional_field!(deserialize_approximate, bool, "approximate");
cadmpeg_core::named_optional_field!(
    deserialize_plane_reference,
    FaceSelection,
    "plane_reference"
);
cadmpeg_core::named_optional_field!(deserialize_side, ThickenSide, "side");
cadmpeg_core::named_optional_field!(
    deserialize_gap_tolerance,
    NonNegativeLength,
    "gap_tolerance"
);
cadmpeg_core::named_optional_field!(deserialize_merge_result, bool, "merge_result");
cadmpeg_core::named_optional_field!(deserialize_reverse_normal, bool, "reverse_normal");
cadmpeg_core::named_optional_field!(deserialize_angle, Angle, "angle");
cadmpeg_core::named_optional_field!(deserialize_alternate_face, bool, "alternate_face");
cadmpeg_core::named_optional_field!(deserialize_corner, RuledSurfaceCorner, "corner");
cadmpeg_core::named_optional_field!(deserialize_reverse, bool, "reverse");
cadmpeg_core::named_optional_field!(deserialize_rotation, AxisAngle, "rotation");
cadmpeg_core::named_optional_field!(deserialize_height, PositiveLength, "height");
cadmpeg_core::named_optional_field!(deserialize_elliptical, bool, "elliptical");
cadmpeg_core::named_optional_field!(deserialize_axis, FeatureDirection3, "axis");
cadmpeg_core::named_optional_field!(deserialize_center, ScaleCenter, "center");
cadmpeg_core::named_optional_field!(deserialize_profile, PlanarProfileRef, "profile");
cadmpeg_core::named_optional_field!(
    deserialize_profile_filter,
    HoleProfileFilter,
    "profile_filter"
);
cadmpeg_core::named_optional_field!(deserialize_face, FaceSelection, "face");
cadmpeg_core::named_optional_field!(deserialize_direction, FeatureDirection3, "direction");
cadmpeg_core::named_optional_field!(deserialize_placements, Vec<HolePlacement>, "placements");
cadmpeg_core::named_optional_field!(
    deserialize_feature_operation_extent,
    LinearTermination,
    "extent"
);
cadmpeg_core::named_optional_field!(deserialize_bottom, HoleBottom, "bottom");
cadmpeg_core::named_optional_field!(deserialize_taper_angle, InteriorAngle, "taper_angle");
cadmpeg_core::named_optional_field!(deserialize_solid, bool, "solid");
cadmpeg_core::named_optional_field!(deserialize_face_maker, FaceMaker, "face_maker");
cadmpeg_core::named_optional_field!(
    deserialize_inner_wire_taper,
    InnerWireTaper,
    "inner_wire_taper"
);
cadmpeg_core::named_optional_field!(
    deserialize_length_along_profile_normal,
    bool,
    "length_along_profile_normal"
);
cadmpeg_core::named_optional_field!(deserialize_max_degree, std::num::NonZeroU32, "max_degree");
cadmpeg_core::named_optional_field!(deserialize_source, ExtrusionDirectionSource, "source");
cadmpeg_core::named_optional_field!(deserialize_face_maker_class, FaceMaker, "face_maker_class");
cadmpeg_core::named_optional_field!(deserialize_fuse_order, RevolutionFuseOrder, "fuse_order");
cadmpeg_core::named_optional_field!(
    deserialize_partial_revolve_construction_axis,
    RevolutionAxis,
    "axis"
);
cadmpeg_core::named_optional_field!(
    deserialize_partial_revolve_construction_extent,
    RevolveExtent,
    "extent"
);

cadmpeg_core::named_optional_field!(deserialize_offset, Length, "offset");
cadmpeg_core::named_optional_field!(deserialize_wrap_depth, Length, "depth");

cadmpeg_core::named_optional_field!(
    deserialize_binder_construction_offset,
    BinderOffset,
    "offset"
);
cadmpeg_core::named_optional_field!(deserialize_context, BinderTarget, "context");
cadmpeg_core::named_optional_field!(deserialize_form, FlexForm, "form");

impl Feature {
    /// Copy the evaluated construction state for one configuration.
    pub fn configuration_state(&self) -> ConfigurationFeatureState {
        ConfigurationFeatureState {
            evaluation: if self.suppressed.unwrap_or(false) {
                ConfigurationEvaluation::Suppressed {}
            } else {
                ConfigurationEvaluation::Active {
                    outputs: self.evaluation.outputs.clone(),
                }
            },
            dependencies: self.dependencies.clone(),
            definition: self.evaluation.definition.clone(),
        }
    }
}

mod identity_rewrite;

pub(crate) mod serialization;

cadmpeg_core::named_optional_field!(
    deserialize_product_definition,
    ProductDefinitionId,
    "product_definition"
);

cadmpeg_core::named_optional_field!(deserialize_plane_target_source, PlaneTargetSource, "source");
cadmpeg_core::named_optional_field!(deserialize_thicken_result, SolidFeatureResult, "result");
cadmpeg_core::named_optional_field!(deserialize_tangent_chain, bool, "tangent_chain");

cadmpeg_core::named_optional_field!(deserialize_feature_operation_outward, bool, "outward");
cadmpeg_core::named_optional_field!(deserialize_feature_operation_angle, SlopeAngle, "angle");
cadmpeg_core::named_optional_field!(deserialize_extension, Length, "extension");
cadmpeg_core::named_optional_field!(deserialize_distance, PositiveLength, "distance");
cadmpeg_core::named_optional_field!(
    deserialize_feature_operation_gap_tolerance,
    PositiveLength,
    "gap_tolerance"
);
cadmpeg_core::named_optional_field!(deserialize_create_solid, bool, "create_solid");
cadmpeg_core::named_optional_field!(deserialize_merge_entities, bool, "merge_entities");
cadmpeg_core::named_optional_field!(deserialize_feature_operation_distance, Length, "distance");
cadmpeg_core::named_optional_field!(
    deserialize_feature_operation_placement,
    FeatureRigidPlacement,
    "placement"
);
cadmpeg_core::named_optional_field!(deserialize_dimensions, [PositiveLength; 3], "dimensions");
cadmpeg_core::named_optional_field!(deserialize_suppressed, bool, "suppressed");
cadmpeg_core::named_optional_field!(deserialize_feature_row_wire_suppressed, bool, "suppressed");
