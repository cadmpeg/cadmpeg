// SPDX-License-Identifier: Apache-2.0

use cadmpeg_ir::annotations::StreamHandle;
use cadmpeg_ir::features::{DesignParameter, DistinctMembers, ParameterId, ParameterValue};
use cadmpeg_ir::AnnotationBuilder;

#[test]
fn configuration_attachment_preserves_active_relation() {
    let ir = {
        let mut ir = cadmpeg_ir::examples::unit_cube().unwrap();
        ir.model.parameters.push(DesignParameter {
            id: ParameterId::mint("test:model:parameter#global").unwrap(),
            owner: None,
            ordinal: 0,
            name: "Global".to_owned(),
            expression: "7.5".into(),
            display: None,
            value: Some(ParameterValue::Real(
                cadmpeg_ir::scalar::FiniteReal::new(7.5).unwrap(),
            )),
            dependencies: DistinctMembers::default(),
            properties: std::collections::BTreeMap::default(),
            pmi: None,
            native_ref: None,
        });
        let mut annotations = AnnotationBuilder::new();
        let stream = StreamHandle::new(cadmpeg_ir::stream_name!("nx:container"));
        super::super::attach_configurations(
            &mut ir,
            std::iter::once((
                "nx:arrangements:configuration#0",
                "Primary",
                0,
                Some("nx:arrangements:attribute-use#0"),
            )),
            &mut annotations,
            &stream,
        )
        .unwrap();
        ir
    };
    let [configuration] = ir.model.configurations.as_slice() else {
        panic!("one configuration");
    };
    assert!(configuration.active);
    assert!(configuration.bodies.is_none());
    assert!(configuration.parameter_values.is_empty());
    assert!(configuration.parameter_overrides.is_empty());
    assert!(configuration.feature_states.is_empty());
    assert_eq!(ir.model.bodies.len(), 1);
    assert_eq!(ir.model.parameters.len(), 1);
    assert_eq!(
        ir.model.parameters[0].value,
        Some(ParameterValue::Real(
            cadmpeg_ir::scalar::FiniteReal::new(7.5).unwrap()
        ))
    );
    assert_eq!(configuration.name.as_deref(), Some("Primary"));
    assert_eq!(
        configuration
            .properties
            .get("active_attribute_use")
            .map(String::as_str),
        Some("nx:arrangements:attribute-use#0")
    );
}

#[test]
fn configuration_attribute_uses_keep_the_first_relation_per_configuration() {
    let configurations = (0..4096)
        .map(|index| format!("nx:arrangements:configuration#{index}"))
        .collect::<Vec<_>>();
    let relations = (0..3 * 4096)
        .map(|index| format!("nx:arrangements:attribute-use#{index}"))
        .collect::<Vec<_>>();
    let pairs = relations
        .iter()
        .enumerate()
        .map(|(index, relation)| {
            (
                configurations[(index * 7) % configurations.len()].as_str(),
                relation.as_str(),
            )
        })
        .collect::<Vec<_>>();

    let first = super::super::first_configuration_attribute_uses(pairs.iter().copied());

    assert_eq!(first.len(), configurations.len());
    for configuration in &configurations {
        let expected = pairs
            .iter()
            .find(|(candidate, _)| candidate == configuration)
            .map(|(_, relation)| *relation);
        assert_eq!(first.get(configuration.as_str()).copied(), expected);
    }
}
