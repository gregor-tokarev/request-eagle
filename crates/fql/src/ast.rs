//! Parsed expressions, shaped for evaluation as JSONata processes its
//! syntax tree: names are path steps, filters are attached to the step they
//! follow, and sort and group-by belong to the path they apply to.

use regex::Regex;

#[derive(Debug)]
pub(crate) struct Node {
    pub kind: Kind,
    /// The character the node's token starts at, for errors.
    pub position: usize,
    /// Filters and indexes applied to what the node evaluates to.
    pub predicates: Vec<Node>,
    /// Filters and indexes applied to each item a path step produces.
    pub stages: Vec<Node>,
    /// `{key: value}` after the node, which groups what it evaluates to.
    pub group: Option<Group>,
    /// `[]` after the node keeps a single result in an array.
    pub keep_array: bool,
    /// An array constructor whose result a path keeps as one array.
    pub cons_array: bool,
    /// `@$name`: the step binds each item to a variable and keeps the
    /// previous context.
    pub focus: Option<String>,
    /// `#$name`: the step binds each item's position to a variable.
    pub index: Option<String>,
    /// The step is part of a path that binds variables with `@` or `#`.
    pub tuple: bool,
}

#[derive(Debug)]
pub(crate) struct Group {
    pub pairs: Vec<(Node, Node)>,
}

#[derive(Debug)]
pub(crate) struct Lambda {
    pub parameters: Vec<String>,
    pub body: Node,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Operator {
    Add,
    Subtract,
    Multiply,
    Divide,
    Remainder,
    Equal,
    NotEqual,
    Less,
    LessOrEqual,
    Greater,
    GreaterOrEqual,
    Concat,
    And,
    Or,
    In,
    /// `??`: the left side unless it is undefined.
    Coalesce,
    /// `?:`: the left side when it is true, as `$boolean` decides.
    Default,
}

#[derive(Debug)]
pub(crate) enum Kind {
    Path {
        steps: Vec<Node>,
        /// A step of the path ends with `[]`.
        keep_singleton: bool,
    },
    Name(String),
    String(String),
    Number(f64),
    Bool(bool),
    Null,
    Variable(String),
    Wildcard,
    Descendants,
    Regex(Regex),
    Negate(Box<Node>),
    Array(Vec<Node>),
    Range(Box<Node>, Box<Node>),
    Object(Vec<(Node, Node)>),
    Binary {
        operator: Operator,
        lhs: Box<Node>,
        rhs: Box<Node>,
    },
    Condition {
        condition: Box<Node>,
        then: Box<Node>,
        otherwise: Option<Box<Node>>,
    },
    Block(Vec<Node>),
    Bind {
        name: String,
        value: Box<Node>,
    },
    Call {
        procedure: Box<Node>,
        arguments: Vec<Node>,
    },
    /// `?` in place of an argument, for partial application.
    Placeholder,
    Lambda(Box<Lambda>),
    /// `~>`: calls the right side with the left side as its first argument.
    Apply {
        lhs: Box<Node>,
        rhs: Box<Node>,
    },
    /// `^(…)` in a path: sorts the items so far.
    Sort(Vec<(Node, bool)>),
    /// `#$name` after a filter: binds each item's position among those the
    /// filters kept. Only appears in a step's stages.
    IndexBind(String),
    Transform {
        pattern: Box<Node>,
        update: Box<Node>,
        delete: Option<Box<Node>>,
    },
}

impl Node {
    pub fn new(kind: Kind, position: usize) -> Self {
        Self {
            kind,
            position,
            predicates: Vec::new(),
            stages: Vec::new(),
            group: None,
            keep_array: false,
            cons_array: false,
            focus: None,
            index: None,
            tuple: false,
        }
    }

    /// A node with nothing after it, such as `5` rather than `5[0]`.
    pub fn is_plain(&self) -> bool {
        self.predicates.is_empty()
            && self.stages.is_empty()
            && self.group.is_none()
            && !self.keep_array
    }
}
