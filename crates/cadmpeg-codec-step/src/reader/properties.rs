// SPDX-License-Identifier: Apache-2.0
//! STEP scalar property decoding.

use std::collections::{BTreeMap, HashSet};

use cadmpeg_core::decode::DecodeContext;
use cadmpeg_core::CodecError;
use cadmpeg_ir::attributes::{AttributeTarget, AttributeValue, SourceAttribute};
use cadmpeg_ir::document::CadIr;
use cadmpeg_ir::ids::{AttributeId, IdentityKey, OccurrenceId, ProductDefinitionId};

use crate::ids::{self, kind};
use crate::loss::StepLossCode;
use crate::parse::{Exchange, Value};

use super::groups::NonemptyGroup;
use super::index::CarrierIndex;
use super::topology::TopologyData;
use super::{decode_text, representation, RecordExt, StageOutcome, ValueExt};

pub(super) fn decode(
    exchange: &Exchange,
    index: &CarrierIndex,
    topology: &TopologyData,
    definitions: &BTreeMap<u64, ProductDefinitionId>,
    occurrences: &BTreeMap<u64, Vec<OccurrenceId>>,
    ir: &mut CadIr,
    ctx: &DecodeContext<'_>,
) -> Result<StageOutcome<()>, CodecError> {
    let mut typed = HashSet::new();
    let mut losses = Vec::new();
    let children: HashSet<u64> = exchange
        .entities("NEXT_ASSEMBLY_USAGE_OCCURRENCE")
        .filter_map(|(_, record)| {
            super::named_parameter(record, "NEXT_ASSEMBLY_USAGE_OCCURRENCE", 4)
                .and_then(ValueExt::reference)
        })
        .collect();
    'properties: for (relation_id, relation) in
        exchange.entities("PROPERTY_DEFINITION_REPRESENTATION")
    {
        let Some(relation) = relation.partial("PROPERTY_DEFINITION_REPRESENTATION") else {
            continue;
        };
        let Some(property_id) = relation.parameters.first().and_then(ValueExt::reference) else {
            continue;
        };
        let Some(representation_id) = relation.parameters.get(1).and_then(ValueExt::reference)
        else {
            continue;
        };
        let Some(property) = exchange
            .records()
            .get(&property_id)
            .and_then(|record| record.partial("PROPERTY_DEFINITION"))
        else {
            continue;
        };
        if !property.parameters.get(1).is_some_and(empty_or_omitted) {
            continue;
        }
        let Some(owner_id) = property.parameters.get(2).and_then(ValueExt::reference) else {
            continue;
        };
        let (mut target, owner_rep_id) = if let Some(definition) = definitions.get(&owner_id) {
            (AttributeTarget::ProductDefinition(definition.clone()), None)
        } else if let Some(occurrence) = occurrences.get(&owner_id).and_then(|items| items.first())
        {
            (AttributeTarget::Occurrence(occurrence.clone()), None)
        } else {
            let Some(owner) = exchange
                .records()
                .get(&owner_id)
                .and_then(|record| record.partial("CHARACTERIZED_ITEM_WITHIN_REPRESENTATION"))
            else {
                continue;
            };
            if !owner.parameters.first().is_some_and(empty_or_omitted)
                || !owner.parameters.get(1).is_some_and(empty_or_omitted)
            {
                continue;
            }
            let Some(item_id) = owner.parameters.get(2).and_then(ValueExt::reference) else {
                continue;
            };
            let Some(owner_rep_id) = owner.parameters.get(3).and_then(ValueExt::reference) else {
                continue;
            };
            let Some(owner_rep) = exchange.records().get(&owner_rep_id) else {
                continue;
            };
            if !representation::items(owner_rep)
                .is_some_and(|mut items| items.any(|id| id == item_id))
            {
                continue;
            }
            let target = if let Some(point) = index.points.get(&item_id) {
                AttributeTarget::Point(ir.model.points[point.index.0].id.clone())
            } else if let Some(curve) = index.curves.get(&item_id) {
                AttributeTarget::Curve(ir.model.curves[curve.0].id.clone())
            } else if let Some(surface) = index.surfaces.get(&item_id) {
                AttributeTarget::Surface(ir.model.surfaces[surface.0].id.clone())
            } else if let Some([body]) = topology.body_by_root.get(&item_id).map(Vec::as_slice) {
                AttributeTarget::Body(body.clone())
            } else if let Some(face) = topology.faces_by_source.get(&item_id).and_then(single) {
                AttributeTarget::Face(face.clone())
            } else if let Some(edge) = topology.edges_by_source.get(&item_id).and_then(single) {
                AttributeTarget::Edge(edge.clone())
            } else if let Some(vertex) = topology.vertices_by_source.get(&item_id).and_then(single)
            {
                AttributeTarget::Vertex(vertex.clone())
            } else {
                continue;
            };
            (target, Some(owner_rep_id))
        };
        let Some(rep) = exchange.records().get(&representation_id) else {
            continue;
        };
        if !representation::parameters(rep)
            .and_then(|parameters| parameters.first())
            .is_some_and(empty_or_omitted)
        {
            continue;
        }
        let Some([Value::Reference(value_id)]) = representation::item_values(rep) else {
            continue;
        };
        let value_id = *value_id;
        let Some(value_record) = exchange.records().get(&value_id) else {
            continue;
        };
        let Some(name) = property.parameters.first().and_then(|value| {
            decode_text(
                exchange,
                value,
                &mut losses,
                property_id,
                "property name",
                StepLossCode::MetadataStringInvalid,
            )
        }) else {
            continue;
        };
        let item_name = value_record.parameter(0).and_then(|value| {
            decode_text(
                exchange,
                value,
                &mut losses,
                value_id,
                "property item name",
                StepLossCode::MetadataStringInvalid,
            )
        });
        let document_property = definitions.contains_key(&owner_id)
            && !children.contains(&owner_id)
            && !name.is_empty()
            && item_name.as_deref() == Some(name.as_str());
        if document_property {
            target = AttributeTarget::Document;
        } else if !value_record.parameter(0).is_some_and(empty_or_omitted) {
            continue;
        }
        let value_ids: Vec<u64> = if let Some(row) = value_record.partial("ROW_REPRESENTATION_ITEM")
        {
            let Some(Value::List(items)) = row.parameters.get(1) else {
                continue;
            };
            if items.is_empty() {
                continue;
            }
            let mut ids = Vec::new();
            ctx.reserve_working(&mut ids, items.len(), "STEP property row references")?;
            for item in items {
                let Some(id) = item.reference() else {
                    continue 'properties;
                };
                ids.push(id);
            }
            ids
        } else {
            vec![value_id]
        };
        let mut values = Vec::new();
        ctx.reserve_working(&mut values, value_ids.len(), "STEP property row values")?;
        for id in &value_ids {
            let Some(value) = scalar_property_value(exchange, *id, &mut losses, document_property)
            else {
                continue 'properties;
            };
            values.push(value);
        }
        ir.model.attributes.push(SourceAttribute::new(
            ctx,
            AttributeId::from(ids::data(kind!("scalar-property"), relation_id)),
            None,
            target,
            name.clone(),
            values.clone(),
        )?);
        if let Some(items) = occurrences.get(&owner_id) {
            for (ordinal, occurrence) in items.iter().enumerate().skip(1) {
                ir.model.attributes.push(SourceAttribute::new(
                    ctx,
                    AttributeId::from(ids::data(
                        kind!("scalar-property"),
                        IdentityKey::from(relation_id).dash(ordinal),
                    )),
                    None,
                    AttributeTarget::Occurrence(occurrence.clone()),
                    name.clone(),
                    values.clone(),
                )?);
            }
        }
        typed.extend([
            relation_id,
            property_id,
            owner_id,
            representation_id,
            value_id,
        ]);
        typed.extend(value_ids);
        if let Some(owner_rep_id) = owner_rep_id {
            let owner_rep = &exchange.records()[&owner_rep_id];
            if representation::item_values(owner_rep).is_some_and(|items| items.len() == 1)
                && representation::parameters(owner_rep)
                    .and_then(|parameters| parameters.first())
                    .is_some_and(empty_or_omitted)
            {
                typed.insert(owner_rep_id);
            }
        }
    }
    Ok(StageOutcome {
        value: (),
        claims: typed,
        losses,
        notes: Vec::new(),
    })
}

