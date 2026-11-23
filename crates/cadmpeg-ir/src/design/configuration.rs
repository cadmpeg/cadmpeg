// SPDX-License-Identifier: Apache-2.0
//! Configurations, arrangements, and publications.
//!
//! A configuration's value for a parameter, feature state, occurrence state,
//! or material is its own where it states one, else its parent's resolved
//! value, else the base's. An arrangement is a named set of overrides that
//! the user activates independently of a configuration. A publication is a
//! named entry point into a scope.

use std::collections::{BTreeMap, BTreeSet};

use cadmpeg_core::text::NonBlankString;
#[cfg(feature = "schema")]
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use super::dimension::RefTarget;
use super::expr::{print, Expression, Neutral};
use super::feature::FeatureDefinition;
use super::ids::{ArrangementId, ParameterProgramId, PublicationId};
use super::ops::body::TransformStep;
use super::parameter::{DesignParameter, ParameterDefinition, ParameterValue};
use super::program::{ParameterProgram, ProgramBody, Statement};
use super::refs::ConfigurationRef;
use super::selection::{OccurrencePath, Picked, SetSelection};
use crate::features::{ConfigurationId, FeatureId, ParameterId};
use crate::ids::{AppearanceId, OccurrenceId, PhysicalMaterialId, ProductDefinitionId};
use crate::products::NestedOverride;
use crate::scalar::Fraction;
use crate::transform::Transform;

/// A configuration of a scope.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct DesignConfiguration {
    /// The configuration's identity.
    pub id: ConfigurationId,
    /// The product definition whose scope holds the configuration.
    pub scope: ProductDefinitionId,
    /// The configuration's name, which identifies it.
    pub name: String,
    /// The configuration a derived configuration inherits from.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_parent"
    )]
    pub parent: Option<ConfigurationId>,
    /// The configuration's free text about itself.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_comment"
    )]
    pub comment: Option<String>,
    /// The configuration's own values.
    pub overrides: Overrides,
    /// The arrangements the configuration puts in force, each in place of the
    /// arrangement of its kind that its parent or the base names.
    #[serde(with = "cadmpeg_core::distinct_keys::set")]
    #[cfg_attr(feature = "schema", schemars(with = "BTreeSet<ArrangementId>"))]
    pub active_arrangements: BTreeSet<ArrangementId>,
    /// The materials that override the base material in this configuration.
    pub materials: Vec<MaterialOverride>,
    /// The configuration's native record.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_native_ref"
    )]
    pub native_ref: Option<String>,
}

impl DesignConfiguration {
    /// Whether the configuration states nothing of its own: no override, no
    /// arrangement put in force, and no material.
    pub(crate) fn states_nothing(&self) -> bool {
        self.overrides == Overrides::default()
            && self.active_arrangements.is_empty()
            && self.materials.is_empty()
    }
}

/// What the overrides that apply where a parameter is read state, beyond the
/// parameters they state (§5.13).
#[derive(Debug, Clone, Copy)]
pub(crate) struct Stated {
    /// Some override, material, or arrangement is stated.
    pub(crate) anything: bool,
    /// Some occurrence override is stated.
    pub(crate) occurrences: bool,
}

impl Stated {
    /// What a configuration chain states.
    pub(crate) fn of(chain: &[&DesignConfiguration]) -> Self {
        Self {
            anything: !chain.iter().all(|level| level.states_nothing()),
            occurrences: chain
                .iter()
                .any(|level| !level.overrides.occurrences.is_empty()),
        }
    }
}

