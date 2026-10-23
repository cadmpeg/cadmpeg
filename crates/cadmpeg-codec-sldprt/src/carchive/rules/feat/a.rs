//! `g_feat.py` rules defined before line 730 (part a): folders, reference
//! planes and their data, components, profiles, parameters, the scale,
//! sweep-thread, pattern and extrusion features, unit choosers and mark
//! records. The functions both parts call are in `core`.

use super::super::super::reader::{bad, ParameterRecord, Ptr, Reader, Res, Stop, V};
use super::super::sketch;
use super::{
    axistail, bodyhead, endtag_next, feature, ftrlink, geo_frame, is76, isclass, isver, named,
    objat, oldgeo, refblock, refgeo, units, ver, ver_or, CONSTRAINTPLN, M12345, REFPLN, UCS,
};

/// Part a's rule state. Its module globals are logs only.
#[derive(Debug, Clone, Default)]
pub(crate) struct AState {}

/// `DISPLAY`: the display-dimension classes.
pub(crate) const DISPLAY: &[&str] = &[
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

const FFFEFF: [u8; 3] = [0xff, 0xfe, 0xff];

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
            if !super::core::new(r) {
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
        r.st.sk.byref = true;
        let refs = byref_refs(r);
        r.st.sk.byref = false;
        let refs = refs?;
        let done = V::map([
            ("pt", V::List(pt)),
            ("M", m),
            ("box", V::List(bx)),
            ("refs", V::from(refs as u32)),
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
    r.expect(&[0xff; 16], "unit component ff")?;
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
    r.expect(&[0, 0], "")?;
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
    r.expect(&[1, 0, 0, 0], "")?;
    Ok(n)
}

/// `moSelectionSetFolder_c`.
pub(crate) fn selfolder(r: &mut Reader<'_>) -> Res<V> {
    let n = feature(r, "moSelectionSetFolder_c")?;
    r.expect(&[0, 0], "")?;
    r.expect(&[1, 0, 0, 0], "")?;
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
            let guid = super::super::super::reader::hex(r.slice(r.p, 16));
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
    r.expect(&[0, 0], "")?;
    let value_at = r.p;
    let val = if integer {
        V::from(r.u32()?)
    } else {
        V::from(r.f64()?)
    };
    let name = n.get("name").clone();
    let featdim = r.st.core.featdim.last().copied().unwrap_or(false);
    if featdim {
        let oid;
        if r.byte(r.p)? == 0 && isclass(r, r.p + 1, &["moDimTolerance_c"])? {
            r.skip(1);
            r.ptr(None)?;
            oid = V::None;
        } else {
            let q = r.p;
            r.skip(23);
            if r.slice(q, 23).len() < 7 {
                return Err(Stop::End(r.p));
            }
            oid = V::from(r.pu32(q + 3)?);
        }
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
    r.expect(&[0], "parameter u8 0")?;
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
    entry: Option<&super::super::super::reader::Trace>,
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
    r.expect(&[1, 0], "scale u16 1")?;
    r.ptr(None)?;
    let count = r.u32()?;
    for _ in 0..count {
        r.u32()?;
    }
    r.expect(&[0xff; 4], "scale -1")?;
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
    r.expect(&[1, 0, 0, 0, 0, 0, 0, 0], "thread 1 0")?;
    r.u32()?;
    r.expect_zeros(12, "thread 12 zero")?;
    let mut m = M12345.to_vec();
    m.extend_from_slice(&[0; 4]);
    r.expect(&m, "thread -12345")?;
    r.u32()?;
    let mut z12ff = vec![0; 12];
    z12ff.extend_from_slice(&[0xff; 4]);
    r.expect(&z12ff, "thread 12 zero -1")?;
    r.f64()?;
    r.f64()?;
    let mut ffb = vec![0];
    ffb.extend_from_slice(&[0xff; 8]);
    ffb.extend_from_slice(&[0; 6]);
    r.expect(&ffb, "thread ff block")?;
    for _ in 0..7 {
        r.f64()?;
    }
    r.expect(&[1, 0, 0, 0, 0, 0, 0, 0], "thread 1 0 b")?;
    let face = r.ptr(None)?;
    if face.obj_class() == Some("moSingleFaceRef_w") {
        r.expect_zeros(10, "thread face 10 zero")?;
        r.u32()?;
        r.expect(
            &[0, 0, 0, 0, 0xff, 0xff, 0xff, 0xff, 0, 0, 0, 0],
            "thread face 0 -1 0",
        )?;
        r.expect(&m, "thread face -12345")?;
        r.u32()?;
        r.expect(&z12ff, "thread face -1")?;
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
        r.expect(&one_one, "thread 1 1 26 zero")?;
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
    r.expect(&[0, 0], "block instance u16 0")?;
    r.skip(16);
    r.expect(&[0, 0, 1, 0], "block instance 0 1")?;
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
            r.expect(&[1, 0], "")?;
            r.skip(18);
            r.expect(&[1, 0, 0, 0], "")?;
        }
    }
    if hx == Some(1) {
        units(r)?;
    }
    Ok(())
}

/// `moLPattern_c`, `moCirPattern_c`, `moMirrorPattern_c`.
pub(crate) fn featpattern(r: &mut Reader<'_>) -> Res<V> {
    let c = r.last_class().ok_or(Stop::End(r.p))?;
    let c = &*c;
    let n = feature(r, c)?;
    let hd = bodyhead(r, c)?;
    units_after_head(r, &hd)?;
    r.ptr(Some(&["moPMarkRecord_c"]))?;
    r.ptr(Some(&["suObList"]))?;
    r.ptr(Some(&["suObList"]))?;
    if !r.eq(r.p, &[1, 0, 0, 0, 0, 0, 1, 0, 0, 0, 0])
        && !r.eq(r.p, &[1, 0, 0, 0, 0, 0, 2, 0, 0, 0, 0])
    {
        return Err(bad("pattern head"));
    }
    r.skip(11);
    r.ptr(Some(&["suObList"]))?;
    r.skip(if c == "moCirPattern_c" { 228 } else { 192 });
    r.ptr(Some(&["suObList"]))?;
    r.expect_zeros(
        if c == "moMirrorPattern_c" { 13 } else { 11 },
        "pattern zero run",
    )?;
    display_objects(r)?;
    if c == "moLPattern_c" {
        r.u32()?;
        r.expect(&[0, 0], "lpattern u16 0")?;
        let d1 = r.ptr(None)?;
        item1(r, &d1)?;
        axistail(r)?;
        let d2 = r.ptr(None)?;
        if d2.is_obj() {
            axistail(r)?;
        }
        let mut x = vec![0; 13];
        x.extend_from_slice(&[1, 0, 0, 0, 1, 0, 0, 0]);
        x.extend_from_slice(&[0; 12]);
        r.expect(&x, "lpattern 1 1")?;
        display_objects(r)?;
        let mut z = 0;
        while r.p + z < r.len() && r.byte(r.p + z)? == 0 {
            z += 1;
        }
        if r.p + z == r.len() || z == 9 || (z == 13 && objat(r, r.p + 13)) {
            r.skip(z);
            return Ok(n);
        }
        return Err(bad(format!("lpattern end {z}")));
    }
    if c == "moCirPattern_c" {
        r.expect(&[0, 0], "cirpattern u16 0")?;
        let ax = r.ptr(None)?;
        item1(r, &ax)?;
        axistail(r)?;
        if r.u8()? > 1 {
            return Err(bad("cirpattern flag byte"));
        }
        display_objects(r)?;
        r.expect(
            &[1, 0, 0, 0, 0, 1, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0],
            "cirpattern end",
        )?;
        if !objat(r, r.p) && r.p < r.len() && r.zeros(r.p, 8) {
            r.skip(8);
            for _ in 0..5 {
                r.f64()?;
            }
        } else if r.p + 52 == r.len() {
            r.skip(52);
        }
        return Ok(n);
    }
    if c == "moMirrorPattern_c" {
        r.st.sk.mirror = true;
        let pl = r.ptr(Some(&["moCompRefPlane_c"]));
        r.st.sk.mirror = false;
        pl?;
        return Ok(n);
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
    r.expect(&[0, 0, 1, 0], "body chooser 0 1")?;
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
pub(crate) fn extrusion(r: &mut Reader<'_>, c: &str) -> Res<V> {
    let cls = r
        .last_class()
        .map_or_else(|| c.to_owned(), |c| c.to_string());
    let cls = cls.as_str();
    let n = feature(r, "moExtrusion_c")?;
    let hd = bodyhead(r, "moExtrusion_c")?;
    let mut hx = hd.get("x").as_int();
    if hd.get("g").as_int() == Some(1) {
        hx = Some(i64::from(body_ids(r)?));
    }
    if hx == Some(1) {
        units(r)?;
    }
    if REVOLVES.contains(&cls) && zrun_to(r, r.p, "suObList")? >= 40 {
        let z = zrun_to(r, r.p, "suObList")?;
        r.skip(z - 9);
    } else if !REVOLVES.contains(&cls) && zrun_to(r, r.p, "moEndSpec_c")? >= 40 {
        // Zero bytes in place of the mark record (64; extruded cuts).
        let z = zrun_to(r, r.p, "moEndSpec_c")?;
        r.skip(z);
    } else {
        r.ptr(Some(&["moPMarkRecord_c"]))?;
    }
    if cls == "moSMBaseFlange_c" {
        let mut dims = 0_u32;
        while isclass(r, r.p, DISPLAY)? {
            r.ptr(None)?;
            dims += 1;
        }
        let mut k = None;
        for q in r.p..r.p + 40 {
            if isclass(r, q, &["moCompSheetmetalFeat_c"])? {
                k = Some(q);
                break;
            }
        }
        let Some(k) = k else {
            return Err(bad("base flange: no sheet metal component"));
        };
        r.p = k;
        r.ptr(None)?;
        let e = (r.p..r.p + 200).find(|&q| q >= r.len() || objat(r, q));
        let Some(e) = e else {
            return Err(bad("base flange tail None"));
        };
        let tail = if e == 0 {
            0
        } else {
            r.slice(r.p, e - r.p).len()
        };
        if !matches!(tail, 71 | 74 | 75) {
            return Err(bad(format!("base flange tail {tail}")));
        }
        r.p = e;
        return Ok(V::map([("n", n), ("dims", V::from(dims))]));
    }
    if (cls == "moSweepCut_c" || cls == "moSweep_c")
        && r.eq(r.p, &[1, 0])
        && isclass(r, r.p + 2, &["moGeneralCurveRef_w"])?
    {
        r.skip(2);
        let path = r.ptr(None)?;
        r.expect(&[1, 0], "sweep u16 1")?;
        r.ptr(Some(&["moWireContact_c"]))?;
        r.ptr(Some(&["moGeneralCurveRef_w"]))?;
        r.skip(105);
        let guide = r.ptr(None)?;
        r.skip(17);
        return Ok(V::map([
            ("n", n),
            ("path", V::from(path)),
            ("guide", V::from(guide)),
        ]));
    }
    if REVOLVES.contains(&cls) {
        if r.zeros(r.p, 5) && r.eq(r.p + 5, &[1, 0, 0, 0]) && isclass(r, r.p + 9, &["suObList"])? {
            r.skip(9);
        } else {
            r.expect_zeros(9, "revolve 9 zero")?;
        }
        r.ptr(Some(&["suObList"]))?;
        r.u32()?;
        r.u32()?;
        r.ptr(None)?;
        if ver_or(r, 99999) >= 16000 {
            r.expect_zeros(4, "revolve u32 0")?;
        }
        for _ in 0..7 {
            r.f64()?;
        }
        r.expect(&[0], "revolve u8 0")?;
        r.ptr(Some(&["moRevEndSpec_c"]))?;
    } else {
        r.ptr(Some(&["moEndSpec_c"]))?;
    }
    let mut dims = 0_u32;
    loop {
        let w = r.pu16(r.p)?;
        let nm: String = if w == 0xffff {
            let k = usize::from(r.pu16(r.p + 4)?);
            r.slice(r.p + 6, k).iter().map(|&b| char::from(b)).collect()
        } else if w & 0x8000 != 0 {
            r.known
                .get(&u32::from(w & 0x7fff))
                .map_or_else(String::new, ToString::to_string)
        } else {
            String::new()
        };
        if !nm.starts_with("moDisplay") {
            break;
        }
        r.ptr(None)?;
        dims += 1;
    }
    if REVOLVES.contains(&cls) {
        let mut z = if ver_or(r, 99999) >= 18000 { 7 } else { 6 };
        if z == 6 && r.zeros(r.p, 7) && objat(r, r.p + 7) && !objat(r, r.p + 6) {
            z = 7;
        }
        r.expect_zeros(z, "revolve zero bytes")?;
        return Ok(V::map([("n", n), ("dims", V::from(dims))]));
    }
    if r.eq(r.p, &[0, 0, 1, 1, 0]) && objat(r, r.p + 5) {
        r.skip(5);
        r.ptr(None)?;
        refblock(r)?;
        if r.zeros(r.p, 2) && r.pf64(r.p + 2)?.abs() == 1.0 {
            r.skip(2); // u16 0 before the four gaps (1 sample)
        }
        for _ in 0..4 {
            r.f64()?;
        }
        r.expect_zeros(4, "up-to-face u32 0")?;
        r.u32()?;
        r.u32()?;
        let t = r.slice(r.p, 71);
        let sub = |a: usize, b: usize| t.get(a.min(t.len())..b.min(t.len())).unwrap_or(&[]);
        if sub(0, 3) != [1, 0, 0]
            || !(sub(59, 63) == [1, 0, 0, 0] || sub(59, 63) == [0, 0, 0, 0])
            || sub(63, 71) != [0; 8]
        {
            return Err(bad("up-to-face block tail"));
        }
        r.skip(71);
    } else if r.eq(r.p, &[0]) && r.eq(r.p + 4, &[0, 1, 0, 0]) && objat(r, r.p + 40) {
        // A drafted extrusion: the block's pointer slot at +40 holds the draft
        // angle's display dimension, then the block's last 34 bytes.
        r.skip(40);
        r.ptr(None)?;
        let t = r.slice(r.p, 34);
        let sub = |a: usize, b: usize| t.get(a.min(t.len())..b.min(t.len())).unwrap_or(&[]);
        if sub(0, 22) != [0; 22]
            || !(sub(22, 26) == [1, 0, 0, 0] || sub(22, 26) == [0, 0, 0, 0])
            || sub(26, 34) != [0; 8]
        {
            return Err(bad("drafted extrusion block tail"));
        }
        r.skip(34);
    } else if !is76(r, r.p) {
        let mut joined = r.slice(r.p, 4).to_vec();
        joined.extend_from_slice(r.slice(r.p + 6, 74));
        if r.eq(r.p + 4, &[0, 0]) && is76_bytes(&joined) && objat(r, r.p + 78) {
            r.skip(78);
        } else {
            return Err(bad("extrusion 76-byte block"));
        }
    } else {
        r.skip(76);
    }
    r.ptr(Some(&["moFromEndSpec_c"]))?;
    if r.p >= r.len() {
        return Ok(V::map([("n", n), ("dims", V::from(dims))]));
    }
    r.ptr(None)?;
    if r.p < r.len() {
        r.ptr(None)?;
    }
    Ok(V::map([("n", n), ("dims", V::from(dims))]))
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
        r.skip(6 * n as usize);
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
