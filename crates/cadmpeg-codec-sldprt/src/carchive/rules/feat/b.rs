//! `g_feat.py` rules (part b): the functions defined from line 730 on, except
//! `geo_frame` and `oldgeo` (in `core`).

#[allow(unused_imports)]
use super::*;

use super::super::super::reader::{bad, Ptr, Reader, Res, V};
use super::super::{ctx, sketch};

/// Part b's rule state.
#[derive(Debug, Clone, Default)]
pub(crate) struct BState {}

/// `DISPLAY` (`g_feat.py` line 167): the display-dimension classes.
const DISPLAY: &[&str] = &[
    "moDisplayDistanceDim_c",
    "moDisplayRadialDim_c",
    "moDisplayAngularDim_c",
    "moDisplayDiameterDim_c",
    "moDisplayRevolveDim_c",
    "moDisplayCircularPattCntDim_c",
    "moDisplayLinearPattCntDim_c",
    "moDisplayScalarDim_c",
    "moDisplayDim_c",
];

/// `FILLET_END`: a fillet's constant 40-byte end block.
const FILLET_END: [u8; 40] = [
    0x06, 0x00, 0x00, 0x00, 0x00, 0x00, 0x30, 0x52, 0x00, 0x00, 0x19, 0x00, //
    0x00, 0x00, 0x2d, 0x5b, 0x00, 0x00, 0xb7, 0x58, 0x00, 0x00, 0xd1, 0x59, //
    0x00, 0x00, //
    0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
];

const FFFEFF: [u8; 3] = [0xff, 0xfe, 0xff];
const PMARK: &[&str] = &["moPMarkRecord_c"];
const SUOBLIST: &[&str] = &["suObList"];
const EF: &[&str] = &["moCompEdge_c", "moCompFace_c"];
const BLOB_MARK: [u8; 4] = [0x23, 0x1d, 0xd5, 0x71];

fn list(values: Vec<V>) -> V {
    V::List(values)
}

/// `bytes(n) + b'\xff' * f + bytes(t)`.
fn zff(n: usize, f: usize, t: usize) -> Vec<u8> {
    let mut out = vec![0; n];
    out.extend(std::iter::repeat_n(0xff, f));
    out.extend(std::iter::repeat_n(0, t));
    out
}

fn ptrs(r: &mut Reader<'_>, n: usize) -> Res<Vec<Ptr>> {
    let mut out = Vec::new();
    for _ in 0..n {
        out.push(r.ptr(None)?);
    }
    Ok(out)
}

fn f64s(r: &mut Reader<'_>, n: usize) -> Res<Vec<V>> {
    let mut out = Vec::new();
    for _ in 0..n {
        out.push(V::from(r.f64()?));
    }
    Ok(out)
}

fn u32s(r: &mut Reader<'_>, n: u32) -> Res<Vec<V>> {
    let mut out = Vec::new();
    for _ in 0..n {
        out.push(V::from(r.u32()?));
    }
    Ok(out)
}

/// The display objects at the read position (`while isclass(r,r.p,DISPLAY)`).
fn displays(r: &mut Reader<'_>, dims: &mut Vec<Ptr>) -> Res<()> {
    while isclass(r, r.p, DISPLAY)? {
        dims.push(r.ptr(None)?);
    }
    Ok(())
}

/// `hd.get('x') == 1`.
fn hd_x_is_1(hd: &V) -> bool {
    hd.get("x").as_int() == Some(1)
}

fn len_v(n: usize) -> V {
    V::Int(i64::try_from(n).unwrap_or(i64::MAX))
}

/// `moEndSpec_c`.
pub(crate) fn endspec(r: &mut Reader<'_>) -> Res<V> {
    r.skip(16);
    let typ = r.u32()?;
    let _v = r.u32()?;
    if matches!(typ, 0 | 5 | 6 | 9) {
        return Ok(list(vec![V::from("blind-ish"), V::from(typ)]));
    }
    if matches!(typ, 1 | 2) {
        r.skip(2);
        if is76(r, r.p) && is76(r, r.p + 76) {
            r.skip(76);
        }
        return Ok(list(vec![V::from("depth"), V::from(typ)]));
    }
    if typ == 4 && r.eq(r.p, &[0, 0, 0, 0, 1]) {
        r.skip(5);
        let n = r.u16()?;
        for _ in 0..n {
            r.ptr(None)?;
            sketch::support(r, 1)?;
        }
        let mut found = false;
        for l in [5_usize, 3] {
            if r.eq(r.p + l + 4, &[0x00, 0x01, 0x00, 0x00]) {
                r.skip(l);
                found = true;
                break;
            }
        }
        if !found {
            return Err(bad("up-to-surface tail"));
        }
        return Ok(list(vec![V::from("up-to-surface"), V::from(n)]));
    }
    if typ == 3 && r.zeros(r.p, 4) {
        r.skip(4);
        let rf = r.ptr(None)?;
        let class = rf.obj_class().map(str::to_owned);
        return Ok(list(vec![V::from("up-to-vertex"), V::from(class)]));
    }
    Err(bad(format!("endspec type {typ}")))
}

/// `moFromEndSpec_c`.
pub(crate) fn fromendspec(r: &mut Reader<'_>) -> Res<V> {
    if !r.zeros(r.p, 4) && objat(r, r.p + 4) {
        let t = r.u32()?;
        let _d = r.ptr(None)?;
        if t == 5 {
            r.u32()?;
            r.expect_zeros(12, "fromend offset 12 zero")?;
        } else if t == 1 && r.zeros(r.p, 10) {
            refblock(r)?;
            f64s(r, 4)?;
            r.expect_zeros(4, "fromend face u32 0")?;
            r.u32()?;
            r.u32()?;
            let tail = if ver_or(r, 99999) >= 16000 { 16 } else { 12 };
            let z = if r.zeros(r.p, 16) && objat(r, r.p + 16 + tail) && !objat(r, r.p + 20 + tail) {
                16
            } else {
                20
            };
            r.expect_zeros(z, "fromend zeros")?;
        } else if t == 1
            && r.eq(r.p, &[0, 0, 1, 0])
            && isclass(r, r.p + 4, &["moContextChainComponent_c"])?
        {
            sketch::support(r, 1)?;
            r.expect(&[0, 0], "fromend context u16 0")?;
            r.u32()?;
            r.u32()?;
            r.expect_zeros(16, "fromend context zeros")?;
        } else {
            return Err(bad("fromend form"));
        }
    } else {
        r.expect_zeros(20, "fromend zeros")?;
    }
    if r.ver.is_none_or(|v| v >= 16000) {
        r.u32()?;
    }
    let off = r.f64()?;
    r.u32()?;
    Ok(V::from(off))
}

