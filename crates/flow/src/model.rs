use std::borrow::Cow;

use serde::{Deserialize, Serialize};

/// Blocks on a canvas and the connections that carry data between their ports.
#[derive(Clone, Debug, Default, PartialEq, Deserialize, Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct Flow {
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub blocks: Vec<Block>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub connections: Vec<Connection>,
}

#[derive(Clone, Debug, PartialEq, Deserialize, Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct Block {
    /// Unique in the flow; connections refer to blocks by it.
    pub id: String,
    /// The name shown instead of the block type's.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    /// The top-left corner on the canvas, in pixels at 100% zoom with the
    /// default 16 px interface font.
    pub x: f32,
    pub y: f32,
    #[serde(flatten)]
    pub kind: BlockKind,
}

/// What a block does, with its settings.
#[derive(Clone, Debug, PartialEq, Deserialize, Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum BlockKind {
    /// Where a run begins. Sends the run's input, or this JSON when the run
    /// has none.
    Start {
        #[serde(default, skip_serializing_if = "String::is_empty")]
        input: String,
    },
    /// Sends a saved HTTP request, chosen by its ID. Each `{{variable}}` it
    /// uses is an input; a connected value replaces the variable's.
    HttpRequest {
        #[serde(default)]
        request: String,
    },
    /// Computes an FQL expression. The variables are fields of its input.
    Evaluate {
        #[serde(default = "default_variables")]
        variables: Vec<String>,
        #[serde(default)]
        expression: String,
    },
    /// Sends its data out of Then or Else, as an FQL condition decides.
    If {
        #[serde(default = "default_variables")]
        variables: Vec<String>,
        #[serde(default)]
        condition: String,
    },
    /// Sends its variables out of the first condition that holds, or Default.
    Condition {
        #[serde(default = "default_variables")]
        variables: Vec<String>,
        #[serde(default)]
        conditions: Vec<String>,
    },
    /// Sends data out of Pass or Fail, as a JSON Schema decides.
    Validate {
        #[serde(default)]
        schema: String,
    },
    Delay {
        #[serde(default = "default_delay")]
        milliseconds: u64,
    },
    /// Sends on whatever arrives at either input.
    Or,
    /// Sends 0, 1, … up to its count, one at a time, for a Collect to gather.
    Repeat,
    /// Sends each item of its list, one at a time, for a Collect to gather.
    For,
    /// Gathers what a loop sends back into a list once the loop ends.
    Collect,
    /// Shows the latest data on the canvas and sends it on.
    Display {
        #[serde(default, skip_serializing_if = "DisplayFormat::is_auto")]
        format: DisplayFormat,
    },
    /// Writes everything it receives to the run log.
    Log,
    String {
        #[serde(default)]
        value: String,
    },
    Number {
        #[serde(default)]
        value: f64,
    },
    Boolean {
        #[serde(default)]
        value: bool,
    },
    Null,
    /// The time the block runs, in milliseconds since the Unix epoch.
    Now,
    /// An ISO 8601 date, in milliseconds since the Unix epoch.
    Date {
        #[serde(default)]
        value: String,
    },
    /// Picks a value out of its data by a dotted path, such as `body.items.0.id`.
    Select {
        #[serde(default)]
        path: String,
    },
    /// An object whose fields are inputs. A field that is not connected
    /// holds its own value.
    Record {
        #[serde(default)]
        fields: Vec<Field>,
    },
    /// A list whose items are inputs. An item that is not connected holds
    /// its own value.
    List {
        #[serde(default)]
        items: Vec<String>,
    },
    /// Fills `{{variables}}` and Mustache sections into text.
    Template {
        #[serde(default = "default_variables")]
        variables: Vec<String>,
        #[serde(default)]
        template: String,
        #[serde(default, skip_serializing_if = "TemplateFormat::is_text")]
        format: TemplateFormat,
    },
    /// Stores what it receives under a name, and sends it from every Get
    /// Variable block of that name.
    SetVariable {
        #[serde(default)]
        name: String,
    },
    GetVariable {
        #[serde(default)]
        name: String,
    },
    /// The values a run returns, one input for each name.
    Output {
        #[serde(default = "default_outputs")]
        names: Vec<String>,
    },
    /// Text on the canvas, which does not run. A Note behind blocks frames
    /// them as a section: moving it moves the blocks inside it.
    Note {
        #[serde(default)]
        text: String,
        /// Its size, in the pixels of `x` and `y`. Without one it has the
        /// default size.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        width: Option<f32>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        height: Option<f32>,
    },
}

