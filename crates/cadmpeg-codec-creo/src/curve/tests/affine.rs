// SPDX-License-Identifier: Apache-2.0
#![allow(clippy::unwrap_used)]

use super::*;

#[test]
fn decodes_counted_curve_expression_source_lines() {
    let payload = b"\xe0\x00entity(crv_fr_eqn)\0\xe3\xe0\x01id\0\x89\x4c\
        \xe0\x0aexpression\0\xf8\x04r=5\0theta=t*360\0z=71*t\0q=r+2*(3)\0\
        \xe0\x00backup_ents(crv_fr_eqn)\0\xe3\xe0\x01id\0\0\
        \xe0\x0aexpression\0\xf8\x01r=5\0";
    let records = expression_records(payload);
    assert_eq!(records.len(), 2);
    assert_eq!(records[0].entity_id, 0x094c);
    assert!(!records[0].backup);
    assert_eq!(
        records[0]
            .lines
            .iter()
            .map(|line| line.text.as_str())
            .collect::<Vec<_>>(),
        ["r=5", "theta=t*360", "z=71*t", "q=r+2*(3)"]
    );
    assert!(records[1].backup);
    assert_eq!(records[1].lines[0].text, "r=5");
    assert!(records[0].lines[0].offset < records[0].lines[1].offset);
    assert_eq!(records[0].assignments.len(), 4);
    assert_eq!(
        records[0].assignments[0].parameter_target(),
        Some(("r", None))
    );
    assert_eq!(records[0].assignments[0].expression, "5");
    assert!(records[0].assignments[0].dependencies.is_empty());
    assert_eq!(
        records[0].assignments[0].value,
        Some(CurveExpressionValue::Number(5.0))
    );
    assert_eq!(
        records[0].assignments[1].parameter_target(),
        Some(("theta", None))
    );
    assert_eq!(records[0].assignments[1].expression, "t*360");
    assert_eq!(records[0].assignments[1].dependencies, ["t"]);
    assert_eq!(records[0].assignments[1].value, None);
    assert_eq!(records[0].assignments[2].value, None);
    assert_eq!(records[0].assignments[3].dependencies, ["r"]);
    assert_eq!(
        records[0].assignments[3].value,
        Some(CurveExpressionValue::Number(11.0))
    );
}

#[test]
fn standalone_equality_does_not_create_an_assignment() {
    let lines = ["ghost==missing", "seen=exists('ghost')", "flag=1==1"]
        .into_iter()
        .enumerate()
        .map(|(offset, text)| CurveExpressionLine {
            text: text.to_owned(),
            offset,
        })
        .collect::<Vec<_>>();

    let assignments =
        evaluate_expression_program(&lines, None, &ExternalRelationSymbols::default());

    assert_eq!(assignments.len(), 2);
    assert_eq!(assignments[0].parameter_target(), Some(("seen", None)));
    assert_eq!(assignments[0].value, None);
    assert_eq!(assignments[1].parameter_target(), Some(("flag", None)));
    assert_eq!(
        assignments[1].value,
        Some(CurveExpressionValue::Number(1.0))
    );
}

