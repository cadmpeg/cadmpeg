// SPDX-License-Identifier: Apache-2.0

use std::io::Cursor;

use std::collections::BTreeSet;

use cadmpeg_core::decode::DecodeDepth;
use cadmpeg_ir::design::container::{Container, OrderedKind, Rollback};
use cadmpeg_ir::design::evaluation::EvaluationStatus;
use cadmpeg_ir::design::feature::{FeatureOperation, FuzzyTolerance, OperationFamily};
use cadmpeg_ir::design::refs::{Member, Position};
use cadmpeg_ir::{CadIr, Codec, DecodeOptions};

use crate::test_support::test_archive::{archive, assert_valid_document};
use crate::FcstdCodec;

const DESIGN_HISTORY: &[u8] = include_bytes!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../corpus/freecad_fcstd/fixtures/design_history.FCStd"
));
const FUZZY_STORED: &[u8] = include_bytes!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../corpus/freecad_fcstd/fixtures/named_constraint_1.1.4_N10a.FCStd"
));
const CORE_OPERATIONS: &[u8] = include_bytes!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../corpus/freecad_fcstd/fixtures/core_operations.FCStd"
));
const BINARY_EXACT_SHAPE: &[u8] = include_bytes!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../corpus/freecad_fcstd/fixtures/binary_exact_shape.FCStd"
));

fn decode(bytes: &[u8], depth: DecodeDepth) -> CadIr {
    let decoded = FcstdCodec
        .decode(
            &mut Cursor::new(bytes),
            &DecodeOptions {
                depth,
                ..DecodeOptions::default()
            },
        )
        .unwrap();
    assert_valid_document(decoded.ir());
    decoded.ir().clone()
}

fn root_bodies(ir: &CadIr) -> BTreeSet<String> {
    ir.model
        .product_definitions
        .iter()
        .find(|definition| definition.is_root())
        .unwrap()
        .bodies
        .iter()
        .map(|body| body.as_str().to_owned())
        .collect()
}

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

#[test]
fn a_body_with_no_member_states_no_scope_and_no_container() {
    let document = r#"<Document SchemaVersion="4" FileVersion="1">
<Objects Count="1"><Object type="PartDesign::Body" name="Body" id="1"/></Objects>
<ObjectData Count="1"><Object name="Body"><Properties Count="1">
  <Property name="Group" type="App::PropertyLinkList"><LinkList count="0"/></Property>
</Properties></Object></ObjectData>
</Document>"#;
    let ir = decode(&archive(document), DecodeDepth::Design);
    assert!(ir.design.scopes.is_empty(), "{:?}", ir.design.scopes);
    assert!(ir.design.containers.is_empty());
}

