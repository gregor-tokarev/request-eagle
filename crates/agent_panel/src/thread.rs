//! The conversation the panel shows: the messages sent, and what the agent
//! answered, thought and did, as its updates arrive.

use agent_client_protocol::schema::v1::{
    ContentBlock, PlanEntryStatus, SessionUpdate, ToolCallContent, ToolCallId, ToolCallStatus,
    ToolCallUpdate, ToolKind,
};
use tab_ui::Cite;

pub(crate) enum Entry {
    User(UserMessage),
    /// What the agent answered, in Markdown.
    Agent(String),
    Thought(String),
    Tool(ToolCall),
    Plan(Vec<PlanStep>),
    /// What the panel says about a turn, such as that it was stopped.
    Notice(Notice),
}

/// A message sent. The panel keeps its text, with a token for each cite.
pub(crate) struct UserMessage {
    /// The cites, by the ID of their token.
    pub(crate) cites: Vec<(String, Cite)>,
}

pub(crate) struct ToolCall {
    pub(crate) id: ToolCallId,
    pub(crate) title: String,
    pub(crate) kind: ToolKind,
    pub(crate) status: ToolCallStatus,
    /// What the tool reported, as text.
    pub(crate) output: String,
}

pub(crate) struct PlanStep {
    pub(crate) text: String,
    pub(crate) status: PlanEntryStatus,
}

pub(crate) struct Notice {
    pub(crate) text: String,
    pub(crate) error: bool,
}

#[derive(Default)]
pub(crate) struct Thread {
    pub(crate) entries: Vec<Entry>,
}

impl Thread {
    pub(crate) fn push(&mut self, entry: Entry) -> usize {
        self.entries.push(entry);
        self.entries.len() - 1
    }

    /// Apply an update from the agent. Returns the entry it changed or
    /// added, if any.
    pub(crate) fn apply(&mut self, update: SessionUpdate) -> Option<usize> {
        match update {
            SessionUpdate::AgentMessageChunk(chunk) => {
                let text = block_text(&chunk.content)?;
                match self.entries.last_mut() {
                    Some(Entry::Agent(answer)) => {
                        answer.push_str(&text);
                        Some(self.entries.len() - 1)
                    }
                    _ => Some(self.push(Entry::Agent(text))),
                }
            }
            SessionUpdate::AgentThoughtChunk(chunk) => {
                let text = block_text(&chunk.content)?;
                match self.entries.last_mut() {
                    Some(Entry::Thought(thought)) => {
                        thought.push_str(&text);
                        Some(self.entries.len() - 1)
                    }
                    _ => Some(self.push(Entry::Thought(text))),
                }
            }
            SessionUpdate::ToolCall(call) => {
                let update = ToolCallUpdate::from(call);
                Some(self.update_tool(&update))
            }
            SessionUpdate::ToolCallUpdate(update) => Some(self.update_tool(&update)),
            SessionUpdate::Plan(plan) => {
                let steps = plan
                    .entries
                    .into_iter()
                    .map(|entry| PlanStep {
                        text: entry.content,
                        status: entry.status,
                    })
                    .collect();

                // A turn has one plan, which the agent sends again as it
                // changes.
                let turn_start = self
                    .entries
                    .iter()
                    .rposition(|entry| matches!(entry, Entry::User(_)))
                    .map_or(0, |index| index + 1);
                match self.entries[turn_start..]
                    .iter()
                    .rposition(|entry| matches!(entry, Entry::Plan(_)))
                {
                    Some(offset) => {
                        self.entries[turn_start + offset] = Entry::Plan(steps);
                        Some(turn_start + offset)
                    }
                    None => Some(self.push(Entry::Plan(steps))),
                }
            }
            // The panel shows the messages it sent itself, and the rest
            // are about the session rather than the conversation.
            _ => None,
        }
    }

    /// Add a tool call or take its changes. Returns its entry.
    pub(crate) fn update_tool(&mut self, update: &ToolCallUpdate) -> usize {
        let index = self
            .entries
            .iter()
            .position(|entry| matches!(entry, Entry::Tool(tool) if tool.id == update.tool_call_id))
            .unwrap_or_else(|| {
                self.push(Entry::Tool(ToolCall {
                    id: update.tool_call_id.clone(),
                    title: String::new(),
                    kind: ToolKind::Other,
                    status: ToolCallStatus::Pending,
                    output: String::new(),
                }))
            });

        let Entry::Tool(tool) = &mut self.entries[index] else {
            unreachable!("the entry is the tool call");
        };
        let fields = &update.fields;
        if let Some(title) = &fields.title {
            tool.title = title.clone();
        }
        if let Some(kind) = fields.kind {
            tool.kind = kind;
        }
        if let Some(status) = fields.status {
            tool.status = status;
        }
        if let Some(content) = &fields.content {
            tool.output = content
                .iter()
                .filter_map(tool_content_text)
                .collect::<Vec<_>>()
                .join("\n");
        }

        index
    }

    /// Tool calls still running when a turn ends will not finish.
    pub(crate) fn settle_tools(&mut self) -> Vec<usize> {
        self.entries
            .iter_mut()
            .enumerate()
            .filter_map(|(index, entry)| match entry {
                Entry::Tool(tool)
                    if matches!(
                        tool.status,
                        ToolCallStatus::Pending | ToolCallStatus::InProgress
                    ) =>
                {
                    tool.status = ToolCallStatus::Failed;
                    Some(index)
                }
                _ => None,
            })
            .collect()
    }

    pub(crate) fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}

/// Output that an agent sends as a Markdown code block, as Claude Code does
/// with what commands print, without its fences.
pub(crate) fn unfenced(text: &str) -> &str {
    let trimmed = text.trim();
    let Some(rest) = trimmed.strip_prefix("```") else {
        return text;
    };
    let Some((_, body)) = rest.split_once('\n') else {
        return text;
    };

    body.strip_suffix("```")
        .map_or(text, |body| body.trim_end_matches('\n'))
}

fn block_text(block: &ContentBlock) -> Option<String> {
    match block {
        ContentBlock::Text(text) => Some(text.text.clone()),
        ContentBlock::ResourceLink(link) => Some(format!("[{}]({})", link.name, link.uri)),
        _ => None,
    }
}

fn tool_content_text(content: &ToolCallContent) -> Option<String> {
    match content {
        ToolCallContent::Content(content) => {
            block_text(&content.content).map(|text| unfenced(&text).to_owned())
        }
        ToolCallContent::Diff(diff) => Some(format!("Edited {}", diff.path.display())),
        _ => None,
    }
}
