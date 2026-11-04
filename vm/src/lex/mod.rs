use crate::location::{AtCoords, AtCoordsOrEof};
use crate::CompileError;
use std::str::Chars;

mod token;
pub use token::{Kind as TokenKind, Token};

#[derive(Clone)]
struct Text<'a> {
    row: u16,
    col: u16,
    text: Chars<'a>,
}

impl<'a> Text<'a> {
    fn new(s: &'a str) -> Self {
        Self {
            row: 1,
            col: 1,
            text: s.chars(),
        }
    }

    fn row(&self) -> u16 {
        self.row
    }

    fn col(&self) -> u16 {
        self.col
    }

    fn peek(&self) -> Option<char> {
        self.text.clone().next()
    }

    fn next_if(&mut self, func: impl FnOnce(char) -> bool) -> Option<char> {
        self.peek().filter(|c| func(*c)).inspect(|_| {
            self.next();
        })
    }

    fn advance_while(&mut self, accept: impl Fn(char) -> bool) -> usize {
        let mut end = 0;
        while let Some(c) = self.next_if(&accept) {
            end += c.len_utf8();
        }
        end
    }

    fn take_str_while<'b>(&'b mut self, accept: impl Fn(char) -> bool) -> &'a str {
        &self.text.as_str()[..self.advance_while(accept)]
    }
}

impl Iterator for Text<'_> {
    type Item = char;

    fn next(&mut self) -> Option<Self::Item> {
        match self.text.next() {
            Some(c) => {
                if c == '\n' {
                    self.col = 1;
                    self.row += 1;
                } else {
                    self.col += 1
                }
                Some(c)
            }
            None => None,
        }
    }
}

pub struct Lexer<'a> {
    source: Text<'a>,
}

impl<'a> Lexer<'a> {
    pub fn new(source: &'a str) -> Self {
        Self {
            source: Text::new(source),
        }
    }
}

impl<'a> Iterator for Lexer<'a> {
    type Item = Result<AtCoords<Token<'a>>, AtCoordsOrEof<CompileError>>;

