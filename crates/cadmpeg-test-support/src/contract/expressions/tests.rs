// SPDX-License-Identifier: Apache-2.0
use super::*;
use cadmpeg_ir::attributes::{AttributeExpression, AttributeValue};
use cadmpeg_ir::features::DesignParameter;
use cadmpeg_ir::ids::AttributeId;

struct Symbols {
    first: ParameterId,
    second: ParameterId,
}
impl ExpressionSourceResolver for Symbols {
    fn source_name(&self, id: &ParameterId) -> Result<String, String> {
        if id == &self.first {
            Ok("Box.Length".into())
        } else if id == &self.second {
            Ok("Other.Length".into())
        } else {
            Err("missing source parameter".into())
        }
    }
    fn reference_spans(
        &self,
        _: &SourceAttribute,
        text: &str,
    ) -> Result<Vec<(Range<usize>, ParameterId)>, String> {
        let mut spans = Vec::new();
        let mut at = 0;
        while at < text.len() {
            let rest = &text[at..];
            if rest.starts_with("ambiguous") {
                return Err("ambiguous source name".into());
            }
            let found = [
                ("Box.Length", &self.first),
                ("Length", &self.first),
                ("<<Box label>>.Length", &self.first),
                ("Other.Length", &self.second),
            ]
            .into_iter()
            .find(|(name, _)| rest.starts_with(name));
            if let Some((name, id)) = found {
                spans.push((at..at + name.len(), id.clone()));
                at += name.len();
            } else {
                at += rest.chars().next().unwrap().len_utf8();
            }
        }
        Ok(spans)
    }
}
fn fixture() -> (CadIr, Symbols) {
    let mut ir = CadIr::empty();
    let first = ParameterId::mint("test:design:parameter#first").unwrap();
    let second = ParameterId::mint("test:design:parameter#second").unwrap();
    for id in [&first, &second] {
        ir.model.parameters.push(DesignParameter {
            id: id.clone(),
            owner: None,
            ordinal: 0,
            name: "Length".into(),
            expression: "5".into(),
            display: None,
            value: None,
            driving: true,
            properties: BTreeMap::new(),
            pmi: None,
            native_ref: None,
        });
    }
    ir.model.attributes.push(SourceAttribute {
        id: AttributeId::mint("test:model:attribute#expression").unwrap(),
        native_ref: Some("test:native:property#Engine".into()),
        target: AttributeTarget::Document,
        name: "ExpressionEngine".into(),
        values: vec![AttributeValue::Expression(AttributeExpression {
            path: "Extra".into(),
            expression: Expression::from_spans("Length + 2", vec![(0..6, first.clone())]).unwrap(),
            comment: Some("preserved comment".into()),
        })],
    });
    (ir, Symbols { first, second })
}
fn set_text(ir: &mut CadIr, text: &str) {
    let AttributeValue::Expression(value) = &mut ir.model.attributes[0].values[0] else {
        unreachable!()
    };
    value.expression = text.into();
}
#[test]
fn local_qualified_and_label_aliases_compare_only_through_actual_source_symbols() {
    let (full, symbols) = fixture();
    for text in ["Length + 2", "Box.Length + 2", "<<Box label>>.Length + 2"] {
        let mut geometry = full.clone();
        geometry.model.parameters.clear();
        set_text(&mut geometry, text);
        let (left, right) =
            neutral_views(&full, &geometry, &[ArenaClass::Design], &symbols).unwrap();
        assert_eq!(left, right);
    }
}
#[test]
fn changed_literals_references_targets_comments_and_value_order_remain_unequal() {
    let (full, symbols) = fixture();
    let mut changed = full.clone();
    changed.model.parameters.clear();
    for text in ["Length + 3", "2 + Length", "Other.Length + 2"] {
        set_text(&mut changed, text);
        let (left, right) =
            neutral_views(&full, &changed, &[ArenaClass::Design], &symbols).unwrap();
        assert_ne!(left, right);
    }
    set_text(&mut changed, "Length + 2");
    changed.model.attributes[0].name = "changed".into();
    let (left, right) = neutral_views(&full, &changed, &[ArenaClass::Design], &symbols).unwrap();
    assert_ne!(left, right);
    changed = full.clone();
    changed.model.parameters.clear();
    let AttributeValue::Expression(value) = &mut changed.model.attributes[0].values[0] else {
        unreachable!()
    };
    value.comment = None;
    let (left, right) = neutral_views(&full, &changed, &[ArenaClass::Design], &symbols).unwrap();
    assert_ne!(left, right);
    changed = full.clone();
    changed.model.parameters.clear();
    changed.model.attributes[0].target = AttributeTarget::Record {
        arena: cadmpeg_ir::ArenaName::parse("bodies").unwrap(),
        id: "test:model:body#other".into(),
    };
    let (left, right) = neutral_views(&full, &changed, &[ArenaClass::Design], &symbols).unwrap();
    assert_ne!(left, right);
}
#[test]
fn dangling_tokens_and_ambiguous_source_names_fail_closed() {
    let (mut full, symbols) = fixture();
    let geometry = full.clone();
    let AttributeValue::Expression(value) = &mut full.model.attributes[0].values[0] else {
        unreachable!()
    };
    value.expression = Expression::from_spans(
        "missing",
        vec![(
            0..7,
            ParameterId::mint("test:design:parameter#missing").unwrap(),
        )],
    )
    .unwrap();
    assert!(neutral_views(&full, &geometry, &[ArenaClass::Design], &symbols).is_err());
    let (full, symbols) = fixture();
    let mut geometry = full.clone();
    set_text(&mut geometry, "ambiguous + 2");
    assert!(neutral_views(&full, &geometry, &[ArenaClass::Design], &symbols).is_err());
}
