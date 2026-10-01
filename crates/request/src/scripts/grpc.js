(function (input, pm, tools) {
    "use strict";
    const {entries, expect, skip} = tools;
    const stringify = JSON.stringify;
    const display = value => {
        try { return (typeof value === "string" ? value : stringify(value)).slice(0, 200); }
        catch { return String(value); }
    };
    const passes = assertion => {
        try { assertion(); return true; }
        catch { return false; }
    };

    // Objects match when each expected key matches, at any depth, so a
    // filter can name only the fields it cares about.
    function matches(actual, expected) {
        if (expected === null || typeof expected !== "object") return Object.is(actual, expected);
        if (actual === null || typeof actual !== "object") return false;
        if (Array.isArray(expected)) {
            return Array.isArray(actual) && actual.length === expected.length
                && expected.every((value, index) => matches(actual[index], value));
        }
        return Object.keys(expected).every(key => Object.hasOwn(actual, key) && matches(actual[key], expected[key]));
    }

    // Messages arrive as {data, at}, with `at` in milliseconds since the epoch.
    function messageList(items, direction) {
        const list = items.map(item => ({data: item.data, timestamp: new Date(item.at)}));
        const everyMessage = check => {
            if (list.length === 0) throw new Error(`expected at least one ${direction} message`);
            list.forEach((item, index) => check(item.data, `message ${index + 1}`));
        };
        const includes = expected => list.some(item => matches(item.data, expected));
        const methods = {
            idx: index => list.at(index),
            count: () => list.length,
            all: () => list.slice(),
            each(callback) { list.forEach(callback); },
            filter(predicate) {
                return Array.prototype.filter.call(list, typeof predicate === "function" ? predicate : item => matches(item, predicate));
            },
            to: {
                include(expected) {
                    if (!includes(expected)) throw new Error(`expected a ${direction} message to include ${display(expected)}`);
                },
                not: {
                    include(expected) {
                        if (includes(expected)) throw new Error(`expected no ${direction} message to include ${display(expected)}`);
                    },
                },
                have: {
                    property(path, ...value) {
                        everyMessage((data, label) => value.length
                            ? expect(data, label).to.have.deep.nested.property(path, value[0])
                            : expect(data, label).to.have.nested.property(path));
                    },
                    jsonSchema(schema) {
                        everyMessage((data, label) => {
                            const validation = pm.schema.validate(data, schema);
                            if (!validation.valid) {
                                throw new Error(`${label} does not match the JSON Schema: ` + validation.errors.map(error => `${error.instancePath || "/"}: ${error.message}`).join("; "));
                            }
                        });
                    },
                },
            },
        };
        for (const [name, value] of Object.entries(methods)) Object.defineProperty(list, name, {value});
        return list;
    }

    let url = String(input.url);
    let message = String(input.message);
    pm.request = {
        get url() { return url; },
        set url(value) { url = String(value); },
        get methodPath() { return input.methodPath; },
        metadata: entries(input.metadata, true),
        get message() { return message; },
        set message(value) { message = typeof value === "string" ? value : stringify(value) ?? String(value); },
    };
    pm.execution = input.phase === "before_invoke" ? {
        skipRequest(reason = "Skipped by Before invoke script") { skip(reason); },
    } : {};

    if (input.phase === "on_message") {
        pm.message = {data: input.received.data, timestamp: new Date(input.received.at)};
    }

    if (input.phase === "after_response") {
        const response = input.response;
        const metadata = entries(response.metadata, true);
        const trailers = entries(response.trailers, true);
        const messages = messageList(response.messages, "received");
        const present = (list, kind) => (key, value) => {
            if (!list.has(key)) throw new Error(`expected response ${kind} to have ${key}`);
            if (value !== undefined) expect(list.get(key), `${kind} ${key}`).to.equal(String(value));
        };

        pm.request.messages = messageList(input.sent, "sent");
        pm.response = {
            code: response.code,
            status: response.status,
            statusMessage: response.statusMessage,
            responseTime: response.responseTime,
            metadata,
            trailers,
            messages,
        };
        // Not enumerable, so logging the response does not run its assertions.
        Object.defineProperty(pm.response, "to", {value: {
            have: {
                statusCode(code) { expect(response.code, "status code").to.equal(code); },
                status(codeOrName) {
                    const actual = typeof codeOrName === "number" ? response.code : response.status;
                    expect(actual, "status").to.equal(codeOrName);
                },
                metadata: present(metadata, "metadata"),
                trailer: present(trailers, "trailers"),
                message(expected) {
                    if (!messages.some(item => passes(() => expect(item.data).to.eql(expected)))) {
                        throw new Error(`expected a received message to equal ${display(expected)}`);
                    }
                },
            },
            be: Object.defineProperties({}, {
                ok: {get() { expect(response.code, `status ${response.status}`).to.equal(0); }},
                error: {get() { expect(response.code, "status").not.to.equal(0); }},
            }),
        }});
    }

    return () => ({url, metadata: input.metadata, message});
})
