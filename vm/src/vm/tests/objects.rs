use super::*;

#[test]
fn class_names_resolve_to_the_class_in_both_local_and_global_methods() {
    let mut vm = TestVm::default();
    vm.interpret(
        "var word = 19; class Top { type() { return Top; } word() { return word; } }
        var top = Top().type(); var fromGlobal = Top().word();
        fun make() { class Local < Top { type() { return Local; } } return Local; }
        var child = make(); var o = child(); var local = o.type(); var inherited = o.word();",
    );
    let top = vm.global("Top").try_as_class().unwrap();
    let child = vm.global("child").try_as_class().unwrap();
    assert_ne!(child, top);
    assert_eq!(vm.global("top").try_as_class(), Ok(top));
    assert_eq!(vm.global("local").try_as_class(), Ok(child));
    assert_eq!(&vm.objects[vm.objects[child].name], "Local");
    assert_eq!(vm.number("fromGlobal"), 19.0);
    assert_eq!(vm.number("inherited"), 19.0);
}

#[test]
fn a_local_subclass_survives_cleanup_of_its_synthetic_super_slot() {
    // clox regression/394: both class names are locals in the same block.
    let mut vm = TestVm::default();
    vm.interpret("var saved; var seen; { class Parent {} class Child < Parent {} saved = Child; seen = Child(); }");
    let child = vm.global("saved").try_as_class().unwrap();
    assert_eq!(&vm.objects[vm.objects[child].name], "Child");
    let seen = vm.global("seen").try_as_instance().unwrap();
    assert_eq!(vm.objects[seen].class, child);
}

#[test]
fn class_and_bound_method_equality_use_identity() {
    let mut vm = TestVm::default();
    vm.interpret("class A { m() {} } class B {} var o = A(); var m = o.m; var fresh = o.m;");
    for (a, b, expected) in [
        ("A", "A", true),
        ("B", "B", true),
        ("A", "B", false),
        ("B", "A", false),
        ("A", "\"A\"", false),
        ("A", "nil", false),
        ("A", "123", false),
        ("A", "true", false),
        ("m", "m", true),
        ("m", "fresh", false),
        ("o.m", "o.m", false),
    ] {
        vm.interpret(&format!("var eq = {a} == {b};"));
        assert_eq!(vm.boolean("eq"), expected, "{a} == {b}");
    }
    let [m, fresh] = ["m", "fresh"].map(|n| vm.global(n).try_as_method().unwrap());
    assert_ne!(m, fresh);
    assert_eq!(vm.objects[m].method, vm.objects[fresh].method);
    assert_eq!(
        vm.objects[m].receiver.try_as_instance(),
        vm.objects[fresh].receiver.try_as_instance()
    );
}

#[test]
fn calling_init_explicitly_reinitializes_and_returns_the_same_instance() {
    let mut vm = TestVm::default();
    vm.interpret(
        "var falls = 0; class C { init(a,b) { this.a = a; this.b = b; if (a) return; falls = falls + 1; } }
        var obj = C(true, 3); var initialA = obj.a; var initialB = obj.b;
        var early = obj.init(true, 4); var later = obj.init(false, 7);
        class Empty {} var empty = Empty();",
    );
    let obj = vm.global("obj").try_as_instance().unwrap();
    assert!(vm.boolean("initialA"));
    assert_eq!(vm.number("initialB"), 3.0);
    assert_eq!(vm.global("early").try_as_instance(), Ok(obj));
    assert_eq!(vm.global("later").try_as_instance(), Ok(obj));
    assert_eq!(vm.field(obj, "a").try_as_bool(), Ok(false));
    assert_eq!(vm.field(obj, "b").try_as_number(), Ok(7.0));
    assert_eq!(vm.number("falls"), 1.0); // only the third call got past `return;`
    let empty = vm.global("empty").try_as_instance().unwrap();
    assert_eq!(
        vm.global("Empty").try_as_class(),
        Ok(vm.objects[empty].class)
    );
    assert!(vm.objects[empty].fields.is_empty());
}

