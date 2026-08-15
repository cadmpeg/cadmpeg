// SPDX-License-Identifier: Apache-2.0
//! STEP product, occurrence, and assembly tests.

#![allow(clippy::unwrap_used)]
#![allow(clippy::default_trait_access)]
#![allow(unused_imports)]

use std::fmt::Write as _;
use std::io::Cursor;

use cadmpeg_core::decode::{DecodeMode, InspectOptions};
use cadmpeg_ir::codec::{Codec, CodecBackend, Confidence, DecodeOptions};
use cadmpeg_ir::eval::{
    model_curve_point_by_id, model_surface_partials_by_id, model_surface_point_by_id, pcurve_uv,
};
use cadmpeg_ir::examples::unit_cube;
use cadmpeg_ir::geometry::{
    Curve, CurveGeometry, NurbsCurve, NurbsSurface, PcurveGeometry, Surface, SurfaceGeometry,
};
use cadmpeg_ir::ids::{CurveId, ProceduralCurveId, SurfaceId};
use cadmpeg_ir::index::ModelIndex;
use cadmpeg_ir::math::{Point2, Point3, Vector3};
use cadmpeg_ir::transform::Transform;
use cadmpeg_ir::units::{LengthUnit, Units};
use cadmpeg_ir::CadIr;
use zip::write::SimpleFileOptions;
use zip::{CompressionMethod, ZipWriter};

use crate::ids::StepIdentity;
use crate::loss::StepLossCode;
use crate::test_support::{decode_inline, export};
use crate::{
    write_step, StepCodec, StepError, StepSchema, StepUnsupportedPolicy, StepWriteOptions,
};

#[test]
fn product_descriptions_transfer_from_product_and_definition() {
    let decoded = decode_inline(
        "#1=APPLICATION_CONTEXT('mechanical design');
#2=PRODUCT_CONTEXT('',#1,'mechanical');
#3=PRODUCT('P','Part','Product description',(#2));
#4=PRODUCT_DEFINITION_FORMATION('','',#3);
#5=PRODUCT_DEFINITION_CONTEXT('part definition',#1,'design');
#6=PRODUCT_DEFINITION('part','Definition description',#4,#5);
#7=PRODUCT('Q','Second part','',(#2));
#8=PRODUCT_DEFINITION_FORMATION('','',#7);
#9=PRODUCT_DEFINITION('second','Fallback description',#8,#5);",
    );
    let descriptions = decoded
        .ir()
        .model
        .product_definitions
        .iter()
        .map(|product| product.description.as_deref())
        .collect::<Vec<_>>();
    assert_eq!(
        descriptions,
        [Some("Product description"), Some("Fallback description")]
    );

    let mut ir = unit_cube();
    ir.model
        .product_definitions
        .push(cadmpeg_ir::products::ProductDefinition {
            id: "test:product#described".into(),
            kind: cadmpeg_ir::products::ProductDefinitionKind::Part,
            source_name: Some("Described part".into()),
            label: Some("Described part".into()),
            description: Some("Round-tripped description".into()),
            part_number: Some("DESCRIBED".into()),
            bom_properties: std::collections::BTreeMap::new(),
            bodies: vec![ir.model.bodies[0].id.clone()],
            native_ref: None,
        });
    let mut output = Vec::new();
    write_step(
        &ir,
        &mut output,
        &StepWriteOptions {
            schema: StepSchema::Ap242Edition3,
            ..StepWriteOptions::default()
        },
    )
    .expect("write described product");
    let roundtrip = StepCodec::default()
        .decode(&mut Cursor::new(output), &DecodeOptions::default())
        .expect("decode described product");
    assert_eq!(
        roundtrip.ir().model.product_definitions[0]
            .description
            .as_deref(),
        Some("Round-tripped description")
    );
}

#[test]
fn product_definition_views_keep_distinct_prototypes_and_metadata() {
    use cadmpeg_ir::products::PrototypeReference;

    let result = decode_inline(
        "#1=APPLICATION_CONTEXT('mechanical design');
#2=PRODUCT_CONTEXT('',#1,'mechanical');
#3=PRODUCT('P','Part','Product description',(#2));
#4=PRODUCT_DEFINITION_FORMATION('v1','',#3);
#5=PRODUCT_DEFINITION_CONTEXT('part definition',#1,'design');
#6=PRODUCT_DEFINITION('design view','Design view description',#4,#5);
#7=PRODUCT_DEFINITION_FORMATION('v2','',#3);
#8=PRODUCT_DEFINITION('manufacturing view','Manufacturing view description',#7,#5);",
    );

    assert_eq!(result.ir().model.product_definitions.len(), 2);
    assert_eq!(
        result
            .ir()
            .model
            .product_definitions
            .iter()
            .map(|definition| definition.description.as_deref())
            .collect::<Vec<_>>(),
        [
            Some("Design view description"),
            Some("Manufacturing view description")
        ]
    );
    assert_eq!(result.ir().model.occurrences.len(), 2);
    let prototypes = result
        .ir()
        .model
        .occurrences
        .iter()
        .filter_map(|occurrence| match &occurrence.prototype {
            PrototypeReference::Local { definition } => Some(definition.clone()),
            _ => None,
        })
        .collect::<Vec<_>>();
    assert_eq!(prototypes.len(), 2);
    assert_ne!(prototypes[0], prototypes[1]);
    assert!(prototypes
        .iter()
        .all(|id| id.as_str().contains("-definition-")));
    let validation = cadmpeg_ir::validate_neutral(result.ir(), result.report().losses.clone());
    assert!(validation.is_ok(), "{:#?}", validation.findings);
}

#[test]
pub(crate) fn decode_builds_product_occurrences_with_relative_placement() {
    use cadmpeg_ir::products::OccurrenceParent;

    let bytes = include_bytes!("../../../tests/fixtures/ap242_assembly.p21");
    let result = StepCodec::default()
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .expect("decode AP242 assembly");

    assert_eq!(result.ir().model.product_definitions.len(), 2);
    assert_eq!(result.ir().model.occurrences.len(), 2);
    let child = result
        .ir()
        .model
        .occurrences
        .iter()
        .find(|occurrence| occurrence.name.as_deref() == Some("Placed child"))
        .unwrap();
    assert!(matches!(child.parent, OccurrenceParent::Occurrence { .. }));
    assert_eq!(child.transform.rows[0][3], 25.0);
    assert_eq!(child.transform.rows[1][3], 0.0);
    assert_eq!(child.transform.rows[2][3], 0.0);
    let validation = cadmpeg_ir::validate_neutral(result.ir(), result.report().losses.clone());
    assert!(validation.is_ok(), "{:#?}", validation.findings);

    let options = StepWriteOptions {
        schema: StepSchema::Ap242Edition3,
        ..StepWriteOptions::default()
    };
    let mut output = Vec::new();
    write_step(result.ir(), &mut output, &options).expect("write product graph");
    let roundtrip = StepCodec::default()
        .decode(&mut Cursor::new(output), &DecodeOptions::default())
        .expect("decode written product graph");
    assert_eq!(roundtrip.ir().model.product_definitions.len(), 2);
    assert_eq!(roundtrip.ir().model.occurrences.len(), 2);
    let child = roundtrip
        .ir()
        .model
        .occurrences
        .iter()
        .find(|occurrence| occurrence.name.as_deref() == Some("Placed child"))
        .expect("round-tripped child occurrence");
    assert!(matches!(child.parent, OccurrenceParent::Occurrence { .. }));
    assert_eq!(child.transform.rows[0][3], 25.0);
}

