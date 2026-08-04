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

/// First-class external document reference without implicit loading.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub struct ExternalDocumentReference {
    /// File path when the source explicitly stores a file attribute.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,
    /// Document identity when the source explicitly stores a document attribute.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub document_id: Option<String>,
    /// Deterministic resolution state; decoding never opens external documents.
    pub resolution: ExternalResolution,
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
    /// Linked prototype placement contribution selected by link-transform policy.
    #[serde(default)]
    pub prototype_transform: Transform,
    /// Per-axis instance scale.
    pub scale: [f64; 3],
    /// Source occurrence identifier or display name.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    /// Persisted prototype subelement selection.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub linked_subelements: Vec<String>,
    /// Per-element visibility override.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub visible: Option<bool>,
    /// Explicit application object representing this array element.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub element_component: Option<ProductDefinitionId>,
    /// Whether this link claims its prototype in the source tree.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub claim_child: Option<bool>,
    /// Copy-on-change ownership policy.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub copy_on_change: Option<CopyOnChangePolicy>,
    /// Original component tracked by copy-on-change.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub copy_on_change_source: Option<ProductDefinitionId>,
    /// Internal component holding owned copies.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub copy_on_change_group: Option<ProductDefinitionId>,
    /// Whether the tracked source was persisted as changed.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub copy_on_change_touched: Option<bool>,
    /// Whether the prototype placement participates in evaluation.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub link_transform: Option<bool>,
    /// Format-native object supplying this instance.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub native_ref: Option<String>,
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
            prototype_transform: translation(10.0),
            scale: [1.0; 3],
            name: None,
            linked_subelements: Vec::new(),
            visible: None,
            element_component: None,
            claim_child: None,
            copy_on_change: None,
            copy_on_change_source: None,
            copy_on_change_group: None,
            copy_on_change_touched: None,
            link_transform: None,
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
        child.link_transform = Some(true);
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
    let mut transform = parent.compose(occurrence.transform);
    if occurrence.link_transform.unwrap_or(false) {
        transform = transform.compose(occurrence.prototype_transform);
    }
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

/// One connector operand and its selected native subelements.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub struct JointOperand {
    /// Local placed occurrence when the object resolves within this document.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub occurrence: Option<OccurrenceId>,
    /// External document token when resolution is intentionally deferred.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub external_document: Option<ExternalDocumentReference>,
    /// Exact referenced application object identity.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub object: Option<String>,
    /// Ordered persistent object/element paths.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub subelements: Vec<String>,
}

/// Optional enabled interval for a joint degree of freedom.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub struct JointLimits {
    /// Lower bound when enabled.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub minimum: Option<f64>,
    /// Upper bound when enabled.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub maximum: Option<f64>,
}

/// Neutral assembly constraint between connector frames.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub struct AssemblyJoint {
    /// Globally unique joint identity.
    pub id: JointId,
    /// Joint kinematic family.
    pub kind: JointKind,
    /// Ordered connector or grounded-object operands.
    pub operands: Vec<JointOperand>,
    /// Connector-local frames in operand order.
    pub frames: Vec<Transform>,
    /// Connector attachment offsets in operand order.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub offset_frames: Vec<Transform>,
    /// Whether solving this joint is suppressed.
    pub suppressed: bool,
    /// Per-connector detach flags.
    pub detached: [bool; 2],
    /// Angular offset in radians.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub angle: Option<f64>,
    /// Connector-local translation offset in document length units.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub translation_offset: Option<[f64; 3]>,
    /// Primary linear offset in document length units.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub distance: Option<f64>,
    /// Secondary linear offset in document length units.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub distance2: Option<f64>,
    /// Enabled angular interval in radians.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub angular_limits: Option<JointLimits>,
    /// Enabled linear interval in document length units.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub linear_limits: Option<JointLimits>,
    /// Exact persisted scalar state, including future controls.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub properties: BTreeMap<String, String>,
    /// Format-native joint record supplying this constraint.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub native_ref: Option<String>,
}
