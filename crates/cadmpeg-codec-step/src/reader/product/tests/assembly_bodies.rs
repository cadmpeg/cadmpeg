// SPDX-License-Identifier: Apache-2.0
//! Which product definition owns which body in an assembly.

use std::io::Cursor;

use cadmpeg_ir::codec::{Codec, DecodeOptions};
use cadmpeg_ir::products::ProductDefinition;

use crate::StepCodec;

const TRANSFORM: &str = "#29=CARTESIAN_POINT('',(0.,0.,0.));\n#30=DIRECTION('',(0.,0.,1.));\n#31=DIRECTION('',(1.,0.,0.));\n#32=AXIS2_PLACEMENT_3D('',#29,#30,#31);\n#33=ITEM_DEFINED_TRANSFORMATION('','',#32,#32);\n";

/// Decodes the surface-set fixture with `entities` appended to its data section.
fn decode_with(entities: &str) -> Vec<ProductDefinition> {
    let source = String::from_utf8(
        include_bytes!("../../../../tests/fixtures/ap242_geometric_set.p21").to_vec(),
    )
    .expect("fixture is UTF-8")
    .replace(
        "ENDSEC;\nEND-ISO-10303-21;",
        &format!("{entities}ENDSEC;\nEND-ISO-10303-21;"),
    );
    StepCodec::default()
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .expect("decode")
        .ir()
        .model
        .product_definitions
        .clone()
}

/// A component placement is a transformed `SHAPE_REPRESENTATION_RELATIONSHIP` named by a
/// `CONTEXT_DEPENDENT_SHAPE_REPRESENTATION`. Here both of its endpoints are representations no
/// definition owns directly: the root reaches the body through `#40 -> #13`, and the child reaches
/// the placement through `#25 -> #50`. The placement is oriented from the assembly to the
/// component through the `NEXT_ASSEMBLY_USAGE_OCCURRENCE`, so the child does not reach the body
/// through it and the root keeps it.
#[test]
fn an_assembly_component_does_not_absorb_the_assemblys_bodies() {
    let definitions = decode_with(&format!(
        "#14=PRODUCT('ROOT','assembly root that owns the body','',());\n\
         #15=PRODUCT_DEFINITION_FORMATION('','',#14);\n\
         #16=APPLICATION_CONTEXT('mechanical design');\n\
         #17=PRODUCT_DEFINITION_CONTEXT('part definition',#16,'design');\n\
         #18=PRODUCT_DEFINITION('root','',#15,#17);\n\
         #19=PRODUCT_DEFINITION_SHAPE('','',#18);\n\
         #20=SHAPE_DEFINITION_REPRESENTATION(#19,#40);\n\
         #40=SHAPE_REPRESENTATION('',(),#2);\n\
         #41=SHAPE_REPRESENTATION_RELATIONSHIP('','',#40,#13);\n\
         #21=PRODUCT('CHILD','component with no geometry of its own','',());\n\
         #22=PRODUCT_DEFINITION_FORMATION('','',#21);\n\
         #23=PRODUCT_DEFINITION('child','',#22,#17);\n\
         #24=PRODUCT_DEFINITION_SHAPE('','',#23);\n\
         #25=SHAPE_REPRESENTATION('',(),#2);\n\
         #26=SHAPE_DEFINITION_REPRESENTATION(#24,#25);\n\
         #50=SHAPE_REPRESENTATION('',(),#2);\n\
         #51=SHAPE_REPRESENTATION_RELATIONSHIP('','',#25,#50);\n\
         #27=NEXT_ASSEMBLY_USAGE_OCCURRENCE('u1','','',#18,#23,$);\n\
         #28=PRODUCT_DEFINITION_SHAPE('','',#27);\n\
         {TRANSFORM}\
         #34=(REPRESENTATION_RELATIONSHIP('','',#13,#50) REPRESENTATION_RELATIONSHIP_WITH_TRANSFORMATION(#33) SHAPE_REPRESENTATION_RELATIONSHIP());\n\
         #35=CONTEXT_DEPENDENT_SHAPE_REPRESENTATION(#34,#28);\n"
    ));
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
    assert_eq!(
        root.bodies.len(),
        1,
        "the assembly lost its own body: {:?}",
        root.bodies
    );
}

