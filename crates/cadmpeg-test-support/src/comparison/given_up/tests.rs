// SPDX-License-Identifier: Apache-2.0
//! Allowances consume real source callbacks and shared helper failure homes.

use super::{Allowances, DecodeSnapshot};
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, View};
use cadmpeg_core::dialect::{DialectLayers, FormatIdentity};
use cadmpeg_core::CodecError;
use cadmpeg_ir::attributes::{AttributeTarget, AttributeValue, SourceAttribute};
use cadmpeg_ir::document::{ArenaName, ContentScope};
use cadmpeg_ir::features::FinitePoint3;
use cadmpeg_ir::ids::{AttributeId, LayerId, PointId};
use cadmpeg_ir::presentation::{PresentationItem, PresentationLayer};
use cadmpeg_ir::report::decode::{DecodeReport, DecodeTransfer};
use cadmpeg_ir::source_fidelity::parts::{DecodeParts, PartLevel, PartOutput, PartSpec};
use cadmpeg_ir::{CadIr, CodecFormat, SourceFidelity};
use serde_json::json;
use std::collections::BTreeSet;

const GOOD: &[u8] = b"#1=CARTESIAN_POINT('first',(1,0,0));#2=CARTESIAN_POINT('peer',(2,0,0));";
const BAD: &[u8] = b"#1=CARTESIAN_POINT('first',(x,0,0));#2=CARTESIAN_POINT('peer',(2,0,0));";

