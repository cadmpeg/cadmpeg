// SPDX-License-Identifier: Apache-2.0
//! STEP writer presentation-style ordering tests.

use cadmpeg_ir::examples::unit_cube;
use cadmpeg_ir::CadIr;

use crate::export::write_step;
use crate::{StepSchema, StepWriteOptions};

use super::round_trips::wire_body_ir;

#[test]
fn damaged_unbound_colours_survive_synthesis_without_bindings() {
    use cadmpeg_ir::codec::{Codec, DecodeOptions};
    use std::io::Cursor;

    let source = include_bytes!("../../../tests/fixtures/damaged/ap214_sheet_bad_real.p21");
    let original = crate::StepCodec::default()
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .expect("decode damaged STEP");
    assert_eq!(original.ir().model.appearances.len(), 2);
    assert!(original.ir().model.appearance_bindings.is_empty());
    assert!(original
        .ir()
        .model
        .appearances
        .iter()
        .all(|appearance| appearance.schema.is_none()));
    let mut bytes = Vec::new();
    write_step(
        original.ir(),
        &mut bytes,
        StepSchema::Ap242Edition3,
        &StepWriteOptions::default(),
    )
    .expect("write STEP");
    let decoded = crate::StepCodec::default()
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .expect("decode written STEP");
    for appearance in &original.ir().model.appearances {
        assert!(
            decoded.ir().model.appearances.iter().any(|written| {
                written.name == appearance.name && written.base_color == appearance.base_color
            }),
            "missing appearance {:?}",
            appearance.name
        );
    }
    assert_eq!(
        decoded.ir().model.appearance_bindings.len(),
        original.ir().model.appearance_bindings.len()
    );
    for loss in &decoded.report().losses {
        assert!(
            original
                .report()
                .losses
                .iter()
                .any(|source| source.code == loss.code),
            "new decode loss {}",
            loss.code
        );
    }
}

#[test]
fn one_appearance_is_shared_across_curve_surface_and_point_styles() {
    use cadmpeg_ir::codec::{Codec, DecodeOptions};
    use std::io::Cursor;

    let source = include_bytes!("../../../tests/fixtures/ap242_geometry.p21");
    let original = crate::StepCodec::default()
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .expect("decode styled geometry");
    assert!(original
        .ir()
        .model
        .appearances
        .iter()
        .all(|appearance| appearance.schema.is_none()));
    let mut bytes = Vec::new();
    write_step(
        original.ir(),
        &mut bytes,
        StepSchema::Ap242Edition3,
        &StepWriteOptions::default(),
    )
    .expect("write styled geometry");
    let decoded = crate::StepCodec::default()
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .expect("decode written styles");
    assert_eq!(decoded.ir().model.appearances.len(), 1);
    assert!(decoded.ir().model.appearances[0].schema.is_none());
    assert_eq!(
        decoded.ir().model.appearance_bindings.len(),
        original.ir().model.appearance_bindings.len()
    );
    let appearance = &decoded.ir().model.appearances[0].id;
    assert!(decoded
        .ir()
        .model
        .appearance_bindings
        .iter()
        .all(|binding| &binding.appearance == appearance));
}

#[test]
fn a_stated_material_schema_is_reported_as_unrepresented() {
    use cadmpeg_ir::codec::{Codec, DecodeOptions};
    let source = include_bytes!("../../../tests/fixtures/ap242_geometry.p21");
    let original = crate::StepCodec::default()
        .decode(&mut std::io::Cursor::new(source), &DecodeOptions::default())
        .expect("decode styled geometry");
    let mut document = original.ir().clone();
    document.model.appearances[0].schema = Some("step_surface_style".into());
    let id = document.model.appearances[0].id.to_string();
    let mut bytes = Vec::new();
    let report = write_step(
        &document,
        &mut bytes,
        StepSchema::Ap242Edition3,
        &StepWriteOptions::default(),
    )
    .expect("write styled geometry");
    assert!(report.losses.iter().any(|loss| loss.code
        == crate::loss::StepLossCode::AppearanceReducedToBaseColor.kind()
        && loss.message.contains(&id)));
}

