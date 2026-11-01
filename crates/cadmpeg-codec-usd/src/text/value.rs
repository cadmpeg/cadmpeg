// SPDX-License-Identifier: Apache-2.0
//! Typed values: the value syntax of §7.1 read under a declared type (§7.2).
//!
//! A [`Typed`] value stores its components flat. A scalar of a tuple type
//! holds its components; an array holds its elements' components in order.
//! Floating-point components are rounded to the precision of the declared
//! type, so a `float` component holds the binary32 value the text names.

use super::syntax::{Number, Numbers, Packed, Value};

/// The component kind of a value type.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Component {
    Bool,
    /// An integer with its inclusive range.
    Integer {
        min: i128,
        max: i128,
    },
    Half,
    Float,
    Double,
    /// A string or token.
    Text,
    Asset,
}

/// The tuple shape of a value type.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Shape {
    Scalar,
    Vector(usize),
    /// A quaternion `(real, i, j, k)`.
    Quaternion,
    /// A square matrix of `n` rows.
    Matrix(usize),
}

impl Shape {
    /// Components per element.
    pub(crate) const fn components(self) -> usize {
        match self {
            Self::Scalar => 1,
            Self::Vector(count) => count,
            Self::Quaternion => 4,
            Self::Matrix(rows) => rows * rows,
        }
    }
}

/// A declared value type of §7.2.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct ValueType {
    pub(crate) component: Component,
    pub(crate) shape: Shape,
}

const fn integer(min: i128, max: i128) -> Component {
    Component::Integer { min, max }
}

impl ValueType {
    /// The type a type name declares, or `None` for an extension type or a
    /// type with no value (`opaque`, `group`).
    pub(crate) fn parse(name: &str) -> Option<Self> {
        let scalar = |component| Self {
            component,
            shape: Shape::Scalar,
        };
        let simple = match name {
            "bool" => Some(scalar(Component::Bool)),
            "uchar" => Some(scalar(integer(0, i128::from(u8::MAX)))),
            "int" => Some(scalar(integer(i128::from(i32::MIN), i128::from(i32::MAX)))),
            "uint" => Some(scalar(integer(0, i128::from(u32::MAX)))),
            "int64" => Some(scalar(integer(i128::from(i64::MIN), i128::from(i64::MAX)))),
            "uint64" => Some(scalar(integer(0, i128::from(u64::MAX)))),
            "half" => Some(scalar(Component::Half)),
            "float" => Some(scalar(Component::Float)),
            "double" | "timecode" => Some(scalar(Component::Double)),
            "string" | "token" | "pathExpression" => Some(scalar(Component::Text)),
            "asset" => Some(scalar(Component::Asset)),
            "matrix2d" => Some(Self {
                component: Component::Double,
                shape: Shape::Matrix(2),
            }),
            "matrix3d" => Some(Self {
                component: Component::Double,
                shape: Shape::Matrix(3),
            }),
            "matrix4d" | "frame4d" => Some(Self {
                component: Component::Double,
                shape: Shape::Matrix(4),
            }),
            _ => None,
        };
        if simple.is_some() {
            return simple;
        }
        let precision = |suffix: char| match suffix {
            'h' => Some(Component::Half),
            'f' => Some(Component::Float),
            'd' => Some(Component::Double),
            _ => None,
        };
        if let Some(rest) = name.strip_prefix("quat") {
            let mut chars = rest.chars();
            let component = chars.next().and_then(precision)?;
            return chars.next().is_none().then_some(Self {
                component,
                shape: Shape::Quaternion,
            });
        }
        for (prefix, count) in [
            ("point3", 3),
            ("normal3", 3),
            ("vector3", 3),
            ("color3", 3),
            ("color4", 4),
            ("texCoord2", 2),
            ("texCoord3", 3),
        ] {
            if let Some(rest) = name.strip_prefix(prefix) {
                let mut chars = rest.chars();
                let component = chars.next().and_then(precision)?;
                return chars.next().is_none().then_some(Self {
                    component,
                    shape: Shape::Vector(count),
                });
            }
        }
        for (prefix, component) in [
            ("int", integer(i128::from(i32::MIN), i128::from(i32::MAX))),
            ("half", Component::Half),
            ("float", Component::Float),
            ("double", Component::Double),
        ] {
            if let Some(count) = name.strip_prefix(prefix) {
                let count = match count {
                    "2" => 2,
                    "3" => 3,
                    "4" => 4,
                    _ => continue,
                };
                return Some(Self {
                    component,
                    shape: Shape::Vector(count),
                });
            }
        }
        None
    }
}

/// The flat components of a typed value.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum Components {
    Bools(Vec<bool>),
    Integers(Vec<i128>),
    Reals(Vec<f64>),
    Texts(Vec<String>),
}

/// A value read under its declared type.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Typed {
    pub(crate) ty: ValueType,
    pub(crate) array: bool,
    pub(crate) components: Components,
}

