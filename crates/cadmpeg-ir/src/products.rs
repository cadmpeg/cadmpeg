// SPDX-License-Identifier: Apache-2.0
//! Neutral product structure and occurrence instancing.

#[cfg(feature = "schema")]
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::collections::{HashMap, HashSet};

use crate::ids::{BodyId, OccurrenceId, ProductDefinitionId};
use crate::transform::Transform;

/// Stable assembly-joint identity.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(transparent)]
pub struct JointId(#[serde(serialize_with = "crate::schema::serialize_reference_id")] pub String);

/// Role of a component definition in the product tree.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(rename_all = "snake_case")]
pub enum ProductDefinitionKind {
    /// Product part or assembly container.
    Part,
    /// Generic ordered object group.
    Group,
    /// Container whose children are link instances.
    LinkGroup,
    /// Reusable leaf object without container semantics.
    Object,
}

/// A reusable product definition or structural container.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub struct ProductDefinition {
    /// Globally unique definition identity.
    pub id: ProductDefinitionId,
    /// Structural role.
    pub kind: ProductDefinitionKind,
    /// Stable source object name used by product/BOM tooling.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_name: Option<String>,
    /// User-visible component label, when distinct from the source name.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
    /// User-maintained BOM description.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    /// User-maintained part or stock number.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub part_number: Option<String>,
    /// Additional persisted BOM identity fields by exact property name.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub bom_properties: BTreeMap<String, String>,
    /// Shape bodies owned by this reusable definition.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub bodies: Vec<BodyId>,
    /// Format-native object supplying this definition.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub native_ref: Option<String>,
}

/// Local or unresolved external prototype of an occurrence.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(tag = "scope", rename_all = "snake_case")]
pub enum PrototypeReference {
    /// Prototype resolves to a definition in this document.
    Local {
        /// Resolved component definition.
        definition: ProductDefinitionId,
    },
    /// Prototype belongs to another document, loaded or not.
    External {
        /// Persisted external-document reference and unresolved state.
        document: ExternalDocumentReference,
        /// Persisted object identity within that document.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        object: Option<String>,
    },
    /// The source intentionally carries no resolvable prototype.
    Unresolved,
}

/// Typed identity of an external document.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ExternalDocument {
    /// File path stored by the source.
    Path(String),
    /// Document identity stored by the source.
    DocumentId(String),
}

/// First-class external document reference without implicit loading.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExternalDocumentReference {
    /// Exact persisted document identity.
    pub document: ExternalDocument,
    /// Deterministic resolution state; decoding never opens external documents.
    pub resolution: ExternalResolution,
}

impl ExternalDocumentReference {
    /// Constructs a reference identified by a persisted file path.
    pub fn path(path: impl Into<String>, resolution: ExternalResolution) -> Self {
        Self {
            document: ExternalDocument::Path(path.into()),
            resolution,
        }
    }

    /// Constructs a reference identified by a persisted document id.
    pub fn document_id(document_id: impl Into<String>, resolution: ExternalResolution) -> Self {
        Self {
            document: ExternalDocument::DocumentId(document_id.into()),
            resolution,
        }
    }

    /// Returns the persisted file path, when the reference uses one.
    pub fn as_path(&self) -> Option<&str> {
        match &self.document {
            ExternalDocument::Path(path) => Some(path),
            ExternalDocument::DocumentId(_) => None,
        }
    }

    /// Returns the persisted document id, when the reference uses one.
    pub fn as_document_id(&self) -> Option<&str> {
        match &self.document {
            ExternalDocument::Path(_) => None,
            ExternalDocument::DocumentId(document_id) => Some(document_id),
        }
    }
}

#[derive(Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
struct ExternalDocumentReferenceWire {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    path: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    document_id: Option<String>,
    resolution: ExternalResolution,
}

impl Serialize for ExternalDocumentReference {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        let (path, document_id) = match &self.document {
            ExternalDocument::Path(path) => (Some(path.clone()), None),
            ExternalDocument::DocumentId(document_id) => (None, Some(document_id.clone())),
        };
        ExternalDocumentReferenceWire {
            path,
            document_id,
            resolution: self.resolution.clone(),
        }
        .serialize(serializer)
    }
}

impl<'de> Deserialize<'de> for ExternalDocumentReference {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let wire = ExternalDocumentReferenceWire::deserialize(deserializer)?;
        let document = match (wire.path, wire.document_id) {
            (Some(path), None) => ExternalDocument::Path(path),
            (None, Some(document_id)) => ExternalDocument::DocumentId(document_id),
            _ => {
                return Err(serde::de::Error::custom(
                    "external document reference must contain exactly one of path or document_id",
                ));
            }
        };
        Ok(Self {
            document,
            resolution: wire.resolution,
        })
    }
}