fn scalar_property_value(
    exchange: &Exchange,
    id: u64,
    losses: &mut Vec<cadmpeg_ir::report::loss::LossNote>,
    named: bool,
) -> Option<AttributeValue> {
    let record = exchange.records().get(&id)?;
    if !named && !record.parameter(0).is_some_and(empty_or_omitted) {
        return None;
    }
    if let Some(item) = record.partial("DESCRIPTIVE_REPRESENTATION_ITEM") {
        decode_text(
            exchange,
            item.parameters.get(1)?,
            losses,
            id,
            "property value",
            StepLossCode::MetadataStringInvalid,
        )
        .map(AttributeValue::String)
    } else if let Some(item) = record.partial("INTEGER_REPRESENTATION_ITEM") {
        item.parameters
            .get(1)?
            .integer()
            .map(AttributeValue::Integer)
    } else if let Some(item) = record.partial("REAL_REPRESENTATION_ITEM") {
        item.parameters
            .get(1)?
            .number()
            .and_then(AttributeValue::float)
    } else if let Some(item) = record.partial("BOOLEAN_REPRESENTATION_ITEM") {
        item.parameters
            .get(1)?
            .logical()
            .map(AttributeValue::Boolean)
    } else if let Some(item) = record.partial("VALUE_REPRESENTATION_ITEM") {
        match item.parameters.get(1)? {
            Value::Typed(kind, value) if kind == "COUNT_MEASURE" => {
                value.integer().map(AttributeValue::Integer)
            }
            Value::Typed(kind, value) if kind == "NUMERIC_MEASURE" => {
                value.number().and_then(AttributeValue::float)
            }
            _ => None,
        }
    } else {
        None
    }
}