#[test]
fn occurrence_transform_direction_follows_relationship_endpoints() {
    let source = String::from_utf8(include_bytes!(
        "../../../tests/fixtures/ap242_assembly.p21"
    )
    .to_vec())
    .expect("fixture is UTF-8")
    .replace(
        "#37=(REPRESENTATION_RELATIONSHIP('','',#23,#22) REPRESENTATION_RELATIONSHIP_WITH_TRANSFORMATION(#36) SHAPE_REPRESENTATION_RELATIONSHIP());",
        "#37=(REPRESENTATION_RELATIONSHIP('','',#22,#23) REPRESENTATION_RELATIONSHIP_WITH_TRANSFORMATION(#36) SHAPE_REPRESENTATION_RELATIONSHIP());",
    );
    let result = StepCodec::default()
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .expect("decode endpoint-reversed assembly relationship");

    let child = result
        .ir()
        .model
        .occurrences
        .iter()
        .find(|occurrence| occurrence.name.as_deref() == Some("Placed child"))
        .expect("placed child occurrence");
    assert_eq!(child.transform.rows[0][3], -25.0);
    assert!(!result.report().losses.iter().any(|loss| {
        loss.code == StepLossCode::NauoPlacementUnresolved.kind()
            && loss.message.contains("NAUO #12")
    }));
}

#[test]
fn occurrence_transform_resolves_through_placed_shape_representation() {
    let source = String::from_utf8(include_bytes!(
        "../../../tests/fixtures/ap242_assembly.p21"
    )
    .to_vec())
    .expect("fixture is UTF-8")
    .replace(
        "#37=(REPRESENTATION_RELATIONSHIP('','',#23,#22) REPRESENTATION_RELATIONSHIP_WITH_TRANSFORMATION(#36) SHAPE_REPRESENTATION_RELATIONSHIP());",
        "#37=(REPRESENTATION_RELATIONSHIP('','',#40,#22) REPRESENTATION_RELATIONSHIP_WITH_TRANSFORMATION(#36) SHAPE_REPRESENTATION_RELATIONSHIP());",
    )
    .replace(
        "ENDSEC;\nEND-ISO-10303-21;",
        "#40=SHAPE_REPRESENTATION('placed child',(),#21);\n#41=SHAPE_REPRESENTATION_RELATIONSHIP('','',#23,#40);\nENDSEC;\nEND-ISO-10303-21;",
    );
    let result = StepCodec::default()
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .expect("decode placed shape representation");

    let child = result
        .ir()
        .model
        .occurrences
        .iter()
        .find(|occurrence| occurrence.name.as_deref() == Some("Placed child"))
        .expect("placed child occurrence");
    assert_eq!(child.transform.rows[0][3], 25.0);
    assert!(!result
        .report()
        .losses
        .iter()
        .any(|loss| { loss.code == StepLossCode::NauoPlacementUnresolved.kind() }));
}

#[test]
fn occurrence_transform_accepts_cartesian_operator_endpoints() {
    let source =
        String::from_utf8(include_bytes!("../../../tests/fixtures/ap242_assembly.p21").to_vec())
            .expect("fixture is UTF-8")
            .replace(
                "#34=AXIS2_PLACEMENT_3D('',#30,#32,#33);",
                "#34=CARTESIAN_TRANSFORMATION_OPERATOR_3D('',#33,$,#30,1.,#32);",
            )
            .replace(
                "#35=AXIS2_PLACEMENT_3D('',#31,#32,#33);",
                "#35=CARTESIAN_TRANSFORMATION_OPERATOR_3D('',#33,$,#31,1.,#32);",
            );
    let result = StepCodec::default()
        .decode(
            &mut Cursor::new(source.into_bytes()),
            &DecodeOptions::default(),
        )
        .expect("decode operator-based occurrence transform");

    let child = result
        .ir()
        .model
        .occurrences
        .iter()
        .find(|occurrence| occurrence.name.as_deref() == Some("Placed child"))
        .expect("placed child occurrence");
    assert_eq!(child.transform.rows[0][3], 25.0);
    assert!(!result
        .report()
        .losses
        .iter()
        .any(|loss| loss.code == StepLossCode::NauoPlacementUnresolved.kind()));
}

#[test]
fn unresolved_occurrence_transform_is_reported_as_error() {
    let result = decode_inline(
        "#1=APPLICATION_CONTEXT('mechanical design');
#2=PRODUCT_CONTEXT('',#1,'mechanical');
#3=PRODUCT('P','parent','',(#2));
#4=PRODUCT_DEFINITION_FORMATION('','',#3);
#5=PRODUCT_DEFINITION_CONTEXT('part definition',#1,'design');
#6=PRODUCT_DEFINITION('parent','',#4,#5);
#7=PRODUCT('C','child','',(#2));
#8=FINAL_SOLUTION('','',#7,'complete');
#9=PRODUCT_DEFINITION('child','',#8,#5);
#10=NEXT_ASSEMBLY_USAGE_OCCURRENCE('u','child instance','',#6,#9,$);",
    );

    assert!(result.report().losses.iter().any(|loss| {
        loss.code == StepLossCode::NauoPlacementUnresolved.kind()
            && loss.severity == cadmpeg_ir::Severity::Error
            && loss.message.contains("NAUO #10")
    }));
}

