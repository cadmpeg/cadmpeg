//! Scratch sweep: which accepting node shapes sit directly under a denying parent.
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::path::{Path, PathBuf};

const TAGS: &[&str] = &[
    "definition", "kind", "alignment", "space", "type", "policy", "role", "boundary_role", "mode",
    "class", "family", "form", "selector", "variant",
];

fn goldens() -> Vec<PathBuf> {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("crates")
        .to_path_buf();
    let mut out = Vec::new();
    fn walk(dir: &Path, out: &mut Vec<PathBuf>) {
        let Ok(rd) = std::fs::read_dir(dir) else {
            return;
        };
        for e in rd.flatten() {
            let p = e.path();
            if p.is_dir() {
                walk(&p, out);
            } else if p.extension().is_some_and(|x| x == "json")
                && p.to_string_lossy().contains("/tests/golden/")
            {
                out.push(p);
            }
        }
    }
    walk(&root, &mut out);
    out.sort();
    out
}

fn shape_of(path: &str, m: &serde_json::Map<String, Value>) -> String {
    let mut keys: Vec<&str> = m.keys().map(String::as_str).collect();
    keys.sort_unstable();
    let tagvals: Vec<String> = TAGS
        .iter()
        .filter_map(|t| m.get(*t).and_then(Value::as_str).map(|s| format!("{t}={s}")))
        .collect();
    format!("{path} |keys:{} |{}", keys.join(","), tagvals.join(","))
}

/// (own shape, parent shape) for every object node, in the same order `inject` counts them.
fn nodes(v: &Value, path: &str, parent: Option<&str>, out: &mut Vec<(String, Option<String>)>) {
    match v {
        Value::Object(m) => {
            let shape = shape_of(path, m);
            out.push((shape.clone(), parent.map(str::to_string)));
            for (k, child) in m {
                nodes(child, &format!("{path}.{k}"), Some(&shape), out);
            }
        }
        Value::Array(a) => {
            for c in a {
                nodes(c, &format!("{path}[]"), parent, out);
            }
        }
        _ => {}
    }
}

fn inject(v: &mut Value, counter: &mut usize, target: usize) -> bool {
    match v {
        Value::Object(m) => {
            let me = *counter;
            *counter += 1;
            if me == target {
                m.insert("zz_bogus".into(), Value::from(1));
                return true;
            }
            let keys: Vec<String> = m.keys().cloned().collect();
            for k in keys {
                if let Some(c) = m.get_mut(&k) {
                    if inject(c, counter, target) {
                        return true;
                    }
                }
            }
            false
        }
        Value::Array(a) => {
            for c in a {
                if inject(c, counter, target) {
                    return true;
                }
            }
            false
        }
        _ => false,
    }
}

#[test]
fn accepting_shapes_under_a_denying_parent() {
    let mut accept: HashMap<String, bool> = HashMap::new();
    let mut parents: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
    let mut source: BTreeMap<String, String> = BTreeMap::new();
    for path in goldens() {
        let Ok(text) = std::fs::read_to_string(&path) else {
            continue;
        };
        let Ok(top) = serde_json::from_str::<Value>(&text) else {
            continue;
        };
        let ir = match top.get("ir") {
            Some(x) => x.clone(),
            None => top.clone(),
        };
        if serde_json::from_value::<cadmpeg_ir::CadIr>(ir.clone()).is_err() {
            continue;
        }
        let mut ns = Vec::new();
        nodes(&ir, "$", None, &mut ns);
        for (i, (shape, parent)) in ns.iter().enumerate() {
            if let Some(p) = parent {
                parents.entry(shape.clone()).or_default().insert(p.clone());
            }
            if accept.contains_key(shape) {
                continue;
            }
            let mut doc = ir.clone();
            let mut c = 0usize;
            if !inject(&mut doc, &mut c, i) {
                continue;
            }
            let ok = serde_json::from_value::<cadmpeg_ir::CadIr>(doc).is_ok();
            accept.insert(shape.clone(), ok);
            if ok {
                source.insert(shape.clone(), path.display().to_string());
            }
        }
    }

    let mut report = String::new();
    let total_shapes = accept.len();
    let accepting: Vec<&String> = accept
        .iter()
        .filter(|(_, ok)| **ok)
        .map(|(s, _)| s)
        .collect();
    let mut ir_holes: Vec<String> = Vec::new();
    let mut ir_free: Vec<String> = Vec::new();
    let mut native: Vec<String> = Vec::new();
    for shape in &accepting {
        let path = shape.split(' ').next().unwrap_or("");
        if path.starts_with("$.native") {
            native.push((*shape).clone());
            continue;
        }
        // A hole is an accepting node whose every observed parent rejects.
        let ps = parents.get(*shape);
        let denying_parent = ps.is_some_and(|set| {
            !set.is_empty() && set.iter().all(|p| accept.get(p).copied() == Some(false))
        });
        if denying_parent {
            ir_holes.push((*shape).clone());
        } else {
            ir_free.push((*shape).clone());
        }
    }
    ir_holes.sort();
    ir_free.sort();
    report.push_str(&format!(
        "shapes={total_shapes} accepting={} ir_holes_under_denying_parent={} ir_free={} native={}\n",
        accepting.len(),
        ir_holes.len(),
        ir_free.len(),
        native.len()
    ));
    report.push_str("\n== IR shapes that accept directly beneath a denying parent ==\n");
    for s in &ir_holes {
        report.push_str(&format!(
            "HOLE {s}\n  <- {}\n",
            source.get(s).map_or("", String::as_str)
        ));
    }
    report.push_str("\n== IR shapes with no denying parent observed ==\n");
    for s in &ir_free {
        report.push_str(&format!("FREE {s}\n"));
    }
    std::fs::write("/tmp/zz_sweep2.txt", &report).ok();
    assert!(ir_holes.is_empty(), "{report}");
}
