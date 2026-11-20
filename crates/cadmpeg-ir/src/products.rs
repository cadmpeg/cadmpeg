// SPDX-License-Identifier: Apache-2.0
//! Product definitions, the occurrences they own, and the documents they
//! come from (CADIR v11 §8.1).

use cadmpeg_derive::DeserializeName;
#[cfg(feature = "schema")]
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

use cadmpeg_core::text::NonBlankString;

use crate::design::refs::ConfigurationRef;
use crate::design::selection::OccurrencePath;
use crate::design::units::UnitSystem;
use crate::document::SourceMeta;
use crate::ids::{BodyId, OccurrenceId, ProductDefinitionId};
use crate::scalar::{FiniteReal, NonNegativeReal, NonZeroReal, PositiveReal};
use crate::sketches::SketchId;
use crate::tessellation::TessellationId;
use crate::transform::Transform;

crate::ids::id_type!(
    /// Stable assembly-joint identity.
    JointId, compose
);

mod general_tolerance;
pub use general_tolerance::{AngularGeneral, GeneralTolerance, LinearGeneral, LinearGeneralClass};
mod representation;
pub use representation::{
    MemberAction, MemberRule, Representation, RepresentationContent, RepresentationDetail,
    RepresentationMember,
};

/// The source's kind of a product definition.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, DeserializeName)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub enum ProductDefinitionKind {
    /// A part document's definition, or a `FreeCAD` `App::Part`.
    Part,
    /// An assembly document's definition.
    Assembly,
    /// A `FreeCAD` group.
    Group,
    /// A `FreeCAD` link group.
    LinkGroup,
    /// Any other `FreeCAD` object that an `App::Link` places.
    Object,
}

/// A reusable product definition: the bodies and occurrences it owns, and
/// the document it was read from.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct ProductDefinition {
    /// Globally unique definition identity.
    pub id: ProductDefinitionId,
    /// The source's kind of the definition; `None` where the source gives
    /// its definitions no kind (a Fusion component).
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_kind"
    )]
    pub kind: Option<ProductDefinitionKind>,
    /// The source's stable object name.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_source_name"
    )]
    pub source_name: Option<String>,
    /// The user-visible label, where it differs from the source name.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_label"
    )]
    pub label: Option<String>,
    /// The description field the source itself defines.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_description"
    )]
    pub description: Option<String>,
    /// The part-number field the source itself defines.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_part_number"
    )]
    pub part_number: Option<String>,
    /// The other BOM identity fields the source itself defines, by exact
    /// property name.
    #[serde(deserialize_with = "cadmpeg_core::distinct_keys::btree_map")]
    pub bom_properties: BTreeMap<NonBlankString, String>,
    /// The configuration of another definition this definition is a saved
    /// copy of: a table-driven member's factory, family, or generic.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_generated_from"
    )]
    pub generated_from: Option<GeneratedFrom>,
    /// Where the definition is stored.
    pub storage: Storage,
    /// The bodies of `CadIr.geometry` the definition owns.
    pub bodies: Vec<BodyId>,
    /// The tessellations of `CadIr.model` the definition owns that represent
    /// no body: the meshes of a part that the source states as triangles.
    pub tessellations: Vec<TessellationId>,
    /// The occurrences the definition owns, in source order.
    pub occurrences: Vec<OccurrenceId>,
    /// Bodies an instance below the definition shows in place of its
    /// prototype's bodies.
    pub instance_bodies: Vec<InstanceBodies>,
    /// Named contents an occurrence can show in place of the full content.
    pub representations: Vec<Representation>,
    /// The sketches of the model's sketch arena that the definition owns.
    /// The list names records of that arena and goes with it.
    pub sketches: Vec<SketchId>,
    /// The unit system the definition's source document is authored and
    /// displayed in.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_units"
    )]
    pub units: Option<UnitSystem>,
    /// The rule for every dimension that states no tolerance of its own.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_general_tolerance"
    )]
    pub general_tolerance: Option<GeneralTolerance>,
    /// `Some(false)` where the source deactivates the definition wherever it
    /// is placed.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_active"
    )]
    pub active: Option<bool>,
    /// Format-native object supplying this definition.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_native_ref"
    )]
    pub native_ref: Option<String>,
}

impl ProductDefinition {
    /// A definition with the given storage and nothing else stated.
    #[must_use]
    pub fn new(id: ProductDefinitionId, storage: Storage) -> Self {
        Self {
            id,
            kind: None,
            source_name: None,
            label: None,
            description: None,
            part_number: None,
            bom_properties: BTreeMap::new(),
            generated_from: None,
            storage,
            bodies: Vec::new(),
            tessellations: Vec::new(),
            occurrences: Vec::new(),
            instance_bodies: Vec::new(),
            representations: Vec::new(),
            sketches: Vec::new(),
            units: None,
            general_tolerance: None,
            active: None,
            native_ref: None,
        }
    }

    /// The root definition of a decode: stored in the root file, with the
    /// identities that file records about itself.
    #[must_use]
    pub fn root(id: ProductDefinitionId, document: ExternalDocument) -> Self {
        Self::new(
            id,
            Storage::File {
                document,
                source: None,
            },
        )
    }

    /// Whether this is a root definition: stored in the root file.
    #[must_use]
    pub const fn is_root(&self) -> bool {
        matches!(self.storage, Storage::File { source: None, .. })
    }

    /// Whether this is a root definition that states nothing beyond its kind
    /// and the bodies and meshes its file holds at its top: the document
    /// itself, which a writer of one document writes as that document and not
    /// as a component or an assembly level.
    #[must_use]
    pub fn is_document_root(&self) -> bool {
        matches!(&self.storage, Storage::File { document, source: None } if *document == ExternalDocument::default())
            && self.source_name.is_none()
            && self.label.is_none()
            && self.description.is_none()
            && self.part_number.is_none()
            && self.bom_properties.is_empty()
            && self.generated_from.is_none()
            && self.occurrences.is_empty()
            && self.instance_bodies.is_empty()
            && self.representations.is_empty()
            && self.sketches.is_empty()
            && self.units.is_none()
            && self.general_tolerance.is_none()
            && self.active.is_none()
            && self.native_ref.is_none()
    }
}