#[test]
fn retains_simultaneous_equations_without_sequential_assignments() {
    let lines = [
        "area=100",
        "base=10",
        "width=99",
        "SOLVE",
        "width=height+1",
        "offset=base+1",
        "width*height=area",
        "FOR width, height",
        "present=exists('width')",
        "after_width=width",
        "result=area+1",
    ]
    .into_iter()
    .enumerate()
    .map(|(offset, text)| CurveExpressionLine {
        text: text.to_owned(),
        offset,
    })
    .collect::<Vec<_>>();

    let program = curve_expression_solve_program(&lines);
    assert!(!program.unresolved_control);
    let [block] = program.blocks.as_slice() else {
        panic!("one solve block");
    };
    assert_eq!(block.variables, ["width", "height"]);
    assert_eq!(block.offset, 3);
    assert_eq!(block.for_offset, 7);
    assert_eq!(block.equations.len(), 2);
    assert_eq!(block.equations[0].left, "width");
    assert_eq!(block.equations[0].right, "height+1");
    assert_eq!(block.equations[0].dependencies, ["width", "height"]);
    assert_eq!(block.equations[1].left, "width*height");
    assert_eq!(block.equations[1].right, "area");
    assert_eq!(block.equations[1].dependencies, ["width", "height", "area"]);
    assert_eq!(block.assignments.len(), 1);
    assert_eq!(
        block.assignments[0].parameter_target(),
        Some(("offset", None))
    );
    assert_eq!(block.assignments[0].expression, "base+1");
    assert_eq!(block.assignments[0].dependencies, ["base"]);

    let assignments =
        evaluate_expression_program(&lines, None, &ExternalRelationSymbols::default());
    assert_eq!(assignments.len(), 7);
    assert_eq!(assignments[0].parameter_target(), Some(("area", None)));
    assert_eq!(assignments[1].parameter_target(), Some(("base", None)));
    assert_eq!(assignments[2].parameter_target(), Some(("width", None)));
    assert_eq!(assignments[2].value, None);
    assert_eq!(assignments[3].parameter_target(), Some(("offset", None)));
    assert_eq!(
        assignments[3].value,
        Some(CurveExpressionValue::Number(11.0))
    );
    assert_eq!(assignments[4].parameter_target(), Some(("present", None)));
    assert_eq!(
        assignments[4].value,
        Some(CurveExpressionValue::Number(1.0))
    );
    assert_eq!(
        assignments[5].parameter_target(),
        Some(("after_width", None))
    );
    assert_eq!(assignments[5].value, None);
    assert_eq!(assignments[6].parameter_target(), Some(("result", None)));
    assert_eq!(
        assignments[6].value,
        Some(CurveExpressionValue::Number(101.0))
    );
}

#[test]
fn solves_complete_affine_simultaneous_equations() {
    let lines = [
        "x=0",
        "y=0",
        "sum=10",
        "SOLVE",
        "x+y=sum",
        "x-y=2",
        "FOR x,y",
        "product=x*y",
    ]
    .into_iter()
    .enumerate()
    .map(|(offset, text)| CurveExpressionLine {
        text: text.to_owned(),
        offset,
    })
    .collect::<Vec<_>>();

    let evaluation =
        evaluate_expression_program_details(&lines, None, &ExternalRelationSymbols::default());

    assert_eq!(
        evaluation.solve_solutions[&3],
        [
            CurveExpressionValue::Number(6.0),
            CurveExpressionValue::Number(4.0),
        ]
    );
    assert_eq!(evaluation.assignments.len(), 4);
    assert_eq!(
        evaluation
            .assignments
            .iter()
            .map(|assignment| assignment.value.clone())
            .collect::<Vec<_>>(),
        [
            Some(CurveExpressionValue::Number(6.0)),
            Some(CurveExpressionValue::Number(4.0)),
            Some(CurveExpressionValue::Number(10.0)),
            Some(CurveExpressionValue::Number(24.0)),
        ]
    );
}

#[test]
fn solves_affine_systems_without_previous_numeric_values() {
    let lines = ["SOLVE", "x+y=10[mm]", "x-y=2[mm]", "FOR x,y", "sum=x+y"]
        .into_iter()
        .enumerate()
        .map(|(offset, text)| CurveExpressionLine {
            text: text.to_owned(),
            offset,
        })
        .collect::<Vec<_>>();

    let evaluation =
        evaluate_expression_program_details(&lines, None, &ExternalRelationSymbols::default());

    assert_eq!(
        evaluation.solve_solutions[&0],
        [
            CurveExpressionValue::Length(6.0),
            CurveExpressionValue::Length(4.0),
        ]
    );
    assert_eq!(
        evaluation.assignments[0].value,
        Some(CurveExpressionValue::Length(10.0))
    );
}

#[test]
fn infers_missing_solve_dimensions_through_known_quantities() {
    let lines = [
        "speed=2[mm/s]",
        "total=10[mm]",
        "SOLVE",
        "distance+speed*duration=total",
        "duration=2[s]",
        "FOR distance,duration",
    ]
    .into_iter()
    .enumerate()
    .map(|(offset, text)| CurveExpressionLine {
        text: text.to_owned(),
        offset,
    })
    .collect::<Vec<_>>();

    let evaluation =
        evaluate_expression_program_details(&lines, None, &ExternalRelationSymbols::default());

    assert_eq!(
        evaluation.solve_solutions[&2],
        [
            CurveExpressionValue::Length(6.0),
            quantity_value(2.0, RelationDimension::TIME),
        ]
    );
}

