// SPDX-License-Identifier: Apache-2.0

use std::io::Cursor;

use cadmpeg_ir::design::container::{Container, OrderedKind, Rollback};
use cadmpeg_ir::design::feature::{FeatureOperation, OperationFamily};
use cadmpeg_ir::design::refs::{Member, Position};
use cadmpeg_ir::{Codec, DecodeOptions};

use crate::test_support::test_archive::{archive, assert_valid_document};
use crate::FcstdCodec;

const DESIGN_HISTORY: &[u8] = include_bytes!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../corpus/freecad_fcstd/fixtures/design_history.FCStd"
));

fn feature(id: &str) -> Member {
    Member::Feature(cadmpeg_ir::features::FeatureId::mint(id).unwrap())
}

#[test]
fn a_partdesign_body_is_an_ordered_body_container_at_its_tip() {
    let decoded = FcstdCodec
        .decode(&mut Cursor::new(DESIGN_HISTORY), &DecodeOptions::default())
        .unwrap();
    let ir = decoded.ir();
    assert_valid_document(ir);
    let design = &ir.design;
    let [scope] = design.scopes.as_slice() else {
        panic!("one scope: {:?}", design.scopes);
    };
    let root = ir
        .model
        .product_definitions
        .iter()
        .find(|definition| definition.is_root())
        .unwrap();
    assert_eq!(scope.definition, root.id);
    let body = design
        .containers
        .iter()
        .find(|container| container.id().as_str() == "fcstd:design:container#Body")
        .unwrap();
    let Container::Ordered {
        kind,
        members,
        rollback,
        ..
    } = body
    else {
        panic!("{body:?}");
    };
    assert_eq!(*kind, OrderedKind::Body);
    assert_eq!(
        members.as_slice(),
        [
            "fcstd:design:feature#PadSketch",
            "fcstd:design:feature#Pad",
            "fcstd:design:feature#PocketSketch",
            "fcstd:design:feature#Pocket",
            "fcstd:design:feature#Fillet",
        ]
        .map(feature)
    );
    assert_eq!(*rollback, Rollback::End);
    assert_eq!(scope.roots.as_slice(), [body.id().clone()]);
    assert_eq!(design.containers.len(), 1);
    let [current] = scope.current.as_slice() else {
        panic!("{:?}", scope.current);
    };
    assert_eq!(
        current.at,
        Position::After(feature("fcstd:design:feature#Fillet"))
    );
    assert_eq!(current.bodies, root.bodies);
    assert!(current.bodies[0]
        .as_str()
        .starts_with("fcstd:model:body#Body:Shape:"));
    let sketch = design
        .features
        .iter()
        .find(|feature| feature.id.as_str() == "fcstd:design:feature#PadSketch")
        .unwrap();
    assert_eq!(
        sketch.definition.operation,
        FeatureOperation::Unresolved {
            family: OperationFamily::Sketch
        }
    );
}

#[test]
fn a_tip_before_the_last_member_rolls_the_body_back() {
    let document = r#"<Document SchemaVersion="4" FileVersion="1">
<Objects Count="3">
 <Object type="PartDesign::Body" name="Body" id="1"/>
 <Object type="Part::Feature" name="First" id="2"/>
 <Object type="Part::Feature" name="Second" id="3"/>
</Objects>
<ObjectData Count="3">
 <Object name="Body"><Properties Count="2">
  <Property name="Group" type="App::PropertyLinkList"><LinkList count="2"><Link value="First"/><Link value="Second"/></LinkList></Property>
  <Property name="Tip" type="App::PropertyLink"><Link value="First"/></Property>
 </Properties></Object>
 <Object name="First"><Properties Count="0"/></Object>
 <Object name="Second"><Properties Count="0"/></Object>
</ObjectData></Document>"#;
    let decoded = FcstdCodec
        .decode(
            &mut Cursor::new(archive(document)),
            &DecodeOptions::default(),
        )
        .unwrap();
    let ir = decoded.ir();
    assert_valid_document(ir);
    let Some(Container::Ordered { rollback, .. }) = ir
        .design
        .containers
        .iter()
        .find(|container| container.id().as_str() == "fcstd:design:container#Body")
    else {
        panic!("{:?}", ir.design.containers);
    };
    assert_eq!(
        *rollback,
        Rollback::Before(feature("fcstd:design:feature#Second"))
    );
    assert_eq!(
        ir.design.scopes[0].current[0].at,
        Position::After(feature("fcstd:design:feature#First"))
    );
}

#[test]
fn a_geometry_decode_states_the_root_and_no_design() {
    let decoded = FcstdCodec
        .decode(
            &mut Cursor::new(DESIGN_HISTORY),
            &DecodeOptions {
                depth: cadmpeg_core::decode::DecodeDepth::Geometry,
                ..DecodeOptions::default()
            },
        )
        .unwrap();
    let ir = decoded.ir();
    assert!(ir.design.scopes.is_empty());
    assert!(ir.design.features.is_empty());
    let root = ir
        .model
        .product_definitions
        .iter()
        .find(|definition| definition.is_root())
        .unwrap();
    assert_eq!(root.bodies.len(), 1);
}

#[test]
fn an_object_outside_every_body_has_no_design_record() {
    let document = r#"<Document SchemaVersion="4" FileVersion="1">
<Objects Count="1"><Object type="Part::Feature" name="Stored" id="1"/></Objects>
<ObjectData Count="1"><Object name="Stored"><Properties Count="0"/></Object></ObjectData>
</Document>"#;
    let decoded = FcstdCodec
        .decode(
            &mut Cursor::new(archive(document)),
            &DecodeOptions::default(),
        )
        .unwrap();
    let design = &decoded.ir().design;
    assert!(design.scopes.is_empty());
    assert!(design.containers.is_empty());
    assert!(design.features.is_empty());
    assert!(design.parameters.is_empty());
}
