use super::*;
use crate::location::Coords;
use FunctionKind::Script;
use OpCode::*;

mod calls;
mod control;
mod errors;
mod objects;
mod values;

type TestVm = Vm<Vec<u8>, Vec<u8>>;

impl TestVm {
    // Multi-instruction behaviors use the real compiler to build a valid chunk.
    // A compile error is always a test failure, never a substitute for RunError.
    #[track_caller]
    fn execute_source(&mut self, source: &str) -> Result<(), AtCoords<RunError>> {
        let compiled = Compiler::new(
            &mut Lexer::new(source).peekable(),
            &mut self.objects,
            Script,
            &mut self.err,
        )
        .compile();
        let script = compiled
            .unwrap_or_else(|()| panic!("{source}\n{}", String::from_utf8_lossy(&self.err)));
        self.run_function(script)
    }

    #[track_caller]
    fn interpret(&mut self, source: &str) {
        self.execute_source(source)
            .unwrap_or_else(|e| panic!("{source}\n{e:?}"));
        assert!(self.stack.is_empty() && self.frames.is_empty() && self.open_upvalues.is_empty());
    }

    #[track_caller]
    fn global(&mut self, name: &str) -> Value {
        let key = self.objects.alloc(name.to_owned());
        *self
            .globals
            .get(&key)
            .unwrap_or_else(|| panic!("missing global {name}"))
    }

    #[track_caller]
    fn number(&mut self, name: &str) -> f64 {
        self.global(name).try_as_number().expect(name)
    }

    #[track_caller]
    fn boolean(&mut self, name: &str) -> bool {
        self.global(name).try_as_bool().expect(name)
    }

    #[track_caller]
    fn text(&mut self, name: &str) -> &str {
        let key = self.global(name).try_as_string().expect(name);
        &self.objects[key]
    }

    #[track_caller]
    fn field(&mut self, instance: GcHandle<Instance>, name: &str) -> Value {
        let key = self.objects.alloc(name.to_owned());
        self.objects[instance].fields[&key]
    }
}

// Use actual VM instructions without the compiler. Each instruction and its
// operands have their own synthetic source line so errors identify the site.
fn bytecode<const N: usize>(
    kind: FunctionKind,
    constants: &[Value],
    instructions: [(OpCode, &[u8]); N],
) -> Function {
    let mut f = Function::new(kind);
    f.chunk.constants.extend_from_slice(constants);
    for (i, (op, operands)) in instructions.into_iter().enumerate() {
        let pos = Coords::new((i + 1).try_into().unwrap(), 1);
        f.chunk.write(op as u8, pos);
        for &byte in operands {
            f.chunk.write(byte, pos);
        }
    }
    f
}

fn binary(
    vm: &mut TestVm,
    left: Value,
    op: OpCode,
    right: Value,
) -> Result<Value, AtCoords<RunError>> {
    let result = vm.objects.alloc("result".to_owned());
    let script = bytecode(
        Script,
        &[left, right, Value::string(result)],
        [
            (Constant, &[0]),
            (Constant, &[1]),
            (op, &[]),
            (DefineGlobal, &[2]),
            (Nil, &[]),
            (Return, &[]),
        ],
    );
    vm.run_function(script)?;
    assert!(vm.stack.is_empty());
    assert!(vm.frames.is_empty());
    Ok(vm.globals[&result])
}

#[test]
fn numeric_opcodes_consume_two_numbers_in_source_order() {
    let mut vm = TestVm::default();
    for (op, expected) in [
        (Add, 10.0),
        (Subtract, 6.0),
        (Multiply, 16.0),
        (Divide, 4.0),
    ] {
        let result = binary(&mut vm, Value::number(8.0), op, Value::number(2.0)).unwrap();
        assert_eq!(result.try_as_number(), Ok(expected));
    }
}

