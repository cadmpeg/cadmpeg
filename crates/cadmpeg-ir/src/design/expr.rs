// SPDX-License-Identifier: Apache-2.0
//! Expressions: a neutral typed tree or the source's own text.
//!
//! Every writer reads the tree. Only a writer of the source's codec reads the
//! text. The two variants are exclusive: a decoder emits `Neutral` when it
//! translates every construct of the source text exactly under the source's
//! semantics, and `Source` otherwise. A tree never holds source text.
//!
//! A [`Neutral`] serializes as canonical text with its references and
//! measures in side lists, [`NeutralWire`]. Reading parses the text, prints
//! the tree, and refuses text that does not print back to itself.

mod parse;
mod print;
mod tree;

#[cfg(feature = "schema")]
use std::borrow::Cow;

use cadmpeg_core::read_session::{self, Repair};
use cadmpeg_core::text::NonBlankString;
use cadmpeg_derive::DeserializeName;
#[cfg(feature = "schema")]
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

pub use self::parse::parse;
pub use self::print::{format_number, print};
use super::construction::FrameAxis;
use super::dimension::ParameterRef;
use super::selection::OrderedSelection;
use super::units::Unit;
use crate::scalar::{FiniteReal, NonNegativeReal};

/// An expression: a neutral typed tree or the source's own text.
#[derive(Debug, Clone, PartialEq, Serialize, cadmpeg_derive::DeserializeTagged)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(
    tag = "kind",
    content = "value",
    rename_all = "snake_case",
    deny_unknown_fields,
    bound(
        serialize = "R: Serialize + PartialEq + Clone",
        deserialize = "R: ::serde::de::DeserializeOwned + PartialEq + Clone"
    )
)]
pub enum Expression<R = ParameterRef> {
    /// A tree that translates every construct of the source text exactly.
    Neutral(Neutral<R>),
    /// The source's own text, kept where the tree cannot state it.
    Source {
        /// The source's expression language.
        dialect: ExpressionDialect,
        /// The text, with each reference the decoder resolves as a
        /// `Reference` token.
        tokens: Vec<ExpressionToken<R>>,
    },
}

/// A neutral expression tree. It serializes as canonical text with its
/// references and measures in side lists.
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(
    try_from = "NeutralWire<R>",
    bound(deserialize = "R: ::serde::de::DeserializeOwned + PartialEq + Clone")
)]
pub struct Neutral<R = ParameterRef>(pub Node<R>);

/// One token of source expression text.
#[derive(Debug, Clone, PartialEq, Serialize, cadmpeg_derive::DeserializeTagged)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(
    tag = "kind",
    content = "value",
    rename_all = "snake_case",
    deny_unknown_fields,
    bound(
        serialize = "R: Serialize",
        deserialize = "R: ::serde::de::DeserializeOwned"
    )
)]
pub enum ExpressionToken<R> {
    /// Source text the decoder does not resolve.
    Text(String),
    /// A reference the decoder resolves.
    Reference(R),
}

/// A source expression language.
#[derive(Debug, Clone, PartialEq, Serialize, cadmpeg_derive::DeserializeTagged)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(
    tag = "kind",
    content = "value",
    rename_all = "snake_case",
    deny_unknown_fields
)]
pub enum ExpressionDialect {
    /// `SolidWorks` equations.
    #[serde(deserialize_with = "cadmpeg_core::tag_only::refuse_value")]
    SolidWorks,
    /// Fusion parameter expressions.
    #[serde(deserialize_with = "cadmpeg_core::tag_only::refuse_value")]
    Fusion,
    /// Fusion CAM expressions.
    #[serde(deserialize_with = "cadmpeg_core::tag_only::refuse_value")]
    FusionCam,
    /// Inventor parameter expressions.
    #[serde(deserialize_with = "cadmpeg_core::tag_only::refuse_value")]
    Inventor,
    /// CATIA formulas and rules.
    #[serde(deserialize_with = "cadmpeg_core::tag_only::refuse_value")]
    Catia,
    /// Creo relations.
    #[serde(deserialize_with = "cadmpeg_core::tag_only::refuse_value")]
    Creo,
    /// NX expressions.
    #[serde(deserialize_with = "cadmpeg_core::tag_only::refuse_value")]
    Nx,
    /// `FreeCAD` expressions.
    #[serde(deserialize_with = "cadmpeg_core::tag_only::refuse_value")]
    FreeCad,
    /// The workbook formula language: operators, functions, `&`, and cell
    /// references relative to the formula's own cell.
    #[serde(deserialize_with = "cadmpeg_core::tag_only::refuse_value")]
    Spreadsheet,
    /// A source language that the vocabulary does not name.
    Native(NonBlankString),
}

