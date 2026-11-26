use super::*;

#[test]
fn functions_and_methods_bind_each_positional_argument_to_its_own_slot() {
    let mut vm = TestVm::default();
    for count in 0..=8 {
        let params = (1..=count)
            .map(|i| format!("a{i}"))
            .collect::<Vec<_>>()
            .join(",");
        let args = (1..=count)
            .map(|i| i.to_string())
            .collect::<Vec<_>>()
            .join(",");
        let digits = (1..=count).fold(String::from("0"), |s, i| format!("({s} * 10 + a{i})"));
        let expected = (1..=count).fold(0, |n, i| n * 10 + i) as f64;
        vm.interpret(&format!("fun f({params}) {{ return {digits}; }} class C {{ m({params}) {{ return {digits}; }} }}
            var fromFunction = f({args}); var fromMethod = C().m({args});"));
        assert_eq!(vm.number("fromFunction"), expected, "{count} arguments");
        assert_eq!(vm.number("fromMethod"), expected, "{count} arguments");
    }
}

#[test]
fn recursion_resolves_global_local_and_mutually_recursive_functions() {
    let mut vm = TestVm::default();
    vm.interpret(
        "
        fun fib(n) { if (n < 2) return n; return fib(n-1) + fib(n-2); }
        var global = fib(8); var local;
        { fun product(n) { if (n == 0) return 1; return n * product(n-1); } local = product(5); }
        fun even(n) { if (n == 0) return true; return odd(n-1); }
        fun odd(n) { if (n == 0) return false; return even(n-1); }
        var isEven = even(4); var isOdd = odd(4);
    ",
    );
    assert_eq!(vm.number("global"), 21.0);
    assert_eq!(vm.number("local"), 120.0);
    assert!(vm.boolean("isEven"));
    assert!(!vm.boolean("isOdd"));
}

#[test]
fn a_call_result_can_be_the_callee_of_the_next_call() {
    let mut vm = TestVm::default();
    vm.interpret(
        "fun pass(f) { return f; } fun apply(f, x) { return pass(f)(x); }
        fun tag(x) { return x + \"!\"; } var message = apply(tag, \"ok\");",
    );
    assert_eq!(vm.text("message"), "ok!");
}

#[test]
fn empty_bodies_and_bare_returns_yield_nil_and_explicit_returns_stop_execution() {
    let mut vm = TestVm::default();
    vm.interpret(
        "fun empty() {} fun bare() { return; missing; } fun yes() { return true; missing; }
        class C { empty() {} no() { return false; missing; } }
        var a = empty(); var b = bare(); var c = C().empty(); var y = yes(); var n = C().no();",
    );
    for name in ["a", "b", "c"] {
        assert!(vm.global(name).is_nil(), "{name}");
    }
    assert!(vm.boolean("y"));
    assert!(!vm.boolean("n"));
}

#[test]
fn returned_closures_keep_function_and_method_parameters_after_return() {
    let mut vm = TestVm::default();
    vm.interpret(
        "fun capture(x) { fun read() { return x; } return read; }
        class C { capture(x) { fun read() { return x; } return read; } }
        var a = capture(7); var b = C().capture(false); var x = a(); var y = b();",
    );
    for name in ["a", "b"] {
        let closure = vm.global(name).try_as_closure().unwrap();
        let capture = vm.objects[closure].upvalues[0];
        assert!(matches!(vm.objects[capture], ObjUpvalue::Closed(_)));
    }
    assert_eq!(vm.number("x"), 7.0);
    assert!(!vm.boolean("y"));
}

#[test]
fn sibling_closures_share_the_same_cell_before_and_after_scope_exit() {
    let mut vm = TestVm::default();
    vm.interpret(
        "var get; var set; var initial; var open;
        { var x = 1; fun read() { return x; } fun write(v) { return x = v; }
          get = read; set = write; initial = get(); set(2); open = x; }
        var closed = get(); var assigned = set(3); var after = get();",
    );
    let [get, set] = ["get", "set"].map(|name| vm.global(name).try_as_closure().unwrap());
    assert_eq!(vm.objects[get].upvalues, vm.objects[set].upvalues);
    let cell = vm.objects[get].upvalues[0];
    let ObjUpvalue::Closed(v) = vm.objects[cell] else {
        panic!("local still open")
    };
    assert_eq!(v.try_as_number(), Ok(3.0));
    assert_eq!(
        ["initial", "open", "closed", "assigned", "after"].map(|n| vm.number(n)),
        [1., 2., 2., 3., 3.]
    );
}

#[test]
fn nested_closures_forward_distinct_locals_across_multiple_frames() {
    let mut vm = TestVm::default();
    vm.interpret(
        "fun first(a) { fun second(b) { fun third(c) {
             fun read() { return c * 100 + b * 10 + a; } return read;
        } return third; } return second; }
        var read = first(1)(2)(3); var result = read();",
    );
    let read = vm.global("read").try_as_closure().unwrap();
    assert_eq!(vm.objects[read].upvalues.len(), 3);
    assert_eq!(vm.number("result"), 321.0);
}

#[test]
fn capture_order_and_shadowing_do_not_change_which_closed_slot_is_read() {
    let mut vm = TestVm::default();
    vm.interpret(
        "var saved; var before; var shadow; var open; var reused;
        { var low = 2; var high = 7;
          fun read() { { before = high; var high = false; shadow = high; } return high * 10 + low; }
          saved = read; open = read(); }
        { var low = 6; var high = 4; reused = saved(); }",
    );
    assert_eq!(vm.number("before"), 7.0);
    assert!(!vm.boolean("shadow"));
    assert_eq!(vm.number("open"), 72.0);
    assert_eq!(vm.number("reused"), 72.0);
    let saved = vm.global("saved").try_as_closure().unwrap();
    assert_eq!(vm.objects[saved].upvalues.len(), 2); // repeated read of high is one capture
}

#[test]
fn skipped_closures_do_not_close_a_lower_live_local() {
    let mut vm = TestVm::default();
    vm.interpret(
        "{ var x; if (false) { fun skipped() { return x; } } }
        var saved; var during;
        { var low = 1;
          { var high; fun read() { return low; } saved = read;
            if (false) { fun skipped() { return high; } } }
          low = 2; during = saved(); }
        var after = saved();",
    );
    assert_eq!(vm.number("during"), 2.0);
    assert_eq!(vm.number("after"), 2.0);
}

#[test]
fn implicit_return_after_a_captured_assignment_preserves_the_callers_argument() {
    // clox regression/40: the callback writes an upvalue and falls off its end.
    let mut vm = TestVm::default();
    vm.interpret(
        "var saved; var stillCallable; var changed;
        fun call(f) { f(); stillCallable = f; }
        fun outer() { var cell = 1; var other = 2;
            fun step() { cell = 3; } saved = step; call(step); changed = cell; }
        outer(); var result = saved();",
    );
    let saved = vm.global("saved").try_as_closure().unwrap();
    assert_eq!(vm.global("stillCallable").try_as_closure(), Ok(saved));
    assert!(vm.global("result").is_nil());
    assert_eq!(vm.number("changed"), 3.0);
}
