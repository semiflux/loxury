use crate::location::AtCoords;

#[derive(Clone, Copy, Debug, PartialOrd, PartialEq, Eq)]
pub enum Kind {
    LeftParen,
    RightParen,
    LeftBrace,
    RightBrace,
    Comma,
    Dot,
    Minus,
    Plus,
    Semicolon,
    Slash,
    Star,
    Bang,
    BangEqual,
    Equal,
    EqualEqual,
    Greater,
    GreaterEqual,
    Less,
    LessEqual,
    And,
    Class,
    Else,
    False,
    Fun,
    For,
    If,
    Nil,
    Or,
    Print,
    Return,
    Super,
    This,
    True,
    Var,
    While,
    String,
    Number,
    Identifier,
}

#[derive(Clone, Debug)]
pub struct Token<'a> {
    kind: Kind,
    span: &'a str,
}

impl<'a> Token<'a> {
    pub fn new(kind: Kind, span: &'a str) -> Self {
        Self { kind, span }
    }
}

impl<'a> AtCoords<Token<'a>> {
    pub fn kind(&self) -> Kind {
        self.value.kind
    }

    pub fn span<'b>(&'b self) -> &'a str {
        match self.kind() {
            Kind::This => "this",
            _ => self.value.span,
        }
    }
}