/// The display-dimension classes (`displaydim`).
pub(crate) fn displaydim(r: &mut Reader<'_>) -> Res<V> {
    r.favflag = None;
    r.skip(151);
    let k = r.u32()?;
    if k > 4096 {
        return Err(bad("display id count"));
    }
    let ids = u32s(r, k)?;
    r.skip(if ver_or(r, 99999) < 14000 { 109 } else { 261 });
    if r.eq(r.p, &FFFEFF) {
        r.cstring()?;
    } else {
        return Err(bad("display dim: no string (pre-14000 form)"));
    }
    r.skip(132);
    r.ptr(None)?;
    r.ptr(None)?;
    if let Some(fp) = r.favflag.take() {
        if r.depth == 1 {
            // A top-level display dimension carries the view block after its handle.
            if !(r.pu32(fp)? == 0 && r.pu32(fp + 4)? == 8) {
                return Err(bad("display dim view prefix"));
            }
            let mut found = None;
            let mut j = fp;
            while j < fp + 600 && j + 4 < r.len() {
                let w = r.pu16(j)?;
                if w & 0x8000 != 0
                    && r.known
                        .get(&u32::from(w & 0x7fff))
                        .is_some_and(|c| DISPLAY.contains(&&**c))
                {
                    found = Some(j);
                    break;
                }
                j += 2;
            }
            let Some(j) = found else {
                return Err(bad("display dim view: no next display"));
            };
            r.p = j;
        }
    } else if r.depth == 1 {
        // A top-level display dimension's placement data before the next object: an empty CString, u32 values and
        // zero bytes; the next object is the first class tag with a rule in the 64 bytes that follow. Numeric
        // placement data (f64 and u32 values, no CString) ends at the next display object within 128 bytes.
        let end = (r.p + 64).min(r.len());
        let has_marker = r.b[r.p.min(end)..end]
            .windows(3)
            .any(|w| w == [0xff, 0xfe, 0xff]);
        if has_marker {
            let mut j = r.p;
            while j < r.p + 64 && j + 4 < r.len() {
                let w = r.pu16(j)?;
                if w & 0x8000 != 0 {
                    if let Some(c) = r.known.get(&u32::from(w & 0x7fff)) {
                        if &**c != "moNodeName_c" && super::super::rule(c).is_some() {
                            r.p = j;
                            break;
                        }
                    }
                }
                j += 1;
            }
        } else {
            let mut j = r.p;
            while j < r.p + 128 && j + 4 < r.len() {
                let w = r.pu16(j)?;
                if w & 0x8000 != 0
                    && r.known
                        .get(&u32::from(w & 0x7fff))
                        .is_some_and(|c| DISPLAY.contains(&&**c))
                {
                    r.p = j;
                    break;
                }
                j += 1;
            }
        }
    }
    Ok(list(ids))
}

/// `dimhandle_prefix(r)`.
fn dimhandle_prefix(r: &mut Reader<'_>) -> Res<()> {
    r.skip(42);
    r.expect(&[0xff; 16], "dim handle ff")?;
    r.skip(20);
    if isver(r, r.p) {
        r.skip(4);
    }
    r.skip(if ver_or(r, 99999) < 12000 {
        2
    } else if ver_or(r, 99999) < 13000 {
        10
    } else {
        14
    });
    r.expect(&[0x00, 0xff, 0xff, 0xff, 0xff], "dim handle -1")?;
    r.expect_zeros(6, "dim handle 6 zero")?;
    Ok(())
}

/// The sketch dimension handles (`skdimhandle`).
pub(crate) fn skdimhandle(r: &mut Reader<'_>) -> Res<V> {
    let c = r.last_class();
    dimhandle_prefix(r)?;
    r.skip(12);
    let mut strs = Vec::new();
    for _ in 0..4 {
        strs.push(V::from(r.cstring()?));
    }
    let count = c.as_deref().is_some_and(|c| {
        c == "moSkDimHandleLinearPattCnt_c" || c == "moSkDimHandleCircularPattCnt_c"
    });
    r.skip(if count { 282 } else { 310 });
    Ok(list(strs))
}

/// The placement block a back-referenced dimension carries: four
/// `CString`s, 234 or 262 bytes, a `CString`, 40 bytes, a `CString`.
fn placement(r: &mut Reader<'_>) -> Res<()> {
    for _ in 0..4 {
        r.cstring()?;
    }
    let short = r.eq(r.p + 234, &FFFEFF) && !r.eq(r.p + 262, &FFFEFF);
    r.skip(if short { 234 } else { 262 });
    r.cstring()?;
    r.skip(40);
    r.cstring()?;
    Ok(())
}

/// `moFeatureDimHandle_c`.
pub(crate) fn featdimhandle(r: &mut Reader<'_>) -> Res<V> {
    dimhandle_prefix(r)?;
    let d = r.ptr(None)?;
    if d.is_ref() && r.eq(r.p + 6, &FFFEFF) {
        r.skip(6);
        placement(r)?;
    }
    Ok(V::from(d))
}

/// The feature dimensions (`ppdd`).
pub(crate) fn ppdd(r: &mut Reader<'_>) -> Res<V> {
    r.u32()?;
    r.st.core.featdim.push(true);
    let p = r.ptr(None);
    r.st.core.featdim.pop();
    let p = p?;
    if p.is_ref() {
        let Some(j) = r.find(&FFFEFF, r.p, r.p + 1200) else {
            return Err(bad("feature dim (back-reference): no placement"));
        };
        r.p = j;
        placement(r)?;
    }
    Ok(V::from(p))
}

/// `favblock(b, p)`: the length of a dimension's view block at `p`.
fn favblock(r: &Reader<'_>, p: usize) -> Option<usize> {
    let b = r.b;
    if p + 122 > b.len()
        || !(r.zeros(p, 4) || r.eq(p, &[1, 0, 0, 0]))
        || !r.zeros(p + 76, 8)
        || b[p + 84] > 1
    {
        return None;
    }
    let n = if b[p + 84] != 0 { 12 } else { 3 };
    let q = p + 85 + 8 * n;
    if q + 13 > b.len() || r.pf64(q).ok()? != 1.0 || !r.eq(q + 8, &[0, 1, 0, 0, 0]) {
        return None;
    }
    Some(q + 13 - p)
}

