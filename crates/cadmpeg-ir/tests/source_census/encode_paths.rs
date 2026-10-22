// SPDX-License-Identifier: Apache-2.0
//! Conservative name-resolved encode reachability and construction checks.

use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::path::Path;

mod rules;
mod source;
#[cfg(test)]
mod tests;

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
enum Call {
    Function(Vec<String>),
    Method(String),
    Macro(String),
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
    owner: Option<String>,
    name: String,
    line: usize,
    root: bool,
    is_macro: bool,
    calls: BTreeSet<Call>,
    sites: Vec<Site>,
}
impl Function {
    fn label(&self) -> String {
        let owner = self
            .owner
            .as_ref()
            .map_or(String::new(), |name| format!("{name}::"));
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
        let mut free: BTreeMap<&str, Vec<usize>> = BTreeMap::new();
        let mut macros: BTreeMap<&str, Vec<usize>> = BTreeMap::new();
        let mut associated: BTreeMap<(&str, &str), Vec<usize>> = BTreeMap::new();
        for (index, function) in self.functions.iter().enumerate() {
            if function.is_macro {
                macros.entry(&function.name).or_default().push(index);
            } else if let Some(owner) = &function.owner {
                methods.entry(&function.name).or_default().push(index);
                associated
                    .entry((owner, &function.name))
                    .or_default()
                    .push(index);
            } else {
                free.entry(&function.name).or_default().push(index);
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
                    Call::Function(path) => {
                        let Some(name) = path.last() else { continue };
                        groups.extend(free.get(name.as_str()));
                        if path.len() > 1 {
                            let owner = &path[path.len() - 2];
                            let owner = if owner == "Self" {
                                self.functions[index].owner.as_deref().unwrap_or("Self")
                            } else {
                                owner.as_str()
                            };
                            groups.extend(associated.get(&(owner, name.as_str())));
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

    fn capacity_owner(&self, function: &Function) -> bool {
        let crate_name = function.scope.first();
        self.capacities.iter().any(|(package, helper)| {
            if crate_name != Some(package) {
                return false;
            }
            let mut path = function.scope.clone();
            if let Some(owner) = &function.owner {
                path.push(owner.clone());
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
    assert!(found.is_empty(), "{} conservative encode-path finding(s); CodecError candidates require fidelity/synthesis catch-boundary review:\n{}", found.len(), found.join("\n"));
}
