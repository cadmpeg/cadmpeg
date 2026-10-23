//! Sketch rules (`g_sketch.py`): the inline entity record, handle objects,
//! `sgSketch`, its tail and the objects a sketch refers to.

use std::sync::Arc;

use super::super::reader::{bad, Ptr, Reader, Res, V};
use super::feature::{isclass, isver, objat, refblock, ver_or, M12345, UCS};

/// `g_sketch.py`'s rule state: the Python's module flags.
#[allow(clippy::struct_excessive_bools)]
#[derive(Debug, Clone, Default)]
pub(crate) struct SketchState {
    /// `MIRROR[0]`.
    pub(crate) mirror: bool,
    /// `BYREF[0]`.
    pub(crate) byref: bool,
    /// `DIMLAST[0]`.
    pub(crate) dimlast: bool,
    /// `PGMDONE[0] is not None`: an offset chain read the offset profile.
    pub(crate) pgmdone: bool,
}

/// `isattr(b, p)`: the 17-byte attribute record.
fn isattr(r: &Reader<'_>, p: usize) -> bool {
    r.eq(p, &[0xff, 0xff]) && r.ffs(p + 5, 4) && r.eq(p + 13, &[0x00, 0x00, 0x80, 0xbf])
}

/// `w == 0xffff or w & 0x8000`.
fn tagword(w: u16) -> bool {
    w == 0xffff || w & 0x8000 != 0
}

/// Three i32 -12345.
fn m3() -> [u8; 12] {
    M12345
}

/// `bytes(n) + tail`.
fn zeros_then(n: usize, tail: &[u8]) -> Vec<u8> {
    let mut x = vec![0; n];
    x.extend_from_slice(tail);
    x
}

/// `r.b[p:p+z] == bytes(z)`, true for an empty slice as in Python.
fn zeros0(r: &Reader<'_>, p: usize, z: usize) -> bool {
    z == 0 || r.zeros(p, z)
}

fn f64s(r: &mut Reader<'_>, n: usize) -> Res<V> {
    let mut out = Vec::with_capacity(n);
    for _ in 0..n {
        out.push(V::from(r.f64()?));
    }
    Ok(V::List(out))
}

/// A handle object whose rule returned slot `0x777f` (a placeholder).
fn is_placeholder(x: &Ptr) -> bool {
    x.is_obj() && x.value() == Some(&V::Int(0x777f))
}

/// `sgPointHandle`, `sgLineHandle`, ... (`handle`).
pub(crate) fn handle(r: &mut Reader<'_>) -> Res<V> {
    let slot = r.u16()?;
    let m1 = r.u32()?;
    let z = r.u32()?;
    if m1 != 0xffff_ffff && m1 != 0xffff_fffe && !(m1 < 4096 && z == 0) {
        return Err(bad("handle -1"));
    }
    Ok(V::from(slot))
}

/// `rhead(r)`: the 64 bytes every sketch record starts with.
fn rhead(r: &mut Reader<'_>) -> Res<V> {
    if !isattr(r, r.p) {
        return Err(bad("no attribute record"));
    }
    r.skip(17);
    let kind = r.u32()?;
    r.expect_bytes(&[0, 0], "")?;
    let la = r.u16()?;
    let lb = r.u16()?;
    let role = r.u16()?;
    let state = r.u16()?;
    if !r.eq(r.p, &[0x00, 0x00, 0x80, 0xbf]) {
        return Err(bad("record f32 -1"));
    }
    r.skip(4);
    let ext = r.ptr(None)?;
    let sel = r.u16()?;
    r.skip(9);
    r.f64()?;
    let w = r.pu16(r.p)?;
    if r.eq(r.p, &[0, 0]) || w >= 0x100 {
        // Before 11000 the next word is the tag (KiCad exports).
        r.expect_zeros(8, "record zeros 2")?;
    }
    Ok(V::map([
        ("kind", V::from(kind)),
        ("la", V::from(la)),
        ("lb", V::from(lb)),
        ("role", V::from(role)),
        ("state", V::from(state)),
        ("ext", V::from(ext)),
        ("sel", V::from(sel)),
    ]))
}

/// `line(r)`: a curve record.
fn line(r: &mut Reader<'_>) -> Res<V> {
    let mut v = rhead(r)?;
    let i0 = r.u16()?;
    let i1 = r.u16()?;
    let a = r.u32()?;
    v.set("i0", V::from(i0));
    v.set("i1", V::from(i1));
    v.set("a", V::from(a));
    if i1 == 0xffff
        && a == 0xffff_fffe
        && r.zeros(r.p, 4)
        && r.eq(r.p + 4, &[0, 0, 0, 0, 0, 0, 0xf0, 0xbf])
    {
        r.skip(4);
    }
    let m1 = r.f64()?;
    let fe4 = [
        0xfe, 0xff, 0xff, 0xff, 0xfe, 0xff, 0xff, 0xff, 0xfe, 0xff, 0xff, 0xff, 0xfe, 0xff, 0xff,
        0xff,
    ];
    if m1 != -1.0 && !(r.eq(r.p, &[1, 0]) && r.eq(r.p + 6, &fe4)) {
        return Err(bad("curve f64 -1"));
    }
    if isclass(r, r.p, &["moDimText_c"])? {
        v.set("b", V::from("text"));
        let text = r.ptr(None)?;
        v.set("text", V::from(text));
        return Ok(v);
    }
    let b = r.u16()?;
    v.set("b", V::from(b));
    let fe = [0xfe, 0xff, 0xff, 0xff];
    if b == 1 && !r.eq(r.p + 4, &fe) && r.eq(r.p, &[0, 0]) {
        r.u16()?;
        let centre = r.u16()?;
        let mut axes = Vec::new();
        for _ in 0..4 {
            axes.push(V::from(r.u16()?));
        }
        v.set("ell", V::List(vec![V::from(centre), V::List(axes)]));
        let q = r.p + 2;
        if r.eq(r.p, &[0, 0]) && r.eq(q, &[0, 0]) && r.eq(q + 4, &[0, 0, 1, 0]) && isattr(r, q + 16)
        {
            r.skip(2);
            v.set("c", V::Int(0));
            v.set("d", V::Int(0));
            v.set("id", V::Int(0));
            return Ok(v);
        }
    } else if b == 0xffff && r.eq(r.p, &[0xff, 0xff]) && !r.eq(r.p + 4, &fe) {
        r.u16()?;
        let centre = r.u16()?;
        let mut axes = Vec::new();
        for _ in 0..4 {
            axes.push(V::from(r.u16()?));
        }
        v.set("ell", V::List(vec![V::from(centre), V::List(axes)]));
        v.set("pell", V::Bool(true));
    } else if b == 1 || b == 0xffff {
        let x = r.u32()?;
        let mut ys = Vec::new();
        for _ in 0..4 {
            ys.push(V::from(r.u32()?));
        }
        v.set("arc", V::List(vec![V::from(x), V::List(ys)]));
    } else if b == 2 {
        r.expect_bytes(&[0, 0], "spline u16 0")?;
        let order = r.u16()?;
        r.ptr(Some(&["cptsSplineList_c", "modifSplineList_c"]))?;
        if !r.eq(r.p, &[1, 0, 0, 0, 0, 0, 0, 0])
            && !r.zeros(r.p, 8)
            && !r.eq(r.p, &[1, 0, 1, 0, 0, 0, 0, 0])
        {
            // u32 1 (or 0) and u32 0; a u16 1 and u32 1 in one part.
            return Err(bad("spline 1 0"));
        }
        r.skip(8);
        r.f64()?;
        r.f64()?;
        r.f64()?;
        r.u32()?;
        r.expect_zeros(48, "spline 48 zero")?;
        r.expect_bytes(&[1, 0], "spline u16 1")?;
        let nk = r.u32()?;
        if nk > 4096 {
            return Err(bad("spline knots"));
        }
        for _ in 0..nk {
            r.f64()?;
        }
        let nc = r.u32()?;
        if nc > 4096 {
            return Err(bad("spline points"));
        }
        for _ in 0..nc {
            r.f64()?;
            r.f64()?;
            r.f64()?;
        }
        if r.zeros(r.p, 4) && r.pu32(r.p + 4)? == 0 && r.pu32(r.p + 8)? == 2 {
            // A spline whose geometry may be a compressed Parasolid body: u32 0, u32 0, u32 2, the parameter
            // range, u32 0, u32 1. When a body follows, a zero run (24 or 56 bytes), u16 1 and a blob
            // (00 A 00 B, B = A - 5) stand before it; when none does, the record's tail follows the two u32.
            r.skip(12);
            r.f64()?;
            r.f64()?;
            r.expect_zeros(4, "spline body u32 0")?;
            r.u32()?;
            let q = r.p;
            let mut j = r.p;
            while j < r.p + 64 && j < r.len() && r.byte(j)? == 0 {
                j += 1;
            }
            let mut hasbody = false;
            if j + 12 <= r.len() && r.eq(j, &[1, 0]) && r.eq(j + 2, &[0]) {
                let a = r.pu32(j + 3)?;
                let b = r.pu32(j + 8)?;
                hasbody = r.eq(j + 7, &[0]) && a.checked_sub(5) == Some(b);
            }
            if hasbody {
                r.skip(j - r.p);
                r.u16()?;
                let data = blob(r)?;
                let len = match &data {
                    V::Bytes(bytes) => bytes.len(),
                    _ => 0,
                };
                v.set(
                    "spline",
                    V::List(vec![V::from("body"), V::from(len as i64)]),
                );
            } else {
                r.p = q;
                v.set("spline", V::List(vec![V::from("bodyless")]));
            }
            let c = r.u16()?;
            let d = r.u32()?;
            let id = r.u32()?;
            v.set("c", V::from(c));
            v.set("d", V::from(d));
            v.set("id", V::from(id));
            return Ok(v);
        }
        r.expect_bytes(
            &[0, 0, 0, 0, 0xff, 0xff, 0xff, 0xff, 1, 0, 0, 0, 1, 0, 0, 0],
            "spline 0 -1 1 1",
        )?;
        if r.u32()? != nc {
            return Err(bad("spline weight count"));
        }
        r.u32()?;
        r.f64()?;
        r.f64()?;
        for _ in 0..nc {
            r.f64()?;
        }
        r.expect_zeros(8, "spline 8 zero")?;
        let sf = r.u16()?;
        r.f64()?;
        if sf != 0 {
            r.f64()?;
        }
        r.expect_zeros(12, "spline 12 zero")?;
        v.set(
            "spline",
            V::List(vec![V::from(order), V::from(nk), V::from(nc)]),
        );
        let c = r.u16()?;
        let d = r.u32()?;
        let id = r.u32()?;
        v.set("c", V::from(c));
        v.set("d", V::from(d));
        v.set("id", V::from(id));
        return Ok(v);
    } else if b == 0x40 {
        r.expect_bytes(&[0, 0], "style spline u16 0")?;
        r.u16()?;
        let cps = r.ptr(Some(&["cptsSplineList_c"]))?;
        r.expect_zeros(8, "style spline 8 zero")?;
        let x = r.f64()?;
        let y = r.f64()?;
        if (x, y) != (-1000.0, -1000.0) {
            return Err(bad("style spline -1000"));
        }
        let mut fixed = vec![0; 60];
        fixed.extend_from_slice(&[1, 0]);
        fixed.extend_from_slice(&[0; 12]);
        fixed.extend_from_slice(&[0xff; 4]);
        fixed.extend_from_slice(&[0; 4]);
        fixed.extend_from_slice(&[1, 0, 0, 0]);
        fixed.extend_from_slice(&[0; 6]);
        r.expect_bytes(&fixed, "style spline fixed")?;
        v.set("style", V::from(cps));
    } else if b != 0 {
        return Err(bad(format!("curve type {b}")));
    }
    let c = r.u16()?;
    let d = r.u32()?;
    let id = r.u32()?;
    v.set("c", V::from(c));
    v.set("d", V::from(d));
    v.set("id", V::from(id));
    Ok(v)
}

