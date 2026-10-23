//! Feature rules (`g_feat.py`): the named-object header, the feature-object
//! block and body head, folders, reference planes and their data, components,
//! profiles, parameters, dimensions, bodies, fillets, sheet metal, patterns
//! and geometry records.

use super::super::reader::{bad, NamedRecord, ParameterRecord, Ptr, Reader, Res, Stop, V};
use super::{ctx, sketch};

const FFFEFF: [u8; 3] = [0xff, 0xfe, 0xff];

/// The display-dimension classes.
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
pub(crate) fn block(reader: &mut Reader<'_>, class: &str, endtag: bool) -> Res<V> {
    reader.expect_zeros(4, &format!("{class} head zeros"))?;
    let _w4 = reader.u16()?;
    let count = reader.u16()?;
    let mut kids: Vec<Ptr> = Vec::new();
    for cursor in 0..count {
        if cursor != 0
            && reader.zeros(reader.p, 12)
            && tailat(reader, reader.p + 12 + fixed(class))?.is_some()
            && !objat(reader, reader.p)
        {
            break;
        }
        let kid = reader.ptr(None)?;
        let plane = kid.obj_class() == Some("moRefPlane_c");
        kids.push(kid);
        if plane && isclass(reader, reader.p, PLNDATA)? {
            reader.ptr(None)?;
        }
    }
    while count != 0
        && isclass(
            reader,
            reader.p,
            &[
                "moFoldUpDownSketchXformData_c",
                "moFoldUpDownFeatXformData_c",
            ],
        )?
    {
        kids.push(reader.ptr(None)?);
    }
    if class == "moEdgeFlange_c" {
        // A sheet-metal flange's display object follows its counted children, then a block whose fields are open
        // (EV-23); the block is framed by the next named object, as the bend's first form is.
        while isclass(reader, reader.p, DISPLAY)? {
            kids.push(reader.ptr(None)?);
        }
        let mut cursor = reader.p;
        let end = loop {
            if cursor >= reader.p + 400 || cursor + 8 >= reader.len() {
                return Err(bad("flange tail: no next object"));
            }
            if reader.eq(cursor, &[0xff, 0xfe, 0xff]) {
                let first_ = reader.pu16(cursor - 4)?;
                let cc = reader.pu16(cursor - 2)?;
                if first_ & 0x8000 != 0
                    && cc & 0x8000 != 0
                    && reader
                        .known
                        .get(&u32::from(cc & 0x7fff))
                        .is_some_and(|s| &**s == "moNodeName_c")
                {
                    break cursor - 4;
                }
            }
            cursor += 1;
        };
        reader.p = end;
        return Ok(V::from(kids));
    }
    while count != 0
        && class == "moSketchBlockInst_c"
        && isclass(reader, reader.p, &["moDetailFolder_c"])?
    {
        kids.push(reader.ptr(None)?);
    }
    let mut word = reader.pu16(reader.p)?;
    if count != 0
        && reader.eq(reader.p, &[1, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0])
        && tailat(reader, reader.p + 16 + fixed(class))?.is_some()
    {
        reader.skip(4);
        word = 0;
    }
    if count != 0 && word != 0 && word & 0x8000 == 0 {
        ftrlink(reader, &format!("{class} children"))?;
    }
    let mut found = None;
    for zeros_ in [12_usize, 14, 10, 16, 8, 2] {
        if reader.zeros(reader.p, zeros_)
            && tailat(reader, reader.p + zeros_ + fixed(class))?.is_some()
        {
            found = Some(zeros_);
            break;
        }
    }
    let zeros_ = match found {
        Some(zeros_) => zeros_,
        None => {
            if (reader.zeros(reader.p, 8)
                && (1..16).contains(&reader.pu32(reader.p + 8)?)
                && tailat(reader, reader.p + 12 + fixed(class))?.is_some())
                || (class == "moSketchBlockInst_c" && count != 0 && {
                    let mut found_ = None;
                    for q in reader.p..reader.p + 2000 {
                        if reader.zeros(q, 12) && tailat(reader, q + 47)?.is_some() {
                            found_ = Some(q);
                            break;
                        }
                    }
                    if let Some(found_) = found_ {
                        reader.p = found_;
                        true
                    } else {
                        false
                    }
                })
            {
                12
            } else if reader.eq(reader.p, &[0, 0, 0, 0, 1, 0, 0, 0, 0, 0, 0, 0])
                && tailat(reader, reader.p + 12 + 35)?.is_some()
            {
                if EXTRUDE.contains(&class) {
                    10
                } else {
                    12
                }
            } else {
                return Err(bad(format!("{class} head zeros 2")));
            }
        }
    };
    reader.skip(zeros_);
    reader.skip(fixed(class));
    let Some(kind) = tailat(reader, reader.p)? else {
        return Err(bad(format!("{class}: block +55 string and tail")));
    };
    reader.p = kind + 64;
    let link = endtag && backlink(reader, reader.p);
    if new(reader) && !objat(reader, reader.p) && !ownnext(reader, class) && !link {
        reader.skip(4);
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
pub(crate) fn bodyhead(reader: &mut Reader<'_>, class: &str) -> Res<V> {
    reader.expect_zeros(6, &format!("{class} head"))?;
    let head_ = reader.u16()?;
    reader.expect_bytes(&[0, 0], "")?;
    let count_ = reader.u16()?;
    for _ in 0..count_ {
        reader.ptr(None)?;
    }
    let flag = reader.u32()?;
    if flag != 0 && flag != 1 {
        return Err(bad(format!("{class} head f")));
    }
    if flag != 0 {
        let found = reader.u16()?;
        for _ in 0..found {
            reader.ptr(None)?;
        }
    }
    if head_ != 0 {
        reader.expect_bytes(&[0, 0], "")?;
    }
    let mask = reader.u32()?;
    let mut big_m = Vec::new();
    for _ in 0..mask {
        big_m.push(V::from(reader.u32()?));
    }
    let kind = reader.u32()?;
    let groups = reader.u32()?;
    if !matches!(kind, 0 | 2 | 3) || !matches!(groups, 0 | 1) {
        return Err(bad(format!("{class} head t={kind} g={groups}")));
    }
    if groups != 0 {
        let found = reader.u16()?;
        let mut bodies = Vec::new();
        for i in 0..found {
            if i != 0 {
                let ns = reader.pu32(reader.p)?;
                let ns = usize::try_from(ns)
                    .map_err(|_| bad("body selector count exceeds address space"))?;
                let len = 4 + 4 * ns + 12;
                if 0 < ns
                    && ns <= 16
                    && reader.zeros(reader.p + len - 8, 8)
                    && objat(reader, reader.p + len)
                {
                    reader.skip(len);
                }
            }
            bodies.push(reader.ptr(None)?);
        }
        return Ok(V::map([
            ("h", V::from(head_)),
            ("n", V::from(count_)),
            ("f", V::from(flag)),
            ("m", V::from(mask)),
            ("t", V::from(kind)),
            ("g", V::from(groups)),
            ("bodies", V::from(bodies)),
        ]));
    }
    let entry = reader.u32()?;
    let x_value = reader.u16()?;
    if x_value == 0 {
        reader.expect_bytes(&[1, 0], "")?;
        if !reader.eq(reader.p + 18, &[1, 0, 0, 0])
            && reader.zeros(reader.p, 14)
            && reader.eq(reader.p + 14, &[1, 0, 0, 0])
        {
            reader.skip(14);
        } else {
            reader.skip(18);
        }
        reader.expect_bytes(&[1, 0, 0, 0], "")?;
    }
    Ok(V::map([
        ("h", V::from(head_)),
        ("n", V::from(count_)),
        ("f", V::from(flag)),
        ("m", V::from(mask)),
        ("t", V::from(kind)),
        ("g", V::from(groups)),
        ("e", V::from(entry)),
        ("x", V::from(x_value)),
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
        r.expect_bytes(&[0], "axis u8 0")?;
        return Ok(V::List(ax));
    }
    let mut z = 0;
    while r.byte(r.p + z)? == 0 {
        z += 1;
    }
    r.skip(z);
    r.expect_bytes(&[1, 0, 0, 0, 0, 0, 0, 0], "axis 1 0")?;
    r.u32()?;
    r.expect_zeros(12, "axis 12 zero")?;
    let mut m = M12345.to_vec();
    m.extend_from_slice(&[0; 4]);
    r.expect_bytes(&m, "axis -12345")?;
    r.u32()?;
    if r.ffs(r.p + 12, 4) {
        let mut x = vec![0; 12];
        x.extend_from_slice(&[0xff; 4]);
        x.extend_from_slice(&[0; 8]);
        r.expect_bytes(&x, "axis -1")?;
    } else {
        r.expect_zeros(20, "axis 20 zero")?;
    }
    let mut ax = Vec::new();
    for _ in 0..7 {
        ax.push(V::from(r.f64()?));
    }
    r.expect_bytes(&[0], "axis u8 0")?;
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
            r.expect_bytes(&x, "ref block context -1")?;
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
        r.expect_bytes(
            &[0, 0, 0, 0, 0xff, 0xff, 0xff, 0xff, 0, 0, 0, 0],
            "ref block 0 -1 0",
        )?;
    }
    let mut m = M12345.to_vec();
    m.extend_from_slice(&[0; 4]);
    r.expect_bytes(&m, "ref block -12345")?;
    r.u32()?;
    if r.ffs(r.p + 12, 4) {
        let mut x = vec![0; 12];
        x.extend_from_slice(&[0xff; 4]);
        r.expect_bytes(&x, "ref block -1")?;
    } else {
        // No i32 -1 (14000 and 15000 samples).
        r.expect_zeros(12, "ref block 12 zero (no -1)")?;
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
        r.expect_bytes(&[0, 0], "cylinder u16 0")?;
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
pub(crate) fn isclass(reader: &Reader<'_>, at: usize, names: &[&str]) -> Res<bool> {
    let word = reader.pu16(at)?;
    if word == 0xffff {
        let count = usize::from(reader.pu16(at + 4)?);
        let class: String = reader
            .slice(at + 6, count)
            .iter()
            .map(|&b| char::from(b))
            .collect();
        return Ok(names.contains(&class.as_str()));
    }
    Ok(word & 0x8000 != 0
        && reader
            .known
            .get(&u32::from(word & 0x7fff))
            .is_some_and(|class| names.contains(&&**class)))
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
        r.expect_bytes(&[0, 0], "geometry chooser zero")?;
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
        r.expect_bytes(&[0, 0], "geometry trailer 2 zero")?;
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
            r.expect_bytes(&[0, 0, 0, 0], "geometry trailer 4 zero")?;
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
pub(crate) fn oldgeo(reader: &mut Reader<'_>) -> Res<bool> {
    let at = reader.p + 12;
    let Some(stop) = reader.find(&M12345[..8], at, at + 40) else {
        return Ok(false);
    };
    let Some(found) = reader.find(&BOX, stop, stop + 100) else {
        return Ok(false);
    };
    let start = stop + 8;
    let between = if found > start {
        reader.slice(start, found - start)
    } else {
        &[]
    };
    let zeros = between
        .iter()
        .fold(0_usize, |count, &byte| count + usize::from(byte == 0));
    if between.len() >= 12 && zeros < between.len() - 12 {
        return Ok(false);
    }
    let mut pos = found + 32 + 48;
    let fl = reader.byte(pos)?;
    if fl > 1 {
        return Ok(false);
    }
    pos += 1 + 8 * if fl != 0 { 12 } else { 3 };
    if reader.pf64(pos)? != 1.0 {
        return Ok(false);
    }
    pos += 8;
    let mut zeros_ = 0;
    while reader.byte(pos + zeros_)? == 0 && zeros_ < 64 {
        zeros_ += 1;
    }
    if !objat(reader, pos + zeros_) && !reader.eq(pos + zeros_, &[0, 0]) {
        return Ok(false);
    }
    reader.p = pos + zeros_;
    Ok(true)
}

/// `GEO_END.search(r.b, start, end) is not None` (the search core's
/// `geo_frame` makes).
fn geo_end_found(r: &Reader<'_>, start: usize, end: usize) -> bool {
    let end = end.min(r.len());
    let b = r.b;
    let head = [0, 0, 0, 0, 0, 0, 0xf0, 0x3f];
    let tail = |at: usize| {
        let Some(&kind) = b.get(at) else {
            return false;
        };
        at + 12 <= end
            && matches!(kind, 2..=4)
            && b[at + 1..at + 12] == [0, 0, 0, 0xff, 0xff, 0xff, 0xff, 0, 0, 0, 0]
    };
    (start..end).any(|s| {
        s + 8 <= end
            && b[s..s + 8] == head
            && ((s + 9 <= end && b[s + 8] == 0 && tail(s + 9))
                || (s + 11 <= end && b[s + 8..s + 11] == [0, 0, 0] && tail(s + 11)))
    })
}

/// `x[1]` of a pointer tuple: an object's class, a reference's index; a null
/// pointer has none (`IndexError`).
fn item1(r: &Reader<'_>, p: &Ptr) -> Res<V> {
    match p {
        Ptr::Obj { class, .. } => Ok(V::from(&**class)),
        Ptr::Ref { index, .. } => Ok(V::from(*index)),
        Ptr::Null => Err(Stop::End(r.p)),
    }
}

/// `nonefolder(r, c)`.
pub(crate) fn nonefolder(r: &mut Reader<'_>, c: &str) -> Res<V> {
    let mut n = feature(r, c)?;
    if n.get("name")
        .as_str()
        .is_some_and(|name| name.ends_with("___EndTag___"))
    {
        // An end-tag folder's link is one pointer, back to its opening folder.
        let open = r.ptr(None)?;
        n.set("open", V::from(open));
        return Ok(n);
    }
    let mut z0 = 0;
    while c == "moDetailFolder_c" && z0 < 64 && r.p + z0 < r.len() && r.byte(r.p + z0)? == 0 {
        z0 += 1;
    }
    if c == "moDetailFolder_c" && z0 >= 24 && r.byte(r.p + z0)? == 1 && !objat(r, r.p) {
        r.skip(z0);
        let fl = r.u8()?;
        for _ in 0..if fl != 0 { 12 } else { 3 } {
            r.f64()?;
        }
        if r.f64()? != 1.0 {
            return Err(bad("detail folder view scale"));
        }
        let mut z = 0;
        while r.p + z < r.len() && r.byte(r.p + z)? == 0 && z < 200 {
            z += 1;
        }
        if !objat(r, r.p + z) {
            return Err(bad("detail folder view end"));
        }
        r.skip(z);
    }
    if endtag_next(r)? {
        ftrlink(r, c)?;
    }
    Ok(n)
}

/// `moSurfaceBodyFolder_c`, `moFeatSurfaceBodyFolder_c`,
/// `moFeatSolidBodyFolder_c`.
pub(crate) fn zero12(r: &mut Reader<'_>, _c: Option<&str>) -> Res<V> {
    let n = feature(r, "moSurfaceBodyFolder_c")?;
    r.expect_zeros(12, "twelve zero bytes")?;
    Ok(n)
}

/// `moSolidBodyFolder_c`.
pub(crate) fn solidfolder(r: &mut Reader<'_>) -> Res<V> {
    let n = feature(r, "moSolidBodyFolder_c")?;
    if r.eq(r.p, &[1, 0, 0, 0, 0, 0, 0, 0, 0, 0])
        && isclass(r, r.p + 10, &["moCutListSortOptions_c"])?
    {
        r.skip(10);
        r.ptr(None)?;
        return Ok(n);
    }
    r.expect_zeros(12, "twelve zero bytes")?;
    let w = r.pu16(r.p)?;
    if w != 0 && w & 0x8000 == 0 {
        r.u16()?;
    }
    Ok(n)
}

/// `moInkMarkupFolder_c`.
pub(crate) fn inkfolder(r: &mut Reader<'_>) -> Res<V> {
    let n = feature(r, "moInkMarkupFolder_c")?;
    let w = r.pu16(r.p)?;
    if w != 0 && w & 0x8000 == 0 {
        r.u16()?;
    }
    Ok(n)
}

/// `moMaterialFolder_c`.
pub(crate) fn matfolder(r: &mut Reader<'_>) -> Res<V> {
    let n = feature(r, "moMaterialFolder_c")?;
    r.cstring()?;
    r.u32()?;
    Ok(n)
}

/// `moDetailCabinet_c`.
pub(crate) fn detailcabinet(r: &mut Reader<'_>) -> Res<V> {
    let n = feature(r, "moDetailCabinet_c")?;
    let q = r.p;
    match cabinettail(r, n.clone()) {
        Err(Stop::Bad(message)) => {
            if !new(r) {
                return Err(Stop::Bad(message));
            }
            // The block's tail without its u32 (`KiCad` exports declaring
            // swVersion 17000): the cabinet's own fields start 4 bytes
            // earlier, and the rest of the stream follows the layout before
            // 11000.
            r.p = q - 4;
            let v = cabinettail(r, n)?;
            r.ver = Some(10000);
            Ok(v)
        }
        other => other,
    }
}

/// `cabinettail(r, n)`: the detail cabinet's own fields after its block.
fn cabinettail(r: &mut Reader<'_>, n: V) -> Res<V> {
    let k = r.u16()?;
    if k == 1 {
        r.u16()?;
    } else if k != 0 {
        return Err(bad("detail cabinet u16"));
    }
    r.f64()?;
    r.f64()?;
    let first = if ver_or(r, 99999) < 12000 { 6 } else { 10 };
    for z in [first, 2] {
        if r.zeros(r.p, z) && (z != 2 || objat(r, r.p + 2)) {
            r.skip(z);
            return Ok(n);
        }
    }
    Err(bad("detail cabinet zero bytes"))
}

/// `moRefPlane_c`, `moRefAxis_c` (both called with `c` = `moRefPlane_c`).
pub(crate) fn refplane(r: &mut Reader<'_>, c: &str) -> Res<V> {
    let n = feature(r, c)?;
    bodyhead(r, c)?;
    r.expect_zeros(6, "")?;
    r.u32()?;
    r.expect_zeros(44, "")?;
    r.f64()?;
    r.f64()?;
    r.f64()?;
    r.u32()?;
    let z = if matches!(ver(r), Some(v) if v < 13000) {
        6
    } else {
        10
    };
    r.expect_zeros(z, "")?;
    Ok(n)
}

/// The plane-data classes (`PLNDATA`).
pub(crate) fn refplndata(r: &mut Reader<'_>) -> Res<V> {
    let mut pt = Vec::new();
    for _ in 0..6 {
        pt.push(V::from(r.f64()?));
    }
    let fl = r.u8()?;
    if fl > 1 {
        return Err(bad("refplndata flag"));
    }
    let m = if fl != 0 {
        let mut m = Vec::new();
        for _ in 0..9 {
            m.push(V::from(r.f64()?));
        }
        V::List(m)
    } else {
        V::None
    };
    for _ in 0..4 {
        r.f64()?;
    }
    r.u8()?;
    let mut bx = Vec::new();
    for _ in 0..4 {
        bx.push(V::from(r.f64()?));
    }
    r.skip(6);
    r.u32()?;
    r.u32()?;
    r.u8()?;
    let last = r.last_class();
    let last = last.as_deref();
    let face = last == Some("moFaceRefPlnData_c");
    let absolute = matches!(last, Some("moAbsoluteRefPlnData_c" | "moFixedRefPlnData_c"));
    let constraint = last.is_some_and(|c| CONSTRAINTPLN.contains(&c));
    let byref = last.is_some_and(|c| REFPLN.contains(&c));
    if !(face || absolute || constraint || byref) {
        for _ in 0..3 {
            r.f64()?;
        }
    }
    let kind = r.u32()?;
    let accepted = kind == 1 || (face && kind == 5) || constraint || (byref && kind < 16);
    if !accepted {
        return Err(bad("refplndata u32 1"));
    }
    r.skip(if ver_or(r, 99999) < 12000 { 6 } else { 12 });
    if constraint {
        let refs = r.ptr(Some(&["moConstraintRefplaneInterfaceData_c"]))?;
        return Ok(V::map([
            ("pt", V::List(pt)),
            ("M", m),
            ("box", V::List(bx)),
            ("kind", V::from(kind)),
            ("refs", V::from(refs)),
        ]));
    }
    if byref {
        r.st.sk.set_byref(true);
        let refs = byref_refs(r);
        r.st.sk.set_byref(false);
        let refs = refs?;
        let done = V::map([
            ("pt", V::List(pt)),
            ("M", m),
            ("box", V::List(bx)),
            (
                "refs",
                V::from(u32::try_from(refs).map_err(|_| bad("reference count exceeds u32"))?),
            ),
        ]);
        if r.eq(r.p, &[0, 0, 0, 0, 0, 0, 0, 0, 1, 0, 0, 0]) && objat(r, r.p + 12) {
            r.skip(12);
            return Ok(done);
        }
        if !(r.zeros(r.p, 4) && objat(r, r.p + 75)) {
            return Err(bad("plane by references: tail"));
        }
        r.skip(4);
        for _ in 0..7 {
            r.f64()?;
        }
        r.skip(15);
        return Ok(done);
    }
    let reference = if face { V::from(r.ptr(None)?) } else { V::None };
    let mut zz = 0;
    while zz < 8 && r.byte(r.p + zz)? == 0 {
        zz += 1;
    }
    if face && r.eq(r.p, &[0, 0, 1, 0]) && isclass(r, r.p + 4, &["moContextChainComponent_c"])? {
        r.skip(4);
        r.ptr(Some(&["moContextChainComponent_c"]))?;
        geo_frame(r, 700)?;
    } else if face
        && r.eq(r.p + zz, &[1, 0, 0, 0])
        && (zz >= 2 || r.slice(r.p, 48).windows(8).any(|x| x == &M12345[..8]))
    {
        if geo_end_found(r, r.p, r.p + 700) {
            geo_frame(r, 700)?;
        } else {
            oldgeo(r)?;
        }
    }
    Ok(V::map([
        ("pt", V::List(pt)),
        ("M", m),
        ("box", V::List(bx)),
        ("ref", reference),
    ]))
}

/// The references of a plane by references (inside `BYREF`).
fn byref_refs(r: &mut Reader<'_>) -> Res<usize> {
    let mut refs = 1;
    r.ptr(None)?;
    loop {
        if r.zeros(r.p, 4) && r.zeros(r.p + 28, 4) && objat(r, r.p + 32) {
            r.skip(4);
            for _ in 0..3 {
                r.f64()?;
            }
            r.skip(4);
        }
        if !objat(r, r.p) {
            break;
        }
        r.ptr(None)?;
        refs += 1;
    }
    Ok(refs)
}

/// `moUnitComponent_c`.
pub(crate) fn unitcomponent(r: &mut Reader<'_>) -> Res<V> {
    if r.u16()? != 2 {
        return Err(bad("unit component u16 2"));
    }
    let flags = r.u16()?;
    r.skip(39);
    if ver_or(r, 99999) < 11000 {
        // Before 11000: no 16 ff bytes (KiCad exports).
        return Ok(V::from(flags));
    }
    r.expect_bytes(&[0xff; 16], "unit component ff")?;
    let v = ver_or(r, 99999);
    r.skip(if v < 12000 {
        8
    } else if v < 13000 {
        16
    } else {
        20
    });
    if isver(r, r.p) {
        r.skip(4);
    }
    Ok(V::from(flags))
}

/// `moCompFeature_c`, `moCompSheetmetalFeat_c`.
pub(crate) fn compfeature(r: &mut Reader<'_>) -> Res<V> {
    let c = r.ptr(Some(UCS))?;
    if c.is_null() {
        return Err(bad("component null"));
    }
    let fid = r.u32()?;
    let ts = r.u32()?;
    Ok(V::map([("fid", V::from(fid)), ("ts", V::from(ts))]))
}

/// `moCompFace_c`.
pub(crate) fn compface(r: &mut Reader<'_>) -> Res<V> {
    r.obj(Some(UCS))?;
    r.u8()?;
    if r.eq(r.p, &FFFEFF) {
        r.cstring()?;
    }
    Ok(V::from(r.ptr(None)?))
}

/// `moCompEdge_c`, `moCompLoop_c`, `moCompVertex_c`, `moCompSilhEdge_c`,
/// `moCompSilhVertex_c`.
pub(crate) fn compedge(r: &mut Reader<'_>) -> Res<V> {
    let me = r.last_class();
    r.obj(Some(UCS))?;
    r.u8()?;
    if r.eq(r.p, &FFFEFF) {
        r.cstring()?;
    }
    let e = r.ptr(None)?;
    if matches!(
        me.as_deref(),
        Some("moCompSilhEdge_c" | "moCompSilhVertex_c")
    ) {
        while isclass(r, r.p, &["moFaceRef_c"])? {
            r.ptr(None)?;
        }
    }
    let w = if r.p + 64 <= r.len() {
        r.pu16(r.p + 62)?
    } else {
        0
    };
    if r.zeros(r.p, 62) && (w == 0xffff || w & 0x8000 != 0) {
        r.skip(62);
    }
    Ok(V::from(e))
}

/// `moHistoryFeatItemData_c`.
pub(crate) fn histitem(r: &mut Reader<'_>) -> Res<V> {
    r.expect_bytes(&[0, 0], "")?;
    r.u32()?;
    r.u32()?;
    r.u32()?;
    r.u32()?;
    r.cstring()?;
    Ok(V::from(r.ptr(None)?))
}

/// `moHistoryFolder_c`.
pub(crate) fn histfolder(r: &mut Reader<'_>) -> Res<V> {
    let n = feature(r, "moHistoryFolder_c")?;
    let k = r.u16()?;
    for _ in 0..k {
        r.obj(Some(&["moHistoryFeatItemData_c"]))?;
    }
    Ok(n)
}

/// `moFavoriteFolder_c`.
pub(crate) fn favfolder(r: &mut Reader<'_>, _c: &str) -> Res<V> {
    let n = feature(r, "moFavoriteFolder_c")?;
    if r.eq(r.p, &[1, 0, 0, 0]) && objat(r, r.p + 4) {
        r.skip(4);
        return Ok(n);
    }
    r.expect_zeros(4, "")?;
    r.expect_bytes(&[1, 0, 0, 0], "")?;
    Ok(n)
}

/// `moSelectionSetFolder_c`.
pub(crate) fn selfolder(r: &mut Reader<'_>) -> Res<V> {
    let n = feature(r, "moSelectionSetFolder_c")?;
    r.expect_bytes(&[0, 0], "")?;
    r.expect_bytes(&[1, 0, 0, 0], "")?;
    Ok(n)
}

/// `moOriginProfileFeature_c`, `moProfileFeature_c`, `moSketchBlockDef_c`.
pub(crate) fn profile(r: &mut Reader<'_>, c: &str) -> Res<V> {
    let cls = r
        .last_class()
        .map_or_else(|| c.to_owned(), |c| c.to_string());
    let mut n = feature(r, c)?;
    if r.zeros(r.p, 6) && !r.zeros(r.p + 6, 4) && isclass(r, r.p + 12, &["moContourChooser_c"])? {
        r.skip(6);
        r.u32()?;
        r.ptr(None)?;
        r.ptr(Some(&["moContourChooser_c"]))?;
        return Ok(n);
    }
    r.expect_zeros(10, "")?;
    let sk = r.ptr(Some(&["sgSketch"]))?;
    if cls == "moSketchBlockDef_c" {
        sketch::planeframe(r, false)?;
        if r.zeros(r.p + 20, 8) && objat(r, r.p + 28) && !r.zeros(r.p, 16) {
            let guid = super::super::reader::hex(r.slice(r.p, 16));
            n.set("guid", V::from(guid));
            r.skip(28);
        }
        return Ok(n);
    }
    if sk.is_ref() && (r.eq(r.p, &[1, 0, 0, 0, 1, 0, 0, 0]) || r.zeros(r.p, 8)) {
        r.skip(8);
        while isclass(r, r.p, &["moSketchChain_c"])? {
            r.ptr(None)?;
        }
        sketch::sktail(r)?;
    }
    Ok(n)
}

/// `moLengthParameter_c`, `moAngleParameter_c`, `moScalerParameter_c`
/// (`integer` false) and, through `intparameter`, `moIntegerParameter_c`.
pub(crate) fn parameter(r: &mut Reader<'_>, integer: bool) -> Res<V> {
    let entry = r.trace.last().cloned();
    let pc = entry
        .as_ref()
        .map_or_else(String::new, |t| t.class.to_string());
    let n = named(r)?;
    r.expect_bytes(&[0, 0], "")?;
    let value_at = r.p;
    let val = if integer {
        V::from(r.u32()?)
    } else {
        V::from(r.f64()?)
    };
    let name = n.get("name").clone();
    let featdim = r.st.core.featdim.last().copied().unwrap_or(false);
    if featdim {
        let oid = if r.byte(r.p)? == 0 && isclass(r, r.p + 1, &["moDimTolerance_c"])? {
            r.skip(1);
            r.ptr(None)?;
            V::None
        } else {
            let q = r.p;
            r.skip(23);
            if r.slice(q, 23).len() < 7 {
                return Err(Stop::End(r.p));
            }
            V::from(r.pu32(q + 3)?)
        };
        let Some(j) = r.find(&FFFEFF, r.p, r.p + 2000) else {
            return Err(bad("feature dim placement: no string"));
        };
        r.p = j;
        for _ in 0..4 {
            r.cstring()?;
        }
        let mut l = if pc == "moScalerParameter_c" || pc == "moIntegerParameter_c" {
            234
        } else {
            262
        };
        if !r.eq(r.p + l, &FFFEFF) && r.eq(r.p + 262, &FFFEFF) {
            l = 262;
        }
        r.skip(l);
        r.cstring()?;
        r.skip(40);
        r.cstring()?;
        record(r, entry.as_ref(), &name, &val, value_at);
        return Ok(V::map([("name", name), ("value", val), ("oid", oid)]));
    }
    let x = usize::from(ver_or(r, 0) >= 18000);
    r.expect_bytes(&[0], "parameter u8 0")?;
    r.ptr(Some(&["moDimTolerance_c"]))?;
    let q = r.p;
    r.skip(32 + x);
    let tlen = r.slice(q, 32 + x).len();
    if tlen < 4 {
        return Err(Stop::End(r.p));
    }
    let oid = r.pu32(q)?;
    if tlen < 26 + x {
        return Err(Stop::End(r.p));
    }
    let cnt = r.pu16(q + 24 + x)?;
    if tlen < 30 + x {
        return Err(Stop::End(r.p));
    }
    let role = r.pu32(q + 26 + x)?;
    let off = r.st.core.offdim.last().copied().unwrap_or(0);
    let mut ops = Vec::new();
    if off != 0 {
        for _ in 0..cnt {
            ops.push(if off == 4 {
                V::from(r.u32()?)
            } else {
                V::from(r.u16()?)
            });
        }
    } else {
        let mut ptrs: Vec<Ptr> = Vec::new();
        for _ in 0..cnt {
            let q = r.p;
            let x = r.ptr(None)?;
            if ptrs.len() >= 2
                && x.is_ref()
                && !x.ref_class().unwrap_or("").ends_with("Handle")
                && ptrs
                    .iter()
                    .all(|o| o.obj_class().is_some_and(|c| c.ends_with("Handle")))
            {
                r.p = q;
                break;
            }
            ptrs.push(x);
        }
        ops = ptrs.into_iter().map(V::from).collect();
    }
    record(r, entry.as_ref(), &name, &val, value_at);
    Ok(V::map([
        ("name", name),
        ("value", val),
        ("oid", V::from(oid)),
        ("role", V::from(role)),
        ("ops", V::List(ops)),
    ]))
}

/// Record a parameter object the rule read.
fn record(
    r: &mut Reader<'_>,
    entry: Option<&super::super::reader::Trace>,
    name: &V,
    val: &V,
    value_at: usize,
) {
    if let Some(t) = entry {
        r.parameters.push(ParameterRecord {
            class: t.class.clone(),
            name: name.clone(),
            value: val.as_f64().unwrap_or(f64::NAN),
            at: t.at,
            value_at,
        });
    }
}

/// `moIntegerParameter_c`.
pub(crate) fn intparameter(r: &mut Reader<'_>) -> Res<V> {
    parameter(r, true)
}

/// `moScale_c`.
pub(crate) fn scale(r: &mut Reader<'_>) -> Res<V> {
    let n = feature(r, "moScale_c")?;
    let hd = bodyhead(r, "moScale_c")?;
    if hd.get("x").as_int() == Some(1) {
        units(r)?;
    }
    r.ptr(Some(&["moPMarkRecord_c"]))?;
    r.u32()?;
    let mut k = Vec::new();
    for _ in 0..3 {
        k.push(V::from(r.f64()?));
    }
    r.expect_zeros(6, "scale 6 zero")?;
    r.expect_bytes(&[1, 0], "scale u16 1")?;
    r.ptr(None)?;
    let count = r.u32()?;
    for _ in 0..count {
        r.u32()?;
    }
    r.expect_bytes(&[0xff; 4], "scale -1")?;
    r.expect_zeros(12, "scale 12 zero")?;
    Ok(V::map([("n", n), ("factors", V::List(k))]))
}

/// A body head's `g == 1` continuation: u32 count and that many body ids,
/// then X, two u32, e and the u16 `x`, which it returns.
fn body_ids(r: &mut Reader<'_>) -> Res<u16> {
    let k = r.u32()?;
    if k > 4096 {
        return Err(bad("body id count"));
    }
    for _ in 0..k {
        r.u32()?;
    }
    r.u32()?;
    r.u32()?;
    r.u32()?;
    r.u32()?;
    r.u16()
}

/// `moSweepThread_c`.
pub(crate) fn sweepthread(r: &mut Reader<'_>) -> Res<V> {
    let n = feature(r, "moSweepThread_c")?;
    let hd = bodyhead(r, "moSweepThread_c")?;
    units_after_head(r, &hd)?;
    r.ptr(Some(&["moPMarkRecord_c"]))?;
    r.cstring()?;
    r.u32()?;
    r.cstring()?;
    r.u32()?;
    r.cstring()?;
    r.cstring()?;
    r.expect_zeros(36, "thread 36 zero")?;
    r.u32()?;
    let pitch = r.f64()?;
    let size = r.cstring()?;
    let lib = r.cstring()?;
    r.ptr(None)?;
    r.expect_zeros(62, "thread 62 zero")?;
    r.expect_bytes(&[1, 0, 0, 0, 0, 0, 0, 0], "thread 1 0")?;
    r.u32()?;
    r.expect_zeros(12, "thread 12 zero")?;
    let mut m = M12345.to_vec();
    m.extend_from_slice(&[0; 4]);
    r.expect_bytes(&m, "thread -12345")?;
    r.u32()?;
    let mut z12ff = vec![0; 12];
    z12ff.extend_from_slice(&[0xff; 4]);
    r.expect_bytes(&z12ff, "thread 12 zero -1")?;
    r.f64()?;
    r.f64()?;
    let mut ffb = vec![0];
    ffb.extend_from_slice(&[0xff; 8]);
    ffb.extend_from_slice(&[0; 6]);
    r.expect_bytes(&ffb, "thread ff block")?;
    for _ in 0..7 {
        r.f64()?;
    }
    r.expect_bytes(&[1, 0, 0, 0, 0, 0, 0, 0], "thread 1 0 b")?;
    let face = r.ptr(None)?;
    if face.obj_class() == Some("moSingleFaceRef_w") {
        r.expect_zeros(10, "thread face 10 zero")?;
        r.u32()?;
        r.expect_bytes(
            &[0, 0, 0, 0, 0xff, 0xff, 0xff, 0xff, 0, 0, 0, 0],
            "thread face 0 -1 0",
        )?;
        r.expect_bytes(&m, "thread face -12345")?;
        r.u32()?;
        r.expect_bytes(&z12ff, "thread face -1")?;
        for _ in 0..4 {
            r.f64()?;
        }
        r.expect_zeros(4, "thread face u32 0")?;
        r.u32()?;
        r.u32()?;
    } else if !face.is_null() {
        return Err(bad("thread face form"));
    }
    let mut one_one = vec![1, 0, 0, 0, 1, 0, 0, 0];
    one_one.extend_from_slice(&[0; 26]);
    if !r.eq(r.p + 40, &one_one) && objat(r, r.p + 74) {
        r.skip(74);
    } else {
        r.skip(40);
        r.expect_bytes(&one_one, "thread 1 1 26 zero")?;
    }
    while objat(r, r.p) {
        let x = r.ptr(None)?;
        // `objat` admits only objects here, so `x[1]` is a class name.
        let Some(class) = x.obj_class().map(str::to_owned) else {
            return Err(bad("thread object is not an object"));
        };
        if class.ends_with("Ref_w") {
            refgeo(r, &class)?;
        }
    }
    Ok(V::map([
        ("n", n),
        ("size", V::from(size)),
        ("pitch", V::from(pitch)),
        ("lib", V::from(lib)),
    ]))
}

/// `moRevEndSpec_c`.
pub(crate) fn revendspec(r: &mut Reader<'_>) -> Res<V> {
    let t = r.u32()?;
    if !r.zeros(r.p, 8) || !r.zeros(r.p + 12, 12) {
        return Err(bad("rev end 24 zero"));
    }
    r.pu32(r.p + 8)?;
    r.skip(24);
    r.f64()?;
    r.f64()?;
    r.expect_zeros(8, "rev end 8 zero")?;
    Ok(V::from(t))
}

/// `moSketchBlockInst_c`.
pub(crate) fn blockinst(r: &mut Reader<'_>) -> Res<V> {
    let c = r.last_class().ok_or(Stop::End(r.p))?;
    let n = feature(r, &c)?;
    r.expect_bytes(&[0, 0], "block instance u16 0")?;
    r.skip(16);
    r.expect_bytes(&[0, 0, 1, 0], "block instance 0 1")?;
    r.ptr(Some(&["moAbsolutePoint_c"]))?;
    r.skip(160);
    Ok(n)
}

/// A body head's `g == 1` continuation (with the u16 1, 18 bytes and u32 1
/// when `x` is 0), then the units when `x` is 1 (`sweepthread`,
/// `featpattern`).
fn units_after_head(r: &mut Reader<'_>, hd: &V) -> Res<()> {
    let mut hx = hd.get("x").as_int();
    if hd.get("g").as_int() == Some(1) {
        let x = body_ids(r)?;
        hx = Some(i64::from(x));
        if x == 0 {
            r.expect_bytes(&[1, 0], "")?;
            r.skip(18);
            r.expect_bytes(&[1, 0, 0, 0], "")?;
        }
    }
    if hx == Some(1) {
        units(r)?;
    }
    Ok(())
}

/// `moLPattern_c`, `moCirPattern_c`, `moMirrorPattern_c`.
pub(crate) fn featpattern(reader: &mut Reader<'_>) -> Res<V> {
    let class = reader.last_class().ok_or(Stop::End(reader.p))?;
    let class = &*class;
    let count = feature(reader, class)?;
    let hd = bodyhead(reader, class)?;
    units_after_head(reader, &hd)?;
    reader.ptr(Some(&["moPMarkRecord_c"]))?;
    reader.ptr(Some(&["suObList"]))?;
    reader.ptr(Some(&["suObList"]))?;
    if !reader.eq(reader.p, &[1, 0, 0, 0, 0, 0, 1, 0, 0, 0, 0])
        && !reader.eq(reader.p, &[1, 0, 0, 0, 0, 0, 2, 0, 0, 0, 0])
    {
        return Err(bad("pattern head"));
    }
    reader.skip(11);
    reader.ptr(Some(&["suObList"]))?;
    reader.skip(if class == "moCirPattern_c" { 228 } else { 192 });
    reader.ptr(Some(&["suObList"]))?;
    reader.expect_zeros(
        if class == "moMirrorPattern_c" { 13 } else { 11 },
        "pattern zero run",
    )?;
    display_objects(reader)?;
    if class == "moLPattern_c" {
        reader.u32()?;
        reader.expect_bytes(&[0, 0], "lpattern u16 0")?;
        let d1 = reader.ptr(None)?;
        item1(reader, &d1)?;
        axistail(reader)?;
        let d2 = reader.ptr(None)?;
        if d2.is_obj() {
            axistail(reader)?;
        }
        let mut x_value = vec![0; 13];
        x_value.extend_from_slice(&[1, 0, 0, 0, 1, 0, 0, 0]);
        x_value.extend_from_slice(&[0; 12]);
        reader.expect_bytes(&x_value, "lpattern 1 1")?;
        display_objects(reader)?;
        let mut zeros_ = 0;
        while reader.p + zeros_ < reader.len() && reader.byte(reader.p + zeros_)? == 0 {
            zeros_ += 1;
        }
        if reader.p + zeros_ == reader.len()
            || zeros_ == 9
            || (zeros_ == 13 && objat(reader, reader.p + 13))
        {
            reader.skip(zeros_);
            return Ok(count);
        }
        return Err(bad(format!("lpattern end {zeros_}")));
    }
    if class == "moCirPattern_c" {
        reader.expect_bytes(&[0, 0], "cirpattern u16 0")?;
        let ax = reader.ptr(None)?;
        item1(reader, &ax)?;
        axistail(reader)?;
        if reader.u8()? > 1 {
            return Err(bad("cirpattern flag byte"));
        }
        display_objects(reader)?;
        reader.expect_bytes(
            &[1, 0, 0, 0, 0, 1, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0],
            "cirpattern end",
        )?;
        if !objat(reader, reader.p) && reader.p < reader.len() && reader.zeros(reader.p, 8) {
            reader.skip(8);
            for _ in 0..5 {
                reader.f64()?;
            }
        } else if reader.p + 52 == reader.len() {
            reader.skip(52);
        }
        return Ok(count);
    }
    if class == "moMirrorPattern_c" {
        reader.st.sk.set_mirror(true);
        let pl = reader.ptr(Some(&["moCompRefPlane_c"]));
        reader.st.sk.set_mirror(false);
        pl?;
        return Ok(count);
    }
    Err(bad("pattern form not read (EV-17)"))
}

/// `while isclass(r, r.p, DISPLAY): objs.append(r.ptr()[1])`.
fn display_objects(r: &mut Reader<'_>) -> Res<()> {
    while isclass(r, r.p, DISPLAY)? {
        let p = r.ptr(None)?;
        item1(r, &p)?;
    }
    Ok(())
}

/// `moInkMarkupView_c`.
pub(crate) fn inkview(r: &mut Reader<'_>) -> Res<V> {
    let mut n = feature(r, "moInkMarkupView_c")?;
    if r.p >= 4 && r.eq(r.p - 4, &FFFEFF) {
        let k = r.byte(r.p - 1)?;
        r.skip(2 * usize::from(k));
    }
    let fl = r.u8()?;
    if fl > 1 {
        return Err(bad("ink view flag"));
    }
    for _ in 0..if fl != 0 { 12 } else { 3 } {
        r.f64()?;
    }
    r.skip(101);
    if isclass(r, r.p, &["moDetailFolder_c"])? {
        r.ptr(None)?;
        r.skip(104);
        r.u32()?;
        let author = r.cstring()?;
        r.u32()?;
        n.set("author", V::from(author));
    }
    Ok(n)
}

/// `moFtrFolder_c` and the folder classes registered with it.
pub(crate) fn ftrfolder(r: &mut Reader<'_>, _c: &str) -> Res<V> {
    let mut n = feature(r, "moFtrFolder_c")?;
    if n.get("name")
        .as_str()
        .is_some_and(|name| name.ends_with("___EndTag___"))
    {
        let open = r.ptr(None)?;
        n.set("open", V::from(open));
        return Ok(n);
    }
    ftrlink(r, "moFtrFolder_c")?;
    Ok(n)
}

/// `moPerBodyChooserData_c`, `moPerBodyChooserDataWithFileName_c`.
pub(crate) fn perbody(r: &mut Reader<'_>) -> Res<V> {
    let n = r.u16()?;
    let mut refs = Vec::new();
    for _ in 0..n {
        refs.push(r.ptr(None)?);
    }
    r.expect_bytes(&[0, 0, 1, 0], "body chooser 0 1")?;
    Ok(V::from(refs))
}

/// `moBBoxCenterData_c`.
pub(crate) fn bbox(r: &mut Reader<'_>) -> Res<V> {
    if r.u32()? != 1 {
        return Err(bad("bbox u32 1"));
    }
    let mut v = Vec::new();
    for _ in 0..4 {
        v.push(V::from(r.f64()?));
    }
    if r.st.core.skbox {
        r.u32()?;
        let bid = r.u16()?;
        let c = r.u16()?;
        r.expect_zeros(14, "sketch box 14 zero")?;
        let x = r.u32()?;
        if 1 < c && c < 16 && r.zeros(r.p, 2 * usize::from(c - 1)) {
            r.skip(2 * usize::from(c - 1));
        }
        return Ok(V::map([
            ("v", V::List(v)),
            ("id", V::from(bid)),
            ("x", V::from(x)),
        ]));
    }
    r.u32()?;
    r.u32()?;
    let n = r.pu16(r.p)?;
    if 0 < n && n <= 64 && (100_000..100_064).contains(&r.pu32(r.p + 2)?) {
        r.skip(2);
        for _ in 0..n {
            r.u32()?;
        }
        r.expect_zeros(4, "bbox ids u32 0")?;
    } else {
        let fl = r.u32()?;
        if r.eq(r.p, &[0, 0]) && r.eq(r.p + 2, &FFFEFF) {
            r.skip(2);
            r.cstring()?;
            r.expect_zeros(4, "bbox guid u32 0")?;
        }
        if fl != 0 {
            r.u32()?;
        }
        r.u16()?;
        if objat(r, r.p) {
            return Ok(V::map([
                ("v", V::List(v)),
                ("id", V::None),
                ("more", V::Bool(true)),
            ]));
        }
    }
    let bid = r.u16()?;
    let more = r.u16()?;
    r.u32()?;
    if more != 0 {
        r.u16()?;
    }
    let mut tail_last = None;
    for l in [6_usize, 8, 20, 10] {
        let w = r.pu16(r.p + l)?;
        if l == 6 {
            if r.pu32(r.p)? <= 1 && (w == 0xffff || w & 0x8000 != 0) {
                r.skip(6);
                tail_last = Some(0);
                break;
            }
            continue;
        }
        if l == 10 {
            if r.slice(r.p, 10).iter().all(|&x| x <= 1) && (w == 0xffff || w & 0x8000 != 0) {
                r.skip(10);
                tail_last = Some(0);
                break;
            }
            continue;
        }
        let mut vals = Vec::new();
        for i in 0..l / 4 {
            vals.push(r.pu32(r.p + 4 * i)?);
        }
        if l == 8 && vals.iter().all(|&x| x <= 1) && zrun_to(r, r.p + 8, "suObList")? >= 40 {
            r.skip(8);
            tail_last = vals.last().copied();
            break;
        }
        // Before an extrusion's zero block in place of its mark record.
        if l == 20 && vals.iter().all(|&x| x <= 1) && zrun_to(r, r.p + 20, "moEndSpec_c")? >= 40 {
            r.skip(20);
            tail_last = vals.last().copied();
            break;
        }
        if vals.iter().all(|&x| x <= 1 || x == 0x10001) && (w == 0xffff || w & 0x8000 != 0) {
            r.skip(l);
            tail_last = vals.last().copied();
            break;
        }
    }
    let Some(tail_last) = tail_last else {
        return Err(bad("bbox tail"));
    };
    Ok(V::map([
        ("v", V::List(v)),
        ("id", V::from(bid)),
        ("more", V::Bool(more != 0 && tail_last == 0x10001)),
    ]))
}

/// `zrun_to(r, q, cls)`: the length of the zero run at `q` when an object of
/// class `cls` follows it, else 0.
pub(crate) fn zrun_to(r: &Reader<'_>, q: usize, cls: &str) -> Res<usize> {
    let mut z = 0;
    while q + z < r.len() && r.byte(q + z)? == 0 && z < 4096 {
        z += 1;
    }
    Ok(if isclass(r, q + z, &[cls])? { z } else { 0 })
}

/// `is76` over a byte string: its first 76 bytes.
fn is76_bytes(t: &[u8]) -> bool {
    t.len() >= 76
        && t[0] == 0
        && t[4..8] == [0x00, 0x01, 0x00, 0x00]
        && (t[64..68] == [1, 0, 0, 0] || t[64..68] == [0, 0, 0, 0])
        && t[68..76] == [0; 8]
}

const REVOLVES: &[&str] = &["moRevolution_c", "moRevCut_c", "moRevolutionThin_c"];

/// The extrusion family (`moExtrusion_c`, `moRevolution_c`, `moSweepCut_c`,
/// `moSMBaseFlange_c`, ...).
pub(crate) fn extrusion(reader: &mut Reader<'_>, class: &str) -> Res<V> {
    let cls = reader
        .last_class()
        .map_or_else(|| class.to_owned(), |class| class.to_string());
    let cls = cls.as_str();
    let count = feature(reader, "moExtrusion_c")?;
    let hd = bodyhead(reader, "moExtrusion_c")?;
    let mut hx = hd.get("x").as_int();
    if hd.get("g").as_int() == Some(1) {
        hx = Some(i64::from(body_ids(reader)?));
    }
    if hx == Some(1) {
        units(reader)?;
    }
    if REVOLVES.contains(&cls) && zrun_to(reader, reader.p, "suObList")? >= 40 {
        let z = zrun_to(reader, reader.p, "suObList")?;
        reader.skip(z - 9);
    } else if !REVOLVES.contains(&cls) && zrun_to(reader, reader.p, "moEndSpec_c")? >= 40 {
        // Zero bytes in place of the mark record (64; extruded cuts).
        let z = zrun_to(reader, reader.p, "moEndSpec_c")?;
        reader.skip(z);
    } else {
        reader.ptr(Some(&["moPMarkRecord_c"]))?;
    }
    if cls == "moSMBaseFlange_c" {
        let mut dims = 0_u32;
        while isclass(reader, reader.p, DISPLAY)? {
            reader.ptr(None)?;
            dims += 1;
        }
        let mut found = None;
        for q in reader.p..reader.p + 40 {
            if isclass(reader, q, &["moCompSheetmetalFeat_c"])? {
                found = Some(q);
                break;
            }
        }
        let Some(found) = found else {
            return Err(bad("base flange: no sheet metal component"));
        };
        reader.p = found;
        reader.ptr(None)?;
        let entry = (reader.p..reader.p + 200).find(|&q| q >= reader.len() || objat(reader, q));
        let Some(entry) = entry else {
            return Err(bad("base flange tail None"));
        };
        let tail = if entry == 0 {
            0
        } else {
            reader.slice(reader.p, entry - reader.p).len()
        };
        if !matches!(tail, 71 | 74 | 75) {
            return Err(bad(format!("base flange tail {tail}")));
        }
        reader.p = entry;
        return Ok(V::map([("n", count), ("dims", V::from(dims))]));
    }
    if (cls == "moSweepCut_c" || cls == "moSweep_c")
        && reader.eq(reader.p, &[1, 0])
        && isclass(reader, reader.p + 2, &["moGeneralCurveRef_w"])?
    {
        reader.skip(2);
        let path = reader.ptr(None)?;
        reader.expect_bytes(&[1, 0], "sweep u16 1")?;
        reader.ptr(Some(&["moWireContact_c"]))?;
        reader.ptr(Some(&["moGeneralCurveRef_w"]))?;
        reader.skip(105);
        let guide = reader.ptr(None)?;
        reader.skip(17);
        return Ok(V::map([
            ("n", count),
            ("path", V::from(path)),
            ("guide", V::from(guide)),
        ]));
    }
    if REVOLVES.contains(&cls) {
        if reader.zeros(reader.p, 5)
            && reader.eq(reader.p + 5, &[1, 0, 0, 0])
            && isclass(reader, reader.p + 9, &["suObList"])?
        {
            reader.skip(9);
        } else {
            reader.expect_zeros(9, "revolve 9 zero")?;
        }
        reader.ptr(Some(&["suObList"]))?;
        reader.u32()?;
        reader.u32()?;
        reader.ptr(None)?;
        if ver_or(reader, 99999) >= 16000 {
            reader.expect_zeros(4, "revolve u32 0")?;
        }
        for _ in 0..7 {
            reader.f64()?;
        }
        reader.expect_bytes(&[0], "revolve u8 0")?;
        reader.ptr(Some(&["moRevEndSpec_c"]))?;
    } else {
        reader.ptr(Some(&["moEndSpec_c"]))?;
    }
    let mut dims = 0_u32;
    loop {
        let word = reader.pu16(reader.p)?;
        let nm: String = if word == 0xffff {
            let found = usize::from(reader.pu16(reader.p + 4)?);
            reader
                .slice(reader.p + 6, found)
                .iter()
                .map(|&b| char::from(b))
                .collect()
        } else if word & 0x8000 != 0 {
            reader
                .known
                .get(&u32::from(word & 0x7fff))
                .map_or_else(String::new, ToString::to_string)
        } else {
            String::new()
        };
        if !nm.starts_with("moDisplay") {
            break;
        }
        reader.ptr(None)?;
        dims += 1;
    }
    if REVOLVES.contains(&cls) {
        let mut z = if ver_or(reader, 99999) >= 18000 { 7 } else { 6 };
        if z == 6
            && reader.zeros(reader.p, 7)
            && objat(reader, reader.p + 7)
            && !objat(reader, reader.p + 6)
        {
            z = 7;
        }
        reader.expect_zeros(z, "revolve zero bytes")?;
        return Ok(V::map([("n", count), ("dims", V::from(dims))]));
    }
    if reader.eq(reader.p, &[0, 0, 1, 1, 0]) && objat(reader, reader.p + 5) {
        reader.skip(5);
        reader.ptr(None)?;
        refblock(reader)?;
        if reader.zeros(reader.p, 2) && reader.pf64(reader.p + 2)?.abs() == 1.0 {
            reader.skip(2); // u16 0 before the four gaps (1 sample)
        }
        for _ in 0..4 {
            reader.f64()?;
        }
        reader.expect_zeros(4, "up-to-face u32 0")?;
        reader.u32()?;
        reader.u32()?;
        let t = reader.slice(reader.p, 71);
        let sub = |a: usize, b: usize| t.get(a.min(t.len())..b.min(t.len())).unwrap_or(&[]);
        if sub(0, 3) != [1, 0, 0]
            || !(sub(59, 63) == [1, 0, 0, 0] || sub(59, 63) == [0, 0, 0, 0])
            || sub(63, 71) != [0; 8]
        {
            return Err(bad("up-to-face block tail"));
        }
        reader.skip(71);
    } else if reader.eq(reader.p, &[0])
        && reader.eq(reader.p + 4, &[0, 1, 0, 0])
        && objat(reader, reader.p + 40)
    {
        // A drafted extrusion: the block's pointer slot at +40 holds the draft
        // angle's display dimension, then the block's last 34 bytes.
        reader.skip(40);
        reader.ptr(None)?;
        let t = reader.slice(reader.p, 34);
        let sub = |a: usize, b: usize| t.get(a.min(t.len())..b.min(t.len())).unwrap_or(&[]);
        if sub(0, 22) != [0; 22]
            || !(sub(22, 26) == [1, 0, 0, 0] || sub(22, 26) == [0, 0, 0, 0])
            || sub(26, 34) != [0; 8]
        {
            return Err(bad("drafted extrusion block tail"));
        }
        reader.skip(34);
    } else if !is76(reader, reader.p) {
        let mut joined = reader.slice(reader.p, 4).to_vec();
        joined.extend_from_slice(reader.slice(reader.p + 6, 74));
        if reader.eq(reader.p + 4, &[0, 0]) && is76_bytes(&joined) && objat(reader, reader.p + 78) {
            reader.skip(78);
        } else {
            return Err(bad("extrusion 76-byte block"));
        }
    } else {
        reader.skip(76);
    }
    reader.ptr(Some(&["moFromEndSpec_c"]))?;
    if reader.p >= reader.len() {
        return Ok(V::map([("n", count), ("dims", V::from(dims))]));
    }
    reader.ptr(None)?;
    if reader.p < reader.len() {
        reader.ptr(None)?;
    }
    Ok(V::map([("n", count), ("dims", V::from(dims))]))
}

/// `moCompSolidBody_c`, `moCompSurfaceBody_c`.
pub(crate) fn compsolid(r: &mut Reader<'_>) -> Res<V> {
    r.obj(Some(UCS))?;
    Ok(V::from(r.ptr(None)?))
}

/// `moPMarkRecord_c`.
pub(crate) fn pmark(r: &mut Reader<'_>) -> Res<V> {
    let big_x = r.u32()?;
    let f = r.u32()?;
    if f == 1 {
        let n = r.u32()?;
        let entries =
            usize::try_from(n).map_err(|_| bad("pmark entry count exceeds address space"))?;
        r.skip(6 * entries);
        r.expect_zeros(8, "pmark 8 zero")?;
        r.u32()?;
        r.u32()?;
        r.u32()?;
        r.expect_zeros(16, "pmark 16 zero")?;
        return Ok(V::List(vec![V::from("list"), V::from(big_x), V::from(n)]));
    }
    if f == 0 {
        return pmark_zero(r, big_x);
    }
    let one = r.u32()?;
    let b = r.u32()?;
    if one != 1 || !r.zeros(r.p, 16) {
        return Err(bad(format!("pmark flag {f}")));
    }
    r.expect_zeros(16, "pmark range 16 zero")?;
    r.u32()?;
    if r.zeros(r.p, 30) {
        r.skip(30);
    } else {
        let mut z = 0;
        while z < 30 && r.byte(r.p + z)? == 0 {
            z += 1;
        }
        z -= z % 2;
        let nh = if z >= 8 { r.pu32(r.p + z)? } else { 0 };
        if !(0 < nh && nh <= 64 && objat(r, r.p + z + 4)) {
            r.expect_zeros(30, "pmark range 30 zero")?;
        }
        r.skip(z);
    }
    let mut hops = 0_u32;
    let k = r.u32()?;
    for _ in 0..k {
        r.ptr(None)?;
        hops += 1;
        if r.eq(r.p, &[0, 0, 0xff, 0xfe, 0xff]) {
            r.skip(2);
            r.cstring()?;
        }
    }
    let mut refs = 0_u32;
    let k = r.u16()?;
    for _ in 0..k {
        r.ptr(None)?;
        refs += 1;
    }
    let n2 = if r.p + 20 <= r.len() {
        r.pu32(r.p + 16)?
    } else {
        0
    };
    if hops != 0 && r.zeros(r.p, 16) && 0 < n2 && n2 <= 64 && objat(r, r.p + 20) {
        r.skip(16);
        let k = r.u32()?;
        for _ in 0..k {
            r.ptr(None)?;
            hops += 1;
        }
        let k = r.u16()?;
        for _ in 0..k {
            r.ptr(None)?;
            refs += 1;
        }
    }
    if isclass(r, r.p, &["suObList"])? {
        r.ptr(None)?;
    } else if ver_or(r, 99999) >= 13000 {
        r.expect_zeros(6, "pmark range 6 zero")?;
    }
    Ok(V::List(vec![
        V::from("range"),
        V::from(big_x),
        V::from(f),
        V::from(b),
        V::from(hops),
        V::from(refs),
    ]))
}

/// `moPMarkRecord_c`'s zero form (flag 0).
fn pmark_zero(r: &mut Reader<'_>, big_x: u32) -> Res<V> {
    let z12 = r.slice(r.p, 12);
    if z12.len() < 12 || z12[4..] != [0; 8] {
        return Err(bad("pmark 32 zero"));
    }
    r.skip(12);
    for _ in 0..2 {
        let c1 = r.u32()?;
        if c1 > 64 {
            return Err(bad("pmark 32 zero"));
        }
        for _ in 0..c1 {
            r.u32()?;
            r.u32()?;
        }
        r.expect_zeros(4, "pmark list end 0")?;
    }
    r.expect_zeros(4, "pmark lists u32 0")?;
    let pid = r.u32()?;
    let w2 = r.pu16(r.p + 4)?;
    if r.pu16(r.p)? & 0x8000 == 0
        && r.pu32(r.p)? < 1_000_000
        && w2 & 0x8000 != 0
        && !r.ffs(r.p + 8, 4)
        && objat(r, r.p + 4)
    {
        r.u32()?;
    }
    let obj_kind: &str;
    if r.pu16(r.p)? & 0x8000 != 0 {
        let mut obj = r.ptr(None)?;
        r.u16()?;
        while r.eq(r.p, &FFFEFF)
            && r.pu16(r.p + 4 + 2 * usize::from(r.byte(r.p + 3)?))? & 0x8000 != 0
        {
            r.cstring()?;
            obj = r.ptr(None)?;
            r.u16()?;
        }
        if r.eq(r.p, &FFFEFF) {
            r.cstring()?;
        }
        obj_kind = obj.kind();
    } else {
        let n = r.u32()?;
        if n > 4096 {
            return Err(bad("pmark entry count"));
        }
        let width = if n != 0 && r.ffs(r.p + 4, 4) { 6 } else { 2 };
        for _ in 0..n {
            for _ in 0..width {
                r.u32()?;
            }
        }
        if n != 0 {
            r.expect_zeros(4, "pmark list u32 0")?;
        }
        obj_kind = "n";
    }
    let g = r.u32()?;
    if g == 0 {
        r.expect_zeros(14, "pmark 14 zero")?;
        return Ok(V::List(vec![
            V::from("zero"),
            V::from(big_x),
            V::from(pid),
            V::from(obj_kind),
        ]));
    }
    if (1..=16).contains(&g) {
        let mut guid = String::new();
        for _ in 0..g {
            r.u32()?;
            guid = r.cstring()?;
        }
        let mut z = 0;
        while z < 16 && r.byte(r.p + z)? == 0 {
            z += 1;
        }
        if z < 12 {
            return Err(bad("pmark guid zeros"));
        }
        r.skip(14);
        return Ok(V::List(vec![
            V::from("guid"),
            V::from(big_x),
            V::from(guid),
            V::None,
        ]));
    }
    Err(bad(format!("pmark g {g}")))
}

/// `FILLET_END`: a fillet's constant 40-byte end block.
const FILLET_END: [u8; 40] = [
    0x06, 0x00, 0x00, 0x00, 0x00, 0x00, 0x30, 0x52, 0x00, 0x00, 0x19, 0x00, //
    0x00, 0x00, 0x2d, 0x5b, 0x00, 0x00, 0xb7, 0x58, 0x00, 0x00, 0xd1, 0x59, //
    0x00, 0x00, //
    0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
];

const PMARK: &[&str] = &["moPMarkRecord_c"];
const SUOBLIST: &[&str] = &["suObList"];
const EF: &[&str] = &["moCompEdge_c", "moCompFace_c"];
const BLOB_MARK: [u8; 4] = [0x23, 0x1d, 0xd5, 0x71];

fn list(values: Vec<V>) -> V {
    V::List(values)
}

/// `b'\xff' * f + bytes(t)`.
fn zff_zero(f: usize, t: usize) -> Vec<u8> {
    let mut out = vec![0; 0];
    out.extend(std::iter::repeat_n(0xff, f));
    out.extend(std::iter::repeat_n(0, t));
    out
}

/// `bytes(12) + b'\xff' * f + bytes(t)`.
fn zff_twelve(f: usize, t: usize) -> Vec<u8> {
    let mut out = vec![0; 12];
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

fn len_v(n: usize) -> Res<V> {
    let n = i64::try_from(n).map_err(|_| bad("count exceeds i64"))?;
    Ok(V::Int(n))
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
            r.expect_bytes(&[0, 0], "fromend context u16 0")?;
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
                        if &**c != "moNodeName_c" && super::rule(c).is_some() {
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
    r.expect_bytes(&[0xff; 16], "dim handle ff")?;
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
    r.expect_bytes(&[0x00, 0xff, 0xff, 0xff, 0xff], "dim handle -1")?;
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
fn favblock(reader: &Reader<'_>, pos: usize) -> Option<usize> {
    let second = reader.b;
    if pos + 122 > second.len()
        || !(reader.zeros(pos, 4) || reader.eq(pos, &[1, 0, 0, 0]))
        || !reader.zeros(pos + 76, 8)
        || second[pos + 84] > 1
    {
        return None;
    }
    let count = if second[pos + 84] != 0 { 12 } else { 3 };
    let at = pos + 85 + 8 * count;
    if at + 13 > second.len()
        || reader.pf64(at).ok()? != 1.0
        || !reader.eq(at + 8, &[0, 1, 0, 0, 0])
    {
        return None;
    }
    Some(at + 13 - pos)
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
    r.expect_bytes(&[0, 0], "fillet u16 0")?;
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
        r.expect_bytes(&[1, 0, 0, 0], "chamfer u32 1")?;
        if !r.eq(r.p, &[6, 0, 0, 0, 0, 0]) || !r.zeros(r.p + 26, 14) {
            return Err(bad("chamfer end block"));
        }
        r.skip(40);
        if r.zeros(r.p, 6) && objat(r, r.p + 6) {
            r.skip(6);
            return Ok(n);
        }
        r.expect_bytes(&[1, 0, 0, 0, 0, 1], "chamfer 1 0 1")?;
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
    r.expect_bytes(&zff_zero(4, 4), "saved body -1 0")?;
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
    r.expect_bytes(&zff_twelve(4, 4), "saved body -1")?;
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
    r.expect_bytes(&[0xff; 16], "context unit ff")?;
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
    r.expect_bytes(&[0, 0], "mbimport u16 0")?;
    let ft = r.u64()?;
    let fmt = r.cstring()?;
    let schema = r.cstring()?;
    r.expect_zeros(4, "mbimport u32 0")?;
    r.u32()?;
    r.expect_zeros(4, "mbimport u32 0 b")?;
    Ok(V::map([
        ("name", V::from(name)),
        ("path", V::from(path)),
        ("maps", len_v(maps)?),
        ("format", V::from(fmt)),
        ("schema", V::from(schema)),
        (
            "filetime",
            V::Int(i64::try_from(ft).map_err(|_| bad("FILETIME exceeds i64"))?),
        ),
    ]))
}

/// `mbBodyImportIDMaps_c`.
pub(crate) fn idmaps(reader: &mut Reader<'_>) -> Res<V> {
    let count = reader.u32()?;
    let mut out = Vec::new();
    for _ in 0..count {
        let span = reader.cstring()?;
        let first = reader.u32()?;
        let second = reader.u32()?;
        out.push(list(vec![V::from(span), V::from(first), V::from(second)]));
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
    r.st.sk.set_mirror(true);
    let rf = r.ptr(None);
    r.st.sk.set_mirror(false);
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
        r.expect_bytes(&m, "plane geom -12345")?;
        r.u32()?;
        r.expect_bytes(&zff_twelve(4, 0), "plane geom -1")?;
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
    r.expect_bytes(&[0, 0, 1], "face radius 0 1")?;
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
pub(crate) fn helix(reader: &mut Reader<'_>) -> Res<V> {
    let count = feature(reader, "moHelix_c")?;
    reader.expect_bytes(&[0, 0], "helix u16 0")?;
    let found = reader.u16()?;
    ptrs(reader, usize::from(found))?;
    let mut length = 142;
    for found in std::iter::once(142_usize).chain((136..160).step_by(2)) {
        if isblob(reader, reader.p + found)? && reader.eq(reader.p + found + 10, &BLOB_MARK) {
            length = found;
            break;
        }
    }
    reader.skip(length);
    sketch::blob(reader)?;
    reader.expect_bytes(&[6, 0, 0, 0], "helix u32 6")?;
    reader.skip(18);
    reader.expect_bytes(&[1, 0, 0, 0], "helix u32 1")?;
    let value = f64s(reader, 5)?;
    let mut zeros = 0;
    while reader.byte(reader.p + zeros)? == 0 && zeros < 200 {
        zeros += 1;
    }
    reader.skip(zeros);
    let mut dims = Vec::new();
    displays(reader, &mut dims)?;
    reader.expect_bytes(&[0], "helix u8 0")?;
    Ok(V::map([
        ("n", count),
        ("values", list(value)),
        ("dims", len_v(dims.len())?),
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
pub(crate) fn cutlistsort(reader: &mut Reader<'_>) -> Res<V> {
    let first = reader.u32()?;
    let second = reader.u16()?;
    let class = reader.u16()?;
    let distance = reader.u16()?;
    Ok(list(vec![
        V::from(first),
        V::from(second),
        V::from(class),
        V::from(distance),
    ]))
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
    r.expect_bytes(&[1, 0, 0, 0, 0, 0, 0, 0], "sheet metal 1 0")?;
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
        r.expect_bytes(&[0xff; 4], "mirror body -1")?;
        r.u32()?;
        r.u32()?;
    }
    r.expect_zeros(4, "mirror body u32 0")?;
    r.st.sk.set_mirror(true);
    let plane = r.ptr(None);
    r.st.sk.set_mirror(false);
    plane?;
    r.expect_zeros(4, "mirror body plane u32 0")?;
    r.ptr(Some(&["moMatrixIndexHelper_c"]))?;
    let c = r.u32()?;
    if c > 64 {
        return Err(bad("mirror body tail count"));
    }
    u32s(r, c)?;
    let mut tail = zff_zero(4, 8);
    tail.extend_from_slice(&[2, 0, 0, 0]);
    tail.extend_from_slice(&[0; 6]);
    tail.extend_from_slice(&[1, 1]);
    tail.extend_from_slice(&[0; 13]);
    r.expect_bytes(&tail, "mirror body tail")?;
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
pub(crate) fn holewzd(reader: &mut Reader<'_>) -> Res<V> {
    let count_ = feature(reader, "moHoleWzd_c")?;
    let hd = bodyhead(reader, "moHoleWzd_c")?;
    if hd_x_is_1(&hd) {
        units(reader)?;
    }
    let word = reader.pu16(reader.p)?;
    if 0 < word && word < 64 && isclass(reader, reader.p + 2, &["moCompSolidBody_c"])? {
        let k = reader.u16()?;
        ptrs(reader, usize::from(k))?;
        let k = reader.u32()?;
        if k > 4096 {
            return Err(bad("hole body ids"));
        }
        u32s(reader, k)?;
        u32s(reader, 4)?;
    }
    reader.ptr(Some(PMARK))?;
    reader.expect_zeros(6, "hole 6 zero")?;
    let d1 = reader.f64()?;
    reader.expect_bytes(&[1, 0, 0, 0], "hole u32 1")?;
    reader.cstring()?;
    reader.cstring()?;
    reader.u32()?;
    reader.u32()?;
    reader.u32()?;
    let std = reader.cstring()?;
    let ftype = reader.cstring()?;
    let size = reader.cstring()?;
    f64s(reader, 5)?;
    reader.cstring()?;
    reader.u32()?;
    reader.u32()?;
    let desc = reader.cstring()?;
    reader.cstring()?;
    reader.u32()?;
    reader.u32()?;
    reader.cstring()?;
    reader.expect_zeros(4, "hole list u32 0")?;
    let n2 = reader.u32()?;
    if n2 > 4096 {
        return Err(bad("hole instance count"));
    }
    let mut inst = Vec::new();
    for _ in 0..n2 {
        let cursor = reader.u32()?;
        let x_value = reader.f64()?;
        inst.push(list(vec![V::from(cursor), V::from(x_value)]));
    }
    reader.expect_zeros(52, "hole 52 zero")?;
    reader.cstring()?;
    reader.expect_zeros(12, "hole 12 zero")?;
    let mut one_one = vec![1, 0, 0, 0, 1, 0, 0, 0];
    one_one.extend_from_slice(&zff_zero(4, 8));
    reader.expect_bytes(&one_one, "hole 1 1 -1")?;
    reader.cstring()?;
    reader.cstring()?;
    reader.expect_zeros(4, "hole u32 0")?;
    reader.f64()?;
    if (reader.eq(reader.p, &[1, 0, 0, 0]) || reader.zeros(reader.p, 4))
        && reader.zeros(reader.p + 4, 12)
    {
        // 14000: the u32 before the zeros.
        reader.skip(4);
        reader.expect_zeros(12, "hole end 12 zero 14000")?;
    } else {
        reader.expect_zeros(12, "hole end 12 zero")?;
        reader.u32()?;
    }
    let mut z = 0;
    while reader.p + z < reader.len() && reader.byte(reader.p + z)? == 0 && z < 16 {
        z += 1;
    }
    if !(reader.p + z >= reader.len() || objat(reader, reader.p + z)) {
        return Err(bad(format!("hole end zeros {z}")));
    }
    reader.skip(z);
    Ok(V::map([
        ("n", count_),
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
pub(crate) fn onebend(reader: &mut Reader<'_>) -> Res<V> {
    let count = feature(reader, "moOneBend_c")?;
    reader.skip(122);
    let mut dims = Vec::new();
    displays(reader, &mut dims)?;
    let cf = reader.ptr(Some(&["moCompFeature_c"]))?;
    reader.skip(33);
    reader.ptr(Some(&["moCompEdge_c"]))?;
    displays(reader, &mut dims)?;
    if reader.p + 8 <= reader.len()
        && reader.eq(reader.p, &[0x00, 0x00, 0x80, 0x00, 0x08, 0x00, 0x00, 0x00])
    {
        reader.skip(608);
        return Ok(V::map([("n", count), ("cf", V::from(cf))]));
    }
    let mut cursor = reader.p;
    let end = loop {
        if cursor >= reader.p + 400 || cursor + 8 >= reader.len() {
            return Err(bad("bend tail: no next object"));
        }
        if reader.eq(cursor, &[0xff, 0xfe, 0xff]) {
            let first = reader.pu16(cursor - 4)?;
            let class = reader.pu16(cursor - 2)?;
            if first & 0x8000 != 0
                && class & 0x8000 != 0
                && reader
                    .known
                    .get(&u32::from(class & 0x7fff))
                    .is_some_and(|s| &**s == "moNodeName_c")
            {
                break cursor - 4;
            }
        }
        cursor += 1;
    };
    reader.p = end;
    Ok(V::map([("n", count), ("cf", V::from(cf))]))
}

/// `moTemplateFlatPattern_c`, `moFlatPattern_c`.
pub(crate) fn flatpattern(r: &mut Reader<'_>) -> Res<V> {
    let n = feature(r, "moTemplateFlatPattern_c")?;
    r.skip(122);
    r.expect_bytes(&[0, 0], "flat pattern null")?;
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
        r.expect_bytes(&[0, 0], "flat pattern u16 0")?;
        r.ptr(Some(&["moCompFace_c"]))?;
        r.skip(66);
        r.ptr(Some(&["moCompFeature_c"]))?;
        let rest = r
            .len()
            .checked_sub(r.p)
            .ok_or_else(|| bad("flat pattern position past end"))?;
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
    Ok(V::map([("gap", len_v(k - q)?)]))
}

/// `moNativeConfigurationTableFeature_c`.
pub(crate) fn cfgtable(r: &mut Reader<'_>) -> Res<V> {
    let n = feature(r, "moNativeConfigurationTableFeature_c")?;
    r.cstring()?;
    r.expect_bytes(&zff_twelve(4, 0), "configuration table -1")?;
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
    r.expect_bytes(&zff_twelve(4, 8), "combine -1")?;
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
    r.expect_bytes(&[1, 0, 0, 0], "bitmap u32 1")?;
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
pub(crate) fn moveface(reader: &mut Reader<'_>) -> Res<V> {
    let count = feature(reader, "moMoveFace_c")?;
    let hd = bodyhead(reader, "moMoveFace_c")?;
    if hd_x_is_1(&hd) {
        units(reader)?;
    }
    reader.ptr(Some(PMARK))?;
    let first = reader.f64()?;
    let second = reader.f64()?;
    reader.skip(14);
    let found = reader.u16()?;
    for _ in 0..found {
        reader.ptr(Some(&["moCompFace_c"]))?;
        if reader.eq(reader.p, &[0, 0]) && isclass(reader, reader.p + 2, &["moCompFace_c"])? {
            reader.skip(2);
        }
    }
    reader.expect_zeros(4, "move face u32 0")?;
    reader.ptr(Some(SUOBLIST))?;
    let mut dims = Vec::new();
    displays(reader, &mut dims)?;
    let mut found = (reader.p..reader.p + 600).find(|&q| objat(reader, q));
    if found.is_none()
        && reader
            .len()
            .checked_sub(reader.p)
            .is_none_or(|rest| rest <= 600)
    {
        found = Some(reader.len());
    }
    let Some(found) = found else {
        return Err(bad("move face end"));
    };
    reader.p = found;
    Ok(V::map([
        ("n", count),
        ("v", list(vec![V::from(first), V::from(second)])),
    ]))
}

/// `moLineAxisData_c`.
pub(crate) fn lineaxis(r: &mut Reader<'_>) -> Res<V> {
    let v = f64s(r, 14)?;
    r.ptr(None)?;
    axistail(r)?;
    Ok(list(v))
}