#[test]
pub(crate) fn decode_builds_occurrence_placement_from_mapped_item() {
    let bytes = include_bytes!("../../../tests/fixtures/ap242_mapped_assembly.p21");
    let result = StepCodec::default()
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .expect("decode mapped-item assembly");

    let child = result
        .ir()
        .model
        .occurrences
        .iter()
        .find(|occurrence| occurrence.name.as_deref() == Some("Mapped child"))
        .unwrap();
    assert_eq!(child.transform.rows[0][3], 40.0);
    assert_eq!(child.transform.rows[1][3], 5.0);
    let validation = cadmpeg_ir::validate_neutral(result.ir(), result.report().losses.clone());
    assert!(validation.is_ok(), "{:#?}", validation.findings);
}

#[test]
fn complex_product_relationships_preserve_mapped_occurrence_placement() {
    let source = String::from_utf8(include_bytes!(
        "../../../tests/fixtures/ap242_mapped_assembly.p21"
    )
    .to_vec())
    .expect("fixture is UTF-8")
    .replace(
        "#7=PRODUCT_DEFINITION_SHAPE('','',#6);",
        "#7=(PRODUCT_DEFINITION_SHAPE('','',#6) PROPERTY_DEFINITION());",
    )
    .replace(
        "#11=PRODUCT_DEFINITION_SHAPE('','',#10);",
        "#11=(PRODUCT_DEFINITION_SHAPE('','',#10) PROPERTY_DEFINITION());",
    )
    .replace(
        "#12=NEXT_ASSEMBLY_USAGE_OCCURRENCE('occ-1','Mapped child','',#6,#10,$);",
        "#12=(ASSEMBLY_COMPONENT_USAGE() NEXT_ASSEMBLY_USAGE_OCCURRENCE('occ-1','Mapped child','',#6,#10,$));",
    )
    .replace(
        "#24=SHAPE_DEFINITION_REPRESENTATION(#7,#22);",
        "#24=(PROPERTY_DEFINITION_REPRESENTATION() SHAPE_DEFINITION_REPRESENTATION(#7,#22));",
    )
    .replace(
        "#25=SHAPE_DEFINITION_REPRESENTATION(#11,#23);",
        "#25=(PROPERTY_DEFINITION_REPRESENTATION() SHAPE_DEFINITION_REPRESENTATION(#11,#23));",
    )
    .replace(
        "#40=MAPPED_ITEM('Mapped child',#39,#35);",
        "#40=(MAPPED_ITEM('Mapped child',#39,#35) REPRESENTATION_ITEM());",
    );
    let result = StepCodec::default()
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .expect("decode complex mapped-item assembly");

    let child = result
        .ir()
        .model
        .occurrences
        .iter()
        .find(|occurrence| occurrence.name.as_deref() == Some("Mapped child"))
        .expect("mapped child occurrence");
    assert_eq!(child.transform.rows[0][3], 40.0);
    assert_eq!(child.transform.rows[1][3], 5.0);
}

#[test]
fn conflicting_standalone_mapped_body_placements_are_not_overwritten() {
    let source = String::from_utf8(include_bytes!("../../../tests/fixtures/ap214_sheet.p21").to_vec())
        .expect("fixture is UTF-8")
        .replace(
            "ENDSEC;\nEND-ISO-10303-21;",
            "#70=CARTESIAN_POINT('',(20.,0.,0.));\n#71=CARTESIAN_POINT('',(40.,0.,0.));\n#72=AXIS2_PLACEMENT_3D('',#70,#9,#10);\n#73=AXIS2_PLACEMENT_3D('',#71,#9,#10);\n#74=REPRESENTATION_MAP(#27,#32);\n#75=MAPPED_ITEM('first',#74,#72);\n#76=MAPPED_ITEM('second',#74,#73);\nENDSEC;\nEND-ISO-10303-21;",
        );
    let result = StepCodec::default()
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .expect("decode conflicting standalone body mappings");

    assert_eq!(result.ir().model.bodies.len(), 1);
    assert!(result.ir().model.bodies[0].transform.is_none());
    assert!(result.report().losses.iter().any(|loss| {
        loss.code == StepLossCode::BodyConflictingMappedPlacements.kind()
            && loss.severity == cadmpeg_ir::Severity::Error
            && loss
                .message
                .contains("conflicting standalone MAPPED_ITEM placements")
            && loss.message.contains("#75")
            && loss.message.contains("#76")
    }));
}

#[test]
fn drawing_mapped_items_do_not_place_exact_bodies() {
    let source = String::from_utf8(
        include_bytes!("../../../tests/fixtures/ap242_vertex_loop.p21").to_vec(),
    )
    .expect("fixture is UTF-8")
    .replace(
        "ENDSEC;\nEND-ISO-10303-21;",
        "#20=ITEM('camera mapping');\n#21=REPRESENTATION_MAP(#20,#19);\n#22=MAPPED_ITEM('',#21,#6);\n#23=DRAUGHTING_MODEL('Drawing',(#22),#2);\nENDSEC;\nEND-ISO-10303-21;",
    );
    let result = StepCodec::default()
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .expect("decode drawing mapped item");

    assert_eq!(result.ir().model.bodies.len(), 1);
    assert!(result.ir().model.bodies[0].transform.is_none());
    assert!(!result.report().losses.iter().any(|loss| {
        loss.message
            .contains("MAPPED_ITEM #22 has no resolved body placement")
    }));
}

#[test]
fn nested_drawing_mapped_items_do_not_place_exact_bodies() {
    let source = String::from_utf8(
        include_bytes!("../../../tests/fixtures/ap242_vertex_loop.p21").to_vec(),
    )
    .expect("fixture is UTF-8")
    .replace(
        "ENDSEC;\nEND-ISO-10303-21;",
        "#20=ITEM('camera mapping');\n#21=REPRESENTATION_MAP(#20,#19);\n#22=MAPPED_ITEM('',#21,#6);\n#23=GEOMETRIC_SET('Drawing items',(#22));\n#24=DRAUGHTING_MODEL('Drawing',(#23),#2);\nENDSEC;\nEND-ISO-10303-21;",
    );
    let result = StepCodec::default()
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .expect("decode nested drawing mapped item");

    assert_eq!(result.ir().model.bodies.len(), 1);
    assert!(result.ir().model.bodies[0].transform.is_none());
    assert!(!result.report().losses.iter().any(|loss| {
        loss.message
            .contains("MAPPED_ITEM #22 has no resolved body placement")
    }));
}

