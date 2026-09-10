//! Round-3 verifier fixture (uncommitted): golden-wide unknown-key sweep.
use serde_json::Value;
use std::collections::{BTreeMap, HashSet};
use std::path::{Path, PathBuf};

const TAGS: &[&str] = &[
    "definition", "kind", "alignment", "space", "type", "policy", "role", "boundary_role",
    "mode", "class", "family", "form", "selector", "variant",
];

fn goldens() -> Vec<PathBuf> {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).parent().expect("crates").to_path_buf();
    let mut out = Vec::new();
    fn walk(dir: &Path, out: &mut Vec<PathBuf>) {
        let Ok(rd) = std::fs::read_dir(dir) else { return };
        for e in rd.flatten() {
            let p = e.path();
            if p.is_dir() { walk(&p, out); }
            else if p.extension().is_some_and(|x| x == "json")
                && p.to_string_lossy().contains("/tests/golden/") { out.push(p); }
        }
    }
    walk(&root, &mut out);
    out.sort();
    out
}

/// Object nodes in document order, each as (path, shape-signature).
fn nodes(v: &Value, path: &str, out: &mut Vec<(String, String)>) {
    match v {
        Value::Object(m) => {
            let mut keys: Vec<&str> = m.keys().map(String::as_str).collect();
            keys.sort_unstable();
            let tagvals: Vec<String> = TAGS
                .iter()
                .filter_map(|t| m.get(*t).and_then(Value::as_str).map(|s| format!("{t}={s}")))
                .collect();
            out.push((path.to_string(), format!("{path} |keys:{} |{}", keys.join(","), tagvals.join(","))));
            for (k, child) in m { nodes(child, &format!("{path}.{k}"), out); }
        }
        Value::Array(a) => { for c in a { nodes(c, &format!("{path}[]"), out); } }
        _ => {}
    }
}

fn inject(v: &mut Value, counter: &mut usize, target: usize) -> bool {
    match v {
        Value::Object(m) => {
            let me = *counter;
            *counter += 1;
            if me == target { m.insert("zz_bogus".into(), Value::from(1)); return true; }
            let keys: Vec<String> = m.keys().cloned().collect();
            for k in keys {
                if let Some(c) = m.get_mut(&k) { if inject(c, counter, target) { return true; } }
            }
            false
        }
        Value::Array(a) => { for c in a { if inject(c, counter, target) { return true; } } false }
        _ => false,
    }
}

#[test]
fn golden_wide_unknown_key_sweep() {
    let mut seen: HashSet<String> = HashSet::new();
    let mut accepting: BTreeMap<String, String> = BTreeMap::new();
    let (mut shapes, mut docs) = (0usize, 0usize);
    for path in goldens() {
        let Ok(text) = std::fs::read_to_string(&path) else { continue };
        let Ok(top) = serde_json::from_str::<Value>(&text) else { continue };
        let ir = match top.get("ir") { Some(x) => x.clone(), None => top.clone() };
        if serde_json::from_value::<cadmpeg_ir::CadIr>(ir.clone()).is_err() { continue; }
        docs += 1;
        let mut ns = Vec::new();
        nodes(&ir, "$", &mut ns);
        for (i, (_p, shape)) in ns.iter().enumerate() {
            if !seen.insert(shape.clone()) { continue; }
            shapes += 1;
            let mut doc = ir.clone();
            let mut c = 0usize;
            if !inject(&mut doc, &mut c, i) { continue; }
            if serde_json::from_value::<cadmpeg_ir::CadIr>(doc).is_ok() {
                accepting.insert(shape.clone(), path.display().to_string());
            }
        }
    }
    let mut report = format!("docs={docs} shapes={shapes} accepting={}\n", accepting.len());
    for (shape, file) in &accepting { report.push_str(&format!("ACCEPT {shape}   <- {file}\n")); }
    std::fs::write("/tmp/zz_verify3_sweep.txt", &report).ok();
    assert!(accepting.is_empty(), "{report}");
}
