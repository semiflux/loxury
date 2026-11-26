use super::*;

#[test]
fn assignment_chains_store_and_yield_the_same_value() {
    let mut vm = TestVm::default();
    vm.interpret(
        r#"
        var a = "before"; var b = false; var c = 3;
        var before = a; var chain = a = b = c;
        var localBefore; var localAfter; var yielded;
        { var a = true; localBefore = a; yielded = a = "3"; localAfter = a; }
    "#,
    );
    assert_eq!(vm.text("before"), "before");
    for name in ["a", "b", "c", "chain"] {
        assert_eq!(vm.number(name), 3.0);
    }
    assert!(vm.boolean("localBefore"));
    for name in ["yielded", "localAfter"] {
        assert_eq!(vm.text(name), "3");
    }
}

#[test]
fn block_bindings_apply_from_the_declaration_until_the_end_of_the_block() {
    let mut vm = TestVm::default();
    vm.interpret(
        "
        var x = 1; var outer; var inner; var restored; var reused; var prior;
        { var x = 2; { outer = x; var x = 3; inner = x; } restored = x; }
        { var x = 4; var y = x + 1; var z = x + y; reused = z; }
        { prior = x; var x; }
    ",
    );
    for (name, expected) in [
        ("outer", 2.0),
        ("inner", 3.0),
        ("restored", 2.0),
        ("reused", 9.0),
        ("prior", 1.0),
        ("x", 1.0),
    ] {
        assert_eq!(vm.number(name), expected, "{name}");
    }
}

#[test]
fn global_redeclaration_can_read_the_previous_binding_or_reset_it_to_nil() {
    let mut vm = TestVm::default();
    vm.interpret("var fresh; var x = 1; var x = x + 1; var preserved = x; var x; var reset = x; var x = true;");
    assert!(vm.global("fresh").is_nil() && vm.global("reset").is_nil());
    assert_eq!(vm.number("preserved"), 2.0);
    assert!(vm.boolean("x"));
}

#[test]
fn closures_resolve_reads_and_writes_before_a_later_shadowing_declaration() {
    let mut vm = TestVm::default();
    vm.interpret(
        "
        var x = 1; var before; var after; var local;
        { fun read() { return x; } fun write(v) { x = v; }
          before = read(); var x = false; write(2); after = read(); local = x; }
    ",
    );
    assert_eq!(vm.number("before"), 1.0);
    assert_eq!(vm.number("after"), 2.0);
    assert_eq!(vm.number("x"), 2.0);
    assert!(!vm.boolean("local"));
}

#[test]
fn dangling_else_binds_to_the_inner_if_and_skipped_branches_have_no_effect() {
    let mut vm = TestVm::default();
    vm.interpret(
        "
        {} if (true) {} if (false) {} else {}
        var assigned = false; var branch;
        if (assigned = true) if (false) missing; else { branch = 1; }
        if (false) if (true) missing; else missing;
        if (nil) { missing; } else { branch = branch + 2; }
    ",
    );
    assert!(vm.boolean("assigned"));
    assert_eq!(vm.number("branch"), 3.0);
}

#[test]
fn for_initializer_and_body_each_introduce_a_scope() {
    let mut vm = TestVm::default();
    vm.interpret(
        "
        var i = 8; var first; var inner; var after; var reused;
        { for (var i = 0; i < 1; i = i + 1) {
              first = i; var i = -4; inner = i;
          }
          after = i; var i; for (i = 2; i < 3; i = i + 1) {} reused = i; }
    ",
    );
    for (name, n) in [
        ("i", 8.),
        ("first", 0.),
        ("inner", -4.),
        ("after", 8.),
        ("reused", 3.),
    ] {
        assert_eq!(vm.number(name), n, "{name}");
    }
}

#[test]
fn omitted_for_clauses_and_single_statement_loop_bodies() {
    let mut vm = TestVm::default();
    vm.interpret(
        "
        var i = 1; var digits = 0;
        for (; i <= 3; i = i + 1) digits = digits * 10 + i;
        var n = 0; for (var j = 3; j > 0;) { n = n + 1; j = j - 1; }
        fun noCondition() { for (var j = 0;; j = j + 1) if (j == 2) return j; }
        var two = noCondition();
        var w = 0; while (w < 3) w = w + 1;
        for (; false;) if (true) missing; else missing;
        for (; false;) while (true) missing;
        for (; false;) for (;;) missing;
        while (false) if (true) missing; else missing;
        while (false) while (true) missing;
        while (false) for (;;) missing;
    ",
    );
    for (name, value) in [
        ("i", 4.),
        ("digits", 123.),
        ("n", 3.),
        ("two", 2.),
        ("w", 3.),
    ] {
        assert_eq!(vm.number(name), value, "{name}");
    }
}

#[test]
fn for_closures_share_the_counter_but_close_a_new_body_local_each_iteration() {
    let mut vm = TestVm::default();
    vm.interpret(
        "
        var a; var b; var c;
        for (var i = 1; i <= 3; i = i + 1) {
            var copy = i; fun read() { return i * 10 + copy; }
            if (i == 1) a = read; else if (i == 2) b = read; else c = read;
        }
        var x = a(); var y = b(); var z = c();
    ",
    );
    let [a, b, c] = ["a", "b", "c"].map(|n| vm.global(n).try_as_closure().unwrap());
    let cells = [a, b, c].map(|h| vm.objects[h].upvalues.as_slice());
    assert_eq!(cells[0][0], cells[1][0]);
    assert_eq!(cells[1][0], cells[2][0]);
    assert_ne!(cells[0][1], cells[1][1]);
    assert_ne!(cells[1][1], cells[2][1]);
    assert_eq!(["x", "y", "z"].map(|n| vm.number(n)), [41., 42., 43.]);
}

#[test]
fn while_closures_keep_the_value_from_their_own_iteration() {
    let mut vm = TestVm::default();
    vm.interpret(
        "
        var i = 0; var a; var b; var c;
        while (i < 3) { var copy = i; fun read() { return copy; }
            if (i == 0) a = read; else if (i == 1) b = read; else c = read;
            i = i + 1; }
        var x = a(); var y = b(); var z = c();
    ",
    );
    assert_eq!(["x", "y", "z"].map(|n| vm.number(n)), [0., 1., 2.]);
}

#[test]
fn returning_from_a_branch_or_loop_unwinds_the_frame_and_its_captures() {
    let mut vm = TestVm::default();
    for header in [
        "if (true)",
        "if (false) nil; else",
        "for (;;)",
        "while (true)",
    ] {
        vm.interpret(&format!(
            "
            fun f(capture) {{ {header} {{ var local = 17; fun get() {{ return local; }}
                if (capture) return get; return local; }} missing; }}
            var plain = f(false); var saved = f(true); var read = saved();"
        ));
        assert_eq!(vm.number("plain"), 17.0, "{header}");
        let saved = vm.global("saved").try_as_closure().unwrap();
        let cell = vm.objects[saved].upvalues[0];
        assert!(
            matches!(vm.objects[cell], ObjUpvalue::Closed(_)),
            "{header}"
        );
        assert_eq!(vm.number("read"), 17.0, "{header}");
    }
}
