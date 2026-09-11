// SPDX-License-Identifier: Apache-2.0
//! Neutral product structure and occurrence instancing.

#[cfg(feature = "schema")]
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::collections::{HashMap, HashSet};

use crate::ids::{BodyId, OccurrenceId, ProductDefinitionId};
use crate::scalar::FiniteReal;
use crate::transform::Transform;

crate::ids::id_type!(
    /// Stable assembly-joint identity.
    JointId
);

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

/// A source string that is not empty.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(transparent)]
pub struct NonEmptyString(String);

impl NonEmptyString {
    /// Constructs a non-empty source string.
    pub fn new(value: impl Into<String>) -> Option<Self> {
        let value = value.into();
        (!value.is_empty()).then_some(Self(value))
    }

    /// Constructs a non-empty string from a leading character and a suffix.
    pub fn prefixed(prefix: char, suffix: impl std::fmt::Display) -> Self {
        Self(format!("{prefix}{suffix}"))
    }

    /// Returns the source string.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// Builds a [`NonEmptyString`] from a format literal whose first character is literal text.
#[macro_export]
macro_rules! nonempty_literal {
    ($template:literal $(, $argument:expr)* $(,)?) => {{
        const FIRST: char = {
            let bytes = $template.as_bytes();
            assert!(
                !bytes.is_empty() && bytes[0].is_ascii() && bytes[0] != b'{',
                "a nonempty literal must start with literal ASCII text",
            );
            bytes[0] as char
        };
        let rendered = format!($template $(, $argument)*);
        $crate::products::NonEmptyString::prefixed(FIRST, &rendered[1..])
    }};
}

impl PartialEq<str> for NonEmptyString {
    fn eq(&self, other: &str) -> bool {
        self.0 == other
    }
}

impl PartialEq<&str> for NonEmptyString {
    fn eq(&self, other: &&str) -> bool {
        self == *other
    }
}

impl std::fmt::Display for NonEmptyString {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(self.as_str())
    }
}

impl<'de> Deserialize<'de> for NonEmptyString {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        Self::new(String::deserialize(deserializer)?)
            .ok_or_else(|| serde::de::Error::custom("external document identity must not be empty"))
    }
}

/// Typed identity or explicit absence of an external document.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(tag = "resolution", rename_all = "snake_case", deny_unknown_fields)]
pub enum ExternalDocument {
    /// Non-empty file path stored by the source.
    Path {
        /// Persisted file path.
        path: NonEmptyString,
    },
    /// Non-empty document identity stored by the source.
    DocumentId {
        /// Persisted document identity.
        document_id: NonEmptyString,
    },
    /// Persisted reference was empty or structurally unusable.
    Missing {},
}

impl ExternalDocument {
    /// Constructs a path reference, or [`Self::Missing`] when the path is empty.
    pub fn path(path: impl Into<String>) -> Self {
        match NonEmptyString::new(path) {
            Some(path) => Self::Path { path },
            None => Self::Missing {},
        }
    }

    /// Constructs a document-id reference, or [`Self::Missing`] when the id is empty.
    pub fn document_id(document_id: impl Into<String>) -> Self {
        match NonEmptyString::new(document_id) {
            Some(document_id) => Self::DocumentId { document_id },
            None => Self::Missing {},
        }
    }

    /// Returns the explicit missing-reference state.
    pub fn missing() -> Self {
        Self::Missing {}
    }

    /// Returns the persisted file path, when the reference uses one.
    pub fn as_path(&self) -> Option<&str> {
        match self {
            Self::Path { path } => Some(path.as_str()),
            Self::DocumentId { .. } | Self::Missing {} => None,
        }
    }

    /// Returns the persisted document id, when the reference uses one.
    pub fn as_document_id(&self) -> Option<&str> {
        match self {
            Self::DocumentId { document_id } => Some(document_id.as_str()),
            Self::Path { .. } | Self::Missing {} => None,
        }
    }

    /// Returns whether the source carried no usable external document identity.
    pub fn is_missing(&self) -> bool {
        matches!(self, Self::Missing {})
    }
}

/// First-class external document reference without implicit loading.
pub type ExternalDocumentReference = ExternalDocument;