/// A field of a Record block. `value` is JSON, or text when it is not JSON.
#[derive(Clone, Debug, Default, PartialEq, Eq, Deserialize, Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct Field {
    pub key: String,
    #[serde(default)]
    pub value: String,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Deserialize, Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "snake_case")]
pub enum DisplayFormat {
    #[default]
    Auto,
    Json,
    Text,
    Table,
}

impl DisplayFormat {
    pub const ALL: [Self; 4] = [Self::Auto, Self::Json, Self::Text, Self::Table];

    fn is_auto(&self) -> bool {
        *self == Self::Auto
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Auto => "Auto",
            Self::Json => "JSON",
            Self::Text => "Text",
            Self::Table => "Table",
        }
    }
}

/// What a Template block sends: its text, or the JSON the text holds.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Deserialize, Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "snake_case")]
pub enum TemplateFormat {
    #[default]
    Text,
    Json,
}

impl TemplateFormat {
    fn is_text(&self) -> bool {
        *self == Self::Text
    }
}

/// Carries what a block sends from one of its outputs to an input of another.
#[derive(Clone, Debug, PartialEq, Eq, Hash, Deserialize, Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct Connection {
    pub from: String,
    pub output: String,
    pub to: String,
    pub input: String,
}

fn default_variables() -> Vec<String> {
    vec!["value1".to_owned()]
}

fn default_outputs() -> Vec<String> {
    vec!["result".to_owned()]
}

fn default_delay() -> u64 {
    1000
}

/// The kinds of block, in the order the block picker lists them.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BlockType {
    Start,
    HttpRequest,
    Evaluate,
    If,
    Condition,
    Validate,
    Delay,
    Or,
    Repeat,
    For,
    Collect,
    Display,
    Log,
    String,
    Number,
    Boolean,
    Null,
    Now,
    Date,
    Select,
    Record,
    List,
    Template,
    SetVariable,
    GetVariable,
    Output,
    Note,
}

impl BlockType {
    pub const ALL: [Self; 27] = [
        Self::Start,
        Self::HttpRequest,
        Self::Evaluate,
        Self::If,
        Self::Condition,
        Self::Validate,
        Self::Delay,
        Self::Or,
        Self::Repeat,
        Self::For,
        Self::Collect,
        Self::Display,
        Self::Log,
        Self::String,
        Self::Number,
        Self::Boolean,
        Self::Null,
        Self::Now,
        Self::Date,
        Self::Select,
        Self::Record,
        Self::List,
        Self::Template,
        Self::SetVariable,
        Self::GetVariable,
        Self::Output,
        Self::Note,
    ];

