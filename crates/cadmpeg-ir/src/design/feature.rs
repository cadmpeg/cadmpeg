// SPDX-License-Identifier: Apache-2.0
//! Features, their definitions, and stored geometry.
//!
//! A feature holds one operation and the cleanup the source states for its
//! result. The operation vocabulary holds typed operations, `Unresolved` for
//! an operation known without its operands, `Native` for one whose meaning is
//! not established, and `Stored` for a feature defined by stored geometry.

use cadmpeg_core::text::NonBlankString;
#[cfg(feature = "schema")]
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use super::construction::{kinds, Construction};
use super::dimension::Dimension;
use super::names::NativeName;
use super::options::NativeOptions;
use super::selection::{DefinitionRef, SetSelection};
use crate::features::FeatureId;
use crate::ids::{BodyId, ProductDefinitionId};
use crate::scalar::{FiniteReal, PositiveLength};
use crate::sketches::{SketchId, SpatialSketchId};
use crate::spreadsheets::SpreadsheetId;
use crate::tessellation::TessellationId;
use crate::transform::Transform;

/// A feature of a scope's history.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct Feature {
    /// The feature's identity.
    pub id: FeatureId,
    /// The feature's name.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_name"
    )]
    pub name: Option<String>,
    /// Whether the feature is suppressed, a literal or bound to a parameter.
    /// `None` means the source does not state it.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_suppressed"
    )]
    pub suppressed: Option<Dimension<bool>>,
    /// The feature's one operation and the cleanup of its result.
    pub definition: FeatureDefinition,
    /// The placement the source applies to the feature's own result after the
    /// operation, as a construction whose components can be bound.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_placement"
    )]
    pub placement: Option<Box<Construction<kinds::Frame>>>,
    /// The definition under which the source lists the feature, in a scope
    /// that spans several definitions. `None` in a scope that spans one, or
    /// where the decoder did not establish the owner.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_component"
    )]
    pub component: Option<ProductDefinitionId>,
    /// The feature that defines this one, where the source generates a
    /// feature in one scope from a feature in another.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_driven_by"
    )]
    pub driven_by: Option<Driver>,
    /// The feature's native record.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_native_ref"
    )]
    pub native_ref: Option<String>,
}

/// A feature's one operation and the cleanup the source states for its
/// result. On the wire it has no wrapper level.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct FeatureDefinition {
    /// The operation.
    pub operation: FeatureOperation,
    /// Whether redundant splitter boundaries are removed from the result.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_refine"
    )]
    pub refine: Option<bool>,
    /// The Boolean tolerance the feature carries.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_fuzzy_tolerance"
    )]
    pub fuzzy_tolerance: Option<FuzzyTolerance>,
}

/// The feature that defines a feature generated in another scope.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(
    tag = "kind",
    content = "value",
    rename_all = "snake_case",
    deny_unknown_fields
)]
pub enum Driver {
    /// A feature of this document.
    Feature(FeatureId),
    /// A feature of a document that was not read, by the source's name.
    Native(NativeName),
}

/// The Boolean tolerance a feature carries.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(
    tag = "kind",
    content = "value",
    rename_all = "snake_case",
    deny_unknown_fields
)]
pub enum FuzzyTolerance {
    /// The modeling kernel's default tolerance.
    KernelDefault,
    /// A tolerance determined from the participating shapes.
    Automatic,
    /// A stated positive tolerance.
    Explicit(Dimension<PositiveLength>),
}

/// The operation of a feature.
///
/// The arms of the feature families (§6.1 to §6.7) are added with each
/// family's types.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(
    tag = "kind",
    content = "value",
    rename_all = "snake_case",
    deny_unknown_fields
)]
pub enum FeatureOperation {
    /// A planar sketch: names the sketch record the feature owns.
    Sketch(SketchId),
    /// A spatial sketch: names the sketch record the feature owns.
    SpatialSketch(SpatialSketchId),
    /// A sheet: names the sheet record the feature owns.
    Spreadsheet(SpreadsheetId),
    /// A design feature whose removed volume is the volume a manufacturing
    /// operation machines.
    MaterialRemoval,
    /// A feature defined by stored geometry.
    Stored {
        /// The bodies the feature contributes, whose records are in the state
        /// after the feature. `None` states that the source stores no
        /// separate geometry for the feature.
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            deserialize_with = "deserialize_bodies"
        )]
        bodies: Option<Vec<BodyId>>,
        /// The tessellations a mesh import contributes.
        tessellations: Vec<TessellationId>,
        /// Where the geometry came from.
        origin: StoredOrigin,
    },
    /// An operation known without its operands.
    Unresolved {
        /// The operation's family.
        family: OperationFamily,
    },
    /// An operation whose meaning is not established.
    Native {
        /// The operation's kind, written `codec:kind`.
        kind: NonBlankString,
        /// Options whose meaning is not established.
        #[serde(deserialize_with = "cadmpeg_core::distinct_keys::btree_map")]
        native_options: NativeOptions,
        /// Options with no design meaning.
        #[serde(deserialize_with = "cadmpeg_core::distinct_keys::btree_map")]
        source_settings: NativeOptions,
    },
}

