// TODO: automate `emit_...` to avoid passing `coords`
// TODO: maybe bind function name to FunctionKind::Function

use crate::chunk::{Chunk, Function, FunctionKind, OpCode, Upvalue, Value};
use crate::gc::{Allocate, Heap};
use crate::lex::{Lexer, Token, TokenKind};
use crate::location::{AtCoords, AtCoordsOrEof, Coords};
use crate::{ArrayVec, CompileError};

use std::io::Write;
use std::iter::Peekable;
use std::mem;

#[derive(PartialEq, Eq, PartialOrd, Ord, Debug)]
enum Precedence {
    None,
    Assignment,
    Or,
    And,
    Equality,
    Comparison,
    Term,
    Factor,
    Unary,
    Call,
    Primary,
}

#[derive(Debug)]
struct Locals<'a> {
    // Some if initialized, None if uninitialized, to implement self-referential initialization
    // error
    locals: ArrayVec<Local<'a>, 256>,
    scope_depth: usize,
}

#[derive(Debug)]
struct Local<'a> {
    name: &'a str,
    depth: Option<usize>,
    is_captured: bool,
}

impl<'a> Local<'a> {
    fn new(name: &'a str) -> Local<'a> {
        Self {
            name,
            depth: None,
            is_captured: false,
        }
    }
}

impl<'a> Locals<'a> {
    fn resolve(&self, name: &str) -> Result<Option<u8>, CompileError> {
        match self
            .locals
            .iter()
            .enumerate()
            .rev()
            .find(|(_, local)| local.name == name)
        {
            Some((p, local)) => {
                if local.depth.is_none() {
                    Err(CompileError::SelfReferentialVariableInitializer(
                        name.into(),
                    ))
                } else {
                    Ok(Some(p as u8))
                }
            }
            None => Ok(None),
        }
    }

    fn mark_initialized(&mut self) {
        self.locals.last_mut().unwrap().depth = Some(self.scope_depth);
    }

    fn begin_scope(&mut self) {
        self.scope_depth += 1
    }

    // FIXME: returning Vec<bool> smells
    fn end_scope(&mut self) -> Vec<bool> {
        self.scope_depth -= 1;
        let locals = self
            .locals
            .iter()
            .rev()
            .take_while(|local| local.depth.unwrap_or(0) > self.scope_depth)
            .map(|l| l.is_captured)
            .collect();
        for _ in &locals {
            self.locals.pop();
        }
        locals
    }

    fn try_push(&mut self, name: &'a str) -> Result<(), ()> {
        if self.locals.len == u8::MAX as usize + 1 {
            Err(())
        } else {
            self.locals.push(Local::new(name));
            Ok(())
        }
    }

    fn is_unique(&self, name: &str) -> bool {
        // innermost first; outer scopes end the search
        for local in self.locals.iter().rev() {
            match local.depth {
                Some(depth) if depth < self.scope_depth => break,
                _ => {
                    if local.name == name {
                        return false;
                    }
                }
            };
        }
        true
    }
}

impl Locals<'_> {
    fn new() -> Self {
        Self {
            locals: ArrayVec::new(),
            scope_depth: 0,
        }
    }
}

struct Errors<'a, W: Write> {
    had_error: bool,
    panic_mode: bool,
    err: &'a mut W,
}

impl<'a, W: Write> Errors<'a, W> {
    fn new(err: &'a mut W) -> Self {
        Self {
            had_error: false,
            panic_mode: false,
            err,
        }
    }

    fn report(&mut self, error: &AtCoordsOrEof<CompileError>) {
        self.had_error = true;
        let _ = writeln!(self.err, "{error}");
    }

    fn sync(&mut self, error: &AtCoordsOrEof<CompileError>) {
        if !self.panic_mode {
            self.panic_mode = true;
            self.report(error);
        }
    }
}

struct ClassCompiler {
    depth: u32,
    superclasses: u32,
}

impl ClassCompiler {
    fn new() -> Self {
        Self {
            depth: 0,
            superclasses: 0
        }
    }

    fn try_nest(&mut self) -> Result<(), ()> {
        if self.depth < u32::BITS {
            self.depth += 1;
            Ok(())
        } else {
            Err(())
        }
    }

    fn try_unnest(&mut self) -> Result<(), ()> {
        if self.depth == 0 {
            Err(())
        } else {
            self.superclasses &= !(1 << (self.depth - 1));
            self.depth -= 1;
            Ok(())
        }
    }

    fn mark_has_superclass(&mut self) {
        self.superclasses |= 1 << (self.depth - 1)
    }

    fn has_superclass(&self) -> bool {
        self.superclasses & (1 << (self.depth - 1)) != 0
    }
}

pub struct Compiler<'a, 't, W: Write> {
    lexer: &'a mut Peekable<Lexer<'t>>,
    objects: &'a mut Heap,
    frame: CompilationFrame<'a>,
    errors: Errors<'a, W>,
    class_compiler : ClassCompiler,
}

pub struct CompilationFrame<'a> {
    function: Function,
    locals: Locals<'a>,
    upvalues: ArrayVec<Upvalue, { u8::MAX as usize + 1 }>,
    enclosing: Option<Box<CompilationFrame<'a>>>,
}

