//! Splits an expression into tokens. The parser asks for each token in
//! turn and says whether it expects an operand, which is when `/` starts a
//! regular expression rather than dividing.

use crate::error::{Error, Result};

#[derive(Clone, Debug, PartialEq)]
pub(crate) enum Token {
    End,
    Operator(&'static str),
    String(String),
    Number(f64),
    Bool(bool),
    Null,
    Name(String),
    Variable(String),
    Regex { pattern: String, flags: String },
}

#[derive(Clone, Debug)]
pub(crate) struct Lexed {
    pub token: Token,
    /// The character the token starts at.
    pub position: usize,
}

/// Operators of two characters, which are matched before those of one.
const DOUBLE: [&str; 9] = ["..", ":=", "!=", ">=", "<=", "**", "~>", "?:", "??"];
const SINGLE: [&str; 23] = [
    ".", "[", "]", "{", "}", "(", ")", ",", "@", "#", ";", ":", "?", "+", "-", "*", "/", "%", "|",
    "=", "<", ">", "^",
];

/// Characters that end a name.
fn is_operator_char(ch: char) -> bool {
    ".[]{}(),@#;:?+-*/%|=<>^&!~".contains(ch)
}

pub(crate) struct Lexer {
    chars: Vec<char>,
    position: usize,
}

impl Lexer {
    pub fn new(source: &str) -> Self {
        Self {
            chars: source.chars().collect(),
            position: 0,
        }
    }

    fn peek(&self, offset: usize) -> Option<char> {
        self.chars.get(self.position + offset).copied()
    }

    pub fn next(&mut self, prefix: bool) -> Result<Lexed> {
        self.skip_space()?;
        let start = self.position;
        let lexed = |token| {
            Ok(Lexed {
                token,
                position: start,
            })
        };

        let Some(ch) = self.peek(0) else {
            return lexed(Token::End);
        };

        if prefix && ch == '/' {
            return self.regex().map(|token| Lexed {
                token,
                position: start,
            });
        }

        if let Some(next) = self.peek(1) {
            let pair: String = [ch, next].iter().collect();
            if let Some(operator) = DOUBLE.iter().find(|operator| **operator == pair) {
                self.position += 2;
                return lexed(Token::Operator(operator));
            }
        }
        if ch == '&' {
            self.position += 1;
            return lexed(Token::Operator("&"));
        }
        if let Some(operator) = SINGLE.iter().find(|operator| operator.starts_with(ch)) {
            self.position += 1;
            return lexed(Token::Operator(operator));
        }

        match ch {
            '"' | '\'' => self.string(ch).map(|text| Lexed {
                token: Token::String(text),
                position: start,
            }),
            '`' => {
                let end = (self.position + 1..self.chars.len())
                    .find(|&index| self.chars[index] == '`')
                    .ok_or_else(|| {
                        Error::at(
                            "S0105",
                            start,
                            "Quoted property name must be terminated with a backquote (`)",
                        )
                    })?;
                let name = self.chars[self.position + 1..end].iter().collect();
                self.position = end + 1;
                lexed(Token::Name(name))
            }
            '0'..='9' => self.number(start),
            '!' | '~' => Err(Error::at(
                "S0204",
                start,
                format!("Unknown operator: \"{ch}\""),
            )),
            _ => self.name(start),
        }
    }

    fn skip_space(&mut self) -> Result<()> {
        loop {
            match self.peek(0) {
                Some(ch) if ch.is_whitespace() => self.position += 1,
                Some('/') if self.peek(1) == Some('*') => {
                    let start = self.position;
                    self.position += 2;
                    loop {
                        match (self.peek(0), self.peek(1)) {
                            (Some('*'), Some('/')) => {
                                self.position += 2;
                                break;
                            }
                            (Some(_), _) => self.position += 1,
                            (None, _) => {
                                return Err(Error::at(
                                    "S0106",
                                    start,
                                    "Comment has no closing tag",
                                ));
                            }
                        }
                    }
                }
                _ => return Ok(()),
            }
        }
    }

