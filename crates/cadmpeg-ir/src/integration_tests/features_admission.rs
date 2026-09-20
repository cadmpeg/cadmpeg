// SPDX-License-Identifier: Apache-2.0
use crate::features::edge_treatments::{FullRoundFilletGroup, FullRoundSideSelection};
use crate::features::holes::{CounterdrillDiameters, HoleConstruction, HoleKind, HoleShape};
use crate::features::{
    BodySelection, FaceBlendOperands, FaceSelection, GeneratedSweepSection, ReplaceFaceOperands,
    SewBodySelection, SweepCircularRegion, SweepMode, SweepSection, SweepShape,
};
use crate::scalar::{InteriorAngle, PositiveLength};

fn positive(value: f64) -> PositiveLength {
    PositiveLength::new(value).unwrap()
}

#[test]
fn hole_and_sweep_edits_preserve_the_previous_admitted_shape() {
    assert!(CounterdrillDiameters::new(positive(4.0), Some(positive(4.0))).is_err());
    let treatment = HoleConstruction::form(HoleKind::Counterbore {
        diameter: positive(4.0),
        depth: positive(2.0),
    });
    assert!(HoleShape::new(treatment.clone(), None, None).is_err());
    assert!(HoleShape::new(treatment.clone(), None, Some(positive(4.0))).is_err());
    let mut hole = HoleShape::new(treatment, None, Some(positive(2.0))).unwrap();
    let before = hole.clone();
    assert!(hole
        .try_edit(|_, _, diameter| *diameter = Some(positive(5.0)))
        .is_err());
    assert_eq!(hole, before);
    assert!(HoleShape::new(
        HoleConstruction::form(HoleKind::Simple),
        Some(HoleKind::Countersink {
            diameter: positive(2.0),
            angle: InteriorAngle::new(1.0).unwrap(),
        }),
        Some(positive(2.0))
    )
    .is_err());
    assert!(HoleShape::new(
        HoleConstruction::form(HoleKind::PartialCounterbore(
            crate::features::holes::PartialPair::First(positive(1.0)),
        )),
        None,
        Some(positive(2.0))
    )
    .is_ok());
    let circular = SweepSection::Generated(GeneratedSweepSection::CircularRegion {
        region: SweepCircularRegion::new(positive(2.0), Some(positive(1.0))).unwrap(),
    });
    let sweep = SweepShape::Solid {
        op: crate::features::SolidSweepOperation::NewBody,
        section: SweepSection::Unresolved(None),
        sections: vec![circular],
    };
    assert!(sweep.generated_section().is_none());
    assert_eq!(sweep.additional_section_count(), 1);
    // A sheet result carries sections whose generated arm is uninhabited, so a
    // circular region under one has no spelling in the type or on the wire.
    let mut sheet = SweepShape::sheet_sections(
        SweepMode::Surface {},
        SweepSection::Unresolved(None),
        Vec::new(),
    );
    assert!(sheet.generated_sections_mut().is_empty());
    let wire = serde_json::to_value(&sweep).expect("a sweep shape serializes");
    assert_eq!(wire["mode"], "solid");
    let mut sheet_wire = serde_json::to_value(&sheet).expect("a sweep shape serializes");
    sheet_wire["sections"] = wire["sections"].clone();
    serde_json::from_value::<SweepShape>(sheet_wire)
        .expect_err("a sheet result generates no geometry");
}

#[test]
fn face_operand_and_full_round_owners_reject_each_overlap() {
    let face = |name: &str| {
        FaceSelection::Faces(vec![crate::ids::FaceId::mint(format!(
            "test:model:face#{name}"
        ))
        .unwrap()])
    };
    let center = face("center");
    let first = face("first");
    let second = face("second");
    assert!(FaceBlendOperands::new(center.clone(), center.clone()).is_err());
    assert!(ReplaceFaceOperands::new(center.clone(), center.clone()).is_err());
    for (one, two) in [
        (center.clone(), second.clone()),
        (first.clone(), center.clone()),
        (first.clone(), first.clone()),
    ] {
        assert!(FullRoundFilletGroup::new(
            center.clone(),
            FullRoundSideSelection::Explicit(one),
            FullRoundSideSelection::Explicit(two)
        )
        .is_err());
    }
    assert!(FullRoundFilletGroup::new(
        center.clone(),
        FullRoundSideSelection::Explicit(first),
        FullRoundSideSelection::Explicit(second.clone())
    )
    .is_ok());
    assert!(FullRoundFilletGroup::new(
        center.clone(),
        FullRoundSideSelection::Explicit(center),
        FullRoundSideSelection::Explicit(second)
    )
    .is_err());
    assert!(SewBodySelection::try_from(BodySelection::NativeSet(
        vec!["single".into()].try_into().unwrap()
    ))
    .is_ok());
}
