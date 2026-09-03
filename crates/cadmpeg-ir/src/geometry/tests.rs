// SPDX-License-Identifier: Apache-2.0
#![allow(clippy::unwrap_used)]

use crate::examples::unit_cube;
use crate::geometry::SurfaceGeometry;
use crate::ids::UnknownId;
use crate::unknown::NativeUnknownRecord;

/// Replace the surface of the cube's first face with an unknown surface,
/// optionally linking a preserved record, and return the face id and its
/// surface id. Leaves every loop/coedge/edge of the face intact.
fn make_first_face_surface_unknown(ir: &mut crate::CadIr, record: Option<UnknownId>) -> String {
    let face = &ir.model.faces[0];
    let surface_id = face.surface.0.clone();
    for s in &mut ir.model.surfaces {
        if s.id.0 == surface_id {
            s.geometry = SurfaceGeometry::Unknown { record };
            break;
        }
    }
    surface_id
}

#[test]
fn unknown_surface_json_round_trips() {
    let mut ir = unit_cube();
    let rec = UnknownId("synthetic:cube:unknown#0".into());
    ir.set_native_unknowns(
        "synthetic",
        &[NativeUnknownRecord {
            id: rec.clone(),
            links: Vec::new(),
        }],
    )
    .unwrap();
    make_first_face_surface_unknown(&mut ir, Some(rec));

    let json = ir.to_canonical_json().unwrap();
    let parsed = crate::CadIr::from_json(&json).unwrap();
    assert_eq!(parsed, ir, "round-trip must preserve the unknown surface");
}

#[test]
fn ordered_pcurve_uses_round_trip_with_isoparametric_state() {
    let uses = vec![
        crate::topology::PcurveUse {
            pcurve: crate::ids::PcurveId("test:pcurve#first".into()),
            isoparametric: Some(true),
            parameter_range: None,
        },
        crate::topology::PcurveUse {
            pcurve: crate::ids::PcurveId("test:pcurve#second".into()),
            isoparametric: Some(false),
            parameter_range: Some([0.0, 1.0]),
        },
    ];
    let json = serde_json::to_string(&uses).unwrap();
    assert_eq!(
        serde_json::from_str::<Vec<crate::topology::PcurveUse>>(&json).unwrap(),
        uses
    );
}

#[test]
fn g2_full_support_keeps_the_flat_wire_shape() {
    let shape = crate::geometry::G2BlendFirstShape::Full {
        support: Some(crate::geometry::G2BlendFullSupport {
            surface: crate::ids::SurfaceId("test:surface#support".into()),
            tolerance: 0.02,
        }),
    };
    let value = serde_json::to_value(&shape).unwrap();
    assert_eq!(
        value,
        serde_json::json!({
            "kind": "full",
            "surface": "test:surface#support",
            "tolerance": 0.02
        })
    );
    assert_eq!(
        serde_json::from_value::<crate::geometry::G2BlendFirstShape>(value).unwrap(),
        shape
    );
}

#[test]
fn g2_full_support_rejects_split_wire_fields() {
    let error = serde_json::from_value::<crate::geometry::G2BlendFirstShape>(serde_json::json!({
        "kind": "full",
        "surface": "test:surface#support"
    }))
    .unwrap_err();
    assert!(error
        .to_string()
        .contains("G2 full surface and tolerance must occur together"));
}

#[derive(Debug, PartialEq, serde::Serialize, serde::Deserialize)]
struct RevisionCompoundLoftDirectionWireTest {
    #[serde(flatten, with = "super::revision_compound_loft_direction_wire")]
    direction: crate::geometry::CompoundLoftDirection,
}

#[test]
fn revision_compound_loft_direction_keeps_the_flat_wire_shape() {
    let value = RevisionCompoundLoftDirectionWireTest {
        direction: crate::geometry::CompoundLoftDirection::Curve {
            curve: crate::ids::CurveId("test:curve#direction".into()),
            selector: std::num::NonZeroI64::new(4).unwrap(),
        },
    };
    let wire = serde_json::to_value(&value).unwrap();
    assert_eq!(
        wire,
        serde_json::json!({
            "selector": 4,
            "direction_curve": "test:curve#direction"
        })
    );
    assert_eq!(
        serde_json::from_value::<RevisionCompoundLoftDirectionWireTest>(wire).unwrap(),
        value
    );
}

#[test]
fn revision_compound_loft_direction_rejects_a_mismatched_selector() {
    let error =
        serde_json::from_value::<RevisionCompoundLoftDirectionWireTest>(serde_json::json!({
            "selector": 0,
            "direction_curve": "test:curve#direction"
        }))
        .unwrap_err();
    assert!(error
        .to_string()
        .contains("compound-loft direction conflicts with its selector"));
}
