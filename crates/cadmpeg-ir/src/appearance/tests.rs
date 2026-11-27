// SPDX-License-Identifier: Apache-2.0
#![allow(clippy::unwrap_used)]

use crate::examples::unit_cube;
use crate::CadIr;
use cadmpeg_core::serde_erase::json;

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
    let body = ir.geometry.bodies[0].id.clone();
    ir.model.appearances.push(Appearance {
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
        native_ref: None,
    });
    ir.model.appearance_bindings.push(AppearanceBinding {
        id: "synthetic:test:appearance-binding#0"
            .try_into()
            .expect("valid identity"),
        target: AppearanceTarget::Body(body).into(),
        appearance: AppearanceId::mint("synthetic:test:appearance#prism-001")
            .expect("valid identity"),
        precedence: None,
        source_entity_id: Some("0_1".into()),
        object_type: Some("Body".into()),
        visible: Some(false),
        channels: std::collections::BTreeMap::new(),
        native_ref: None,
    });
    ir.model.appearance_bindings.push(AppearanceBinding {
        id: "synthetic:test:appearance-binding#edge"
            .try_into()
            .expect("valid identity"),
        target: AppearanceTarget::Edge(ir.geometry.edges[0].id.clone()).into(),
        appearance: AppearanceId::mint("synthetic:test:appearance#prism-001")
            .expect("valid identity"),
        precedence: None,
        source_entity_id: Some("0_1".into()),
        object_type: Some("Edge".into()),
        visible: None,
        channels: std::collections::BTreeMap::new(),
        native_ref: None,
    });
    ir.model.appearance_bindings.push(AppearanceBinding {
        id: "synthetic:test:appearance-binding#vertex"
            .try_into()
            .expect("valid identity"),
        target: AppearanceTarget::Vertex(ir.geometry.vertices[0].id.clone()).into(),
        appearance: AppearanceId::mint("synthetic:test:appearance#prism-001")
            .expect("valid identity"),
        precedence: None,
        source_entity_id: Some("0_1".into()),
        object_type: Some("Vertex".into()),
        visible: None,
        channels: std::collections::BTreeMap::new(),
        native_ref: None,
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
        TessellationTriangleGroup::new(Some("/Mesh/Front".into()), vec![0]),
        TessellationTriangleGroup::new(None, vec![1]),
    ])
    .unwrap();
    let tessellation = mesh.id.clone();
    ir.model.tessellations.push(mesh);
    let appearance = AppearanceId::mint("synthetic:test:appearance#paint").expect("valid identity");
    ir.model.appearances.push(Appearance {
        id: appearance.clone(),
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
        native_ref: None,
    });
    let binding = |group: &str| AppearanceBinding {
        id: "synthetic:test:appearance-binding#group"
            .try_into()
            .expect("valid identity"),
        target: AppearanceTarget::TriangleGroup {
            tessellation: tessellation.clone(),
            group: group.to_owned(),
        }
        .into(),
        appearance: appearance.clone(),
        precedence: None,
        source_entity_id: None,
        object_type: None,
        visible: None,
        channels: std::collections::BTreeMap::new(),
        native_ref: None,
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

#[test]
fn occurrence_and_record_targets_and_native_refs_read_back() {
    use crate::appearance::{Appearance, AppearanceBinding, AppearanceTarget};
    use cadmpeg_core::decode::ResourceLimits;
    use cadmpeg_core::json;
    use serde_json::Value;

    let read = |wire: &Value| {
        crate::design::read::from_str::<AppearanceBinding>(
            &wire.to_string(),
            &ResourceLimits::desktop(),
        )
    };
    for target in [
        json!({"kind": "occurrence", "value": "test:model:occurrence#1"}),
        json!({"kind": "record", "value": {"arena": "point_clouds", "id": "test:model:cloud#1"}}),
    ] {
        let wire = json!({
            "id": "test:model:appearance-binding#1",
            "target": {"entity": target},
            "appearance": "test:model:appearance#1",
            "channels": {},
            "native_ref": "test:native:record#1"
        });
        let binding = read(&wire).unwrap();
        assert!(matches!(
            binding.target.entity,
            AppearanceTarget::Occurrence(_) | AppearanceTarget::Record { .. }
        ));
        assert_eq!(json::to_value(&binding).unwrap(), wire);
        let mut null_ref = wire.clone();
        null_ref["native_ref"] = Value::Null;
        assert!(read(&null_ref).is_err());
    }
    assert!(read(&json!({
        "id": "test:model:appearance-binding#1",
        "target": {"entity": {"kind": "record", "value": {"arena": "", "id": "x"}}},
        "appearance": "test:model:appearance#1",
        "channels": {}
    }))
    .is_err());
    let appearance = json!({
        "id": "test:model:appearance#1", "properties": {}, "textures": [],
        "native_ref": "test:native:record#1"
    });
    let read_appearance = |wire: &Value| {
        crate::design::read::from_str::<Appearance>(&wire.to_string(), &ResourceLimits::desktop())
    };
    let read_back = read_appearance(&appearance).unwrap();
    assert_eq!(json::to_value(&read_back).unwrap(), appearance);
    let mut null_ref = appearance.clone();
    null_ref["native_ref"] = Value::Null;
    assert!(read_appearance(&null_ref).is_err());
}

#[test]
fn a_binding_states_its_precedence_and_a_definition_target_by_name() {
    use super::{AppearanceBinding, AppearancePrecedence, AppearanceTarget};
    let wire = cadmpeg_core::json!({
        "id": "test:model:appearance-binding#1",
        "target": {"entity": {"kind": "definition", "value": "test:model:product#assembly"}},
        "appearance": "test:model:appearance#1",
        "precedence": "outer",
        "channels": {},
    });
    let binding: AppearanceBinding = json::from_value(wire.clone()).unwrap();
    assert_eq!(binding.precedence, Some(AppearancePrecedence::Outer));
    assert_eq!(
        binding.target.entity,
        AppearanceTarget::Definition("test:model:product#assembly".try_into().unwrap())
    );
    assert_eq!(json::to_value(&binding).unwrap(), wire);

    let mut nearest = wire.clone();
    nearest["precedence"] = cadmpeg_core::json!("nearest");
    let binding: AppearanceBinding = json::from_value(nearest).unwrap();
    assert_eq!(binding.precedence, Some(AppearancePrecedence::Nearest));

    let mut unstated = wire.clone();
    unstated.as_object_mut().unwrap().remove("precedence");
    let binding: AppearanceBinding = json::from_value(unstated.clone()).unwrap();
    assert_eq!(binding.precedence, None);
    assert_eq!(json::to_value(&binding).unwrap(), unstated);

    for refused in [serde_json::Value::Null, cadmpeg_core::json!("closest")] {
        let mut wire = wire.clone();
        wire["precedence"] = refused;
        assert!(json::from_value::<AppearanceBinding>(wire).is_err());
    }
}

#[test]
fn an_arrangement_appearance_states_its_precedence_with_its_appearance() {
    use crate::design::configuration::ArrangementAppearance;
    let path = cadmpeg_core::json!([{"occurrence": {"kind": "occurrence", "value": "test:model:occurrence#0"}}]);
    let wire = cadmpeg_core::json!({
        "path": path,
        "appearance": {"appearance": "test:model:appearance#1", "precedence": "outer"},
    });
    let read: ArrangementAppearance = json::from_value(wire.clone()).unwrap();
    assert_eq!(
        read.appearance
            .as_ref()
            .map(|appearance| appearance.precedence),
        Some(super::AppearancePrecedence::Outer)
    );
    assert_eq!(json::to_value(&read).unwrap(), wire);

    let transparency_only = cadmpeg_core::json!({"path": path, "transparency": 0.5});
    let read: ArrangementAppearance = json::from_value(transparency_only).unwrap();
    assert!(read.appearance.is_none());

    let bare = cadmpeg_core::json!({"path": path, "appearance": "test:model:appearance#1"});
    assert!(json::from_value::<ArrangementAppearance>(bare).is_err());
    let unordered = cadmpeg_core::json!({"path": path, "appearance": {"appearance": "test:model:appearance#1"}});
    assert!(json::from_value::<ArrangementAppearance>(unordered).is_err());
}