#[cfg(feature = "schema")]
impl JsonSchema for ExternalDocumentReference {
    fn schema_name() -> std::borrow::Cow<'static, str> {
        "ExternalDocumentReference".into()
    }

    fn schema_id() -> std::borrow::Cow<'static, str> {
        concat!(module_path!(), "::ExternalDocumentReference").into()
    }

    fn json_schema(generator: &mut schemars::SchemaGenerator) -> schemars::Schema {
        ExternalDocumentReferenceWire::json_schema(generator)
    }
}

/// Resolution state of an external product reference.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(rename_all = "snake_case")]
pub enum ExternalResolution {
    /// Target document was not loaded by this decode.
    Unresolved,
    /// Persisted reference was present but empty or structurally unusable.
    MissingReference,
}

/// Copy-on-change ownership behavior of a link.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(tag = "policy", content = "native_policy", rename_all = "snake_case")]
pub enum CopyOnChangePolicy {
    /// Link follows its prototype without making an owned copy.
    Disabled,
    /// Copy is created when a marked prototype property changes.
    Enabled,
    /// Link currently owns a changed copy.
    Owned,
    /// Owned copy continues tracking its original source.
    Tracking,
    /// Future policy retained without reinterpretation.
    Native(String),
}

/// Position of an occurrence in the canonical placed-instance tree.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum OccurrenceParent {
    /// A root occurrence has no containing occurrence.
    Root,
    /// A child is placed inside another occurrence.
    Occurrence {
        /// Containing occurrence identity.
        occurrence: OccurrenceId,
    },
}

/// One placed use, including an element of a link array.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub struct Occurrence {
    /// Globally unique instance identity.
    pub id: OccurrenceId,
    /// Reusable definition used by this instance.
    pub prototype: PrototypeReference,
    /// Position in the occurrence tree.
    pub parent: OccurrenceParent,
    /// Stable zero-based source order within the parent.
    pub ordinal: u32,
    /// Placement relative to the direct container.
    pub transform: Transform,
    /// Linked prototype placement contribution when link-transform policy applies.
    #[serde(flatten, with = "linked_prototype_wire")]
    #[cfg_attr(feature = "schema", schemars(with = "LinkedPrototypeWire"))]
    pub linked_prototype: Option<Transform>,
    /// Per-axis instance scale.
    pub scale: [f64; 3],
    /// Source occurrence identifier or display name.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    /// Per-element visibility override.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub visible: Option<bool>,
    /// FreeCAD App::Link-specific occurrence state.
    #[serde(flatten, with = "link_state_wire")]
    #[cfg_attr(feature = "schema", schemars(with = "LinkStateWire"))]
    pub link: Option<LinkState>,
    /// Format-native object supplying this instance.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub native_ref: Option<String>,
}

/// FreeCAD App::Link-specific occurrence state.
#[derive(Debug, Clone, PartialEq)]
pub struct LinkState {
    /// Persisted prototype subelement selection.
    pub linked_subelements: Vec<String>,
    /// Explicit application object representing this array element.
    pub element_component: Option<ProductDefinitionId>,
    /// Whether this link claims its prototype in the source tree.
    pub claim_child: Option<bool>,
    /// Copy-on-change ownership state, when enabled on the link.
    pub copy_on_change: Option<CopyOnChange>,
}

/// Copy-on-change ownership state carried by an App::Link occurrence.
#[derive(Debug, Clone, PartialEq)]
pub struct CopyOnChange {
    /// Ownership policy.
    pub policy: CopyOnChangePolicy,
    /// Original component tracked by copy-on-change.
    pub source: Option<ProductDefinitionId>,
    /// Internal component holding owned copies.
    pub group: Option<ProductDefinitionId>,
    /// Whether the tracked source was persisted as changed.
    pub touched: Option<bool>,
}

