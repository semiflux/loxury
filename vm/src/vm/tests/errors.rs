use super::*;
use RunError::*;

#[track_caller]
fn fails(source: &str, row: u16, col: u16, error: RunError) {
    let mut vm = TestVm::default();
    assert_eq!(
        vm.execute_source(source),
        Err(AtCoords::at(row, col, error)),
        "{source}"
    );
}

#[test]
fn add_rejects_pairs_that_are_neither_two_numbers_nor_two_strings() {
    for (left, right) in [
        ("true", "nil"),
        ("true", "1"),
        ("true", "\"s\""),
        ("nil", "nil"),
        ("1", "nil"),
        ("\"s\"", "nil"),
    ] {
        fails(
            &format!("{left} + {right};"),
            1,
            left.len() as u16 + 2,
            ExpectedNumbersOrStrings,
        );
    }
}

#[test]
fn numeric_operators_reject_a_string_on_either_side_at_the_operator() {
    for op in ["/", "*", "-", ">", ">=", "<", "<="] {
        fails(&format!("\"1\" {op} 1;"), 1, 5, ExpectedNumbers);
        fails(&format!("1 {op} \"1\";"), 1, 3, ExpectedNumbers);
    }
    fails("-\"1\";", 1, 1, ExpectedNumber);
}

#[test]
fn bool_nil_number_string_and_instance_cannot_be_called() {
    for callee in ["true", "nil", "12", "\"hi\"", "C()"] {
        fails(
            &format!("class C {{}}\n{callee}();"),
            2,
            callee.len() as u16 + 1,
            NotCallable,
        );
    }
}

#[test]
fn field_reads_and_writes_require_an_instance() {
    for (receiver, payload) in [
        ("true", "true"),
        ("C", "C"),
        ("f", "<fn f>"),
        ("nil", "nil"),
        ("123", "123"),
        ("\"str\"", "str"),
    ] {
        for suffix in [";", " = true;"] {
            fails(
                &format!("class C {{}} fun f() {{}}\n{receiver}.field{suffix}"),
                2,
                receiver.len() as u16 + 2,
                NotAnInstance(payload.into()),
            );
        }
    }
}

#[test]
fn assigning_a_field_evaluates_its_receiver_before_the_right_hand_side() {
    let mut vm = TestVm::default();
    let source = "var called = false; fun rhs() { called = true; return missingValue; }\nmissingReceiver.x = rhs();";
    assert_eq!(
        vm.execute_source(source),
        Err(AtCoords::at(
            2,
            1,
            UndefinedVariable("missingReceiver".into())
        ))
    );
    assert!(!vm.boolean("called"));
}

#[test]
fn reads_invokes_and_noncallable_fields_report_the_failing_property_operation() {
    fails(
        "class C {} var o = C();\no.lost;",
        2,
        3,
        UndefinedProperty("lost".into()),
    );
    fails(
        "class C {} var o = C();\no.lost();",
        2,
        7,
        UndefinedProperty("lost".into()),
    );
    fails(
        "class C {} var o = C(); o.run = false;\no.run();",
        2,
        6,
        NotCallable,
    );
    fails(
        "class B {} class C < B { m() {\nsuper.lost(); } } C().m();",
        2,
        11,
        UndefinedProperty("lost".into()),
    );
}

#[test]
fn superclass_values_must_be_classes() {
    for declaration in ["fun parent() {}", "var parent = nil;", "var parent = 2;"] {
        fails(
            &format!("{declaration}\nclass Child < parent {{}}"),
            2,
            15,
            UninheritableValue,
        );
    }
}

#[test]
fn arity_errors_preserve_expected_and_actual_counts_for_each_call_kind() {
    for (decl, callee) in [
        ("fun f(a,b) {}", "f"),
        ("class C { m(a,b) {} }", "C().m"),
        ("class C { init(a,b) {} }", "C"),
    ] {
        for (args, got) in [("1", 1), ("1,2,3,4", 4)] {
            fails(
                &format!("{decl}\n{callee}({args});"),
                2,
                callee.len() as u16 + 1,
                WrongArity(2, got),
            );
        }
    }
    fails("class C {}\nC(1,2,3);", 2, 2, WrongArity(0, 3));
}

#[test]
fn super_arity_errors_preserve_effects_before_the_call() {
    for (args, count) in [("1", 1), ("1,2,3,4", 4)] {
        let mut vm = TestVm::default();
        let source = format!(
            "var state = 0; class Base {{ act(a,b) {{ state = 2; }} }}
class Child < Base {{ act() {{ state = 1; print state;
super.act({args}); state = 3; }} }} Child().act();"
        );
        assert_eq!(
            vm.execute_source(&source),
            Err(AtCoords::at(3, 10, WrongArity(2, count)))
        );
        assert_eq!(vm.number("state"), 1.0);
        assert_eq!(vm.out, b"1\n");
    }
}

#[test]
fn unresolved_names_are_runtime_errors_in_the_scope_that_uses_them() {
    for (source, row, col, name) in [
        ("missing;", 1, 1, "missing"),
        ("{ missing; }", 1, 3, "missing"),
        ("missing = 1;", 1, 9, "missing"),
        ("class C { act() {\nreturn act; } } C().act();", 2, 8, "act"),
        (
            "{ fun a() {\nreturn b(); } fun b() { return a(); } a(); }",
            2,
            8,
            "b",
        ),
    ] {
        fails(source, row, col, UndefinedVariable(name.into()));
    }
}

#[test]
fn a_runtime_error_after_a_multiline_string_uses_the_following_source_line() {
    fails(
        "var lines = \"one\ntwo\nthree\";\n  missing;",
        4,
        3,
        UndefinedVariable("missing".into()),
    );
}

#[test]
fn a_recursive_function_with_locals_reports_frame_overflow_in_the_callee() {
    let locals = (0..16).map(|i| format!("var x{i};")).collect::<String>();
    fails(
        &format!("fun recurse() {{ {locals}\nrecurse(); }}\nrecurse();"),
        2,
        8,
        StackOverflow,
    );
}