#[test]
fn leaves_free_solve_dimensions_unresolved() {
    let lines = ["SOLVE", "x+y=y", "x-y=x", "FOR x,y"]
        .into_iter()
        .enumerate()
        .map(|(offset, text)| CurveExpressionLine {
            text: text.to_owned(),
            offset,
        })
        .collect::<Vec<_>>();

    let evaluation =
        evaluate_expression_program_details(&lines, None, &ExternalRelationSymbols::default());

    assert!(evaluation.solve_solutions.is_empty());
    assert!(evaluation.assignments.is_empty());
}

#[test]
fn rejects_missing_solve_dimensions_with_conflicting_units() {
    let lines = ["SOLVE", "x=1[mm]", "x=1[s]", "FOR x"]
        .into_iter()
        .enumerate()
        .map(|(offset, text)| CurveExpressionLine {
            text: text.to_owned(),
            offset,
        })
        .collect::<Vec<_>>();

    let evaluation =
        evaluate_expression_program_details(&lines, None, &ExternalRelationSymbols::default());

    assert!(evaluation.solve_solutions.is_empty());
    assert!(evaluation.assignments.is_empty());
}

#[test]
fn leaves_underdetermined_affine_systems_unsolved() {
    let lines = ["x=0", "y=0", "SOLVE", "x+y=10", "FOR x,y"]
        .into_iter()
        .enumerate()
        .map(|(offset, text)| CurveExpressionLine {
            text: text.to_owned(),
            offset,
        })
        .collect::<Vec<_>>();

    let evaluation =
        evaluate_expression_program_details(&lines, None, &ExternalRelationSymbols::default());

    assert!(evaluation.solve_solutions.is_empty());
    assert_eq!(evaluation.assignments[0].value, None);
    assert_eq!(evaluation.assignments[1].value, None);
}

#[test]
fn rejects_overflow_when_reducing_affine_comparison() {
    let overflow = [
        "x=0",
        "limit=1e308",
        "SOLVE",
        "if(limit-(-limit)>0,x,0)=1",
        "FOR x",
    ]
    .into_iter()
    .enumerate()
    .map(|(offset, text)| CurveExpressionLine {
        text: text.to_owned(),
        offset,
    })
    .collect::<Vec<_>>();
    let evaluation =
        evaluate_expression_program_details(&overflow, None, &ExternalRelationSymbols::default());
    assert!(evaluation.solve_solutions.is_empty());
    assert_eq!(evaluation.assignments[0].value, None);
}

#[test]
fn solves_affine_systems_with_fixed_boolean_annihilators() {
    let lines = ["x=0", "y=0", "SOLVE", "x=3", "y+(0&x)+(x&0)=4", "FOR x,y"]
        .into_iter()
        .enumerate()
        .map(|(offset, text)| CurveExpressionLine {
            text: text.to_owned(),
            offset,
        })
        .collect::<Vec<_>>();

    let evaluation =
        evaluate_expression_program_details(&lines, None, &ExternalRelationSymbols::default());

    assert_eq!(
        evaluation.solve_solutions[&2],
        [
            CurveExpressionValue::Number(3.0),
            CurveExpressionValue::Number(4.0),
        ]
    );

    let lines = ["x=0", "y=0", "SOLVE", "x=3", "y+(1|x)+(x|1)=6", "FOR x,y"]
        .into_iter()
        .enumerate()
        .map(|(offset, text)| CurveExpressionLine {
            text: text.to_owned(),
            offset,
        })
        .collect::<Vec<_>>();

    let evaluation =
        evaluate_expression_program_details(&lines, None, &ExternalRelationSymbols::default());

    assert_eq!(
        evaluation.solve_solutions[&2],
        [
            CurveExpressionValue::Number(3.0),
            CurveExpressionValue::Number(4.0),
        ]
    );
}