/// Where a feature's stored geometry came from.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum StoredOrigin {
    /// Geometry the source holds as the feature's own content, with no record
    /// of where it came from.
    Base {},
    /// Geometry the source records as imported from a file.
    Imported {},
    /// A copy of geometry defined elsewhere in the source that keeps no link
    /// to it.
    Snapshot {},
    /// A frozen copy that keeps its link.
    Linked(Box<Link>),
}

/// The link of a frozen copy of geometry to the geometry it copies.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct Link {
    /// The linked entities. Its picks' contexts state the source definition,
    /// the assembly the link runs through, and the configuration each level
    /// reads, and its anchor the state it reads.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_source"
    )]
    pub source: Option<SetSelection>,
    /// The source definition, and the configuration of it the link reads,
    /// only where `source` is absent or unread.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_document"
    )]
    pub document: Option<DefinitionRef>,
    /// A frozen placement of the copy.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_transform"
    )]
    pub transform: Option<Transform>,
    /// A uniform scale of the copy.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_scale"
    )]
    pub scale: Option<FiniteReal>,
    /// Whether the copy follows its source.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_associative"
    )]
    pub associative: Option<bool>,
    /// The source's record of the link's health.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_state"
    )]
    pub state: Option<LinkState>,
    /// The native record that holds the link.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_native_ref"
    )]
    pub native_ref: Option<String>,
}

/// The source's record of a link's associativity.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub enum LinkState {
    /// The copy matches its source.
    Current,
    /// The source changed after the copy was made.
    OutOfDate,
    /// The source does not resolve.
    Broken,
    /// The copy does not follow its source.
    Locked,
}

/// The fieldless discriminant of [`FeatureOperation`], one variant per arm
/// except `Unresolved`, `Native`, and `Stored`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub enum OperationFamily {
    /// A planar sketch.
    Sketch,
    /// A spatial sketch.
    SpatialSketch,
    /// A sheet.
    Spreadsheet,
    /// A datum.
    Datum,
    /// An extrusion.
    Extrude,
    /// A revolution.
    Revolve,
    /// A sweep.
    Sweep,
    /// A helical sweep.
    HelicalSweep,
    /// A rib.
    Rib,
    /// A primitive.
    Primitive,
    /// A loft.
    Loft,
    /// A fillet.
    Fillet,
    /// A chamfer.
    Chamfer,
    /// A shell.
    Shell,
    /// A draft.
    Draft,
    /// A thicken.
    Thicken,
    /// An offset.
    Offset,
    /// A hole.
    Hole,
    /// A thread.
    Thread,
    /// An emboss.
    Emboss,
    /// A dome.
    Dome,
    /// A decal.
    Decal,
    /// A pattern or mirror.
    Pattern,
    /// A template instance.
    TemplateInstance,
    /// A combine.
    Combine,
    /// A split.
    Split,
    /// A deletion of bodies.
    DeleteBodies,
    /// An extraction.
    Extract,
    /// A transformation of bodies.
    TransformBodies,
    /// A flex.
    Flex,
    /// A move of faces.
    MoveFaces,
    /// A deletion of faces.
    DeleteFaces,
    /// A replacement of faces.
    ReplaceFaces,
    /// A graft.
    Graft,
    /// A patch.
    Patch,
    /// A knit.
    Knit,
    /// An unsew.
    Unsew,
    /// An extension.
    Extend,
    /// An implicit-section sweep.
    ImplicitSweep,
    /// A boundary fill.
    BoundaryFill,
    /// A wireframe curve.
    Curve,
    /// A subdivision form.
    Form,
    /// A sheet-metal feature.
    SheetMetal,
    /// A placement of a component.
    Place,
    /// A joint.
    Joint,
    /// A rigid group.
    RigidGroup,
    /// A component pattern.
    ComponentPattern,
    /// A mirror of components.
    MirrorComponents,
    /// An assembly feature.
    AssemblyFeature,
    /// A material removal.
    MaterialRemoval,
}

cadmpeg_core::named_optional_field!(deserialize_associative<T>, T, "associative");
cadmpeg_core::named_optional_field!(deserialize_bodies<T>, T, "bodies");
cadmpeg_core::named_optional_field!(deserialize_component<T>, T, "component");
cadmpeg_core::named_optional_field!(deserialize_document<T>, T, "document");
cadmpeg_core::named_optional_field!(deserialize_driven_by<T>, T, "driven_by");
cadmpeg_core::named_optional_field!(deserialize_fuzzy_tolerance<T>, T, "fuzzy_tolerance");
cadmpeg_core::named_optional_field!(deserialize_name<T>, T, "name");
cadmpeg_core::named_optional_field!(deserialize_native_ref<T>, T, "native_ref");
cadmpeg_core::named_optional_field!(deserialize_placement<T>, T, "placement");
cadmpeg_core::named_optional_field!(deserialize_refine<T>, T, "refine");
cadmpeg_core::named_optional_field!(deserialize_scale<T>, T, "scale");
cadmpeg_core::named_optional_field!(deserialize_source<T>, T, "source");
cadmpeg_core::named_optional_field!(deserialize_state<T>, T, "state");
cadmpeg_core::named_optional_field!(deserialize_suppressed<T>, T, "suppressed");
cadmpeg_core::named_optional_field!(deserialize_transform<T>, T, "transform");
