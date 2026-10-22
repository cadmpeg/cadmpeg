// SPDX-License-Identifier: Apache-2.0
//! `diff --geometry`: ID-free comparison of two models' measured bodies.
//!
//! Each document is measured at body level in model space under its own
//! session, and `cadmpeg_measure::compare` matches the bodies by geometry.
//! Text output is for people; the TSV prints one row per compared value.

use std::io::{self, Write};
use std::path::Path;

use cadmpeg_ir::report::decode::DecodeReport;
use cadmpeg_ir::DecodeOptions;
use cadmpeg_measure::compare::{compare, CompareOptions, Comparison, Operand, Row, Verdict};
use cadmpeg_measure::report::MeasureReport;
use cadmpeg_measure::{Selection, WorkProfile};
use serde::Serialize;

use super::render::{name, number};
use super::{input_summary, require_geometry, run, MeasureBody, Session};
use crate::application::document::LoadedDocument;
use crate::application::refusal::ApplicationError;

/// How the geometry comparison runs.
#[derive(Debug, Clone, Copy)]
pub(crate) struct GeometryArgs {
    pub(crate) compare: CompareOptions,
    pub(crate) profile: WorkProfile,
    /// Print the comparison table instead of text.
    pub(crate) tsv: bool,
}

/// Both documents measured and compared.
pub(crate) struct GeometryDiff {
    a: MeasureReport,
    b: MeasureReport,
    comparison: Comparison,
}

/// The `geometry` member of a `diff` report.
#[derive(Serialize)]
pub(crate) struct GeometryBody<'a> {
    a: MeasureBody<'a>,
    b: MeasureBody<'a>,
    comparison: &'a Comparison,
}

/// The TSV header of the comparison table.
const HEADER: [&str; 10] = [
    "a",
    "b",
    "quantity",
    "unit",
    "value_a",
    "value_b",
    "difference",
    "tolerance",
    "rule",
    "agrees",
];

impl GeometryDiff {
    /// Measure the bodies of both documents and compare them.
    pub(crate) fn measure(
        left: &LoadedDocument,
        right: &LoadedDocument,
        options: &DecodeOptions,
        args: &GeometryArgs,
    ) -> Result<Self, ApplicationError> {
        let session = Session {
            options,
            profile: args.profile,
        };
        let measured = |loaded: &LoadedDocument| -> Result<MeasureReport, ApplicationError> {
            require_geometry(loaded)?;
            let transferred = loaded
                .decode_report()
                .is_none_or(DecodeReport::geometry_transferred);
            run(session, loaded, &Selection::default(), transferred)
        };
        let a = measured(left)?;
        let b = measured(right)?;
        let comparison = compare(&left.ir, &a, &right.ir, &b, &args.compare);
        Ok(Self { a, b, comparison })
    }

    /// The comparison verdict.
    pub(crate) const fn verdict(&self) -> Verdict {
        self.comparison.verdict()
    }

