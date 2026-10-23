//! The feature rules' shared core: the named-object header, the
//! feature-object block, the feature body head and the lookahead tests the
//! other rule files call (`g_feat.py`: `named`, `block`, `feature`,
//! `bodyhead`, `refblock`, `objat`, `isclass`, ...).

use super::super::super::reader::{bad, NamedRecord, Ptr, Reader, Res, V};

/// `UCS`: the unit-component classes.
pub(crate) const UCS: &[&str] = &[
    "moUnitComponent_c",
    "moContextChainComponent_c",
    "moContextUnitComponent_c",
];

/// `EXTRUDE`: classes whose block carries 37 fixed bytes instead of 35.
pub(crate) const EXTRUDE: &[&str] = &[
    "moSMBaseFlange_c",
    "moExtruThin_c",
    "moExtrusion_c",
    "moICE_c",
    "moRevolution_c",
    "moRevCut_c",
    "moHelix_c",
    "moSweepCut_c",
    "moCut_c",
];

/// `CONSTRAINTPLN`.
pub(crate) const CONSTRAINTPLN: &[&str] = &[
    "moConstraintCoincPlaneRefplaneData_c",
    "moConstraintMidPlaneRefplaneData_c",
    "moConstraintCoincLinePerpPlaneRefplaneData_c",
    "moConstraintCoincTwoLinesRefplaneData_c",
    "moConstraintCoincCurveRefplaneData_c",
    "moConstraintCoincLineParallelPlaneRefplaneData_c",
    "moConstraintCoincLineAtAnglePlaneRefplaneData_c",
    "moConstraintCoincTwoPointsPerpPlnRefplaneData_c",
    "moConstraintCoincLinePerpLineRefplaneData_c",
    "moConstraintPerpPlnTanOneCylinderRefplaneData_c",
    "moConstraintTanCylAtAnglePlaneRefplaneData_c",
    "moConstraintCoincLineTanOneCylinderRefplaneData_c",
    "moConstraintCoincPointPerpTwoPlnsRefplaneData_c",
];

/// `REFPLN`.
pub(crate) const REFPLN: &[&str] = &["moPerLinePtRefPlnData_c", "moFacePtRefPlnData_c"];

/// `PLNDATA` = the four plane-data classes + `CONSTRAINTPLN` + `REFPLN`.
pub(crate) const PLNDATA: &[&str] = &[
    "moDefaultRefPlnData_c",
    "moFaceRefPlnData_c",
    "moAbsoluteRefPlnData_c",
    "moFixedRefPlnData_c",
    "moConstraintCoincPlaneRefplaneData_c",
    "moConstraintMidPlaneRefplaneData_c",
    "moConstraintCoincLinePerpPlaneRefplaneData_c",
    "moConstraintCoincTwoLinesRefplaneData_c",
    "moConstraintCoincCurveRefplaneData_c",
    "moConstraintCoincLineParallelPlaneRefplaneData_c",
    "moConstraintCoincLineAtAnglePlaneRefplaneData_c",
    "moConstraintCoincTwoPointsPerpPlnRefplaneData_c",
    "moConstraintCoincLinePerpLineRefplaneData_c",
    "moConstraintPerpPlnTanOneCylinderRefplaneData_c",
    "moConstraintTanCylAtAnglePlaneRefplaneData_c",
    "moConstraintCoincLineTanOneCylinderRefplaneData_c",
    "moConstraintCoincPointPerpTwoPlnsRefplaneData_c",
    "moPerLinePtRefPlnData_c",
    "moFacePtRefPlnData_c",
];

/// Three i32 -12345.
pub(crate) const M12345: [u8; 12] = [
    0xc7, 0xcf, 0xff, 0xff, 0xc7, 0xcf, 0xff, 0xff, 0xc7, 0xcf, 0xff, 0xff,
];

/// The shared rule state the Python keeps in module globals.
#[derive(Debug, Clone)]
pub(crate) struct CoreState {
    /// `FEATDIM`: a stack, `[False]` at the start.
    pub(crate) featdim: Vec<bool>,
    /// `OFFDIM`: a stack, `[False]` (0 here) at the start; 2 or 4 when set.
    pub(crate) offdim: Vec<u8>,
    /// `SKBOX[0]`.
    pub(crate) skbox: bool,
}

