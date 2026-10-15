// SPDX-License-Identifier: Apache-2.0

use cadmpeg_ir::annotations::StreamHandle;
use cadmpeg_ir::{AnnotationBuilder, CadIr};

#[test]
fn part_attribute_attachment_preserves_value() {
    let ir = {
        let mut ir = CadIr::empty();
        let mut annotations = AnnotationBuilder::new();
        let stream = StreamHandle::new(cadmpeg_ir::stream_name!("nx:container"));
        super::super::attach_part_attributes(
            &mut ir,
            std::iter::once(("nx:part:attribute#0", "Title", "Value", 0)),
            &mut annotations,
            &stream,
        )
        .unwrap();
        ir
    };
    let [attribute] = ir.model.attributes.as_slice() else {
        panic!("one attached part attribute");
    };
    assert_eq!(attribute.name.as_str(), "Title");
    assert_eq!(
        attribute.values.as_slice(),
        [cadmpeg_ir::attributes::AttributeValue::String(
            "Value".into()
        )]
    );
}
