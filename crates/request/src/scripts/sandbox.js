(function (source, log, test) {
    "use strict";
    const input = JSON.parse(source);
    const stringify = JSON.stringify;
    const variables = Object.assign(Object.create(null), input.variables);
    const format = value => typeof value === "string" ? value : (stringify(value) ?? String(value));
    const replaceIn = text => String(text).replace(/\{\{([^{}]+)\}\}/g, (match, key) => variables[key] ?? match);

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

    function deepEqual(a, b) {
        if (a === b) return true;
        if (!a || !b || typeof a !== "object" || typeof b !== "object") return false;
        if (Array.isArray(a) !== Array.isArray(b)) return false;
        const keys = Object.keys(a);
        return keys.length === Object.keys(b).length && keys.every(key => Object.hasOwn(b, key) && deepEqual(a[key], b[key]));
    }

    function expect(actual, message) {
        let negate = false;
        let deep = false;
        const chain = new Proxy({}, {
            get(target, key, receiver) {
                if (key === "then") return undefined;
                if (!Reflect.has(target, key)) throw new Error(`Unsupported assertion: ${String(key)}`);
                return Reflect.get(target, key, receiver);
            },
        });
        function check(ok, description) {
            if (negate ? ok : !ok) throw new Error(message || `Expected ${format(actual)} ${negate ? "not " : ""}${description}`);
            negate = false;
            return chain;
        }
        for (const key of ["to", "be", "been", "is", "that", "which", "and", "has", "have", "with", "at", "of", "same"]) {
            Object.defineProperty(chain, key, {get: () => chain});
        }
        Object.defineProperty(chain, "not", {get() { negate = !negate; return chain; }});
        Object.defineProperty(chain, "deep", {get() { deep = true; return chain; }});
        chain.equal = chain.equals = chain.eq = expected => check(deep ? deepEqual(actual, expected) : actual === expected, `to equal ${format(expected)}`);
        chain.eql = expected => check(deepEqual(actual, expected), `to deeply equal ${format(expected)}`);
        chain.include = expected => check(typeof actual === "string" || Array.isArray(actual) ? actual.includes(expected) : !!actual && Object.keys(expected).every(key => deepEqual(actual[key], expected[key])), `to include ${format(expected)}`);
        chain.property = function (key, expected) {
            const exists = actual != null && Object.hasOwn(Object(actual), key);
            const result = check(exists && (arguments.length < 2 || deepEqual(actual[key], expected)), `to have property ${key}`);
            if (exists) actual = actual[key];
            return result;
        };
        chain.a = chain.an = type => check((Array.isArray(actual) ? "array" : actual === null ? "null" : typeof actual) === type, `to be ${type}`);
        chain.above = expected => check(actual > expected, `to be above ${expected}`);
        chain.below = expected => check(actual < expected, `to be below ${expected}`);
        chain.least = expected => check(actual >= expected, `to be at least ${expected}`);
        chain.most = expected => check(actual <= expected, `to be at most ${expected}`);
        chain.lengthOf = expected => check(actual?.length === expected, `to have length ${expected}`);
        chain.match = pattern => check(pattern.test(actual), `to match ${pattern}`);
        for (const [key, predicate] of Object.entries({true: v => v === true, false: v => v === false, null: v => v === null, undefined: v => v === undefined, ok: v => !!v, empty: v => v != null && (typeof v === "object" ? Object.keys(v).length === 0 : v.length === 0)})) {
            Object.defineProperty(chain, key, {get: () => check(predicate(actual), `to be ${key}`)});
        }
        return chain;
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
                status(code) { expect(response.code).to.equal(code); },
                header(name, value) {
                    expect(pm.response.headers.has(name)).to.be.true;
                    if (value !== undefined) expect(pm.response.headers.get(name)).to.equal(value);
                },
            }},
        };
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