/// The definitions that state product structure: every one but a document
/// root ([`ProductDefinition::is_document_root`]).
pub fn stated_definitions(
    definitions: &[ProductDefinition],
) -> impl Iterator<Item = &ProductDefinition> {
    definitions
        .iter()
        .filter(|definition| !definition.is_document_root())
}

/// The configuration of another definition that a definition was generated
/// from.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct GeneratedFrom {
    /// The factory definition; `None` only where the member's own record
    /// proves its membership and names no factory.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_definition"
    )]
    pub definition: Option<ProductDefinitionId>,
    /// The factory's configuration the definition was generated from.
    pub configuration: ConfigurationRef,
}

/// Where a product definition is stored.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum Storage {
    /// Stored in the file of the definition that places it.
    #[serde(deserialize_with = "cadmpeg_core::tag_only::read")]
    Embedded,
    /// Read from its own file of the decode set, or the root definition.
    File {
        /// The identities the file records about itself.
        document: ExternalDocument,
        /// The file's own format, attributes, and dialect layers; `None` for
        /// the root definition, whose source is `CadIr.source`.
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            deserialize_with = "deserialize_file_source"
        )]
        source: Option<SourceMeta>,
    },
    /// A document that was not read.
    External(ExternalDocument),
}

/// Every identity a source records for a document link, and the object
/// inside the document that the link names.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct ExternalDocument {
    /// The document's path.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_path"
    )]
    pub path: Option<NonBlankString>,
    /// The document's identity.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_document_id"
    )]
    pub document_id: Option<NonBlankString>,
    /// The document's version.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_version"
    )]
    pub version: Option<NonBlankString>,
    /// The document's stamp.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_stamp"
    )]
    pub stamp: Option<NonBlankString>,
    /// The object inside the document that the link names.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_object"
    )]
    pub object: Option<NonBlankString>,
}

impl ExternalDocument {
    /// A document named by its path; no identity where the path is blank.
    pub fn path(path: impl Into<String>) -> Self {
        Self {
            path: NonBlankString::new(path),
            ..Self::default()
        }
    }

    /// A document named by its identity; no identity where it is blank.
    pub fn document_id(document_id: impl Into<String>) -> Self {
        Self {
            document_id: NonBlankString::new(document_id),
            ..Self::default()
        }
    }

    /// The same document with the object the link names inside it; no
    /// object where it is blank.
    #[must_use]
    pub fn with_object(self, object: impl Into<String>) -> Self {
        Self {
            object: NonBlankString::new(object),
            ..self
        }
    }
}

/// One placement of a prototype definition inside the definition that owns
/// the occurrence.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct Occurrence {
    /// Globally unique occurrence identity.
    pub id: OccurrenceId,
    /// The definition the occurrence places.
    pub prototype: ProductDefinitionId,
    /// The base placement in the owning definition's frame; `None` where the
    /// source states no placement.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_transform"
    )]
    pub transform: Option<Transform>,
    /// Whether the source holds the instance out of the product.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_suppressed"
    )]
    pub suppressed: Option<bool>,
    /// The instance's display state.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_visible"
    )]
    pub visible: Option<bool>,
    /// The representation of the prototype the occurrence shows.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_representation"
    )]
    pub representation: Option<NonBlankString>,
    /// Changes to instances nested inside the prototype, for this occurrence
    /// only, in every configuration.
    pub overrides: Vec<NestedOverride>,
    /// The source's occurrence name.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_name"
    )]
    pub name: Option<String>,
    /// Format-native object supplying this occurrence.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_native_ref"
    )]
    pub native_ref: Option<String>,
}

impl Occurrence {
    /// An occurrence of `prototype` at `transform`, with nothing else stated.
    #[must_use]
    pub const fn new(
        id: OccurrenceId,
        prototype: ProductDefinitionId,
        transform: Option<Transform>,
    ) -> Self {
        Self {
            id,
            prototype,
            transform,
            suppressed: None,
            visible: None,
            representation: None,
            overrides: Vec::new(),
            name: None,
            native_ref: None,
        }
    }
}

impl crate::document::CadIr {
    /// States the root definition (§8.1) and returns it: the first
    /// definition stored in the root file, or else a new one with `id` and
    /// the identities `document` the root file records about itself. The
    /// root then owns every body that no definition owns or shows for an
    /// instance, and every tessellation that represents no body and that no
    /// definition owns: what the file holds at its top. A new root is charged
    /// to `ctx` as one entity, so a decoder that admits its entity total calls
    /// this after its last admission. A container decode holds no model
    /// records, so it states none and returns `None`.
    pub fn state_root_definition(
        &mut self,
        ctx: &cadmpeg_core::decode::DecodeContext<'_>,
        id: ProductDefinitionId,
        document: ExternalDocument,
    ) -> Result<Option<&mut ProductDefinition>, cadmpeg_core::CodecError> {
        if ctx.container_only() {
            return Ok(None);
        }
        let definitions = &self.model.product_definitions;
        let held: std::collections::BTreeSet<&str> = definitions
            .iter()
            .flat_map(|definition| {
                definition.bodies.iter().chain(
                    definition
                        .instance_bodies
                        .iter()
                        .flat_map(|instance| &instance.bodies),
                )
            })
            .map(BodyId::as_str)
            .collect();
        let bodies: Vec<BodyId> = self
            .geometry
            .bodies
            .iter()
            .filter(|body| !held.contains(body.id.as_str()))
            .map(|body| body.id.clone())
            .collect();
        let listed: std::collections::BTreeSet<&str> = definitions
            .iter()
            .flat_map(|definition| &definition.tessellations)
            .map(TessellationId::as_str)
            .collect();
        let tessellations: Vec<TessellationId> = self
            .model
            .tessellations
            .iter()
            .filter(|tessellation| {
                tessellation.body.is_none() && !listed.contains(tessellation.id.as_str())
            })
            .map(|tessellation| tessellation.id.clone())
            .collect();
        let index = match definitions.iter().position(ProductDefinition::is_root) {
            Some(index) => index,
            None => {
                ctx.charge_entities(1, "root product definition")?;
                self.model
                    .product_definitions
                    .push(ProductDefinition::root(id, document));
                self.model.product_definitions.len() - 1
            }
        };
        let root = &mut self.model.product_definitions[index];
        root.bodies.extend(bodies);
        root.tessellations.extend(tessellations);
        Ok(Some(root))
    }
}