fn single<T>(group: &NonemptyGroup<T>) -> Option<&T> {
    let mut items = group.iter();
    let first = items.next()?;
    items.next().is_none().then_some(first)
}

fn empty_or_omitted(value: &Value) -> bool {
    matches!(value, Value::Omitted) || matches!(value, Value::String(bytes) if bytes.is_empty())
}

#[cfg(test)]
mod tests {
    use crate::test_support::exchange::decode_inline;
    use cadmpeg_ir::attributes::{AttributeTarget, AttributeValue};

    #[test]
    fn named_root_property_decodes_as_document_attribute() {
        let decoded = decode_inline(
            "#1=APPLICATION_CONTEXT('mechanical design');
#2=PRODUCT_CONTEXT('',#1,'mechanical');
#3=PRODUCT('P','Part','',(#2));
#4=PRODUCT_DEFINITION_FORMATION('','',#3);
#5=PRODUCT_DEFINITION_CONTEXT('part definition',#1,'design');
#6=PRODUCT_DEFINITION('part','',#4,#5);
#7=PROPERTY_DEFINITION('Count','',#6);
#8=VALUE_REPRESENTATION_ITEM('Count',COUNT_MEASURE(7));
#9=(GEOMETRIC_REPRESENTATION_CONTEXT(3) REPRESENTATION_CONTEXT('',''));
#10=REPRESENTATION('',(#8),#9);
#11=PROPERTY_DEFINITION_REPRESENTATION(#7,#10);",
        );
        assert!(decoded.ir().model.attributes.iter().any(|attribute| {
            attribute.name == "Count"
                && attribute.target == AttributeTarget::Document
                && attribute.values == [Some(AttributeValue::Integer(7))]
        }));
    }

    #[test]
    fn document_attributes_write_cax_if_form_and_round_trip() {
        use cadmpeg_ir::attributes::{AttributeTarget, AttributeValue, SourceAttribute};
        use cadmpeg_ir::codec::{Codec, DecodeOptions};
        use std::io::Cursor;

        let mut ir = cadmpeg_ir::examples::unit_cube().expect("cube");
        for (name, value) in [
            ("Title", AttributeValue::String("Bracket".into())),
            ("Revision", AttributeValue::Integer(3)),
            ("Mass", AttributeValue::float(1.5).expect("finite mass")),
        ] {
            ir.model.attributes.push(SourceAttribute {
                id: format!("test:model:attribute#{name}")
                    .try_into()
                    .expect("attribute id"),
                native_ref: None,
                target: AttributeTarget::Document,
                name: name.into(),
                values: vec![Some(value)],
            });
        }
        let mut bytes = Vec::new();
        crate::export::write_step(
            &ir,
            &mut bytes,
            crate::StepSchema::Ap242Edition3,
            &crate::StepWriteOptions::default(),
        )
        .expect("write document attributes");
        let text = std::str::from_utf8(&bytes).expect("utf-8");
        assert!(text.contains("PROPERTY_DEFINITION_REPRESENTATION"));
        assert!(text.contains("DESCRIPTIVE_REPRESENTATION_ITEM"));
        assert!(text.contains("VALUE_REPRESENTATION_ITEM"));
        assert!(text.contains("COUNT_MEASURE(3)"));
        assert!(text.contains("NUMERIC_MEASURE"));

        let decoded = crate::StepCodec::default()
            .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
            .expect("decode document attributes");
        for (name, value) in [
            ("Title", AttributeValue::String("Bracket".into())),
            ("Revision", AttributeValue::Integer(3)),
            ("Mass", AttributeValue::float(1.5).expect("finite mass")),
        ] {
            assert!(
                decoded.ir().model.attributes.iter().any(|attribute| {
                    attribute.target == AttributeTarget::Document
                        && attribute.name == name
                        && attribute.values == [Some(value.clone())]
                }),
                "missing document attribute {name}"
            );
        }
    }