impl ExpressionDialect {
    /// The expression language of the sources the codec `codec` reads: the
    /// language of their process records where `process`, and of their
    /// design otherwise. `None` where `codec` is blank.
    pub(crate) fn of_codec(codec: &str, process: bool) -> Option<Self> {
        Some(match (codec, process) {
            ("catia", _) => Self::Catia,
            ("creo", _) => Self::Creo,
            ("f3d", false) => Self::Fusion,
            ("f3d", true) => Self::FusionCam,
            ("fcstd", _) => Self::FreeCad,
            ("inventor", _) => Self::Inventor,
            ("nx", _) => Self::Nx,
            ("sldprt", _) => Self::SolidWorks,
            (codec, _) => Self::Native(NonBlankString::new(codec)?),
        })
    }
}

/// A node of a neutral expression tree. Its clone, comparison, and drop
/// walk the tree without recursion.
#[derive(Debug, Serialize, cadmpeg_derive::DeserializeTagged)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(
    tag = "kind",
    content = "value",
    rename_all = "snake_case",
    deny_unknown_fields,
    bound(
        serialize = "R: Serialize",
        deserialize = "R: ::serde::de::DeserializeOwned"
    )
)]
pub enum Node<R> {
    /// A literal in its authored unit. A number has no sign of its own: a
    /// negative literal is `Unary { op: Negate }` over a `Number`.
    Number {
        /// The authored value.
        value: NonNegativeReal,
        /// The authored unit. An empty unit is dimensionless.
        unit: Unit,
    },
    /// A dimensionless constant.
    Constant(Constant),
    /// A Boolean.
    Boolean(bool),
    /// A text.
    Text(String),
    /// A parameter reference.
    Reference(R),
    /// A quantity of geometry.
    Measure(Box<Measure>),
    /// The path parameter, dimensionless from 0 to 1, of a `Law::Expression`.
    /// It occurs only there.
    #[serde(deserialize_with = "cadmpeg_core::tag_only::refuse_value")]
    LawParameter,
    /// A value of the bend for which a bend-allowance expression is
    /// evaluated. It occurs only in an `AngleRange.value`.
    Bend(BendVariable),
    /// A unary operation.
    Unary {
        /// The operation.
        op: UnaryOp,
        /// The operand.
        operand: Box<Node<R>>,
    },
    /// A binary operation.
    Binary {
        /// The operation.
        op: BinaryOp,
        /// The left operand.
        left: Box<Node<R>>,
        /// The right operand.
        right: Box<Node<R>>,
    },
    /// A run of `+` and `-`, evaluated from the left: `first`, then each
    /// later operand with the operator before it.
    Sum {
        /// The first operand.
        first: Box<Node<R>>,
        /// Each later operand with the operator before it.
        rest: Vec<(AddOp, Node<R>)>,
    },
    /// A run of `*` and `/`, evaluated from the left: `first`, then each
    /// later operand with the operator before it.
    Product {
        /// The first operand.
        first: Box<Node<R>>,
        /// Each later operand with the operator before it.
        rest: Vec<(MulOp, Node<R>)>,
    },
    /// A run of `and`. Every operand evaluates.
    All(Vec<Node<R>>),
    /// A run of `or`. Every operand evaluates.
    Any(Vec<Node<R>>),
    /// A function call.
    Call {
        /// The function.
        function: Function,
        /// The arguments.
        args: Vec<Node<R>>,
    },
    /// A ladder of conditions: `if(p, a, if(q, b, c))` is `cases: [(p, a),
    /// (q, b)], otherwise: c`. The conditions are tested in order, and only
    /// the value of the first that is true, or `otherwise` where none is,
    /// evaluates.
    If {
        /// Each condition with its value, in order.
        cases: Vec<(Node<R>, Node<R>)>,
        /// The value where no condition is true.
        otherwise: Box<Node<R>>,
    },
}

