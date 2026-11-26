// SPDX-License-Identifier: Apache-2.0

use crate::design::parameter::Tolerance;
use crate::design::units::{Quantity, ValueKind};
use crate::pmi::{FitZone, LimitsAndFits};
use crate::validate::design::fixture::{assert_finding, design_findings, document, parameter};

fn designation(form_variance: &str, zone: FitZone) -> LimitsAndFits {
    LimitsAndFits {
        form_variance: form_variance.to_owned(),
        zone,
        grade: "7".to_owned(),
        source: None,
    }
}

fn fit(fit: FitZone, mate: Option<FitZone>) -> Tolerance {
    Tolerance::Fit {
        fit: designation("H", fit),
        mate: mate.map(|zone| designation("G", zone)),
        deviations: None,
    }
}

#[test]
fn a_parameter_fit_and_its_mate_state_the_other_zone() {
    for (own, mate) in [
        (FitZone::Hole, Some(FitZone::Shaft)),
        (FitZone::Shaft, Some(FitZone::Hole)),
        (FitZone::Hole, None),
    ] {
        let mut ir = document();
        let mut bore = parameter("bore", ValueKind::Quantity(Quantity::LENGTH));
        bore.tolerance = Some(fit(own, mate));
        ir.design.parameters.push(bore);
        let findings = design_findings(&ir);
        assert!(
            !findings
                .iter()
                .any(|finding| finding.message.contains("the same zone")),
            "{findings:#?}"
        );
    }
    for zone in [FitZone::Hole, FitZone::Shaft] {
        let mut ir = document();
        let mut bore = parameter("bore", ValueKind::Quantity(Quantity::LENGTH));
        bore.tolerance = Some(fit(zone, Some(zone)));
        ir.design.parameters.push(bore);
        assert_finding(&ir, "parameter's fit and its mate state the same zone");
    }
}