impl Default for CoreState {
    fn default() -> Self {
        Self {
            featdim: vec![false],
            offdim: vec![0],
            skbox: false,
        }
    }
}

/// `new()`: swVersion >= 11000, or assumed when unknown.
pub(crate) fn new(r: &Reader<'_>) -> bool {
    r.ver.is_none_or(|v| v >= 11000)
}

/// `ver()`.
pub(crate) fn ver(r: &Reader<'_>) -> Option<u32> {
    r.ver
}

/// `(ver() or default)`: a version of 0 or none reads as `default`.
pub(crate) fn ver_or(r: &Reader<'_>, default: u32) -> u32 {
    match r.ver {
        Some(v) if v != 0 => v,
        _ => default,
    }
}

/// `moNodeName_c`.
pub(crate) fn nodename(r: &mut Reader<'_>) -> Res<V> {
    Ok(V::Str(r.cstring()?))
}

/// `named(r)`: the named-object header.
pub(crate) fn named(r: &mut Reader<'_>) -> Res<V> {
    let class = r.last_class();
    let nm = r.ptr(Some(&["moNodeName_c"]))?;
    r.u32()?;
    let fam = r.u16()?;
    let _boolean = r.u8()?;
    let _schema = r.u8()?;
    let oid = r.u32()?;
    r.u32()?;
    let _s = r.cstring()?;
    let name = match nm {
        Ptr::Obj { value, .. } => value,
        other => V::from(other),
    };
    r.named.push(NamedRecord {
        class,
        name: name.clone(),
        oid,
    });
    Ok(V::map([
        ("name", name),
        ("fam", V::from(fam)),
        ("oid", V::from(oid)),
    ]))
}

/// `tailat(b, s)`: the end of the block's +55 string when the block tail's
/// marks follow it.
pub(crate) fn tailat(r: &Reader<'_>, s: usize) -> Res<Option<usize>> {
    if !r.eq(s, &[0xff, 0xfe, 0xff]) {
        return Ok(None);
    }
    let n = usize::from(r.byte(s + 3)?);
    let e = s + 4 + 2 * n;
    Ok((r.eq(e, &[0x00, 0xff, 0xff, 0x00, 0x00]) && r.eq(e + 54, &[0x02, 0x00])).then_some(e))
}

fn fixed(c: &str) -> usize {
    if EXTRUDE.contains(&c) {
        37
    } else {
        35
    }
}

/// `backlink(r, q)`: an end-tag folder's link starts at `q` at once: a
/// back-reference (to its opening folder) and eight zero bytes, with no
/// block-tail u32 or folder u32 0 before it (`KiCad` exports).
pub(crate) fn backlink(r: &Reader<'_>, q: usize) -> bool {
    q + 10 <= r.len() && r.pu16(q).is_ok_and(|w| 0 < w && w < 0x8000) && r.zeros(q + 2, 8)
}

