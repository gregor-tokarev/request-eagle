# Flows

A flow connects blocks on a canvas, as in Postman Flows. Data leaves a
block's outputs on the right and travels along connections into the inputs
of other blocks on the left. Flows chain saved requests, reshape their
responses with FQL, branch, loop and collect results, without writing a
script.

Right-click a collection or folder in the sidebar and choose **New Flow**. A
flow is saved in its collection next to the requests it sends, and opens in
its own tab.

## The canvas

| Action | How |
| --- | --- |
| Add a block | Right-click the canvas, choose **Block** in the toolbar, or press `A`. Type to search blocks and saved requests; Enter adds the highlighted one. |
| Connect | Drag from an output to an input. Dropping on a block joins its first free input; dropping on empty canvas opens the picker and connects the new block. |
| Move a connection | Drag it off the input it ends at. |
| Select | Click a block or a connection. Shift-click or Ctrl/Cmd-click adds blocks; Shift-drag selects an area. |
| Move | Drag a block. Every selected block moves with it. Arrow keys nudge the selection; with Shift, further. |
| Pan | Drag empty canvas, drag with the middle button, or scroll. |
| Zoom | Ctrl/Cmd-scroll, the toolbar, `Ctrl/Cmd =` and `Ctrl/Cmd -`. `Ctrl/Cmd 0` shows the whole flow. |
| Delete | Backspace or Delete removes the selection. |
| Copy, paste, duplicate | `Ctrl/Cmd C`, `Ctrl/Cmd V`, `Ctrl/Cmd D`. Copies keep the connections between the copied blocks. |
| Undo, redo | `Ctrl/Cmd Z`, `Ctrl/Cmd Shift Z`. |
| Arrange | **Arrange** places blocks in columns so connections run left to right. |

Selecting one block opens its settings on the right, with what it received
and sent in the last run. Shortcuts can be changed in the keyboard settings.

## Running

Click **Run** or press `Ctrl/Cmd Enter`. The flow runs as it is in the
editor, saved or not, with the environment selected in the tab bar. Blocks
show a spinner while they work, a check and their run count when they ran,
and their error when they failed. Connections that carried data are drawn
brighter, Display blocks show what they received last, and the run log lists
every block's run with what it sent.

A run sends requests like their tabs do: their collection's variables, the
active environment, session values set by scripts, the cookie jar and the
collection's and request's scripts all apply. A run stops after 100,000 block
runs, which ends a loop that never does.

## How data moves

- A block runs when every **connected** input has received a value, and runs
  again whenever one of them receives another, with the latest value of the
  others.
- When a run starts, Start blocks send their input. Blocks that need no input
  to work, such as String, Evaluate or HTTP Request, run then as well when
  none of their inputs are connected.
- An HTTP Request block with a connected **Send** input waits for it, which
  orders requests that share no data: connect Login's Success to the next
  request's Send.
- **For** sends each item of a list and **Repeat** each index up to its count,
  one at a time. Each item travels as far as it can before the next starts. A
  **Collect** block after the loop sends the results, in order, once the loop
  ends; an empty loop collects an empty list. Adding a loop adds its Collect.
- A block only combines values of the same loop iteration. A value from
  outside the loop, such as a token, combines with every iteration.
- A connection may lead back to an earlier block, for example to fetch pages
  until there is no next page. Values of one pass through such a cycle are
  combined only with each other.
- When a block fails, the run log shows why, and the rest of the flow carries
  on.

## Blocks