    #[test]
    fn document_attribute_value_not_representable_in_cax_if_is_degraded() {
        use cadmpeg_ir::attributes::{AttributeTarget, AttributeValue, SourceAttribute};

        let mut ir = cadmpeg_ir::examples::unit_cube().expect("cube");
        ir.model.attributes.push(SourceAttribute {
            id: "test:model:attribute#Flag"
                .try_into()
                .expect("attribute id"),
            native_ref: None,
            target: AttributeTarget::Document,
            name: "Flag".into(),
            values: vec![Some(AttributeValue::Boolean(true))],
        });
        let mut bytes = Vec::new();
        let report = crate::export::write_step(
            &ir,
            &mut bytes,
            crate::StepSchema::Ap242Edition3,
            &crate::StepWriteOptions::default(),
        )
        .expect("write degraded document attribute");
        assert!(report.losses.iter().any(|loss| {
            loss.code == crate::loss::StepLossCode::DocumentAttributeValueDegraded.kind()
                && loss.message.contains("Flag")
        }));
    }

    #[test]
    fn ordered_property_row_decodes_and_round_trips() {
        use cadmpeg_ir::codec::{Codec, DecodeOptions};
        let source = decode_inline(
            "#1=CARTESIAN_POINT('',(1.,2.,3.));
#2=(GEOMETRIC_REPRESENTATION_CONTEXT(3) REPRESENTATION_CONTEXT('',''));
#3=REPRESENTATION('',(#1),#2);
#4=CHARACTERIZED_ITEM_WITHIN_REPRESENTATION('','',#1,#3);
#5=PROPERTY_DEFINITION('Values','',#4);
#6=INTEGER_REPRESENTATION_ITEM('',7);
#7=DESCRIPTIVE_REPRESENTATION_ITEM('','example');
#8=BOOLEAN_REPRESENTATION_ITEM('',.T.);
#9=ROW_REPRESENTATION_ITEM('',(#6,#7,#8));
#10=REPRESENTATION('',(#9),#2);
#11=PROPERTY_DEFINITION_REPRESENTATION(#5,#10);",
        );
        let expected = vec![
            Some(AttributeValue::Integer(7)),
            Some(AttributeValue::String("example".into())),
            Some(AttributeValue::Boolean(true)),
        ];
        assert!(source
            .ir()
            .model
            .attributes
            .iter()
            .any(|attribute| attribute.name == "Values"
                && attribute.values == expected
                && matches!(attribute.target, AttributeTarget::Point(_))));
        let mut bytes = Vec::new();
        crate::export::write_step(
            source.ir(),
            &mut bytes,
            crate::StepSchema::Ap242Edition3,
            &crate::StepWriteOptions::default(),
        )
        .expect("write ordered property");
        assert!(std::str::from_utf8(&bytes)
            .unwrap()
            .contains("ROW_REPRESENTATION_ITEM"));
        let decoded = crate::StepCodec::default()
            .decode(&mut std::io::Cursor::new(bytes), &DecodeOptions::default())
            .expect("decode ordered property");
        assert!(decoded
            .ir()
            .model
            .attributes
            .iter()
            .any(|attribute| attribute.name == "Values"
                && attribute.values == expected
                && matches!(attribute.target, AttributeTarget::Point(_))));
    }

