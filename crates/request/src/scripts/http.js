(function (input, pm, tools) {
    "use strict";
    const {entries, readBody, responseObject, skip, warn, cookies: jar} = tools;
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

    // Scripts see the cookies of this exchange: those the request sends,
    // from its Cookie header and the cookie jar, replaced or deleted by those
    // the response sets.
    function cookieList(cookies) {
        const one = name => cookies.find(cookie => cookie.name === String(name));
        return {
            get: name => one(name)?.value,
            has(name, value) {
                const cookie = one(name);
                return cookie !== undefined && (value === undefined || cookie.value === value);
            },
            one,
            all: () => cookies.slice(),
            count: () => cookies.length,
            idx: index => cookies[index],
            each(callback) { cookies.forEach(callback); },
            filter: callback => cookies.filter(callback),
            find: callback => cookies.find(callback),
            map: callback => cookies.map(callback),
            toObject: () => Object.fromEntries(cookies.map(cookie => [cookie.name, cookie.value])),
            toJSON: () => cookies.slice(),
        };
    }
    // Split `name=value` at the first `=`. Without one, the value is null.
    const split = text => {
        const index = text.indexOf("=");
        return index < 0 ? [text.trim(), null] : [text.slice(0, index).trim(), text.slice(index + 1).trim()];
    };
    const sentCookies = input.headers
        .filter(([key]) => key.toLowerCase() === "cookie")
        // Before sending, the header may still contain {{variables}}.
        .flatMap(([, header]) => (input.response ? header : pm.variables.replaceIn(header)).split(";").map(split))
        .filter(([name, value]) => name && value !== null)
        .map(([name, value]) => ({name, value}));
    const setCookies = (input.response?.headers ?? [])
        .filter(([key]) => key.toLowerCase() === "set-cookie")
        .map(([, header]) => {
            const [pair, ...attributes] = header.split(";");
            const [name, value] = split(pair);
            if (!name || value === null) return null;

            const cookie = {name, value, httpOnly: false, secure: false};
            for (const [attribute, setting] of attributes.map(split)) {
                switch (attribute.toLowerCase()) {
                    case "domain": cookie.domain = (setting ?? "").replace(/^\./, ""); break;
                    case "path": cookie.path = setting ?? ""; break;
                    case "expires": if (!Number.isNaN(Date.parse(setting))) cookie.expires = new Date(setting); break;
                    case "max-age": if (/^-?\d+$/.test(setting)) cookie.maxAge = Number(setting); break;
                    case "httponly": cookie.httpOnly = true; break;
                    case "secure": cookie.secure = true; break;
                    case "samesite": cookie.sameSite = setting ?? ""; break;
                }
            }
            return cookie;
        })
        .filter(Boolean);
    // The jar's cookies for a URL, with expiry dates. Throws when the jar is off.
    const jarCookies = target => JSON.parse(jar("list", String(target), "")).map(cookie => ({
        ...cookie,
        expires: cookie.expires === null ? undefined : new Date(cookie.expires),
    }));
    let sentFromJar = [];
    try {
        // Before sending, the URL may still contain {{variables}} and lack a scheme.
        let target = (input.response ? input.url : pm.variables.replaceIn(input.url)).trim();
        if (target && !target.includes("://")) target = "https://" + target;
        sentFromJar = jarCookies(target);
    } catch {}
    const named = new Set(sentCookies.map(cookie => cookie.name));
    const cookies = sentCookies.concat(sentFromJar.filter(cookie => !named.has(cookie.name)));
    for (const cookie of setCookies) {
        const index = cookies.findIndex(existing => existing.name === cookie.name);
        if (index >= 0) cookies.splice(index, 1);
        const expired = cookie.maxAge !== undefined ? cookie.maxAge <= 0 : cookie.expires !== undefined && cookie.expires <= Date.now();
        if (!expired) cookies.push(cookie);
    }

    pm.request = request;
    pm.cookies = cookieList(cookies);
    pm.cookies.jar = () => {
        // As in Postman, results arrive through callbacks. Without one, a
        // failure is logged.
        const call = (callback, action) => {
            let error = null, result;
            try { result = action(); } catch (caught) { error = caught; }
            if (typeof callback === "function") callback(error, result);
            else if (error) warn(error.message);
        };
        const header = cookie => {
            let text = `${cookie.name ?? cookie.key ?? ""}=${cookie.value ?? ""}`;
            if (cookie.domain) text += `; Domain=${cookie.domain}`;
            if (cookie.path) text += `; Path=${cookie.path}`;
            if (cookie.expires != null) text += `; Expires=${new Date(cookie.expires).toUTCString()}`;
            if (cookie.maxAge != null) text += `; Max-Age=${cookie.maxAge}`;
            if (cookie.secure) text += "; Secure";
            if (cookie.httpOnly) text += "; HttpOnly";
            if (cookie.sameSite) text += `; SameSite=${cookie.sameSite}`;
            return text;
        };
        return {
            get(target, name, callback) {
                call(callback, () => jarCookies(target).find(cookie => cookie.name === String(name))?.value);
            },
            getAll(target, options, callback) {
                call(typeof options === "function" ? options : callback, () => jarCookies(target));
            },
            // set(url, name, value, callback) or set(url, {name, value, ...attributes}, callback)
            set(target, name, value, callback) {
                const cookie = name !== null && typeof name === "object" ? name : {name, value};
                if (cookie === name) callback = value;
                call(callback, () => {
                    jar("set", String(target), header(cookie));
                    return jarCookies(target).find(stored => stored.name === String(cookie.name ?? cookie.key)) ?? null;
                });
            },
            unset(target, name, callback) {
                call(callback, () => { jar("unset", String(target), String(name)); });
            },
            clear(target, callback) {
                call(callback, () => { jar("clear", String(target), ""); });
            },
        };
    };
    // Sending one request has no next request to set, as in Postman outside
    // the Collection Runner.
    pm.execution = input.response ? {setNextRequest() {}} : {
        skipRequest(reason = "Skipped by pre-request script") { skip(reason); },
        setNextRequest() {},
    };
    if (input.response) {
        pm.response = responseObject(input.response, () => readBody(true));
        pm.response.cookies = cookieList(setCookies);
    }

    return () => ({
        method: request.method,
        url,
        query,
        headers: input.headers,
        body: bodyChanged ? body : null,
        body_changed: bodyChanged,
    });
})