/// `moFavoriteHandle_c`.
pub(crate) fn favhandle(r: &mut Reader<'_>) -> Res<V> {
    r.skip(206);
    let mut s = Vec::new();
    for _ in 0..4 {
        s.push(V::from(r.cstring()?));
    }
    r.skip(92);
    s.push(V::from(r.cstring()?));
    s.push(V::from(r.cstring()?));
    r.skip(12);
    s.push(V::from(r.cstring()?));
    r.skip(40);
    s.push(V::from(r.cstring()?));
    if r.trace.len() > 1 && r.trace[r.trace.len() - 2].class.ends_with("PattCnt_c") {
        return Ok(list(s));
    }
    if objat(r, r.p) {
        return Ok(list(s));
    }
    if r.eq(r.p, &[0, 0]) && objat(r, r.p + 2) {
        r.skip(2);
        return Ok(list(s));
    }
    for l in [54_usize, 52, 50] {
        if r.zeros(r.p, l) && is76(r, r.p + l) {
            r.skip(l);
            return Ok(list(s));
        }
    }
    let fl = r.u32()?;
    if fl == 0 && r.p + 8 <= r.len() && r.pu32(r.p)? == 8 && r.pu32(r.p + 4)? == 0x0100_0000 {
        // A view block follows; a top-level display dimension consumes it (`displaydim`).
        r.favflag = Some(r.p - 4);
    }
    if (fl == 0 && objat(r, r.p)) || isclass(r, r.p, DISPLAY)? {
        return Ok(list(s));
    }
    if fl & 0x10080 == 0 && r.zeros(r.p, 6) && r.eq(r.p + 6, &FFFEFF) {
        let q = r.p;
        r.skip(6);
        s.push(V::from(r.cstring()?));
        if objat(r, r.p + 28) {
            r.skip(28);
            return Ok(list(s));
        }
        r.p = q;
    }
    let l = if ver_or(r, 99999) < 15000 && fl & 0x10080 == 0 {
        46
    } else {
        50
    };
    let short = if fl & 0x10080 == 0 && !objat(r, r.p + l) {
        [28_usize, 30].into_iter().find(|&k| objat(r, r.p + k))
    } else {
        None
    };
    r.skip(short.unwrap_or(l));
    if fl & 0x10000 != 0 {
        r.skip(35);
        if fl & 1 != 0 && r.eq(r.p, &[0, 0, 0, 0, 0xff, 0xfe, 0xff]) {
            r.skip(4);
            s.push(V::from(r.cstring()?));
        }
    }
    if fl & 0x80 != 0 {
        r.skip(52);
        s.push(V::from(r.cstring()?));
        let mut l = if ver_or(r, 0) >= 16000 || r.ver.is_none() {
            18
        } else {
            14
        };
        if ver_or(r, 99999) < 12000 {
            for l2 in [12_usize, 13, 14] {
                if r.zeros(r.p, l2) && objat(r, r.p + l2) {
                    l = l2;
                    break;
                }
            }
        }
        r.skip(l);
    }
    if fl & 0x8800 == 0x8800 {
        let Some(n) = favblock(r, r.p) else {
            return Err(bad("favourite handle view block"));
        };
        r.skip(n);
    }
    Ok(list(s))
}

/// `moBaseBody_c`.
pub(crate) fn basebody(r: &mut Reader<'_>) -> Res<V> {
    feature(r, "moBaseBody_c")?;
    bodyhead(r, "moBaseBody_c")?;
    r.ptr(Some(PMARK))?;
    let mut nm = V::None;
    if r.eq(r.p, &FFFEFF) {
        r.cstring()?;
        r.u32()?;
        nm = V::from(r.cstring()?);
    }
    if r.p < r.len() {
        r.expect_zeros(4, "base body u32 0")?;
    }
    Ok(V::map([("name", nm)]))
}

/// The zero bytes before a fillet's or chamfer's lists.
fn lists_lead(r: &mut Reader<'_>) -> Res<()> {
    let mut z = 0;
    while z < 16 && r.byte(r.p + z)? == 0 {
        z += 1;
    }
    if 0 < z && z < 16 && isclass(r, r.p + z, SUOBLIST)? {
        r.skip(z);
    }
    Ok(())
}

/// `Fillet_c`.
pub(crate) fn fillet(r: &mut Reader<'_>) -> Res<V> {
    let n = feature(r, "Fillet_c")?;
    r.skip(58);
    r.ptr(Some(PMARK))?;
    lists_lead(r)?;
    let mut lists: Vec<Ptr> = Vec::new();
    loop {
        let w = r.pu16(r.p)?;
        let is_list = (w & 0x8000 != 0
            && w != 0xffff
            && r.known
                .get(&u32::from(w & 0x7fff))
                .is_some_and(|c| &**c == "suObList"))
            || (w == 0xffff && r.eq(r.p + 6, b"suObList"));
        if !is_list {
            if !lists.is_empty() && isclass(r, r.p, &["moCompFace_c"])? {
                lists.push(r.ptr(None)?);
                if r.eq(r.p, &[0, 0]) && isclass(r, r.p + 2, SUOBLIST)? {
                    r.skip(2);
                }
                continue;
            }
            break;
        }
        lists.push(r.ptr(None)?);
    }
    r.u32()?;
    r.u32()?;
    r.ptr(Some(SUOBLIST))?;
    r.expect(&[0, 0], "fillet u16 0")?;
    r.ptr(Some(SUOBLIST))?;
    loop {
        let w = r.pu16(r.p)?;
        if w < 256 && isclass(r, r.p + 2, EF)? {
            r.skip(2);
            continue;
        }
        if r.zeros(r.p, 62) && r.pu16(r.p + 62)? < 256 && isclass(r, r.p + 64, EF)? {
            r.skip(62);
            continue;
        }
        if isclass(r, r.p, EF)? {
            r.ptr(None)?;
            continue;
        }
        break;
    }
    let mut z = 0;
    while r.byte(r.p + z)? == 0 {
        z += 1;
    }
    if !r.ffs(r.p + z, 8) {
        return Err(bad("fillet ff block"));
    }
    r.skip(z + 8);
    if r.zeros(r.p, 4) && !r.zeros(r.p + 4, 4) {
        r.skip(8);
    } else {
        r.expect_zeros(8, "fillet 8 zero")?;
    }
    let k = r.u32()?;
    if k > 4096 {
        return Err(bad("fillet entry count"));
    }
    for _ in 0..k {
        r.u32()?;
        r.u32()?;
        r.f64()?;
        r.ptr(Some(EF))?;
        if r.zeros(r.p, 62) {
            r.skip(62);
        }
    }
    if !r.eq(r.p, &FILLET_END) {
        return Err(bad("fillet end block"));
    }
    r.skip(40);
    Ok(n)
}

/// `Chamfer_c`.
pub(crate) fn chamfer(r: &mut Reader<'_>) -> Res<V> {
    let n = feature(r, "Fillet_c")?;
    r.skip(58);
    r.ptr(Some(PMARK))?;
    lists_lead(r)?;
    while isclass(r, r.p, SUOBLIST)? {
        r.ptr(None)?;
    }
    let mut dims = Vec::new();
    displays(r, &mut dims)?;
    if isclass(r, r.p, EF)? {
        let save = r.p;
        r.ptr(None)?;
        if !(r.eq(r.p, &[0, 0]) && isclass(r, r.p + 2, SUOBLIST)?) {
            r.p = save;
        }
    }
    if r.eq(r.p, &[0, 0]) && isclass(r, r.p + 2, SUOBLIST)? {
        r.skip(2);
        r.ptr(None)?;
        displays(r, &mut dims)?;
    }
    let pre = if r.eq(r.p, &[0, 0])
        && r.byte(r.p + 2)? <= 1
        && r.eq(r.p + 3, &[0x0a, 0x01])
        && objat(r, r.p + 5)
    {
        Some(5)
    } else if isclass(r, r.p, EF)? {
        Some(0)
    } else {
        None
    };
    if let Some(pre) = pre {
        r.skip(pre);
        r.ptr(None)?;
        r.expect_zeros(66, "chamfer 66 zero")?;
        r.expect(&[1, 0, 0, 0], "chamfer u32 1")?;
        if !r.eq(r.p, &[6, 0, 0, 0, 0, 0]) || !r.zeros(r.p + 26, 14) {
            return Err(bad("chamfer end block"));
        }
        r.skip(40);
        if r.zeros(r.p, 6) && objat(r, r.p + 6) {
            r.skip(6);
            return Ok(n);
        }
        r.expect(&[1, 0, 0, 0, 0, 1], "chamfer 1 0 1")?;
        if r.zeros(r.p, 4) && objat(r, r.p + 4) {
            r.skip(4);
        }
        return Ok(n);
    }
    Err(bad("chamfer form not read (EV-17)"))
}

