// SPDX-License-Identifier: Apache-2.0
//! Edit helpers and resolvers of the design layer (§4 of the CADIR v11
//! specification).
//!
//! The resolvers read a value through the configuration that applies:
//! [`dimension_value`](crate::CadIr::dimension_value) resolves a bound
//! dimension from its parameter's cache, and
//! [`is_stateless`](crate::CadIr::is_stateless) decides which features have
//! evaluations. The edit helpers change one canonical fact and keep every
//! cache that depends on it honest:
//! [`set_dimension`](crate::CadIr::set_dimension),
//! [`set_parameter_expression`](crate::CadIr::set_parameter_expression), and
//! [`set_feature_definition`](crate::CadIr::set_feature_definition) mark the
//! dependent states `Stale`, recompute the dependent caches they can, and
//! clear the rest; [`insert_member`](crate::CadIr::insert_member) and
//! [`remove_member`](crate::CadIr::remove_member) edit a container and keep
//! every position valid; and
//! [`set_active_configuration`](crate::CadIr::set_active_configuration)
//! exchanges the current geometry of one scope for another configuration's
//! state.
//!
//! Positions, references, measures, and constructions occur throughout the
//! feature vocabulary. The helpers find them in a record's wire form, in which
//! each has one spelling (§2.5), and read the record back through its strict
//! reader after a rewrite.

mod caches;
mod configuration;
pub(crate) mod eval;
mod history;
mod members;
mod resolve;
mod states;
mod wire;

#[cfg(test)]
mod tests;

use crate::design::container::Container;
use crate::design::dimension::ParameterRef;
use crate::design::ids::{ContainerId, ParameterProgramId};
use crate::design::refs::Member;
use crate::features::{ConfigurationId, FeatureId, ParameterId};
use crate::ids::ProductDefinitionId;

pub use eval::{evaluate, EvalError, Value};

/// Why an edit helper or a resolver refuses.
#[derive(Debug, Clone, PartialEq, thiserror::Error)]
pub enum EditError {
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
    /// The member is already in a container or is a root of a scope.
    #[error("the member {0:?} is already in a container or is a scope root")]
    AlreadyPlaced(Member),
    /// The index is past the end of the ordered container.
    #[error("index {index} is past the end of container {container}")]
    IndexOutOfRange {
        /// The container.
        container: ContainerId,
        /// The index.
        index: usize,
    },
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
    /// A position names the removed member of an unordered container, which
    /// has no position before it.
    #[error("a position names {0:?}, a member of an unordered container, which has no position before it")]
    NoPositionBefore(Member),
    /// A container is its own ancestor, or a member is in two containers.
    #[error("the containers do not form a tree: {0}")]
    MalformedHistory(String),
    /// A rewritten record does not read back, or an id could not be minted.
    #[error("a record does not read back after the edit: {0}")]
    Malformed(String),
}

/// What drives a parameter that
/// [`set_dimension`](crate::CadIr::set_dimension) refuses.
#[derive(Debug, Clone, PartialEq)]
pub enum ParameterDrivers {
    /// Geometry determines the value: a reference dimension.
    Geometry,
    /// A parameter program sets the value.
    Program(ParameterProgramId),
    /// The expression's references, in order of first use.
    References(Vec<ParameterRef>),
}

/// A literal value for [`set_dimension`](crate::CadIr::set_dimension): a
/// number in its authored unit, a Boolean, or a text. An integer is a
/// dimensionless number.
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

fn container_members(container: &Container) -> Vec<&Member> {
    match container {
        Container::Ordered { members, .. } => members.iter().collect(),
        Container::Unordered { members, .. } => members.iter().collect(),
    }
}

fn malformed(error: impl std::fmt::Display) -> EditError {
    EditError::Malformed(error.to_string())
}
