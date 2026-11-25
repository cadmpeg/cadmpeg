//! Relation instance records and scalar roles.

use super::parameter_classes::{
    declaration_end, relation_kind, stated_parameters, token_relation_classes, Statement,
};
use super::scalars::FeatureObjectNames;
use super::{is_class_token, RELATION_INSTANCE_ID_PREFIX, SKETCH_POINT_TOLERANCE};
use crate::archive::compound_opening;
use crate::classification::{native_object_class, NativeClassKind};
use crate::history::classify::HistoryMetadata;
use crate::layout::feature_input_scalar_class_prefix as class_prefix;
use crate::layout::feature_input_shifted_scalar_trailer as shifted_trailer;
use crate::records::operand_tag::NativeOperandTag;
use crate::records::{
    ArchiveParameterClass, FeatureInputClass, FeatureInputLane, FeatureInputOperand,
    FeatureInputOperandKind, FeatureInputRelationFamily, FeatureInputRelationInstance,
    FeatureInputScalar, FeatureInputScalarRole,
};
use cadmpeg_core::decode::View;
use cadmpeg_core::CodecError;
use std::collections::{HashMap, HashSet};

/// The offset interval each feature owns, in start order.
///
/// An interval's end is the next feature's start. The last feature has no
/// next start, so its end is `None`: the interval is open, and every offset at
/// or after its start is inside it.
pub(super) fn feature_intervals(
    histories: &[crate::records::FeatureHistory],
    lane: &FeatureInputLane,
) -> Vec<(u64, Option<u64>, String)> {
    let names = FeatureObjectNames::new(&lane.names);
    let mut starts = Vec::new();
    for history in histories {
        let metadata = HistoryMetadata::new(&history.features);
        for feature in &history.features {
            if metadata.contains(feature) {
                continue;
            }
            if let Some(name) = names.of(feature) {
                starts.push((name.offset, feature.id.clone()));
            }
        }
    }
    starts.sort_unstable_by_key(|(left, _)| *left);
    starts.dedup_by_key(|(offset, _)| *offset);
    let mut intervals = Vec::new();
    let mut starts = starts.into_iter().peekable();
    while let Some((start, feature)) = starts.next() {
        intervals.push((start, starts.peek().map(|(next, _)| *next), feature));
    }
    intervals
}

/// The interval that contains `offset`, if one does.
///
/// `intervals` is the start-ordered partition [`feature_intervals`] builds, so
/// the only interval that can contain `offset` is the last one starting at or
/// before it.
fn feature_at_offset(
    offset: u64,
    intervals: &[(u64, Option<u64>, String)],
) -> Option<&(u64, Option<u64>, String)> {
    let after = intervals.partition_point(|(start, _, _)| *start <= offset);
    intervals
        .get(after.checked_sub(1)?)
        .filter(|(_, end, _)| end.is_none_or(|end| offset < end))
}

/// The feature that owns `offset`, if one does.
fn feature_name_at_offset(offset: u64, intervals: &[(u64, Option<u64>, String)]) -> Option<&str> {
    feature_at_offset(offset, intervals).map(|(_, _, feature)| feature.as_str())
}

/// Bytes a class with no feature interval may carry its relation over.
const UNKNOWN_FEATURE_SPAN: u64 = 128;

/// Where a relation's scope ends.
///
/// The three states are separate because neither of the last two is an offset.
/// The last feature interval is open, so a class inside it that no later
/// relation class of the same feature follows states a scope no offset bounds,
/// and every scalar after the class is in it. A class outside every interval
/// states no scope at all when no `u64` can name its unknown-feature span,
/// because reading its scalars unbounded would take every scalar after it.
enum RelationScope {
    /// The exclusive offset the scope ends at.
    Ends(u64),
    /// The class sits in the open last feature interval and no later relation
    /// class of that feature bounds it.
    Unbounded,
    /// The class states an offset whose unknown-feature span no `u64` can
    /// name. The class is refused: it declares no relation.
    Unstatable,
}

/// Lookups the declaration search repeats for every class.
struct DeclarationIndex<'a, 'i> {
    intervals: &'i [(u64, Option<u64>, String)],
    /// Ascending offsets of the relation classes each feature owns.
    relation_offsets: HashMap<&'i str, Vec<u64>>,
    /// Scalars in offset order; equal offsets keep slice order.
    scalars: Vec<&'a FeatureInputScalar>,
}

impl<'a, 'i> DeclarationIndex<'a, 'i> {
    fn new(
        classes: &[FeatureInputClass],
        scalars: &'a [FeatureInputScalar],
        intervals: &'i [(u64, Option<u64>, String)],
    ) -> Self {
        let mut relation_offsets = HashMap::<&str, Vec<u64>>::new();
        for class in classes
            .iter()
            .filter(|class| relation_family(&class.name).is_some())
        {
            if let Some(feature) = feature_name_at_offset(class.offset, intervals) {
                relation_offsets
                    .entry(feature)
                    .or_default()
                    .push(class.offset);
            }
        }
        for offsets in relation_offsets.values_mut() {
            offsets.sort_unstable();
        }
        let mut scalars = scalars.iter().collect::<Vec<_>>();
        scalars.sort_by_key(|scalar| scalar.offset);
        Self {
            intervals,
            relation_offsets,
            scalars,
        }
    }
}

/// Where the relation's scope ends.
fn relation_scope_end(
    class: &FeatureInputClass,
    index: &DeclarationIndex<'_, '_>,
) -> RelationScope {
    let class_interval = feature_at_offset(class.offset, index.intervals);
    let class_feature = class_interval.map(|(_, _, feature)| feature.as_str());
    let next_class = class_feature
        .and_then(|feature| index.relation_offsets.get(feature))
        .and_then(|offsets| {
            offsets
                .get(offsets.partition_point(|offset| *offset <= class.offset))
                .copied()
        });
    // The interval the class sits in states its own end; a class in no
    // interval carries its relation over the unknown-feature span instead.
    let (feature_end, unknown_feature_limit) = match class_interval {
        Some((_, end, _)) => (*end, None),
        None => {
            let Some(limit) = class.offset.checked_add(UNKNOWN_FEATURE_SPAN) else {
                return RelationScope::Unstatable;
            };
            (None, Some(limit))
        }
    };
    match [next_class, feature_end, unknown_feature_limit]
        .into_iter()
        .flatten()
        .min()
    {
        Some(end) => RelationScope::Ends(end),
        None => RelationScope::Unbounded,
    }
}

#[cfg(test)]
fn relation_declaration_candidates<'a>(
    classes: &'a [FeatureInputClass],
    scalars: &'a [FeatureInputScalar],
    intervals: &[(u64, Option<u64>, String)],
) -> Vec<(
    &'a FeatureInputClass,
    &'a FeatureInputScalar,
    FeatureInputRelationFamily,
)> {
    relation_declaration_candidates_impl(classes, scalars, intervals, false)
}

fn relation_declaration_candidates_impl<'a>(
    classes: &'a [FeatureInputClass],
    scalars: &'a [FeatureInputScalar],
    intervals: &[(u64, Option<u64>, String)],
    allow_dynamic: bool,
) -> Vec<(
    &'a FeatureInputClass,
    &'a FeatureInputScalar,
    FeatureInputRelationFamily,
)> {
    let index = DeclarationIndex::new(classes, scalars, intervals);
    classes
        .iter()
        .filter_map(|class| relation_declaration_candidate(class, &index, allow_dynamic))
        .collect()
}

/// The nearest scalar after `class`, inside its scope, that states the class's
/// relation.
fn relation_declaration_candidate<'a>(
    class: &'a FeatureInputClass,
    index: &DeclarationIndex<'a, '_>,
    allow_dynamic: bool,
) -> Option<(
    &'a FeatureInputClass,
    &'a FeatureInputScalar,
    FeatureInputRelationFamily,
)> {
    let family = relation_family(&class.name)?;
    let class_feature = feature_name_at_offset(class.offset, index.intervals);
    let scope_end = match relation_scope_end(class, index) {
        RelationScope::Ends(end) => Some(end),
        RelationScope::Unbounded => None,
        RelationScope::Unstatable => return None,
    };
    let first_after = index
        .scalars
        .partition_point(|scalar| scalar.offset <= class.offset);
    let scalar = index
        .scalars
        .iter()
        .skip(first_after)
        .take_while(|scalar| scope_end.is_none_or(|end| scalar.offset < end))
        .find(|scalar| {
            class_feature.is_none_or(|feature| scalar.feature_ref.as_deref() == Some(feature))
                && if allow_dynamic {
                    relation_signature_for_declaration(family, scalar)
                } else {
                    relation_signature(family, &scalar.operands)
                }
        })
        .copied()?;
    Some((class, scalar, family))
}

pub(super) fn unique_relation_declaration_candidates<'a>(
    classes: &'a [FeatureInputClass],
    scalars: &'a [FeatureInputScalar],
    intervals: &[(u64, Option<u64>, String)],
) -> Vec<(
    &'a FeatureInputClass,
    &'a FeatureInputScalar,
    FeatureInputRelationFamily,
)> {
    let candidates = relation_declaration_candidates_impl(classes, scalars, intervals, false);
    let mut counts = HashMap::<&str, usize>::new();
    for (_, scalar, _) in &candidates {
        *counts.entry(scalar.id.as_str()).or_default() += 1;
    }
    candidates
        .into_iter()
        .filter(|candidate| counts.get(candidate.1.id.as_str()) == Some(&1))
        .collect()
}

