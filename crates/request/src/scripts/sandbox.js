(function (source, log, test, expect, dynamic) {
    "use strict";
    const input = JSON.parse(source);
    const stringify = JSON.stringify;
    const variables = Object.assign(Object.create(null), input.variables);
    const format = value => typeof value === "string" ? value : (stringify(value) ?? String(value));
    const replaceIn = text => String(text).replace(/\{\{([^{}]+)\}\}/g, (match, key) => variables[key] ?? dynamic(key) ?? match);

    function headers(pairs) {
        return {
            get(name) { return pairs.find(([key]) => key.toLowerCase() === String(name).toLowerCase())?.[1]; },
            has(name) { return this.get(name) !== undefined; },
            add({key, value}) { pairs.push([String(key), String(value)]); },
            remove(name) {
                for (let i = pairs.length - 1; i >= 0; i--) {
                    if (pairs[i][0].toLowerCase() === String(name).toLowerCase()) pairs.splice(i, 1);
                }
            },
            upsert(header) { this.remove(header.key); this.add(header); },
            toJSON() { return pairs.map(([key, value]) => ({key, value})); },
        };
    }

    const request = {
        method: input.method,
        url: input.url,
        headers: headers(input.headers),
        body: {mode: "raw", raw: input.body, update(value) { this.raw = String(value); }},
    };
    const pm = {
        request,
        variables: {
            get: key => variables[key],
            has: key => Object.hasOwn(variables, key),
            set(key, value) { variables[String(key)] = String(value); },
            unset(key) { delete variables[key]; },
            clear() { for (const key of Object.keys(variables)) delete variables[key]; },
            toObject: () => ({...variables}),
            replaceIn,
        },
        expect,
        test(name, callback) {
            try {
                if (callback.length || callback.constructor.name === "AsyncFunction") throw new Error("Tests must use a synchronous callback");
                const result = callback();
                if (result && typeof result.then === "function") throw new Error("Tests must use a synchronous callback");
                test(String(name), null);
            } catch (error) {
                test(String(name), String(error));
            }
        },
    };
    if (input.response) {
        const response = input.response;
        pm.response = {
            code: response.code,
            status: response.status,
            responseTime: response.responseTime,
            headers: headers(response.headers),
            text: () => response.body,
            json: () => JSON.parse(response.body),
            to: {have: {
                status(codeOrReason) {
                    const actual = typeof codeOrReason === "number" ? response.code : response.status;
                    expect(actual).to.equal(codeOrReason);
                },
                body(content) {
                    if (arguments.length === 0) expect(response.body).not.to.be.empty;
                    else if (content instanceof RegExp) expect(response.body).to.match(content);
                    else if (content !== null && typeof content === "object" && !Array.isArray(content)) expect(pm.response.json()).to.deep.equal(content);
                    else expect(response.body).to.equal(content);
                },
                jsonBody(path, value) {
                    const data = pm.response.json();
                    if (arguments.length === 1) expect(data).to.have.nested.property(path);
                    else if (arguments.length > 1) expect(data).to.have.deep.nested.property(path, value);
                },
                header(name, value) {
                    expect(pm.response.headers.has(name)).to.be.true;
                    if (value !== undefined) expect(pm.response.headers.get(name)).to.equal(value);
                },
            }},
        };

        pm.response.to.be = new Proxy({}, {
            get(_, name) {
                switch (name) {
                    case "then": return undefined;
                    case "ok": expect(response.code).to.equal(200); break;
                    case "success": expect(response.code).to.be.within(200, 299); break;
                    case "error": expect(response.code).to.be.within(400, 599); break;
                    case "clientError": expect(response.code).to.be.within(400, 499); break;
                    case "serverError": expect(response.code).to.be.within(500, 599); break;
                    case "json": pm.response.json(); break;
                    default: throw new Error(`Unsupported response assertion: ${String(name)}`);
                }
            },
        });
    }
    globalThis.pm = pm;
    globalThis.console = Object.fromEntries(["log", "info", "warn", "error", "debug"].map(level => [level, (...values) => log(level, values.map(format).join(" "))]));

    return () => stringify({
        method: request.method,
        url: String(request.url),
        headers: input.headers,
        body: request.body.raw,
        body_changed: request.body.raw !== input.body,
        variables,
    });
})
