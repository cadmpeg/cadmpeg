// SPDX-License-Identifier: Apache-2.0

use super::{payload, reduce};
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, View};
use cadmpeg_ir::attributes::{AttributeTarget, AttributeValue, SourceAttribute};
use cadmpeg_ir::features::{DesignParameter, Dimension, Expression, ParameterId, ParameterValue};
use cadmpeg_ir::ids::{AttributeId, PmiId};
use cadmpeg_ir::pmi::{DimensionKind, PmiAnnotation, PmiDefinition, PmiDimension};
use cadmpeg_ir::scalar::Length;
use cadmpeg_ir::CadIr;
use std::collections::BTreeMap;

fn document() -> (CadIr, ParameterId) {
    let mut ir = CadIr::empty();
    let parameter = ParameterId::mint("test:parameter:dimension#length").unwrap();
    let pmi = PmiId::mint("test:model:pmi#length").unwrap();
    let value = ParameterValue::Length(Length::new(25.0).unwrap());
    ir.model.parameters.push(DesignParameter {
        id: parameter.clone(),
        owner: None,
        ordinal: 0,
        name: "Length".into(),
        expression: Expression::literal(&value),
        driving: true,
        display: None,
        value: Some(value),
        properties: BTreeMap::new(),
        pmi: Some(pmi.clone()),
        native_ref: None,
    });
    ir.model.pmi.push(PmiAnnotation {
        id: pmi.clone(),
        name: None,
        visible: None,
        targets: Vec::new(),
        definition: PmiDefinition::Dimension(
            PmiDimension::new(
                DimensionKind::Size,
                Some(Dimension::Parameter(parameter.clone())),
                None,
            )
            .unwrap(),
        ),
    });
    ir.model.pmi.push(PmiAnnotation {
        id: PmiId::mint("test:model:pmi#text").unwrap(),
        name: None,
        visible: None,
        targets: Vec::new(),
        definition: PmiDefinition::Presentation {
            text: Some("current dimension text".repeat(400)),
            placement: None,
            semantics: vec![pmi.clone()],
        },
    });
    ir.model.attributes.push(SourceAttribute {
        id: AttributeId::mint("test:attribute:pmi#precision").unwrap(),
        target: AttributeTarget::Pmi(pmi),
        name: "precision".into(),
        values: vec![AttributeValue::Integer(9)],
    });
    (ir, parameter)
}

#[test]
fn authored_database_round_trips_current_bound_value_names_and_overflow_text() {
    let (mut ir, parameter) = document();
    ir.model.parameters[0].name = "CurrentLength".into();
    let value = ParameterValue::Length(Length::new(30.0).unwrap());
    ir.set_parameter_expression(&parameter, Expression::literal(&value), Some(value))
        .unwrap();
    let bytes = payload(&ir, None).unwrap().unwrap();
    assert_eq!(bytes.get(..7), Some(b"unqlite".as_slice()));
    assert_eq!(View::u32_be_at(&bytes, 7), Some(0xdb7c_2712));
    let arena = DecodeArena::new();
    let (ctx, _) =
        DecodeContext::from_root_bytes(&bytes, &arena, &DecodePolicy::service()).unwrap();
    let mut losses = Vec::new();
    let records = crate::pmi::parse_payload(&ctx, &bytes, &mut losses).unwrap();
    assert!(losses.is_empty(), "{losses:?}");
    let [record] = records.as_slice() else {
        panic!("one semantic dimension survives");
    };
    assert_eq!(record.cad_text, "CurrentLength");
    assert_eq!(record.value.get(), 0.03);
    assert_eq!(record.precision, 9);
    assert_eq!(
        record.display_text(),
        Some("current dimension text".repeat(400).as_str())
    );
    assert!(record.offset < Some(u64::try_from(bytes.len()).unwrap()));
    assert!(record.value_offset < Some(u64::try_from(bytes.len()).unwrap()));
    ctx.finish_session().unwrap();
}

#[test]
fn unsupported_semantics_drop_only_their_own_occurrence() {
    let (mut ir, _) = document();
    let mut unsupported = ir.model.pmi[0].clone();
    unsupported.id = PmiId::mint("test:model:pmi#unevaluated").unwrap();
    unsupported.definition =
        PmiDefinition::Dimension(PmiDimension::new(DimensionKind::Size, None, None).unwrap());
    ir.model.pmi.push(unsupported.clone());
    let mut losses = Vec::new();
    reduce(&mut ir, &mut losses).unwrap();
    assert_eq!(ir.model.pmi.len(), 2);
    assert!(!ir
        .model
        .pmi
        .iter()
        .any(|annotation| annotation.id == unsupported.id));
    assert!(losses
        .iter()
        .any(|loss| loss.message.contains(unsupported.id.as_str())));
    assert!(payload(&ir, None).unwrap().is_some());
}
