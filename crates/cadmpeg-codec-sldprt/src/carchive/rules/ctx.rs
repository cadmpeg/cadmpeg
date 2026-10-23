//! In-context (assembly) references: `moContextChainComponent_c` and the
//! chain it names, and the backed-up sketch geometry records.

use super::super::reader::{bad, Reader, Res, V};
use super::feature::isver;

/// `xform(r)`: u8 1, u8 0, a flag, 12 or 3 f64, f64 1, u8 0.
pub(crate) fn xform(r: &mut Reader<'_>) -> Res<V> {
    let a = r.u8()?;
    let b = r.u8()?;
    let fl = r.u8()?;
    if (a, b) != (1, 0) || fl > 1 {
        return Err(bad("context transform head"));
    }
    let mut m = Vec::new();
    for _ in 0..if fl != 0 { 12 } else { 3 } {
        m.push(V::from(r.f64()?));
    }
    if r.f64()? != 1.0 {
        return Err(bad("context transform scale"));
    }
    r.expect_bytes(&[0], "context transform u8 0")?;
    Ok(V::List(m))
}

/// `moContextChainComponent_c`.
pub(crate) fn contextcomp(r: &mut Reader<'_>) -> Res<V> {
    if r.u16()? != 2 {
        return Err(bad("context u16 2"));
    }
    let chain = r.ptr(None)?;
    let comp = r.ptr(None)?;
    if !r.eq(r.p, &[0, 0, 4, 0, 1, 0, 0, 0]) && !r.eq(r.p, &[0, 0, 5, 0, 1, 0, 0, 0]) {
        return Err(bad("context 0 4 1 0"));
    }
    r.skip(8);
    let _fl = r.u32()?;
    let cid = r.u32()?;
    let form = if r.pu32(r.p)? == cid {
        r.u32()?;
        r.u32()?;
        r.skip(24);
        r.expect_bytes(&[0xff; 16], "context ff")?;
        r.skip(20);
        "A"
    } else {
        xform(r)?;
        let mut c = vec![0; 12];
        c.extend_from_slice(&[0xff; 4]);
        if r.zeros(r.p, 10) && isver(r, r.p + 26) && r.eq(r.p + 30, &c) {
            r.skip(10);
            let mut v = Vec::new();
            for _ in 0..4 {
                v.push(V::from(r.u32()?));
            }
            r.skip(20);
            return Ok(V::map([
                ("chain", V::from(chain)),
                ("comp", V::from(comp)),
                ("id", V::from(cid)),
                ("form", V::from("C")),
                ("v", V::List(v)),
            ]));
        }
        xform(r)?;
        for _ in 0..6 {
            r.u32()?;
        }
        "B"
    };
    if isver(r, r.p) {
        r.skip(4);
    }
    Ok(V::map([
        ("chain", V::from(chain)),
        ("comp", V::from(comp)),
        ("id", V::from(cid)),
        ("form", V::from(form)),
    ]))
}

/// `moReferenceChain_c`.
pub(crate) fn refchain(r: &mut Reader<'_>) -> Res<V> {
    r.u16()?;
    Ok(V::from(r.ptr(Some(&["moExtFeatureList_c"]))?))
}

/// `moExtFeatureList_c`.
pub(crate) fn extfeatlist(r: &mut Reader<'_>) -> Res<V> {
    let n = r.u32()?;
    let mut fs = Vec::new();
    for _ in 0..n {
        fs.push(r.ptr(Some(&["moExtFeature_c"]))?);
    }
    r.u32()?;
    let cfg = r.cstring()?;
    r.u32()?;
    if r.zeros(r.p, 4) && r.pu16(r.p + 4)? != 0 {
        r.skip(4);
    }
    Ok(V::map([("features", V::from(fs)), ("cfg", V::from(cfg))]))
}

/// `moExtFeature_c`.
pub(crate) fn extfeature(r: &mut Reader<'_>) -> Res<V> {
    let path = r.ptr(None)?;
    let title = r.ptr(None)?;
    r.skip(3);
    let ts = r.u32()?;
    for _ in 0..3 {
        r.cstring()?;
    }
    r.u8()?;
    r.cstring()?;
    if r.u32()? != 0xffff_ffff {
        return Err(bad("ext feature -1"));
    }
    r.u8()?;
    r.u32()?;
    let inst = r.ptr(None)?;
    r.expect_bytes(&[0, 0], "ext feature u16 0")?;
    let b0 = r.u32()?;
    Ok(V::map([
        ("path", V::from(path)),
        ("title", V::from(title)),
        ("ts", V::from(ts)),
        ("inst", V::from(inst)),
        ("b0", V::from(b0)),
    ]))
}

/// `moArcBackedUpData_c`.
pub(crate) fn arcbackup(reader: &mut Reader<'_>) -> Res<V> {
    let first = reader.ptr(None)?;
    let second = reader.ptr(None)?;
    let x_value = reader.f64()?;
    let found = reader.u32()?;
    let class = reader.ptr(None)?;
    let y_value = reader.f64()?;
    Ok(V::List(vec![
        V::from(first),
        V::from(second),
        V::from(x_value),
        V::from(found),
        V::from(class),
        V::from(y_value),
    ]))
}

/// `moEllipseBackedUpData_c`.
pub(crate) fn ellipsebackup(r: &mut Reader<'_>) -> Res<V> {
    let a = r.ptr(None)?;
    let b = r.ptr(None)?;
    let x = r.f64()?;
    if r.u32()? != 1 {
        return Err(bad("ellipse backup u32 1"));
    }
    let mut pts = Vec::new();
    for _ in 0..5 {
        pts.push(r.ptr(None)?);
    }
    Ok(V::List(vec![
        V::from(a),
        V::from(b),
        V::from(x),
        V::from(pts),
    ]))
}

/// `moLineBackedUpData_c`.
pub(crate) fn linebackup(r: &mut Reader<'_>) -> Res<V> {
    let a = r.ptr(None)?;
    let b = r.ptr(None)?;
    let x = r.f64()?;
    Ok(V::List(vec![V::from(a), V::from(b), V::from(x)]))
}
