// SPDX-License-Identifier: Apache-2.0
//! Content classes and the scope a document declares over them.

use cadmpeg_derive::DeserializeName;
#[cfg(feature = "schema")]
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::fmt;

use super::DecodeDepth;

/// The class of a neutral arena.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, DeserializeName)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(rename_all = "snake_case")]
pub enum ArenaClass {
    /// The current geometry, product structure, presentation, physics,
    /// assets, drawings, and PMI.
    Model,
    /// The design that built the model: features, sketches, parameters,
    /// configurations, spreadsheets, and their topology bindings.
    Design,
    /// The manufacturing process that makes the model.
    Process,
}

impl ArenaClass {
    /// Every class, in registry-class order.
    pub const ALL: [Self; 3] = [Self::Model, Self::Design, Self::Process];

    /// The class a document must hold to hold this class.
    #[must_use]
    pub const fn requires(self) -> Option<Self> {
        match self {
            Self::Model => None,
            Self::Design => Some(Self::Model),
            Self::Process => Some(Self::Design),
        }
    }
}

/// The classes a document holds, one required boolean per class.
///
/// A held class has meaningful arenas, including empty ones. A class that is
/// not held has empty arenas. `design` requires `model`, and `process`
/// requires `design`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields, try_from = "ContentScopeWire")]
pub struct ContentScope {
    /// Model arenas are meaningful, including when empty.
    pub model: bool,
    /// Design arenas are meaningful, including when empty.
    pub design: bool,
    /// Process arenas are meaningful, including when empty.
    pub process: bool,
}

#[derive(Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
struct ContentScopeWire {
    /// Model arenas are meaningful, including when empty.
    model: bool,
    /// Design arenas are meaningful, including when empty.
    design: bool,
    /// Process arenas are meaningful, including when empty.
    process: bool,
}

impl TryFrom<ContentScopeWire> for ContentScope {
    type Error = ScopeDependency;

    fn try_from(wire: ContentScopeWire) -> Result<Self, Self::Error> {
        let scope = Self {
            model: wire.model,
            design: wire.design,
            process: wire.process,
        };
        match scope.unmet_requirement() {
            Some(error) => Err(error),
            None => Ok(scope),
        }
    }
}

/// A held class whose required class the scope excludes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[error("the scope holds {held} but excludes {required}, which {held} requires")]
pub struct ScopeDependency {
    /// The held class.
    pub held: ArenaClass,
    /// The class it requires.
    pub required: ArenaClass,
}

impl ContentScope {
    /// Holds every class.
    #[must_use]
    pub const fn full() -> Self {
        Self {
            model: true,
            design: true,
            process: true,
        }
    }

    /// Holds no class.
    #[must_use]
    pub const fn container() -> Self {
        Self {
            model: false,
            design: false,
            process: false,
        }
    }

    /// Holds the model.
    #[must_use]
    pub const fn geometry() -> Self {
        Self {
            model: true,
            design: false,
            process: false,
        }
    }

    /// Holds the model and its design.
    #[must_use]
    pub const fn design() -> Self {
        Self {
            model: true,
            design: true,
            process: false,
        }
    }

    /// Whether the specified class is held.
    #[must_use]
    pub const fn holds(self, class: ArenaClass) -> bool {
        match class {
            ArenaClass::Model => self.model,
            ArenaClass::Design => self.design,
            ArenaClass::Process => self.process,
        }
    }

    /// This scope with `class` held or excluded.
    #[must_use]
    pub const fn with(mut self, class: ArenaClass, held: bool) -> Self {
        match class {
            ArenaClass::Model => self.model = held,
            ArenaClass::Design => self.design = held,
            ArenaClass::Process => self.process = held,
        }
        self
    }

    /// The classes this scope holds, in registry-class order.
    pub fn classes(self) -> impl Iterator<Item = ArenaClass> {
        ArenaClass::ALL
            .into_iter()
            .filter(move |class| self.holds(*class))
    }

    /// Whether no class is held.
    #[must_use]
    pub const fn is_empty(self) -> bool {
        !self.model && !self.design && !self.process
    }

    /// Required classes absent from this scope.
    #[must_use]
    pub fn missing_from(self, required: Self) -> ArenaClasses {
        required
            .classes()
            .filter(|class| !self.holds(*class))
            .collect()
    }

    /// The first held class whose required class this scope excludes.
    #[must_use]
    pub fn unmet_requirement(self) -> Option<ScopeDependency> {
        self.classes().find_map(|held| {
            let required = held.requires()?;
            (!self.holds(required)).then_some(ScopeDependency { held, required })
        })
    }

    /// The deepest request this scope satisfies.
    #[must_use]
    pub const fn depth(self) -> DecodeDepth {
        if !self.model {
            DecodeDepth::Container
        } else if !self.design {
            DecodeDepth::Geometry
        } else if !self.process {
            DecodeDepth::Design
        } else {
            DecodeDepth::Full
        }
    }
}

impl DecodeDepth {
    /// The classes a decode at this depth requests.
    #[must_use]
    pub const fn scope(self) -> ContentScope {
        match self {
            Self::Container => ContentScope::container(),
            Self::Geometry => ContentScope::geometry(),
            Self::Design => ContentScope::design(),
            Self::Full => ContentScope::full(),
        }
    }
}

/// A sorted set of arena classes.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(transparent)]
pub struct ArenaClasses(
    #[serde(with = "crate::distinct_keys::set")]
    #[cfg_attr(
        feature = "schema",
        schemars(with = "std::collections::BTreeSet<ArenaClass>")
    )]
    std::collections::BTreeSet<ArenaClass>,
);

