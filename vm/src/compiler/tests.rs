use super::*;
use OpCode as Op;

mod limits;
mod syntax;

fn at(row: u16, col: u16, error: CompileError) -> Diagnostic {
    Diagnostic::at_coords(row, col, error)
}

fn compile(source: &str, heap: &mut Heap) -> Result<Function, Vec<Diagnostic>> {
    let mut diagnostics = Vec::new();
    let result = Compiler::with_reporter(
        &mut Lexer::new(source).peekable(),
        heap,
        FunctionKind::Script,
        &mut diagnostics,
        |diagnostics, error| diagnostics.push(error.clone()),
    )
    .compile();
    match result {
        Ok(function) => {
            assert!(diagnostics.is_empty(), "{diagnostics:?}");
            Ok(function)
        }
        Err(()) => Err(diagnostics),
    }
}

fn compiled(source: &str) -> (Function, Heap) {
    let mut heap = Heap::default();
    let script = compile(source, &mut heap).unwrap();
    (script, heap)
}

fn diagnostics(source: &str) -> Vec<Diagnostic> {
    compile(source, &mut Heap::default()).unwrap_err()
}

fn nested<'a>(parent: &Function, heap: &'a Heap, name: &str) -> &'a Function {
    parent
        .chunk
        .constants
        .iter()
        .filter_map(|v| v.try_as_function().ok())
        .map(|h| &heap[h])
        .find(|f| &heap[f.name.unwrap()] == name)
        .expect(name)
}

// Read only the instructions under test. Resolve constant operands through the
// chunk and label jump destinations by position rather than snapshotting a dump.
struct Code<'a> {
    chunk: &'a Chunk,
    pc: usize,
}

impl<'a> Code<'a> {
    fn new(f: &'a Function) -> Self {
        Self {
            chunk: &f.chunk,
            pc: 0,
        }
    }

    fn byte(&mut self) -> u8 {
        let byte = self.chunk.byte_at(self.pc);
        self.pc += 1;
        byte
    }

    #[track_caller]
    fn op(&mut self, expected: Op) {
        let pc = self.pc;
        assert_eq!(self.byte(), expected as u8, "opcode at byte {pc}");
    }

    #[track_caller]
    fn operand(&mut self, op: Op) -> u8 {
        self.op(op);
        self.byte()
    }

    #[track_caller]
    fn constant(&mut self, op: Op) -> Value {
        let index = self.operand(op);
        *self.chunk.get_constant(index)
    }

    fn distance(&mut self) -> usize {
        u16::from_be_bytes([self.byte(), self.byte()]) as usize
    }

    #[track_caller]
    fn forward(&mut self, op: Op) -> usize {
        self.op(op);
        let distance = self.distance();
        self.pc + distance
    }

    #[track_caller]
    fn loops_to(&mut self, target: usize) {
        self.op(Op::Loop);
        let back = self.distance();
        assert_eq!(back, self.pc - target);
    }
}

#[test]
fn arithmetic_respects_precedence_and_left_associativity() {
    let (script, _) = compiled("9 - 4 - 2 * -3;");
    let mut c = Code::new(&script);
    assert_eq!(c.constant(Op::Constant).try_as_number(), Ok(9.0));
    assert_eq!(c.constant(Op::Constant).try_as_number(), Ok(4.0));
    c.op(Op::Subtract);
    assert_eq!(c.constant(Op::Constant).try_as_number(), Ok(2.0));
    assert_eq!(c.constant(Op::Constant).try_as_number(), Ok(3.0));
    c.op(Op::Negate);
    c.op(Op::Multiply);
    assert_eq!(script.chunk.coords(c.pc), Coords::new(1, 7));
    c.op(Op::Subtract);
    c.op(Op::Pop);
}

#[test]
fn chained_assignment_resolves_the_shadowing_slot_then_the_global() {
    let (script, heap) = compiled("{ var x; { var x; saved = x = true; } x; }");
    let mut c = Code::new(&script);
    c.op(Op::Nil);
    c.op(Op::Nil);
    c.op(Op::True);
    assert_eq!(c.operand(Op::SetLocal), 2);
    assert_eq!(
        &heap[c.constant(Op::SetGlobal).try_as_string().unwrap()],
        "saved"
    );
    c.op(Op::Pop); // assignment result
    c.op(Op::Pop); // inner x
    assert_eq!(c.operand(Op::GetLocal), 1);
}

#[test]
fn while_patches_both_edges_across_more_than_one_byte_of_code() {
    let (script, _) = compiled(&format!("while (true) {{ {} }}", "nil;".repeat(140)));
    let mut c = Code::new(&script);
    let condition = c.pc;
    c.op(Op::True);
    let exit = c.forward(Op::JumpIfFalse);
    c.op(Op::Pop);
    for _ in 0..140 {
        c.op(Op::Nil);
        c.op(Op::Pop);
    }
    c.loops_to(condition);
    assert_eq!(exit, c.pc);
    c.op(Op::Pop); // also discard the condition on the false edge
    c.op(Op::Nil);
    c.op(Op::Return);
}