#[test]
fn solves_affine_systems_with_fixed_function_powers() {
    let lines = [
        "x=0[mm]",
        "y=0",
        "SOLVE",
        "pow(x,1)=3[mm]",
        "y+pow(x,0)=5",
        "FOR x,y",
    ]
    .into_iter()
    .enumerate()
    .map(|(offset, text)| CurveExpressionLine {
        text: text.to_owned(),
        offset,
    })
    .collect::<Vec<_>>();

    let evaluation =
        evaluate_expression_program_details(&lines, None, &ExternalRelationSymbols::default());

    assert_eq!(
        evaluation.solve_solutions[&2],
        [
            CurveExpressionValue::Length(3.0),
            CurveExpressionValue::Number(4.0),
        ]
    );
}

#[test]
fn solves_affine_systems_with_branch_and_sign_invariants() {
    let lines = [
        "x=0",
        "y=0[mm]",
        "SOLVE",
        "x=3",
        "if(x,y,y)+sign(0[mm],x)=4[mm]",
        "FOR x,y",
    ]
    .into_iter()
    .enumerate()
    .map(|(offset, text)| CurveExpressionLine {
        text: text.to_owned(),
        offset,
    })
    .collect::<Vec<_>>();

    let evaluation =
        evaluate_expression_program_details(&lines, None, &ExternalRelationSymbols::default());

    assert_eq!(
        evaluation.solve_solutions[&2],
        [
            CurveExpressionValue::Number(3.0),
            CurveExpressionValue::Length(4.0),
        ]
    );
}

#[test]
fn leaves_inconsistent_affine_systems_unsolved() {
    let lines = [
        "x=0", "y=0", "SOLVE", "x+y=10", "x-y=2", "x+y=11", "FOR x,y",
    ]
    .into_iter()
    .enumerate()
    .map(|(offset, text)| CurveExpressionLine {
        text: text.to_owned(),
        offset,
    })
    .collect::<Vec<_>>();

    let evaluation =
        evaluate_expression_program_details(&lines, None, &ExternalRelationSymbols::default());

    assert!(evaluation.solve_solutions.is_empty());
    assert_eq!(evaluation.assignments[0].value, None);
    assert_eq!(evaluation.assignments[1].value, None);
}

#[test]
fn solves_unique_nonlinear_simultaneous_equations() {
    let lines = ["x=2", "SOLVE", "x*x*x=8", "FOR x", "after=x+1"]
        .into_iter()
        .enumerate()
        .map(|(offset, text)| CurveExpressionLine {
            text: text.to_owned(),
            offset,
        })
        .collect::<Vec<_>>();

    let evaluation =
        evaluate_expression_program_details(&lines, None, &ExternalRelationSymbols::default());

    let [CurveExpressionValue::Number(solution)] = evaluation.solve_solutions[&1].as_slice() else {
        panic!("expected one numeric nonlinear solution");
    };
    assert!((*solution - 2.0).abs() <= 1.0e-9);
    let Some(CurveExpressionValue::Number(after)) = &evaluation.assignments[1].value else {
        panic!("expected evaluated assignment after nonlinear solve");
    };
    assert!((*after - 3.0).abs() <= 1.0e-9);
}

#[test]
fn leaves_nonlinear_systems_without_previous_values_unsolved() {
    let lines = ["SOLVE", "x*x*x=8", "FOR x"]
        .into_iter()
        .enumerate()
        .map(|(offset, text)| CurveExpressionLine {
            text: text.to_owned(),
            offset,
        })
        .collect::<Vec<_>>();

    let evaluation =
        evaluate_expression_program_details(&lines, None, &ExternalRelationSymbols::default());

    assert!(evaluation.solve_solutions.is_empty());
    assert!(evaluation.assignments.is_empty());
}

#[test]
fn rejects_nonlinear_systems_with_multiple_roots() {
    let lines = ["x=1", "SOLVE", "x*x=4", "FOR x"]
        .into_iter()
        .enumerate()
        .map(|(offset, text)| CurveExpressionLine {
            text: text.to_owned(),
            offset,
        })
        .collect::<Vec<_>>();

    let evaluation =
        evaluate_expression_program_details(&lines, None, &ExternalRelationSymbols::default());

    assert!(evaluation.solve_solutions.is_empty());
    assert_eq!(evaluation.assignments.len(), 1);
    assert_eq!(evaluation.assignments[0].value, None);
}

