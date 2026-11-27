// SPDX-License-Identifier: Apache-2.0
//! The shared operation types: the spacing and sides of a run, and loci.

use cadmpeg_core::json;

use super::{
    angle, bound, construction, length, lit, point3, refuses, selection, text, to_value, unread,
    Report,
};
use crate::design::construction::kinds;
use crate::design::ops::shared::{Locus, RunSides, Spacing, UpToFill};
use crate::design::options::SymmetricMeasure;
use crate::scalar::{Angle, Length};

fn locus() -> Vec<Locus> {
    vec![
        Locus::Centroid,
        Locus::Point(Box::new(construction::<kinds::Point>(Some(point3(
            1.0, 2.0, 3.0,
        ))))),
    ]
}

fn length_fills() -> Vec<UpToFill<Length>> {
    vec![UpToFill::Count(lit(4)), UpToFill::Pitch(lit(length(2.5)))]
}

fn angle_fills() -> Vec<UpToFill<Angle>> {
    vec![UpToFill::Count(bound(1)), UpToFill::Pitch(lit(angle(0.5)))]
}

pub(super) fn length_spacings() -> Vec<Spacing<Length>> {
    vec![
        Spacing::Pitch {
            count: lit(4),
            pitch: lit(length(2.5)),
        },
        Spacing::Span {
            count: bound(1),
            span: bound(2),
        },
        Spacing::PitchSpan {
            pitch: lit(length(2.0)),
            span: lit(length(10.0)),
        },
        Spacing::Fit { count: lit(6) },
        Spacing::UpTo {
            reference: Box::new(selection(5)),
            fill: UpToFill::Pitch(lit(length(3.0))),
            offset: Some(lit(length(1.0))),
            from: Some(Locus::Centroid),
        },
        Spacing::UpTo {
            reference: Box::new(unread()),
            fill: UpToFill::Count(lit(3)),
            offset: None,
            from: None,
        },
        Spacing::List(vec![lit(length(1.0)), lit(length(2.0))]),
        Spacing::Native(text("nx:pattern_spacing")),
    ]
}

pub(super) fn angle_spacings() -> Vec<Spacing<Angle>> {
    vec![
        Spacing::Pitch {
            count: lit(6),
            pitch: lit(angle(1.0)),
        },
        Spacing::Span {
            count: lit(4),
            span: lit(angle(3.0)),
        },
        Spacing::Fit { count: lit(8) },
        Spacing::List(Vec::new()),
    ]
}

pub(super) fn length_sides() -> Vec<RunSides<Length>> {
    vec![
        RunSides::One,
        RunSides::Against,
        RunSides::Symmetric {
            count: Some(SymmetricMeasure::Total),
            extent: Some(SymmetricMeasure::PerSide),
        },
        RunSides::Symmetric {
            count: None,
            extent: None,
        },
        RunSides::Reverse(Box::new(Spacing::Pitch {
            count: lit(2),
            pitch: lit(length(5.0)),
        })),
    ]
}

pub(super) fn angle_sides() -> Vec<RunSides<Angle>> {
    vec![
        RunSides::One,
        RunSides::Symmetric {
            count: Some(SymmetricMeasure::PerSide),
            extent: None,
        },
        RunSides::Reverse(Box::new(Spacing::Fit { count: lit(3) })),
    ]
}

#[test]
fn every_shared_type_round_trips() {
    let mut report = Report::default();
    report.check("RunSides<Length>", length_sides());
    report.check("RunSides<Angle>", angle_sides());
    report.check("Spacing<Length>", length_spacings());
    report.check("Spacing<Angle>", angle_spacings());
    report.check("UpToFill<Length>", length_fills());
    report.check("UpToFill<Angle>", angle_fills());
    report.check("Locus", locus());
    assert!(report.failures.is_empty(), "{:#?}", report.failures);
}

/// §2.5: an adjacently tagged spacing writes its fields under `value`, and a
/// unit-only enum writes a string.
#[test]
fn the_examples_of_section_2_5() {
    let pitch = Spacing::<Length>::Pitch {
        count: lit(4),
        pitch: lit(length(2.5)),
    };
    assert_eq!(
        to_value(&pitch),
        json!({"kind": "pitch", "value": {"count": {"kind": "literal", "value": 4}, "pitch": {"kind": "literal", "value": 2.5}}})
    );
    assert_eq!(
        to_value(&Spacing::<Length>::Native(text("nx:x")))["kind"],
        json!("native")
    );
    assert_eq!(
        to_value(&RunSides::<Length>::Against),
        json!({"kind": "against"})
    );
}

/// A unit arm of an internally tagged enum writes only its tag and refuses
/// another key.
#[test]
fn a_centroid_locus_is_its_tag_alone() {
    assert_eq!(to_value(&Locus::Centroid), json!({"kind": "centroid"}));
    refuses::<Locus>(r#"{"kind": "centroid", "extra": 1}"#);
}

/// §2.5: an `Option` field refuses `null`.
#[test]
fn an_unstated_sides_count_refuses_null() {
    let message = refuses::<RunSides<Length>>(r#"{"kind": "symmetric", "value": {"count": null}}"#);
    assert!(message.contains("count"), "{message}");
}
