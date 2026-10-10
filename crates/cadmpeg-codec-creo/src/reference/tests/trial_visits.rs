// SPDX-License-Identifier: Apache-2.0
use super::super::arc_z_fields;
use crate::scalar::ScalarCache;



#[test]
fn arc_trials_admit_two_present_offset_passes_without_a_terminal_visit() {
    let valid = b"\x01\xe4\xe4\x0f\x0f\x43\xf0\x00\x0f\x0f".as_slice();
    for (body, valid) in [(b"\x00\x00\x00".as_slice(), false), (valid, true)] {
        let cache = ScalarCache::default();
        let circle =
            crate::test_support::assert_work_boundaries(&["creo arc-z numeric trials"], |ctx| {
                arc_z_fields(ctx, body, &cache, 7)
            });
        if valid {
            let circle = circle.expect("diameter image");
            assert_eq!(circle.entity_id, 7);
            assert_eq!(circle.offset, 1);
            assert_eq!(<[f64; 3]>::from(circle.center().get()), [0.0; 3]);
            assert_eq!(circle.radius().get(), 1.0);
            assert_eq!(<[f64; 3]>::from(circle.start().get()), [1.0, 0.0, 0.0]);
            assert_eq!(<[f64; 3]>::from(circle.end().get()), [-1.0, 0.0, 0.0]);
            assert!(!circle.center_stored());
        } else {
            assert!(circle.is_none());
        }
    }
}


#[test]
fn singleton_reference_ellipse_admits_only_its_source_visit() {
    use cadmpeg_ir::features::FinitePoint3;
    use cadmpeg_ir::scalar::{FiniteReal, PositiveLength};
    use cadmpeg_ir::units::{FiniteVector, UnitVector3};
    let conic = super::super::ReferenceConic {
        entity_id: 7,
        type_id: super::super::ConicType::Ellipse,
        flip: 1,
        start: FinitePoint3::new([-1.0, 0.0, 0.0].into()).expect("start"),
        end: FinitePoint3::new([1.0, 0.0, 0.0].into()).expect("end"),
        parameter_start: None,
        parameter_end: None,
        coefficient_1: FiniteReal::ONE,
        coefficient_2: FiniteReal::ONE,
        local_system: FiniteVector::new([
            1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0,
        ]),
        body: Vec::new(),
        offset: 12,
    };
    let expected = super::super::ReferenceEllipse::try_new(
        7,
        FinitePoint3::new([0.0; 3].into()).expect("center"),
        UnitVector3::Z_AXIS,
        UnitVector3::X_AXIS.reversed(),
        [PositiveLength::new(1.0).expect("radius"); 2],
        12,
    )
    .expect("circle is an ellipse");
    let actual =
        crate::test_support::assert_work_boundaries(&["creo reference conic traversal"], |ctx| {
            super::super::ellipse_carriers(ctx, std::slice::from_ref(&conic))
        });
    assert_eq!(actual, [expected]);
}