/// The relation-class declarations that claim the lane's dimensions.
struct DimensionClaims<'l> {
    /// The parameter each scalar's record states.
    stated: HashMap<&'l str, Statement>,
    /// Each declaration with the dimension it claims and its family.
    candidates: Vec<(
        &'l FeatureInputClass,
        &'l FeatureInputScalar,
        FeatureInputRelationFamily,
    )>,
    /// The number of declarations that claim each dimension.
    counts: HashMap<&'l str, usize>,
    /// The dimensions that no declaration claims, in stream order: the
    /// nearest declaration of each states another quantity than its record.
    unclaimed: Vec<&'l FeatureInputScalar>,
}

impl<'l> DimensionClaims<'l> {
    fn of(lane: &'l FeatureInputLane, intervals: &[(u64, Option<u64>, String)]) -> Self {
        // A dimension whose own record states a parameter of another
        // quantity than the declared relation's (§2) is no dimension of that
        // relation.
        let stated = stated_parameters(lane, None);
        let mut candidates =
            relation_declaration_candidates_impl(&lane.classes, &lane.scalars, intervals, true);
        let mut refused = Vec::new();
        candidates.retain(|(class, scalar, _)| {
            let claims = match (relation_kind(&class.name), stated.get(scalar.id.as_str())) {
                (Some(kind), Some(Statement::Parameter(parameter))) => parameter.class() == kind,
                _ => true,
            };
            if !claims {
                refused.push(*scalar);
            }
            claims
        });
        let mut counts = HashMap::<&str, usize>::new();
        for (_, scalar, _) in &candidates {
            *counts.entry(scalar.id.as_str()).or_default() += 1;
        }
        let mut seen = HashSet::new();
        refused.retain(|scalar| {
            !counts.contains_key(scalar.id.as_str()) && seen.insert(scalar.id.as_str())
        });
        refused.sort_by_key(|scalar| scalar.offset);
        Self {
            stated,
            candidates,
            counts,
            unclaimed: refused,
        }
    }
}

/// Whether a point-distance relation's bound operands are two sketch points
/// whose distance, along the family's axis, is the relation's value.
fn relation_measures_its_value(
    relation: &FeatureInputRelationInstance,
    index: &RelationOperandIndex<'_>,
) -> bool {
    let horizontal = match relation.family {
        FeatureInputRelationFamily::PointPointDistance => None,
        FeatureInputRelationFamily::PointPointHorizontalDistance => Some(true),
        FeatureInputRelationFamily::PointPointVerticalDistance => Some(false),
        _ => return false,
    };
    let Some(target) = index.target_value(relation) else {
        return false;
    };
    let entities = index.feature_entities(relation.feature_ref.as_str());
    let point = |operand: &FeatureInputOperand| {
        let entity_ref = operand.entity_ref.as_deref()?;
        entities
            .iter()
            .find(|entity| entity.id() == entity_ref && is_finite_point(entity))?
            .coordinates_m
            .map(cadmpeg_ir::units::FiniteVector::get)
    };
    let [first, second] = relation.operands.as_slice() else {
        return false;
    };
    let (Some(first), Some(second)) = (point(first), point(second)) else {
        return false;
    };
    let measured = horizontal.map_or_else(
        || point_distance(first, second),
        |horizontal| axis_distance(first, second, horizontal),
    );
    same_relation_dimension(measured, target.get())
}

/// One loss note naming the sketch dimensions of a compound-file lane that
/// form no relation instance because no declaration claims them and their
/// relation class is unknown (open item EV-05).
pub(crate) fn unclassed_dimension_note(
    histories: &[crate::records::FeatureHistory],
    lane: &FeatureInputLane,
) -> Option<cadmpeg_ir::report::loss::LossNote> {
    if !compound_opening(&lane.native_payload) {
        return None;
    }
    let intervals = feature_intervals(histories, lane);
    let claims = DimensionClaims::of(lane, &intervals);
    if claims.unclaimed.is_empty() {
        return None;
    }
    let sketch_features = sketch_features(histories);
    let formed = lane
        .relation_instances
        .iter()
        .flat_map(FeatureInputRelationInstance::scalar_refs)
        .map(String::as_str)
        .collect::<HashSet<_>>();
    let unclassed = claims
        .unclaimed
        .iter()
        .filter(|scalar| {
            scalar
                .feature_ref
                .as_deref()
                .is_some_and(|feature| sketch_features.contains(feature))
                && !formed.contains(scalar.id.as_str())
        })
        .map(|scalar| scalar.id.as_str())
        .collect::<Vec<_>>();
    if unclassed.is_empty() {
        return None;
    }
    Some(
        crate::loss::SldprtLossCode::SketchDimensionClassUnknown.note(format!(
            "sketch dimension scalar(s) {} of a compound-file lane form no relation instance: \
             the nearest relation-class declaration of each states another quantity than its \
             record, and its relation class is unknown (open item EV-05). The scalar records \
             are kept as native records.",
            crate::loss::named(&unclassed)
        )),
    )
}

fn sketch_features(histories: &[crate::records::FeatureHistory]) -> HashSet<&str> {
    histories
        .iter()
        .flat_map(|history| &history.features)
        .filter(|feature| feature.xml_tag.eq_ignore_ascii_case("Sketch"))
        .map(|feature| feature.id.as_str())
        .collect()
}

struct RelationGroup<'a> {
    feature_ref: &'a str,
    family: FeatureInputRelationFamily,
    class_ref: &'a str,
    /// Whether `class_ref` comes only from relation-class tokens of a
    /// compound-file lane (open item EV-05), so that the group forms only
    /// where its sketch points measure its value.
    gated: bool,
    operands: &'a [FeatureInputOperand],
    scalars: Vec<(usize, &'a FeatureInputScalar)>,
    /// Present while every scalar is a one-operand display scalar of one
    /// operand kind and object name.
    circle_displays: Option<CircleDisplays<'a>>,
}

impl<'a> RelationGroup<'a> {
    fn push(&mut self, index: usize, scalar: &'a FeatureInputScalar, name: Option<&'a str>) {
        self.scalars.push((index, scalar));
        if let Some(displays) = &mut self.circle_displays {
            match display_operand(scalar) {
                Some(operand) if operand.kind == displays.kind && name == Some(displays.name) => {
                    displays.entity_indices.insert(operand.entity_index);
                }
                _ => self.circle_displays = None,
            }
        }
    }
}

/// The operand kind, object name and entity indices a group's display scalars
/// share.
struct CircleDisplays<'a> {
    kind: FeatureInputOperandKind,
    name: &'a str,
    entity_indices: HashSet<u16>,
}

impl<'a> CircleDisplays<'a> {
    fn of(scalar: &FeatureInputScalar, name: Option<&'a str>) -> Option<Self> {
        let operand = display_operand(scalar)?;
        Some(Self {
            kind: operand.kind,
            name: name?,
            entity_indices: HashSet::from([operand.entity_index]),
        })
    }

    /// Whether `scalar`, named `name`, repeats the shared display on another
    /// entity.
    fn repeated_by(&self, scalar: &FeatureInputScalar, name: Option<&str>) -> bool {
        display_operand(scalar).is_some_and(|operand| {
            operand.kind == self.kind
                && name == Some(self.name)
                && !self.entity_indices.contains(&operand.entity_index)
        })
    }
}

fn display_operand(scalar: &FeatureInputScalar) -> Option<&FeatureInputOperand> {
    match scalar.operands.as_slice() {
        [operand] if scalar.role == FeatureInputScalarRole::Display => Some(operand),
        _ => None,
    }
}

/// Whether two scalars hold the same operand cells: one kind and one entity
/// index at each position.
fn same_operand_cells(left: &[FeatureInputOperand], right: &[FeatureInputOperand]) -> bool {
    left.iter()
        .map(|operand| (operand.kind, operand.entity_index))
        .eq(right
            .iter()
            .map(|operand| (operand.kind, operand.entity_index)))
}

/// The relation class of a sketch dimension written in the token form
/// (`docs/formats/sldprt.md` §2), from the class map's index order: a class
/// declaration takes the next map index and the object it opens the index
/// after it. A relation class declared with its parameter class inside its
/// first relation object, after that object's zero u32, is therefore two
/// indices below the parameter class, and a token form whose parameter-class
/// token is two above its relation-class token names that relation class.
struct TokenRelations<'l> {
    lane: &'l FeatureInputLane,
    name_offsets: HashMap<&'l str, u64>,
    /// What each scalar's record states of its parameter, read on first use.
    stated: Option<HashMap<&'l str, Statement>>,
}

impl<'l> TokenRelations<'l> {
    fn new(lane: &'l FeatureInputLane) -> Self {
        let mut name_offsets = HashMap::new();
        for name in &lane.names {
            name_offsets.entry(name.id.as_str()).or_insert(name.offset);
        }
        Self {
            lane,
            name_offsets,
            stated: None,
        }
    }

    /// The relation-class and parameter-class token indices of the token
    /// form before `scalar`'s name record.
    fn tokens(&self, scalar: &FeatureInputScalar) -> Option<(u16, u16)> {
        let payload = self.lane.native_payload.as_slice();
        let name = usize::try_from(*self.name_offsets.get(scalar.name.as_str())?).ok()?;
        let start = name.checked_sub(class_prefix::LEN)?;
        let relation = View::u16_le_at(
            payload,
            start.checked_add(class_prefix::RELATION_CLASS_TOKEN)?,
        )?;
        let zero = View::u32_le_at(payload, start.checked_add(class_prefix::ZERO)?)?;
        let parameter = View::u16_le_at(
            payload,
            start.checked_add(class_prefix::PARAMETER_CLASS_TOKEN)?,
        )?;
        (is_class_token(relation) && zero == 0 && is_class_token(parameter))
            .then_some((relation & 0x7fff, parameter & 0x7fff))
    }

