// SPDX-License-Identifier: Apache-2.0

use cadmpeg_core::decode::{DecodePolicy, ResourceDimension};
use cadmpeg_core::CodecError;
use cadmpeg_ir::annotations::StreamHandle;
use cadmpeg_ir::features::{DesignParameter, DistinctMembers, ParameterId, ParameterValue};
use cadmpeg_ir::{AnnotationBuilder, CadIr};

fn attach_one_configuration(
    configure: impl FnOnce(&mut DecodePolicy),
) -> Result<CadIr, CodecError> {
    crate::test_support::with_decode_context_over(
        &[],
        |policy| {
            configure(policy);
        },
        |ctx| {
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
            let stream = StreamHandle::new(
                &cadmpeg_test_support::service_decode_context(),
                cadmpeg_ir::stream_name!("nx:container"),
                "fixture stream handle",
            )
            .unwrap();
            super::super::attach_configurations(
                ctx,
                &mut ir,
                std::iter::once((
                    "nx:arrangements:configuration#0",
                    "Primary",
                    0,
                    Some("nx:arrangements:attribute-use#0"),
                )),
                &mut annotations,
                &stream,
            )?;
            Ok(ir)
        },
    )
}

#[test]
fn configuration_attachment_preserves_active_relation() {
    let ir = attach_one_configuration(|_| {}).unwrap();
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
fn configuration_attachment_refuses_collection_limit() {
    let error =
        attach_one_configuration(|policy| policy.limits.max_collection_items = 0).unwrap_err();
    assert!(
        matches!(error, CodecError::ResourceLimit(limit) if limit.dimension == ResourceDimension::CollectionItems)
    );
}

#[test]
fn configuration_attachment_refuses_retained_limit() {
    let error =
        attach_one_configuration(|policy| policy.limits.max_retained_bytes = 0).unwrap_err();
    assert!(
        matches!(error, CodecError::ResourceLimit(limit) if limit.dimension == ResourceDimension::RetainedBytes)
    );
}
