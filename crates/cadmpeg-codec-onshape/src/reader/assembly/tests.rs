// SPDX-License-Identifier: Apache-2.0
use super::merge_design;
use cadmpeg_ir::design::names::{NameEntry, NativeName, NativeNameValue};
use cadmpeg_ir::design::refs::EntityRef;
use cadmpeg_ir::document::Design;
use std::collections::BTreeMap;

fn design(native: &str) -> Design {
    let mut design = Design::default();
    let body = super::id("body", native).try_into().unwrap();
    let entry = NameEntry {
        name: None,
        native: Some(NativeName {
            codec: cadmpeg_ir::codec_id!("onshape"),
            value: NativeNameValue::Text(cadmpeg_core::text::NonBlankString::new(native).unwrap()),
        }),
    };
    design.names = BTreeMap::from([(EntityRef::Body(body), entry)]).into();
    design
}
#[test]
fn typed_design_append_preserves_each_name_table_entry() {
    let mut target = design("left");
    let source = design("right");
    let expected: BTreeMap<_, _> = target
        .names
        .entries()
        .iter()
        .chain(source.names.entries())
        .map(|(entity, name)| (entity.clone(), name.clone()))
        .collect();
    merge_design(&mut target, source);
    assert_eq!(target.names.entries(), &expected);
    merge_design(&mut target, Design::default());
    assert_eq!(target.names.entries(), &expected);
}
