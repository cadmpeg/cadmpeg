// SPDX-License-Identifier: Apache-2.0
//! Shared source omissions use finalized spans, not native stub inventories.

use super::{CadirEncoder, CatalogEncoder, EncodeInput, Encoder, TargetRequest};
use crate::source_fidelity::b1::{classification::ClassificationIndex, partition_stream};
use crate::{CadIr, SourceFidelity};
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};

const OWNER: &str = "test:model:point#source";

fn source(bytes: &[u8]) -> (CadIr, SourceFidelity) {
    let arena = DecodeArena::new();
    let (ctx, root) = DecodeContext::from_root_bytes(bytes, &arena, &DecodePolicy::service())
        .expect("registered actual source");
    let point = ctx
        .record(OWNER, root, || {
            let coordinate = root.read_at(0, 1).expect("source scalar")[0];
            Ok(crate::topology::Point::new(
                OWNER.try_into().expect("point identity"),
                crate::features::FinitePoint3::new(crate::math::Point3::new(
                    f64::from(coordinate),
                    0.0,
                    0.0,
                ))
                .expect("finite source coordinate"),
                None,
            ))
        })
        .expect("source point");
    let mut ir = CadIr::empty();
    ir.model.points.push(point);
    let mut fidelity = SourceFidelity::default();
    fidelity
        .install_record_captures(&ctx, |_, _| Ok(()))
        .expect("actual source capture");
    let partition = partition_stream(
        &ctx,
        "file",
        u64::try_from(bytes.len()).expect("source length"),
        fidelity.record_layout(),
        &[],
        &[],
        &[],
    )
    .expect("actual read partition");
    let index = ClassificationIndex::new(&ctx, &ir).expect("actual point ownership");
    fidelity
        .finalize_b1(&ctx, &index, "test", &[partition], &mut Vec::new())
        .expect("validated B1 inventory");
    (ir, fidelity)
}

#[test]
fn native_synthesis_names_every_finalized_undecoded_span_and_its_actual_record() {
    let (ir, fidelity) = source(&[1, 2, 3, 4]);
    let spans = fidelity.b1_spans()["file"].spans();
    assert_eq!(spans.len(), 1);
    assert_eq!(spans[0].range(), 1..4);
    assert_eq!(
        spans[0].classes(),
        crate::ArenaClasses::from([crate::ArenaClass::Model])
    );
    assert!(fidelity.retained_records().is_empty());
    let plan = CatalogEncoder
        .plan(
            EncodeInput::new(&ir, Some(&fidelity)),
            TargetRequest::Inherit,
        )
        .expect("source-independent synthesis");
    let losses = &plan.report().losses;
    assert_eq!(losses.len(), 1);
    assert_eq!(losses[0].code.namespace(), "export");
    assert_eq!(losses[0].code.local_code(), "source.undecoded-omitted");
    assert_eq!(losses[0].severity, crate::report::Severity::Warning);
    assert!(losses[0]
        .message
        .contains("stream=file offset=1 length=3 classes=model"));
    assert_eq!(
        losses[0].message.rsplit_once("record(s): ").unwrap().1,
        OWNER
    );
    assert_eq!(ir.model.points.len(), 1);
}

#[test]
fn cadir_and_absent_or_fully_consumed_source_do_not_invent_source_omissions() {
    let (ir, fidelity) = source(&[1, 2, 3, 4]);
    let plan = CadirEncoder
        .plan(
            EncodeInput::new(&ir, Some(&fidelity)),
            TargetRequest::Inherit,
        )
        .expect("lossless CADIR writer");
    assert!(plan.report().losses.is_empty());
    let mut bytes = Vec::new();
    plan.write_to(&mut bytes).expect("write current document");
    let reopened = CadIr::from_json(std::str::from_utf8(&bytes).unwrap()).unwrap();
    assert_eq!(reopened.model.points, ir.model.points);
    let plan = CatalogEncoder
        .plan(EncodeInput::new(&ir, None), TargetRequest::Inherit)
        .expect("no source inventory");
    assert!(plan.report().losses.is_empty());
    let (ir, fidelity) = source(&[1]);
    assert!(fidelity.b1_spans()["file"].spans().is_empty());
    let plan = CatalogEncoder
        .plan(
            EncodeInput::new(&ir, Some(&fidelity)),
            TargetRequest::Inherit,
        )
        .expect("fully consumed real source");
    assert!(plan.report().losses.is_empty());
}
