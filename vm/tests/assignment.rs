use vm::vm::Vm;

#[test]
fn associativity() {
    let mut vm = Vm::new();
    let source = r#"
        var a = "a";
        var b = "b";
        var c = "c";

        // Assignment is right-associative.
        a = b = c;
        if (a != "c") fail;
        if (b != "c") fail;
        if (c != "c") fail;
    "#;
    assert!(vm.run(source, None::<&mut String>).is_ok());
}

#[test]
fn global() {
    let mut vm = Vm::new();
    let source = r#"
        var a = "before";
        if (a != "before") fail;

        a = "after";
        if (a != "after") fail;

        if ((a = "arg") != "arg") fail;
        if (a != "arg") fail;
    "#;
    assert!(vm.run(source, None::<&mut String>).is_ok());
}

#[test]
fn grouping() {
    let mut vm = Vm::new();
    let source = r#"
        var a = "a";
        (a) = "value"; // Error at '=': Invalid assignment target.
    "#;
    assert!(vm.run(source, None::<&mut String>).is_err());
}

#[test]
fn infix_operator() {
    let mut vm = Vm::new();
    let source = r#"
        var a = "a";
        var b = "b";
        a + b = "value"; // Error at '=': Invalid assignment target.
    "#;
    assert!(vm.run(source, None::<&mut String>).is_err());
}

#[test]
fn local() {
    let mut vm = Vm::new();
    let source = r#"
        {
          var a = "before";
          if (a != "before") fail;

          a = "after";
          if (a != "after") fail;

          if ((a = "arg") != "arg") fail;
          if (a != "arg") fail;
        }
    "#;
    assert!(vm.run(source, None::<&mut String>).is_ok());
}

#[test]
fn prefix_operator() {
    let mut vm = Vm::new();
    let source = r#"
        var a = "a";
        !a = "value"; // Error at '=': Invalid assignment target.
    "#;
    assert!(vm.run(source, None::<&mut String>).is_err());
}

#[test]
fn syntax() {
    let mut vm = Vm::new();
    let source = r#"
        // Assignment on RHS of variable.
        var a = "before";
        var c = a = "var";
        if (a != "var") fail;
        if (c != "var") fail;
    "#;
    assert!(vm.run(source, None::<&mut String>).is_ok());
}

#[test]
fn to_this() {
    let mut vm = Vm::new();
    let source = r#"
        class Foo {
          Foo() {
            this = "value"; // Error at '=': Invalid assignment target.
          }
        }

        Foo();
    "#;
    assert!(vm.run(source, None::<&mut String>).is_err());
}

#[test]
fn undefined() {
    let mut vm = Vm::new();
    let source = r#"
        unknown = "what"; // expect runtime error: Undefined variable 'unknown'.
    "#;
    assert!(vm.run(source, None::<&mut String>).is_err());
}