/// Why a value does not have its declared type.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum TypeError {
    /// The declared type name is not a type of §7.2.
    UnknownType(String),
    /// The value syntax does not match the declared shape.
    Shape(&'static str),
    /// A number is outside the range of an integer type.
    Range,
    /// A component value does not have the component type.
    Component(&'static str),
}

impl std::fmt::Display for TypeError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::UnknownType(name) => write!(formatter, "type `{name}` has no value grammar"),
            Self::Shape(expected) => write!(formatter, "expected {expected}"),
            Self::Range => formatter.write_str("integer outside the range of its type"),
            Self::Component(expected) => write!(formatter, "expected {expected}"),
        }
    }
}

/// Reads `value` as a value of the declared `type_name`, with `array` when
/// the declaration has `[]`.
pub(crate) fn typed(value: &Value, type_name: &str, array: bool) -> Result<Typed, TypeError> {
    let ty = ValueType::parse(type_name).ok_or_else(|| TypeError::UnknownType(type_name.into()))?;
    let mut builder = Builder::new(ty.component);
    if array {
        match value {
            Value::List(elements) => {
                for element in elements {
                    element_components(element, ty, &mut builder)?;
                }
            }
            Value::Packed(packed) => builder.push_packed(packed, ty)?,
            _ => return Err(TypeError::Shape("a list for an array type")),
        }
    } else {
        if matches!(value, Value::List(_)) {
            return Err(TypeError::Shape("a value without `[]`"));
        }
        element_components(value, ty, &mut builder)?;
    }
    Ok(Typed {
        ty,
        array,
        components: builder.finish(),
    })
}

struct Builder {
    component: Component,
    components: Components,
}

impl Builder {
    fn new(component: Component) -> Self {
        let components = match component {
            Component::Bool => Components::Bools(Vec::new()),
            Component::Integer { .. } => Components::Integers(Vec::new()),
            Component::Half | Component::Float | Component::Double => Components::Reals(Vec::new()),
            Component::Text | Component::Asset => Components::Texts(Vec::new()),
        };
        Self {
            component,
            components,
        }
    }

    fn push(&mut self, value: &Value) -> Result<(), TypeError> {
        match (&mut self.components, self.component) {
            (Components::Bools(out), Component::Bool) => out.push(bool_component(value)?),
            (Components::Integers(out), Component::Integer { min, max }) => {
                let Value::Number(number) = value else {
                    return Err(TypeError::Component("an integer"));
                };
                out.push(integer_component(*number, min, max)?);
            }
            (Components::Reals(out), precision) => {
                let Value::Number(number) = value else {
                    return Err(TypeError::Component("a number"));
                };
                out.push(round_to(number.float, precision));
            }
            (Components::Texts(out), Component::Asset) => match value {
                Value::Asset(text) | Value::String(text) => out.push(text.clone()),
                _ => return Err(TypeError::Component("an asset path")),
            },
            (Components::Texts(out), _) => match value {
                Value::String(text) => out.push(text.clone()),
                _ => return Err(TypeError::Component("a string")),
            },
            _ => return Err(TypeError::Component("a value of the declared type")),
        }
        Ok(())
    }

    /// The components of a packed list whose elements have `ty`'s shape.
    fn push_packed(&mut self, packed: &Packed, ty: ValueType) -> Result<(), TypeError> {
        if packed.width != ty.shape.components() {
            return Err(TypeError::Shape("a tuple with one item per component"));
        }
        let number = |float: f64, integer: Option<i128>| Value::Number(Number { float, integer });
        match &packed.numbers {
            Numbers::Reals(values) => match (&mut self.components, self.component) {
                (Components::Reals(out), precision) => {
                    out.extend(values.iter().map(|value| round_to(*value, precision)));
                }
                _ => {
                    for value in values {
                        self.push(&number(*value, None))?;
                    }
                }
            },
            Numbers::Literals { integers, floats } => {
                for (integer, float) in integers.iter().zip(floats) {
                    self.push(&number(*float, Some(*integer)))?;
                }
            }
            Numbers::Integers(values) => {
                for value in values {
                    let float = i64::try_from(*value)
                        .ok()
                        .and_then(cadmpeg_core::convert::f64_from_i64);
                    match (float, self.component) {
                        (Some(float), _) => self.push(&number(float, Some(*value)))?,
                        // An integer component reads the exact value.
                        (None, Component::Integer { .. }) => {
                            self.push(&number(0.0, Some(*value)))?;
                        }
                        (None, _) => return Err(TypeError::Range),
                    }
                }
            }
        }
        Ok(())
    }

    fn finish(self) -> Components {
        self.components
    }
}

fn element_components(value: &Value, ty: ValueType, out: &mut Builder) -> Result<(), TypeError> {
    match ty.shape {
        Shape::Scalar => out.push(value),
        Shape::Vector(count) => tuple_components(value, count, out),
        Shape::Quaternion => tuple_components(value, 4, out),
        Shape::Matrix(rows) => {
            let Value::Tuple(row_values) = value else {
                return Err(TypeError::Shape("a tuple of matrix rows"));
            };
            if row_values.len() != rows {
                return Err(TypeError::Shape("one tuple per matrix row"));
            }
            for row in row_values {
                tuple_components(row, rows, out)?;
            }
            Ok(())
        }
    }
}