#[test]
fn only_a_method_named_init_is_an_initializer() {
    let mut vm = TestVm::default();
    vm.interpret(
        "fun init() { return false; }
        class C { init() { fun init() { return 23; } this.fromNested = init(); } }
        var ordinary = init(); var obj = C();",
    );
    let obj = vm.global("obj").try_as_instance().unwrap();
    assert!(!vm.boolean("ordinary"));
    assert_eq!(vm.field(obj, "fromNested").try_as_number(), Ok(23.0));
}

#[test]
fn inherited_methods_and_initializers_use_the_subclass_instance() {
    let mut vm = TestVm::default();
    vm.interpret("class A { init(x) { this.x = x; } get() { return this.x; } set(x) { this.x = x; this.y = x+10; }
                           pick() { return 1; } root() { return this; } }
        class B < A { pick() { return 2; } change(x) { this.x = x; this.y = x+20; } }
        class C < B { leaf() { return 3; } }
        var c = C(4); var initial = c.get(); c.set(5); var baseWrite = c.x; var baseY = c.y;
        c.change(6); var derivedWrite = c.get(); var overridden = c.pick(); var leaf = c.leaf(); var root = c.root();");
    let c = vm.global("c").try_as_instance().unwrap();
    assert_eq!(vm.global("C").try_as_class(), Ok(vm.objects[c].class));
    assert_eq!(vm.global("root").try_as_instance(), Ok(c));
    assert_eq!(
        ["initial", "baseWrite", "derivedWrite", "overridden", "leaf"].map(|n| vm.number(n)),
        [4., 5., 6., 2., 3.]
    );
    assert_eq!(vm.field(c, "x").try_as_number(), Ok(6.0));
    assert_eq!(vm.number("baseY"), 15.0);
    assert_eq!(vm.field(c, "y").try_as_number(), Ok(26.0));
}

#[test]
fn super_init_writes_base_and_derived_fields_on_one_receiver() {
    let mut vm = TestVm::default();
    vm.interpret(
        "class A { init(x) { this.x = x; } } class B < A {
        init(x,y) { this.same = super.init(x); this.y = y; } } var b = B(1, false);",
    );
    let b = vm.global("b").try_as_instance().unwrap();
    assert_eq!(vm.field(b, "same").try_as_instance(), Ok(b));
    assert_eq!(vm.field(b, "x").try_as_number(), Ok(1.0));
    assert_eq!(vm.field(b, "y").try_as_bool(), Ok(false));
}