#[test]
fn equal_colours_keep_distinct_appearances() {
    use cadmpeg_ir::codec::{Codec, DecodeOptions};
    use cadmpeg_ir::ids::AppearanceId;
    use std::io::Cursor;

    let source = include_bytes!("../../../tests/fixtures/ap242_geometry.p21");
    let original = crate::StepCodec::default()
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .expect("decode styled geometry");
    let mut ir = original.ir().clone();
    let mut second = ir.model.appearances[0].clone();
    second.id = AppearanceId::mint("test:model:appearance#second").expect("appearance id");
    ir.model.appearance_bindings[0].appearance = second.id.clone();
    ir.model.appearances.push(second);
    let mut bytes = Vec::new();
    write_step(
        &ir,
        &mut bytes,
        StepSchema::Ap242Edition3,
        &StepWriteOptions::default(),
    )
    .expect("write styled geometry");
    let decoded = crate::StepCodec::default()
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .expect("decode written styles");
    assert_eq!(decoded.ir().model.appearances.len(), 2);
    assert_eq!(
        decoded
            .ir()
            .model
            .appearance_bindings
            .iter()
            .map(|binding| &binding.appearance)
            .collect::<std::collections::BTreeSet<_>>()
            .len(),
        2
    );
}

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

#[test]
fn body_appearance_keeps_its_owner_when_faces_inherit_its_color() {
    use cadmpeg_ir::appearance::{Appearance, AppearanceBinding, AppearanceTarget};
    use cadmpeg_ir::codec::{Codec, DecodeOptions};
    use cadmpeg_ir::ids::AppearanceId;
    use cadmpeg_ir::topology::Color;
    use std::io::Cursor;

    let mut ir = unit_cube().expect("unit cube");
    let body = ir.model.bodies[0].id.clone();
    let appearance = AppearanceId::mint("test:model:appearance#body").expect("appearance id");
    let color = Color::new(0.125, 0.25, 0.5, 1.0).expect("color");
    ir.model.appearances.push(Appearance {
        id: appearance.clone(),
        native_ref: None,
        name: Some("body material".into()),
        asset_guid: None,
        library_id: None,
        visual_guid: None,
        physical_token: None,
        schema: None,
        category: None,
        base_color: Some(color),
        properties: std::collections::BTreeMap::default(),
        textures: Vec::new(),
    });
    let binding = "test:model:appearance-binding#body";
    ir.model.appearance_bindings.push(AppearanceBinding {
        id: binding.try_into().expect("binding id"),
        native_ref: None,
        target: AppearanceTarget::Body(body),
        appearance,
        source_entity_id: None,
        object_type: None,
        visible: None,
        channels: std::collections::BTreeMap::default(),
    });
    let mut bytes = Vec::new();
    let report = write_step(
        &ir,
        &mut bytes,
        StepSchema::Ap242Edition3,
        &StepWriteOptions::default(),
    )
    .expect("write body appearance");
    let text = String::from_utf8_lossy(&bytes);
    let solid = text
        .lines()
        .find(|line| line.contains("MANIFOLD_SOLID_BREP("))
        .expect("solid")
        .split_once(" = ")
        .unwrap()
        .0;
    assert!(text
        .lines()
        .any(|line| line.contains("STYLED_ITEM(") && line.ends_with(&format!(",{solid});"))));
    assert!(!report.losses.iter().any(|loss| loss.code
        == crate::loss::StepLossCode::AppearanceBindingBodyFallback.kind()
        && loss.message.contains(binding)));
    let decoded = crate::StepCodec::default()
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .expect("decode styles");
    assert_eq!(decoded.ir().model.appearance_bindings.len(), 1);
    let output_body = &decoded.ir().model.bodies[0].id;
    let output_binding = decoded
        .ir()
        .model
        .appearance_bindings
        .iter()
        .find(|record| matches!(&record.target, AppearanceTarget::Body(id) if id == output_body))
        .expect("body binding");
    let material = decoded
        .ir()
        .model
        .appearances
        .iter()
        .find(|record| record.id == output_binding.appearance)
        .expect("bound material");
    assert_eq!(material.base_color, Some(color));
}