fn point(number: usize) -> PointId {
    PointId::mint(format!("step:model:point#{number}")).unwrap()
}
fn report(ir: &CadIr, losses: &[cadmpeg_ir::report::loss::LossNote]) -> DecodeReport {
    serde_json::from_value(json!({
        "identity": FormatIdentity::<DialectLayers>::unclassified("step"),
        "transfer": DecodeTransfer { scope: ir.scope, geometry_transferred: false },
        "losses": losses, "notes": [],
    }))
    .unwrap()
}
fn decode(bytes: &[u8]) -> (CadIr, SourceFidelity, DecodeReport) {
    let arena = DecodeArena::new();
    let (ctx, root) =
        DecodeContext::from_root_bytes(bytes, &arena, &DecodePolicy::service()).unwrap();
    let mut output = PartOutput::new(CodecFormat::Step);
    let mut start = 0;
    for (number, end) in bytes
        .iter()
        .enumerate()
        .filter_map(|(index, byte)| (*byte == b';').then_some(index + 1))
        .enumerate()
    {
        let extent = root.child(start, end).unwrap();
        let id = point(number + 1);
        ctx.decode_part(
            &mut output,
            extent,
            PartSpec::new(
                PartLevel::parse("step.simple_entity_instance").unwrap(),
                vec![
                    u64::try_from(start).unwrap()..u64::try_from(start + 3).unwrap(),
                    u64::try_from(end - 1).unwrap()..u64::try_from(end).unwrap(),
                ],
            ),
            |stage| {
                ctx.record(id.as_str(), extent, || {
                    let coordinates = read_coordinates(extent)?;
                    stage.insert_model(
                        &ctx,
                        cadmpeg_ir::topology::Point::new(id.clone(), coordinates, None),
                    )
                })
            },
        )
        .unwrap();
        start = end;
    }
    // Authored users exercise the real neutral owners' required/list rules;
    // they are not attributed to invented source positions.
    let first = AttributeId::mint("step:model:attribute#required").unwrap();
    for (id, target) in [
        (first.clone(), AttributeTarget::Point(point(1))),
        (
            AttributeId::mint("step:model:attribute#transitive").unwrap(),
            AttributeTarget::Record {
                arena: ArenaName::parse("attributes").unwrap(),
                id: first.to_string(),
            },
        ),
    ] {
        output
            .insert_model(
                &ctx,
                SourceAttribute {
                    id,
                    native_ref: None,
                    target,
                    name: "required user".into(),
                    values: vec![AttributeValue::Integer(17)],
                },
            )
            .unwrap();
    }
    output
        .insert_model(
            &ctx,
            PresentationLayer {
                id: LayerId::mint("step:model:layer#list").unwrap(),
                native_ref: None,
                name: "independent holder".into(),
                parent: None,
                description: None,
                visible: None,
                items: vec![
                    PresentationItem::Point { point: point(1) },
                    PresentationItem::Point { point: point(2) },
                ],
            },
        )
        .unwrap();
    output
        .fidelity_mut()
        .install_record_captures(&ctx, |_, _| Ok(()))
        .unwrap();
    let (model, native, fidelity, losses) = output.into_parts(&ctx).unwrap().into_parts();
    let mut ir = CadIr::empty();
    ir.model = model;
    ir.native = native;
    let report = report(&ir, &losses);
    (ir, fidelity, report)
}
fn read_coordinates(extent: View<'_>) -> Result<FinitePoint3, CodecError> {
    let text = std::str::from_utf8(extent.window()).map_err(CodecError::malformed)?;
    let start = text
        .find(",(")
        .ok_or_else(|| CodecError::malformed("point has no coordinate tuple"))?
        + 2;
    let end = start
        + text[start..]
            .find(')')
            .ok_or_else(|| CodecError::malformed("point tuple has no endpoint"))?;
    let mut values = Vec::new();
    let mut offset = extent.start() + start;
    for raw in text[start..end].split(',') {
        let bytes = extent
            .read_slice(offset..offset + raw.len())
            .ok_or_else(|| CodecError::malformed("coordinate source is outside actual point"))?;
        let value = std::str::from_utf8(bytes)
            .map_err(CodecError::malformed)?
            .parse::<f64>()
            .map_err(CodecError::malformed)?;
        values.push(value);
        offset += raw.len() + 1;
    }
    let [x, y, z]: [f64; 3] = values
        .try_into()
        .map_err(|_| CodecError::malformed("point needs three coordinates"))?;
    FinitePoint3::new(cadmpeg_ir::math::Point3::new(x, y, z))
        .ok_or_else(|| CodecError::malformed("point coordinate is not finite"))
}
fn allowances(
    before: &CadIr,
    previous: &SourceFidelity,
    after: &CadIr,
    current: &SourceFidelity,
    report: &DecodeReport,
    source: &[u8],
) -> Result<Allowances, String> {
    Allowances::given_up(
        DecodeSnapshot::new(before, previous),
        DecodeSnapshot::new(after, current),
        report,
        "file",
        |stream, range| {
            if stream != "file" {
                return None;
            }
            source.get(usize::try_from(range.start).ok()?..usize::try_from(range.end).ok()?)
        },
    )
}
#[test]
fn actual_given_up_home_allows_only_its_whole_record_required_cascade_and_list_entry() {
    let (before, previous, _) = decode(GOOD);
    let (after, current, report) = decode(BAD);
    assert_eq!(before.model.points.len(), 2);
    assert_eq!(before.model.attributes.len(), 2);
    assert_eq!(after.model.points.len(), 1);
    assert_eq!(after.model.points[0], before.model.points[1]);
    assert!(after.model.attributes.is_empty());
    assert_eq!(
        after.model.presentation_layers[0].name,
        "independent holder"
    );
    assert_eq!(
        after.model.presentation_layers[0].items,
        [PresentationItem::Point { point: point(2) }]
    );
    assert_eq!(current.retained_records().len(), 1);
    assert_eq!(
        current.retained_records().values().next().unwrap().data(),
        Some(&BAD[..BAD.iter().position(|byte| *byte == b';').unwrap() + 1])
    );
    assert_eq!(
        previous.record_layout()[point(2).as_str()],
        current.record_layout()[point(2).as_str()]
    );
    let allowed = allowances(&before, &previous, &after, &current, &report, BAD).unwrap();
    assert_eq!(
        allowed.removed,
        [
            point(1).to_string(),
            "step:model:attribute#required".into(),
            "step:model:attribute#transitive".into()
        ]
        .into()
    );
    super::super::agree(
        &serde_json::to_value(&before).unwrap(),
        &serde_json::to_value(&after).unwrap(),
        &allowed,
        super::super::Mode::Same,
    )
    .unwrap();
    let mut unreported = after.clone();
    unreported.model.points.clear();
    assert!(super::super::agree(
        &serde_json::to_value(&before).unwrap(),
        &serde_json::to_value(&unreported).unwrap(),
        &allowed,
        super::super::Mode::Same
    )
    .is_err());
}
#[test]
fn wrong_home_wrong_scope_missing_loss_and_loss_without_part_never_license_removal() {
    let (before, previous, _) = decode(GOOD);
    let (after, current, report) = decode(BAD);
    assert!(allowances(&before, &previous, &after, &current, &report, GOOD).is_err());
    let mut no_loss = report.clone();
    no_loss.losses.clear();
    assert!(allowances(&before, &previous, &after, &current, &no_loss, BAD).is_err());
    let mut wrong_scope = serde_json::to_value(&report).unwrap();
    wrong_scope["transfer"]["scope"] = serde_json::to_value(ContentScope::container()).unwrap();
    let wrong_scope: DecodeReport = serde_json::from_value(wrong_scope).unwrap();
    assert!(allowances(&before, &previous, &after, &current, &wrong_scope, BAD).is_err());
    let mut wrong_format = serde_json::to_value(&report).unwrap();
    wrong_format["identity"] =
        serde_json::to_value(FormatIdentity::<DialectLayers>::unclassified("rhino")).unwrap();
    let wrong_format: DecodeReport = serde_json::from_value(wrong_format).unwrap();
    assert!(allowances(&before, &previous, &after, &current, &wrong_format, BAD).is_err());
    assert!(allowances(&before, &previous, &before, &previous, &report, GOOD).is_err());
    let unsupported = allowances(
        &before,
        &SourceFidelity::default(),
        &after,
        &current,
        &report,
        BAD,
    )
    .unwrap();
    assert!(unsupported.removed.is_empty());
    assert!(super::super::agree(
        &serde_json::to_value(&before).unwrap(),
        &serde_json::to_value(&after).unwrap(),
        &unsupported,
        super::super::Mode::Same
    )
    .is_err());
}