/// The definition that lists `occurrence` among the occurrences it owns.
pub fn owner<'ir>(
    definitions: &'ir [ProductDefinition],
    occurrence: &OccurrenceId,
) -> Option<&'ir ProductDefinition> {
    definitions
        .iter()
        .find(|definition| definition.occurrences.contains(occurrence))
}

/// The one occurrence that places `definition`, or `None` where no
/// occurrence or more than one places it.
pub fn sole_placement<'ir>(
    occurrences: &'ir [Occurrence],
    definition: &ProductDefinitionId,
) -> Option<&'ir Occurrence> {
    let mut placements = occurrences
        .iter()
        .filter(|occurrence| &occurrence.prototype == definition);
    let placement = placements.next()?;
    placements.next().is_none().then_some(placement)
}

/// The bodies an instance at a path shows in place of its prototype's
/// bodies, in the prototype's coordinates.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct InstanceBodies {
    /// The instance, by its path from the owning definition.
    pub occurrence: OccurrencePath,
    /// The bodies of `CadIr.geometry` it shows.
    pub bodies: Vec<BodyId>,
}

#[cfg(test)]
mod tests {
    mod joints;
    mod root;
    mod wire;
    use super::{ExternalDocument, JointLimits, JointOperand};
    use crate::ids::OccurrenceId;

    #[test]
    fn a_blank_selection_id_has_no_wire_spelling() {
        assert!(serde_json::from_value::<crate::features::SelectionMember>(
            serde_json::json!({"kind": "face", "id": "   "})
        )
        .is_err());
        assert!(serde_json::from_value::<crate::features::SelectionMember>(
            serde_json::json!({"kind": "face", "id": " a "})
        )
        .is_ok());
        let topology = |id: &str| {
            serde_json::json!({
                "id": "test:model:feature-result#1",
                "output_of": "test:test:feature#1",
                "members": [{"kind": "face", "id": id}],
            })
        };
        assert!(
            serde_json::from_value::<crate::features::FeatureResultTopology>(topology("   "))
                .is_err()
        );
        assert!(
            serde_json::from_value::<crate::features::FeatureResultTopology>(topology(" a "))
                .is_ok()
        );
    }

    #[test]
    fn a_joint_operand_states_the_container_that_owns_its_object() {
        let root = JointOperand::root("Body1", Vec::new());
        let root_wire = serde_json::to_value(&root).unwrap();
        assert_eq!(
            root_wire,
            serde_json::json!({"container": {"kind": "root"}, "object": "Body1", "subelements": []})
        );
        assert_eq!(
            serde_json::from_value::<JointOperand>(root_wire.clone()).unwrap(),
            root
        );

        let occurrence_id = OccurrenceId::mint("test:model:occurrence#0").unwrap();
        let occurrence = JointOperand::occurrence(occurrence_id.clone(), "Body1", Vec::new());
        let occurrence_wire = serde_json::to_value(&occurrence).unwrap();
        assert_eq!(occurrence_wire["container"]["kind"], "occurrence");
        assert_eq!(
            serde_json::from_value::<JointOperand>(occurrence_wire.clone()).unwrap(),
            occurrence
        );

        let external = JointOperand::external(
            ExternalDocument::path("parts/widget.FCStd"),
            "Body1",
            Vec::new(),
        );
        let external_wire = serde_json::to_value(&external).unwrap();
        assert_eq!(external_wire["container"]["kind"], "external");
        assert_eq!(
            external_wire["container"]["external_document"],
            serde_json::json!({"path": "parts/widget.FCStd"})
        );
        assert_eq!(
            serde_json::from_value::<JointOperand>(external_wire).unwrap(),
            external
        );

        let mut both = occurrence_wire;
        both["container"]["external_document"] = serde_json::json!({});
        let error = serde_json::from_value::<JointOperand>(both)
            .unwrap_err()
            .to_string();
        assert!(error.contains("external_document"), "{error}");

        let mut stray = root_wire.clone();
        stray["container"]["occurrence"] = serde_json::json!(occurrence_id.as_str());
        let error = serde_json::from_value::<JointOperand>(stray)
            .unwrap_err()
            .to_string();
        assert!(error.contains("occurrence"), "{error}");

        let mut without_object = root_wire;
        without_object.as_object_mut().unwrap().remove("object");
        let error = serde_json::from_value::<JointOperand>(without_object)
            .unwrap_err()
            .to_string();
        assert!(error.contains("object"), "{error}");
    }

