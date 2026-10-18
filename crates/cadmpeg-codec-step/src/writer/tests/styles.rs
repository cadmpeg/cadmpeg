// SPDX-License-Identifier: Apache-2.0
//! STEP writer presentation-style ordering tests.

use cadmpeg_ir::examples::unit_cube;
use cadmpeg_ir::CadIr;

use crate::export::write_step;
use crate::{StepSchema, StepWriteOptions};

use super::round_trips::wire_body_ir;

/// One coloured wire body per cube edge, in arena order, with red channels
/// 0.1, 0.2, ... so each style's `COLOUR_RGB` names its body.
fn coloured_wire_bodies_ir(count: usize) -> CadIr {
    use cadmpeg_ir::ids::{BodyId, RegionId, ShellId};

    let template = wire_body_ir(1.0);
    let cube = unit_cube().expect("unit cube fixture is admitted");
    let mut ir = template.clone();
    let edges = cube.model.edges[..count].to_vec();
    ir.model.edges.clone_from(&edges);
    ir.model.curves = cube
        .model
        .curves
        .iter()
        .filter(|curve| edges.iter().any(|edge| edge.curve() == Some(&curve.id)))
        .cloned()
        .collect();
    ir.model.vertices = cube
        .model
        .vertices
        .iter()
        .filter(|vertex| {
            edges
                .iter()
                .any(|edge| edge.start == vertex.id || edge.end == vertex.id)
        })
        .cloned()
        .collect();
    ir.model.points = cube
        .model
        .points
        .iter()
        .filter(|point| ir.model.vertices.iter().any(|v| v.point == point.id))
        .cloned()
        .collect();
    ir.model.shells.clear();
    ir.model.regions.clear();
    ir.model.bodies.clear();
    for (index, edge) in edges.iter().enumerate() {
        let shell_id = ShellId::mint(format!("test:model:shell#wire{index}")).expect("shell id");
        let region_id =
            RegionId::mint(format!("test:model:region#wire{index}")).expect("region id");
        let body_id = BodyId::mint(format!("test:model:body#wire{index}")).expect("body id");
        let mut shell = template.model.shells[0].clone();
        shell.id = shell_id.clone();
        shell.region = region_id.clone();
        shell
            .edit_topology(|_, wire_edges, _| *wire_edges = vec![edge.id.clone()])
            .expect("wire topology");
        let mut region = template.model.regions[0].clone();
        region.id = region_id.clone();
        region.body = body_id.clone();
        region.shells = vec![shell_id];
        let mut body = template.model.bodies[0].clone();
        body.id = body_id;
        body.regions = vec![region_id];
        let red = f32::from(u8::try_from(index + 1).expect("small count")) / 10.0;
        body.color = Some(cadmpeg_ir::topology::Color::new(red, 0.0, 0.0, 1.0).expect("color"));
        ir.model.shells.push(shell);
        ir.model.regions.push(region);
        ir.model.bodies.push(body);
    }
    ir
}

#[test]
fn whole_body_styles_follow_body_arena_order() {
    const BODIES: usize = 8;
    let ir = coloured_wire_bodies_ir(BODIES);
    let write = || {
        let mut output = Vec::new();
        write_step(
            &ir,
            &mut output,
            StepSchema::Ap214,
            &StepWriteOptions::default(),
        )
        .expect("write coloured wire bodies");
        String::from_utf8(output).expect("STEP output is UTF-8")
    };
    let text = write();
    let reds = text
        .lines()
        .filter_map(|line| line.split_once("COLOUR_RGB('',").map(|(_, rest)| rest))
        .map(|rest| {
            rest.split(',')
                .next()
                .and_then(|red| red.parse::<f64>().ok())
                .expect("COLOUR_RGB red channel")
        })
        .collect::<Vec<_>>();
    let expected = (1..=BODIES)
        .map(|index| f64::from(u8::try_from(index).expect("small count")) / 10.0)
        .collect::<Vec<_>>();
    assert_eq!(reds.len(), BODIES, "{text}");
    for (red, wanted) in reds.iter().zip(&expected) {
        assert!((red - wanted).abs() < 1e-6, "{reds:?} != {expected:?}");
    }
    assert_eq!(text.matches("STYLED_ITEM").count(), BODIES);
    for _ in 0..4 {
        assert_eq!(write(), text);
    }
}