#[test]
fn comparisons_push_booleans_and_do_not_coerce_types() {
    let mut vm = TestVm::default();
    let n = Value::number(1.0);
    for (left, op, right, expected) in [
        (n, Greater, Value::number(-3.0), true),
        (n, Less, Value::number(-3.0), false),
        (n, Equal, Value::boolean(true), false),
        (Value::number(-0.0), Equal, Value::number(0.0), true),
        (
            Value::number(f64::NAN),
            Equal,
            Value::number(f64::NAN),
            false,
        ),
    ] {
        assert_eq!(
            binary(&mut vm, left, op, right).unwrap().try_as_bool(),
            Ok(expected)
        );
    }
}

#[test]
fn add_concatenates_string_values_without_changing_the_operands() {
    let mut vm = TestVm::default();
    let a = vm.objects.alloc("left".to_owned());
    let b = vm.objects.alloc("right".to_owned());
    let joined = binary(&mut vm, Value::string(a), Add, Value::string(b))
        .unwrap()
        .try_as_string()
        .unwrap();
    assert_eq!(&vm.objects[joined], "leftright");
    assert_eq!(&vm.objects[a], "left");
    assert_eq!(&vm.objects[b], "right");
}

#[test]
fn invalid_operands_report_the_failing_opcode() {
    for (op, error) in [
        (Add, RunError::ExpectedNumbersOrStrings),
        (Subtract, RunError::ExpectedNumbers),
    ] {
        let mut vm = TestVm::default();
        let err = binary(&mut vm, Value::nil(), op, Value::number(1.0)).unwrap_err();
        assert_eq!(err, AtCoords::at(3, 1, error));
    }
}

#[test]
fn jump_if_false_leaves_the_condition_on_the_stack() {
    let mut vm = TestVm::default();
    let result = vm.objects.alloc("result".to_owned());
    let empty = Value::string(vm.objects.alloc(String::new()));
    let fallback = Value::number(42.0);
    for (condition, expected) in [
        (Value::nil(), Value::nil()),
        (Value::boolean(false), Value::boolean(false)),
        (Value::number(0.0), fallback),
        (empty, fallback),
    ] {
        let script = bytecode(
            Script,
            &[condition, fallback, Value::string(result)],
            [
                (Constant, &[0]),
                (JumpIfFalse, &[0, 3]),
                (Pop, &[]),
                (Constant, &[1]), // replace the condition only on fallthrough
                (DefineGlobal, &[2]),
                (Nil, &[]),
                (Return, &[]),
            ],
        );
        vm.run_function(script).unwrap();
        assert!(vm.globals[&result].values_equal(&expected));
        assert!(vm.stack.is_empty());
    }
}

#[test]
fn returning_replaces_the_callee_and_arguments_but_preserves_caller_locals() {
    let mut vm = TestVm::default();
    let mut callee = bytecode(
        FunctionKind::Function,
        &[],
        [
            (GetLocal, &[1]),
            (GetLocal, &[2]),
            (Subtract, &[]),
            (SetLocal, &[1]),
            (Pop, &[]),
            (GetLocal, &[1]),
            (Return, &[]),
        ],
    );
    callee.arity = 2;
    let callee = vm.objects.alloc(callee);
    let result = vm.objects.alloc("result".to_owned());
    let saved = vm.objects.alloc("saved".to_owned());
    let script = bytecode(
        Script,
        &[
            Value::function(callee),
            Value::number(11.0),
            Value::number(4.0),
            Value::string(result),
            Value::string(saved),
        ],
        [
            (True, &[]), // caller's local in slot 1
            (Closure, &[0]),
            (Constant, &[1]),
            (Constant, &[2]),
            (Call, &[2]),
            (DefineGlobal, &[3]),
            (DefineGlobal, &[4]),
            (Nil, &[]),
            (Return, &[]),
        ],
    );
    vm.run_function(script).unwrap();
    assert_eq!(vm.globals[&result].try_as_number(), Ok(7.0));
    assert_eq!(vm.globals[&saved].try_as_bool(), Ok(true));
    assert!(vm.frames.is_empty());
    assert!(vm.stack.is_empty());
}

