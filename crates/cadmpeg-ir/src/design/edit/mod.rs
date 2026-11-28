// SPDX-License-Identifier: Apache-2.0
//! Edit helpers and resolvers of the design layer (§4 of the CADIR v11
//! specification).
//!
//! The resolvers read a value through the configuration that applies:
//! [`dimension_value`](crate::CadIr::dimension_value) resolves a bound
//! dimension from its parameter's cache,
//! [`is_stateless`](crate::CadIr::is_stateless) decides which features have
//! evaluations, and
//! [`effective_transform`](crate::CadIr::effective_transform),
//! [`effective_visibility`](crate::CadIr::effective_visibility), and
//! [`effective_display_color`](crate::CadIr::effective_display_color)
//! resolve an instance's placement, visibility, and colour. They read any
//! document.
//!
//! The edit helpers are the methods of an [`Editor`], which
//! [`CadIr::edit`](crate::CadIr::edit) returns over a document with the
//! findings validation reports on it. Each changes one canonical fact and keeps
//! every cache that depends on it honest: [`Editor::set_dimension`],
//! [`Editor::set_parameter_expression`], and
//! [`Editor::set_feature_definition`] mark the dependent states `Stale`,
//! recompute the dependent caches they can, and clear the rest;
//! [`Editor::insert_member`] and [`Editor::remove_member`] add a member with
//! its records and delete one with what it owns, keeping every position
//! valid; and [`Editor::set_active_configuration`] exchanges the current
//! geometry of one scope for another configuration's state. An edit whose
//! result holds more Error findings of a rule on a record than the document
//! held before it is refused, and the document is left unchanged, so no edit
//! adds an Error. An Error the document held before the edit refuses nothing.
//!
//! Positions, references, measures, and constructions occur throughout the
//! feature vocabulary. The helpers find them in a record's wire form, in which
//! each has one spelling (§2.5), and read the record back through its strict
//! reader after a rewrite.

mod caches;
mod configuration;
mod eval;
mod history;
mod members;
mod removal;
mod resolve;
pub mod state_view;
pub(crate) mod states;
mod wire;

#[cfg(test)]
mod tests;

use crate::design::container::Container;
use crate::design::dimension::ParameterRef;
use crate::design::feature::FeatureDefinition;
use crate::design::ids::{ContainerId, ParameterProgramId};
use crate::design::refs::{Member, Position};
use crate::document::Design;
use crate::features::{ConfigurationId, FeatureId, ParameterId};
use crate::ids::ProductDefinitionId;
use crate::report::check::{Finding, ValidationReport};
use crate::report::strictness::Strictness;
use crate::report::Severity;
use crate::CadIr;

pub use eval::{cache_value, evaluate, kind_value, EvalError, Value};
pub(crate) use eval::{check_types, kind_type, unit_factor, ExpressionSite, Type};