/// `block(r, c, endtag)`: the feature-object block; returns the children.
pub(crate) fn block(r: &mut Reader<'_>, c: &str, endtag: bool) -> Res<V> {
    r.expect_zeros(4, &format!("{c} head zeros"))?;
    let _w4 = r.u16()?;
    let n = r.u16()?;
    let mut kids: Vec<Ptr> = Vec::new();
    for i in 0..n {
        if i != 0 && r.zeros(r.p, 12) && tailat(r, r.p + 12 + fixed(c))?.is_some() && !objat(r, r.p)
        {
            break;
        }
        let kid = r.ptr(None)?;
        let plane = kid.obj_class() == Some("moRefPlane_c");
        kids.push(kid);
        if plane && isclass(r, r.p, PLNDATA)? {
            r.ptr(None)?;
        }
    }
    while n != 0
        && isclass(
            r,
            r.p,
            &[
                "moFoldUpDownSketchXformData_c",
                "moFoldUpDownFeatXformData_c",
            ],
        )?
    {
        kids.push(r.ptr(None)?);
    }
    if c == "moEdgeFlange_c" {
        // A sheet-metal flange's display object follows its counted children, then a block whose fields are open
        // (EV-23); the block is framed by the next named object, as the bend's first form is.
        while isclass(r, r.p, super::a::DISPLAY)? {
            kids.push(r.ptr(None)?);
        }
        let mut i = r.p;
        let end = loop {
            if i >= r.p + 400 || i + 8 >= r.len() {
                return Err(bad("flange tail: no next object"));
            }
            if r.eq(i, &[0xff, 0xfe, 0xff]) {
                let a = r.pu16(i - 4)?;
                let cc = r.pu16(i - 2)?;
                if a & 0x8000 != 0
                    && cc & 0x8000 != 0
                    && r.known
                        .get(&u32::from(cc & 0x7fff))
                        .is_some_and(|s| &**s == "moNodeName_c")
                {
                    break i - 4;
                }
            }
            i += 1;
        };
        r.p = end;
        return Ok(V::from(kids));
    }
    while n != 0 && c == "moSketchBlockInst_c" && isclass(r, r.p, &["moDetailFolder_c"])? {
        kids.push(r.ptr(None)?);
    }
    let mut w = r.pu16(r.p)?;
    if n != 0
        && r.eq(r.p, &[1, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0])
        && tailat(r, r.p + 16 + fixed(c))?.is_some()
    {
        r.skip(4);
        w = 0;
    }
    if n != 0 && w != 0 && w & 0x8000 == 0 {
        ftrlink(r, &format!("{c} children"))?;
    }
    let mut found = None;
    for z in [12_usize, 14, 10, 16, 8, 2] {
        if r.zeros(r.p, z) && tailat(r, r.p + z + fixed(c))?.is_some() {
            found = Some(z);
            break;
        }
    }
    let z = match found {
        Some(z) => z,
        None => {
            if r.zeros(r.p, 8)
                && (1..16).contains(&r.pu32(r.p + 8)?)
                && tailat(r, r.p + 12 + fixed(c))?.is_some()
            {
                12
            } else if c == "moSketchBlockInst_c" && n != 0 && {
                let mut k = None;
                for q in r.p..r.p + 2000 {
                    if r.zeros(q, 12) && tailat(r, q + 47)?.is_some() {
                        k = Some(q);
                        break;
                    }
                }
                if let Some(k) = k {
                    r.p = k;
                    true
                } else {
                    false
                }
            } {
                12
            } else if r.eq(r.p, &[0, 0, 0, 0, 1, 0, 0, 0, 0, 0, 0, 0])
                && tailat(r, r.p + 12 + 35)?.is_some()
            {
                if EXTRUDE.contains(&c) {
                    10
                } else {
                    12
                }
            } else {
                return Err(bad(format!("{c} head zeros 2")));
            }
        }
    };
    r.skip(z);
    r.skip(fixed(c));
    let Some(t) = tailat(r, r.p)? else {
        return Err(bad(format!("{c}: block +55 string and tail")));
    };
    r.p = t + 64;
    let link = endtag && backlink(r, r.p);
    if new(r) && !objat(r, r.p) && !ownnext(r, c) && !link {
        r.skip(4);
    }
    Ok(V::from(kids))
}

/// `OWNB`.
fn ownb(c: &str) -> Option<&'static [u8]> {
    match c {
        "moHistoryFolder_c" => Some(&[0, 0]),
        "moSelectionSetFolder_c" => Some(&[0, 0, 1, 0, 0, 0]),
        _ => None,
    }
}

/// `ownnext(r, c)`.
pub(crate) fn ownnext(r: &Reader<'_>, c: &str) -> bool {
    ownb(c).is_some_and(|o| r.eq(r.p, o) && objat(r, r.p + o.len()))
}