#[test]
fn two_dimensional_mapping_does_not_change_body_placement() {
    let mut source = export(&unit_cube());
    let representation_line = source
        .lines()
        .find(|line| line.contains("ADVANCED_BREP_SHAPE_REPRESENTATION("))
        .expect("written body representation");
    let representation = representation_line
        .split_once('=')
        .and_then(|(id, _)| id.trim().strip_prefix('#'))
        .and_then(|id| id.parse::<u64>().ok())
        .expect("body representation id");
    let next_id = source
        .lines()
        .filter_map(|line| {
            line.strip_prefix('#')
                .and_then(|line| line.split_once('='))
                .and_then(|(id, _)| id.trim().parse::<u64>().ok())
        })
        .max()
        .expect("written STEP entity")
        + 1;
    let origin_point = next_id;
    let origin_direction = next_id + 1;
    let origin = next_id + 2;
    let map = next_id + 3;
    let target_point = next_id + 4;
    let target = next_id + 5;
    let mapped_item = next_id + 6;
    let records = format!(
        "#{origin_point}=CARTESIAN_POINT('',(0.,0.));\n\
#{origin_direction}=DIRECTION('',(1.,0.));\n\
#{origin}=AXIS2_PLACEMENT_2D('',#{origin_point},#{origin_direction});\n\
#{map}=REPRESENTATION_MAP(#{origin},#{representation});\n\
#{target_point}=CARTESIAN_POINT('',(10.,0.));\n\
#{target}=AXIS2_PLACEMENT_2D('',#{target_point},#{origin_direction});\n\
#{mapped_item}=MAPPED_ITEM('',#{map},#{target});\n"
    );
    let end = source.rfind("ENDSEC;").expect("STEP data section end");
    source.insert_str(end, &records);

    let result = StepCodec::default()
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .expect("decode 2D mapped presentation item");

    assert_eq!(result.ir().model.bodies.len(), 1);
    assert!(result.ir().model.bodies[0].transform.is_none());
    assert!(!result.report().losses.iter().any(|loss| {
        loss.message
            .contains("MAPPED_ITEM has no resolved body placement")
    }));
}

#[test]
fn decode_builds_mapped_item_placement_from_canonical_cartesian_operator() {
    let bytes = include_bytes!("../../../tests/fixtures/ap242_mapped_assembly.p21");
    let mut source = String::from_utf8(bytes.to_vec()).expect("fixture is UTF-8");
    source = source.replace(
        "#30=CARTESIAN_POINT('',(0.,0.,0.));",
        "#30=CARTESIAN_POINT('',(10.,0.,0.));",
    );
    source = source.replace(
        "#33=DIRECTION('',(1.,0.,0.));",
        "#33=DIRECTION('',(1.,0.,0.));\n#36=DIRECTION('',(0.,1.,0.));",
    );
    source = source.replace(
        "#35=AXIS2_PLACEMENT_3D('',#31,#32,#33);",
        "#35=CARTESIAN_TRANSFORMATION_OPERATOR_3D('','','',#33,#36,#31,2.,#32);",
    );
    let result = StepCodec::default()
        .decode(
            &mut Cursor::new(source.into_bytes()),
            &DecodeOptions::default(),
        )
        .expect("decode canonical mapped-item assembly");

    let child = result
        .ir()
        .model
        .occurrences
        .iter()
        .find(|occurrence| occurrence.name.as_deref() == Some("Mapped child"))
        .expect("mapped child occurrence");
    assert_eq!(child.transform.rows[0], [2.0, 0.0, 0.0, 20.0]);
    assert_eq!(child.transform.rows[1], [0.0, 2.0, 0.0, 5.0]);
    assert_eq!(child.transform.rows[2], [0.0, 0.0, 2.0, 0.0]);
    assert!(!result
        .report()
        .losses
        .iter()
        .any(|loss| loss.code == StepLossCode::NauoPlacementUnresolved.kind()));
    let validation = cadmpeg_ir::validate_neutral(result.ir(), result.report().losses.clone());
    assert!(validation.is_ok(), "{:#?}", validation.findings);
}

#[test]
fn decode_builds_repeated_occurrence_placements_from_their_shape_representations() {
    let bytes = include_bytes!("../../../tests/fixtures/ap242_occurrence_mapped_assembly.p21");
    let result = StepCodec::default()
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .expect("decode occurrence-mapped assembly");

    let mut children = result
        .ir()
        .model
        .occurrences
        .iter()
        .filter(|occurrence| occurrence.name.is_some())
        .collect::<Vec<_>>();
    children.sort_by(|left, right| left.name.cmp(&right.name));
    assert_eq!(children.len(), 2);
    assert_eq!(children[0].name.as_deref(), Some("First child"));
    assert_eq!(children[0].transform.rows[0][3], 25.0);
    assert_eq!(children[0].transform.rows[1][3], 0.0);
    assert_eq!(children[1].name.as_deref(), Some("Second child"));
    assert_eq!(children[1].transform.rows[0][3], -10.0);
    assert_eq!(children[1].transform.rows[1][3], 4.0);
    assert!(!result
        .report()
        .losses
        .iter()
        .any(|loss| loss.code == StepLossCode::NauoPlacementUnresolved.kind()));
    let validation = cadmpeg_ir::validate_neutral(result.ir(), result.report().losses.clone());
    assert!(validation.is_ok(), "{:#?}", validation.findings);
}

