// SPDX-License-Identifier: Apache-2.0
//! Canonical expressions with identity references separated from source text.

use std::ops::Range;

#[cfg(feature = "schema")]
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use super::{ParameterId, ParameterValue};

const MILLIMETERS_PER_MICROMETER: f64 = 1.0e-3;
const MILLIMETERS_PER_NANOMETER: f64 = 1.0e-6;

/// An expression whose referenced parameters survive renaming by identity.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct Expression {
    /// Ordered literal fragments and parameter references.
    pub tokens: Vec<ExpressionToken>,
}

/// One literal fragment or parameter reference in an expression.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(
    tag = "kind",
    content = "value",
    rename_all = "snake_case",
    deny_unknown_fields
)]
pub enum ExpressionToken {
    /// Text with no resolved parameter reference.
    Text(String),
    /// A parameter rendered using its current name in the target dialect.
    Parameter(ParameterId),
}

/// An invalid source span supplied while constructing expression tokens.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExpressionError {
    /// Invalid byte range in the original expression.
    pub span: Range<usize>,
}

impl std::fmt::Display for ExpressionError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            formatter,
            "invalid or overlapping expression span {:?}",
            self.span
        )
    }
}
impl std::error::Error for ExpressionError {}

impl Expression {
    /// A canonical literal expression in millimeters, radians, or scalar units.
    pub fn literal(value: &ParameterValue) -> Self {
        let text = match value {
            ParameterValue::Length(value) => format!("{} mm", value.get()),
            ParameterValue::Angle(value) => format!("{} rad", value.get()),
            ParameterValue::Real(value) => value.get().to_string(),
            ParameterValue::Integer(value) => value.to_string(),
            ParameterValue::Boolean(value) => value.to_string(),
            ParameterValue::String(value) => serde_json::Value::String(value.clone()).to_string(),
        };
        Self::text(text)
    }

    /// Evaluate a single literal using the parameter's declared quantity.
    /// Expressions with references or nonliteral operators are not evaluated here.
    pub fn literal_value_as(&self, expected: &ParameterValue) -> Option<ParameterValue> {
        let text = self.literal_text()?;
        let text = text.trim();
        let quantity = |units: &[(&str, f64)]| {
            for (suffix, factor) in units {
                if let Some(number) = text.strip_suffix(suffix) {
                    return number
                        .trim()
                        .parse::<f64>()
                        .ok()
                        .map(|value| value * factor);
                }
            }
            text.parse::<f64>().ok()
        };
        Some(match expected {
            ParameterValue::Length(_) => {
                ParameterValue::Length(crate::scalar::Length::new(quantity(&[
                    ("mm", 1.0),
                    ("cm", 10.0),
                    ("km", 1.0e6),
                    ("um", MILLIMETERS_PER_MICROMETER),
                    ("µm", MILLIMETERS_PER_MICROMETER),
                    ("nm", MILLIMETERS_PER_NANOMETER),
                    ("m", 1.0e3),
                    ("inch", 25.4),
                    ("in", 25.4),
                    ("ft", 304.8),
                ])?)?)
            }
            ParameterValue::Angle(_) => {
                ParameterValue::Angle(crate::scalar::Angle::new(quantity(&[
                    ("rad", 1.0),
                    ("deg", std::f64::consts::PI / 180.0),
                    ("°", std::f64::consts::PI / 180.0),
                ])?)?)
            }
            ParameterValue::Real(_) => {
                ParameterValue::Real(crate::scalar::FiniteReal::new(text.parse().ok()?)?)
            }
            ParameterValue::Integer(_) => ParameterValue::Integer(text.parse().ok()?),
            ParameterValue::Boolean(_) => ParameterValue::Boolean(text.parse().ok()?),
            ParameterValue::String(_) => ParameterValue::String(serde_json::from_str(text).ok()?),
        })
    }
    /// An expression containing one literal fragment, including an empty one.
    pub fn text(text: impl Into<String>) -> Self {
        Self {
            tokens: vec![ExpressionToken::Text(text.into())],
        }
    }

    /// Preserve source text between nonoverlapping, UTF-8-aligned identifier spans.
    /// Spans may arrive in any order; their byte positions define token order.
    pub fn from_spans(
        text: &str,
        spans: impl IntoIterator<Item = (Range<usize>, ParameterId)>,
    ) -> Result<Self, ExpressionError> {
        let mut spans = spans.into_iter().collect::<Vec<_>>();
        spans.sort_by_key(|(span, _)| span.start);
        let mut cursor = 0;
        let mut tokens = Vec::new();
        for (span, parameter) in spans {
            if span.start < cursor
                || span.start >= span.end
                || !text.is_char_boundary(span.start)
                || !text.is_char_boundary(span.end)
            {
                return Err(ExpressionError { span });
            }
            let Some(fragment) = text.get(cursor..span.start) else {
                return Err(ExpressionError { span });
            };
            if !fragment.is_empty() {
                tokens.push(ExpressionToken::Text(fragment.to_owned()));
            }
            tokens.push(ExpressionToken::Parameter(parameter));
            cursor = span.end;
        }
        if cursor < text.len() || tokens.is_empty() {
            tokens.push(ExpressionToken::Text(text[cursor..].to_owned()));
        }
        Ok(Self { tokens })
    }