/// `feature(r, c)`: the named header and the block.
pub(crate) fn feature(r: &mut Reader<'_>, c: &str) -> Res<V> {
    let mut n = named(r)?;
    let et = n
        .get("name")
        .as_str()
        .is_some_and(|name| name.ends_with("___EndTag___"));
    let kids = block(r, c, et)?;
    n.set("kids", kids);
    if et && backlink(r, r.p) {
        return Ok(n);
    }
    if new(r) && c.ends_with("Folder_c") && !objat(r, r.p) && !ownnext(r, c) {
        if r.eq(r.p, &[1, 0, 0, 0]) && objat(r, r.p + 4) && c == "moFavoriteFolder_c" {
        } else if r.u32()? != 0 {
            return Err(bad(format!("{c}: folder u32 0")));
        }
    }
    Ok(n)
}

/// `___EndTag___` in UTF-16LE.
const ENDTAG: &[u8] = &[
    b'_', 0, b'_', 0, b'_', 0, b'E', 0, b'n', 0, b'd', 0, b'T', 0, b'a', 0, b'g', 0, b'_', 0, b'_',
    0, b'_', 0,
];

/// `endtag_next(r)`.
pub(crate) fn endtag_next(r: &Reader<'_>) -> Res<bool> {
    let w = r.pu16(r.p)?;
    let c: String = if w == 0xffff {
        let n = usize::from(r.pu16(r.p + 4)?);
        r.slice(r.p + 6, n).iter().map(|&b| char::from(b)).collect()
    } else if w & 0x8000 != 0 {
        r.known
            .get(&u32::from(w & 0x7fff))
            .map_or_else(String::new, ToString::to_string)
    } else {
        return Ok(false);
    };
    let window = r.slice(r.p, 120);
    Ok(c.ends_with("FtrFolder_c") && window.windows(ENDTAG.len()).any(|x| x == ENDTAG))
}

/// `bodyhead(r, c)`: the feature body head.
pub(crate) fn bodyhead(r: &mut Reader<'_>, c: &str) -> Res<V> {
    r.expect_zeros(6, &format!("{c} head"))?;
    let h = r.u16()?;
    r.expect(&[0, 0], "")?;
    let n = r.u16()?;
    for _ in 0..n {
        r.ptr(None)?;
    }
    let f = r.u32()?;
    if f != 0 && f != 1 {
        return Err(bad(format!("{c} head f")));
    }
    if f != 0 {
        let k = r.u16()?;
        for _ in 0..k {
            r.ptr(None)?;
        }
    }
    if h != 0 {
        r.expect(&[0, 0], "")?;
    }
    let m = r.u32()?;
    let mut big_m = Vec::new();
    for _ in 0..m {
        big_m.push(V::from(r.u32()?));
    }
    let t = r.u32()?;
    let g = r.u32()?;
    if !matches!(t, 0 | 2 | 3) || !matches!(g, 0 | 1) {
        return Err(bad(format!("{c} head t={t} g={g}")));
    }
    if g != 0 {
        let k = r.u16()?;
        let mut bodies = Vec::new();
        for i in 0..k {
            if i != 0 {
                let ns = r.pu32(r.p)?;
                let len = 4 + 4 * ns as usize + 12;
                if 0 < ns && ns <= 16 && r.zeros(r.p + len - 8, 8) && objat(r, r.p + len) {
                    r.skip(len);
                }
            }
            bodies.push(r.ptr(None)?);
        }
        return Ok(V::map([
            ("h", V::from(h)),
            ("n", V::from(n)),
            ("f", V::from(f)),
            ("m", V::from(m)),
            ("t", V::from(t)),
            ("g", V::from(g)),
            ("bodies", V::from(bodies)),
        ]));
    }
    let e = r.u32()?;
    let x = r.u16()?;
    if x == 0 {
        r.expect(&[1, 0], "")?;
        if !r.eq(r.p + 18, &[1, 0, 0, 0]) && r.zeros(r.p, 14) && r.eq(r.p + 14, &[1, 0, 0, 0]) {
            r.skip(14);
        } else {
            r.skip(18);
        }
        r.expect(&[1, 0, 0, 0], "")?;
    }
    Ok(V::map([
        ("h", V::from(h)),
        ("n", V::from(n)),
        ("f", V::from(f)),
        ("m", V::from(m)),
        ("t", V::from(t)),
        ("g", V::from(g)),
        ("e", V::from(e)),
        ("x", V::from(x)),
        ("M", V::List(big_m)),
    ]))
}

