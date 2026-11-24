// SPDX-License-Identifier: Apache-2.0
//! The rules on constructions (§5.9, part 10 "Constructions").
//!
//! Each rule reads one construction, nested ones included, and the inputs
//! it states; an input whose role the decoder did not determine is kept and
//! checked against nothing. Where an input's reference is a rule selection
//! or an unread one, its pick count is not stated and is not checked.

use crate::report::check::Ruled;
use std::collections::{BTreeMap, BTreeSet};

use crate::design::construction::{
    CurveValue, FrameValue, InputReference, PlaneValue, PostStep, Relation, ScalarDimension,
    Subject,
};
use crate::design::dimension::Dimension;
use crate::design::feature::FeatureOperation;
use crate::design::names::NameKind;
use crate::design::ops::datum::{Datum, DatumElement};
use crate::design::ops::sketch::{
    EntityDefinition, EntityForm, PlanarCurve, SketchSpace, SpatialCurve,
};
use crate::design::selection::{
    DatumPart, PickContext, PickTarget, Picked, Picks, PrincipalEntity,
};
use crate::math::portable::Portable;
use crate::math::{Point3, Vector3};
use crate::report::check::Finding;
use crate::units::UnitVector3;

use super::index::DesignIndex;
use super::names::derived_kinds;
use super::records::MemberKey;
use super::walk::{ConstructionView, Kind};
use super::{Held, Holder};

/// Checks every construction of the design.
pub(super) fn check<'a>(index: &DesignIndex<'a>, held: &[Held<'a>], findings: &mut Vec<Finding>) {
    let tolerances = &index.records.ir.tolerances;
    let geometry = Geometry {
        linear: tolerances.linear.get(),
        angular: tolerances.angular.get(),
    };
    for record in held {
        let at = record.holder.record();
        let scope = match record.holder {
            Holder::Record { scope, .. } => scope.map(crate::ids::ProductDefinitionId::as_str),
            Holder::Definition(_) | Holder::Owned { .. } => record
                .holder
                .feature()
                .and_then(|feature| index.scope_of(MemberKey::Feature(feature)))
                .map(crate::ids::ProductDefinitionId::as_str),
        };
        for &construction in &record.constructions {
            check_construction(index, construction, scope, geometry, at, findings);
            super::caches::constructions::check_construction(
                &index.records,
                construction,
                scope,
                at,
                findings,
            );
        }
    }
    for feature in &index.records.ir.design.features {
        let at = feature.id.as_str();
        let primitive = match &feature.definition.operation {
            FeatureOperation::Primitive(primitive) => primitive.placement.as_ref(),
            _ => None,
        };
        for placement in feature.placement.as_deref().into_iter().chain(primitive) {
            if placement.inputs.is_empty()
                && placement.after.iter().all(literal_step)
                && placement.resolved.as_ref().is_some_and(is_identity)
            {
                super::error(findings, at, crate::rule!("design.placement-no-inputs-no-bound-step", "a placement with no inputs and no bound step resolves to the identity; it is absent"),
                );
            }
        }
        if let FeatureOperation::Datum(datum) = &feature.definition.operation {
            check_datum_link(datum, at, findings);
        }
    }
    check_principal_datums(index, held, findings);
}

#[derive(Clone, Copy)]
struct Geometry {
    linear: f64,
    angular: f64,
}

