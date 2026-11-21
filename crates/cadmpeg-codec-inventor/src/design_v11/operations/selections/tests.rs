// SPDX-License-Identifier: Apache-2.0

use super::*;
use cadmpeg_ir::sketches::{SketchEntityId, SketchId};

#[test]
fn selected_path_curves_keep_source_order_at_the_input_state() {
    let sketch = SketchId::mint("test:design:sketch#path").expect("sketch");
    let curves = ["last", "first", "middle"]
        .map(|name| SketchEntityId::mint(format!("test:design:entity#{name}")).expect("entity"));
    let source = old::PathRef::SketchCurves {
        sketch,
        curves: curves.to_vec().try_into().expect("curves"),
    };
    let selected = path(&source).expect("selected path");
    assert_eq!(selected.at, Anchor::Input);
    let Picks::Named(members) = selected.picks else {
        panic!("named path members");
    };
    assert_eq!(
        members
            .as_slice()
            .iter()
            .map(|member| member.target.clone())
            .collect::<Vec<_>>(),
        curves
            .iter()
            .cloned()
            .map(PickTarget::SketchEntity)
            .collect::<Vec<_>>()
    );
    assert!(selected.native.is_none());
}

#[test]
fn native_path_keeps_the_complete_operand_at_the_input_state() {
    let text = "inventor:pmdc:property#path";
    let selected = path(&old::PathRef::Native(text.to_owned())).expect("native path");
    assert_eq!(selected.at, Anchor::Input);
    assert!(matches!(selected.picks, Picks::Unread));
    assert_eq!(
        selected.native.expect("native reference").as_slice(),
        [text]
    );
}
