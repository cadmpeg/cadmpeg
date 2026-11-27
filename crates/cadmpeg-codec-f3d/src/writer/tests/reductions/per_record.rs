// SPDX-License-Identifier: Apache-2.0
//! A record the generator cannot write as it stands is degraded or renamed
//! with a loss naming it, and the rest of the document is written.

use cadmpeg_core::serde_erase::json;
use std::collections::BTreeMap;
use std::io::Cursor;

use cadmpeg_ir::codec::{Codec, DecodeOptions};
use cadmpeg_ir::document::CadIr;
use cadmpeg_ir::geometry::curve_payloads::SubsetCurveConstruction;
use cadmpeg_ir::geometry::procedural_curves::ProceduralCurveDefinition;
use cadmpeg_ir::geometry::ProceduralCurve;

use crate::loss::F3dLossCode;
use crate::records::configuration::DesignConfiguration;
use crate::test_support::configuration_table::project;
use crate::test_support::native_test::{f3d_native, update_f3d_native};
use crate::test_support::smbh_curves_test::synthetic_geometry_with_subset_curve_smbh;
use crate::test_support::smbh_geometry_test::synthetic_geometry_smbh;
use crate::test_support::zip_test::f3d_with_configuration;
use crate::F3dCodec;

fn is_subset(procedural: &ProceduralCurve) -> bool {
    matches!(
        procedural.definition(),
        ProceduralCurveDefinition::Subset(_)
    )
}

#[test]
fn a_subset_that_cannot_be_restated_on_its_source_is_written_as_its_cache() {
    let mut ir = super::source_less(&synthetic_geometry_with_subset_curve_smbh());
    let index = ir
        .geometry
        .procedural_curves
        .iter()
        .position(is_subset)
        .expect("a subset construction");
    let construction = ir.geometry.procedural_curves[index].id.clone();
    let carrier = ir
        .geometry
        .procedural_curve_owner(&construction)
        .expect("the subset's carrier")
        .clone();
    let ProceduralCurveDefinition::Subset(subset) =
        ir.geometry.procedural_curves[index].definition()
    else {
        panic!("a subset construction");
    };
    let cyclic = SubsetCurveConstruction::try_new(
        carrier.clone(),
        subset.parameter_range().endpoints(),
        *subset.sense(),
        subset.legacy_cache(),
    )
    .expect("a subset of its own carrier");
    ir.geometry.procedural_curves[index] = ProceduralCurve::new(
        construction.clone(),
        ProceduralCurveDefinition::Subset(cyclic),
    );

    let (report, decoded) = super::write(&ir);
    let reduced = report
        .losses
        .iter()
        .find(|loss| {
            loss.code == F3dLossCode::WriterProceduralReduced.kind()
                && loss.message.contains(construction.as_str())
        })
        .unwrap_or_else(|| panic!("{:#?}", report.losses));
    assert!(
        reduced.message.contains(carrier.as_str()),
        "{}",
        reduced.message
    );
    assert!(!decoded.geometry.procedural_curves.iter().any(is_subset));
    assert_eq!(decoded.geometry.bodies.len(), ir.geometry.bodies.len());
    assert_eq!(decoded.geometry.edges.len(), ir.geometry.edges.len());

    let cache = ir
        .geometry
        .curves
        .iter()
        .find(|curve| curve.id == carrier)
        .and_then(|curve| curve.geometry.solved_cache())
        .expect("the carrier's cache");
    let edge = ir
        .geometry
        .edges
        .iter()
        .position(|edge| edge.curve() == Some(&carrier))
        .expect("an edge on the carrier");
    let written_curve = decoded.geometry.edges[edge]
        .curve()
        .expect("the written edge's curve");
    let written = decoded
        .geometry
        .curves
        .iter()
        .find(|curve| &curve.id == written_curve)
        .and_then(|curve| curve.geometry.solved())
        .expect("the written carrier is solved geometry");
    let [written, cache] = [written, cache]
        .map(|geometry| json::to_value(geometry).expect("curve geometry serializes"));
    assert!(agree(&written, &cache), "{written:#}\n{cache:#}");
}

/// The largest difference between a written number and the number it
/// states.
const EPS_WRITTEN_VALUE: f64 = 1.0e-9;

