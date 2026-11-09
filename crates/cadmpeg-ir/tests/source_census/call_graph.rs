// SPDX-License-Identifier: Apache-2.0
//! Name-resolved workspace functions, calls and constructor-position paths.

use std::collections::{BTreeMap, BTreeSet, VecDeque};

mod bindings;
mod body;
pub(crate) mod source;

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum Call {
    Function(Vec<String>),
    Method(String),
    SelfMethod(String),
    MethodOnConstructor {
        constructor: Vec<String>,
        method: String,
        unwrapped: bool,
    },
    Macro(String),
    UnresolvedValue(String),
}

/// Constructor-position path use (§5.2 scan): where a path builds a value.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum CtorKind {
    /// A struct literal path (`Type { .. }`, including struct variants).
    Struct,
    /// A call callee (`path(...)`, including tuple-variant constructors).
    Call,
    /// A unit path used as a value (not a comparison operand).
    Unit,
}

/// One constructor-position path use with its source line.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct CtorUse {
    pub(crate) path: Vec<String>,
    pub(crate) kind: CtorKind,
    pub(crate) line: usize,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum SelfReturn {
    Direct,
    Wrapped,
}

#[derive(Debug)]
pub(crate) struct Function {
    pub(crate) scope: Vec<String>,
    pub(crate) owner: Option<Vec<String>>,
    pub(crate) name: String,
    pub(crate) line: usize,
    pub(crate) root: bool,
    pub(crate) root_trait: Option<Vec<String>>,
    pub(crate) is_macro: bool,
    pub(crate) returns_self: Option<SelfReturn>,
    pub(crate) calls: BTreeSet<Call>,
    pub(crate) ctor: Vec<CtorUse>,
}
/// The bound on alias expansions of one path; re-export chains are short.
const EXPANSION_LIMIT: usize = 256;

#[derive(Default)]
pub(crate) struct Graph {
    pub(crate) functions: Vec<Function>,
    aliases: BTreeMap<Vec<String>, BTreeSet<Vec<String>>>,
    pub(crate) globs: BTreeMap<Vec<String>, BTreeSet<Vec<String>>>,
}

impl Graph {
    fn forwarded_paths(&self, path: &[String]) -> BTreeSet<Vec<String>> {
        let mut found = BTreeSet::new();
        let mut pending = VecDeque::from([(path.to_vec(), BTreeMap::<Vec<String>, usize>::new())]);
        while let Some((path, used)) = pending.pop_front() {
            found.insert(path.clone());
            for length in (1..=path.len()).rev() {
                let prefix = &path[..length];
                let Some(targets) = self.aliases.get(prefix) else {
                    continue;
                };
                if used
                    .get(prefix)
                    .is_some_and(|previous| path.len() >= *previous)
                {
                    break;
                }
                let mut used = used.clone();
                used.insert(prefix.to_vec(), path.len());
                for target in targets {
                    let mut forwarded = target.clone();
                    forwarded.extend_from_slice(&path[length..]);
                    pending.push_back((forwarded, used.clone()));
                }
                break;
            }
        }
        found
    }

    /// Every path that `path` names through named re-exports and aliases and
    /// through the glob imports of the modules on its route.
    pub(crate) fn resolved_paths(&self, path: &[String]) -> BTreeSet<Vec<String>> {
        let mut found = BTreeSet::new();
        let mut pending = VecDeque::from([path.to_vec()]);
        while let Some(path) = pending.pop_front() {
            if found.contains(&path) {
                continue;
            }
            for form in self.forwarded_paths(&path) {
                for split in 1..form.len() {
                    for glob in self.globs.get(&form[..split]).into_iter().flatten() {
                        let mut next = glob.clone();
                        next.extend_from_slice(&form[split..]);
                        if !found.contains(&next) && found.len() < EXPANSION_LIMIT {
                            pending.push_back(next);
                        }
                    }
                }
                found.insert(form);
            }
            found.insert(path);
        }
        found
    }
}

fn module_scope(scope: &[String]) -> Vec<String> {
    scope
        .iter()
        .take_while(|part| !part.starts_with('$'))
        .cloned()
        .collect()
}

fn absolute(path: &[String], scope: &[String]) -> Vec<String> {
    let mut module = module_scope(scope);
    let mut position = 0;
    match path.first().map(String::as_str) {
        Some("crate") => {
            module.truncate(1);
            position = 1;
        }
        Some("self") => {
            position = 1;
        }
        Some("super") => {
            while path.get(position).is_some_and(|part| part == "super") {
                if module.len() > 1 {
                    module.pop();
                }
                position += 1;
            }
        }
        _ => return path.to_vec(),
    }
    module.extend_from_slice(&path[position..]);
    module
}

pub(crate) fn symbol_candidates(path: &[String], caller: &Function) -> Vec<Vec<String>> {
    if matches!(
        path.first().map(String::as_str),
        Some("crate" | "self" | "super")
    ) {
        return vec![absolute(path, &caller.scope)];
    }
    let mut candidates = Vec::new();
    let mut scope = caller.scope.clone();
    let owner = caller
        .owner
        .as_ref()
        .map_or(String::new(), |owner| owner.join("::") + "::");
    scope.push(format!("${owner}{}@{}", caller.name, caller.line));
    loop {
        let mut candidate = scope.clone();
        candidate.extend_from_slice(path);
        candidates.push(candidate);
        if scope.last().is_some_and(|part| part.starts_with('$')) {
            scope.pop();
        } else {
            break;
        }
    }
    candidates.push(path.to_vec());
    candidates
}