#[test]
fn many_named_fields_can_be_written_and_read_independently() {
    // Split writes and reads into functions so this exercises the field table,
    // rather than the independent one-byte limit on constants per chunk.
    let mut vm = TestVm::default();
    let set = (0..80)
        .map(|i| format!("o.key{i} = {i};"))
        .collect::<String>();
    let copy = (0..80)
        .map(|i| format!("dst.key{i} = src.key{i};"))
        .collect::<String>();
    vm.interpret(&format!("class Record {{}} var original = Record(); var copied = Record();
        fun fill(o) {{ {set} }} fun copy(src,dst) {{ {copy} }} fill(original); copy(original,copied);"));
    for name in ["original", "copied"] {
        let record = vm.global(name).try_as_instance().unwrap();
        assert_eq!(vm.objects[record].fields.len(), 80);
        for i in 0..80 {
            assert_eq!(
                vm.field(record, &format!("key{i}")).try_as_number(),
                Ok(i as f64)
            );
        }
    }
}

#[test]
fn function_fields_shadow_methods_and_old_bound_methods_keep_their_receiver() {
    let mut vm = TestVm::default();
    vm.interpret(
        "class C { pick(n) { return this.id + n; } other(n) { return this.id - n; } }
        fun free(a,b) { return a * 10 + b; }
        var a = C(); a.id = 10; var b = C(); b.id = 90;
        var saved = a.pick; a.pick = a.other; var shadow = a.pick(2);
        b.pick = saved; var moved = b.pick(3); var detached = saved(4);
        b.pick = free; var called = b.pick(5,6);",
    );
    let a = vm.global("a").try_as_instance().unwrap();
    let saved = vm.global("saved").try_as_method().unwrap();
    assert_eq!(vm.objects[saved].receiver.try_as_instance(), Ok(a));
    assert_eq!(
        ["shadow", "moved", "detached", "called"].map(|n| vm.number(n)),
        [8., 13., 14., 56.]
    );
}

#[test]
fn inherited_super_calls_and_escaped_bindings_use_the_defining_class() {
    let mut vm = TestVm::default();
    vm.interpret("class Base { pick(n) { return this.id + n; } } class Mid < Base {
          pick(n) { return super.pick(n) + 10; } other(n) { return super.pick(n); }
          bind() { return super.pick; } later() { fun f(n) { return super.pick(n); } return f; } }
        class Leaf < Mid { pick(n) { return 100; } }
        class Replacement { pick(n) { return 200; } }
        var mid = Mid(); mid.id = 1; var same = mid.pick(2); var direct = mid.later()(3);
        var leaf = Leaf(); leaf.id = 4; var before = leaf.other(1); var bound = leaf.bind(); var later = leaf.later();
        Base = Replacement; var after = leaf.other(1); var binding = bound(2); var closure = later(3);");
    let leaf = vm.global("leaf").try_as_instance().unwrap();
    let bound = vm.global("bound").try_as_method().unwrap();
    assert_eq!(vm.objects[bound].receiver.try_as_instance(), Ok(leaf));
    assert_eq!(
        ["same", "direct", "before", "after", "binding", "closure"].map(|n| vm.number(n)),
        [13., 4., 5., 5., 6., 7.]
    );
}

#[test]
fn super_invokes_the_nearest_inherited_implementation_across_an_empty_parent() {
    let mut vm = TestVm::default();
    vm.interpret(
        "class A { key() { return 5; } } class B < A {} class C < B {
        key() { return super.key() + 2; } } var key = C().key();",
    );
    assert_eq!(vm.number("key"), 7.0);
}

#[test]
fn this_is_captured_through_nested_functions_but_rebound_in_a_nested_class() {
    let mut vm = TestVm::default();
    vm.interpret("var inner; var nested; var enclosing;
        class Outer { me() { return this; } once() { fun get() { return this.me(); } return get; } capture() {
            fun a() { enclosing = this; class Inner { me() { return this; } }
                inner = Inner(); nested = inner.me();
                fun b() { fun c() { return this.me(); } return c; } return b; } return a; } }
        var outer = Outer(); var direct = outer.me().me(); var get = outer.once(); var single = get();
        var closure = outer.capture(); var escaped = closure()()();");
    let outer = vm.global("outer").try_as_instance().unwrap();
    for name in ["direct", "single", "enclosing", "escaped"] {
        assert_eq!(vm.global(name).try_as_instance(), Ok(outer), "{name}");
    }
    let inner = vm.global("inner").try_as_instance().unwrap();
    assert_ne!(outer, inner);
    assert_eq!(vm.global("nested").try_as_instance(), Ok(inner));
}

#[test]
fn field_assignment_yields_the_value_just_stored_without_disturbing_other_fields() {
    let mut vm = TestVm::default();
    vm.interpret(
        "class C {} var o = C(); var a = o.x = 3; var b = o.y = false;
        var x = o.x; var y = o.y;",
    );
    let o = vm.global("o").try_as_instance().unwrap();
    assert_eq!(vm.number("a"), 3.0);
    assert!(!vm.boolean("b"));
    assert_eq!(vm.number("x"), 3.0);
    assert!(!vm.boolean("y"));
    assert_eq!(vm.field(o, "x").try_as_number(), Ok(3.0));
    assert_eq!(vm.field(o, "y").try_as_bool(), Ok(false));
}
