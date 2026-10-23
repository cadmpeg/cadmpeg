//! Sketch rules: the inline entity record, handle objects,
//! `sgSketch`, its tail and the objects a sketch refers to.

use std::sync::Arc;

use super::super::reader::{bad, Ptr, Reader, Res, V};
use super::feature::{isclass, isver, objat, refblock, ver_or, M12345, UCS};

/// The rule state shared by the sketch rules.
#[derive(Debug, Clone, Copy, Default)]
pub(crate) struct SketchState(u8);

impl SketchState {
    const MIRROR: u8 = 1 << 0;
    const BYREF: u8 = 1 << 1;
    const DIMLAST: u8 = 1 << 2;
    const PGMDONE: u8 = 1 << 3;

    /// The mirror flag.
    pub(crate) fn mirror(self) -> bool {
        self.0 & Self::MIRROR != 0
    }

    /// Sets the mirror flag.
    pub(crate) fn set_mirror(&mut self, value: bool) {
        self.set(Self::MIRROR, value);
    }

    /// The by-reference flag.
    pub(crate) fn byref(self) -> bool {
        self.0 & Self::BYREF != 0
    }

    /// Sets the by-reference flag.
    pub(crate) fn set_byref(&mut self, value: bool) {
        self.set(Self::BYREF, value);
    }

    /// The last-dimension flag.
    pub(crate) fn dimlast(self) -> bool {
        self.0 & Self::DIMLAST != 0
    }

    /// Sets the last-dimension flag.
    pub(crate) fn set_dimlast(&mut self, value: bool) {
        self.set(Self::DIMLAST, value);
    }

    /// An offset chain read the offset profile.
    pub(crate) fn pgmdone(self) -> bool {
        self.0 & Self::PGMDONE != 0
    }

    /// Sets the offset-profile flag.
    pub(crate) fn set_pgmdone(&mut self, value: bool) {
        self.set(Self::PGMDONE, value);
    }

    fn set(&mut self, flag: u8, value: bool) {
        if value {
            self.0 |= flag;
        } else {
            self.0 &= !flag;
        }
    }
}

/// Largest deviation of a stored unit vector from unit length.
const UNIT_VECTOR_TOLERANCE: f64 = 1e-6;

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
fn zeros12_then(tail: &[u8]) -> Vec<u8> {
    let mut x = vec![0; 12];
    x.extend_from_slice(tail);
    x
}