    /// The family and class of the relation instance that the display
    /// scalar `display` and the driving scalar `driving` after it form: both
    /// in one feature with the same operand cells and one token form whose
    /// parameter-class token is two above its relation-class token, where
    /// that token names the parameter class the lane declares once and a
    /// relation class is declared directly before that declaration, followed
    /// by four zero bytes.
    fn relation(
        &mut self,
        display: &FeatureInputScalar,
        driving: &FeatureInputScalar,
    ) -> Option<(FeatureInputRelationFamily, &'l str)> {
        if display.role != FeatureInputScalarRole::Display
            || driving.role != FeatureInputScalarRole::Driving
            || display.feature_ref != driving.feature_ref
            || display.operands.is_empty()
            || !same_operand_cells(&display.operands, &driving.operands)
        {
            return None;
        }
        let (relation_token, parameter_token) = self.tokens(display)?;
        if self.tokens(driving)? != (relation_token, parameter_token)
            || relation_token.checked_add(2)? != parameter_token
        {
            return None;
        }
        let lane = self.lane;
        let class = match self
            .stated
            .get_or_insert_with(|| stated_parameters(lane, None))
            .get(display.id.as_str())?
        {
            Statement::Parameter(stated) => stated.class(),
            Statement::Assumed(_) | Statement::Contested => return None,
        };
        let mut declared = lane.classes.iter().filter(|declared| {
            ArchiveParameterClass::from_class_name(&declared.name) == Some(class)
        });
        let parameter = declared.next()?;
        if declared.next().is_some() {
            return None;
        }
        let payload = lane.native_payload.as_slice();
        let relation = lane.classes.iter().find(|relation| {
            declaration_end(relation).is_some_and(|end| {
                end.checked_add(RELATION_OBJECT_ZERO) == Some(parameter.offset)
                    && usize::try_from(end)
                        .ok()
                        .and_then(|end| View::u32_le_at(payload, end))
                        == Some(0)
            })
        })?;
        let family = relation_family(&relation.name)?;
        (relation.offset < display.offset && relation_signature_for_declaration(family, display))
            .then_some((family, relation.id.as_str()))
    }
}

/// Whether the display scalar at `index` of `scalars`, in `feature`, is the
/// first scalar after the one scalar of `group` that a display or driving
/// role frames, where that scalar, the one its relation-class declaration
/// opens, holds the native role.
fn first_framed_after_native(
    group: &RelationGroup<'_>,
    feature: &str,
    scalars: &[FeatureInputScalar],
    index: usize,
) -> bool {
    let [(declared, scalar)] = group.scalars.as_slice() else {
        return false;
    };
    group.feature_ref == feature
        && scalar.role == FeatureInputScalarRole::Native
        && declared
            .checked_add(1)
            .and_then(|start| scalars.get(start..index))
            .is_some_and(|between| {
                between
                    .iter()
                    .all(|scalar| scalar.role == FeatureInputScalarRole::Native)
            })
}

/// The zero u32 that opens a relation object, before its parameter class.
const RELATION_OBJECT_ZERO: u64 = 4;

pub(super) fn relation_instances(
    histories: &[crate::records::FeatureHistory],
    lane: &FeatureInputLane,
) -> Result<Vec<FeatureInputRelationInstance>, CodecError> {
    let sketch_features = sketch_features(histories);
    let intervals = feature_intervals(histories, lane);
    let claims = DimensionClaims::of(lane, &intervals);
    let mut declarations = HashMap::new();
    for &(class, scalar, family) in &claims.candidates {
        if claims.counts.get(scalar.id.as_str()) == Some(&1) {
            declarations.insert(
                scalar.id.as_str(),
                (class.offset, family, class.id.as_str(), false),
            );
        }
    }
    let candidate_counts = claims.counts;
    // An unclaimed dimension has the relation class its own relation-class
    // token names, of the quantity its record states (§2). A compound-file
    // lane does not state which tokens are its own (open item EV-05); there
    // the class holds only where the sketch points its relation binds
    // measure the relation's value, and the lane is grouped again without
    // each declaration whose relation they do not measure.
    let compound = compound_opening(&lane.native_payload);
    let token_classes = if claims.unclaimed.is_empty() {
        HashMap::new()
    } else {
        token_relation_classes(lane, &sketch_features)
    };
    for scalar in claims.unclaimed {
        let Some(class) = token_classes.get(scalar.id.as_str()) else {
            continue;
        };
        let Some(family) = relation_family(&class.name) else {
            continue;
        };
        let stated_kind = match claims.stated.get(scalar.id.as_str()) {
            Some(Statement::Parameter(parameter)) => Some(parameter.class()),
            _ => None,
        };
        if stated_kind.is_none()
            || relation_kind(&class.name) != stated_kind
            || !relation_signature_for_declaration(family, scalar)
        {
            continue;
        }
        declarations.insert(
            scalar.id.as_str(),
            (class.offset, family, class.id.as_str(), compound),
        );
    }
    let index = RelationOperandIndex::new(lane);
    loop {
        let (mut instances, gated) = form_relations(
            lane,
            &sketch_features,
            &declarations,
            &candidate_counts,
            &index,
        )?;
        let unmeasured = instances
            .iter()
            .filter(|relation| {
                gated.contains(&relation.id) && !relation_measures_its_value(relation, &index)
            })
            .map(|relation| relation.id.clone())
            .collect::<HashSet<_>>();
        if unmeasured.is_empty() {
            return Ok(instances);
        }
        let withdrawn = instances
            .iter()
            .filter(|relation| unmeasured.contains(&relation.id))
            .flat_map(FeatureInputRelationInstance::scalar_refs)
            .filter(|id| {
                declarations
                    .get(id.as_str())
                    .is_some_and(|declaration| declaration.3)
            })
            .cloned()
            .collect::<Vec<_>>();
        if withdrawn.is_empty() {
            instances.retain(|relation| !unmeasured.contains(&relation.id));
            for (ordinal, relation) in instances.iter_mut().enumerate() {
                relation.ordinal = u32::try_from(ordinal).map_err(|_| {
                    CodecError::malformed("SLDPRT relation instance ordinal overflows u32")
                })?;
            }
            return Ok(instances);
        }
        for id in &withdrawn {
            declarations.remove(id.as_str());
        }
    }
}

/// A relation-class declaration: the class's offset, family and identity,
/// and whether the class comes only from a compound-file lane's
/// relation-class token (open item EV-05).
type Declaration<'l> = (u64, FeatureInputRelationFamily, &'l str, bool);

