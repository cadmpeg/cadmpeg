// SPDX-License-Identifier: Apache-2.0
//! Every pattern, template, body operation, and shared type of §6.5, written,
//! read back strictly, and written again; the stated wire forms; and the
//! schema of each type.

mod joints;
mod operations;
mod patterns;
mod unit_variants;

use cadmpeg_core::serde_erase::json;
use std::collections::BTreeMap;
use std::fmt::Debug;

use cadmpeg_core::decode::ResourceLimits;
use cadmpeg_core::text::NonBlankString;
use serde::de::DeserializeOwned;
use serde::Serialize;

use crate::design::construction::{
    kinds, AxisValue, Construction, ConstructionInput, ConstructionKind, FrameValue,
    InputReference, InputRole, PlaneValue, Relation, Subject,
};
use crate::design::dimension::Dimension;
use crate::design::names::{NameKind, TopoName};
use crate::design::options::{NativeOption, NativeOptions};
use crate::design::read;
use crate::design::selection::{
    Anchor, MemberOrder, Members, OrderedSelection, PickTarget, Picked, Picks, Selection,
    SetSelection,
};
use crate::features::geometry::FinitePoint3;
use crate::features::ParameterId;
use crate::math::Point3;
use crate::scalar::{Angle, Length};
use crate::units::UnitVector3;

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

fn literal<T>(value: T) -> Dimension<T> {
    Dimension::Literal(value)
}

fn parameter<T>(key: u32) -> Dimension<T> {
    Dimension::Parameter(id::<ParameterId>("parameter", key))
}

fn length(value: f64) -> Length {
    Length::new(value).unwrap()
}

fn angle(value: f64) -> Angle {
    Angle::new(value).unwrap()
}

fn point(x: f64, y: f64, z: f64) -> FinitePoint3 {
    FinitePoint3::new(Point3 { x, y, z }).unwrap()
}

fn picked(kind: NameKind, key: u32) -> Picked {
    Picked {
        target: PickTarget::Entity(TopoName::Generated {
            kind,
            feature: id("feature", key),
            role: None,
            generator: None,
            index: None,
        }),
        context: None,
        native: None,
        domain: None,
        witness: None,
        resolved: None,
    }
}

fn selection<O: MemberOrder>(key: u32) -> Selection<O> {
    Selection {
        at: Anchor::Input,
        picks: Picks::Named(Members::new(vec![picked(NameKind::Face, key)]).unwrap()),
        native: None,
    }
}

fn faces(key: u32) -> SetSelection {
    selection(key)
}

fn ordered(key: u32) -> OrderedSelection {
    selection(key)
}

fn unread<O: MemberOrder>() -> Selection<O> {
    Selection {
        at: Anchor::Current,
        picks: Picks::Unread,
        native: None,
    }
}

fn construction<K: ConstructionKind>(resolved: Option<K::Value>) -> Construction<K> {
    Construction {
        inputs: vec![ConstructionInput {
            role: Some(InputRole {
                subject: Subject::Whole,
                relation: Relation::Coincident,
            }),
            reference: Some(InputReference::Picks(selection(90))),
            value: None,
            reversed: None,
        }],
        after: Vec::new(),
        solution: None,
        mode: None,
        resolved,
        resolved_in: BTreeMap::new(),
    }
}

fn point_construction() -> Construction<kinds::Point> {
    construction(Some(point(1.0, 2.0, 3.0)))
}

fn direction_construction() -> Construction<kinds::Direction> {
    construction(Some(UnitVector3::Z_AXIS))
}

fn axis_construction() -> Construction<kinds::Axis> {
    construction(Some(AxisValue {
        origin: point(0.0, 0.0, 0.0),
        direction: UnitVector3::Z_AXIS,
    }))
}

fn plane_construction() -> Construction<kinds::Plane> {
    construction(Some(PlaneValue {
        origin: point(0.0, 0.0, 0.0),
        normal: UnitVector3::Z_AXIS,
        x_axis: UnitVector3::X_AXIS,
    }))
}

fn frame_construction() -> Construction<kinds::Frame> {
    construction(Some(FrameValue {
        origin: point(0.0, 0.0, 1.0),
        x_axis: UnitVector3::X_AXIS,
        y_axis: UnitVector3::Y_AXIS,
        z_axis: UnitVector3::Z_AXIS,
    }))
}

fn options() -> NativeOptions {
    NativeOptions::from([(
        crate::codec_name!("nx:single_output"),
        NativeOption::Bool(literal(true)),
    )])
}

fn settings() -> NativeOptions {
    NativeOptions::from([(
        crate::codec_name!("sldprt:flex_accuracy"),
        NativeOption::Integer(literal(3)),
    )])
}

/// Writes `value`, reads it back strictly under the desktop ceilings, writes
/// it again, and requires the same bytes and the same value.
fn round_trip<T: Serialize + DeserializeOwned + PartialEq + Debug>(
    value: &T,
) -> Result<(), String> {
    let first = json::to_string(value).map_err(|error| error.to_string())?;
    let back: T = read::from_str(&first, &ResourceLimits::desktop())
        .map_err(|error| format!("{first}: {error}"))?;
    let second = json::to_string(&back).map_err(|error| error.to_string())?;
    if first != second {
        return Err(format!("{first} wrote back as {second}"));
    }
    if &back != value {
        return Err(format!("{first} read back as another value"));
    }
    Ok(())
}

fn to_value<T: Serialize>(value: &T) -> serde_json::Value {
    json::to_value(value).unwrap()
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

    fn finish(self) {
        assert!(self.failures.is_empty(), "{:#?}", self.failures);
    }
}
