// SPDX-License-Identifier: Apache-2.0
//! Tolerance-zone constraints survive semantic STEP transfer.

use std::io::Cursor;

use cadmpeg_ir::codec::{Codec, DecodeOptions};
use cadmpeg_ir::pmi::zones::ToleranceConstraints;
use cadmpeg_ir::pmi::PmiDefinition;

use crate::export::write_step;
use crate::test_support::exchange::decode_inline;
use crate::{StepCodec, StepSchema, StepWriteOptions};

const EPS_ZONE_LENGTH: f64 = 1.0e-12;
const ZONES: &str = "#1=(LENGTH_UNIT() NAMED_UNIT(*) SI_UNIT(.MILLI.,.METRE.));
#2=(GEOMETRIC_REPRESENTATION_CONTEXT(3) GLOBAL_UNIT_ASSIGNED_CONTEXT((#1)) REPRESENTATION_CONTEXT('model','3D'));
#5=PRODUCT_DEFINITION_SHAPE('PMI shape','',#99);
#6=SHAPE_ASPECT('feature','',#5,.T.);
#10=(GEOMETRIC_TOLERANCE('profile','',#11,#6) SURFACE_PROFILE_TOLERANCE() UNEQUALLY_DISPOSED_GEOMETRIC_TOLERANCE(#12));
#11=LENGTH_MEASURE_WITH_UNIT(LENGTH_MEASURE(2.5),#1);
#12=LENGTH_MEASURE_WITH_UNIT(LENGTH_MEASURE(0.75),#1);
#20=TOLERANCE_ZONE('','',#5,.F.,(#10),#21);
#21=TOLERANCE_ZONE_FORM('cylindrical or circular');
#30=PROJECTED_ZONE_DEFINITION_WITH_OFFSET(#20,(#6),#6,#31,#32);
#31=LENGTH_MEASURE_WITH_UNIT(LENGTH_MEASURE(50.),#1);
#32=LENGTH_MEASURE_WITH_UNIT(LENGTH_MEASURE(-2.),#1);
#40=(GEOMETRIC_TOLERANCE('position','',#41,#6) GEOMETRIC_TOLERANCE_WITH_MAXIMUM_TOLERANCE(#42) GEOMETRIC_TOLERANCE_WITH_MODIFIERS((.MAXIMUM_MATERIAL_REQUIREMENT.)) POSITION_TOLERANCE());
#41=LENGTH_MEASURE_WITH_UNIT(LENGTH_MEASURE(0.1),#1);
#42=LENGTH_MEASURE_WITH_UNIT(LENGTH_MEASURE(0.2),#1);
#99=UNRESOLVED_PRODUCT();";

fn constraints<'a>(
    result: &'a cadmpeg_ir::codec::DecodeResult,
    name: &str,
) -> &'a ToleranceConstraints {
    result
        .ir()
        .model
        .pmi
        .iter()
        .find_map(|annotation| {
            let PmiDefinition::GeometricTolerance { constraints, .. } = &annotation.definition
            else {
                return None;
            };
            (annotation.name.as_deref() == Some(name)).then_some(constraints)
        })
        .expect("named tolerance")
}

fn assert_constraints(result: &cadmpeg_ir::codec::DecodeResult) {
    let profile = constraints(result, "profile");
    assert!((profile.displacement.expect("displacement").get() - 0.75).abs() < EPS_ZONE_LENGTH);
    let [zone] = profile.zones.as_slice() else {
        panic!("one zone: {profile:?}")
    };
    assert_eq!(zone.form.as_str(), "cylindrical or circular");
    let [projection] = zone.projections.as_slice() else {
        panic!("one projection: {zone:?}")
    };
    assert!((projection.length.get() - 50.0).abs() < EPS_ZONE_LENGTH);
    assert!((projection.offset.expect("offset").get() + 2.0).abs() < EPS_ZONE_LENGTH);
    assert_eq!(
        projection.boundaries.as_slice(),
        std::slice::from_ref(&projection.end)
    );
    assert!(
        (constraints(result, "position")
            .maximum
            .expect("maximum")
            .get()
            - 0.2)
            .abs()
            < EPS_ZONE_LENGTH
    );
    assert!(!result
        .report()
        .losses
        .iter()
        .any(|loss| loss.code.local_code() == "pmi.zone-unresolved"));
    assert!(cadmpeg_ir::validate_neutral(result.ir(), Vec::new())
        .expect("validate")
        .is_ok());
}

