// SPDX-License-Identifier: Apache-2.0
//! Rewrite the identities owned by the design references that model-class
//! records hold.

use super::construction::{
    kinds, AnyConstruction, AxisValue, Construction, ConstructionInput, CurveValue, FrameAxis,
    FrameValue, InputReference, InputRole, PlaneValue, PostStep, ScalarDimension, SourceMode,
};
use super::dimension::{Dimension, ParameterRef, RefTarget};
use super::expr::{Expression, ExpressionToken, Neutral, Node};
use super::names::{
    CodecId, GenerationRole, Generator, InstanceIndex, NameKind, NativeName, NativeNameValue,
    TopoName,
};
use super::options::NativeOption;
use super::parameter::Deviations;
use super::refs::{ConfigurationRef, EntityRef, Evaluated, Member, Position, StateRef};
use super::selection::{
    Anchor, BoundaryUse, Continuity, DatumPart, DefinitionRef, Fragment, Keyed, MemberOrder,
    Members, NativeOwner, OccurrencePath, Ordered, PathStep, PickContext, PickDomain, PickTarget,
    Picked, Picks, PrincipalEntity, Propagation, RuleTerm, Selection, SelectionRule, SketchRegion,
    StepRef, Unordered, Witness,
};
use super::units::UnitSystem;
use crate::schema::rewrite::typed::{IdentityMap, RewriteIdentities};
use cadmpeg_core::decode::DecodeContext;
use cadmpeg_core::CodecError;

impl<T: Ord + RewriteIdentities> RewriteIdentities for std::collections::BTreeSet<T> {
    fn visit_identity_references(
        &self,
        ctx: &DecodeContext<'_>,
        visitor: &mut dyn FnMut(&str) -> Result<(), CodecError>,
    ) -> Result<(), CodecError> {
        let _depth = ctx.enter_nested("structured name set")?;
        for value in self {
            value.visit_identity_references(ctx, visitor)?;
        }
        Ok(())
    }

    fn rewrite_identities<F: FnMut(&str) -> Result<String, CodecError>>(
        self,
        ctx: &DecodeContext<'_>,
        map: &mut IdentityMap<'_, F>,
    ) -> Result<Self, CodecError> {
        let _depth = ctx.enter_nested("rewrite structured name set")?;
        let mut values = Self::new();
        for value in self {
            if !values.insert(value.rewrite_identities(ctx, map)?) {
                return Err(CodecError::malformed(
                    "rewritten name set repeats an identity",
                ));
            }
        }
        Ok(values)
    }
}

impl<T, P: RewriteIdentities> RewriteIdentities for Dimension<T, P> {
    fn visit_identity_references(
        &self,
        ctx: &DecodeContext<'_>,
        visitor: &mut dyn FnMut(&str) -> Result<(), CodecError>,
    ) -> Result<(), CodecError> {
        match self {
            Self::Literal(_) => Ok(()),
            Self::Parameter(parameter) | Self::Negated(parameter) => {
                parameter.visit_identity_references(ctx, visitor)
            }
        }
    }

    fn rewrite_identities<F: FnMut(&str) -> Result<String, CodecError>>(
        self,
        ctx: &DecodeContext<'_>,
        map: &mut IdentityMap<'_, F>,
    ) -> Result<Self, CodecError> {
        Ok(match self {
            Self::Literal(value) => Self::Literal(value),
            Self::Parameter(parameter) => Self::Parameter(parameter.rewrite_identities(ctx, map)?),
            Self::Negated(parameter) => Self::Negated(parameter.rewrite_identities(ctx, map)?),
        })
    }
}

impl<T: Keyed + RewriteIdentities, O: MemberOrder> RewriteIdentities for Members<T, O> {
    fn visit_identity_references(
        &self,
        ctx: &DecodeContext<'_>,
        visitor: &mut dyn FnMut(&str) -> Result<(), CodecError>,
    ) -> Result<(), CodecError> {
        let _depth = ctx.enter_nested("selection members")?;
        for value in self.as_slice() {
            value.visit_identity_references(ctx, visitor)?;
        }
        Ok(())
    }

    fn rewrite_identities<F: FnMut(&str) -> Result<String, CodecError>>(
        self,
        ctx: &DecodeContext<'_>,
        map: &mut IdentityMap<'_, F>,
    ) -> Result<Self, CodecError> {
        Self::new(self.into_vec().rewrite_identities(ctx, map)?).map_err(CodecError::malformed)
    }
}

rewrite_scalar!(CodecId);
rewrite_scalar!(super::parameter::Tolerance);
rewrite_scalar!(super::units::Quantity);
rewrite_scalar!(NativeName);
rewrite_scalar!(NativeNameValue);
rewrite_scalar!(NameKind);
rewrite_scalar!(GenerationRole);
rewrite_scalar!(InstanceIndex);
rewrite_scalar!(Fragment);
rewrite_scalar!(DatumPart);
rewrite_scalar!(PrincipalEntity);
rewrite_scalar!(Continuity);
rewrite_scalar!(InputRole);
rewrite_scalar!(FrameAxis);
rewrite_scalar!(AxisValue);
rewrite_scalar!(PlaneValue);
rewrite_scalar!(FrameValue);
rewrite_scalar!(CurveValue);
rewrite_scalar!(UnitSystem);
rewrite_scalar!(Deviations);