impl CompilationFrame<'_> {
    fn new(function_kind: FunctionKind) -> Self {
        let mut locals = Locals::new();
        let _ = locals.try_push(if let FunctionKind::Function = function_kind {
            ""
        } else {
            "this"
        });
        locals.mark_initialized();
        Self {
            function: Function::new(function_kind),
            upvalues: ArrayVec::new(),
            enclosing: None,
            locals,
        }
    }

    fn resolve_upvalue(
        &mut self,
        name: &AtCoords<Token<'_>>,
    ) -> Result<Option<u8>, AtCoordsOrEof<CompileError>> {
        if let Some(ref mut enclosing) = self.enclosing {
            let local = enclosing.resolve_local(name)?;
            if let Some(local) = local {
                // FIXME: locals should be directly indexable
                self.enclosing.as_mut().unwrap().locals.locals[local as usize].is_captured = true;
                self.add_upvalue(local, true, name.coords()).map(Some)
            } else if let Some(upvalue) = enclosing.resolve_upvalue(name)? {
                self.add_upvalue(upvalue, false, name.coords()).map(Some)
            } else {
                Ok(None)
            }
        } else {
            Ok(None)
        }
    }

    fn add_upvalue(
        &mut self,
        index: u8,
        is_local: bool,
        coords: Coords,
    ) -> Result<u8, AtCoordsOrEof<CompileError>> {
        let upvalue = Upvalue::new(index, is_local);
        if let Some((i, _)) = self
            .upvalues
            .iter()
            .enumerate()
            .find(|(_, u)| *u == &upvalue)
        {
            Ok(i as u8)
        } else if self.upvalues.len() == u8::MAX as usize + 1 {
            Err(coords.locate(CompileError::TooManyUpvalues).into())
        } else {
            self.upvalues.push(upvalue);
            self.function.upvalue_count += 1;
            Ok((self.upvalues.len() - 1) as u8)
        }
    }

    fn resolve_local(
        &self,
        name: &AtCoords<Token<'_>>,
    ) -> Result<Option<u8>, AtCoordsOrEof<CompileError>> {
        self.locals
            .resolve(name.span())
            .map_err(|e| name.co_locate(e).into())
    }
}

