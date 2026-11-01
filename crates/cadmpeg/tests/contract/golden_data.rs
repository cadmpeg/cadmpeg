// SPDX-License-Identifier: Apache-2.0
//! Data checks 5, 6 and 7 (#246 IV.2) over every committed decode golden.
//!
//! `FreeCAD` decode goldens pin native arenas by digest, so its checks
//! decode the corpus fixtures those goldens snapshot.

use cadmpeg_test_support::data_checks::properties::property_map_findings;
use cadmpeg_test_support::data_checks::{
    decode_goldens, equation_identities, expression_text_findings, feature_ordinal_findings,
    freecad_stale_findings, never_stale_findings, solidworks_stale_findings, DecodeGolden,
};
use serde_json::Value;

fn assert_documents_clean(
    label: &str,
    documents: &[DecodeGolden],
    check: impl Fn(&DecodeGolden) -> Vec<String>,
) {
    assert!(!documents.is_empty(), "{label} has no decoded documents");
    let findings: Vec<String> = documents
        .iter()
        .flat_map(|golden| {
            check(golden)
                .into_iter()
                .map(move |finding| format!("{}: {finding}", golden.path))
        })
        .collect();
    assert!(
        findings.is_empty(),
        "{} finding(s):\n{}",
        findings.len(),
        findings.join("\n")
    );
}

fn assert_clean(crate_name: &str, check: impl Fn(&Value) -> Vec<String>) {
    let goldens = super::sampled_goldens(decode_goldens(crate_name));
    assert!(!goldens.is_empty(), "{crate_name} has no decode goldens");
    let findings: Vec<String> = goldens
        .iter()
        .flat_map(|golden| {
            check(&golden.ir)
                .into_iter()
                .map(move |finding| format!("{}: {finding}", golden.path))
        })
        .collect();
    assert!(
        findings.is_empty(),
        "{} finding(s):\n{}",
        findings.len(),
        findings.join("\n")
    );
}

macro_rules! golden_data_checks {
    ($($codec:ident => $crate_name:literal, $namespace:literal, $properties:ident, $ordinals:ident, $expressions:ident),* $(,)?) => {
        $(mod $codec {
            const NAMESPACE: &str = $namespace;

            #[test]
            fn $properties() {
                super::assert_clean($crate_name, |document| super::property_map_findings(NAMESPACE, document));
            }

            #[test]
            fn $ordinals() {
                super::assert_clean($crate_name, super::feature_ordinal_findings);
            }

            #[test]
            fn $expressions() {
                super::assert_clean($crate_name, |document| {
                    super::expression_text_findings(document, &super::equation_identities(document))
                });
            }
        })*
    };
}

macro_rules! never_stale {
    ($($module:ident => $crate_name:literal, $test:ident),* $(,)?) => {
        $(mod $module {
            #[test]
            fn $test() {
                super::assert_clean($crate_name, super::never_stale_findings);
            }
        })*
    };
}

golden_data_checks! {
    sat => "cadmpeg-codec-sat", "sat", sat_property_maps_hold_no_native_fact, sat_feature_ordinals_are_dense, sat_expression_text_names_no_parameter,
    step => "cadmpeg-codec-step", "step", step_property_maps_hold_no_native_fact, step_feature_ordinals_are_dense, step_expression_text_names_no_parameter,
    iges => "cadmpeg-codec-iges", "iges", iges_property_maps_hold_no_native_fact, iges_feature_ordinals_are_dense, iges_expression_text_names_no_parameter,
    rhino => "cadmpeg-codec-rhino", "rhino", rhino_property_maps_hold_no_native_fact, rhino_feature_ordinals_are_dense, rhino_expression_text_names_no_parameter,
    freecad => "cadmpeg-codec-freecad", "fcstd", freecad_property_maps_hold_no_native_fact, freecad_feature_ordinals_are_dense, freecad_expression_text_names_no_parameter,
    f3d => "cadmpeg-codec-f3d", "f3d", f3d_property_maps_hold_no_native_fact, f3d_feature_ordinals_are_dense, f3d_expression_text_names_no_parameter,
    sldprt => "cadmpeg-codec-sldprt", "sldprt", sldprt_property_maps_hold_no_native_fact, sldprt_feature_ordinals_are_dense, sldprt_expression_text_names_no_parameter,
    inventor => "cadmpeg-codec-inventor", "inventor", inventor_property_maps_hold_no_native_fact, inventor_feature_ordinals_are_dense, inventor_expression_text_names_no_parameter,
    catia => "cadmpeg-codec-catia", "catia", catia_property_maps_hold_no_native_fact, catia_feature_ordinals_are_dense, catia_expression_text_names_no_parameter,
    creo => "cadmpeg-codec-creo", "creo", creo_property_maps_hold_no_native_fact, creo_feature_ordinals_are_dense, creo_expression_text_names_no_parameter,
    nx => "cadmpeg-codec-nx", "nx", nx_property_maps_hold_no_native_fact, nx_feature_ordinals_are_dense, nx_expression_text_names_no_parameter,
}