/// `record(r)`: a sketch entity record.
fn record(r: &mut Reader<'_>) -> Res<V> {
    if !isattr(r, r.p) {
        return Err(bad("no attribute record"));
    }
    r.skip(17);
    let kind = r.u32()?;
    r.expect_bytes(&[0, 0], "")?;
    r.u16()?;
    r.u16()?;
    r.u16()?;
    r.u16()?;
    if !r.eq(r.p, &[0x00, 0x00, 0x80, 0xbf]) {
        return Err(bad("record f32 -1"));
    }
    r.skip(4);
    r.ptr(None)?;
    r.u16()?;
    r.skip(9);
    r.f64()?;
    let w = r.pu16(r.p)?;
    let tag = if !r.eq(r.p, &[0, 0]) && w < 0x100 {
        // Before 11000 the tag word is here (KiCad exports).
        r.skip(2);
        w
    } else {
        r.expect_zeros(8, "record zeros 2")?;
        r.u16()?
    };
    r.f64()?;
    r.f64()?;
    if tag & 0x10 == 0 {
        r.f64()?;
    }
    let a = r.u16()?;
    let n = r.u16()?;
    for _ in 0..3 {
        r.ptr(None)?;
    }
    let m2 = r.u32()?;
    if m2 != 0xffff_fffe && m2 > 0xffff {
        return Err(bad("record -2"));
    }
    r.skip(42);
    let id = r.u32()?;
    if r.zeros(r.p, 12) && (1..16).contains(&r.pu32(r.p + 12)?) && isattr(r, r.p + 16) {
        r.skip(16);
    }
    Ok(V::map([
        ("kind", V::from(kind)),
        ("tag", V::from(tag)),
        ("a", V::from(a)),
        ("n", V::from(n)),
        ("id", V::from(id)),
    ]))
}

/// Whether a curve record's value has an `ell` entry.
fn has_ell(v: &V) -> bool {
    matches!(v, V::Map(map) if map.contains_key("ell"))
}

/// `okat(q)` (inside `sketch`).
fn okat(r: &Reader<'_>, q: usize) -> Res<bool> {
    let nd = r.pu16(q)?;
    if nd != 0 {
        let w = r.pu16(q + 2)?;
        return Ok(tagword(w));
    }
    Ok(r.zeros(q + 2, 4))
}

/// `isdim(q)` (inside `sketch`).
fn isdim(r: &Reader<'_>, q: usize) -> Res<bool> {
    let w = r.pu16(q)?;
    let nm: String = if w == 0xffff {
        let l = usize::from(r.pu16(q + 4)?);
        r.slice(q + 6, l).iter().map(|&b| char::from(b)).collect()
    } else if w & 0x8000 != 0 {
        r.known
            .get(&u32::from(w & 0x7fff))
            .map_or_else(String::new, ToString::to_string)
    } else {
        return Ok(false);
    };
    Ok(nm.starts_with("sg") && (nm.ends_with("Dist") || nm.ends_with("Dim")))
}

/// `ishandle()` (inside `sketch`).
fn ishandle(r: &Reader<'_>) -> Res<bool> {
    let w = r.pu16(r.p)?;
    if w == 0xffff {
        let l = usize::from(r.pu16(r.p + 4)?);
        return Ok(r.slice(r.p + 6, l).ends_with(b"Handle"));
    }
    Ok(w & 0x8000 != 0
        && r.known
            .get(&u32::from(w & 0x7fff))
            .is_some_and(|c| c.ends_with("Handle")))
}

/// A relation entry of `sketch`, by kind.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Rel {
    Core,
    Obj,
    Polygon,
    Pattern,
}

/// `core(gi)` (inside `sketch`): one relation.
fn core(r: &mut Reader<'_>, gi: usize) -> Res<(usize, Rel)> {
    r.expect_zeros(8, &format!("relation zeros (group {gi})"))?;
    if r.u32()? != 0xffff_ffff {
        return Err(bad("relation -1"));
    }
    let ver = ver_or(r, 0);
    let l = if ver >= 18000 { 17 } else { 16 };
    if !r.zeros(r.p, l - 2) || !(r.eq(r.p + l - 2, &[0, 0]) || r.eq(r.p + l - 2, &[1, 0])) {
        return Err(bad("relation 16 zero"));
    }
    r.skip(l);
    r.u32()?;
    r.u32()?;
    r.u16()?;
    r.u32()?;
    let mut ops = 0;
    while ishandle(r)? {
        r.ptr(None)?;
        ops += 1;
    }
    if r.zeros(r.p, 4) && isclass(r, r.p + 4, &["suObArray"])? {
        // Helper data after the operands (a spline's dimension): u32 0 and a
        // suObArray (a sgDimEntityHelpData_c and a null).
        r.skip(4);
        r.ptr(None)?;
    }
    if r.zeros(r.p, 6) && r.ffs(r.p + 8, 8) && r.zeros(r.p + 16, 24) {
        r.skip(40);
        if ver >= 19000 && r.ffs(r.p, 4) {
            r.skip(4);
        }
    } else if ops == 3 {
        r.expect_zeros(6, "relation 3-operand 6 zero")?;
    }
    Ok((gi, Rel::Core))
}

/// `h()` (inside `polygon`).
fn polygon_h(r: &mut Reader<'_>) -> Res<Ptr> {
    let x = r.ptr(None)?;
    if is_placeholder(&x) {
        r.expect_zeros(4, "polygon placeholder u32 0")?;
    }
    Ok(x)
}

/// `polygon(gi)` (inside `sketch`).
fn polygon(r: &mut Reader<'_>, gi: usize) -> Res<(usize, Rel)> {
    r.u32()?;
    r.u32()?;
    r.skip(10);
    r.u16()?;
    r.u16()?;
    for _ in 0..3 {
        r.f64()?;
    }
    polygon_h(r)?;
    polygon_h(r)?;
    r.u16()?;
    polygon_h(r)?;
    r.u32()?;
    r.ptr(None)?;
    r.expect_bytes(&[1, 0], "polygon u16 1")?;
    r.expect_zeros(4, "polygon u32 0")?;
    let w = r.pu16(r.p)?;
    if tagword(w) {
        let d = r.ptr(None)?;
        if !d.obj_class().is_some_and(|c| c.ends_with("PattCntDim")) {
            return Err(bad("pattern: not a count dimension"));
        }
        return Ok((gi, Rel::Pattern));
    }
    r.expect_bytes(&[0, 0], "polygon u16 0")?;
    Ok((gi, Rel::Polygon))
}

/// `linpatt(gi)` (inside `sketch`).
fn linpatt(r: &mut Reader<'_>, gi: usize) -> Res<(usize, Rel)> {
    r.u32()?;
    r.u32()?;
    r.skip(18);
    for _ in 0..3 {
        r.f64()?;
    }
    for _ in 0..2 {
        let h = r.ptr(None)?;
        if is_placeholder(&h) {
            r.expect_zeros(4, "linear pattern u32 0")?;
        }
    }
    r.u16()?;
    if (r.eq(r.p, &[1, 0, 0, 0]) || r.eq(r.p, &[1, 0, 1, 0])) && objat(r, r.p + 4) {
        r.skip(4);
        for di in 0..2 {
            r.ptr(None)?;
            let mut z = 0;
            while r.byte(r.p + z)? == 0 && z < 120 {
                z += 1;
            }
            let window = r.slice(r.p + z, 60);
            if window.windows(4).any(|w| w == [0xc7, 0xcf, 0xff, 0xff]) {
                refblock(r)?;
            }
            if r.zeros(r.p, 8) && !r.zeros(r.p + 8, 8) {
                r.skip(8);
            } else {
                r.expect_zeros(4, "pattern direction u32 0")?;
            }
            for _ in 0..7 {
                r.f64()?;
            }
            if di == 0
                && r.byte(r.p)? == 0
                && isclass(
                    r,
                    r.p + 1,
                    &["moLineRef_w", "moSketchExtRef_w", "moEdgeRef_w"],
                )?
            {
                r.skip(1);
                continue;
            }
            break;
        }
        let zz = [7_usize, 3, 1]
            .into_iter()
            .find(|&z| r.zeros(r.p, z) && objat(r, r.p + z));
        if let Some(zz) = zz {
            r.skip(zz);
        } else {
            r.expect_zeros(7, "pattern direction 7 zero")?;
            // The Python then calls `r.skip(None)`: a TypeError.
            return Err(bad("pattern direction: skip(None)"));
        }
    } else {
        r.expect_zeros(8, "linear pattern 8 zero")?;
    }
    let x = r.ptr(None)?;
    if !(x.is_null() || x.obj_class().is_some_and(|c| c.ends_with("PattCntDim"))) {
        return Err(bad("linear pattern: not a count dimension"));
    }
    r.f64()?;
    r.expect_bytes(&[1, 0], "linear pattern u16 1")?;
    Ok((gi, Rel::Pattern))
}

/// `islinpatt(q)` (inside `sketch`).
fn islinpatt(r: &Reader<'_>, q: usize) -> Res<bool> {
    if r.zeros(q, 8) {
        return Ok(false);
    }
    let n1 = r.pu16(q + 14)?;
    let n2 = r.pu16(q + 16)?;
    let a = r.pu16(q + 18)?;
    let c = r.pu16(q + 20)?;
    if (n1, n2) != (a, c) || n1 == 0 {
        return Ok(false);
    }
    let d0 = r.pf64(q + 26)?;
    let d1 = r.pf64(q + 34)?;
    let d2 = r.pf64(q + 42)?;
    let s = d0 * d0 + d1 * d1 + d2 * d2;
    if (s - 1.0).abs() > 1e-6 {
        return Ok(false);
    }
    let w = r.pu16(q + 50)?;
    Ok(tagword(w))
}