/// Whether a parameter's own `value` holds where overrides apply (§5.13):
/// nothing it rests on can differ there from the base. `overridden` tells
/// whether an override that applies states a parameter; `parameter` and
/// `program` look up records of the design.
///
/// It does not hold where the parameter's definition reads, directly or
/// through other parameters, an overridden parameter. Where `stated`
/// holds anything it also does not hold where the parameter or one it reads
/// is `Driven`, is assigned by a `Source` program, or has a `Source`
/// expression; where a definition among them reads a measure or a reference
/// `through` an occurrence path; or, where `stated` holds an occurrence
/// override, an `External` reference. An `Assigned` parameter reads what its
/// `Neutral` program's conditions and assigned values read. A record that
/// does not resolve or print does not let the value hold.
pub(crate) fn own_value_holds<'a>(
    record: &'a DesignParameter,
    stated: Stated,
    overridden: &dyn Fn(&ParameterId) -> bool,
    parameter: &dyn Fn(&ParameterId) -> Option<&'a DesignParameter>,
    program: &dyn Fn(&ParameterProgramId) -> Option<&'a ParameterProgram>,
) -> bool {
    if !stated.anything {
        return true;
    }
    let mut seen = BTreeSet::from([&record.id]);
    let mut pending = vec![record];
    while let Some(record) = pending.pop() {
        let mut nodes = Vec::new();
        match &record.definition {
            ParameterDefinition::Driven
            | ParameterDefinition::Expression(Expression::Source { .. }) => return false,
            ParameterDefinition::Expression(Expression::Neutral(Neutral(node))) => {
                nodes.push(node);
            }
            ParameterDefinition::Assigned(id) => {
                let Some(ParameterProgram {
                    body: ProgramBody::Neutral(statements),
                    ..
                }) = program(id)
                else {
                    return false;
                };
                let mut statements: Vec<&Statement> = statements.iter().collect();
                while let Some(statement) = statements.pop() {
                    match statement {
                        Statement::Assign { value, .. } => nodes.push(&value.0),
                        Statement::If {
                            condition,
                            then,
                            otherwise,
                        } => {
                            nodes.push(&condition.0);
                            statements.extend(then.iter().chain(otherwise));
                        }
                    }
                }
            }
        }
        for node in nodes {
            let Ok(printed) = print(node) else {
                return false;
            };
            if !printed.measures.is_empty() {
                return false;
            }
            for reference in printed.refs {
                if reference.through.is_some() {
                    return false;
                }
                match &reference.target {
                    RefTarget::External { .. } if stated.occurrences => return false,
                    RefTarget::External { .. } => {}
                    RefTarget::Parameter(target) => {
                        if overridden(target) {
                            return false;
                        }
                        let Some(next) = parameter(target) else {
                            return false;
                        };
                        if seen.insert(&next.id) {
                            pending.push(next);
                        }
                    }
                }
            }
        }
    }
    true
}

/// A configuration's or an arrangement's own values.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct Overrides {
    /// Parameter values, keyed by parameter.
    #[serde(deserialize_with = "cadmpeg_core::distinct_keys::btree_map")]
    pub parameters: BTreeMap<ParameterId, ParameterOverride>,
    /// Feature states, keyed by feature.
    #[serde(deserialize_with = "cadmpeg_core::distinct_keys::btree_map")]
    pub features: BTreeMap<FeatureId, FeatureStateOverride>,
    /// Occurrence states, keyed by occurrence owned by a definition the scope
    /// spans.
    #[serde(deserialize_with = "cadmpeg_core::distinct_keys::btree_map")]
    pub occurrences: BTreeMap<OccurrenceId, OccurrenceOverride>,
    /// The configuration's own values of the identity fields of a definition
    /// its scope spans, keyed by definition.
    #[serde(deserialize_with = "cadmpeg_core::distinct_keys::btree_map")]
    pub definitions: BTreeMap<ProductDefinitionId, DefinitionOverride>,
}

/// A configuration's own values of a definition's identity fields; `None`
/// leaves the resolved value.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct DefinitionOverride {
    /// The member's file name, where the source states it apart from the
    /// member's name.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_source_name"
    )]
    pub source_name: Option<String>,
    /// The definition's label.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_label"
    )]
    pub label: Option<String>,
    /// The definition's description.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_description"
    )]
    pub description: Option<String>,
    /// The definition's part number.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_part_number"
    )]
    pub part_number: Option<PartNumber>,
    /// The definition's source-defined bill-of-materials fields; an absent key
    /// leaves the resolved value.
    #[serde(deserialize_with = "cadmpeg_core::distinct_keys::btree_map")]
    pub bom_properties: BTreeMap<NonBlankString, String>,
}