impl Occurrence {
    /// Placement after applying the linked prototype contribution, when present.
    #[must_use]
    pub fn effective_transform(&self) -> Transform {
        self.linked_prototype.map_or(self.transform, |prototype| {
            self.transform.compose(prototype)
        })
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
struct LinkedPrototypeWire {
    #[serde(default)]
    prototype_transform: Transform,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    link_transform: Option<bool>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
struct LinkStateWire {
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    linked_subelements: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    element_component: Option<ProductDefinitionId>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    claim_child: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    copy_on_change: Option<CopyOnChangePolicy>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    copy_on_change_source: Option<ProductDefinitionId>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    copy_on_change_group: Option<ProductDefinitionId>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    copy_on_change_touched: Option<bool>,
}

mod linked_prototype_wire {
    use super::LinkedPrototypeWire;
    use crate::transform::Transform;
    use serde::{de::Error, Deserialize, Deserializer, Serialize, Serializer};

    pub fn serialize<S>(value: &Option<Transform>, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        LinkedPrototypeWire {
            prototype_transform: value.unwrap_or_else(Transform::identity),
            link_transform: value.map(|_| true),
        }
        .serialize(serializer)
    }

    pub fn deserialize<'de, D>(deserializer: D) -> Result<Option<Transform>, D::Error>
    where
        D: Deserializer<'de>,
    {
        let wire = LinkedPrototypeWire::deserialize(deserializer)?;
        match wire.link_transform {
            Some(true) => Ok(Some(wire.prototype_transform)),
            None | Some(false) if wire.prototype_transform == Transform::identity() => Ok(None),
            None | Some(false) => Err(D::Error::custom(
                "prototype_transform must be identity unless link_transform is true",
            )),
        }
    }
}

mod link_state_wire {
    use super::{CopyOnChange, LinkState, LinkStateWire};
    use serde::{de::Error, Deserialize, Deserializer, Serialize, Serializer};

    pub fn serialize<S>(value: &Option<LinkState>, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        let wire = value.as_ref().map_or_else(
            || LinkStateWire {
                linked_subelements: Vec::new(),
                element_component: None,
                claim_child: None,
                copy_on_change: None,
                copy_on_change_source: None,
                copy_on_change_group: None,
                copy_on_change_touched: None,
            },
            |link| LinkStateWire {
                linked_subelements: link.linked_subelements.clone(),
                element_component: link.element_component.clone(),
                claim_child: link.claim_child,
                copy_on_change: link.copy_on_change.as_ref().map(|copy| copy.policy.clone()),
                copy_on_change_source: link
                    .copy_on_change
                    .as_ref()
                    .and_then(|copy| copy.source.clone()),
                copy_on_change_group: link
                    .copy_on_change
                    .as_ref()
                    .and_then(|copy| copy.group.clone()),
                copy_on_change_touched: link.copy_on_change.as_ref().and_then(|copy| copy.touched),
            },
        );
        wire.serialize(serializer)
    }

    pub fn deserialize<'de, D>(deserializer: D) -> Result<Option<LinkState>, D::Error>
    where
        D: Deserializer<'de>,
    {
        let wire = LinkStateWire::deserialize(deserializer)?;
        let copy_payload_present = wire.copy_on_change_source.is_some()
            || wire.copy_on_change_group.is_some()
            || wire.copy_on_change_touched.is_some();
        let copy_on_change = match wire.copy_on_change {
            Some(policy) => Some(CopyOnChange {
                policy,
                source: wire.copy_on_change_source,
                group: wire.copy_on_change_group,
                touched: wire.copy_on_change_touched,
            }),
            None if !copy_payload_present => None,
            None => {
                return Err(D::Error::custom(
                    "copy_on_change_source, copy_on_change_group, and \
                     copy_on_change_touched require copy_on_change",
                ));
            }
        };
        let present = !wire.linked_subelements.is_empty()
            || wire.element_component.is_some()
            || wire.claim_child.is_some()
            || copy_on_change.is_some();
        Ok(present.then_some(LinkState {
            linked_subelements: wire.linked_subelements,
            element_component: wire.element_component,
            claim_child: wire.claim_child,
            copy_on_change,
        }))
    }
}

/// Failure to construct a canonical occurrence graph.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AssemblyGraphError {
    /// Two occurrences carry the same identity.
    DuplicateOccurrence(OccurrenceId),
    /// An occurrence names a parent that is not present.
    MissingParent {
        /// Child occurrence.
        occurrence: OccurrenceId,
        /// Missing parent occurrence.
        parent: OccurrenceId,
    },
    /// Parent links contain a cycle.
    ParentCycle(OccurrenceId),
}

impl std::fmt::Display for AssemblyGraphError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::DuplicateOccurrence(id) => write!(formatter, "duplicate occurrence {id}"),
            Self::MissingParent { occurrence, parent } => {
                write!(
                    formatter,
                    "occurrence {occurrence} has missing parent {parent}"
                )
            }
            Self::ParentCycle(id) => write!(formatter, "occurrence parent cycle at {id}"),
        }
    }
}

impl std::error::Error for AssemblyGraphError {}

