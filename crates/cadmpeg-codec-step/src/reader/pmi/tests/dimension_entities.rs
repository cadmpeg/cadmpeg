// SPDX-License-Identifier: Apache-2.0

use crate::test_support::exchange::decode_inline;
use cadmpeg_ir::pmi::{
    PmiAnnotation, PmiDefinition, PmiDimension, PmiDimensionKind as DimensionKind, PmiTarget,
};
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
    assert_eq!(relation.dimension, DimensionKind::DirectedLocation);
    assert!(relation
        .literal_nominal()
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
        .map(|(annotation, relation)| (annotation.name.clone(), relation.dimension.clone()))
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
    assert_eq!(dimensions[0].1.dimension, directed_location());
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
            dimensions[0].1.dimension,
            DimensionKind::Other("dimensional_size".to_owned()),
            "{dimension}"
        );
        assert!(dimensions[0]
            .1
            .literal_nominal()
            .is_some_and(|value| (value.value.get() - 7.5).abs() < 1.0e-12));
        assert!(result.report().losses.iter().any(|loss| {
            loss.code == crate::loss::StepLossCode::PmiDimensionKindQuantityMismatch.kind()
                && loss.message.contains("in place of Size")
        }));
    }
}

type MeasureItems<'a> = &'a [(&'a str, &'a str)];

fn measured_dimension(dimension: &str, items: MeasureItems<'_>) -> cadmpeg_ir::codec::DecodeResult {
    let mut records = vec![
        "#1=(LENGTH_UNIT() NAMED_UNIT(*) SI_UNIT(.MILLI.,.METRE.));".to_owned(),
        "#2=(GEOMETRIC_REPRESENTATION_CONTEXT(3) GLOBAL_UNIT_ASSIGNED_CONTEXT((#1)) REPRESENTATION_CONTEXT('model','3D'));".to_owned(),
        "#3=(NAMED_UNIT(*) PLANE_ANGLE_UNIT() SI_UNIT($,.RADIAN.));".to_owned(),
        "#5=PRODUCT_DEFINITION_SHAPE('PMI shape','',#99);".to_owned(),
        "#6=SHAPE_ASPECT('from','',#5,.T.);".to_owned(),
        "#99=UNRESOLVED_PRODUCT();".to_owned(),
        format!("#10={dimension};"),
    ];
    let mut ids = Vec::new();
    for (index, (name, measure)) in items.iter().enumerate() {
        let id = 20 + index;
        let unit = if measure.starts_with("PLANE_ANGLE") {
            3
        } else {
            1
        };
        records.push(format!(
            "#{id}=MEASURE_REPRESENTATION_ITEM('{name}',{measure},#{unit});"
        ));
        ids.push(format!("#{id}"));
    }
    records.push(format!(
        "#12=SHAPE_DIMENSION_REPRESENTATION('w',({}),#2);",
        ids.join(",")
    ));
    records.push("#13=DIMENSIONAL_CHARACTERISTIC_REPRESENTATION(#10,#12);".to_owned());
    decode_inline(&records.join(
        "
",
    ))
}

fn has_loss(
    result: &cadmpeg_ir::codec::DecodeResult,
    code: crate::loss::StepLossCode,
    text: &str,
) -> bool {
    result
        .report()
        .losses
        .iter()
        .any(|loss| loss.code == code.kind() && loss.message.contains(text))
}

#[test]
fn a_mismatched_subtype_or_angular_dimension_keeps_its_own_entity_name() {
    for (dimension, nominal, kind, replaced) in [
        (
            "DIMENSIONAL_SIZE_WITH_PATH(#6,'w',#6)",
            "POSITIVE_RATIO_MEASURE(7.5)",
            "dimensional_size_with_path",
            "in place of Size",
        ),
        (
            "ANGULAR_SIZE(#6,'w',.EQUAL.)",
            "LENGTH_MEASURE(7.5)",
            "angular_size",
            "in place of Angular",
        ),
    ] {
        let result = measured_dimension(dimension, &[("nominal value", nominal)]);
        let dimensions = dimensions(result.ir());
        assert_eq!(dimensions.len(), 1, "{dimension}");
        assert_eq!(
            dimensions[0].1.dimension,
            DimensionKind::Other(kind.to_owned()),
            "{dimension}"
        );
        assert!(
            has_loss(
                &result,
                crate::loss::StepLossCode::PmiDimensionKindQuantityMismatch,
                replaced
            ),
            "{dimension}"
        );
    }
}