fn check_construction(
    index: &DesignIndex<'_>,
    construction: ConstructionView<'_>,
    scope: Option<&str>,
    geometry: Geometry,
    at: &str,
    findings: &mut Vec<Finding>,
) {
    let kind = construction.kind();
    let mut error = |message: Ruled<'_>| super::error(findings, at, message);
    let inputs = construction.inputs();
    let after = construction.after();
    if inputs.is_empty() && after.is_empty() && !construction.has_resolved() {
        error(crate::rule!(
            "design.construction-no-input-no-step-no",
            "a {} construction has no input, no step, and no resolved value",
            kind.name()
        ));
    }
    if construction.solution() == Some(0) {
        error(crate::rule!(
            "design.construction-solution-0-counts-1",
            "a construction's solution is 0; it counts from 1"
        ));
    }
    for step in after {
        match step {
            PostStep::Flip(Dimension::Literal(false)) => {
                error(crate::rule!(
                    "design.flip-step-literal-false-absent",
                    "a Flip step with a literal false is absent"
                ));
            }
            PostStep::AxisRotate { axis, .. }
                if axis
                    .iter()
                    .all(|component| matches!(component, Dimension::Literal(_))) =>
            {
                error(crate::rule!(
                    "design.axisrotate-step-binds-no-axis-component",
                    "an AxisRotate step binds no axis component; it is a Fixed and a Rotate"
                ));
            }
            _ => {}
        }
    }
    let resolved_in = construction.resolved_in();
    if inputs.is_empty() && after.is_empty() && !resolved_in.is_empty() {
        error(crate::rule!(
            "design.construction-no-inputs-no-steps-resolved",
            "a construction with no inputs and no steps has resolved_in values"
        ));
    }
    if let Some(scope) = scope {
        let active = index
            .records
            .scopes
            .get(scope)
            .and_then(|record| record.active_configuration.as_ref());
        for configuration in resolved_in {
            let other = index
                .records
                .ir
                .design
                .configurations
                .iter()
                .any(|record| record.id == *configuration && record.scope.as_str() == scope);
            if !other || active == Some(configuration) {
                error(crate::rule!("design.resolved-names-not-configuration-construction-scope", 
                        "resolved_in names {}, which is not a configuration of the construction's scope other than its active one",
                        configuration.as_str()
                    ),
                );
            }
        }
    }
    for input in inputs {
        if input.reference.is_none() && input.value.is_none() {
            error(crate::rule!(
                "design.construction-input-neither-reference-nor-value",
                "a {} construction has an input with neither a reference nor a value",
                kind.name()
            ));
        }
        let Some(role) = &input.role else {
            continue;
        };
        let (subject, relation) = (role.subject, &role.relation);
        if !subject_admitted(kind, subject) {
            error(crate::rule!(
                "design.construction-subject",
                "a {} construction has a {subject:?} subject",
                kind.name()
            ));
        }
        if subject == Subject::ZAxis
            && matches!(
                relation,
                Relation::Parallel | Relation::Perpendicular | Relation::Tangent
            )
        {
            error(crate::rule!(
                "design.zaxis-subject-carries-relation-whole-carries",
                "a ZAxis subject carries a {relation:?} relation; the whole carries it"
            ));
        }
        if !value_admitted(
            kind,
            subject,
            relation,
            input.value.as_ref(),
            input.reference.is_some(),
        ) {
            error(crate::rule!(
                "design.input-construction-value-does-not-take",
                "a {relation:?} input of a {} construction has a value it does not take",
                kind.name()
            ));
        }
        if let Some(ScalarDimension::PlaneEquation { a, b, c, .. }) = &input.value {
            let zero = |value: &Dimension<crate::scalar::FiniteReal>| matches!(value, Dimension::Literal(value) if value.get() == 0.0);
            if zero(a) && zero(b) && zero(c) {
                error(crate::rule!(
                    "design.planeequation-literal-b-c-all-zero",
                    "a PlaneEquation's literal a, b, and c are all zero"
                ));
            }
        }
        if matches!(relation, Relation::Radius | Relation::Limit) && kind != Kind::Curve {
            error(crate::rule!(
                "design.input-occurs-construction",
                "a {relation:?} input occurs on a {} construction",
                kind.name()
            ));
        }
        if matches!(relation, Relation::Fold)
            && (!matches!(kind, Kind::Plane | Kind::Frame) || subject != Subject::Whole)
        {
            error(crate::rule!(
                "design.fold-input-not-whole-plane-frame",
                "a Fold input is not the Whole of a plane or frame"
            ));
        }
        if input.reversed.is_some() && !reversible(construction, subject, relation) {
            error(crate::rule!(
                "design.input-states-reversed",
                "a {relation:?} input states reversed"
            ));
        }
        super::fit::check_input(index, relation, input.reference.as_ref(), &mut error);
        if matches!(input.value, Some(ScalarDimension::ParameterRatio(_)))
            && !parameter_ratio_admitted(index, relation, input.reference.as_ref())
        {
            error(crate::rule!("design.parameterratio-value-not-atparameter-input-picks", "a ParameterRatio value is not on an AtParameter input that picks one edge or curve"),
            );
        }
        check_ratio_spelling(
            index,
            input.value.as_ref(),
            input.reference.as_ref(),
            &mut error,
        );
        match &input.reference {
            Some(InputReference::Picks(selection)) => {
                if let Picks::Named(picks) = &selection.picks {
                    let picks = picks.as_slice();
                    if !pick_count_admitted(relation, picks.len()) {
                        error(crate::rule!(
                            "design.input-reference-holds-picks",
                            "a {relation:?} input's reference holds {} picks",
                            picks.len()
                        ));
                    }
                    if matches!(relation, Relation::Coordinates)
                        && picks
                            .first()
                            .is_some_and(|first| !picks_frame(index, first))
                    {
                        error(crate::rule!(
                            "design.coordinates-reference-does-not-pick-frame",
                            "a Coordinates reference does not pick a frame first"
                        ));
                    }
                    if subject == Subject::Whole
                        && matches!(relation, Relation::Coincident)
                        && kind != Kind::Frame
                        && matches!(picks, [one] if picks_whole_frame(index, one))
                    {
                        error(crate::rule!(
                            "design.whole-coincident-input-whole-frame-construction",
                            "a Whole Coincident input on a whole frame is on a {} construction",
                            kind.name()
                        ));
                    }
                }
            }
            Some(InputReference::Built(built)) => {
                if matches!(relation, Relation::Fold) {
                    error(crate::rule!(
                        "design.nested-construction-reference-input",
                        "a nested construction is the reference of a {relation:?} input"
                    ));
                }
                if subject == Subject::Whole
                    && matches!(relation, Relation::Coincident)
                    && kind != Kind::Frame
                {
                    if let crate::design::construction::AnyConstruction::Frame(_) = built.as_ref() {
                        error(crate::rule!(
                            "design.whole-coincident-input-whole-frame-construction",
                            "a Whole Coincident input on a whole frame is on a {} construction",
                            kind.name()
                        ));
                    }
                }
            }
            None => {}
        }
    }
    if matches!(kind, Kind::Plane | Kind::Frame) && leading_offset(index, construction) {
        error(crate::rule!("design.plane-frame-coincident-one-planar-reference", "a plane or frame Coincident with one planar reference begins its steps with a Z translation; it is an Offset input"),
        );
    }
    check_values(construction, geometry, at, findings);
}