/// `isver(b, p)`: a u32 that looks like a `SolidWorks` version.
pub(crate) fn isver(r: &Reader<'_>, p: usize) -> bool {
    if p + 4 > r.len() {
        return false;
    }
    r.pu32(p)
        .is_ok_and(|x| (9000..=40000).contains(&x) && x % 100 == 0)
}

/// `objat(r, q)`: an object (a declaration or a known class's tag) at `q`.
pub(crate) fn objat(r: &Reader<'_>, q: usize) -> bool {
    if q + 2 > r.len() {
        return false;
    }
    let Ok(w) = r.pu16(q) else {
        return false;
    };
    (w == 0xffff && r.eq(q + 2, &[1, 0]))
        || (w & 0x8000 != 0 && w != 0xffff && r.known.contains_key(&u32::from(w & 0x7fff)))
}

/// `axistail(r)`.
pub(crate) fn axistail(r: &mut Reader<'_>) -> Res<V> {
    if r.p >= 8 && r.ffs(r.p - 8, 4) && r.zeros(r.p - 4, 8) && !r.zeros(r.p + 4, 8) {
        r.skip(4);
        let mut ax = Vec::new();
        for _ in 0..7 {
            ax.push(V::from(r.f64()?));
        }
        r.expect(&[0], "axis u8 0")?;
        return Ok(V::List(ax));
    }
    let mut z = 0;
    while r.byte(r.p + z)? == 0 {
        z += 1;
    }
    r.skip(z);
    r.expect(&[1, 0, 0, 0, 0, 0, 0, 0], "axis 1 0")?;
    r.u32()?;
    r.expect_zeros(12, "axis 12 zero")?;
    let mut m = M12345.to_vec();
    m.extend_from_slice(&[0; 4]);
    r.expect(&m, "axis -12345")?;
    r.u32()?;
    if r.ffs(r.p + 12, 4) {
        let mut x = vec![0; 12];
        x.extend_from_slice(&[0xff; 4]);
        x.extend_from_slice(&[0; 8]);
        r.expect(&x, "axis -1")?;
    } else {
        r.expect_zeros(20, "axis 20 zero")?;
    }
    let mut ax = Vec::new();
    for _ in 0..7 {
        ax.push(V::from(r.f64()?));
    }
    r.expect(&[0], "axis u8 0")?;
    Ok(V::List(ax))
}

/// `refblock(r)`: a reference's handle block.
pub(crate) fn refblock(r: &mut Reader<'_>) -> Res<()> {
    if r.eq(r.p, &[0, 0, 1, 0]) && isclass(r, r.p + 4, &["moContextChainComponent_c"])? {
        r.skip(4);
        r.ptr(None)?;
        if r.zeros(r.p, 12) && !r.ffs(r.p + 12, 4) {
            r.skip(12);
        } else {
            let mut x = vec![0; 12];
            x.extend_from_slice(&[0xff; 4]);
            r.expect(&x, "ref block context -1")?;
        }
        return Ok(());
    }
    let mut z = 0;
    while r.byte(r.p + z)? == 0 {
        z += 1;
    }
    if r.eq(r.p + z, &[1, 0, 0, 0]) && r.eq(r.p + z + 24, &M12345) {
        r.skip(z + 8);
        r.u32()?;
        r.expect_zeros(12, "ref block 12 zero")?;
    } else if r.zeros(r.p, 10) && r.zeros(r.p + 14, 12) && r.eq(r.p + 26, &M12345) {
        r.skip(10);
        r.u32()?;
        r.skip(12);
    } else {
        r.expect_zeros(10, "ref block 10 zero")?;
        r.u32()?;
        r.expect(
            &[0, 0, 0, 0, 0xff, 0xff, 0xff, 0xff, 0, 0, 0, 0],
            "ref block 0 -1 0",
        )?;
    }
    let mut m = M12345.to_vec();
    m.extend_from_slice(&[0; 4]);
    r.expect(&m, "ref block -12345")?;
    r.u32()?;
    if !r.ffs(r.p + 12, 4) {
        // No i32 -1 (14000 and 15000 samples).
        r.expect_zeros(12, "ref block 12 zero (no -1)")?;
    } else {
        let mut x = vec![0; 12];
        x.extend_from_slice(&[0xff; 4]);
        r.expect(&x, "ref block -1")?;
    }
    Ok(())
}

