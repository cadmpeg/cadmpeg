// SPDX-License-Identifier: Apache-2.0
#![allow(clippy::unwrap_used)]

use crate::examples::unit_cube;
use crate::CadIr;

#[test]
fn appearance_asset_and_binding_round_trip() {
    use crate::appearance::{
        Appearance, AppearanceBinding, AppearanceTarget, BumpMap, TextureMap2d, TextureRef,
    };
    use crate::ids::AppearanceId;
    use crate::scalar::{Angle, FiniteReal, Length};

    let real = |value| FiniteReal::new(value).unwrap();
    let length = |value| Length::new(value).unwrap();
    let mut ir = unit_cube().expect("valid unit cube fixture");
    let body = ir.model.bodies[0].id.clone();
    ir.model.appearances.push(Appearance {
        native_ref: None,
        id: AppearanceId::mint("synthetic:test:appearance#prism-001").expect("valid identity"),
        name: Some("Prism-001".into()),
        asset_guid: Some("visual-guid".into()),
        library_id: None,
        visual_guid: Some("visual-guid".into()),
        physical_token: Some("physical-token".into()),
        schema: Some("GenericSchema".into()),
        category: None,
        base_color: Some(crate::topology::Color::new(0.1, 0.2, 0.3, 1.0).expect("valid color")),
        properties: std::collections::BTreeMap::new(),
        textures: vec![TextureRef {
            asset_guid: "texture-guid".into(),
            slot: "generic_bump_map".into(),
            schema: "BumpMapSchema".into(),
            paths: vec!["cloud/resource/texture.png".into()],
            urn: Some("adsk.raas:asset.name:texture".into()),
            mapping: TextureMap2d {
                map_channel: 1,
                uvw_source: 0,
                u_offset: real(0.25),
                v_offset: real(-0.5),
                u_scale: real(2.0),
                v_scale: real(3.0),
                rotation: Angle::new(std::f64::consts::FRAC_PI_2).unwrap(),
                repeat_u: true,
                repeat_v: false,
                real_world_offset_x: length(12.7),
                real_world_offset_y: length(25.4),
                real_world_scale_x: length(304.8),
                real_world_scale_y: length(609.6),
                coordinates: None,
            },
            bump: Some(BumpMap {
                normal_map: true,
                depth: length(2.54),
                normal_scale: real(0.75),
            }),
        }],
    });
    ir.model.appearance_bindings.push(AppearanceBinding {
        native_ref: None,
        id: "synthetic:test:appearance-binding#0"
            .try_into()
            .expect("valid identity"),
        target: AppearanceTarget::Body(body),
        appearance: AppearanceId::mint("synthetic:test:appearance#prism-001")
            .expect("valid identity"),
        source_entity_id: Some("0_1".into()),
        object_type: Some("Body".into()),
        visible: Some(false),
        channels: std::collections::BTreeMap::new(),
    });
    ir.model.appearance_bindings.push(AppearanceBinding {
        native_ref: None,
        id: "synthetic:test:appearance-binding#edge"
            .try_into()
            .expect("valid identity"),
        target: AppearanceTarget::Edge(ir.model.edges[0].id.clone()),
        appearance: AppearanceId::mint("synthetic:test:appearance#prism-001")
            .expect("valid identity"),
        source_entity_id: Some("0_1".into()),
        object_type: Some("Edge".into()),
        visible: None,
        channels: std::collections::BTreeMap::new(),
    });
    ir.model.appearance_bindings.push(AppearanceBinding {
        native_ref: None,
        id: "synthetic:test:appearance-binding#vertex"
            .try_into()
            .expect("valid identity"),
        target: AppearanceTarget::Vertex(ir.model.vertices[0].id.clone()),
        appearance: AppearanceId::mint("synthetic:test:appearance#prism-001")
            .expect("valid identity"),
        source_entity_id: Some("0_1".into()),
        object_type: Some("Vertex".into()),
        visible: None,
        channels: std::collections::BTreeMap::new(),
    });

    let json = ir.to_canonical_json().unwrap();
    let decoded = CadIr::from_json(&json).unwrap();
    assert_eq!(decoded.model.appearances, ir.model.appearances);
    assert_eq!(
        decoded.model.appearance_bindings,
        ir.model.appearance_bindings
    );
}

#[test]
fn a_triangle_group_binding_names_a_group_of_its_tessellation() {
    use crate::appearance::{Appearance, AppearanceBinding, AppearanceTarget};
    use crate::ids::AppearanceId;
    use crate::math::Point3;
    use crate::report::check::Check;
    use crate::tessellation::{Tessellation, TessellationMesh, TessellationTriangleGroup};
    use crate::validate::validate_neutral;

    let mut ir = unit_cube().expect("valid unit cube fixture");
    let mesh = Tessellation::new(
        "synthetic:test:tessellation#0"
            .try_into()
            .expect("valid identity"),
        TessellationMesh::List {
            vertices: vec![
                Point3::new(0.0, 0.0, 0.0),
                Point3::new(1.0, 0.0, 0.0),
                Point3::new(1.0, 1.0, 0.0),
                Point3::new(0.0, 1.0, 0.0),
            ],
            triangles: vec![[0, 1, 2], [0, 2, 3]],
        },
        Vec::new(),
    )
    .unwrap()
    .with_triangle_groups(vec![
        TessellationTriangleGroup {
            source_id: Some("/Mesh/Front".into()),
            triangles: vec![0],
        },
        TessellationTriangleGroup {
            source_id: None,
            triangles: vec![1],
        },
    ])
    .unwrap();
    let tessellation = mesh.id.to_string();
    ir.model.tessellations.push(mesh);
    let appearance = AppearanceId::mint("synthetic:test:appearance#paint").expect("valid identity");
    ir.model.appearances.push(Appearance {
        id: appearance.clone(),
        native_ref: None,
        name: Some("Paint".into()),
        asset_guid: None,
        library_id: None,
        visual_guid: None,
        physical_token: None,
        schema: None,
        category: None,
        base_color: None,
        properties: std::collections::BTreeMap::new(),
        textures: Vec::new(),
    });
    let binding = |group: &str| AppearanceBinding {
        id: "synthetic:test:appearance-binding#group"
            .try_into()
            .expect("valid identity"),
        native_ref: None,
        target: AppearanceTarget::TriangleGroup {
            tessellation: tessellation.clone(),
            group: group.to_owned(),
        },
        appearance: appearance.clone(),
        source_entity_id: None,
        object_type: None,
        visible: None,
        channels: std::collections::BTreeMap::new(),
    };
    ir.model.appearance_bindings.push(binding("/Mesh/Front"));
    let json = ir.to_canonical_json().unwrap();
    let decoded = CadIr::from_json(&json).unwrap();
    assert_eq!(
        decoded.model.appearance_bindings,
        ir.model.appearance_bindings
    );
    let report = validate_neutral(&ir, Vec::new()).expect("validate");
    assert!(
        !report
            .findings
            .iter()
            .any(|finding| finding.check == Check::ReferentialIntegrity),
        "{:?}",
        report.findings
    );
    ir.model.appearance_bindings[0] = binding("/Mesh/Back");
    let report = validate_neutral(&ir, Vec::new()).expect("validate");
    assert!(report
        .findings
        .iter()
        .any(|finding| finding.check == Check::ReferentialIntegrity));
}
