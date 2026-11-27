// SPDX-License-Identifier: Apache-2.0
//! Datums: every element, with and without a link.

use cadmpeg_core::json;
use cadmpeg_core::serde_erase::json;

use super::{
    construction, id, native_options, point3, positive_real, refuses, selection, to_value, unread,
    Report,
};
use crate::design::construction::{kinds, AxisValue, FrameValue, PlaneValue};
use crate::design::feature::{Link, LinkState};
use crate::design::ops::datum::{Datum, DatumElement};
use crate::design::options::NativeOptions;
use crate::design::selection::DefinitionRef;
use crate::design::tests::arm_failures;
use crate::ids::ProductDefinitionId;
use crate::transform::Transform;
use crate::units::UnitVector3;

fn links() -> Vec<Link> {
    vec![
        Link {
            source: Some(selection(4)),
            document: Some(DefinitionRef {
                definition: id::<ProductDefinitionId>("product-definition", 1),
                configuration: None,
            }),
            transform: Some(Transform::identity()),
            scale: Some(positive_real(2.0)),
            associative: Some(true),
            state: Some(LinkState::OutOfDate),
            native_ref: Some("nx:extract_parms#3".to_owned()),
        },
        Link {
            source: Some(unread()),
            document: None,
            transform: None,
            scale: None,
            associative: None,
            state: None,
            native_ref: None,
        },
    ]
}

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

/// §5.10: a datum is the `FeatureOperation` arm `datum`, holding the datum as
/// its value.
#[test]
fn every_datum_is_a_feature_operation_arm() {
    use crate::design::feature::{FeatureOperation as Op, OperationFamily as Family};
    let failures = arm_failures(Family::Datum, datums(), Op::Datum);
    assert!(failures.is_empty(), "{failures:#?}");
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

/// §2.5: an unstated link field is omitted and refuses `null`.
#[test]
fn an_unstated_link_field_is_omitted_and_refuses_null() {
    let empty = Link {
        source: None,
        document: None,
        transform: None,
        scale: None,
        associative: None,
        state: None,
        native_ref: None,
    };
    assert_eq!(json::to_string(&empty).unwrap(), "{}");
    refuses::<Link>(r#"{"state": null}"#);
    refuses::<Link>(r#"{"health": "current"}"#);
}