/// `edgeRadiusObject_c`.
pub(crate) fn edgeradius(r: &mut Reader<'_>) -> Res<V> {
    let d = r.ptr(None)?;
    r.ptr(None)?;
    if r.zeros(r.p, 66) {
        r.skip(66);
    } else {
        if !(r.byte(r.p)? == 0 && r.zeros(r.p + 5, 64)) {
            return Err(bad("radius 66 zero"));
        }
        r.skip(69);
    }
    if objat(r, r.p) && isclass(r, r.p, &["edgeRadiusObject_c"])? {
        return Ok(V::map([("dim", V::from(d)), ("n", V::Int(0))]));
    }
    if r.u8()? > 1 {
        return Err(bad("radius byte 1"));
    }
    r.ptr(Some(SUOBLIST))?;
    r.skip(58);
    let n = r.u32()?;
    for _ in 0..n {
        r.ptr(None)?;
    }
    Ok(V::map([("dim", V::from(d)), ("n", V::from(n))]))
}

/// `moSavedExtBody_c`.
pub(crate) fn savedext(r: &mut Reader<'_>) -> Res<V> {
    let n = feature(r, "moSavedExtBody_c")?;
    bodyhead(r, "moSavedExtBody_c")?;
    r.ptr(Some(PMARK))?;
    r.expect_zeros(16, "saved body 16 zero")?;
    r.ptr(Some(&["moSolidRef_w"]))?;
    let k = r.u32()?;
    if k > 4096 {
        return Err(bad("saved body ids"));
    }
    u32s(r, k)?;
    r.expect(&zff(0, 4, 4), "saved body -1 0")?;
    r.u32()?;
    r.u32()?;
    r.u32()?;
    r.u32()?;
    ctx::xform(r)?;
    r.expect_zeros(10, "saved body 10 zero")?;
    u32s(r, 4)?;
    if isver(r, r.p) {
        r.skip(4);
    }
    r.expect(&zff(12, 4, 4), "saved body -1")?;
    let count = r.u32()?;
    let mut bodies = 0_i64;
    for _ in 0..count {
        sketch::blob(r)?;
        bodies += 1;
    }
    if r.eq(r.p, &[1, 0]) {
        r.skip(2);
        r.ptr(None)?;
        f64s(r, 9)?;
        r.u32()?;
        f64s(r, 3)?;
        r.u32()?;
        if r.f64()? != 1.0 {
            return Err(bad("saved body scale"));
        }
        r.expect_zeros(55, "saved body 55 zero")?;
        let cfg = r.cstring()?;
        r.expect_zeros(4, "saved body 4 zero")?;
        // `r.b[r.p:] == bytes(4)`: exactly four zero bytes remain.
        if r.p <= r.len() && r.len() - r.p == 4 && r.zeros(r.p, 4) {
            r.skip(4);
        }
        return Ok(V::map([
            ("n", n),
            ("bodies", V::Int(bodies)),
            ("cfg", V::from(cfg)),
        ]));
    }
    Ok(V::map([("n", n), ("bodies", V::Int(bodies))]))
}

/// `moSolidRef_w`.
pub(crate) fn solidref(r: &mut Reader<'_>) -> Res<V> {
    Ok(V::from(r.ptr(None)?))
}

/// `moContextUnitComponent_c`.
pub(crate) fn ctxunit(r: &mut Reader<'_>) -> Res<V> {
    if r.u16()? != 2 {
        return Err(bad("context unit u16 2"));
    }
    r.u16()?;
    r.u32()?;
    let cfg = r.cstring()?;
    r.skip(44);
    r.expect(&[0xff; 16], "context unit ff")?;
    r.skip(20);
    if isver(r, r.p) {
        r.skip(4);
    }
    Ok(V::from(cfg))
}

/// `moThinSpec_c`.
pub(crate) fn thinspec(r: &mut Reader<'_>) -> Res<V> {
    r.ptr(None)?;
    r.ptr(None)?;
    r.u32()?;
    let th = r.f64()?;
    r.f64()?;
    r.skip(8);
    r.ptr(None)?;
    for z in [12_usize, 8, 10] {
        if r.zeros(r.p, z) && (z == 12 || objat(r, r.p + z)) {
            r.skip(z);
            return Ok(V::from(th));
        }
    }
    Err(bad("thin 12 zero"))
}

/// `moMBimport_c`.
pub(crate) fn mbimport(r: &mut Reader<'_>) -> Res<V> {
    feature(r, "moBaseBody_c")?;
    bodyhead(r, "moBaseBody_c")?;
    r.ptr(Some(PMARK))?;
    let name = r.cstring()?;
    let path = r.cstring()?;
    r.u32()?;
    let k = r.u32()?;
    let mut maps = 0_usize;
    for _ in 0..k {
        r.u32()?;
        r.ptr(Some(&["mbBodyImportIDMaps_c"]))?;
        maps += 1;
    }
    r.u32()?;
    r.u16()?;
    r.u16()?;
    r.expect(&[0, 0], "mbimport u16 0")?;
    let ft = r.u64()?;
    let fmt = r.cstring()?;
    let schema = r.cstring()?;
    r.expect_zeros(4, "mbimport u32 0")?;
    r.u32()?;
    r.expect_zeros(4, "mbimport u32 0 b")?;
    Ok(V::map([
        ("name", V::from(name)),
        ("path", V::from(path)),
        ("maps", len_v(maps)),
        ("format", V::from(fmt)),
        ("schema", V::from(schema)),
        ("filetime", V::Int(i64::try_from(ft).unwrap_or(i64::MAX))),
    ]))
}

/// `mbBodyImportIDMaps_c`.
pub(crate) fn idmaps(r: &mut Reader<'_>) -> Res<V> {
    let n = r.u32()?;
    let mut out = Vec::new();
    for _ in 0..n {
        let s = r.cstring()?;
        let a = r.u32()?;
        let b = r.u32()?;
        out.push(list(vec![V::from(s), V::from(a), V::from(b)]));
    }
    Ok(list(out))
}

/// `moDimTolerance_c`.
pub(crate) fn dimtol(r: &mut Reader<'_>) -> Res<V> {
    let kind = r.u32()?;
    r.u32()?;
    let lo = r.f64()?;
    let hi = r.f64()?;
    r.u8()?;
    f64s(r, 3)?;
    r.cstring()?;
    r.cstring()?;
    r.u8()?;
    r.u32()?;
    r.u8()?;
    f64s(r, 3)?;
    r.expect_zeros(6, "tolerance 6 zero")?;
    for _ in 0..3 {
        r.cstring()?;
    }
    r.expect_zeros(4, "tolerance u32 0")?;
    Ok(V::map([
        ("kind", V::from(kind)),
        ("limits", list(vec![V::from(lo), V::from(hi)])),
    ]))
}

