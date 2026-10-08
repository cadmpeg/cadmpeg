// SPDX-License-Identifier: Apache-2.0
//! Primitive prefix selection order, overlap and index admission.

use super::super::{
    AppearancePlan, PrimitiveAppearanceSource, PrimitiveIndex, PrimitiveSize, PrimitiveStyle,
};
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
use cadmpeg_ir::appearance::AppearanceTarget;
use cadmpeg_ir::ids::{EdgeId, PointId, VertexId};
use cadmpeg_ir::topology::{Edge, EdgeCarrier, Vertex};
use cadmpeg_ir::{CadIr, SourceProvenance};

fn topology() -> CadIr {
    let mut ir = CadIr::empty();
    for key in ["b:v0", "a:nested:v1", "unowned:v2", "a:v3", "b:v0"] {
        let id = VertexId::mint(format!("fcstd:model:vertex#{key}")).expect("vertex id");
        ir.model.vertices.push(Vertex {
            id: id.clone(),
            point: PointId::mint(format!("fcstd:model:point#{key}")).expect("point id"),
            tolerance: None,
        });
        ir.model.edges.push(Edge {
            id: EdgeId::mint(format!("fcstd:model:edge#{key}")).expect("edge id"),
            carrier: EdgeCarrier::unbounded(None),
            start: id.clone(),
            end: id,
            tolerance: None,
        });
    }
    ir
}

#[test]
fn primitive_prefix_index_keeps_arena_order_and_deduplicates_overlap() {
    let ir = topology();
    crate::test_support::with_service_context(&[], |ctx| {
        for style in [
            PrimitiveStyle::Line(PrimitiveSize::Absent),
            PrimitiveStyle::Point(PrimitiveSize::Absent),
        ] {
            let index = PrimitiveIndex::new(ctx, &ir, style).expect("primitive index");
            for prefixes in [
                vec!["a:nested:".into(), "b:".into(), "a:".into(), "b:".into()],
                vec!["a:".into(), "b:".into()],
            ] {
                let mut plan = AppearancePlan::new(ctx).expect("plan storage");
                super::super::transfer_primitive_appearance(
                    ctx,
                    &index,
                    &mut plan,
                    &mut Vec::new(),
                    PrimitiveAppearanceSource {
                        provider_name: "Model",
                        object_id: "fcstd:native:object#Model",
                        packed_color: 0x1122_3344,
                        style,
                        payload_prefixes: &prefixes,
                        provenance: SourceProvenance::in_stream(
                            "fcstd",
                            cadmpeg_ir::stream_name!("GuiDocument.xml"),
                            17,
                        ),
                    },
                )
                .expect("primitive transfer");
                let keys: Vec<_> = plan
                    .bindings
                    .iter()
                    .map(|binding| match &binding.target {
                        AppearanceTarget::Edge(id) => id.as_str(),
                        AppearanceTarget::Vertex(id) => id.as_str(),
                        target => panic!("unexpected primitive target: {target:?}"),
                    })
                    .collect();
                let kind = match style {
                    PrimitiveStyle::Line(_) => "edge",
                    PrimitiveStyle::Point(_) => "vertex",
                };
                let expected: Vec<_> = ["b:v0", "a:nested:v1", "a:v3", "b:v0"]
                    .iter()
                    .map(|key| format!("fcstd:model:{kind}#{key}"))
                    .collect();
                assert_eq!(keys, expected);
                assert_eq!(plan.appearances.len(), 1);
                for (ordinal, binding) in plan.bindings.iter().enumerate() {
                    assert_eq!(
                        binding.id.as_str(),
                        format!("fcstd:appearance:binding#{kind}:Model:{ordinal}")
                    );
                }
            }
        }
    });
}

#[test]
fn primitive_prefix_index_storage_is_scoped_and_borrowed() {
    let ir = topology();
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_retained_bytes = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("empty input");
    PrimitiveIndex::new(&ctx, &ir, PrimitiveStyle::Line(PrimitiveSize::Absent))
        .expect("borrowed index has no retained allocation");
}

