//! Inspect SAB subtype-table entries and selected B-rep records.
//!
//! Run with `cargo run -p cadmpeg-codec-f3d --example dbg_corner -- <file.f3d>`.
use cadmpeg_codec_f3d::{asm_header, container, nurbs, sab};
use std::fs::File;

fn subtype_table(bytes: &[u8]) -> Vec<usize> {
    let mut table = Vec::new();
    let mut pos = 0usize;
    while pos + 4 < bytes.len() {
        if bytes[pos] == 0x0f && matches!(bytes.get(pos + 1), Some(0x0d | 0x0e)) {
            let len = *bytes.get(pos + 2).unwrap_or(&0) as usize;
            if let Some(name) = bytes.get(pos + 3..pos + 3 + len) {
                if name != b"ref" && name.iter().all(|b| (0x21..=0x7e).contains(b)) {
                    table.push(pos);
                }
            }
        }
        pos += 1;
    }
    table
}

fn subtype_name(bytes: &[u8], pos: usize) -> String {
    let len = bytes[pos + 2] as usize;
    String::from_utf8_lossy(&bytes[pos + 3..pos + 3 + len]).into_owned()
}

fn main() {
    let path = std::env::args().nth(1).expect("usage: dbg_corner <f3d>");
    let mut f = File::open(&path).expect("open");
    let scan = container::scan(&mut f).expect("scan");
    let active = container::select_active_brep(&scan)
        .expect("active brep")
        .clone();
    let bytes = container::decompress_entry(&mut f, &active.name).expect("decompress");
    let start = asm_header::record_stream_start(&bytes).expect("stream start");
    let limit = active.delta_state_offset.unwrap_or(bytes.len());
    let width = active.header.as_ref().map(|h| h.width).unwrap_or(4);
    let records = sab::frame(&bytes, start, limit, usize::from(width)).expect("frame");

    // Active slice = what brep::decode sees as `bytes` (the WHOLE decompressed buffer).
    let table = subtype_table(&bytes);
    println!("subtype table entries: {}", table.len());
    for (i, &pos) in table.iter().enumerate().take(40) {
        println!("  [{i}] @{pos} {}", subtype_name(&bytes, pos));
    }

    for want in [439usize, 748, 289, 309, 695, 742] {
        let Some(r) = records.iter().find(|r| r.index == want) else {
            println!("record #{want}: NOT FOUND");
            continue;
        };
        println!(
            "\n=== record #{} {} offset={} len={}",
            r.index, r.name, r.offset, r.len
        );
        let slice = &bytes[r.offset..r.offset + r.len];
        println!("  hex: {}", hex(&slice[..slice.len().min(120)]));
        println!("  tokens: {:?}", &r.tokens[..r.tokens.len().min(20)]);
    }

    // Edges 289/309/695/742: what curve record do they reference (slot 8)?
    for want in [289usize, 309, 695, 742] {
        let Some(r) = records.iter().find(|r| r.index == want) else {
            continue;
        };
        let cv = r.ref_at(8);
        println!("\nedge #{want} curve ref = {:?}", cv);
        if let Some(cv) = cv {
            if let Some(cr) = records.iter().find(|r| r.index as i64 == cv) {
                println!("  curve record #{} {} len={}", cr.index, cr.name, cr.len);
                let slice = &bytes[cr.offset..cr.offset + cr.len];
                println!("  strings: {:?}", strings(slice));
                println!("  hex[..160]: {}", hex(&slice[..slice.len().min(160)]));
                let dec = nurbs::decode_curve_cache_resolving_refs(slice, &bytes);
                println!("  decode_curve_cache_resolving_refs -> {}", dec.is_some());
                let pdec = nurbs::decode_procedural_curve_resolving_refs(slice, &bytes);
                println!(
                    "  decode_procedural_curve_resolving_refs -> {:?}",
                    pdec.map(|d| d.native_kind)
                );
            }
        }
    }

    // Surfaces 439/748: dump their ref index and the referenced table entry.
    for want in [439usize, 748] {
        let Some(r) = records.iter().find(|r| r.index == want) else {
            continue;
        };
        let slice = &bytes[r.offset..r.offset + r.len];
        println!("\nspline surface #{want} strings: {:?}", strings(slice));
        // ref index
        let marker: &[u8] = b"\x0f\x0d\x03ref\x04";
        if let Some(p) = slice.windows(marker.len()).position(|w| w == marker) {
            let idx = i64::from_le_bytes(
                slice[p + marker.len()..p + marker.len() + 8]
                    .try_into()
                    .unwrap(),
            );
            println!("  ref index = {idx}");
            if let Some(&tpos) = table.get(idx as usize) {
                println!("  table[{idx}] @{} = {}", tpos, subtype_name(&bytes, tpos));
                // dump the span's strings
                let span = &bytes[tpos..(tpos + 4000).min(bytes.len())];
                println!(
                    "  span strings: {:?}",
                    strings(&span[..span.len().min(2000)])
                );
            }
        }
        let dec = nurbs::decode_surface_cache_resolving_refs(slice, &bytes);
        println!("  decode_surface_cache_resolving_refs -> {}", dec.is_some());
    }
}

fn hex(b: &[u8]) -> String {
    b.iter().map(|x| format!("{x:02x}")).collect()
}

fn strings(b: &[u8]) -> Vec<String> {
    let mut out = Vec::new();
    let mut cur = Vec::new();
    for &x in b {
        if (0x20..=0x7e).contains(&x) {
            cur.push(x);
        } else {
            if cur.len() >= 3 {
                out.push(String::from_utf8_lossy(&cur).into_owned());
            }
            cur.clear();
        }
    }
    if cur.len() >= 3 {
        out.push(String::from_utf8_lossy(&cur).into_owned());
    }
    out
}