const fn subject_admitted(kind: Kind, subject: Subject) -> bool {
    match kind {
        Kind::Point | Kind::Direction | Kind::Axis => matches!(subject, Subject::Whole),
        Kind::Plane => !matches!(subject, Subject::ZAxis),
        Kind::Frame => true,
        Kind::Curve => matches!(subject, Subject::Whole | Subject::Origin),
    }
}

/// The values each relation takes (part 10 "Values by relation").
fn value_admitted(
    kind: Kind,
    subject: Subject,
    relation: &Relation,
    value: Option<&ScalarDimension>,
    referenced: bool,
) -> bool {
    use ScalarDimension as V;
    match relation {
        Relation::Offset => matches!(value, Some(V::Length(_))),
        Relation::Angle => matches!(value, Some(V::Angle(_))),
        Relation::AtParameter => matches!(
            value,
            Some(V::Ratio(_) | V::ParameterRatio(_) | V::Length(_) | V::Uv { .. })
        ),
        Relation::Coordinates => match (kind, subject) {
            (Kind::Point, Subject::Whole) | (_, Subject::Origin) => {
                matches!(value, Some(V::Position { .. }))
            }
            (Kind::Direction | Kind::Axis, Subject::Whole)
            | (_, Subject::XAxis | Subject::YAxis | Subject::ZAxis) => {
                matches!(value, Some(V::Components { .. }))
            }
            (Kind::Plane | Kind::Frame, Subject::Whole) => {
                matches!(value, Some(V::PlaneEquation { .. }))
            }
            (Kind::Curve, Subject::Whole) => false,
        },
        Relation::Midway => matches!(value, None | Some(V::Ratio(_))),
        Relation::Radius => matches!(value, Some(V::Length(_))) || value.is_none() && referenced,
        Relation::Limit => {
            matches!(value, Some(V::Length(_) | V::Angle(_))) || value.is_none() && referenced
        }
        Relation::Native(_) => true,
        Relation::Coincident
        | Relation::Parallel
        | Relation::Perpendicular
        | Relation::Tangent
        | Relation::Center
        | Relation::Centroid
        | Relation::AxisOf
        | Relation::Projection
        | Relation::PrincipalNormal
        | Relation::Binormal
        | Relation::Fold => value.is_none(),
    }
}