    #[test]
    fn joint_limits_admit_only_finite_ordered_bounds() {
        assert!(JointLimits::new(None, None).is_none());
        assert!(JointLimits::new(Some(2.0), Some(1.0)).is_none());
        for value in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
            assert!(JointLimits::new(Some(value), None).is_none());
            assert!(JointLimits::new(None, Some(value)).is_none());
        }
        for (minimum, maximum) in [
            (Some(-2.0), None),
            (None, Some(-1.0)),
            (Some(0.0), Some(0.0)),
        ] {
            let limits = JointLimits::new(minimum, maximum).unwrap();
            assert_eq!(
                serde_json::from_value::<JointLimits>(serde_json::to_value(&limits).unwrap())
                    .unwrap(),
                limits
            );
        }
    }

    #[test]
    fn joint_limits_from_parts_checks_only_presence_and_order() {
        let lower = crate::scalar::FiniteReal::new(-2.0).unwrap();
        let upper = crate::scalar::FiniteReal::new(1.0).unwrap();
        assert!(JointLimits::from_parts(None, None).is_none());
        assert!(JointLimits::from_parts(Some(upper), Some(lower)).is_none());
        assert_eq!(
            JointLimits::from_parts(Some(lower), Some(upper)),
            Some(JointLimits::Range(
                super::JointLimitRange::new(lower, upper).unwrap()
            ))
        );
    }

    #[test]
    fn a_joint_limit_range_owns_the_order_of_its_bounds() {
        use super::JointLimitRange;
        use crate::scalar::FiniteReal;

        let bound = |value: f64| FiniteReal::new(value).unwrap();
        assert!(JointLimitRange::new(bound(2.0), bound(1.0)).is_none());
        let equal = JointLimitRange::new(bound(1.0), bound(1.0)).unwrap();
        assert_eq!([equal.minimum(), equal.maximum()], [bound(1.0), bound(1.0)]);
        let range = JointLimitRange::new(bound(-2.0), bound(3.0)).unwrap();
        assert_eq!(
            [range.minimum(), range.maximum()],
            [bound(-2.0), bound(3.0)]
        );

        let wire = serde_json::to_value(JointLimits::Range(range)).unwrap();
        assert_eq!(
            wire,
            serde_json::json!({"kind": "range", "minimum": -2.0, "maximum": 3.0})
        );
        assert_eq!(
            serde_json::from_value::<JointLimits>(wire).unwrap(),
            JointLimits::Range(range)
        );
        let error = serde_json::from_value::<JointLimits>(
            serde_json::json!({"kind": "range", "minimum": 2.0, "maximum": 1.0}),
        )
        .unwrap_err()
        .to_string();
        assert!(
            error.contains("joint limit range must be ordered"),
            "{error}"
        );
    }
}

/// Container that owns a joint operand object.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum OperandContainer {
    /// Object in the current document root.
    Root {},
    /// Object in a placed local occurrence.
    Occurrence {
        /// Placed local occurrence that owns the object.
        occurrence: OccurrenceId,
        /// Native keys of the nested occurrences, outermost first, that lead
        /// from the prototype of `occurrence` to the document that owns the
        /// object. Empty when the prototype of `occurrence` owns the object.
        occurrence_path: Vec<String>,
    },
    /// Object in an external document.
    External {
        /// External document that owns the object.
        external_document: ExternalDocument,
    },
}

/// One connector operand and its selected native subelements.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct JointOperand {
    /// Container that owns the object.
    pub container: OperandContainer,
    /// Exact referenced application object identity.
    pub object: String,
    /// Ordered persistent object/element paths.
    pub subelements: Vec<String>,
}

impl JointOperand {
    /// Constructs an operand owned by the current document root.
    pub fn root(object: impl Into<String>, subelements: Vec<String>) -> Self {
        Self {
            container: OperandContainer::Root {},
            object: object.into(),
            subelements,
        }
    }

    /// Constructs an operand owned by a local occurrence.
    pub fn occurrence(
        occurrence: OccurrenceId,
        object: impl Into<String>,
        subelements: Vec<String>,
    ) -> Self {
        Self::nested_occurrence(occurrence, Vec::new(), object, subelements)
    }

    /// Constructs an operand owned by a document nested below a local
    /// occurrence. `occurrence_path` lists the native keys of the nested
    /// occurrences, outermost first.
    pub fn nested_occurrence(
        occurrence: OccurrenceId,
        occurrence_path: Vec<String>,
        object: impl Into<String>,
        subelements: Vec<String>,
    ) -> Self {
        Self {
            container: OperandContainer::Occurrence {
                occurrence,
                occurrence_path,
            },
            object: object.into(),
            subelements,
        }
    }

    /// Constructs an operand owned by an external document.
    pub fn external(
        document: ExternalDocument,
        object: impl Into<String>,
        subelements: Vec<String>,
    ) -> Self {
        Self {
            container: OperandContainer::External {
                external_document: document,
            },
            object: object.into(),
            subelements,
        }
    }
}

/// Enabled bounds for one joint degree of freedom.
///
/// Each arm names the bounds the source enabled, so "neither bound" has no
/// spelling. The two-bound arm holds a [`JointLimitRange`], which owns the
/// order of its bounds.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(try_from = "JointLimitsWire", into = "JointLimitsWire")]
pub enum JointLimits {
    /// Only the lower bound is enabled.
    Minimum {
        /// Lower bound.
        minimum: FiniteReal,
    },
    /// Only the upper bound is enabled.
    Maximum {
        /// Upper bound.
        maximum: FiniteReal,
    },
    /// Both bounds are enabled, in order.
    Range(JointLimitRange),
}

/// Finite lower and upper joint bounds with the upper bound at or above the
/// lower bound.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct JointLimitRange {
    minimum: FiniteReal,
    maximum: FiniteReal,
}

impl JointLimitRange {
    /// Admit bounds whose upper bound is at or above the lower bound.
    #[must_use]
    pub fn new(minimum: FiniteReal, maximum: FiniteReal) -> Option<Self> {
        (minimum.get() <= maximum.get()).then_some(Self { minimum, maximum })
    }

    /// Return the lower bound.
    #[must_use]
    pub const fn minimum(self) -> FiniteReal {
        self.minimum
    }

    /// Return the upper bound, at or above [`Self::minimum`].
    #[must_use]
    pub const fn maximum(self) -> FiniteReal {
        self.maximum
    }
}