impl<R> Node<R> {
    /// `left op right` as one `Sum`: `right` joins the run of `left` where
    /// `left` is a `Sum`.
    #[must_use]
    pub fn add(mut left: Self, op: AddOp, right: Self) -> Self {
        if let Self::Sum { rest, .. } = &mut left {
            rest.push((op, right));
            left
        } else {
            Self::Sum {
                first: Box::new(left),
                rest: vec![(op, right)],
            }
        }
    }

    /// `left op right` as one `Product`: `right` joins the run of `left`
    /// where `left` is a `Product`.
    #[must_use]
    pub fn mul(mut left: Self, op: MulOp, right: Self) -> Self {
        if let Self::Product { rest, .. } = &mut left {
            rest.push((op, right));
            left
        } else {
            Self::Product {
                first: Box::new(left),
                rest: vec![(op, right)],
            }
        }
    }

    /// `left and right` as one `All`: `right` joins the run of `left` where
    /// `left` is an `All`.
    #[must_use]
    pub fn and(mut left: Self, right: Self) -> Self {
        if let Self::All(operands) = &mut left {
            operands.push(right);
            left
        } else {
            Self::All(vec![left, right])
        }
    }

    /// `left or right` as one `Any`: `right` joins the run of `left` where
    /// `left` is an `Any`.
    #[must_use]
    pub fn or(mut left: Self, right: Self) -> Self {
        if let Self::Any(operands) = &mut left {
            operands.push(right);
            left
        } else {
            Self::Any(vec![left, right])
        }
    }

    /// `if(condition, then, otherwise)` as one `If`: where `otherwise` is an
    /// `If`, the condition and `then` become the first case of its ladder.
    #[must_use]
    pub fn branch(condition: Self, then: Self, mut otherwise: Self) -> Self {
        if let Self::If { cases, .. } = &mut otherwise {
            cases.insert(0, (condition, then));
            otherwise
        } else {
            Self::If {
                cases: vec![(condition, then)],
                otherwise: Box::new(otherwise),
            }
        }
    }

    /// `left op right` as a `Binary`.
    #[must_use]
    pub fn binary(op: BinaryOp, left: Self, right: Self) -> Self {
        Self::Binary {
            op,
            left: Box::new(left),
            right: Box::new(right),
        }
    }
}

/// A dimensionless constant.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, DeserializeName)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub enum Constant {
    /// π.
    Pi,
    /// e.
    E,
}

/// A value of the bend for which a bend-allowance expression is evaluated.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, DeserializeName)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub enum BendVariable {
    /// The bend's angle, measured as its rule's `BendEquations.angle` states.
    Angle,
    /// The bend's inside radius.
    InsideRadius,
    /// The thickness of the bend's sheet.
    Thickness,
}

/// A unary operation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, DeserializeName)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub enum UnaryOp {
    /// `-`, of any quantity.
    Negate,
    /// `not`, of a Boolean.
    Not,
}

/// An operator of a `Sum`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, DeserializeName)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub enum AddOp {
    /// `+`, of the same quantity.
    Add,
    /// `-`, of the same quantity.
    Subtract,
}

/// An operator of a `Product`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, DeserializeName)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub enum MulOp {
    /// `*`: exponents add.
    Multiply,
    /// `/`: exponents subtract; real division.
    Divide,
}

/// A binary operation: `^` and the comparisons.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, DeserializeName)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub enum BinaryOp {
    /// `^`: right-associative, and binds tighter than unary minus.
    Power,
    /// `==`: exact comparison of canonical values.
    Equal,
    /// `!=`: exact comparison of canonical values.
    NotEqual,
    /// `<`, of the same quantity.
    Less,
    /// `<=`, of the same quantity.
    LessEqual,
    /// `>`, of the same quantity.
    Greater,
    /// `>=`, of the same quantity.
    GreaterEqual,
}