/// `moConstraintRefplaneInterfaceData_c`.
pub(crate) fn planeiface(r: &mut Reader<'_>) -> Res<V> {
    Ok(V::from(ptrs(r, 3)?))
}

/// The constraint-plane geometry classes (`planegeom`).
pub(crate) fn planegeom(r: &mut Reader<'_>) -> Res<V> {
    r.st.sk.mirror = true;
    let rf = r.ptr(None);
    r.st.sk.mirror = false;
    let rf = rf?;
    if !rf.is_null() && objat(r, r.p) {
        // A line reference whose slot ends at once: the next geometry object follows.
        return Ok(V::map([("ref", V::from(rf)), ("v", V::None)]));
    }
    let one0 = [1, 0, 0, 0, 0, 0, 0, 0];
    let mut z0 = 0;
    while z0 < 8 && r.byte(r.p + z0)? == 0 {
        z0 += 1;
    }
    if !r.eq(r.p + z0, &one0) {
        z0 = 0;
    }
    if rf.is_obj() && r.eq(r.p + z0, &one0) {
        r.skip(z0);
        r.skip(8);
        r.u32()?;
        let mut z = 0;
        while r.byte(r.p + z)? == 0 && z < 32 {
            z += 1;
        }
        r.skip(z);
        let mut m = M12345.to_vec();
        m.extend_from_slice(&[0; 4]);
        r.expect(&m, "plane geom -12345")?;
        r.u32()?;
        r.expect(&zff(12, 4, 0), "plane geom -1")?;
        f64s(r, 4)?;
        f64s(r, 6)?;
        let fl = r.u8()?;
        if fl > 1 {
            return Err(bad("plane geom flag"));
        }
        f64s(r, if fl != 0 { 12 } else { 3 })?;
        if r.f64()? != 1.0 {
            return Err(bad("plane geom scale"));
        }
        r.expect_zeros(21, "plane geom 21 zero")?;
    }
    if !rf.is_null() {
        r.expect_zeros(4, "plane geom u32 0")?;
    }
    let a = r.f64()?;
    let b = r.f64()?;
    r.expect_zeros(24, "plane geom 24 zero")?;
    if r.eq(r.p, &[0]) && objat(r, r.p + 1) && !objat(r, r.p) {
        r.skip(1);
    }
    Ok(V::map([
        ("ref", V::from(rf)),
        ("v", list(vec![V::from(a), V::from(b)])),
    ]))
}

/// `faceRadiusObject_c`.
pub(crate) fn faceradius(r: &mut Reader<'_>) -> Res<V> {
    let d = r.ptr(None)?;
    r.ptr(None)?;
    r.expect(&[0, 0, 1], "face radius 0 1")?;
    r.ptr(Some(SUOBLIST))?;
    r.skip(58);
    let n = r.u32()?;
    if n > 4096 {
        return Err(bad("face radius count"));
    }
    for _ in 0..n {
        r.ptr(None)?;
        r.ptr(None)?;
        r.ptr(None)?;
    }
    Ok(V::map([("dim", V::from(d)), ("n", V::from(n))]))
}

/// `isblob(q)`: a compressed body's two lengths at `q` (`B == A - 5`).
fn isblob(r: &Reader<'_>, q: usize) -> Res<bool> {
    Ok(r.byte(q)? == 0
        && r.byte(q + 5)? == 0
        && i64::from(r.pu32(q + 6)?) == i64::from(r.pu32(q + 1)?) - 5)
}

/// `moHelix_c`.
pub(crate) fn helix(r: &mut Reader<'_>) -> Res<V> {
    let n = feature(r, "moHelix_c")?;
    r.expect(&[0, 0], "helix u16 0")?;
    let k = r.u16()?;
    ptrs(r, usize::from(k))?;
    let mut l = 142;
    for k in std::iter::once(142_usize).chain((136..160).step_by(2)) {
        if isblob(r, r.p + k)? && r.eq(r.p + k + 10, &BLOB_MARK) {
            l = k;
            break;
        }
    }
    r.skip(l);
    sketch::blob(r)?;
    r.expect(&[6, 0, 0, 0], "helix u32 6")?;
    r.skip(18);
    r.expect(&[1, 0, 0, 0], "helix u32 1")?;
    let v = f64s(r, 5)?;
    let mut z = 0;
    while r.byte(r.p + z)? == 0 && z < 200 {
        z += 1;
    }
    r.skip(z);
    let mut dims = Vec::new();
    displays(r, &mut dims)?;
    r.expect(&[0], "helix u8 0")?;
    Ok(V::map([
        ("n", n),
        ("values", list(v)),
        ("dims", len_v(dims.len())),
    ]))
}

/// `moCutListFolder_c`.
pub(crate) fn cutlist(r: &mut Reader<'_>) -> Res<V> {
    let n = feature(r, "moFtrFolder_c")?;
    r.expect_zeros(12, "cut list 12 zero")?;
    let bodies = r.ptr(Some(SUOBLIST))?;
    let mut tail = vec![1, 0, 0, 0, 0xff, 0xff, 0xff, 0xff, 2, 0, 0, 0];
    tail.extend_from_slice(&[0; 23]);
    tail.extend_from_slice(&[1, 0, 0, 0, 1, 0, 0, 0]);
    let mut items = Vec::new();
    while !r.eq(r.p, &tail) {
        let mut k = None;
        for q in r.p..r.p + 80 {
            if isclass(r, q, &["moCompFeature_c", "moCompSolidBody_c"])? {
                k = Some(q);
                break;
            }
        }
        let Some(k) = k else {
            return Err(bad("cut list item"));
        };
        let ff = r.slice(r.p, k - r.p).windows(4).any(|w| w == [0xff; 4]);
        if !ff || items.len() > 64 {
            return Err(bad("cut list item"));
        }
        r.p = k;
        items.push(r.ptr(None)?);
    }
    r.skip(tail.len());
    Ok(V::map([
        ("n", n),
        ("bodies", V::from(bodies)),
        ("items", V::from(items)),
    ]))
}

/// `moCutListSortOptions_c`.
pub(crate) fn cutlistsort(r: &mut Reader<'_>) -> Res<V> {
    let a = r.u32()?;
    let b = r.u16()?;
    let c = r.u16()?;
    let d = r.u16()?;
    Ok(list(vec![V::from(a), V::from(b), V::from(c), V::from(d)]))
}

/// `upto(lim)` inside `sheetmetal`: bytes to the next display object, then
/// the display objects there.
fn upto(r: &mut Reader<'_>, lim: usize) -> Res<()> {
    let mut k = None;
    for q in r.p..r.p + lim {
        if isclass(r, q, DISPLAY)? {
            k = Some(q);
            break;
        }
    }
    let Some(k) = k else {
        return Err(bad("sheet metal: no display object"));
    };
    r.p = k;
    let mut d = Vec::new();
    displays(r, &mut d)?;
    Ok(())
}

