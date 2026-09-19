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

    let mut ir = unit_cube().expect("valid unit cube fixture");
    let body = ir.model.bodies[0].id.clone();
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
                u_offset: 0.25,
                v_offset: -0.5,
                u_scale: 2.0,
                v_scale: 3.0,
                rotation: std::f64::consts::FRAC_PI_2,
                repeat_u: true,
                repeat_v: false,
                real_world_offset_x: 12.7,
                real_world_offset_y: 25.4,
                real_world_scale_x: 304.8,
                real_world_scale_y: 609.6,
            },
            bump: Some(BumpMap {
                normal_map: true,
                depth: 2.54,
                normal_scale: 0.75,
            }),
        }],
    });
    ir.model.appearance_bindings.push(AppearanceBinding {
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

/// `TextureMap2d` carries nine plain public `f64` fields with no refusing
/// constructor, so validation is the one route that states a non-finite
/// mapping value is illegal.
#[test]
fn a_non_finite_texture_mapping_value_is_refused_by_validation() {
    use crate::report::check::Check;
    use crate::validate::validate_neutral;

    let mut ir = CadIr::empty();
    crate::test_support::push_texture_offset(&mut ir, 0.25);
    assert!(validate_neutral(&ir, Vec::new()).is_ok());

    ir.model.appearances[0].textures[0].mapping.u_scale = f64::INFINITY;
    let report = validate_neutral(&ir, Vec::new());
    assert!(!report.is_ok());
    assert!(report
        .findings
        .iter()
        .any(|finding| finding.check == Check::Presentation
            && finding.message == "non-finite texture mapping value"));
}

/// `BumpMap::depth` and `BumpMap::normal_scale` are plain public `f64` fields
/// on the same carrier chain and are refused by the same validation.
#[test]
fn a_non_finite_bump_map_value_is_refused_by_validation() {
    use crate::appearance::BumpMap;
    use crate::report::check::Check;
    use crate::validate::validate_neutral;

    let mut ir = CadIr::empty();
    crate::test_support::push_texture_offset(&mut ir, 0.25);
    ir.model.appearances[0].textures[0].bump = Some(BumpMap {
        normal_map: false,
        depth: 1.0,
        normal_scale: 1.0,
    });
    assert!(validate_neutral(&ir, Vec::new()).is_ok());

    ir.model.appearances[0].textures[0].bump = Some(BumpMap {
        normal_map: false,
        depth: f64::NAN,
        normal_scale: 1.0,
    });
    let report = validate_neutral(&ir, Vec::new());
    assert!(!report.is_ok());
    assert!(report
        .findings
        .iter()
        .any(|finding| finding.check == Check::Presentation
            && finding.message == "non-finite texture bump-map value"));
}