/// A configuration's part number: a text of its own, or a rule evaluated at
/// the configuration that states it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, cadmpeg_derive::DeserializeTagged)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(
    tag = "kind",
    content = "value",
    rename_all = "snake_case",
    deny_unknown_fields
)]
pub enum PartNumber {
    /// A part number of the configuration's own.
    Text(String),
    /// The configuration's name.
    #[serde(deserialize_with = "cadmpeg_core::tag_only::refuse_value")]
    ConfigurationName,
    /// The name of the configuration's parent.
    #[serde(deserialize_with = "cadmpeg_core::tag_only::refuse_value")]
    ParentConfigurationName,
    /// The definition's title, else its source name.
    #[serde(deserialize_with = "cadmpeg_core::tag_only::refuse_value")]
    DocumentName,
}

/// A configuration's value of one parameter. It states at least one of
/// `expression` and `value`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct ParameterOverride {
    /// The expression that replaces the parameter's expression in this
    /// configuration.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_expression"
    )]
    pub expression: Option<Expression>,
    /// The value the source caches for this configuration.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_value"
    )]
    pub value: Option<ParameterValue>,
}

/// A configuration's state of one feature. It states at least one of its
/// fields.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct FeatureStateOverride {
    /// Whether the feature is suppressed in this configuration.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_suppressed"
    )]
    pub suppressed: Option<bool>,
    /// The feature's definition in this configuration: a structural
    /// difference.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_definition"
    )]
    pub definition: Option<FeatureDefinition>,
}

/// A configuration's state of one occurrence.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct OccurrenceOverride {
    /// Whether the occurrence is suppressed.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_suppressed"
    )]
    pub suppressed: Option<bool>,
    /// Whether the occurrence is visible.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_visible"
    )]
    pub visible: Option<bool>,
    /// The occurrence's placement.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_transform"
    )]
    pub transform: Option<Transform>,
    /// The configuration of its prototype that the occurrence uses.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_configuration"
    )]
    pub configuration: Option<ConfigurationRef>,
    /// The occurrence's prototype.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_prototype"
    )]
    pub prototype: Option<ProductDefinitionId>,
    /// Whether the occurrence is grounded.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_grounded"
    )]
    pub grounded: Option<bool>,
    /// Whether the occurrence is flexible.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_flexible"
    )]
    pub flexible: Option<bool>,
    /// The representation of its prototype that the occurrence shows.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_representation"
    )]
    pub representation: Option<NonBlankString>,
    /// The configuration's nested overrides of the occurrences below this
    /// one.
    pub nested: Vec<NestedOverride>,
}

/// A material that overrides the base material in a configuration.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct MaterialOverride {
    /// The bodies the material applies to, selected by name in the
    /// configuration's current states. `None` is the whole definition.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_target"
    )]
    pub target: Option<SetSelection>,
    /// The material.
    pub material: MaterialRef,
}

/// A material of this document, or by the source's name for it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, cadmpeg_derive::DeserializeTagged)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(
    tag = "kind",
    content = "value",
    rename_all = "snake_case",
    deny_unknown_fields
)]
pub enum MaterialRef {
    /// A physical material of this document.
    Material(PhysicalMaterialId),
    /// A material by its name.
    Named(NonBlankString),
}

/// A named entry point into a scope.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct Publication {
    /// The publication's identity.
    pub id: PublicationId,
    /// The product definition whose scope the publication enters.
    pub scope: ProductDefinitionId,
    /// The publication's name.
    pub name: NonBlankString,
    /// What the publication names, resolved in the scope's current geometry.
    pub target: Picked,
}

/// A named set of overrides of a scope's occurrences, constraint values, and
/// display that the user activates independently of a configuration.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct Arrangement {
    /// The arrangement's identity.
    pub id: ArrangementId,
    /// The product definition whose scope holds the arrangement.
    pub scope: ProductDefinitionId,
    /// The arrangement's name.
    pub name: NonBlankString,
    /// What the arrangement overrides.
    pub kind: ArrangementKind,
    /// The configuration the arrangement belongs to. `None` is an arrangement
    /// available in every configuration.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_configuration"
    )]
    pub configuration: Option<ConfigurationId>,
    /// The arrangement's own values, which resolve over the occurrence's
    /// values in the configuration that applies.
    pub overrides: Overrides,
    /// An instance's appearance and transparency.
    pub appearances: Vec<ArrangementAppearance>,
    /// Moves of sets of instances, applied in order after the overrides.
    pub moves: Vec<ArrangementMove>,
    /// For an instance of a sub-assembly, the arrangement of its prototype it
    /// shows while this arrangement is in force.
    pub nested: Vec<NestedArrangement>,
    /// The arrangement's native record.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_native_ref"
    )]
    pub native_ref: Option<String>,
}

