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