#[test]
fn primitive_prefix_index_admits_lookup_and_storage() {
    let ir = topology();
    for dimension in [
        ResourceDimension::WorkUnits,
        ResourceDimension::CollectionItems,
        ResourceDimension::MaterializedBytes,
    ] {
        crate::test_support::refusal_at(dimension, &[], "FCStd GUI primitive index", |ctx| {
            PrimitiveIndex::new(ctx, &ir, PrimitiveStyle::Line(PrimitiveSize::Absent)).map(drop)
        });
    }
    crate::test_support::refusal_at(
        ResourceDimension::WorkUnits,
        &[],
        "FCStd GUI primitive prefix lookup",
        |ctx| {
            let style = PrimitiveStyle::Line(PrimitiveSize::Absent);
            let index = PrimitiveIndex::new(ctx, &ir, style)?;
            super::super::transfer_primitive_appearance(
                ctx,
                &index,
                &mut AppearancePlan::new(ctx)?,
                &mut Vec::new(),
                PrimitiveAppearanceSource {
                    provider_name: "Model",
                    object_id: "fcstd:native:object#Model",
                    packed_color: 0x1122_3344,
                    style,
                    payload_prefixes: &["a:".into()],
                    provenance: SourceProvenance::in_stream(
                        "fcstd",
                        cadmpeg_ir::stream_name!("GuiDocument.xml"),
                        17,
                    ),
                },
            )
        },
    );
}

#[test]
fn providers_without_payloads_add_no_primitive_arena_work() {
    let source = topology();
    let mut larger = topology();
    larger.model.edges.extend(source.model.edges.clone());
    larger.model.vertices.extend(source.model.vertices.clone());
    let object = crate::native::ObjectRecord {
        identity: crate::native::object_identity::ObjectIdentity::try_new(
            "fcstd:native:object#P".into(),
            "P".into(),
        )
        .expect("object identity"),
        type_name: "Part::Feature".into(),
        persistent_id: None,
        view_type: None,
        attributes: std::collections::BTreeMap::new(),
        dependencies: Vec::new(),
        dependency_allow_partial: None,
        order: 0,
        data: None,
    };
    let texts = [
        r#"<Document><Camera settings=""/><ViewProviderData Count="1"><ViewProvider name="P"><Properties Count="0"/></ViewProvider></ViewProviderData></Document>"#,
        r#"<Document><Camera settings=""/><ViewProviderData Count="1"><ViewProvider name="P"><Properties Count="4">
<Property name="LineColor" type="App::PropertyColor"><PropertyColor value="287454020"/></Property>
<Property name="PointColor" type="App::PropertyColor"><PropertyColor value="287454020"/></Property>
<Property name="LineWidth" type="App::PropertyFloatConstraint"><Float value="2"/></Property>
<Property name="PointSize" type="App::PropertyFloatConstraint"><Float value="3"/></Property>
</Properties></ViewProvider></ViewProviderData></Document>"#,
    ];
    let entries = std::collections::BTreeMap::new();
    let sources = super::super::GuiSources {
        entries: &entries,
        objects: std::slice::from_ref(&object),
        properties: &[],
        payloads: &[],
        element_maps: &[],
        requires_alpha_conversion: false,
    };
    let mut first_added_work = None;
    for ir in [source, larger] {
        let mut used = [0; 2];
        for (index, text) in texts.iter().enumerate() {
            let xml = roxmltree::Document::parse(text).expect("GUI XML");
            let error = crate::test_support::refusal_at(
                ResourceDimension::WorkUnits,
                text.as_bytes(),
                "FCStd presentation provider sources",
                |ctx| {
                    super::super::transfer_schema_one(ctx, &ir, text, &xml, None, None, &sources)
                        .map(|_| ())
                },
            );
            let cadmpeg_core::CodecError::ResourceLimit(limit) = error else {
                panic!("work refusal");
            };
            used[index] = limit.used;
            if index == 1 {
                crate::test_support::with_service_context(text.as_bytes(), |ctx| {
                    let (graph, plan) = super::super::transfer_schema_one(
                        ctx, &ir, text, &xml, None, None, &sources,
                    )
                    .expect("provider without shape payloads");
                    assert_eq!(graph.providers.len(), 1);
                    assert_eq!(graph.properties.len(), 4);
                    assert!(plan.bindings.is_empty());
                    assert_eq!(plan.view_presentations.len(), 1);
                    let view = &plan.view_presentations[0];
                    assert_eq!(
                        view.line_width
                            .map(cadmpeg_ir::scalar::NonNegativeReal::get),
                        Some(2.0)
                    );
                    assert_eq!(
                        view.point_size
                            .map(cadmpeg_ir::scalar::NonNegativeReal::get),
                        Some(3.0)
                    );
                });
            }
        }
        let added_work = used[1].checked_sub(used[0]).expect("colored property work");
        assert!(added_work > 0);
        if let Some(expected) = first_added_work {
            assert_eq!(
                added_work, expected,
                "primitive arena growth adds no color work"
            );
        } else {
            first_added_work = Some(added_work);
        }
    }
}