/// A function. Its name in canonical text is the lower-case variant name.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, DeserializeName)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub enum Function {
    /// Sine of an angle.
    Sin,
    /// Cosine of an angle.
    Cos,
    /// Tangent of an angle.
    Tan,
    /// Inverse sine, an angle.
    Asin,
    /// Inverse cosine, an angle.
    Acos,
    /// Inverse tangent, an angle in (-π/2, π/2).
    Atan,
    /// `atan2(y, x)`, an angle in (-π, π].
    Atan2,
    /// Square root: halved exponents.
    Sqrt,
    /// Absolute value.
    Abs,
    /// Natural exponential.
    Exp,
    /// Natural logarithm.
    Ln,
    /// Base-10 logarithm.
    Log10,
    /// Minimum, of the same quantity.
    Min,
    /// Maximum, of the same quantity.
    Max,
    /// Floor.
    Floor,
    /// Ceiling.
    Ceil,
    /// Rounding, ties away from zero.
    Round,
    /// Truncation toward zero.
    Trunc,
    /// `mod(a, b)` is `a - trunc(a / b) * b`, with the sign of `a`.
    Mod,
    /// Sign: dimensionless -1, 0, or 1.
    Sign,
}

impl Function {
    /// Every function, in declared order.
    pub const ALL: [Self; 20] = [
        Self::Sin,
        Self::Cos,
        Self::Tan,
        Self::Asin,
        Self::Acos,
        Self::Atan,
        Self::Atan2,
        Self::Sqrt,
        Self::Abs,
        Self::Exp,
        Self::Ln,
        Self::Log10,
        Self::Min,
        Self::Max,
        Self::Floor,
        Self::Ceil,
        Self::Round,
        Self::Trunc,
        Self::Mod,
        Self::Sign,
    ];

    /// The function's name in canonical text.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::Sin => "sin",
            Self::Cos => "cos",
            Self::Tan => "tan",
            Self::Asin => "asin",
            Self::Acos => "acos",
            Self::Atan => "atan",
            Self::Atan2 => "atan2",
            Self::Sqrt => "sqrt",
            Self::Abs => "abs",
            Self::Exp => "exp",
            Self::Ln => "ln",
            Self::Log10 => "log10",
            Self::Min => "min",
            Self::Max => "max",
            Self::Floor => "floor",
            Self::Ceil => "ceil",
            Self::Round => "round",
            Self::Trunc => "trunc",
            Self::Mod => "mod",
            Self::Sign => "sign",
        }
    }
}

/// A quantity of geometry.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct Measure {
    /// What the measure reads.
    pub kind: MeasureKind,
    /// The geometry it reads.
    pub of: MeasureTarget,
    /// The measured value in canonical units, a cache of the measure in its
    /// anchored state, as the source stores it and at the source's precision.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_cached"
    )]
    pub cached: Option<FiniteReal>,
}

/// What a measure reads.
#[derive(Debug, Clone, PartialEq, Serialize, cadmpeg_derive::DeserializeTagged)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(
    tag = "kind",
    content = "value",
    rename_all = "snake_case",
    deny_unknown_fields
)]
pub enum MeasureKind {
    /// The length of edges or curves.
    #[serde(deserialize_with = "cadmpeg_core::tag_only::refuse_value")]
    Length,
    /// The area of faces.
    #[serde(deserialize_with = "cadmpeg_core::tag_only::refuse_value")]
    Area,
    /// The volume of bodies.
    #[serde(deserialize_with = "cadmpeg_core::tag_only::refuse_value")]
    Volume,
    /// The mass of bodies, using each body's material in the configuration.
    #[serde(deserialize_with = "cadmpeg_core::tag_only::refuse_value")]
    Mass,
    /// The distance between two picks.
    #[serde(deserialize_with = "cadmpeg_core::tag_only::refuse_value")]
    Distance,
    /// The angle between two picks.
    #[serde(deserialize_with = "cadmpeg_core::tag_only::refuse_value")]
    Angle,
    /// The radius of an arc or cylinder.
    #[serde(deserialize_with = "cadmpeg_core::tag_only::refuse_value")]
    Radius,
    /// The coordinate of a point or frame origin along an axis of the owner's
    /// definition.
    Coordinate(FrameAxis),
}