    pub fn name(self) -> &'static str {
        match self {
            Self::Start => "Start",
            Self::HttpRequest => "HTTP Request",
            Self::Evaluate => "Evaluate",
            Self::If => "If",
            Self::Condition => "Condition",
            Self::Validate => "Validate",
            Self::Delay => "Delay",
            Self::Or => "OR",
            Self::Repeat => "Repeat",
            Self::For => "For",
            Self::Collect => "Collect",
            Self::Display => "Display",
            Self::Log => "Log",
            Self::String => "String",
            Self::Number => "Number",
            Self::Boolean => "Bool",
            Self::Null => "Null",
            Self::Now => "Now",
            Self::Date => "Date",
            Self::Select => "Select",
            Self::Record => "Record",
            Self::List => "List",
            Self::Template => "Template",
            Self::SetVariable => "Create Variable",
            Self::GetVariable => "Get Variable",
            Self::Output => "Output",
            Self::Note => "Note",
        }
    }

    pub fn description(self) -> &'static str {
        match self {
            Self::Start => "Starts the run with its input",
            Self::HttpRequest => "Sends a saved request; its variables are inputs",
            Self::Evaluate => "Computes a value with FQL",
            Self::If => "Routes data by a condition",
            Self::Condition => "Routes by the first condition that holds",
            Self::Validate => "Checks data against a JSON Schema",
            Self::Delay => "Waits before sending data on",
            Self::Or => "Sends whichever input arrives",
            Self::Repeat => "Loops a number of times",
            Self::For => "Loops over each item of a list",
            Self::Collect => "Gathers a loop's results into a list",
            Self::Display => "Shows data on the canvas",
            Self::Log => "Writes data to the run log",
            Self::String => "A text value",
            Self::Number => "A number value",
            Self::Boolean => "True or false",
            Self::Null => "The null value",
            Self::Now => "The current time",
            Self::Date => "A date and time",
            Self::Select => "Picks a value by its path",
            Self::Record => "Builds an object",
            Self::List => "Builds a list",
            Self::Template => "Fills variables into text or JSON",
            Self::SetVariable => "Stores a value under a name",
            Self::GetVariable => "Sends a stored value",
            Self::Output => "Returns values from the run",
            Self::Note => "Text on the canvas",
        }
    }

    /// A new block of this type with its default settings.
    pub fn block_kind(self) -> BlockKind {
        match self {
            Self::Start => BlockKind::Start {
                input: String::new(),
            },
            Self::HttpRequest => BlockKind::HttpRequest {
                request: String::new(),
            },
            Self::Evaluate => BlockKind::Evaluate {
                variables: default_variables(),
                expression: "value1".to_owned(),
            },
            Self::If => BlockKind::If {
                variables: default_variables(),
                condition: String::new(),
            },
            Self::Condition => BlockKind::Condition {
                variables: default_variables(),
                conditions: vec![String::new()],
            },
            Self::Validate => BlockKind::Validate {
                schema: "{\n  \"type\": \"object\"\n}".to_owned(),
            },
            Self::Delay => BlockKind::Delay {
                milliseconds: default_delay(),
            },
            Self::Or => BlockKind::Or,
            Self::Repeat => BlockKind::Repeat,
            Self::For => BlockKind::For,
            Self::Collect => BlockKind::Collect,
            Self::Display => BlockKind::Display {
                format: DisplayFormat::Auto,
            },
            Self::Log => BlockKind::Log,
            Self::String => BlockKind::String {
                value: String::new(),
            },
            Self::Number => BlockKind::Number { value: 0. },
            Self::Boolean => BlockKind::Boolean { value: true },
            Self::Null => BlockKind::Null,
            Self::Now => BlockKind::Now,
            Self::Date => BlockKind::Date {
                value: chrono::Utc::now().format("%Y-%m-%dT%H:%M:%SZ").to_string(),
            },
            Self::Select => BlockKind::Select {
                path: String::new(),
            },
            Self::Record => BlockKind::Record {
                fields: vec![Field {
                    key: "key1".to_owned(),
                    value: String::new(),
                }],
            },
            Self::List => BlockKind::List {
                items: vec![String::new()],
            },
            Self::Template => BlockKind::Template {
                variables: default_variables(),
                template: "{{value1}}".to_owned(),
                format: TemplateFormat::Text,
            },
            Self::SetVariable => BlockKind::SetVariable {
                name: "variable".to_owned(),
            },
            Self::GetVariable => BlockKind::GetVariable {
                name: "variable".to_owned(),
            },
            Self::Output => BlockKind::Output {
                names: default_outputs(),
            },
            Self::Note => BlockKind::Note {
                text: String::new(),
                width: None,
                height: None,
            },
        }
    }
}