rewrite_enum!(TopoName, []; {
    Generated {kind, feature, role, generator, index},
    Instance {feature, instance, of},
    Blend {feature, between, index},
    Bounded {faces, ends, index},
    Meeting {faces, index},
    Modified {feature, from},
    Split {feature, from, piece},
    Merged {feature, from},
    Root {container},
});
rewrite_enum!(Generator, []; {SketchEntity(value), SpatialSketchEntity(value), Entity(value)});
rewrite_enum!(EntityRef, []; {Body(value), Face(value), Edge(value), Vertex(value), Surface(value), Curve(value), Subd(value)});
rewrite_enum!(Member, []; {Feature(value), Container(value)});
rewrite_enum!(Position, []; {Start(value), After(value)});
rewrite_enum!(Evaluated, []; {State(value), Tool(value)});
rewrite_record!(StateRef, []; {configuration, of});

impl RewriteIdentities for ConfigurationRef {
    fn visit_identity_references(
        &self,
        ctx: &DecodeContext<'_>,
        visitor: &mut dyn FnMut(&str) -> Result<(), CodecError>,
    ) -> Result<(), CodecError> {
        match self {
            Self::Local(id) => id.visit_identity_references(ctx, visitor),
            Self::Named(_) => Ok(()),
        }
    }
    fn rewrite_identities<F: FnMut(&str) -> Result<String, CodecError>>(
        self,
        ctx: &DecodeContext<'_>,
        map: &mut IdentityMap<'_, F>,
    ) -> Result<Self, CodecError> {
        Ok(match self {
            Self::Local(id) => Self::Local(id.rewrite_identities(ctx, map)?),
            literal @ Self::Named(_) => literal,
        })
    }
}

rewrite_record!(Selection<O>, [O: MemberOrder]; {at, picks}; literal {native});
rewrite_enum!(Anchor, []; {Input, Pinned(value), Current, Native(value)});
rewrite_enum!(Picks<O>, [O: MemberOrder]; {Unread, Named(value), Rule {terms, excluded, resolved}});
rewrite_record!(Picked, []; {target, context, native, domain, witness, resolved});
rewrite_enum!(PickTarget, []; {
    Entity(value), Native {within}, Sketch(value), SketchEntity(value), SpatialSketchEntity(value),
    SketchRegion {sketch, region}, SketchFrame {sketch, entity}, Datum {feature, part},
    Principal {entity, container}, Feature(value), FeatureResult(value), TemplateInput {instance, input},
    Component, Publication(value),
});
rewrite_enum!(NativeOwner, []; {Sketch(value), SpatialSketch(value), Feature(value)});
rewrite_enum!(PickContext, []; {Occurrence(value), Definition(value), InContext {assembly, own, target}});
rewrite_record!(DefinitionRef, []; {definition, configuration});
rewrite_record!(OccurrencePath, []; (value));
rewrite_record!(PathStep, []; {occurrence, configuration});
rewrite_enum!(StepRef, []; {Occurrence(value), Native(value)});
rewrite_record!(Witness, []; {point, direction, endpoints, frame, curves, surface, body});
rewrite_enum!(PickDomain, []; {EdgeInterval {start, end}, Limits {start, end}, Fragment(value), Containing(value)});
rewrite_record!(RuleTerm<O>, [O: MemberOrder]; {rule, seeds, propagation});
rewrite_enum!(SelectionRule, []; {Explicit, All {kind}, Of {kind}, LoopEdges, Boundary, Chain {end}, Region {boundary}, Intersection {with}, Native(literal)});
rewrite_enum!(Propagation, []; {Off, Minimal, Connected {continuity, angle, gap}, Native(literal)});
rewrite_record!(SketchRegion, []; {outer, holes});
rewrite_record!(BoundaryUse, []; {entity, fragment});
rewrite_scalar!(Ordered);
rewrite_scalar!(Unordered);

rewrite_record!(Construction<kinds::Point>, []; {inputs, after, solution, mode, resolved, resolved_in});
rewrite_record!(Construction<kinds::Direction>, []; {inputs, after, solution, mode, resolved, resolved_in});
rewrite_record!(Construction<kinds::Axis>, []; {inputs, after, solution, mode, resolved, resolved_in});
rewrite_record!(Construction<kinds::Plane>, []; {inputs, after, solution, mode, resolved, resolved_in});
rewrite_record!(Construction<kinds::Frame>, []; {inputs, after, solution, mode, resolved, resolved_in});
rewrite_record!(Construction<kinds::Curve>, []; {inputs, after, solution, mode, resolved, resolved_in});
rewrite_record!(ConstructionInput, []; {role, reference, value, reversed});
rewrite_enum!(InputReference, []; {Picks(value), Built(value)});
rewrite_enum!(AnyConstruction, []; {Point(value), Direction(value), Axis(value), Plane(value), Frame(value), Curve(value)});
rewrite_enum!(ScalarDimension, []; {Length(value), Angle(value), Ratio(value), Uv {u, v}, Position {x, y, z}, Components {x, y, z}, PlaneEquation {a, b, c, d}, ParameterRatio(value)});
rewrite_enum!(PostStep, []; {Flip(value), Translate {axis, distance}, Rotate {axis, angle}, AxisRotate {axis, angle}, Fixed(value)});
rewrite_record!(SourceMode, []; {codec, method, native_options, source_settings});
rewrite_enum!(NativeOption, []; {Bool(value), Integer(value), Real(value), Length(value), Angle(value), Text(value), Selection(value), OrderedSelection(value)});
rewrite_enum!(RefTarget, []; {
    Parameter(value),
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
