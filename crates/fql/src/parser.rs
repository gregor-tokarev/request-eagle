//! A Pratt parser with JSONata's operators and binding powers. It shapes the
//! tree as JSONata's `processAST` does while parsing.

use regex::RegexBuilder;

use crate::ast::{Group, Kind, Lambda, Node, Operator};
use crate::error::{Error, Result};
use crate::lexer::{Lexed, Lexer, Token};

/// How deeply expressions may nest. Parsing grows the stack as it nests, so
/// this bounds the memory a pathological expression takes.
const MAX_DEPTH: usize = 400;

pub(crate) fn parse(source: &str) -> Result<Node> {
    let mut parser = Parser {
        lexer: Lexer::new(source),
        current: Lexed {
            token: Token::End,
            position: 0,
        },
        depth: 0,
    };
    parser.current = parser.lexer.next(true)?;

    let node = parser.expression(0)?;
    if parser.current.token != Token::End {
        return Err(parser.unexpected());
    }

    Ok(node)
}

struct Parser {
    lexer: Lexer,
    current: Lexed,
    depth: usize,
}

/// How tightly an operator binds what is on its left.
fn binding_power(token: &Token) -> u8 {
    match token {
        Token::Operator(operator) => match *operator {
            "." => 75,
            "[" | "(" | "@" | "#" => 80,
            "{" => 70,
            "*" | "/" | "%" => 60,
            "+" | "-" | "&" => 50,
            "=" | "!=" | "<" | "<=" | ">" | ">=" | "in" | "~>" | "^" | "?:" | "??" => 40,
            "and" => 30,
            "or" => 25,
            "?" => 20,
            ":=" => 10,
            _ => 0,
        },
        _ => 0,
    }
}

/// Whether the token after `token` starts an operand, where `/` begins a
/// regular expression.
fn expects_operand(token: &Token) -> bool {
    match token {
        Token::Operator(operator) => !matches!(*operator, ")" | "]" | "}"),
        Token::End => false,
        _ => false,
    }
}

impl Parser {
    fn advance(&mut self) -> Result<Lexed> {
        let next = self.lexer.next(expects_operand(&self.current.token))?;
        Ok(std::mem::replace(&mut self.current, next))
    }

    fn is(&self, operator: &str) -> bool {
        self.current.token == Token::Operator(operator_static(operator))
    }

    fn expect(&mut self, operator: &'static str) -> Result<Lexed> {
        if self.current.token == Token::Operator(operator) {
            return self.advance();
        }

        Err(match self.current.token {
            Token::End => Error::at(
                "S0203",
                self.current.position,
                format!("Expected \"{operator}\" before end of expression"),
            ),
            _ => Error::at(
                "S0202",
                self.current.position,
                format!(
                    "Expected \"{operator}\", got {}",
                    describe(&self.current.token)
                ),
            ),
        })
    }

    fn unexpected(&self) -> Error {
        match &self.current.token {
            Token::End => Error::at(
                "S0203",
                self.current.position,
                "Unexpected end of expression",
            ),
            token => Error::at(
                "S0201",
                self.current.position,
                format!("Syntax error: {}", describe(token)),
            ),
        }
    }

    fn expression(&mut self, right_power: u8) -> Result<Node> {
        stacker::maybe_grow(256 * 1024, 2 * 1024 * 1024, || {
            self.nested_expression(right_power)
        })
    }

    fn nested_expression(&mut self, right_power: u8) -> Result<Node> {
        self.depth += 1;
        if self.depth > MAX_DEPTH {
            return Err(Error::at(
                "S0217",
                self.current.position,
                "The expression is nested too deeply",
            ));
        }

        let token = self.advance()?;
        let mut left = self.prefix(token)?;
        while right_power < binding_power(&self.current.token) {
            let token = self.advance()?;
            left = self.infix(token, left)?;
        }

        self.depth -= 1;
        Ok(left)
    }

