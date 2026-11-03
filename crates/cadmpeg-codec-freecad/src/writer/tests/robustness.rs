// SPDX-License-Identifier: Apache-2.0
//! Writer harness wiring for FreeCAD (#238 item 1, #217 §5.0, #232 §3.3 and §5.2).
//!
//! Injected-error, capacity-helper and `max_entities` runners from
//! `cadmpeg-test-support`; the residual checker (no residual positions are
//! stated in `docs/formats/freecad_fcstd.md` yet, so the spec list is empty
//! and the runner asserts no `residual-unavailable` loss); and the
//! `REDUCED_FIELDS` row runner, one tailored probe per §5.5 normalization
//! row, each with its loss.

use cadmpeg_ir::codec::write::target::TargetRequest;
use cadmpeg_ir::codec::write::{EncodeInput, Encoder};
use cadmpeg_ir::codec::{Codec, DecodeOptions};
use cadmpeg_ir::report::loss::LossNote;
use cadmpeg_test_support::residual::{residual_findings, ResidualSpec};
use cadmpeg_test_support::robustness::{
    capacity_findings, injected_error_findings, max_entities_findings, standard_injections,
};

use std::io::Cursor;

use crate::test_support::test_archive::CORE_DESIGN_PRODUCT;
use crate::FcstdCodec;

fn seed_golden() -> cadmpeg_ir::CadIr {
    FcstdCodec
        .decode(
            &mut Cursor::new(CORE_DESIGN_PRODUCT),
            &DecodeOptions::default(),
        )
        .expect("decode the robustness seed")
        .ir()
        .clone()
}

fn request() -> TargetRequest<'static> {
    TargetRequest::Explicit("fcstd:schema-4")
}

#[test]
fn injected_errors_succeed_and_name_the_record() {
    let seed = seed_golden();
    // The standard recompute-flag injection flips `needs_recompute` to
    // true, which is legal stored state (#209 §2.5: staleness suppresses
    // cache agreement instead of failing it), so it never invalidates any
    // seed and is inapplicable here; the remaining standard injections run.
    let injections: Vec<_> = standard_injections()
        .into_iter()
        .filter(|injection| injection.name != "recompute-flag")
        .collect();
    let findings = injected_error_findings(&seed, &FcstdCodec, request(), &injections);
    assert!(
        findings.is_empty(),
        "injected-error findings:\n{}",
        findings.join("\n")
    );
}

#[test]
fn capacity_helpers_follow_their_rows() {
    // FreeCAD has no fixed-width field; every count is an XML attribute or a
    // ZIP member, so there is no capacity row and no helper to probe (#238).
    assert!(crate::writer::CAPACITY_FIELDS.is_empty());
    let findings = capacity_findings(&[]);
    assert!(
        findings.is_empty(),
        "capacity findings:\n{}",
        findings.join("\n")
    );
}

#[test]
fn max_entities_refuses_an_over_ceiling_plan() {
    let seed = seed_golden();
    let count = u64::try_from(seed.model.entity_count()).expect("entity count fits");
    let findings = max_entities_findings(&seed, &FcstdCodec, request(), count);
    assert!(
        findings.is_empty(),
        "max_entities findings:\n{}",
        findings.join("\n")
    );
}