    fn next(&mut self) -> Option<Self::Item> {
        Some({
            let row = self.source.row();
            let col = self.source.col();

            let local = |t| AtCoords::at(row, col, t);
            let local_spanned_token = |kind, span| Ok(local(Token::new(kind, span)));
            let local_token = |kind| local_spanned_token(kind, "");
            let local_err = |err| Err(AtCoordsOrEof::at_coords(row, col, err));

            match self.source.peek()? {
                '(' => {
                    self.source.next();
                    local_token(TokenKind::LeftParen)
                }
                ')' => {
                    self.source.next();
                    local_token(TokenKind::RightParen)
                }
                '{' => {
                    self.source.next();
                    local_token(TokenKind::LeftBrace)
                }
                '}' => {
                    self.source.next();
                    local_token(TokenKind::RightBrace)
                }
                ',' => {
                    self.source.next();
                    local_token(TokenKind::Comma)
                }
                '.' => {
                    self.source.next();
                    local_token(TokenKind::Dot)
                }
                '-' => {
                    self.source.next();
                    local_token(TokenKind::Minus)
                }
                '+' => {
                    self.source.next();
                    local_token(TokenKind::Plus)
                }
                ';' => {
                    self.source.next();
                    local_token(TokenKind::Semicolon)
                }
                '*' => {
                    self.source.next();
                    local_token(TokenKind::Star)
                }
                '/' => {
                    self.source.next();
                    if self.source.next_if(|c| c == '/').is_some() {
                        self.source
                            .by_ref()
                            .take_while(|c| *c != '\n')
                            .for_each(drop);
                        self.next()?
                    } else {
                        local_token(TokenKind::Slash)
                    }
                }
                '!' => {
                    self.source.next();
                    let token = self
                        .source
                        .next_if(|c| c == '=')
                        .map_or(TokenKind::Bang, |_| TokenKind::BangEqual);
                    local_token(token)
                }
                '=' => {
                    self.source.next();
                    let token = self
                        .source
                        .next_if(|c| c == '=')
                        .map_or(TokenKind::Equal, |_| TokenKind::EqualEqual);
                    local_token(token)
                }
                '>' => {
                    self.source.next();
                    let token = self
                        .source
                        .next_if(|c| c == '=')
                        .map_or(TokenKind::Greater, |_| TokenKind::GreaterEqual);
                    local_token(token)
                }
                '<' => {
                    self.source.next();
                    let token = self
                        .source
                        .next_if(|c| c == '=')
                        .map_or(TokenKind::Less, |_| TokenKind::LessEqual);
                    local_token(token)
                }
                '"' => {
                    self.source.next();
                    let s = self.source.take_str_while(|c| c != '"');
                    if self.source.next().is_some() {
                        local_spanned_token(TokenKind::String, s)
                    } else {
                        Err(AtCoordsOrEof::at_eof(CompileError::UnclosedString))
                    }
                }
                c if c.is_ascii_digit() => {
                    let start = self.source.text.as_str();
                    let mut len = self.source.advance_while(|c| c.is_ascii_digit());
                    let mut cloned = self.source.text.clone();
                    if cloned.next().is_some_and(|c| c == '.')
                        && cloned.next().is_some_and(|c| c.is_ascii_digit())
                    {
                        // remove '.'
                        self.source.next();
                        len += 1 + self.source.advance_while(|c| c.is_ascii_digit())
                    }
                    local_spanned_token(TokenKind::Number, &start[..len])
                }
                c if c.is_alphabetic() || c == '_' => {
                    let identifier = self
                        .source
                        .take_str_while(|c| c.is_alphanumeric() || c == '_');

                    let map_identifier = || {
                        let default = || local_spanned_token(TokenKind::Identifier, identifier);

                        let mut iter = identifier.bytes();
                        let (rest, token) = match iter.next().unwrap() {
                            b'a' => ("nd", TokenKind::And),
                            b'c' => ("lass", TokenKind::Class),
                            b'e' => ("lse", TokenKind::Else),
                            b'i' => ("f", TokenKind::If),
                            b'n' => ("il", TokenKind::Nil),
                            b'o' => ("r", TokenKind::Or),
                            b'p' => ("rint", TokenKind::Print),
                            b'r' => ("eturn", TokenKind::Return),
                            b's' => ("uper", TokenKind::Super),
                            b'v' => ("ar", TokenKind::Var),
                            b'w' => ("hile", TokenKind::While),
                            b'f' => match iter.next() {
                                Some(b'a') => ("lse", TokenKind::False),
                                Some(b'o') => ("r", TokenKind::For),
                                Some(b'u') => ("n", TokenKind::Fun),
                                _ => return default(),
                            },
                            b't' => match iter.next() {
                                Some(b'h') => ("is", TokenKind::This),
                                Some(b'r') => ("ue", TokenKind::True),
                                _ => return default(),
                            },
                            _ => return default(),
                        };

                        if iter.eq(rest.bytes()) {
                            local_token(token)
                        } else {
                            default()
                        }
                    };

                    map_identifier()
                }
                c if c.is_whitespace() => {
                    self.source.next();
                    while self.source.next_if(|c| c.is_whitespace()).is_some() {}
                    self.next()?
                }
                c => {
                    self.source.next();
                    local_err(CompileError::StrayChar(c))
                }
            }
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // #[test]
    // fn big_program() {
    //     use std::time::Instant;
    //     let source = include_str!("input");
    //     let mut lexer = Lexer::new(source);
    //     let before = Instant::now();
    //     while let Some(a) = lexer.next() {
    //         drop(a);
    //     }
    //     let elapsed = before.elapsed();
    //     println!("{:?}", elapsed.as_millis());
    // }

    #[test]
    fn left_paren() {
        assert!(matches!(
            Lexer::new("(").next().unwrap().unwrap().kind(),
            TokenKind::LeftParen
        ))
    }

    #[test]
    fn right_paren() {
        assert!(matches!(
            Lexer::new(")").next().unwrap().unwrap().kind(),
            TokenKind::RightParen
        ))
    }

    #[test]
    fn left_brace() {
        assert!(matches!(
            Lexer::new("{").next().unwrap().unwrap().kind(),
            TokenKind::LeftBrace
        ))
    }

    #[test]
    fn right_brace() {
        assert!(matches!(
            Lexer::new("}").next().unwrap().unwrap().kind(),
            TokenKind::RightBrace
        ))
    }

    #[test]
    fn comma() {
        assert!(matches!(
            Lexer::new(",").next().unwrap().unwrap().kind(),
            TokenKind::Comma
        ))
    }

    #[test]
    fn dot() {
        assert!(matches!(
            Lexer::new(".").next().unwrap().unwrap().kind(),
            TokenKind::Dot
        ))
    }

    #[test]
    fn minus() {
        assert!(matches!(
            Lexer::new("-").next().unwrap().unwrap().kind(),
            TokenKind::Minus
        ))
    }

    #[test]
    fn plus() {
        assert!(matches!(
            Lexer::new("+").next().unwrap().unwrap().kind(),
            TokenKind::Plus
        ))
    }

    #[test]
    fn semicolon() {
        assert!(matches!(
            Lexer::new(";").next().unwrap().unwrap().kind(),
            TokenKind::Semicolon
        ))
    }

    #[test]
    fn slash() {
        assert!(matches!(
            Lexer::new("/").next().unwrap().unwrap().kind(),
            TokenKind::Slash
        ))
    }

    #[test]
    fn star() {
        assert!(matches!(
            Lexer::new("*").next().unwrap().unwrap().kind(),
            TokenKind::Star
        ))
    }

    #[test]
    fn bang() {
        assert!(matches!(
            Lexer::new("!").next().unwrap().unwrap().kind(),
            TokenKind::Bang
        ))
    }

    #[test]
    fn bang_equal() {
        assert!(matches!(
            Lexer::new("!=").next().unwrap().unwrap().kind(),
            TokenKind::BangEqual
        ))
    }

    #[test]
    fn equal() {
        assert!(matches!(
            Lexer::new("=").next().unwrap().unwrap().kind(),
            TokenKind::Equal
        ))
    }

    #[test]
    fn equal_equal() {
        assert!(matches!(
            Lexer::new("==").next().unwrap().unwrap().kind(),
            TokenKind::EqualEqual
        ))
    }

    #[test]
    fn greater() {
        assert!(matches!(
            Lexer::new(">").next().unwrap().unwrap().kind(),
            TokenKind::Greater
        ))
    }

    #[test]
    fn greater_equal() {
        assert!(matches!(
            Lexer::new(">=").next().unwrap().unwrap().kind(),
            TokenKind::GreaterEqual
        ))
    }

    #[test]
    fn less() {
        assert!(matches!(
            Lexer::new("<").next().unwrap().unwrap().kind(),
            TokenKind::Less
        ))
    }

    #[test]
    fn less_equal() {
        assert!(matches!(
            Lexer::new("<=").next().unwrap().unwrap().kind(),
            TokenKind::LessEqual
        ))
    }

    #[test]
    fn and() {
        assert!(matches!(
            Lexer::new("and").next().unwrap().unwrap().kind(),
            TokenKind::And
        ))
    }

    #[test]
    fn class() {
        assert!(matches!(
            Lexer::new("class").next().unwrap().unwrap().kind(),
            TokenKind::Class
        ))
    }

    #[test]
    fn r#else() {
        assert!(matches!(
            Lexer::new("else").next().unwrap().unwrap().kind(),
            TokenKind::Else
        ))
    }

    #[test]
    fn r#false() {
        assert!(matches!(
            Lexer::new("false").next().unwrap().unwrap().kind(),
            TokenKind::False
        ))
    }

    #[test]
    fn fun() {
        assert!(matches!(
            Lexer::new("fun").next().unwrap().unwrap().kind(),
            TokenKind::Fun
        ))
    }

    #[test]
    fn r#for() {
        assert!(matches!(
            Lexer::new("for").next().unwrap().unwrap().kind(),
            TokenKind::For
        ))
    }

