(function (source, log, test, expect, dynamic, readBody, send, utilities) {
    "use strict";
    const input = JSON.parse(source);
    const stringify = JSON.stringify;
    const variables = Object.assign(Object.create(null), input.variables.values);
    const environment = Object.assign(Object.create(null), input.variables.environment);
    const environmentChanges = Object.create(null);
    let skipReason = null;
    const skipSignal = {};
    let pendingTests = 0;
    const generated = Object.assign(Object.create(null), input.variables.generated);
    const format = value => {
        try { return typeof value === "string" ? value : (stringify(value) ?? String(value)); }
        catch {
            try { return String(value).slice(0, 4096); }
            catch { return "[Unserializable value]"; }
        }
    };
    const substitute = (text, values, strict = false, isUrl = false) => {
        const resolve = (match, key) => {
            if (strict && key.startsWith("!")) return "{{" + key.slice(1) + "}}";
            key = key.trim();
            const value = values[key] ?? generated[key];
            if (value !== undefined) return value;
            const fresh = dynamic(key);
            if (fresh == null) {
                if (strict) throw new Error(`Unknown variable {{${key}}}`);
                return match;
            }
            generated[key] = fresh;
            return fresh;
        };
        if (!strict) return String(text).replace(/\{\{([^{}]+)\}\}/g, resolve);

        // Match the primary request parser: each opening pair consumes through
        // the next closing pair, including nested braces in the variable name.
        let remaining = isUrl ? String(text).split("#", 1)[0] : String(text);
        let result = "";
        while (true) {
            const start = remaining.indexOf("{{");
            if (start < 0) return result + remaining;
            result += remaining.slice(0, start);
            const end = remaining.indexOf("}}", start + 2);
            if (end < 0) throw new Error("Unclosed variable. Complete the reference with }} before sending.");
            const value = resolve(remaining.slice(start, end + 2), remaining.slice(start + 2, end));
            // URL fragments are not transmitted, including those introduced
            // by a variable value. Do not inspect references after a fragment.
            if (isUrl && value.includes("#")) return result + value.split("#", 1)[0];
            result += value;
            remaining = remaining.slice(end + 2);
        }
    };

    const visibleVariables = () => Object.assign(Object.create(null), environment, variables);
    const replaceIn = text => substitute(text, visibleVariables());

    function entries(pairs, ignoreCase = false) {
        const normalize = name => ignoreCase ? String(name).toLowerCase() : String(name);
        return {
            get(name) { return pairs.find(([key]) => normalize(key) === normalize(name))?.[1]; },
            has(name) { return this.get(name) !== undefined; },
            add({key, value}) { pairs.push([String(key), String(value)]); },
            remove(name) {
                for (let i = pairs.length - 1; i >= 0; i--) {
                    if (normalize(pairs[i][0]) === normalize(name)) pairs.splice(i, 1);
                }
            },
            upsert(header) { this.remove(header.key); this.add(header); },
            clear() { pairs.length = 0; },
            toJSON() { return pairs.map(([key, value]) => ({key, value})); },
        };
    }

    let url = input.url;
    const query = input.query;
    const extraQuery = entries(query);
    const decode = value => { try { return decodeURIComponent(value.replace(/\+/g, " ")); } catch { return value; } };
    function inlineQuery() {
        const end = url.includes("#") ? url.indexOf("#") : url.length;
        const start = url.indexOf("?");
        const prefix = start >= 0 && start < end ? url.slice(0, start) : url.slice(0, end);
        const raw = start >= 0 && start < end ? url.slice(start + 1, end) : "";
        const pairs = raw ? raw.split("&") : [];
        return {prefix, pairs, fragment: url.slice(end)};
    }
    function decodedPair(pair) {
        const separator = pair.indexOf("=");
        return separator < 0 ? [decode(pair), ""] : [decode(pair.slice(0, separator)), decode(pair.slice(separator + 1))];
    }
    const allQuery = {
        get(name) { return this.toJSON().find(item => item.key === String(name))?.value; },
        has(name) { return this.get(name) !== undefined; },
        add(item) { extraQuery.add(item); },
        remove(name) {
            const {prefix, pairs, fragment} = inlineQuery();
            const kept = pairs.filter(pair => decodedPair(pair)[0] !== String(name));
            if (kept.length !== pairs.length) url = prefix + (kept.length ? "?" + kept.join("&") : "") + fragment;
            extraQuery.remove(name);
        },
        upsert(item) { this.remove(item.key); this.add(item); },
        clear() { const {prefix, fragment} = inlineQuery(); url = prefix + fragment; extraQuery.clear(); },
        toJSON() { return inlineQuery().pairs.map(pair => { const [key, value] = decodedPair(pair); return {key, value}; }).concat(extraQuery.toJSON()); },
    };
    // Preserve the encoding of untouched URL pairs. Encode Params rows only
    // when displaying the URL; the executor resolves and encodes their values.
    const encode = text => String(text).split(/(\{\{[^{}]+\}\})/g).map(part =>
        part.startsWith("{{") ? part : encodeURIComponent(part).replace(/[!'()~]/g, ch => `%${ch.charCodeAt(0).toString(16).toUpperCase()}`).replace(/%20/g, "+")
    ).join("");
    const requestUrl = {
        query: allQuery,
        update(value) { url = String(value); query.length = 0; },
        toString() {
            const base = url.split("#", 1)[0];
            const suffix = query.map(([key, value]) => `${encode(key)}=${encode(value)}`).join("&");
            const start = base.indexOf("?");
            return suffix ? base + (start < 0 ? "?" : start === base.length - 1 ? "" : "&") + suffix : base;
        },
        toJSON() { return this.toString(); },
    };

    let body, originalBody;
    let bodyLoaded = false, bodyChanged = false;
    const request = {
        method: input.method,
        get url() { return requestUrl; },
        set url(value) { requestUrl.update(value); },
        headers: entries(input.headers, true),
        body: {
            mode: "raw",
            get raw() {
                if (!bodyLoaded && !bodyChanged) {
                    body = originalBody = readBody(false);
                    bodyLoaded = true;
                }
                return body;
            },
            set raw(value) { body = value; bodyChanged = !bodyLoaded || value !== originalBody; },
            update(value) { this.raw = String(value); },
        },
    };
    const pm = {
        request,
        variables: {
            get: key => variables[key] ?? environment[key],
            has: key => Object.hasOwn(variables, key) || Object.hasOwn(environment, key),
            set(key, value) { variables[String(key)] = String(value); },
            unset(key) { delete variables[key]; },
            clear() { for (const key of Object.keys(variables)) delete variables[key]; },
            toObject: visibleVariables,
            replaceIn,
        },
        environment: {
            get: key => environment[key],
            has: key => Object.hasOwn(environment, key),
            set(key, value) {
                key = String(key);
                if (!key || key.startsWith("$")) throw new Error("Environment variable names must be nonempty and cannot start with $");
                environment[key] = environmentChanges[key] = String(value);
            },
            unset(key) { key = String(key); delete environment[key]; environmentChanges[key] = null; },
            clear() { for (const key of Object.keys(environment)) this.unset(key); },
            toObject: () => ({...environment}),
            replaceIn: text => substitute(text, environment),
        },
        crypto: {
            sha256: utilities.sha256,
            hmacSha256: utilities.hmacSha256,
            randomBytes: utilities.randomBytes,
        },
        encoding: {
            base64Encode: utilities.base64Encode,
            base64Decode: utilities.base64Decode,
            base64UrlEncode: utilities.base64UrlEncode,
            base64UrlDecode: utilities.base64UrlDecode,
        },
        schema: {
            validate(data, schema) {
                return JSON.parse(utilities.validateSchema(stringify(data), stringify(schema)));
            },
        },
        execution: input.response ? {} : {
            skipRequest(reason = "Skipped by pre-request script") {
                skipReason = String(reason).slice(0, 4096);
                throw skipSignal;
            },
        },
        async sendRequest(config, callback) {
            if (skipReason !== null) throw skipSignal;
            let response;
            try {
                if (typeof config === "string") config = {url: config};
                if (!config || typeof config.url !== "string") throw new TypeError("sendRequest requires a URL string or an object with a url string");
                if (callback !== undefined && typeof callback !== "function") throw new TypeError("sendRequest callback must be a function");
                const method = String(config.method ?? "GET").toUpperCase();
                const resolve = text => substitute(text, visibleVariables(), true);
                const rawHeaders = config.headers ?? config.header ?? {};
                const headers = Array.isArray(rawHeaders)
                    ? rawHeaders.map(item => Array.isArray(item) ? item : [item.key, item.value])
                    : Object.entries(rawHeaders);
                let body = method === "GET" || method === "HEAD" ? null : config.body ?? null;
                if (body !== null && typeof body !== "string") {
                    if (body.mode !== "raw" || typeof body.raw !== "string") throw new TypeError("sendRequest body must be text or {mode: 'raw', raw: text}");
                    body = body.raw;
                }
                const json = await send(stringify({
                    url: substitute(config.url, visibleVariables(), true, true),
                    method,
                    headers: headers.map(([key, value]) => [resolve(key), resolve(value)]),
                    body: body === null ? null : resolve(body),
                }));
                const data = JSON.parse(json);
                response = responseObject(data, () => data.body);
            } catch (error) {
                if (callback) { await callback(error, null); return; }
                throw error;
            }
            if (callback) await callback(null, response);
            return response;
        },
        expect,
        test(name, callback) {
            if (skipReason !== null) throw skipSignal;
            name = String(name);
            const fail = error => {
                if (error === skipSignal) throw error;
                test(name, String(error));
            };
            try {
                if (callback.length) throw new Error("Use a synchronous or async test function without a done callback");
                const result = callback();
                if (result && typeof result.then === "function") {
                    pendingTests++;
                    return Promise.resolve(result).then(() => test(name, null), fail)
                        .finally(() => pendingTests--);
                }
                test(name, null);
            } catch (error) { fail(error); }
        },
    };
    function responseObject(response, readText) {
        let responseBody;
        const text = () => responseBody ??= readText();
        const result = {
            code: response.code,
            status: response.status,
            responseTime: response.responseTime,
            headers: entries(response.headers, true),
            text,
            json: () => JSON.parse(text()),
            to: {have: {
                status(codeOrReason) {
                    const actual = typeof codeOrReason === "number" ? response.code : response.status;
                    expect(actual).to.equal(codeOrReason);
                },
                body(content) {
                    if (arguments.length === 0) expect(text()).not.to.be.empty;
                    else if (content instanceof RegExp) expect(text()).to.match(content);
                    else if (content !== null && typeof content === "object" && !Array.isArray(content)) expect(result.json()).to.deep.equal(content);
                    else expect(text()).to.equal(content);
                },
                jsonBody(path, value) {
                    const data = result.json();
                    if (arguments.length === 1) expect(data).to.have.nested.property(path);
                    else if (arguments.length > 1) expect(data).to.have.deep.nested.property(path, value);
                },
                jsonSchema(schema) {
                    const validation = pm.schema.validate(result.json(), schema);
                    if (!validation.valid) {
                        throw new Error("JSON Schema validation failed: " + validation.errors.map(error => `${error.instancePath || "/"}: ${error.message}`).join("; "));
                    }
                },
                header(name, value) {
                    expect(result.headers.has(name)).to.be.true;
                    if (value !== undefined) expect(result.headers.get(name)).to.equal(value);
                },
            }},
        };

        result.to.be = new Proxy({}, {
            get(_, name) {
                switch (name) {
                    case "then": return undefined;
                    case "ok": expect(response.code).to.equal(200); break;
                    case "success": expect(response.code).to.be.within(200, 299); break;
                    case "error": expect(response.code).to.be.within(400, 599); break;
                    case "clientError": expect(response.code).to.be.within(400, 499); break;
                    case "serverError": expect(response.code).to.be.within(500, 599); break;
                    case "json": result.json(); break;
                    default: throw new Error(`Unsupported response assertion: ${String(name)}`);
                }
            },
        });
        return result;
    }
    if (input.response) pm.response = responseObject(input.response, () => readBody(true));
    globalThis.pm = pm;
    globalThis.console = Object.fromEntries(["log", "info", "warn", "error", "debug"].map(level => [level, (...values) => log(level, values.map(format).join(" "))]));

    return {
        isSkipped: () => skipReason !== null,
        export() {
            if (pendingTests && skipReason === null) throw new Error("A test is awaiting a promise that cannot settle");
            return stringify({
        method: request.method,
        url,
        query,
        headers: input.headers,
        body: bodyChanged ? body : null,
        body_changed: bodyChanged,
        variables: {values: variables, environment, generated},
        environment_changes: environmentChanges,
        skip_reason: skipReason,
            });
        },
    };
})
