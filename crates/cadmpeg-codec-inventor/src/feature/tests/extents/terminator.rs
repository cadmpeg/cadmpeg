// SPDX-License-Identifier: Apache-2.0
//! To Next terminators of an extrusion side.

use super::*;

fn terminator_side(items: &[u32]) -> Option<Feature> {
    extent_extrusion(
        4,
        false,
        &[(24, 20)],
        vec![test_property(
            20,
            PmDcFeaturePropertyKind::References {
                family: PmDcFeatureReferenceFamily::BodySelection,
                items: reference_list(items),
            },
        )],
    )
}

#[test]
fn to_next_without_a_terminator_projects_as_the_first_face_limit() {
    let one_sided = extent_extrusion(4, false, &[], Vec::new()).expect("to next");
    assert!(matches!(
        extrude_definition(&one_sided).0,
        ExtrudeExtent::OneSided {
            side: ExtrudeSide {
                termination: LinearTermination::ToFirst {},
                draft: None,
            }
        }
    ));
}

#[test]
fn to_next_terminator_on_the_result_body_projects_as_the_first_face_limit() {
    let own = terminator_side(&[9]).expect("terminator on the result body");
    assert!(matches!(
        extrude_definition(&own).0,
        ExtrudeExtent::OneSided {
            side: ExtrudeSide {
                termination: LinearTermination::ToFirst {},
                ..
            }
        }
    ));
}

#[test]
fn to_next_terminator_on_another_body_stays_unresolved() {
    for items in [&[11][..], &[9, 11][..], &[][..]] {
        let other = terminator_side(items).expect("terminator beyond the result body");
        assert!(
            matches!(
                extrude_definition(&other).0,
                ExtrudeExtent::OneSided {
                    side: ExtrudeSide {
                        termination: LinearTermination::Unresolved {},
                        ..
                    }
                }
            ),
            "{items:?}"
        );
    }
}

#[test]
fn to_next_second_side_has_no_read_terminator() {
    let two_sided = extent_extrusion(
        1,
        false,
        &[(27, 72), (28, 71), (29, 22)],
        vec![enum_property(22, PmDcFeatureEnumFamily::Extent, 4)],
    )
    .expect("to next on the second side");
    assert!(matches!(
        extrude_definition(&two_sided).0,
        ExtrudeExtent::TwoSided {
            first: ExtrudeSide {
                termination: LinearTermination::Blind { .. },
                ..
            },
            second: ExtrudeSide {
                termination: LinearTermination::Unresolved {},
                ..
            },
        }
    ));
}