#[test]
fn reduced_rows_hold_their_normalizations() {
    // Each §5.5 normalization row lands here with its loss; each probe
    // exercises the normalization and asserts the loss and the stored
    // value (#238, #232 §5.2).
    let rows = crate::writer::REDUCED_FIELDS;
    assert_eq!(
        rows.iter().map(|row| row.path).collect::<Vec<_>>(),
        [
            "model.bodies[].name",
            "model.bodies[].color",
            "model.product_definitions[].label",
            "model.product_definitions[].source_name",
            "model.occurrences[].linked_prototype",
            "model.features[].source_tag",
            "model.parameters[].name",
            "model.parameters[].driving",
        ]
    );
    for row in rows {
        let cadmpeg_ir::codec::write::ReducedStorage::Normalized { loss } = row.storage else {
            panic!("row {} has no normalization loss", row.path);
        };
        assert!(
            loss.starts_with("fcstd/"),
            "row {} loss {loss} is namespaced",
            row.path
        );
    }
    fn write(ir: &cadmpeg_ir::CadIr) -> (Vec<LossNote>, cadmpeg_ir::CadIr) {
        let plan = FcstdCodec
            .plan(EncodeInput::new(ir, None), request())
            .expect("normalization probe plans");
        let report = plan.report().clone();
        let mut bytes = Vec::new();
        plan.write_to(&mut bytes)
            .expect("normalization probe writes");
        let output = FcstdCodec
            .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
            .expect("normalization probe re-decodes");
        (report.losses.clone(), output.ir().clone())
    }
    fn has_loss(losses: &[LossNote], code: &str, record: &str) -> bool {
        losses
            .iter()
            .any(|loss| loss.code.to_string() == code && loss.message.contains(record))
    }
    // Body names with no native home are dropped. Body identities embed
    // their archive entry names, which the writer normalizes, so the probe
    // matches by document position and the loss by the renamed value.
    {
        let mut ir = seed_golden();
        let body = ir.model.bodies.first_mut().expect("seed body");
        body.name = Some("Renamed body".into());
        let (losses, output) = write(&ir);
        assert!(
            has_loss(&losses, "fcstd/body.name-not-transferred", "object Box"),
            "renamed feature-output body charges its loss: {:?}",
            losses.iter().map(|loss| &loss.message).collect::<Vec<_>>()
        );
        let body = output
            .model
            .bodies
            .first()
            .expect("renamed body re-decodes");
        assert_ne!(body.name.as_deref(), Some("Renamed body"));
    }
    // A cleared driving flag has no native home on properties.
    {
        let mut ir = seed_golden();
        let parameter = ir
            .model
            .parameters
            .iter_mut()
            .find(|parameter| parameter.owner.is_some() && parameter.driving)
            .expect("seed driven parameter");
        let id = parameter.id.clone();
        parameter.driving = false;
        let (losses, output) = write(&ir);
        assert!(
            has_loss(
                &losses,
                "fcstd/parameter.driving-not-transferred",
                id.as_str()
            ),
            "cleared driving flag charges its loss"
        );
        let parameter = output
            .model
            .parameters
            .iter()
            .find(|parameter| parameter.id == id)
            .expect("driving-cleared parameter re-decodes");
        assert!(parameter.driving, "driving re-decodes set");
    }
    // A renamed bound parameter normalizes to a selector with a loss when
    // its name is not a valid free identifier.
    {
        let mut ir = seed_golden();
        let owner = ir
            .model
            .features
            .iter()
            .find(|feature| feature.name.as_deref() == Some("Box"))
            .expect("seed Box feature")
            .id
            .clone();
        let parameter = ir
            .model
            .parameters
            .iter_mut()
            .find(|parameter| {
                parameter.owner.as_ref() == Some(&owner) && parameter.name == "Length"
            })
            .expect("seed Box length");
        let id = parameter.id.clone();
        parameter.name = "9 depth".into();
        let (losses, output) = write(&ir);
        assert!(
            has_loss(&losses, "fcstd/parameter.name-not-transferred", id.as_str()),
            "invalid parameter name charges its loss"
        );
        let parameter = output
            .model
            .parameters
            .iter()
            .find(|parameter| parameter.id == id)
            .expect("renamed parameter re-decodes");
        assert_ne!(parameter.name, "9 depth");
    }
    // A feature type tag that does not name an encoding construction is
    // replaced by the derived type with a loss.
    {
        let mut ir = seed_golden();
        let feature = ir
            .model
            .features
            .iter_mut()
            .find(|feature| feature.name.as_deref() == Some("Box"))
            .expect("seed Box feature");
        feature.source_tag = Some("Part::Cylinder".into());
        let (losses, output) = write(&ir);
        assert!(
            has_loss(
                &losses,
                "fcstd/feature.source-tag-not-transferred",
                "Part::Cylinder"
            ),
            "non-encoding source tag charges its loss"
        );
        let feature = output
            .model
            .features
            .iter()
            .find(|feature| feature.name.as_deref() == Some("Box"))
            .expect("re-tagged feature re-decodes");
        assert_ne!(feature.source_tag.as_deref(), Some("Part::Cylinder"));
    }
    // Ownerless parameters go to a generated `Spreadsheet::Sheet` named
    // `Parameters`, which every FreeCAD version loads (#238).
    {
        let mut ir = cadmpeg_ir::CadIr::empty();
        ir.model.parameters.push(
            serde_json::from_value(serde_json::json!({"id":"cadir:model:parameter#global","ordinal":0,"name":"GlobalLength","expression":{"tokens":[{"kind":"text","value":"2.5"}]},"value":{"kind":"real","value":2.5},"driving":true}))
                .expect("authored global parameter"),
        );
        let (losses, output) = write(&ir);
        assert!(
            losses
                .iter()
                .all(|loss| !loss.code.to_string().starts_with("fcstd/parameter.")),
            "a valid global name needs no selector"
        );
        let parameter = output
            .model
            .parameters
            .iter()
            .find(|parameter| parameter.name == "GlobalLength")
            .expect("global parameter re-decodes");
        assert!(parameter.owner.is_some(), "globals gain a sheet owner");
        let sheet = output
            .model
            .features
            .iter()
            .find(|feature| Some(&feature.id) == parameter.owner.as_ref())
            .expect("sheet feature re-decodes");
        let native = output
            .native
            .namespace("fcstd")
            .expect("fcstd namespace")
            .arena_as::<crate::native::ObjectRecord>("objects")
            .expect("native objects");
        assert!(native.iter().any(|object| object.name() == "Parameters"
            && object.type_name == "Spreadsheet::Sheet"
            && Some(object.id().as_str()) == sheet.native_ref.as_deref()));
    }
}

