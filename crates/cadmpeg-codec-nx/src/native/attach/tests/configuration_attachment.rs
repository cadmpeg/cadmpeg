// SPDX-License-Identifier: Apache-2.0

use cadmpeg_ir::annotations::StreamHandle;
use cadmpeg_ir::{AnnotationBuilder, CadIr};

#[test]
fn configuration_attachment_preserves_active_relation() {
    let ir = {
        let mut ir = CadIr::empty();
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
    assert_eq!(configuration.name.as_deref(), Some("Primary"));
    assert_eq!(
        configuration
            .properties
            .get("active_attribute_use")
            .map(String::as_str),
        Some("nx:arrangements:attribute-use#0")
    );
}