/// `ispoly(q)` (inside `sketch`).
fn ispoly(r: &Reader<'_>, q: usize) -> Res<bool> {
    if r.zeros(q, 8) {
        return Ok(false);
    }
    let mut n = r.pu32(q + 4)?;
    if r.ffs(q, 4) && n == 1 {
        n = u32::from(r.pu16(q + 18)?);
    }
    // The u16 pair repeats a count, not always n (a circular pattern's: 3
    // with 8 instances, 15 with 30).
    let a = r.pu16(q + 18)?;
    let b = r.pu16(q + 20)?;
    if !(0 < n && n < 1000) || a != b || !(0 < a && a < 1000) {
        return Ok(false);
    }
    let w = r.pu16(q + 46)?;
    Ok(tagword(w))
}

/// `sgSketch` (`sketch`).
pub(crate) fn sketch(r: &mut Reader<'_>) -> Res<V> {
    r.st.sk.pgmdone = false;
    let big_n = r.u32()?;
    r.expect_bytes(&[0, 0], "")?;
    r.u16()?;
    r.expect_bytes(&[0, 0], "")?;
    let big_k = r.u32()?;
    for _ in 0..big_n {
        record(r)?;
    }
    r.u32()?;
    let mut lines: Vec<V> = Vec::new();
    loop {
        if isattr(r, r.p) {
            lines.push(line(r)?);
            continue;
        }
        if r.eq(r.p, &[0, 0])
            && r.eq(r.p + 4, &[0, 0])
            && r.byte(r.p + 7)? == 0
            && isattr(r, r.p + 16)
        {
            r.pu16(r.p + 2)?;
            r.skip(4);
            continue;
        }
        if r.eq(r.p, &[0, 0])
            && matches!(r.byte(r.p + 2)?, 0..=2)
            && r.byte(r.p + 3)? == 0
            && isattr(r, r.p + 12)
        {
            r.pu32(r.p + 4)?;
            r.pu32(r.p + 8)?;
            r.skip(12);
            continue;
        }
        if r.eq(r.p, &[0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 1, 0])
            && r.eq(r.p + 12, &[0, 0])
            && r.eq(r.p + 20, &[1, 0, 0, 0])
            && isattr(r, r.p + 24)
        {
            r.pu32(r.p + 16)?;
            r.skip(24);
            continue;
        }
        if r.eq(r.p, &[0, 0]) && isattr(r, r.p + 2) {
            r.skip(2);
            continue;
        }
        let u = r.pu32(r.p)?;
        if 0 < u && u < 0x10000 && isattr(r, r.p + 4) {
            r.skip(4);
            continue;
        }
        break;
    }
    let lastarc = matches!(lines.last().map(|v| v.get("b")), Some(V::Int(1 | 0xffff)));
    let lastell = lines.last().is_some_and(has_ell);
    let order: &[usize] = if lastell {
        &[20, 24, 22]
    } else if lastarc {
        &[22, 24, 36, 34]
    } else {
        &[36, 34, 22, 24]
    };
    let mut found = None;
    for &z in order {
        if r.zeros(r.p, z) && okat(r, r.p + z)? {
            found = Some(z);
            break;
        }
    }
    let Some(z) = found else {
        return Err(bad("sketch list trailer"));
    };
    r.skip(z);
    let mut rels: Vec<(usize, Rel)> = Vec::new();
    let mut ndims = 0_usize;
    for gi in 0..3 {
        let k = r.u16()?;
        for i in 0..k {
            if !isdim(r, r.p)? {
                if r.zeros(r.p, 8) && r.ffs(r.p + 8, 4) {
                    core(r, gi)?;
                    ndims += 1;
                    continue;
                }
                return Err(bad("dimension group: not a dimension"));
            }
            r.st.sk.dimlast = i + 1 == k;
            let res = r.ptr(None);
            r.st.sk.dimlast = false;
            res?;
            ndims += 1;
        }
    }
    for gi in 0..8 {
        let w = r.pu16(r.p)?;
        if gi >= 1 && tagword(w) {
            break;
        }
        let k = r.u16()?;
        for _ in 0..k {
            let w0 = r.pu16(r.p)?;
            if r.ffs(r.p, 4) && ispoly(r, r.p)? {
                rels.push(polygon(r, gi)?);
                continue;
            }
            if r.ffs(r.p, 4) && islinpatt(r, r.p)? {
                rels.push(linpatt(r, gi)?);
                continue;
            }
            if tagword(w0) {
                r.ptr(None)?;
                rels.push((gi, Rel::Obj));
                continue;
            }
            if ispoly(r, r.p)? {
                rels.push(polygon(r, gi)?);
                continue;
            }
            if islinpatt(r, r.p)? {
                rels.push(linpatt(r, gi)?);
                continue;
            }
            rels.push(core(r, gi)?);
        }
        if rels
            .iter()
            .any(|x| x.0 == gi && matches!(x.1, Rel::Polygon | Rel::Pattern))
        {
            let m = r.u16()?;
            if m > 4096 {
                return Err(bad("polygon relation count"));
            }
            for _ in 0..m {
                rels.push(core(r, gi)?);
                r.skip(20);
            }
        }
    }
    while r.eq(r.p, &[0, 0]) {
        r.skip(2);
    }
    r.ptr(None)?;
    let q = r.p + 10;
    if r.zeros(r.p, 8) && r.eq(r.p + 8, &[1, 0]) && q + 8 <= r.len() {
        let c = r.pu32(q)? as usize;
        if 0 < c && c < 256 && q + 4 * c + 8 <= r.len() {
            let mut ok = true;
            for i in 0..c {
                let x = r.pu32(q + 4 + 4 * i)?;
                if !(0 < x && x < 1_000_000) {
                    ok = false;
                }
            }
            let a = r.pu32(q + 4 + 4 * c)? as usize;
            if ok && (a == 0 || a == 1 || a == c) {
                r.skip(2);
            }
        }
    }
    if r.p + 12 <= r.len() && r.zeros(r.p, 4) && r.pu16(r.p + 4)? == 0 {
        let n0 = r.pu16(r.p + 6)?;
        let c = r.pu32(r.p + 8)?;
        if 0 < n0 && n0 <= 256 && c > 0xffff {
            // The same 52-byte tail before a group header: n0 is the shifted n, and the group data's class
            // tag read as a chain count is not a count (2025 Margie).
            r.skip(2);
        }
    }
    r.expect_zeros(4, "settings u32 0")?;
    let n = r.u16()?;
    let mut k = u32::from(r.u16()?);
    if n > 256 || k > 4096 {
        return Err(bad("in-context block counts"));
    }
    for g in 0..n {
        k = ingroup(r, k)?;
        if g + 1 == n && k != 0 && chains(r)? != 0 {
            return Err(bad("group count"));
        }
    }
    if n == 0 && k != 0 && chains(r)? != 0 {
        return Err(bad("chain list end"));
    }
    if r.st.sk.pgmdone {
        r.st.sk.pgmdone = false;
    } else {
        if r.eq(r.p, &[0, 0, 1, 0, 0, 0]) && objat(r, r.p + 18) {
            r.skip(2);
            for _ in 0..4 {
                r.u32()?;
            }
            r.ptr(None)?;
        }
        r.expect_zeros(4, "settings u32 0 b")?;
        let w = r.pu16(r.p + 4)?;
        if r.eq(r.p, &[0, 0, 2, 0]) && tagword(w) {
            r.skip(4);
            r.ptr(Some(&["sgPGMProfileData_c"]))?;
        }
    }
    let ver = ver_or(r, 0);
    let mut l = if ver >= 19000 { 78 } else { 76 };
    if !r.eq(r.p + l, &[0xff, 0xfe, 0xff]) && r.eq(r.p + 76, &[0xff, 0xfe, 0xff]) {
        l = 76;
    }
    r.skip(l);
    r.cstring()?;
    r.skip(42);
    r.u32()?;
    r.u32()?;
    let k = r.u32()?;
    if k > 4096 {
        return Err(bad("settings list count"));
    }
    for _ in 0..k {
        r.ptr(None)?;
    }
    r.expect_zeros(24, "settings 24 zero")?;
    r.ptr(None)?;
    let t = sktail(r)?;
    Ok(V::map([
        ("N", V::from(big_n)),
        ("K", V::from(big_k)),
        ("lines", V::Int(lines.len() as i64)),
        ("dims", V::Int(ndims as i64)),
        ("rels", V::Int(rels.len() as i64)),
        ("plane", t.get("plane").clone()),
    ]))
}