/// `moTemplateSheetMetal_c`, `moSheetMetal_c`.
pub(crate) fn sheetmetal(r: &mut Reader<'_>) -> Res<V> {
    let n = feature(r, "moTemplateSheetMetal_c")?;
    upto(r, 400)?;
    let Some(j) = r.find(&FFFEFF, r.p, r.p + 64) else {
        return Err(bad("sheet metal: no bend table"));
    };
    r.p = j;
    let table = r.cstring()?;
    r.skip(5);
    let mut d2 = Vec::new();
    displays(r, &mut d2)?;
    r.expect(&[1, 0, 0, 0, 0, 0, 0, 0], "sheet metal 1 0")?;
    for _ in 0..4 {
        r.cstring()?;
    }
    for _ in 0..2 {
        upto(r, 120)?;
    }
    Ok(V::map([("n", n), ("table", V::from(table))]))
}

/// `moEdgeFlange_c`: the feature; its block's trailing block is framed by the next named object.
pub(crate) fn edgeflange(r: &mut Reader<'_>) -> Res<V> {
    feature(r, "moEdgeFlange_c")
}

/// `moSurfaceAxisData_c`.
pub(crate) fn surfaxis(r: &mut Reader<'_>) -> Res<V> {
    let v = f64s(r, 14)?;
    let rf = r.ptr(None)?;
    if rf.obj_class() == Some("moCylinderRef_w") {
        refgeo(r, "moCylinderRef_w")?;
    } else {
        return Err(bad(format!(
            "surface axis: reference {} {}",
            rf.kind(),
            rf.class().unwrap_or("")
        )));
    }
    Ok(list(v))
}

/// `moMirrorSolid_c`.
pub(crate) fn mirrorsolid(r: &mut Reader<'_>) -> Res<V> {
    let n = feature(r, "moMirrorSolid_c")?;
    bodyhead(r, "moMirrorSolid_c")?;
    r.ptr(Some(PMARK))?;
    if !r.eq(r.p, &[0, 0, 1, 0, 0, 0, 1, 0, 0, 0]) || !r.eq(r.p + 34, &1.0_f64.to_le_bytes()) {
        return Err(bad("mirror body block"));
    }
    r.skip(60);
    let nb = r.u16()?;
    if nb > 64 {
        return Err(bad("mirror body count"));
    }
    for _ in 0..nb {
        r.ptr(None)?;
        let c = r.u32()?;
        if c > 64 {
            return Err(bad("mirror body id count"));
        }
        u32s(r, c)?;
        r.expect(&[0xff; 4], "mirror body -1")?;
        r.u32()?;
        r.u32()?;
    }
    r.expect_zeros(4, "mirror body u32 0")?;
    r.st.sk.mirror = true;
    let plane = r.ptr(None);
    r.st.sk.mirror = false;
    plane?;
    r.expect_zeros(4, "mirror body plane u32 0")?;
    r.ptr(Some(&["moMatrixIndexHelper_c"]))?;
    let c = r.u32()?;
    if c > 64 {
        return Err(bad("mirror body tail count"));
    }
    u32s(r, c)?;
    let mut tail = zff(0, 4, 8);
    tail.extend_from_slice(&[2, 0, 0, 0]);
    tail.extend_from_slice(&[0; 6]);
    tail.extend_from_slice(&[1, 1]);
    tail.extend_from_slice(&[0; 13]);
    r.expect(&tail, "mirror body tail")?;
    Ok(V::map([("n", n), ("bodies", V::from(nb))]))
}

/// `suUIntArray`.
pub(crate) fn uintarray(r: &mut Reader<'_>) -> Res<V> {
    let n = r.u32()?;
    if n > 65536 {
        return Err(bad("uint array count"));
    }
    Ok(list(u32s(r, n)?))
}

/// `moMatrixIndexHelper_c`.
pub(crate) fn matrixhelper(r: &mut Reader<'_>) -> Res<V> {
    let a = r.u16()?;
    let arr = r.ptr(Some(&["suUIntArray"]))?;
    let b = r.u32()?;
    let k = r.u16()?;
    if k > 64 {
        return Err(bad("matrix helper count"));
    }
    let objs = ptrs(r, usize::from(k))?;
    Ok(list(vec![
        V::from(a),
        V::from(arr),
        V::from(b),
        V::from(objs),
    ]))
}

/// `moHoleWzd_c`.
pub(crate) fn holewzd(r: &mut Reader<'_>) -> Res<V> {
    let n = feature(r, "moHoleWzd_c")?;
    let hd = bodyhead(r, "moHoleWzd_c")?;
    if hd_x_is_1(&hd) {
        units(r)?;
    }
    let w = r.pu16(r.p)?;
    if 0 < w && w < 64 && isclass(r, r.p + 2, &["moCompSolidBody_c"])? {
        let k = r.u16()?;
        ptrs(r, usize::from(k))?;
        let k = r.u32()?;
        if k > 4096 {
            return Err(bad("hole body ids"));
        }
        u32s(r, k)?;
        u32s(r, 4)?;
    }
    r.ptr(Some(PMARK))?;
    r.expect_zeros(6, "hole 6 zero")?;
    let d1 = r.f64()?;
    r.expect(&[1, 0, 0, 0], "hole u32 1")?;
    r.cstring()?;
    r.cstring()?;
    r.u32()?;
    r.u32()?;
    r.u32()?;
    let std = r.cstring()?;
    let ftype = r.cstring()?;
    let size = r.cstring()?;
    f64s(r, 5)?;
    r.cstring()?;
    r.u32()?;
    r.u32()?;
    let desc = r.cstring()?;
    r.cstring()?;
    r.u32()?;
    r.u32()?;
    r.cstring()?;
    r.expect_zeros(4, "hole list u32 0")?;
    let n2 = r.u32()?;
    if n2 > 4096 {
        return Err(bad("hole instance count"));
    }
    let mut inst = Vec::new();
    for _ in 0..n2 {
        let i = r.u32()?;
        let x = r.f64()?;
        inst.push(list(vec![V::from(i), V::from(x)]));
    }
    r.expect_zeros(52, "hole 52 zero")?;
    r.cstring()?;
    r.expect_zeros(12, "hole 12 zero")?;
    let mut one_one = vec![1, 0, 0, 0, 1, 0, 0, 0];
    one_one.extend_from_slice(&zff(0, 4, 8));
    r.expect(&one_one, "hole 1 1 -1")?;
    r.cstring()?;
    r.cstring()?;
    r.expect_zeros(4, "hole u32 0")?;
    r.f64()?;
    if (r.eq(r.p, &[1, 0, 0, 0]) || r.zeros(r.p, 4)) && r.zeros(r.p + 4, 12) {
        // 14000: the u32 before the zeros.
        r.skip(4);
        r.expect_zeros(12, "hole end 12 zero 14000")?;
    } else {
        r.expect_zeros(12, "hole end 12 zero")?;
        r.u32()?;
    }
    let mut z = 0;
    while r.p + z < r.len() && r.byte(r.p + z)? == 0 && z < 16 {
        z += 1;
    }
    if !(r.p + z >= r.len() || objat(r, r.p + z)) {
        return Err(bad(format!("hole end zeros {z}")));
    }
    r.skip(z);
    Ok(V::map([
        ("n", n),
        ("standard", V::from(std)),
        ("type", V::from(ftype)),
        ("size", V::from(size)),
        ("description", V::from(desc)),
        ("diameter", V::from(d1)),
        ("instances", list(inst)),
    ]))
}

