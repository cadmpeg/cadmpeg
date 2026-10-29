// SPDX-License-Identifier: Apache-2.0

use super::apply;
use crate::brep::{
    ShapePayloadRecord, TextOrientation, TextShapeUse, TextTShape, TextTShapeGeometry, TextTShapes,
};
use crate::native::source_use::{SourceUse, SourceUseSelector};
use crate::native::{PropertyBody, PropertyFamily, PropertyRecord, ValueRecord};
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};
use cadmpeg_core::CodecError;
use cadmpeg_ir::features::FinitePoint3;
use cadmpeg_ir::scalar::FiniteReal;
use cadmpeg_ir::{ids::BodyId, CadIr};
use std::collections::BTreeMap;

fn shape_use(shape: usize, orientation: TextOrientation) -> TextShapeUse {
    TextShapeUse {
        shape,
        location: 0.into(),
        orientation,
    }
}

fn property(owner: &str, name: &str, values: &[&str]) -> PropertyRecord {
    PropertyRecord {
        dynamic: None,
        id: crate::native::native_child_id(
            "property",
            &crate::native::native_id("object", owner),
            name,
        ),
        owner: crate::native::native_id("object", owner),
        name: name.to_owned(),
        type_name: if name == "Shape" {
            "Part::PropertyPartShape"
        } else {
            "App::PropertyStringList"
        }
        .to_owned(),
        family: if name == "Shape" {
            PropertyFamily::Geometry
        } else {
            PropertyFamily::List
        },
        status: None,
        order: 0,
        body: Some(PropertyBody::Persisted {
            values: values
                .iter()
                .enumerate()
                .map(|(order, value)| ValueRecord {
                    tag: "String".to_owned(),
                    parent: None,
                    order,
                    attributes: BTreeMap::from([("value".to_owned(), (*value).to_owned())]),
                    decoded_json: None,
                    text: None,
                })
                .collect(),
            links: Vec::new(),
            side_entries: Vec::new(),
        }),
    }
}

fn payload(owner: &str, roots: usize) -> ShapePayloadRecord {
    let shapes = TextTShapes::from(vec![
        TextTShape {
            geometry: TextTShapeGeometry::Vertex {
                tolerance: FiniteReal::ZERO,
                point: FinitePoint3::ZERO,
                representations: Vec::new(),
            },
            flags: [false; 7],
            children: Vec::new(),
        },
        TextTShape {
            geometry: TextTShapeGeometry::Edge {
                tolerance: FiniteReal::ZERO,
                same_parameter: false,
                same_range: false,
                degenerated: true,
                representations: Vec::new(),
            },
            flags: [false; 7],
            children: vec![
                shape_use(1, TextOrientation::Forward),
                shape_use(1, TextOrientation::Reversed),
            ],
        },
        TextTShape {
            geometry: TextTShapeGeometry::Wire,
            flags: [false; 7],
            children: vec![shape_use(2, TextOrientation::Forward)],
        },
    ]);
    let property = crate::native::native_child_id(
        "property",
        &crate::native::native_id("object", owner),
        "Shape",
    );
    let entry = format!("{owner}.Shape.brp");
    serde_json::from_value(serde_json::json!({
        "id": crate::native::native_child_id("shape-payload", &property, &entry), "property": property,
        "entry": crate::native::native_id("entry", &entry), "form": "binary", "text": null,
        "binary": { "topology_version": 1, "locations": [], "curve2ds": [], "curves": [], "surfaces": [],
            "polygons3d": [], "polygons_on_triangulations": [], "triangulations": [], "tshapes": shapes,
            "roots": (0..roots).map(|_| shape_use(3, TextOrientation::Forward)).collect::<Vec<_>>() }
    })).unwrap()
}

fn fixture(owners: &[(&str, &[&str])]) -> (CadIr, Vec<ShapePayloadRecord>, Vec<PropertyRecord>) {
    let ctx = cadmpeg_test_support::service_decode_context();
    let payloads = owners
        .iter()
        .map(|(owner, names)| payload(owner, names.len()))
        .collect::<Vec<_>>();
    let properties = owners
        .iter()
        .flat_map(|(owner, names)| {
            [
                property(owner, "Shape", &[]),
                property(owner, "CadmpegBodyNames", names),
            ]
        })
        .collect::<Vec<_>>();
    let mut ir = CadIr::empty();
    let mut losses = Vec::new();
    crate::topology_transfer::transfer(&ctx, &mut ir, &payloads, &properties, &mut losses).unwrap();
    assert!(losses.is_empty());
    assert_eq!(
        ir.model.bodies.len(),
        owners.iter().map(|(_, names)| names.len()).sum::<usize>()
    );
    let namespace = ir.native.namespace_mut("fcstd");
    namespace
        .set_arena(&ctx, "shape_payloads", &payloads)
        .unwrap();
    namespace
        .set_arena(&ctx, "properties", &properties)
        .unwrap();
    (ir, payloads, properties)
}