/// A transformed `SHAPE_REPRESENTATION_RELATIONSHIP` that no
/// `CONTEXT_DEPENDENT_SHAPE_REPRESENTATION` names is not an assembly placement: it links a part's
/// placement-only representation to its geometry, and the part keeps the body it reaches.
#[test]
fn a_transformed_relationship_inside_one_part_keeps_the_parts_body() {
    let definitions = decode_with(&format!(
        "#14=PRODUCT('PART','part placed into its own geometry','',());\n\
         #15=PRODUCT_DEFINITION_FORMATION('','',#14);\n\
         #16=APPLICATION_CONTEXT('mechanical design');\n\
         #17=PRODUCT_DEFINITION_CONTEXT('part definition',#16,'design');\n\
         #18=PRODUCT_DEFINITION('part','',#15,#17);\n\
         #19=PRODUCT_DEFINITION_SHAPE('','',#18);\n\
         #20=SHAPE_DEFINITION_REPRESENTATION(#19,#40);\n\
         #40=SHAPE_REPRESENTATION('',(),#2);\n\
         {TRANSFORM}\
         #41=(REPRESENTATION_RELATIONSHIP('','',#40,#13) REPRESENTATION_RELATIONSHIP_WITH_TRANSFORMATION(#33) SHAPE_REPRESENTATION_RELATIONSHIP());\n"
    ));
    assert_eq!(definitions.len(), 1, "{definitions:#?}");
    assert_eq!(
        definitions[0].bodies.len(),
        1,
        "the part lost the body its own relationship reaches: {:?}",
        definitions[0].bodies
    );
}

/// The placement edge joins the parent's and the child's own representations, not the
/// relationship's endpoints. Here the placement's child-side endpoint `#13` is the component's
/// geometry, which no definition owns directly; an edge from `#40` to `#13` would hand the
/// component's body to the assembly as well.
#[test]
fn an_assembly_does_not_list_its_components_body_as_its_own() {
    let definitions = decode_with(&format!(
        "#14=PRODUCT('ROOT','assembly with no geometry of its own','',());\n\
         #15=PRODUCT_DEFINITION_FORMATION('','',#14);\n\
         #16=APPLICATION_CONTEXT('mechanical design');\n\
         #17=PRODUCT_DEFINITION_CONTEXT('part definition',#16,'design');\n\
         #18=PRODUCT_DEFINITION('root','',#15,#17);\n\
         #19=PRODUCT_DEFINITION_SHAPE('','',#18);\n\
         #20=SHAPE_DEFINITION_REPRESENTATION(#19,#40);\n\
         #40=SHAPE_REPRESENTATION('',(),#2);\n\
         #21=PRODUCT('CHILD','component that owns the body','',());\n\
         #22=PRODUCT_DEFINITION_FORMATION('','',#21);\n\
         #23=PRODUCT_DEFINITION('child','',#22,#17);\n\
         #24=PRODUCT_DEFINITION_SHAPE('','',#23);\n\
         #25=SHAPE_REPRESENTATION('',(),#2);\n\
         #26=SHAPE_DEFINITION_REPRESENTATION(#24,#25);\n\
         #51=SHAPE_REPRESENTATION_RELATIONSHIP('','',#25,#13);\n\
         #27=NEXT_ASSEMBLY_USAGE_OCCURRENCE('u1','','',#18,#23,$);\n\
         #28=PRODUCT_DEFINITION_SHAPE('','',#27);\n\
         {TRANSFORM}\
         #34=(REPRESENTATION_RELATIONSHIP('','',#40,#13) REPRESENTATION_RELATIONSHIP_WITH_TRANSFORMATION(#33) SHAPE_REPRESENTATION_RELATIONSHIP());\n\
         #35=CONTEXT_DEPENDENT_SHAPE_REPRESENTATION(#34,#28);\n"
    ));
    assert_eq!(definitions.len(), 2, "{definitions:#?}");
    let root = definitions
        .iter()
        .find(|d| d.part_number.as_deref() == Some("ROOT"))
        .expect("root definition");
    let child = definitions
        .iter()
        .find(|d| d.part_number.as_deref() == Some("CHILD"))
        .expect("child definition");
    assert!(
        root.bodies.is_empty(),
        "the assembly listed its component's body as its own: {:?}",
        root.bodies
    );
    assert_eq!(
        child.bodies.len(),
        1,
        "the component lost its body: {:?}",
        child.bodies
    );
}