#[test]
fn native_call_reads_its_arguments_in_order_and_replaces_only_its_slots() {
    let mut vm = TestVm::default();
    vm.define_native("choose", 2, |args| {
        assert_eq!(args.len(), 2);
        assert!(args[0].is_nil());
        assert_eq!(args[1].try_as_number(), Ok(5.0));
        args[1]
    });
    let name = vm.objects.alloc("choose".to_owned());
    vm.stack.push(Value::boolean(true));
    vm.stack.push(vm.globals[&name]);
    vm.stack.push(Value::nil());
    vm.stack.push(Value::number(5.0));
    vm.call_value(2).unwrap();
    assert!(vm.frames.is_empty());
    assert_eq!(vm.stack.len(), 2);
    assert_eq!(vm.stack[0].try_as_bool(), Ok(true));
    assert_eq!(vm.stack[1].try_as_number(), Ok(5.0));
}

#[test]
fn calls_check_arity_before_entering_the_callee() {
    let mut vm = TestVm::default();
    let mut callee = bytecode(FunctionKind::Function, &[], [(Nil, &[]), (Return, &[])]);
    callee.arity = 2;
    let function = vm.objects.alloc(callee);
    let script = bytecode(
        Script,
        &[Value::function(function)],
        [(Closure, &[0]), (Nil, &[]), (Call, &[1]), (Return, &[])],
    );
    assert_eq!(
        vm.run_function(script),
        Err(AtCoords::at(3, 1, RunError::WrongArity(2, 1)))
    );
}

#[test]
fn recursive_call_reports_frame_overflow_at_the_call_site() {
    let mut vm = TestVm::default();
    let script = bytecode(Script, &[], [(GetLocal, &[0]), (Call, &[0]), (Return, &[])]);
    assert_eq!(
        vm.run_function(script),
        Err(AtCoords::at(2, 1, RunError::StackOverflow))
    );
}

#[test]
fn assigning_an_undefined_global_does_not_define_it() {
    let mut vm = TestVm::default();
    let missing = vm.objects.alloc("missing".to_owned());
    let script = bytecode(
        Script,
        &[Value::string(missing)],
        [(True, &[]), (SetGlobal, &[0]), (Return, &[])],
    );
    assert_eq!(
        vm.run_function(script),
        Err(AtCoords::at(
            2,
            1,
            RunError::UndefinedVariable("missing".into())
        ))
    );
    assert!(!vm.globals.contains_key(&missing));
}

#[test]
fn writing_a_property_updates_the_instance_and_leaves_the_assigned_value() {
    let mut vm = TestVm::default();
    let name = vm.objects.alloc("Box".to_owned());
    let class = vm.objects.alloc(crate::chunk::Class::new(name));
    let object = vm.objects.alloc(Instance::new(class));
    let field = vm.objects.alloc("field".to_owned());
    let assigned = vm.objects.alloc("assigned".to_owned());
    let read = vm.objects.alloc("read".to_owned());
    let script = bytecode(
        Script,
        &[
            Value::instance(object),
            Value::string(field),
            Value::string(assigned),
            Value::string(read),
        ],
        [
            (Constant, &[0]),
            (True, &[]),
            (SetProperty, &[1]),
            (DefineGlobal, &[2]),
            (Constant, &[0]),
            (GetProperty, &[1]),
            (DefineGlobal, &[3]),
            (Nil, &[]),
            (Return, &[]),
        ],
    );
    vm.run_function(script).unwrap();
    assert_eq!(vm.objects[object].fields[&field].try_as_bool(), Ok(true));
    assert_eq!(vm.globals[&assigned].try_as_bool(), Ok(true));
    assert_eq!(vm.globals[&read].try_as_bool(), Ok(true));
    assert!(vm.stack.is_empty());
}