/// The lane's relation instances under `declarations`, with their operands
/// bound, and the identities of the instances whose class comes only from a
/// compound-file lane's relation-class tokens.
fn form_relations<'l>(
    lane: &'l FeatureInputLane,
    sketch_features: &HashSet<&str>,
    declarations: &HashMap<&'l str, Declaration<'l>>,
    candidate_counts: &HashMap<&'l str, usize>,
    index: &RelationOperandIndex<'_>,
) -> Result<(Vec<FeatureInputRelationInstance>, HashSet<String>), CodecError> {
    let mut relation_class_offsets = lane
        .classes
        .iter()
        .filter(|class| relation_family(&class.name).is_some())
        .map(|class| class.offset)
        .collect::<Vec<_>>();
    relation_class_offsets.sort_unstable();
    // Name lookup keeps the first directory record at a repeated identity.
    let mut names_by_id = HashMap::<&str, &str>::new();
    for name in &lane.names {
        names_by_id
            .entry(name.id.as_str())
            .or_insert(name.value.as_str());
    }
    let mut token_relations = TokenRelations::new(lane);
    let mut groups = Vec::<RelationGroup<'_>>::new();
    for (scalar_index, scalar) in lane.scalars.iter().enumerate() {
        let Some(feature_ref) = scalar
            .feature_ref
            .as_deref()
            .filter(|feature| sketch_features.contains(feature))
        else {
            continue;
        };
        let scalar_name = names_by_id.get(scalar.name.as_str()).copied();
        let declaration = declarations.get(scalar.id.as_str());
        let Some((class_offset, family, class_ref, gated)) = declaration else {
            if candidate_counts
                .get(scalar.id.as_str())
                .is_some_and(|count| *count > 1)
            {
                continue;
            }
            let declared_class = groups
                .last()
                .filter(|group| {
                    first_framed_after_native(group, feature_ref, &lane.scalars, scalar_index)
                })
                .map(|group| group.class_ref);
            let token_pair = declared_class
                .and(lane.scalars.get(scalar_index + 1))
                .filter(|driving| {
                    !candidate_counts.contains_key(driving.id.as_str())
                        && relation_class_offsets
                            .get(
                                relation_class_offsets
                                    .partition_point(|offset| *offset <= scalar.offset),
                            )
                            .is_none_or(|offset| *offset >= driving.offset)
                })
                .and_then(|driving| token_relations.relation(scalar, driving))
                .filter(|&(_, class_ref)| Some(class_ref) == declared_class);
            if let Some((family, class_ref)) = token_pair {
                groups.push(RelationGroup {
                    feature_ref,
                    family,
                    class_ref,
                    gated: false,
                    operands: &scalar.operands,
                    scalars: vec![(scalar_index, scalar)],
                    circle_displays: CircleDisplays::of(scalar, scalar_name),
                });
                continue;
            }
            let Some(group) = groups.last_mut() else {
                continue;
            };
            let Some((last_index, last_scalar)) = group.scalars.last() else {
                continue;
            };
            // Run the class interval search and operand comparisons only
            // when this scalar can extend an existing group.
            let same_scope = group.feature_ref == feature_ref
                && *last_index + 1 == scalar_index
                && relation_class_offsets
                    .get(
                        relation_class_offsets
                            .partition_point(|offset| *offset <= last_scalar.offset),
                    )
                    .is_none_or(|offset| *offset >= scalar.offset);
            let same_operands = same_operand_cells(group.operands, &scalar.operands);
            let repeated_circle_display = same_scope
                && group.family == FeatureInputRelationFamily::CircleDiameter
                && group
                    .circle_displays
                    .as_ref()
                    .is_some_and(|displays| displays.repeated_by(scalar, scalar_name));
            if repeated_circle_display {
                group.push(scalar_index, scalar, scalar_name);
            } else if same_scope && same_operands && scalar.role == FeatureInputScalarRole::Driving
            {
                if group.scalars.len() == 1 {
                    group.push(scalar_index, scalar, scalar_name);
                } else {
                    let next = RelationGroup {
                        feature_ref: group.feature_ref,
                        family: group.family,
                        class_ref: group.class_ref,
                        gated: group.gated,
                        operands: &scalar.operands,
                        scalars: vec![(scalar_index, scalar)],
                        circle_displays: CircleDisplays::of(scalar, scalar_name),
                    };
                    groups.push(next);
                }
            }
            continue;
        };
        // A display scalar can precede the declaration selected by its adjacent
        // driving scalar. The driving scalar's declaration is authoritative for
        // that pair; display-only scalars still stop at a class declaration.
        let promote_class = groups.last().is_some_and(|group| {
            group.class_ref != *class_ref
                && group.scalars.len() == 1
                && group.scalars[0].1.role == FeatureInputScalarRole::Display
                && scalar.role == FeatureInputScalarRole::Driving
                && *class_offset > group.scalars[0].1.offset
                && *class_offset < scalar.offset
        });
        let append = groups.last_mut().filter(|group| {
            group.feature_ref == feature_ref
                && (group.family == *family || promote_class)
                && (group.class_ref == *class_ref || promote_class)
                && matches!(group.scalars.as_slice(), [(index, _)] if *index + 1 == scalar_index)
                && same_operand_cells(group.operands, &scalar.operands)
        });
        if let Some(group) = append {
            if promote_class {
                group.family = *family;
                group.class_ref = *class_ref;
                group.gated = *gated;
            } else {
                group.gated &= *gated;
            }
            group.push(scalar_index, scalar, scalar_name);
        } else {
            groups.push(RelationGroup {
                feature_ref,
                family: *family,
                class_ref,
                gated: *gated,
                operands: &scalar.operands,
                scalars: vec![(scalar_index, scalar)],
                circle_displays: CircleDisplays::of(scalar, scalar_name),
            });
        }
    }
    let mut instances = Vec::new();
    let mut gated = HashSet::new();
    let mut claimed_scalar_refs = HashSet::new();
    let lane_key = lane
        .id
        .rsplit_once('#')
        .map_or(lane.id.as_str(), |(_, key)| key);
    for (ordinal, group) in groups.into_iter().enumerate() {
        for (_, scalar) in &group.scalars {
            claimed_scalar_refs.insert(scalar.id.as_str());
        }
        let offset = group.scalars[0].1.offset;
        let id = format!("{RELATION_INSTANCE_ID_PREFIX}{lane_key}:{offset}");
        if group.gated {
            gated.insert(id.clone());
        }
        let ordinal = u32::try_from(ordinal)
            .map_err(|_| CodecError::malformed("SLDPRT relation instance ordinal overflows u32"))?;
        instances.push(FeatureInputRelationInstance {
            id,
            parent: lane.id.clone(),
            ordinal,
            offset,
            family: group.family,
            class_ref: group.class_ref.to_owned(),
            feature_ref: group.feature_ref.to_owned(),
            scalars: crate::records::relation_scalars::RelationScalars::from_scalars(
                group.scalars.into_iter().map(|(_, scalar)| scalar),
            )?,
            operands: group.operands.to_vec(),
        });
    }
    // A class can declare more than one scalar relation. The ordinary
    // instance grouper joins a display scalar to its adjacent driving scalar
    // when their operand signatures agree; a second scalar with a different
    // signature is still a declared relation binding and must not disappear.
    // Promote only bindings whose scalar is not already claimed by an
    // instance. A scalar has one relation-instance owner even if malformed
    // input associates it with more than one class. Non-sketch bindings remain
    // native-only, matching the existing relation-instance scope.
    for binding in &lane.relation_bindings {
        let Some(feature_ref) = binding
            .feature_ref
            .as_deref()
            .filter(|feature| sketch_features.contains(*feature))
        else {
            continue;
        };
        let Some(scalar) = lane
            .scalars
            .iter()
            .find(|scalar| scalar.id == binding.scalar_ref)
        else {
            continue;
        };
        if claimed_scalar_refs.contains(binding.scalar_ref.as_str()) {
            continue;
        }
        claimed_scalar_refs.insert(binding.scalar_ref.as_str());
        instances.push(FeatureInputRelationInstance {
            id: format!("{RELATION_INSTANCE_ID_PREFIX}{lane_key}:{}", scalar.offset),
            parent: lane.id.clone(),
            ordinal: u32::try_from(instances.len()).map_err(|_| {
                CodecError::malformed("SLDPRT relation instance ordinal overflows u32")
            })?,
            offset: scalar.offset,
            family: binding.family,
            class_ref: binding.class_ref.clone(),
            feature_ref: feature_ref.to_owned(),
            scalars: crate::records::relation_scalars::RelationScalars::from_scalars([scalar])?,
            operands: scalar.operands.clone(),
        });
    }
    instances.sort_unstable_by(|left, right| {
        (left.offset, left.ordinal).cmp(&(right.offset, right.ordinal))
    });
    for (ordinal, relation) in instances.iter_mut().enumerate() {
        relation.ordinal = u32::try_from(ordinal)
            .map_err(|_| CodecError::malformed("SLDPRT relation instance ordinal overflows u32"))?;
    }
    bind_detached_relation_drivers(&mut instances, lane)?;
    bind_circle_dimension_centers(&mut instances, lane)?;
    for relation in &mut instances {
        bind_relation_geometry(relation, index);
    }
    Ok((instances, gated))
}

pub(super) fn bind_circle_dimension_centers(
    relations: &mut [FeatureInputRelationInstance],
    lane: &FeatureInputLane,
) -> Result<(), CodecError> {
    let mut scalars = HashMap::new();
    for scalar in &lane.scalars {
        scalars.insert(scalar.id.as_str(), scalar);
    }
    let mut names = HashMap::new();
    for name in &lane.names {
        names.insert(name.id.as_str(), name.value.as_str());
    }
    for relation in relations.iter_mut().filter(|relation| {
        relation.family == FeatureInputRelationFamily::CircleDiameter
            && relation.operands.len() == 1
    }) {
        let Some(display) = relation
            .display_scalar_ref()
            .and_then(|id| scalars.get(id).copied())
        else {
            continue;
        };
        let Some(display_name) = names.get(display.name.as_str()) else {
            continue;
        };
        let Some(display_index) = lane
            .scalars
            .iter()
            .position(|scalar| scalar.id == display.id)
        else {
            continue;
        };
        let first = &relation.operands[0];
        let candidates = lane
            .scalars
            .iter()
            .enumerate()
            .filter(|scalar| {
                let scalar = scalar.1;
                scalar.feature_ref == display.feature_ref
                    && names.get(scalar.name.as_str()) == Some(display_name)
                    && matches!(scalar.operands.as_slice(), [candidate, _]
                        if candidate.kind == first.kind
                            && candidate.entity_index == first.entity_index)
            })
            .collect::<Vec<_>>();
        if candidates.first().map(|candidate| candidate.0) != Some(display_index + 1)
            || candidates.windows(2).any(|pair| pair[1].0 != pair[0].0 + 1)
        {
            continue;
        }
        let mut centers = candidates
            .iter()
            .filter_map(|(_, scalar)| scalar.operands.get(1))
            .map(|operand| {
                (
                    operand.kind,
                    operand.entity_index,
                    operand.entity_ref.as_deref(),
                )
            });
        let Some(center) = centers.next() else {
            continue;
        };
        if centers.any(|candidate| candidate != center) {
            continue;
        }
        let Some((_, source)) = candidates.iter().find(|(_, scalar)| {
            scalar.operands.get(1).is_some_and(|operand| {
                (
                    operand.kind,
                    operand.entity_index,
                    operand.entity_ref.as_deref(),
                ) == center
            })
        }) else {
            continue;
        };
        relation.operands = source.operands.clone();
        for (_, scalar) in candidates {
            if !relation.scalar_refs().contains(&scalar.id) {
                relation.scalars.push(&scalar.id)?;
            }
        }
    }
    Ok(())
}

