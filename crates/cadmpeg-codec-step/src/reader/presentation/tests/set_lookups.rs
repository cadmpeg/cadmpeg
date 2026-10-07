// SPDX-License-Identifier: Apache-2.0

use std::collections::{BTreeMap, BTreeSet};

use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
use cadmpeg_core::CodecError;

use super::super::{presentation_item_one, EntityIds, PresentationIndices};

#[test]
fn pmi_presentation_predicate_preserves_lookup_refusal() {
    let source = b"ISO-10303-21;HEADER;FILE_DESCRIPTION(('test'),'2;1');FILE_NAME('','',(''),(''),'','','');FILE_SCHEMA(('AP242'));ENDSEC;DATA;#1=DATUM();ENDSEC;END-ISO-10303-21;";
    let (exchange, _) =
        crate::test_support::with_service_context(source, crate::parse::parse_inner)
            .expect("datum record exchange");
    let products = BTreeMap::new();
    let entity_ids = EntityIds {
        edges: BTreeSet::new(),
        vertices: BTreeSet::new(),
        points: BTreeSet::new(),
        curves: BTreeSet::new(),
        surfaces: BTreeSet::new(),
        products: &products,
        occurrences: BTreeSet::new(),
        pmi: BTreeSet::from(["step:presentation:pmi#1"]),
        tessellations: BTreeSet::new(),
    };
    let faces = BTreeMap::new();
    let bodies = BTreeMap::new();
    let indices = PresentationIndices {
        faces: &faces,
        bodies: &bodies,
    };
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    // The occurrence test visits the one partial and probes the end; the PMI
    // test visits it again; the PMI key lookup then refuses.
    policy.limits.max_work_units = 3;
    let (ctx, _) = DecodeContext::from_root_bytes(source, &arena, &policy).unwrap();
    let error = presentation_item_one(1, &exchange, &entity_ids, indices, &ctx).unwrap_err();
    let CodecError::ResourceLimit(refusal) = error else {
        panic!("PMI selection must preserve its lookup resource refusal");
    };
    assert_eq!(refusal.dimension, ResourceDimension::WorkUnits);
    assert_eq!(refusal.operation, "STEP pmi membership");
    assert_eq!(ctx.resource_refusal(), Some(refusal));
}
