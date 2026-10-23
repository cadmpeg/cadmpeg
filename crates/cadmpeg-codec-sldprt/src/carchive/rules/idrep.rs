//! The persistent-identity family (`g_idrep.py`): `moFR_c`, the `*IdRep_c`
//! classes, `moExtObject_c`, `moCStringHandle_c`.

use super::super::reader::{bad, Reader, Res, V};

/// `moCStringHandle_c`.
pub(crate) fn cstringhandle(r: &mut Reader<'_>) -> Res<V> {
    Ok(V::Str(r.cstring()?))
}

/// `moExtObject_c`.
pub(crate) fn extobject(r: &mut Reader<'_>) -> Res<V> {
    let path = r.ptr(None)?;
    let name = r.ptr(None)?;
    if r.u16()? != 2 {
        return Err(bad("ExtObject version"));
    }
    r.u8()?;
    r.u32()?;
    r.cstring()?;
    r.cstring()?;
    r.cstring()?;
    let q = r.p;
    let Some(j) = r.find(&[0xff, 0xfe, 0xff], q, q + 64) else {
        return Err(bad("ExtObject: no configuration string"));
    };
    let x = j - q;
    r.p = j;
    let v = super::feature::ver_or(r, 0);
    let cfg = r.cstring()?;
    r.skip(if v >= 17000 { 16 } else { 12 });
    Ok(V::map([
        ("path", V::from(path)),
        ("name", V::from(name)),
        ("X", V::Int(i64::try_from(x).unwrap_or(i64::MAX))),
        ("cfg", V::from(cfg)),
    ]))
}

/// `moFR_c`.
pub(crate) fn fr(r: &mut Reader<'_>) -> Res<V> {
    let prod = r.ptr(None)?;
    let fid = r.u32()?;
    let ts = r.u32()?;
    Ok(V::map([
        ("prod", V::from(prod)),
        ("fid", V::from(fid)),
        ("ts", V::from(ts)),
    ]))
}

/// `OWN`: the id-rep classes and the own bytes after their `moFR_c`.
pub(crate) fn own(c: &str) -> Option<usize> {
    Some(match c {
        "moSimpleSurfIdRep_c"
        | "moFromSktEntSurfIdRep_c"
        | "moFilletSurfIdRep_c"
        | "moThinOffsetSurfIdRep_c"
        | "moWireEdgeIdRep_c"
        | "moSimpleEdgeIdRep_c" => 4,
        "moFromSktEnt3IntSurfIdRep_c" | "moEndFace3IntSurfIdRep_c" => 12,
        "moEndFaceSurfIdRep_c"
        | "moThinClosingSurfIdRep_c"
        | "moUnBendSurfIdRep_c"
        | "moBendEndSurfIdRep_c"
        | "moSegmLeftTrimEdgeIdRep_c"
        | "moSegmRightTrimEdgeIdRep_c"
        | "moSweepSideSurfIdRep_c"
        | "moWzdHoleSurfIdRep_c" => 8,
        "moLPatternSurfIdRep_c"
        | "moIndirectSurfIdRep_c"
        | "moSurfaceIdRep_c"
        | "moDerivedSurfIdRep_c"
        | "moCirPatternSurfIdRep_c"
        | "moCirPatternSurfIdRep2_c"
        | "moMirPatternSurfIdRep_c" => 0,
        _ => return None,
    })
}

/// `WRAP`: id-rep classes that wrap a child, and the u32 count after it.
fn wrap(c: &str) -> Option<usize> {
    Some(match c {
        "moDerivedSurfIdRep_c" => 1,
        "moLPatternSurfIdRep_c" | "moCirPatternSurfIdRep_c" | "moCirPatternSurfIdRep2_c" => 2,
        "moMirPatternSurfIdRep_c" | "moIndirectSurfIdRep_c" => 0,
        _ => return None,
    })
}

/// The rule `make(c)` builds for each id-rep class.
pub(crate) fn rep(r: &mut Reader<'_>) -> Res<V> {
    let c = r
        .last_class()
        .ok_or_else(|| bad("id rep without a class"))?;
    r.u16()?;
    let f = r.obj(Some(&["moFR_c"]))?;
    if let Some(k) = wrap(&c) {
        let kid = r.ptr(None)?;
        let mut tail = Vec::new();
        for _ in 0..k {
            tail.push(V::from(r.u32()?));
        }
        return Ok(V::map([
            ("fr", V::from(f)),
            ("kid", V::from(kid)),
            ("tail", V::List(tail)),
        ]));
    }
    let n = own(&c).unwrap_or(0);
    let own_bytes = r.slice(r.p, n).to_vec();
    r.skip(n);
    Ok(V::map([("fr", V::from(f)), ("own", V::Bytes(own_bytes))]))
}
