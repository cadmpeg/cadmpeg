// SPDX-License-Identifier: Apache-2.0
#![allow(clippy::unwrap_used)]

use std::collections::{BTreeMap, BTreeSet};

use cadmpeg_core::decode::ContentScope;

use crate::attributes::{AttributeTarget, AttributeValue, SourceAttribute};
use crate::features::{ConfigurationId, ParameterId};
use crate::ids::{AttributeId, ProductDefinitionId};
use crate::validate::validate_neutral;
use crate::CadIr;

#[derive(Clone, Copy, Debug)]
enum Home {
    Model,
    Design,
    Neither,
}

fn missing(home: Home) -> Vec<String> {
    let parameter = ParameterId::mint("test:design:parameter#mass").unwrap();
    let configuration = ConfigurationId::mint("test:design:configuration#large").unwrap();
    let scope = ProductDefinitionId::mint("test:model:product-definition#part").unwrap();
    let mut ir = CadIr::empty();
    ir.set_scope(ContentScope::design()).unwrap();
    match home {
        Home::Model => {
            ir.model.parameters.push(crate::features::DesignParameter {
                product_definition: None,
                id: parameter.clone(),
                owner: None,
                ordinal: 0,
                name: "Mass".into(),
                expression: "1".into(),
                display: None,
                value: None,
                dependencies: crate::features::DistinctMembers::default(),
                properties: BTreeMap::new(),
                pmi: None,
                native_ref: None,
            });
            ir.model
                .configurations
                .push(crate::features::DesignConfiguration {
                    id: configuration.clone(),
                    ordinal: 0,
                    active: true,
                    source_index: None,
                    name: Some("Large".into()),
                    material: None,
                    properties: BTreeMap::new(),
                    parameter_overrides: BTreeMap::new(),
                    bodies: None,
                    parameter_values: BTreeMap::new(),
                    feature_states: BTreeMap::new(),
                    native_ref: None,
                });
        }
        Home::Design => {
            ir.design
                .parameters
                .push(crate::design::parameter::DesignParameter {
                    id: parameter.clone(),
                    scope: scope.clone(),
                    owner: None,
                    name: None,
                    kind: crate::design::units::ValueKind::Text,
                    unit: None,
                    definition: crate::design::parameter::ParameterDefinition::Driven,
                    value: None,
                    display: None,
                    tolerance: None,
                    decimals: None,
                    comment: None,
                    native_options: BTreeMap::new(),
                    source_settings: BTreeMap::new(),
                    native_ref: None,
                });
            ir.design
                .configurations
                .push(crate::design::configuration::DesignConfiguration {
                    id: configuration.clone(),
                    scope,
                    name: "Large".into(),
                    parent: None,
                    comment: None,
                    overrides: crate::design::configuration::Overrides::default(),
                    active_arrangements: BTreeSet::new(),
                    materials: Vec::new(),
                    native_ref: None,
                });
        }
        Home::Neither => {}
    }
    ir.model.attributes.push(SourceAttribute {
        id: AttributeId::mint("test:model:attribute#part-number").unwrap(),
        target: AttributeTarget::Configuration(configuration).into(),
        name: "Part Number".into(),
        values: vec![Some(AttributeValue::Parameter(parameter))],
        native_ref: None,
    });
    validate_neutral(&ir, Vec::new())
        .unwrap()
        .findings
        .into_iter()
        .map(|finding| finding.message)
        .filter(|message| message.starts_with("references missing"))
        .collect()
}

#[test]
fn an_attribute_resolves_a_configuration_and_a_parameter_in_either_home() {
    for home in [Home::Model, Home::Design] {
        assert_eq!(missing(home), Vec::<String>::new(), "{home:?}");
    }
    assert_eq!(
        missing(Home::Neither),
        [
            "references missing parameter `test:design:parameter#mass`",
            "references missing configuration `test:design:configuration#large`",
        ]
    );
}