/// The geometry a measure reads.
#[derive(Debug, Clone, PartialEq, Serialize, cadmpeg_derive::DeserializeTagged)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum MeasureTarget {
    /// The scope's whole current model.
    #[serde(deserialize_with = "cadmpeg_core::tag_only::read")]
    Model,
    /// The selected geometry.
    Selection(OrderedSelection),
}

/// A [`Neutral`] on the wire: canonical text with its references and
/// measures in side lists. `$i` in the text is `refs[i]` and `@i` is
/// `measures[i]`; each reference and measure appears once in its list, in
/// order of first use.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct NeutralWire<R> {
    /// The canonical text.
    pub text: String,
    /// The references, in order of first use.
    pub refs: Vec<R>,
    /// The measures, in order of first use.
    pub measures: Vec<Measure>,
}

/// Why expression text does not read as a [`Neutral`].
#[derive(Debug, Clone, PartialEq, thiserror::Error)]
pub enum ExprError {
    /// The text is not canonical text of a tree over its side lists, or its
    /// tree is deeper than an expression's bound (§2.5).
    #[error("{0}")]
    Malformed(String),
}

/// Reads a wire payload: parses the text, then requires that printing the
/// tree gives the same payload back.
///
/// # Errors
///
/// Returns [`ExprError::Malformed`] for text that does not parse, whose tree
/// is deeper than an expression's bound, that is not canonical, or whose side
/// lists are not each stated once in order of first use.
pub fn read<R: PartialEq + Clone>(wire: &NeutralWire<R>) -> Result<Node<R>, ExprError> {
    let node = parse(&wire.text, &wire.refs, &wire.measures)?;
    let again = print(&node)?;
    if again.text != wire.text
        && !read_session::repair(Repair::Spelling, &|| {
            format!(
                "expression text {:?} is restated as its canonical text {:?}",
                wire.text, again.text
            )
        })
    {
        return Err(ExprError::Malformed(format!(
            "text {:?} is not canonical; it prints as {:?}",
            wire.text, again.text
        )));
    }
    if (again.refs != wire.refs || again.measures != wire.measures)
        && !read_session::repair(Repair::Spelling, &|| {
            "expression refs and measures are restated in order of first use".into()
        })
    {
        return Err(ExprError::Malformed(
            "refs and measures are not each stated once in order of first use".into(),
        ));
    }
    Ok(node)
}

/// Writes the tree's canonical text. A tree deeper than an expression's bound
/// still writes, and is noted for the decode wrapper, which gives up the
/// record that holds it (§2.5).
impl<R: Serialize + PartialEq + Clone> Serialize for Neutral<R> {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        if self.0.depth() > crate::bounds::MAX_EXPRESSION_DEPTH {
            crate::bounds::note_expression_past();
        }
        print(&self.0)
            .map_err(serde::ser::Error::custom)?
            .serialize(serializer)
    }
}

impl<R: PartialEq + Clone> TryFrom<NeutralWire<R>> for Neutral<R> {
    type Error = ExprError;

    /// Reads a wire payload; see [`read`].
    fn try_from(wire: NeutralWire<R>) -> Result<Self, Self::Error> {
        read(&wire).map(Self)
    }
}

#[cfg(feature = "schema")]
impl<R: JsonSchema> JsonSchema for Neutral<R> {
    fn schema_name() -> Cow<'static, str> {
        format!("Neutral_{}", R::schema_name()).into()
    }

    fn json_schema(generator: &mut schemars::SchemaGenerator) -> schemars::Schema {
        NeutralWire::<R>::json_schema(generator)
    }
}

cadmpeg_core::named_optional_field!(deserialize_cached, FiniteReal, "cached");