impl ArenaClasses {
    /// An empty class set.
    #[must_use]
    pub const fn new() -> Self {
        Self(std::collections::BTreeSet::new())
    }
    /// Whether no classes are present.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
    /// Number of classes.
    #[must_use]
    pub fn len(&self) -> usize {
        self.0.len()
    }
    /// Adds a class, returning whether it was absent.
    pub fn insert(&mut self, class: ArenaClass) -> bool {
        self.0.insert(class)
    }
    /// Whether the class is present.
    #[must_use]
    pub fn contains(&self, class: &ArenaClass) -> bool {
        self.0.contains(class)
    }
    /// Classes in registry-class order.
    pub fn iter(&self) -> impl Iterator<Item = &ArenaClass> {
        self.0.iter()
    }
}
impl Extend<ArenaClass> for ArenaClasses {
    fn extend<T: IntoIterator<Item = ArenaClass>>(&mut self, iter: T) {
        self.0.extend(iter);
    }
}
impl FromIterator<ArenaClass> for ArenaClasses {
    fn from_iter<T: IntoIterator<Item = ArenaClass>>(iter: T) -> Self {
        Self(iter.into_iter().collect())
    }
}
impl<const N: usize> From<[ArenaClass; N]> for ArenaClasses {
    fn from(classes: [ArenaClass; N]) -> Self {
        classes.into_iter().collect()
    }
}
impl IntoIterator for ArenaClasses {
    type Item = ArenaClass;
    type IntoIter = std::collections::btree_set::IntoIter<ArenaClass>;
    fn into_iter(self) -> Self::IntoIter {
        self.0.into_iter()
    }
}
impl<'a> IntoIterator for &'a ArenaClasses {
    type Item = &'a ArenaClass;
    type IntoIter = std::collections::btree_set::Iter<'a, ArenaClass>;
    fn into_iter(self) -> Self::IntoIter {
        self.0.iter()
    }
}
impl fmt::Display for ArenaClass {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Model => "model",
            Self::Design => "design",
            Self::Process => "process",
        })
    }
}
impl fmt::Display for ArenaClasses {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for (index, class) in self.iter().enumerate() {
            if index != 0 {
                f.write_str(", ")?;
            }
            fmt::Display::fmt(class, f)?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::{ArenaClass, ArenaClasses, ContentScope, ScopeDependency};
    use crate::decode::DecodeDepth;

    #[test]
    fn scope_is_three_required_wire_booleans() {
        let full = serde_json::to_value(ContentScope::full()).expect("scope serializes");
        assert_eq!(
            full,
            serde_json::json!({"model": true, "design": true, "process": true})
        );
        assert!(serde_json::from_str::<ContentScope>(r#"{"model":true,"design":true}"#).is_err());
        assert!(serde_json::from_str::<ContentScope>(
            r#"{"model":true,"design":true,"process":false,"extra":false}"#
        )
        .is_err());
    }

    #[test]
    fn design_requires_model_and_process_requires_design() {
        assert_eq!(
            ContentScope::container()
                .with(ArenaClass::Design, true)
                .unmet_requirement(),
            Some(ScopeDependency {
                held: ArenaClass::Design,
                required: ArenaClass::Model,
            })
        );
        assert_eq!(
            ContentScope::geometry()
                .with(ArenaClass::Process, true)
                .unmet_requirement(),
            Some(ScopeDependency {
                held: ArenaClass::Process,
                required: ArenaClass::Design,
            })
        );
        assert!(serde_json::from_str::<ContentScope>(
            r#"{"model":false,"design":true,"process":false}"#
        )
        .is_err());
        assert!(serde_json::from_str::<ContentScope>(
            r#"{"model":true,"design":false,"process":true}"#
        )
        .is_err());
    }

    #[test]
    fn each_depth_requests_the_scope_it_satisfies() {
        for (depth, scope) in [
            (DecodeDepth::Container, ContentScope::container()),
            (DecodeDepth::Geometry, ContentScope::geometry()),
            (DecodeDepth::Design, ContentScope::design()),
            (DecodeDepth::Full, ContentScope::full()),
        ] {
            assert_eq!(depth.scope(), scope);
            assert_eq!(scope.depth(), depth);
            assert_eq!(scope.unmet_requirement(), None);
            let wire = serde_json::to_string(&scope).expect("scope serializes");
            assert_eq!(
                serde_json::from_str::<ContentScope>(&wire).expect("scope parses"),
                scope
            );
        }
    }

    #[test]
    fn missing_classes_are_those_the_requirement_holds() {
        assert_eq!(
            ContentScope::geometry().missing_from(ContentScope::full()),
            ArenaClasses::from([ArenaClass::Design, ArenaClass::Process])
        );
    }

    #[test]
    fn class_sets_have_a_sorted_array_wire_shape() {
        let classes = ArenaClasses::from([ArenaClass::Process, ArenaClass::Model]);
        let wire = serde_json::to_value(&classes).expect("class set serializes");
        assert_eq!(wire, serde_json::json!(["model", "process"]));
        assert_eq!(
            serde_json::from_value::<ArenaClasses>(wire).expect("class set parses"),
            classes
        );
    }

    #[test]
    fn a_class_set_refuses_a_repeat_and_names_out_of_order() {
        for (wire, refusal) in [
            (r#"["process","model"]"#, "out of order"),
            (r#"["model","model"]"#, "twice"),
        ] {
            let error = serde_json::from_str::<ArenaClasses>(wire)
                .expect_err("a class set has one spelling")
                .to_string();
            assert!(error.contains(refusal), "{wire}: {error}");
        }
        let sorted: ArenaClasses =
            serde_json::from_str(r#"["model","process"]"#).expect("sorted classes read");
        assert_eq!(
            sorted,
            ArenaClasses::from([ArenaClass::Model, ArenaClass::Process])
        );
    }
}