/// `refgeo(r, kind)`.
pub(crate) fn refgeo(r: &mut Reader<'_>, kind: &str) -> Res<()> {
    refblock(r)?;
    if kind == "moCylinderRef_w" {
        for _ in 0..11 {
            r.f64()?;
        }
        r.expect(&[0, 0], "cylinder u16 0")?;
    } else if kind == "moSurfaceRef_w" {
        for _ in 0..4 {
            r.f64()?;
        }
        r.u32()?;
        for _ in 0..9 {
            r.f64()?;
        }
    } else {
        return Err(bad(format!("thread reference {kind}")));
    }
    Ok(())
}

/// `is76(b, q)`.
pub(crate) fn is76(r: &Reader<'_>, q: usize) -> bool {
    let t = r.slice(q, 76);
    t.len() == 76
        && t[0] == 0
        && t[4..8] == [0x00, 0x01, 0x00, 0x00]
        && (t[64..68] == [1, 0, 0, 0] || t[64..68] == [0, 0, 0, 0])
        && t[68..76] == [0; 8]
}

/// `isclass(r, q, names)`: the object at `q` is of one of `names`.
pub(crate) fn isclass(r: &Reader<'_>, q: usize, names: &[&str]) -> Res<bool> {
    let w = r.pu16(q)?;
    if w == 0xffff {
        let n = usize::from(r.pu16(q + 4)?);
        let c: String = r.slice(q + 6, n).iter().map(|&b| char::from(b)).collect();
        return Ok(names.contains(&c.as_str()));
    }
    Ok(w & 0x8000 != 0
        && r.known
            .get(&u32::from(w & 0x7fff))
            .is_some_and(|c| names.contains(&&**c)))
}

/// `ftrlink(r, c)`.
pub(crate) fn ftrlink(r: &mut Reader<'_>, c: &str) -> Res<V> {
    r.ptr(None)?;
    r.expect_zeros(8, &format!("{c} 8 zero"))?;
    let mut run = Vec::new();
    loop {
        let v = r.pu32(r.p)?;
        if v <= 2 && run.len() < 16 {
            run.push(V::from(v));
            r.skip(4);
        } else {
            break;
        }
    }
    r.ptr(None)?;
    Ok(V::List(run))
}

/// `units(r)`.
pub(crate) fn units(r: &mut Reader<'_>) -> Res<()> {
    r.u16()?;
    loop {
        r.obj(Some(&[
            "moPerBodyChooserData_c",
            "moPerBodyChooserDataWithFileName_c",
        ]))?;
        let bb = r.obj(Some(&["moBBoxCenterData_c"]))?;
        if !bb.value().is_some_and(|v| v.get("more").truthy()) {
            break;
        }
    }
    Ok(())
}

/// `GEO_END.search(r.b, start, end)`: the match's start and end.
fn geo_end(r: &Reader<'_>, start: usize, end: usize) -> Option<(usize, usize)> {
    let end = end.min(r.len());
    let head = [0, 0, 0, 0, 0, 0, 0xf0, 0x3f];
    let tail = |at: usize| {
        let Some(&kind) = r.b.get(at) else {
            return false;
        };
        at + 12 <= end
            && matches!(kind, 2..=4)
            && r.b[at + 1..at + 12] == [0, 0, 0, 0xff, 0xff, 0xff, 0xff, 0, 0, 0, 0]
    };
    for s in start..end {
        if s + 8 > end || r.b[s..s + 8] != head {
            continue;
        }
        // `(?:\x00|\x00\x00\x00)`: one zero byte first, then three.
        if s + 9 <= end && r.b[s + 8] == 0 && tail(s + 9) {
            return Some((s, s + 9 + 12));
        }
        if s + 11 <= end && r.b[s + 8..s + 11] == [0, 0, 0] && tail(s + 11) {
            return Some((s, s + 11 + 12));
        }
    }
    None
}