#[test]
fn decode_infers_unlinked_occurrence_placements_from_parent_shape_items() {
    let result = decode_inline(
        "#1=APPLICATION_CONTEXT('mechanical design');
#2=PRODUCT_CONTEXT('',#1,'mechanical');
#3=PRODUCT('ROOT','Root assembly','',(#2));
#4=PRODUCT_DEFINITION_FORMATION('','',#3);
#5=PRODUCT_DEFINITION_CONTEXT('part definition',#1,'design');
#6=PRODUCT_DEFINITION('root definition','',#4,#5);
#7=PRODUCT_DEFINITION_SHAPE('','',#6);
#8=PRODUCT('ONE','First child','',(#2));
#9=PRODUCT_DEFINITION_FORMATION('','',#8);
#10=PRODUCT_DEFINITION('first definition','',#9,#5);
#11=PRODUCT_DEFINITION_SHAPE('','',#10);
#12=PRODUCT('TWO','Second child','',(#2));
#13=PRODUCT_DEFINITION_FORMATION('','',#12);
#14=PRODUCT_DEFINITION('second definition','',#13,#5);
#15=PRODUCT_DEFINITION_SHAPE('','',#14);
#16=NEXT_ASSEMBLY_USAGE_OCCURRENCE('one','First child','',#6,#10,$);
#17=NEXT_ASSEMBLY_USAGE_OCCURRENCE('two','Second child','',#6,#14,$);
#20=(LENGTH_UNIT() NAMED_UNIT(*) SI_UNIT(.MILLI.,.METRE.));
#21=(GEOMETRIC_REPRESENTATION_CONTEXT(3) GLOBAL_UNIT_ASSIGNED_CONTEXT((#20)) REPRESENTATION_CONTEXT('model','3D'));
#22=(CHARACTERIZED_REPRESENTATION() REPRESENTATION('root',(#39,#41),#21) SHAPE_REPRESENTATION());
#23=SHAPE_REPRESENTATION('first',(),#21);
#24=SHAPE_REPRESENTATION('second',(),#21);
#25=SHAPE_DEFINITION_REPRESENTATION(#7,#22);
#26=SHAPE_DEFINITION_REPRESENTATION(#11,#23);
#27=SHAPE_DEFINITION_REPRESENTATION(#15,#24);
#30=CARTESIAN_POINT('',(0.,0.,0.));
#31=CARTESIAN_POINT('',(25.,0.,0.));
#32=CARTESIAN_POINT('',(-10.,4.,0.));
#33=DIRECTION('',(0.,0.,1.));
#34=DIRECTION('',(1.,0.,0.));
#35=AXIS2_PLACEMENT_3D('',#30,#33,#34);
#36=AXIS2_PLACEMENT_3D('',#31,#33,#34);
#37=AXIS2_PLACEMENT_3D('',#32,#33,#34);
#38=REPRESENTATION_MAP(#35,#23);
#39=MAPPED_ITEM('First child',#38,#36);
#40=REPRESENTATION_MAP(#35,#24);
#41=MAPPED_ITEM('Second child',#40,#37);",
    );

    let mut children = result
        .ir()
        .model
        .occurrences
        .iter()
        .filter(|occurrence| occurrence.id.0.contains("#16") || occurrence.id.0.contains("#17"))
        .collect::<Vec<_>>();
    children.sort_by_key(|occurrence| occurrence.id.clone());
    assert_eq!(children.len(), 2);
    assert_eq!(children[0].transform.rows[0][3], 25.0);
    assert_eq!(children[1].transform.rows[0][3], -10.0);
    assert_eq!(children[1].transform.rows[1][3], 4.0);
    assert!(!result
        .report()
        .losses
        .iter()
        .any(|loss| { loss.code == StepLossCode::NauoPlacementUnresolved.kind() }));
    let validation = cadmpeg_ir::validate_neutral(result.ir(), result.report().losses.clone());
    assert!(validation.is_ok(), "{:#?}", validation.findings);
}

#[test]
fn unrelated_representation_mapping_does_not_place_an_occurrence() {
    let result = decode_inline(
        "#1=APPLICATION_CONTEXT('mechanical design');
#2=PRODUCT_CONTEXT('',#1,'mechanical');
#3=PRODUCT('ROOT','Root assembly','',(#2));
#4=PRODUCT_DEFINITION_FORMATION('','',#3);
#5=PRODUCT_DEFINITION_CONTEXT('part definition',#1,'design');
#6=PRODUCT_DEFINITION('root definition','',#4,#5);
#7=PRODUCT_DEFINITION_SHAPE('','',#6);
#8=PRODUCT('CHILD','Child','',(#2));
#9=PRODUCT_DEFINITION_FORMATION('','',#8);
#10=PRODUCT_DEFINITION('child definition','',#9,#5);
#11=PRODUCT_DEFINITION_SHAPE('','',#10);
#16=NEXT_ASSEMBLY_USAGE_OCCURRENCE('one','First child','',#6,#10,$);
#20=(LENGTH_UNIT() NAMED_UNIT(*) SI_UNIT(.MILLI.,.METRE.));
#21=(GEOMETRIC_REPRESENTATION_CONTEXT(3) GLOBAL_UNIT_ASSIGNED_CONTEXT((#20)) REPRESENTATION_CONTEXT('model','3D'));
#22=SHAPE_REPRESENTATION('root',(),#21);
#23=SHAPE_REPRESENTATION('child',(),#21);
#25=SHAPE_DEFINITION_REPRESENTATION(#7,#22);
#26=SHAPE_DEFINITION_REPRESENTATION(#11,#23);
#30=CARTESIAN_POINT('',(0.,0.,0.));
#31=CARTESIAN_POINT('',(25.,0.,0.));
#33=DIRECTION('',(0.,0.,1.));
#34=DIRECTION('',(1.,0.,0.));
#35=AXIS2_PLACEMENT_3D('',#30,#33,#34);
#36=AXIS2_PLACEMENT_3D('',#31,#33,#34);
#38=REPRESENTATION_MAP(#35,#23);
#39=MAPPED_ITEM('unrelated',#38,#36);
#50=SHAPE_REPRESENTATION('unrelated',(#39),#21);",
    );

    let occurrence = result
        .ir()
        .model
        .occurrences
        .iter()
        .find(|occurrence| occurrence.id.0.contains("#16"))
        .expect("child occurrence");
    assert_eq!(
        occurrence.transform,
        cadmpeg_ir::transform::Transform::identity()
    );
    assert!(result.report().losses.iter().any(|loss| {
        loss.code == StepLossCode::NauoPlacementUnresolved.kind()
            && loss.severity == cadmpeg_ir::Severity::Error
            && loss.message.contains("NAUO #16")
    }));
}

