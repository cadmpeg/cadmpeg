// SPDX-License-Identifier: Apache-2.0
//! Leaf sample values and the strict round trip that the operation tests
//! share.

use std::collections::BTreeMap;
use std::fmt::Debug;

use cadmpeg_core::decode::ResourceLimits;
use cadmpeg_core::text::NonBlankString;
use serde::de::DeserializeOwned;
use serde::Serialize;

use crate::design::construction::{
    kinds, AxisValue, Construction, ConstructionInput, ConstructionKind, FrameValue,
    InputReference, InputRole, PlaneValue, PostStep, Relation, Subject,
};
use crate::design::dimension::Dimension;
use crate::design::expr::{Expression, ExpressionDialect, ExpressionToken};
use crate::design::options::{
    BodyForm, Boolean, BooleanOp, NativeOption, NativeOptions, SheetOutput, SolidOutput, Solidify,
};
use crate::design::read;
use crate::design::selection::{
    Anchor, MemberOrder, Members, PickTarget, Picked, Picks, Selection,
};
use crate::features::{FinitePoint3, ParameterId};
use crate::math::Point3;
use crate::scalar::{
    Angle, FiniteReal, Fraction, Length, NonNegativeLength, NonNegativeReal, NonZeroLength,
    PositiveLength, PositiveReal,
};
use crate::units::UnitVector3;

pub(in crate::design::ops) fn id<T>(kind: &str, key: u32) -> T
where
    T: TryFrom<String>,
    T::Error: Debug,
{
    T::try_from(format!("test:design:{kind}#{key}")).unwrap()
}

pub(in crate::design::ops) fn text(value: &str) -> NonBlankString {
    NonBlankString::new(value).unwrap()
}

pub(in crate::design::ops) fn lit<T>(value: T) -> Dimension<T> {
    Dimension::Literal(value)
}

pub(in crate::design::ops) fn bound<T>(key: u32) -> Dimension<T> {
    Dimension::Parameter(id::<ParameterId>("parameter", key))
}

pub(in crate::design::ops) fn length(value: f64) -> Length {
    Length::new(value).unwrap()
}

pub(in crate::design::ops) fn angle(value: f64) -> Angle {
    Angle::new(value).unwrap()
}

pub(in crate::design::ops) fn positive(value: f64) -> PositiveLength {
    PositiveLength::new(value).unwrap()
}

pub(in crate::design::ops) fn nonnegative(value: f64) -> NonNegativeLength {
    NonNegativeLength::new(value).unwrap()
}

pub(in crate::design::ops) fn nonzero(value: f64) -> NonZeroLength {
    NonZeroLength::new(value).unwrap()
}

pub(in crate::design::ops) fn real(value: f64) -> FiniteReal {
    FiniteReal::new(value).unwrap()
}

pub(in crate::design::ops) fn positive_real(value: f64) -> PositiveReal {
    PositiveReal::new(value).unwrap()
}

pub(in crate::design::ops) fn nonnegative_real(value: f64) -> NonNegativeReal {
    NonNegativeReal::new(value).unwrap()
}

pub(in crate::design::ops) fn fraction(value: f64) -> Fraction {
    Fraction::new(value).unwrap()
}

fn point(x: f64, y: f64, z: f64) -> FinitePoint3 {
    FinitePoint3::new(Point3 { x, y, z }).unwrap()
}

/// A named selection of one sketch entity.
pub(in crate::design::ops) fn selection<O: MemberOrder>(key: u32) -> Selection<O> {
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

/// A selection whose picks the decoder did not read.
pub(in crate::design::ops) fn unread<O: MemberOrder>() -> Selection<O> {
    Selection {
        at: Anchor::Current,
        picks: Picks::Unread,
        native: None,
    }
}

fn construction<K: ConstructionKind>(resolved: Option<K::Value>) -> Construction<K> {
    let inputs = if resolved.is_some() {
        vec![ConstructionInput {
            role: Some(InputRole {
                subject: Subject::Whole,
                relation: Relation::Coincident,
            }),
            reference: Some(InputReference::Picks(selection(9))),
            value: None,
            reversed: None,
        }]
    } else {
        Vec::new()
    };
    Construction {
        inputs,
        after: vec![PostStep::Flip(lit(true))],
        solution: None,
        mode: None,
        resolved,
        resolved_in: BTreeMap::new(),
    }
}

pub(in crate::design::ops) fn point_construction() -> Construction<kinds::Point> {
    construction(Some(point(1.0, 2.0, 3.0)))
}

pub(in crate::design::ops) fn direction() -> Construction<kinds::Direction> {
    construction(Some(UnitVector3::X_AXIS))
}

pub(in crate::design::ops) fn axis() -> Construction<kinds::Axis> {
    construction(Some(AxisValue {
        origin: point(0.0, 0.0, 0.0),
        direction: UnitVector3::Z_AXIS,
    }))
}

pub(in crate::design::ops) fn plane() -> Construction<kinds::Plane> {
    construction(Some(PlaneValue {
        origin: point(0.0, 0.0, 5.0),
        normal: UnitVector3::Z_AXIS,
        x_axis: UnitVector3::X_AXIS,
    }))
}

pub(in crate::design::ops) fn frame() -> Construction<kinds::Frame> {
    construction(Some(FrameValue {
        origin: point(1.0, 2.0, 3.0),
        x_axis: UnitVector3::X_AXIS,
        y_axis: UnitVector3::Y_AXIS,
        z_axis: UnitVector3::Z_AXIS,
    }))
}

pub(in crate::design::ops) fn curve() -> Construction<kinds::Curve> {
    construction(None)
}

pub(in crate::design::ops) fn native_options() -> NativeOptions {
    NativeOptions::from([(text("codec:option"), NativeOption::Bool(lit(true)))])
}

pub(in crate::design::ops) fn sheet_output() -> SheetOutput {
    SheetOutput {
        solid: Some(Solidify::IfClosed),
        boolean: Some(Boolean::Apply {
            op: BooleanOp::Sew,
            targets: None,
        }),
    }
}

pub(in crate::design::ops) fn solid_output() -> SolidOutput {
    SolidOutput {
        body: Some(BodyForm::Sheet {}),
        boolean: Some(Boolean::NewBody {}),
    }
}

pub(in crate::design::ops) fn expression() -> Expression {
    Expression::Source {
        dialect: ExpressionDialect::Inventor,
        tokens: vec![ExpressionToken::Text("2 * μ".to_owned())],
    }
}

/// Writes `value`, reads it back strictly under the desktop ceilings, writes
/// it again, and requires the same bytes and the same value.
pub(in crate::design::ops) fn round_trip<T: Serialize + DeserializeOwned + PartialEq + Debug>(
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

pub(in crate::design::ops) fn to_value<T: Serialize>(value: &T) -> serde_json::Value {
    serde_json::to_value(value).unwrap()
}

/// Reads `text`, requires a refusal, and returns its message.
pub(in crate::design::ops) fn refuses<T: DeserializeOwned + Debug>(text: &str) -> String {
    match read::from_str::<T>(text, &ResourceLimits::desktop()) {
        Ok(value) => panic!("{text} was read as {value:?}"),
        Err(error) => error.to_string(),
    }
}

/// The failures of a set of round trips.
#[derive(Default)]
pub(in crate::design::ops) struct Report {
    pub(in crate::design::ops) failures: Vec<String>,
}

impl Report {
    /// Round-trips every sample of one type.
    pub(in crate::design::ops) fn check<T: Serialize + DeserializeOwned + PartialEq + Debug>(
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
