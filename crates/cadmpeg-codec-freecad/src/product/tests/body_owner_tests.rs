// SPDX-License-Identifier: Apache-2.0
//! Exact payload-prefix ownership and body arena order.

use crate::brep::{ShapePayload, ShapePayloadRecord};
use crate::native;
use cadmpeg_core::decode::ResourceDimension;
use cadmpeg_ir::ids::BodyId;
use cadmpeg_ir::topology::{Body, BodyKind};

fn property(owner: &str, name: &str) -> native::PropertyRecord {
    native::PropertyRecord {
        id: native::native_child_id("property", owner, name),
        owner: owner.into(),
        name: name.into(),
        type_name: "Part::PropertyPartShape".into(),
        family: native::PropertyFamily::Unknown,
        status: None,
        body: native::PropertyBody::Transient,
        order: 0,
        xml: native::RetainedXml::from_text("<Property/>".into(), 0).expect("XML span"),
    }
}

fn payload(property: &native::PropertyRecord, name: &str) -> ShapePayloadRecord {
    ShapePayloadRecord {
        id: native::native_child_id("shape-payload", &property.id, name),
        property: property.id.clone(),
        entry: name.into(),
        payload: ShapePayload::Empty,
    }
}

fn body(payload: &ShapePayloadRecord, label: &str) -> Body {
    Body {
        id: BodyId::mint(native::model_id("body", &payload.id, label)).expect("body identity"),
        kind: BodyKind::default(),
        regions: Vec::new(),
        transform: None,
        name: None,
        color: None,
        visible: None,
    }
}

#[test]
fn body_owner_index_keeps_body_order_and_one_binding_per_owner() {
    let part = super::resource_product_container();
    let other = native::ProductNodeRecord {
        id: "fcstd:native:product#Other".into(),
        object: "fcstd:native:object#Other".into(),
        node: part.node.clone(),
    };
    let properties = [
        property(&part.object, "Shape"),
        property(&part.object, "SecondShape"),
        property(&other.object, "Shape"),
    ];
    let payloads = [
        payload(&properties[0], "shape:a.brp"),
        payload(&properties[1], "shape:b.brp"),
        payload(&properties[2], "shape:c.brp"),
        payload(&properties[0], "shape:a.brp"),
    ];
    let unknown = payload(&properties[0], "unknown.brp");
    let bodies = [
        body(&payloads[1], "8"),
        body(&payloads[2], "2"),
        body(&unknown, "5"),
        body(&payloads[0], "4"),
        body(&payloads[0], "1:located@transform~root0"),
    ];
    crate::test_support::with_service_context(&[], |ctx| {
        let (definitions, _occurrences) = super::super::transfer_neutral(
            ctx,
            &[part, other],
            &[],
            &[],
            &properties,
            &payloads,
            &bodies,
        )
        .expect("neutral products");
        let part = definitions
            .iter()
            .find(|definition| definition.native_ref.as_deref() == Some("fcstd:native:object#Part"))
            .expect("part definition");
        assert_eq!(
            part.bodies,
            [
                bodies[0].id.clone(),
                bodies[3].id.clone(),
                bodies[4].id.clone()
            ]
        );
        let other = definitions
            .iter()
            .find(|definition| {
                definition.native_ref.as_deref() == Some("fcstd:native:object#Other")
            })
            .expect("other definition");
        assert_eq!(other.bodies, [bodies[1].id.clone()]);
    });
}

#[test]
fn body_owner_index_lookup_is_admitted() {
    let record = super::resource_product_container();
    let property = property(&record.object, "Shape");
    let payload = payload(&property, "shape:a.brp");
    let body = body(&payload, "1");
    crate::test_support::refusal_at(
        ResourceDimension::WorkUnits,
        &[],
        "fcstd product body owner lookup",
        |ctx| {
            super::super::transfer_neutral(
                ctx,
                std::slice::from_ref(&record),
                &[],
                &[],
                std::slice::from_ref(&property),
                std::slice::from_ref(&payload),
                std::slice::from_ref(&body),
            )
        },
    );
}
