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
    let prefix_sets = [
        vec!["a:nested:".into(), "b:".into(), "a:".into(), "b:".into()],
        vec!["a:".into(), "b:".into()],
    ];
    crate::test_support::with_service_context(&[], |ctx| {
        for style in [
            PrimitiveStyle::Line(PrimitiveSize::Absent),
            PrimitiveStyle::Point(PrimitiveSize::Absent),
        ] {
            for prefixes in &prefix_sets {
                let index =
                    PrimitiveIndex::new(ctx, &ir, style, prefixes).expect("primitive index");
                let requested: std::collections::BTreeSet<_> =
                    prefixes.iter().map(String::as_str).collect();
                let indexed: std::collections::BTreeSet<_> =
                    index.by_prefix.keys().copied().collect();
                assert_eq!(indexed, requested);
                assert!(!index.by_prefix.contains_key(""));
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
                        payload_prefixes: prefixes.as_slice(),
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
fn primitive_prefix_index_adds_later_provider_prefixes_in_arena_order() {
    let ir = topology();
    let initial = [String::from("a:")];
    let later = [
        String::from("b:"),
        String::from("a:nested:"),
        String::new(),
        String::from("unused:"),
        String::from("b:"),
    ];
    crate::test_support::with_service_context(&[], |ctx| {
        for style in [
            PrimitiveStyle::Line(PrimitiveSize::Absent),
            PrimitiveStyle::Point(PrimitiveSize::Absent),
        ] {
            let mut index =
                PrimitiveIndex::new(ctx, &ir, style, &initial).expect("initial provider prefixes");
            assert_eq!(
                index.by_prefix.keys().copied().collect::<Vec<_>>(),
                vec!["a:"]
            );
            let candidate_count = index.candidates.len();

            index
                .add_prefixes(ctx, &ir, style, &later)
                .expect("later provider prefixes");
            assert_eq!(index.candidates.len(), candidate_count);
            assert_eq!(
                index.by_prefix.keys().copied().collect::<Vec<_>>(),
                vec!["", "a:", "a:nested:", "b:"]
            );
            let mut plan = AppearancePlan::new(ctx).expect("plan storage");
            let prefixes = [initial.as_slice(), later.as_slice()].concat();
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
            let expected: Vec<_> = ["b:v0", "a:nested:v1", "unowned:v2", "a:v3", "b:v0"]
                .iter()
                .map(|key| format!("fcstd:model:{kind}#{key}"))
                .collect();
            assert_eq!(keys, expected);
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
    let prefixes = [String::from("b:")];
    PrimitiveIndex::new(
        &ctx,
        &ir,
        PrimitiveStyle::Line(PrimitiveSize::Absent),
        &prefixes,
    )
    .expect("borrowed index has no retained allocation");
}

#[test]
fn primitive_prefix_index_admits_lookup_and_storage() {
    let ir = topology();
    let prefixes = [String::from("a:")];
    for dimension in [
        ResourceDimension::WorkUnits,
        ResourceDimension::CollectionItems,
        ResourceDimension::MaterializedBytes,
    ] {
        crate::test_support::refusal_at(dimension, &[], "FCStd GUI primitive index", |ctx| {
            PrimitiveIndex::new(
                ctx,
                &ir,
                PrimitiveStyle::Line(PrimitiveSize::Absent),
                &prefixes,
            )
            .map(drop)
        });
    }
    crate::test_support::refusal_at(
        ResourceDimension::WorkUnits,
        &[],
        "FCStd GUI primitive prefix lookup",
        |ctx| {
            let style = PrimitiveStyle::Line(PrimitiveSize::Absent);
            let index = PrimitiveIndex::new(ctx, &ir, style, &prefixes)?;
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
                    payload_prefixes: &prefixes,
                    provenance: SourceProvenance::in_stream(
                        "fcstd",
                        cadmpeg_ir::stream_name!("GuiDocument.xml"),
                        17,
                    ),
                },
            )
        },
    );
    crate::test_support::refusal_at(
        ResourceDimension::WorkUnits,
        &[],
        "FCStd GUI primitive prefix lower bound",
        |ctx| {
            let style = PrimitiveStyle::Line(PrimitiveSize::Absent);
            let mut index = PrimitiveIndex::new(ctx, &ir, style, &[])?;
            let prefixes = [String::from("a:")];
            index.add_prefixes(ctx, &ir, style, &prefixes)
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

#[test]
fn zero_provider_transfer_does_not_visit_source_arenas() {
    let empty_ir = CadIr::empty();
    let mut populated_ir = CadIr::empty();
    for index in 0..64 {
        let key = format!("source-{index}");
        let vertex = cadmpeg_ir::ids::VertexId::mint(format!("fcstd:model:vertex#{key}"))
            .expect("vertex identity");
        populated_ir
            .model
            .vertices
            .push(cadmpeg_ir::topology::Vertex {
                id: vertex.clone(),
                point: cadmpeg_ir::ids::PointId::mint(format!("fcstd:model:point#{key}"))
                    .expect("point identity"),
                tolerance: None,
            });
        populated_ir.model.edges.push(cadmpeg_ir::topology::Edge {
            id: cadmpeg_ir::ids::EdgeId::mint(format!("fcstd:model:edge#{key}"))
                .expect("edge identity"),
            carrier: cadmpeg_ir::topology::EdgeCarrier::unbounded(None),
            start: vertex.clone(),
            end: vertex,
            tolerance: None,
        });
        populated_ir.model.bodies.push(cadmpeg_ir::topology::Body {
            id: cadmpeg_ir::ids::BodyId::mint(format!("fcstd:model:body#{key}:1"))
                .expect("body identity"),
            kind: cadmpeg_ir::topology::BodyKind::default(),
            regions: Vec::new(),
            transform: None,
            name: None,
            color: None,
            visible: None,
        });
    }
    let objects = (0..64)
        .map(|index| {
            let name = format!("Model{index}");
            crate::native::ObjectRecord {
                identity: crate::native::object_identity::ObjectIdentity::try_new(
                    format!("fcstd:native:object#{name}"),
                    name,
                )
                .expect("object identity"),
                type_name: "Part::Feature".into(),
                persistent_id: None,
                view_type: None,
                attributes: std::collections::BTreeMap::new(),
                dependencies: Vec::new(),
                dependency_allow_partial: None,
                order: index,
                data: None,
            }
        })
        .collect::<Vec<_>>();
    let payloads = (0..64)
        .map(|index| crate::brep::ShapePayloadRecord {
            id: format!("fcstd:native:shape-payload#Model{index}:Shape"),
            property: format!("fcstd:native:property#Model{index}:Shape"),
            entry: format!("Model{index}.brp"),
            payload: crate::brep::ShapePayload::Empty,
        })
        .collect::<Vec<_>>();
    let entries = std::collections::BTreeMap::new();
    let empty_sources = super::super::GuiSources {
        entries: &entries,
        objects: &[],
        properties: &[],
        payloads: &[],
        element_maps: &[],
        requires_alpha_conversion: false,
    };
    let populated_sources = super::super::GuiSources {
        entries: &entries,
        objects: &objects,
        properties: &[],
        payloads: &payloads,
        element_maps: &[],
        requires_alpha_conversion: false,
    };
    let text = r#"<Document SchemaVersion="1"><ViewProviderData Count="0"/><Camera settings=""/></Document>"#;
    let xml = roxmltree::Document::parse(text).expect("GUI XML");
    let mut source_work = None;
    for (ir, sources) in [
        (&empty_ir, &empty_sources),
        (&populated_ir, &populated_sources),
    ] {
        let error = crate::test_support::refusal_at(
            ResourceDimension::WorkUnits,
            text.as_bytes(),
            "FCStd GUI document records",
            |ctx| {
                super::super::transfer_schema_one(ctx, ir, text, &xml, Some("1"), Some(1), sources)
                    .map(|_| ())
            },
        );
        let cadmpeg_core::CodecError::ResourceLimit(limit) = error else {
            panic!("document record refusal");
        };
        if let Some(expected) = source_work {
            assert_eq!(limit.used, expected, "zero providers skip source indexes");
        } else {
            source_work = Some(limit.used);
        }
    }
    crate::test_support::with_service_context(text.as_bytes(), |ctx| {
        let (graph, plan) = super::super::transfer_schema_one(
            ctx,
            &populated_ir,
            text,
            &xml,
            Some("1"),
            Some(1),
            &populated_sources,
        )
        .expect("zero-provider GUI transfer");
        assert_eq!(graph.documents.len(), 1);
        assert!(graph.providers.is_empty());
        assert_eq!(plan.presentation_documents.len(), 1);
    });
}