fn same_scalar_operands(left: &FeatureInputScalar, right: &FeatureInputScalar) -> bool {
    left.operands.len() == right.operands.len()
        && left
            .operands
            .iter()
            .zip(&right.operands)
            .all(|(left, right)| {
                left.kind == right.kind
                    && left.entity_index == right.entity_index
                    && left.entity_ref == right.entity_ref
            })
}

/// One lane's circle-dimension handle drivers.
///
/// A circle diameter whose only scalar is a display scalar is driven by the
/// one driving scalar that follows it in a display and driving pair of the
/// same name, feature and operands, after a declared entity handle and before
/// the next relation class.
pub(super) struct CircleHandleDrivers<'a> {
    /// The first scalar with each id, in offset order.
    scalars: HashMap<&'a str, &'a FeatureInputScalar>,
    relation_starts: Vec<u64>,
    handles: Vec<u64>,
    /// Adjacent display and driving scalars in offset order, by their shared
    /// feature and operands.
    pairs: HashMap<HandlePairKey<'a>, Vec<(&'a FeatureInputScalar, &'a FeatureInputScalar)>>,
}

type HandlePairKey<'a> = (
    Option<&'a str>,
    Vec<(FeatureInputOperandKind, u16, Option<&'a str>)>,
);

fn handle_pair_key(scalar: &FeatureInputScalar) -> HandlePairKey<'_> {
    let operands = scalar
        .operands
        .iter()
        .map(|operand| {
            (
                operand.kind,
                operand.entity_index,
                operand.entity_ref.as_deref(),
            )
        })
        .collect();
    (scalar.feature_ref.as_deref(), operands)
}

impl<'a> CircleHandleDrivers<'a> {
    pub(super) fn new(lane: &'a FeatureInputLane) -> Self {
        let mut sorted = lane.scalars.iter().collect::<Vec<_>>();
        sorted.sort_unstable_by_key(|left| left.offset);
        let mut names = HashMap::new();
        for name in &lane.names {
            names.insert(name.id.as_str(), name.value.as_str());
        }
        let mut scalars = HashMap::new();
        for scalar in &sorted {
            scalars.entry(scalar.id.as_str()).or_insert(*scalar);
        }
        let mut pairs = HashMap::<_, Vec<_>>::new();
        for pair in sorted.windows(2) {
            let &[display, driving] = pair else {
                continue;
            };
            let same_name = matches!(
                (
                    names.get(display.name.as_str()),
                    names.get(driving.name.as_str())
                ),
                (Some(left), Some(right)) if left == right
            );
            if display.role == FeatureInputScalarRole::Display
                && driving.role == FeatureInputScalarRole::Driving
                && display.feature_ref == driving.feature_ref
                && same_scalar_operands(display, driving)
                && same_name
            {
                pairs
                    .entry(handle_pair_key(display))
                    .or_default()
                    .push((display, driving));
            }
        }
        let class_offsets = |keep: fn(&FeatureInputClass) -> bool| {
            let mut offsets = lane
                .classes
                .iter()
                .filter(|class| keep(class))
                .map(|class| class.offset)
                .collect::<Vec<_>>();
            offsets.sort_unstable();
            offsets
        };
        Self {
            scalars,
            relation_starts: class_offsets(|class| relation_family(&class.name).is_some()),
            handles: class_offsets(|class| class.name == "sgEntHandle"),
            pairs,
        }
    }

    pub(super) fn driver(
        &self,
        relation: &FeatureInputRelationInstance,
    ) -> Option<&'a FeatureInputScalar> {
        if relation.family != FeatureInputRelationFamily::CircleDiameter
            || relation.parameter_scalar_ref().is_some()
            || relation.scalar_refs().len() != 1
        {
            return None;
        }
        let first = relation
            .scalar_refs()
            .first()
            .and_then(|id| self.scalars.get(id.as_str()))
            .copied()
            .filter(|scalar| scalar.role == FeatureInputScalarRole::Display)?;
        let next_after = |offsets: &[u64]| {
            offsets
                .get(offsets.partition_point(|offset| *offset <= first.offset))
                .copied()
        };
        let handle = next_after(&self.handles)?;
        // No following relation class states no upper bound on this relation.
        let next_relation = next_after(&self.relation_starts);
        let pairs = self.pairs.get(&handle_pair_key(first))?;
        let start = pairs.partition_point(|(display, _)| display.offset <= handle);
        let end = next_relation.map_or(pairs.len(), |limit| {
            pairs.partition_point(|(_, driving)| driving.offset < limit)
        });
        let [(_, driving)] = pairs.get(start..end)? else {
            return None;
        };
        Some(*driving)
    }
}

pub(super) fn bind_detached_relation_drivers(
    relations: &mut [FeatureInputRelationInstance],
    lane: &FeatureInputLane,
) -> Result<(), CodecError> {
    let mut scalars = HashMap::new();
    for scalar in &lane.scalars {
        scalars.insert(scalar.id.as_str(), scalar);
    }
    let mut names = HashMap::new();
    for name in &lane.names {
        names.insert(name.id.as_str(), name.value.as_str());
    }
    let mut claimed = HashSet::new();
    for id in relations
        .iter()
        .flat_map(crate::records::FeatureInputRelationInstance::scalar_refs)
    {
        claimed.insert(id.as_str());
    }
    let mut drivers = HashMap::<(&str, &str), Vec<&FeatureInputScalar>>::new();
    for scalar in lane.scalars.iter().filter(|scalar| {
        scalar.role == FeatureInputScalarRole::Driving
            && scalar.operands.is_empty()
            && !claimed.contains(scalar.id.as_str())
    }) {
        let (Some(feature), Some(name)) = (
            scalar.feature_ref.as_deref(),
            names.get(scalar.name.as_str()).copied(),
        ) else {
            continue;
        };
        let values = drivers.entry((feature, name)).or_default();
        values.push(scalar);
    }
    let mut candidates = HashMap::<(String, String), Vec<usize>>::new();
    for (index, relation) in relations.iter().enumerate() {
        if relation.parameter_scalar_ref().is_some() {
            continue;
        }
        let mut relation_names = relation
            .scalar_refs()
            .iter()
            .filter_map(|id| scalars.get(id.as_str()))
            .filter(|scalar| scalar.role == FeatureInputScalarRole::Display)
            .filter_map(|scalar| names.get(scalar.name.as_str()).copied());
        let Some(name) = relation_names.next() else {
            continue;
        };
        if relation_names.any(|candidate| candidate != name) {
            continue;
        }
        let key = (relation.feature_ref.clone(), name.to_owned());
        let values = candidates.entry(key).or_default();
        values.push(index);
    }
    for (key, relation_indices) in candidates {
        let [relation_index] = relation_indices.as_slice() else {
            continue;
        };
        let Some([driver]) = drivers
            .get(&(key.0.as_str(), key.1.as_str()))
            .map(Vec::as_slice)
        else {
            continue;
        };
        let relation = &mut relations[*relation_index];
        relation.scalars.push_parameter(&driver.id)?;
    }
    Ok(())
}

fn relation_family(name: &str) -> Option<FeatureInputRelationFamily> {
    match native_object_class(name) {
        NativeClassKind::SketchRelation(family) => Some(family),
        _ => None,
    }
}

fn relation_signature(
    family: FeatureInputRelationFamily,
    operands: &[FeatureInputOperand],
) -> bool {
    // `80d5`, `8138`, `80ac`, and `810f` are class-scoped relation cells. Keep
    // them behind the declared family instead of treating them as global
    // marker kinds.
    use FeatureInputOperandKind::{Native, D6, E1};
    use FeatureInputRelationFamily::{
        Angle, CircleDiameter, LineLineDistance, PointLineDistance, PointPointDistance,
        PointPointHorizontalDistance, PointPointVerticalDistance,
    };
    match (family, operands) {
        (CircleDiameter, [operand]) => matches!(operand.kind, Native(_)),
        (PointPointDistance, [first, second]) => {
            (first.kind == D6 && second.kind == D6)
                || (first.kind == Native(NativeOperandTag::TAG_81B2)
                    && second.kind == Native(NativeOperandTag::TAG_81B2))
                || (first.kind == Native(NativeOperandTag::TAG_8152)
                    && second.kind == Native(NativeOperandTag::TAG_8152))
                || (first.kind == Native(NativeOperandTag::TAG_80D5)
                    && second.kind == Native(NativeOperandTag::TAG_80D5))
                || (first.kind == Native(NativeOperandTag::TAG_8138)
                    && second.kind == Native(NativeOperandTag::TAG_8138))
                || (first.kind == Native(NativeOperandTag::TAG_80AC)
                    && second.kind == Native(NativeOperandTag::TAG_80AC))
                || (first.kind == Native(NativeOperandTag::TAG_837B)
                    && second.kind == Native(NativeOperandTag::TAG_837B))
                || (first.kind == Native(NativeOperandTag::TAG_BC7C)
                    && second.kind == Native(NativeOperandTag::TAG_BC7C))
                || (first.kind == Native(NativeOperandTag::TAG_81DD)
                    && second.kind == Native(NativeOperandTag::TAG_81DD))
                || (first.kind == Native(NativeOperandTag::TAG_8100)
                    && second.kind == Native(NativeOperandTag::TAG_8100))
                || (first.kind == Native(NativeOperandTag::TAG_820F)
                    && second.kind == Native(NativeOperandTag::TAG_820F))
        }
        (LineLineDistance, [first, second]) => {
            (first.kind == E1 && second.kind == E1)
                || (first.kind == Native(NativeOperandTag::TAG_8386)
                    && second.kind == Native(NativeOperandTag::TAG_8386))
                || (first.kind == Native(NativeOperandTag::TAG_810F)
                    && second.kind == Native(NativeOperandTag::TAG_810F))
                || (first.kind == Native(NativeOperandTag::TAG_BC87)
                    && second.kind == Native(NativeOperandTag::TAG_BC87))
                || (first.kind == Native(NativeOperandTag::TAG_81E7)
                    && second.kind == Native(NativeOperandTag::TAG_81E7))
        }
        (PointLineDistance, [first, second]) => {
            (first.kind == D6 && second.kind == E1)
                || (first.kind == Native(NativeOperandTag::TAG_837B)
                    && second.kind == Native(NativeOperandTag::TAG_8386))
                || (first.kind == Native(NativeOperandTag::TAG_BC7C)
                    && second.kind == Native(NativeOperandTag::TAG_BC87))
                || (first.kind == Native(NativeOperandTag::TAG_81DD)
                    && second.kind == Native(NativeOperandTag::TAG_81E7))
        }
        (PointPointHorizontalDistance | PointPointVerticalDistance, [first, second]) => {
            (first.kind == Native(NativeOperandTag::TAG_8152)
                && second.kind == Native(NativeOperandTag::TAG_8152))
                || (first.kind == Native(NativeOperandTag::TAG_80D5)
                    && second.kind == Native(NativeOperandTag::TAG_80D5))
                || (first.kind == Native(NativeOperandTag::TAG_8DCB)
                    && second.kind == Native(NativeOperandTag::TAG_8DCB))
        }
        (Angle, [first, second]) => {
            (first.kind == Native(NativeOperandTag::TAG_8DDA)
                && second.kind == Native(NativeOperandTag::TAG_8DDA))
                || (first.kind == Native(NativeOperandTag::TAG_80D5)
                    && second.kind == Native(NativeOperandTag::TAG_80D5))
        }
        _ => false,
    }
}