    /// Referenced parameters in token order. Repeated references remain repeated.
    pub fn dependencies(&self) -> impl Iterator<Item = &ParameterId> {
        self.tokens.iter().filter_map(|token| match token {
            ExpressionToken::Parameter(parameter) => Some(parameter),
            ExpressionToken::Text(_) => None,
        })
    }

    /// Render references through the caller's current-name and target-quoting policy.
    /// A dangling reference has no rendered expression.
    pub fn render_with(
        &self,
        mut name: impl FnMut(&ParameterId) -> Option<String>,
    ) -> Option<String> {
        let mut output = String::new();
        for token in &self.tokens {
            match token {
                ExpressionToken::Text(text) => output.push_str(text),
                ExpressionToken::Parameter(parameter) => output.push_str(&name(parameter)?),
            }
        }
        Some(output)
    }

    /// Literal text when the expression has no bound parameter references.
    pub fn literal_text(&self) -> Option<String> {
        self.render_with(|_| None)
    }
}

impl From<String> for Expression {
    fn from(text: String) -> Self {
        Self::text(text)
    }
}
impl From<&str> for Expression {
    fn from(text: &str) -> Self {
        Self::text(text)
    }
}

impl crate::schema::rewrite::typed::RewriteIdentities for ExpressionToken {
    fn visit_dependency_references(
        &self,
        ctx: &cadmpeg_core::decode::DecodeContext<'_>,
        visitor: &mut dyn FnMut(
            crate::schema::rewrite::typed::prune::DependencyReference<'_>,
        ) -> Result<(), cadmpeg_core::CodecError>,
    ) -> Result<(), cadmpeg_core::CodecError> {
        match self {
            Self::Parameter(id) => id.visit_dependency_references(ctx, visitor),
            Self::Text(_) => Ok(()),
        }
    }

    fn visit_identity_references(
        &self,
        ctx: &cadmpeg_core::decode::DecodeContext<'_>,
        visitor: &mut dyn FnMut(&str) -> Result<(), cadmpeg_core::CodecError>,
    ) -> Result<(), cadmpeg_core::CodecError> {
        match self {
            Self::Parameter(id) => id.visit_identity_references(ctx, visitor),
            Self::Text(_) => Ok(()),
        }
    }
    fn rewrite_identities<F>(
        self,
        ctx: &cadmpeg_core::decode::DecodeContext<'_>,
        map: &mut crate::schema::rewrite::typed::IdentityMap<'_, F>,
    ) -> Result<Self, cadmpeg_core::CodecError>
    where
        F: FnMut(&str) -> Result<String, cadmpeg_core::CodecError>,
    {
        Ok(match self {
            Self::Parameter(id) => Self::Parameter(id.rewrite_identities(ctx, map)?),
            Self::Text(text) => Self::Text(text),
        })
    }
}
impl crate::schema::rewrite::typed::RewriteIdentities for Expression {
    fn visit_dependency_references(
        &self,
        ctx: &cadmpeg_core::decode::DecodeContext<'_>,
        visitor: &mut dyn FnMut(
            crate::schema::rewrite::typed::prune::DependencyReference<'_>,
        ) -> Result<(), cadmpeg_core::CodecError>,
    ) -> Result<(), cadmpeg_core::CodecError> {
        self.tokens.visit_dependency_references(ctx, visitor)
    }

    fn visit_identity_references(
        &self,
        ctx: &cadmpeg_core::decode::DecodeContext<'_>,
        visitor: &mut dyn FnMut(&str) -> Result<(), cadmpeg_core::CodecError>,
    ) -> Result<(), cadmpeg_core::CodecError> {
        self.tokens.visit_identity_references(ctx, visitor)
    }
    fn rewrite_identities<F>(
        self,
        ctx: &cadmpeg_core::decode::DecodeContext<'_>,
        map: &mut crate::schema::rewrite::typed::IdentityMap<'_, F>,
    ) -> Result<Self, cadmpeg_core::CodecError>
    where
        F: FnMut(&str) -> Result<String, cadmpeg_core::CodecError>,
    {
        Ok(Self {
            tokens: self.tokens.rewrite_identities(ctx, map)?,
        })
    }
}

#[cfg(test)]
mod tests;
