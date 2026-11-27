// SPDX-License-Identifier: Apache-2.0
//! Rules that bound a dimension's value (part 10, Bounded values). A rule is
//! an Error where every value it reads is literal. A bound dimension stores
//! no value: its value in a configuration is the cache of its parameter there
//! (§5.13), so the rule is a Warning in each configuration in which the
//! operation's definition is in force and the caches it reads are stated.

use std::cell::Cell;

use super::super::dimensions::{from_cache, Bound};
use super::super::parameters::cache_in;
use super::super::records::DesignRecords;
use super::{error, warning, Site};
use crate::design::configuration::DesignConfiguration;
use crate::design::dimension::Dimension;
use crate::report::check::Finding;

/// The values a bound rule reads: the literals, or, in a configuration, the
/// literals and the stated caches of the parameters the dimensions bind.
pub(super) struct Values<'v, 'a> {
    configuration: Option<(&'v DesignRecords<'a>, &'v [&'a DesignConfiguration])>,
    /// Whether the rule read a dimension that binds a parameter.
    bound: Cell<bool>,
}

impl<'v, 'a> Values<'v, 'a> {
    fn new(configuration: Option<(&'v DesignRecords<'a>, &'v [&'a DesignConfiguration])>) -> Self {
        Self {
            configuration,
            bound: Cell::new(false),
        }
    }

    /// The value of `dimension`: its literal, or its parameter's cache in the
    /// configuration, with the opposite sign or the complement under
    /// `Negated`. `None` where no cache is stated, or where the dimension's
    /// type does not admit it.
    pub(super) fn get<T: Bound + Clone>(&self, dimension: &Dimension<T>) -> Option<T> {
        let (parameter, negated) = match dimension {
            Dimension::Literal(value) => return Some(value.clone()),
            Dimension::Parameter(parameter) => (parameter, false),
            Dimension::Negated(parameter) => (parameter, true),
        };
        self.bound.set(true);
        let (index, chain) = self.configuration?;
        from_cache(cache_in(index, chain, parameter)?, negated)
    }
}

impl<'a> Site<'_, 'a> {
    /// Checks a rule that bounds the values `breach` reads, which returns the
    /// rule's message where they break it. A breach among the literals alone
    /// is an Error. Where the rule reads a bound dimension, a breach in a
    /// configuration is a Warning.
    pub(super) fn bound(
        self,
        findings: &mut Vec<Finding>,
        breach: impl Fn(&Values<'_, 'a>) -> Option<String>,
    ) {
        let literal = Values::new(None);
        if let Some(message) = breach(&literal) {
            error(
                findings,
                self.entity,
                crate::rule!("design.bounded-value", "{message}"),
            );
            return;
        }
        if !literal.bound.get() {
            return;
        }
        let Some(index) = self.index else {
            return;
        };
        for configuration in self.configurations() {
            let chain = index.configuration_chain(configuration);
            let values = Values::new(Some((index, &chain)));
            if let Some(message) = breach(&values) {
                let at = configuration.map_or_else(
                    || "the base".to_owned(),
                    |configuration| format!("configuration {configuration}"),
                );
                warning(
                    findings,
                    self.entity,
                    crate::rule!("design.cached-values-parameters-dimensions-bind", 
                        "{message}, in {at}, by the cached values of the parameters its dimensions bind"
                    ),
                );
            }
        }
    }

    /// The configurations in which the site's definition is in force: each
    /// configuration of the scope whose chain states no other definition of
    /// the feature, or the base where the scope has no configuration. None
    /// where an arrangement may state another definition of the feature. A
    /// site with no definition, a sketch's, is in force in every
    /// configuration of its scope.
    fn configurations(self) -> Vec<Option<&'a str>> {
        let (Some(index), Some(scope)) = (self.index, self.scope) else {
            return Vec::new();
        };
        let configurations: Vec<&'a str> = index
            .ir
            .design
            .configurations
            .iter()
            .filter(|configuration| configuration.scope.as_str() == scope)
            .map(|configuration| configuration.id.as_str())
            .collect();
        let Some(definition) = self.definition else {
            return if configurations.is_empty() {
                vec![None]
            } else {
                configurations.into_iter().map(Some).collect()
            };
        };
        let Some(feature) = index.features.get(self.feature).copied() else {
            return Vec::new();
        };
        let arranged = index.ir.design.arrangements.iter().any(|arrangement| {
            arrangement
                .overrides
                .features
                .get(&feature.id)
                .is_some_and(|state| state.definition.is_some())
        });
        if arranged {
            return Vec::new();
        }
        let in_force = |configuration: Option<&str>| {
            std::ptr::eq(
                index.definition_in(feature, configuration, None),
                definition,
            )
        };
        if configurations.is_empty() {
            return if in_force(None) {
                vec![None]
            } else {
                Vec::new()
            };
        }
        configurations
            .into_iter()
            .filter(|configuration| in_force(Some(configuration)))
            .map(Some)
            .collect()
    }
}