#[test]
fn short_circuit_jumps_keep_the_left_operand_and_skip_the_call() {
    for (source, is_or) in [("false and rhs();", false), ("true or rhs();", true)] {
        let (script, heap) = compiled(source);
        let mut c = Code::new(&script);
        c.op(if is_or { Op::True } else { Op::False });
        let on_false = c.forward(Op::JumpIfFalse);
        let end = if is_or {
            let on_true = c.forward(Op::Jump);
            assert_eq!(on_false, c.pc);
            on_true
        } else {
            on_false
        };
        c.op(Op::Pop);
        assert_eq!(
            &heap[c.constant(Op::GetGlobal).try_as_string().unwrap()],
            "rhs"
        );
        assert_eq!(c.operand(Op::Call), 0);
        assert_eq!(end, c.pc);
        c.op(Op::Pop); // common end of expression statement
    }
}

#[test]
fn nested_closures_forward_one_shared_capture_and_close_its_local() {
    let (script, heap) =
        compiled("{ var cell; fun make() { fun next() { cell = !cell; } return next; } }");
    let mut top = Code::new(&script);
    top.op(Op::Nil);
    let make = &heap[top.constant(Op::Closure).try_as_function().unwrap()];
    assert_eq!(make.upvalue_count, 1);
    assert_eq!([top.byte(), top.byte()], [1, 1]); // capture local slot 1
    top.op(Op::Pop); // make
    top.op(Op::CloseUpvalue); // cell

    let mut outer = Code::new(make);
    let next = &heap[outer.constant(Op::Closure).try_as_function().unwrap()];
    assert_eq!(next.upvalue_count, 1);
    assert_eq!([outer.byte(), outer.byte()], [0, 0]); // forward make's upvalue 0
    let mut inner = Code::new(next);
    assert_eq!(inner.operand(Op::GetUpvalue), 0);
    inner.op(Op::Not);
    assert_eq!(inner.operand(Op::SetUpvalue), 0);
}

#[test]
fn method_call_and_property_read_emit_different_instructions() {
    let (script, heap) = compiled("box.take(nil, true); box.take;");
    let mut c = Code::new(&script);
    let receiver = c.constant(Op::GetGlobal).try_as_string().unwrap();
    assert_eq!(&heap[receiver], "box");
    c.op(Op::Nil);
    c.op(Op::True);
    let method = c.constant(Op::Invoke).try_as_string().unwrap();
    assert_eq!(&heap[method], "take");
    assert_eq!(c.byte(), 2);
    c.op(Op::Pop);
    assert_eq!(c.constant(Op::GetGlobal).try_as_string(), Ok(receiver));
    assert_eq!(c.constant(Op::GetProperty).try_as_string(), Ok(method));
}

#[test]
fn initializers_return_the_receiver_on_both_return_paths() {
    let (script, heap) = compiled("class Box { init(x) { if (x) return; } empty(a,b) {} }");
    let init = nested(&script, &heap, "init");
    assert!(matches!(init.kind, FunctionKind::Initializer));
    assert_eq!(init.arity, 1);
    let mut c = Code::new(init);
    assert_eq!(c.operand(Op::GetLocal), 1); // parameter x
    let on_false = c.forward(Op::JumpIfFalse);
    c.op(Op::Pop);
    assert_eq!(c.operand(Op::GetLocal), 0); // explicit return
    c.op(Op::Return);
    c.pc = on_false;
    c.op(Op::Pop);
    assert_eq!(c.operand(Op::GetLocal), 0); // fall off the end
    c.op(Op::Return);
    let method = nested(&script, &heap, "empty");
    assert!(matches!(method.kind, FunctionKind::Method));
    assert_eq!(method.arity, 2);
    let mut c = Code::new(method);
    c.op(Op::Nil);
    c.op(Op::Return);
}

#[test]
fn invalid_bindings_report_the_name_and_the_offending_token() {
    use CompileError::*;
    for (source, row, col, error) in [
        (
            "{\nvar x;\nvar x;\n}",
            3,
            5,
            VariableRedeclaration("x".into()),
        ),
        (
            "{\nvar value = value;\n}",
            2,
            13,
            SelfReferentialVariableInitializer("value".into()),
        ),
        ("(true) = nil;", 1, 8, InvalidAssignmentTarget),
        ("class X < X {}", 1, 11, SelfInheritance),
        (
            "class X { init() { return nil; } }",
            1,
            20,
            InitializerReturn,
        ),
    ] {
        assert_eq!(
            diagnostics(source),
            [Diagnostic::at_coords(row, col, error)],
            "{source}"
        );
    }
}

#[test]
fn diagnostics_from_separate_statements_are_ordered_and_distinguish_eof() {
    assert_eq!(
        diagnostics("return;\nthis;\nvar x ="),
        [
            Diagnostic::at_coords(1, 1, CompileError::TopLevelReturn),
            Diagnostic::at_coords(2, 1, CompileError::ThisOutsideClass),
            Diagnostic::Eof(CompileError::ExpectedExpression),
        ]
    );
}

#[test]
fn last_encodable_constant_succeeds_and_the_next_one_is_rejected() {
    let mut source: String = (0..256).map(|i| format!("{i};\n")).collect();
    let (script, _) = compiled(&source);
    let mut c = Code::new(&script);
    for i in 0..256 {
        assert_eq!(c.constant(Op::Constant).try_as_number(), Ok(i as f64));
        c.op(Op::Pop);
    }
    source.push_str("256;");
    assert_eq!(
        diagnostics(&source),
        [Diagnostic::at_coords(
            257,
            1,
            CompileError::TooManyConstants
        )]
    );
}