    fn string(&mut self, quote: char) -> Result<String> {
        let start = self.position;
        self.position += 1;
        let mut text = String::new();

        loop {
            let Some(ch) = self.peek(0) else {
                return Err(Error::at(
                    "S0101",
                    start,
                    "String literal must be terminated by a matching quote",
                ));
            };
            self.position += 1;

            if ch == quote {
                return Ok(text);
            }
            if ch != '\\' {
                text.push(ch);
                continue;
            }

            let Some(escape) = self.peek(0) else {
                return Err(Error::at(
                    "S0101",
                    start,
                    "String literal must be terminated by a matching quote",
                ));
            };
            self.position += 1;
            match escape {
                '"' | '\\' | '/' | '\'' => text.push(escape),
                'b' => text.push('\u{8}'),
                'f' => text.push('\u{c}'),
                'n' => text.push('\n'),
                'r' => text.push('\r'),
                't' => text.push('\t'),
                'u' => {
                    let unit = self.hex_unit(start)?;
                    // A surrogate pair is two escapes for one character.
                    if (0xD800..0xDC00).contains(&unit)
                        && self.peek(0) == Some('\\')
                        && self.peek(1) == Some('u')
                    {
                        self.position += 2;
                        let low = self.hex_unit(start)?;
                        let code =
                            0x10000 + ((unit - 0xD800) << 10) + (low.wrapping_sub(0xDC00) & 0x3FF);
                        text.push(char::from_u32(code).unwrap_or('\u{FFFD}'));
                    } else {
                        text.push(char::from_u32(unit).unwrap_or('\u{FFFD}'));
                    }
                }
                other => {
                    return Err(Error::at(
                        "S0103",
                        self.position - 1,
                        format!("Unsupported escape sequence: \\{other}"),
                    ));
                }
            }
        }
    }

    fn hex_unit(&mut self, start: usize) -> Result<u32> {
        let digits: String = (0..4).filter_map(|offset| self.peek(offset)).collect();
        if digits.len() != 4 {
            return Err(Error::at(
                "S0104",
                start,
                "The escape sequence \\u must be followed by 4 hex digits",
            ));
        }
        let unit = u32::from_str_radix(&digits, 16).map_err(|_| {
            Error::at(
                "S0104",
                start,
                "The escape sequence \\u must be followed by 4 hex digits",
            )
        })?;
        self.position += 4;
        Ok(unit)
    }

    fn number(&mut self, start: usize) -> Result<Lexed> {
        let digits = |lexer: &mut Self| {
            while lexer.peek(0).is_some_and(|ch| ch.is_ascii_digit()) {
                lexer.position += 1;
            }
        };

        if self.peek(0) == Some('0') {
            self.position += 1;
        } else {
            digits(self);
        }
        if self.peek(0) == Some('.') && self.peek(1).is_some_and(|ch| ch.is_ascii_digit()) {
            self.position += 1;
            digits(self);
        }
        if matches!(self.peek(0), Some('e' | 'E')) {
            let mark = self.position;
            self.position += 1;
            if matches!(self.peek(0), Some('+' | '-')) {
                self.position += 1;
            }
            if self.peek(0).is_some_and(|ch| ch.is_ascii_digit()) {
                digits(self);
            } else {
                self.position = mark;
            }
        }

        let text: String = self.chars[start..self.position].iter().collect();
        let number: f64 = text
            .parse()
            .map_err(|_| Error::at("S0102", start, format!("Number out of range: {text}")))?;
        if !number.is_finite() {
            return Err(Error::at(
                "S0102",
                start,
                format!("Number out of range: {text}"),
            ));
        }

        Ok(Lexed {
            token: Token::Number(number),
            position: start,
        })
    }

    fn name(&mut self, start: usize) -> Result<Lexed> {
        while self
            .peek(0)
            .is_some_and(|ch| !ch.is_whitespace() && !is_operator_char(ch))
        {
            self.position += 1;
        }

        let text: String = self.chars[start..self.position].iter().collect();
        let token = if let Some(variable) = text.strip_prefix('$') {
            Token::Variable(variable.to_owned())
        } else {
            match text.as_str() {
                "and" => Token::Operator("and"),
                "or" => Token::Operator("or"),
                "in" => Token::Operator("in"),
                "true" => Token::Bool(true),
                "false" => Token::Bool(false),
                "null" => Token::Null,
                _ => Token::Name(text),
            }
        };

        Ok(Lexed {
            token,
            position: start,
        })
    }

    fn regex(&mut self) -> Result<Token> {
        let start = self.position;
        self.position += 1;
        let mut pattern = String::new();
        let mut in_class = false;

        loop {
            let Some(ch) = self.peek(0) else {
                return Err(Error::at(
                    "S0302",
                    start,
                    "No terminating / in regular expression",
                ));
            };
            self.position += 1;

            match ch {
                '\\' => {
                    pattern.push(ch);
                    if let Some(escaped) = self.peek(0) {
                        pattern.push(escaped);
                        self.position += 1;
                    }
                }
                '[' => {
                    in_class = true;
                    pattern.push(ch);
                }
                ']' => {
                    in_class = false;
                    pattern.push(ch);
                }
                '/' if !in_class => break,
                _ => pattern.push(ch),
            }
        }

        if pattern.is_empty() {
            return Err(Error::at(
                "S0301",
                start,
                "Empty regular expressions are not allowed",
            ));
        }

        let mut flags = String::new();
        while let Some(ch) = self.peek(0).filter(|ch| ch.is_ascii_alphabetic()) {
            flags.push(ch);
            self.position += 1;
        }

        Ok(Token::Regex { pattern, flags })
    }
}
