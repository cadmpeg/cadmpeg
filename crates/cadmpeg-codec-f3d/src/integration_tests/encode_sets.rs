// SPDX-License-Identifier: Apache-2.0
//! An F3Z archive written to a format that stores one definition per file
//! (§3.4, Encode sets).

use std::collections::{BTreeMap, BTreeSet};
use std::io::{Cursor, Write};
use std::sync::Mutex;

use cadmpeg_core::decode::{DecodeContext, DecodeDepth};
use cadmpeg_core::text::NonBlankString;
use cadmpeg_core::CodecError;
use cadmpeg_ir::codec::write::target::DialectFree;
use cadmpeg_ir::codec::write::{
    ArenaCoverage, EncodeInput, Encoder, EncoderBackend, ExportBody, NativeDispositions,
    OutputNames, Outputs,
};
use cadmpeg_ir::codec::FormatId;
use cadmpeg_ir::ids::ProductDefinitionId;
use cadmpeg_ir::products::Storage;
use cadmpeg_ir::CadIr;

use super::decode;
use crate::test_support::assembly_test::{f3d_without_brep, f3z_archive, XREF_ROLE};
use crate::test_support::manifest_test::write_synthetic_manifests;
use crate::test_support::smbh_geometry_test::synthetic_geometry_with_history_smbh;
use crate::test_support::zip_test::f3d_with_smbh_and_protein_with_generated_base_feature;

/// A writer of a format that stores one definition per file. It keeps the
/// document of each output it plans.
#[derive(Default)]
struct PerFileRecorder(Mutex<Vec<CadIr>>);

impl EncoderBackend for PerFileRecorder {
    const FORMAT: FormatId = FormatId::new("cadir");
    const NAME: &'static str = "CADIR";
    const REQUEST: DecodeDepth = DecodeDepth::Full;
    const REUSES_SOURCE: bool = false;
    const DEFINITION_PER_FILE: bool = true;
    type Target = DialectFree;
    const TARGET: DialectFree = DialectFree;

    fn plan_resolved(
        &self,
        _: &DecodeContext<'_>,
        input: EncodeInput<'_>,
        (): (),
    ) -> Result<ExportBody, CodecError> {
        self.0.lock().unwrap().push(input.ir.clone());
        Ok(ExportBody::synthesized(
            Vec::new(),
            input.ir,
            ArenaCoverage::Complete,
            NativeDispositions::written(),
        ))
    }
}

/// A design whose one user parameter has no owner feature.
fn user_parameter_design() -> Vec<u8> {
    const TAIL_TYPE_GUID: &str = "B0556F58-EE09-46A7-B18F-E296F48E5B13";
    let target = 19u32;
    let mut frame = Vec::new();
    frame.extend_from_slice(&3u32.to_le_bytes());
    frame.extend_from_slice(b"372");
    frame.extend_from_slice(&901u32.to_le_bytes());
    frame.extend_from_slice(&[0; 15]);
    frame.extend_from_slice(&87u32.to_le_bytes());
    frame.push(0);
    crate::test_support::lp_utf16(&mut frame, "6.35 mm");
    frame.extend_from_slice(&[0, 0, 0, 1, 0]);
    crate::test_support::lp_utf16(&mut frame, "Linear Dimension-2");
    crate::test_support::lp_utf16(&mut frame, "mm");
    crate::test_support::lp_utf16(&mut frame, "d87");
    frame.extend_from_slice(&0.635f64.to_le_bytes());
    frame.extend_from_slice(&[0, 1]);
    frame.extend_from_slice(&target.to_le_bytes());
    frame.extend_from_slice(&[0; 6]);
    let metadata = crate::test_support::streams_test::design_metastream_with_records(
        &[(
            TAIL_TYPE_GUID,
            "A7AEA631-985B-4DD1-8CE2-DE2C-14B54081",
            3,
            "CommonData",
            &[u64::from(target)],
        )],
        &[],
    );
    let mut zip = zip::ZipWriter::new(Cursor::new(Vec::new()));
    let stored = crate::zip_write::file_options(zip::CompressionMethod::Stored);
    write_synthetic_manifests(&mut zip, stored);
    for (name, payload) in [("BulkStream.dat", frame), ("MetaStream.dat", metadata)] {
        zip.start_file(format!("FusionAssetName[Active]/Design1/{name}"), stored)
            .unwrap();
        zip.write_all(&payload).unwrap();
    }
    zip.finish().unwrap().into_inner()
}

/// The ids of the features, parameters, sketches, and bodies of `ir`.
fn record_ids(ir: &CadIr) -> BTreeSet<String> {
    let model = &ir.model;
    model
        .features
        .iter()
        .map(|row| row.id.as_str().to_owned())
        .chain(
            model
                .parameters
                .iter()
                .map(|row| row.id.as_str().to_owned()),
        )
        .chain(model.sketches.iter().map(|row| row.id.as_str().to_owned()))
        .chain(
            ir.geometry
                .bodies
                .iter()
                .map(|row| row.id.as_str().to_owned()),
        )
        .collect()
}