/// Validated, memoized view over a canonical occurrence tree.
pub struct AssemblyGraph<'a> {
    occurrences: HashMap<&'a str, &'a Occurrence>,
    resolved: HashMap<&'a str, Transform>,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn translation(x: f64) -> Transform {
        let mut transform = Transform::identity();
        transform.rows[0][3] = x;
        transform
    }

    fn occurrence(id: &str, parent: OccurrenceParent, x: f64) -> Occurrence {
        Occurrence {
            id: OccurrenceId(id.into()),
            prototype: PrototypeReference::Unresolved,
            parent,
            ordinal: 0,
            transform: translation(x),
            linked_prototype: None,
            scale: [1.0; 3],
            name: None,
            visible: None,
            link: None,
            native_ref: None,
        }
    }

    #[test]
    fn resolves_parent_chains_and_conditional_prototype_placement() {
        let root = occurrence("root", OccurrenceParent::Root, 1.0);
        let mut child = occurrence(
            "child",
            OccurrenceParent::Occurrence {
                occurrence: root.id.clone(),
            },
            2.0,
        );
        child.linked_prototype = Some(translation(10.0));
        let occurrences = [child, root];
        let graph = AssemblyGraph::new(&occurrences).expect("valid graph");
        assert_eq!(
            graph
                .resolved_transform(&OccurrenceId("child".into()))
                .expect("resolved child")
                .rows[0][3],
            13.0
        );
    }

    #[test]
    fn linked_prototype_wire_preserves_the_legacy_fields_and_rejects_ignored_transforms() {
        let plain = occurrence("plain", OccurrenceParent::Root, 1.0);
        let mut plain_wire = serde_json::to_value(&plain).expect("plain occurrence wire");
        assert_eq!(
            plain_wire.get("prototype_transform"),
            Some(&serde_json::to_value(Transform::identity()).unwrap())
        );
        assert!(plain_wire.get("link_transform").is_none());

        plain_wire["link_transform"] = serde_json::json!(false);
        let decoded: Occurrence = serde_json::from_value(plain_wire.clone()).unwrap();
        assert_eq!(decoded.linked_prototype, None);

        let mut linked = plain;
        linked.linked_prototype = Some(translation(10.0));
        let linked_wire = serde_json::to_value(&linked).expect("linked occurrence wire");
        assert_eq!(
            linked_wire.get("link_transform"),
            Some(&serde_json::json!(true))
        );
        assert_eq!(
            serde_json::from_value::<Occurrence>(linked_wire).unwrap(),
            linked
        );

        plain_wire["prototype_transform"] = serde_json::to_value(translation(10.0)).unwrap();
        assert!(serde_json::from_value::<Occurrence>(plain_wire).is_err());
    }

    #[test]
    fn link_state_wire_preserves_the_legacy_fields_and_requires_a_copy_policy() {
        let mut linked = occurrence("link", OccurrenceParent::Root, 1.0);
        linked.link = Some(LinkState {
            linked_subelements: vec!["Face1".into()],
            element_component: Some(ProductDefinitionId("test:product#element".into())),
            claim_child: Some(true),
            copy_on_change: Some(CopyOnChange {
                policy: CopyOnChangePolicy::Owned,
                source: Some(ProductDefinitionId("test:product#source".into())),
                group: Some(ProductDefinitionId("test:product#group".into())),
                touched: Some(true),
            }),
        });
        let wire = serde_json::to_value(&linked).expect("App::Link occurrence wire");
        assert_eq!(wire["linked_subelements"], serde_json::json!(["Face1"]));
        assert_eq!(
            wire["copy_on_change"],
            serde_json::json!({"policy": "owned"})
        );
        assert_eq!(serde_json::from_value::<Occurrence>(wire).unwrap(), linked);

        let mut invalid =
            serde_json::to_value(occurrence("invalid-link", OccurrenceParent::Root, 1.0)).unwrap();
        invalid["copy_on_change_source"] = serde_json::json!("test:product#source");
        assert!(serde_json::from_value::<Occurrence>(invalid).is_err());
    }

    #[test]
    fn rejects_duplicate_missing_and_cyclic_parent_links() {
        let duplicate = occurrence("same", OccurrenceParent::Root, 0.0);
        assert!(matches!(
            AssemblyGraph::new(&[duplicate.clone(), duplicate]),
            Err(AssemblyGraphError::DuplicateOccurrence(_))
        ));

        let missing = occurrence(
            "child",
            OccurrenceParent::Occurrence {
                occurrence: OccurrenceId("missing".into()),
            },
            0.0,
        );
        assert!(matches!(
            AssemblyGraph::new(&[missing]),
            Err(AssemblyGraphError::MissingParent { .. })
        ));

        let first = occurrence(
            "first",
            OccurrenceParent::Occurrence {
                occurrence: OccurrenceId("second".into()),
            },
            0.0,
        );
        let second = occurrence(
            "second",
            OccurrenceParent::Occurrence {
                occurrence: OccurrenceId("first".into()),
            },
            0.0,
        );
        assert!(matches!(
            AssemblyGraph::new(&[first, second]),
            Err(AssemblyGraphError::ParentCycle(_))
        ));
    }
}

impl<'a> AssemblyGraph<'a> {
    /// Validates parent links and precomputes every resolved occurrence transform.
    pub fn new(occurrences: &'a [Occurrence]) -> Result<Self, AssemblyGraphError> {
        let mut by_id = HashMap::with_capacity(occurrences.len());
        for occurrence in occurrences {
            if by_id.insert(occurrence.id.as_str(), occurrence).is_some() {
                return Err(AssemblyGraphError::DuplicateOccurrence(
                    occurrence.id.clone(),
                ));
            }
        }
        let mut resolved = HashMap::with_capacity(occurrences.len());
        for occurrence in occurrences {
            resolve_occurrence(occurrence, &by_id, &mut resolved, &mut HashSet::new())?;
        }
        Ok(Self {
            occurrences: by_id,
            resolved,
        })
    }

