// SPDX-License-Identifier: Apache-2.0

use crate::document::Model;
use crate::geometry::{
    Curve, CurveGeometry, ProceduralCurve, ProceduralCurveDefinition, ProceduralSurface,
    ProceduralSurfaceDefinition, RecordBounds, Surface, SurfaceGeometry,
};

#[test]
fn procedural_wire_borrows_definitions_and_preserves_row_bytes() {
    let surface_owner = "test:model:surface#wire".try_into().expect("surface identity");
    let curve_owner = "test:model:curve#wire".try_into().expect("curve identity");
    let curve = ProceduralCurve::new(
        "test:model:curve-construction#wire".try_into().expect("construction identity"),
        ProceduralCurveDefinition::Exact { cache: None },
    );
    let curve_text = r#"{"id":"test:model:curve-construction#wire","curve":"test:model:curve#wire","definition":{"kind":"exact"}}"#;
    assert_eq!(
        serde_json::to_string(&super::super::ProceduralCurveWire {
            owner: Some(&curve_owner),
            procedural: &curve,
        }).expect("curve row serialization"),
        curve_text,
    );
    for (bounds, surface_text) in [
        (None, r#"{"id":"test:model:surface-construction#wire","surface":"test:model:surface#wire","definition":{"kind":"unknown"}}"#),
        (
            Some(RecordBounds::try_new([Some(1.0), None, Some(2.0), None]).expect("finite bounds")),
            r#"{"id":"test:model:surface-construction#wire","surface":"test:model:surface#wire","definition":{"kind":"unknown"},"record_bounds":[1.0,null,2.0,null]}"#,
        ),
    ] {
        let surface = ProceduralSurface::new(
            "test:model:surface-construction#wire".try_into().expect("construction identity"),
            ProceduralSurfaceDefinition::Unknown { record: None, cache: None },
            bounds,
        );
        assert_eq!(
            serde_json::to_string(&super::super::ProceduralSurfaceWire {
                owner: Some(&surface_owner),
                procedural: &surface,
            }).expect("surface row serialization"),
            surface_text,
        );
        let model = Model {
            surfaces: vec![Surface {
                id: surface_owner.clone(),
                geometry: SurfaceGeometry::Procedural {
                    construction: surface.id.clone(),
                    cache: None,
                },
                source_object: None,
            }],
            curves: vec![Curve {
                id: curve_owner.clone(),
                geometry: CurveGeometry::Procedural {
                    construction: curve.id.clone(),
                    cache: None,
                },
                source_object: None,
            }],
            procedural_surfaces: vec![surface],
            procedural_curves: vec![curve.clone()],
            ..Model::default()
        };
        let ctx = cadmpeg_test_support::service_decode_context();
        assert_eq!(
            serde_json::to_string(&model.sorted(&ctx).expect("sorted model admission"))
                .expect("sorted model serialization"),
            serde_json::to_string(&model).expect("model serialization"),
        );
        let snapshot = serde_json::to_string(
            &model.geometry_snapshot(&ctx, "brep").expect("snapshot admission"),
        ).expect("snapshot serialization");
        assert!(snapshot.contains(surface_text));
        assert!(snapshot.contains(curve_text));
        assert_eq!(
            serde_json::from_str::<Model>(&serde_json::to_string(&model).expect("model serialization"))
                .expect("model deserialization"),
            model,
        );
    }
}