/// Why an edit helper or a resolver refuses.
#[derive(Debug, Clone, PartialEq, thiserror::Error)]
pub enum EditError {
    /// The result of the edit holds more Error findings of a rule on a
    /// record than the document held before it: the added findings.
    #[error(
        "the edit adds {} Error finding(s) the document did not hold",
        .0.len()
    )]
    Invalid(Vec<Finding>),
    /// A finding reaches the caller's severity floor.
    #[error("validation findings reach the requested severity floor")]
    Refused(ValidationReport),
    /// Validation exceeded a resource ceiling.
    #[error("validation exceeded a resource ceiling: {0:?}")]
    ResourceLimit(cadmpeg_core::decode::ResourceLimit),
    /// Validation could not run to its end on the document or on the result
    /// of the edit.
    #[error("validation did not complete: {0}")]
    Validation(String),
    /// No feature has the id.
    #[error("no feature has the id {0}")]
    UnknownFeature(FeatureId),
    /// No design parameter has the id.
    #[error("no design parameter has the id {0}")]
    UnknownParameter(ParameterId),
    /// No configuration has the id, or it is not a configuration of the scope
    /// the edit names.
    #[error("no configuration {0} in the scope")]
    UnknownConfiguration(ConfigurationId),
    /// No design scope belongs to the definition.
    #[error("no design scope belongs to the definition {0}")]
    UnknownScope(ProductDefinitionId),
    /// No container has the id.
    #[error("no container has the id {0}")]
    UnknownContainer(ContainerId),
    /// The member is in no container.
    #[error("the member {0:?} is in no container")]
    NotAMember(Member),
    /// The records to insert are not one new member and the records it owns.
    #[error("the records are not one member and what it owns: {0}")]
    NotOneMember(String),
    /// The document already holds a record with the id.
    #[error("the document already holds the id {0}")]
    DuplicateId(String),
    /// The index is past the end of the ordered container.
    #[error("index {index} is past the end of container {container}")]
    IndexOutOfRange {
        /// The container.
        container: ContainerId,
        /// The index.
        index: usize,
    },
    /// Records the removal keeps name a record it deletes by id: the
    /// referring records.
    #[error("kept records name a deleted record: {0:?}")]
    Referenced(Vec<String>),
    /// The parameter's value is not set by a literal: it is `Driven`,
    /// `Assigned`, or its expression references other parameters.
    #[error("parameter {parameter} is driven: {drivers:?}")]
    DrivenParameter {
        /// The parameter.
        parameter: ParameterId,
        /// What drives it.
        drivers: ParameterDrivers,
    },
    /// The value does not have the parameter's declared kind, or the
    /// expression does not type-check (§5.13).
    #[error("the value of parameter {parameter} does not have its kind: {reason}")]
    KindMismatch {
        /// The parameter.
        parameter: ParameterId,
        /// What differs.
        reason: String,
    },
    /// The parameter's cache does not read as the dimension's value type.
    #[error("the cache of parameter {parameter} is not a value of the dimension: {reason}")]
    DimensionValue {
        /// The parameter.
        parameter: ParameterId,
        /// What differs.
        reason: String,
    },
    /// A configuration is its own ancestor through its parents.
    #[error("configuration {0} is its own ancestor")]
    ConfigurationCycle(ConfigurationId),
    /// A rewritten record does not read back, or an id could not be minted.
    #[error("a record does not read back after the edit: {0}")]
    Malformed(String),
}

/// What drives a parameter that [`Editor::set_dimension`] refuses.
#[derive(Debug, Clone, PartialEq)]
pub enum ParameterDrivers {
    /// Geometry determines the value: a reference dimension.
    Geometry,
    /// A parameter program sets the value.
    Program(ParameterProgramId),
    /// The expression's references, in order of first use.
    References(Vec<ParameterRef>),
}

/// A literal value for [`Editor::set_dimension`]: a number in its authored
/// unit, a Boolean, or a text. An integer is a whole number with no unit.
#[derive(Debug, Clone, PartialEq)]
pub enum Literal {
    /// A number in its authored unit; an empty unit is dimensionless.
    Number {
        /// The authored value.
        value: crate::scalar::FiniteReal,
        /// The authored unit.
        unit: crate::design::units::Unit,
    },
    /// A Boolean.
    Boolean(bool),
    /// A text.
    Text(String),
}

/// What [`Editor::set_active_configuration`] left out (§4).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ConfigurationSwitch {
    /// The positions of the scope's current states at an unstated rollback
    /// (§5.2): `After(Container(c))`, which no evaluation may name (part
    /// 10). The switch reads each as built through `c`'s end, so the incoming
    /// state is the incoming configuration's evaluation after `c`'s last
    /// feature; the outgoing configuration's state there is not written as an
    /// evaluation, and the evaluations and process states based on it are
    /// rebased so that their resolved states are unchanged. Where an incoming
    /// state replaces it, the outgoing state leaves the document.
    pub unspelled: Vec<Position>,
}