#[test]
fn repeated_child_uses_without_owned_placements_remain_unresolved() {
    let result = decode_inline(
        "#1=APPLICATION_CONTEXT('mechanical design');
#2=PRODUCT_CONTEXT('',#1,'mechanical');
#3=PRODUCT('ROOT','Root assembly','',(#2));
#4=PRODUCT_DEFINITION_FORMATION('','',#3);
#5=PRODUCT_DEFINITION_CONTEXT('part definition',#1,'design');
#6=PRODUCT_DEFINITION('root definition','',#4,#5);
#7=PRODUCT_DEFINITION_SHAPE('','',#6);
#8=PRODUCT('CHILD','Child','',(#2));
#9=PRODUCT_DEFINITION_FORMATION('','',#8);
#10=PRODUCT_DEFINITION('child definition','',#9,#5);
#11=PRODUCT_DEFINITION_SHAPE('','',#10);
#16=NEXT_ASSEMBLY_USAGE_OCCURRENCE('one','First child','',#6,#10,$);
#17=NEXT_ASSEMBLY_USAGE_OCCURRENCE('two','Second child','',#6,#10,$);
#20=(LENGTH_UNIT() NAMED_UNIT(*) SI_UNIT(.MILLI.,.METRE.));
#21=(GEOMETRIC_REPRESENTATION_CONTEXT(3) GLOBAL_UNIT_ASSIGNED_CONTEXT((#20)) REPRESENTATION_CONTEXT('model','3D'));
#22=SHAPE_REPRESENTATION('root',(#39,#41),#21);
#23=SHAPE_REPRESENTATION('child',(),#21);
#25=SHAPE_DEFINITION_REPRESENTATION(#7,#22);
#26=SHAPE_DEFINITION_REPRESENTATION(#11,#23);
#30=CARTESIAN_POINT('',(0.,0.,0.));
#31=CARTESIAN_POINT('',(25.,0.,0.));
#32=CARTESIAN_POINT('',(-10.,4.,0.));
#33=DIRECTION('',(0.,0.,1.));
#34=DIRECTION('',(1.,0.,0.));
#35=AXIS2_PLACEMENT_3D('',#30,#33,#34);
#36=AXIS2_PLACEMENT_3D('',#31,#33,#34);
#37=AXIS2_PLACEMENT_3D('',#32,#33,#34);
#38=REPRESENTATION_MAP(#35,#23);
#39=MAPPED_ITEM('First child',#38,#36);
#40=REPRESENTATION_MAP(#35,#23);
#41=MAPPED_ITEM('Second child',#40,#37);",
    );

    let children = result
        .ir()
        .model
        .occurrences
        .iter()
        .filter(|occurrence| occurrence.id.0.contains("#16") || occurrence.id.0.contains("#17"))
        .collect::<Vec<_>>();
    assert_eq!(children.len(), 2);
    assert!(children
        .iter()
        .all(|occurrence| occurrence.transform == cadmpeg_ir::transform::Transform::identity()));
    for usage_id in [16, 17] {
        assert!(result.report().losses.iter().any(|loss| {
            loss.code == StepLossCode::NauoPlacementUnresolved.kind()
                && loss.severity == cadmpeg_ir::Severity::Error
                && loss.message.contains(&format!("NAUO #{usage_id}"))
        }));
    }
}

#[test]
fn mapped_child_unique_per_parent_uses_parent_local_uniqueness() {
    let result = decode_inline(
        "#1=APPLICATION_CONTEXT('mechanical design');
#2=PRODUCT_CONTEXT('',#1,'mechanical');
#3=PRODUCT('ROOT','Root assembly','',(#2));
#4=PRODUCT_DEFINITION_FORMATION('','',#3);
#5=PRODUCT_DEFINITION_CONTEXT('part definition',#1,'design');
#6=PRODUCT_DEFINITION('root definition','',#4,#5);
#7=PRODUCT_DEFINITION_SHAPE('','',#6);
#8=PRODUCT('PARENT-A','Parent A','',(#2));
#9=PRODUCT_DEFINITION_FORMATION('','',#8);
#10=PRODUCT_DEFINITION('parent A definition','',#9,#5);
#11=PRODUCT_DEFINITION_SHAPE('','',#10);
#12=PRODUCT('PARENT-B','Parent B','',(#2));
#13=PRODUCT_DEFINITION_FORMATION('','',#12);
#14=PRODUCT_DEFINITION('parent B definition','',#13,#5);
#15=PRODUCT_DEFINITION_SHAPE('','',#14);
#16=PRODUCT('CHILD','Child','',(#2));
#17=PRODUCT_DEFINITION_FORMATION('','',#16);
#18=PRODUCT_DEFINITION('child definition','',#17,#5);
#19=PRODUCT_DEFINITION_SHAPE('','',#18);
#20=NEXT_ASSEMBLY_USAGE_OCCURRENCE('a','Child A','',#10,#18,$);
#21=NEXT_ASSEMBLY_USAGE_OCCURRENCE('b','Child B','',#14,#18,$);
#22=NEXT_ASSEMBLY_USAGE_OCCURRENCE('pa','Parent A','',#6,#10,$);
#23=NEXT_ASSEMBLY_USAGE_OCCURRENCE('pb','Parent B','',#6,#14,$);
#30=(LENGTH_UNIT() NAMED_UNIT(*) SI_UNIT(.MILLI.,.METRE.));
#31=(GEOMETRIC_REPRESENTATION_CONTEXT(3) GLOBAL_UNIT_ASSIGNED_CONTEXT((#30)) REPRESENTATION_CONTEXT('model','3D'));
#32=SHAPE_REPRESENTATION('root',(#53,#55),#31);
#33=SHAPE_REPRESENTATION('parent A',(#57),#31);
#34=SHAPE_REPRESENTATION('parent B',(#59),#31);
#35=SHAPE_REPRESENTATION('child',(),#31);
#36=SHAPE_DEFINITION_REPRESENTATION(#7,#32);
#37=SHAPE_DEFINITION_REPRESENTATION(#11,#33);
#38=SHAPE_DEFINITION_REPRESENTATION(#15,#34);
#39=SHAPE_DEFINITION_REPRESENTATION(#19,#35);
#40=CARTESIAN_POINT('',(0.,0.,0.));
#41=CARTESIAN_POINT('',(100.,0.,0.));
#42=CARTESIAN_POINT('',(200.,0.,0.));
#43=CARTESIAN_POINT('',(10.,0.,0.));
#44=CARTESIAN_POINT('',(20.,0.,0.));
#45=DIRECTION('',(0.,0.,1.));
#46=DIRECTION('',(1.,0.,0.));
#47=AXIS2_PLACEMENT_3D('',#40,#45,#46);
#48=AXIS2_PLACEMENT_3D('',#41,#45,#46);
#49=AXIS2_PLACEMENT_3D('',#42,#45,#46);
#50=AXIS2_PLACEMENT_3D('',#43,#45,#46);
#51=AXIS2_PLACEMENT_3D('',#44,#45,#46);
#52=REPRESENTATION_MAP(#47,#33);
#53=MAPPED_ITEM('Parent A',#52,#48);
#54=REPRESENTATION_MAP(#47,#34);
#55=MAPPED_ITEM('Parent B',#54,#49);
#56=REPRESENTATION_MAP(#47,#35);
#57=MAPPED_ITEM('Child A',#56,#50);
#58=REPRESENTATION_MAP(#47,#35);
#59=MAPPED_ITEM('Child B',#58,#51);",
    );

    let children = result
        .ir()
        .model
        .occurrences
        .iter()
        .filter(|occurrence| occurrence.name.is_some())
        .collect::<Vec<_>>();
    assert_eq!(children.len(), 4);
    assert!(children.iter().any(|occurrence| {
        occurrence.name.as_deref() == Some("Child A") && occurrence.transform.rows[0][3] == 10.0
    }));
    assert!(children.iter().any(|occurrence| {
        occurrence.name.as_deref() == Some("Child B") && occurrence.transform.rows[0][3] == 20.0
    }));
    assert!(!result
        .report()
        .losses
        .iter()
        .any(|loss| loss.code == StepLossCode::NauoPlacementUnresolved.kind()));
}