/// Whether an input of `relation` on `subject` may state `reversed`: a
/// `Coincident` one only where the subject is a line.
fn reversible(construction: ConstructionView<'_>, subject: Subject, relation: &Relation) -> bool {
    match relation {
        Relation::Parallel
        | Relation::Perpendicular
        | Relation::Tangent
        | Relation::AxisOf
        | Relation::PrincipalNormal
        | Relation::Binormal
        | Relation::AtParameter => true,
        Relation::Coincident => match subject {
            Subject::XAxis | Subject::YAxis | Subject::ZAxis => true,
            Subject::Origin => false,
            Subject::Whole => match construction {
                ConstructionView::Axis(_) => true,
                ConstructionView::Curve(curve) => curve
                    .resolved
                    .as_ref()
                    .is_none_or(|value| matches!(value, CurveValue::Segment { .. })),
                ConstructionView::Point(_)
                | ConstructionView::Direction(_)
                | ConstructionView::Plane(_)
                | ConstructionView::Frame(_) => false,
            },
        },
        _ => false,
    }
}

/// Whether a `ParameterRatio` value's input is `AtParameter` and its
/// reference picks one edge or curve. An unread reference states no picks.
fn parameter_ratio_admitted(
    index: &DesignIndex<'_>,
    relation: &Relation,
    reference: Option<&InputReference>,
) -> bool {
    matches!(relation, Relation::AtParameter)
        && match reference {
            Some(InputReference::Picks(selection)) => match &selection.picks {
                Picks::Named(picks) => {
                    matches!(picks.as_slice(), [one] if picks_curve(index, one))
                }
                Picks::Unread => true,
                Picks::Rule { .. } => false,
            },
            Some(InputReference::Built(_)) | None => false,
        }
}

/// A `ParameterRatio` value is no literal 0 or 1, and picks no curve the
/// document states as a line, a circle, or a circular arc, whose spelling is
/// `Ratio`; a datum axis, a principal axis, and an `InfiniteLine` have no
/// ends and take neither a `Ratio` nor a `ParameterRatio` (§5.9).
fn check_ratio_spelling(
    index: &DesignIndex<'_>,
    value: Option<&ScalarDimension>,
    reference: Option<&InputReference>,
    error: &mut impl FnMut(Ruled<'_>),
) {
    let parameter_ratio = match value {
        Some(ScalarDimension::ParameterRatio(ratio)) => Some(ratio),
        Some(ScalarDimension::Ratio(_)) => None,
        _ => return,
    };
    if let Some(Dimension::Literal(ratio)) = parameter_ratio {
        if ratio.get() == 0.0 || ratio.get() == 1.0 {
            error(crate::rule!(
                "design.parameterratio-value-literal-end-curve",
                "a ParameterRatio value is a literal {}, an end of the curve",
                ratio.get()
            ));
        }
    }
    let Some(InputReference::Picks(selection)) = reference else {
        return;
    };
    let Picks::Named(picks) = &selection.picks else {
        return;
    };
    for pick in picks.as_slice() {
        match stated_curve(index, pick) {
            StatedCurve::Endless => error(crate::rule!("design.ratio-parameterratio-value-picks-datum-axis", 
                "a Ratio or ParameterRatio value picks a datum axis, a principal axis, or an infinite line, which has no ends"
            )),
            StatedCurve::Conic if parameter_ratio.is_some() => error(crate::rule!("design.parameterratio-value-picks-curve-document-states", 
                "a ParameterRatio value picks a curve the document states as a line, a circle, or an arc; it is a Ratio"
            )),
            StatedCurve::Conic | StatedCurve::Other => {}
        }
    }
}

/// What the document states a picked curve to be, where a ratio along it
/// reads that.
enum StatedCurve {
    /// A line with no ends: a datum axis, a principal axis, or an
    /// `InfiniteLine` sketch entity.
    Endless,
    /// A sketch entity stated as a bounded line, a circle, or a circular arc.
    Conic,
    /// Any other pick, or one whose curve is not stated.
    Other,
}

fn stated_curve(index: &DesignIndex<'_>, pick: &Picked) -> StatedCurve {
    match &pick.target {
        PickTarget::Principal { entity, .. }
        | PickTarget::Datum {
            part: Some(DatumPart::Principal(entity)),
            ..
        } if matches!(
            entity,
            PrincipalEntity::XAxis | PrincipalEntity::YAxis | PrincipalEntity::ZAxis
        ) =>
        {
            StatedCurve::Endless
        }
        PickTarget::Datum {
            feature,
            part: None,
        } => match datum_element(index, feature.as_str()) {
            DatumFeature::Datum(DatumElement::Axis(_)) => StatedCurve::Endless,
            _ => StatedCurve::Other,
        },
        PickTarget::SketchEntity(entity) => {
            match index
                .sketch_entities
                .get(entity.as_str())
                .and_then(|entity| solved_curve(&entity.definition))
            {
                Some(PlanarCurve::InfiniteLine { .. }) => StatedCurve::Endless,
                Some(
                    PlanarCurve::Line { .. } | PlanarCurve::Circle { .. } | PlanarCurve::Arc { .. },
                ) => StatedCurve::Conic,
                _ => StatedCurve::Other,
            }
        }
        PickTarget::SpatialSketchEntity(entity) => {
            match index
                .spatial_sketch_entities
                .get(entity.as_str())
                .and_then(|entity| solved_curve(&entity.definition))
            {
                Some(SpatialCurve::Bounded(bounded)) => match &bounded.carrier {
                    crate::geometry::InlineCurve::Circle(_) => StatedCurve::Conic,
                    crate::geometry::InlineCurve::Line(_) if bounded.range.is_some() => {
                        StatedCurve::Conic
                    }
                    _ => StatedCurve::Other,
                },
                _ => StatedCurve::Other,
            }
        }
        _ => StatedCurve::Other,
    }
}

/// A sketch entity's curve: a free entity's own, a defined one's solved
/// cache.
fn solved_curve<S: SketchSpace>(definition: &EntityDefinition<S>) -> Option<&S::Curve> {
    match definition {
        EntityDefinition::Free(curve) => Some(curve),
        EntityDefinition::Defined { solved, .. } => solved.as_ref(),
    }
}

/// The number of picks each relation's reference holds.
const fn pick_count_admitted(relation: &Relation, count: usize) -> bool {
    match relation {
        Relation::Midway => matches!(count, 1 | 2),
        Relation::Centroid => count >= 1,
        Relation::Fold => count == 4,
        Relation::AtParameter | Relation::Coordinates => matches!(count, 1 | 2),
        Relation::Native(_) => true,
        _ => count == 1,
    }
}

/// What a datum pick's feature is known to be.
pub(super) enum DatumFeature<'a> {
    /// A typed datum.
    Datum(&'a DatumElement),
    /// A typed operation other than a datum.
    Other,
    /// An unresolved or native operation, or no such feature.
    Unstated,
}

pub(super) fn datum_element<'a>(index: &DesignIndex<'a>, feature: &str) -> DatumFeature<'a> {
    match index.operation(feature) {
        Some(FeatureOperation::Datum(datum)) => DatumFeature::Datum(&datum.element),
        Some(FeatureOperation::Unresolved { .. } | FeatureOperation::Native { .. }) | None => {
            DatumFeature::Unstated
        }
        Some(_) => DatumFeature::Other,
    }
}

