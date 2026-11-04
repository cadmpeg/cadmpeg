// SPDX-License-Identifier: Apache-2.0
//! Polygon arenas: typed records, payload id lists, and W1 re-emission (#224).

use super::super::{
    parse_text, polygon3d_text, polygon_on_triangulation_text, ShapePayload, Tables,
    TextTopologyVersion,
};

fn text_member(polygons3d: &str, indexed: &str) -> Vec<u8> {
    format!(
        "CASCADE Topology V3, (c) Open Cascade\nLocations 0\nCurve2ds 0\nCurves 0\nPolygon3D 2\n{polygons3d}\nPolygonOnTriangulations 2\n{indexed}\nSurfaces 0\nTriangulations 1\n3 1 1 1 0.01 0 0 0 1 0 0 0 1 0 0 0 1 0 0 1 1 2 3 0 0 1 0 0 1 0 0 1\nTShapes 0\n*"
    )
    .into_bytes()
}

fn literal_member() -> Vec<u8> {
    text_member(
        "2 1 0.1 0 0 0 1 0 0 0 1\n1 0 0.2 2 2 2",
        "2 1 2 p 0.3 1 0 1\n1 3 p 0.4 0",
    )
}

#[test]
fn text_payload_names_two_records_per_table_in_order() {
    let bytes = literal_member();
    let (facts, polygons, _) = parse_text(&bytes, "payload").expect("polygonal member");
    assert_eq!(
        facts.polygons3d,
        [
            "fcstd:native:polygons3d#payload%3A1",
            "fcstd:native:polygons3d#payload%3A2"
        ]
    );
    assert_eq!(
        facts.polygons_on_triangulations,
        [
            "fcstd:native:polygons_on_triangulations#payload%3A1",
            "fcstd:native:polygons_on_triangulations#payload%3A2"
        ]
    );
    assert_eq!(polygons.polygons3d.len(), 2);
    assert_eq!(polygons.polygons_on_triangulations.len(), 2);
    assert_eq!(polygons.polygons3d[0].id, facts.polygons3d[0]);
    assert_eq!(polygons.polygons3d[1].id, facts.polygons3d[1]);
    assert_eq!(polygons.polygons3d[0].nodes.len(), 2);
    assert_eq!(polygons.polygons3d[1].nodes.len(), 1);
    assert_eq!(polygons.polygons_on_triangulations[1].nodes, [3]);
    assert!(polygons.polygons3d[0].parameters.is_some());
    assert!(polygons.polygons3d[1].parameters.is_none());
    assert!(polygons.polygons_on_triangulations[0].parameters.is_some());
    assert!(polygons.polygons_on_triangulations[1].parameters.is_none());
}

#[test]
fn missing_record_id_fails_table_resolution() {
    let bytes = literal_member();
    let (facts, _, _) = parse_text(&bytes, "payload").expect("polygonal member");
    let payload = ShapePayload::Text {
        facts,
        version: TextTopologyVersion::V3,
    };
    let error = match Tables::from_shape(&payload, "payload", &[], &[]) {
        Ok(_) => panic!("absent record resolves"),
        Err(error) => error,
    };
    assert!(
        error.to_string().contains("absent polygon record"),
        "{error}"
    );
}

#[test]
fn written_tables_round_trip_to_equal_values() {
    let bytes = literal_member();
    let (_, polygons, _) = parse_text(&bytes, "payload").expect("polygonal member");
    let entries3d: Vec<String> = polygons.polygons3d.iter().map(polygon3d_text).collect();
    let entries_indexed: Vec<String> = polygons
        .polygons_on_triangulations
        .iter()
        .map(polygon_on_triangulation_text)
        .collect();
    assert_eq!(entries3d.len(), 2);
    assert_eq!(entries_indexed.len(), 2);
    let rewritten = text_member(&entries3d.join("\n"), &entries_indexed.join("\n"));
    let (facts, reparsed, _) = parse_text(&rewritten, "payload").expect("rewritten member parses");
    assert_eq!(reparsed.polygons3d, polygons.polygons3d);
    assert_eq!(
        reparsed.polygons_on_triangulations,
        polygons.polygons_on_triangulations
    );
    // The canonical text is a fixpoint: writing the reparsed records
    // yields byte-equal tables.
    let rewrite3d: Vec<String> = reparsed.polygons3d.iter().map(polygon3d_text).collect();
    let rewrite_indexed: Vec<String> = reparsed
        .polygons_on_triangulations
        .iter()
        .map(polygon_on_triangulation_text)
        .collect();
    assert_eq!(rewrite3d, entries3d);
    assert_eq!(rewrite_indexed, entries_indexed);
    assert_eq!(facts.polygons3d.len(), 2);
}
