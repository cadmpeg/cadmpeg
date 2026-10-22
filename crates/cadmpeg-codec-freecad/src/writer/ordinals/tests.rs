// SPDX-License-Identifier: Apache-2.0
//! Order metadata remains typed content after native declaration reordering.

use std::collections::HashMap;
use std::io::Cursor;

use cadmpeg_ir::{Codec, DecodeOptions};

use super::super::xml::{self, Object};
use crate::FcstdCodec;

fn objects() -> Vec<Object> {
    let mut object = Object::new("Box".into(), "Part::Box", "Box");
    for (name, value) in [("Length", 9), ("Width", 8), ("Height", 7)] {
        object.set(
            name,
            xml::scalar(name, "App::PropertyLength", "Float", value),
        );
    }
    vec![object]
}

fn decode(
    objects: &[Object],
) -> Result<cadmpeg_ir::codec::DecodeResult, cadmpeg_ir::codec::DecodeFailure> {
    let document = xml::document(objects, &std::collections::BTreeMap::default(), "4");
    FcstdCodec.decode(
        &mut Cursor::new(crate::test_support::test_archive::archive(&document)),
        &DecodeOptions::default(),
    )
}

#[test]
fn typed_metadata_preserves_feature_and_parameter_ordinals_and_absent_source_tag() {
    let mut objects = objects();
    let mut ir = decode(&objects).expect("primitive document").into_parts().0;
    ir.model.features[0].ordinal = u64::MAX;
    ir.model.features[0].source_tag = None;
    let parameter = ir
        .model
        .parameters
        .iter_mut()
        .find(|parameter| parameter.name == "Length")
        .expect("length parameter");
    parameter.ordinal = u32::MAX;
    let names = HashMap::from([(ir.model.features[0].id.to_string(), "Box".to_owned())]);
    super::preserve(&ir, &mut objects, &names).expect("typed order properties");
    let decoded = decode(&objects).expect("order metadata document");
    assert_eq!(decoded.ir().model.features[0].ordinal, u64::MAX);
    assert_eq!(decoded.ir().model.features[0].source_tag, None);
    assert_eq!(
        decoded.ir().model.parameters.len(),
        ir.model.parameters.len()
    );
    for expected in &ir.model.parameters {
        let actual = decoded
            .ir()
            .model
            .parameters
            .iter()
            .find(|parameter| parameter.name == expected.name)
            .expect("parameter survives");
        assert_eq!(actual.ordinal, expected.ordinal);
        assert_eq!(actual.expression, expected.expression);
        assert_eq!(actual.value, expected.value);
    }
}

#[test]
fn malformed_order_metadata_is_a_document_precondition() {
    let mut objects = objects();
    super::scalar(&mut objects[0], super::FEATURE, "-1");
    let failure = decode(&objects).expect_err("feature ordinals are unsigned");
    assert!(matches!(
        failure,
        cadmpeg_ir::codec::DecodeFailure::Codec(cadmpeg_core::CodecError::Malformed(_))
    ));
}