#[test]
pub(crate) fn repeated_subassembly_instances_each_receive_the_subtree() {
    use cadmpeg_ir::products::{OccurrenceParent, PrototypeReference};

    let result = decode_inline(
        "#1=APPLICATION_CONTEXT('mechanical design');
#2=PRODUCT_CONTEXT('',#1,'mechanical');
#3=PRODUCT('P','parent','',(#2));
#4=PRODUCT_DEFINITION_FORMATION('','',#3);
#5=PRODUCT_DEFINITION_CONTEXT('part definition',#1,'design');
#6=PRODUCT_DEFINITION('parent','',#4,#5);
#7=PRODUCT('S','subassembly','',(#2));
#8=PRODUCT_DEFINITION_FORMATION('','',#7);
#9=PRODUCT_DEFINITION('subassembly','',#8,#5);
#10=PRODUCT('L','leaf','',(#2));
#11=PRODUCT_DEFINITION_FORMATION('','',#10);
#12=PRODUCT_DEFINITION('leaf','',#11,#5);
#20=NEXT_ASSEMBLY_USAGE_OCCURRENCE('u1','sub one','',#6,#9,$);
#21=NEXT_ASSEMBLY_USAGE_OCCURRENCE('u2','sub two','',#6,#9,$);
#22=NEXT_ASSEMBLY_USAGE_OCCURRENCE('u3','leaf','',#9,#12,$);",
    );
    assert_eq!(result.ir().model.occurrences.len(), 5);
    let subassemblies = result
        .ir()
        .model
        .occurrences
        .iter()
        .filter(|occurrence| {
            matches!(
                &occurrence.prototype,
                PrototypeReference::Local { definition }
                    if definition.as_str() == "step:product:product#7"
            )
        })
        .collect::<Vec<_>>();
    assert_eq!(subassemblies.len(), 2);
    for subassembly in subassemblies {
        assert_eq!(
            result
                .ir()
                .model
                .occurrences
                .iter()
                .filter(|occurrence| matches!(
                    &occurrence.parent,
                    OccurrenceParent::Occurrence { occurrence: parent }
                        if parent == &subassembly.id
                ))
                .count(),
            1
        );
    }
}

#[test]
pub(crate) fn ap203_specified_source_formations_build_occurrence_tree() {
    let result = decode_inline(
        "#1=APPLICATION_CONTEXT('configuration controlled design');
#2=PRODUCT_CONTEXT('',#1,'mechanical');
#3=PRODUCT('A','assembly','',(#2));
#4=PRODUCT_DEFINITION_FORMATION_WITH_SPECIFIED_SOURCE('','',#3,.NOT_KNOWN.);
#5=PRODUCT_DEFINITION_CONTEXT('part definition',#1,'design');
#6=PRODUCT_DEFINITION('assembly','',#4,#5);
#7=PRODUCT('P','part','',(#2));
#8=PRODUCT_DEFINITION_FORMATION_WITH_SPECIFIED_SOURCE('','',#7,.NOT_KNOWN.);
#9=PRODUCT_DEFINITION('part','',#8,#5);
#10=NEXT_ASSEMBLY_USAGE_OCCURRENCE('u1','part instance','',#6,#9,$);",
    );

    assert_eq!(result.ir().model.product_definitions.len(), 2);
    assert_eq!(result.ir().model.occurrences.len(), 2);
    assert!(result
        .ir()
        .model
        .occurrences
        .iter()
        .any(|occurrence| matches!(
            &occurrence.prototype,
            cadmpeg_ir::products::PrototypeReference::Local { definition }
                if definition.as_str() == "step:product:product#7"
        )));
    assert!(!result
        .ir()
        .native_unknowns("step")
        .unwrap()
        .iter()
        .any(|record| {
            record.id.0.contains("product_definition_formation")
                || record.id.0.contains("next_assembly_usage_occurrence")
        }));
}

#[test]
fn product_definition_subtypes_preserve_assembly_occurrences() {
    use cadmpeg_ir::products::OccurrenceParent;

    let result = decode_inline(
        "#1=APPLICATION_CONTEXT('mechanical design');
#2=PRODUCT_CONTEXT('',#1,'mechanical');
#3=PRODUCT('ROOT','Root assembly','',(#2));
#4=PRODUCT_DEFINITION_FORMATION('','',#3);
#5=PRODUCT_DEFINITION_CONTEXT('part definition',#1,'design');
#6=PRODUCT_DEFINITION_WITH_ASSOCIATED_DOCUMENTS('root definition','',#4,#5,(#15));
#7=PRODUCT('CHILD','Child part','',(#2));
#8=PRODUCT_DEFINITION_FORMATION('','',#7);
#9=PRODUCT_DEFINITION('child definition','',#8,#5);
#10=NEXT_ASSEMBLY_USAGE_OCCURRENCE('occ-1','Placed child','',#6,#9,$);
#15=DOCUMENT('manual','assembly manual','');",
    );

    assert_eq!(result.ir().model.product_definitions.len(), 2);
    assert_eq!(result.ir().model.occurrences.len(), 2);
    let child = result
        .ir()
        .model
        .occurrences
        .iter()
        .find(|occurrence| occurrence.name.as_deref() == Some("Placed child"))
        .expect("subtype-backed child occurrence");
    assert!(matches!(child.parent, OccurrenceParent::Occurrence { .. }));
    assert!(!result.report().losses.iter().any(|loss| {
        loss.message
            .contains("NAUO #10 references an unresolved child definition")
    }));
}

#[test]
fn geometric_bounded_surface_representation_reaches_its_product() {
    let source = String::from_utf8(
        include_bytes!("../../../tests/fixtures/ap242_geometric_set.p21").to_vec(),
    )
    .expect("fixture is UTF-8")
    .replace(
        "ENDSEC;\nEND-ISO-10303-21;",
        "#20=PRODUCT('P','bounded part','',());\n#21=PRODUCT_DEFINITION_FORMATION('','',#20);\n#22=APPLICATION_CONTEXT('mechanical design');\n#23=PRODUCT_DEFINITION_CONTEXT('part definition',#22,'design');\n#24=PRODUCT_DEFINITION('part','',#21,#23);\n#25=PRODUCT_DEFINITION_SHAPE('','',#24);\n#26=SHAPE_DEFINITION_REPRESENTATION(#25,#13);\nENDSEC;\nEND-ISO-10303-21;",
    );
    let decoded = StepCodec::default()
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .expect("decode product-bound bounded surface");

    assert_eq!(decoded.ir().model.product_definitions.len(), 1);
    assert_eq!(decoded.ir().model.product_definitions[0].bodies.len(), 1);
    assert_eq!(
        decoded.ir().model.product_definitions[0].bodies[0].as_str(),
        "step:data:body#13"
    );
}