    /// Returns an occurrence by identity.
    pub fn occurrence(&self, id: &OccurrenceId) -> Option<&'a Occurrence> {
        self.occurrences.get(id.as_str()).copied()
    }

    /// Returns the transform composed from the root through this occurrence.
    pub fn resolved_transform(&self, id: &OccurrenceId) -> Option<Transform> {
        self.resolved.get(id.as_str()).copied()
    }
}

fn resolve_occurrence<'a>(
    occurrence: &'a Occurrence,
    occurrences: &HashMap<&'a str, &'a Occurrence>,
    resolved: &mut HashMap<&'a str, Transform>,
    active: &mut HashSet<&'a str>,
) -> Result<Transform, AssemblyGraphError> {
    if let Some(transform) = resolved.get(occurrence.id.as_str()) {
        return Ok(*transform);
    }
    if !active.insert(occurrence.id.as_str()) {
        return Err(AssemblyGraphError::ParentCycle(occurrence.id.clone()));
    }
    let parent = match &occurrence.parent {
        OccurrenceParent::Root => Transform::identity(),
        OccurrenceParent::Occurrence {
            occurrence: parent_id,
        } => {
            let Some(parent_occurrence) = occurrences.get(parent_id.as_str()).copied() else {
                return Err(AssemblyGraphError::MissingParent {
                    occurrence: occurrence.id.clone(),
                    parent: parent_id.clone(),
                });
            };
            resolve_occurrence(parent_occurrence, occurrences, resolved, active)?
        }
    };
    let transform = parent.compose(occurrence.effective_transform());
    active.remove(occurrence.id.as_str());
    resolved.insert(occurrence.id.as_str(), transform);
    Ok(transform)
}

/// Neutral family of an assembly joint.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(tag = "kind", content = "native_kind", rename_all = "snake_case")]
pub enum JointKind {
    /// Rigid connection with no relative degrees of freedom.
    Fixed,
    /// Rotation about one axis.
    Revolute,
    /// Translation along one axis.
    Slider,
    /// Coupled rotation and translation on one axis.
    Cylindrical,
    /// Rotation about a common point.
    Ball,
    /// Maintains a scalar separation.
    Distance,
    /// Maintains parallel connector directions.
    Parallel,
    /// Maintains perpendicular connector directions.
    Perpendicular,
    /// Maintains an angular separation.
    Angle,
    /// Couples rack translation to pinion rotation.
    RackPinion,
    /// Couples translation and rotation by screw pitch.
    Screw,
    /// Couples two gear rotations.
    Gears,
    /// Couples two pulley rotations through a belt.
    Belt,
    /// Persisted grounding of a component.
    Grounded,
    /// Future application-defined family retained without relabeling.
    Native(String),
}

/// Container that owns a joint operand object.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OperandContainer {
    /// Object in the current document root.
    Root,
    /// Object in a placed local occurrence.
    Occurrence(OccurrenceId),
    /// Object in an external document.
    External(ExternalDocumentReference),
}

/// One connector operand and its selected native subelements.
#[derive(Debug, Clone, PartialEq, Eq)]
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
            container: OperandContainer::Root,
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
        Self {
            container: OperandContainer::Occurrence(occurrence),
            object: object.into(),
            subelements,
        }
    }

    /// Constructs an operand owned by an external document.
    pub fn external(
        document: ExternalDocumentReference,
        object: impl Into<String>,
        subelements: Vec<String>,
    ) -> Self {
        Self {
            container: OperandContainer::External(document),
            object: object.into(),
            subelements,
        }
    }
}

#[derive(Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
struct JointOperandWire {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    occurrence: Option<OccurrenceId>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    external_document: Option<ExternalDocumentReference>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    object: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    subelements: Vec<String>,
}

impl Serialize for JointOperand {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        let (occurrence, external_document) = match &self.container {
            OperandContainer::Root => (None, None),
            OperandContainer::Occurrence(occurrence) => (Some(occurrence.clone()), None),
            OperandContainer::External(document) => (None, Some(document.clone())),
        };
        JointOperandWire {
            occurrence,
            external_document,
            object: Some(self.object.clone()),
            subelements: self.subelements.clone(),
        }
        .serialize(serializer)
    }
}