impl<'a, 't, W: Write> Compiler<'a, 't, W> {
    pub fn new<'b>(
        lexer: &'b mut Peekable<Lexer<'t>>,
        objects: &'b mut Heap,
        function_kind: FunctionKind,
        err: &'b mut W,
    ) -> Compiler<'b, 't, W> {
        Compiler {
            lexer,
            objects,
            frame: CompilationFrame::new(function_kind),
            errors: Errors::new(err),
            class_compiler: ClassCompiler::new(),
        }
    }

    #[allow(clippy::result_unit_err)]
    pub fn compile(&mut self) -> Result<Function, ()> {
        while self.peek_token().is_some() {
            self.declaration();
        }
        if !self.errors.had_error {
            self.current_chunk().write_nowhere(OpCode::Nil as u8);
            self.current_chunk().write_nowhere(OpCode::Return as u8);

            let ret = mem::replace(
                &mut self.frame.function,
                Function::new(FunctionKind::Script),
            );
            Ok(ret)
        } else {
            Err(())
        }
    }

    fn synchronize(&mut self) {
        while let Some(t) = self.peek_token() {
            match t.kind() {
                TokenKind::Class
                | TokenKind::Fun
                | TokenKind::Var
                | TokenKind::For
                | TokenKind::If
                | TokenKind::While
                | TokenKind::Print
                | TokenKind::Return => break,
                TokenKind::Semicolon => {
                    self.lexer.next();
                    break;
                }
                _ => {
                    self.next_token();
                }
            }
        }
    }

    fn next_token<'b>(&'b mut self) -> Option<AtCoords<Token<'a>>> {
        let next = self.lexer.next()?;
        match next {
            Ok(t) => Some(t),
            Err(e) => {
                self.errors.sync(&e);
                self.next_token()
            }
        }
    }

    fn peek_token(&mut self) -> Option<AtCoords<Token<'a>>> {
        let peek = self.lexer.peek()?;
        match peek {
            Ok(t) => Some(t.clone()),
            Err(e) => {
                self.errors.report(e);
                self.lexer.next();
                self.peek_token()
            }
        }
    }

    fn next_token_if<F>(&mut self, f: F) -> Option<AtCoords<Token<'a>>>
    where
        F: Fn(TokenKind) -> bool,
    {
        self.peek_token()
            .filter(|t| f(t.kind()))
            .inspect(|_| {
                self.lexer.next();
            })
    }

    fn next_token_if_eq(&mut self, kind: TokenKind) -> Option<AtCoords<Token<'_>>> {
        self.next_token_if(|t| t == kind)
    }

    fn consume<'b>(
        &'b mut self,
        kind: TokenKind,
        error: CompileError,
    ) -> Option<AtCoords<Token<'a>>> {
        let peek = self.peek_token();
        if let Some(peek) = peek {
            if peek.kind() == kind {
                let ret = peek.clone();
                self.lexer.next();
                Some(ret)
            } else {
                self.errors.sync(&peek.co_locate(error).into());
                None
            }
        } else {
            self.errors.sync(&AtCoordsOrEof::Eof(error));
            None
        }
    }

    pub fn emit_op(&mut self, op: OpCode, coords: Coords) {
        self.emit_byte(op as u8, coords)
    }

    fn emit_byte(&mut self, byte: u8, coords: Coords) {
        self.current_chunk().write(byte, coords)
    }

    fn emit_loop(&mut self, start: usize, coords: Coords) {
        self.emit_op(OpCode::Loop, coords);

        let offset = self.current_chunk().len() - start + 2;
        if offset > u16::MAX as usize {
            self.errors
                .sync(&coords.locate(CompileError::JumpTooWide).into());
        }

        self.emit_byte(((offset & 0xff00) >> 8) as u8, coords);
        self.emit_byte((offset & 0xff) as u8, coords);
    }

    fn emit_jump(&mut self, op: OpCode, coords: Coords) -> usize {
        self.emit_op(op, coords);
        self.emit_byte(0xff, coords);
        self.emit_byte(0xff, coords);
        self.current_chunk().len() - 2
    }

    fn emit_constant(&mut self, value: Value, coords: Coords) {
        let constant = self.make_constant(value, coords);
        self.emit_op(OpCode::Constant, coords);
        self.emit_byte(constant, coords);
    }

    fn patch_jump(&mut self, offset: usize) {
        let jump = self.current_chunk().len() - offset - 2;

        if jump > u16::MAX as usize {
            let index = self.current_chunk().len() - 1;
            let coords = self.current_chunk().coords(index);
            self.errors
                .sync(&coords.locate(CompileError::JumpTooWide).into())
        }

        *self.current_chunk().at_mut(offset) = (jump & 0xff00) as u8;
        *self.current_chunk().at_mut(offset + 1) = (jump & 0xff) as u8;
    }

    fn current_chunk(&mut self) -> &mut Chunk {
        &mut self.frame.function.chunk
    }

    fn make_constant(&mut self, value: Value, coords: Coords) -> u8 {
        let constant = self.current_chunk().add_constant(value);
        if constant > u8::MAX as u32 {
            self.errors
                .report(&coords.locate(CompileError::TooManyConstants).into());
            0
        } else {
            constant as u8
        }
    }

    fn begin_scope(&mut self) {
        self.frame.locals.begin_scope()
    }

    fn end_scope(&mut self, coords: Coords) {
        for is_captured in self.frame.locals.end_scope() {
            self.emit_op(
                if is_captured {
                    OpCode::CloseUpvalue
                } else {
                    OpCode::Pop
                },
                coords,
            );
        }
    }

    fn expression(&mut self) {
        self.parse_precedence(Precedence::Assignment);
    }

    fn var_declaration(&mut self) {
        if let Ok((index, coords)) = self.parse_variable(CompileError::ExpectedVariableName) {
            if self.next_token_if_eq(TokenKind::Equal).is_some() {
                self.expression();
            } else {
                self.emit_op(OpCode::Nil, coords);
            }
            if let Some(coords) = self
                .consume(TokenKind::Semicolon, CompileError::UnclosedStatement)
                .map(|t| t.coords())
            {
                self.define_variable(index, coords);
            }
        }
    }

    fn declaration(&mut self) {
        if self.next_token_if_eq(TokenKind::Class).is_some() {
            self.class_declaration();
        } else if self.next_token_if_eq(TokenKind::Fun).is_some() {
            self.fun_declaration();
        } else if self.next_token_if_eq(TokenKind::Var).is_some() {
            self.var_declaration()
        } else {
            self.statement();
        }

        if self.errors.panic_mode {
            self.synchronize();
        }
    }

    fn statement(&mut self) {
        if self.next_token_if_eq(TokenKind::Print).is_some() {
            self.print_statement();
        } else if let Some(coords) = self.next_token_if_eq(TokenKind::If).map(|t| t.coords()) {
            self.if_statement(coords);
        } else if let Some(coords) = self.next_token_if_eq(TokenKind::Return).map(|t| t.coords()) {
            self.return_statement(coords);
        } else if let Some(coords) = self.next_token_if_eq(TokenKind::While).map(|t| t.coords()) {
            self.while_statement(coords);
        } else if let Some(coords) = self.next_token_if_eq(TokenKind::For).map(|t| t.coords()) {
            self.for_statement(coords);
        } else if let Some(coords) = self
            .next_token_if_eq(TokenKind::LeftBrace)
            .map(|t| t.coords())
        {
            self.begin_scope();
            // TODO: `end_scope` should be called nonetheless
            self.block();
            self.end_scope(coords);
        } else {
            self.expression_statement();
        }
    }

    fn print_statement(&mut self) {
        self.expression();
        if let Some(c) = self
            .consume(TokenKind::Semicolon, CompileError::UnclosedStatement)
            .map(|t| t.coords())
        {
            self.emit_op(OpCode::Print, c)
        }
    }

    fn return_statement(&mut self, coords: Coords) {
        if let FunctionKind::Script = self.frame.function.kind {
            self.errors
                .report(&coords.locate(CompileError::TopLevelReturn).into());
        }

        if self.next_token_if_eq(TokenKind::Semicolon).is_some() {
            if let FunctionKind::Initializer = self.frame.function.kind {
                self.emit_op(OpCode::GetLocal, coords);
                self.emit_byte(0, coords);
            } else {
                self.emit_op(OpCode::Nil, coords);
            }
            self.emit_op(OpCode::Return, coords);
        } else {
            if let FunctionKind::Initializer = self.frame.function.kind {
                self.errors.report(&coords.locate(CompileError::InitializerReturn).into());
            }

            self.expression();
            self.consume(TokenKind::Semicolon, CompileError::UnclosedStatement);
            self.emit_op(OpCode::Return, coords);
        }
    }

    fn while_statement(&mut self, coords: Coords) {
        let to_start = self.current_chunk().len();
        self.consume(TokenKind::LeftParen, CompileError::ExpectedControlLeftParen);
        self.expression();
        self.consume(
            TokenKind::RightParen,
            CompileError::ExpectedControlRightParen,
        );

        let to_end = self.emit_jump(OpCode::JumpIfFalse, coords);
        self.emit_op(OpCode::Pop, coords);
        self.statement();
        self.emit_loop(to_start, coords);

        self.patch_jump(to_end);
        self.emit_op(OpCode::Pop, coords);
    }

    fn identifier_constant(&mut self, name: String, coords: Coords) -> u8 {
        let value = self.objects.alloc(name);
        self.make_constant(Value::String(value), coords)
    }

    fn add_local(&mut self, name: AtCoords<Token<'a>>) {
        if let Err(()) = self.frame.locals.try_push(name.span()) {
            self.errors
                .sync(&name.co_locate(CompileError::TooManyLocals).into());
        }
    }

    fn declare_variable(&mut self, name: AtCoords<Token<'a>>) {
        if self.frame.locals.scope_depth == 0 {
            return;
        }
        let span = name.span().to_owned();

        if self.frame.locals.is_unique(&span) {
            self.add_local(name);
        } else {
            self.errors
                .sync(&name.co_locate(CompileError::VariableRedeclaration(span.clone())).into())
        }
    }

    #[allow(clippy::result_unit_err)]
    fn parse_variable(&mut self, error: CompileError) -> Result<(u8, Coords), ()> {
        if let Some(identifier) = self.consume(TokenKind::Identifier, error) {
            let coords = identifier.coords();
            // `span` should be put inside `else`
            let span = identifier.span().into();
            self.declare_variable(identifier);
            if self.frame.locals.scope_depth > 0 {
                Ok((0, coords))
            } else {
                Ok((self.identifier_constant(span, coords), coords))
            }
        } else {
            Err(())
        }
    }

    fn expression_statement(&mut self) {
        self.expression();
        if let Some(c) = self
            .consume(TokenKind::Semicolon, CompileError::UnclosedStatement)
            .map(|t| t.coords())
        {
            self.emit_op(OpCode::Pop, c);
        }
    }

    fn for_statement(&mut self, coords: Coords) {
        self.begin_scope();
        self.consume(TokenKind::LeftParen, CompileError::ExpectedControlLeftParen);

        if self.next_token_if_eq(TokenKind::Var).is_some() {
            self.var_declaration();
        } else if self.next_token_if_eq(TokenKind::Semicolon).is_none() {
            self.expression_statement();
        }

        let mut loop_start = self.current_chunk().len();

        let to_exit = if self.next_token_if_eq(TokenKind::Semicolon).is_none() {
            self.expression();
            self.consume(TokenKind::Semicolon, CompileError::UnclosedStatement);
            let to_exit = self.emit_jump(OpCode::JumpIfFalse, coords);
            self.emit_op(OpCode::Pop, coords);
            Some(to_exit)
        } else {
            None
        };

        if self.next_token_if_eq(TokenKind::RightParen).is_none() {
            let to_body = self.emit_jump(OpCode::Jump, coords);
            let increment_start = self.current_chunk().len();
            self.expression();
            self.emit_op(OpCode::Pop, coords);
            self.consume(
                TokenKind::RightParen,
                CompileError::ExpectedControlRightParen,
            );
            self.emit_loop(loop_start, coords);
            loop_start = increment_start;
            self.patch_jump(to_body);
        }

        self.statement();
        self.emit_loop(loop_start, coords);

        if let Some(j) = to_exit {
            self.patch_jump(j);
            self.emit_op(OpCode::Pop, coords);
        }

        self.end_scope(coords);
    }

    fn if_statement(&mut self, coords: Coords) {
        self.consume(TokenKind::LeftParen, CompileError::ExpectedControlLeftParen);
        self.expression();
        self.consume(
            TokenKind::RightParen,
            CompileError::ExpectedControlRightParen,
        );

        let else_branch = self.emit_jump(OpCode::JumpIfFalse, coords);
        self.emit_op(OpCode::Pop, coords);
        self.statement();
        let end = self.emit_jump(OpCode::Jump, coords);
        self.patch_jump(else_branch);
        self.emit_op(OpCode::Pop, coords);

        if self.next_token_if_eq(TokenKind::Else).is_some() {
            self.statement();
        }
        self.patch_jump(end);
    }

    fn block(&mut self) {
        loop {
            if let Some(TokenKind::RightBrace) | None = self.peek_token().map(|t| t.kind()) {
                break;
            }
            self.declaration();
        }
        self.consume(TokenKind::RightBrace, CompileError::UnclosedBlock);
    }

    fn nest(&mut self, kind: FunctionKind) {
        let frame = CompilationFrame::new(kind);
        let enclosing = mem::replace(&mut self.frame, frame);
        self.frame.enclosing = Some(enclosing.into());
    }

    fn unnest(&mut self) -> (Function, ArrayVec<Upvalue, { u8::MAX as usize + 1 }>) {
        let enclosing = mem::take(&mut self.frame.enclosing).unwrap();
        let frame = mem::replace(&mut self.frame, *enclosing);
        (frame.function, frame.upvalues)
    }

    fn function(&mut self, kind: FunctionKind, coords: Coords, name: &str) {
        self.nest(kind);
        self.begin_scope();
        self.consume(TokenKind::LeftParen, CompileError::UnopenedArgumentsList);
        if self
            .peek_token()
            .filter(|t| t.kind() != TokenKind::RightParen)
            .is_some()
        {
            loop {
                if self.frame.function.arity < u8::MAX {
                    self.frame.function.arity += 1;
                    if let Ok((constant, coords)) =
                        self.parse_variable(CompileError::ExpectedVariableName)
                    {
                        self.define_variable(constant, coords);
                    }
                } else {
                    // over the limit: consume the name but don't declare it
                    if let Some(t) = self.peek_token() {
                        self.errors
                            .report(&t.co_locate(CompileError::TooManyParameters).into());
                    }
                    self.next_token_if_eq(TokenKind::Identifier);
                }
                if self.next_token_if_eq(TokenKind::Comma).is_none() {
                    break;
                }
            }
        }
        self.consume(TokenKind::RightParen, CompileError::UnclosedArgumentsList);
        self.consume(TokenKind::LeftBrace, CompileError::UnopenedBlock);
        self.block();

        // FIXME: why write_nowhere? ideally return latest location
        if let FunctionKind::Initializer = kind {
            self.current_chunk().write_nowhere(OpCode::GetLocal as u8);
            self.current_chunk().write_nowhere(0);
        } else {
            self.current_chunk().write_nowhere(OpCode::Nil as u8);
        }
        self.current_chunk().write_nowhere(OpCode::Return as u8);

        let (mut function, upvalues) = self.unnest();

        let name = self.objects.alloc(name.into());
        function.name = Some(name);

        let function_obj = self.objects.alloc(function);

        self.emit_op(OpCode::Closure, coords);
        let function = self.make_constant(Value::Function(function_obj), coords);
        self.emit_byte(function, coords);

        for upvalue in &*upvalues {
            self.emit_byte(if upvalue.is_local { 1 } else { 0 }, coords);
            self.emit_byte(upvalue.index, coords);
        }
    }

    fn method(&mut self) {
        if let Some(t) = self.consume(TokenKind::Identifier, CompileError::ExpectedMethodName) {
            let coords = t.coords();
            let name = t.span();
            let constant = self.identifier_constant(name.into(), coords);

            self.function(if name == "init" { FunctionKind::Initializer } else { FunctionKind::Method }, coords, name);
            self.emit_op(OpCode::Method, coords);
            self.emit_byte(constant, coords);
        } else if self
            .peek_token()
            .map(|t| t.kind() != TokenKind::RightBrace)
            .unwrap_or(false)
        {
            // skip the bad token so the method loop makes progress
            self.next_token();
        }
    }



    fn class_declaration(&mut self) {
        if let Some(class) = self.consume(TokenKind::Identifier, CompileError::ExpectedClassName) {
            let class_clone = class.clone();
            let index = self.identifier_constant(class_clone.span().into(), class.coords());
            let coords = class.coords();
            self.declare_variable(class_clone);

            self.emit_op(OpCode::Class, coords);
            self.emit_byte(index, coords);
            self.define_variable(index, coords);

            if let Err(()) = self.class_compiler.try_nest() {
                self.errors.report(&coords.locate(CompileError::TooMuchClassNesting).into())
            }

            if self.next_token_if_eq(TokenKind::Less).is_some() {
                if let Some(superclass) = self.consume(TokenKind::Identifier, CompileError::ExpectedClassName) {
                    if class.span() == superclass.span() {
                        self.errors.report(&superclass.co_locate(CompileError::SelfInheritance).into());
                    }

                    self.variable(&superclass, false);

                    self.begin_scope();
                    self.add_local(superclass.co_locate(Token::new(TokenKind::Identifier, "super")));
                    self.define_variable(0, superclass.coords());

                    self.variable(&class, false);
                    self.emit_op(OpCode::Inherit, superclass.coords());
                    self.class_compiler.mark_has_superclass();
                }

            }

            self.named_variable(&class, false);

            self.consume(TokenKind::LeftBrace, CompileError::UnopenedBlock);

            // like block(): stop at EOF too
            while self
                .peek_token()
                .map(|t| t.kind() != TokenKind::RightBrace)
                .unwrap_or(false)
            {
                self.method();
            }

            if let Some(t) = self.consume(TokenKind::RightBrace, CompileError::UnclosedBlock) {
                self.emit_op(OpCode::Pop, t.coords());
                if self.class_compiler.has_superclass() {
                    self.end_scope(t.coords());
                }
            }


            let _ = self.class_compiler.try_unnest();
        }
    }

    fn fun_declaration(&mut self) {
        let Some(name) = self
            .peek_token()
            .filter(|t| t.kind() == TokenKind::Identifier)
            .map(|t| t.span())
        else {
            if let Some(token) = self.peek_token() {
                self.errors
                    .sync(&token.co_locate(CompileError::ExpectedVariableName).into());
            } else {
                self.errors
                    .sync(&AtCoordsOrEof::Eof(CompileError::ExpectedVariableName));
            }
            return;
        };
        if let Ok((global, coords)) = self.parse_variable(CompileError::ExpectedVariableName) {
            self.frame.locals.mark_initialized();
            self.function(FunctionKind::Function, coords, name);
            self.define_variable(global, coords);
        }
    }

    fn define_variable(&mut self, global: u8, coords: Coords) {
        if self.frame.locals.scope_depth > 0 {
            self.frame.locals.mark_initialized();
        } else {
            self.emit_op(OpCode::DefineGlobal, coords);
            self.emit_byte(global, coords);
        }
    }

    fn argument_list(&mut self) -> u8 {
        let mut count = 0;
        if self
            .peek_token()
            .filter(|t| t.kind() == TokenKind::RightParen)
            .is_none()
        {
            loop {
                self.expression();
                // don't overflow the u8 arg count
                if count == u8::MAX {
                    if let Some(t) = self.peek_token() {
                        self.errors
                            .report(&t.co_locate(CompileError::TooManyArguments).into());
                    }
                } else {
                    count += 1;
                }
                if self.next_token_if_eq(TokenKind::Comma).is_none() {
                    break;
                }
            }
        }
        self.consume(TokenKind::RightParen, CompileError::UnclosedArgumentsList);
        count
    }

    fn and(&mut self, token: &AtCoords<Token>) {
        let end = self.emit_jump(OpCode::JumpIfFalse, token.coords());
        self.emit_op(OpCode::Pop, token.coords());
        self.parse_precedence(Precedence::And);
        self.patch_jump(end);
    }

    fn parse_precedence(&mut self, precedence: Precedence) {
        if let Some(token) = self.peek_token() {
            let can_assign = precedence <= Precedence::Assignment;
            if self.prefix_rule(&token, can_assign).is_some() {
                while let Some(token) = self.next_token_if(|t| precedence <= Self::precedence(t)) {
                    self.infix_rule(&token, can_assign).unwrap();
                }

                if let Some(coords) = self
                    .next_token_if(|t| can_assign && t == TokenKind::Equal)
                    .map(|t| t.coords())
                {
                    self.expression();
                    self.errors
                        .report(&coords.locate(CompileError::InvalidAssignmentTarget).into());
                }
            } else {
                self.errors
                    .report(&token.co_locate(CompileError::ExpectedExpression).into());
            }
        } else {
            self.errors
                .sync(&AtCoordsOrEof::Eof(CompileError::ExpectedExpression));
        }
    }

    fn prefix_rule<'b>(&'b mut self, token: &AtCoords<Token<'a>>, can_assign: bool) -> Option<()> {
        match token.kind() {
            TokenKind::LeftParen => {
                self.next_token();
                self.grouping(token)
            }
            TokenKind::Minus => {
                self.next_token();
                self.unary(token)
            }
            TokenKind::Number => {
                self.next_token();
                self.number(token)
            }
            TokenKind::False => {
                self.next_token();
                self.literal(token)
            }
            TokenKind::True => {
                self.next_token();
                self.literal(token)
            }
            TokenKind::This => {
                self.next_token();
                self.this(token)
            }
            TokenKind::Nil => {
                self.next_token();
                self.literal(token)
            }
            TokenKind::Bang => {
                self.next_token();
                self.unary(token)
            }
            TokenKind::String => {
                self.next_token();
                self.string(token)
            }
            TokenKind::Identifier => {
                self.next_token();
                self.variable(token, can_assign)
            }
            TokenKind::Super => {
                self.next_token();
                self.super_(token)
            }
            _ => return None,
        };
        Some(())
    }

    fn infix_rule<'b>(&'b mut self, token: &AtCoords<Token<'a>>, can_assign: bool) -> Option<()> {
        match token.kind() {
            TokenKind::Minus => self.binary(token),
            TokenKind::Plus => self.binary(token),
            TokenKind::Slash => self.binary(token),
            TokenKind::Star => self.binary(token),
            TokenKind::BangEqual => self.binary(token),
            TokenKind::EqualEqual => self.binary(token),
            TokenKind::Greater => self.binary(token),
            TokenKind::GreaterEqual => self.binary(token),
            TokenKind::Less => self.binary(token),
            TokenKind::LessEqual => self.binary(token),
            TokenKind::And => self.and(token),
            TokenKind::Or => self.or(token),
            TokenKind::LeftParen => self.call(token),
            TokenKind::Dot => self.dot(token, can_assign),
            _ => return None,
        };
        Some(())
    }

    fn precedence(kind: TokenKind) -> Precedence {
        match kind {
            TokenKind::LeftParen => Precedence::Call,
            TokenKind::RightParen => Precedence::None,
            TokenKind::LeftBrace => Precedence::None,
            TokenKind::RightBrace => Precedence::None,
            TokenKind::Comma => Precedence::None,
            TokenKind::Dot => Precedence::Call,
            TokenKind::Minus => Precedence::Term,
            TokenKind::Plus => Precedence::Term,
            TokenKind::Semicolon => Precedence::None,
            TokenKind::Slash => Precedence::Factor,
            TokenKind::Star => Precedence::Factor,
            TokenKind::Bang => Precedence::None,
            TokenKind::BangEqual => Precedence::Equality,
            TokenKind::Equal => Precedence::None,
            TokenKind::EqualEqual => Precedence::Equality,
            TokenKind::Greater => Precedence::Comparison,
            TokenKind::GreaterEqual => Precedence::Comparison,
            TokenKind::Less => Precedence::Comparison,
            TokenKind::LessEqual => Precedence::Comparison,
            TokenKind::And => Precedence::And,
            TokenKind::Class => Precedence::None,
            TokenKind::Else => Precedence::None,
            TokenKind::False => Precedence::None,
            TokenKind::Fun => Precedence::None,
            TokenKind::For => Precedence::None,
            TokenKind::If => Precedence::None,
            TokenKind::Nil => Precedence::None,
            TokenKind::Or => Precedence::Or,
            TokenKind::Print => Precedence::None,
            TokenKind::Return => Precedence::None,
            TokenKind::Super => Precedence::None,
            TokenKind::This => Precedence::None,
            TokenKind::True => Precedence::None,
            TokenKind::Var => Precedence::None,
            TokenKind::While => Precedence::None,
            TokenKind::String => Precedence::None,
            TokenKind::Number => Precedence::None,
            TokenKind::Identifier => Precedence::None,
        }
    }

    fn number(&mut self, token: &AtCoords<Token<'_>>) {
        self.emit_constant(Value::Number(token.span().parse().unwrap()), token.coords());
    }

    fn or(&mut self, token: &AtCoords<Token<'_>>) {
        let else_branch = self.emit_jump(OpCode::JumpIfFalse, token.coords());
        let end = self.emit_jump(OpCode::Jump, token.coords());
        self.patch_jump(else_branch);
        self.emit_op(OpCode::Pop, token.coords());
        self.parse_precedence(Precedence::Or);
        self.patch_jump(end);
    }

    fn string(&mut self, token: &AtCoords<Token<'_>>) {
        let s = self.objects.alloc(token.span().to_owned());
        self.emit_constant(Value::String(s), token.coords())
    }

    fn resolve_local(&mut self, name: &AtCoords<Token<'_>>) -> Option<u8> {
        match self.frame.resolve_local(name) {
            Ok(local) => {
                local
            }
            Err(e) => {
                self.errors.sync(&e);
                Some(0)
            }
        }
    }

    fn resolve_upvalue(&mut self, name: &AtCoords<Token<'_>>) -> Option<u8> {
        match self.frame.resolve_upvalue(name) {
            Ok(upvalue) => upvalue,
            Err(e) => {
                self.errors.sync(&e);
                Some(0)
            }
        }
    }

    fn named_variable(&mut self, token: &AtCoords<Token<'_>>, can_assign: bool) {
        let (arg, get, set) = if let Some(arg) = self.resolve_local(token) {
            (arg, OpCode::GetLocal, OpCode::SetLocal)
        } else if let Some(arg) = self.resolve_upvalue(token) {
            (arg, OpCode::GetUpvalue, OpCode::SetUpvalue)
        } else {
            (
                self.identifier_constant(token.span().into(), token.coords()),
                OpCode::GetGlobal,
                OpCode::SetGlobal,
            )
        };

        match self
            .next_token_if(|t| can_assign && t == TokenKind::Equal)
            .map(|t| t.coords())
        {
            Some(coords) => {
                self.expression();
                self.emit_op(set, coords);
                self.emit_byte(arg, coords);
            }
            _ => {
                self.emit_op(get, token.coords());
                self.emit_byte(arg, token.coords());
            }
        }
    }

    // FIXME: this function is useless, remove
    fn variable(&mut self, token: &AtCoords<Token<'_>>, can_assign: bool) {
        self.named_variable(token, can_assign);
    }

    fn super_(&mut self, token: &AtCoords<Token<'_>>) {
        if self.class_compiler.depth == 0 {
            self.errors.report(&token.co_locate(CompileError::SuperOutsideClass).into());
        } else if !self.class_compiler.has_superclass() {
            self.errors.report(&token.co_locate(CompileError::SuperWithoutClass).into());
        }

        self.consume(TokenKind::Dot, CompileError::ExpectedDotAfterSuper);
        if let Some(name) = self.consume(TokenKind::Identifier, CompileError::ExpectedMethodName) {
            let constant = self.identifier_constant(name.span().into(), name.coords());

            self.named_variable(&name.co_locate(Token::new(TokenKind::Identifier, "this")), false);
            if let Some(paren) = self.next_token_if_eq(TokenKind::LeftParen) {
                let paren_coords = paren.coords();
                let name_coords = name.coords();
                let arg_count = self.argument_list();
                self.named_variable(&name.co_locate(Token::new(TokenKind::Identifier, "super")), false);
                self.emit_op(OpCode::SuperInvoke, paren_coords);
                self.emit_byte(constant, name_coords);
                self.emit_byte(arg_count, paren_coords);
            } else {
                self.named_variable(&name.co_locate(Token::new(TokenKind::Identifier, "super")), false);
                self.emit_op(OpCode::GetSuper, name.coords());
                self.emit_byte(constant, name.coords());
            }
        }
    }

    fn this(&mut self, token: &AtCoords<Token<'_>>) {
        if self.class_compiler.depth == 0 {
            self.errors.report(&token.co_locate(CompileError::ThisOutsideClass).into())
        }
        self.variable(token, false);
    }

    fn grouping<'b>(&'b mut self, _token: &AtCoords<Token<'a>>) {
        self.expression();
        self.consume(TokenKind::RightParen, CompileError::UnclosedGrouping);
    }

    fn unary<'b>(&'b mut self, token: &AtCoords<Token<'a>>) {
        self.parse_precedence(Precedence::Unary);
        match token.kind() {
            TokenKind::Bang => self.emit_op(OpCode::Not, token.coords()),
            TokenKind::Minus => self.emit_op(OpCode::Negate, token.coords()),
            _ => unreachable!(),
        }
    }

    fn binary<'b>(&'b mut self, token: &AtCoords<Token<'a>>) {
        let operator = token.kind();
        self.parse_precedence(Self::precedence(operator).next());
        match operator {
            TokenKind::Plus => self.emit_op(OpCode::Add, token.coords()),
            TokenKind::Minus => self.emit_op(OpCode::Subtract, token.coords()),
            TokenKind::Star => self.emit_op(OpCode::Multiply, token.coords()),
            TokenKind::Slash => self.emit_op(OpCode::Divide, token.coords()),
            TokenKind::BangEqual => {
                self.emit_op(OpCode::Equal, token.coords());
                self.emit_op(OpCode::Not, token.coords());
            }
            TokenKind::EqualEqual => self.emit_op(OpCode::Equal, token.coords()),
            TokenKind::Greater => self.emit_op(OpCode::Greater, token.coords()),
            TokenKind::GreaterEqual => {
                self.emit_op(OpCode::Less, token.coords());
                self.emit_op(OpCode::Not, token.coords());
            }
            TokenKind::Less => self.emit_op(OpCode::Less, token.coords()),
            TokenKind::LessEqual => {
                self.emit_op(OpCode::Greater, token.coords());
                self.emit_op(OpCode::Not, token.coords());
            }
            _ => unreachable!(),
        }
    }

    fn call(&mut self, token: &AtCoords<Token<'a>>) {
        let arg_count = self.argument_list();
        self.emit_op(OpCode::Call, token.coords());
        self.emit_byte(arg_count, token.coords());
    }

    fn dot(&mut self, _token: &AtCoords<Token<'a>>, can_assign: bool) {
        if let Some(identifier) =
            self.consume(TokenKind::Identifier, CompileError::ExpectedProperty)
        {
            let coords = identifier.coords();
            let name = self.identifier_constant(identifier.span().into(), coords);
            if can_assign && self.next_token_if_eq(TokenKind::Equal).is_some() {
                self.expression();
                self.emit_op(OpCode::SetProperty, coords);
                self.emit_byte(name, coords);
            } else if let Some(t) = self.next_token_if_eq(TokenKind::LeftParen) {
                let coords_paren = t.coords();
                let arg_count = self.argument_list();
                self.emit_op(OpCode::Invoke, coords_paren);
                self.emit_byte(name, coords);
                self.emit_byte(arg_count, coords_paren);
            } else {
                self.emit_op(OpCode::GetProperty, coords);
                self.emit_byte(name, coords);
            }
        }
    }

    fn literal<'b>(&'b mut self, token: &AtCoords<Token<'a>>) {
        match token.kind() {
            TokenKind::False => self.emit_op(OpCode::False, token.coords()),
            TokenKind::Nil => self.emit_op(OpCode::Nil, token.coords()),
            TokenKind::True => self.emit_op(OpCode::True, token.coords()),
            _ => unreachable!(),
        }
    }
}

impl Precedence {
    fn next(&self) -> Self {
        match self {
            Precedence::None => Self::Assignment,
            Precedence::Assignment => Self::Or,
            Precedence::Or => Self::And,
            Precedence::And => Self::Equality,
            Precedence::Equality => Self::Comparison,
            Precedence::Comparison => Self::Term,
            Precedence::Term => Self::Factor,
            Precedence::Factor => Self::Unary,
            Precedence::Unary => Self::Call,
            Precedence::Call => Self::Primary,
            Precedence::Primary => Self::Primary,
        }
    }
}
