// SPDX-License-Identifier: Apache-2.0

use cadmpeg_core::decode::DecodeContext;
use cadmpeg_core::text::NonBlankString;
use cadmpeg_ir::features::{
    FeatureDefinition, FeatureOperation, FeatureResultMembers, FeatureResultTopology,
};
use cadmpeg_ir::ids::FeatureResultTopologyId;

use super::super::composite::{evaluate, valid};
use super::super::NativeData;
use crate::feature::composite::tests::{fixture, with_context, Fixture};

fn record(ordinal: u32) -> crate::native::RseRecordRecord {
    serde_json::from_value(serde_json::json!({
        "id": "test:record",
        "token": "dc",
        "ordinal": ordinal,
        "selector": 0,
        "type_index": 0,
        "type_id": crate::pmdc::type_id_string(crate::kernel::KERNEL_RECORD_TYPE_ID),
        "payload_offset": 0,
        "payload_len": 0,
        "payload_sha256": "",
        "trailing_payload_len": 0,
        "trailer_len": 0,
        "trailer_sha256": ""
    }))
    .expect("record fixture")
}

fn data(fixture: &Fixture) -> NativeData {
    let mut data = super::empty_data();
    data.pm_dc_feature_labels = vec![fixture.label.clone()];
    data.pm_dc_feature_properties = fixture.properties.clone();
    data.pm_dc_kernel_entity_links = fixture.links.clone();
    data.composite_member_links = fixture.members.clone();
    data.body_native_keys = fixture.keys.clone();
    data.active_carrier = fixture.carrier.clone();
    data.metadata = fixture.metadata.clone();
    data.pm_dc_model_roots = fixture.roots.clone();
    data.records = vec![record(crate::feature::composite::tests::STORED_CARRIER)];
    data
}

fn projected(fixture: &Fixture, ctx: &DecodeContext<'_>) -> cadmpeg_ir::CadIr {
    let mut ir = cadmpeg_ir::CadIr::empty();
    ir.model = fixture.model(ctx);
    fixture.project(ctx, &mut ir.model);
    ir
}

fn topology(
    ir: &cadmpeg_ir::CadIr,
    links: Vec<String>,
    native: Option<String>,
) -> FeatureResultTopology {
    FeatureResultTopology::new(
        FeatureResultTopologyId::mint("test:model:feature-result#composite").expect("id"),
        ir.model.features[0].id.clone(),
        FeatureResultMembers::new(
            links
                .into_iter()
                .map(|link| NonBlankString::new(link).expect("link id"))
                .collect(),
            Vec::new(),
            Vec::new(),
            Vec::new(),
        )
        .expect("members"),
        native,
    )
}

#[test]
fn composite_validation_accepts_the_decoder_projection_and_its_outputs() {
    with_context(|ctx| {
        let fixture = fixture();
        let ir = projected(&fixture, ctx);
        let data = data(&fixture);
        assert!(valid(&ir, &data, &ir.model.features[0], &fixture.feature));
        let (_, bodies) = evaluate(&ir, &data, &fixture.feature).expect("join");
        let joined = bodies.into_iter().collect();
        let mut with_outputs = ir.model.clone();
        crate::feature::body_selection::project(&mut with_outputs, &joined, &fixture.inventory());
        assert_eq!(with_outputs.features[0].evaluation.outputs().len(), 2);
        assert!(crate::feature::body_selection::outputs_match(
            &with_outputs.features[0],
            &with_outputs.feature_result_topologies,
            &joined
        ));
        assert!(!crate::feature::body_selection::outputs_match(
            &ir.model.features[0],
            &ir.model.feature_result_topologies,
            &joined
        ));
    });
}