/// Copy-on-change ownership behavior of a link.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(
    tag = "policy",
    content = "native_policy",
    rename_all = "snake_case",
    deny_unknown_fields
)]
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
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub linked_prototype: Option<Transform>,
    /// Per-axis instance scale.
    #[serde(deserialize_with = "deserialize_occurrence_scale")]
    pub scale: [FiniteReal; 3],
    /// Source occurrence identifier or display name.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    /// Per-element visibility override.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub visible: Option<bool>,
    /// `FreeCAD` `App::Link`-specific occurrence state.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub link: Option<LinkState>,
    /// Format-native object supplying this instance.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub native_ref: Option<String>,
}

/// `FreeCAD` `App::Link`-specific occurrence state.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(try_from = "LinkStateWire")]
pub struct LinkState {
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    linked_subelements: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    element_component: Option<ProductDefinitionId>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    claim_child: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    copy_on_change: Option<CopyOnChange>,
}

impl LinkState {
    /// Nonempty link state, or absence when all members are empty.
    pub fn new(
        linked_subelements: Vec<String>,
        element_component: Option<ProductDefinitionId>,
        claim_child: Option<bool>,
        copy_on_change: Option<CopyOnChange>,
    ) -> Option<Self> {
        (!linked_subelements.is_empty()
            || element_component.is_some()
            || claim_child.is_some()
            || copy_on_change.is_some())
        .then_some(Self {
            linked_subelements,
            element_component,
            claim_child,
            copy_on_change,
        })
    }

    /// Persisted prototype subelement selection.
    pub fn linked_subelements(&self) -> &[String] {
        &self.linked_subelements
    }

    /// Explicit application object representing this array element.
    pub fn element_component(&self) -> Option<&ProductDefinitionId> {
        self.element_component.as_ref()
    }

    /// Whether this link claims its prototype in the source tree.
    pub fn claim_child(&self) -> Option<bool> {
        self.claim_child
    }

    /// Copy-on-change ownership state.
    pub fn copy_on_change(&self) -> Option<&CopyOnChange> {
        self.copy_on_change.as_ref()
    }
}

/// Copy-on-change ownership state carried by an `App::Link` occurrence.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct CopyOnChange {
    /// Ownership policy.
    pub policy: CopyOnChangePolicy,
    /// Original component tracked by copy-on-change.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source: Option<ProductDefinitionId>,
    /// Internal component holding owned copies.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub group: Option<ProductDefinitionId>,
    /// Whether the tracked source was persisted as changed.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub touched: Option<bool>,
}

crate::units::named_field!(deserialize_occurrence_scale, [FiniteReal; 3], "scale");

impl Occurrence {
    /// Placement after applying the linked prototype contribution, when present.
    pub fn effective_transform(&self) -> Result<Transform, crate::transform::TransformError> {
        self.linked_prototype
            .map_or(Ok(self.transform), |prototype| {
                self.transform.compose(prototype)
            })
    }
}

#[derive(Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
struct LinkStateWire {
    #[serde(default)]
    linked_subelements: Vec<String>,
    #[serde(default)]
    element_component: Option<ProductDefinitionId>,
    #[serde(default)]
    claim_child: Option<bool>,
    #[serde(default)]
    copy_on_change: Option<CopyOnChange>,
}

impl TryFrom<LinkStateWire> for LinkState {
    type Error = &'static str;

    fn try_from(wire: LinkStateWire) -> Result<Self, Self::Error> {
        Self::new(
            wire.linked_subelements,
            wire.element_component,
            wire.claim_child,
            wire.copy_on_change,
        )
        .ok_or("link state must carry at least one member")
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
    /// An occurrence placement cannot be composed into a finite transform.
    Transform {
        /// The occurrence whose placement failed.
        occurrence: OccurrenceId,
        /// The transform arithmetic failure.
        source: crate::transform::TransformError,
    },
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
            Self::Transform { occurrence, source } => {
                write!(formatter, "occurrence {occurrence}: {source}")
            }
        }
    }
}

impl std::error::Error for AssemblyGraphError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Transform { source, .. } => Some(source),
            _ => None,
        }
    }
}

/// Validated, memoized view over a canonical occurrence tree.
pub struct AssemblyGraph<'a> {
    occurrences: HashMap<&'a str, &'a Occurrence>,
    resolved: HashMap<&'a str, Transform>,
}

#[cfg(test)]
mod tests {
    mod joints;
    mod occurrences;
    use super::*;

