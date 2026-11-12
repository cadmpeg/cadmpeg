// SPDX-License-Identifier: Apache-2.0

use super::decode;
use cadmpeg_ir::math::Point3;

fn assert_square(bytes: &[u8]) {
    let decoded = decode(bytes.to_vec());
    let ir = decoded.ir();
    let validation =
        cadmpeg_ir::validate_neutral(ir, Vec::new()).expect("validate generated sheet");
    assert!(validation.is_ok(), "{:?}", validation.findings);
    assert_eq!(ir.model.faces.len(), 1);
    assert_eq!(ir.model.edges.len(), 4);
    assert_eq!(ir.model.vertices.len(), 4);
    for (x, y) in [(0.0, 0.0), (1.0, 0.0), (1.0, 1.0), (0.0, 1.0)] {
        let expected = Point3::new(x, y, 0.0);
        assert!(ir.model.vertices.iter().any(|vertex| {
            ir.model
                .points
                .iter()
                .find(|point| point.id == vertex.point)
                .expect("vertex point")
                .position()
                .get()
                .distance(expected)
                <= 1.0e-6
        }));
    }
}

#[test]
fn native_sheet_with_extended_endpoint_stations_decodes() {
    assert_square(include_bytes!(
        "../../tests/fixtures/endpoint_stations/extended-station-sheet.catpart"
    ));
}

#[test]
fn native_sheet_reports_ambiguous_station_readings() {
    let bytes =
        include_bytes!("../../tests/fixtures/endpoint_stations/ambiguous-station-sheet.catpart");
    assert_square(bytes);
    let decoded = decode(bytes.to_vec());
    let notes = decoded
        .report()
        .losses
        .iter()
        .filter(|note| note.code.local_code() == "topology.endpoint-station-reading-inferred")
        .collect::<Vec<_>>();
    assert!(!notes.is_empty());
    assert!(notes
        .iter()
        .all(|note| note.message.contains("first-scalar tag")));
}
