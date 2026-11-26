// SPDX-License-Identifier: Apache-2.0
//! Features, their definitions, and stored geometry.
//!
//! A feature holds one operation and the cleanup the source states for its
//! result. The operation vocabulary holds typed operations, `Unresolved` for
//! an operation known without its operands, `Native` for one whose meaning is
//! not established, and `Stored` for a feature defined by stored geometry.

use cadmpeg_derive::DeserializeName;
#[cfg(feature = "schema")]
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use super::construction::{kinds, Construction};
use super::dimension::Dimension;
use super::names::{CodecName, NativeName};
use super::ops::assembly::{
    AssemblyContext, ComponentPattern, MirrorComponents, Place, PlaceSet, RigidGroup,
};
use super::ops::body::{
    Combine, DeleteBodies, DeleteFaces, Extract, Flex, Graft, MoveFaces, Pattern, ReplaceFaces,
    Split, TemplateInstance, TransformBodies,
};
use super::ops::datum::Datum;
use super::ops::dressup::{
    Chamfer, Decal, Dome, Draft, Emboss, Fillet, Hole, Offset, ShellFeature, Thicken, Thread,
};
use super::ops::shared::Joint;
use super::ops::sheet_metal::SheetMetal;
use super::ops::surface::{
    BoundaryFill, CurveFeature, Extend, Form, ImplicitSweep, Knit, MeshFeature, Patch, Unsew,
    UnsewFaces,
};
use super::ops::swept::{Extrude, HelicalSweep, Loft, Primitive, Revolve, Rib, Sweep};
use super::options::NativeOptions;
use super::selection::{DefinitionRef, SetSelection};
use crate::features::FeatureId;
use crate::ids::{BodyId, ProductDefinitionId};
use crate::scalar::{PositiveLength, PositiveReal};
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

/// A feature's one operation, the assembly context it runs in, and the
/// cleanup the source states for its result. On the wire it has no wrapper
/// level.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct FeatureDefinition {
    /// The operation.
    pub operation: FeatureOperation,
    /// The participants of an assembly feature, which runs its operation in
    /// an assembly over them. `None` on every other feature.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_assembly"
    )]
    pub assembly: Option<AssemblyContext>,
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
#[derive(Debug, Clone, PartialEq, Serialize, cadmpeg_derive::DeserializeTagged)]
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
#[derive(Debug, Clone, PartialEq, Serialize, cadmpeg_derive::DeserializeTagged)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(
    tag = "kind",
    content = "value",
    rename_all = "snake_case",
    deny_unknown_fields
)]
pub enum FuzzyTolerance {
    /// The modeling kernel's default tolerance.
    #[serde(deserialize_with = "cadmpeg_core::tag_only::refuse_value")]
    KernelDefault,
    /// A tolerance determined from the participating shapes.
    #[serde(deserialize_with = "cadmpeg_core::tag_only::refuse_value")]
    Automatic,
    /// A stated positive tolerance.
    Explicit(Dimension<PositiveLength>),
}