#[test]
fn affine_solver_is_invariant_under_independent_equation_scaling() {
    let mut rows = [
        AffineEquationRow {
            coefficients: vec![1e-15, 0.0],
            rhs: 6e-15,
        },
        AffineEquationRow {
            coefficients: vec![0.0, 1e15],
            rhs: 4e15,
        },
    ];

    let solution =
        solve_unique_affine_system(&mut rows, 2).expect("independently scaled unique system");
    assert!((solution[0] - 6.0).abs() <= 1.0e-12);
    assert!((solution[1] - 4.0).abs() <= 1.0e-12);
}

#[test]
fn solves_dimensioned_affine_simultaneous_equations() {
    let lines = [
        "x=0[mm]",
        "y=0[mm]",
        "SOLVE",
        "x+y=10[mm]",
        "x-y=2[mm]",
        "FOR x,y",
        "area=x*y",
    ]
    .into_iter()
    .enumerate()
    .map(|(offset, text)| CurveExpressionLine {
        text: text.to_owned(),
        offset,
    })
    .collect::<Vec<_>>();

    let evaluation =
        evaluate_expression_program_details(&lines, None, &ExternalRelationSymbols::default());

    assert_eq!(
        evaluation.solve_solutions[&2],
        [
            CurveExpressionValue::Length(6.0),
            CurveExpressionValue::Length(4.0),
        ]
    );
    assert_eq!(
        evaluation.assignments[0].value,
        Some(CurveExpressionValue::Length(6.0))
    );
    assert_eq!(
        evaluation.assignments[1].value,
        Some(CurveExpressionValue::Length(4.0))
    );
    assert_eq!(
        evaluation.assignments[2].value,
        Some(quantity_value(
            24.0,
            RelationDimension::LENGTH
                .scale(2)
                .expect("squared length dimension")
        ))
    );
}

#[test]
fn solves_affine_piecewise_expressions_with_unknown_independent_branches() {
    let lines = [
        "x=0[mm]",
        "y=0[mm]",
        "SOLVE",
        "min(x+2[mm],x+5[mm])+y=10[mm]",
        "max(x-1[mm],x-3[mm])-y=3[mm]",
        "if(x+1[mm]>x,x,x+100[mm])-y=4[mm]",
        "FOR x,y",
    ]
    .into_iter()
    .enumerate()
    .map(|(offset, text)| CurveExpressionLine {
        text: text.to_owned(),
        offset,
    })
    .collect::<Vec<_>>();

    let evaluation =
        evaluate_expression_program_details(&lines, None, &ExternalRelationSymbols::default());

    assert_eq!(
        evaluation.solve_solutions[&2],
        [
            CurveExpressionValue::Length(6.0),
            CurveExpressionValue::Length(2.0),
        ]
    );
}

#[test]
fn leaves_unknown_dependent_piecewise_systems_unsolved() {
    let lines = [
        "x=0[mm]",
        "y=0[mm]",
        "SOLVE",
        "min(x,-x)+y=10[mm]",
        "x-y=2[mm]",
        "FOR x,y",
    ]
    .into_iter()
    .enumerate()
    .map(|(offset, text)| CurveExpressionLine {
        text: text.to_owned(),
        offset,
    })
    .collect::<Vec<_>>();

    let evaluation =
        evaluate_expression_program_details(&lines, None, &ExternalRelationSymbols::default());

    assert!(evaluation.solve_solutions.is_empty());
    assert_eq!(evaluation.assignments[0].value, None);
    assert_eq!(evaluation.assignments[1].value, None);
}