    #[test]
    fn prefixes_preserve_nonempty_strings_and_wire_values() {
        for (prefix, suffix, expected) in [('#', "42", "#42"), ('λ', "", "λ"), ('\0', "", "\0")] {
            let value = NonEmptyString::prefixed(prefix, suffix);
            assert_eq!(value.as_str(), expected);
            assert_eq!(serde_json::to_value(&value).unwrap(), expected);
        }
        assert_eq!(NonEmptyString::prefixed('#', 42).as_str(), "#42");
    }

    #[test]
    fn an_external_document_states_its_resolution_and_carries_one_identity() {
        let path = ExternalDocument::path("parts/widget.FCStd");
        let path_wire = serde_json::to_value(&path).unwrap();
        assert_eq!(
            path_wire,
            serde_json::json!({
                "resolution": "path",
                "path": "parts/widget.FCStd"
            })
        );
        assert_eq!(
            serde_json::from_value::<ExternalDocument>(path_wire.clone()).unwrap(),
            path
        );

        let document_id = ExternalDocument::document_id("document-7");
        let document_id_wire = serde_json::to_value(&document_id).unwrap();
        assert_eq!(
            document_id_wire,
            serde_json::json!({"resolution": "document_id", "document_id": "document-7"})
        );
        assert_eq!(
            serde_json::from_value::<ExternalDocument>(document_id_wire).unwrap(),
            document_id
        );

        let missing = ExternalDocument::missing();
        assert_eq!(ExternalDocument::path(""), missing);
        assert_eq!(ExternalDocument::document_id(""), missing);
        let missing_wire = serde_json::to_value(&missing).unwrap();
        assert_eq!(missing_wire, serde_json::json!({"resolution": "missing"}));
        assert_eq!(
            serde_json::from_value::<ExternalDocument>(missing_wire).unwrap(),
            missing
        );

        for (invalid, named) in [
            (
                serde_json::json!({"resolution": "path", "path": ""}),
                "must not be empty",
            ),
            (
                serde_json::json!({
                    "resolution": "path",
                    "path": "parts/widget.FCStd",
                    "document_id": "document-7"
                }),
                "document_id",
            ),
            (
                serde_json::json!({"resolution": "missing", "path": ""}),
                "path",
            ),
            (serde_json::json!({"resolution": "path"}), "path"),
        ] {
            let error = serde_json::from_value::<ExternalDocument>(invalid)
                .unwrap_err()
                .to_string();
            assert!(error.contains(named), "{error}");
        }
    }

