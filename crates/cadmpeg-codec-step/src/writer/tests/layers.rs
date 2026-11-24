// SPDX-License-Identifier: Apache-2.0
//! STEP writer presentation-layer tests.

use std::io::Cursor;

use cadmpeg_ir::codec::{Codec, DecodeOptions};
use cadmpeg_ir::examples::unit_cube;

use crate::export::write_step;
use crate::loss::StepLossCode;
use crate::{StepCodec, StepSchema, StepWriteOptions};

/// STEP has no layer hierarchy: a sublayer is written as a top-level layer,
/// hidden where a layer above it is hidden, and the writer reports the
/// flattening.
#[test]
fn a_sublayer_of_a_hidden_layer_is_written_hidden() {
    use cadmpeg_ir::ids::LayerId;
    use cadmpeg_ir::presentation::{PresentationItem, PresentationLayer};

    let mut ir = unit_cube().expect("unit cube fixture is admitted");
    let body = ir.geometry.bodies[0].id.clone();
    let parent = LayerId::mint("test:model:layer#parent").expect("identity grammar");
    ir.model.presentation_layers.push(PresentationLayer {
        id: parent.clone(),
        name: "parent".into(),
        parent: None,
        description: None,
        visible: Some(false),
        items: Vec::new(),
        native_ref: None,
    });
    ir.model.presentation_layers.push(PresentationLayer {
        id: LayerId::mint("test:model:layer#child").expect("identity grammar"),
        name: "child".into(),
        parent: Some(parent),
        description: None,
        visible: None,
        items: vec![PresentationItem::Body { body }.into()],
        native_ref: None,
    });

    let mut output = Vec::new();
    let report = write_step(
        &ir,
        &mut output,
        StepSchema::Ap214,
        &StepWriteOptions::default(),
    )
    .expect("write a sublayer");
    assert!(report
        .losses
        .iter()
        .any(|loss| loss.code == StepLossCode::LayerHierarchyFlattened.kind()));
    let text = String::from_utf8(output).expect("STEP output is UTF-8");
    let decoded = StepCodec::default()
        .decode(&mut Cursor::new(text), &DecodeOptions::default())
        .expect("decode the flattened layer");
    let child = decoded
        .ir()
        .model
        .presentation_layers
        .iter()
        .find(|layer| layer.name == "child")
        .expect("decoded sublayer");
    assert_eq!(child.visible, Some(false));
    assert_eq!(child.parent, None);
}