/// `sktail(r)`: the end of a sketch, to its plane.
pub(crate) fn sktail(r: &mut Reader<'_>) -> Res<V> {
    let q = r.p + 18;
    if r.zeros(r.p, 18) && q + 14 <= r.len() {
        let nxt = r.pu32(q)?;
        let cnt = r.pu32(q + 4)?;
        if 0 < cnt && cnt <= nxt && nxt < 4096 && objat(r, q + 12) {
            r.p = q + 8;
            for _ in 0..cnt {
                r.u32()?;
                r.ptr(Some(&["sgSlot_c"]))?;
            }
        }
    }
    let mut z = 0;
    while r.p + z < r.len() && r.byte(r.p + z)? == 0 && z < 80 {
        z += 1;
    }
    if r.p + z + 8 < r.len() {
        let a2 = r.pu32(r.p + z)?;
        let n2 = r.pu32(r.p + z + 4)?;
        if 0 < a2 && a2 < 64 && n2 > a2 && isclass(r, r.p + z + 8, &["sgBlock"])? {
            r.skip(z + 4);
            for _ in 0..a2 {
                r.u32()?;
                r.ptr(Some(&["sgBlock"]))?;
            }
        } else if a2 == 1
            && n2 == 0
            && r.zeros(r.p + z + 8, 4)
            && (r.eq(r.p + z + 12, &[3, 0, 0, 0]) || r.eq(r.p + z + 12, &[7, 0, 0, 0]))
        {
            r.skip(z + 8);
        } else if 0 < n2 && n2 <= a2 && a2 < 4096 && objat(r, r.p + z + 8) {
            r.skip(z + 8);
            for _ in 0..n2 {
                r.ptr(None)?;
            }
        }
    }
    loop {
        let marker = r.eq(r.p, &[3, 0, 0, 0]) || r.eq(r.p, &[7, 0, 0, 0]);
        if marker && r.byte(r.p + 4)? <= 1 {
            break;
        }
        if r.byte(r.p)? != 0 {
            return Err(bad("sketch tail"));
        }
        r.p += 1;
    }
    let mk = r.u32()?;
    if r.byte(r.p)? == 0 {
        blob(r)?;
    } else {
        r.skip(1);
    }
    r.expect_zeros(4, "sketch tail u32 0")?;
    let c = r.u16()?;
    for _ in 0..c {
        r.ptr(None)?;
    }
    if c != 0 {
        let a = r.u32()?;
        let b = r.u32()?;
        let d = r.u32()?;
        if [a, b, d] != [4, 16, 1] {
            return Err(bad("sketch tail 4 16 1"));
        }
        let n = r.u32()?;
        for _ in 0..n {
            r.u32()?;
        }
    }
    // `(mk == 3 or not c) and not (c and 100000 <= u32 <= 100064)`.
    if mk == 3 || c == 0 {
        let marked = c != 0 && (100_000..=100_064).contains(&r.pu32(r.p)?);
        if !marked {
            r.expect_zeros(4, "sketch tail u32 0 b")?;
        }
    }
    let mv = r.u32()?;
    if !(100_000..=100_064).contains(&mv) {
        return Err(bad("sketch tail 100000"));
    }
    if mv > 100_000 {
        let g = r.u16()?;
        let mut ids = Vec::new();
        if u32::from(g) != mv - 100_000 {
            return Err(bad("sketch tail group count"));
        }
        if g > 64 {
            return Err(bad("sketch tail groups"));
        }
        for _ in 0..g {
            r.ptr(Some(&["moOneContour_c"]))?;
            r.st.core.skbox = true;
            let res = (|| -> Res<()> {
                let n = r.u16()?;
                for _ in 0..n {
                    r.ptr(None)?;
                }
                Ok(())
            })();
            r.st.core.skbox = false;
            res?;
            r.u16()?;
            ids.push(r.u32()?);
        }
        ids.sort_unstable();
        if ids != (100_000..100_000 + u32::from(g)).collect::<Vec<_>>() {
            return Err(bad(format!("sketch tail group ids {ids:?}")));
        }
        r.ptr(None)?;
        let plane = r.ptr(None)?;
        return Ok(V::map([("plane", V::from(plane))]));
    }
    r.expect_bytes(&[0, 0], "sketch tail u16 0")?;
    r.ptr(None)?;
    let plane = r.ptr(None)?;
    Ok(V::map([("plane", V::from(plane))]))
}

/// `sgExtEnt_c` (`extent`).
pub(crate) fn extent(r: &mut Reader<'_>) -> Res<V> {
    Ok(V::from(r.ptr(None)?))
}

/// `ingroup(r, k)`: one in-context group; returns the next group's size.
fn ingroup(r: &mut Reader<'_>, k: u32) -> Res<u32> {
    if k == 0 {
        r.expect_zeros(10, "empty group 10 zero")?;
        r.u32()?;
        r.expect_bytes(&[0, 0, 1, 0, 1, 0], "empty group 0 1 1")?;
        loop {
            let c = r.u32()?;
            if c > 4096 {
                return Err(bad("empty group chain count"));
            }
            for _ in 0..c {
                r.u32()?;
            }
            r.u32()?;
            for _ in 0..6 {
                r.f64()?;
            }
            let v = r.u16()?;
            if v == 1 && r.pu16(r.p)? & 0x8000 == 0 {
                continue;
            }
            return Ok(u32::from(v));
        }
    }
    for _ in 0..k {
        r.ptr(None)?;
    }
    for _ in 0..k {
        r.u32()?;
    }
    for _ in 0..k {
        let n = r.u16()?;
        for _ in 0..n {
            r.ptr(None)?;
        }
    }
    r.f64()?;
    r.u16()?;
    let st = r.u32()?;
    if st == 0 && r.eq(r.p, &[0, 0]) {
        r.skip(2);
        return chains(r);
    }
    if st != 1 && st != 2 {
        return Err(bad("group support type"));
    }
    r.ptr(None)?;
    let bodies = support(r, i64::from(st))?;
    if st == 1 {
        if bodies.truthy() {
            r.expect_bytes(&[0, 0], "group u16 0")?;
        }
        r.u32()?;
        r.u32()?;
    }
    chains(r)
}

/// `support(r, st)`: after a face (1) or loop (2) support reference;
/// returns the compressed bodies.
pub(crate) fn support(r: &mut Reader<'_>, st: i64) -> Res<V> {
    if st == 1 {
        r.expect_bytes(&[0, 0], "support gap")?;
    } else {
        r.expect_bytes(&[2, 0, 0, 0, 0, 0, 0, 0], "support gap")?;
    }
    if r.eq(r.p, &[1, 0]) {
        r.skip(2);
        r.ptr(Some(&["moContextChainComponent_c"]))?;
        if r.zeros(r.p, 12) && !r.ffs(r.p + 12, 4) {
            r.skip(12);
        } else {
            r.expect_bytes(&zeros_then(12, &[0xff; 4]), "support -1")?;
        }
    } else {
        if r.zeros(r.p, 8)
            && r.eq(r.p + 12, &[0, 0, 0, 0, 0xff, 0xff, 0xff, 0xff, 0, 0, 0, 0])
            && r.eq(r.p + 24, &m3())
        {
            r.skip(8);
            r.u32()?;
            r.skip(12);
            r.skip(12);
            r.expect_zeros(4, "")?;
            r.u32()?;
            if r.ffs(r.p + 12, 4) {
                r.expect_bytes(&zeros_then(12, &[0xff; 4]), "support face -1")?;
            } else {
                r.expect_zeros(12, "support face 12 zero")?;
            }
            if st == 1 {
                for _ in 0..4 {
                    r.f64()?;
                }
            }
            return blobs(r);
        }
        let q = r.p;
        if r.zeros(q, 8)
            && r.zeros(q + 12, 12)
            && r.eq(q + 24, &M12345[..8])
            && r.zeros(q + 32, 4)
            && isver(r, q + 36)
            && r.zeros(q + 40, 12)
        {
            r.p = q + 52;
            if st == 1 {
                for _ in 0..4 {
                    r.f64()?;
                }
            }
            return blobs(r);
        }
        let vals = |r: &Reader<'_>, q: usize| {
            r.zeros(q + 8, 4) && (r.eq(q + 28, &m3()) || (r.zeros(q + 40, 4) && isver(r, q + 44)))
        };
        if !vals(r, r.p) && r.p >= 4 && r.zeros(r.p - 4, 4) && vals(r, r.p - 4) {
            r.p -= 4;
        }
        r.expect_zeros(8, "support null form")?;
        r.expect_zeros(4, "")?;
        r.u32()?;
        r.expect_zeros(12, "")?;
        if !r.eq(r.p, &m3()) && (!r.zeros(r.p + 12, 4) || !isver(r, r.p + 16)) {
            return Err(bad("support -12345"));
        }
        r.skip(12);
        r.expect_zeros(4, "")?;
        if isver(r, r.p) {
            r.skip(4);
        }
        r.expect_bytes(&zeros_then(12, &[0xff; 4]), "support block -1")?;
    }
    if st == 1 {
        for _ in 0..4 {
            r.f64()?;
        }
    }
    blobs(r)
}

/// `[blob(r) for i in range(r.u32())]`.
fn blobs(r: &mut Reader<'_>) -> Res<V> {
    let n = r.u32()?;
    let mut out = Vec::new();
    for _ in 0..n {
        out.push(blob(r)?);
    }
    Ok(V::List(out))
}

/// `chains(r)`: chains, each followed by a u16; returns the last u16.
fn chains(r: &mut Reader<'_>) -> Res<u32> {
    loop {
        let c = r.u32()?;
        for _ in 0..c {
            r.u32()?;
        }
        let a = r.u32()?;
        let fl = r.u32()?;
        if fl == 0 && r.eq(r.p + 56, &[2, 0]) && isclass(r, r.p + 58, &["sgPGMProfileData_c"])? {
            r.f64()?;
            for _ in 0..6 {
                r.f64()?;
            }
            r.skip(2);
            r.ptr(Some(&["sgPGMProfileData_c"]))?;
            r.st.sk.pgmdone = true;
            return Ok(0);
        }
        if c == 0 && 0 < a && a < 4096 && fl <= 1 && r.pu32(r.p)? < 256 {
            r.u32()?;
        } else if (a > 1 && a != c) || fl > 255 {
            return Err(bad("chain u32 0/1"));
        }
        for _ in 0..6 {
            r.f64()?;
        }
        let v = r.u16()?;
        if v == 1 && r.pu16(r.p)? & 0x8000 == 0 {
            continue;
        }
        return Ok(u32::from(v));
    }
}

/// `blob(r)`: a compressed body; returns its decompressed bytes.
pub(crate) fn blob(r: &mut Reader<'_>) -> Res<V> {
    use std::io::Read;
    r.expect_bytes(&[0], "blob u8 0")?;
    let a = r.u32()?;
    r.expect_bytes(&[0], "blob u8 0 b")?;
    let b = r.u32()?;
    if i64::from(b) != i64::from(a) - 5 {
        return Err(bad("blob lengths"));
    }
    let q = r.p;
    let ul = r.pu32(q + 16)?;
    let cl = r.pu32(q + 20)?;
    let compressed = r.slice(q + 24, cl as usize);
    let mut data = Vec::new();
    flate2::read::ZlibDecoder::new(compressed)
        .take(u64::from(ul) + 1)
        .read_to_end(&mut data)
        .map_err(|_| bad("blob zlib"))?;
    if data.len() != ul as usize {
        return Err(bad("blob length"));
    }
    r.p = q + b as usize;
    Ok(V::Bytes(data))
}

/// `iscomp(r, q)`: a context or unit component at `q`.
fn iscomp(r: &Reader<'_>, q: usize) -> Res<bool> {
    let w = r.pu16(q)?;
    if w == 0xffff {
        let n = usize::from(r.pu16(q + 4)?);
        let name = r.slice(q + 6, n);
        return Ok(name == b"moContextChainComponent_c" || name == b"moUnitComponent_c");
    }
    Ok(w & 0x8000 != 0
        && r.known
            .get(&u32::from(w & 0x7fff))
            .is_some_and(|c| &**c == "moContextChainComponent_c" || &**c == "moUnitComponent_c"))
}

