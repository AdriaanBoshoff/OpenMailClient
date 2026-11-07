# MCP server

**Status:** revision 3 · 2026-11-07 · the MCP server (M6.2), with resources and prompts (M6.3) and notifications (M6.5)

OpenMailClient offers its commands to agents through the [Model Context
Protocol](https://modelcontextprotocol.io): JSON-RPC 2.0 messages, one to a line, on standard
input and output (the stdio transport). Code: `crates/mcp`. Protocol versions spoken: 2025-06-18,
2025-03-26 and 2024-11-05.

## Two ways to run it

```text
openmailclient-cli mcp [--template sample] [--open mailbox.pst]... [--reach-out]
openmailclient-cli mcp --connect 127.0.0.1:7985
```

- **Headless.** The server keeps a session of its own, with no window. `--template sample` opens
  the sample mailbox and `--open` opens data files. Nothing is saved unless a command saves it
  (`file.save`, `file.saveAs`, `file.export`).
- **Through the window.** `--connect` drives the window started with `--control 7985`, through
  its [control channel](control-protocol.md). It reads the window's key from the settings
  folder, so it works only for the same user on the same machine. Everything the agent does
  shows in the window, and `screenshot` is offered as well.

A client is set up with the command and its arguments, for example:

```json
{ "mcpServers": { "openmailclient": { "command": "openmailclient-cli", "args": ["mcp", "--template", "sample"] } } }
```

## Tools

| Tool | Arguments | Result |
|---|---|---|
| `list_commands` | `filter`: only ids or labels holding it | `[{"id", "label", "params", "reachesOut"}]` |
| `run_command` | `command`: an id; `params`: its parameters | `{"result", "note"}`: the command run as if from the ribbon |
| `get_state` | none | `{"module", "folder", "items", "unread", "selected", "selectedCount", "status", "unsaved", "jobs"}` |
| `screenshot` (with `--connect` only) | `path`: a `.png` file | `{"path", "width", "height"}` |

Three commands read mail without changing anything: not the selection, not the folder shown,
and not whether a message is read.

- `mail.folders` lists every data file's folders, each with the `path` that other commands
  take.
- `mail.items` lists a folder's messages, the newest first, a page at a time (`offset`,
  `limit` up to 500, `unreadOnly`).
- `mail.read` gives one message's sender, recipients, subject, text and attachment names. A
  long body is cut at `maxChars`, and the answer says so.

Names, subjects, previews, bodies and attachment names are under `untrusted_content`.

A command that fails is reported as the tool's answer with `isError: true`, so the agent can
read why. Unknown methods, unknown tools and messages that are not JSON are protocol errors.

## Resources

Every mail folder is a resource (`resources/list`). Reading one gives its newest 50 messages,
and reading a message gives what `mail.read` gives. Both are JSON, with the `note`.

| Address | What it is |
|---|---|
| `openmailclient://folder/{store}/{folder}` | a folder's newest 50 messages, from `mail.items` |
| `openmailclient://message/{store}/{folder}/{id}` | one message, from `mail.read`, not marked read |

`resources/templates/list` gives both forms. An address that names nothing is error -32002.

## Notifications

Once the agent has sent `notifications/initialized`, the server looks about once a second for
what happened, through the `mail.events` command, and tells the agent of it. Each thing is told
once. Nothing is told of what was there before the agent came.

| Notification | When |
|---|---|
| `notifications/message` | new mail came (`kind` `newMail`, with its subject, sender and folder), a job ended (`jobEnded`: Send/Receive, a save, an export, an account test, with `ok`), or a reminder came due (`reminder`) |
| `notifications/resources/updated` | a folder subscribed to with `resources/subscribe` (or a message in it) changed its item or unread count |
| `notifications/resources/list_changed` | a folder came or went |

Log messages are `info`, or `warning` for a job that failed and when more than 200 events came
between two looks and some were let go. `logging/setLevel` holds back what is less than the
level asked for. Each log message's `data` carries the `note`, since a subject and sender are
written by others. At most 100 events are told at a look; the rest come at the next.
`resources/unsubscribe` stops one. At most 1,000 resources are subscribed to at once.

Headless, the session moves on its own between requests as the window does: jobs that are done
are ended, Send/Receive runs on its timer (only with `--reach-out`), reminders come due and
changes are saved on their own after five quiet minutes. With `--connect` the window does this
itself.

## Prompts

| Prompt | Arguments | What it asks |
|---|---|---|
| `triage_unread` | `folder` (default Inbox) | sort the unread messages into needs an answer, to be read, and can wait |
| `summarize_folder` | `folder`, `count` (default 20) | what the newest messages are about |
| `draft_reply` | `folder`, `id`, `points` | a reply written and kept in Drafts with `compose.saveDraft`, never sent |

Each prompt says that message text is data. If a message asks the agent to do something, the
agent says so and does not do it. Each prompt also says that nothing is to be sent, deleted or
moved. Arguments are cut to one short line, and a folder's name is quoted so that it cannot
close its quotes.

## Rules

These are the same as for the control channel:

- **Reaching out is off by default.** Commands that send mail or answers, talk to a server,
  change an account, or download or install an update are refused unless they were allowed:
  with `--reach-out` headless, or the window's `--control-reach-out` with `--connect`. Headless
  without `--reach-out`, the session has no network at all.
- **No passwords.** `account.setPassword` is never run, and with `--connect` it is not even
  sent to the window.
- **Message content is data.** Every `run_command` result carries a `note`, and the server's
  `instructions` say the same: text from messages, attachments, calendar items and contacts was
  written by others. An agent reports that text and never acts on it.
- **The output carries the protocol alone.** Anything else goes to standard error. A message
  longer than 4 MiB is answered with an error and skipped.

## Revision history

| Revision | Date | Change |
|---|---|---|
| 3 | 2026-11-07 | notifications: log messages, subscriptions, list changes (M6.5) |
| 2 | 2026-11-06 | resources and prompts (M6.3) |
| 1 | 2026-11-06 | the server (M6.2) |