#[test]
fn unbound_appearance_does_not_style_an_unrelated_carrier() {
    use cadmpeg_ir::appearance::Appearance;
    use cadmpeg_ir::ids::AppearanceId;
    use cadmpeg_ir::topology::Color;

    let mut ir = unit_cube().expect("unit cube");
    let id = AppearanceId::mint("test:model:appearance#unbound").expect("appearance id");
    ir.model.appearances.push(Appearance {
        id: id.clone(),
        native_ref: None,
        name: Some("unbound".into()),
        asset_guid: None,
        library_id: None,
        visual_guid: None,
        physical_token: None,
        schema: None,
        category: None,
        base_color: Some(Color::new(0.125, 0.25, 0.5, 0.75).expect("color")),
        properties: std::collections::BTreeMap::default(),
        textures: Vec::new(),
    });
    let mut bytes = Vec::new();
    let report = write_step(
        &ir,
        &mut bytes,
        StepSchema::Ap242Edition3,
        &StepWriteOptions::default(),
    )
    .expect("write unbound color");
    let text = String::from_utf8(bytes.clone()).expect("STEP text");
    assert!(text.contains("COLOUR_RGB('unbound',0.125,0.25,0.5)"));
    assert!(text.contains("SURFACE_STYLE_TRANSPARENT(0.25)"));
    assert!(!text.contains("STYLED_ITEM("));
    assert!(report.losses.iter().any(|loss| loss.code
        == crate::loss::StepLossCode::AppearanceReducedToBaseColor.kind()
        && loss.message.contains(id.as_str())));
    let decoded = <crate::StepCodec as cadmpeg_ir::codec::Codec>::decode(
        &crate::StepCodec::default(),
        &mut std::io::Cursor::new(bytes),
        &cadmpeg_ir::codec::DecodeOptions::default(),
    )
    .expect("decode unbound color");
    assert!(decoded.ir().model.appearance_bindings.is_empty());
    assert!(decoded.ir().model.appearances.iter().any(|appearance| {
        appearance.name.as_deref() == Some("unbound")
            && appearance.base_color == Some(Color::new(0.125, 0.25, 0.5, 0.75).expect("color"))
    }));
}

#[test]
fn appearance_of_omitted_face_keeps_its_color_without_a_binding() {
    use cadmpeg_ir::appearance::{Appearance, AppearanceBinding, AppearanceTarget};
    use cadmpeg_ir::codec::{Codec, DecodeOptions};
    use cadmpeg_ir::geometry::{SolvedSurfaceGeometry, SurfaceGeometry};
    use cadmpeg_ir::ids::AppearanceId;
    use cadmpeg_ir::topology::Color;
    use std::io::Cursor;

    let mut ir = unit_cube().expect("unit cube");
    let face = ir.model.faces[0].id.clone();
    let surface = ir.model.faces[0].surface.clone();
    ir.model
        .surfaces
        .iter_mut()
        .find(|item| item.id == surface)
        .unwrap()
        .geometry = SurfaceGeometry::Solved(SolvedSurfaceGeometry::Cone(
        cadmpeg_ir::geometry::analytic::ConeSurface::try_new(
            cadmpeg_ir::math::Point3::new(0.0, 0.0, 0.0),
            cadmpeg_ir::math::Vector3::new(0.0, 0.0, 1.0),
            cadmpeg_ir::math::Vector3::new(1.0, 0.0, 0.0),
            2.0,
            0.4,
            0.5,
        )
        .expect("elliptical cone"),
    ));
    let appearance = AppearanceId::mint("test:model:appearance#omitted-face").expect("id");
    let color = Color::new(0.125, 0.25, 0.5, 1.0).expect("color");
    ir.model.appearances.push(Appearance {
        id: appearance.clone(),
        native_ref: None,
        name: Some("omitted face".into()),
        asset_guid: None,
        library_id: None,
        visual_guid: None,
        physical_token: None,
        schema: None,
        category: None,
        base_color: Some(color),
        properties: std::collections::BTreeMap::default(),
        textures: Vec::new(),
    });
    let binding = "test:model:appearance-binding#omitted-face";
    ir.model.appearance_bindings.push(AppearanceBinding {
        id: binding.try_into().expect("id"),
        native_ref: None,
        target: AppearanceTarget::Face(face),
        appearance,
        source_entity_id: None,
        object_type: None,
        visible: None,
        channels: std::collections::BTreeMap::default(),
    });
    let mut bytes = Vec::new();
    let report = write_step(
        &ir,
        &mut bytes,
        StepSchema::Ap242Edition3,
        &StepWriteOptions::default(),
    )
    .expect("write appearance");
    assert!(report
        .losses
        .iter()
        .any(|loss| loss.message.contains(binding)));
    let decoded = crate::StepCodec::default()
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .expect("decode appearance");
    assert!(
        decoded
            .ir()
            .model
            .appearances
            .iter()
            .any(|item| item.name.as_deref() == Some("omitted face")
                && item.base_color == Some(color))
    );
    assert!(decoded.ir().model.appearance_bindings.is_empty());
}
