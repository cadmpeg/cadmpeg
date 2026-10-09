// SPDX-License-Identifier: Apache-2.0
//! Body style emission uses the IR body sequence.

use cadmpeg_ir::ids::{BodyId, EdgeId, RegionId, ShellId};
use cadmpeg_ir::topology::Color;
use cadmpeg_ir::CadIr;

use crate::export::write_step;
use crate::loss::StepLossCode;
use crate::{StepSchema, StepWriteOptions};

fn two_colored_wire_bodies(first_alpha: f32, second_alpha: f32) -> CadIr {
    let mut ir = super::wire_body_ir(first_alpha);
    let first_id = BodyId::try_from("test:model:body#a-first").expect("first body identity");
    let second_id = BodyId::try_from("test:model:body#z-second").expect("second body identity");
    let second_region_id =
        RegionId::try_from("test:model:region#a-second").expect("second region identity");
    let second_shell_id =
        ShellId::try_from("test:model:shell#a-second").expect("second shell identity");
    ir.model.bodies[0].id = first_id.clone();
    ir.model.regions[0].body = first_id;
    ir.model.bodies[0].color = Some(Color::new(1.0, 0.0, 0.0, first_alpha).expect("red color"));

    let mut edge = ir.model.edges[0].clone();
    edge.id = EdgeId::try_from("test:model:edge#a-second").expect("second edge identity");
    let shell = cadmpeg_ir::topology::Shell::with_wire_edge(
        second_shell_id.clone(),
        second_region_id.clone(),
        edge.id.clone(),
    );
    let mut region = ir.model.regions[0].clone();
    region.id = second_region_id.clone();
    region.body = second_id.clone();
    region.shells = vec![second_shell_id];
    let mut body = ir.model.bodies[0].clone();
    body.id = second_id;
    body.regions = vec![second_region_id];
    body.color = Some(Color::new(0.0, 1.0, 0.0, second_alpha).expect("green color"));
    ir.model.edges.push(edge);
    ir.model.shells.push(shell);
    ir.model.regions.push(region);
    ir.model.bodies.push(body);
    let validation = cadmpeg_ir::validate_neutral(&ir, Vec::new())
        .expect("fixture validation does not refuse resources");
    assert!(validation.is_ok(), "{:#?}", validation.findings);
    ir
}

#[test]
fn colored_wire_body_styles_follow_source_order_and_repeat_identically() {
    let ir = two_colored_wire_bodies(1.0, 1.0);
    for schema in [StepSchema::Ap214, StepSchema::Ap242Edition3] {
        let options = StepWriteOptions::default();
        let mut expected = Vec::new();
        write_step(&ir, &mut expected, schema, &options).expect("write two colored wire bodies");
        let text = std::str::from_utf8(&expected).expect("STEP writer emits UTF-8");
        let red = text.find("COLOUR_RGB('',1.,0.,0.)").expect("first red body color");
        let green = text.find("COLOUR_RGB('',0.,1.,0.)").expect("second green body color");
        assert!(red < green, "body source order governs style instance order");
        assert_eq!(text.matches("STYLED_ITEM").count(), 2);
        for _ in 0..64 {
            let mut repeated = Vec::new();
            write_step(&ir, &mut repeated, schema, &options).expect("repeat two-body write");
            assert_eq!(repeated, expected, "new RandomState seeds must not reorder emission");
        }
    }
}

#[test]
fn wire_body_transparency_losses_follow_body_source_order() {
    let ir = two_colored_wire_bodies(0.5, 0.25);
    let mut output = Vec::new();
    let report = write_step(
        &ir,
        &mut output,
        StepSchema::Ap214,
        &StepWriteOptions::default(),
    )
    .expect("write translucent colored wire bodies");
    let losses = report.losses.iter()
        .filter(|loss| loss.code == StepLossCode::WireBodyTransparencyOmitted.kind())
        .collect::<Vec<_>>();
    assert_eq!(losses.len(), 2);
    assert!(losses[0].message.contains("test:model:body#a-first"));
    assert!(losses[1].message.contains("test:model:body#z-second"));
}
