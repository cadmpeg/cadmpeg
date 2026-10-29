// SPDX-License-Identifier: Apache-2.0
//! Independent content classes and declarations shared by codecs and documents.

#[cfg(feature = "schema")]
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::fmt;

/// Whether a model arena holds the product model or the design that built it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(rename_all = "snake_case")]
pub enum ArenaClass {
    /// B-rep, tessellation, product structure, placements, appearance, PMI,
    /// attributes, and the assets they reference.
    Model,
    /// Features, sketches, parameters, configurations, spreadsheets, and the
    /// feature topology bindings.
    Design,
}

/// Classes held by a document, independently of decode history.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct ContentScope {
    /// Model arenas are meaningful, including when empty.
    pub model: bool,
    /// Design arenas are meaningful, including when empty.
    pub design: bool,
}

impl ContentScope {
    /// Holds all registered classes.
    pub const fn full() -> Self {
        Self {
            model: true,
            design: true,
        }
    }
    /// Holds no neutral class.
    pub const fn container() -> Self {
        Self {
            model: false,
            design: false,
        }
    }
    /// Holds the product model.
    pub const fn geometry() -> Self {
        Self {
            model: true,
            design: false,
        }
    }
    /// Whether the specified class is held.
    pub const fn holds(self, class: ArenaClass) -> bool {
        match class {
            ArenaClass::Model => self.model,
            ArenaClass::Design => self.design,
        }
    }
    /// The classes this document holds, in registry-class order.
    pub fn classes(self) -> impl Iterator<Item = ArenaClass> {
        [ArenaClass::Model, ArenaClass::Design]
            .into_iter()
            .filter(move |class| self.holds(*class))
    }
    /// Whether no class is held.
    pub const fn is_empty(self) -> bool {
        !self.model && !self.design
    }
    /// Required classes absent from this scope.
    pub fn missing_from(self, required: Self) -> ArenaClasses {
        required
            .classes()
            .filter(|class| !self.holds(*class))
            .collect()
    }
}

/// A sorted set of arena classes.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(transparent)]
pub struct ArenaClasses(std::collections::BTreeSet<ArenaClass>);

impl ArenaClasses {
    /// An empty class set.
    pub const fn new() -> Self {
        Self(std::collections::BTreeSet::new())
    }
    /// Whether no classes are present.
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
    /// Number of classes.
    pub fn len(&self) -> usize {
        self.0.len()
    }
    /// Adds a class, returning whether it was absent.
    pub fn insert(&mut self, class: ArenaClass) -> bool {
        self.0.insert(class)
    }
    /// Whether the class is present.
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
    use super::{ArenaClass, ArenaClasses, ContentScope};

    #[test]
    fn classes_are_independent_and_required_wire_booleans() {
        let design = ContentScope {
            model: false,
            design: true,
        };
        assert!(design.holds(ArenaClass::Design));
        assert!(!design.holds(ArenaClass::Model));
        assert_eq!(
            design.missing_from(ContentScope::full()),
            ArenaClasses::from([ArenaClass::Model])
        );
        assert!(serde_json::from_str::<ContentScope>(r#"{"model":true}"#).is_err());
        assert!(serde_json::from_str::<ContentScope>(
            r#"{"model":true,"design":true,"extra":false}"#
        )
        .is_err());
    }

    #[test]
    fn class_sets_have_a_sorted_array_wire_shape() {
        let classes = ArenaClasses::from([ArenaClass::Design, ArenaClass::Model]);
        let wire = serde_json::to_value(&classes).expect("class set serializes");
        assert_eq!(wire, serde_json::json!(["model", "design"]));
        assert_eq!(
            serde_json::from_value::<ArenaClasses>(wire).expect("class set parses"),
            classes
        );
    }
}
