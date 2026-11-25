use super::geometry_error;
use crate::design::construction::{
    AnyConstruction, Construction, ConstructionInput, ConstructionKind, CurveValue, FrameValue,
    InputReference, InputRole, PostStep, Relation, ScalarDimension, Subject,
};
use crate::design::dimension::Dimension;
use crate::design::options::{NativeOption, NativeOptions};
use crate::design::selection::{
    MemberOrder, Members, PickDomain, Picked, Picks, Propagation, RuleTerm, Selection,
    SelectionRule,
};
use crate::features::datums::DatumAxis;
use crate::features::parameters::ParameterValue;
use crate::features::Feature;
use crate::index::ModelIndex;
use crate::math::portable::Portable;
use crate::report::check::Ruled;
use crate::report::{
    check::{Check, Finding},
    Severity,
};
use crate::schema::rewrite::typed::RewriteIdentities;
use cadmpeg_core::decode::DecodeContext;
use cadmpeg_core::CodecError;

mod references;

pub(super) fn validate(
    ctx: &DecodeContext<'_>,
    ids: &ModelIndex<'_>,
    owner: &Feature,
    axis: &DatumAxis,
    linear_tolerance: f64,
    findings: &mut Vec<Finding>,
) -> Result<(), CodecError> {
    references::validate(ctx, ids, owner, axis, findings)?;
    let mut references = std::collections::HashSet::new();
    axis.visit_identity_references(ctx, &mut |id| {
        references.insert(id.to_owned());
        Ok(())
    })?;
    for reference in references {
        let Some(feature) = ids.features(&reference, crate::index::StandardIndex) else {
            continue;
        };
        if feature.ordinal >= owner.ordinal
            || !owner.dependencies.iter().any(|id| id == &feature.id)
        {
            error(
                findings,
                owner,
                crate::rule!("topology.datum-axis-feature-input-must-precede", "datum-axis feature input must precede its consumer and occur in its dependencies"),
            );
        }
    }
    let mut walk = Walk {
        ctx,
        ids,
        owner,
        linear_tolerance,
        findings,
    };
    walk.inputs(axis.inputs(), Kind::Axis)?;
    walk.options(axis.native_options())?;
    walk.options(axis.source_settings())
}
fn error(findings: &mut Vec<Finding>, owner: &Feature, message: Ruled<'_>) {
    super::super::record_finding(
        findings,
        Check::ReferentialIntegrity,
        Severity::Error,
        Some(owner.id.as_str()),
        message,
    );
}
struct Walk<'a, 'b, 'ir> {
    ctx: &'a DecodeContext<'b>,
    ids: &'a ModelIndex<'ir>,
    owner: &'a Feature,
    linear_tolerance: f64,
    findings: &'a mut Vec<Finding>,
}
#[derive(Clone, Copy, PartialEq, Eq)]
enum Kind {
    Point,
    Direction,
    Axis,
    Plane,
    Frame,
    Curve,
}
impl Kind {
    fn admits(self, subject: Subject) -> bool {
        match self {
            Self::Point | Self::Direction | Self::Axis => subject == Subject::Whole,
            Self::Plane => subject != Subject::ZAxis,
            Self::Frame => true,
            Self::Curve => matches!(subject, Subject::Whole | Subject::Origin),
        }
    }
}
impl Walk<'_, '_, '_> {
    fn role(&mut self, input: &ConstructionInput, role: &InputRole, kind: Kind) {
        if matches!(role.relation, Relation::Native(_)) {
            return;
        }
        if role.subject == Subject::ZAxis
            && matches!(
                role.relation,
                Relation::Parallel | Relation::Perpendicular | Relation::Tangent
            )
        {
            error(
                self.findings,
                self.owner,
                crate::rule!(
                    "topology.zaxis-constraint-must-act-whole-construction",
                    "a ZAxis constraint must act on the whole construction"
                ),
            );
        }
        if kind != Kind::Curve && matches!(role.relation, Relation::Radius | Relation::Limit) {
            error(
                self.findings,
                self.owner,
                crate::rule!(
                    "topology.radius-limit-require-curve-construction",
                    "Radius and Limit require a curve construction"
                ),
            );
        }
        if matches!(role.relation, Relation::Fold)
            && (!matches!(kind, Kind::Plane | Kind::Frame) || role.subject != Subject::Whole)
        {
            error(
                self.findings,
                self.owner,
                crate::rule!(
                    "topology.fold-requires-whole-plane-frame",
                    "Fold requires the whole of a plane or frame"
                ),
            );
        }
        if input.reversed.is_some()
            && !matches!(
                role.relation,
                Relation::Parallel
                    | Relation::Perpendicular
                    | Relation::Tangent
                    | Relation::AxisOf
                    | Relation::Coincident
                    | Relation::PrincipalNormal
                    | Relation::Binormal
                    | Relation::AtParameter
            )
        {
            error(
                self.findings,
                self.owner,
                crate::rule!(
                    "topology.construction-relation-does-not-admit-reversed",
                    "construction relation does not admit reversed"
                ),
            );
        }
        if let Some(value) = &input.value {
            let valid = match role.relation {
                Relation::Offset => matches!(value, ScalarDimension::Length(_)),
                Relation::Angle => matches!(value, ScalarDimension::Angle(_)),
                Relation::AtParameter => matches!(
                    value,
                    ScalarDimension::Ratio(_)
                        | ScalarDimension::ParameterRatio(_)
                        | ScalarDimension::Length(_)
                        | ScalarDimension::Uv { .. }
                ),
                Relation::Coordinates => match (kind, role.subject) {
                    (Kind::Point, _) | (_, Subject::Origin) => {
                        matches!(value, ScalarDimension::Position { .. })
                    }
                    (Kind::Direction | Kind::Axis, _) => {
                        matches!(value, ScalarDimension::Components { .. })
                    }
                    (Kind::Plane | Kind::Frame, Subject::Whole) => {
                        matches!(value, ScalarDimension::PlaneEquation { .. })
                    }
                    _ => matches!(value, ScalarDimension::Components { .. }),
                },
                Relation::Midway => matches!(value, ScalarDimension::Ratio(_)),
                Relation::Radius => matches!(value, ScalarDimension::Length(_)),
                Relation::Limit => matches!(
                    value,
                    ScalarDimension::Length(_) | ScalarDimension::Angle(_)
                ),
                _ => false,
            };
            if !valid {
                error(
                    self.findings,
                    self.owner,
                    crate::rule!(
                        "topology.construction-relation-incompatible-scalar-value",
                        "construction relation has an incompatible scalar value"
                    ),
                );
            }
        }
        if let Some(InputReference::Picks(Selection {
            picks: Picks::Named(values),
            ..
        })) = &input.reference
        {
            let count = values.as_slice().len();
            let valid = match role.relation {
                Relation::Midway | Relation::AtParameter | Relation::Coordinates => {
                    matches!(count, 1 | 2)
                }
                Relation::Centroid => count > 0,
                Relation::Fold => count == 4,
                _ => count == 1,
            };
            if !valid {
                error(
                    self.findings,
                    self.owner,
                    crate::rule!(
                        "topology.construction-relation-incompatible-operand-count",
                        "construction relation has an incompatible operand count"
                    ),
                );
            }
        }
        if let Some(InputReference::Built(value)) = &input.reference {
            if matches!(role.relation, Relation::Fold) {
                error(
                    self.findings,
                    self.owner,
                    crate::rule!(
                        "topology.fold-requires-four-picked-edges",
                        "Fold requires four picked edges"
                    ),
                );
            }
            if matches!(role.relation, Relation::Coordinates)
                && !matches!(value.as_ref(), AnyConstruction::Frame(_))
            {
                error(
                    self.findings,
                    self.owner,
                    crate::rule!(
                        "topology.coordinates-requires-frame-reference",
                        "Coordinates requires a frame reference"
                    ),
                );
            }
            if kind != Kind::Frame
                && role.subject == Subject::Whole
                && matches!(role.relation, Relation::Coincident)
                && matches!(value.as_ref(), AnyConstruction::Frame(_))
            {
                error(
                    self.findings,
                    self.owner,
                    crate::rule!(
                        "topology.whole-coincident-frame-requires-frame-result",
                        "a whole coincident frame requires a frame result"
                    ),
                );
            }
        }
    }
    fn frame(&mut self, value: &FrameValue) {
        if crate::features::geometry::FeatureCoordinateFrame::new(
            value.origin.get(),
            *value.x_axis.as_raw(),
            *value.y_axis.as_raw(),
            *value.z_axis.as_raw(),
        )
        .is_none()
        {
            geometry_error(
                self.findings,
                self.owner.id.as_str(),
                crate::rule!(
                    "topology.construction-frame-must-orthonormal-right-handed",
                    "construction frame must be orthonormal and right-handed"
                ),
            );
        }
    }
    fn curve(&mut self, value: &CurveValue) {
        if let CurveValue::Arc {
            center,
            normal,
            radius,
            start,
            end,
        } = value
        {
            for point in [start, end] {
                let delta = point.get().vector_from(center.get());
                let length = delta.x.portable_hypot(delta.y).portable_hypot(delta.z);
                let plane = delta.dot(*normal.as_raw()).abs();
                if !length.is_finite()
                    || !plane.is_finite()
                    || (length - radius.get()).abs() > self.linear_tolerance
                    || plane > self.linear_tolerance
                {
                    geometry_error(
                        self.findings,
                        self.owner.id.as_str(),
                        crate::rule!(
                            "topology.construction-arc-endpoint-outside-radius-plane",
                            "construction arc endpoint is outside its radius or plane tolerance"
                        ),
                    );
                }
            }
        }
    }
    fn dimension<T>(&mut self, value: &Dimension<T>, kind: &str) {
        let (Dimension::Parameter(id) | Dimension::Negated(id)) = value else {
            return;
        };
        let Some(parameter) = self
            .ids
            .parameters(id.as_str(), crate::index::StandardIndex)
        else {
            error(
                self.findings,
                self.owner,
                crate::rule!(
                    "topology.datum-axis-dimension-target-must-name",
                    "datum-axis dimension target must name a design parameter"
                ),
            );
            return;
        };
        if parameter.product_definition != self.owner.product_definition {
            error(
                self.findings,
                self.owner,
                crate::rule!(
                    "topology.datum-axis-dimension-must-bind-parameter",
                    "datum-axis dimension must bind a parameter of its own definition scope"
                ),
            );
        }
        if kind == "text" && matches!(value, Dimension::Negated(_)) {
            error(
                self.findings,
                self.owner,
                crate::rule!(
                    "topology.text-dimensions-cannot-negate-parameter",
                    "text dimensions cannot negate a parameter"
                ),
            );
        }
        if let Some(owner) = parameter.owner.as_ref().filter(|id| *id != &self.owner.id) {
            if self
                .ids
                .features(owner.as_str(), crate::index::StandardIndex)
                .is_some_and(|feature| {
                    feature.ordinal >= self.owner.ordinal
                        || !self.owner.dependencies.iter().any(|id| id == owner)
                })
            {
                error(self.findings,self.owner,crate::rule!("topology.datum-axis-parameter-owner-must-precede", "datum-axis parameter owner must precede its consumer and occur in its dependencies"));
            }
        }
        if let Some(value) = &parameter.value {
            let actual = match value {
                ParameterValue::Length(_) => "length",
                ParameterValue::Angle(_) => "angle",
                ParameterValue::Real(_) => "real",
                ParameterValue::Integer(_) => "integer",
                ParameterValue::Boolean(_) => "boolean",
                ParameterValue::String(_) => "text",
            };
            if actual != kind {
                error(
                    self.findings,
                    self.owner,
                    crate::rule!(
                        "topology.datum-axis-dimension-binds-parameter-incompatible",
                        "datum-axis dimension binds a parameter with an incompatible value kind"
                    ),
                );
            }
        }
    }
    fn options(&mut self, options: &NativeOptions) -> Result<(), CodecError> {
        for value in options.values() {
            match value {
                NativeOption::Bool(v) => self.dimension(v, "boolean"),
                NativeOption::Integer(v) => self.dimension(v, "integer"),
                NativeOption::Real(v) => self.dimension(v, "real"),
                NativeOption::Length(v) => self.dimension(v, "length"),
                NativeOption::Angle(v) => self.dimension(v, "angle"),
                NativeOption::Text(v) => self.dimension(v, "text"),
                NativeOption::Selection(v) => self.selection(v)?,
                NativeOption::OrderedSelection(v) => self.selection(v)?,
            }
        }
        Ok(())
    }
    fn inputs(&mut self, inputs: &[ConstructionInput], kind: Kind) -> Result<(), CodecError> {
        let _depth = self
            .ctx
            .enter_nested("datum-axis construction validation")?;
        for input in inputs {
            if let Some(role) = &input.role {
                if !kind.admits(role.subject) {
                    error(
                        self.findings,
                        self.owner,
                        crate::rule!(
                            "topology.construction-subject-not-part-result-kind",
                            "construction subject is not a part of its result kind"
                        ),
                    );
                }
                if matches!(input.reference.as_ref(),Some(InputReference::Built(value)) if matches!(value.as_ref(),AnyConstruction::Point(_)))
                    && matches!(
                        role.relation,
                        Relation::Parallel
                            | Relation::Perpendicular
                            | Relation::Angle
                            | Relation::PrincipalNormal
                            | Relation::Binormal
                            | Relation::AxisOf
                    )
                {
                    error(
                        self.findings,
                        self.owner,
                        crate::rule!(
                            "topology.point-operand-cannot-supply-construction-direction",
                            "a point operand cannot supply a construction direction"
                        ),
                    );
                }
                self.role(input, role, kind);
            }
            if let Some(value) = &input.value {
                match value {
                    ScalarDimension::Length(v) => self.dimension(v, "length"),
                    ScalarDimension::Angle(v) => self.dimension(v, "angle"),
                    ScalarDimension::Ratio(v) | ScalarDimension::ParameterRatio(v) => {
                        self.dimension(v, "real");
                    }
                    ScalarDimension::Uv { u, v } => {
                        self.dimension(u, "real");
                        self.dimension(v, "real");
                    }
                    ScalarDimension::Position { x, y, z } => {
                        self.dimension(x, "length");
                        self.dimension(y, "length");
                        self.dimension(z, "length");
                    }
                    ScalarDimension::Components { x, y, z } => {
                        self.dimension(x, "real");
                        self.dimension(y, "real");
                        self.dimension(z, "real");
                    }
                    ScalarDimension::PlaneEquation { a, b, c, d } => {
                        if [a, b, c]
                            .iter()
                            .all(|v| matches!(v, Dimension::Literal(v) if v.get() == 0.0))
                        {
                            error(
                                self.findings,
                                self.owner,
                                crate::rule!(
                                    "topology.plane-equation-normal-cannot-zero",
                                    "plane equation normal cannot be zero"
                                ),
                            );
                        }
                        self.dimension(a, "real");
                        self.dimension(b, "real");
                        self.dimension(c, "real");
                        self.dimension(d, "length");
                    }
                }
            }
            if let Some(reference) = &input.reference {
                match reference {
                    InputReference::Picks(value) => self.selection(value)?,
                    InputReference::Built(value) => match value.as_ref() {
                        AnyConstruction::Point(v) => self.construction(v, Kind::Point)?,
                        AnyConstruction::Direction(v) => self.construction(v, Kind::Direction)?,
                        AnyConstruction::Axis(v) => self.construction(v, Kind::Axis)?,
                        AnyConstruction::Plane(v) => {
                            self.construction(v, Kind::Plane)?;
                            for value in v.resolved.iter().chain(v.resolved_in.values()) {
                                if crate::features::geometry::FeatureUnitPlaneFrame::from_parts(
                                    value.origin,
                                    value.normal,
                                    value.x_axis,
                                )
                                .is_none()
                                {
                                    error(
                                        self.findings,
                                        self.owner,
                                        crate::rule!(
                                            "topology.construction-plane-axes-must-perpendicular",
                                            "construction plane axes must be perpendicular"
                                        ),
                                    );
                                }
                            }
                        }
                        AnyConstruction::Frame(v) => {
                            self.construction(v, Kind::Frame)?;
                            for value in v.resolved.iter().chain(v.resolved_in.values()) {
                                self.frame(value);
                            }
                        }
                        AnyConstruction::Curve(v) => {
                            self.construction(v, Kind::Curve)?;
                            for value in v.resolved.iter().chain(v.resolved_in.values()) {
                                self.curve(value);
                            }
                        }
                    },
                }
            }
        }
        Ok(())
    }
    fn construction<K: ConstructionKind>(
        &mut self,
        value: &Construction<K>,
        kind: Kind,
    ) -> Result<(), CodecError> {
        if value.inputs.is_empty() && value.after.is_empty() {
            if value.resolved.is_none() {
                error(
                    self.findings,
                    self.owner,
                    crate::rule!(
                        "topology.construction-no-input-step-resolved-value",
                        "construction has no input, step or resolved value"
                    ),
                );
            }
            if !value.resolved_in.is_empty() {
                error(
                    self.findings,
                    self.owner,
                    crate::rule!(
                        "topology.fixed-construction-cannot-carry-resolved-values",
                        "a fixed construction cannot carry resolved_in values"
                    ),
                );
            }
        }
        if value.solution == Some(0) {
            error(
                self.findings,
                self.owner,
                crate::rule!(
                    "topology.construction-solution-must-positive",
                    "construction solution must be positive"
                ),
            );
        }
        self.inputs(&value.inputs, kind)?;
        for step in &value.after {
            match step {
                PostStep::Flip(v) => {
                    self.dimension(v, "boolean");
                    if matches!(v, Dimension::Literal(false)) {
                        error(
                            self.findings,
                            self.owner,
                            crate::rule!(
                                "topology.literal-false-flip-redundant",
                                "a literal false Flip is redundant"
                            ),
                        );
                    }
                }
                PostStep::Translate { distance, .. } => self.dimension(distance, "length"),
                PostStep::Rotate { angle, .. } => self.dimension(angle, "angle"),
                PostStep::AxisRotate { axis, angle } => {
                    if axis.iter().all(|v| matches!(v, Dimension::Literal(_))) {
                        error(
                            self.findings,
                            self.owner,
                            crate::rule!(
                                "topology.axisrotate-must-bind-axis-component",
                                "AxisRotate must bind an axis component"
                            ),
                        );
                    }
                    for value in axis {
                        self.dimension(value, "real");
                    }
                    self.dimension(angle, "angle");
                }
                PostStep::Fixed(_) => {}
            }
        }
        if let Some(mode) = &value.mode {
            self.options(&mode.native_options)?;
            self.options(&mode.source_settings)?;
        }
        Ok(())
    }
    fn selection<O: MemberOrder>(&mut self, value: &Selection<O>) -> Result<(), CodecError> {
        match &value.picks {
            Picks::Unread => {}
            Picks::Named(values) => self.members(values)?,
            Picks::Rule {
                terms,
                excluded,
                resolved,
            } => {
                if matches!(
                    terms.as_slice(),
                    [RuleTerm {
                        rule: SelectionRule::Explicit,
                        propagation: None,
                        ..
                    }]
                ) && excluded.is_empty()
                {
                    error(
                        self.findings,
                        self.owner,
                        crate::rule!(
                            "topology.single-explicit-selection-term-must-use",
                            "a single explicit selection term must use Named picks"
                        ),
                    );
                }
                for term in terms.iter().chain(excluded) {
                    if matches!(
                        term.rule,
                        SelectionRule::All { .. } | SelectionRule::Affected
                    ) != term.seeds.as_slice().is_empty()
                    {
                        error(
                            self.findings,
                            self.owner,
                            crate::rule!(
                                "topology.selection-rule-incompatible-seed-count",
                                "selection rule has an incompatible seed count"
                            ),
                        );
                    }
                    self.members(&term.seeds)?;
                    match &term.rule {
                        SelectionRule::Chain { end: Some(end) } => self.pick(end)?,
                        SelectionRule::Region { boundary } => self.members(boundary)?,
                        SelectionRule::Intersection { with } => self.members(with)?,
                        _ => {}
                    }
                    if let Some(Propagation::Connected { angle, gap, .. }) = &term.propagation {
                        if let Some(v) = angle {
                            self.dimension(v, "angle");
                        }
                        if let Some(v) = gap {
                            self.dimension(v, "length");
                        }
                    }
                }
                if let Some(values) = resolved {
                    self.members(values)?;
                }
            }
        }
        Ok(())
    }
    fn members<O: MemberOrder>(&mut self, values: &Members<Picked, O>) -> Result<(), CodecError> {
        for value in values.as_slice() {
            self.pick(value)?;
        }
        Ok(())
    }
    fn pick(&mut self, value: &Picked) -> Result<(), CodecError> {
        let _depth = self.ctx.enter_nested("datum-axis pick validation")?;
        if let Some(witness) = &value.witness {
            if let Some(frame) = &witness.frame {
                self.frame(frame);
            }
            if let Some(body) = &witness.body {
                body.visit_identity_references(self.ctx, &mut |_| Ok(()))?;
                let mut nested = Vec::new();
                super::nested::check(
                    self.ctx,
                    self.ids.ir().tolerances,
                    body.as_ref().clone(),
                    &mut nested,
                )?;
                for mut finding in nested {
                    if let Some(entity) = finding.entity.replace(self.owner.id.as_str().to_owned())
                    {
                        finding.related.insert(0, entity);
                    }
                    finding.message = format!("datum-axis witness topology: {}", finding.message);
                    self.findings.push(finding);
                }
            }
        }
        if let Some(PickDomain::Limits { start, end }) = &value.domain {
            for value in start.iter().chain(end) {
                self.pick(value)?;
            }
        }
        Ok(())
    }
}