    fn prefix(&mut self, lexed: Lexed) -> Result<Node> {
        let position = lexed.position;
        let node = |kind| Node::new(kind, position);

        Ok(match lexed.token {
            Token::Name(name) => path(vec![node(Kind::Name(name))], position),
            // Operators that are words are names where an operand goes.
            Token::Operator(word @ ("and" | "or" | "in")) => {
                path(vec![node(Kind::Name(word.to_owned()))], position)
            }
            Token::String(text) => node(Kind::String(text)),
            Token::Number(number) => node(Kind::Number(number)),
            Token::Bool(value) => node(Kind::Bool(value)),
            Token::Null => node(Kind::Null),
            Token::Variable(name) => node(Kind::Variable(name)),
            Token::Regex { pattern, flags } => {
                node(Kind::Regex(regex(&pattern, &flags, position)?))
            }
            Token::Operator("-") => {
                let operand = self.expression(70)?;
                match operand.kind {
                    Kind::Number(number) if operand.is_plain() => node(Kind::Number(-number)),
                    _ => node(Kind::Negate(Box::new(operand))),
                }
            }
            Token::Operator("*") => node(Kind::Wildcard),
            Token::Operator("**") => node(Kind::Descendants),
            Token::Operator("%") => {
                return Err(Error::at(
                    "S0217",
                    position,
                    "The parent operator % is not supported; bind the parent with @$name instead",
                ));
            }
            Token::Operator("(") => {
                let mut expressions = Vec::new();
                while !self.is(")") {
                    expressions.push(self.expression(0)?);
                    if !self.is(";") {
                        break;
                    }
                    self.advance()?;
                }
                self.expect(")")?;
                node(Kind::Block(expressions))
            }
            Token::Operator("[") => {
                let mut items = Vec::new();
                if !self.is("]") {
                    loop {
                        let item = self.expression(0)?;
                        if self.is("..") {
                            let range = self.advance()?;
                            let end = self.expression(0)?;
                            items.push(Node::new(
                                Kind::Range(Box::new(item), Box::new(end)),
                                range.position,
                            ));
                        } else {
                            items.push(item);
                        }
                        if !self.is(",") {
                            break;
                        }
                        self.advance()?;
                    }
                }
                self.expect("]")?;
                node(Kind::Array(items))
            }
            Token::Operator("{") => node(Kind::Object(self.pairs()?)),
            Token::Operator("|") => {
                let pattern = self.expression(0)?;
                self.expect("|")?;
                let update = self.expression(0)?;
                let delete = if self.is(",") {
                    self.advance()?;
                    Some(Box::new(self.expression(0)?))
                } else {
                    None
                };
                self.expect("|")?;
                node(Kind::Transform {
                    pattern: Box::new(pattern),
                    update: Box::new(update),
                    delete,
                })
            }
            Token::End => {
                return Err(Error::at("S0203", position, "Unexpected end of expression"));
            }
            token => {
                return Err(Error::at(
                    "S0211",
                    position,
                    format!(
                        "The symbol {} cannot be used as a unary operator",
                        describe(&token)
                    ),
                ));
            }
        })
    }

    fn infix(&mut self, lexed: Lexed, left: Node) -> Result<Node> {
        let position = lexed.position;
        let Token::Operator(operator) = lexed.token else {
            return Err(Error::at("S0201", position, "Syntax error"));
        };

        let binary =
            |parser: &mut Self, operator: Operator, power: u8, left: Node| -> Result<Node> {
                let rhs = parser.expression(power)?;
                Ok(Node::new(
                    Kind::Binary {
                        operator,
                        lhs: Box::new(left),
                        rhs: Box::new(rhs),
                    },
                    position,
                ))
            };

        match operator {
            "." => {
                let rest = self.expression(75)?;
                join_path(left, rest, position)
            }
            "+" => binary(self, Operator::Add, 50, left),
            "-" => binary(self, Operator::Subtract, 50, left),
            "*" => binary(self, Operator::Multiply, 60, left),
            "/" => binary(self, Operator::Divide, 60, left),
            "%" => binary(self, Operator::Remainder, 60, left),
            "=" => binary(self, Operator::Equal, 40, left),
            "!=" => binary(self, Operator::NotEqual, 40, left),
            "<" => binary(self, Operator::Less, 40, left),
            "<=" => binary(self, Operator::LessOrEqual, 40, left),
            ">" => binary(self, Operator::Greater, 40, left),
            ">=" => binary(self, Operator::GreaterOrEqual, 40, left),
            "&" => binary(self, Operator::Concat, 50, left),
            "and" => binary(self, Operator::And, 30, left),
            "or" => binary(self, Operator::Or, 25, left),
            "in" => binary(self, Operator::In, 40, left),
            // The right side of these takes the rest of the expression.
            "??" => binary(self, Operator::Coalesce, 0, left),
            "?:" => binary(self, Operator::Default, 0, left),
            "~>" => {
                let rhs = self.expression(40)?;
                Ok(Node::new(
                    Kind::Apply {
                        lhs: Box::new(left),
                        rhs: Box::new(rhs),
                    },
                    position,
                ))
            }
            "?" => {
                let then = self.expression(0)?;
                let otherwise = if self.is(":") {
                    self.advance()?;
                    Some(Box::new(self.expression(0)?))
                } else {
                    None
                };
                Ok(Node::new(
                    Kind::Condition {
                        condition: Box::new(left),
                        then: Box::new(then),
                        otherwise,
                    },
                    position,
                ))
            }
            ":=" => {
                let Kind::Variable(name) = left.kind else {
                    return Err(Error::at(
                        "S0212",
                        left.position,
                        "The left side of := must be a variable name (start with $)",
                    ));
                };
                let value = self.expression(9)?;
                Ok(Node::new(
                    Kind::Bind {
                        name,
                        value: Box::new(value),
                    },
                    position,
                ))
            }
            "(" => self.call(left, position),
            "[" => self.filter(left, position),
            "{" => {
                let pairs = self.pairs()?;
                let mut left = left;
                if left.group.is_some() {
                    return Err(Error::at(
                        "S0210",
                        position,
                        "Each step can only have one grouping expression",
                    ));
                }
                left.group = Some(Group { pairs });
                Ok(left)
            }
            "^" => {
                self.expect("(")?;
                let mut terms = Vec::new();
                loop {
                    let descending = if self.is("<") {
                        self.advance()?;
                        false
                    } else if self.is(">") {
                        self.advance()?;
                        true
                    } else {
                        false
                    };
                    terms.push((self.expression(0)?, descending));
                    if !self.is(",") {
                        break;
                    }
                    self.advance()?;
                }
                self.expect(")")?;

                let mut steps = into_steps(left);
                steps.push(Node::new(Kind::Sort(terms), position));
                Ok(path(steps, position))
            }
            "@" | "#" => {
                let target = self.expression(80)?;
                let Kind::Variable(name) = target.kind else {
                    return Err(Error::at(
                        "S0214",
                        target.position,
                        format!(
                            "The right side of {operator} must be a variable name (start with $)"
                        ),
                    ));
                };
                bind_step(left, operator == "@", name, position)
            }
            _ => Err(Error::at(
                "S0201",
                position,
                format!("Syntax error: \"{operator}\""),
            )),
        }
    }

