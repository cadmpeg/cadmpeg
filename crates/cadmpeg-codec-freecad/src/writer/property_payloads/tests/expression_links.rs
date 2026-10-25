// SPDX-License-Identifier: Apache-2.0
//! Sparse expression link metadata is normalized from actual held link ordinals.

use super::*;

#[test]
fn source_less_expression_metadata_keeps_authored_null_and_renumbers_only_held_links() {
    let document=b"<Document SchemaVersion='4' FileVersion='1'><Objects Count='2'><Object name='A' type='App::Feature'/><Object name='B' type='App::Feature'/></Objects><ObjectData Count='2'><Object name='A'><Properties Count='1'><Property name='Expressions' type='App::PropertyExpressionEngine'><ExpressionEngine count='0'><XLinks count='4' docs='1' hidden='1 2 3'><DocMap name='Current' label='Original label' index='2'/><XLink name='A' file=''/><XLink name='NeverDeclared' file=''/><XLink name='B' file=''/><XLink name='' file=''/></XLinks></ExpressionEngine></Property></Properties></Object><Object name='B'><Properties Count='0'/></Object></ObjectData></Document>";
    let decoded = FcstdCodec
        .decode(
            &mut Cursor::new(archive_entries(&[("Document.xml", document)])),
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
    let property = properties
        .iter()
        .find(|property| property.name == "Expressions")
        .unwrap();
    assert_eq!(property.links().len(), 4);
    assert!(property.links()[1].is_none());
    assert!(property.links()[3].is_none());
    let original_map = property
        .values()
        .iter()
        .find(|value| value.tag == "DocMap")
        .unwrap();
    assert_eq!(original_map.attributes["index"], "2");
    assert_eq!(
        property
            .values()
            .iter()
            .filter(|value| value.tag == "XLink")
            .map(|value| value.link_index)
            .collect::<Vec<_>>(),
        [Some(0), Some(2), Some(3)]
    );
    let plan = FcstdCodec
        .plan(EncodeInput::new(decoded.ir(), None), TargetRequest::Inherit)
        .unwrap();
    assert!(plan
        .report()
        .losses
        .iter()
        .any(|loss| loss.code.local_code() == "source.undecoded-omitted"));
    let mut bytes = Vec::new();
    plan.write_to(&mut bytes).unwrap();
    let output = FcstdCodec
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .unwrap();
    let properties = output
        .ir()
        .native
        .namespace("fcstd")
        .unwrap()
        .arena_as::<crate::native::PropertyRecord>("properties")
        .unwrap();
    let property = properties
        .iter()
        .find(|property| property.name == "Expressions")
        .unwrap();
    assert_eq!(property.links().len(), 3);
    assert!(property.links()[0].is_some());
    assert!(property.links()[1].is_some());
    assert!(property.links()[2].is_none());
    let container = property
        .values()
        .iter()
        .find(|value| value.tag == "XLinks")
        .unwrap();
    assert_eq!(container.attributes["count"], "3");
    assert_eq!(container.attributes["docs"], "1");
    assert_eq!(container.attributes["hidden"], "1 2");
    let map = property
        .values()
        .iter()
        .find(|value| value.tag == "DocMap")
        .unwrap();
    assert_eq!(map.attributes["index"], "1");
    assert_eq!(map.attributes["label"], "Original label");
    assert!(output
        .source_fidelity()
        .parts()
        .values()
        .all(|part| !matches!(
            part.state(),
            cadmpeg_ir::source_fidelity::parts::PartState::GivenUp { .. }
        )));
}
