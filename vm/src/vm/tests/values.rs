use super::*;

#[test]
fn equality_and_inequality_distinguish_types_even_when_values_print_alike() {
    let mut vm = TestVm::default();
    // Equal group numbers mean equal Lox values, independently of spelling.
    let samples = [
        ("nil", 0),
        ("true", 1),
        ("false", 2),
        ("1", 3),
        ("1.0", 3),
        ("0", 4),
        ("-0", 4),
        ("\"0\"", 5),
        ("\"true\"", 6),
        ("\"false\"", 7),
        ("\"\"", 8),
        ("\"fox\"", 9),
        ("(\"f\" + \"ox\")", 9),
    ];
    for (left, a) in samples {
        for (right, b) in samples {
            vm.interpret(&format!(
                "var eq = {left} == {right}; var ne = {left} != {right};"
            ));
            assert_eq!(vm.boolean("eq"), a == b, "{left} == {right}");
            assert_eq!(vm.boolean("ne"), a != b, "{left} != {right}");
        }
    }
}

#[test]
fn nan_from_division_is_a_number_unequal_to_itself() {
    let mut vm = TestVm::default();
    vm.interpret(
        "var n = 0 / 0; var eq = n == n; var ne = n != n; var zero = n == 0; var one = n != 1;",
    );
    assert!(vm.number("n").is_nan());
    assert!(!vm.boolean("eq"));
    assert!(vm.boolean("ne"));
    assert!(!vm.boolean("zero"));
    assert!(vm.boolean("one"));
}

#[test]
fn numeric_literals_negation_and_grouping_preserve_the_numeric_value() {
    let mut vm = TestVm::default();
    for (expr, expected) in [
        ("987654", 987654.0_f64),
        ("123.456", 123.456),
        ("-0.001", -0.001),
        ("0", 0.0),
        ("-0", -0.0),
        ("--(3)", 3.0),
        ("---(3)", -3.0),
        ("2 + 3 * 4", 14.0),
        ("20 - 3 * 4", 8.0),
        ("2 + 6 / 3", 4.0),
        ("2 - 6 / 3", 0.0),
        ("(2 * (6 - (2 + 2)))", 4.0),
        ("5-2", 3.0),
        ("5 -2", 3.0),
        ("5- 2", 3.0),
        ("5 - 2", 3.0),
        ("12.34 * 0.3", 12.34 * 0.3),
        ("12.34 / 12.34", 1.0),
        ("1.2 - 1.2", 0.0),
    ] {
        vm.interpret(&format!("var n = {expr};"));
        assert_eq!(vm.number("n").to_bits(), expected.to_bits(), "{expr}"); // includes the sign of zero
    }
}

#[test]
fn numeric_ordering_compares_signed_zeros_as_equal() {
    let mut vm = TestVm::default();
    for (a, b, less, greater) in [
        ("1", "2", true, false),
        ("2", "2", false, false),
        ("2", "1", false, true),
        ("0", "-0", false, false),
        ("-0", "0", false, false),
    ] {
        for (op, expected) in [("<", less), (">", greater), ("<=", !greater), (">=", !less)] {
            vm.interpret(&format!("var ordered = {a} {op} {b};"));
            assert_eq!(vm.boolean("ordered"), expected, "{a} {op} {b}");
        }
    }
}

#[test]
fn truthiness_is_shared_by_not_branches_and_value_returning_logic() {
    let mut vm = TestVm::default();
    vm.interpret("fun f() {} class C {}");
    for (expr, truth) in [
        ("false", false),
        ("nil", false),
        ("true", true),
        ("0", true),
        ("\"\"", true),
        ("f", true),
        ("C", true),
        ("C()", true),
    ] {
        vm.interpret(&format!(
            "var x = {expr}; var no = !x; var yes = !!x;
            var branch = false; if (x) branch = true;
            var both = x and 42; var either = x or 42;"
        ));
        let x = vm.global("x");
        let rhs = Value::number(42.0);
        assert_eq!(vm.boolean("no"), !truth, "!{expr}");
        assert_eq!(vm.boolean("yes"), truth, "!!{expr}");
        assert_eq!(vm.boolean("branch"), truth, "if ({expr})");
        assert!(
            vm.global("both")
                .values_equal(if truth { &rhs } else { &x }),
            "{expr} and 42"
        );
        assert!(
            vm.global("either")
                .values_equal(if truth { &x } else { &rhs }),
            "{expr} or 42"
        );
    }
}

#[test]
fn logical_chains_stop_before_the_next_side_effect() {
    let mut vm = TestVm::default();
    vm.interpret(
        "var n = 0; fun mark(v) { n = n + 1; return v; }
        var a = mark(1) and mark(false) and mark(3); var afterAnd = n;
        var b = mark(nil) or mark(0) or mark(3); var afterOr = n;
        var lastAnd = 1 and 2 and 3; var lastOr = false or nil or false;",
    );
    assert!(!vm.boolean("a"));
    assert_eq!(vm.number("afterAnd"), 2.0);
    assert_eq!(vm.number("b"), 0.0);
    assert_eq!(vm.number("afterOr"), 4.0);
    assert_eq!(vm.number("lastAnd"), 3.0);
    assert!(!vm.boolean("lastOr"));
}

#[test]
fn strings_preserve_unicode_newlines_and_empty_operands() {
    let mut vm = TestVm::default();
    vm.interpret("var text = \"¶\nॐ\n☃\"; var wrapped = \"(\" + \"\" + \")\";");
    assert_eq!(vm.text("text"), "¶\nॐ\n☃");
    assert_eq!(vm.text("wrapped"), "()");
}

#[test]
fn print_formats_lox_values_including_functions_and_bound_methods() {
    let mut vm = TestVm::default();
    vm.interpret("fun work() {} class Job { step() {} } var job = Job(); var step = job.step;
        print nil; print true; print -0; print \"a\nb\"; print work; print clock; print Job; print job; print step;");
    assert!(vm.global("work").try_as_closure().is_ok());
    assert!(vm.global("clock").try_as_native().is_ok());
    assert!(vm.global("step").try_as_method().is_ok());
    assert_eq!(
        vm.out,
        b"nil\ntrue\n-0\na\nb\n<fn work>\n<native fn>\nJob\nJob instance\n<fn step>\n"
    );
}
