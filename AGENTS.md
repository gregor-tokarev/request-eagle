You are working in project called Request Eagle. It's postman alternative written in Rust's gpui-kit framework

You've might work in worktree with other agents in parallel, so if you want to test something with computer-use open separate application with some label like: Request Eagle(sidebar facelift) or Request Eagle(Codebase rewrite)

# Code quality

Keep complexity justified. Start with the simplest model that meets the
current requirements. Introduce a concept or abstraction when it solves a
concrete problem in the code. Avoid speculative flexibility.

Prefer straightforward code. A little duplication is preferable to an
abstraction that adds indirection without making the code easier to understand.
Repetition alone is not sufficient reason to extract a helper.

Organize code around cohesive responsibilities and ownership. Reconsider large
files, but split them where there is a meaningful boundary. Keep related logic
together and make control flow easy to follow.

Leave spaces between code lines, logically group lines

For modules split across multiple files, use entry files such as `appearance.rs`,
`lib.rs`, and `mod.rs` only for module declarations and re-exports. Put their
implementations and tests in the corresponding module directories. Keep
single-file modules directly alongside their sibling modules; do not create a
folder and an export-only entry file for just one implementation file.

If a module only wraps one child module, remove that extra layer and place the
child's files directly in the parent module. This also applies to crate roots.
Judge this by responsibility, not just the number of child modules: a wrapper
that repeats its parent's role and exports is redundant even when the parent
also contains supporting types. Keep the core implementation and its tests at
the parent level; reserve nested modules for distinct responsibilities.

# Language

**Tab**: An open page in the workspace. Tabs can show a collection or a request; a tab is not necessarily an HTTP request.

**Request draft**: The editable, unsaved state of a request in one tab. Changing a draft does not change the saved request until it is saved.

**Response**: The result received from executing a request. Its presentation depends on the request protocol.