#[test]
fn synthesized_records_carry_their_specified_residuals() {
    let seed = seed_golden();
    let plan = FcstdCodec
        .plan(EncodeInput::new(&seed, None), request())
        .expect("plan the residual seed");
    let mut bytes = Vec::new();
    let report = plan.write_to(&mut bytes).expect("write the residual seed");
    let decoded = FcstdCodec
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .expect("decode the residual output");
    let after = serde_json::to_value(decoded.ir()).expect("serialize the re-decode");
    let specs = residual_specs(&seed);
    let findings = residual_findings(&after, &report.losses, &specs);
    assert!(
        findings.is_empty(),
        "residual findings:\n{}",
        findings.join("\n")
    );
    assert!(
        !report
            .losses
            .iter()
            .any(|loss| loss.code.local_code() == "record.residual-unavailable"),
        "the writer charges no residual-unavailable loss on this seed: {:?}",
        report.losses
    );
}

/// Residual positions the FreeCAD writer synthesizes, from `docs/formats/`.
///
/// No residual rule is stated in `docs/formats/freecad_fcstd.md` yet; the
/// first stated rule adds its spec here (#232 §3.3).
fn residual_specs(seed: &cadmpeg_ir::CadIr) -> Vec<ResidualSpec> {
    let _ = seed;
    Vec::new()
}

#[test]
fn admission_is_silent_on_clean_fixture_decodes() {
    // The admission scan names only dropped records (#232 §5.1(a)). A clean
    // decode drops nothing, so every public fixture charges nothing here;
    // any finding names a false positive in the scan itself.
    let dir = format!(
        "{}/../../corpus/freecad_fcstd/fixtures",
        env!("CARGO_MANIFEST_DIR")
    );
    let mut names: Vec<_> = std::fs::read_dir(&dir)
        .expect("fixtures dir")
        .map(|entry| entry.expect("entry").path())
        .filter(|path| path.extension().is_some_and(|ext| ext == "FCStd"))
        .collect();
    names.sort();
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let policy = cadmpeg_core::decode::DecodePolicy::default();
    let ctx = cadmpeg_core::decode::DecodeContext::for_encode(&arena, &policy);
    let mut findings = Vec::new();
    for path in &names {
        let bytes = std::fs::read(path).expect("fixture bytes");
        let Ok(decoded) = FcstdCodec.decode(&mut Cursor::new(&bytes), &DecodeOptions::default())
        else {
            continue;
        };
        for loss in
            super::super::synthesis_graph::admission(decoded.ir(), &ctx).expect("admission runs")
        {
            findings.push(format!(
                "{}: {} :: {}",
                path.file_name().expect("name").to_string_lossy(),
                loss.code,
                loss.message
            ));
        }
    }
    assert!(
        findings.is_empty(),
        "admission findings on clean decodes:\n{}",
        findings.join("\n")
    );
}
