// SPDX-License-Identifier: Apache-2.0

use crate::application::validators::validate_loaded;
use crate::loader::load_artifact;
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};
use cadmpeg_ir::codec::DecodeOptions;
use cadmpeg_ir::hash::digest::Sha256Digest;
use cadmpeg_ir::report::check::Check;
use cadmpeg_ir::report::Severity;
use cadmpeg_ir::{decode_sidecar_path, CadIr, DecodeSidecar, SourceFidelity};
use cadmpeg_registry::{ForcedInput, InputCatalog};

fn write_pair(
    path: &std::path::Path,
    ir: &CadIr,
    report: &cadmpeg_ir::report::decode::DecodeReport,
    fidelity: &SourceFidelity,
) {
    let json = ir.to_canonical_json().unwrap();
    let sidecar = DecodeSidecar::bind_sha256(
        Sha256Digest::digest(json.as_bytes()),
        report.clone(),
        fidelity.clone(),
    );
    std::fs::write(path, json).unwrap();
    std::fs::write(
        decode_sidecar_path(path).unwrap(),
        sidecar.to_canonical_json().unwrap(),
    )
    .unwrap();
}

#[test]
fn loaded_cadir_compares_native_metadata_with_its_rebound_sidecar() {
    let directory = tempfile::tempdir().unwrap();
    let native_path = directory.path().join("source.prt");
    std::fs::write(&native_path, PRT).unwrap();
    let catalog = InputCatalog::with_builtins();
    let decoded = load_artifact(&catalog, &native_path, DecodeOptions::default(), None).unwrap();
    let original_report = decoded.decode_report().unwrap();
    let fidelity = decoded.fidelity().unwrap();
    assert!(
        fidelity
            .retained_record("nx:container:parasolid#0")
            .unwrap()
            .offset()
            > 0
    );
    let path = directory.path().join("source.cadir.json");
    write_pair(&path, &decoded.ir, original_report, fidelity);
    let load = || {
        load_artifact(
            &catalog,
            &path,
            DecodeOptions::default(),
            Some(ForcedInput::Cadir),
        )
        .unwrap()
    };
    let restored = load();
    assert!(matches!(
        restored.origin,
        crate::application::document::LoadOrigin::Restored { .. }
    ));
    let report = validate_loaded(
        &catalog,
        &restored,
        &DecodePolicy::service(),
        super::super::WorkProfile::Service,
    )
    .unwrap();
    assert!(report.is_ok(), "{report:?}");
    assert!(report
        .findings
        .iter()
        .all(|finding| finding.check != Check::NativeLinks));

    let mut edited = restored.ir;
    let namespace = edited.native.namespace_mut("nx");
    let mut schemas = namespace
        .arena_as::<serde_json::Value>("parasolid_deltas_schemas")
        .unwrap();
    let schema = schemas
        .iter_mut()
        .find(|schema| schema["definition"]["kind"] == 240)
        .unwrap();
    assert_eq!(schema["custom_name"], "MY_KIND");
    schema["custom_name"] = serde_json::json!("EDITED_KIND");
    let arena = DecodeArena::new();
    let policy = DecodePolicy::service();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    namespace
        .set_arena(&ctx, "parasolid_deltas_schemas", &schemas)
        .unwrap();
    write_pair(&path, &edited, original_report, fidelity);
    let report = validate_loaded(
        &catalog,
        &load(),
        &policy,
        super::super::WorkProfile::Service,
    )
    .unwrap();
    assert!(
        report
            .findings
            .iter()
            .any(|finding| finding.check == Check::NativeLinks
                && finding.severity == Severity::Error
                && finding.message.contains("retained source")),
        "{report:?}"
    );

    std::fs::remove_file(decode_sidecar_path(&path).unwrap()).unwrap();
    let without_sidecar = load();
    assert!(without_sidecar.fidelity().is_none());
    let report = validate_loaded(
        &catalog,
        &without_sidecar,
        &policy,
        super::super::WorkProfile::Service,
    )
    .unwrap();
    let native = report
        .findings
        .iter()
        .filter(|finding| finding.check == Check::NativeLinks)
        .collect::<Vec<_>>();
    assert_eq!(native.len(), 1, "{native:?}");
    assert_eq!(native[0].severity, Severity::Info);
    assert_eq!(
        native[0].entity.as_deref(),
        Some("nx:container:parasolid#0")
    );
    assert!(native[0].message.contains("structural admission only"));
}

const PRT: &[u8] = &[
    0x53, 0x50, 0x4c, 0x4d, 0x53, 0x53, 0x54, 0x52, 0x06, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    0x00, 0xc6, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x48, 0x45, 0x41, 0x44, 0x45, 0x52, 0x01,
    0x00, 0x00, 0x00, 0x15, 0x00, 0x00, 0x00, 0x2f, 0x52, 0x6f, 0x6f, 0x74, 0x2f, 0x55, 0x47, 0x5f,
    0x50, 0x41, 0x52, 0x54, 0x2f, 0x55, 0x47, 0x5f, 0x50, 0x41, 0x52, 0x54, 0x4c, 0x00, 0x00, 0x00,
    0x00, 0x00, 0x00, 0x00, 0x7a, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x78, 0x9c, 0x0b, 0x08,
    0x66, 0x60, 0x60, 0x90, 0xb0, 0x52, 0x08, 0x09, 0x72, 0xf4, 0x0b, 0xf6, 0xf5, 0x0c, 0x51, 0x70,
    0xf3, 0xf4, 0x71, 0x55, 0xd0, 0x48, 0x49, 0xcd, 0x29, 0x49, 0x2c, 0xd6, 0x04, 0xca, 0x89, 0x07,
    0x3b, 0x7b, 0xc4, 0x1b, 0x9b, 0x1a, 0x18, 0x1a, 0x9a, 0x1b, 0x02, 0x69, 0x43, 0x03, 0xa3, 0x78,
    0x43, 0x63, 0x03, 0x03, 0x33, 0xc6, 0x2f, 0x0c, 0x20, 0xc0, 0xfc, 0x9f, 0x81, 0x09, 0x48, 0x00,
    0x01, 0x03, 0xcb, 0x7f, 0x06, 0x66, 0x06, 0x46, 0x28, 0xc4, 0x0a, 0x3e, 0x30, 0xb2, 0xfb, 0x46,
    0xc6, 0x7b, 0x7b, 0xfa, 0xb9, 0xb0, 0x25, 0x97, 0x16, 0x97, 0xe4, 0xe7, 0xb2, 0x96, 0x25, 0xe6,
    0x94, 0xa6, 0x02, 0x65, 0x18, 0x19, 0x53, 0x80, 0xe6, 0x30, 0x30, 0xb0, 0x23, 0x69, 0x87, 0x1a,
    0x05, 0x00, 0xba, 0x86, 0x1c, 0x7d, 0x46, 0x4f, 0x4f, 0x54, 0x45, 0x52, 0x00, 0x00, 0x00, 0x00,
    0x00, 0x00, 0x00, 0x00,
];
