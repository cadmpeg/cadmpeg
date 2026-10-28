// SPDX-License-Identifier: Apache-2.0
use crate::test_support::container::{make_block, native_envelope_fixture, sldprt_with_body};
use crate::test_support::parasolid::{parasolid_with_body, triangle_body};
use crate::SldprtCodec;
use cadmpeg_ir::codec::{Codec, DecodeOptions};
use cadmpeg_test_support::EditableDecodeResult;
use std::io::Cursor;

fn auxiliary(bad: bool) -> Vec<u8> {
    let mut xt = parasolid_with_body("auxiliary XT", "SCH_2900195_28101_13006", &[0, 1, 0, 1]);
    if bad {
        *xt.last_mut().unwrap() = 2;
    }
    let mut value = 1u32.to_le_bytes().to_vec();
    value.extend_from_slice(&u32::try_from(xt.len()).unwrap().to_le_bytes());
    value.extend(xt);
    value
}

fn source(aux: Option<&[u8]>) -> Vec<u8> {
    let mut bytes = sldprt_with_body(&triangle_body());
    if let Some(payload) = aux {
        bytes.extend(make_block(0x43, "ThirdPty/WELDMENT_STORAGE", payload));
    }
    native_envelope_fixture(&bytes)
}

#[test]
fn malformed_optional_weldment_keeps_the_primary_model_and_whole_source() {
    let baseline_bytes = source(None);
    let bytes = source(Some(&auxiliary(true)));
    if let Ok(folder) = std::env::var("SW05_FIXTURE_EXPORT") {
        std::fs::write(
            std::path::Path::new(&folder).join("valid-primary.sldprt"),
            &baseline_bytes,
        )
        .unwrap();
        std::fs::write(
            std::path::Path::new(&folder).join("valid-primary-malformed-aux.sldprt"),
            &bytes,
        )
        .unwrap();
    }
    let baseline = EditableDecodeResult::from(
        SldprtCodec
            .decode(&mut Cursor::new(baseline_bytes), &DecodeOptions::default())
            .unwrap(),
    );
    let decoded = EditableDecodeResult::from(
        SldprtCodec
            .decode(&mut Cursor::new(bytes.clone()), &DecodeOptions::default())
            .unwrap(),
    );
    assert!(!baseline.ir().model.faces.is_empty());
    assert_eq!(decoded.ir().model, baseline.ir().model);
    let code = crate::loss::SldprtLossCode::ContainerAuxiliaryParasolidRefused.kind();
    assert_eq!(
        decoded
            .report()
            .losses
            .iter()
            .filter(|loss| loss.code == code)
            .count(),
        1
    );
    let records = crate::source_records(decoded.ir(), decoded.source_fidelity()).unwrap();
    assert_eq!(
        records
            .iter()
            .find(|r| r.id.as_str() == "sldprt:file:source-image#0")
            .unwrap()
            .data,
        Some(bytes.as_slice())
    );
}

#[test]
fn valid_optional_weldment_keeps_the_primary_model_without_a_refusal() {
    let baseline = EditableDecodeResult::from(
        SldprtCodec
            .decode(&mut Cursor::new(source(None)), &DecodeOptions::default())
            .unwrap(),
    );
    let decoded = EditableDecodeResult::from(
        SldprtCodec
            .decode(
                &mut Cursor::new(source(Some(&auxiliary(false)))),
                &DecodeOptions::default(),
            )
            .unwrap(),
    );
    assert_eq!(decoded.ir().model, baseline.ir().model);
    assert!(!decoded
        .report()
        .losses
        .iter()
        .any(|loss| loss.code
            == crate::loss::SldprtLossCode::ContainerAuxiliaryParasolidRefused.kind()));
}

#[test]
fn refused_auxiliary_owner_does_not_resurrect_a_later_generic_wrapper() {
    let mut payload = auxiliary(true);
    let xt = parasolid_with_body("auxiliary XT", "SCH_2900195_28101_13006", &triangle_body());
    payload.extend(crate::test_support::parasolid::legacy_probe_payload(&xt, 0));
    let bytes = source(Some(&payload));
    let scan = crate::test_support::container::scan(&bytes);
    let owner = scan
        .sections()
        .find(|s| s.name() == Some("ThirdPty/WELDMENT_STORAGE"))
        .unwrap();
    assert!(owner.ps_auxiliary_refusal().is_some());
    assert!(owner.ps_streams().is_empty());
}

#[test]
fn malformed_auxiliary_owner_blocks_semantic_writes() {
    let decoded = EditableDecodeResult::from(
        SldprtCodec
            .decode(
                &mut Cursor::new(source(Some(&auxiliary(true)))),
                &DecodeOptions::default(),
            )
            .unwrap(),
    );
    let records = crate::source_records(decoded.ir(), decoded.source_fidelity()).unwrap();
    let mut output = Vec::new();
    let result = crate::writer::write_semantic_with_records(
        decoded.ir(),
        &decoded.source_fidelity().annotations,
        &records,
        &mut output,
    );
    assert!(
        matches!(result, Err(cadmpeg_core::CodecError::NotImplemented(message)) if message.contains("auxiliary Parasolid"))
    );
    assert!(output.is_empty());
}

#[test]
fn malformed_auxiliary_owner_still_allows_exact_native_passthrough() {
    let bytes = source(Some(&auxiliary(true)));
    let decoded = EditableDecodeResult::from(
        SldprtCodec
            .decode(&mut Cursor::new(bytes.clone()), &DecodeOptions::default())
            .unwrap(),
    );
    let mut output = Vec::new();
    crate::test_support::plan_inherited_write(decoded.ir(), decoded.source_fidelity(), &mut output)
        .unwrap();
    assert_eq!(output, bytes);
}
