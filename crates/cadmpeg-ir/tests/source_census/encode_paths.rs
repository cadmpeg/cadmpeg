// SPDX-License-Identifier: Apache-2.0
//! Conservative name-resolved encode reachability and construction checks.

use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::path::Path;

mod bindings;
mod resolve;
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
struct PathCheck {
    path: Vec<String>,
    span: proc_macro2::Span,
    construction: bool,
}

#[derive(Debug)]
struct Function {
    file: String,
    scope: Vec<String>,
    owner: Option<Vec<String>>,
    name: String,
    line: usize,
    root: bool,
    root_trait: Option<Vec<String>>,
    is_macro: bool,
    /// Type parameters in scope; a call through one dispatches by name.
    generics: BTreeSet<String>,
    calls: BTreeSet<Call>,
    sites: Vec<Site>,
    path_checks: Vec<PathCheck>,
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
    aliases: BTreeMap<Vec<String>, BTreeSet<Vec<String>>>,
    globs: BTreeMap<Vec<String>, BTreeSet<Vec<String>>>,
    modules: BTreeSet<Vec<String>>,
    items: BTreeMap<Vec<String>, resolve::Item>,
    externals: BTreeSet<String>,
    generated: BTreeMap<String, BTreeSet<String>>,
}

#[derive(Default)]
struct Walk {
    previous: BTreeMap<usize, Option<usize>>,
    unresolved: BTreeSet<(usize, Vec<String>)>,
    external: BTreeSet<(usize, Vec<String>)>,
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
    fn resolved_paths(&self, path: &[String]) -> BTreeSet<Vec<String>> {
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
                        if !found.contains(&next) && found.len() < resolve::EXPANSION_LIMIT {
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

    fn is_root(&self, function: &Function) -> bool {
        function.root
            || function.root_trait.as_ref().is_some_and(|path| {
                symbol_candidates(path, function).iter().any(|candidate| {
                    self.resolved_paths(candidate).iter().any(|path| {
                        path.iter().map(String::as_str).eq([
                            "cadmpeg_ir",
                            "codec",
                            "write",
                            "EncoderBackend",
                        ])
                    })
                })
            })
    }

    fn reachable(&self) -> BTreeMap<usize, Option<usize>> {
        self.walk().previous
    }

    fn walk(&self) -> Walk {
        let index = resolve::Index::new(self);
        let mut walk = Walk::default();
        let mut pending = VecDeque::new();
        for (at, function) in self.functions.iter().enumerate() {
            if self.is_root(function) {
                walk.previous.insert(at, None);
                pending.push_back(at);
            }
        }
        while let Some(at) = pending.pop_front() {
            for call in &self.functions[at].calls {
                let mut groups = Vec::new();
                match call {
                    Call::Method(name) => {
                        groups.extend(index.methods.get(name.as_str()).cloned());
                    }
                    Call::Macro(name) => {
                        groups.extend(index.macros.get(name.as_str()).cloned());
                    }
                    Call::UnresolvedValue(_) => {}
                    Call::Function(path) => match index.resolve(&self.functions[at], path) {
                        resolve::Resolution::Functions(found) => groups.push(found),
                        resolve::Resolution::NotCallable => {}
                        resolve::Resolution::External => {
                            walk.external.insert((at, path.clone()));
                        }
                        resolve::Resolution::Unresolved => {
                            walk.unresolved.insert((at, path.clone()));
                        }
                    },
                }
                for next in groups.into_iter().flatten() {
                    if let std::collections::btree_map::Entry::Vacant(entry) =
                        walk.previous.entry(next)
                    {
                        entry.insert(Some(at));
                        pending.push_back(next);
                    }
                }
            }
        }
        walk
    }

    fn chain(&self, reached: &BTreeMap<usize, Option<usize>>, index: usize) -> String {
        let mut chain = Vec::new();
        let mut at = Some(index);
        while let Some(index) = at {
            chain.push(self.functions[index].label());
            at = reached[&index];
        }
        chain.reverse();
        chain.join(" -> ")
    }

    fn findings(&self) -> Vec<String> {
        let walk = self.walk();
        let reached = &walk.previous;
        let mut found = self.global.clone();
        for &index in reached.keys() {
            let function = &self.functions[index];
            let chain = self.chain(reached, index);
            for call in &function.calls {
                if let Call::UnresolvedValue(name) = call {
                    found.push(format!(
                        "{}: unresolved callable value {name}; no static target proved [via {chain}]",
                        function.label(),
                    ));
                }
            }
            let mut sites = function.sites.clone();
            for check in &function.path_checks {
                for candidate in symbol_candidates(&check.path, function) {
                    for path in self.resolved_paths(&candidate) {
                        sites.extend(rules::check(&path, check.span, check.construction));
                    }
                }
            }
            for site in &sites {
                if site.rule == rules::CAPACITY && self.capacity_owner(function) {
                    continue;
                }
                found.push(format!(
                    "{}:{}:{}: {} [via {chain}]",
                    function.file, site.line, site.column, site.rule,
                ));
            }
        }
        for (index, path) in &walk.unresolved {
            found.push(format!(
                "{}: unresolved workspace call {}; no workspace function proved [via {}]",
                self.functions[*index].label(),
                path.join("::"),
                self.chain(reached, *index)
            ));
        }
        found.sort();
        found.dedup();
        found
    }

    fn unresolved_symbols(&self) -> Vec<String> {
        self.walk()
            .unresolved
            .iter()
            .map(|(index, path)| {
                format!(
                    "{}: {} (no workspace target proved; classification remains unresolved)",
                    self.functions[*index].label(),
                    path.join("::")
                )
            })
            .collect()
    }

    fn external_calls(&self) -> Vec<String> {
        self.walk()
            .external
            .iter()
            .map(|(index, path)| {
                format!(
                    "{}: {} (outside the workspace)",
                    self.functions[*index].label(),
                    path.join("::")
                )
            })
            .collect()
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
        graph
            .functions
            .iter()
            .any(|function| graph.is_root(function)),
        "no EncoderBackend roots found"
    );
    let found = graph.findings();
    let external = graph.external_calls();
    assert!(found.is_empty(), "{} conservative encode-path finding(s); CodecError candidates require fidelity/synthesis catch-boundary review:\n{}\n{} external call classifications (first ten):\n{}", found.len(), found.join("\n"), external.len(), external.iter().take(10).cloned().collect::<Vec<_>>().join("\n"));
}