/// `r.b[p:p+z] == bytes(z)`, true for an empty slice.
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
fn line(reader: &mut Reader<'_>) -> Res<V> {
    let mut value = rhead(reader)?;
    let i0 = reader.u16()?;
    let i1 = reader.u16()?;
    let first = reader.u32()?;
    value.set("i0", V::from(i0));
    value.set("i1", V::from(i1));
    value.set("a", V::from(first));
    if i1 == 0xffff
        && first == 0xffff_fffe
        && reader.zeros(reader.p, 4)
        && reader.eq(reader.p + 4, &[0, 0, 0, 0, 0, 0, 0xf0, 0xbf])
    {
        reader.skip(4);
    }
    let m1 = reader.f64()?;
    let fe4 = [
        0xfe, 0xff, 0xff, 0xff, 0xfe, 0xff, 0xff, 0xff, 0xfe, 0xff, 0xff, 0xff, 0xfe, 0xff, 0xff,
        0xff,
    ];
    if m1 != -1.0 && !(reader.eq(reader.p, &[1, 0]) && reader.eq(reader.p + 6, &fe4)) {
        return Err(bad("curve f64 -1"));
    }
    if isclass(reader, reader.p, &["moDimText_c"])? {
        value.set("b", V::from("text"));
        let text = reader.ptr(None)?;
        value.set("text", V::from(text));
        return Ok(value);
    }
    let second = reader.u16()?;
    value.set("b", V::from(second));
    let fe = [0xfe, 0xff, 0xff, 0xff];
    if second == 1 && !reader.eq(reader.p + 4, &fe) && reader.eq(reader.p, &[0, 0]) {
        reader.u16()?;
        let centre = reader.u16()?;
        let mut axes = Vec::new();
        for _ in 0..4 {
            axes.push(V::from(reader.u16()?));
        }
        value.set("ell", V::List(vec![V::from(centre), V::List(axes)]));
        let at = reader.p + 2;
        if reader.eq(reader.p, &[0, 0])
            && reader.eq(at, &[0, 0])
            && reader.eq(at + 4, &[0, 0, 1, 0])
            && isattr(reader, at + 16)
        {
            reader.skip(2);
            value.set("c", V::Int(0));
            value.set("d", V::Int(0));
            value.set("id", V::Int(0));
            return Ok(value);
        }
    } else if second == 0xffff
        && reader.eq(reader.p, &[0xff, 0xff])
        && !reader.eq(reader.p + 4, &fe)
    {
        reader.u16()?;
        let centre = reader.u16()?;
        let mut axes = Vec::new();
        for _ in 0..4 {
            axes.push(V::from(reader.u16()?));
        }
        value.set("ell", V::List(vec![V::from(centre), V::List(axes)]));
        value.set("pell", V::Bool(true));
    } else if second == 1 || second == 0xffff {
        let x_value = reader.u32()?;
        let mut ys = Vec::new();
        for _ in 0..4 {
            ys.push(V::from(reader.u32()?));
        }
        value.set("arc", V::List(vec![V::from(x_value), V::List(ys)]));
    } else if second == 2 {
        reader.expect_bytes(&[0, 0], "spline u16 0")?;
        let order = reader.u16()?;
        reader.ptr(Some(&["cptsSplineList_c", "modifSplineList_c"]))?;
        if !reader.eq(reader.p, &[1, 0, 0, 0, 0, 0, 0, 0])
            && !reader.zeros(reader.p, 8)
            && !reader.eq(reader.p, &[1, 0, 1, 0, 0, 0, 0, 0])
        {
            // u32 1 (or 0) and u32 0; or u16 1 and u32 1.
            return Err(bad("spline 1 0"));
        }
        reader.skip(8);
        reader.f64()?;
        reader.f64()?;
        reader.f64()?;
        reader.u32()?;
        reader.expect_zeros(48, "spline 48 zero")?;
        reader.expect_bytes(&[1, 0], "spline u16 1")?;
        let nk = reader.u32()?;
        if nk > 4096 {
            return Err(bad("spline knots"));
        }
        for _ in 0..nk {
            reader.f64()?;
        }
        let nc = reader.u32()?;
        if nc > 4096 {
            return Err(bad("spline points"));
        }
        for _ in 0..nc {
            reader.f64()?;
            reader.f64()?;
            reader.f64()?;
        }
        if reader.zeros(reader.p, 4)
            && reader.pu32(reader.p + 4)? == 0
            && reader.pu32(reader.p + 8)? == 2
        {
            // A spline whose geometry may be a compressed Parasolid body: u32 0, u32 0, u32 2, the parameter
            // range, u32 0, u32 1. When a body follows, a zero run (24 or 56 bytes), u16 1 and a blob
            // (00 A 00 B, B = A - 5) stand before it; when none does, the record's tail follows the two u32.
            reader.skip(12);
            reader.f64()?;
            reader.f64()?;
            reader.expect_zeros(4, "spline body u32 0")?;
            reader.u32()?;
            let at = reader.p;
            let mut stop = reader.p;
            while stop < reader.p + 64 && stop < reader.len() && reader.byte(stop)? == 0 {
                stop += 1;
            }
            let mut hasbody = false;
            if stop + 12 <= reader.len() && reader.eq(stop, &[1, 0]) && reader.eq(stop + 2, &[0]) {
                let first = reader.pu32(stop + 3)?;
                let second = reader.pu32(stop + 8)?;
                hasbody = reader.eq(stop + 7, &[0]) && first.checked_sub(5) == Some(second);
            }
            if hasbody {
                reader.skip(stop - reader.p);
                reader.u16()?;
                let data = blob(reader)?;
                let len = match &data {
                    V::Bytes(bytes) => bytes.len(),
                    _ => 0,
                };
                value.set(
                    "spline",
                    V::List(vec![
                        V::from("body"),
                        V::from(i64::try_from(len).map_err(|_| bad("spline length exceeds i64"))?),
                    ]),
                );
            } else {
                reader.p = at;
                value.set("spline", V::List(vec![V::from("bodyless")]));
            }
            let class = reader.u16()?;
            let distance = reader.u32()?;
            let id = reader.u32()?;
            value.set("c", V::from(class));
            value.set("d", V::from(distance));
            value.set("id", V::from(id));
            return Ok(value);
        }
        reader.expect_bytes(
            &[0, 0, 0, 0, 0xff, 0xff, 0xff, 0xff, 1, 0, 0, 0, 1, 0, 0, 0],
            "spline 0 -1 1 1",
        )?;
        if reader.u32()? != nc {
            return Err(bad("spline weight count"));
        }
        reader.u32()?;
        reader.f64()?;
        reader.f64()?;
        for _ in 0..nc {
            reader.f64()?;
        }
        reader.expect_zeros(8, "spline 8 zero")?;
        let sf = reader.u16()?;
        reader.f64()?;
        if sf != 0 {
            reader.f64()?;
        }
        reader.expect_zeros(12, "spline 12 zero")?;
        value.set(
            "spline",
            V::List(vec![V::from(order), V::from(nk), V::from(nc)]),
        );
        let class = reader.u16()?;
        let distance = reader.u32()?;
        let id = reader.u32()?;
        value.set("c", V::from(class));
        value.set("d", V::from(distance));
        value.set("id", V::from(id));
        return Ok(value);
    } else if second == 0x40 {
        reader.expect_bytes(&[0, 0], "style spline u16 0")?;
        reader.u16()?;
        let cps = reader.ptr(Some(&["cptsSplineList_c"]))?;
        reader.expect_zeros(8, "style spline 8 zero")?;
        let x_value = reader.f64()?;
        let y_value = reader.f64()?;
        if (x_value, y_value) != (-1000.0, -1000.0) {
            return Err(bad("style spline -1000"));
        }
        let mut fixed = vec![0; 60];
        fixed.extend_from_slice(&[1, 0]);
        fixed.extend_from_slice(&[0; 12]);
        fixed.extend_from_slice(&[0xff; 4]);
        fixed.extend_from_slice(&[0; 4]);
        fixed.extend_from_slice(&[1, 0, 0, 0]);
        fixed.extend_from_slice(&[0; 6]);
        reader.expect_bytes(&fixed, "style spline fixed")?;
        value.set("style", V::from(cps));
    } else if second != 0 {
        return Err(bad(format!("curve type {second}")));
    }
    let class = reader.u16()?;
    let distance = reader.u32()?;
    let id = reader.u32()?;
    value.set("c", V::from(class));
    value.set("d", V::from(distance));
    value.set("id", V::from(id));
    Ok(value)
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
            return Err(bad("pattern direction: no zero run to skip"));
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
fn islinpatt(reader: &Reader<'_>, at: usize) -> Res<bool> {
    if reader.zeros(at, 8) {
        return Ok(false);
    }
    let n1 = reader.pu16(at + 14)?;
    let n2 = reader.pu16(at + 16)?;
    let first = reader.pu16(at + 18)?;
    let class = reader.pu16(at + 20)?;
    if (n1, n2) != (first, class) || n1 == 0 {
        return Ok(false);
    }
    let d0 = reader.pf64(at + 26)?;
    let d1 = reader.pf64(at + 34)?;
    let d2 = reader.pf64(at + 42)?;
    let span = d0 * d0 + d1 * d1 + d2 * d2;
    if (span - 1.0).abs() > UNIT_VECTOR_TOLERANCE {
        return Ok(false);
    }
    let word = reader.pu16(at + 50)?;
    Ok(tagword(word))
}

/// `ispoly(q)` (inside `sketch`).
fn ispoly(reader: &Reader<'_>, at: usize) -> Res<bool> {
    if reader.zeros(at, 8) {
        return Ok(false);
    }
    let mut count_ = reader.pu32(at + 4)?;
    if reader.ffs(at, 4) && count_ == 1 {
        count_ = u32::from(reader.pu16(at + 18)?);
    }
    // The u16 pair repeats a count, not always n (a circular pattern's: 3
    // with 8 instances, 15 with 30).
    let first = reader.pu16(at + 18)?;
    let second = reader.pu16(at + 20)?;
    if !(0 < count_ && count_ < 1000) || first != second || !(0 < first && first < 1000) {
        return Ok(false);
    }
    let word = reader.pu16(at + 46)?;
    Ok(tagword(word))
}

/// `sgSketch` (`sketch`).
pub(crate) fn sketch(reader: &mut Reader<'_>) -> Res<V> {
    reader.st.sk.set_pgmdone(false);
    let big_n = reader.u32()?;
    reader.expect_bytes(&[0, 0], "")?;
    reader.u16()?;
    reader.expect_bytes(&[0, 0], "")?;
    let big_k = reader.u32()?;
    for _ in 0..big_n {
        record(reader)?;
    }
    reader.u32()?;
    let mut lines: Vec<V> = Vec::new();
    loop {
        if isattr(reader, reader.p) {
            lines.push(line(reader)?);
            continue;
        }
        if reader.eq(reader.p, &[0, 0])
            && reader.eq(reader.p + 4, &[0, 0])
            && reader.byte(reader.p + 7)? == 0
            && isattr(reader, reader.p + 16)
        {
            reader.pu16(reader.p + 2)?;
            reader.skip(4);
            continue;
        }
        if reader.eq(reader.p, &[0, 0])
            && matches!(reader.byte(reader.p + 2)?, 0..=2)
            && reader.byte(reader.p + 3)? == 0
            && isattr(reader, reader.p + 12)
        {
            reader.pu32(reader.p + 4)?;
            reader.pu32(reader.p + 8)?;
            reader.skip(12);
            continue;
        }
        if reader.eq(reader.p, &[0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 1, 0])
            && reader.eq(reader.p + 12, &[0, 0])
            && reader.eq(reader.p + 20, &[1, 0, 0, 0])
            && isattr(reader, reader.p + 24)
        {
            reader.pu32(reader.p + 16)?;
            reader.skip(24);
            continue;
        }
        if reader.eq(reader.p, &[0, 0]) && isattr(reader, reader.p + 2) {
            reader.skip(2);
            continue;
        }
        let u = reader.pu32(reader.p)?;
        if 0 < u && u < 0x10000 && isattr(reader, reader.p + 4) {
            reader.skip(4);
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
    for &zeros_ in order {
        if reader.zeros(reader.p, zeros_) && okat(reader, reader.p + zeros_)? {
            found = Some(zeros_);
            break;
        }
    }
    let Some(zeros_) = found else {
        return Err(bad("sketch list trailer"));
    };
    reader.skip(zeros_);
    let mut rels: Vec<(usize, Rel)> = Vec::new();
    let mut ndims = 0_usize;
    for gi in 0..3 {
        let found_ = reader.u16()?;
        for i in 0..found_ {
            if !isdim(reader, reader.p)? {
                if reader.zeros(reader.p, 8) && reader.ffs(reader.p + 8, 4) {
                    core(reader, gi)?;
                    ndims += 1;
                    continue;
                }
                return Err(bad("dimension group: not a dimension"));
            }
            reader.st.sk.set_dimlast(i + 1 == found_);
            let res = reader.ptr(None);
            reader.st.sk.set_dimlast(false);
            res?;
            ndims += 1;
        }
    }
    for gi in 0..8 {
        let word = reader.pu16(reader.p)?;
        if gi >= 1 && tagword(word) {
            break;
        }
        let found_ = reader.u16()?;
        for _ in 0..found_ {
            let w0 = reader.pu16(reader.p)?;
            if reader.ffs(reader.p, 4) && ispoly(reader, reader.p)? {
                rels.push(polygon(reader, gi)?);
                continue;
            }
            if reader.ffs(reader.p, 4) && islinpatt(reader, reader.p)? {
                rels.push(linpatt(reader, gi)?);
                continue;
            }
            if tagword(w0) {
                reader.ptr(None)?;
                rels.push((gi, Rel::Obj));
                continue;
            }
            if ispoly(reader, reader.p)? {
                rels.push(polygon(reader, gi)?);
                continue;
            }
            if islinpatt(reader, reader.p)? {
                rels.push(linpatt(reader, gi)?);
                continue;
            }
            rels.push(core(reader, gi)?);
        }
        if rels
            .iter()
            .any(|x_value| x_value.0 == gi && matches!(x_value.1, Rel::Polygon | Rel::Pattern))
        {
            let mask = reader.u16()?;
            if mask > 4096 {
                return Err(bad("polygon relation count"));
            }
            for _ in 0..mask {
                rels.push(core(reader, gi)?);
                reader.skip(20);
            }
        }
    }
    while reader.eq(reader.p, &[0, 0]) {
        reader.skip(2);
    }
    reader.ptr(None)?;
    let at = reader.p + 10;
    if reader.zeros(reader.p, 8) && reader.eq(reader.p + 8, &[1, 0]) && at + 8 <= reader.len() {
        let class_ = usize::try_from(reader.pu32(at)?)
            .map_err(|_| bad("sketch table count exceeds address space"))?;
        if 0 < class_ && class_ < 256 && at + 4 * class_ + 8 <= reader.len() {
            let mut ok = true;
            for i in 0..class_ {
                let x_value = reader.pu32(at + 4 + 4 * i)?;
                if !(0 < x_value && x_value < 1_000_000) {
                    ok = false;
                }
            }
            let first = usize::try_from(reader.pu32(at + 4 + 4 * class_)?)
                .map_err(|_| bad("sketch table end exceeds address space"))?;
            if ok && (first == 0 || first == 1 || first == class_) {
                reader.skip(2);
            }
        }
    }
    if reader.p + 12 <= reader.len() && reader.zeros(reader.p, 4) && reader.pu16(reader.p + 4)? == 0
    {
        let n0 = reader.pu16(reader.p + 6)?;
        let class_ = reader.pu32(reader.p + 8)?;
        if 0 < n0 && n0 <= 256 && class_ > 0xffff {
            // The same 52-byte tail before a group header: n0 is the shifted n, and the group data's class
            // tag read as a chain count is not a count (2025 Margie).
            reader.skip(2);
        }
    }
    reader.expect_zeros(4, "settings u32 0")?;
    let count_ = reader.u16()?;
    let mut found_ = u32::from(reader.u16()?);
    if count_ > 256 || found_ > 4096 {
        return Err(bad("in-context block counts"));
    }
    for g in 0..count_ {
        found_ = ingroup(reader, found_)?;
        if g + 1 == count_ && found_ != 0 && chains(reader)? != 0 {
            return Err(bad("group count"));
        }
    }
    if count_ == 0 && found_ != 0 && chains(reader)? != 0 {
        return Err(bad("chain list end"));
    }
    if reader.st.sk.pgmdone() {
        reader.st.sk.set_pgmdone(false);
    } else {
        if reader.eq(reader.p, &[0, 0, 1, 0, 0, 0]) && objat(reader, reader.p + 18) {
            reader.skip(2);
            for _ in 0..4 {
                reader.u32()?;
            }
            reader.ptr(None)?;
        }
        reader.expect_zeros(4, "settings u32 0 b")?;
        let word = reader.pu16(reader.p + 4)?;
        if reader.eq(reader.p, &[0, 0, 2, 0]) && tagword(word) {
            reader.skip(4);
            reader.ptr(Some(&["sgPGMProfileData_c"]))?;
        }
    }
    let ver = ver_or(reader, 0);
    let mut length = if ver >= 19000 { 78 } else { 76 };
    if !reader.eq(reader.p + length, &[0xff, 0xfe, 0xff])
        && reader.eq(reader.p + 76, &[0xff, 0xfe, 0xff])
    {
        length = 76;
    }
    reader.skip(length);
    reader.cstring()?;
    reader.skip(42);
    reader.u32()?;
    reader.u32()?;
    let found_ = reader.u32()?;
    if found_ > 4096 {
        return Err(bad("settings list count"));
    }
    for _ in 0..found_ {
        reader.ptr(None)?;
    }
    reader.expect_zeros(24, "settings 24 zero")?;
    reader.ptr(None)?;
    let kind = sktail(reader)?;
    Ok(V::map([
        ("N", V::from(big_n)),
        ("K", V::from(big_k)),
        (
            "lines",
            V::Int(i64::try_from(lines.len()).map_err(|_| bad("line count exceeds i64"))?),
        ),
        (
            "dims",
            V::Int(i64::try_from(ndims).map_err(|_| bad("dimension count exceeds i64"))?),
        ),
        (
            "rels",
            V::Int(i64::try_from(rels.len()).map_err(|_| bad("relation count exceeds i64"))?),
        ),
        ("plane", kind.get("plane").clone()),
    ]))
}

/// `sktail(r)`: the end of a sketch, to its plane.
pub(crate) fn sktail(reader: &mut Reader<'_>) -> Res<V> {
    let at = reader.p + 18;
    if reader.zeros(reader.p, 18) && at + 14 <= reader.len() {
        let nxt = reader.pu32(at)?;
        let cnt = reader.pu32(at + 4)?;
        if 0 < cnt && cnt <= nxt && nxt < 4096 && objat(reader, at + 12) {
            reader.p = at + 8;
            for _ in 0..cnt {
                reader.u32()?;
                reader.ptr(Some(&["sgSlot_c"]))?;
            }
        }
    }
    let mut zeros_ = 0;
    while reader.p + zeros_ < reader.len() && reader.byte(reader.p + zeros_)? == 0 && zeros_ < 80 {
        zeros_ += 1;
    }
    if reader.p + zeros_ + 8 < reader.len() {
        let a2 = reader.pu32(reader.p + zeros_)?;
        let n2 = reader.pu32(reader.p + zeros_ + 4)?;
        if 0 < a2 && a2 < 64 && n2 > a2 && isclass(reader, reader.p + zeros_ + 8, &["sgBlock"])? {
            reader.skip(zeros_ + 4);
            for _ in 0..a2 {
                reader.u32()?;
                reader.ptr(Some(&["sgBlock"]))?;
            }
        } else if a2 == 1
            && n2 == 0
            && reader.zeros(reader.p + zeros_ + 8, 4)
            && (reader.eq(reader.p + zeros_ + 12, &[3, 0, 0, 0])
                || reader.eq(reader.p + zeros_ + 12, &[7, 0, 0, 0]))
        {
            reader.skip(zeros_ + 8);
        } else if 0 < n2 && n2 <= a2 && a2 < 4096 && objat(reader, reader.p + zeros_ + 8) {
            reader.skip(zeros_ + 8);
            for _ in 0..n2 {
                reader.ptr(None)?;
            }
        }
    }
    loop {
        let marker = reader.eq(reader.p, &[3, 0, 0, 0]) || reader.eq(reader.p, &[7, 0, 0, 0]);
        if marker && reader.byte(reader.p + 4)? <= 1 {
            break;
        }
        if reader.byte(reader.p)? != 0 {
            return Err(bad("sketch tail"));
        }
        reader.p += 1;
    }
    let mk = reader.u32()?;
    if reader.byte(reader.p)? == 0 {
        blob(reader)?;
    } else {
        reader.skip(1);
    }
    reader.expect_zeros(4, "sketch tail u32 0")?;
    let class = reader.u16()?;
    for _ in 0..class {
        reader.ptr(None)?;
    }
    if class != 0 {
        let first = reader.u32()?;
        let second = reader.u32()?;
        let distance = reader.u32()?;
        if [first, second, distance] != [4, 16, 1] {
            return Err(bad("sketch tail 4 16 1"));
        }
        let count_ = reader.u32()?;
        for _ in 0..count_ {
            reader.u32()?;
        }
    }
    // `(mk == 3 or not c) and not (c and 100000 <= u32 <= 100064)`.
    if mk == 3 || class == 0 {
        let marked = class != 0 && (100_000..=100_064).contains(&reader.pu32(reader.p)?);
        if !marked {
            reader.expect_zeros(4, "sketch tail u32 0 b")?;
        }
    }
    let mv = reader.u32()?;
    if !(100_000..=100_064).contains(&mv) {
        return Err(bad("sketch tail 100000"));
    }
    if mv > 100_000 {
        let groups_ = reader.u16()?;
        let mut ids = Vec::new();
        if u32::from(groups_) != mv - 100_000 {
            return Err(bad("sketch tail group count"));
        }
        if groups_ > 64 {
            return Err(bad("sketch tail groups"));
        }
        for _ in 0..groups_ {
            reader.ptr(Some(&["moOneContour_c"]))?;
            reader.st.core.skbox = true;
            let res = (|| -> Res<()> {
                let count_ = reader.u16()?;
                for _ in 0..count_ {
                    reader.ptr(None)?;
                }
                Ok(())
            })();
            reader.st.core.skbox = false;
            res?;
            reader.u16()?;
            ids.push(reader.u32()?);
        }
        ids.sort_unstable();
        if ids != (100_000..100_000 + u32::from(groups_)).collect::<Vec<_>>() {
            return Err(bad(format!("sketch tail group ids {ids:?}")));
        }
        reader.ptr(None)?;
        let plane = reader.ptr(None)?;
        return Ok(V::map([("plane", V::from(plane))]));
    }
    reader.expect_bytes(&[0, 0], "sketch tail u16 0")?;
    reader.ptr(None)?;
    let plane = reader.ptr(None)?;
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
            r.expect_bytes(&zeros12_then(&[0xff; 4]), "support -1")?;
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
                r.expect_bytes(&zeros12_then(&[0xff; 4]), "support face -1")?;
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
        r.expect_bytes(&zeros12_then(&[0xff; 4]), "support block -1")?;
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
            r.st.sk.set_pgmdone(true);
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
    let compressed = r.slice(
        q + 24,
        usize::try_from(cl).map_err(|_| bad("compressed length exceeds address space"))?,
    );
    let mut data = Vec::new();
    flate2::read::ZlibDecoder::new(compressed)
        .take(u64::from(ul) + 1)
        .read_to_end(&mut data)
        .map_err(|_| bad("blob zlib"))?;
    if data.len()
        != usize::try_from(ul).map_err(|_| bad("uncompressed length exceeds address space"))?
    {
        return Err(bad("blob length"));
    }
    r.p = q + usize::try_from(b).map_err(|_| bad("blob length exceeds address space"))?;
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
pub(crate) fn sketchextref(reader: &mut Reader<'_>) -> Res<V> {
    let me = reader
        .last_class()
        .map_or_else(String::new, |c| c.to_string());
    let entry = reader.ptr(None)?;
    let at = reader.p;
    let eclass = entry.obj_class().map(str::to_owned);
    let eclass = eclass.as_deref();
    if eclass == Some("moCompSilhVertex_c")
        && (1..=16).contains(&reader.pu32(at)?)
        && (1..=8).contains(&reader.pu32(at + 20)?)
    {
        reader.skip(4);
        let first = vec![V::from(reader.f64()?), V::from(reader.f64()?)];
        reader.skip(4);
        reader.expect_zeros(10, "silhouette vertex 10 zero")?;
        reader.u32()?;
        let mut x_value = zeros12_then(&m3());
        x_value.extend_from_slice(&[0; 4]);
        reader.expect_bytes(&x_value, "silhouette vertex -12345")?;
        reader.u32()?;
        let mut y_value = zeros12_then(&[0xff; 4]);
        y_value.extend_from_slice(&[0, 0]);
        reader.expect_bytes(&y_value, "silhouette vertex -1")?;
        let bk = reader.ptr(None)?;
        let nm = entname(reader)?;
        return Ok(V::List(vec![
            V::from(entry),
            V::List(first),
            V::from(bk),
            nm,
        ]));
    }
    if eclass == Some("moCompSilhEdge_c") && (1..=16).contains(&reader.pu32(at)?) {
        reader.skip(4);
        let first = vec![V::from(reader.f64()?), V::from(reader.f64()?)];
        let mut zeros_ = 0;
        while reader.byte(reader.p + zeros_)? == 0 && zeros_ < 200 {
            zeros_ += 1;
        }
        reader.skip(zeros_);
        if reader.eq(reader.p + 16, &m3()) && !reader.eq(reader.p, &[1, 0, 0, 0]) {
            reader.skip(16);
            reader.skip(12);
            reader.expect_zeros(4, "silhouette u32 0")?;
            reader.u32()?;
            let mut x_value = zeros12_then(&[0xff; 4]);
            x_value.extend_from_slice(&[0; 10]);
            reader.expect_bytes(&x_value, "silhouette -1")?;
            let nm = if reader.eq(reader.p, &[0xff, 0xfe, 0xff]) {
                V::Str(reader.cstring()?)
            } else {
                V::None
            };
            return Ok(V::List(vec![V::from(entry), V::List(first), nm]));
        }
        reader.expect_bytes(&[1, 0, 0, 0], "silhouette u32 1")?;
        let fl = reader.u8()?;
        if fl > 1 {
            return Err(bad("silhouette flag"));
        }
        for _ in 0..if fl != 0 { 12 } else { 3 } {
            reader.f64()?;
        }
        if reader.f64()? != 1.0 {
            return Err(bad("silhouette scale"));
        }
        reader.expect_bytes(&[0], "silhouette u8 0")?;
        let kind = cseh_tail(reader, true)?;
        return Ok(V::List(vec![V::from(entry), V::List(first), kind]));
    }
    let mut zeros_ = 0;
    while reader.byte(at + zeros_)? == 0 && zeros_ < 80 {
        zeros_ += 1;
    }
    if me == "moEndPointRef_w"
        && zeros_ >= 2
        && reader.eq(at + zeros_, &[1, 0])
        && isclass(reader, at + zeros_ + 2, &["moContextChainComponent_c"])?
        && reader.byte(at + zeros_ - 1)? == 0
    {
        reader.p = at + zeros_ + 2;
        reader.ptr(None)?;
        reader.expect_zeros(12, "end point 12 zero")?;
        if reader.eq(reader.p, &[0xff, 0xff, 0xff, 0xff]) {
            reader.skip(4); // an i32 -1 before the point (1 part)
        }
        let pt = f64s(reader, 3)?;
        let fr = reader.f64()?;
        if !(0.0..=1.0).contains(&fr) {
            // The fraction along the edge lies in [0, 1].
            return Err(bad("end point fraction"));
        }
        reader.expect_bytes(&[0, 0], "end point u16 0")?;
        return Ok(V::List(vec![V::from(entry), pt]));
    }
    if reader.st.sk.byref()
        && reader.zeros(at, 62)
        && reader.eq(at + 62, &[1, 0])
        && reader.eq(at + 66 + 20, &m3())
    {
        reader.skip(58);
        let kind = cseh_tail(reader, false)?;
        return Ok(V::List(vec![V::from(entry), kind]));
    }
    if reader.zeros(at, 64)
        && (iscomp(reader, at + 64)?
            || reader.eq(at + 66 + 20, &m3())
            || (reader.zeros(at + 66 + 32, 4) && isver(reader, at + 66 + 36))
            || (reader.eq(at + 66 + 20, &M12345[..8]) && isver(reader, at + 66 + 32)))
    {
        reader.skip(58);
        let kind = cseh_tail(reader, false)?;
        return Ok(V::List(vec![V::from(entry), kind]));
    }
    if eclass == Some("moCompVertex_c")
        && reader.zeros(at, 4)
        && isclass(reader, at + 4, &["moContextChainComponent_c"])?
    {
        reader.skip(4);
        reader.ptr(None)?;
        if reader.zeros(reader.p, 14) && objat(reader, reader.p + 14) {
            reader.skip(14);
        } else {
            let mut x_value = zeros12_then(&[0xff; 4]);
            x_value.extend_from_slice(&[0, 0]);
            reader.expect_bytes(&x_value, "vertex context tail")?;
        }
        let bk = reader.ptr(None)?;
        let nm = entname(reader)?;
        return Ok(V::List(vec![V::from(entry), V::from(bk), nm]));
    }
    // `blk = r.b[q+6:q+64]`: its offsets below are `blk + i`.
    let blk = at + 6;
    if eclass == Some("moCompVertex_c")
        && reader.zeros(at, 6)
        && reader.eq(blk + 20, &M12345[..8])
        && reader.zeros(blk + 28, 4)
        && isver(reader, blk + 32)
        && reader.zeros(blk + 36, 22)
    {
        reader.skip(64);
        let nm = if reader.eq(reader.p, &[0xff, 0xfe, 0xff]) {
            V::Str(reader.cstring()?)
        } else {
            V::None
        };
        return Ok(V::List(vec![V::from(entry), V::None, nm]));
    }
    if eclass == Some("moCompVertex_c")
        && reader.zeros(at, 6)
        && (reader.eq(at + 6 + 20, &m3())
            || (reader.zeros(at + 6 + 32, 4) && isver(reader, at + 6 + 36)))
    {
        reader.skip(6);
        let at = reader.p + 40;
        let word = reader.pu16(at + 14)?;
        reader.skip(if reader.zeros(at, 14) && tagword(word) {
            54
        } else {
            58
        });
        let bk = reader.ptr(None)?;
        let nm = entname(reader)?;
        return Ok(V::List(vec![V::from(entry), V::from(bk), nm]));
    }
    Ok(V::from(entry))
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
pub(crate) fn skintersect(r: &mut Reader<'_>) -> V {
    let b = r.slice(r.p, 20).to_vec();
    r.skip(20);
    V::Bytes(b)
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
            let mut x = zeros12_then(&[0xff; 4]);
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
    if r.st.sk.dimlast() {
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
    if r.st.sk.dimlast() && 0 < w && w < 4096 && (w2 == 0xffff || (w2 & 0x8000 != 0 && dimish)) {
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
    if r.eq(r.p + 30, &m3())
        && (r.st.sk.mirror() || (r.ffs(r.p + 62, 4) && !r.eq(r.p + 66, &[0, 0])))
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
        r.expect_bytes(&zeros12_then(&[0xff; 4]), "mirror plane -1")?;
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
fn planetrailer(reader: &mut Reader<'_>, short: bool) -> Res<()> {
    let at = reader.p;
    let zeros_ = if matches!(reader.byte(at + 1)?, 2..=4) && reader.zeros(at + 2, 3) {
        1
    } else {
        3
    };
    if !reader.zeros(at, zeros_)
        || !matches!(reader.byte(at + zeros_)?, 2..=4)
        || !reader.zeros(at + zeros_ + 1, 3)
    {
        return Err(bad("plane trailer"));
    }
    reader.skip(zeros_ + 16);
    if short {
        if reader.zeros(reader.p, 12) {
            reader.skip(12);
            return Ok(());
        }
        reader.u32()?;
        reader.expect_bytes(&[0, 0, 0, 0, 1, 0, 0, 0], "plane trailer 0 1")?;
        return Ok(());
    }
    if isclass(reader, reader.p + 2, &["moContourChooser_c"])? {
        reader.u16()?;
        reader.ptr(None)?;
        let count = reader.u16()?;
        for _ in 0..count {
            reader.ptr(None)?;
        }
        reader.expect_zeros(2, "plane trailer chooser zero")?;
        return Ok(());
    }
    let word = reader.pu16(reader.p + 2)?;
    let nok = reader.pu16(reader.p)? != 0 && tagword(word);
    if !nok {
        reader.u32()?;
    }
    let count = reader.u16()?;
    for _ in 0..count {
        reader.ptr(None)?;
    }
    let mask = reader.u16()?;
    for _ in 0..mask {
        reader.ptr(None)?;
    }
    if !nok && count == 0 && mask == 0 {
        for zeros_ in [0_usize, 2, 6, 8] {
            if zeros0(reader, reader.p, zeros_) && objat(reader, reader.p + zeros_) {
                reader.skip(zeros_);
                return Ok(());
            }
        }
    }
    reader.expect_zeros(if nok { 2 } else { 4 }, "plane trailer zero")
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
pub(crate) fn pgmprofile(reader: &mut Reader<'_>) -> Res<V> {
    let nl = reader.u16()?;
    for _ in 0..nl {
        reader.ptr(None)?;
    }
    reader.u16()?;
    let distance = reader.f64()?;
    let mut zeros = 0;
    while reader.byte(reader.p + zeros)? == 0 && zeros < 64 {
        zeros += 1;
    }
    let at = reader.p + zeros;
    let count_ = if zeros >= 8 { reader.pu32(at)? } else { 0 };
    let n_width = usize::try_from(count_).map_err(|_| bad("offset count exceeds address space"))?;
    let ok =
        zeros >= 8 && 0 < count_ && count_ < 1000 && reader.pu32(at + 4 + 4 * n_width + 4)? == 0;
    if !ok {
        reader.expect_zeros(12, "offset profile 12 zero")?;
        return Ok(V::map([
            ("loops", V::from(nl)),
            ("dist", V::from(distance)),
        ]));
    }
    reader.skip(zeros + 4);
    for _ in 0..count_ {
        reader.u32()?;
    }
    reader.u32()?;
    reader.expect_zeros(4, "offset profile u32 0")?;
    reader.f64()?;
    for _ in 0..6 {
        reader.f64()?;
    }
    reader.expect_bytes(&[2, 0], "offset profile u16 2")?;
    let nxt = reader.ptr(Some(&["sgPGMProfileData_c"]))?;
    Ok(V::map([
        ("loops", V::from(nl)),
        ("dist", V::from(distance)),
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
        r.expect_bytes(&zeros12_then(&[0xff; 4]), what)?;
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
        V::Int(i64::try_from(*cs).map_err(|_| bad("contour count exceeds i64"))?),
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
    Ok(V::List(vec![
        V::Int(i64::try_from(cs).map_err(|_| bad("contour count exceeds i64"))?),
        V::List(out),
    ]))
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
    for _ in 1..g {
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
    let gap = r.slice(
        r.p,
        k.checked_sub(r.p)
            .ok_or_else(|| bad("comp profile marker before position"))?,
    );
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
        (
            "body",
            V::Int(i64::try_from(body).map_err(|_| bad("body length exceeds i64"))?),
        ),
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
pub(crate) fn dimtext(reader: &mut Reader<'_>) -> Res<V> {
    let kind = reader.cstring()?;
    let pos = f64s(reader, 3)?;
    reader.u32()?;
    let fmt = reader.ptr(None)?;
    if fmt.is_null() {
        outlines(reader)?;
    }
    reader.u32()?;
    reader.u32()?;
    let mask = reader.u32()?;
    if mask > 4096 {
        return Err(bad("text character count"));
    }
    if mask != 0 {
        for _ in 0..mask * 3 {
            reader.f64()?;
        }
        for _ in 0..mask {
            reader.u32()?;
        }
        reader.expect_zeros(8, "text 8 zero")?;
        for _ in 0..mask * 3 {
            reader.f64()?;
        }
    }
    reader.f64()?;
    reader.f64()?;
    reader.expect_zeros(18, "text 18 zero")?;
    if reader.f64()? != 1.0 {
        return Err(bad("text f64 1"));
    }
    reader.expect_zeros(4, "text u32 0")?;
    if reader.eq(reader.p, &[0xff, 0xfe, 0xff]) {
        reader.cstring()?;
        reader.expect_bytes(&[1, 0, 0, 0, 1, 0, 0, 0], "text 1 1")?;
        return Ok(V::map([("text", V::Str(kind)), ("pos", pos)]));
    }
    reader.st.sk.set_byref(true);
    let path = reader.ptr(None);
    reader.st.sk.set_byref(false);
    let path = path?;
    for _ in 0..6 {
        reader.f64()?;
    }
    let at = reader.p;
    let mut zeros = 0;
    while zeros < 16 && reader.byte(at + zeros)? == 0 {
        zeros += 1;
    }
    let first = if zeros != 0 {
        i64::from(reader.pu32(at + zeros)?)
    } else {
        0
    };
    if zeros == 0
        || reader.byte(at + zeros + 4)? != 0
        || i64::from(reader.pu32(at + zeros + 5)?) != first - 5
    {
        return Err(bad("text path: no body"));
    }
    if zeros != 10 {
        return Err(bad("text path zero bytes"));
    }
    reader.p = at + 9;
    blob(reader)?;
    reader.cstring()?;
    reader.expect_bytes(&[1, 0, 0, 0, 1, 0, 0, 0], "text 1 1")?;
    Ok(V::map([
        ("text", V::Str(kind)),
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
    let left = i128::try_from(r.len()).map_err(|_| bad("stream length exceeds i128"))?
        - i128::try_from(r.p).map_err(|_| bad("position exceeds i128"))?;
    if i128::from(p) * 24 > left {
        return Err(bad("glyph point count"));
    }
    let point_bytes = usize::try_from(p).map_err(|_| bad("point count exceeds address space"))?;
    r.skip(24 * point_bytes);
    Ok(V::map([
        ("loops", V::from(n)),
        (
            "segments",
            V::Int(i64::try_from(s).map_err(|_| bad("segment count exceeds i64"))?),
        ),
        (
            "points",
            V::Int(i64::try_from(p).map_err(|_| bad("point count exceeds i64"))?),
        ),
    ]))
}

/// `sgEntHandleData_c` (`enthandledata`): a block's entity-handle table. u32 `0`, a u32, 22-byte entries (a
/// handle class token and 20 bytes of data) until a null token, then a zero run, u32 `3` and either a
/// compressed body or u32 `1`, seven bytes and a u64.
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
    r.bump_counter()?;
    r.objs.insert(r.counter, class);
    r.bump_counter()?;
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
    if r.eq(r.p, &zeros12_then(&[0xff; 4])) {
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
