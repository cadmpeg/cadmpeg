// SPDX-License-Identifier: Apache-2.0

use crate::design::decode::parameters::parse_design_parameter_record;
use crate::design::feature_project::project_hole;
use crate::design::test_support::parameter_record;
use crate::records::feature::scope::{DesignFeatureKind, DesignParameterScope};
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
use cadmpeg_core::CodecError;
use cadmpeg_ir::features::{FaceSelection, FeatureDefinition, FeatureOperation};

fn hole_parameters() -> [crate::records::parameters::DesignParameter; 3] {
    let mut tip = parse_design_parameter_record(&parameter_record(
        Some(114),
        "180 deg",
        "TipAngle",
        Some("mm"),
        "d6",
        std::f64::consts::PI,
    ))
    .unwrap();
    tip.try_set_unit_value("deg".to_owned()).unwrap();
    [
        parse_design_parameter_record(&parameter_record(
            Some(94),
            "10 mm",
            "HoleDepth",
            Some("mm"),
            "d4",
            1.0,
        ))
        .unwrap(),
        parse_design_parameter_record(&parameter_record(
            Some(104),
            "4 mm",
            "HoleDiameter",
            Some("mm"),
            "d5",
            0.4,
        ))
        .unwrap(),
        tip,
    ]
}

#[test]
fn hole_fallback_face_id_refuses_retained_limit() {
    let scope = DesignParameterScope::empty(
        "f3d:Design/BulkStream.dat:scope#32",
        DesignFeatureKind::Hole,
        32,
    );
    let parameters = hole_parameters();
    let indexed = [
        (0, &parameters[0]),
        (1, &parameters[1]),
        (2, &parameters[2]),
    ];
    let definition = crate::test_support::with_decode_context(|decode_ctx| project_hole(decode_ctx, &scope, &indexed, &[])).unwrap().unwrap();
    assert!(matches!(
        definition,
        FeatureDefinition::Operation(FeatureOperation::Hole {
            face: Some(FaceSelection::Native(id)), ..
        }) if id == scope.id
    ));

    for limit in 0..128 {
        let mut policy = DecodePolicy::default();
        policy.limits.max_retained_bytes = limit;
        let arena = DecodeArena::new();
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        if matches!(
            project_hole(&ctx, &scope, &indexed, &[]),
            Err(CodecError::ResourceLimit(failure))
                if failure.dimension == ResourceDimension::RetainedBytes
                    && failure.operation == "f3d Hole fallback face id"
        ) {
            return;
        }
    }
    panic!("no Hole fallback face ID refusal");
}