fn rewrite_rows(ir: &mut CadIr, bodies: &BTreeMap<String, String>) {
    let ctx = cadmpeg_test_support::service_decode_context();
    let mut rows = ir
        .native
        .namespace("fcstd")
        .unwrap()
        .arena_as::<SourceUseSelector>("topology_uses")
        .unwrap();
    let ids = rows
        .iter()
        .enumerate()
        .map(|(index, row)| (row.id.clone(), format!("opaque:native:body-use#{index}")))
        .collect::<BTreeMap<_, _>>();
    for row in &mut rows {
        row.id = ids[&row.id].clone();
        if let SourceUse::Child { parent, .. } = &mut row.source_use {
            *parent = ids[parent.as_str()].clone();
        }
    }
    use cadmpeg_ir::schema::rewrite::typed::{IdentityMap, RewriteIdentities};
    let mut map = IdentityMap::new(|id: &str| {
        Ok(ids
            .get(id)
            .or_else(|| bodies.get(id))
            .cloned()
            .unwrap_or_else(|| id.to_owned()))
    });
    macro_rules! rewrite {
        ($($arena:ident),*) => { $(
            ir.model.$arena = std::mem::take(&mut ir.model.$arena).rewrite_identities(&ctx, &mut map).unwrap();
        )* };
    }
    rewrite!(bodies, regions, shells, edges, vertices, points);
    map.finish().unwrap();
    rows.reverse();
    ir.native
        .namespace_mut("fcstd")
        .set_arena(&ctx, "topology_uses", &rows)
        .unwrap();
}

#[test]
fn body_names_follow_actual_root_slots_after_opaque_identity_and_arena_reordering() {
    let (mut ir, mut payloads, mut properties) = fixture(&[
        ("first", &["\"first-root\"", "\"second-root\""]),
        ("second", &["\"other-owner\""]),
    ]);
    let ctx = cadmpeg_test_support::service_decode_context();
    let body_ids = ir
        .model
        .bodies
        .iter()
        .enumerate()
        .map(|(index, body)| {
            (
                body.id.as_str().to_owned(),
                format!("opaque:model:body#{index}"),
            )
        })
        .collect::<BTreeMap<_, _>>();
    let expected = ir
        .model
        .bodies
        .iter()
        .enumerate()
        .map(|(index, body)| {
            (
                BodyId::mint(body_ids[body.id.as_str()].clone()).unwrap(),
                ["first-root", "second-root", "other-owner"][index],
            )
        })
        .collect::<Vec<_>>();
    rewrite_rows(&mut ir, &body_ids);
    ir.model.bodies.reverse();
    payloads.reverse();
    properties.reverse();
    let native_before = ir.native.clone();
    let before = ir.clone();
    apply(&ctx, &mut ir, &payloads, &properties).unwrap();
    for (id, name) in expected {
        assert_eq!(
            ir.model
                .bodies
                .iter()
                .find(|body| body.id == id)
                .unwrap()
                .name
                .as_deref(),
            Some(name)
        );
    }
    assert_eq!(ir.native, native_before);
    let mut normalized = ir.clone();
    for body in &mut normalized.model.bodies {
        body.name = before
            .model
            .bodies
            .iter()
            .find(|old| old.id == body.id)
            .unwrap()
            .name
            .clone();
    }
    assert_eq!(normalized, before);
}

#[test]
fn missing_first_body_reference_cannot_shift_its_name_to_a_surviving_root() {
    let (mut ir, payloads, properties) =
        fixture(&[("owner", &["\"missing-root-name\"", "\"second-root-name\""])]);
    ir.model.bodies[0].native_ref = None;
    ir.model.bodies[0].name = Some("authored-unlinked-name".to_owned());
    let ctx = cadmpeg_test_support::service_decode_context();
    apply(&ctx, &mut ir, &payloads, &properties).unwrap();
    assert_eq!(
        ir.model.bodies[0].name.as_deref(),
        Some("authored-unlinked-name")
    );
    assert_eq!(ir.model.bodies[1].name.as_deref(), Some("second-root-name"));
}

#[test]
fn child_source_use_cannot_borrow_the_name_of_its_payload_root() {
    let (mut ir, payloads, properties) = fixture(&[("owner", &["\"root-name\""])]);
    let row = ir
        .native
        .namespace("fcstd")
        .unwrap()
        .arena_as::<SourceUseSelector>("topology_uses")
        .unwrap()
        .into_iter()
        .find(|row| matches!(row.source_use, SourceUse::Child { .. }))
        .unwrap();
    ir.model.bodies[0].native_ref = Some(row.id);
    ir.model.bodies[0].name = Some("authored-child-bound-name".to_owned());
    let before = ir.clone();
    let ctx = cadmpeg_test_support::service_decode_context();
    apply(&ctx, &mut ir, &payloads, &properties).unwrap();
    assert_eq!(ir, before);
}