impl JointLimits {
    /// Constructs finite ordered limits with at least one enabled bound.
    pub fn new(minimum: Option<f64>, maximum: Option<f64>) -> Option<Self> {
        let minimum = match minimum {
            None => None,
            Some(value) => Some(FiniteReal::new(value)?),
        };
        let maximum = match maximum {
            None => None,
            Some(value) => Some(FiniteReal::new(value)?),
        };
        Self::from_parts(minimum, maximum)
    }

    /// Build from admitted bounds; check only presence and order.
    pub fn from_parts(minimum: Option<FiniteReal>, maximum: Option<FiniteReal>) -> Option<Self> {
        match (minimum, maximum) {
            (None, None) => None,
            (Some(minimum), None) => Some(Self::Minimum { minimum }),
            (None, Some(maximum)) => Some(Self::Maximum { maximum }),
            (Some(minimum), Some(maximum)) => {
                JointLimitRange::new(minimum, maximum).map(Self::Range)
            }
        }
    }
}

/// The wire spelling of [`JointLimits`], before the ordered-pair mint.
#[derive(Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(tag = "kind", rename_all = "snake_case")]
#[serde(deny_unknown_fields)]
enum JointLimitsWire {
    /// Only the lower bound is enabled.
    Minimum {
        /// Lower bound.
        minimum: FiniteReal,
    },
    /// Only the upper bound is enabled.
    Maximum {
        /// Upper bound.
        maximum: FiniteReal,
    },
    /// Both bounds are enabled, in order.
    Range {
        /// Lower bound.
        minimum: FiniteReal,
        /// Upper bound, at or above `minimum`.
        maximum: FiniteReal,
    },
}

impl From<JointLimits> for JointLimitsWire {
    fn from(limits: JointLimits) -> Self {
        match limits {
            JointLimits::Minimum { minimum } => Self::Minimum { minimum },
            JointLimits::Maximum { maximum } => Self::Maximum { maximum },
            JointLimits::Range(range) => Self::Range {
                minimum: range.minimum(),
                maximum: range.maximum(),
            },
        }
    }
}

impl TryFrom<JointLimitsWire> for JointLimits {
    type Error = &'static str;

    fn try_from(wire: JointLimitsWire) -> Result<Self, Self::Error> {
        Ok(match wire {
            JointLimitsWire::Minimum { minimum } => Self::Minimum { minimum },
            JointLimitsWire::Maximum { maximum } => Self::Maximum { maximum },
            JointLimitsWire::Range { minimum, maximum } => Self::Range(
                JointLimitRange::new(minimum, maximum)
                    .ok_or("joint limit range must be ordered")?,
            ),
        })
    }
}

#[cfg(feature = "schema")]
impl JsonSchema for JointLimits {
    fn schema_name() -> std::borrow::Cow<'static, str> {
        "JointLimits".into()
    }

    fn schema_id() -> std::borrow::Cow<'static, str> {
        concat!(module_path!(), "::JointLimits").into()
    }

    fn json_schema(generator: &mut schemars::SchemaGenerator) -> schemars::Schema {
        JointLimitsWire::json_schema(generator)
    }
}

/// One joint connector with its local frame.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct JointConnector {
    /// Referenced operand.
    pub operand: JointOperand,
    /// Connector-local frame.
    pub frame: Transform,
    /// Whether this connector is detached from the solve.
    pub detached: bool,
}

/// Structurally complete operands and frames for an assembly joint.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(
    tag = "kind",
    content = "value",
    rename_all = "snake_case",
    deny_unknown_fields
)]
pub enum JointOperands {
    /// One grounded connector and its optional attachment offset.
    Grounded {
        /// Grounded connector.
        connector: JointConnector,
        /// Connector attachment offset.
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            deserialize_with = "deserialize_offset_frame"
        )]
        offset_frame: Option<Transform>,
    },
    /// Two paired connectors and their optional attachment offsets.
    Pair {
        /// Non-grounded joint family.
        kind: PairedJointKind,
        /// Connectors in operand order.
        connectors: Box<[JointConnector; 2]>,
        /// Connector attachment offsets in operand order.
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            deserialize_with = "deserialize_offset_frames"
        )]
        offset_frames: Option<[Transform; 2]>,
        /// How the z axes of the two connector frames mate when the joint is
        /// solved, when the source states it: `aligned` is `z₁ = z₀` and
        /// `opposed` is `z₁ = −z₀`. A kind that states its own sense requires
        /// this one to agree or be absent.
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            deserialize_with = "deserialize_frame_sense"
        )]
        frame_sense: Option<PlaneSense>,
    },
}

/// Relative direction of the two normals of a plane coincidence or an insert,
/// or of the z axes of a joint's two connector frames.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, DeserializeName)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(rename_all = "snake_case")]
pub enum PlaneSense {
    /// The normals point toward each other: `z₁ = −z₀`.
    Opposed,
    /// The normals point the same way: `z₁ = z₀`.
    Aligned,
}

/// The side of a tangent plane on which the touching face lies.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, DeserializeName)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(rename_all = "snake_case")]
pub enum TangentContact {
    /// The face lies on the side that the plane normal points to.
    Outside,
    /// The face lies on the side opposite the plane normal.
    Inside,
}