    #[test]
    fn occurrence_property_follows_each_expansion_of_a_shared_usage() {
        let decoded = decode_inline(
            "#1=APPLICATION_CONTEXT('mechanical design');
#2=PRODUCT_CONTEXT('',#1,'mechanical');
#3=PRODUCT('A','Assembly','',(#2));
#4=PRODUCT_DEFINITION_FORMATION('','',#3);
#5=PRODUCT_DEFINITION_CONTEXT('',#1,'design');
#6=PRODUCT_DEFINITION('','',#4,#5);
#7=PRODUCT('B','Subassembly','',(#2));
#8=PRODUCT_DEFINITION_FORMATION('','',#7);
#9=PRODUCT_DEFINITION('','',#8,#5);
#10=PRODUCT('C','Part','',(#2));
#11=PRODUCT_DEFINITION_FORMATION('','',#10);
#12=PRODUCT_DEFINITION('','',#11,#5);
#13=NEXT_ASSEMBLY_USAGE_OCCURRENCE('1','','',#6,#9,$);
#14=NEXT_ASSEMBLY_USAGE_OCCURRENCE('2','','',#6,#9,$);
#15=NEXT_ASSEMBLY_USAGE_OCCURRENCE('3','','',#9,#12,$);
#16=PROPERTY_DEFINITION('Grade','',#15);
#17=INTEGER_REPRESENTATION_ITEM('',7);
#18=(GEOMETRIC_REPRESENTATION_CONTEXT(3) REPRESENTATION_CONTEXT('',''));
#19=REPRESENTATION('',(#17),#18);
#20=PROPERTY_DEFINITION_REPRESENTATION(#16,#19);",
        );
        let attributes: Vec<_> = decoded
            .ir()
            .model
            .attributes
            .iter()
            .filter(|attribute| attribute.name == "Grade")
            .collect();
        assert_eq!(attributes.len(), 2);
        assert!(attributes.iter().all(|attribute| matches!(
            attribute.target,
            AttributeTarget::Occurrence(_)
        ) && attribute.values
            == [Some(AttributeValue::Integer(7))]));
        assert_ne!(attributes[0].target, attributes[1].target);
    }

    #[test]
    fn occurrence_scalar_property_is_retained() {
        let decoded = decode_inline(
            "#1=APPLICATION_CONTEXT('mechanical design');
#2=PRODUCT_CONTEXT('',#1,'mechanical');
#3=PRODUCT('A','Assembly','',(#2));
#4=PRODUCT_DEFINITION_FORMATION('','',#3);
#5=PRODUCT_DEFINITION_CONTEXT('',#1,'design');
#6=PRODUCT_DEFINITION('','',#4,#5);
#7=PRODUCT('B','Part','',(#2));
#8=PRODUCT_DEFINITION_FORMATION('','',#7);
#9=PRODUCT_DEFINITION('','',#8,#5);
#10=NEXT_ASSEMBLY_USAGE_OCCURRENCE('','', '',#6,#9,$);
#11=PROPERTY_DEFINITION('Grade','',#10);
#12=INTEGER_REPRESENTATION_ITEM('',7);
#13=(GEOMETRIC_REPRESENTATION_CONTEXT(3) REPRESENTATION_CONTEXT('',''));
#14=REPRESENTATION('',(#12),#13);
#15=PROPERTY_DEFINITION_REPRESENTATION(#11,#14);",
        );
        assert!(decoded.ir().model.attributes.iter().any(|attribute| {
            matches!(attribute.target, AttributeTarget::Occurrence(_))
                && attribute.name == "Grade"
                && attribute.values == [Some(AttributeValue::Integer(7))]
        }));
    }

    #[test]
    fn product_definition_scalar_property_is_retained() {
        let decoded = decode_inline(
            "#1=APPLICATION_CONTEXT('mechanical design');
#2=PRODUCT_CONTEXT('',#1,'mechanical');
#3=PRODUCT('P','Part','',(#2));
#4=PRODUCT_DEFINITION_FORMATION('','',#3);
#5=PRODUCT_DEFINITION_CONTEXT('',#1,'design');
#6=PRODUCT_DEFINITION('','',#4,#5);
#7=PROPERTY_DEFINITION('Grade','',#6);
#8=INTEGER_REPRESENTATION_ITEM('',7);
#9=(GEOMETRIC_REPRESENTATION_CONTEXT(3) REPRESENTATION_CONTEXT('',''));
#10=REPRESENTATION('',(#8),#9);
#11=PROPERTY_DEFINITION_REPRESENTATION(#7,#10);",
        );
        assert!(decoded.ir().model.attributes.iter().any(|attribute| {
            matches!(attribute.target, AttributeTarget::ProductDefinition(_))
                && attribute.name == "Grade"
                && attribute.values == [Some(AttributeValue::Integer(7))]
        }));
    }

