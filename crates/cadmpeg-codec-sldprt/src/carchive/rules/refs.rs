//! Face, edge, loop and vertex references (`g_ref.py`).

use super::super::reader::{bad, Reader, Res, V};

const MARKER: [u8; 16] = [
    0x7d, 0xc3, 0x94, 0x25, 0xad, 0x49, 0xb2, 0x54, 0x7d, 0xc3, 0x94, 0x25, 0xad, 0x49, 0xb2, 0x54,
];

/// `moFaceRef_c`, `moEdgeRef_c`, `moLoopRef_c`, `moSilhEdgeRef_c`,
/// `moSilhVertexRef_c` (`ref`).
pub(crate) fn ref_(r: &mut Reader<'_>) -> Res<V> {
    let face = r.last_class().is_some_and(|c| &*c == "moFaceRef_c");
    r.st.face = face;
    let h = r.p;
    let kind2 = r.eq(h, &[2, 0, 0, 0]);
    r.skip(if kind2 { 36 } else { 38 });
    if !r.eq(h + 20, &MARKER) {
        return Err(bad("no marker"));
    }
    let cnt = r.pu32(h + 8)?;
    if cnt > 4096 {
        return Err(bad("count"));
    }
    if kind2 {
        r.ptr(Some(&["moVertexRef_c"]))?;
    }
    for _ in 0..cnt {
        r.ptr(None)?;
    }
    if !r.zeros(r.p, 8) {
        return Err(bad("trailer zeros"));
    }
    r.skip(8);
    let nv = if r.p + 8 <= r.len() {
        Some(r.pu32(r.p)?)
    } else {
        None
    };
    if nv.is_some_and(|nv| (9000..=40000).contains(&nv) && nv % 100 == 0) && r.zeros(r.p + 4, 4) {
        r.skip(8);
        if r.st.face && r.zeros(r.p, 4) {
            r.skip(4);
        }
    } else if r.st.face && super::feat::ver_or(r, 99999) < 13000 && r.zeros(r.p, 4) {
        r.skip(4);
    }
    Ok(V::map([("cnt", V::from(cnt))]))
}

/// `moVertexRef_c`.
pub(crate) fn vertexref(r: &mut Reader<'_>) -> Res<V> {
    let h = r.p;
    r.skip(38);
    let cnt = r.pu32(h + 8)?;
    if !r.ffs(h, 8) || cnt > 4096 {
        return Err(bad("vertex ref header"));
    }
    for _ in 0..cnt {
        r.ptr(None)?;
    }
    if r.eq(r.p, &[2, 0, 0, 0]) && r.zeros(r.p + 4, 4) {
        r.skip(8);
    } else {
        r.expect_zeros(8, "vertex ref 8 zero")?;
    }
    let nv = r.pu32(r.p)?;
    if (9000..=40000).contains(&nv) && nv % 100 == 0 && r.zeros(r.p + 4, 4) {
        r.skip(8);
    }
    let k = r.u32()?;
    for _ in 0..k {
        r.f64()?;
        r.f64()?;
        r.f64()?;
    }
    Ok(V::from(cnt))
}
