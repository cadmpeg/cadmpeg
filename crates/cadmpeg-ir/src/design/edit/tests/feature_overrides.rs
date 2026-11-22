// SPDX-License-Identifier: Apache-2.0
//! Constructions of feature overrides exchange when a configuration switches.

use super::*;

/// A point datum on one coincident input, at (1, 0, 0) in the active
/// configuration and at (2, 0, 0) in configuration 2.
fn point_datum(key: u32) -> Feature {
    let point = |x: f64| FinitePoint3::from_coordinates(real(x), real(0.0), real(0.0));
    operation(
        key,
        FeatureOperation::Datum(Box::new(Datum {
            element: DatumElement::Points(vec![Construction {
                inputs: vec![ConstructionInput {
                    role: Some(InputRole {
                        subject: Subject::Whole,
                        relation: Relation::Coincident,
                    }),
                    reference: None,
                    value: None,
                    reversed: None,
                }],
                after: Vec::new(),
                solution: None,
                mode: None,
                resolved: Some(point(1.0)),
                resolved_in: BTreeMap::from([(configuration(2), point(2.0))]),
            }]),
            link: None,
            native_options: NativeOptions::new(),
            source_settings: NativeOptions::new(),
        })),
    )
}

#[test]
fn switching_exchanges_the_constructions_of_feature_overrides() {
    let mut document = configured();
    document.design.features[0] = point_datum(1);
    document.design.spreadsheets.clear();
    let state = FeatureStateOverride {
        suppressed: None,
        definition: Some(document.design.features[0].definition.clone()),
    };
    document.design.configurations[1].overrides.features =
        BTreeMap::from([(feature(1), state.clone())]);
    let mut arranged = arrangement(1, BTreeMap::new());
    arranged.overrides.features = BTreeMap::from([(feature(1), state)]);
    document.design.arrangements.push(arranged);

    document
        .edit()
        .unwrap()
        .set_active_configuration(&definition(), &configuration(2))
        .unwrap();

    let construction = |definition: &FeatureDefinition| {
        let FeatureOperation::Datum(datum) = &definition.operation else {
            panic!("{definition:?} is a datum");
        };
        let DatumElement::Points(points) = &datum.element else {
            panic!("{datum:?} holds points");
        };
        (
            points[0].resolved.map(|point| point.get().x),
            points[0].resolved_in.keys().cloned().collect::<Vec<_>>(),
        )
    };
    let expected = (Some(2.0), vec![configuration(1)]);
    assert_eq!(
        construction(&document.design.features[0].definition),
        expected
    );
    let overridden = |overrides: &Overrides| {
        construction(overrides.features[&feature(1)].definition.as_ref().unwrap())
    };
    assert_eq!(
        overridden(&document.design.configurations[1].overrides),
        expected,
        "a configuration's feature override"
    );
    assert_eq!(
        overridden(&document.design.arrangements[0].overrides),
        expected,
        "an arrangement's feature override"
    );
}