    #[test]
    fn writer_round_trips_point_properties() {
        use cadmpeg_ir::codec::{Codec, DecodeOptions};
        use std::io::Cursor;

        let source = decode_inline(
            "#1=CARTESIAN_POINT('',(1.,2.,3.));
#2=(GEOMETRIC_REPRESENTATION_CONTEXT(3) REPRESENTATION_CONTEXT('',''));
#3=REPRESENTATION('',(#1),#2);
#4=CHARACTERIZED_ITEM_WITHIN_REPRESENTATION('','',#1,#3);
#5=PROPERTY_DEFINITION('Grade','',#4);
#6=INTEGER_REPRESENTATION_ITEM('',7);
#7=REPRESENTATION('',(#6),#2);
#8=PROPERTY_DEFINITION_REPRESENTATION(#5,#7);",
        );
        let mut bytes = Vec::new();
        crate::export::write_step(
            source.ir(),
            &mut bytes,
            crate::StepSchema::Ap242Edition3,
            &crate::StepWriteOptions::default(),
        )
        .expect("write property");
        let decoded = crate::StepCodec::default()
            .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
            .expect("decode property");
        assert!(decoded.ir().model.attributes.iter().any(|attribute| {
            attribute.name == "Grade"
                && attribute.values == [Some(AttributeValue::Integer(7))]
                && matches!(attribute.target, AttributeTarget::Point(_))
        }));
    }

    #[test]
    fn writer_round_trips_topology_properties() {
        use cadmpeg_ir::attributes::SourceAttribute;
        use cadmpeg_ir::codec::{Codec, DecodeOptions};
        use std::io::Cursor;

        let mut ir = cadmpeg_ir::examples::unit_cube().expect("cube");
        let targets = [
            ("body", AttributeTarget::Body(ir.model.bodies[0].id.clone())),
            ("face", AttributeTarget::Face(ir.model.faces[0].id.clone())),
            ("edge", AttributeTarget::Edge(ir.model.edges[0].id.clone())),
            (
                "vertex",
                AttributeTarget::Vertex(ir.model.vertices[0].id.clone()),
            ),
        ];
        for (name, target) in targets {
            ir.model.attributes.push(SourceAttribute {
                id: format!("test:model:attribute#{name}")
                    .try_into()
                    .expect("attribute id"),
                native_ref: None,
                target,
                name: name.into(),
                values: vec![Some(AttributeValue::Integer(7))],
            });
        }
        let mut bytes = Vec::new();
        crate::export::write_step(
            &ir,
            &mut bytes,
            crate::StepSchema::Ap242Edition3,
            &crate::StepWriteOptions::default(),
        )
        .expect("write properties");
        let decoded = crate::StepCodec::default()
            .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
            .expect("decode properties");
        for name in ["body", "face", "edge", "vertex"] {
            assert!(
                decoded.ir().model.attributes.iter().any(|attribute| {
                    attribute.name == name
                        && matches!(
                            (name, &attribute.target),
                            ("body", AttributeTarget::Body(_))
                                | ("face", AttributeTarget::Face(_))
                                | ("edge", AttributeTarget::Edge(_))
                                | ("vertex", AttributeTarget::Vertex(_))
                        )
                        && attribute.values == [Some(AttributeValue::Integer(7))]
                }),
                "missing {name} property"
            );
        }
    }

    #[test]
    fn characterized_point_scalar_property_is_retained() {
        let decoded = decode_inline("#1=CARTESIAN_POINT('',(1.,2.,3.));\n#2=(GEOMETRIC_REPRESENTATION_CONTEXT(3) REPRESENTATION_CONTEXT('',''));\n#3=REPRESENTATION('',(#1),#2);\n#4=CHARACTERIZED_ITEM_WITHIN_REPRESENTATION('','',#1,#3);\n#5=PROPERTY_DEFINITION('Grade','',#4);\n#6=INTEGER_REPRESENTATION_ITEM('',7);\n#7=REPRESENTATION('',(#6),#2);\n#8=PROPERTY_DEFINITION_REPRESENTATION(#5,#7);");
        let point = &decoded.ir().model.points[0];
        assert!(decoded.ir().model.attributes.iter().any(|attribute| {
            attribute.target == AttributeTarget::Point(point.id.clone())
                && attribute.name == "Grade"
                && attribute.values == [Some(AttributeValue::Integer(7))]
        }));
    }
}