impl BlockKind {
    pub fn block_type(&self) -> BlockType {
        match self {
            Self::Start { .. } => BlockType::Start,
            Self::HttpRequest { .. } => BlockType::HttpRequest,
            Self::Evaluate { .. } => BlockType::Evaluate,
            Self::If { .. } => BlockType::If,
            Self::Condition { .. } => BlockType::Condition,
            Self::Validate { .. } => BlockType::Validate,
            Self::Delay { .. } => BlockType::Delay,
            Self::Or => BlockType::Or,
            Self::Repeat => BlockType::Repeat,
            Self::For => BlockType::For,
            Self::Collect => BlockType::Collect,
            Self::Display { .. } => BlockType::Display,
            Self::Log => BlockType::Log,
            Self::String { .. } => BlockType::String,
            Self::Number { .. } => BlockType::Number,
            Self::Boolean { .. } => BlockType::Boolean,
            Self::Null => BlockType::Null,
            Self::Now => BlockType::Now,
            Self::Date { .. } => BlockType::Date,
            Self::Select { .. } => BlockType::Select,
            Self::Record { .. } => BlockType::Record,
            Self::List { .. } => BlockType::List,
            Self::Template { .. } => BlockType::Template,
            Self::SetVariable { .. } => BlockType::SetVariable,
            Self::GetVariable { .. } => BlockType::GetVariable,
            Self::Output { .. } => BlockType::Output,
            Self::Note { .. } => BlockType::Note,
        }
    }

    /// The block's inputs, in the order they are drawn. An HTTP Request also
    /// has an input for each variable of its request; see
    /// `request_variables`.
    pub fn inputs(&self) -> Vec<Cow<'_, str>> {
        fn fixed<'a>(names: &[&'static str]) -> Vec<Cow<'a, str>> {
            names.iter().copied().map(Cow::Borrowed).collect()
        }
        fn named(names: &[String]) -> Vec<Cow<'_, str>> {
            names.iter().map(|name| Cow::from(name.as_str())).collect()
        }

        match self {
            Self::HttpRequest { .. } => fixed(&["send"]),
            Self::Evaluate { variables, .. }
            | Self::Condition { variables, .. }
            | Self::Template { variables, .. } => named(variables),
            Self::If { variables, .. } => {
                let mut inputs = named(variables);
                inputs.push(Cow::Borrowed("data"));
                inputs
            }
            Self::Validate { .. }
            | Self::Delay { .. }
            | Self::Display { .. }
            | Self::Log
            | Self::Select { .. } => fixed(&["data"]),
            Self::Or => fixed(&["first", "second"]),
            Self::Repeat => fixed(&["count", "start"]),
            Self::For => fixed(&["list", "start"]),
            Self::Collect => fixed(&["item"]),
            Self::Record { fields } => fields
                .iter()
                .map(|field| Cow::from(field.key.as_str()))
                .collect(),
            Self::List { items } => (1..=items.len())
                .map(|index| Cow::Owned(format!("item{index}")))
                .collect(),
            Self::SetVariable { .. } => fixed(&["value"]),
            Self::Output { names } => named(names),
            Self::Start { .. }
            | Self::String { .. }
            | Self::Number { .. }
            | Self::Boolean { .. }
            | Self::Null
            | Self::Now
            | Self::Date { .. }
            | Self::GetVariable { .. }
            | Self::Note { .. } => Vec::new(),
        }
    }

    /// The block's outputs, in the order they are drawn.
    pub fn outputs(&self) -> Vec<Cow<'_, str>> {
        fn fixed<'a>(names: &[&'static str]) -> Vec<Cow<'a, str>> {
            names.iter().copied().map(Cow::Borrowed).collect()
        }

        match self {
            Self::Start { .. } => fixed(&["data"]),
            Self::HttpRequest { .. } => fixed(&["success", "fail"]),
            Self::Evaluate { .. } | Self::Template { .. } => fixed(&["result"]),
            Self::If { .. } => fixed(&["then", "else"]),
            Self::Condition { conditions, .. } => (1..=conditions.len())
                .map(|index| Cow::Owned(format!("condition{index}")))
                .chain([Cow::Borrowed("default")])
                .collect(),
            Self::Validate { .. } => fixed(&["pass", "fail"]),
            Self::Delay { .. } | Self::Or | Self::Display { .. } => fixed(&["data"]),
            Self::Repeat => fixed(&["index"]),
            Self::For => fixed(&["item"]),
            Self::Collect => fixed(&["list", "finish"]),
            Self::String { .. }
            | Self::Number { .. }
            | Self::Boolean { .. }
            | Self::Null
            | Self::Now
            | Self::Date { .. }
            | Self::Select { .. }
            | Self::GetVariable { .. } => fixed(&["value"]),
            Self::Record { .. } => fixed(&["record"]),
            Self::List { .. } => fixed(&["list"]),
            Self::Log | Self::SetVariable { .. } | Self::Output { .. } | Self::Note { .. } => {
                Vec::new()
            }
        }
    }

    /// Whether the block runs when the flow starts while none of its inputs
    /// are connected. Others wait for data.
    pub fn is_source(&self) -> bool {
        matches!(
            self,
            Self::Start { .. }
                | Self::HttpRequest { .. }
                | Self::Evaluate { .. }
                | Self::If { .. }
                | Self::Condition { .. }
                | Self::String { .. }
                | Self::Number { .. }
                | Self::Boolean { .. }
                | Self::Null
                | Self::Now
                | Self::Date { .. }
                | Self::Record { .. }
                | Self::List { .. }
                | Self::Template { .. }
        )
    }

    /// The user-named inputs that FQL and templates read as variables.
    pub fn variables(&self) -> Option<&Vec<String>> {
        match self {
            Self::Evaluate { variables, .. }
            | Self::If { variables, .. }
            | Self::Condition { variables, .. }
            | Self::Template { variables, .. } => Some(variables),
            _ => None,
        }
    }

    pub fn variables_mut(&mut self) -> Option<&mut Vec<String>> {
        match self {
            Self::Evaluate { variables, .. }
            | Self::If { variables, .. }
            | Self::Condition { variables, .. }
            | Self::Template { variables, .. } => Some(variables),
            _ => None,
        }
    }
}

