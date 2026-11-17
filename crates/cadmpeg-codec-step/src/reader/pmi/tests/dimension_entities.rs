// SPDX-License-Identifier: Apache-2.0

use crate::test_support::exchange::decode_inline;
use cadmpeg_ir::pmi::{DimensionKind, PmiAnnotation, PmiDefinition, PmiDimension, PmiTarget};
use cadmpeg_ir::references::InstanceTarget;

fn dimensions(ir: &cadmpeg_ir::CadIr) -> Vec<(&PmiAnnotation, &PmiDimension)> {
    ir.model
        .pmi
        .iter()
        .filter_map(|annotation| match &annotation.definition {
            PmiDefinition::Dimension(relation) => Some((annotation, relation)),
            _ => None,
        })
        .collect()
}

fn aspect(id: &str) -> InstanceTarget<PmiTarget> {
    PmiTarget::ShapeAspect {
        source_id: cadmpeg_core::text::NonBlankString::new(id).expect("nonempty source identity"),
    }
    .into()
}

fn directed_location() -> DimensionKind {
    DimensionKind::Other("directed_dimensional_location".to_owned())
}

#[test]
fn only_dimension_entities_decode_as_dimensions() {
    let result = decode_inline(
        "#1=(LENGTH_UNIT() NAMED_UNIT(*) SI_UNIT(.MILLI.,.METRE.));
#2=(GEOMETRIC_REPRESENTATION_CONTEXT(3) GLOBAL_UNIT_ASSIGNED_CONTEXT((#1)) REPRESENTATION_CONTEXT('model','3D'));
#5=PRODUCT_DEFINITION_SHAPE('PMI shape','',#99);
#6=SHAPE_ASPECT('from','',#5,.T.);
#7=SHAPE_ASPECT('to','',#5,.T.);
#10=DIRECTED_DIMENSIONAL_LOCATION('offset','',#6,#7);
#11=(LENGTH_MEASURE_WITH_UNIT() MEASURE_REPRESENTATION_ITEM() MEASURE_WITH_UNIT(POSITIVE_LENGTH_MEASURE(7.5),#1) REPRESENTATION_ITEM('nominal value'));
#12=SHAPE_DIMENSION_REPRESENTATION('offset value',(#11),#2);
#13=DIMENSIONAL_CHARACTERISTIC_REPRESENTATION(#10,#12);
#20=PRESENTATION_VIEW('Front',(),#2);
#21=DESCRIPTIVE_REPRESENTATION_ITEM('A3','');
#22=PRESENTATION_SIZE(#20,#21);
#30=SURFACE_ELEMENT_LOCATION('element point','',#6);
#99=UNRESOLVED_PRODUCT();",
    );
    let dimensions = dimensions(result.ir());
    assert_eq!(dimensions.len(), 1);
    let (annotation, relation) = dimensions[0];
    assert_eq!(relation.kind(), &directed_location());
    assert!(relation
        .nominal()
        .is_some_and(|value| (value.value.get() - 7.5).abs() < 1.0e-12));
    assert_eq!(annotation.targets, vec![aspect("#6"), aspect("#7")]);
}

#[test]
fn dimension_subtype_names_keep_their_kind() {
    let result = decode_inline(
        "#5=PRODUCT_DEFINITION_SHAPE('PMI shape','',#99);
#6=SHAPE_ASPECT('from','',#5,.T.);
#7=SHAPE_ASPECT('to','',#5,.T.);
#10=DIMENSIONAL_SIZE_WITH_PATH(#6,'width',#7);
#11=DIMENSIONAL_LOCATION_WITH_PATH('pitch','',#6,#7,#7);
#12=ANGULAR_LOCATION('turn','',#6,#7,.EQUAL.);
#99=UNRESOLVED_PRODUCT();",
    );
    let mut kinds = dimensions(result.ir())
        .into_iter()
        .map(|(annotation, relation)| (annotation.name.clone(), relation.kind().clone()))
        .collect::<Vec<_>>();
    kinds.sort_by(|a, b| a.0.cmp(&b.0));
    assert_eq!(
        kinds,
        vec![
            (Some("pitch".to_owned()), DimensionKind::Location),
            (Some("turn".to_owned()), DimensionKind::Angular),
            (Some("width".to_owned()), DimensionKind::Size),
        ]
    );
}

#[test]
fn a_directed_location_with_a_ratio_nominal_keeps_its_named_kind() {
    let result = decode_inline(
        "#1=(LENGTH_UNIT() NAMED_UNIT(*) SI_UNIT(.MILLI.,.METRE.));
#2=(GEOMETRIC_REPRESENTATION_CONTEXT(3) GLOBAL_UNIT_ASSIGNED_CONTEXT((#1)) REPRESENTATION_CONTEXT('model','3D'));
#5=PRODUCT_DEFINITION_SHAPE('PMI shape','',#99);
#6=SHAPE_ASPECT('from','',#5,.T.);
#7=SHAPE_ASPECT('to','',#5,.T.);
#10=DIRECTED_DIMENSIONAL_LOCATION('offset','',#6,#7);
#11=MEASURE_REPRESENTATION_ITEM('nominal value',POSITIVE_RATIO_MEASURE(7.5),#1);
#12=SHAPE_DIMENSION_REPRESENTATION('offset value',(#11),#2);
#13=DIMENSIONAL_CHARACTERISTIC_REPRESENTATION(#10,#12);
#99=UNRESOLVED_PRODUCT();",
    );
    let dimensions = dimensions(result.ir());
    assert_eq!(dimensions.len(), 1);
    assert_eq!(dimensions[0].1.kind(), &directed_location());
}

fn ratio_nominal_dimension(dimension: &str) -> cadmpeg_ir::codec::DecodeResult {
    decode_inline(&format!(
        "#1=(LENGTH_UNIT() NAMED_UNIT(*) SI_UNIT(.MILLI.,.METRE.));
#2=(GEOMETRIC_REPRESENTATION_CONTEXT(3) GLOBAL_UNIT_ASSIGNED_CONTEXT((#1)) REPRESENTATION_CONTEXT('model','3D'));
#5=PRODUCT_DEFINITION_SHAPE('PMI shape','',#99);
#6=SHAPE_ASPECT('from','',#5,.T.);
#10={dimension};
#11=MEASURE_REPRESENTATION_ITEM('nominal value',POSITIVE_RATIO_MEASURE(7.5),#1);
#12=SHAPE_DIMENSION_REPRESENTATION('w value',(#11),#2);
#13=DIMENSIONAL_CHARACTERISTIC_REPRESENTATION(#10,#12);
#99=UNRESOLVED_PRODUCT();"
    ))
}

#[test]
fn a_size_dimension_with_a_ratio_nominal_keeps_its_entity_kind() {
    for dimension in [
        "(BOX_SIZE() DIMENSIONAL_SIZE(#6,'w'))",
        "DIMENSIONAL_SIZE(#6,'w')",
    ] {
        let result = ratio_nominal_dimension(dimension);
        let dimensions = dimensions(result.ir());
        assert_eq!(dimensions.len(), 1, "{dimension}");
        assert_eq!(
            dimensions[0].1.kind(),
            &DimensionKind::Other("dimensional_size".to_owned())
        );
        assert!(dimensions[0]
            .1
            .nominal()
            .is_some_and(|value| (value.value.get() - 7.5).abs() < 1.0e-12));
        assert!(result.report().losses.iter().any(|loss| {
            loss.code == crate::loss::StepLossCode::PmiDimensionKindQuantityMismatch.kind()
        }));
    }
}