fn is_solver_point_operand(kind: FeatureInputOperandKind) -> bool {
    matches!(
        kind,
        FeatureInputOperandKind::Native(NativeOperandTag::TAG_8100 | NativeOperandTag::TAG_820F)
    )
}

pub(super) fn relation_uses_solver_points(relation: &FeatureInputRelationInstance) -> bool {
    relation.family == FeatureInputRelationFamily::PointPointDistance
        && relation.operands.len() == 2
        && relation
            .operands
            .iter()
            .all(|operand| is_solver_point_operand(operand.kind))
}

fn relation_signature_for_declaration(
    family: FeatureInputRelationFamily,
    scalar: &FeatureInputScalar,
) -> bool {
    relation_signature(family, &scalar.operands)
        || (matches!(
            scalar.role,
            FeatureInputScalarRole::Display | FeatureInputScalarRole::Driving
        ) && dynamic_relation_signature(family, &scalar.operands))
}

fn dynamic_relation_signature(
    family: FeatureInputRelationFamily,
    operands: &[FeatureInputOperand],
) -> bool {
    match family {
        FeatureInputRelationFamily::CircleDiameter => false,
        _ => matches!(
            operands,
            [
                FeatureInputOperand {
                    kind: FeatureInputOperandKind::Native(_),
                    ..
                },
                FeatureInputOperand {
                    kind: FeatureInputOperandKind::Native(_),
                    ..
                }
            ]
        ),
    }
}

/// Returns whether a relation uses a lane-local operand tag whose meaning is
/// supplied by the declared relation family and operand position.
pub(super) fn relation_uses_dynamic_operands(relation: &FeatureInputRelationInstance) -> bool {
    match relation.family {
        // `80d5` is a point carrier in point-distance records but occurs in the
        // line-role cells of angular records. The family therefore owns its
        // meaning in this case even though the static signature recognizes it.
        FeatureInputRelationFamily::Angle => {
            dynamic_relation_signature(relation.family, &relation.operands)
                && !matches!(
                    relation.operands.as_slice(),
                    [
                        FeatureInputOperand {
                            kind: FeatureInputOperandKind::Native(NativeOperandTag::TAG_8DDA),
                            ..
                        },
                        FeatureInputOperand {
                            kind: FeatureInputOperandKind::Native(NativeOperandTag::TAG_8DDA),
                            ..
                        }
                    ]
                )
        }
        FeatureInputRelationFamily::CircleDiameter => false,
        family => {
            !relation_signature(family, &relation.operands)
                && dynamic_relation_signature(family, &relation.operands)
        }
    }
}

/// A lane's scalar values and per-feature sketch entities, indexed once for
/// every relation that measures against them.
struct RelationOperandIndex<'a> {
    scalar_values: HashMap<&'a str, cadmpeg_ir::scalar::FiniteReal>,
    feature_entities: HashMap<&'a str, Vec<&'a crate::records::SketchInputEntity>>,
}

impl<'a> RelationOperandIndex<'a> {
    fn new(lane: &'a FeatureInputLane) -> Self {
        let mut scalar_values = HashMap::new();
        for scalar in &lane.scalars {
            scalar_values
                .entry(scalar.id.as_str())
                .or_insert(scalar.value);
        }
        let mut feature_entities = HashMap::<_, Vec<_>>::new();
        for entity in &lane.sketch_entities {
            if let Some(feature) = entity.feature_ref.as_deref() {
                feature_entities.entry(feature).or_default().push(entity);
            }
        }
        for entities in feature_entities.values_mut() {
            entities.sort_unstable_by(|left, right| {
                (left.offset(), left.ordinal()).cmp(&(right.offset(), right.ordinal()))
            });
        }
        Self {
            scalar_values,
            feature_entities,
        }
    }

    fn scalar_value(&self, id: &str) -> Option<cadmpeg_ir::scalar::FiniteReal> {
        self.scalar_values.get(id).copied()
    }

    fn target_value(
        &self,
        relation: &FeatureInputRelationInstance,
    ) -> Option<cadmpeg_ir::scalar::FiniteReal> {
        self.scalar_value(
            relation
                .parameter_scalar_ref()
                .or(relation.display_scalar_ref())?,
        )
    }

    /// The feature's entities in offset and ordinal order.
    fn feature_entities(&self, feature: &str) -> &[&'a crate::records::SketchInputEntity] {
        self.feature_entities
            .get(feature)
            .map_or(&[], Vec::as_slice)
    }
}

fn is_finite_point(entity: &crate::records::SketchInputEntity) -> bool {
    matches!(
        entity.kind(),
        crate::records::SketchInputKind::Point | crate::records::SketchInputKind::ConstrainedPoint
    ) && entity
        .coordinates_m
        .is_some_and(|coordinates| coordinates.into_iter().all(f64::is_finite))
}

fn push_point_candidate<'a>(
    candidates: &mut Vec<&'a crate::records::SketchInputEntity>,
    seen: &mut HashSet<&'a str>,
    candidate: Option<&'a crate::records::SketchInputEntity>,
) {
    let Some(candidate) = candidate.filter(|candidate| is_finite_point(candidate)) else {
        return;
    };
    if seen.insert(candidate.id()) {
        candidates.push(candidate);
    }
}

fn dynamic_point_candidates<'a>(
    entities: &[&'a crate::records::SketchInputEntity],
    operand: &FeatureInputOperand,
    use_explicit: bool,
) -> Vec<&'a crate::records::SketchInputEntity> {
    let address = usize::from(operand.entity_index);
    if let Some(entity_ref) = operand.entity_ref.as_deref().filter(|_| use_explicit) {
        return entities
            .iter()
            .copied()
            .filter(|entity| entity.id() == entity_ref && is_finite_point(entity))
            .collect();
    }
    let mut candidates = Vec::new();
    let mut seen = HashSet::new();
    push_point_candidate(&mut candidates, &mut seen, entities.get(address).copied());
    let coordinate_point = entities
        .iter()
        .copied()
        .filter(|entity| is_finite_point(entity))
        .nth(address);
    push_point_candidate(&mut candidates, &mut seen, coordinate_point);
    for entity in entities.iter().copied().filter(|entity| {
        entity.object_index() == Some(u32::from(operand.entity_index))
            || entity.local_id() == Some(u32::from(operand.entity_index))
    }) {
        push_point_candidate(&mut candidates, &mut seen, Some(entity));
    }
    candidates.sort_unstable_by(|left, right| left.id().cmp(right.id()));
    candidates
}

fn dynamic_solver_line<'a>(
    entities: &[&'a crate::records::SketchInputEntity],
    index: u16,
) -> Option<[&'a crate::records::SketchInputEntity; 2]> {
    let start = usize::from(index).checked_mul(2)?;
    let mut points = entities
        .iter()
        .copied()
        .filter(|entity| is_finite_point(entity));
    let first = points.nth(start)?;
    let second = points.next()?;
    (first.coordinates_m?.get() != second.coordinates_m?.get()).then_some([first, second])
}

fn point_distance(first: [f64; 2], second: [f64; 2]) -> f64 {
    (second[0] - first[0]).hypot(second[1] - first[1])
}

fn axis_distance(first: [f64; 2], second: [f64; 2], horizontal: bool) -> f64 {
    if horizontal {
        (second[0] - first[0]).abs()
    } else {
        (second[1] - first[1]).abs()
    }
}

fn line_direction(line: [[f64; 2]; 2]) -> [f64; 2] {
    [line[1][0] - line[0][0], line[1][1] - line[0][1]]
}