// #224 comment6376513563: generic actual typed fields may retain Native
// semantics after an operand disappears. The enclosing record stays strict.
fn native_fallback_pair() -> (serde_json::Value, serde_json::Value, Allowances) {
    use cadmpeg_ir::features::{
        BodySelection, DistinctMembers, Feature, FeatureContent, FeatureDefinition,
        FeatureEvaluation, FeatureId, FeatureOperation,
    };
    let body = cadmpeg_ir::ids::BodyId::mint("step:model:body#operand").unwrap();
    let feature = FeatureId::mint("step:model:feature#consumer").unwrap();
    let mut ir = CadIr::empty();
    ir.model.bodies.push(cadmpeg_ir::topology::Body {
        id: body.clone(),
        kind: cadmpeg_ir::topology::BodyKind::General,
        regions: vec![],
        transform: None,
        name: None,
        color: None,
        visible: None,
        native_ref: None,
    });
    ir.model.features.push(Feature {
        id: feature.clone(),
        ordinal: 0,
        name: Some("independent name".into()),
        suppressed: None,
        needs_recompute: false,
        dependencies: DistinctMembers::default(),
        source_properties: Default::default(),
        source_tag: None,
        source_text: None,
        source_content: FeatureContent::default(),
        native_ref: None,
        evaluation: FeatureEvaluation::from_definition(FeatureDefinition::Operation(
            FeatureOperation::BaseFeature {
                bodies: BodySelection::Bodies(
                    DistinctMembers::try_from(vec![body.clone()]).unwrap(),
                ),
            },
        )),
    });
    let parameter = cadmpeg_ir::ParameterId::mint("step:model:parameter#lost").unwrap();
    ir.model
        .parameters
        .push(cadmpeg_ir::features::DesignParameter {
            id: parameter.clone(),
            owner: None,
            ordinal: 0,
            name: "Length".into(),
            expression: "10 mm".into(),
            driving: true,
            display: None,
            value: None,
            properties: Default::default(),
            pmi: None,
            native_ref: None,
        });
    let before = serde_json::to_value(&ir).unwrap();
    ir.model.bodies.clear();
    ir.model.parameters.clear();
    let native: FeatureOperation =
        serde_json::from_value(json!({"definition":"native","kind":"source operation"})).unwrap();
    ir.model.features[0]
        .evaluation
        .set_definition(FeatureDefinition::Operation(native));
    let after = serde_json::to_value(ir).unwrap();
    let allowances = Allowances {
        removed: BTreeSet::from([body.as_str().to_owned(), parameter.as_str().to_owned()]),
        fallbacks: BTreeSet::from([feature.as_str().to_owned()]),
        given_up: true,
        ..Default::default()
    };
    (before, after, allowances)
}

#[test]
fn typed_native_fallback_retains_its_independent_fields_and_rejects_a_top_level_key_gain() {
    let (before, mut after, allowances) = native_fallback_pair();
    super::super::agree(&before, &after, &allowances, super::super::Mode::Same).unwrap();
    after["model"]["features"][0]["bonus"] = json!(true);
    assert!(super::super::agree(&before, &after, &allowances, super::super::Mode::Same).is_err());
}

#[test]
fn typed_native_fallback_rejects_a_reference_to_the_disappeared_parameter() {
    let (before, mut after, allowances) = native_fallback_pair();
    let removed = before["model"]["parameters"][0]["id"].clone();
    after["model"]["features"][0]["definition"]["kind"] = removed;
    assert!(super::super::agree(&before, &after, &allowances, super::super::Mode::Same).is_err());
    let (before, mut after, allowances) = native_fallback_pair();
    after["model"]["features"][0]["name"] = json!("unrelated changed name");
    assert!(super::super::agree(&before, &after, &allowances, super::super::Mode::Same).is_err());
}

