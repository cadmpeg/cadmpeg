// SPDX-License-Identifier: Apache-2.0
#![allow(clippy::unwrap_used)]

use std::collections::BTreeMap;

#[test]
fn affine_parameter_map_retains_finite_ratio_of_overflowing_span() {
    let (scale, offset) = crate::entities::affine_parameter_map([-f64::MAX, f64::MAX], [0.0, 1.0])
        .expect("finite affine map");
    assert!((scale * f64::MAX - 0.5).abs() < 8.0 * f64::EPSILON);
    assert!((offset - 0.5).abs() < 8.0 * f64::EPSILON);
}

#[test]
fn affine_parameter_map_retains_identity_between_overflowing_spans() {
    assert_eq!(
        crate::entities::affine_parameter_map([-f64::MAX, f64::MAX], [-f64::MAX, f64::MAX]),
        Some((1.0, 0.0))
    );
}

#[test]
fn directed_cycle_detection_handles_long_branching_graphs_iteratively() {
    let mut graph = (1..=100_000_u32)
        .map(|sequence| (sequence, vec![sequence + 1]))
        .collect::<BTreeMap<_, _>>();
    graph.entry(50_000).or_default().push(100_001);
    let mut visited = std::collections::BTreeSet::new();

    assert!(!crate::entities::directed_cycle(
        1,
        &mut visited,
        |sequence| graph.get(&sequence).into_iter().flatten().copied()
    ));
    assert_eq!(visited.len(), 100_001);

    graph.insert(100_001, vec![50_000]);
    assert!(crate::entities::directed_cycle(
        1,
        &mut std::collections::BTreeSet::new(),
        |sequence| graph.get(&sequence).into_iter().flatten().copied()
    ));
}

#[test]
fn diagnostic_error_text_keeps_a_non_resource_error() {
    assert_eq!(
        super::non_resource_error(cadmpeg_core::CodecError::Malformed("invalid source".into()))
            .unwrap(),
        "malformed container: invalid source"
    );
}

#[test]
fn invalid_entity_parameters_report_projection_losses() {
    use crate::loss::IgesLossCode;
    use crate::test_support::test_owned::{owned_test_file, OwnedTestEntity};
    use cadmpeg_ir::codec::{Codec, DecodeOptions};

    let cases = [
        (110, 0, "110,0;", "endpoint coordinate"),
        (150, 0, "150,0;", "primitive dimensions"),
        (502, 1, "502,0;", "vertex-list count"),
        (308, 0, "308,0;", "subfigure"),
        (130, 0, "130,0;", "offset distance flag"),
        (108, 0, "108,0;", "plane coefficients"),
        (142, 0, "142,0;", "curve-on-surface"),
        (112, 0, "112,0;", "spline header"),
        (102, 0, "102,0;", "child count"),
        (212, 0, "212,0;", "text count"),
    ];
    for (entity_type, form, parameters, reason) in cases {
        let bytes = owned_test_file(&[OwnedTestEntity {
            entity_type,
            form,
            label: "INVALID".into(),
            status: "00000000",
            parameters: parameters.into(),
        }]);
        let result = crate::IgesCodec
            .decode(&mut std::io::Cursor::new(&bytes), &DecodeOptions::default())
            .unwrap();
        let expected = if matches!(entity_type, 308 | 212) {
            IgesLossCode::EntityNotProjected.kind()
        } else {
            IgesLossCode::GeometryNotProjected.kind()
        };
        assert!(
            result
                .report()
                .losses
                .iter()
                .any(|loss| loss.code == expected && loss.message.contains(reason)),
            "{entity_type}: {:#?}",
            result.report().losses
        );
    }
}