/// A relationship a `CONTEXT_DEPENDENT_SHAPE_REPRESENTATION` names is an assembly placement even
/// when its chain does not resolve. Here the context-dependent representation names the child's
/// own `PRODUCT_DEFINITION_SHAPE` rather than one wrapping a `NEXT_ASSEMBLY_USAGE_OCCURRENCE`, so
/// the placement has no known direction. It is left out rather than kept undirected, which would
/// let the child reach the root's body through `#25 -> #50 -> #13`.
#[test]
fn an_unresolved_assembly_placement_is_not_followed() {
    let definitions = decode_with(&format!(
        "#14=PRODUCT('ROOT','assembly root that owns the body','',());\n\
         #15=PRODUCT_DEFINITION_FORMATION('','',#14);\n\
         #16=APPLICATION_CONTEXT('mechanical design');\n\
         #17=PRODUCT_DEFINITION_CONTEXT('part definition',#16,'design');\n\
         #18=PRODUCT_DEFINITION('root','',#15,#17);\n\
         #19=PRODUCT_DEFINITION_SHAPE('','',#18);\n\
         #20=SHAPE_DEFINITION_REPRESENTATION(#19,#40);\n\
         #40=SHAPE_REPRESENTATION('',(),#2);\n\
         #41=SHAPE_REPRESENTATION_RELATIONSHIP('','',#40,#13);\n\
         #21=PRODUCT('CHILD','component with no geometry of its own','',());\n\
         #22=PRODUCT_DEFINITION_FORMATION('','',#21);\n\
         #23=PRODUCT_DEFINITION('child','',#22,#17);\n\
         #24=PRODUCT_DEFINITION_SHAPE('','',#23);\n\
         #25=SHAPE_REPRESENTATION('',(),#2);\n\
         #26=SHAPE_DEFINITION_REPRESENTATION(#24,#25);\n\
         #50=SHAPE_REPRESENTATION('',(),#2);\n\
         #51=SHAPE_REPRESENTATION_RELATIONSHIP('','',#25,#50);\n\
         #27=NEXT_ASSEMBLY_USAGE_OCCURRENCE('u1','','',#18,#23,$);\n\
         {TRANSFORM}\
         #34=(REPRESENTATION_RELATIONSHIP('','',#13,#50) REPRESENTATION_RELATIONSHIP_WITH_TRANSFORMATION(#33) SHAPE_REPRESENTATION_RELATIONSHIP());\n\
         #35=CONTEXT_DEPENDENT_SHAPE_REPRESENTATION(#34,#24);\n"
    ));
    assert_eq!(definitions.len(), 2, "{definitions:#?}");
    let root = definitions
        .iter()
        .find(|d| d.part_number.as_deref() == Some("ROOT"))
        .expect("root definition");
    let child = definitions
        .iter()
        .find(|d| d.part_number.as_deref() == Some("CHILD"))
        .expect("child definition");
    assert!(
        child.bodies.is_empty(),
        "an unresolved placement gave the component the assembly's body: {:?}",
        child.bodies
    );
    assert_eq!(
        root.bodies.len(),
        1,
        "the assembly lost its own body: {:?}",
        root.bodies
    );
}