#[derive(serde::Serialize, serde::Deserialize)]
struct AggregateHeader {
    id: cadmpeg_ir::ids::Identity,
    header: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    coordinates: Option<FinitePoint3>,
}
impl cadmpeg_ir::schema::rewrite::typed::RewriteIdentities for AggregateHeader {
    fn visit_identity_references(
        &self,
        _ctx: &DecodeContext<'_>,
        _visitor: &mut dyn FnMut(&str) -> Result<(), CodecError>,
    ) -> Result<(), CodecError> {
        Ok(())
    }
    fn rewrite_identities<F: FnMut(&str) -> Result<String, CodecError>>(
        mut self,
        ctx: &DecodeContext<'_>,
        map: &mut cadmpeg_ir::schema::rewrite::typed::IdentityMap<'_, F>,
    ) -> Result<Self, CodecError> {
        self.id = cadmpeg_ir::schema::rewrite::typed::RewriteIdentities::rewrite_identities(
            self.id, ctx, map,
        )?;
        Ok(self)
    }
    fn rewrite_native_value<F: FnMut(&str) -> Result<String, CodecError>>(
        ctx: &DecodeContext<'_>,
        value: &mut serde_json::Value,
        map: &mut cadmpeg_ir::schema::rewrite::typed::IdentityMap<'_, F>,
    ) -> Result<(), CodecError> {
        cadmpeg_ir::schema::rewrite::typed::native_fields::rewrite_field(
            ctx,
            value,
            "id",
            map,
            |row: &Self| Some(&row.id),
        )
    }
}

fn aggregate_and_independent_point(bytes: &[u8]) -> (CadIr, SourceFidelity, DecodeReport) {
    let arena = DecodeArena::new();
    let (ctx, root) =
        DecodeContext::from_root_bytes(bytes, &arena, &DecodePolicy::service()).unwrap();
    let mut output = PartOutput::new(CodecFormat::Step);
    let header = root.child(0, 2).unwrap();
    let native_id = "step:native:header#aggregate";
    let (header_value, owner) = ctx
        .record_owner(native_id, root, || {
            let mut getter = header;
            Ok(String::from_utf8(getter.req_take(2)?.to_vec()).map_err(CodecError::malformed)?)
        })
        .unwrap();
    let mut prior = 2;
    let mut coordinates = None;
    for (number, end) in bytes
        .iter()
        .enumerate()
        .skip(2)
        .filter_map(|(index, byte)| (*byte == b';').then_some(index + 1))
        .enumerate()
    {
        let source = root.child(prior, end).unwrap();
        let parsed = ctx
            .decode_part(
                &mut output,
                source,
                PartSpec::new(
                    PartLevel::parse("step.simple_entity_instance").unwrap(),
                    vec![
                        u64::try_from(prior).unwrap()..u64::try_from(prior + 3).unwrap(),
                        u64::try_from(end - 1).unwrap()..u64::try_from(end).unwrap(),
                    ],
                ),
                |stage| {
                    if number == 0 {
                        ctx.record_fragment(&owner, source, || read_coordinates(source))
                            .map(Some)
                    } else {
                        ctx.record(point(2).as_str(), source, || {
                            let value = read_coordinates(source)?;
                            stage.insert_model(
                                &ctx,
                                cadmpeg_ir::topology::Point::new(point(2), value, None)
                                    .with_native_ref(Some(native_id.to_owned())),
                            )?;
                            Ok(None)
                        })
                    }
                },
            )
            .unwrap();
        if number == 0 {
            coordinates = parsed.flatten();
        }
        prior = end;
    }
    let native = AggregateHeader {
        id: cadmpeg_ir::ids::Identity::new(native_id).unwrap(),
        header: header_value,
        coordinates,
    };
    output.insert_native(&ctx, "headers", native).unwrap();
    output
        .fidelity_mut()
        .install_record_captures(&ctx, |_, _| Ok(()))
        .unwrap();
    let (model, native, fidelity, losses) = output.into_parts(&ctx).unwrap().into_parts();
    let mut ir = CadIr::empty();
    ir.model = model;
    ir.native = native;
    let report = report(&ir, &losses);
    (ir, fidelity, report)
}

