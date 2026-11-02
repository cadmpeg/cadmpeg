// SPDX-License-Identifier: Apache-2.0
//! The §4.5 cut-short contract (#215): cutting a source at 50% and 90% either
//! refuses with `Truncated` when no part survives, or decodes a prefix whose
//! every held record agrees with the uncut decode under
//! `Allowances::given_up`.

use cadmpeg_test_support::comparison::Allowances;

use super::support;

/// The cut fractions of the source length the rule names.
const CUTS: [usize; 2] = [50, 90];

fn check_format(format: &str) {
    let sampled = super::support::sampled_fixtures(format);
    let mut fixtures: Vec<super::support::Fixture> = sampled
        .into_iter()
        .filter(|fixture| !fixture.damaged)
        .collect();
    for damaged in super::support::damaged_fixtures(format) {
        if !fixtures.iter().any(|fixture| fixture.path == damaged.path) {
            fixtures.push(damaged);
        }
    }
    fixtures.sort_by(|left, right| left.path.cmp(&right.path));
    let mut findings = Vec::new();
    for fixture in &fixtures {
        if fixture.bytes.is_empty() {
            continue;
        }
        let uncut = match super::support::decode(format, &fixture.bytes) {
            Ok(decoded) => decoded,
            Err(_) if super::support::pins_decode_error(&fixture.path) => continue,
            Err(error) => {
                findings.push(format!(
                    "{format}: {} refuses its uncut input: {error}",
                    fixture.path.display()
                ));
                continue;
            }
        };
        let uncut_document = super::support::document(&uncut);
        for percent in CUTS {
            let length = fixture.bytes.len() * percent / 100;
            let cut = &fixture.bytes[..length];
            match super::support::decode(format, cut) {
                Ok(decoded) => {
                    let document = super::support::document(&decoded);
                    let report = super::support::report(&decoded);
                    match Allowances::given_up(&uncut_document, &report) {
                        Ok(allowances) => {
                            if let Some(mismatch) =
                                super::support::check_agree(&uncut_document, &document, &allowances)
                            {
                                findings.push(format!(
                                    "{format}: {} cut at {percent}% disagrees with the uncut decode: {mismatch}",
                                    fixture.path.display()
                                ));
                            }
                        }
                        Err(error) => findings.push(format!(
                            "{format}: {} cut at {percent}%: Allowances::given_up refuses the cut report: {error}",
                            fixture.path.display()
                        )),
                    }
                }
                Err(error) if super::support::is_truncated(&error) => {}
                Err(error) => findings.push(format!(
                    "{format}: {} cut at {percent}% refuses without Truncated: {error}",
                    fixture.path.display()
                )),
            }
        }
    }
    support::assert_no_findings(&findings);
}

macro_rules! cut_short {
    ($($module:ident => $format:literal, $test:ident);* $(;)?) => {
        $(#[cfg(feature = $format)]
        mod $module {
            #[test]
            fn $test() {
                super::check_format($format);
            }
        })*
    };
}

cut_short! {
    step_cut_short => "step", step_cut_short_files_decode_their_prefix_or_are_truncated;
    iges_cut_short => "iges", iges_cut_short_files_decode_their_prefix_or_are_truncated;
    rhino_cut_short => "rhino", rhino_cut_short_files_decode_their_prefix_or_are_truncated;
    freecad_cut_short => "fcstd", freecad_cut_short_files_decode_their_prefix_or_are_truncated;
    f3d_cut_short => "f3d", f3d_cut_short_files_decode_their_prefix_or_are_truncated;
    sldprt_cut_short => "sldprt", sldprt_cut_short_files_decode_their_prefix_or_are_truncated;
    catia_cut_short => "catia", catia_cut_short_files_decode_their_prefix_or_are_truncated;
    creo_cut_short => "creo", creo_cut_short_files_decode_their_prefix_or_are_truncated;
    usd_cut_short => "usd", usd_cut_short_files_decode_their_prefix_or_are_truncated;
}