impl<'de> Deserialize<'de> for JointOperand {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let wire = JointOperandWire::deserialize(deserializer)?;
        let container = match (wire.occurrence, wire.external_document) {
            (None, None) => OperandContainer::Root,
            (Some(occurrence), None) => OperandContainer::Occurrence(occurrence),
            (None, Some(document)) => OperandContainer::External(document),
            (Some(_), Some(_)) => {
                return Err(serde::de::Error::custom(
                    "joint operand cannot name both an occurrence and an external document",
                ));
            }
        };
        Ok(Self {
            container,
            object: wire
                .object
                .ok_or_else(|| serde::de::Error::missing_field("object"))?,
            subelements: wire.subelements,
        })
    }
}

#[cfg(feature = "schema")]
impl JsonSchema for JointOperand {
    fn schema_name() -> std::borrow::Cow<'static, str> {
        "JointOperand".into()
    }

    fn schema_id() -> std::borrow::Cow<'static, str> {
        concat!(module_path!(), "::JointOperand").into()
    }

    fn json_schema(generator: &mut schemars::SchemaGenerator) -> schemars::Schema {
        JointOperandWire::json_schema(generator)
    }
}

/// Enabled bounds for one joint degree of freedom.
#[derive(Debug, Clone, PartialEq)]
pub enum JointLimits {
    /// Lower bound only.
    Minimum(f64),
    /// Upper bound only.
    Maximum(f64),
    /// Lower and upper bounds.
    Both {
        /// Lower bound.
        minimum: f64,
        /// Upper bound.
        maximum: f64,
    },
}

impl JointLimits {
    /// Constructs enabled bounds when at least one bound is present.
    pub fn new(minimum: Option<f64>, maximum: Option<f64>) -> Option<Self> {
        match (minimum, maximum) {
            (Some(minimum), None) => Some(Self::Minimum(minimum)),
            (None, Some(maximum)) => Some(Self::Maximum(maximum)),
            (Some(minimum), Some(maximum)) => Some(Self::Both { minimum, maximum }),
            (None, None) => None,
        }
    }

    /// Returns the lower bound, when enabled.
    pub fn minimum(&self) -> Option<f64> {
        match *self {
            Self::Minimum(minimum) | Self::Both { minimum, .. } => Some(minimum),
            Self::Maximum(_) => None,
        }
    }

    /// Returns the upper bound, when enabled.
    pub fn maximum(&self) -> Option<f64> {
        match *self {
            Self::Maximum(maximum) | Self::Both { maximum, .. } => Some(maximum),
            Self::Minimum(_) => None,
        }
    }
}

#[derive(Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
struct JointLimitsWire {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    minimum: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    maximum: Option<f64>,
}

impl Serialize for JointLimits {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        JointLimitsWire {
            minimum: self.minimum(),
            maximum: self.maximum(),
        }
        .serialize(serializer)
    }
}

impl<'de> Deserialize<'de> for JointLimits {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let wire = JointLimitsWire::deserialize(deserializer)?;
        Self::new(wire.minimum, wire.maximum)
            .ok_or_else(|| serde::de::Error::custom("joint limits must contain at least one bound"))
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
#[derive(Debug, Clone, PartialEq)]
pub struct JointConnector {
    /// Referenced operand.
    pub operand: JointOperand,
    /// Connector-local frame.
    pub frame: Transform,
}

/// Structurally complete operands and frames for an assembly joint.
#[derive(Debug, Clone, PartialEq)]
// Inline fixed-size arrays encode the one-or-two connector invariant directly.
#[allow(clippy::large_enum_variant)]
pub enum JointOperands {
    /// One grounded connector and its optional attachment offset.
    Grounded {
        /// Grounded connector.
        connector: JointConnector,
        /// Connector attachment offset.
        offset_frame: Option<Transform>,
    },
    /// Two paired connectors and their optional attachment offsets.
    Pair {
        /// Non-grounded joint family.
        kind: PairedJointKind,
        /// Connectors in operand order.
        connectors: [JointConnector; 2],
        /// Connector attachment offsets in operand order.
        offset_frames: Option<[Transform; 2]>,
    },
}

/// Assembly-joint families that connect two operands.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PairedJointKind {
    /// Rigid connection with no relative degrees of freedom.
    Fixed,
    /// Rotation about one axis.
    Revolute,
    /// Translation along one axis.
    Slider,
    /// Coupled rotation and translation on one axis.
    Cylindrical,
    /// Rotation about a common point.
    Ball,
    /// Maintains a scalar separation.
    Distance,
    /// Maintains parallel connector directions.
    Parallel,
    /// Maintains perpendicular connector directions.
    Perpendicular,
    /// Maintains an angular separation.
    Angle,
    /// Couples rack translation to pinion rotation.
    RackPinion,
    /// Couples translation and rotation by screw pitch.
    Screw,
    /// Couples two gear rotations.
    Gears,
    /// Couples two pulley rotations through a belt.
    Belt,
    /// Future application-defined family retained without relabeling.
    Native(String),
}

