// SPDX-License-Identifier: Apache-2.0
//! Type 144 boundary byte fixtures with stated traversal directions.

use crate::test_support::test_owned::{owned_test_file, OwnedTestEntity};

/// A Type 144 face on the plane `z = 0` with the outer boundary `outer`
/// (model curve) and `outer_pcurve` and the inner boundary `inner`, which
/// serves as both its model and its parameter curve.
pub(crate) fn trimmed_plane_with_outer_and_inner(
    outer: &str,
    outer_pcurve: &str,
    inner: &str,
    trimmed_parameters: &str,
) -> Vec<u8> {
    owned_test_file(&[
        OwnedTestEntity {
            entity_type: 108,
            form: 0,
            label: "PLANE".into(),
            status: "00010000",
            parameters: "108,0,0,1,0,0,0,0,0,0;".into(),
        },
        OwnedTestEntity {
            entity_type: 106,
            form: 63,
            label: "OUTMODEL".into(),
            status: "00010000",
            parameters: outer.into(),
        },
        OwnedTestEntity {
            entity_type: 106,
            form: 63,
            label: "OUTPCURV".into(),
            status: "00010500",
            parameters: outer_pcurve.into(),
        },
        OwnedTestEntity {
            entity_type: 142,
            form: 0,
            label: "OUTBOUND".into(),
            status: "00010000",
            parameters: "142,0,1,5,3,3;".into(),
        },
        OwnedTestEntity {
            entity_type: 106,
            form: 63,
            label: "INMODEL".into(),
            status: "00010000",
            parameters: inner.into(),
        },
        OwnedTestEntity {
            entity_type: 106,
            form: 63,
            label: "INPCURVE".into(),
            status: "00010500",
            parameters: inner.into(),
        },
        OwnedTestEntity {
            entity_type: 142,
            form: 0,
            label: "INBOUND".into(),
            status: "00010000",
            parameters: "142,0,1,11,9,3;".into(),
        },
        OwnedTestEntity {
            entity_type: 144,
            form: 0,
            label: "ANNULUS".into(),
            status: "00000000",
            parameters: trimmed_parameters.into(),
        },
    ])
}

/// A Type 144 face with an explicit outer boundary on the bilinear NURBS
/// patch `S(u, v) = (u, v, 0)` over `[0, 1]²`. `boundary` is the Type 106
/// model curve and also the parameter curve, which the Type 142 entity uses
/// when `with_pcurve` holds.
pub(crate) fn trimmed_nurbs_patch_file(boundary: &str, with_pcurve: bool) -> Vec<u8> {
    let (pcurve, preference) = if with_pcurve { (5, 3) } else { (0, 1) };
    owned_test_file(&[
        OwnedTestEntity {
            entity_type: 128,
            form: 0,
            label: "SURFACE".into(),
            status: "00010000",
            parameters:
                "128,1,1,1,1,0,0,1,0,0,0,0,1,1,0,0,1,1,1,1,1,1,0,0,0,1,0,0,0,1,0,1,1,0,0,1,0,1;"
                    .into(),
        },
        OwnedTestEntity {
            entity_type: 106,
            form: 63,
            label: "MODEL".into(),
            status: "00010000",
            parameters: boundary.into(),
        },
        OwnedTestEntity {
            entity_type: 106,
            form: 63,
            label: "PCURVE".into(),
            status: "00010500",
            parameters: boundary.into(),
        },
        OwnedTestEntity {
            entity_type: 142,
            form: 0,
            label: "ONSURF".into(),
            status: "00010000",
            parameters: format!("142,0,1,{pcurve},3,{preference};"),
        },
        OwnedTestEntity {
            entity_type: 144,
            form: 0,
            label: "TRIMMED".into(),
            status: "00000000",
            parameters: "144,1,1,0,7;".into(),
        },
    ])
}