/// Assembly-joint families that connect two operands.
///
/// `angle` and `translation_offset` relate the world frames of the first and
/// second connectors by `C0 = C1 * T(offset) * Rz(angle) * F`. `F` is the
/// identity for an aligned frame sense and a half-turn about the local X axis
/// for an opposed one. The offset and angle therefore apply on the second
/// connector.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum PairedJointKind {
    /// Rigid connection with no relative degrees of freedom.
    Fixed {
        /// Angular offset in radians.
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            deserialize_with = "deserialize_angle"
        )]
        angle: Option<FiniteReal>,
        /// Connector-local translation offset in document length units.
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            deserialize_with = "deserialize_translation_offset"
        )]
        translation_offset: Option<[FiniteReal; 3]>,
        /// Enabled angular interval in radians.
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            deserialize_with = "deserialize_angular_limits"
        )]
        angular_limits: Option<JointLimits>,
        /// Enabled linear interval in document length units.
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            deserialize_with = "deserialize_linear_limits"
        )]
        linear_limits: Option<JointLimits>,
    },
    /// Rotation about one axis.
    Revolute {
        /// Angular offset in radians.
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            deserialize_with = "deserialize_angle"
        )]
        angle: Option<FiniteReal>,
        /// Enabled angular interval in radians.
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            deserialize_with = "deserialize_angular_limits"
        )]
        angular_limits: Option<JointLimits>,
    },
    /// Translation along one axis.
    Slider {
        /// Primary linear offset in document length units.
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            deserialize_with = "deserialize_distance"
        )]
        distance: Option<FiniteReal>,
        /// Connector-local translation offset in document length units.
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            deserialize_with = "deserialize_translation_offset"
        )]
        translation_offset: Option<[FiniteReal; 3]>,
        /// Enabled linear interval in document length units.
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            deserialize_with = "deserialize_linear_limits"
        )]
        linear_limits: Option<JointLimits>,
    },
    /// Coupled rotation and translation on one axis.
    Cylindrical {
        /// Angular offset in radians.
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            deserialize_with = "deserialize_angle"
        )]
        angle: Option<FiniteReal>,
        /// Primary linear offset in document length units.
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            deserialize_with = "deserialize_distance"
        )]
        distance: Option<FiniteReal>,
        /// Enabled angular interval in radians.
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            deserialize_with = "deserialize_angular_limits"
        )]
        angular_limits: Option<JointLimits>,
        /// Enabled linear interval in document length units.
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            deserialize_with = "deserialize_linear_limits"
        )]
        linear_limits: Option<JointLimits>,
    },
    /// Rotation about a common point.
    Ball {},
    /// Maintains a scalar separation.
    Distance {
        /// Primary linear offset in document length units.
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            deserialize_with = "deserialize_distance"
        )]
        distance: Option<FiniteReal>,
    },
    /// Maintains parallel connector directions.
    Parallel {},
    /// Maintains perpendicular connector directions.
    Perpendicular {},
    /// Maintains an angular separation.
    Angle {
        /// Angular offset in radians.
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            deserialize_with = "deserialize_angle"
        )]
        angle: Option<FiniteReal>,
    },
    /// Couples rack translation to pinion rotation.
    RackPinion {
        /// Primary linear offset in document length units.
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            deserialize_with = "deserialize_distance"
        )]
        distance: Option<FiniteReal>,
        /// Secondary linear offset in document length units.
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            deserialize_with = "deserialize_distance2"
        )]
        distance2: Option<FiniteReal>,
        /// Pinion rotation in radians per millimetre of rack translation:
        /// positive when the rack moves along its slider axis as the pinion
        /// turns positively about its revolute axis.
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            deserialize_with = "deserialize_ratio"
        )]
        ratio: Option<NonZeroReal>,
        /// The revolute joint of the pinion and the slider joint of the rack that
        /// the coupling acts on, in that order. Empty when the coupling acts on
        /// the connectors directly.
        coupled_joints: Vec<JointId>,
    },
    /// Couples translation and rotation by screw pitch.
    Screw {
        /// Primary linear offset in document length units.
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            deserialize_with = "deserialize_distance"
        )]
        distance: Option<FiniteReal>,
    },
    /// Couples two gear rotations.
    Gears {
        /// Primary linear offset in document length units.
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            deserialize_with = "deserialize_distance"
        )]
        distance: Option<FiniteReal>,
        /// Secondary linear offset in document length units.
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            deserialize_with = "deserialize_distance2"
        )]
        distance2: Option<FiniteReal>,
        /// Rotation of the second gear per rotation of the first, each about its
        /// own revolute axis: negative when the gears turn opposite ways. For
        /// external gears with `N0` and `N1` teeth it is `-N0 / N1`, and for an
        /// internal gear `N0 / N1`.
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            deserialize_with = "deserialize_ratio"
        )]
        ratio: Option<NonZeroReal>,
        /// The revolute joints of the first and the second gear that the
        /// coupling acts on, in that order. Empty when the coupling acts on the
        /// connectors directly.
        coupled_joints: Vec<JointId>,
    },
    /// Couples two pulley rotations through a belt.
    Belt {
        /// Primary linear offset in document length units.
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            deserialize_with = "deserialize_distance"
        )]
        distance: Option<FiniteReal>,
        /// Secondary linear offset in document length units.
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            deserialize_with = "deserialize_distance2"
        )]
        distance2: Option<FiniteReal>,
    },
    /// Coincident connector planes. The z axis of each connector frame is
    /// the outward normal of its plane, and the frame origin lies on the plane.
    PlaneCoincidence {
        /// Relative direction of the two plane normals.
        sense: PlaneSense,
        /// Signed separation `(origin₁ − origin₀) · z₀` in document length units.
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            deserialize_with = "deserialize_offset"
        )]
        offset: Option<FiniteReal>,
    },
    /// Coincident connector axes. The z axis of each connector frame is the
    /// axis direction, and the frame origin lies on the axis. The two
    /// directions are parallel in either sense.
    AxisCoincidence {},
    /// The origin of the first connector frame lies at `distance` from the
    /// axis that the z axis of the second connector frame spans.
    PointOnAxis {
        /// Point-to-axis distance in document length units.
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            deserialize_with = "deserialize_distance"
        )]
        distance: Option<FiniteReal>,
    },
    /// Coincident circle axes with coincident or offset circle planes. The z
    /// axis of each connector frame is the normal of its circle, and the
    /// frame origin is the circle centre. Both origins lie on the line that
    /// the z axis of the first connector frame spans.
    Insert {
        /// Relative direction of the two circle normals.
        sense: PlaneSense,
        /// Signed separation `(origin₀ − origin₁) · z₀` in document length units.
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            deserialize_with = "deserialize_offset"
        )]
        offset: Option<FiniteReal>,
    },
    /// A cylindrical face of the first connector touches the plane of the
    /// second connector. The z axis of the first connector frame is the
    /// cylinder axis and its origin lies on that axis; the z axis of the
    /// second connector frame is the outward plane normal and its origin lies
    /// on the plane. The cylinder radius is that of the referenced face.
    Tangent {
        /// The side of the plane on which the cylinder lies.
        contact: TangentContact,
    },
    /// Future application-defined family retained without relabeling.
    Native {
        /// Application-defined family name.
        name: String,
        /// Angular offset in radians.
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            deserialize_with = "deserialize_angle"
        )]
        angle: Option<FiniteReal>,
        /// Connector-local translation offset in document length units.
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            deserialize_with = "deserialize_translation_offset"
        )]
        translation_offset: Option<[FiniteReal; 3]>,
        /// Primary linear offset in document length units.
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            deserialize_with = "deserialize_distance"
        )]
        distance: Option<FiniteReal>,
        /// Secondary linear offset in document length units.
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            deserialize_with = "deserialize_distance2"
        )]
        distance2: Option<FiniteReal>,
        /// Enabled angular interval in radians.
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            deserialize_with = "deserialize_angular_limits"
        )]
        angular_limits: Option<JointLimits>,
        /// Enabled linear interval in document length units.
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            deserialize_with = "deserialize_linear_limits"
        )]
        linear_limits: Option<JointLimits>,
    },
}

