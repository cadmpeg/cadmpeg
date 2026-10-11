// SPDX-License-Identifier: Apache-2.0
//! GUI paths whose admission depends on a selected consumer.

#[test]
fn gui_color_list_validation_checks_layout_without_materializing_values() {
    let count = 256_usize;
    let encoded_count = u32::try_from(count).expect("test count fits u32");
    let mut bytes = encoded_count.to_le_bytes().to_vec();
    bytes.extend(std::iter::repeat_n(
        0_u8,
        count
            .checked_mul(4)
            .expect("test payload length fits usize"),
    ));
    let entries = std::collections::BTreeMap::from([(
        "colors.bin".to_owned(),
        cadmpeg_core::decode::View::over_retained(&bytes),
    )]);
    let property = crate::native::GuiPropertyRecord {
        id: "fcstd:gui:property#Colors".into(),
        owner: "fcstd:gui:view-provider#Provider".into(),
        name: "Colors".into(),
        type_name: "App::PropertyColorList".into(),
        status: None,
        order: 0,
        values: Vec::new(),
        side_entries: vec!["colors.bin".into()],
        xml: crate::native::RetainedXml::from_text("<Property/>".into(), 0)
            .expect("valid property XML"),
    };
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let mut policy = cadmpeg_core::decode::DecodePolicy::service();
    policy.limits.max_materialized_bytes = 0;
    let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(&[], &arena, &policy)
        .expect("empty root is within policy");
    let properties = [property];
    let material_lists =
        super::super::validate_gui_list_payloads(&ctx, &properties, &entries, false)
            .expect("color-list layout needs no materialized values");
    assert!(material_lists.is_empty());
}

#[test]
fn presentation_without_providers_skips_the_property_owner_index() {
    let graph = super::super::Graph {
        properties: vec![crate::native::GuiPropertyRecord {
            id: "fcstd:gui:property#Unused".into(),
            owner: "fcstd:gui:view-provider#Missing".into(),
            name: "Unused".into(),
            type_name: "App::PropertyString".into(),
            status: None,
            order: 0,
            values: Vec::new(),
            side_entries: Vec::new(),
            xml: crate::native::RetainedXml::from_text("<Property/>".into(), 0)
                .expect("valid property XML"),
        }],
        ..Default::default()
    };
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let mut policy = cadmpeg_core::decode::DecodePolicy::service();
    policy.limits.max_collection_items = 0;
    let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(&[], &arena, &policy)
        .expect("empty root is within policy");
    let mut plan = super::super::AppearancePlan::new(&ctx).expect("plan storage");
    let mut losses = Vec::new();
    super::super::transfer_neutral_presentation(&ctx, &mut plan, &graph, None, &mut losses)
        .expect("no provider consumes the property owner index");
}