#[test]
fn projected_and_unequal_tolerance_zones_round_trip() {
    let result = decode_inline(ZONES);
    assert_constraints(&result);
    let mut output = Vec::new();
    write_step(
        result.ir(),
        &mut output,
        StepSchema::Ap242Edition3,
        &StepWriteOptions::default(),
    )
    .expect("write zones");
    let text = std::str::from_utf8(&output).expect("STEP text");
    for name in [
        "UNEQUALLY_DISPOSED_GEOMETRIC_TOLERANCE",
        "GEOMETRIC_TOLERANCE_WITH_MAXIMUM_TOLERANCE",
        "PROJECTED_ZONE_DEFINITION_WITH_OFFSET",
    ] {
        assert!(text.contains(name), "{name} in {text}");
    }
    let roundtrip = StepCodec::default()
        .decode(&mut Cursor::new(output), &DecodeOptions::default())
        .expect("decode zones");
    assert_constraints(&roundtrip);
}

#[test]
fn complex_zone_records_use_attributes_of_the_owning_partials() {
    let source = ZONES.replace("#20=TOLERANCE_ZONE('','',#5,.F.,(#10),#21);",
        "#20=(SHAPE_ASPECT('','',#5,.F.) TOLERANCE_ZONE((#10),#21));")
        .replace("#30=PROJECTED_ZONE_DEFINITION_WITH_OFFSET(#20,(#6),#6,#31,#32);",
        "#30=(PROJECTED_ZONE_DEFINITION(#6,#31) PROJECTED_ZONE_DEFINITION_WITH_OFFSET(#32) TOLERANCE_ZONE_DEFINITION(#20,(#6)));");
    assert_constraints(&decode_inline(&source));
}

#[test]
fn invalid_projection_is_reported_instead_of_becoming_unprojected() {
    for bad in ["0.", "-50."] {
        let result =
            decode_inline(&ZONES.replace("LENGTH_MEASURE(50.)", &format!("LENGTH_MEASURE({bad})")));
        assert!(constraints(&result, "profile").zones[0]
            .projections
            .is_empty());
        assert!(result
            .report()
            .losses
            .iter()
            .any(|loss| loss.code.local_code() == "pmi.zone-unresolved"));
    }
}

#[test]
fn spherical_dimension_modifiers_round_trip_through_compound_items() {
    use crate::test_support::pmi_limits::LIMIT_DIMENSION;
    use cadmpeg_ir::pmi::DimensionKind;
    let source = LIMIT_DIMENSION
        .replace("'diameter'", "'spherical diameter'")
        .replace("(#12,#11,#13)", "(#12,#11,#13,#16,#19)");
    let source = format!("{source}\n#16=DESCRIPTIVE_REPRESENTATION_ITEM('dimensional note','auxiliary');\n#17=DESCRIPTIVE_REPRESENTATION_ITEM('dimensional note','statistical');\n#18=(DESCRIPTIVE_REPRESENTATION_ITEM('controlled radius') REPRESENTATION_ITEM('dimensional note'));\n#19=COMPOUND_REPRESENTATION_ITEM('dimensional note',(#17,#18));");
    let input = decode_inline(&source);
    let mut output = Vec::new();
    write_step(
        input.ir(),
        &mut output,
        StepSchema::Ap242Edition3,
        &StepWriteOptions::default(),
    )
    .expect("write modified dimension");
    let roundtrip = StepCodec::default()
        .decode(&mut Cursor::new(output), &DecodeOptions::default())
        .expect("decode modified dimension");
    for result in [&input, &roundtrip] {
        let dimension = result
            .ir()
            .model
            .pmi
            .iter()
            .find_map(|annotation| {
                if let PmiDefinition::Dimension(dimension) = &annotation.definition {
                    Some(dimension)
                } else {
                    None
                }
            })
            .expect("dimension");
        assert_eq!(dimension.kind(), &DimensionKind::SphericalDiameter);
        assert_eq!(
            dimension.modifiers(),
            ["auxiliary", "controlled radius", "statistical"]
        );
    }
}

#[test]
fn shared_tolerance_zone_ownership_is_reported_without_independent_zone_clones() {
    let input = ZONES.replace("(#10),#21", "(#10,#40),#21");
    let result = decode_inline(&input);
    assert!(constraints(&result, "profile").zones.is_empty());
    assert!(constraints(&result, "position").zones.is_empty());
    assert!(result
        .report()
        .losses
        .iter()
        .any(|loss| loss.code.local_code() == "pmi.zone-unresolved"));
}

#[test]
fn a_projection_boundary_must_reference_a_shape_aspect() {
    let input = ZONES.replace("(#20,(#6),#6,#31,#32)", "(#20,(#99),#6,#31,#32)");
    let result = decode_inline(&input);
    let [zone] = constraints(&result, "profile").zones.as_slice() else {
        panic!("one zone form")
    };
    assert!(zone.projections.is_empty());
    assert!(result
        .report()
        .losses
        .iter()
        .any(|loss| loss.code.local_code() == "pmi.zone-unresolved"));
}
