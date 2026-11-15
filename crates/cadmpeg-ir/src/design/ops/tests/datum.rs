// SPDX-License-Identifier: Apache-2.0
//! Datums: every element, with and without a link.

use serde_json::json;

use super::shared::links;
use super::{construction, native_options, point3, refuses, to_value, Report};
use crate::design::construction::{kinds, AxisValue, FrameValue, PlaneValue};
use crate::design::ops::datum::{Datum, DatumElement};
use crate::design::options::NativeOptions;
use crate::units::UnitVector3;

fn elements() -> Vec<DatumElement> {
    vec![
        DatumElement::Points(vec![
            construction::<kinds::Point>(Some(point3(1.0, 2.0, 3.0))),
            construction::<kinds::Point>(None),
        ]),
        DatumElement::Axis(construction::<kinds::Axis>(Some(AxisValue {
            origin: point3(0.0, 0.0, 0.0),
            direction: UnitVector3::Z_AXIS,
        }))),
        DatumElement::Plane(construction::<kinds::Plane>(Some(PlaneValue {
            origin: point3(0.0, 0.0, 5.0),
            normal: UnitVector3::Z_AXIS,
            x_axis: UnitVector3::X_AXIS,
        }))),
        DatumElement::Frame(construction::<kinds::Frame>(Some(FrameValue {
            origin: point3(1.0, 2.0, 3.0),
            x_axis: UnitVector3::X_AXIS,
            y_axis: UnitVector3::Y_AXIS,
            z_axis: UnitVector3::Z_AXIS,
        }))),
        DatumElement::Plane(construction::<kinds::Plane>(None)),
    ]
}

fn datums() -> Vec<Datum> {
    let mut datums = elements()
        .into_iter()
        .map(|element| Datum {
            element,
            link: None,
            native_options: NativeOptions::new(),
            source_settings: native_options(),
        })
        .collect::<Vec<_>>();
    for link in links() {
        datums.push(Datum {
            element: DatumElement::Plane(construction::<kinds::Plane>(None)),
            link: Some(link),
            native_options: native_options(),
            source_settings: NativeOptions::new(),
        });
    }
    datums
}

#[test]
fn every_datum_type_round_trips() {
    let mut report = Report::default();
    report.check("Datum", datums());
    report.check("DatumElement", elements());
    assert!(report.failures.is_empty(), "{:#?}", report.failures);
}

/// §2.5: an element is adjacently tagged; the option maps are always written
/// and an absent link is omitted.
#[test]
fn a_datum_writes_its_element_and_both_option_maps() {
    let datum = Datum {
        element: DatumElement::Points(Vec::new()),
        link: None,
        native_options: NativeOptions::new(),
        source_settings: NativeOptions::new(),
    };
    assert_eq!(
        to_value(&datum),
        json!({
            "element": {"kind": "points", "value": []},
            "native_options": {},
            "source_settings": {},
        })
    );
    refuses::<Datum>(r#"{"element": {"kind": "points", "value": []}, "native_options": {}}"#);
    refuses::<Datum>(
        r#"{"element": {"kind": "points", "value": []}, "link": null, "native_options": {}, "source_settings": {}}"#,
    );
}