impl From<PairedJointKind> for JointKind {
    fn from(kind: PairedJointKind) -> Self {
        match kind {
            PairedJointKind::Fixed => Self::Fixed,
            PairedJointKind::Revolute => Self::Revolute,
            PairedJointKind::Slider => Self::Slider,
            PairedJointKind::Cylindrical => Self::Cylindrical,
            PairedJointKind::Ball => Self::Ball,
            PairedJointKind::Distance => Self::Distance,
            PairedJointKind::Parallel => Self::Parallel,
            PairedJointKind::Perpendicular => Self::Perpendicular,
            PairedJointKind::Angle => Self::Angle,
            PairedJointKind::RackPinion => Self::RackPinion,
            PairedJointKind::Screw => Self::Screw,
            PairedJointKind::Gears => Self::Gears,
            PairedJointKind::Belt => Self::Belt,
            PairedJointKind::Native(kind) => Self::Native(kind),
        }
    }
}

impl TryFrom<JointKind> for PairedJointKind {
    type Error = ();

    fn try_from(kind: JointKind) -> Result<Self, Self::Error> {
        match kind {
            JointKind::Fixed => Ok(Self::Fixed),
            JointKind::Revolute => Ok(Self::Revolute),
            JointKind::Slider => Ok(Self::Slider),
            JointKind::Cylindrical => Ok(Self::Cylindrical),
            JointKind::Ball => Ok(Self::Ball),
            JointKind::Distance => Ok(Self::Distance),
            JointKind::Parallel => Ok(Self::Parallel),
            JointKind::Perpendicular => Ok(Self::Perpendicular),
            JointKind::Angle => Ok(Self::Angle),
            JointKind::RackPinion => Ok(Self::RackPinion),
            JointKind::Screw => Ok(Self::Screw),
            JointKind::Gears => Ok(Self::Gears),
            JointKind::Belt => Ok(Self::Belt),
            JointKind::Native(kind) => Ok(Self::Native(kind)),
            JointKind::Grounded => Err(()),
        }
    }
}

/// Neutral assembly constraint between connector frames.
#[derive(Debug, Clone, PartialEq)]
pub struct AssemblyJoint {
    /// Globally unique joint identity.
    pub id: JointId,
    /// Structurally complete connector state.
    operands: JointOperands,
    /// Whether solving this joint is suppressed.
    pub suppressed: bool,
    /// Per-connector detach flags.
    pub detached: [bool; 2],
    /// Angular offset in radians.
    pub angle: Option<f64>,
    /// Connector-local translation offset in document length units.
    pub translation_offset: Option<[f64; 3]>,
    /// Primary linear offset in document length units.
    pub distance: Option<f64>,
    /// Secondary linear offset in document length units.
    pub distance2: Option<f64>,
    /// Enabled angular interval in radians.
    pub angular_limits: Option<JointLimits>,
    /// Enabled linear interval in document length units.
    pub linear_limits: Option<JointLimits>,
    /// Exact persisted scalar state, including future controls.
    pub properties: BTreeMap<String, String>,
    /// Format-native joint record supplying this constraint.
    pub native_ref: Option<String>,
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
                connectors,
                offset_frames,
            },
        )
    }

    fn with_operands(id: JointId, operands: JointOperands) -> Self {
        Self {
            id,
            operands,
            suppressed: false,
            detached: [false; 2],
            angle: None,
            translation_offset: None,
            distance: None,
            distance2: None,
            angular_limits: None,
            linear_limits: None,
            properties: BTreeMap::new(),
            native_ref: None,
        }
    }

    /// Returns the joint kinematic family.
    pub fn kind(&self) -> JointKind {
        match &self.operands {
            JointOperands::Grounded { .. } => JointKind::Grounded,
            JointOperands::Pair { kind, .. } => kind.clone().into(),
        }
    }

    /// Returns the structurally complete operand and frame state.
    pub fn operands(&self) -> &JointOperands {
        &self.operands
    }

    /// Visits every connector in operand order.
    pub fn connectors(&self) -> impl Iterator<Item = &JointConnector> {
        let slice: &[JointConnector] = match &self.operands {
            JointOperands::Grounded { connector, .. } => std::slice::from_ref(connector),
            JointOperands::Pair { connectors, .. } => connectors,
        };
        slice.iter()
    }

    /// Visits every attachment offset in operand order.
    pub fn offset_frames(&self) -> impl Iterator<Item = &Transform> {
        let slice: &[Transform] = match &self.operands {
            JointOperands::Grounded {
                offset_frame: Some(offset),
                ..
            } => std::slice::from_ref(offset),
            JointOperands::Pair {
                offset_frames: Some(offsets),
                ..
            } => offsets,
            _ => &[],
        };
        slice.iter()
    }
}