never_stale! {
    sat_rule_e => "cadmpeg-codec-sat", sat_needs_recompute_is_false,
    step_rule_e => "cadmpeg-codec-step", step_needs_recompute_is_false,
    iges_rule_e => "cadmpeg-codec-iges", iges_needs_recompute_is_false,
    rhino_rule_e => "cadmpeg-codec-rhino", rhino_needs_recompute_is_false,
    f3d_rule_e => "cadmpeg-codec-f3d", f3d_needs_recompute_is_false,
    inventor_rule_e => "cadmpeg-codec-inventor", inventor_needs_recompute_is_false,
    catia_rule_e => "cadmpeg-codec-catia", catia_needs_recompute_is_false,
    nx_rule_e => "cadmpeg-codec-nx", nx_needs_recompute_is_false,
}

mod sldprt_rule_e {
    #[test]
    fn sldprt_needs_recompute_follows_the_active_configuration() {
        super::assert_clean("cadmpeg-codec-sldprt", super::solidworks_stale_findings);
    }
}

#[cfg(feature = "fcstd")]
mod freecad_corpus {
    use std::io::Cursor;
    use std::path::Path;

    use cadmpeg_ir::codec::{Codec, DecodeOptions};
    use cadmpeg_test_support::data_checks::DecodeGolden;

    fn corpus_decodes() -> Vec<DecodeGolden> {
        let directory =
            Path::new(env!("CARGO_MANIFEST_DIR")).join("../../corpus/freecad_fcstd/fixtures");
        let mut paths: Vec<_> = std::fs::read_dir(&directory)
            .expect("FreeCAD corpus fixtures")
            .filter_map(Result::ok)
            .map(|entry| entry.path())
            .filter(|path| {
                path.extension()
                    .is_some_and(|extension| extension == "FCStd")
            })
            .collect();
        paths.sort();
        super::super::sampled_paths(paths)
            .into_iter()
            .map(|path| {
                let bytes = std::fs::read(&path).expect("FreeCAD fixture bytes");
                let decoded = cadmpeg_codec_freecad::FcstdCodec
                    .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
                    .unwrap_or_else(|error| panic!("{}: {error}", path.display()));
                DecodeGolden {
                    path: path.display().to_string(),
                    ir: serde_json::to_value(decoded.ir()).expect("CADIR JSON"),
                    fidelity: serde_json::to_value(decoded.source_fidelity())
                        .expect("fidelity JSON"),
                }
            })
            .collect()
    }

    #[test]
    fn freecad_property_maps_hold_no_native_fact_on_every_corpus_fixture() {
        super::assert_documents_clean("FreeCAD corpus", &corpus_decodes(), |golden| {
            super::property_map_findings("fcstd", &golden.ir)
        });
    }

    #[test]
    fn freecad_needs_recompute_follows_touched_on_every_corpus_fixture() {
        super::assert_documents_clean("FreeCAD corpus", &corpus_decodes(), |golden| {
            super::freecad_stale_findings(&golden.ir)
        });
    }
}
