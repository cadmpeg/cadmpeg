// SPDX-License-Identifier: Apache-2.0
//! Dense neutral ranks preserve native source order and dependency identities.

use cadmpeg_ir::features::{DesignParameter, Feature, ParameterValue};
use cadmpeg_ir::{Codec, DecodeOptions};
use std::io::Cursor;

fn feature<'a>(ir: &'a cadmpeg_ir::CadIr, object: &str) -> &'a Feature {
    let native = crate::native::native_id("object", object);
    ir.model
        .features
        .iter()
        .find(|feature| feature.native_ref.as_deref() == Some(native.as_str()))
        .unwrap()
}

#[test]
fn sparse_interleaved_objects_get_dense_dependency_first_feature_ranks() {
    let document = r#"<Document SchemaVersion="4" FileVersion="1"><Objects Count="7" Dependencies="1">
<ObjectDeps Name="Noise0" Count="0"/>
<ObjectDeps Name="Consumer" Count="1"><Dep Name="Driver"/></ObjectDeps>
<ObjectDeps Name="Noise2" Count="0"/><ObjectDeps Name="Driver" Count="0"/>
<ObjectDeps Name="Independent" Count="0"/><ObjectDeps Name="Noise5" Count="0"/><ObjectDeps Name="Tail" Count="0"/>
<Object name="Noise0" type="App::Feature"/><Object name="Consumer" type="PartDesign::Feature"/>
<Object name="Noise2" type="App::Feature"/><Object name="Driver" type="PartDesign::Feature"/>
<Object name="Independent" type="PartDesign::Feature"/><Object name="Noise5" type="App::Feature"/><Object name="Tail" type="PartDesign::Feature"/>
</Objects><ObjectData Count="7"><Object name="Noise0"><Properties Count="0"/></Object>
<Object name="Consumer"><Properties Count="0"/></Object><Object name="Noise2"><Properties Count="0"/></Object>
<Object name="Driver"><Properties Count="0"/></Object><Object name="Independent"><Properties Count="0"/></Object>
<Object name="Noise5"><Properties Count="0"/></Object><Object name="Tail"><Properties Count="0"/></Object>
</ObjectData></Document>"#;
    let bytes = crate::test_support::test_archive::archive(document);
    let decoded = crate::FcstdCodec
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .unwrap();
    let ir = decoded.ir();
    assert_eq!(ir.model.features.len(), 4);
    for (name, rank) in [
        ("Driver", 0),
        ("Consumer", 1),
        ("Independent", 2),
        ("Tail", 3),
    ] {
        assert_eq!(feature(ir, name).ordinal, rank, "{name}");
    }
    let consumer = feature(ir, "Consumer");
    let driver = feature(ir, "Driver");
    // #209 D26: stored results have no regeneration definition operands.
    // ObjectDeps still determines source history order and remains native.
    for feature in [consumer, driver] {
        assert!(matches!(
            feature.evaluation.definition().operation(),
            cadmpeg_ir::features::FeatureOperation::StoredGeometry {}
        ));
        assert!(feature.dependencies.is_empty());
    }
    let native = ir
        .native
        .namespace("fcstd")
        .unwrap()
        .arena_as::<crate::native::ObjectRecord>("objects")
        .unwrap();
    assert_eq!(native.len(), 7);
    assert_eq!(
        native
            .iter()
            .find(|object| object.name() == "Consumer")
            .unwrap()
            .dependencies,
        [crate::native::native_id("object", "Driver")]
    );
    for (name, original) in [
        ("Consumer", 1),
        ("Driver", 3),
        ("Independent", 4),
        ("Tail", 6),
    ] {
        assert_eq!(
            native
                .iter()
                .find(|object| object.name() == name)
                .unwrap()
                .order,
            original
        );
    }
}