#[test]
fn limits_that_no_kind_accepts_are_omitted_from_the_dimension() {
    let cases: [(MeasureItems<'_>, DimensionKind, bool); 3] = [
        (
            &[
                ("nominal value", "LENGTH_MEASURE(7.5)"),
                ("lower limit", "PLANE_ANGLE_MEASURE(1.0)"),
                ("upper limit", "PLANE_ANGLE_MEASURE(2.0)"),
            ],
            DimensionKind::Size,
            false,
        ),
        (
            &[
                ("nominal value", "LENGTH_MEASURE(7.5)"),
                ("lower limit", "LENGTH_MEASURE(8.0)"),
                ("upper limit", "LENGTH_MEASURE(7.0)"),
            ],
            DimensionKind::Size,
            false,
        ),
        (
            &[
                ("nominal value", "POSITIVE_RATIO_MEASURE(7.5)"),
                ("lower limit", "LENGTH_MEASURE(7.0)"),
                ("upper limit", "LENGTH_MEASURE(8.0)"),
            ],
            DimensionKind::Other("dimensional_size".to_owned()),
            true,
        ),
    ];
    for (items, kind, replaced) in cases {
        let result = measured_dimension("DIMENSIONAL_SIZE(#6,'w')", items);
        let dimensions = dimensions(result.ir());
        assert_eq!(dimensions.len(), 1, "{items:?}");
        assert_eq!(dimensions[0].1.dimension, kind, "{items:?}");
        assert!(dimensions[0].1.tolerance.as_ref().is_none(), "{items:?}");
        assert!(dimensions[0]
            .1
            .literal_nominal()
            .is_some_and(|value| (value.value.get() - 7.5).abs() < 1.0e-12));
        assert!(
            has_loss(
                &result,
                crate::loss::StepLossCode::DimensionalLimitsOmitted,
                "keeps no limits"
            ),
            "{items:?}"
        );
        assert_eq!(
            has_loss(
                &result,
                crate::loss::StepLossCode::PmiDimensionKindQuantityMismatch,
                "in place of Size"
            ),
            replaced,
            "{items:?}"
        );
    }
}

#[test]
fn a_size_dimension_with_angle_limits_keeps_its_entity_kind() {
    let result = decode_inline(
        "#1=(LENGTH_UNIT() NAMED_UNIT(*) SI_UNIT(.MILLI.,.METRE.));
#2=(GEOMETRIC_REPRESENTATION_CONTEXT(3) GLOBAL_UNIT_ASSIGNED_CONTEXT((#1)) REPRESENTATION_CONTEXT('model','3D'));
#3=(NAMED_UNIT(*) PLANE_ANGLE_UNIT() SI_UNIT($,.RADIAN.));
#5=PRODUCT_DEFINITION_SHAPE('PMI shape','',#99);
#6=SHAPE_ASPECT('from','',#5,.T.);
#10=DIMENSIONAL_SIZE(#6,'w');
#11=MEASURE_REPRESENTATION_ITEM('lower limit',PLANE_ANGLE_MEASURE(1.0),#3);
#12=MEASURE_REPRESENTATION_ITEM('upper limit',PLANE_ANGLE_MEASURE(2.0),#3);
#13=SHAPE_DIMENSION_REPRESENTATION('w',(#11,#12),#2);
#14=DIMENSIONAL_CHARACTERISTIC_REPRESENTATION(#10,#13);
#99=UNRESOLVED_PRODUCT();",
    );
    let dimensions = dimensions(result.ir());
    assert_eq!(dimensions.len(), 1);
    assert_eq!(
        dimensions[0].1.dimension,
        DimensionKind::Other("dimensional_size".to_owned())
    );
    assert!(result.report().losses.iter().any(|loss| {
        loss.code == crate::loss::StepLossCode::PmiDimensionKindQuantityMismatch.kind()
            && loss.message.contains("in place of Size")
    }));
}

#[test]
fn incomplete_dimensional_limits_record_a_loss() {
    let result = decode_inline(
        &crate::test_support::pmi_limits::LIMIT_DIMENSION.replace("(#12,#11,#13)", "(#12,#11)"),
    );
    let limits = result
        .report()
        .losses
        .iter()
        .filter(|loss| loss.code == crate::loss::StepLossCode::DimensionalLimitsOmitted.kind())
        .collect::<Vec<_>>();
    assert_eq!(limits.len(), 1, "{:#?}", result.report().losses);
    assert!(limits[0]
        .message
        .contains("#15 has 0 lower and 1 upper limit measures"));
    assert!(result.ir().model.pmi.iter().any(|annotation| matches!(
        &annotation.definition,
        PmiDefinition::Dimension(relation)
            if relation.tolerance.as_ref().is_none()
                && relation.literal_nominal().is_some_and(|value| (value.value.get() - 35.0).abs() < 1.0e-12)
    )));
}