#[test]
fn a_changed_retained_native_home_does_not_license_disjoint_neutral_reads() {
    let good = [b"H;".as_slice(), GOOD].concat();
    let bad = [b"H;".as_slice(), BAD].concat();
    let (before, previous, _) = aggregate_and_independent_point(&good);
    let (after, current, report) = aggregate_and_independent_point(&bad);
    let native_id = "step:native:header#aggregate";
    assert!(before.native.namespace("step").unwrap().arenas()["headers"]
        .iter()
        .any(|row| row.id() == native_id));
    assert!(after.native.namespace("step").unwrap().arenas()["headers"]
        .iter()
        .any(|row| row.id() == native_id));
    assert_eq!(
        previous.record_layout()[point(2).as_str()],
        current.record_layout()[point(2).as_str()]
    );
    let allowed = allowances(&before, &previous, &after, &current, &report, &bad).unwrap();
    let baseline = serde_json::to_value(&before).unwrap();
    let valid = serde_json::to_value(&after).unwrap();
    assert!(super::super::agree(&baseline, &valid, &allowed, super::super::Mode::Same).is_ok());
    let mut changed_ir = after.clone();
    changed_ir.model.points[0]
        .set_position(FinitePoint3::new(cadmpeg_ir::math::Point3::new(999.0, 0.0, 0.0)).unwrap());
    let changed = serde_json::to_value(changed_ir).unwrap();
    assert!(super::super::agree(&baseline, &changed, &allowed, super::super::Mode::Same).is_err());
    let mut removed = valid;
    removed["model"]["points"] = json!([]);
    assert!(super::super::agree(&baseline, &removed, &allowed, super::super::Mode::Same).is_err());
}

#[test]
fn scalar_references_inside_array_objects_need_the_target_but_direct_elements_only_list_it() {
    let (mut before, previous, _) = decode(GOOD);
    let (mut after, current, report) = decode(BAD);
    let arena = DecodeArena::new();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &DecodePolicy::service()).unwrap();
    before
        .native
        .namespace_mut("test")
        .set_arena(
            &ctx,
            "holders",
            &[
                json!({"id":"test:native:holder#object", "items":[{"target":point(1)}]}),
                json!({"id":"test:native:holder#nested", "items":[[{"target":point(1)}]]}),
                json!({"id":"test:native:holder#listed", "items":[point(1), point(2)]}),
            ],
        )
        .unwrap();
    after
        .native
        .namespace_mut("test")
        .set_arena(
            &ctx,
            "holders",
            &[json!({"id":"test:native:holder#listed", "items":[point(2)]})],
        )
        .unwrap();
    let allowed = allowances(&before, &previous, &after, &current, &report, BAD).unwrap();
    assert!(allowed.removed.contains("test:native:holder#object"));
    assert!(allowed.removed.contains("test:native:holder#nested"));
    assert!(!allowed.removed.contains("test:native:holder#listed"));
    super::super::agree(
        &serde_json::to_value(&before).unwrap(),
        &serde_json::to_value(&after).unwrap(),
        &allowed,
        super::super::Mode::Same,
    )
    .unwrap();
    after
        .native
        .namespace_mut("test")
        .set_arena::<serde_json::Value>(&ctx, "holders", &[])
        .unwrap();
    assert!(
        super::super::agree(
            &serde_json::to_value(&before).unwrap(),
            &serde_json::to_value(&after).unwrap(),
            &allowed,
            super::super::Mode::Same,
        )
        .is_err(),
        "a holder that only lists the lost reference must remain"
    );
}

#[test]
fn wrapped_typed_native_fallback_preserves_outside_fields_and_rejects_gains_and_dangling_refs() {
    let (mut before, after, allowances) = native_fallback_pair();
    let operation = before["model"]["features"][0]["definition"].clone();
    before["model"]["features"][0]["definition"] = json!({
        "definition":"post_process", "operation":operation, "refine":true,
        "fuzzy_tolerance":{"kind":"kernel_default"}
    });
    super::super::agree(&before, &after, &allowances, super::super::Mode::Same).unwrap();
    let mut outside = after.clone();
    outside["model"]["features"][0]["bonus"] = json!(true);
    assert!(super::super::agree(&before, &outside, &allowances, super::super::Mode::Same).is_err());
    let mut dangling = after.clone();
    dangling["model"]["features"][0]["definition"]["kind"] =
        before["model"]["parameters"][0]["id"].clone();
    assert!(
        super::super::agree(&before, &dangling, &allowances, super::super::Mode::Same).is_err()
    );
    let mut name = after.clone();
    name["model"]["features"][0]["name"] = json!("unrelated changed name");
    assert!(super::super::agree(&before, &name, &allowances, super::super::Mode::Same).is_err());
}