/// `moSketchExtRef_w`, `moSingleFaceRef_w`, ... (`sketchextref`).
pub(crate) fn sketchextref(r: &mut Reader<'_>) -> Res<V> {
    let me = r.last_class().map_or_else(String::new, |c| c.to_string());
    let e = r.ptr(None)?;
    let q = r.p;
    let eclass = e.obj_class().map(str::to_owned);
    let eclass = eclass.as_deref();
    if eclass == Some("moCompSilhVertex_c")
        && (1..=16).contains(&r.pu32(q)?)
        && (1..=8).contains(&r.pu32(q + 20)?)
    {
        r.skip(4);
        let a = vec![V::from(r.f64()?), V::from(r.f64()?)];
        r.skip(4);
        r.expect_zeros(10, "silhouette vertex 10 zero")?;
        r.u32()?;
        let mut x = zeros_then(12, &m3());
        x.extend_from_slice(&[0; 4]);
        r.expect_bytes(&x, "silhouette vertex -12345")?;
        r.u32()?;
        let mut y = zeros_then(12, &[0xff; 4]);
        y.extend_from_slice(&[0, 0]);
        r.expect_bytes(&y, "silhouette vertex -1")?;
        let bk = r.ptr(None)?;
        let nm = entname(r)?;
        return Ok(V::List(vec![V::from(e), V::List(a), V::from(bk), nm]));
    }
    if eclass == Some("moCompSilhEdge_c") && (1..=16).contains(&r.pu32(q)?) {
        r.skip(4);
        let a = vec![V::from(r.f64()?), V::from(r.f64()?)];
        let mut z = 0;
        while r.byte(r.p + z)? == 0 && z < 200 {
            z += 1;
        }
        r.skip(z);
        if r.eq(r.p + 16, &m3()) && !r.eq(r.p, &[1, 0, 0, 0]) {
            r.skip(16);
            r.skip(12);
            r.expect_zeros(4, "silhouette u32 0")?;
            r.u32()?;
            let mut x = zeros_then(12, &[0xff; 4]);
            x.extend_from_slice(&[0; 10]);
            r.expect_bytes(&x, "silhouette -1")?;
            let nm = if r.eq(r.p, &[0xff, 0xfe, 0xff]) {
                V::Str(r.cstring()?)
            } else {
                V::None
            };
            return Ok(V::List(vec![V::from(e), V::List(a), nm]));
        }
        r.expect_bytes(&[1, 0, 0, 0], "silhouette u32 1")?;
        let fl = r.u8()?;
        if fl > 1 {
            return Err(bad("silhouette flag"));
        }
        for _ in 0..if fl != 0 { 12 } else { 3 } {
            r.f64()?;
        }
        if r.f64()? != 1.0 {
            return Err(bad("silhouette scale"));
        }
        r.expect_bytes(&[0], "silhouette u8 0")?;
        let t = cseh_tail(r, true)?;
        return Ok(V::List(vec![V::from(e), V::List(a), t]));
    }
    let mut z = 0;
    while r.byte(q + z)? == 0 && z < 80 {
        z += 1;
    }
    if me == "moEndPointRef_w"
        && z >= 2
        && r.eq(q + z, &[1, 0])
        && isclass(r, q + z + 2, &["moContextChainComponent_c"])?
        && r.byte(q + z - 1)? == 0
    {
        r.p = q + z + 2;
        r.ptr(None)?;
        r.expect_zeros(12, "end point 12 zero")?;
        if r.eq(r.p, &[0xff, 0xff, 0xff, 0xff]) {
            r.skip(4); // an i32 -1 before the point (1 part)
        }
        let pt = f64s(r, 3)?;
        let fr = r.f64()?;
        if !(0.0..=1.0).contains(&fr) {
            // The fraction along the edge (1.0 in one sample, 0.5 in another).
            return Err(bad("end point fraction"));
        }
        r.expect_bytes(&[0, 0], "end point u16 0")?;
        return Ok(V::List(vec![V::from(e), pt]));
    }
    if r.st.sk.byref && r.zeros(q, 62) && r.eq(q + 62, &[1, 0]) && r.eq(q + 66 + 20, &m3()) {
        r.skip(58);
        let t = cseh_tail(r, false)?;
        return Ok(V::List(vec![V::from(e), t]));
    }
    if r.zeros(q, 64)
        && (iscomp(r, q + 64)?
            || r.eq(q + 66 + 20, &m3())
            || (r.zeros(q + 66 + 32, 4) && isver(r, q + 66 + 36))
            || (r.eq(q + 66 + 20, &M12345[..8]) && isver(r, q + 66 + 32)))
    {
        r.skip(58);
        let t = cseh_tail(r, false)?;
        return Ok(V::List(vec![V::from(e), t]));
    }
    if eclass == Some("moCompVertex_c")
        && r.zeros(q, 4)
        && isclass(r, q + 4, &["moContextChainComponent_c"])?
    {
        r.skip(4);
        r.ptr(None)?;
        if r.zeros(r.p, 14) && objat(r, r.p + 14) {
            r.skip(14);
        } else {
            let mut x = zeros_then(12, &[0xff; 4]);
            x.extend_from_slice(&[0, 0]);
            r.expect_bytes(&x, "vertex context tail")?;
        }
        let bk = r.ptr(None)?;
        let nm = entname(r)?;
        return Ok(V::List(vec![V::from(e), V::from(bk), nm]));
    }
    // `blk = r.b[q+6:q+64]`: its offsets below are `blk + i`.
    let blk = q + 6;
    if eclass == Some("moCompVertex_c")
        && r.zeros(q, 6)
        && r.eq(blk + 20, &M12345[..8])
        && r.zeros(blk + 28, 4)
        && isver(r, blk + 32)
        && r.zeros(blk + 36, 22)
    {
        r.skip(64);
        let nm = if r.eq(r.p, &[0xff, 0xfe, 0xff]) {
            V::Str(r.cstring()?)
        } else {
            V::None
        };
        return Ok(V::List(vec![V::from(e), V::None, nm]));
    }
    if eclass == Some("moCompVertex_c")
        && r.zeros(q, 6)
        && (r.eq(q + 6 + 20, &m3()) || (r.zeros(q + 6 + 32, 4) && isver(r, q + 6 + 36)))
    {
        r.skip(6);
        let q = r.p + 40;
        let w = r.pu16(q + 14)?;
        r.skip(if r.zeros(q, 14) && tagword(w) { 54 } else { 58 });
        let bk = r.ptr(None)?;
        let nm = entname(r)?;
        return Ok(V::List(vec![V::from(e), V::from(bk), nm]));
    }
    Ok(V::from(e))
}

/// `entname(r)`: an entity's optional name after its backed-up geometry.
fn entname(r: &mut Reader<'_>) -> Res<V> {
    if r.zeros(r.p, 6) && r.eq(r.p + 6, &[0xff, 0xfe, 0xff]) {
        r.skip(6);
        return Ok(V::Str(r.cstring()?));
    }
    if r.eq(r.p, &[0, 0]) && r.eq(r.p + 2, &[0xff, 0xfe, 0xff]) {
        r.skip(2);
        return Ok(V::Str(r.cstring()?));
    }
    if r.eq(r.p, &[0, 0]) && isclass(r, r.p + 2, &["moSkIntersectInfo_c"])? {
        r.skip(2);
        r.ptr(None)?;
        if r.eq(r.p, &[0xff, 0xfe, 0xff]) {
            return Ok(V::Str(r.cstring()?));
        }
    }
    Ok(V::None)
}

/// `moSkIntersectInfo_c` (`skintersect`).
#[allow(clippy::unnecessary_wraps)]
pub(crate) fn skintersect(r: &mut Reader<'_>) -> Res<V> {
    let b = r.slice(r.p, 20).to_vec();
    r.skip(20);
    Ok(V::Bytes(b))
}

/// `cseh_tail(r, nok)`: a `moCompSketchEntHandle_c`'s tail; `nok`: no
/// leading u32.
fn cseh_tail(r: &mut Reader<'_>, nok: bool) -> Res<V> {
    if !nok {
        r.u32()?;
    }
    if r.u16()? > 1 {
        return Err(bad("cseh u16 0"));
    }
    let c2 = r.ptr(None)?;
    if c2.is_obj() {
        if r.zeros(r.p, 14) && tagword(r.pu16(r.p + 14)?) {
            r.skip(14);
        } else {
            let mut x = zeros_then(12, &[0xff; 4]);
            x.extend_from_slice(&[0, 0]);
            r.expect_bytes(&x, "cseh context tail")?;
        }
    } else {
        // `blk = r.b[r.p:r.p+58]`: its offsets below are `p + i`.
        let p = r.p;
        let two = r.eq(p + 20, &M12345[..8]) && r.zeros(p + 28, 4);
        if two && isver(r, p + 32) && r.zeros(p + 36, 18) && r.eq(p + 54, &[0xff, 0xfe, 0xff]) {
            r.skip(54);
            return Ok(V::List(vec![V::None, V::Str(r.cstring()?)]));
        }
        if two && (isver(r, p + 32) || r.zeros(p + 32, 4)) && r.zeros(p + 36, 20) {
            r.skip(56);
            if r.eq(r.p, &[0, 0, 0xff, 0xfe, 0xff]) {
                r.skip(2);
                return Ok(V::List(vec![V::None, V::Str(r.cstring()?)]));
            }
            return Ok(V::List(vec![V::None, V::None]));
        } else if two && isver(r, p + 32) && r.zeros(p + 36, 16) && !r.zeros(p + 52, 4) {
            r.skip(52);
            return Ok(V::List(vec![V::None, V::None]));
        }
        if !r.eq(p + 20, &m3()) && (!r.zeros(p + 32, 4) || !isver(r, p + 36)) {
            return Err(bad("cseh -12345 run"));
        }
        let q = r.p + 40;
        let w = r.pu16(q + 14)?;
        if r.zeros(q, 14) && tagword(w) {
            r.skip(54);
        } else {
            r.skip(58);
        }
    }
    let p = r.ptr(None)?;
    let nm = entname(r)?;
    Ok(V::List(vec![V::from(p), nm]))
}

/// `moCompSketchEntHandle_c` (`compskent`).
pub(crate) fn compskent(r: &mut Reader<'_>) -> Res<V> {
    r.obj(Some(UCS))?;
    r.u32()?;
    r.u32()?;
    r.ptr(None)?;
    cseh_tail(r, false)
}

/// `moPointBackedUpData_c` (`pointbackup`).
pub(crate) fn pointbackup(r: &mut Reader<'_>) -> Res<V> {
    f64s(r, 3)
}

/// `sgEntHandle` (`enthandle`).
pub(crate) fn enthandle(r: &mut Reader<'_>) -> Res<V> {
    let slot = r.u16()?;
    r.u32()?;
    r.u32()?;
    r.expect_bytes(&[0, 0], "ent handle u16 0")?;
    Ok(V::from(slot))
}