/// Whether a pick may pick a coordinate frame: a frame datum, a principal or
/// sketch frame, or a pick whose target is not stated.
fn picks_frame(index: &DesignIndex<'_>, pick: &Picked) -> bool {
    match &pick.target {
        PickTarget::Datum { feature, part } => match part {
            Some(DatumPart::Principal(entity)) => *entity == PrincipalEntity::Frame,
            Some(DatumPart::Point(_)) => false,
            None => matches!(
                datum_element(index, feature.as_str()),
                DatumFeature::Unstated | DatumFeature::Datum(DatumElement::Frame(_))
            ),
        },
        PickTarget::Principal { entity, .. } | PickTarget::SketchFrame { entity, .. } => {
            *entity == PrincipalEntity::Frame
        }
        PickTarget::Native { .. } => true,
        _ => false,
    }
}

/// Whether a pick is known to pick a whole frame.
fn picks_whole_frame(index: &DesignIndex<'_>, pick: &Picked) -> bool {
    match &pick.target {
        PickTarget::Datum {
            feature,
            part: None,
        } => {
            matches!(
                datum_element(index, feature.as_str()),
                DatumFeature::Datum(DatumElement::Frame(_))
            )
        }
        PickTarget::Datum {
            part: Some(DatumPart::Principal(entity)),
            ..
        }
        | PickTarget::Principal { entity, .. }
        | PickTarget::SketchFrame { entity, .. } => *entity == PrincipalEntity::Frame,
        _ => false,
    }
}

