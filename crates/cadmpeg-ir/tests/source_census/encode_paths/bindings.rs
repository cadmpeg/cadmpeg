// SPDX-License-Identifier: Apache-2.0
//! Lexical value bindings and conservative function-value flow.

use std::collections::{BTreeMap, BTreeSet};

#[derive(Clone, Debug)]
pub(super) enum Value {
    Unknown,
    Functions(BTreeSet<Vec<String>>),
}

#[derive(Default)]
pub(super) struct Bindings(Vec<BTreeMap<String, Value>>);
impl Bindings {
    pub(super) fn push(&mut self) {
        self.0.push(BTreeMap::new());
    }
    pub(super) fn pop(&mut self) {
        self.0.pop();
    }
    pub(super) fn get(&self, name: &str) -> Option<&Value> {
        self.0.iter().rev().find_map(|scope| scope.get(name))
    }
    pub(super) fn bind(&mut self, pattern: &syn::Pat, value: Value) {
        match pattern {
            syn::Pat::Ident(name) => {
                if let Some(scope) = self.0.last_mut() {
                    scope.insert(name.ident.to_string(), value);
                }
                if let Some((_, pattern)) = &name.subpat {
                    self.bind(pattern, Value::Unknown);
                }
            }
            syn::Pat::Type(pattern) => self.bind(&pattern.pat, value),
            syn::Pat::Reference(pattern) => self.bind(&pattern.pat, value),
            syn::Pat::Tuple(pattern) => {
                for part in &pattern.elems {
                    self.bind(part, Value::Unknown);
                }
            }
            syn::Pat::Struct(pattern) => {
                for part in &pattern.fields {
                    self.bind(&part.pat, Value::Unknown);
                }
            }
            syn::Pat::TupleStruct(pattern) => {
                for part in &pattern.elems {
                    self.bind(part, Value::Unknown);
                }
            }
            syn::Pat::Slice(pattern) => {
                for part in &pattern.elems {
                    self.bind(part, Value::Unknown);
                }
            }
            syn::Pat::Or(pattern) => {
                for part in &pattern.cases {
                    self.bind(part, Value::Unknown);
                }
            }
            _ => {}
        }
    }
    pub(super) fn assign(&mut self, name: &str, value: Value) {
        if let Some(old) = self
            .0
            .iter_mut()
            .rev()
            .find_map(|scope| scope.get_mut(name))
        {
            match (old, value) {
                (Value::Functions(old), Value::Functions(new)) => {
                    old.extend(new);
                }
                (old, _) => *old = Value::Unknown,
            }
        }
    }
}
