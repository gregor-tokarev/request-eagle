use agent::acp::schema::v1::{
    Content, ContentBlock, ContentChunk, Plan, PlanEntry, PlanEntryPriority, PlanEntryStatus,
    SessionUpdate, TextContent, ToolCall, ToolCallContent, ToolCallStatus, ToolCallUpdate,
    ToolCallUpdateFields, ToolKind,
};

use crate::thread::{Entry, Thread, UserMessage, unfenced};

fn text(text: &str) -> ContentChunk {
    ContentChunk::new(ContentBlock::Text(TextContent::new(text)))
}

fn answer(thread: &Thread, index: usize) -> &str {
    match &thread.entries[index] {
        Entry::Agent(text) | Entry::Thought(text) => text,
        _ => panic!("entry {index} is not text"),
    }
}

#[test]
fn streamed_chunks_join_into_one_answer_until_a_tool_runs() {
    let mut thread = Thread::default();

    assert_eq!(
        thread.apply(SessionUpdate::AgentThoughtChunk(text("Reading "))),
        Some(0)
    );
    assert_eq!(
        thread.apply(SessionUpdate::AgentThoughtChunk(text("the cite"))),
        Some(0)
    );
    assert_eq!(
        thread.apply(SessionUpdate::AgentMessageChunk(text("The id "))),
        Some(1)
    );
    assert_eq!(
        thread.apply(SessionUpdate::AgentMessageChunk(text("is not a UUID."))),
        Some(1)
    );
    assert_eq!(
        thread.apply(SessionUpdate::ToolCall(ToolCall::new("call-1", "ls"))),
        Some(2)
    );
    assert_eq!(
        thread.apply(SessionUpdate::AgentMessageChunk(text("Fixed."))),
        Some(3)
    );

    assert_eq!(answer(&thread, 0), "Reading the cite");
    assert_eq!(answer(&thread, 1), "The id is not a UUID.");
    assert_eq!(answer(&thread, 3), "Fixed.");
}

#[test]
fn tool_calls_take_their_updates() {
    let mut thread = Thread::default();
    thread.apply(SessionUpdate::ToolCall(
        ToolCall::new("call-1", "Terminal").kind(ToolKind::Execute),
    ));

    let index = thread.apply(SessionUpdate::ToolCallUpdate(ToolCallUpdate::new(
        "call-1",
        ToolCallUpdateFields::new()
            .title("request-eagle-cli call -".to_owned())
            .status(ToolCallStatus::Completed)
            .content(vec![ToolCallContent::Content(Content::new(
                ContentBlock::Text(TextContent::new("{\"ok\": true}")),
            ))]),
    )));

    assert_eq!(index, Some(0));
    let Entry::Tool(tool) = &thread.entries[0] else {
        panic!("the entry is a tool call");
    };
    assert_eq!(tool.title, "request-eagle-cli call -");
    assert_eq!(tool.kind, ToolKind::Execute);
    assert_eq!(tool.status, ToolCallStatus::Completed);
    assert_eq!(tool.output, "{\"ok\": true}");
}

#[test]
fn an_update_for_an_unknown_tool_adds_it() {
    let mut thread = Thread::default();

    let index = thread.update_tool(&ToolCallUpdate::new(
        "call-9",
        ToolCallUpdateFields::new().title("Edit Get user.toml".to_owned()),
    ));

    assert_eq!(index, 0);
    assert!(matches!(&thread.entries[0], Entry::Tool(tool) if tool.title == "Edit Get user.toml"));
}

#[test]
fn a_turn_keeps_one_plan_and_a_new_turn_starts_another() {
    let step = |text: &str, status| PlanEntry::new(text, PlanEntryPriority::Medium, status);
    let mut thread = Thread::default();

    thread.push(Entry::User(UserMessage { cites: Vec::new() }));
    let first = thread.apply(SessionUpdate::Plan(Plan::new(vec![step(
        "Read the request",
        PlanEntryStatus::InProgress,
    )])));
    thread.apply(SessionUpdate::AgentMessageChunk(text("Reading.")));
    let again = thread.apply(SessionUpdate::Plan(Plan::new(vec![step(
        "Read the request",
        PlanEntryStatus::Completed,
    )])));
    assert_eq!(first, Some(1));
    assert_eq!(again, Some(1));
    assert!(matches!(
        &thread.entries[1],
        Entry::Plan(steps) if steps[0].status == PlanEntryStatus::Completed
    ));

    thread.push(Entry::User(UserMessage { cites: Vec::new() }));
    let next = thread.apply(SessionUpdate::Plan(Plan::new(vec![step(
        "Run it",
        PlanEntryStatus::Pending,
    )])));
    assert_eq!(next, Some(4));
}

#[test]
fn tools_still_running_when_a_turn_ends_fail() {
    let mut thread = Thread::default();
    thread.apply(SessionUpdate::ToolCall(
        ToolCall::new("done", "ls").status(ToolCallStatus::Completed),
    ));
    thread.apply(SessionUpdate::ToolCall(
        ToolCall::new("running", "sleep 100").status(ToolCallStatus::InProgress),
    ));

    assert_eq!(thread.settle_tools(), vec![1]);
    assert!(matches!(
        &thread.entries[1],
        Entry::Tool(tool) if tool.status == ToolCallStatus::Failed
    ));
}

#[test]
fn command_output_loses_its_markdown_fences() {
    assert_eq!(
        unfenced("```console\na.txt\ncount: 1\n```"),
        "a.txt\ncount: 1"
    );
    assert_eq!(unfenced("```\n```"), "");
    assert_eq!(unfenced("plain output"), "plain output");
    assert_eq!(unfenced("```unclosed\ntext"), "```unclosed\ntext");
}