/// Whether a pick may pick one edge or curve: an axis datum is a line with
/// no parameter range, and a sketch point entity is no curve. A sketch
/// entity that does not exist is the existence check's finding.
fn picks_curve(index: &DesignIndex<'_>, pick: &Picked) -> bool {
    match &pick.target {
        PickTarget::Entity(name) => {
            let kinds = derived_kinds(name);
            kinds.len() == 1
                && kinds
                    .first()
                    .is_some_and(|kind| matches!(kind, NameKind::Edge | NameKind::Curve))
        }
        PickTarget::SketchEntity(entity) => index
            .sketch_entities
            .get(entity.as_str())
            .is_none_or(|entity| entity_is_curve(&entity.definition, planar_point)),
        PickTarget::SpatialSketchEntity(entity) => index
            .spatial_sketch_entities
            .get(entity.as_str())
            .is_none_or(|entity| entity_is_curve(&entity.definition, spatial_point)),
        PickTarget::Native { .. } => true,
        PickTarget::Datum {
            feature,
            part: None,
        } => matches!(
            datum_element(index, feature.as_str()),
            DatumFeature::Unstated
        ),
        _ => false,
    }
}

/// Whether a sketch entity may be a curve. A `Frame` is a point entity, and
/// a `Text` or `BlockInstance` is no curve; a `Native` form, or another form
/// with no solved curve, states no kind.
fn entity_is_curve<S: SketchSpace>(
    definition: &EntityDefinition<S>,
    is_point: fn(&S::Curve) -> bool,
) -> bool {
    match definition {
        EntityDefinition::Free(curve) => !is_point(curve),
        EntityDefinition::Defined { form, solved } => match form.as_ref() {
            EntityForm::Frame | EntityForm::Text(_) | EntityForm::BlockInstance(_) => false,
            EntityForm::Native(_) => true,
            EntityForm::Interpolated(_)
            | EntityForm::Conic { .. }
            | EntityForm::Derived(_)
            | EntityForm::Instanced { .. } => solved.as_ref().is_none_or(|curve| !is_point(curve)),
        },
    }
}

fn planar_point(curve: &PlanarCurve) -> bool {
    matches!(
        curve,
        PlanarCurve::Point(_) | PlanarCurve::Spatial(SpatialCurve::Point(_))
    )
}

fn spatial_point(curve: &SpatialCurve) -> bool {
    matches!(curve, SpatialCurve::Point(_))
}

/// Whether a pick is known to pick a plane.
fn picks_plane(index: &DesignIndex<'_>, pick: &Picked) -> bool {
    let planar = |entity: &PrincipalEntity| {
        matches!(
            entity,
            PrincipalEntity::XyPlane | PrincipalEntity::XzPlane | PrincipalEntity::YzPlane
        )
    };
    match &pick.target {
        PickTarget::Datum {
            feature,
            part: None,
        } => {
            matches!(
                datum_element(index, feature.as_str()),
                DatumFeature::Datum(DatumElement::Plane(_))
            )
        }
        PickTarget::Datum {
            part: Some(DatumPart::Principal(entity)),
            ..
        }
        | PickTarget::Principal { entity, .. }
        | PickTarget::SketchFrame { entity, .. } => planar(entity),
        _ => false,
    }
}

/// Whether a plane or frame whose whole is `Coincident` with one planar
/// reference begins its steps with a translation along Z.
fn leading_offset(index: &DesignIndex<'_>, construction: ConstructionView<'_>) -> bool {
    let planar_whole = construction.inputs().iter().any(|input| {
        let whole_coincident = input
            .role
            .as_ref()
            .is_some_and(|role| role.subject == Subject::Whole && matches!(role.relation, Relation::Coincident));
        whole_coincident
            && match &input.reference {
                Some(InputReference::Picks(selection)) => {
                    matches!(&selection.picks, Picks::Named(picks) if matches!(picks.as_slice(), [one] if picks_plane(index, one)))
                }
                Some(InputReference::Built(built)) => {
                    matches!(built.as_ref(), crate::design::construction::AnyConstruction::Plane(_))
                }
                None => false,
            }
    });
    planar_whole
        && matches!(
            construction.after().first(),
            Some(PostStep::Translate {
                axis: crate::design::construction::FrameAxis::Z,
                ..
            })
        )
}

const fn literal_step(step: &PostStep) -> bool {
    match step {
        PostStep::Flip(dimension) => matches!(dimension, Dimension::Literal(_)),
        PostStep::Translate { distance, .. } => matches!(distance, Dimension::Literal(_)),
        PostStep::Rotate { angle, .. } => matches!(angle, Dimension::Literal(_)),
        PostStep::AxisRotate { axis, angle } => {
            matches!(angle, Dimension::Literal(_))
                && matches!(axis[0], Dimension::Literal(_))
                && matches!(axis[1], Dimension::Literal(_))
                && matches!(axis[2], Dimension::Literal(_))
        }
        PostStep::Fixed(_) => true,
    }
}

