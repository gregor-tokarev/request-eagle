# Collection Runner

The Collection Runner sends a collection's requests one after another and
reports what their tests found. Use it to check a whole API, replay a flow
such as sign in, create, read and delete, or run the same requests for every
row of a data file.

## Start a run

Right-click a collection or folder in the sidebar and choose **Run
collection** or **Run folder**, or click **Run** on a collection's page. The
runner opens in a tab with:

- **Run Sequence**: the HTTP requests in the collection's order. Clear a
  request's checkbox to leave it out, or drag a request to send it at another
  point. **Deselect All**, **Select All** and **Reset** change every request
  at once; Reset also restores the collection's order. gRPC and WebSocket
  requests are left out.
- **Iterations**: how many times the sequence runs.
- **Delay**: how long to wait between requests, in milliseconds.
- **Test data file**: a CSV or JSON file whose rows give each iteration its
  values.
- **Advanced settings**, described below.

Click **Run** with the collection's name to start. Each run sends the
requests as they are saved, so save a request's changes before running it.
Variables come from the collection and the environment selected in the tab
bar. Values a script sets with `pm.variables` last for the whole run.

## Read the results

The results show how many tests passed and failed, how long the run took and
the average response time. Requests are listed by iteration with their status
and tests. **Passed**, **Failed**, **Skipped** and **Errors** narrow the list;
**Console log** shows what scripts logged.

Click a request to see its response beside the list: its body, cookies and
headers, the request as it was sent, its test results and its console. Use
the numbers on the left to see the same request in other iterations.

While a run is going, **Pause** waits after the current request and
**Stop** ends the run. **Run Again** repeats the run; **New Run** returns to
the configuration.

## Use a data file

A CSV file's first row names its columns. A JSON file is an array of objects.
Each row is one iteration, so choosing a file sets **Iterations** to its row
count; extra iterations reuse the last row.

```csv
username,item
ada,book
grace,laptop
```

Requests use a row's values as `{{username}}`, ahead of the environment's,
the collection's and global values, and scripts read them with
`pm.iterationData`:

```js
pm.test("Signed in as the row's user", () => {
    pm.expect(pm.response.json().user).to.eql(pm.iterationData.get("username"));
});
```

## Choose the next request

A script can change where the run goes next with
`pm.execution.setNextRequest`, as in Postman. Give it a request's name or ID;
the run continues from there after the current request. `null` ends the
iteration.

```js
// Fetch every page before going on.
const body = pm.response.json();
if (body.next) {
    pm.collectionVariables.set("page", body.next);
    pm.execution.setNextRequest("List users");
}
```

The last choice of a request's scripts wins. A folder's run can only go to
requests in that folder. Sending one request on its own ignores the choice.

## Advanced settings

| Setting | Effect |
| --- | --- |
| Persist responses for a session | Keep each response's headers and body to view after the run. |
| Turn off logs during run | Leave out the scripts' console output. |
| Stop run if an error occurs | End the run when a request cannot be sent or a script fails. Failed tests do not stop it. |
| Keep variable values | Keep the variables scripts change for the rest of the session. Otherwise the run changes a copy. |
| Run collection without using stored cookies | Start with an empty cookie jar. |
| Save cookies after collection run | Keep the cookies the run's responses set in the cookie jar. |