/// `geo_frame(r, limit)`.
pub(crate) fn geo_frame(r: &mut Reader<'_>, limit: usize) -> Res<()> {
    let Some((m_start, m_end)) = geo_end(r, r.p, r.p + limit) else {
        return Err(bad("face geometry: no trailer"));
    };
    if r.find(&[0xff, 0xff, 0x01, 0x00], r.p, m_start).is_some() {
        return Err(bad("face geometry: object inside the run"));
    }
    r.p = m_end;
    r.u32()?;
    if isclass(r, r.p + 2, &["moContourChooser_c"])? {
        r.u16()?;
        r.ptr(None)?;
        let k = r.u16()?;
        for _ in 0..k {
            r.ptr(None)?;
        }
        r.expect(&[0, 0], "geometry chooser zero")?;
        return Ok(());
    }
    let w = r.pu16(r.p + 2)?;
    if r.pu16(r.p)? != 0 && (w == 0xffff || w & 0x8000 != 0) {
        for _ in 0..2 {
            let k = r.u16()?;
            for _ in 0..k {
                r.ptr(None)?;
            }
        }
        r.expect(&[0, 0], "geometry trailer 2 zero")?;
    } else {
        r.u32()?;
        let w = r.pu16(r.p + 2)?;
        if r.pu16(r.p)? != 0 && (w == 0xffff || w & 0x8000 != 0) {
            for _ in 0..2 {
                let k = r.u16()?;
                for _ in 0..k {
                    r.ptr(None)?;
                }
            }
            r.expect(&[0, 0, 0, 0], "geometry trailer 4 zero")?;
        } else {
            let zz = [8_usize, 4]
                .into_iter()
                .find(|&z| r.zeros(r.p, z) && objat(r, r.p + z));
            if let Some(zz) = zz {
                r.skip(zz);
            } else {
                r.expect_zeros(6, "geometry trailer 6 zero")?;
            }
        }
    }
    Ok(())
}

/// `BOX`: f64 -1, 1, 1, -1.
const BOX: [u8; 32] = {
    let mut out = [0; 32];
    let values = [
        (-1.0_f64).to_le_bytes(),
        1.0_f64.to_le_bytes(),
        1.0_f64.to_le_bytes(),
        (-1.0_f64).to_le_bytes(),
    ];
    let mut i = 0;
    while i < 4 {
        let mut j = 0;
        while j < 8 {
            out[i * 8 + j] = values[i][j];
            j += 1;
        }
        i += 1;
    }
    out
};

/// `oldgeo(r)`: reads the older face-plane form, or nothing and `false`.
pub(crate) fn oldgeo(r: &mut Reader<'_>) -> Res<bool> {
    let q = r.p + 12;
    let Some(j) = r.find(&M12345[..8], q, q + 40) else {
        return Ok(false);
    };
    let Some(k) = r.find(&BOX, j, j + 100) else {
        return Ok(false);
    };
    let between = r.slice(j + 8, k.saturating_sub(j + 8));
    let zeros = between.iter().filter(|&&b| b == 0).count();
    if zeros < between.len().saturating_sub(12) && between.len() >= 12 {
        return Ok(false);
    }
    let mut p = k + 32 + 48;
    let fl = r.byte(p)?;
    if fl > 1 {
        return Ok(false);
    }
    p += 1 + 8 * if fl != 0 { 12 } else { 3 };
    if r.pf64(p)? != 1.0 {
        return Ok(false);
    }
    p += 8;
    let mut z = 0;
    while r.byte(p + z)? == 0 && z < 64 {
        z += 1;
    }
    if !objat(r, p + z) && !r.eq(p + z, &[0, 0]) {
        return Ok(false);
    }
    r.p = p + z;
    Ok(true)
}
