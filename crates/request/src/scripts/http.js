(function (input, pm, tools) {
    "use strict";
    const {entries, readBody, responseObject, skip} = tools;
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

    pm.request = request;
    pm.execution = input.response ? {} : {
        skipRequest(reason = "Skipped by pre-request script") { skip(reason); },
    };
    if (input.response) pm.response = responseObject(input.response, () => readBody(true));

    return () => ({
        method: request.method,
        url,
        query,
        headers: input.headers,
        body: bodyChanged ? body : null,
        body_changed: bodyChanged,
    });
})