#[test]
fn root_name_values_preserve_json_null_escaped_text_and_plain_text_semantics() {
    let (mut ir, payloads, properties) =
        fixture(&[("owner", &["\"first\\nname\"", "null", "plain name"])]);
    let ctx = cadmpeg_test_support::service_decode_context();
    apply(&ctx, &mut ir, &payloads, &properties).unwrap();
    assert_eq!(
        ir.model
            .bodies
            .iter()
            .map(|body| body.name.as_deref())
            .collect::<Vec<_>>(),
        vec![Some("first\nname"), None, Some("plain name")]
    );
}

#[test]
fn existing_root_name_projection_uses_working_storage_in_the_actual_encode_context() {
    let (mut ir, payloads, properties) = fixture(&[("owner", &["\"first\"", "\"second\""])]);
    let before = ir.native.clone();
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_entities = 0;
    policy.limits.max_retained_bytes = 0;
    let ctx = DecodeContext::for_encode(&arena, &policy);
    apply(&ctx, &mut ir, &payloads, &properties).unwrap();
    assert_eq!(
        ir.model
            .bodies
            .iter()
            .map(|body| body.name.as_deref())
            .collect::<Vec<_>>(),
        vec![Some("first"), Some("second")]
    );
    assert_eq!(ir.native, before);
    assert_eq!(ctx.retained_bytes_available(), 0);
    assert!(ctx.resource_refusal().is_none());
    ctx.finish_session().unwrap();
}

#[test]
fn prior_actual_context_refusal_cannot_apply_partial_body_names() {
    let (mut ir, payloads, properties) = fixture(&[("owner", &["\"first\"", "\"second\""])]);
    let before = ir.clone();
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_retained_bytes = 0;
    let ctx = DecodeContext::for_encode(&arena, &policy);
    let Err(CodecError::ResourceLimit(refusal)) =
        ctx.retain_bytes(1, "body name fixture prior refusal")
    else {
        panic!("fixture must refuse")
    };
    assert!(
        matches!(apply(&ctx, &mut ir, &payloads, &properties), Err(CodecError::ResourceLimit(actual)) if actual == refusal)
    );
    assert_eq!(ir, before);
    assert!(
        matches!(apply(&ctx, &mut ir, &payloads, &[]), Err(CodecError::ResourceLimit(actual)) if actual == refusal)
    );
    assert_eq!(ir, before);
    assert_eq!(ctx.resource_refusal(), Some(refusal));
}

#[test]
fn missing_native_root_rows_do_not_retarget_a_surviving_root_name() {
    let (mut ir, payloads, properties) =
        fixture(&[("owner", &["\"missing-root-name\"", "\"second-root-name\""])]);
    let ctx = cadmpeg_test_support::service_decode_context();
    let namespace = ir.native.namespace("fcstd").unwrap();
    let mut rows = namespace
        .arena_as::<SourceUseSelector>("topology_uses")
        .unwrap();
    let mut deleted =
        std::collections::BTreeSet::from([ir.model.bodies[0].native_ref.clone().unwrap()]);
    loop {
        let count = deleted.len();
        for row in &rows {
            if let SourceUse::Child { parent, .. } = &row.source_use {
                if deleted.contains(parent) {
                    deleted.insert(row.id.clone());
                }
            }
        }
        if deleted.len() == count {
            break;
        }
    }
    rows.retain(|row| !deleted.contains(&row.id));
    ir.native
        .namespace_mut("fcstd")
        .set_arena(&ctx, "topology_uses", &rows)
        .unwrap();
    ir.model.bodies[0].name = Some("authored-missing-owner-name".to_owned());
    let native_before = ir.native.clone();
    apply(&ctx, &mut ir, &payloads, &properties).unwrap();
    assert_eq!(
        ir.model.bodies[0].name.as_deref(),
        Some("authored-missing-owner-name")
    );
    assert_eq!(ir.model.bodies[1].name.as_deref(), Some("second-root-name"));
    assert_eq!(ir.native, native_before);
}

#[test]
fn no_eligible_name_property_keeps_the_original_no_work_no_op() {
    let (original, payloads, properties) = fixture(&[("owner", &["\"first\""])]);
    for paired_shape in [true, false] {
        let mut ir = original.clone();
        let setup = cadmpeg_test_support::service_decode_context();
        ir.native.namespace_mut("fcstd").set_arena(&setup, "topology_uses", &[
            serde_json::json!({"id": crate::native::native_id("topology-use", "unrelated"), "owner": crate::native::native_id("shape-payload", "missing"), "source_use": {"kind": "root", "root": 1}})
        ]).unwrap();
        let selected = properties
            .iter()
            .filter(|property| {
                if paired_shape {
                    property.name == "Shape"
                } else {
                    property.name == "CadmpegBodyNames"
                }
            })
            .cloned()
            .collect::<Vec<_>>();
        let before = ir.clone();
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_entities = 0;
        policy.limits.max_retained_bytes = 0;
        let ctx = DecodeContext::for_encode(&arena, &policy);
        apply(&ctx, &mut ir, &payloads, &selected).unwrap();
        assert_eq!(ir, before);
        assert_eq!(ctx.retained_bytes_available(), 0);
        assert!(ctx.resource_refusal().is_none());
        ctx.finish_session().unwrap();
    }
}