/// A known class's tag (`w & 0x8000 and w != 0xffff and index in known`).
fn known_tag(r: &Reader<'_>, w: u16) -> bool {
    w & 0x8000 != 0 && w != 0xffff && r.known.contains_key(&u32::from(w & 0x7fff))
}

/// `sgPntPntDist`, `sgCircleDim`, ... (`pntpntdist`).
pub(crate) fn pntpntdist(r: &mut Reader<'_>) -> Res<V> {
    r.expect_zeros(4, "dim u32 0")?;
    let p = r.obj(None)?;
    let mut l = 34;
    if r.st.sk.dimlast {
        for l2 in [32_usize, 34] {
            let w = r.pu16(r.p + l2)?;
            let w2 = r.pu16(r.p + l2 + 2)?;
            if 0 < w && w < 4096 && (w2 == 0xffff || known_tag(r, w2)) {
                r.skip(l2);
                return Ok(V::from(p));
            }
        }
    }
    for l2 in [32_usize, 34, 36] {
        let w = r.pu16(r.p + l2)?;
        if known_tag(r, w) || (w == 0xffff && r.eq(r.p + l2 + 2, &[1, 0])) {
            l = l2;
            break;
        }
    }
    r.skip(l);
    Ok(V::from(p))
}

/// `sgValG3Dim` (`valg3dim`).
pub(crate) fn valg3dim(r: &mut Reader<'_>) -> Res<V> {
    r.expect_zeros(4, "dim u32 0")?;
    let p = r.obj(None)?;
    let ops = p
        .value()
        .and_then(|v| v.get("ops").as_list().map(<[V]>::len))
        .unwrap_or(0);
    if ops < 3 {
        r.ptr(None)?;
    }
    let w = r.pu16(r.p)?;
    let w2 = r.pu16(r.p + 2)?;
    let dimish = r
        .known
        .get(&u32::from(w2 & 0x7fff))
        .is_some_and(|c| c.ends_with("Dim") || c.ends_with("Dist"));
    if r.st.sk.dimlast && 0 < w && w < 4096 && (w2 == 0xffff || (w2 & 0x8000 != 0 && dimish)) {
        return Ok(V::from(p));
    }
    r.u16()?;
    Ok(V::from(p))
}

/// `sgCircularPattCntDim`, `sgLinearPattCntDim` (`pattcntdim`).
pub(crate) fn pattcntdim(r: &mut Reader<'_>) -> Res<V> {
    r.expect_zeros(4, "dim u32 0")?;
    Ok(V::from(r.obj(None)?))
}

/// `dimtail(r)` (no caller in the grammar).
fn dimtail(r: &mut Reader<'_>) -> Res<()> {
    let w = r.pu16(r.p + 32)?;
    // `(w & 0x8000 and w != 0xffff) or w == 0xffff`.
    let l = if w & 0x8000 != 0 { 32 } else { 34 };
    r.skip(l);
    Ok(())
}

/// `sgOffsetDim`, `sgSkOffsetDim` (`offsetdim`).
pub(crate) fn offsetdim(r: &mut Reader<'_>) -> Res<V> {
    r.expect_zeros(4, "dim u32 0")?;
    r.st.core.offdim.push(2);
    let res = r.obj(None);
    r.st.core.offdim.pop();
    Ok(V::from(res?))
}

/// `suObList`, `suObArray`, `cptsSplineList_c`, `modifSplineList_c`
/// (`oblist`).
pub(crate) fn oblist(r: &mut Reader<'_>) -> Res<V> {
    let mut n = u32::from(r.u16()?);
    if n == 0xffff {
        n = r.u32()?;
    }
    let mut out = Vec::new();
    for _ in 0..n {
        out.push(V::from(r.ptr(None)?));
    }
    Ok(V::List(out))
}

/// `moCompRefPlane_c` (`comprefplane`).
pub(crate) fn comprefplane(r: &mut Reader<'_>) -> Res<V> {
    r.obj(Some(UCS))?;
    let fid = r.u32()?;
    let ts = r.u32()?;
    if fid == 0xffff_ffff {
        let pl = r.obj(Some(&["moRefPlane_c"]))?;
        let data = r.obj(None)?;
        if matches!(
            data.obj_class(),
            Some("moAbsoluteRefPlnData_c" | "moFixedRefPlnData_c")
        ) && r.zeros(r.p, 4)
            && r.byte(r.p + 4)? <= 1
        {
            r.skip(4);
            planeframe(r, true)?;
        }
        return Ok(V::map([
            ("fid", V::from(fid)),
            ("ts", V::from(ts)),
            ("plane", V::from(pl)),
        ]));
    }
    if r.eq(r.p + 30, &m3()) && (r.st.sk.mirror || (r.ffs(r.p + 62, 4) && !r.eq(r.p + 66, &[0, 0])))
    {
        r.expect_bytes(&[0, 0], "plane u16 0")?;
        r.u32()?;
        r.u16()?;
        r.expect_bytes(&[0, 0], "mirror plane null")?;
        r.skip(20);
        let mut x = m3().to_vec();
        x.extend_from_slice(&[0; 4]);
        r.expect_bytes(&x, "mirror plane -12345")?;
        r.u32()?;
        r.expect_bytes(&zeros_then(12, &[0xff; 4]), "mirror plane -1")?;
        for _ in 0..4 {
            r.f64()?;
        }
        let pn = f64s(r, 6)?;
        let fl = r.u8()?;
        if fl > 1 {
            return Err(bad("mirror plane flag"));
        }
        for _ in 0..if fl != 0 { 12 } else { 3 } {
            r.f64()?;
        }
        if r.f64()? != 1.0 {
            return Err(bad("mirror plane scale"));
        }
        r.expect_zeros(21, "mirror plane 21 zero")?;
        return Ok(V::map([
            ("fid", V::from(fid)),
            ("ts", V::from(ts)),
            ("mirror", pn),
        ]));
    }
    if r.eq(r.p + 30, &m3()) {
        r.expect_bytes(&[0, 0], "plane u16 0")?;
        let t = cseh_tail(r, false)?;
        return Ok(V::map([
            ("fid", V::from(fid)),
            ("ts", V::from(ts)),
            ("tail", t),
        ]));
    }
    r.expect_bytes(&[0, 0], "plane u16 0")?;
    let plane = r.u16()?;
    r.expect_bytes(&[0, 0], "plane u16 0 b")?;
    if r.eq(r.p, &[1, 0]) && isclass(r, r.p + 2, &["moContextChainComponent_c"])? {
        r.skip(2);
        r.ptr(Some(&["moContextChainComponent_c"]))?;
        for _ in 0..10 {
            r.f64()?;
        }
        let fl = r.u8()?;
        if fl > 1 {
            return Err(bad("plane flag"));
        }
        for _ in 0..if fl != 0 { 12 } else { 3 } {
            r.f64()?;
        }
        if r.f64()? != 1.0 {
            return Err(bad("plane scale"));
        }
        let mut x = vec![0; 31];
        x.push(1);
        x.extend_from_slice(&[0; 15]);
        r.expect_bytes(&x, "in-context plane 31 zero, 1, 15 zero")?;
    }
    planeframe(r, false)?;
    Ok(V::map([
        ("fid", V::from(fid)),
        ("ts", V::from(ts)),
        ("plane", V::from(plane)),
    ]))
}

/// `planeframe(r, short)`: a plane's frame and trailer.
pub(crate) fn planeframe(r: &mut Reader<'_>, short: bool) -> Res<V> {
    let fl = r.u8()?;
    if fl > 1 {
        return Err(bad("plane flag"));
    }
    for _ in 0..if fl != 0 { 12 } else { 3 } {
        r.f64()?;
    }
    let scale = r.f64()?;
    if scale != 1.0 {
        return Err(bad("plane scale"));
    }
    planetrailer(r, short)?;
    Ok(V::None)
}

/// `planetrailer(r, short)`.
fn planetrailer(r: &mut Reader<'_>, short: bool) -> Res<()> {
    let q = r.p;
    let z = if matches!(r.byte(q + 1)?, 2..=4) && r.zeros(q + 2, 3) {
        1
    } else {
        3
    };
    if !r.zeros(q, z) || !matches!(r.byte(q + z)?, 2..=4) || !r.zeros(q + z + 1, 3) {
        return Err(bad("plane trailer"));
    }
    r.skip(z + 16);
    if short {
        if r.zeros(r.p, 12) {
            r.skip(12);
            return Ok(());
        }
        r.u32()?;
        r.expect_bytes(&[0, 0, 0, 0, 1, 0, 0, 0], "plane trailer 0 1")?;
        return Ok(());
    }
    if isclass(r, r.p + 2, &["moContourChooser_c"])? {
        r.u16()?;
        r.ptr(None)?;
        let n = r.u16()?;
        for _ in 0..n {
            r.ptr(None)?;
        }
        r.expect_zeros(2, "plane trailer chooser zero")?;
        return Ok(());
    }
    let w = r.pu16(r.p + 2)?;
    let nok = r.pu16(r.p)? != 0 && tagword(w);
    if !nok {
        r.u32()?;
    }
    let n = r.u16()?;
    for _ in 0..n {
        r.ptr(None)?;
    }
    let m = r.u16()?;
    for _ in 0..m {
        r.ptr(None)?;
    }
    if !nok && n == 0 && m == 0 {
        for z in [0_usize, 2, 6, 8] {
            if zeros0(r, r.p, z) && objat(r, r.p + z) {
                r.skip(z);
                return Ok(());
            }
        }
    }
    r.expect_zeros(if nok { 2 } else { 4 }, "plane trailer zero")
}

/// `moSketchRegion_c` (`region`).
pub(crate) fn region(r: &mut Reader<'_>) -> Res<V> {
    let l = r.ptr(None)?;
    if !r.zeros(r.p, 30) {
        return Ok(V::List(vec![V::from(l), V::List(Vec::new())]));
    }
    r.expect_zeros(30, "region 30 zero")?;
    r.u32()?;
    r.u32()?;
    let mut chains = Vec::new();
    loop {
        let w = r.pu16(r.p)?;
        let isch = (w == 0xffff && r.eq(r.p + 6, b"moSketchChain_c"))
            || (w & 0x8000 != 0
                && w != 0xffff
                && r.known
                    .get(&u32::from(w & 0x7fff))
                    .is_some_and(|c| &**c == "moSketchChain_c"));
        if !isch {
            break;
        }
        chains.push(V::from(r.ptr(None)?));
    }
    Ok(V::List(vec![V::from(l), V::List(chains)]))
}