pub(super) fn line_line_distance(first: [[f64; 2]; 2], second: [[f64; 2]; 2]) -> Option<f64> {
    let first_direction = line_direction(first);
    let second_direction = line_direction(second);
    let unit = |direction: [f64; 2]| {
        let v = cadmpeg_ir::math::Vector3::new(direction[0], direction[1], 0.0);
        (v.norm() > SKETCH_POINT_TOLERANCE)
            .then(|| {
                cadmpeg_ir::features::geometry::FiniteVector3::new(v)
                    .and_then(cadmpeg_ir::features::geometry::FiniteVector3::unit_nonzero)
            })
            .flatten()
    };
    let a = unit(first_direction)?;
    let b = unit(second_direction)?;
    if (a.x * b.y - a.y * b.x).abs() > SKETCH_POINT_TOLERANCE {
        return None;
    }
    let distance = ((second[0][0] - first[0][0]) * a.y - (second[0][1] - first[0][1]) * a.x).abs();
    distance.is_finite().then_some(distance)
}

pub(super) fn line_line_angle(first: [[f64; 2]; 2], second: [[f64; 2]; 2]) -> Option<f64> {
    let first = line_direction(first);
    let second = line_direction(second);
    let first = cadmpeg_ir::math::Vector3::new(first[0], first[1], 0.0);
    let second = cadmpeg_ir::math::Vector3::new(second[0], second[1], 0.0);
    if first.norm() <= SKETCH_POINT_TOLERANCE || second.norm() <= SKETCH_POINT_TOLERANCE {
        return None;
    }
    let first = cadmpeg_ir::features::geometry::FiniteVector3::new(first)?.unit_nonzero()?;
    let second = cadmpeg_ir::features::geometry::FiniteVector3::new(second)?.unit_nonzero()?;
    Some(first.cross(second).norm().atan2(first.dot(second)))
}

fn dynamic_line_line_angle(first: [[f64; 2]; 2], second: [[f64; 2]; 2]) -> Option<f64> {
    let angle = line_line_angle(first, second)?;
    Some(angle.min(std::f64::consts::PI - angle))
}

fn same_relation_dimension(left: f64, right: f64) -> bool {
    (left - right).abs() <= SKETCH_POINT_TOLERANCE * left.abs().max(right.abs()).max(1.0)
}

fn clear_relation_operands(relation: &mut FeatureInputRelationInstance) {
    for operand in &mut relation.operands {
        operand.entity_ref = None;
    }
}

fn dynamic_curve_reference_is_valid(
    entities: &[&crate::records::SketchInputEntity],
    entity_ref: Option<&str>,
) -> bool {
    entity_ref.is_some_and(|entity_ref| {
        entities.iter().any(|entity| {
            entity.id() == entity_ref
                && matches!(
                    entity.kind(),
                    crate::records::SketchInputKind::LineOrCircle
                        | crate::records::SketchInputKind::Arc
                        | crate::records::SketchInputKind::Relation(_)
                )
        })
    })
}

