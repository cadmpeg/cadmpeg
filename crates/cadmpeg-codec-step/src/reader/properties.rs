// SPDX-License-Identifier: Apache-2.0
//! STEP scalar property decoding.

use std::collections::HashSet;

use cadmpeg_core::decode::DecodeContext;
use cadmpeg_core::CodecError;
use cadmpeg_ir::attributes::{AttributeTarget, AttributeValue, SourceAttribute};
use cadmpeg_ir::document::CadIr;
use cadmpeg_ir::ids::AttributeId;

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
    ir: &mut CadIr,
    ctx: &DecodeContext<'_>,
) -> Result<StageOutcome<()>, CodecError> {
    let mut typed = HashSet::new();
    let mut losses = Vec::new();
    for (relation_id, relation) in exchange.entities("PROPERTY_DEFINITION_REPRESENTATION") {
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
        if !representation::items(owner_rep).is_some_and(|mut items| items.any(|id| id == item_id))
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
        } else if let Some(vertex) = topology.vertices_by_source.get(&item_id).and_then(single) {
            AttributeTarget::Vertex(vertex.clone())
        } else {
            continue;
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
        if !value_record.parameter(0).is_some_and(empty_or_omitted) {
            continue;
        }
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
        let value = if let Some(item) = value_record.partial("DESCRIPTIVE_REPRESENTATION_ITEM") {
            let Some(value) = item.parameters.get(1).and_then(|value| {
                decode_text(
                    exchange,
                    value,
                    &mut losses,
                    value_id,
                    "property value",
                    StepLossCode::MetadataStringInvalid,
                )
            }) else {
                continue;
            };
            AttributeValue::String(value)
        } else if let Some(item) = value_record.partial("INTEGER_REPRESENTATION_ITEM") {
            let Some(value) = item.parameters.get(1).and_then(ValueExt::integer) else {
                continue;
            };
            AttributeValue::Integer(value)
        } else if let Some(item) = value_record.partial("REAL_REPRESENTATION_ITEM") {
            let Some(value) = item
                .parameters
                .get(1)
                .and_then(ValueExt::number)
                .and_then(AttributeValue::float)
            else {
                continue;
            };
            value
        } else if let Some(item) = value_record.partial("BOOLEAN_REPRESENTATION_ITEM") {
            let Some(value) = item.parameters.get(1).and_then(ValueExt::logical) else {
                continue;
            };
            AttributeValue::Boolean(value)
        } else {
            continue;
        };
        ir.model.attributes.push(SourceAttribute::new(
            ctx,
            AttributeId::from(ids::data(kind!("scalar-property"), relation_id)),
            None,
            target,
            name,
            vec![value],
        )?);
        typed.extend([
            relation_id,
            property_id,
            owner_id,
            representation_id,
            value_id,
        ]);
        if matches!(representation::item_values(owner_rep), Some([Value::Reference(id)]) if *id == item_id)
            && representation::parameters(owner_rep)
                .and_then(|parameters| parameters.first())
                .is_some_and(empty_or_omitted)
        {
            typed.insert(owner_rep_id);
        }
    }
    Ok(StageOutcome {
        value: (),
        claims: typed,
        losses,
        notes: Vec::new(),
    })
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