#[test]
fn a_body_freecad_marks_touched_is_stale() {
    let document = r#"<Document SchemaVersion="4" FileVersion="1">
<Objects Count="2">
 <Object type="PartDesign::Body" name="Body" id="1" Touched="1"/>
 <Object type="Part::Feature" name="First" id="2"/>
</Objects>
<ObjectData Count="2">
 <Object name="Body"><Properties Count="2">
  <Property name="Group" type="App::PropertyLinkList"><LinkList count="1"><Link value="First"/></LinkList></Property>
  <Property name="Tip" type="App::PropertyLink"><Link value="First"/></Property>
 </Properties></Object>
 <Object name="First"><Properties Count="0"/></Object>
</ObjectData></Document>"#;
    let stale = decode(&archive(document), DecodeDepth::Design);
    assert_eq!(
        stale.design.scopes[0].current[0].status,
        EvaluationStatus::Stale
    );
    let current = decode(
        &archive(&document.replace(r#" Touched="1""#, "")),
        DecodeDepth::Design,
    );
    assert_eq!(
        current.design.scopes[0].current[0].status,
        EvaluationStatus::Current
    );
}

#[test]
fn a_multitransform_stage_is_no_feature() {
    let document = r#"<Document SchemaVersion="4" FileVersion="1">
<Objects Count="4">
 <Object type="PartDesign::Body" name="Body" id="1"/>
 <Object type="Part::Feature" name="Base" id="2"/>
 <Object type="PartDesign::LinearPattern" name="Stage" id="3"/>
 <Object type="PartDesign::MultiTransform" name="Pattern" id="4"/>
</Objects>
<ObjectData Count="4">
 <Object name="Body"><Properties Count="2">
  <Property name="Group" type="App::PropertyLinkList"><LinkList count="3"><Link value="Base"/><Link value="Stage"/><Link value="Pattern"/></LinkList></Property>
  <Property name="Tip" type="App::PropertyLink"><Link value="Pattern"/></Property>
 </Properties></Object>
 <Object name="Base"><Properties Count="0"/></Object>
 <Object name="Stage"><Properties Count="0"/></Object>
 <Object name="Pattern"><Properties Count="1">
  <Property name="Transformations" type="App::PropertyLinkList"><LinkList count="1"><Link value="Stage"/></LinkList></Property>
 </Properties></Object>
</ObjectData></Document>"#;
    let ir = decode(&archive(document), DecodeDepth::Design);
    let Some(Container::Ordered { members, .. }) = ir.design.containers.first() else {
        panic!("{:?}", ir.design.containers);
    };
    assert_eq!(
        members.as_slice(),
        ["fcstd:design:feature#Base", "fcstd:design:feature#Pattern"].map(feature)
    );
    assert!(ir
        .design
        .features
        .iter()
        .all(|feature| feature.id.as_str() != "fcstd:design:feature#Stage"));
}

#[test]
fn refine_and_fuzzy_tolerance_are_stated_where_stored() {
    let pad = |ir: &CadIr| {
        ir.design
            .features
            .iter()
            .find(|feature| feature.id.as_str() == "fcstd:design:feature#Pad")
            .unwrap()
            .definition
            .clone()
    };
    let refine_only = pad(&decode(DESIGN_HISTORY, DecodeDepth::Design));
    assert!(refine_only.refine.is_some());
    assert_eq!(refine_only.fuzzy_tolerance, None);
    let both = pad(&decode(FUZZY_STORED, DecodeDepth::Design));
    assert!(both.refine.is_some());
    assert!(matches!(
        both.fuzzy_tolerance,
        Some(FuzzyTolerance::KernelDefault | FuzzyTolerance::Automatic)
    ));
}

#[test]
fn a_native_kind_uses_the_codec_id() {
    let FeatureOperation::Native { kind, .. } = super::native("PartDesign::FeatureBase") else {
        panic!();
    };
    assert_eq!(kind.as_str(), "fcstd:PartDesign::FeatureBase");
}

#[test]
fn a_parameter_with_an_empty_expression_is_stated_with_its_stored_value() {
    let document = r#"<Document SchemaVersion="4" FileVersion="1">
<Objects Count="2">
 <Object type="PartDesign::Body" name="Body" id="1"/>
 <Object type="PartDesign::Pad" name="Pad" id="2"/>
</Objects>
<ObjectData Count="2">
 <Object name="Body"><Properties Count="2">
  <Property name="Group" type="App::PropertyLinkList"><LinkList count="1"><Link value="Pad"/></LinkList></Property>
  <Property name="Tip" type="App::PropertyLink"><Link value="Pad"/></Property>
 </Properties></Object>
 <Object name="Pad"><Properties Count="2">
  <Property name="ExpressionEngine" type="App::PropertyExpressionEngine"><ExpressionEngine count="1"><Expression path="Length" expression="EXPRESSION"/></ExpressionEngine></Property>
  <Property name="Length" type="App::PropertyLength"><Float value="20.0000000000000000"/></Property>
 </Properties></Object>
</ObjectData></Document>"#;
    for text in ["", "  "] {
        let decoded = FcstdCodec
            .decode(
                &mut Cursor::new(archive(&document.replace("EXPRESSION", text))),
                &DecodeOptions::default(),
            )
            .unwrap();
        let ir = decoded.ir();
        assert_valid_document(ir);
        let length = ir
            .design
            .parameters
            .iter()
            .find(|parameter| parameter.id.as_str() == "fcstd:design:parameter#Pad:Length")
            .unwrap_or_else(|| panic!("{:?}", ir.design.parameters));
        let wire = serde_json::to_value(&length.definition).unwrap();
        assert_eq!(wire["value"]["kind"], "neutral", "{wire}");
        assert_eq!(wire["value"]["value"]["text"], "20[mm]", "{wire}");
        let empty = crate::loss::FreecadLossCode::ParameterExpressionEmpty.note("");
        assert_eq!(empty.code.to_string(), "fcstd/parameter.expression-empty");
        assert!(
            decoded
                .report()
                .losses
                .iter()
                .any(|loss| loss.code == empty.code
                    && loss.message.contains("fcstd:design:parameter#Pad:Length")),
            "{:?}",
            decoded.report().losses
        );
    }
}

#[test]
fn a_scoped_root_owns_its_current_states_and_unproduced_shapes_at_every_depth() {
    let design = decode(CORE_OPERATIONS, DecodeDepth::Design);
    let [scope] = design.design.scopes.as_slice() else {
        panic!("{:?}", design.design.scopes);
    };
    let current = scope
        .current
        .iter()
        .flat_map(|state| &state.bodies)
        .map(|body| body.as_str().to_owned())
        .collect::<BTreeSet<_>>();
    let unproduced = scope
        .unproduced
        .iter()
        .map(|body| body.as_str().to_owned())
        .collect::<BTreeSet<_>>();
    assert!(!unproduced.is_empty());
    assert!(current.is_disjoint(&unproduced));
    let root = root_bodies(&design);
    assert_eq!(root, &current | &unproduced);
    // A Body member's shape is a state of its Body: no definition owns it.
    assert!(root
        .iter()
        .all(|body| !body.starts_with("fcstd:model:body#Hole:Shape:")));
    assert!(design
        .geometry
        .bodies
        .iter()
        .any(|body| body.id.as_str().starts_with("fcstd:model:body#Hole:Shape:")));
    assert_eq!(
        root_bodies(&decode(CORE_OPERATIONS, DecodeDepth::Geometry)),
        root
    );
}

#[test]
fn an_unscoped_root_owns_every_top_level_shape() {
    let ir = decode(BINARY_EXACT_SHAPE, DecodeDepth::Design);
    assert!(ir.design.scopes.is_empty());
    let bodies = root_bodies(&ir);
    assert!(!bodies.is_empty());
    assert!(bodies
        .iter()
        .all(|body| body.starts_with("fcstd:model:body#BinaryCarrier:Shape:")));
}