fn dynamic_point_matches<'a>(
    first_candidates: &[&'a crate::records::SketchInputEntity],
    second_candidates: &[&'a crate::records::SketchInputEntity],
    target: f64,
    horizontal: Option<bool>,
) -> Vec<(&'a str, &'a str)> {
    let mut matches = Vec::<(&str, &str)>::new();
    for first in first_candidates {
        let Some(first_coordinates) = first
            .coordinates_m
            .map(cadmpeg_ir::units::FiniteVector::get)
        else {
            continue;
        };
        for second in second_candidates {
            if first.id() == second.id() {
                continue;
            }
            let Some(second_coordinates) = second
                .coordinates_m
                .map(cadmpeg_ir::units::FiniteVector::get)
            else {
                continue;
            };
            let measured = horizontal.map_or_else(
                || point_distance(first_coordinates, second_coordinates),
                |horizontal| axis_distance(first_coordinates, second_coordinates, horizontal),
            );
            if same_relation_dimension(measured, target) {
                matches.push((first.id(), second.id()));
            }
        }
    }
    matches
}

fn bind_dynamic_point_relation(
    relation: &mut FeatureInputRelationInstance,
    entities: &[&crate::records::SketchInputEntity],
    target: f64,
    horizontal: Option<bool>,
) {
    let [first, second] = relation.operands.as_slice() else {
        clear_relation_operands(relation);
        return;
    };
    let first_candidates = dynamic_point_candidates(entities, first, true);
    let second_candidates = dynamic_point_candidates(entities, second, true);
    let coordinate_points = entities
        .iter()
        .copied()
        .filter(|entity| is_finite_point(entity))
        .collect::<Vec<_>>();
    let mut matches =
        dynamic_point_matches(&first_candidates, &second_candidates, target, horizontal);
    if matches.is_empty() {
        let first_fallback = if first.entity_ref.is_none() {
            &coordinate_points
        } else {
            &first_candidates
        };
        let second_fallback = if second.entity_ref.is_none() {
            &coordinate_points
        } else {
            &second_candidates
        };
        matches = dynamic_point_matches(first_fallback, second_fallback, target, horizontal);
    }
    if matches.is_empty() {
        let first_relaxed = if first.entity_ref.is_some() {
            Some(dynamic_point_candidates(entities, first, false))
        } else {
            None
        };
        let second_relaxed = if second.entity_ref.is_some() {
            Some(dynamic_point_candidates(entities, second, false))
        } else {
            None
        };
        matches = dynamic_point_matches(
            first_relaxed.as_deref().unwrap_or(&first_candidates),
            second_relaxed.as_deref().unwrap_or(&second_candidates),
            target,
            horizontal,
        );
    }
    if matches.is_empty() {
        let first_relaxed = if first.entity_ref.is_some() {
            &coordinate_points
        } else {
            &first_candidates
        };
        let second_relaxed = if second.entity_ref.is_some() {
            &coordinate_points
        } else {
            &second_candidates
        };
        matches = dynamic_point_matches(first_relaxed, second_relaxed, target, horizontal);
    }
    matches.sort_unstable_by(Ord::cmp);
    matches.dedup();
    if let [(first, second)] = matches.as_slice() {
        relation.operands[0].entity_ref = Some((*first).to_owned());
        relation.operands[1].entity_ref = Some((*second).to_owned());
    } else {
        clear_relation_operands(relation);
    }
}

fn bind_dynamic_point_line_relation(
    relation: &mut FeatureInputRelationInstance,
    entities: &[&crate::records::SketchInputEntity],
    target: f64,
) {
    let Ok([point_operand, line_operand]) =
        <&mut [FeatureInputOperand; 2]>::try_from(relation.operands.as_mut_slice())
    else {
        clear_relation_operands(relation);
        return;
    };
    if dynamic_curve_reference_is_valid(entities, line_operand.entity_ref.as_deref()) {
        return;
    }
    line_operand.entity_ref = None;
    let line_index = line_operand.entity_index;
    let point_candidates = dynamic_point_candidates(entities, point_operand, true);
    let Some(line_markers) = dynamic_solver_line(entities, line_index) else {
        clear_relation_operands(relation);
        return;
    };
    let [Some(first), Some(second)] = line_markers.map(|marker| marker.coordinates_m) else {
        clear_relation_operands(relation);
        return;
    };
    let direction = [second[0] - first[0], second[1] - first[1]];
    let length = direction[0].hypot(direction[1]);
    if length <= SKETCH_POINT_TOLERANCE {
        clear_relation_operands(relation);
        return;
    }
    let mut matches = point_candidates
        .iter()
        .filter_map(|point| {
            let coordinates = point.coordinates_m?.get();
            let measured = ((coordinates[0] - first[0]) * direction[1]
                - (coordinates[1] - first[1]) * direction[0])
                .abs()
                / length;
            same_relation_dimension(measured, target).then(|| point.id())
        })
        .collect::<Vec<_>>();
    matches.sort_unstable_by(Ord::cmp);
    matches.dedup();
    if let [point] = matches.as_slice() {
        relation.operands[0].entity_ref = Some((*point).to_owned());
        relation.operands[1].entity_ref = None;
    } else {
        clear_relation_operands(relation);
    }
}

fn bind_dynamic_line_relation(
    relation: &mut FeatureInputRelationInstance,
    entities: &[&crate::records::SketchInputEntity],
    target: f64,
    angle: bool,
) {
    let Ok([first_operand, second_operand]) =
        <&mut [FeatureInputOperand; 2]>::try_from(relation.operands.as_mut_slice())
    else {
        clear_relation_operands(relation);
        return;
    };
    let first_valid =
        dynamic_curve_reference_is_valid(entities, first_operand.entity_ref.as_deref());
    let second_valid =
        dynamic_curve_reference_is_valid(entities, second_operand.entity_ref.as_deref());
    if !first_valid {
        first_operand.entity_ref = None;
    }
    if !second_valid {
        second_operand.entity_ref = None;
    }
    if first_valid || second_valid {
        return;
    }
    let (first_index, second_index) = (first_operand.entity_index, second_operand.entity_index);
    let Some(first_markers) = dynamic_solver_line(entities, first_index) else {
        clear_relation_operands(relation);
        return;
    };
    let Some(second_markers) = dynamic_solver_line(entities, second_index) else {
        clear_relation_operands(relation);
        return;
    };
    if first_operand.entity_index == second_operand.entity_index {
        clear_relation_operands(relation);
        return;
    }
    let [Some(first_line_first), Some(first_line_second)] = first_markers.map(|marker| {
        marker
            .coordinates_m
            .map(cadmpeg_ir::units::FiniteVector::get)
    }) else {
        clear_relation_operands(relation);
        return;
    };
    let [Some(second_line_first), Some(second_line_second)] = second_markers.map(|marker| {
        marker
            .coordinates_m
            .map(cadmpeg_ir::units::FiniteVector::get)
    }) else {
        clear_relation_operands(relation);
        return;
    };
    let measured = if angle {
        dynamic_line_line_angle(
            [first_line_first, first_line_second],
            [second_line_first, second_line_second],
        )
    } else {
        line_line_distance(
            [first_line_first, first_line_second],
            [second_line_first, second_line_second],
        )
    };
    if measured.is_some_and(|measured| same_relation_dimension(measured, target)) {
        relation.operands[0].entity_ref = None;
        relation.operands[1].entity_ref = None;
    } else {
        clear_relation_operands(relation);
    }
}

/// Binds the operands of a relation whose family states their meaning to
/// the sketch entities that measure its value.
fn bind_relation_geometry(
    relation: &mut FeatureInputRelationInstance,
    index: &RelationOperandIndex<'_>,
) {
    let dynamic = relation_uses_dynamic_operands(relation);
    let unbound_points = matches!(
        relation.family,
        FeatureInputRelationFamily::PointPointDistance
            | FeatureInputRelationFamily::PointPointHorizontalDistance
            | FeatureInputRelationFamily::PointPointVerticalDistance
    ) && relation
        .operands
        .iter()
        .all(|operand| operand.entity_ref.is_none());
    if !(dynamic || unbound_points) || relation.family == FeatureInputRelationFamily::CircleDiameter
    {
        return;
    }
    let Some(target) = index.target_value(relation) else {
        if dynamic {
            clear_relation_operands(relation);
        }
        return;
    };
    if relation_uses_solver_points(relation) {
        return;
    }
    if target.get() < 0.0 {
        if dynamic {
            clear_relation_operands(relation);
        }
        return;
    }
    let entities = index.feature_entities(relation.feature_ref.as_str());
    match relation.family {
        FeatureInputRelationFamily::PointPointDistance => {
            bind_dynamic_point_relation(relation, entities, target.get(), None);
        }
        FeatureInputRelationFamily::PointPointHorizontalDistance => {
            bind_dynamic_point_relation(relation, entities, target.get(), Some(true));
        }
        FeatureInputRelationFamily::PointPointVerticalDistance => {
            bind_dynamic_point_relation(relation, entities, target.get(), Some(false));
        }
        FeatureInputRelationFamily::PointLineDistance => {
            bind_dynamic_point_line_relation(relation, entities, target.get());
        }
        FeatureInputRelationFamily::LineLineDistance => {
            bind_dynamic_line_relation(relation, entities, target.get(), false);
        }
        FeatureInputRelationFamily::Angle => {
            bind_dynamic_line_relation(relation, entities, target.get(), true);
        }
        FeatureInputRelationFamily::CircleDiameter => {}
    }
}

pub(super) fn scalar_role(payload: &[u8], trailer_offset: usize) -> FeatureInputScalarRole {
    let shifted_layout = shifted_value_only_scalar_trailer(payload, trailer_offset);
    let fixed_layout = payload.get(trailer_offset..trailer_offset + 3) == Some(&[0, 0, 0])
        && payload
            .get(trailer_offset + 7..trailer_offset + 21)
            .is_some_and(|bytes| bytes.iter().all(|byte| *byte == 0))
        && payload.get(trailer_offset + 24..trailer_offset + 29) == Some(&[0, 0, 0, 2, 0]);
    let role_offset = if shifted_layout {
        trailer_offset + shifted_trailer::ROLE
    } else if compact_scalar_layout(payload, trailer_offset) {
        trailer_offset + 27
    } else if fixed_layout {
        trailer_offset + 29
    } else if legacy_scalar_layout(payload, trailer_offset) {
        trailer_offset + 30
    } else {
        return FeatureInputScalarRole::Native;
    };
    match payload.get(role_offset) {
        Some(0) => FeatureInputScalarRole::Driving,
        Some(1) => FeatureInputScalarRole::Display,
        _ => FeatureInputScalarRole::Native,
    }
}

pub(super) fn shifted_value_only_scalar_trailer(payload: &[u8], trailer_offset: usize) -> bool {
    payload.get(
        trailer_offset + shifted_trailer::ZERO_PREFIX..trailer_offset + shifted_trailer::OBJECT_ID,
    ) == Some(&[0, 0, 0])
        && payload
            .get(
                trailer_offset + shifted_trailer::ZERO_OBJECT_TAIL
                    ..trailer_offset + shifted_trailer::LAYOUT_MARKER,
            )
            .is_some_and(|bytes| bytes.iter().all(|byte| *byte == 0))
        && payload.get(
            trailer_offset + shifted_trailer::LAYOUT_MARKER..trailer_offset + shifted_trailer::ROLE,
        ) == Some(&[1, 0, 0, 0, 2, 0])
        && payload
            .get(trailer_offset + shifted_trailer::ROLE)
            .is_some_and(|role| *role <= 1)
        && payload
            .get(trailer_offset + shifted_trailer::ZERO_TAIL..trailer_offset + shifted_trailer::LEN)
            .is_some_and(|bytes| bytes.iter().all(|byte| *byte == 0))
}

fn shifted_value_only_scalar_layout(payload: &[u8], trailer_offset: usize) -> bool {
    shifted_value_only_scalar_trailer(payload, trailer_offset)
        && payload
            .get(trailer_offset + 35..trailer_offset + 47)
            .is_some_and(|cell| {
                cell[0..2] != [0, 0]
                    && cell[0..2] != [0xff, 0xff]
                    && cell[4..8] == [0xff; 4]
                    && cell[8..12] == [0; 4]
            })
}

pub(super) fn compact_scalar_layout(payload: &[u8], trailer_offset: usize) -> bool {
    !shifted_value_only_scalar_layout(payload, trailer_offset)
        && payload.get(trailer_offset..trailer_offset + 3) == Some(&[0, 0, 0])
        && payload
            .get(trailer_offset + 7..trailer_offset + 21)
            .is_some_and(|bytes| bytes.iter().all(|byte| *byte == 0))
        && payload.get(trailer_offset + 21..trailer_offset + 27) == Some(&[1, 0, 0, 0, 2, 0])
        && payload
            .get(trailer_offset + 28..trailer_offset + 35)
            .is_some_and(|bytes| bytes.iter().all(|byte| *byte == 0))
        && payload.get(trailer_offset + 39..trailer_offset + 43) == Some(&[0xff; 4])
        && payload.get(trailer_offset + 47..trailer_offset + 51) == Some(&[0xff; 4])
}

pub(super) fn legacy_scalar_layout(payload: &[u8], trailer_offset: usize) -> bool {
    payload.get(trailer_offset..trailer_offset + 3) == Some(&[0, 0, 0])
        && payload
            .get(trailer_offset + 7..trailer_offset + 24)
            .is_some_and(|bytes| bytes.iter().all(|byte| *byte == 0))
        && payload.get(trailer_offset + 24..trailer_offset + 30) == Some(&[0x0f, 0, 0, 0, 2, 0])
}

#[cfg(test)]
mod binary_relation_operand_tests {
    #[test]
    fn large_perpendicular_lines_have_no_parallel_distance() {
        assert_eq!(
            super::line_line_distance([[0., 0.], [1e200, 0.]], [[0., 1.], [0., 1e200]]),
            None
        );
        assert_eq!(
            super::line_line_distance([[0., 0.], [1e200, 0.]], [[0., 1.], [1e200, 1.]]),
            Some(1.)
        );
    }

    use super::{
        bind_dynamic_line_relation, bind_dynamic_point_line_relation, FeatureInputOperand,
        FeatureInputOperandKind, FeatureInputRelationFamily, FeatureInputRelationInstance,
    };

    fn operand(entity_index: u16) -> FeatureInputOperand {
        FeatureInputOperand {
            offset: u64::from(entity_index),
            reference_ref: format!("reference#{entity_index}"),
            kind: FeatureInputOperandKind::D6,
            entity_index,
            entity_ref: Some(format!("entity#{entity_index}")),
        }
    }

    fn relation(operands: Vec<FeatureInputOperand>) -> FeatureInputRelationInstance {
        FeatureInputRelationInstance {
            id: "relation#0".to_string(),
            parent: "lane#0".to_string(),
            ordinal: 0,
            offset: 0,
            family: FeatureInputRelationFamily::PointPointDistance,
            class_ref: "class#0".to_string(),
            feature_ref: "feature#0".to_string(),
            scalars: crate::records::relation_scalars::RelationScalars::from_refs(
                vec!["scalar#0".into()],
                None,
                None,
            )
            .unwrap(),
            operands,
        }
    }

    #[test]
    fn a_relation_that_is_not_binary_is_cleared_rather_than_indexed() {
        for operands in [
            vec![],
            vec![operand(0)],
            vec![operand(0), operand(1), operand(2)],
        ] {
            let mut point_line = relation(operands.clone());
            bind_dynamic_point_line_relation(&mut point_line, &[], 1.0);
            assert!(point_line
                .operands
                .iter()
                .all(|operand| operand.entity_ref.is_none()));

            let mut line = relation(operands);
            bind_dynamic_line_relation(&mut line, &[], 1.0, false);
            assert!(line
                .operands
                .iter()
                .all(|operand| operand.entity_ref.is_none()));
        }
    }
}

#[cfg(test)]
mod relation_records_tests;