/// `moSketchChain_c` (`chain`).
pub(crate) fn chain(r: &mut Reader<'_>) -> Res<V> {
    let n = r.u16()?;
    let mut ids = Vec::new();
    for _ in 0..n {
        ids.push(V::from(r.u32()?));
    }
    let k = r.u16()?;
    if k > 4096 {
        return Err(bad("chain k count"));
    }
    for _ in 0..k {
        r.u32()?;
    }
    for _ in 0..6 {
        r.u32()?;
    }
    r.cstring()?;
    let h = r.u16()?;
    for _ in 0..h {
        r.ptr(None)?;
    }
    r.u32()?;
    r.expect_zeros(20, "chain 20 zero")?;
    Ok(V::List(ids))
}

/// `sgPGMProfileData_c` (`pgmprofile`).
pub(crate) fn pgmprofile(r: &mut Reader<'_>) -> Res<V> {
    let nl = r.u16()?;
    for _ in 0..nl {
        r.ptr(None)?;
    }
    r.u16()?;
    let d = r.f64()?;
    let mut z = 0;
    while r.byte(r.p + z)? == 0 && z < 64 {
        z += 1;
    }
    let q = r.p + z;
    let n = if z >= 8 { r.pu32(q)? } else { 0 };
    let ok = z >= 8 && 0 < n && n < 1000 && r.pu32(q + 4 + 4 * n as usize + 4)? == 0;
    if !ok {
        r.expect_zeros(12, "offset profile 12 zero")?;
        return Ok(V::map([("loops", V::from(nl)), ("dist", V::from(d))]));
    }
    r.skip(z + 4);
    for _ in 0..n {
        r.u32()?;
    }
    r.u32()?;
    r.expect_zeros(4, "offset profile u32 0")?;
    r.f64()?;
    for _ in 0..6 {
        r.f64()?;
    }
    r.expect_bytes(&[2, 0], "offset profile u16 2")?;
    let nxt = r.ptr(Some(&["sgPGMProfileData_c"]))?;
    Ok(V::map([
        ("loops", V::from(nl)),
        ("dist", V::from(d)),
        ("next", V::from(nxt)),
    ]))
}

/// `sgPGMLoopOffsetData_c` (`pgmloop`).
pub(crate) fn pgmloop(r: &mut Reader<'_>) -> Res<V> {
    r.u32()?;
    r.u32()?;
    let n = r.u16()?;
    for _ in 0..n {
        r.ptr(None)?;
    }
    Ok(V::map([("edges", V::from(n))]))
}

/// `sgPGMLoopOffsetEdgeData_c` (`pgmedge`).
pub(crate) fn pgmedge(r: &mut Reader<'_>) -> Res<V> {
    Ok(V::from(r.u32()?))
}

/// The temporary axis's handle-block end: twelve zero bytes, i32 -1 and
/// eight zero bytes, or twenty zero bytes in older files.
fn tempaxis_tail(r: &mut Reader<'_>, what: &str) -> Res<()> {
    if r.ffs(r.p + 12, 4) {
        r.expect_bytes(&zeros_then(12, &[0xff; 4]), what)?;
        r.expect_zeros(8, "temp axis 8 zero")
    } else {
        r.expect_zeros(20, "temp axis 20 zero")
    }
}

/// `moTempAxisRef_w` (`tempaxis`).
pub(crate) fn tempaxis(r: &mut Reader<'_>) -> Res<V> {
    let e = r.ptr(None)?;
    if !r.eq(r.p, &[0, 0, 1, 0]) {
        return Ok(V::from(e));
    }
    r.skip(4);
    let cc = r.ptr(None)?;
    if cc.is_obj() {
        tempaxis_tail(r, "temp axis -1")?;
    } else {
        r.expect_zeros(4, "temp axis u32 0")?;
        r.u32()?;
        r.expect_zeros(12, "")?;
        r.expect_bytes(&m3(), "temp axis -12345")?;
        r.expect_zeros(4, "")?;
        if isver(r, r.p) {
            r.skip(4);
        }
        tempaxis_tail(r, "temp axis -1 b")?;
    }
    let ax = f64s(r, 7)?;
    let z = if ver_or(r, 99999) < 15000 { 7 } else { 59 };
    r.expect_zeros(z, &format!("temp axis {z} zero"))?;
    let nm = r.cstring()?;
    Ok(V::List(vec![V::from(e), ax, V::Str(nm)]))
}

/// `moExtensionLineIndicator_c` (`extline`).
pub(crate) fn extline(r: &mut Reader<'_>) -> Res<V> {
    let p = r.ptr(None)?;
    let w = r.u16()?;
    Ok(V::List(vec![V::from(p), V::from(w)]))
}

/// `moOneContour_c` (`onecontour`).
pub(crate) fn onecontour(r: &mut Reader<'_>) -> Res<V> {
    let n = r.u16()?;
    let mut out = Vec::new();
    for _ in 0..n {
        out.push(V::from(r.ptr(None)?));
    }
    Ok(V::List(out))
}

/// Two hop lists (u16 count and pointers, twice); returns their lengths.
fn hops2(r: &mut Reader<'_>) -> Res<(u16, u16)> {
    let n = r.u16()?;
    for _ in 0..n {
        r.ptr(None)?;
    }
    let m = r.u16()?;
    for _ in 0..m {
        r.ptr(None)?;
    }
    Ok((n, m))
}

fn triple(bid: u16, n: u16, m: u16) -> V {
    V::List(vec![V::from(bid), V::from(n), V::from(m)])
}

/// `contourchooser`'s several-contours form, read with `SKBOX` set.
fn contours_boxed(r: &mut Reader<'_>, g: u16, cs: &mut usize) -> Res<V> {
    for i in 0..g {
        if i != 0 {
            r.ptr(Some(&["moOneContour_c"]))?;
            *cs += 1;
        }
        let nb = r.u16()?;
        for _ in 0..nb {
            r.ptr(Some(&["moBBoxCenterData_c"]))?;
        }
    }
    r.u32()?;
    let bid = r.u16()?;
    r.u32()?;
    r.u32()?;
    r.u16()?;
    if isclass(r, r.p, &["suObArray"])? {
        r.ptr(None)?;
    } else {
        r.u16()?;
        r.u16()?;
    }
    let (n, m) = hops2(r)?;
    Ok(V::List(vec![
        V::Int(*cs as i64),
        V::List(vec![triple(bid, n, m)]),
    ]))
}

/// `contourchooser`'s boxes, read with `SKBOX` set.
fn contour_boxes(r: &mut Reader<'_>, g: u16, cs: usize) -> Res<V> {
    let nb = r.u16()?;
    let mut out = Vec::new();
    for _ in 0..nb {
        let bx = r.ptr(Some(&["moBBoxCenterData_c"]))?;
        if !bx.is_null() || g != 0 {
            r.u32()?;
        }
        let bid = r.u16()?;
        r.u32()?;
        r.u32()?;
        r.u16()?;
        if isclass(r, r.p, &["suObArray"])? {
            r.ptr(None)?;
        } else {
            r.u16()?;
            r.u16()?;
        }
        let (n, m) = hops2(r)?;
        out.push(triple(bid, n, m));
    }
    Ok(V::List(vec![V::Int(cs as i64), V::List(out)]))
}

/// `moContourChooser_c` (`contourchooser`).
pub(crate) fn contourchooser(r: &mut Reader<'_>) -> Res<V> {
    let g = r.u16()?;
    if g == 0 && r.eq(r.p, &[1, 0, 0, 0]) && isclass(r, r.p + 16, &["suObArray"])? {
        r.skip(4);
        let bid = r.u16()?;
        r.u32()?;
        r.u32()?;
        r.u16()?;
        r.ptr(Some(&["suObArray"]))?;
        let (n, m) = hops2(r)?;
        return Ok(V::List(vec![V::Int(0), V::List(vec![triple(bid, n, m)])]));
    }
    let mut cs = usize::from(g != 0);
    if g != 0 {
        r.ptr(None)?;
    }
    if g > 1 && isclass(r, r.p + 2, &["moBBoxCenterData_c"])? {
        r.st.core.skbox = true;
        let res = contours_boxed(r, g, &mut cs);
        r.st.core.skbox = false;
        return res;
    }
    for _ in 0..g.saturating_sub(1) {
        r.ptr(None)?;
        cs += 1;
    }
    r.st.core.skbox = true;
    let res = contour_boxes(r, g, cs);
    r.st.core.skbox = false;
    res
}

/// `sgSlot_c` (`slot`).
pub(crate) fn slot(r: &mut Reader<'_>) -> Res<V> {
    r.u32()?;
    let sid = r.u32()?;
    let blk = r.p;
    r.skip(72);
    let mut hs = Vec::new();
    for _ in 0..6 {
        hs.push(V::from(r.ptr(None)?));
    }
    // `struct.unpack_from('<I', blk, 17)` on the 72-byte copy.
    let kind = r.pu32(blk + 17)?;
    Ok(V::map([
        ("id", V::from(sid)),
        ("kind", V::from(kind)),
        ("handles", V::List(hs)),
    ]))
}

/// `sgSlotHandle` (`slothandle`).
pub(crate) fn slothandle(r: &mut Reader<'_>) -> Res<V> {
    let slot = r.u16()?;
    r.expect_bytes(&[0xff; 4], "slot handle -1")?;
    r.u32()?;
    let sel = r.u16()?;
    let idx = r.u16()?;
    r.expect_bytes(&[0xff, 0xff, 0xff, 0xff, 0, 0, 0, 0], "slot handle cell")?;
    Ok(V::List(vec![V::from(slot), V::from(sel), V::from(idx)]))
}

/// `moCompProfile_c`, `moCompReferenceCurve_c` (`compprofile`).
pub(crate) fn compprofile(r: &mut Reader<'_>) -> Res<V> {
    r.obj(Some(UCS))?;
    let fid = r.u32()?;
    r.u32()?;
    let Some(j) = r.find(&M12345[..8], r.p, r.p + 40) else {
        return Err(bad("comp profile -12345"));
    };
    r.p = j;
    while r.eq(r.p, &[0xc7, 0xcf, 0xff, 0xff]) {
        r.p += 4;
    }
    r.expect_zeros(4, "comp profile u32 0")?;
    r.u32()?;
    let Some(k) = r.find(&1.0_f64.to_le_bytes(), r.p, r.p + 40) else {
        return Err(bad("comp profile f64 1"));
    };
    let gap = r.slice(r.p, k.saturating_sub(r.p));
    if gap.iter().any(|&b| b != 0 && b != 0xff) || r.byte(k + 8)? > 1 {
        return Err(bad("comp profile f64 1"));
    }
    let fl = r.byte(k + 8)?;
    r.p = k + 9;
    let body = if fl != 0 {
        match blob(r)? {
            V::Bytes(b) => b.len(),
            _ => 0,
        }
    } else {
        0
    };
    let mut x = vec![0xff; 8];
    x.extend_from_slice(&[0; 6]);
    r.expect_bytes(&x, "comp profile ff")?;
    Ok(V::map([
        ("fid", V::from(fid)),
        ("body", V::Int(body as i64)),
    ]))
}