/// `moMoldSrfBodyFolder_c`.
pub(crate) fn moldfolder(r: &mut Reader<'_>) -> Res<V> {
    let n = feature(r, "moMoldSrfBodyFolder_c")?;
    r.expect_zeros(12, "mold folder 12 zero")?;
    r.u32()?;
    Ok(n)
}

/// `moRefSurface_c`.
pub(crate) fn refsurface(r: &mut Reader<'_>) -> Res<V> {
    let n = feature(r, "moRefSurface_c")?;
    let k = r.u16()?;
    if k > 4096 {
        return Err(bad("ref surface count"));
    }
    ptrs(r, usize::from(k))?;
    let k2 = r.u16()?;
    if k2 > 4096 {
        return Err(bad("ref surface edge count"));
    }
    ptrs(r, usize::from(k2))?;
    r.skip(62);
    r.ptr(Some(PMARK))?;
    if !r.zeros(r.p, 14) || !r.zeros(r.p + 18, 10) || !r.zeros(r.p + 36, 8) {
        return Err(bad("ref surface tail"));
    }
    r.skip(44);
    Ok(V::map([
        ("n", n),
        ("faces", V::from(k)),
        ("edges", V::from(k2)),
    ]))
}

/// `moOneBend_c`: the feature, 122 bytes, its displays, a `moCompFeature_c`, 33 bytes, its `moCompEdge_c` and its
/// displays, then a block whose fields are open (EV-23). A block that opens `00 00 80 00 08 00 00 00` is 608 bytes
/// and ends at the next object; any other block is framed by the next named object: a class tag, the name object's
/// class tag and a Unicode `CString`.
pub(crate) fn onebend(r: &mut Reader<'_>) -> Res<V> {
    let n = feature(r, "moOneBend_c")?;
    r.skip(122);
    let mut dims = Vec::new();
    displays(r, &mut dims)?;
    let cf = r.ptr(Some(&["moCompFeature_c"]))?;
    r.skip(33);
    r.ptr(Some(&["moCompEdge_c"]))?;
    displays(r, &mut dims)?;
    if r.p + 8 <= r.len() && r.eq(r.p, &[0x00, 0x00, 0x80, 0x00, 0x08, 0x00, 0x00, 0x00]) {
        r.skip(608);
        return Ok(V::map([("n", n), ("cf", V::from(cf))]));
    }
    let mut i = r.p;
    let end = loop {
        if i >= r.p + 400 || i + 8 >= r.len() {
            return Err(bad("bend tail: no next object"));
        }
        if r.eq(i, &[0xff, 0xfe, 0xff]) {
            let a = r.pu16(i - 4)?;
            let c = r.pu16(i - 2)?;
            if a & 0x8000 != 0
                && c & 0x8000 != 0
                && r.known
                    .get(&u32::from(c & 0x7fff))
                    .is_some_and(|s| &**s == "moNodeName_c")
            {
                break i - 4;
            }
        }
        i += 1;
    };
    r.p = end;
    Ok(V::map([("n", n), ("cf", V::from(cf))]))
}

/// `moTemplateFlatPattern_c`, `moFlatPattern_c`.
pub(crate) fn flatpattern(r: &mut Reader<'_>) -> Res<V> {
    let n = feature(r, "moTemplateFlatPattern_c")?;
    r.skip(122);
    r.expect(&[0, 0], "flat pattern null")?;
    let cf = r.ptr(Some(&["moCompFeature_c"]))?;
    // `r.trace and r.trace[-1][0]=='moFlatPattern_c' or isclass(...)`: the
    // most recently opened object, as the Python reads it.
    if r.trace
        .last()
        .is_some_and(|t| &*t.class == "moFlatPattern_c")
        || isclass(r, r.p + 17, &["moCompSheetmetalFeat_c"])?
    {
        r.skip(17);
        r.ptr(Some(&["moCompSheetmetalFeat_c"]))?;
        let mut k = None;
        for q in r.p..r.p + 200 {
            if isclass(r, q, &["moCompFace_c"])? {
                k = Some(q);
                break;
            }
        }
        let Some(k) = k else {
            return Err(bad("flat pattern: no fixed face"));
        };
        r.p = k;
        r.ptr(None)?;
        r.expect(&[0, 0], "flat pattern u16 0")?;
        r.ptr(Some(&["moCompFace_c"]))?;
        r.skip(66);
        r.ptr(Some(&["moCompFeature_c"]))?;
        let rest = r.len() as i64 - r.p as i64;
        if rest > 400 || objat(r, r.p) {
            return Err(bad(format!("flat pattern tail {rest}")));
        }
        r.p = r.len();
        return Ok(V::map([("n", n)]));
    }
    let mut want = Vec::new();
    for v in [4_u32, 4, 4, 4] {
        want.extend_from_slice(&v.to_le_bytes());
    }
    want.extend_from_slice(&[0xff; 4]);
    want.extend_from_slice(&[0, 0, 0, 0, 1, 0, 0, 0]);
    if !r.eq(r.p + 237, &want) {
        return Err(bad("flat pattern 269 bytes"));
    }
    r.skip(269);
    r.ptr(Some(&["moPerBodySheetMetalDataNew_c"]))?;
    Ok(V::map([("n", n), ("cf", V::from(cf))]))
}

/// `moPerBodySheetMetalDataNew_c`.
pub(crate) fn perbodysm(r: &mut Reader<'_>) -> Res<V> {
    let k = r.u16()?;
    let objs = ptrs(r, usize::from(k))?;
    let a = r.u32()?;
    let b = r.u32()?;
    Ok(list(vec![V::from(objs), V::from(a), V::from(b)]))
}

/// `moFoldUpDownSketchXformData_c`.
pub(crate) fn foldsketchxform(r: &mut Reader<'_>) -> Res<V> {
    r.expect_zeros(16, "fold sketch xform 16 zero")?;
    Ok(V::None)
}

/// `moFoldUpDownFeatXformData_c`.
pub(crate) fn foldfeatxform(r: &mut Reader<'_>) -> Res<V> {
    r.expect_zeros(8, "fold feature xform 8 zero")?;
    Ok(V::None)
}

/// `moMirrorStock_c` (probe).
pub(crate) fn mirrorstock(r: &mut Reader<'_>) -> Res<V> {
    feature(r, "moMirrorStock_c")?;
    let hd = bodyhead(r, "moMirrorStock_c")?;
    if hd_x_is_1(&hd) {
        units(r)?;
    }
    r.ptr(Some(PMARK))?;
    r.expect_zeros(16, "mirror stock 16 zero")?;
    r.ptr(Some(&["moBodyRef_w"]))?;
    Err(bad("mirror stock (probe)"))
}

