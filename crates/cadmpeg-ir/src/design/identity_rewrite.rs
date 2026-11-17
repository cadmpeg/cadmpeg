// SPDX-License-Identifier: Apache-2.0
//! Rewrite the identities owned by the design references that model-class
//! records hold.

use cadmpeg_core::decode::DecodeContext;
use cadmpeg_core::CodecError;

use super::dimension::{ParameterRef, RefTarget};
use super::expr::{Expression, ExpressionToken, Neutral, Node};
use super::names::NativeName;
use super::refs::{ConfigurationRef, EntityRef, Evaluated, Member, Position, StateRef};
use super::selection::{OccurrencePath, PathStep, StepRef};
use crate::schema::rewrite::typed::{IdentityMap, RewriteIdentities};

rewrite_enum!(EntityRef, []; {
    Body(field0),
    Face(field0),
    Edge(field0),
    Vertex(field0),
    Surface(field0),
    Curve(field0),
    Subd(field0),
});
rewrite_enum!(ConfigurationRef, []; {
    Local(field0),
    Named(field0),
});
rewrite_enum!(Member, []; {
    Feature(field0),
    Container(field0),
});
rewrite_enum!(Position, []; {
    Start(field0),
    After(field0),
});
rewrite_enum!(Evaluated, []; {
    State(field0),
    Tool(field0),
});
rewrite_record!(StateRef, []; {configuration, of});
rewrite_record!(OccurrencePath, []; (field0));
rewrite_record!(PathStep, []; {occurrence, configuration});
rewrite_enum!(StepRef, []; {
    Occurrence(field0),
    Native(field0),
});
rewrite_scalar!(NativeName);
rewrite_enum!(RefTarget, []; {
    Parameter(field0),
    External {definition, name},
});
rewrite_record!(ParameterRef, []; {target, through});

/// The parameter references of an expression: the `Reference` leaves of its
/// tree, walked on a work stack since a tree is deeper than the ceiling that
/// admits it, and the `Reference` tokens of its source text. A measure's
/// selection is design content, which has no identity rewrite.
impl RewriteIdentities for Expression {
    fn visit_identity_references(
        &self,
        ctx: &DecodeContext<'_>,
        visitor: &mut dyn FnMut(&str) -> Result<(), CodecError>,
    ) -> Result<(), CodecError> {
        match self {
            Self::Neutral(Neutral(root)) => {
                let mut pending = vec![root];
                while let Some(node) = pending.pop() {
                    if let Node::Reference(reference) = node {
                        reference.visit_identity_references(ctx, visitor)?;
                    }
                    pending.extend(node.operands());
                }
                Ok(())
            }
            Self::Source { tokens, .. } => tokens.iter().try_for_each(|token| match token {
                ExpressionToken::Reference(reference) => {
                    reference.visit_identity_references(ctx, visitor)
                }
                ExpressionToken::Text(_) => Ok(()),
            }),
        }
    }
    fn rewrite_identities<F: FnMut(&str) -> Result<String, CodecError>>(
        mut self,
        ctx: &DecodeContext<'_>,
        map: &mut IdentityMap<'_, F>,
    ) -> Result<Self, CodecError> {
        let mut rewrite = |reference: &mut ParameterRef| -> Result<(), CodecError> {
            *reference = reference.clone().rewrite_identities(ctx, map)?;
            Ok(())
        };
        match &mut self {
            Self::Neutral(Neutral(root)) => {
                let mut pending = vec![root];
                while let Some(node) = pending.pop() {
                    if let Node::Reference(reference) = node {
                        rewrite(reference)?;
                    }
                    pending.extend(node.operands_mut());
                }
            }
            Self::Source { tokens, .. } => {
                for token in tokens {
                    if let ExpressionToken::Reference(reference) = token {
                        rewrite(reference)?;
                    }
                }
            }
        }
        Ok(self)
    }
}