/// The boundaries of a band on the ring patch `S(u, v) = (P(u), v)` over
/// `[0, 3] × [0, 1]`, where `P` runs the triangle `(1, 0)`, `(−1/2, 1)`,
/// `(−1/2, −1)` at `u = 0, 1, 2` and closes at `u = 3`. The surface is
/// directory entry 1. The ring `v = 0` toward greater `u` is entry 7, and
/// the ring `v = 1` toward smaller `u` is entry 13. Where `bounded`, the
/// rings are Type 141 records, and otherwise Type 142 records. The upper
/// Type 142 states no parameter curve where `upper_pcurve` does not hold.
fn ring_band_entities(bounded: bool, upper_pcurve: bool) -> Vec<OwnedTestEntity> {
    let boundary = |model: u32, pcurve: u32| {
        if bounded {
            (141, format!("141,1,2,1,1,{model},1,1,{pcurve};"))
        } else if pcurve == 0 {
            (142, format!("142,0,1,0,{model},1;"))
        } else {
            (142, format!("142,0,1,{pcurve},{model},2;"))
        }
    };
    let (lower_type, lower) = boundary(3, 5);
    let (upper_type, upper) = boundary(9, if upper_pcurve { 11 } else { 0 });
    vec![
        OwnedTestEntity {
            entity_type: 128,
            form: 0,
            label: "RING".into(),
            status: "00010000",
            parameters: concat!(
                "128,3,1,1,1,1,0,1,0,0,0,0,1,2,3,3,0,0,1,1,1,1,1,1,1,1,1,1,",
                "1,0,0,-0.5,1,0,-0.5,-1,0,1,0,0,1,0,1,-0.5,1,1,-0.5,-1,1,1,0,1,",
                "0,3,0,1;"
            )
            .into(),
        },
        OwnedTestEntity {
            entity_type: 106,
            form: 12,
            label: "LOWMODEL".into(),
            status: "00010000",
            parameters: "106,2,4,1,0,0,-0.5,1,0,-0.5,-1,0,1,0,0;".into(),
        },
        OwnedTestEntity {
            entity_type: 110,
            form: 0,
            label: "LOWPCURV".into(),
            status: "00010500",
            parameters: "110,0,0,0,3,0,0;".into(),
        },
        OwnedTestEntity {
            entity_type: lower_type,
            form: 0,
            label: "LOWBOUND".into(),
            status: "00010000",
            parameters: lower,
        },
        OwnedTestEntity {
            entity_type: 106,
            form: 12,
            label: "UPMODEL".into(),
            status: "00010000",
            parameters: "106,2,4,1,0,1,-0.5,-1,1,-0.5,1,1,1,0,1;".into(),
        },
        OwnedTestEntity {
            entity_type: 110,
            form: 0,
            label: "UPPCURV".into(),
            status: "00010500",
            parameters: "110,3,1,0,0,1,0;".into(),
        },
        OwnedTestEntity {
            entity_type: upper_type,
            form: 0,
            label: "UPBOUND".into(),
            status: "00010000",
            parameters: upper,
        },
    ]
}

/// The band of [`ring_band_entities`] with the face record `face` at
/// directory entry 15: a Type 143 face where `bounded`, and otherwise a
/// Type 144 face.
pub(crate) fn ring_band_file(face: &str, bounded: bool, upper_pcurve: bool) -> Vec<u8> {
    let mut entities = ring_band_entities(bounded, upper_pcurve);
    entities.push(OwnedTestEntity {
        entity_type: if bounded { 143 } else { 144 },
        form: 0,
        label: "BAND".into(),
        status: "00000000",
        parameters: face.into(),
    });
    owned_test_file(&entities)
}

/// The Type 144 band of [`ring_band_entities`] with the lower ring as its
/// outer boundary, and as inner boundaries the upper ring and a square hole
/// at directory entry 17 that states no parameter curve. The hole runs
/// through `S(u, v)` at `u` in `[0.4, 0.6]` and `v` in `[0.4, 0.6]`. Where
/// `closed` does not hold, the hole stops at its fourth corner.
pub(crate) fn ring_band_with_bare_hole_file(closed: bool) -> Vec<u8> {
    let corners = "0.4,0.4,0.4,0.1,0.6,0.4,0.1,0.6,0.6,0.4,0.4,0.6";
    let hole = if closed {
        format!("106,2,5,{corners},0.4,0.4,0.4;")
    } else {
        format!("106,2,4,{corners};")
    };
    let mut entities = ring_band_entities(false, true);
    entities.extend([
        OwnedTestEntity {
            entity_type: 106,
            form: 12,
            label: "HOLEMODL".into(),
            status: "00010000",
            parameters: hole,
        },
        OwnedTestEntity {
            entity_type: 142,
            form: 0,
            label: "HOLEBND".into(),
            status: "00010000",
            parameters: "142,0,1,0,15,1;".into(),
        },
        OwnedTestEntity {
            entity_type: 144,
            form: 0,
            label: "BAND".into(),
            status: "00000000",
            parameters: "144,1,1,2,7,13,17;".into(),
        },
    ]);
    owned_test_file(&entities)
}