#[test]
fn bound_call_installs_its_receiver_in_frame_slot_zero() {
    let mut vm = TestVm::default();
    let name = vm.objects.alloc("Box".to_owned());
    let class = vm.objects.alloc(crate::chunk::Class::new(name));
    let object = vm.objects.alloc(Instance::new(class));
    let method = vm.objects.alloc("self".to_owned());
    let f = bytecode(FunctionKind::Method, &[], [(GetLocal, &[0]), (Return, &[])]);
    let f = vm.objects.alloc(f);
    let closure = vm.objects.alloc(crate::chunk::Closure::new(f));
    vm.objects[class]
        .methods
        .insert(method, Value::closure(closure));

    vm.stack.push(Value::nil()); // caller slot below receiver
    vm.stack.push(Value::instance(object));
    vm.bind_method(class, method).unwrap();
    let bound = vm.stack[1].try_as_method().unwrap();
    assert_eq!(vm.objects[bound].receiver.try_as_instance(), Ok(object));
    vm.call_value(0).unwrap();
    assert_eq!(vm.frames.len(), 1);
    assert_eq!(vm.frames[0].base(), 1);
    assert_eq!(vm.frames[0].closure, closure);
    assert_eq!(vm.stack[1].try_as_instance(), Ok(object));
    assert!(vm.stack[0].is_nil());
}

#[test]
fn captures_share_a_cell_and_only_slots_leaving_scope_are_closed() {
    let mut vm = TestVm::default();
    vm.stack.push(Value::number(2.0));
    vm.stack.push(Value::number(3.0));
    let upper = vm.capture_upvalue(1);
    let lower = vm.capture_upvalue(0);
    let also_upper = vm.capture_upvalue(1);
    assert_eq!(upper, also_upper);
    assert_ne!(lower, upper);

    vm.stack.push(Value::number(5.0));
    vm.set_upvalue(also_upper);
    assert_eq!(vm.stack[1].try_as_number(), Ok(5.0));
    vm.stack.pop();
    vm.close_upvalues(1);
    assert_eq!(vm.open_upvalues, [lower]);
    assert!(matches!(vm.objects[lower], ObjUpvalue::Open(0)));
    vm.stack[1] = Value::nil(); // reuse the former local slot
    assert_eq!(vm.get_upvalue(upper).try_as_number(), Ok(5.0));
    vm.stack.push(Value::boolean(true));
    vm.set_upvalue(upper);
    assert_eq!(vm.get_upvalue(also_upper).try_as_bool(), Ok(true));
    assert!(vm.stack[1].is_nil());
}

#[test]
fn runtime_error_closes_an_escaped_local_before_the_next_run_reuses_its_slot() {
    let mut vm = TestVm::default();
    let saved = vm.objects.alloc("saved".to_owned());
    let missing = vm.objects.alloc("missing".to_owned());
    let mut getter = bytecode(
        FunctionKind::Function,
        &[],
        [(GetUpvalue, &[0]), (Return, &[])],
    );
    getter.upvalue_count = 1;
    let getter = vm.objects.alloc(getter);
    let script = bytecode(
        Script,
        &[
            Value::number(41.0),
            Value::function(getter),
            Value::string(saved),
            Value::string(missing),
        ],
        [
            (Constant, &[0]),
            (Closure, &[1, 1, 1]), // capture local slot 1
            (DefineGlobal, &[2]),
            (GetGlobal, &[3]),
            (Pop, &[]),
            (CloseUpvalue, &[]),
            (Nil, &[]),
            (Return, &[]),
        ],
    );
    assert_eq!(
        vm.run_function(script),
        Err(AtCoords::at(
            4,
            1,
            RunError::UndefinedVariable("missing".into())
        ))
    );
    assert!(vm.open_upvalues.is_empty());
    let closure = vm.globals[&saved].try_as_closure().unwrap();
    let cell = vm.objects[closure].upvalues[0];
    let ObjUpvalue::Closed(value) = vm.objects[cell] else {
        panic!("capture left open after error")
    };
    assert_eq!(value.try_as_number(), Ok(41.0));

    // Exercise the same reset/entry point as the REPL for its next input.
    vm.run(
        "var observed; { var replacement = false; observed = saved(); }",
        None::<&mut String>,
    )
    .unwrap();
    let observed = vm.objects.alloc("observed".to_owned());
    assert_eq!(vm.globals[&observed].try_as_number(), Ok(41.0));
    assert!(vm.frames.is_empty());
    assert!(vm.stack.is_empty());
}