#[test]
fn solves_parallel_affine_clamps_deadbands_and_tolerance_tests() {
    let lines = [
        "x=0[mm]",
        "y=0[mm]",
        "SOLVE",
        "bound(x+2[mm],x+1[mm],x+3[mm])+y=10[mm]",
        "dead(x+4[mm],x+1[mm],x+3[mm])-y=1[mm]",
        "near(x+1[mm],x+2[mm],1[mm])=1",
        "dbl_in_tol(x+2[mm],x+1[mm],1[mm])=1",
        "FOR x,y",
    ]
    .into_iter()
    .enumerate()
    .map(|(offset, text)| CurveExpressionLine {
        text: text.to_owned(),
        offset,
    })
    .collect::<Vec<_>>();

    let evaluation =
        evaluate_expression_program_details(&lines, None, &ExternalRelationSymbols::default());

    assert_eq!(
        evaluation.solve_solutions[&2],
        [
            CurveExpressionValue::Length(8.0),
            CurveExpressionValue::Length(0.0),
        ]
    );
}

#[test]
fn leaves_unknown_dependent_clamps_unsolved() {
    let lines = [
        "x=0[mm]",
        "y=0[mm]",
        "SOLVE",
        "bound(x,0[mm],10[mm])+y=10[mm]",
        "x-y=2[mm]",
        "FOR x,y",
    ]
    .into_iter()
    .enumerate()
    .map(|(offset, text)| CurveExpressionLine {
        text: text.to_owned(),
        offset,
    })
    .collect::<Vec<_>>();

    let evaluation =
        evaluate_expression_program_details(&lines, None, &ExternalRelationSymbols::default());

    assert!(evaluation.solve_solutions.is_empty());
    assert_eq!(evaluation.assignments[0].value, None);
    assert_eq!(evaluation.assignments[1].value, None);
}

#[test]
fn solves_affine_systems_with_different_unknown_dimensions() {
    let lines = [
        "distance=0[mm]",
        "duration=0[s]",
        "speed=2[mm/s]",
        "total=10[mm]",
        "SOLVE",
        "distance+speed*duration=total",
        "duration=2[s]",
        "FOR distance,duration",
    ]
    .into_iter()
    .enumerate()
    .map(|(offset, text)| CurveExpressionLine {
        text: text.to_owned(),
        offset,
    })
    .collect::<Vec<_>>();

    let evaluation =
        evaluate_expression_program_details(&lines, None, &ExternalRelationSymbols::default());

    assert_eq!(
        evaluation.solve_solutions[&4],
        [
            CurveExpressionValue::Length(6.0),
            quantity_value(2.0, RelationDimension::TIME),
        ]
    );
}

#[test]
fn preserves_reserved_quantity_dimensions_in_affine_systems() {
    let lines = [
        "acceleration=0[mm/s^2]",
        "SOLVE",
        "acceleration=G",
        "FOR acceleration",
    ]
    .into_iter()
    .enumerate()
    .map(|(offset, text)| CurveExpressionLine {
        text: text.to_owned(),
        offset,
    })
    .collect::<Vec<_>>();

    let evaluation =
        evaluate_expression_program_details(&lines, None, &ExternalRelationSymbols::default());

    assert_eq!(
        evaluation.solve_solutions[&1],
        [quantity_value(9_800.0, RelationDimension::ACCELERATION)]
    );
}

#[test]
fn leaves_dimensionally_inconsistent_affine_systems_unsolved() {
    let lines = ["x=0[mm]", "SOLVE", "x=1[s]", "FOR x"]
        .into_iter()
        .enumerate()
        .map(|(offset, text)| CurveExpressionLine {
            text: text.to_owned(),
            offset,
        })
        .collect::<Vec<_>>();

    let evaluation =
        evaluate_expression_program_details(&lines, None, &ExternalRelationSymbols::default());

    assert!(evaluation.solve_solutions.is_empty());
    assert_eq!(evaluation.assignments[0].value, None);
}

#[test]
fn unterminated_solve_block_cannot_create_assignments() {
    let lines = ["before=1", "SOLVE", "false_parameter=2", "after=3"]
        .into_iter()
        .enumerate()
        .map(|(offset, text)| CurveExpressionLine {
            text: text.to_owned(),
            offset,
        })
        .collect::<Vec<_>>();

    let program = curve_expression_solve_program(&lines);
    assert!(program.unresolved_control);
    assert!(program.blocks.is_empty());
    let assignments =
        evaluate_expression_program(&lines, None, &ExternalRelationSymbols::default());
    assert_eq!(assignments.len(), 1);
    assert_eq!(assignments[0].parameter_target(), Some(("before", None)));
}