#[test]
fn each_output_of_an_archive_holds_the_records_its_definition_reaches() {
    let part = f3d_with_smbh_and_protein_with_generated_base_feature(
        &synthetic_geometry_with_history_smbh(),
    );
    let root = f3d_without_brep(
        "owner-root",
        "root.f3d",
        &[("part.f3d", XREF_ROLE), ("parameters.f3d", XREF_ROLE)],
    );
    let archive = f3z_archive(
        "root.f3d",
        &[
            ("root.f3d", &root),
            ("part.f3d", &part),
            ("parameters.f3d", &user_parameter_design()),
        ],
    );
    let decoded = decode(archive);
    let ir = decoded.ir();
    let root_definition = ir
        .model
        .product_definitions
        .iter()
        .find(|definition| definition.is_root())
        .map(|definition| definition.id.clone())
        .unwrap();
    let names: OutputNames = ir
        .model
        .product_definitions
        .iter()
        .filter(|definition| {
            definition.is_root()
                || matches!(
                    &definition.storage,
                    Storage::File {
                        source: Some(_),
                        ..
                    }
                )
        })
        .enumerate()
        .map(|(index, definition)| {
            (
                definition.id.clone(),
                NonBlankString::new(format!("output-{index}.out")).unwrap(),
            )
        })
        .collect();
    assert_eq!(names.len(), 3);
    let recorder = PerFileRecorder::default();
    let plan = recorder
        .plan(EncodeInput::new(ir, None).with_outputs(Outputs::Named(&names)))
        .unwrap();
    let documents = recorder.0.into_inner().unwrap();
    let outputs: BTreeMap<ProductDefinitionId, CadIr> = plan
        .outputs()
        .iter()
        .map(|output| output.definition.clone().unwrap())
        .zip(documents)
        .collect();
    assert_eq!(outputs.len(), 3, "{:#?}", plan.report().losses);

    let listed: BTreeSet<&str> = ir
        .model
        .product_definitions
        .iter()
        .flat_map(|definition| &definition.sketches)
        .map(cadmpeg_ir::SketchId::as_str)
        .collect();
    let mut placed: BTreeMap<String, Vec<&ProductDefinitionId>> = BTreeMap::new();
    for (definition, document) in &outputs {
        let is_root = *definition == root_definition;
        let model = &document.model;
        let features: BTreeMap<&str, Option<&ProductDefinitionId>> = model
            .features
            .iter()
            .map(|feature| (feature.id.as_str(), feature.product_definition.as_ref()))
            .collect();
        for (feature, owner) in &features {
            assert!(
                owner.map_or(is_root, |owner| owner == definition),
                "the output of {definition} holds feature {feature} of {owner:?}"
            );
        }
        for parameter in &model.parameters {
            let reached = match (&parameter.product_definition, &parameter.owner) {
                (Some(owner), _) => owner == definition,
                (None, Some(feature)) => features.contains_key(feature.as_str()),
                (None, None) => is_root,
            };
            assert!(
                reached,
                "the output of {definition} holds parameter {}",
                parameter.id
            );
        }
        let own = model
            .product_definitions
            .iter()
            .find(|stated| &stated.id == definition)
            .unwrap();
        for sketch in &model.sketches {
            let id = sketch.id.as_str();
            assert!(
                own.sketches.contains(&sketch.id) || (is_root && !listed.contains(id)),
                "the output of {definition} holds sketch {id}"
            );
        }
        for id in record_ids(document) {
            placed.entry(id).or_default().push(definition);
        }
    }
    for id in record_ids(ir) {
        assert_eq!(
            placed.get(&id).map_or(0, Vec::len),
            1,
            "record {id} is in one output: {:?}",
            placed.get(&id)
        );
    }
    let natives = |document: &CadIr| -> Vec<String> {
        document
            .native
            .namespace("f3d")
            .map(|namespace| {
                namespace
                    .arenas()
                    .values()
                    .flatten()
                    .map(|record| record.id().to_owned())
                    .collect()
            })
            .unwrap_or_default()
    };
    let mut written: BTreeMap<String, usize> = BTreeMap::new();
    for (definition, document) in &outputs {
        let key = definition.as_str().split_once('#').unwrap().1;
        for id in natives(document) {
            let scope = id.split(':').nth(1).unwrap();
            let qualifier = scope.split_once('@').map(|(qualifier, _)| qualifier);
            if *definition == root_definition {
                assert_eq!(qualifier, None, "the root output holds native record {id}");
            } else {
                assert_eq!(
                    qualifier,
                    Some(key),
                    "the output of {definition} holds native record {id}"
                );
            }
            *written.entry(id).or_default() += 1;
        }
    }
    for id in natives(ir) {
        assert_eq!(
            written.get(&id),
            Some(&1),
            "native record {id} is in one output"
        );
    }
}