#[test]
fn sparse_parameter_source_positions_become_dense_within_each_owner_after_dependencies() {
    let document = r#"<Document SchemaVersion="4" FileVersion="1"><Objects Count="5">
<Object name="Noise" type="App::Feature"/><Object name="A" type="PartDesign::Feature"/>
<Object name="Vars" type="App::VarSet"/><Object name="B" type="PartDesign::Feature"/><Object name="Tail" type="App::Feature"/>
</Objects><ObjectData Count="5"><Object name="Noise"><Properties Count="0"/></Object>
<Object name="A"><Properties Count="5">
<Property name="Label" type="App::PropertyString"><String value="A"/></Property>
<Property name="AForward" type="App::PropertyFloat" group="Custom"><Float value="20"/></Property>
<Property name="Visibility" type="App::PropertyBool"><Bool value="true"/></Property>
<Property name="ADriver" type="App::PropertyFloat" group="Custom"><Float value="19"/></Property>
<Property name="ExpressionEngine" type="App::PropertyExpressionEngine"><ExpressionEngine count="1"><Expression path="AForward" expression="ADriver + 1"/></ExpressionEngine></Property>
</Properties></Object><Object name="Vars"><Properties Count="5">
<Property name="Label" type="App::PropertyString"><String value="Vars"/></Property>
<Property name="GForward" type="App::PropertyFloat"><Float value="40"/></Property>
<Property name="Visibility" type="App::PropertyBool"><Bool value="true"/></Property>
<Property name="GDriver" type="App::PropertyFloat"><Float value="20"/></Property>
<Property name="ExpressionEngine" type="App::PropertyExpressionEngine"><ExpressionEngine count="1"><Expression path="GForward" expression="GDriver * 2"/></ExpressionEngine></Property>
</Properties></Object><Object name="B"><Properties Count="5">
<Property name="Label" type="App::PropertyString"><String value="B"/></Property>
<Property name="BForward" type="App::PropertyFloat" group="Custom"><Float value="31"/></Property>
<Property name="Visibility" type="App::PropertyBool"><Bool value="true"/></Property>
<Property name="BDriver" type="App::PropertyFloat" group="Custom"><Float value="30"/></Property>
<Property name="ExpressionEngine" type="App::PropertyExpressionEngine"><ExpressionEngine count="1"><Expression path="BForward" expression="BDriver + 1"/></ExpressionEngine></Property>
</Properties></Object><Object name="Tail"><Properties Count="0"/></Object></ObjectData></Document>"#;
    let decoded = crate::FcstdCodec
        .decode(
            &mut Cursor::new(crate::test_support::test_archive::archive(document)),
            &DecodeOptions::default(),
        )
        .unwrap();
    let ir = decoded.ir();
    assert_eq!(ir.model.parameters.len(), 6);
    let parameter = |name: &str| -> &DesignParameter {
        ir.model
            .parameters
            .iter()
            .find(|parameter| parameter.name == name)
            .unwrap()
    };
    for (owner, prefix, driver_value, forward_value) in [
        (Some("A"), "A", 19.0, 20.0),
        (None, "G", 20.0, 40.0),
        (Some("B"), "B", 30.0, 31.0),
    ] {
        let driver = parameter(&format!("{prefix}Driver"));
        let forward = parameter(&format!("{prefix}Forward"));
        assert_eq!(driver.ordinal, 0);
        assert_eq!(forward.ordinal, 1);
        assert_eq!(
            driver.owner.as_ref(),
            owner.map(|name| &feature(ir, name).id)
        );
        assert_eq!(forward.owner, driver.owner);
        assert!(driver.expression.dependencies().next().is_none());
        assert_eq!(
            forward.expression.dependencies().collect::<Vec<_>>(),
            [&driver.id]
        );
        assert_eq!(
            driver.value,
            Some(ParameterValue::Real(
                cadmpeg_ir::scalar::FiniteReal::new(driver_value).unwrap()
            ))
        );
        assert_eq!(
            forward.value,
            Some(ParameterValue::Real(
                cadmpeg_ir::scalar::FiniteReal::new(forward_value).unwrap()
            ))
        );
    }
    let properties = ir
        .native
        .namespace("fcstd")
        .unwrap()
        .arena_as::<crate::native::PropertyRecord>("properties")
        .unwrap();
    for object in ["A", "Vars", "B"] {
        let owner = crate::native::native_id("object", object);
        let source = properties
            .iter()
            .filter(|property| property.owner == owner)
            .collect::<Vec<_>>();
        assert_eq!(source.len(), 5);
        assert_eq!(
            source
                .iter()
                .find(|property| property.name.ends_with("Forward"))
                .unwrap()
                .order,
            1
        );
        assert_eq!(
            source
                .iter()
                .find(|property| property.name.ends_with("Driver"))
                .unwrap()
                .order,
            3
        );
    }
}
