use std::io::Cursor;

use cadmpeg_ir::codec::{Codec, DecodeOptions};
use cadmpeg_ir::features::{FeatureDefinition, FeatureOperation};
use cadmpeg_ir::pmi::{DimensionKind, PmiDefinition, PmiTarget};

use crate::test_support::swift::empty_hole_pattern_nominal_fixture;
use crate::SldprtCodec;

const HISTORY_HOLE_DIAMETER_TOLERANCE_MM: f64 = 1.0e-12;

#[test]
fn empty_hole_pattern_diameter_does_not_infer_nominal_from_suffix() {
    let decoded = SldprtCodec
        .decode(
            &mut Cursor::new(empty_hole_pattern_nominal_fixture()),
            &DecodeOptions::default(),
        )
        .expect("empty hole-pattern PMI decodes");

    let history = &decoded.ir().model.features;
    assert_eq!(history.len(), 3, "synthetic history has three features");
    let [seed, pattern_feature, hole] = history.as_slice() else {
        panic!("seed, pattern, and hole history features");
    };
    assert_eq!(pattern_feature.name.as_deref(), Some("LPattern6"));
    assert!(seed.ordinal < pattern_feature.ordinal);
    assert!(pattern_feature.ordinal < hole.ordinal);
    let FeatureDefinition::Operation(FeatureOperation::Pattern { seeds, .. }) =
        pattern_feature.evaluation.definition()
    else {
        panic!("linear pattern history feature");
    };
    assert_eq!(
        seeds,
        &[cadmpeg_ir::features::patterns::PatternSeed::Feature(
            seed.id.clone()
        )]
    );
    assert_eq!(
        pattern_feature.dependencies.as_slice(),
        std::slice::from_ref(&seed.id)
    );
    assert_eq!(hole.name.as_deref(), Some("Hole6"));
    assert_eq!(hole.dependencies.as_slice(), std::slice::from_ref(&seed.id));
    let FeatureDefinition::Operation(FeatureOperation::Hole { shape, .. }) =
        hole.evaluation.definition()
    else {
        panic!("hole history feature");
    };
    let history_hole_diameter = shape.diameter().expect("hole diameter").get();
    assert!(
        (history_hole_diameter - 6.0).abs() < HISTORY_HOLE_DIAMETER_TOLERANCE_MM,
        "history hole diameter is {history_hole_diameter} mm"
    );

    let annotations = &decoded.ir().model.pmi;
    assert_eq!(
        annotations.len(),
        1,
        "diameter annotation remains projected"
    );
    let annotation = annotations.first().expect("diameter annotation");
    assert_eq!(annotation.id.as_str(), "sldprt:model:pmi#A8");
    let PmiDefinition::Dimension(dimension) = &annotation.definition else {
        panic!("diameter dimension");
    };
    assert_eq!(dimension.kind(), &DimensionKind::Diameter);
    assert!(dimension.nominal().is_none(), "source supplies no nominal");
    assert!(matches!(
        annotation.targets.as_slice(),
        [PmiTarget::ShapeAspect { source_id }] if source_id.as_str() == "FP"
    ));
    assert_eq!(
        decoded
            .report()
            .losses
            .iter()
            .filter(|loss| {
                loss.code == crate::loss::SldprtLossCode::PmiSwiftImplicitNominalUnresolved.kind()
            })
            .count(),
        1,
        "unresolved implicit nominal is reported"
    );
}