/// `moBodyRef_w` (probe).
pub(crate) fn bodyref(r: &mut Reader<'_>) -> Res<V> {
    Ok(V::from(r.ptr(Some(&["moCompBody_c"]))?))
}

/// `moCompBody_c` (probe).
pub(crate) fn compbody(r: &mut Reader<'_>) -> Res<V> {
    r.obj(Some(UCS))?;
    let q = r.p;
    let mut k = None;
    for p in r.p..r.p + 600 {
        if r.byte(p)? == 0
            && r.byte(p + 5)? == 0
            && i64::from(r.pu32(p + 6)?) == i64::from(r.pu32(p + 1)?) - 5
            && r.eq(p + 10, &BLOB_MARK)
        {
            k = Some(p);
            break;
        }
    }
    let Some(k) = k else {
        return Err(bad("component body: no compressed body"));
    };
    r.p = k;
    sketch::blob(r)?;
    Ok(V::map([("gap", len_v(k - q))]))
}

/// `moNativeConfigurationTableFeature_c`.
pub(crate) fn cfgtable(r: &mut Reader<'_>) -> Res<V> {
    let n = feature(r, "moNativeConfigurationTableFeature_c")?;
    r.cstring()?;
    r.expect(&zff(12, 4, 0), "configuration table -1")?;
    Ok(n)
}

/// `moMacroFeature_c` (probe).
pub(crate) fn macrofeature(r: &mut Reader<'_>) -> Res<V> {
    feature(r, "moMacroFeature_c")?;
    Err(bad("macro feature (probe)"))
}

/// `moAbsolutePoint_c`.
pub(crate) fn abspoint(r: &mut Reader<'_>) -> Res<V> {
    Ok(list(f64s(r, 3)?))
}

/// `moCombineBodies_c`.
pub(crate) fn combine(r: &mut Reader<'_>) -> Res<V> {
    let n = feature(r, "moCombineBodies_c")?;
    let hd = bodyhead(r, "moCombineBodies_c")?;
    if hd_x_is_1(&hd) {
        units(r)?;
    }
    r.ptr(Some(PMARK))?;
    r.ptr(None)?;
    let k = r.u32()?;
    if k > 4096 {
        return Err(bad("combine id count"));
    }
    u32s(r, k)?;
    let op = r.u32()?;
    r.expect_zeros(8, "combine 8 zero")?;
    let nt = r.u16()?;
    ptrs(r, usize::from(nt))?;
    let k2 = r.u32()?;
    if k2 > 4096 {
        return Err(bad("combine id count 2"));
    }
    u32s(r, k2)?;
    r.u32()?;
    let Some(j) = r.find(&M12345, r.p, r.p + 80) else {
        return Err(bad("combine handle block"));
    };
    r.p = j + 12;
    r.expect_zeros(4, "combine u32 0")?;
    r.u32()?;
    r.expect(&zff(12, 4, 8), "combine -1")?;
    let mut done = false;
    for l in [4_usize, 5] {
        let head = r.eq(r.p, &[1, 0, 0, 0]) || r.zeros(r.p, 4);
        // `r.b[r.p+4:r.p+L]==bytes(L-4)`: an empty slice always matches.
        let pad = l == 4 || r.zeros(r.p + 4, l - 4);
        if head && pad && (r.p + l == r.len() || objat(r, r.p + l)) {
            r.skip(l);
            done = true;
            break;
        }
    }
    if !done {
        return Err(bad("combine end"));
    }
    Ok(V::map([
        ("n", n),
        ("op", V::from(op)),
        ("tools", V::from(nt)),
    ]))
}

/// `moSketchBitmapData_c`.
pub(crate) fn bitmapdata(r: &mut Reader<'_>) -> Res<V> {
    let v = f64s(r, 6)?;
    let w = r.u32()?;
    let h = r.u32()?;
    r.expect_zeros(4, "bitmap u32 0")?;
    let nm = r.cstring()?;
    r.expect(&[1, 0, 0, 0], "bitmap u32 1")?;
    Ok(V::map([
        ("v", list(v)),
        ("w", V::from(w)),
        ("h", V::from(h)),
        ("name", V::from(nm)),
    ]))
}

/// `moSketchBitmap_c` (probe).
pub(crate) fn sketchbitmap(r: &mut Reader<'_>) -> Res<V> {
    feature(r, "moSketchBitmap_c")?;
    r.ptr(Some(&["moSketchBitmapData_c"]))?;
    while objat(r, r.p) {
        r.ptr(None)?;
    }
    Err(bad("sketch bitmap (probe)"))
}

/// `moRefPoint_c`.
pub(crate) fn refpoint(r: &mut Reader<'_>) -> Res<V> {
    let n = feature(r, "moRefPoint_c")?;
    r.skip(112);
    let pt = f64s(r, 3)?;
    let typ = r.u32()?;
    r.expect_zeros(10, "reference point 10 zero")?;
    r.ptr(None)?;
    Ok(V::map([("n", n), ("pt", list(pt)), ("type", V::from(typ))]))
}

/// `moFaceCenterRefPointData_c`.
pub(crate) fn facecenterpt(r: &mut Reader<'_>) -> Res<V> {
    r.ptr(None)?;
    refblock(r)?;
    let bx = f64s(r, 4)?;
    let k = (r.p..r.p + 40).find(|&p| objat(r, p));
    let Some(k) = k else {
        return Err(bad("face centre point end"));
    };
    r.p = k;
    Ok(list(bx))
}

/// `moMoveFace_c`.
pub(crate) fn moveface(r: &mut Reader<'_>) -> Res<V> {
    let n = feature(r, "moMoveFace_c")?;
    let hd = bodyhead(r, "moMoveFace_c")?;
    if hd_x_is_1(&hd) {
        units(r)?;
    }
    r.ptr(Some(PMARK))?;
    let a = r.f64()?;
    let b = r.f64()?;
    r.skip(14);
    let k = r.u16()?;
    for _ in 0..k {
        r.ptr(Some(&["moCompFace_c"]))?;
        if r.eq(r.p, &[0, 0]) && isclass(r, r.p + 2, &["moCompFace_c"])? {
            r.skip(2);
        }
    }
    r.expect_zeros(4, "move face u32 0")?;
    r.ptr(Some(SUOBLIST))?;
    let mut dims = Vec::new();
    displays(r, &mut dims)?;
    let mut k = (r.p..r.p + 600).find(|&q| objat(r, q));
    if k.is_none() && r.len() as i64 - r.p as i64 <= 600 {
        k = Some(r.len());
    }
    let Some(k) = k else {
        return Err(bad("move face end"));
    };
    r.p = k;
    Ok(V::map([
        ("n", n),
        ("v", list(vec![V::from(a), V::from(b)])),
    ]))
}

/// `moLineAxisData_c`.
pub(crate) fn lineaxis(r: &mut Reader<'_>) -> Res<V> {
    let v = f64s(r, 14)?;
    r.ptr(None)?;
    axistail(r)?;
    Ok(list(v))
}
