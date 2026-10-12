// SPDX-License-Identifier: Apache-2.0
//! Which product definition owns which body in an assembly.

use std::io::Cursor;

use cadmpeg_ir::codec::{Codec, DecodeOptions};

use crate::StepCodec;

/// A component placement is a `SHAPE_REPRESENTATION_RELATIONSHIP` named by a
/// `CONTEXT_DEPENDENT_SHAPE_REPRESENTATION`. The relationship is oriented from
/// the assembly to the component through the CDSR's `NEXT_ASSEMBLY_USAGE_OCCURRENCE`,
/// so a component does not reach the bodies of the assembly that places it, and
/// the assembly keeps its own.
#[test]
fn an_assembly_component_does_not_absorb_the_assemblys_bodies() {
    let source = String::from_utf8(
        include_bytes!("../../../../tests/fixtures/ap242_geometric_set.p21").to_vec(),
    )
    .expect("fixture is UTF-8")
    .replace(
        "ENDSEC;\nEND-ISO-10303-21;",
        "#14=PRODUCT('ROOT','assembly root that owns the body','',());\n#15=PRODUCT_DEFINITION_FORMATION('','',#14);\n#16=APPLICATION_CONTEXT('mechanical design');\n#17=PRODUCT_DEFINITION_CONTEXT('part definition',#16,'design');\n#18=PRODUCT_DEFINITION('root','',#15,#17);\n#19=PRODUCT_DEFINITION_SHAPE('','',#18);\n#20=SHAPE_DEFINITION_REPRESENTATION(#19,#13);\n#21=PRODUCT('CHILD','component with no geometry of its own','',());\n#22=PRODUCT_DEFINITION_FORMATION('','',#21);\n#23=PRODUCT_DEFINITION('child','',#22,#17);\n#24=PRODUCT_DEFINITION_SHAPE('','',#23);\n#25=SHAPE_REPRESENTATION('',(),#2);\n#26=SHAPE_DEFINITION_REPRESENTATION(#24,#25);\n#27=NEXT_ASSEMBLY_USAGE_OCCURRENCE('u1','','',#18,#23,$);\n#28=PRODUCT_DEFINITION_SHAPE('','',#27);\n#29=CARTESIAN_POINT('',(0.,0.,0.));\n#30=DIRECTION('',(0.,0.,1.));\n#31=DIRECTION('',(1.,0.,0.));\n#32=AXIS2_PLACEMENT_3D('',#29,#30,#31);\n#33=ITEM_DEFINED_TRANSFORMATION('','',#32,#32);\n#34=(REPRESENTATION_RELATIONSHIP('','',#13,#25) REPRESENTATION_RELATIONSHIP_WITH_TRANSFORMATION(#33) SHAPE_REPRESENTATION_RELATIONSHIP());\n#35=CONTEXT_DEPENDENT_SHAPE_REPRESENTATION(#34,#28);\nENDSEC;\nEND-ISO-10303-21;",
    );
    let decoded = StepCodec::default()
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .expect("decode flat assembly");

    let definitions = &decoded.ir().model.product_definitions;
    assert_eq!(definitions.len(), 2, "{definitions:#?}");
    let root = definitions
        .iter()
        .find(|d| d.part_number.as_deref() == Some("ROOT"))
        .expect("root definition");
    let child = definitions
        .iter()
        .find(|d| d.part_number.as_deref() == Some("CHILD"))
        .expect("child definition");

    // The child owns no geometry, so any body it lists was absorbed from the assembly.
    assert!(
        child.bodies.is_empty(),
        "the component absorbed {} body(ies) from the assembly that places it: {:?}",
        child.bodies.len(),
        child.bodies
    );
    // The assembly keeps its own body.
    assert_eq!(
        root.bodies.len(),
        1,
        "the assembly lost its own body: {:?}",
        root.bodies
    );
}