/// Neutral assembly constraint between connector frames.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct AssemblyJoint {
    /// Globally unique joint identity.
    pub id: JointId,
    /// Structurally complete connector state.
    operands: JointOperands,
    /// Whether solving this joint is suppressed.
    pub suppressed: bool,
    /// Design parameter whose value drives the offset, distance, or angle of the joint.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_driving_parameter"
    )]
    pub driving_parameter: Option<crate::features::ParameterId>,
    /// Motors and springs that drive degrees of freedom of the joint.
    pub drives: Vec<JointDrive>,
    /// Format-native joint record supplying this constraint.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_native_ref"
    )]
    pub native_ref: Option<String>,
}

/// An axis of a connector frame.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, DeserializeName)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(rename_all = "snake_case")]
pub enum FrameAxis {
    /// The x axis.
    X,
    /// The y axis.
    Y,
    /// The z axis.
    Z,
}

/// The degree of freedom that a drive acts on, along or about an axis of
/// the first connector frame.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum DriveAxis {
    /// Rotation about the axis.
    Rotation {
        /// The axis.
        axis: FrameAxis,
    },
    /// Translation along the axis.
    Translation {
        /// The axis.
        axis: FrameAxis,
    },
}

/// How a drive's output acts on the bodies.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, DeserializeName)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(rename_all = "snake_case")]
pub enum DriveMode {
    /// The output is a force or a torque.
    Force,
    /// The output is an acceleration, independent of the bodies' masses.
    Acceleration,
}

/// A motor or spring on one degree of freedom of a joint. The drive output is
/// `stiffness · (target_position − position) + damping · (target_velocity −
/// velocity)`, limited to `max_force`.
///
/// Rotations are radians and translations millimetres, and every coefficient
/// is in kilograms, millimetres, seconds, and radians: stiffness is
/// kg·mm²/(s²·rad), or kg/s²; damping kg·mm²/(s·rad), or kg/s; the force
/// limit kg·mm²/s², or kg·mm/s².
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct JointDrive {
    /// The driven degree of freedom.
    pub axis: DriveAxis,
    /// How the output acts.
    pub mode: DriveMode,
    /// Target position.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_target_position"
    )]
    pub target_position: Option<FiniteReal>,
    /// Target velocity per second.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_target_velocity"
    )]
    pub target_velocity: Option<FiniteReal>,
    /// Stiffness.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_stiffness"
    )]
    pub stiffness: Option<NonNegativeReal>,
    /// Damping.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_damping"
    )]
    pub damping: Option<NonNegativeReal>,
    /// Largest force or torque. Absent means unlimited.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_max_force"
    )]
    pub max_force: Option<PositiveReal>,
}

impl AssemblyJoint {
    /// Constructs a grounded joint with exactly one connector.
    pub fn grounded(
        id: JointId,
        connector: JointConnector,
        offset_frame: Option<Transform>,
    ) -> Self {
        Self::with_operands(
            id,
            JointOperands::Grounded {
                connector,
                offset_frame,
            },
        )
    }

    /// Constructs a non-grounded joint with exactly two connectors.
    pub fn paired(
        id: JointId,
        kind: PairedJointKind,
        connectors: [JointConnector; 2],
        offset_frames: Option<[Transform; 2]>,
    ) -> Self {
        Self::with_operands(
            id,
            JointOperands::Pair {
                kind,
                connectors: Box::new(connectors),
                offset_frames,
                frame_sense: None,
            },
        )
    }

    fn with_operands(id: JointId, operands: JointOperands) -> Self {
        Self {
            id,
            operands,
            suppressed: false,
            driving_parameter: None,
            drives: Vec::new(),
            native_ref: None,
        }
    }

    /// The connector state: one grounded connector, or two connectors and
    /// their paired family.
    pub fn operands(&self) -> &JointOperands {
        &self.operands
    }

    /// How the two connector frames of a paired joint mate, when the source
    /// states it. A grounded joint has one connector and states none.
    pub fn frame_sense(&self) -> Option<PlaneSense> {
        match &self.operands {
            JointOperands::Grounded { .. } => None,
            JointOperands::Pair { frame_sense, .. } => *frame_sense,
        }
    }

    /// Sets a paired joint's frame sense. A grounded joint has no second
    /// connector to mate with, so it keeps no sense and this returns `false`.
    pub fn set_frame_sense(&mut self, sense: Option<PlaneSense>) -> bool {
        match &mut self.operands {
            JointOperands::Grounded { .. } => false,
            JointOperands::Pair { frame_sense, .. } => {
                *frame_sense = sense;
                true
            }
        }
    }

