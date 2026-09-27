(function (source, log, test, expect, dynamic, readBody) {
    "use strict";
    const input = JSON.parse(source);
    const stringify = JSON.stringify;
    const variables = Object.assign(Object.create(null), input.variables.values);
    const generated = Object.assign(Object.create(null), input.variables.generated);
    const format = value => {
        try { return typeof value === "string" ? value : (stringify(value) ?? String(value)); }
        catch {
            try { return String(value).slice(0, 4096); }
            catch { return "[Unserializable value]"; }
        }
    };
    const replaceIn = text => String(text).replace(/\{\{([^{}]+)\}\}/g, (match, key) => {
        const value = variables[key] ?? generated[key];
        if (value !== undefined) return value;
        const fresh = dynamic(key);
        if (fresh == null) return match;
        generated[key] = fresh;
        return fresh;
    });

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
        let responseBody;
        const text = () => responseBody ??= readBody(true);
        pm.response = {
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
                    else if (content !== null && typeof content === "object" && !Array.isArray(content)) expect(pm.response.json()).to.deep.equal(content);
                    else expect(text()).to.equal(content);
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
        url,
        query,
        headers: input.headers,
        body: bodyChanged ? body : null,
        body_changed: bodyChanged,
        variables: {values: variables, generated},
    });
})