/// An editing session over a document, holding the findings validation
/// (part 10) reports on it. The document changes only through its methods
/// while it lives, and each refuses an edit whose result holds an Error
/// finding the document did not hold, leaving the document unchanged.
#[derive(Debug)]
pub struct Editor<'a> {
    document: &'a mut CadIr,
    findings: Vec<Finding>,
}

impl CadIr {
    /// Validates the document and returns an [`Editor`] over it, which holds
    /// the findings (§4). An Error the document holds refuses no edit.
    ///
    /// # Errors
    ///
    /// Returns [`EditError::Refused`] where a finding reaches the requested
    /// floor, [`EditError::ResourceLimit`] where validation reaches a
    /// resource ceiling, and [`EditError::Validation`] for another failure.
    pub fn edit(&mut self, strictness: &Strictness) -> Result<Editor<'_>, EditError> {
        let report = validate(self)?;
        if report
            .findings
            .iter()
            .any(|finding| strictness.refuses_finding(finding))
        {
            return Err(EditError::Refused(report));
        }
        Ok(Editor {
            document: self,
            findings: report.findings,
        })
    }
}

fn validate(document: &CadIr) -> Result<ValidationReport, EditError> {
    crate::validate::validate_neutral(document, Vec::new()).map_err(|error| match error {
        cadmpeg_core::CodecError::ResourceLimit(limit) => EditError::ResourceLimit(limit),
        error => EditError::Validation(error.to_string()),
    })
}

fn findings(document: &CadIr) -> Result<Vec<Finding>, EditError> {
    validate(document).map(|report| report.findings)
}

/// The Error findings of `after` beyond those of `before`: for each rule
/// and record, the findings past the count `before` holds. A finding that
/// names no record counts under its rule alone. A Warning is never added.
fn added_errors(before: &[Finding], after: &[Finding]) -> Vec<Finding> {
    let rule = |finding: &Finding| (finding.rule.clone(), finding.entity.clone());
    let mut held = std::collections::BTreeMap::<_, usize>::new();
    for finding in before
        .iter()
        .filter(|finding| finding.severity >= Severity::Error)
    {
        *held.entry(rule(finding)).or_default() += 1;
    }
    let mut added = Vec::new();
    for finding in after
        .iter()
        .filter(|finding| finding.severity >= Severity::Error)
    {
        match held.get_mut(&rule(finding)) {
            Some(count) if *count > 0 => *count -= 1,
            _ => added.push(finding.clone()),
        }
    }
    added
}

