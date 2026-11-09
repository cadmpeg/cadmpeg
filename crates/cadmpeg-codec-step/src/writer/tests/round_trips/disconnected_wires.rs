// SPDX-License-Identifier: Apache-2.0
//! Disconnected wire export and reload.

use super::*;

#[test]
fn disconnected_wire_edges_export_in_the_connected_set_of_their_shell() {
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
    assert_eq!(exchange.entities("CONNECTED_EDGE_SET").count(), 1);
    let decoded = StepCodec::default()
        .decode(&mut Cursor::new(output), &DecodeOptions::default())
        .expect("round trip");
    assert_eq!(decoded.ir().model.edges.len(), 2);
    assert_eq!(decoded.ir().model.shells.len(), 1);
    assert_eq!(decoded.ir().model.shells[0].wire_edges().len(), 2);
    assert!(cadmpeg_ir::validate_neutral(decoded.ir(), Vec::new())
        .expect("validation")
        .is_ok());
}

#[test]
fn a_wire_body_with_two_regions_is_one_wireframe_model() {
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
    let region = cadmpeg_ir::ids::RegionId::mint("test:model:region#second").expect("id");
    let shell = cadmpeg_ir::ids::ShellId::mint("test:model:shell#second").expect("id");
    ir.model.shells.push(
        cadmpeg_ir::topology::Shell::new(
            shell.clone(),
            region.clone(),
            Vec::new(),
            vec![second.id.clone()],
            Vec::new(),
        )
        .expect("second shell"),
    );
    ir.model.regions.push(cadmpeg_ir::topology::Region {
        native_ref: None,
        id: region.clone(),
        body: ir.model.bodies[0].id.clone(),
        shells: vec![shell],
    });
    ir.model.bodies[0].regions.push(region);
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
    assert_eq!(exchange.entities("EDGE_BASED_WIREFRAME_MODEL").count(), 1);
    assert_eq!(exchange.entities("CONNECTED_EDGE_SET").count(), 2);
    let decoded = StepCodec::default()
        .decode(&mut Cursor::new(output), &DecodeOptions::default())
        .expect("round trip");
    assert_eq!(decoded.ir().model.bodies.len(), 1);
    assert_eq!(decoded.ir().model.bodies[0].regions.len(), 2);
    assert_eq!(decoded.ir().model.regions.len(), 2);
    assert!(cadmpeg_ir::validate_neutral(decoded.ir(), Vec::new())
        .expect("validation")
        .is_ok());
}
