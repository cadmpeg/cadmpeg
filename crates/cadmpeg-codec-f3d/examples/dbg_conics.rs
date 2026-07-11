//! Correlate edge-record sense bits with referenced curve kinds.
//!
//! Run with `cargo run -p cadmpeg-codec-f3d --example dbg_conics -- <file.f3d>`.
use cadmpeg_codec_f3d::{asm_header, container, sab};
use sab::Token;
use std::collections::HashMap;
use std::fs::File;

fn main() {
    let path = std::env::args().nth(1).expect("usage: dbg_conics <f3d>");
    let mut f = File::open(&path).expect("open");
    let scan = container::scan(&mut f).expect("scan");
    let active = container::select_active_brep(&scan)
        .expect("active")
        .clone();
    let bytes = container::decompress_entry(&mut f, &active.name).expect("decompress");
    let start = asm_header::record_stream_start(&bytes).expect("stream start");
    let limit = active.delta_state_offset.unwrap_or(bytes.len());
    let width = active.header.as_ref().map(|h| h.width).unwrap_or(4);
    let records = sab::frame(&bytes, start, limit, usize::from(width)).expect("frame");
    let by_index: HashMap<usize, &sab::Record> = records.iter().map(|r| (r.index, r)).collect();
    let mut hist: HashMap<(String, bool), usize> = HashMap::new();
    let mut reversed_edges: Vec<(usize, i64)> = Vec::new();
    for r in &records {
        if r.head != "edge" {
            continue;
        }
        let curve = r.ref_at(8).unwrap_or(-1);
        let head = by_index
            .get(&(curve as usize))
            .map_or("<none>".into(), |c| c.head.clone());
        let rev = matches!(r.chunk(9), Some(Token::True));
        *hist.entry((head, rev)).or_default() += 1;
        if rev {
            reversed_edges.push((r.index, curve));
        }
    }
    let mut rows: Vec<_> = hist.into_iter().collect();
    rows.sort();
    for ((head, rev), n) in rows {
        println!("{head:<20} reversed={rev} count={n}");
    }
    println!("reversed edges: {reversed_edges:?}");
    // curve reference counts: is any curve shared by multiple edges?
    let mut refs: HashMap<i64, usize> = HashMap::new();
    for r in &records {
        if r.head == "edge" {
            if let Some(c) = r.ref_at(8) {
                *refs.entry(c).or_default() += 1;
            }
        }
    }
    let shared: Vec<_> = refs.iter().filter(|(_, n)| **n > 1).collect();
    println!("curves referenced by >1 edge: {shared:?}");
}