impl Block {
    /// The block's own title, or its type's name.
    pub fn title(&self) -> &str {
        self.title
            .as_deref()
            .filter(|title| !title.trim().is_empty())
            .unwrap_or(self.kind.block_type().name())
    }
}

impl Flow {
    /// What a new flow holds: a Start block, where its runs begin.
    pub fn starter() -> Self {
        Self {
            blocks: vec![Block {
                id: "b1".to_owned(),
                title: None,
                x: 0.,
                y: 0.,
                kind: BlockType::Start.block_kind(),
            }],
            connections: Vec::new(),
        }
    }

    pub fn block(&self, id: &str) -> Option<&Block> {
        self.blocks.iter().find(|block| block.id == id)
    }

    pub fn block_mut(&mut self, id: &str) -> Option<&mut Block> {
        self.blocks.iter_mut().find(|block| block.id == id)
    }

    /// An ID no block of the flow has, such as `b4`.
    pub fn next_block_id(&self) -> String {
        let highest = self
            .blocks
            .iter()
            .filter_map(|block| block.id.strip_prefix('b')?.parse::<u64>().ok())
            .max()
            .unwrap_or(0);

        format!("b{}", highest + 1)
    }

    /// The connection into an input. An input takes one connection.
    pub fn connection_into(&self, block: &str, input: &str) -> Option<&Connection> {
        self.connections
            .iter()
            .find(|connection| connection.to == block && connection.input == input)
    }

    /// Remove blocks and every connection to or from them.
    pub fn remove_blocks(&mut self, ids: &[String]) {
        self.blocks.retain(|block| !ids.contains(&block.id));
        self.connections
            .retain(|connection| !ids.contains(&connection.from) && !ids.contains(&connection.to));
    }

    /// Connect an output to an input, replacing the input's connection.
    pub fn connect(&mut self, connection: Connection) {
        self.connections
            .retain(|existing| existing.to != connection.to || existing.input != connection.input);
        self.connections.push(connection);
    }

