(function (source, log, test, expect, dynamic, readBody, send, utilities, library, cookies, protocol) {
    "use strict";
    const input = JSON.parse(source);
    const stringify = JSON.stringify;
    const object = values => Object.assign(Object.create(null), values);
    const variables = object(input.variables.values);
    // The Collection Runner's data file row, which `{{name}}` prefers to
    // every scope and `pm.variables` overrides.
    const data = object(input.variables.data ?? {});
    // From lowest to highest: the environment covers the collection's
    // variables, which cover the globals. Null hides a name in its scope and
    // the scopes beneath it.
    const scopes = [input.variables.globals, input.variables.collection, input.variables.environment].map(object);
    const changes = scopes.map(() => Object.create(null));
    let skipReason = null;
    // Undefined until a script chooses the next request; null ends the iteration.
    let nextRequest;
    const setNextRequest = name => { nextRequest = name === null ? null : String(name); };
    const skipSignal = {};
    let pendingTests = 0;
    const generated = object(input.variables.generated);
    const format = value => {
        try { return typeof value === "string" ? value : (stringify(value) ?? String(value)); }
        catch {
            try { return String(value).slice(0, 4096); }
            catch { return "[Unserializable value]"; }
        }
    };
    // `lookup` returns a variable's value, or undefined.
    const substitute = (text, lookup, strict = false, isUrl = false) => {
        const resolve = (match, key) => {
            if (strict && key.startsWith("!")) return "{{" + key.slice(1) + "}}";
            key = key.trim();
            const value = lookup(key) ?? generated[key];
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

    // A scope reads the scopes from `top` down to `bottom`.
    const scopeValue = (key, top, bottom = 0) => {
        for (let index = top; index >= bottom; index--) {
            if (Object.hasOwn(scopes[index], key)) return scopes[index][key] ?? undefined;
        }
    };
    const scopeValues = (top, bottom = 0) => {
        const values = Object.create(null);
        for (const scope of scopes.slice(bottom, top + 1)) {
            for (const [key, value] of Object.entries(scope)) {
                if (value === null) delete values[key];
                else values[key] = value;
            }
        }
        return values;
    };
    function scope(top, bottom) {
        const values = () => scopeValues(top, bottom);
        const read = key => scopeValue(String(key), top, bottom);
        return {
            get: read,
            has: key => read(key) !== undefined,
            set(key, value) {
                key = String(key);
                if (!key || key.startsWith("$")) throw new Error("Variable names must be nonempty and cannot start with $");
                scopes[top][key] = changes[top][key] = String(value);
            },
            unset(key) { key = String(key); scopes[top][key] = changes[top][key] = null; },
            clear() { for (const key of Object.keys(values())) this.unset(key); },
            toObject: () => ({...values()}),
            replaceIn: text => substitute(text, read),
        };
    }
    const visibleValue = key => variables[key] ?? data[key] ?? scopeValue(key, 2);
    const visibleVariables = () => Object.assign(scopeValues(2), data, variables);
    const replaceIn = text => substitute(text, visibleValue);

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

    const pm = {
        variables: {
            get: key => visibleValue(String(key)),
            has: key => visibleValue(String(key)) !== undefined,
            set(key, value) { variables[String(key)] = String(value); },
            unset(key) { delete variables[key]; },
            clear() { for (const key of Object.keys(variables)) delete variables[key]; },
            toObject: visibleVariables,
            replaceIn,
        },
        iterationData: {
            get: key => data[String(key)],
            has: key => Object.hasOwn(data, String(key)),
            unset(key) { delete data[String(key)]; },
            toObject: () => ({...data}),
            toJSON: () => ({...data}),
            replaceIn: text => substitute(text, key => data[key]),
        },
        info: Object.freeze({
            eventName: input.info?.eventName ?? "",
            iteration: input.info?.iteration ?? 0,
            iterationCount: input.info?.iterationCount ?? 1,
            requestName: input.info?.requestName ?? "",
            requestId: input.info?.requestId ?? "",
        }),
        globals: scope(0, 0),
        collectionVariables: scope(1, 1),
        // The environment includes the collection's variables.
        environment: scope(2, 1),
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
        async sendRequest(config, callback) {
            if (skipReason !== null) throw skipSignal;
            let response;
            try {
                if (typeof config === "string") config = {url: config};
                if (!config || typeof config.url !== "string") throw new TypeError("sendRequest requires a URL string or an object with a url string");
                if (callback !== undefined && typeof callback !== "function") throw new TypeError("sendRequest callback must be a function");
                const method = String(config.method ?? "GET").toUpperCase();
                const resolve = text => substitute(text, visibleValue, true);
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
                    url: substitute(config.url, visibleValue, true, true),
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
    function skip(reason) {
        skipReason = String(reason).slice(0, 4096);
        throw skipSignal;
    }

    // Postman's sandbox libraries. Each loads the first time it is required.
    const uuid = Object.assign(() => dynamic("$guid"), {v4: () => dynamic("$guid")});
    const builtins = {atob: utilities.atob, btoa: utilities.btoa, uuid};
    const modules = new Map();
    const getRandomValues = array => {
        if (!(array instanceof Int8Array || array instanceof Uint8Array || array instanceof Uint8ClampedArray
            || array instanceof Int16Array || array instanceof Uint16Array || array instanceof Int32Array
            || array instanceof Uint32Array || array instanceof BigInt64Array || array instanceof BigUint64Array)) {
            throw new TypeError("getRandomValues requires an integer typed array");
        }
        const bytes = new Uint8Array(array.buffer, array.byteOffset, array.byteLength);
        const hex = utilities.randomBytes(bytes.length);
        for (let index = 0; index < bytes.length; index++) bytes[index] = parseInt(hex.substr(index * 2, 2), 16);
        return array;
    };
    function require(name) {
        name = String(name);
        if (modules.has(name)) return modules.get(name).exports;
        if (Object.hasOwn(builtins, name)) {
            modules.set(name, {exports: builtins[name], loaded: true});
            return builtins[name];
        }

        const load = library(name);
        if (!load) throw new Error(`Cannot find module '${name}'. Scripts can require atob, btoa, crypto-js, lodash, moment and uuid.`);
        // Like Node, a module required while it loads returns its exports so far.
        const module = {exports: {}, loaded: false};
        modules.set(name, module);
        try {
            load.call(module.exports, module, module.exports, {crypto: {getRandomValues}});
        } catch (error) {
            modules.delete(name);
            throw error;
        }
        module.loaded = true;
        return module.exports;
    }
    // Postman also provides some libraries as globals.
    for (const [name, module] of [["_", "lodash"], ["CryptoJS", "crypto-js"]]) {
        const replace = value => Object.defineProperty(globalThis, name, {value, writable: true, configurable: true});
        Object.defineProperty(globalThis, name, {
            configurable: true,
            get() {
                // Lodash reads `_` while it loads, to restore it on noConflict.
                if (modules.get(module)?.loaded === false) return undefined;
                const value = require(module);
                replace(value);
                return value;
            },
            set: replace,
        });
    }

    // The protocol adds pm.request, pm.response and pm.execution, and
    // reports the request changes to send.
    const warn = message => log("warn", message);
    const exportRequest = protocol(input, pm, {entries, expect, readBody, responseObject, skip, setNextRequest, warn, cookies});
    globalThis.pm = pm;
    globalThis.console = Object.fromEntries(["log", "info", "warn", "error", "debug"].map(level => [level, (...values) => log(level, values.map(format).join(" "))]));
    globalThis.require = require;
    globalThis.atob = utilities.atob;
    globalThis.btoa = utilities.btoa;
    // Postman's legacy API.
    globalThis.postman = {
        setNextRequest,
        getEnvironmentVariable: key => pm.environment.get(key),
        setEnvironmentVariable: (key, value) => pm.environment.set(key, value),
        clearEnvironmentVariable: key => pm.environment.unset(key),
        clearEnvironmentVariables: () => pm.environment.clear(),
        getGlobalVariable: key => pm.globals.get(key),
        setGlobalVariable: (key, value) => pm.globals.set(key, value),
        clearGlobalVariable: key => pm.globals.unset(key),
        clearGlobalVariables: () => pm.globals.clear(),
    };

    return {
        isSkipped: () => skipReason !== null,
        export() {
            if (pendingTests && skipReason === null) throw new Error("A test is awaiting a promise that cannot settle");
            return stringify({
                request: exportRequest(),
                variables: {values: variables, data, globals: scopes[0], collection: scopes[1], environment: scopes[2], generated},
                changes: {globals: changes[0], collection: changes[1], environment: changes[2]},
                skip_reason: skipReason,
                next_request: nextRequest === undefined ? null : {name: nextRequest},
            });
        },
    };
})