    /// The sense a paired joint's kind states for itself: a plane coincidence
    /// or an insert.
    pub fn kind_sense(&self) -> Option<PlaneSense> {
        match &self.operands {
            JointOperands::Pair {
                kind:
                    PairedJointKind::PlaneCoincidence { sense, .. }
                    | PairedJointKind::Insert { sense, .. },
                ..
            } => Some(*sense),
            _ => None,
        }
    }

    /// Visits every connector in operand order.
    pub fn connectors(&self) -> impl Iterator<Item = &JointConnector> {
        let slice: &[JointConnector] = match &self.operands {
            JointOperands::Grounded { connector, .. } => std::slice::from_ref(connector),
            JointOperands::Pair { connectors, .. } => connectors.as_slice(),
        };
        slice.iter()
    }

    /// Visits every connector in operand order for mutation.
    pub fn connectors_mut(&mut self) -> impl Iterator<Item = &mut JointConnector> {
        let slice: &mut [JointConnector] = match &mut self.operands {
            JointOperands::Grounded { connector, .. } => std::slice::from_mut(connector),
            JointOperands::Pair { connectors, .. } => connectors.as_mut_slice(),
        };
        slice.iter_mut()
    }
}

#[cfg(test)]
mod nonblank_literal_tests {
    /// A whitespace-leading template fails the build, so the probe below is
    /// the run-time reach the macro does not have: the `const { … }` block
    /// evaluates the constructor at compile time at every use.
    #[test]
    fn a_formatted_literal_keeps_its_non_blank_prefix() {
        assert_eq!(
            cadmpeg_core::nonblank_literal!("sldprt:marker-relation:{}", 34).as_str(),
            "sldprt:marker-relation:34"
        );
        assert_eq!(cadmpeg_core::nonblank_literal!("d6").as_str(), "d6");
    }
}

// Each optional key below names itself in whatever it refuses.
cadmpeg_core::named_optional_field!(deserialize_kind, ProductDefinitionKind, "kind");
cadmpeg_core::named_optional_field!(deserialize_source_name, String, "source_name");
cadmpeg_core::named_optional_field!(deserialize_label, String, "label");
cadmpeg_core::named_optional_field!(deserialize_description, String, "description");
cadmpeg_core::named_optional_field!(deserialize_part_number, String, "part_number");
cadmpeg_core::named_optional_field!(deserialize_generated_from, GeneratedFrom, "generated_from");
cadmpeg_core::named_optional_field!(deserialize_units, UnitSystem, "units");
cadmpeg_core::named_optional_field!(
    deserialize_general_tolerance,
    GeneralTolerance,
    "general_tolerance"
);
cadmpeg_core::named_optional_field!(deserialize_active, bool, "active");
cadmpeg_core::named_optional_field!(deserialize_native_ref, String, "native_ref");
cadmpeg_core::named_optional_field!(deserialize_definition, ProductDefinitionId, "definition");
cadmpeg_core::named_optional_field!(deserialize_file_source, SourceMeta, "source");
cadmpeg_core::named_optional_field!(deserialize_path, NonBlankString, "path");
cadmpeg_core::named_optional_field!(deserialize_document_id, NonBlankString, "document_id");
cadmpeg_core::named_optional_field!(deserialize_version, NonBlankString, "version");
cadmpeg_core::named_optional_field!(deserialize_stamp, NonBlankString, "stamp");
cadmpeg_core::named_optional_field!(deserialize_object, NonBlankString, "object");
cadmpeg_core::named_optional_field!(deserialize_name, String, "name");
cadmpeg_core::named_optional_field!(deserialize_offset_frame, Transform, "offset_frame");
cadmpeg_core::named_optional_field!(deserialize_offset_frames, [Transform; 2], "offset_frames");
cadmpeg_core::named_optional_field!(deserialize_angle, FiniteReal, "angle");
cadmpeg_core::named_optional_field!(
    deserialize_translation_offset,
    [FiniteReal; 3],
    "translation_offset"
);
cadmpeg_core::named_optional_field!(deserialize_angular_limits, JointLimits, "angular_limits");
cadmpeg_core::named_optional_field!(deserialize_linear_limits, JointLimits, "linear_limits");
cadmpeg_core::named_optional_field!(deserialize_distance, FiniteReal, "distance");
cadmpeg_core::named_optional_field!(deserialize_distance2, FiniteReal, "distance2");
cadmpeg_core::named_optional_field!(deserialize_ratio, NonZeroReal, "ratio");
cadmpeg_core::named_optional_field!(deserialize_offset, FiniteReal, "offset");
cadmpeg_core::named_optional_field!(deserialize_frame_sense, PlaneSense, "frame_sense");
cadmpeg_core::named_optional_field!(deserialize_target_position, FiniteReal, "target_position");
cadmpeg_core::named_optional_field!(deserialize_target_velocity, FiniteReal, "target_velocity");
cadmpeg_core::named_optional_field!(deserialize_stiffness, NonNegativeReal, "stiffness");
cadmpeg_core::named_optional_field!(deserialize_damping, NonNegativeReal, "damping");
cadmpeg_core::named_optional_field!(deserialize_max_force, PositiveReal, "max_force");
cadmpeg_core::named_optional_field!(
    deserialize_driving_parameter,
    crate::features::ParameterId,
    "driving_parameter"
);
cadmpeg_core::named_optional_field!(deserialize_representation<T>, T, "representation");
cadmpeg_core::named_optional_field!(deserialize_suppressed<T>, T, "suppressed");
cadmpeg_core::named_optional_field!(deserialize_transform<T>, T, "transform");
cadmpeg_core::named_optional_field!(deserialize_visible<T>, T, "visible");

mod identity_rewrite;

mod nested_override;
pub use nested_override::NestedOverride;