#[test]
fn composite_validation_rejects_mismatched_extra_and_missing_results() {
    with_context(|ctx| {
        let fixture = fixture();
        let data = data(&fixture);
        let links = [fixture.links[2].id(), fixture.links[3].id()];
        let native = Some(fixture.properties[1].id());
        let bad: [(Vec<String>, Option<String>); 4] = [
            (vec![links[1].clone(), links[0].clone()], native.clone()),
            (vec![links[0].clone()], native.clone()),
            (links.to_vec(), Some(fixture.properties[0].id())),
            (links.to_vec(), None),
        ];
        for (members, native) in bad {
            let mut ir = projected(&fixture, ctx);
            ir.model.feature_result_topologies = vec![topology(&ir, members, native)];
            assert!(!valid(&ir, &data, &ir.model.features[0], &fixture.feature));
        }
        let mut ir = projected(&fixture, ctx);
        let extra = ir.model.feature_result_topologies[0].clone();
        ir.model.feature_result_topologies.push(extra);
        assert!(!valid(&ir, &data, &ir.model.features[0], &fixture.feature));
        let mut ir = projected(&fixture, ctx);
        ir.model.feature_result_topologies.clear();
        assert!(!valid(&ir, &data, &ir.model.features[0], &fixture.feature));
    });
}

#[test]
fn composite_validation_rejects_wrong_or_missing_source_properties() {
    with_context(|ctx| {
        let fixture = fixture();
        let data = data(&fixture);
        for key in ["source_body_set", "result_body_set", "control"] {
            let mut ir = projected(&fixture, ctx);
            ir.model.features[0].source_properties.remove(key);
            assert!(!valid(&ir, &data, &ir.model.features[0], &fixture.feature));
            let mut ir = projected(&fixture, ctx);
            ir.model.features[0]
                .source_properties
                .insert(NonBlankString::new(key).expect("key"), "other".into());
            assert!(!valid(&ir, &data, &ir.model.features[0], &fixture.feature));
        }
        let mut ir = projected(&fixture, ctx);
        ir.model.features[0]
            .source_properties
            .insert(NonBlankString::new("extra").expect("key"), "x".into());
        assert!(!valid(&ir, &data, &ir.model.features[0], &fixture.feature));
        let mut ir = projected(&fixture, ctx);
        ir.model.features[0].source_tag = Some("base".into());
        assert!(!valid(&ir, &data, &ir.model.features[0], &fixture.feature));
    });
}

#[test]
fn composite_validation_requires_stored_geometry_exactly_when_the_join_resolves() {
    with_context(|ctx| {
        let fixture = fixture();
        let data = data(&fixture);
        let native = fixture.model(ctx);
        let mut ir = cadmpeg_ir::CadIr::empty();
        ir.model = native;
        assert!(
            !valid(&ir, &data, &ir.model.features[0], &fixture.feature),
            "a resolved join needs stored geometry"
        );
        let projected_ir = projected(&fixture, ctx);
        let mut refused = self::data(&fixture);
        refused.pm_dc_kernel_entity_links[2].edit(|link| link.state = 0);
        assert!(
            !valid(
                &projected_ir,
                &refused,
                &projected_ir.model.features[0],
                &fixture.feature
            ),
            "an unresolved join needs the native definition"
        );
        let mut broken = fixture;
        broken.links[2].edit(|link| link.state = 0);
        let broken_data = self::data(&broken);
        let mut native_ir = cadmpeg_ir::CadIr::empty();
        native_ir.model = broken.model(ctx);
        assert!(valid(
            &native_ir,
            &broken_data,
            &native_ir.model.features[0],
            &broken.feature
        ));
        native_ir.model.features[0]
            .evaluation
            .set_definition(FeatureDefinition::Operation(
                FeatureOperation::StoredGeometry {},
            ));
        assert!(!valid(
            &native_ir,
            &broken_data,
            &native_ir.model.features[0],
            &broken.feature
        ));
    });
}

#[test]
fn composite_validation_rejects_a_native_composite_that_keeps_a_result() {
    with_context(|ctx| {
        let mut broken = fixture();
        broken.links[2].edit(|link| link.state = 0);
        let data = data(&broken);
        let mut ir = cadmpeg_ir::CadIr::empty();
        ir.model = broken.model(ctx);
        let links = vec![broken.links[2].id(), broken.links[3].id()];
        ir.model.feature_result_topologies = vec![topology(&ir, links, None)];
        assert!(!valid(&ir, &data, &ir.model.features[0], &broken.feature));
        ir.model.feature_result_topologies.clear();
        ir.model.features[0]
            .source_properties
            .insert(NonBlankString::new("control").expect("key"), "x".into());
        assert!(!valid(&ir, &data, &ir.model.features[0], &broken.feature));
    });
}