    /// Follow a renamed input or output of a block.
    pub fn rename_port(&mut self, block: &str, output: bool, from: &str, to: &str) {
        for connection in &mut self.connections {
            if output && connection.from == block && connection.output == from {
                connection.output = to.to_owned();
            } else if !output && connection.to == block && connection.input == from {
                connection.input = to.to_owned();
            }
        }
    }

    /// Why the flow cannot be saved or run as it is, if it cannot.
    pub fn check(&self) -> Result<(), String> {
        for (index, block) in self.blocks.iter().enumerate() {
            if block.id.is_empty()
                || !block
                    .id
                    .chars()
                    .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '_' | '-'))
            {
                return Err(format!(
                    "Block ID \"{}\" must be letters, digits, '_' or '-'",
                    block.id
                ));
            }
            if self.blocks[..index]
                .iter()
                .any(|other| other.id == block.id)
            {
                return Err(format!("Two blocks have the ID \"{}\"", block.id));
            }
            if !block.x.is_finite() || !block.y.is_finite() {
                return Err(format!("Block \"{}\" has no position", block.id));
            }
            if let BlockKind::Note { width, height, .. } = &block.kind
                && [width, height]
                    .into_iter()
                    .flatten()
                    .any(|size| !size.is_finite() || *size <= 0.)
            {
                return Err(format!("Note \"{}\" must have a positive size", block.id));
            }

            check_names(block)?;
        }

        for (index, connection) in self.connections.iter().enumerate() {
            let from = self.block(&connection.from).ok_or_else(|| {
                format!(
                    "A connection starts at unknown block \"{}\"",
                    connection.from
                )
            })?;
            let to = self.block(&connection.to).ok_or_else(|| {
                format!("A connection ends at unknown block \"{}\"", connection.to)
            })?;

            if !from
                .kind
                .outputs()
                .iter()
                .any(|output| *output == connection.output)
            {
                return Err(format!(
                    "Block \"{}\" has no output \"{}\"",
                    from.id, connection.output
                ));
            }
            // An HTTP Request's inputs follow the variables of its request,
            // which can change after the connection is made.
            let request_variable = matches!(to.kind, BlockKind::HttpRequest { .. })
                && !connection.input.trim().is_empty();
            if !request_variable
                && !to
                    .kind
                    .inputs()
                    .iter()
                    .any(|input| *input == connection.input)
            {
                return Err(format!(
                    "Block \"{}\" has no input \"{}\"",
                    to.id, connection.input
                ));
            }
            if self.connections[..index]
                .iter()
                .any(|other| other.to == connection.to && other.input == connection.input)
            {
                return Err(format!(
                    "Input \"{}\" of block \"{}\" has more than one connection",
                    connection.input, connection.to
                ));
            }
        }

        Ok(())
    }
}

/// Variables are FQL field names, so they must be plain identifiers, and
/// every input or output of a block needs its own name.
fn check_names(block: &Block) -> Result<(), String> {
    if let Some(variables) = block.kind.variables() {
        for variable in variables {
            if !is_identifier(variable) {
                return Err(format!(
                    "Variable \"{variable}\" of block \"{}\" must start with a letter or '_' and \
                     contain only letters, digits and '_'",
                    block.id
                ));
            }
        }
    }

    let inputs = block.kind.inputs();
    for (index, input) in inputs.iter().enumerate() {
        if input.trim().is_empty() {
            return Err(format!(
                "Block \"{}\" has an input without a name",
                block.id
            ));
        }
        if inputs[..index].contains(input) {
            return Err(format!(
                "Block \"{}\" has two inputs named \"{input}\"",
                block.id
            ));
        }
    }

    Ok(())
}

pub fn is_identifier(name: &str) -> bool {
    let mut chars = name.chars();
    chars
        .next()
        .is_some_and(|first| first.is_alphabetic() || first == '_')
        && chars.all(|ch| ch.is_alphanumeric() || ch == '_')
}