    fn pairs(&mut self) -> Result<Vec<(Node, Node)>> {
        let mut pairs = Vec::new();
        if !self.is("}") {
            loop {
                let key = self.expression(0)?;
                self.expect(":")?;
                let value = self.expression(0)?;
                pairs.push((key, value));
                if !self.is(",") {
                    break;
                }
                self.advance()?;
            }
        }
        self.expect("}")?;
        Ok(pairs)
    }

    fn call(&mut self, procedure: Node, position: usize) -> Result<Node> {
        let mut arguments = Vec::new();
        if !self.is(")") {
            loop {
                if self.is("?") {
                    let placeholder = self.advance()?;
                    arguments.push(Node::new(Kind::Placeholder, placeholder.position));
                } else {
                    arguments.push(self.expression(0)?);
                }
                if !self.is(",") {
                    break;
                }
                self.advance()?;
            }
        }
        self.expect(")")?;

        // `function($x) { … }`, also written `λ($x)` or, as in Postman, `fn($x)`.
        let lambda = matches!(&procedure.kind, Kind::Path { steps, .. }
            if steps.len() == 1 && matches!(&steps[0].kind, Kind::Name(name) if name == "function" || name == "λ" || name == "fn"));
        if !lambda {
            return Ok(Node::new(
                Kind::Call {
                    procedure: Box::new(procedure),
                    arguments,
                },
                position,
            ));
        }

        let mut parameters = Vec::new();
        for argument in arguments {
            match argument.kind {
                Kind::Variable(name) => parameters.push(name),
                _ => {
                    return Err(Error::at(
                        "S0208",
                        argument.position,
                        "Parameters of a function definition must be variables (start with $)",
                    ));
                }
            }
        }

        // A signature such as `<n-n:n>` documents the parameters' types.
        if self.is("<") {
            let mut depth = 0;
            loop {
                let token = self.advance()?;
                match token.token {
                    Token::Operator("<") => depth += 1,
                    Token::Operator(">") => depth -= 1,
                    Token::End => return Err(self.unexpected()),
                    _ => {}
                }
                if depth == 0 {
                    break;
                }
            }
        }

        self.expect("{")?;
        let body = self.expression(0)?;
        self.expect("}")?;

        Ok(Node::new(
            Kind::Lambda(Box::new(Lambda { parameters, body })),
            position,
        ))
    }

    fn filter(&mut self, left: Node, position: usize) -> Result<Node> {
        let mut left = left;

        // `[]` keeps a single result in an array.
        if self.is("]") {
            self.advance()?;
            match &mut left.kind {
                Kind::Path {
                    steps,
                    keep_singleton,
                } => {
                    *keep_singleton = true;
                    if let Some(step) = steps.last_mut() {
                        step.keep_array = true;
                    }
                }
                _ => left.keep_array = true,
            }
            return Ok(left);
        }

        let predicate = self.expression(0)?;
        self.expect("]")?;

        match &mut left.kind {
            Kind::Path { steps, .. } => {
                let step = steps.last_mut().expect("a path has steps");
                if step.group.is_some() {
                    return Err(Error::at(
                        "S0209",
                        position,
                        "A predicate cannot follow a grouping expression in a step",
                    ));
                }
                step.stages.push(predicate);
            }
            _ => left.predicates.push(predicate),
        }

        Ok(left)
    }
}

