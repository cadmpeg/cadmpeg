// SPDX-License-Identifier: Apache-2.0
//! Conservative name-resolved encode reachability and construction checks.

use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::path::Path;

mod bindings;
mod rules;
mod source;
#[cfg(test)]
mod tests;

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
enum Call {
    Function(Vec<String>),
    Method(String),
    Macro(String),
    UnresolvedValue(String),
}

#[derive(Clone, Debug)]
struct Site {
    line: usize,
    column: usize,
    rule: String,
}

#[derive(Debug)]
struct Function {
    file: String,
    scope: Vec<String>,
    owner: Option<Vec<String>>,
    name: String,
    line: usize,
    root: bool,
    is_macro: bool,
    calls: BTreeSet<Call>,
    sites: Vec<Site>,
}
impl Function {
    fn label(&self) -> String {
        let owner = self.owner.as_ref().map_or(String::new(), |name| {
            format!("{}::", name.last().expect("owner type"))
        });
        format!(
            "{}:{} {}{owner}{}",
            self.file,
            self.line,
            self.scope.join("::") + "::",
            self.name
        )
    }
}

#[derive(Default)]
struct Graph {
    functions: Vec<Function>,
    capacities: Vec<(String, String)>,
    global: Vec<String>,
}

impl Graph {
    fn reachable(&self) -> BTreeMap<usize, Option<usize>> {
        let mut methods: BTreeMap<&str, Vec<usize>> = BTreeMap::new();
        let mut symbols: BTreeMap<Vec<String>, Vec<usize>> = BTreeMap::new();
        let mut macros: BTreeMap<&str, Vec<usize>> = BTreeMap::new();

        for (index, function) in self.functions.iter().enumerate() {
            if function.is_macro {
                macros.entry(&function.name).or_default().push(index);
            } else if let Some(owner) = &function.owner {
                methods.entry(&function.name).or_default().push(index);
                let mut path = owner.clone();
                path.push(function.name.clone());
                symbols.entry(path).or_default().push(index);
            } else {
                let mut path = function.scope.clone();
                path.push(function.name.clone());
                symbols.entry(path).or_default().push(index);
            }
        }
        let mut previous = BTreeMap::new();
        let mut pending = VecDeque::new();
        for (index, function) in self.functions.iter().enumerate() {
            if function.root {
                previous.insert(index, None);
                pending.push_back(index);
            }
        }
        while let Some(index) = pending.pop_front() {
            for call in &self.functions[index].calls {
                let mut groups = Vec::new();
                match call {
                    Call::Method(name) => {
                        groups.extend(methods.get(name.as_str()));
                    }
                    Call::Macro(name) => {
                        groups.extend(macros.get(name.as_str()));
                    }
                    Call::UnresolvedValue(_) => {}
                    Call::Function(path) => {
                        for candidate in symbol_candidates(path, &self.functions[index]) {
                            if let Some(found) = symbols.get(&candidate) {
                                groups.push(found);
                                break;
                            }
                        }
                    }
                }
                for &next in groups.into_iter().flatten() {
                    if let std::collections::btree_map::Entry::Vacant(entry) = previous.entry(next)
                    {
                        entry.insert(Some(index));
                        pending.push_back(next);
                    }
                }
            }
        }
        previous
    }

    fn findings(&self) -> Vec<String> {
        let reached = self.reachable();
        let mut found = self.global.clone();
        for &index in reached.keys() {
            let function = &self.functions[index];
            let mut chain = Vec::new();
            let mut at = Some(index);
            while let Some(index) = at {
                chain.push(self.functions[index].label());
                at = reached[&index];
            }
            chain.reverse();
            for call in &function.calls {
                if let Call::UnresolvedValue(name) = call {
                    found.push(format!(
                        "{}: unresolved callable value {name}; no static target proved [via {}]",
                        function.label(),
                        chain.join(" -> ")
                    ));
                }
            }
            for site in &function.sites {
                if site.rule == rules::CAPACITY && self.capacity_owner(function) {
                    continue;
                }
                found.push(format!(
                    "{}:{}:{}: {} [via {}]",
                    function.file,
                    site.line,
                    site.column,
                    site.rule,
                    chain.join(" -> ")
                ));
            }
        }
        found.sort();
        found.dedup();
        found
    }

    fn unresolved_symbols(&self) -> Vec<String> {
        let mut symbols = BTreeSet::new();
        for function in &self.functions {
            if function.is_macro {
                continue;
            }
            let mut path = function
                .owner
                .clone()
                .unwrap_or_else(|| function.scope.clone());
            path.push(function.name.clone());
            symbols.insert(path);
        }
        let mut unresolved = BTreeSet::new();
        for &index in self.reachable().keys() {
            let function = &self.functions[index];
            for call in &function.calls {
                if let Call::Function(path) = call {
                    if !symbol_candidates(path, function)
                        .iter()
                        .any(|path| symbols.contains(path))
                    {
                        unresolved.insert(format!("{}: {} (no workspace target proved; external/constructor/reexport/function-flow classification remains unresolved)", function.label(), path.join("::")));
                    }
                }
            }
        }
        unresolved.into_iter().collect()
    }

    fn capacity_owner(&self, function: &Function) -> bool {
        let crate_name = function.scope.first();
        self.capacities.iter().any(|(package, helper)| {
            if crate_name != Some(package) {
                return false;
            }
            let mut path = function.scope.clone();
            if let Some(owner) = &function.owner {
                path = owner.clone();
            }
            path.push(function.name.clone());
            let wanted: Vec<_> = helper.split("::").collect();
            path.iter()
                .map(String::as_str)
                .collect::<Vec<_>>()
                .ends_with(&wanted)
        })
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

fn symbol_candidates(path: &[String], caller: &Function) -> Vec<Vec<String>> {
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

#[test]
fn encode_path_constructions_follow_the_typed_error_contract() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("workspace root");
    let graph = source::workspace(root);
    assert!(
        graph.functions.iter().any(|function| function.root),
        "no EncoderBackend roots found"
    );
    let found = graph.findings();
    let unresolved = graph.unresolved_symbols();
    assert!(found.is_empty(), "{} conservative encode-path finding(s); CodecError candidates require fidelity/synthesis catch-boundary review:\n{}\n{} unresolved symbol classifications (first ten):\n{}", found.len(), found.join("\n"), unresolved.len(), unresolved.iter().take(10).cloned().collect::<Vec<_>>().join("\n"));
}