| Block | Inputs → outputs | What it does |
| --- | --- | --- |
| Start | → Data | Sends its JSON input when the run starts. The CLI can send other input. |
| HTTP Request | Send, one per `{{variable}}` → Success, Fail | Sends a saved HTTP request. A connected value fills its variable. Success sends 2xx responses; Fail other statuses and errors. |
| Evaluate | variables → Result | Computes an FQL expression. An undefined result sends nothing. |
| If | variables, Data → Then, Else | Sends Data, or the variables when Data is not connected, out of Then when the FQL condition holds. |
| Condition | variables → Condition 1…n, Default | Sends the variables out of the first condition that holds. |
| Validate | Data → Pass, Fail | Checks data against a JSON Schema. Fail sends `{data, errors}`. |
| Delay | Data → Data | Waits, then sends the data on. |
| OR | First, Second → Data | Sends whatever arrives at either input. |
| Repeat | Count, Start → Index | Loops a number of times. |
| For | List, Start → Item | Loops over a list. |
| Collect | Item → List, Finish | Gathers a loop's results. |
| Display | Data → Data | Shows the latest data as text, JSON or a table. |
| Log | Data | Writes each value to the run log. |
| String, Number, Bool, Null | → Value | A value. |
| Now, Date | → Value | Milliseconds since the Unix epoch: now, or of an ISO 8601 date. |
| Select | Data → Value | Picks a value by a dotted path such as `body.items.0.id`. |
| Record | one per field → Record | Builds an object. A field that is not connected holds its own value, read as JSON when it is JSON. |
| List | Item 1…n → List | Builds a list the same way. |
| Template | variables → Result | Fills `{{name}}` and Mustache sections such as `{{#items}}…{{/items}}`. Sends text, or JSON. |
| Create Variable | Value | Stores a value under a name and sends it from every Get Variable block of that name. |
| Get Variable | → Value | |
| Output | one per name | What a run returns, for example to the CLI. |
| Note | | Text on the canvas. |

An HTTP Request block sends:

```json
{
  "body": {"parsed": "JSON, or the text"},
  "http": {"status": 200, "headers": {"content-type": "application/json"}, "time": 41.2},
  "tests": [{"name": "Status is 200", "passed": true, "error": null}],
  "binary": false
}
```

Header names are lowercase. A body that is not UTF-8 is Base64 with
`"binary": true`. A request that could not be sent goes out of Fail as
`{"error": "…"}`.

## FQL

Evaluate, If and Condition blocks use FQL, the query language of Postman
Flows, which is [JSONata](https://docs.jsonata.org). Their variables are
fields of the input, so an expression reads them by name:

```
value1.http.status = 200
response.body.users[active].{ "id": id, "name": $uppercase(name) }
$sum(order.items.(price * quantity))
```

Inside a predicate a bare name reads the item, so compare with a variable
through `$$` or bind it first: `users[name = $$.search]`.

FQL adds Postman's functions to JSONata's, such as `$jsonParse`, `$json`,
`$uuid`, `$partition` and date arithmetic like `$datePlus` and `$diffDate`.
Lambdas can be written `fn($x) { … }` as well as `function($x) { … }`.
Selecting a block with an expression shows its result for the inputs of the
last run as you type.

## From the CLI

`request-eagle-cli` creates, edits and runs flows without the app:

```sh
request-eagle-cli call '{"command":"flows.blocks"}'
request-eagle-cli call '{"command":"flows.list"}'
request-eagle-cli call '{"command":"flows.run","path":"/absolute/path/Checkout.toml","input":{"user":"ada"}}'
request-eagle-cli call '{"command":"fql.evaluate","expression":"$sum(items.price)","input":{"items":[{"price":2}]}}'
```

See the [CLI guide](cli.md) for the rest.

## The file

A flow is a TOML file in its collection, with a `flow` table where a request
has a `request` table:

```toml
id = "6c1d…"
name = "Checkout"
schema_version = 1

[[flow.blocks]]
id = "b1"
type = "start"
x = 0.0
y = 0.0

[[flow.blocks]]
id = "b2"
type = "http_request"
x = 320.0
y = 0.0
request = "id of the saved request"

[[flow.connections]]
from = "b1"
output = "data"
to = "b2"
input = "send"
```

HTTP Request blocks refer to requests by ID, so moving or renaming a request
keeps its blocks working.