fn operator_static(operator: &str) -> &'static str {
    const ALL: [&str; 35] = [
        ".", "[", "]", "{", "}", "(", ")", ",", "@", "#", ";", ":", "?", "+", "-", "*", "/", "%",
        "|", "=", "<", ">", "^", "..", ":=", "!=", ">=", "<=", "**", "~>", "?:", "??", "&", "and",
        "or",
    ];
    ALL.iter()
        .find(|candidate| **candidate == operator)
        .copied()
        .unwrap_or("")
}

fn describe(token: &Token) -> String {
    match token {
        Token::End => "end of expression".to_owned(),
        Token::Operator(operator) => format!("\"{operator}\""),
        Token::String(text) => format!("\"{text}\""),
        Token::Number(number) => number.to_string(),
        Token::Bool(value) => value.to_string(),
        Token::Null => "null".to_owned(),
        Token::Name(name) => name.clone(),
        Token::Variable(name) => format!("${name}"),
        Token::Regex { pattern, .. } => format!("/{pattern}/"),
    }
}

fn path(steps: Vec<Node>, position: usize) -> Node {
    let keep_singleton = steps.iter().any(|step| step.keep_array);
    Node::new(
        Kind::Path {
            steps,
            keep_singleton,
        },
        position,
    )
}

/// The steps of a node used as the start of a path.
fn into_steps(node: Node) -> Vec<Node> {
    match node {
        Node {
            kind: Kind::Path { steps, .. },
            group: None,
            ..
        } => steps,
        node => vec![node],
    }
}

/// `left.rest`: a path of the steps of both.
fn join_path(left: Node, mut rest: Node, position: usize) -> Result<Node> {
    let mut steps = into_steps(left);

    match rest.kind {
        Kind::Path {
            steps: rest_steps, ..
        } if rest.group.is_none() => steps.extend(rest_steps),
        _ => {
            // A step's filters apply to each item it produces.
            rest.stages.append(&mut rest.predicates);
            steps.push(rest);
        }
    }

    for step in &mut steps {
        match &step.kind {
            Kind::String(text) => step.kind = Kind::Name(text.clone()),
            Kind::Number(_) | Kind::Bool(_) | Kind::Null => {
                return Err(Error::at(
                    "S0213",
                    step.position,
                    "A literal value cannot be used as a step within a path expression",
                ));
            }
            _ => {}
        }
    }

    // Array constructors at either end of a path keep their arrays.
    if let Some(first) = steps.first_mut()
        && matches!(first.kind, Kind::Array(_))
    {
        first.cons_array = true;
    }
    if let Some(last) = steps.last_mut()
        && matches!(last.kind, Kind::Array(_))
    {
        last.cons_array = true;
    }

    Ok(path(steps, position))
}

/// `step@$name` or `step#$name`.
fn bind_step(left: Node, focus: bool, name: String, position: usize) -> Result<Node> {
    let mut steps = into_steps(left);
    let step = steps.last_mut().expect("a path has steps");

    if focus {
        if !step.stages.is_empty() || !step.predicates.is_empty() {
            return Err(Error::at(
                "S0215",
                position,
                "A context variable binding must precede any predicates on a step",
            ));
        }
        if matches!(step.kind, Kind::Sort(_)) {
            return Err(Error::at(
                "S0216",
                position,
                "A context variable binding must precede the order-by clause on a step",
            ));
        }
        step.focus = Some(name);
    } else {
        // Filters of a step that was not in a path apply to its items.
        let mut predicates = std::mem::take(&mut step.predicates);
        step.stages.append(&mut predicates);
        if step.stages.is_empty() {
            step.index = Some(name);
        } else {
            step.stages.push(Node::new(Kind::IndexBind(name), position));
        }
    }
    step.tuple = true;

    Ok(path(steps, position))
}

fn regex(pattern: &str, flags: &str, position: usize) -> Result<regex::Regex> {
    let mut builder = RegexBuilder::new(&crate::patterns::translate(pattern));
    for flag in flags.chars() {
        match flag {
            'i' => {
                builder.case_insensitive(true);
            }
            'm' => {
                builder.multi_line(true);
            }
            // Matching is always global.
            'g' => {}
            _ => {
                return Err(Error::at(
                    "S0302",
                    position,
                    format!("Unsupported regular expression flag: {flag}"),
                ));
            }
        }
    }

    builder.size_limit(1 << 20).build().map_err(|error| {
        Error::at(
            "S0302",
            position,
            format!(
                "Invalid regular expression: {}",
                error.to_string().lines().last().unwrap_or_default()
            ),
        )
    })
}