#[derive(Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
struct AssemblyJointWire {
    id: JointId,
    kind: JointKind,
    operands: Vec<JointOperand>,
    frames: Vec<Transform>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    offset_frames: Vec<Transform>,
    suppressed: bool,
    detached: [bool; 2],
    #[serde(default, skip_serializing_if = "Option::is_none")]
    angle: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    translation_offset: Option<[f64; 3]>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    distance: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    distance2: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    angular_limits: Option<JointLimits>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    linear_limits: Option<JointLimits>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    properties: BTreeMap<String, String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    native_ref: Option<String>,
}

impl From<&AssemblyJoint> for AssemblyJointWire {
    fn from(joint: &AssemblyJoint) -> Self {
        let (operands, frames, offset_frames) = match &joint.operands {
            JointOperands::Grounded {
                connector,
                offset_frame,
            } => (
                vec![connector.operand.clone()],
                vec![connector.frame],
                offset_frame.iter().copied().collect(),
            ),
            JointOperands::Pair {
                connectors,
                offset_frames,
                ..
            } => (
                connectors
                    .iter()
                    .map(|connector| connector.operand.clone())
                    .collect(),
                connectors.iter().map(|connector| connector.frame).collect(),
                offset_frames.iter().flatten().copied().collect::<Vec<_>>(),
            ),
        };
        Self {
            id: joint.id.clone(),
            kind: joint.kind(),
            operands,
            frames,
            offset_frames,
            suppressed: joint.suppressed,
            detached: joint.detached,
            angle: joint.angle,
            translation_offset: joint.translation_offset,
            distance: joint.distance,
            distance2: joint.distance2,
            angular_limits: joint.angular_limits.clone(),
            linear_limits: joint.linear_limits.clone(),
            properties: joint.properties.clone(),
            native_ref: joint.native_ref.clone(),
        }
    }
}

impl TryFrom<AssemblyJointWire> for AssemblyJoint {
    type Error = &'static str;

    fn try_from(wire: AssemblyJointWire) -> Result<Self, Self::Error> {
        let AssemblyJointWire {
            id,
            kind,
            operands,
            frames,
            offset_frames,
            suppressed,
            detached,
            angle,
            translation_offset,
            distance,
            distance2,
            angular_limits,
            linear_limits,
            properties,
            native_ref,
        } = wire;
        let mut joint = if kind == JointKind::Grounded {
            let [operand] = operands
                .try_into()
                .map_err(|_| "grounded joint must contain one operand")?;
            let [frame] = frames
                .try_into()
                .map_err(|_| "grounded joint must contain one frame")?;
            let offset_frame = match offset_frames.as_slice() {
                [] => None,
                [offset] => Some(*offset),
                _ => return Err("grounded joint must contain zero or one offset frame"),
            };
            Self::grounded(id, JointConnector { operand, frame }, offset_frame)
        } else {
            let kind = PairedJointKind::try_from(kind)
                .map_err(|()| "paired joint cannot use the grounded kind")?;
            let [first_operand, second_operand] = operands
                .try_into()
                .map_err(|_| "paired joint must contain two operands")?;
            let [first_frame, second_frame] = frames
                .try_into()
                .map_err(|_| "paired joint must contain two frames")?;
            let offset_frames = if offset_frames.is_empty() {
                None
            } else {
                Some(
                    <Vec<Transform> as TryInto<[Transform; 2]>>::try_into(offset_frames)
                        .map_err(|_| "paired joint must contain zero or two offset frames")?,
                )
            };
            Self::paired(
                id,
                kind,
                [
                    JointConnector {
                        operand: first_operand,
                        frame: first_frame,
                    },
                    JointConnector {
                        operand: second_operand,
                        frame: second_frame,
                    },
                ],
                offset_frames,
            )
        };
        joint.suppressed = suppressed;
        joint.detached = detached;
        joint.angle = angle;
        joint.translation_offset = translation_offset;
        joint.distance = distance;
        joint.distance2 = distance2;
        joint.angular_limits = angular_limits;
        joint.linear_limits = linear_limits;
        joint.properties = properties;
        joint.native_ref = native_ref;
        Ok(joint)
    }
}

impl Serialize for AssemblyJoint {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        AssemblyJointWire::from(self).serialize(serializer)
    }
}

impl<'de> Deserialize<'de> for AssemblyJoint {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        AssemblyJointWire::deserialize(deserializer)?
            .try_into()
            .map_err(serde::de::Error::custom)
    }
}

#[cfg(feature = "schema")]
impl JsonSchema for AssemblyJoint {
    fn schema_name() -> std::borrow::Cow<'static, str> {
        "AssemblyJoint".into()
    }

    fn schema_id() -> std::borrow::Cow<'static, str> {
        concat!(module_path!(), "::AssemblyJoint").into()
    }

    fn json_schema(generator: &mut schemars::SchemaGenerator) -> schemars::Schema {
        AssemblyJointWire::json_schema(generator)
    }
}
