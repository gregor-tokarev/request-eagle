# Agent

The agent panel brings the coding agent you already use into Request Eagle:
Claude Code, Codex or OpenCode. Point at what you mean in a request or its
response, ask about it, and let the agent read your code and run, fix or
create saved requests and flows. The agent keeps its own sign-in, models and
tools.

## Open the panel

Click **Agent** in the status bar, or press Ctrl+Shift+I (Command+Shift+I on
macOS). The panel runs the first agent it finds on your computer; pick
another from the menu at its top. Install one if none is found:

| Agent | Needs |
| --- | --- |
| Claude Code | `claude` and Node.js (`npx`) |
| Codex | `codex` and Node.js (`npx`) |
| OpenCode | `opencode` |

Sign in to the agent in a terminal first, the way you use it there. When an
agent asks to sign in, the panel says how.

## Cite what you mean

Select text in the URL, the request body, a script or the response, and press
Ctrl+I (Command+I on macOS), or right-click it and choose **Add to Agent**.
The selection appears as a cite in your message, where the caret is, named
after what it quotes, such as **Response body · L3–4**. Without a selection,
the field with focus is cited whole; the button with sparkles in the response
toolbar cites the response. Click a cite to go back to its tab.

The agent reads each cite with where it comes from: the request, the
response status, the lines, and the file the request is saved in. Variables
such as `{{token}}` are cited as written, not filled in. A tab's changes that
are not saved yet are in its cites, and the agent is told they are not in the
file.

## What the agent can do

The agent works in the Git repository of the collection in the active tab,
so it can read the code that serves your API, or else in Request Eagle's own
folder. It sees your saved collections and flows either way. With the
[Request Eagle CLI](cli.md) installed, it lists, edits and runs saved requests
and flows through it; otherwise it edits their files.

Before it runs a command or changes a file, the agent asks in the panel, as
it does in a terminal. The menus under the message choose its mode, such as
asking first or accepting edits, its model, and how hard it thinks.

After each of the agent's turns, Request Eagle reads collections and flows
again. Requests and flows the agent added show in the sidebar, and open tabs
without unsaved changes show the agent's edits. Tabs with unsaved changes keep
them, and saving asks whose changes to keep.

**New Thread** at the top of the panel starts over with a fresh agent.

## Things to know

- Request Eagle talks to agents through the
  [Agent Client Protocol](https://agentclientprotocol.com). Claude Code and
  Codex speak it through adapters that `npx` downloads the first time, so the
  first thread takes longer to start.
- Request Eagle finds agents on the `PATH` of your login shell, as a terminal
  would, also when you open the app from the Dock or a launcher.
- What you cite and ask goes to the agent's model provider, like anything you
  send the agent in a terminal.
- A very long cite, such as a large response, is cut in the message; the
  agent gets a file with all of it.