/// Whether `written` has the shape of `stated` and every number in it is
/// within [`EPS_WRITTEN_VALUE`] of the number it states.
fn agree(written: &serde_json::Value, stated: &serde_json::Value) -> bool {
    use serde_json::Value;
    match (written, stated) {
        (Value::Number(written), Value::Number(stated)) => written
            .as_f64()
            .zip(stated.as_f64())
            .is_some_and(|(written, stated)| (written - stated).abs() <= EPS_WRITTEN_VALUE),
        (Value::Array(written), Value::Array(stated)) => {
            written.len() == stated.len()
                && written
                    .iter()
                    .zip(stated)
                    .all(|(written, stated)| agree(written, stated))
        }
        (Value::Object(written), Value::Object(stated)) => {
            written.len() == stated.len()
                && written.iter().all(|(key, written)| {
                    stated.get(key).is_some_and(|stated| agree(written, stated))
                })
        }
        _ => written == stated,
    }
}

/// The source-less document of a design holding the configuration rule
/// `name`.
fn with_configuration_rule(name: &str) -> CadIr {
    let decoded = F3dCodec
        .decode(
            &mut Cursor::new(f3d_with_configuration(
                &synthetic_geometry_smbh(),
                name,
                br#"{"when":"width > 20 mm","activate":"wide"}"#,
            )),
            &DecodeOptions::default(),
        )
        .expect("a configuration rule decodes");
    let (mut ir, _, _) = decoded.into_parts();
    ir.source = None;
    ir.set_native_unknowns(
        &cadmpeg_core::decode::test_support::service_decode_context(),
        "f3d",
        &[],
    )
    .expect("no native unknowns");
    ir
}

#[test]
fn a_configuration_entry_name_outside_the_design_folder_is_renamed_into_it() {
    let stored = "FusionAssetName[Active]/DesignConfigurationTable.BlobParts/DesignConfigurationRule.456.dsgcfgrule";
    let outside = "../outside/DesignConfigurationRule.456.dsgcfgrule";
    let mut ir = with_configuration_rule(stored);
    let payload = update_f3d_native(&mut ir, |native| {
        let configuration = &mut native.design_configurations[0];
        let payload = configuration.payload().clone();
        *configuration =
            DesignConfiguration::try_new(outside.into(), configuration.kind(), payload.clone())
                .expect("a configuration rule name");
        payload
    });
    ir.model.configurations = project(&ir);

    let (report, decoded) = super::write(&ir);
    let native = f3d_native(&decoded);
    assert_eq!(native.design_configurations.len(), 1);
    assert_eq!(native.design_configurations[0].entry_name(), stored);
    assert_eq!(native.design_configurations[0].payload(), &payload);
    assert!(
        report
            .losses
            .iter()
            .any(|loss| loss.code.to_string() == "export/value.unwritable"
                && loss.message.contains(outside)
                && loss.message.contains(stored)),
        "{:#?}",
        report.losses
    );
}

#[test]
fn a_repeated_configuration_entry_name_is_renamed() {
    let stored = "FusionAssetName[Active]/DesignConfigurationTable.BlobParts/DesignConfigurationRule.456.dsgcfgrule";
    let renamed = "FusionAssetName[Active]/DesignConfigurationTable.BlobParts/DesignConfigurationRule.1.dsgcfgrule";
    let mut ir = with_configuration_rule(stored);
    let payloads = update_f3d_native(&mut ir, |native| {
        let first = native.design_configurations[0].clone();
        let mut payload = first.payload().clone();
        payload.insert("activate".into(), "narrow".into());
        let repeat = DesignConfiguration::try_new(stored.into(), first.kind(), payload)
            .expect("a configuration rule name");
        native.design_configurations.push(repeat);
        native
            .design_configurations
            .iter()
            .map(|configuration| configuration.payload().clone())
            .collect::<Vec<_>>()
    });
    ir.model.configurations = project(&ir);

    let (report, decoded) = super::write(&ir);
    let written = f3d_native(&decoded)
        .design_configurations
        .iter()
        .map(|configuration| {
            (
                configuration.entry_name().clone(),
                configuration.payload().clone(),
            )
        })
        .collect::<BTreeMap<_, _>>();
    assert_eq!(
        written,
        BTreeMap::from([
            (stored.to_owned(), payloads[0].clone()),
            (renamed.to_owned(), payloads[1].clone()),
        ])
    );
    assert_eq!(decoded.model.configurations, project(&decoded));
    let charged = report
        .losses
        .iter()
        .filter(|loss| loss.code.to_string() == "export/value.unwritable")
        .collect::<Vec<_>>();
    assert_eq!(charged.len(), 1, "{:#?}", report.losses);
    assert!(
        charged[0].message.contains("repeated") && charged[0].message.contains(renamed),
        "{}",
        charged[0].message
    );
}