fn is_identity(frame: &FrameValue) -> bool {
    let origin = frame.origin.get();
    origin.x == 0.0
        && origin.y == 0.0
        && origin.z == 0.0
        && frame.x_axis == UnitVector3::X_AXIS
        && frame.y_axis == UnitVector3::Y_AXIS
        && frame.z_axis == UnitVector3::Z_AXIS
}

/// A datum with a link has a construction with one `Coincident` input or
/// none; its link has no source, and a document only where the construction
/// has no input.
fn check_datum_link(datum: &Datum, at: &str, findings: &mut Vec<Finding>) {
    let Some(link) = &datum.link else {
        return;
    };
    let inputs: Vec<usize> = match &datum.element {
        DatumElement::Points(points) => points.iter().map(|point| point.inputs.len()).collect(),
        DatumElement::Axis(axis) => vec![axis.inputs.len()],
        DatumElement::Plane(plane) => vec![plane.inputs.len()],
        DatumElement::Frame(frame) => vec![frame.inputs.len()],
    };
    let coincident = |inputs: &[crate::design::construction::ConstructionInput]| match inputs {
        [] => true,
        [one] => one
            .role
            .as_ref()
            .is_none_or(|role| matches!(role.relation, Relation::Coincident)),
        _ => false,
    };
    let shapes = match &datum.element {
        DatumElement::Points(points) => points.iter().all(|point| coincident(&point.inputs)),
        DatumElement::Axis(axis) => coincident(&axis.inputs),
        DatumElement::Plane(plane) => coincident(&plane.inputs),
        DatumElement::Frame(frame) => coincident(&frame.inputs),
    };
    let mut error = |message: Ruled<'_>| {
        super::error(findings, at, message);
    };
    if !shapes {
        error(crate::rule!(
            "design.linked-datum-construction-input-other-one",
            "a linked datum's construction has an input other than one Coincident input"
        ));
    }
    if link.source.is_some() {
        error(crate::rule!(
            "design.linked-datum-link-states-source-construction",
            "a linked datum's link states a source; its construction's input picks it"
        ));
    }
    if link.document.is_some() && inputs.iter().any(|count| *count > 0) {
        error(crate::rule!(
            "design.linked-datum-link-states-document-while",
            "a linked datum's link states a document while its construction has an input"
        ));
    }
}

/// A datum feature whose construction is one `Coincident` input on a
/// `Principal` entity, with no steps, is the only one for that entity and
/// container, and every other pick of the entity in its definition picks
/// the feature as `Datum` (§6.1, §6.2).
fn check_principal_datums(index: &DesignIndex<'_>, held: &[Held<'_>], findings: &mut Vec<Finding>) {
    let mut restating: BTreeMap<(&str, PrincipalEntity, Option<&str>), Vec<&str>> = BTreeMap::new();
    for feature in &index.records.ir.design.features {
        let FeatureOperation::Datum(datum) = &feature.definition.operation else {
            continue;
        };
        let Some(scope) = index.scope_of(MemberKey::Feature(feature.id.as_str())) else {
            continue;
        };
        if let Some((entity, container)) = principal_restatement(datum) {
            restating
                .entry((scope.as_str(), entity, container))
                .or_default()
                .push(feature.id.as_str());
        }
    }
    for ((_, entity, _), features) in &restating {
        if let [first, others @ ..] = features.as_slice() {
            for other in others {
                super::error(findings, other, crate::rule!("design.datum-feature-restates-principal-datum-feature", "datum feature restates principal {entity:?}, which datum feature {first} already restates"),
                );
            }
        }
    }
    let restated: BTreeSet<(&str, PrincipalEntity, Option<&str>)> =
        restating.keys().copied().collect();
    let own: BTreeSet<&str> = restating.values().flatten().copied().collect();
    for record in held {
        let Some(feature) = record.holder.feature() else {
            continue;
        };
        let Some(definition) = index.owner_definition(feature) else {
            continue;
        };
        if own.contains(feature) {
            continue;
        }
        for selection in &record.selections {
            for pick in selection.picks() {
                let PickTarget::Principal { entity, container } = &pick.target else {
                    continue;
                };
                if matches!(
                    pick.context,
                    Some(
                        PickContext::Occurrence(_)
                            | PickContext::Definition(_)
                            | PickContext::InContext { .. }
                    )
                ) {
                    continue;
                }
                let key = (
                    definition.as_str(),
                    *entity,
                    container
                        .as_ref()
                        .map(crate::design::ids::ContainerId::as_str),
                );
                if restated.contains(&key) {
                    super::error(findings, record.holder.record(), crate::rule!("design.pick-names-principal-datum-feature-restates", "a pick names principal {entity:?}, which a datum feature restates; it picks that feature as Datum"),
                    );
                }
            }
        }
    }
}

