// SPDX-License-Identifier: Apache-2.0

use super::*;

#[test]
fn assembly_joints_and_linked_prototypes_are_not_occurrences() {
    let document = r#"<Document SchemaVersion="4" FileVersion="1">
<Objects Count="5">
 <Object type="Assembly::AssemblyObject" name="Assembly" id="1"/>
 <Object type="Assembly::JointGroup" name="Joints" id="2"/>
 <Object type="App::FeaturePython" name="Ground" id="3"/>
 <Object type="App::Link" name="BlockLink" id="4"/>
 <Object type="Part::Feature" name="Block" id="5"/>
</Objects>
<ObjectData Count="5">
 <Object name="Assembly"><Properties Count="1">
  <Property name="Group" type="App::PropertyLinkList"><LinkList count="3"><Link value="Joints"/><Link value="BlockLink"/><Link value="Ground"/></LinkList></Property>
 </Properties></Object>
 <Object name="Joints"><Properties Count="1">
  <Property name="Group" type="App::PropertyLinkList"><LinkList count="1"><Link value="Ground"/></LinkList></Property>
 </Properties></Object>
 <Object name="Ground"><Properties Count="1">
  <Property name="ObjectToGround" type="App::PropertyLinkGlobal"><Link value="BlockLink"/></Property>
 </Properties></Object>
 <Object name="BlockLink"><Properties Count="1">
  <Property name="LinkedObject" type="App::PropertyXLink"><XLink file="" stamp="" name="Block"/></Property>
 </Properties></Object>
 <Object name="Block"><Properties Count="0"/></Object>
</ObjectData></Document>"#;
    let result = FcstdCodec
        .decode(
            &mut Cursor::new(archive(document)),
            &DecodeOptions::default(),
        )
        .expect("assembly");
    let model = &result.ir().model;
    let occurrences = model
        .occurrences
        .iter()
        .map(|occurrence| occurrence.id.as_str())
        .collect::<Vec<_>>();
    assert_eq!(
        occurrences,
        [
            "fcstd:model:occurrence#Assembly:container",
            "fcstd:model:occurrence#BlockLink:instance"
        ]
    );
    let definitions = model
        .product_definitions
        .iter()
        .map(|definition| definition.id.as_str())
        .collect::<HashSet<_>>();
    assert_eq!(
        definitions,
        HashSet::from([
            "fcstd:model:product_definition#Assembly:definition",
            "fcstd:model:product_definition#Block:definition"
        ])
    );
    let connectors = model.assembly_joints[0].connectors().collect::<Vec<_>>();
    assert!(matches!(
        &connectors[0].operand.container,
        cadmpeg_ir::OperandContainer::Occurrence { occurrence, .. }
            if occurrence.as_str() == "fcstd:model:occurrence#BlockLink:instance"
    ));
    assert_valid_document(result.ir());
}