impl Editor<'_> {
    /// The document the editor holds.
    #[must_use]
    pub fn document(&self) -> &CadIr {
        self.document
    }

    /// The findings validation reports on the document as it now stands.
    #[must_use]
    pub fn findings(&self) -> &[Finding] {
        &self.findings
    }

    /// Applies an edit to a copy of the document, and keeps the copy only
    /// where the edit succeeds and its result holds no Error finding that
    /// the document did not hold.
    fn apply<T>(
        &mut self,
        edit: impl FnOnce(&mut CadIr) -> Result<T, EditError>,
    ) -> Result<T, EditError> {
        let mut next = self.document.clone();
        let done = edit(&mut next)?;
        let findings = findings(&next)?;
        let added = added_errors(&self.findings, &findings);
        if !added.is_empty() {
            return Err(EditError::Invalid(added));
        }
        *self.document = next;
        self.findings = findings;
        Ok(done)
    }

    /// Sets a parameter's value to a literal: its base definition becomes a
    /// one-literal expression (§4). It writes no configuration's or
    /// arrangement's override; each override of the parameter is then
    /// recomputed, or removed where it states no expression and its value is
    /// cleared. Every evaluation and current state that depends on the
    /// parameter is marked `Stale`, and every dependent cache is recomputed
    /// or cleared.
    ///
    /// # Errors
    ///
    /// Returns [`EditError::DrivenParameter`] for a parameter that is
    /// `Driven`, `Assigned`, or whose expression has references, with what
    /// drives it; [`EditError::KindMismatch`] for a literal that is not a
    /// value of the parameter's kind (a number of another quantity, a number
    /// with a unit or a fraction for an integer kind, another variant, or any
    /// value for a `Native` kind); [`EditError::UnknownParameter`] for a
    /// parameter the design does not hold; and [`EditError::Invalid`] where
    /// the edit adds an Error finding.
    pub fn set_dimension(
        &mut self,
        parameter: &ParameterId,
        value: Literal,
    ) -> Result<(), EditError> {
        self.apply(|document| document.set_dimension(parameter, value))
    }

    /// Replaces a parameter's definition with an expression (§4). Then every
    /// evaluation and current state that depends on the parameter is marked
    /// `Stale`, and every dependent cache is recomputed or cleared.
    ///
    /// # Errors
    ///
    /// Returns [`EditError::KindMismatch`] for a neutral expression that does
    /// not type-check by §5.13 (every `if` branch included, `min` and `max`
    /// with at least two arguments), whose root or whose value does not have
    /// the parameter's kind, or that holds the law parameter or a bend
    /// variable; [`EditError::UnknownParameter`] for a parameter, or a
    /// referenced parameter, that the design does not hold; and
    /// [`EditError::Invalid`] where the edit adds an Error finding. An
    /// integer-kind expression whose value is not whole is not refused: it
    /// has no cache (§5.13).
    pub fn set_parameter_expression(
        &mut self,
        parameter: &ParameterId,
        expression: crate::design::expr::Expression,
    ) -> Result<(), EditError> {
        self.apply(|document| document.set_parameter_expression(parameter, expression))
    }

    /// Replaces a feature's definition (§4). Then every evaluation and
    /// current state that depends on the feature is marked `Stale`, and
    /// every dependent cache is recomputed or cleared.
    ///
    /// # Errors
    ///
    /// Returns [`EditError::UnknownFeature`] for a feature the design does
    /// not hold, and [`EditError::Invalid`] where the edit adds an Error
    /// finding.
    pub fn set_feature_definition(
        &mut self,
        feature: &FeatureId,
        definition: FeatureDefinition,
    ) -> Result<(), EditError> {
        self.apply(|document| document.set_feature_definition(feature, definition))
    }

    /// Adds `records` to the document and makes their one new member a
    /// member of `container`, at `index` where it is ordered (§4).
    /// `records` holds one feature, or one container with its members and
    /// theirs, and the records they own: their sketches and spatial sketches
    /// with their entities and constraints, the sheets their `Spreadsheet`
    /// operations name, and the parameters and programs they own. Every
    /// evaluation and current state at or after the member's position is
    /// marked `Stale`, and so is every one that depends on a feature at or
    /// after it. A folder that holds the index grows to hold the member, and
    /// a later folder moves with the members after it.
    ///
    /// # Errors
    ///
    /// Returns [`EditError::NotOneMember`] where `records` holds anything
    /// else, [`EditError::DuplicateId`] where the document already holds one
    /// of their ids, [`EditError::UnknownContainer`] for a container the
    /// design does not hold, [`EditError::IndexOutOfRange`] for an index past
    /// the end of an ordered container, and [`EditError::Invalid`] where the
    /// edit adds an Error finding.
    pub fn insert_member(
        &mut self,
        container: &ContainerId,
        index: usize,
        records: Design,
    ) -> Result<(), EditError> {
        self.apply(|document| document.insert_member(container, index, records))
    }

    /// Deletes `member` from the document with what it owns (§4): where it is
    /// a container, every member under it; the sketches and spatial sketches
    /// a deleted feature owns, with their entities and constraints; the sheet
    /// a deleted feature's `Spreadsheet` operation names; the parameters a
    /// deleted feature or container owns, and the programs a deleted feature
    /// owns; and every override keyed by a deleted record. It marks `Stale`
    /// every evaluation and current state at or after the member's position
    /// in an ordered container, and every one that depends on a feature at
    /// or after it; in an unordered container, every one that depends on a
    /// deleted member. It deletes the evaluations keyed by a deleted member
    /// and rebases the evaluations and process states based on them, and
    /// strips from each kept state's name table the names that mention a
    /// deleted record. In an ordered container every other position that
    /// names a deleted member retargets to the position before the member,
    /// and a rollback before it moves to the member after it. In an
    /// unordered container, and for a current state inside a removed
    /// container, the current state at the position is deleted with the part
    /// of its extent no other body holds, after what was based on it is
    /// rebased; selections pinned to a deleted member's result are anchored
    /// `Current`; each model-class record that targets only deleted geometry
    /// or deleted evaluations is deleted, with the tessellations a removed
    /// `Stored` feature names; and presentation layers lose the deleted
    /// entities. Every pick whose `resolved` lists a deleted record loses
    /// that cache.
    ///
    /// # Errors
    ///
    /// Returns [`EditError::NotAMember`] for a member in no container,
    /// [`EditError::Referenced`] with the referring records where a record
    /// the removal keeps names a record it deletes by id, and
    /// [`EditError::Invalid`] where the edit adds an Error finding.
    pub fn remove_member(&mut self, member: &Member) -> Result<(), EditError> {
        self.apply(|document| document.remove_member(member))
    }

    /// Makes `configuration` the active configuration of `scope`'s design
    /// (§4, §5.5).
    ///
    /// For each current state of that scope it writes the outgoing extent
    /// (its bodies and the records they reach, their names and owners, and
    /// its poses) into an evaluation of the outgoing configuration at the
    /// state's position, and installs the incoming configuration's
    /// evaluation of that position in its place: its records into
    /// `CadIr.geometry`, its names into `Design.names`, its bodies' owners
    /// into the product definitions' `bodies`, and its poses into the current
    /// state, which lists the installed bodies; that evaluation is removed.
    /// The outgoing records that no other body reaches leave the geometry,
    /// with their names and owners. Where the document holds no evaluation
    /// of that position for `configuration`, the current state is kept and
    /// marked `Stale`. Evaluations based on an exchanged current state are
    /// rebased onto the outgoing evaluation, and those based on an installed
    /// evaluation onto the current state, so their resolved states are
    /// unchanged. Each construction of the scope's design whose `resolved`
    /// is a cache moves it into `resolved_in` under the outgoing
    /// configuration, and takes the incoming configuration's entry. Other
    /// scopes, and the scope's other records, are untouched.
    ///
    /// A current state at an unstated rollback is read as built through its
    /// container's end, and its outgoing state, which has no spelling as an
    /// evaluation, is not written: the returned [`ConfigurationSwitch`]
    /// lists each such state's position.
    ///
    /// # Errors
    ///
    /// Returns [`EditError::UnknownScope`] for a definition with no scope,
    /// [`EditError::UnknownConfiguration`] for a configuration that is not
    /// one of the scope's, and [`EditError::Invalid`] where the edit adds an
    /// Error finding.
    pub fn set_active_configuration(
        &mut self,
        scope: &ProductDefinitionId,
        configuration: &ConfigurationId,
    ) -> Result<ConfigurationSwitch, EditError> {
        self.apply(|document| document.set_active_configuration(scope, configuration))
    }
}

fn container_members(container: &Container) -> Vec<&Member> {
    match container {
        Container::Ordered { members, .. } => members.iter().collect(),
        Container::Unordered { members, .. } => members.iter().collect(),
    }
}

fn malformed(error: impl std::fmt::Display) -> EditError {
    EditError::Malformed(error.to_string())
}

pub(crate) mod effective;
mod process;
