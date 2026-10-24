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