/// What an arrangement overrides.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, cadmpeg_derive::DeserializeTagged)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(
    tag = "kind",
    content = "value",
    rename_all = "snake_case",
    deny_unknown_fields
)]
pub enum ArrangementKind {
    /// Relationship values and suppression, pattern offsets, positions,
    /// grounding, flexibility, and the positional arrangement of each
    /// sub-assembly.
    #[serde(deserialize_with = "cadmpeg_core::tag_only::refuse_value")]
    Positional,
    /// Occurrence visibility, appearance, and transparency.
    #[serde(deserialize_with = "cadmpeg_core::tag_only::refuse_value")]
    Display,
    /// Ordered moves of sets of occurrences, which change appearance only.
    #[serde(deserialize_with = "cadmpeg_core::tag_only::refuse_value")]
    Explode,
    /// A kind whose meaning is not established, by the source's kind.
    Native(NonBlankString),
}

/// An instance's appearance and transparency in an arrangement.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct ArrangementAppearance {
    /// The instance's path.
    pub path: OccurrencePath,
    /// The instance's appearance.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_appearance"
    )]
    pub appearance: Option<AppearanceId>,
    /// The instance's transparency.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_transparency"
    )]
    pub transparency: Option<Fraction>,
}

/// A move of the instances it lists in an arrangement: a `Translate` or
/// `Rotate` step (§6.5).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct ArrangementMove {
    /// The paths of the instances the step moves.
    pub occurrences: Vec<OccurrencePath>,
    /// The step.
    pub step: TransformStep,
}

/// The arrangement of its prototype that an instance of a sub-assembly shows.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct NestedArrangement {
    /// The instance's path.
    pub path: OccurrencePath,
    /// The arrangement of the instance's prototype.
    pub arrangement: ArrangementRef,
}

/// An arrangement of this document, or by the source's name for it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, cadmpeg_derive::DeserializeTagged)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(
    tag = "kind",
    content = "value",
    rename_all = "snake_case",
    deny_unknown_fields
)]
pub enum ArrangementRef {
    /// An arrangement of this document.
    Local(ArrangementId),
    /// An arrangement by its name.
    Named(NonBlankString),
}

cadmpeg_core::named_optional_field!(deserialize_appearance<T>, T, "appearance");
cadmpeg_core::named_optional_field!(deserialize_comment<T>, T, "comment");
cadmpeg_core::named_optional_field!(deserialize_configuration<T>, T, "configuration");
cadmpeg_core::named_optional_field!(deserialize_definition<T>, T, "definition");
cadmpeg_core::named_optional_field!(deserialize_description<T>, T, "description");
cadmpeg_core::named_optional_field!(deserialize_expression<T>, T, "expression");
cadmpeg_core::named_optional_field!(deserialize_flexible<T>, T, "flexible");
cadmpeg_core::named_optional_field!(deserialize_grounded<T>, T, "grounded");
cadmpeg_core::named_optional_field!(deserialize_label<T>, T, "label");
cadmpeg_core::named_optional_field!(deserialize_native_ref<T>, T, "native_ref");
cadmpeg_core::named_optional_field!(deserialize_parent<T>, T, "parent");
cadmpeg_core::named_optional_field!(deserialize_part_number<T>, T, "part_number");
cadmpeg_core::named_optional_field!(deserialize_prototype<T>, T, "prototype");
cadmpeg_core::named_optional_field!(deserialize_representation<T>, T, "representation");
cadmpeg_core::named_optional_field!(deserialize_source_name<T>, T, "source_name");
cadmpeg_core::named_optional_field!(deserialize_suppressed<T>, T, "suppressed");
cadmpeg_core::named_optional_field!(deserialize_target<T>, T, "target");
cadmpeg_core::named_optional_field!(deserialize_transform<T>, T, "transform");
cadmpeg_core::named_optional_field!(deserialize_transparency<T>, T, "transparency");
cadmpeg_core::named_optional_field!(deserialize_value<T>, T, "value");
cadmpeg_core::named_optional_field!(deserialize_visible<T>, T, "visible");