fn tuple_components(value: &Value, count: usize, out: &mut Builder) -> Result<(), TypeError> {
    let Value::Tuple(items) = value else {
        return Err(TypeError::Shape("a tuple"));
    };
    if items.len() != count {
        return Err(TypeError::Shape("a tuple with one item per component"));
    }
    items.iter().try_for_each(|item| out.push(item))
}

fn bool_component(value: &Value) -> Result<bool, TypeError> {
    match value {
        Value::Number(number) => Ok(number.float != 0.0 || number.float.is_nan()),
        Value::Identifier(word) => match word.to_ascii_lowercase().as_str() {
            "true" | "yes" => Ok(true),
            "false" | "no" => Ok(false),
            _ => Err(TypeError::Component("`true`, `false`, `yes`, or `no`")),
        },
        _ => Err(TypeError::Component("a boolean")),
    }
}

fn integer_component(number: Number, min: i128, max: i128) -> Result<i128, TypeError> {
    let value = match number.integer {
        Some(value) => value,
        None => {
            cadmpeg_core::convert::truncate_f64_to_i128(number.float).ok_or(TypeError::Range)?
        }
    };
    if (min..=max).contains(&value) {
        Ok(value)
    } else {
        Err(TypeError::Range)
    }
}

/// `value` rounded to the precision of `component` (§2.6). A `half` rounds
/// through binary32, as a binary32 value converts to binary16.
pub(crate) fn round_to(value: f64, component: Component) -> f64 {
    match component {
        Component::Half => f64::from(half_from_f32(round_f32(value))),
        Component::Float => f64::from(round_f32(value)),
        _ => value,
    }
}

/// `value` rounded to binary32; a finite value outside its range is an
/// infinity of the same sign.
pub(crate) fn round_f32(value: f64) -> f32 {
    if value.is_nan() {
        return f32::NAN;
    }
    cadmpeg_core::convert::f32_from_f64(value).unwrap_or(if value.is_sign_negative() {
        f32::NEG_INFINITY
    } else {
        f32::INFINITY
    })
}

/// `value` rounded to the nearest binary16 value, ties to even, then widened
/// back to binary32.
fn half_from_f32(value: f32) -> f32 {
    const MANTISSA_BITS: u32 = 23;
    const HALF_MANTISSA_BITS: u32 = 10;
    const DROPPED: u32 = MANTISSA_BITS - HALF_MANTISSA_BITS;
    if !value.is_finite() {
        return value;
    }
    let magnitude = value.abs();
    let sign = if value.is_sign_negative() { -1.0 } else { 1.0 };
    let largest = 65_504.0_f32;
    let smallest_normal = 6.103_515_6e-5_f32;
    if magnitude >= 65_520.0 {
        return sign * f32::INFINITY;
    }
    if magnitude >= smallest_normal {
        let bits = magnitude.to_bits();
        let keep = bits >> DROPPED;
        let remainder = bits & ((1 << DROPPED) - 1);
        let halfway = 1 << (DROPPED - 1);
        let rounded = if remainder > halfway || (remainder == halfway && keep & 1 == 1) {
            keep + 1
        } else {
            keep
        };
        let result = f32::from_bits(rounded << DROPPED);
        return sign * result.min(largest);
    }
    let step = 5.960_464_5e-8_f32;
    let quotient = magnitude / step;
    let floor = quotient.floor();
    let fraction = quotient - floor;
    let even = (floor / 2.0).fract() == 0.0;
    let count = if fraction > 0.5 || (fraction == 0.5 && !even) {
        floor + 1.0
    } else {
        floor
    };
    sign * count * step
}

impl Typed {
    /// Components per element.
    pub(crate) const fn width(&self) -> usize {
        self.ty.shape.components()
    }

    pub(crate) fn reals(&self) -> Option<&[f64]> {
        match &self.components {
            Components::Reals(values) => Some(values),
            _ => None,
        }
    }

    pub(crate) fn integers(&self) -> Option<&[i128]> {
        match &self.components {
            Components::Integers(values) => Some(values),
            _ => None,
        }
    }

    pub(crate) fn bools(&self) -> Option<&[bool]> {
        match &self.components {
            Components::Bools(values) => Some(values),
            _ => None,
        }
    }

    pub(crate) fn texts(&self) -> Option<&[String]> {
        match &self.components {
            Components::Texts(values) => Some(values),
            _ => None,
        }
    }

    /// Real or integer components as binary64 values.
    pub(crate) fn numbers(&self) -> Option<Vec<f64>> {
        match &self.components {
            Components::Reals(values) => Some(values.clone()),
            Components::Integers(values) => values
                .iter()
                .map(|value| {
                    i64::try_from(*value)
                        .ok()
                        .and_then(cadmpeg_core::convert::f64_from_i64)
                })
                .collect(),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests;