/// `moWireContact_c` (`wirecontact`).
pub(crate) fn wirecontact(r: &mut Reader<'_>) -> Res<V> {
    let a = f64s(r, 3)?;
    r.expect_bytes(
        &[0, 0, 0, 0, 1, 0, 0, 0, 0xff, 0xff, 0xff, 0xff, 2, 0],
        "wire contact 0 1 -1 2",
    )?;
    let b = f64s(r, 3)?;
    // The word after it is the cross-section reference's class tag.
    Ok(V::List(vec![a, b]))
}

/// `sgDimEntityHelpData_c` (`dimentityhelp`): a dimension's helper data on a
/// spline operand: f64 (a curve parameter), u32 0.
pub(crate) fn dimentityhelp(r: &mut Reader<'_>) -> Res<V> {
    let t = r.f64()?;
    r.expect_zeros(4, "dim entity help u32 0")?;
    Ok(V::from(t))
}

/// `moDimText_c` (`dimtext`).
pub(crate) fn dimtext(r: &mut Reader<'_>) -> Res<V> {
    let t = r.cstring()?;
    let pos = f64s(r, 3)?;
    r.u32()?;
    let fmt = r.ptr(None)?;
    if fmt.is_null() {
        outlines(r)?;
    }
    r.u32()?;
    r.u32()?;
    let m = r.u32()?;
    if m > 4096 {
        return Err(bad("text character count"));
    }
    if m != 0 {
        for _ in 0..m * 3 {
            r.f64()?;
        }
        for _ in 0..m {
            r.u32()?;
        }
        r.expect_zeros(8, "text 8 zero")?;
        for _ in 0..m * 3 {
            r.f64()?;
        }
    }
    r.f64()?;
    r.f64()?;
    r.expect_zeros(18, "text 18 zero")?;
    if r.f64()? != 1.0 {
        return Err(bad("text f64 1"));
    }
    r.expect_zeros(4, "text u32 0")?;
    if r.eq(r.p, &[0xff, 0xfe, 0xff]) {
        r.cstring()?;
        r.expect_bytes(&[1, 0, 0, 0, 1, 0, 0, 0], "text 1 1")?;
        return Ok(V::map([("text", V::Str(t)), ("pos", pos)]));
    }
    r.st.sk.byref = true;
    let path = r.ptr(None);
    r.st.sk.byref = false;
    let path = path?;
    for _ in 0..6 {
        r.f64()?;
    }
    let q = r.p;
    let mut z = 0;
    while z < 16 && r.byte(q + z)? == 0 {
        z += 1;
    }
    let a = if z != 0 { i64::from(r.pu32(q + z)?) } else { 0 };
    if z == 0 || r.byte(q + z + 4)? != 0 || i64::from(r.pu32(q + z + 5)?) != a - 5 {
        return Err(bad("text path: no body"));
    }
    if z != 10 {
        return Err(bad("text path zero bytes"));
    }
    r.p = q + 9;
    blob(r)?;
    r.cstring()?;
    r.expect_bytes(&[1, 0, 0, 0, 1, 0, 0, 0], "text 1 1")?;
    Ok(V::map([
        ("text", V::Str(t)),
        ("pos", pos),
        ("path", V::from(path)),
    ]))
}

/// `utCharFormat_c` (`charformat`).
pub(crate) fn charformat(r: &mut Reader<'_>) -> Res<V> {
    let f = r.cstring()?;
    let height = r.f64()?;
    for _ in 0..7 {
        r.f64()?;
    }
    let mut o = outlines(r)?;
    o.set("font", V::Str(f));
    o.set("height", V::from(height));
    Ok(o)
}

/// `outlines(r)`: a character format's flag and four f64, then the glyph
/// outlines.
fn outlines(r: &mut Reader<'_>) -> Res<V> {
    if r.u32()? > 1 && r.u32()? != 1 {
        return Err(bad("char format flag"));
    }
    for _ in 0..4 {
        r.f64()?;
    }
    let n = r.u32()?;
    if n > 100_000 {
        return Err(bad("glyph loop count"));
    }
    let mut s: u64 = 0;
    for _ in 0..n {
        s += u64::from(r.u32()?);
    }
    if s > 1_000_000 {
        return Err(bad("glyph segment count"));
    }
    for _ in 0..s {
        r.u32()?;
    }
    let mut p: u64 = 0;
    for _ in 0..s {
        p += u64::from(r.u32()?);
    }
    let left = r.len() as i128 - r.p as i128;
    if i128::from(p) * 24 > left {
        return Err(bad("glyph point count"));
    }
    r.skip(24 * p as usize);
    Ok(V::map([
        ("loops", V::from(n)),
        ("segments", V::Int(s as i64)),
        ("points", V::Int(p as i64)),
    ]))
}

/// `sgEntHandleData_c` (`enthandledata`): a block's entity-handle table. u32 `0`, a u32, 22-byte entries (a
/// handle class token and 20 bytes of data) until a null token, then a zero run, u32 `3` and either a
/// compressed body or u32 `1`, seven bytes and a u64 (100000 in the sample).
fn enthandledata(r: &mut Reader<'_>) -> Res<()> {
    if r.u32()? != 0 {
        return Err(bad("entity handle data u32 0"));
    }
    r.u32()?;
    loop {
        let w = r.pu16(r.p)?;
        if w == 0 {
            break;
        }
        let handle = r
            .known
            .get(&u32::from(w & 0x7fff))
            .is_some_and(|name| name.ends_with("Handle"));
        if w & 0x8000 == 0 || !handle {
            return Err(bad(format!("entity handle token {w:#x}")));
        }
        r.skip(22);
    }
    let limit = (r.p + 80).min(r.len());
    let mut j = r.p;
    while j + 4 <= limit && !r.eq(j, &[3, 0, 0, 0]) {
        j += 1;
    }
    if j + 4 > limit {
        return Err(bad("entity handle tail 3"));
    }
    if !r.zeros(r.p, j - r.p) {
        return Err(bad("entity handle tail zeros"));
    }
    r.p = j + 4;
    if r.byte(r.p)? == 0 {
        let a = r.pu32(r.p + 1)?;
        let b = r.pu32(r.p + 6)?;
        if i64::from(b) == i64::from(a) - 5 {
            blob(r)?;
            return Ok(());
        }
    }
    if r.u32()? != 1 {
        return Err(bad("entity handle tail 1"));
    }
    r.expect_zeros(7, "entity handle 7 zero")?;
    r.u64()?;
    Ok(())
}

/// `sgBlock` (`sgblock`): a block's geometry in a sketch. u32 `0` and the instance identifier, the sketch-block
/// instance record (marker prefix, f64 `1` at +48, eight zero bytes, nine f64 at +64, the flags at +136, the origin
/// at +144, the scale at +168, the mirror word at +176 and the box triples at +184, +208 and +232), one f64, u32 `0`,
/// a count and that many (block, entity, sketch entity) triples, u32 `1`, a point handle, the entity-handle table,
/// then the block's plane and its profile feature.
pub(crate) fn sgblock(r: &mut Reader<'_>) -> Res<V> {
    if r.u32()? != 0 {
        return Err(bad("sgBlock u32 0"));
    }
    let iid = r.u32()?;
    if !isattr(r, r.p) {
        return Err(bad("sgBlock instance marker"));
    }
    r.skip(17);
    r.u32()?;
    r.expect_bytes(&[0, 0], "sgBlock head zeros")?;
    for _ in 0..4 {
        r.u16()?;
    }
    if !r.eq(r.p, &[0x00, 0x00, 0x80, 0xbf]) {
        return Err(bad("sgBlock instance f32"));
    }
    r.skip(4);
    r.ptr(None)?;
    r.u16()?;
    r.skip(9);
    r.f64()?;
    r.expect_zeros(8, "sgBlock instance 8 zero")?;
    for _ in 0..9 {
        r.f64()?;
    }
    r.u32()?;
    r.u32()?;
    for _ in 0..3 {
        r.f64()?;
    }
    r.f64()?;
    r.skip(8);
    for _ in 0..9 {
        r.f64()?;
    }
    r.f64()?;
    if r.u32()? != 0 {
        return Err(bad("sgBlock list u32 0"));
    }
    let n = r.u32()?;
    if n > 4096 {
        return Err(bad("sgBlock triples"));
    }
    for _ in 0..n {
        r.u32()?;
        r.u32()?;
        r.u32()?;
    }
    r.u32()?;
    r.ptr(Some(&["sgPointHandle"]))?;
    if !r.eq(r.p, &[0xff, 0xff]) {
        return Err(bad("sgBlock handle data declaration"));
    }
    r.skip(2);
    r.u16()?;
    let len = usize::from(r.u16()?);
    let name = r.slice(r.p, len).to_vec();
    r.skip(len);
    if name.as_slice() != b"sgEntHandleData_c" {
        return Err(bad("sgBlock handle data class"));
    }
    let class = Arc::<str>::from("sgEntHandleData_c");
    r.known.insert(r.counter, class.clone());
    r.counter += 1;
    r.objs.insert(r.counter, class);
    r.counter += 1;
    enthandledata(r)?;
    r.ptr(None)?;
    r.ptr(Some(&["moProfileFeature_c"]))?;
    Ok(V::map([
        ("id", V::from(i64::from(iid))),
        ("n", V::from(i64::from(n))),
    ]))
}

/// `moCompRefAxis_c`, `moCompRefPoint_c` (`comprefaxis`).
pub(crate) fn comprefaxis(r: &mut Reader<'_>) -> Res<V> {
    r.obj(Some(UCS))?;
    let fid = r.u32()?;
    r.u32()?;
    let Some(j) = r.find(&m3(), r.p, r.p + 40) else {
        return Err(bad("reference axis: no handle block"));
    };
    r.p = j + 12;
    r.expect_zeros(4, "axis u32 0")?;
    r.u32()?;
    if r.eq(r.p, &zeros_then(12, &[0xff; 4])) {
        r.skip(16);
    } else {
        r.expect_zeros(12, "axis 12 zero")?;
    }
    let k = (r.p..r.p + 400).find(|&q| objat(r, q));
    match k {
        Some(k) if k - r.p <= 200 => {
            r.p = k;
            Ok(V::map([("fid", V::from(fid))]))
        }
        _ => Err(bad("reference axis geometry")),
    }
}