/// The operation of a feature: an arm per operation of the feature families
/// (§6.1 to §6.7), and the stored, unresolved, and native forms.
#[derive(Debug, Clone, PartialEq, Serialize, cadmpeg_derive::DeserializeTagged)]
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
    /// A datum (§6.2).
    Datum(Box<Datum>),
    /// An extrusion (§6.3).
    Extrude(Box<Extrude>),
    /// A revolution (§6.3).
    Revolve(Box<Revolve>),
    /// A sweep (§6.3).
    Sweep(Box<Sweep>),
    /// A helical sweep (§6.3).
    HelicalSweep(Box<HelicalSweep>),
    /// A rib (§6.3).
    Rib(Box<Rib>),
    /// A primitive (§6.3).
    Primitive(Box<Primitive>),
    /// A loft (§6.3).
    Loft(Box<Loft>),
    /// A fillet (§6.4).
    Fillet(Box<Fillet>),
    /// A chamfer (§6.4).
    Chamfer(Box<Chamfer>),
    /// A shell (§6.4).
    Shell(Box<ShellFeature>),
    /// A draft (§6.4).
    Draft(Box<Draft>),
    /// A thicken (§6.4).
    Thicken(Box<Thicken>),
    /// An offset (§6.4).
    Offset(Box<Offset>),
    /// A hole (§6.4).
    Hole(Box<Hole>),
    /// A thread (§6.4).
    Thread(Box<Thread>),
    /// An emboss (§6.4).
    Emboss(Box<Emboss>),
    /// A dome (§6.4).
    Dome(Box<Dome>),
    /// A decal (§6.4).
    Decal(Box<Decal>),
    /// A pattern or mirror (§6.5).
    Pattern(Box<Pattern>),
    /// A template instance (§6.5).
    TemplateInstance(Box<TemplateInstance>),
    /// A combine (§6.5).
    Combine(Box<Combine>),
    /// A split (§6.5).
    Split(Box<Split>),
    /// A deletion of bodies (§6.5).
    DeleteBodies(Box<DeleteBodies>),
    /// An extraction (§6.5).
    Extract(Box<Extract>),
    /// A transformation of bodies (§6.5).
    TransformBodies(Box<TransformBodies>),
    /// A flex (§6.5).
    Flex(Box<Flex>),
    /// A move of faces (§6.5).
    MoveFaces(Box<MoveFaces>),
    /// A deletion of faces (§6.5).
    DeleteFaces(Box<DeleteFaces>),
    /// A replacement of faces (§6.5).
    ReplaceFaces(Box<ReplaceFaces>),
    /// A graft (§6.5).
    Graft(Box<Graft>),
    /// A patch (§6.6).
    Patch(Box<Patch>),
    /// A knit (§6.6).
    Knit(Box<Knit>),
    /// An unsew (§6.6).
    Unsew(Box<Unsew>),
    /// An unsew of faces (§6.6).
    UnsewFaces(Box<UnsewFaces>),
    /// An extension (§6.6).
    Extend(Box<Extend>),
    /// An implicit-section sweep (§6.6).
    ImplicitSweep(Box<ImplicitSweep>),
    /// A boundary fill (§6.6).
    BoundaryFill(Box<BoundaryFill>),
    /// A wireframe curve (§6.6).
    Curve(Box<CurveFeature>),
    /// A subdivision form (§6.6).
    Form(Box<Form>),
    /// A sheet-metal feature (§6.6).
    SheetMetal(Box<SheetMetal>),
    /// A mesh feature (§6.6).
    Mesh(Box<MeshFeature>),
    /// A placement of one occurrence (§6.7).
    Place(Box<Place>),
    /// A placement of several occurrences in one feature (§6.7).
    PlaceSet(Box<PlaceSet>),
    /// A joint between two occurrences (§6.7).
    Joint(Box<Joint>),
    /// Occurrences held in their relative placement (§6.7).
    RigidGroup(Box<RigidGroup>),
    /// Components repeated along a layout (§6.7).
    ComponentPattern(Box<ComponentPattern>),
    /// Components mirrored across a plane (§6.7).
    MirrorComponents(Box<MirrorComponents>),
    /// A design feature whose removed volume is the volume a manufacturing
    /// operation machines.
    #[serde(deserialize_with = "cadmpeg_core::tag_only::refuse_value")]
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
        kind: CodecName,
        /// Options whose meaning is not established.
        #[serde(deserialize_with = "cadmpeg_core::distinct_keys::btree_map")]
        native_options: NativeOptions,
        /// Options with no design meaning.
        #[serde(deserialize_with = "cadmpeg_core::distinct_keys::btree_map")]
        source_settings: NativeOptions,
    },
}

impl FeatureOperation {
    /// Whether the operation's kind cannot change a state (§5.4): a sketch,
    /// a sheet, a datum, a pattern whose instances are features of their
    /// own, a template instance, a decal, a mesh section, and an
    /// `Unresolved` operation of a family whose every operation is one of
    /// these.
    pub(crate) fn cannot_change_state(&self) -> bool {
        match self {
            Self::Sketch(_)
            | Self::SpatialSketch(_)
            | Self::Spreadsheet(_)
            | Self::Datum(_)
            | Self::TemplateInstance(_)
            | Self::Decal(_) => true,
            Self::Pattern(pattern) => {
                matches!(
                    pattern.instances,
                    super::ops::body::PatternInstances::Members(_)
                )
            }
            Self::Mesh(mesh) => matches!(mesh.kind, super::ops::surface::MeshKind::Section { .. }),
            Self::Unresolved { family } => matches!(
                family,
                OperationFamily::Sketch
                    | OperationFamily::SpatialSketch
                    | OperationFamily::Spreadsheet
                    | OperationFamily::Datum
                    | OperationFamily::TemplateInstance
                    | OperationFamily::Decal
            ),
            _ => false,
        }
    }

    /// The operation's family: the [`OperationFamily`] whose wire name is
    /// the operation's `kind`. `Unresolved` states its own family; `Native`
    /// and `Stored` have none.
    pub(crate) fn family(&self) -> Option<OperationFamily> {
        if let Self::Unresolved { family } = self {
            return Some(*family);
        }
        let wire = crate::hash::finite_json::to_json_value(self).ok()?;
        cadmpeg_core::serde_erase::from_json_value::<OperationFamily>(wire.get("kind")?).ok()
    }
}

/// Where a feature's stored geometry came from.
#[derive(Debug, Clone, PartialEq, Serialize, cadmpeg_derive::DeserializeTagged)]
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
    /// The uniform factor the source stores as such, applied about the
    /// source definition's origin before `transform`.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_scale"
    )]
    pub scale: Option<PositiveReal>,
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
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, DeserializeName)]
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
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, DeserializeName)]
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
    /// An unsew of faces.
    UnsewFaces,
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
    /// A mesh feature.
    Mesh,
    /// A placement of a component.
    Place,
    /// A placement of a set of components.
    PlaceSet,
    /// A joint.
    Joint,
    /// A rigid group.
    RigidGroup,
    /// A component pattern.
    ComponentPattern,
    /// A mirror of components.
    MirrorComponents,
    /// A material removal.
    MaterialRemoval,
}

cadmpeg_core::named_optional_field!(deserialize_assembly<T>, T, "assembly");
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
