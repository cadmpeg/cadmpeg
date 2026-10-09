// SPDX-License-Identifier: Apache-2.0
//! First visited identity records are admitted before lookup or allocation.

use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
use cadmpeg_core::CodecError;

use crate::loss::Diagnostics;
use crate::objects::ObjectRecord;
use crate::settings::{DocumentMetadata, LayerRecord, SourceRange};

fn assert_first_visit(objects: Vec<ObjectRecord<()>>, metadata: &DocumentMetadata) {
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = 0;
    policy.limits.max_collection_items = 0;
    policy.limits.max_materialized_bytes = 0;
    policy.limits.max_retained_bytes = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(b"", &arena, &policy).unwrap();
    let mut warnings = Diagnostics::new();
    let error = crate::objects::resolve_identities(&ctx, objects, metadata, &mut warnings)
        .expect_err("only the first identity visit is refused");
    let CodecError::ResourceLimit(refusal) = error else { panic!("identity visit refusal"); };
    assert_eq!(refusal.dimension, ResourceDimension::WorkUnits);
    assert_eq!(refusal.operation, "Rhino resolve identities traversal");
    assert_eq!((refusal.used, refusal.additional), (0, 1));
    assert!(warnings.is_empty());
    assert_eq!(ctx.resource_refusal(), Some(refusal));
    assert!(matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(sticky)) if sticky == refusal));
}

#[test]
fn identity_object_traversal_refuses_only_first_visit() {
    let objects = vec![ObjectRecord::Degraded { range: 0..0, warning: String::new() }; 1024];
    assert_first_visit(objects, &DocumentMetadata::default());
}

#[test]
fn identity_layer_traversal_refuses_only_first_visit() {
    let layer = LayerRecord {
        source: SourceRange { range: 0..0 },
        index: 0,
        iges_level: None,
        render_material_index: -1,
        color: [0; 4],
        name: String::new(),
        description: None,
        visible: true,
        locked: false,
        id: None,
        hierarchy: None,
        linetype_index: None,
        plot: None,
        display_material_id: None,
        no_clipping_planes: None,
        visible_in_new_details: None,
        rendering_range: None,
        extension_items: Vec::new(),
        embedded_linetype: None,
        embedded_section_style: None,
        per_viewport_settings: Vec::new(),
    };
    let metadata = DocumentMetadata { layers: vec![layer; 1024], ..DocumentMetadata::default() };
    assert_first_visit(Vec::new(), &metadata);
}
