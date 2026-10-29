// SPDX-License-Identifier: Apache-2.0
//! Disconnected wire export and reload.

use super::*;

#[test]
fn disconnected_wire_edges_export_in_separate_connected_sets() {
    let full = unit_cube().expect("cube");
    let mut ir = wire_body_ir(1.0);
    let second = full
        .model
        .edges
        .iter()
        .find(|edge| {
            ir.model
                .vertices
                .iter()
                .all(|v| v.id != edge.start && v.id != edge.end)
        })
        .expect("disconnected edge")
        .clone();
    let curve = second.curve().expect("curve");
    ir.model.curves.push(
        full.model
            .curves
            .iter()
            .find(|item| item.id == *curve)
            .expect("carrier")
            .clone(),
    );
    for vertex in full
        .model
        .vertices
        .iter()
        .filter(|v| v.id == second.start || v.id == second.end)
    {
        ir.model.points.push(
            full.model
                .points
                .iter()
                .find(|p| p.id == vertex.point)
                .expect("point")
                .clone(),
        );
        ir.model.vertices.push(vertex.clone());
    }
    ir.model.shells[0]
        .edit_topology(|_, edges, _| edges.push(second.id.clone()))
        .expect("wire topology");
    ir.model.edges.push(second);
    assert!(cadmpeg_ir::validate_neutral(&ir, Vec::new())
        .expect("neutral validation")
        .is_ok());
    let mut output = Vec::new();
    write_step(
        &ir,
        &mut output,
        StepSchema::Ap242Edition3,
        &StepWriteOptions::default(),
    )
    .expect("export");
    let (exchange, _) =
        crate::test_support::with_service_context(&output, crate::parse::parse_inner)
            .expect("exchange");
    assert_eq!(exchange.entities("CONNECTED_EDGE_SET").count(), 2);
    let decoded = StepCodec::default()
        .decode(&mut Cursor::new(output), &DecodeOptions::default())
        .expect("round trip");
    assert_eq!(decoded.ir().model.edges.len(), 2);
    assert!(cadmpeg_ir::validate_neutral(decoded.ir(), Vec::new())
        .expect("validation")
        .is_ok());
}