    #[test]
    fn a_joint_operand_states_the_container_that_owns_its_object() {
        let root = JointOperand::root("Body1", Vec::new());
        let root_wire = serde_json::to_value(&root).unwrap();
        assert_eq!(
            root_wire,
            serde_json::json!({"container": "root", "object": "Body1"})
        );
        assert_eq!(
            serde_json::from_value::<JointOperand>(root_wire.clone()).unwrap(),
            root
        );

        let occurrence_id = OccurrenceId::mint("test:model:occurrence#0").unwrap();
        let occurrence = JointOperand::occurrence(occurrence_id.clone(), "Body1", Vec::new());
        let occurrence_wire = serde_json::to_value(&occurrence).unwrap();
        assert_eq!(occurrence_wire["container"], "occurrence");
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
        assert_eq!(external_wire["container"], "external");
        assert_eq!(
            serde_json::from_value::<JointOperand>(external_wire).unwrap(),
            external
        );

        let mut both = occurrence_wire;
        both["external_document"] = serde_json::json!({"resolution": "missing"});
        let error = serde_json::from_value::<JointOperand>(both)
            .unwrap_err()
            .to_string();
        assert!(error.contains("external_document"), "{error}");

        let mut stray = root_wire.clone();
        stray["occurrence"] = serde_json::json!(occurrence_id.as_str());
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

    fn translation(x: f64) -> Transform {
        Transform::affine([
            [1.0, 0.0, 0.0, x],
            [0.0, 1.0, 0.0, 0.0],
            [0.0, 0.0, 1.0, 0.0],
        ])
        .expect("translation is affine")
    }

    fn occurrence(id: &str, parent: OccurrenceParent, x: f64) -> Occurrence {
        Occurrence {
            id: OccurrenceId::mint(id).expect("valid identity"),
            prototype: PrototypeReference::Unresolved,
            parent,
            ordinal: 0,
            transform: translation(x),
            linked_prototype: None,
            scale: [crate::scalar::FiniteReal::ONE; 3],
            name: None,
            visible: None,
            link: None,
            native_ref: None,
        }
    }

    #[test]
    fn resolves_parent_chains_and_conditional_prototype_placement() {
        let root = occurrence("test:model:entity#root", OccurrenceParent::Root, 1.0);
        let mut child = occurrence(
            "test:model:entity#child",
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
                .resolved_transform(
                    &OccurrenceId::mint("test:model:entity#child").expect("valid identity")
                )
                .expect("resolved child")
                .rows()[0][3],
            13.0
        );
    }

    #[test]
    fn an_absent_linked_prototype_key_is_the_only_spelling_of_absence() {
        let plain = occurrence("test:model:occurrence#plain", OccurrenceParent::Root, 1.0);
        let plain_wire = serde_json::to_value(&plain).expect("plain occurrence wire");
        assert!(plain_wire.get("linked_prototype").is_none());
        assert_eq!(
            serde_json::from_value::<Occurrence>(plain_wire).unwrap(),
            plain
        );

        let mut linked = plain;
        linked.linked_prototype = Some(translation(10.0));
        let linked_wire = serde_json::to_value(&linked).expect("linked occurrence wire");
        assert_eq!(
            linked_wire.get("linked_prototype"),
            Some(&serde_json::to_value(translation(10.0)).unwrap())
        );
        assert_eq!(
            serde_json::from_value::<Occurrence>(linked_wire).unwrap(),
            linked
        );

        let mut identity = serde_json::to_value(&linked).unwrap();
        identity["linked_prototype"] = serde_json::to_value(Transform::identity()).unwrap();
        assert_eq!(
            serde_json::from_value::<Occurrence>(identity)
                .unwrap()
                .linked_prototype,
            Some(Transform::identity())
        );
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
        assert!(serde_json::from_value::<JointLimits>(
            serde_json::json!({"minimum":2.0,"maximum":1.0})
        )
        .is_err());
    }

    #[test]
    fn empty_link_state_is_absent() {
        assert!(LinkState::new(Vec::new(), None, None, None).is_none());
        assert!(LinkState::new(Vec::new(), None, Some(false), None).is_some());
        let plain = occurrence(
            "test:model:occurrence#empty-link",
            OccurrenceParent::Root,
            0.0,
        );
        let wire = serde_json::to_value(&plain).unwrap();
        assert!(serde_json::from_value::<Occurrence>(wire)
            .unwrap()
            .link
            .is_none());
    }

    #[test]
    fn the_link_state_is_one_nested_object_with_its_own_copy_on_change() {
        let mut linked = occurrence("test:model:occurrence#link", OccurrenceParent::Root, 1.0);
        linked.link = LinkState::new(
            vec!["Face1".into()],
            Some(ProductDefinitionId::mint("test:model:product#element").expect("valid identity")),
            Some(true),
            Some(CopyOnChange {
                policy: CopyOnChangePolicy::Owned,
                source: Some(
                    ProductDefinitionId::mint("test:model:product#source").expect("valid identity"),
                ),
                group: Some(
                    ProductDefinitionId::mint("test:model:product#group").expect("valid identity"),
                ),
                touched: Some(true),
            }),
        );
        let wire = serde_json::to_value(&linked).expect("App::Link occurrence wire");
        assert_eq!(
            wire["link"]["linked_subelements"],
            serde_json::json!(["Face1"])
        );
        assert_eq!(
            wire["link"]["copy_on_change"],
            serde_json::json!({
                "policy": {"policy": "owned"},
                "source": "test:model:product#source",
                "group": "test:model:product#group",
                "touched": true
            })
        );
        assert_eq!(
            serde_json::from_value::<Occurrence>(wire.clone()).unwrap(),
            linked
        );

        let mut without_policy = wire.clone();
        without_policy["link"]["copy_on_change"]
            .as_object_mut()
            .expect("a copy-on-change object")
            .remove("policy");
        let error = serde_json::from_value::<Occurrence>(without_policy)
            .unwrap_err()
            .to_string();
        assert!(error.contains("policy"), "{error}");

        let mut bogus = wire;
        bogus["link"]["copy_on_change"]["zz_bogus"] = serde_json::json!(1);
        let error = serde_json::from_value::<Occurrence>(bogus)
            .unwrap_err()
            .to_string();
        assert!(error.contains("zz_bogus"), "{error}");

        let mut empty = serde_json::to_value(occurrence(
            "test:model:occurrence#empty-link",
            OccurrenceParent::Root,
            1.0,
        ))
        .unwrap();
        empty["link"] = serde_json::json!({});
        assert!(serde_json::from_value::<Occurrence>(empty).is_err());
    }

    #[test]
    fn rejects_duplicate_missing_and_cyclic_parent_links() {
        let duplicate = occurrence("test:model:entity#same", OccurrenceParent::Root, 0.0);
        assert!(matches!(
            AssemblyGraph::new(&[duplicate.clone(), duplicate]),
            Err(AssemblyGraphError::DuplicateOccurrence(_))
        ));

        let missing = occurrence(
            "test:model:entity#child",
            OccurrenceParent::Occurrence {
                occurrence: OccurrenceId::mint("test:model:entity#missing")
                    .expect("valid identity"),
            },
            0.0,
        );
        assert!(matches!(
            AssemblyGraph::new(&[missing]),
            Err(AssemblyGraphError::MissingParent { .. })
        ));

        let first = occurrence(
            "test:model:entity#first",
            OccurrenceParent::Occurrence {
                occurrence: OccurrenceId::mint("test:model:entity#second").expect("valid identity"),
            },
            0.0,
        );
        let second = occurrence(
            "test:model:entity#second",
            OccurrenceParent::Occurrence {
                occurrence: OccurrenceId::mint("test:model:entity#first").expect("valid identity"),
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
    let transform = occurrence
        .effective_transform()
        .and_then(|local| parent.compose(local))
        .map_err(|source| AssemblyGraphError::Transform {
            occurrence: occurrence.id.clone(),
            source,
        })?;
    active.remove(occurrence.id.as_str());
    resolved.insert(occurrence.id.as_str(), transform);
    Ok(transform)
}

/// Container that owns a joint operand object.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(tag = "container", rename_all = "snake_case", deny_unknown_fields)]
pub enum OperandContainer {
    /// Object in the current document root.
    Root {},
    /// Object in a placed local occurrence.
    Occurrence {
        /// Placed local occurrence that owns the object.
        occurrence: OccurrenceId,
    },
    /// Object in an external document.
    External {
        /// External document that owns the object.
        external_document: ExternalDocumentReference,
    },
}

/// One connector operand and its selected native subelements.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub struct JointOperand {
    /// Container that owns the object.
    #[serde(flatten)]
    pub container: OperandContainer,
    /// Exact referenced application object identity.
    pub object: String,
    /// Ordered persistent object/element paths.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
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
        Self {
            container: OperandContainer::Occurrence { occurrence },
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
            container: OperandContainer::External {
                external_document: document,
            },
            object: object.into(),
            subelements,
        }
    }
}

/// Enabled bounds for one joint degree of freedom.
#[derive(Debug, Clone, PartialEq)]
pub struct JointLimits {
    minimum: Option<f64>,
    maximum: Option<f64>,
}

impl JointLimits {
    /// Constructs finite ordered limits with at least one enabled bound.
    pub fn new(minimum: Option<f64>, maximum: Option<f64>) -> Option<Self> {
        if minimum.is_none() && maximum.is_none()
            || minimum.is_some_and(|value| !value.is_finite())
            || maximum.is_some_and(|value| !value.is_finite())
            || minimum
                .zip(maximum)
                .is_some_and(|(minimum, maximum)| minimum > maximum)
        {
            return None;
        }
        Some(Self { minimum, maximum })
    }

    /// Returns the lower bound, when enabled.
    pub fn minimum(&self) -> Option<f64> {
        self.minimum
    }

    /// Returns the upper bound, when enabled.
    pub fn maximum(&self) -> Option<f64> {
        self.maximum
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
        Self::new(wire.minimum, wire.maximum).ok_or_else(|| {
            serde::de::Error::custom(
                "joint limits minimum/maximum must be finite and ordered, with at least one bound",
            )
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
#[serde(tag = "arity", rename_all = "snake_case", deny_unknown_fields)]
// Inline fixed-size arrays encode the one-or-two connector invariant directly.
#[allow(clippy::large_enum_variant)]
pub enum JointOperands {
    /// One grounded connector and its optional attachment offset.
    Grounded {
        /// Grounded connector.
        connector: JointConnector,
        /// Connector attachment offset.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        offset_frame: Option<Transform>,
    },
    /// Two paired connectors and their optional attachment offsets.
    Pair {
        /// Non-grounded joint family.
        kind: PairedJointKind,
        /// Connectors in operand order.
        connectors: [JointConnector; 2],
        /// Connector attachment offsets in operand order.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        offset_frames: Option<[Transform; 2]>,
    },
}

/// Assembly-joint families that connect two operands.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum PairedJointKind {
    /// Rigid connection with no relative degrees of freedom.
    Fixed {
        /// Angular offset in radians.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        angle: Option<FiniteReal>,
        /// Connector-local translation offset in document length units.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        translation_offset: Option<[FiniteReal; 3]>,
        /// Enabled angular interval in radians.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        angular_limits: Option<JointLimits>,
        /// Enabled linear interval in document length units.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        linear_limits: Option<JointLimits>,
    },
    /// Rotation about one axis.
    Revolute {
        /// Angular offset in radians.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        angle: Option<FiniteReal>,
        /// Enabled angular interval in radians.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        angular_limits: Option<JointLimits>,
    },
    /// Translation along one axis.
    Slider {
        /// Primary linear offset in document length units.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        distance: Option<FiniteReal>,
        /// Connector-local translation offset in document length units.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        translation_offset: Option<[FiniteReal; 3]>,
        /// Enabled linear interval in document length units.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        linear_limits: Option<JointLimits>,
    },
    /// Coupled rotation and translation on one axis.
    Cylindrical {
        /// Angular offset in radians.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        angle: Option<FiniteReal>,
        /// Primary linear offset in document length units.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        distance: Option<FiniteReal>,
        /// Enabled angular interval in radians.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        angular_limits: Option<JointLimits>,
        /// Enabled linear interval in document length units.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        linear_limits: Option<JointLimits>,
    },
    /// Rotation about a common point.
    Ball {},
    /// Maintains a scalar separation.
    Distance {
        /// Primary linear offset in document length units.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        distance: Option<FiniteReal>,
    },
    /// Maintains parallel connector directions.
    Parallel {},
    /// Maintains perpendicular connector directions.
    Perpendicular {},
    /// Maintains an angular separation.
    Angle {
        /// Angular offset in radians.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        angle: Option<FiniteReal>,
    },
    /// Couples rack translation to pinion rotation.
    RackPinion {
        /// Primary linear offset in document length units.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        distance: Option<FiniteReal>,
        /// Secondary linear offset in document length units.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        distance2: Option<FiniteReal>,
    },
    /// Couples translation and rotation by screw pitch.
    Screw {
        /// Primary linear offset in document length units.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        distance: Option<FiniteReal>,
    },
    /// Couples two gear rotations.
    Gears {
        /// Primary linear offset in document length units.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        distance: Option<FiniteReal>,
        /// Secondary linear offset in document length units.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        distance2: Option<FiniteReal>,
    },
    /// Couples two pulley rotations through a belt.
    Belt {
        /// Primary linear offset in document length units.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        distance: Option<FiniteReal>,
        /// Secondary linear offset in document length units.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        distance2: Option<FiniteReal>,
    },
    /// Future application-defined family retained without relabeling.
    Native {
        /// Application-defined family name.
        name: String,
        /// Angular offset in radians.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        angle: Option<FiniteReal>,
        /// Connector-local translation offset in document length units.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        translation_offset: Option<[FiniteReal; 3]>,
        /// Primary linear offset in document length units.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        distance: Option<FiniteReal>,
        /// Secondary linear offset in document length units.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        distance2: Option<FiniteReal>,
        /// Enabled angular interval in radians.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        angular_limits: Option<JointLimits>,
        /// Enabled linear interval in document length units.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        linear_limits: Option<JointLimits>,
    },
}

impl PairedJointKind {
    fn set_angular_limits(&mut self, limits: Option<JointLimits>) {
        match self {
            Self::Fixed { angular_limits, .. }
            | Self::Revolute { angular_limits, .. }
            | Self::Cylindrical { angular_limits, .. }
            | Self::Native { angular_limits, .. } => *angular_limits = limits,
            _ => {}
        }
    }
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
    /// Format-native joint record supplying this constraint.
    #[serde(default, skip_serializing_if = "Option::is_none")]
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
            native_ref: None,
        }
    }

    /// Paired family when this joint is not grounded.
    #[must_use]
    pub fn paired_kind(&self) -> Option<&PairedJointKind> {
        match &self.operands {
            JointOperands::Pair { kind, .. } => Some(kind),
            JointOperands::Grounded { .. } => None,
        }
    }

    /// Whether this joint grounds a single connector.
    #[must_use]
    pub fn is_grounded(&self) -> bool {
        matches!(self.operands, JointOperands::Grounded { .. })
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

    /// Per-connector detach flags in operand order. Grounded joints emit a false second flag.
    #[must_use]
    pub fn detached(&self) -> [bool; 2] {
        match &self.operands {
            JointOperands::Grounded { connector, .. } => [connector.detached, false],
            JointOperands::Pair { connectors, .. } => {
                [connectors[0].detached, connectors[1].detached]
            }
        }
    }

    fn pair_kind_mut(&mut self) -> Option<&mut PairedJointKind> {
        match &mut self.operands {
            JointOperands::Pair { kind, .. } => Some(kind),
            JointOperands::Grounded { .. } => None,
        }
    }

    /// Angular offset in radians.
    #[must_use]
    pub fn angle(&self) -> Option<f64> {
        match self.paired_kind()? {
            PairedJointKind::Fixed { angle, .. }
            | PairedJointKind::Revolute { angle, .. }
            | PairedJointKind::Cylindrical { angle, .. }
            | PairedJointKind::Angle { angle }
            | PairedJointKind::Native { angle, .. } => angle.map(FiniteReal::get),
            _ => None,
        }
    }

    /// Connector-local translation offset in document length units.
    #[must_use]
    pub fn translation_offset(&self) -> Option<[f64; 3]> {
        match self.paired_kind()? {
            PairedJointKind::Fixed {
                translation_offset, ..
            }
            | PairedJointKind::Slider {
                translation_offset, ..
            }
            | PairedJointKind::Native {
                translation_offset, ..
            } => translation_offset.map(|values| values.map(FiniteReal::get)),
            _ => None,
        }
    }

    /// Primary linear offset in document length units.
    #[must_use]
    pub fn distance(&self) -> Option<f64> {
        match self.paired_kind()? {
            PairedJointKind::Slider { distance, .. }
            | PairedJointKind::Cylindrical { distance, .. }
            | PairedJointKind::Distance { distance }
            | PairedJointKind::RackPinion { distance, .. }
            | PairedJointKind::Screw { distance }
            | PairedJointKind::Gears { distance, .. }
            | PairedJointKind::Belt { distance, .. }
            | PairedJointKind::Native { distance, .. } => distance.map(FiniteReal::get),
            _ => None,
        }
    }

    /// Secondary linear offset in document length units.
    #[must_use]
    pub fn distance2(&self) -> Option<f64> {
        match self.paired_kind()? {
            PairedJointKind::RackPinion { distance2, .. }
            | PairedJointKind::Gears { distance2, .. }
            | PairedJointKind::Belt { distance2, .. }
            | PairedJointKind::Native { distance2, .. } => distance2.map(FiniteReal::get),
            _ => None,
        }
    }

    /// Enabled angular interval in radians.
    #[must_use]
    pub fn angular_limits(&self) -> Option<&JointLimits> {
        match self.paired_kind() {
            Some(
                PairedJointKind::Fixed { angular_limits, .. }
                | PairedJointKind::Revolute { angular_limits, .. }
                | PairedJointKind::Cylindrical { angular_limits, .. }
                | PairedJointKind::Native { angular_limits, .. },
            ) => angular_limits.as_ref(),
            _ => None,
        }
    }

    /// Enabled linear interval in document length units.
    #[must_use]
    pub fn linear_limits(&self) -> Option<&JointLimits> {
        match self.paired_kind() {
            Some(
                PairedJointKind::Fixed { linear_limits, .. }
                | PairedJointKind::Slider { linear_limits, .. }
                | PairedJointKind::Cylindrical { linear_limits, .. }
                | PairedJointKind::Native { linear_limits, .. },
            ) => linear_limits.as_ref(),
            _ => None,
        }
    }

    /// Replace the angular interval when the joint family admits one.
    pub fn set_angular_limits(&mut self, limits: Option<JointLimits>) {
        if let Some(kind) = self.pair_kind_mut() {
            kind.set_angular_limits(limits);
        }
    }
}
