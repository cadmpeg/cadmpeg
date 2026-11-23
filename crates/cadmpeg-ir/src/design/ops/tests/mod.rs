// SPDX-License-Identifier: Apache-2.0
//! Strict round trips, wire forms, and schemas of the sketch, datum, and
//! shared operation types.

mod datum;
#[cfg(feature = "schema")]
mod schema;
mod shared;
pub(in crate::design) mod sketch;
mod unit_variants;

use std::collections::BTreeMap;
use std::fmt::Debug;

use cadmpeg_core::decode::ResourceLimits;
use cadmpeg_core::text::NonBlankString;
use serde::de::DeserializeOwned;
use serde::Serialize;

use crate::design::construction::{
    Construction, ConstructionInput, ConstructionKind, InputReference, InputRole, PostStep,
    Relation, Subject,
};
use crate::design::dimension::Dimension;
use crate::design::options::{NativeOption, NativeOptions};
use crate::design::read;
use crate::design::selection::{
    Anchor, MemberOrder, Members, PickTarget, Picked, Picks, Selection,
};
use crate::features::geometry::FinitePoint3;
use crate::features::ParameterId;
use crate::math::{Point2, Point3};
use crate::scalar::{Angle, FiniteReal, Length, PositiveLength, PositiveReal};
use crate::units::FinitePoint2;

fn id<T>(kind: &str, key: u32) -> T
where
    T: TryFrom<String>,
    T::Error: Debug,
{
    T::try_from(format!("test:design:{kind}#{key}")).unwrap()
}

fn text(value: &str) -> NonBlankString {
    NonBlankString::new(value).unwrap()
}

fn lit<T>(value: T) -> Dimension<T> {
    Dimension::Literal(value)
}

fn bound<T>(key: u32) -> Dimension<T> {
    Dimension::Parameter(id::<ParameterId>("parameter", key))
}

fn length(value: f64) -> Length {
    Length::new(value).unwrap()
}

fn angle(value: f64) -> Angle {
    Angle::new(value).unwrap()
}

fn real(value: f64) -> FiniteReal {
    FiniteReal::new(value).unwrap()
}

fn positive(value: f64) -> PositiveLength {
    PositiveLength::new(value).unwrap()
}

fn positive_real(value: f64) -> PositiveReal {
    PositiveReal::new(value).unwrap()
}

fn point2(u: f64, v: f64) -> FinitePoint2 {
    FinitePoint2::new(Point2::new(u, v)).unwrap()
}

fn point3(x: f64, y: f64, z: f64) -> FinitePoint3 {
    FinitePoint3::new(Point3 { x, y, z }).unwrap()
}

fn selection<O: MemberOrder>(key: u32) -> Selection<O> {
    let pick = Picked {
        target: PickTarget::SketchEntity(id("sketch-entity", key)),
        context: None,
        native: None,
        domain: None,
        witness: None,
        resolved: None,
    };
    Selection {
        at: Anchor::Input,
        picks: Picks::Named(Members::new(vec![pick]).unwrap()),
        native: None,
    }
}

fn unread<O: MemberOrder>() -> Selection<O> {
    Selection {
        at: Anchor::Current,
        picks: Picks::Unread,
        native: None,
    }
}

/// A construction with one `Coincident` input and a `Flip` step where
/// `resolved` is stated, and with neither where it is not.
fn construction<K: ConstructionKind>(resolved: Option<K::Value>) -> Construction<K> {
    let (inputs, after) = if resolved.is_some() {
        let input = ConstructionInput {
            role: Some(InputRole {
                subject: Subject::Whole,
                relation: Relation::Coincident,
            }),
            reference: Some(InputReference::Picks(selection(9))),
            value: None,
            reversed: None,
        };
        (vec![input], vec![PostStep::Flip(lit(true))])
    } else {
        (Vec::new(), Vec::new())
    };
    Construction {
        inputs,
        after,
        solution: None,
        mode: None,
        resolved,
        resolved_in: BTreeMap::new(),
    }
}

fn native_options() -> NativeOptions {
    NativeOptions::from([(text("f3d:point_flags"), NativeOption::Integer(lit(5)))])
}

/// Writes `value`, reads it back strictly under the desktop ceilings, writes
/// it again, and requires the same bytes and the same value.
fn round_trip<T: Serialize + DeserializeOwned + PartialEq + Debug>(
    value: &T,
) -> Result<(), String> {
    let first = serde_json::to_string(value).map_err(|error| error.to_string())?;
    let back: T = read::from_str(&first, &ResourceLimits::desktop())
        .map_err(|error| format!("{first}: {error}"))?;
    let second = serde_json::to_string(&back).map_err(|error| error.to_string())?;
    if first != second {
        return Err(format!("{first} wrote back as {second}"));
    }
    if &back != value {
        return Err(format!("{first} read back as another value"));
    }
    Ok(())
}

fn to_value<T: Serialize>(value: &T) -> serde_json::Value {
    serde_json::to_value(value).unwrap()
}

/// Reads `text`, requires a refusal, and returns its message.
fn refuses<T: DeserializeOwned + Debug>(text: &str) -> String {
    match read::from_str::<T>(text, &ResourceLimits::desktop()) {
        Ok(value) => panic!("{text} was read as {value:?}"),
        Err(error) => error.to_string(),
    }
}

#[derive(Default)]
struct Report {
    failures: Vec<String>,
}

impl Report {
    fn check<T: Serialize + DeserializeOwned + PartialEq + Debug>(
        &mut self,
        name: &str,
        samples: Vec<T>,
    ) {
        assert!(!samples.is_empty(), "{name} has no sample");
        for sample in samples {
            if let Err(error) = round_trip(&sample) {
                self.failures.push(format!("{name}: {error}"));
            }
        }
    }
}