/// The principal entity and container a datum restates.
fn principal_restatement(datum: &Datum) -> Option<(PrincipalEntity, Option<&str>)> {
    let (inputs, after) = match &datum.element {
        DatumElement::Axis(axis) => (&axis.inputs, &axis.after),
        DatumElement::Plane(plane) => (&plane.inputs, &plane.after),
        DatumElement::Frame(frame) => (&frame.inputs, &frame.after),
        DatumElement::Points(points) => match points.as_slice() {
            [point] => (&point.inputs, &point.after),
            _ => return None,
        },
    };
    let [input] = inputs.as_slice() else {
        return None;
    };
    if !after.is_empty()
        || !input
            .role
            .as_ref()
            .is_some_and(|role| matches!(role.relation, Relation::Coincident))
    {
        return None;
    }
    let Some(InputReference::Picks(selection)) = &input.reference else {
        return None;
    };
    let Picks::Named(picks) = &selection.picks else {
        return None;
    };
    match picks.as_slice() {
        [Picked {
            target: PickTarget::Principal { entity, container },
            context: None,
            ..
        }] => Some((
            *entity,
            container
                .as_ref()
                .map(crate::design::ids::ContainerId::as_str),
        )),
        _ => None,
    }
}

/// The resolved values of a construction are the values of its kind:
/// a plane's x axis perpendicular to its normal, a frame orthonormal and
/// right-handed, an arc's ends on its circle.
fn check_values(
    construction: ConstructionView<'_>,
    geometry: Geometry,
    at: &str,
    findings: &mut Vec<Finding>,
) {
    let mut bad = Vec::new();
    match construction {
        ConstructionView::Plane(plane) => {
            for value in plane.resolved.iter().chain(plane.resolved_in.values()) {
                if !plane_ok(value, geometry) {
                    bad.push("a plane's x axis is not perpendicular to its normal");
                }
            }
        }
        ConstructionView::Frame(frame) => {
            for value in frame.resolved.iter().chain(frame.resolved_in.values()) {
                if !frame_ok(value, geometry) {
                    bad.push("a frame's axes are not orthonormal and right-handed");
                }
            }
        }
        ConstructionView::Curve(curve) => {
            for value in curve.resolved.iter().chain(curve.resolved_in.values()) {
                if !arc_ok(value, geometry) {
                    bad.push("an arc's start or end is not on its circle");
                }
            }
        }
        ConstructionView::Point(_) | ConstructionView::Direction(_) | ConstructionView::Axis(_) => {
        }
    }
    for message in bad {
        super::error(
            findings,
            at,
            crate::rule!("design.construction-resolved-values", "{message}"),
        );
    }
}

fn plane_ok(plane: &PlaneValue, geometry: Geometry) -> bool {
    Vector3::from(plane.x_axis)
        .dot(Vector3::from(plane.normal))
        .abs()
        <= geometry.angular.portable_sin()
}

fn frame_ok(frame: &FrameValue, geometry: Geometry) -> bool {
    let (x, y, z) = (
        Vector3::from(frame.x_axis),
        Vector3::from(frame.y_axis),
        Vector3::from(frame.z_axis),
    );
    let tolerance = geometry.angular.portable_sin();
    let cross = x.cross(y);
    x.dot(y).abs() <= tolerance
        && y.dot(z).abs() <= tolerance
        && z.dot(x).abs() <= tolerance
        && cross.dot(z) >= 1.0 - tolerance
}

fn arc_ok(curve: &CurveValue, geometry: Geometry) -> bool {
    let CurveValue::Arc {
        center,
        normal,
        radius,
        start,
        end,
    } = curve
    else {
        return true;
    };
    let center = center.get();
    let normal = Vector3::from(*normal);
    let on_circle = |point: Point3| {
        let offset = Vector3 {
            x: point.x - center.x,
            y: point.y - center.y,
            z: point.z - center.z,
        };
        (offset.norm() - radius.get()).abs() <= geometry.linear
            && offset.dot(normal).abs() <= geometry.linear
    };
    on_circle(start.get()) && on_circle(end.get())
}