    #[test]
    fn r#if() {
        assert!(matches!(
            Lexer::new("if").next().unwrap().unwrap().kind(),
            TokenKind::If
        ))
    }

    #[test]
    fn nil() {
        assert!(matches!(
            Lexer::new("nil").next().unwrap().unwrap().kind(),
            TokenKind::Nil
        ))
    }

    #[test]
    fn or() {
        assert!(matches!(
            Lexer::new("or").next().unwrap().unwrap().kind(),
            TokenKind::Or
        ))
    }

    #[test]
    fn print() {
        assert!(matches!(
            Lexer::new("print").next().unwrap().unwrap().kind(),
            TokenKind::Print
        ))
    }

    #[test]
    fn r#return() {
        assert!(matches!(
            Lexer::new("return").next().unwrap().unwrap().kind(),
            TokenKind::Return
        ))
    }

    #[test]
    fn ssuper() {
        assert!(matches!(
            Lexer::new("super").next().unwrap().unwrap().kind(),
            TokenKind::Super
        ))
    }

    #[test]
    fn this() {
        assert!(matches!(
            Lexer::new("this").next().unwrap().unwrap().kind(),
            TokenKind::This
        ))
    }

    #[test]
    fn r#true() {
        assert!(matches!(
            Lexer::new("true").next().unwrap().unwrap().kind(),
            TokenKind::True
        ))
    }

    #[test]
    fn r#var() {
        assert!(matches!(
            Lexer::new("var").next().unwrap().unwrap().kind(),
            TokenKind::Var
        ))
    }

    #[test]
    fn r#while() {
        assert!(matches!(
            Lexer::new("while").next().unwrap().unwrap().kind(),
            TokenKind::While
        ))
    }

    #[test]
    fn string() {
        let mut lexer = Lexer::new("\"_A0$\"");
        let token = lexer.next().unwrap().unwrap();
        assert_eq!(TokenKind::String, token.kind());
        assert_eq!("_A0$", token.span());
    }

    #[test]
    fn complete_number() {
        let mut lexer = Lexer::new("12.34");
        let token = lexer.next().unwrap().unwrap();
        assert_eq!(TokenKind::Number, token.kind());
        assert_eq!("12.34", token.span());
    }

    #[test]
    fn integer_number() {
        let mut lexer = Lexer::new("12");
        let token = lexer.next().unwrap().unwrap();
        assert_eq!(TokenKind::Number, token.kind());
        assert_eq!("12", token.span());
    }

    #[test]
    fn dot_number() {
        let mut lexer = Lexer::new(".12");
        assert_eq!(TokenKind::Dot, lexer.next().unwrap().unwrap().kind());
        let token = lexer.next().unwrap().unwrap();
        assert_eq!(TokenKind::Number, token.kind());
        assert_eq!("12", token.span());
    }

    #[test]
    fn number_dot() {
        let mut lexer = Lexer::new("12.");
        let token = lexer.next().unwrap().unwrap();
        assert_eq!("12", token.span());
        assert_eq!(TokenKind::Dot, lexer.next().unwrap().unwrap().kind());
    }

    #[test]
    fn comment() {
        assert!(Lexer::new("//").next().is_none())
    }

    #[test]
    fn unclosed_string() {
        assert!(matches!(
            Lexer::new("\"").next().unwrap().err().unwrap().value(),
            CompileError::UnclosedString
        ))
    }

    #[test]
    fn stray_char() {
        assert!(matches!(
            Lexer::new("`").next().unwrap().err().unwrap().value(),
            CompileError::StrayChar('`')
        ))
    }
}
