// SPDX-License-Identifier: Apache-2.0
use super::*;

#[test]
fn a_failed_polygon_row_retains_the_payload_and_all_other_readable_tables() {
    for (table, row) in [(3, "2 0 0 0 0 0 1 0 0"), (4, "2 1 2 p 0 0")] {
        let polygon = if table == 3 {
            format!("2\n{row}\nNaN")
        } else {
            "0".into()
        };
        let indexed = if table == 4 {
            format!("2\n{row}\nNaN")
        } else {
            "0".into()
        };
        let bytes = format!("CASCADE Topology V3, (c) Open Cascade\nLocations 0\nCurve2ds 0\nCurves 1\n1 0 0 0 1 0 0\nPolygon3D {polygon}\nPolygonOnTriangulations {indexed}\nSurfaces 1\n1 0 0 0 0 0 1 1 0 0 0 1 0\nTriangulations 0\nTShapes 0\n*\n");
        decode(bytes.as_bytes(), false, |ctx, output, payload| {
            let payload =
                payload.expect("a failed own polygon row cannot discard the canonical payload");
            let facts = payload.shape_set().unwrap();
            assert_eq!(facts.curves.len(), 1);
            assert_eq!(facts.surfaces.len(), 1);
            let native = output.native().namespace("fcstd").unwrap();
            let payloads = records::payloads(ctx, native).unwrap();
            assert_eq!(payloads.len(), 1);
            let wire = serde_json::to_value(&payloads[0]).unwrap();
            let field = if table == 3 {
                "polygons3d"
            } else {
                "polygons_on_triangulations"
            };
            let selector = if table == 3 {
                "Polygon3D"
            } else {
                "PolygonOnTriangulations"
            };
            assert_eq!(wire["text"][field].as_array().unwrap().len(), 1);
            assert_eq!(wire["text"]["section_counts"][selector], 2);
            assert!(wire["text"]["source_indices"].get("3").is_none());
            assert!(wire["text"]["source_indices"].get("4").is_none());
            let rows = records::polygons::polygon_rows(native, &payloads)
                .unwrap()
                .into_iter()
                .filter(|row| row.value.table_order() == table)
                .collect::<Vec<_>>();
            assert_eq!(rows.len(), 1);
            assert_eq!(rows[0].index, 1);
            let mut invalid = native.clone();
            let wrong_variant = serde_json::json!({"id": rows[0].id, "table":"roots", "value":[]});
            invalid.set_arena(ctx, field, &[wrong_variant]).unwrap();
            assert!(records::validate(ctx, &invalid, &payloads).is_err());
            assert!(output
                .fidelity()
                .parts()
                .values()
                .any(|part| matches!(part.state(), PartState::GivenUp {})));
        });
    }
}
