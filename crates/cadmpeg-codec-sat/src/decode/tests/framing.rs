// SPDX-License-Identifier: Apache-2.0
use std::collections::BTreeMap;

use cadmpeg_asm::brep::AsmBrep;
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};

use super::{text_sphere_stream, SatLossCode};

#[test]
fn framed_entity_presence_controls_the_no_carrier_event_without_unknown_records() {
    let bytes = text_sphere_stream(1.0);
    let arena = DecodeArena::new();
    let (ctx, _) =
        DecodeContext::from_root_bytes(&bytes, &arena, &DecodePolicy::service()).expect("source");
    let stream = cadmpeg_asm::sat::parse(&ctx, &bytes).expect("actual header");
    let header = stream.header.as_kernel_header();
    let evidence = super::super::StreamEvidence::Text(Some(super::super::TextEvidence {
        branch: stream.terminator,
        header: &header,
    }));
    let (matched, kernel) = super::super::layers(&evidence);
    for has_entities in [false, true] {
        let decoded = super::super::build_result(
            &ctx,
            Some(super::super::FramedStream {
                brep: AsmBrep::default(),
                has_entities,
            }),
            BTreeMap::new(),
            &header,
            Some(stream.terminator),
            matched.clone(),
            &kernel,
        )
        .expect("framed empty carrier graph");
        assert!(!decoded.body.transfer.geometry_transferred);
        assert_eq!(
            decoded
                .body
                .losses
                .iter()
                .filter(|loss| { loss.code == SatLossCode::GeometryFramedWithoutCarriers.kind() })
                .count(),
            usize::from(has_entities),
        );
        assert!(decoded.ir.scope.model);
        assert!(decoded.ir.scope.design);
        assert!(decoded.ir.native.namespace("sat").is_none_or(|native| {
            native
                .arenas()
                .get("unknowns")
                .is_none_or(|records| records.is_empty())
        }));
    }
}