    /// The JSON body: both measure bodies and the comparison.
    pub(crate) fn body<'a>(
        &'a self,
        left: &'a LoadedDocument,
        right: &'a LoadedDocument,
    ) -> GeometryBody<'a> {
        GeometryBody {
            a: MeasureBody {
                input: input_summary(left),
                report: &self.a,
            },
            b: MeasureBody {
                input: input_summary(right),
                report: &self.b,
            },
            comparison: &self.comparison,
        }
    }

    /// Human-readable blocks: inputs, pairs, unpaired bodies, document rows
    /// and the verdict.
    pub(crate) fn text(&self, out: &mut impl Write, paths: [&Path; 2]) -> io::Result<()> {
        let comparison = &self.comparison;
        let count = |report: &MeasureReport| match report.bodies().len() {
            1 => "1 body".to_owned(),
            count => format!("{count} bodies"),
        };
        writeln!(
            out,
            "geometry diff {} ({}) vs {} ({})",
            paths[0].display(),
            count(&self.a),
            paths[1].display(),
            count(&self.b)
        )?;
        let placement = comparison.placement();
        writeln!(
            out,
            "  placement     {} / {}",
            name(&placement.a()),
            name(&placement.b())
        )?;
        for pair in comparison.pairs() {
            writeln!(out, "  pair {} = {}", pair.a(), pair.b())?;
            for row in pair.quantities() {
                writeln!(out, "    {}", row_text(row))?;
            }
        }
        for (label, ids) in [
            ("unmatched a", comparison.unmatched_a()),
            ("unmatched b", comparison.unmatched_b()),
            ("unmeasured a", comparison.unmeasured_a()),
            ("unmeasured b", comparison.unmeasured_b()),
        ] {
            for id in ids {
                writeln!(out, "  {label:<13} {id}")?;
            }
        }
        for class in comparison.ambiguous() {
            writeln!(
                out,
                "  ambiguous     a: {}; b: {}",
                class.a().join(", "),
                class.b().join(", ")
            )?;
        }
        writeln!(out, "  document")?;
        for row in comparison.document() {
            writeln!(out, "    {}", row_text(row))?;
        }
        let causes: Vec<&str> = comparison
            .inconclusive_causes()
            .iter()
            .map(|cause| cause.code())
            .collect();
        if causes.is_empty() {
            writeln!(out, "verdict: {}", name(&comparison.verdict()))
        } else {
            writeln!(
                out,
                "verdict: {} ({})",
                name(&comparison.verdict()),
                causes.join(", ")
            )
        }
    }

    /// The comparison table: one row per compared value, document rows with
    /// `document` in both ID cells, unmatched and ambiguous bodies.
    pub(crate) fn tsv(&self) -> String {
        let comparison = &self.comparison;
        let mut lines = vec![HEADER.join("\t")];
        for pair in comparison.pairs() {
            for row in pair.quantities() {
                lines.push(row_cells(pair.a(), pair.b(), row).join("\t"));
            }
        }
        for row in comparison.document() {
            lines.push(row_cells("document", "document", row).join("\t"));
        }
        let marker = |a: &str, b: &str, quantity: &str| {
            [a, b, quantity]
                .into_iter()
                .chain(HEADER.iter().skip(3).map(|_| ""))
                .collect::<Vec<&str>>()
                .join("\t")
        };
        for id in comparison.unmatched_a() {
            lines.push(marker(id, "", "unmatched"));
        }
        for id in comparison.unmatched_b() {
            lines.push(marker("", id, "unmatched"));
        }
        for id in comparison.unmeasured_a() {
            lines.push(marker(id, "", "unmeasured"));
        }
        for id in comparison.unmeasured_b() {
            lines.push(marker("", id, "unmeasured"));
        }
        for class in comparison.ambiguous() {
            for id in class.a() {
                lines.push(marker(id, "", "ambiguous"));
            }
            for id in class.b() {
                lines.push(marker("", id, "ambiguous"));
            }
        }
        let mut text = lines.join("\n");
        text.push('\n');
        text
    }
}

/// An operand in full precision.
fn operand(value: Option<&Operand>) -> String {
    match value {
        None => String::new(),
        Some(Operand::Number(value)) => value.to_string(),
        Some(Operand::Flag(flag)) => flag.to_string(),
        Some(Operand::Count(count)) => count.to_string(),
        Some(Operand::Text(text)) => text.clone(),
        Some(Operand::Texts(texts)) => texts.join(","),
    }
}

/// An operand for people.
fn operand_text(value: Option<&Operand>) -> String {
    match value {
        Some(Operand::Number(value)) => number(*value),
        other => operand(other),
    }
}

fn unit_name(row: &Row) -> String {
    row.unit().map_or_else(String::new, |unit| name(&unit))
}

fn row_cells(a: &str, b: &str, row: &Row) -> Vec<String> {
    vec![
        a.to_owned(),
        b.to_owned(),
        row.quantity().to_owned(),
        unit_name(row),
        operand(row.a()),
        operand(row.b()),
        row.difference()
            .map_or_else(String::new, |value| value.to_string()),
        row.tolerance()
            .map_or_else(String::new, |value| value.to_string()),
        name(&row.rule()),
        row.agrees()
            .map_or_else(String::new, |agrees| agrees.to_string()),
    ]
}

fn row_text(row: &Row) -> String {
    let outcome = match (row.agrees(), row.cause()) {
        (Some(true), _) => "agree".to_owned(),
        (Some(false), _) => "disagree".to_owned(),
        (None, Some(cause)) => format!("not compared ({})", cause.code()),
        (None, None) => "not compared".to_owned(),
    };
    format!(
        "{:<24} {} = {}  {outcome}",
        row.quantity(),
        operand_text(row.a()),
        operand_text(row.b())
    )
}
