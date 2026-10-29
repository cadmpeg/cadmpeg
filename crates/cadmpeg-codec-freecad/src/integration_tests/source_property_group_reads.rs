// SPDX-License-Identifier: Apache-2.0
//! Kept local property group strings have genuine source reads and parameter flow.

use cadmpeg_ir::{Codec, DecodeOptions};
use std::io::Cursor;

#[test]
fn current_group_and_documentation_fields_keep_exact_source_ownership_and_dynamic_parameter() {
    let property = "<Property name='CustomMeasure' type='App::PropertyFloat' group='Custom&amp;Parameters' doc=' Ω &amp; β ' Vendor='opaque-extra'><Float value='7.5'/></Property>";
    let document = format!("<Document SchemaVersion='4' FileVersion='1'><Objects Count='1'><Object name='Owner' type='PartDesign::Feature'/></Objects><ObjectData Count='1'><Object name='Owner'><Properties Count='1'>{property}</Properties></Object></ObjectData></Document>");
    let decoded = crate::FcstdCodec
        .decode(
            &mut Cursor::new(crate::test_support::test_archive::archive(&document)),
            &DecodeOptions::default(),
        )
        .unwrap();
    let properties = decoded
        .ir()
        .native
        .namespace("fcstd")
        .unwrap()
        .arena_as::<crate::native::PropertyRecord>("properties")
        .unwrap();
    let [native] = properties.as_slice() else {
        panic!("one property: {properties:?}")
    };
    assert!(matches!(
        &native.body,
        Some(crate::native::PropertyBody::Persisted { .. })
    ));
    let Some(metadata) = &native.dynamic else {
        panic!("actual typed metadata")
    };
    assert_eq!(metadata.group, "Custom&Parameters");
    assert_eq!(metadata.documentation.as_deref(), Some(" Ω & β "));
    let parameter = decoded
        .ir()
        .model
        .parameters
        .iter()
        .find(|parameter| parameter.native_ref.as_deref() == Some(native.id.as_str()))
        .expect("current dynamic parameter");
    assert_eq!(parameter.name, "CustomMeasure");
    assert_eq!(
        parameter.value,
        Some(cadmpeg_ir::features::ParameterValue::Real(
            cadmpeg_ir::scalar::FiniteReal::new(7.5).unwrap()
        ))
    );
    assert!(parameter.owner.is_some());
}