#[test]
fn shape_representation_relationship_reaches_its_product_body() {
    let source = String::from_utf8(
        include_bytes!("../../../tests/fixtures/ap242_geometric_set.p21").to_vec(),
    )
    .expect("fixture is UTF-8")
    .replace(
        "ENDSEC;\nEND-ISO-10303-21;",
        "#14=PRODUCT('P','related shape part','',());\n#15=PRODUCT_DEFINITION_FORMATION('','',#14);\n#16=APPLICATION_CONTEXT('mechanical design');\n#17=PRODUCT_DEFINITION_CONTEXT('part definition',#16,'design');\n#18=PRODUCT_DEFINITION('part','',#15,#17);\n#19=PRODUCT_DEFINITION_SHAPE('','',#18);\n#20=SHAPE_DEFINITION_REPRESENTATION(#19,#21);\n#21=SHAPE_REPRESENTATION('',(),#2);\n#22=SHAPE_REPRESENTATION_RELATIONSHIP('','',#21,#13);\nENDSEC;\nEND-ISO-10303-21;",
    );
    let decoded = StepCodec::default()
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .expect("decode related shape representation");

    assert_eq!(decoded.ir().model.product_definitions.len(), 1);
    assert_eq!(decoded.ir().model.product_definitions[0].bodies.len(), 1);
    assert_eq!(
        decoded.ir().model.product_definitions[0].bodies[0].as_str(),
        "step:data:body#13"
    );
    assert!(!decoded.report().losses.iter().any(|loss| {
        loss.message
            .contains("has a shape representation with no committed topology body")
    }));
    let validation = cadmpeg_ir::validate_neutral(decoded.ir(), decoded.report().losses.clone());
    assert!(validation.is_ok(), "{:#?}", validation.findings);
}

#[test]
fn complex_shape_representation_relationship_inherits_references() {
    let source = String::from_utf8(
        include_bytes!("../../../tests/fixtures/ap242_geometric_set.p21").to_vec(),
    )
    .expect("fixture is UTF-8")
    .replace(
        "ENDSEC;\nEND-ISO-10303-21;",
        "#14=PRODUCT('P','complex related shape part','',());\n#15=PRODUCT_DEFINITION_FORMATION('','',#14);\n#16=APPLICATION_CONTEXT('mechanical design');\n#17=PRODUCT_DEFINITION_CONTEXT('part definition',#16,'design');\n#18=PRODUCT_DEFINITION('part','',#15,#17);\n#19=PRODUCT_DEFINITION_SHAPE('','',#18);\n#20=SHAPE_DEFINITION_REPRESENTATION(#19,#21);\n#21=SHAPE_REPRESENTATION('',(),#2);\n#22=(REPRESENTATION_RELATIONSHIP('','',#21,#13) SHAPE_REPRESENTATION_RELATIONSHIP());\nENDSEC;\nEND-ISO-10303-21;",
    );
    let decoded = StepCodec::default()
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .expect("decode complex related shape representation");

    assert_eq!(decoded.ir().model.product_definitions.len(), 1);
    assert_eq!(decoded.ir().model.product_definitions[0].bodies.len(), 1);
    assert!(!decoded.report().losses.iter().any(|loss| {
        loss.message
            .contains("has a shape representation with no committed topology body")
    }));
    let validation = cadmpeg_ir::validate_neutral(decoded.ir(), decoded.report().losses.clone());
    assert!(validation.is_ok(), "{:#?}", validation.findings);
}

#[test]
fn decode_applies_canonical_cartesian_operator_to_mapped_body() {
    let transform = cadmpeg_ir::transform::Transform {
        rows: [
            [0.0, -1.0, 0.0, 15.0],
            [1.0, 0.0, 0.0, 4.0],
            [0.0, 0.0, 1.0, 2.0],
            [0.0, 0.0, 0.0, 1.0],
        ],
    };
    let mut ir = unit_cube();
    ir.model.bodies[0].transform = Some(transform);
    let mut output = Vec::new();
    write_step(&ir, &mut output, &StepWriteOptions::default()).expect("write placed body");
    let mut source = String::from_utf8(output).expect("STEP output is UTF-8");

    let mapped_line = source
        .lines()
        .find(|line| line.contains("MAPPED_ITEM('cadmpeg body placement'"))
        .expect("mapped body item");
    let target = mapped_line
        .trim_end_matches(';')
        .trim_end_matches(')')
        .rsplit_once(',')
        .and_then(|(_, reference)| reference.strip_prefix('#'))
        .expect("mapped target reference")
        .parse::<u64>()
        .expect("mapped target id");
    let target_line = source
        .lines()
        .find(|line| {
            line.split_once('=')
                .is_some_and(|(id, _)| id.trim() == format!("#{target}"))
        })
        .expect("mapped target record");
    let parameters = target_line
        .split_once('(')
        .and_then(|(_, value)| value.strip_suffix(");"))
        .expect("mapped target parameters")
        .split(',')
        .collect::<Vec<_>>();
    assert_eq!(parameters.len(), 4, "unexpected placement target");
    let origin = parameters[1];
    let axis_z = parameters[2];
    let axis_x = parameters[3];
    let next_id = source
        .lines()
        .filter_map(|line| {
            line.strip_prefix('#')
                .and_then(|line| line.split_once('='))
                .and_then(|(id, _)| id.trim().parse::<u64>().ok())
        })
        .max()
        .expect("STEP entity ids")
        + 1;
    let axis_y = format!("#{next_id}");
    let replacement = format!(
        "#{target}=CARTESIAN_TRANSFORMATION_OPERATOR_3D('','','',{axis_x},{axis_y},{origin},1.,{axis_z});"
    );
    source = source.replace(target_line, &replacement);
    let insert_at = source.rfind("ENDSEC;").expect("data section terminator");
    source.insert_str(
        insert_at,
        &format!(
            "#{next_id}=DIRECTION('',({},{},{}));\n",
            transform.rows[0][1], transform.rows[1][1], transform.rows[2][1]
        ),
    );

    let decoded = StepCodec::default()
        .decode(
            &mut Cursor::new(source.into_bytes()),
            &DecodeOptions::default(),
        )
        .expect("decode mapped body with canonical operator");
    assert_eq!(decoded.ir().model.bodies[0].transform, Some(transform));
}
