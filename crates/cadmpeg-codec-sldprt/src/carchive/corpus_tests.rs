//! Corpus dumps compared against the research grammar
//! (`research/sldprt-mfc-archive/grammar/`). Ignored: they read a local
//! corpus named by `CARCHIVE_CORPUS_LIST` (lines `path<TAB>Contents/Config-N`)
//! and write JSON lines to `CARCHIVE_CORPUS_OUT`.

use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};
use std::io::Write;

fn corpus() -> Option<(Vec<(String, String)>, std::fs::File)> {
    let list = std::env::var("CARCHIVE_CORPUS_LIST").ok()?;
    let out = std::env::var("CARCHIVE_CORPUS_OUT").ok()?;
    let rows = std::fs::read_to_string(list)
        .expect("corpus list")
        .lines()
        .filter_map(|line| {
            let (path, cfg) = line.split_once('\t')?;
            Some((path.to_owned(), cfg.to_owned()))
        })
        .collect();
    Some((rows, std::fs::File::create(out).expect("corpus output")))
}

/// The `Config-N` and `Config-N-ResolvedFeatures` payloads of `path`.
fn streams(path: &str, cfg: &str) -> Option<(Vec<u8>, Vec<u8>)> {
    let bytes = std::fs::read(path).ok()?;
    let arena = DecodeArena::new();
    let (ctx, root) =
        DecodeContext::from_root_bytes(&bytes, &arena, &DecodePolicy::desktop()).ok()?;
    let scan = crate::container::scan(&ctx, root).ok()?;
    let want = cfg.to_ascii_lowercase();
    let want_rf = format!("{want}-resolvedfeatures");
    let (mut config, mut resolved) = (None, None);
    for section in scan.sections() {
        let Some(name) = section.name() else { continue };
        let name = name.replace('\\', "/").to_ascii_lowercase();
        if name == want {
            config = Some(section.payload().to_vec());
        } else if name == want_rf {
            resolved = Some(section.payload().to_vec());
        }
    }
    Some((config?, resolved?))
}

#[test]
#[ignore = "reads a local corpus"]
fn dump_config_class_maps() {
    let Some((rows, mut out)) = corpus() else {
        return;
    };
    for (path, cfg) in rows {
        let row = match streams(&path, &cfg) {
            Some((config, resolved)) => {
                let count = crate::archive::resolved_features_count(&resolved);
                let map = count.and_then(|count| {
                    crate::archive::class_index_map(&config, count)
                        .ok()
                        .flatten()
                });
                serde_json::json!({"f": path, "cfg": cfg, "u": count, "map": map})
            }
            None => serde_json::json!({"f": path, "cfg": cfg, "u": null, "map": null}),
        };
        writeln!(out, "{row}").expect("write");
    }
}

/// One walk per line of `CARCHIVE_CORPUS_LIST` (`path<TAB>cfg<TAB>ver`): the
/// status, the end position and the trace, as the research grammar's
/// `pytrace.py` writes them.
#[test]
#[ignore = "reads a local corpus"]
fn dump_walks() {
    let Some((rows, mut out)) = corpus() else {
        return;
    };
    for (path, rest) in rows {
        let (cfg, ver) = rest.split_once('\t').unwrap_or((rest.as_str(), ""));
        let ver = ver.trim().parse::<u32>().ok();
        let row = (|| {
            let (config, resolved) = streams(&path, cfg)?;
            let walk = super::walk_configuration(&config, &resolved, ver).ok()??;
            let (st, why) = match &walk.status {
                super::Status::End => ("END", String::new()),
                super::Status::Stop(stop) => ("STOP", format!("{stop:?}")),
            };
            let trace = walk
                .trace
                .iter()
                .map(|t| serde_json::json!([&*t.class, t.at]))
                .collect::<Vec<_>>();
            let params = walk
                .parameters
                .iter()
                .map(|p| serde_json::json!([&*p.class, format!("{:?}", p.name), p.value]))
                .collect::<Vec<_>>();
            let named = walk
                .named
                .iter()
                .map(|n| {
                    let name = n
                        .name
                        .as_str()
                        .map_or_else(|| format!("{:?}", n.name), str::to_owned);
                    serde_json::json!([n.class.as_deref(), name, n.oid])
                })
                .collect::<Vec<_>>();
            Some(serde_json::json!({
                "f": path, "st": st, "p": walk.p, "why": why, "tops": walk.tops,
                "trace": trace, "params": params, "named": named,
            }))
        })()
        .unwrap_or_else(|| serde_json::json!({"f": path, "st": "ERR"}));
        writeln!(out, "{row}").expect("write");
    }
}
