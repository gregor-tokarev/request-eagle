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
    // A form's fields, and a multipart form's parts as Postman lists them:
    // text parts with a value, file parts with the path in `src`.
    const fields = input.body.fields ?? [];
    const parts = (input.body.parts ?? []).map(part => part.file
        ? {key: part.name, src: part.value, type: "file"}
        : {key: part.name, value: part.value, type: "text"});
    const formdata = {
        get(name) { return parts.find(part => part.key === String(name))?.value; },
        has(name) { return parts.some(part => part.key === String(name)); },
        add(part) {
            parts.push(part.type === "file"
                ? {key: String(part.key), src: String(part.src ?? ""), type: "file"}
                : {key: String(part.key), value: String(part.value ?? ""), type: "text"});
        },
        remove(name) {
            for (let i = parts.length - 1; i >= 0; i--) {
                if (parts[i].key === String(name)) parts.splice(i, 1);
            }
        },
        upsert(part) { this.remove(part.key); this.add(part); },
        clear() { parts.length = 0; },
        toJSON() { return parts.map(part => ({...part})); },
    };
    const requestBody = {
        mode: input.body.mode,
        get raw() {
            if (!bodyLoaded && !bodyChanged) {
                body = originalBody = readBody(false);
                bodyLoaded = true;
            }
            return body;
        },
        // Setting text makes the body raw, as `update` does in Postman.
        set raw(value) {
            body = value;
            bodyChanged = !bodyLoaded || value !== originalBody;
            if (bodyChanged) this.mode = "raw";
        },
        update(value) { this.raw = String(value); },
    };
    if (input.body.mode === "urlencoded") requestBody.urlencoded = entries(fields);
    if (input.body.mode === "formdata") requestBody.formdata = formdata;
    if (input.body.mode === "file") requestBody.file = {src: input.body.file};
    const request = {
        method: input.method,
        get url() { return requestUrl; },
        set url(value) { requestUrl.update(value); },
        headers: entries(input.headers, true),
        body: requestBody,
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
    // A cookie from the jar, with its expiry date and attributes.
    const fromJar = cookie => ({
        ...cookie,
        expires: cookie.expires === null ? undefined : new Date(cookie.expires),
        sameSite: cookie.sameSite ?? undefined,
    });
    // The jar's cookies for a URL, in the order a request sends them. Throws
    // when the jar is off.
    const jarCookies = target => JSON.parse(jar("list", String(target), "")).map(fromJar);
    // Before sending, the URL the request goes to, with its {{variables}}
    // and :path variables filled as sending fills them.
    let target = (input.response ? input.url : input.sentUrl ?? input.url).trim();
    if (target && !target.includes("://")) target = "https://" + target;
    let sentFromJar = null, setInJar = [];
    try {
        sentFromJar = jarCookies(target);
        // The response's cookies the jar still holds, after their later
        // replacements, deletions and expiry, including those for other
        // paths or hosts that a request to this URL does not send.
        const headers = (input.response?.headers ?? [])
            .filter(([key]) => key.toLowerCase() === "set-cookie")
            .map(([, header]) => header);
        // A redirect's response came from its final URL, which decides the
        // default path and host of its cookies.
        const from = input.response?.url ?? target;
        if (headers.length) setInJar = JSON.parse(jar("stored", from, JSON.stringify(headers))).map(fromJar);
    } catch {}
    const expired = cookie => cookie.maxAge !== undefined ? cookie.maxAge <= 0 : cookie.expires !== undefined && cookie.expires <= Date.now();
    let cookies;
    if (sentFromJar) {
        // The jar already holds the cookies the response set, and replaces
        // those of the Cookie header that the response named.
        const renamed = new Set(setCookies.map(cookie => cookie.name));
        const own = sentCookies.filter(cookie => !renamed.has(cookie.name));
        const named = new Set(own.map(cookie => cookie.name));
        cookies = own.concat(sentFromJar.filter(cookie => !named.has(cookie.name)));
        const same = (a, b) => a.name === b.name && a.domain === b.domain && a.path === b.path;
        for (const cookie of setInJar) {
            if (!cookies.some(existing => same(existing, cookie))) cookies.push(cookie);
        }
    } else {
        cookies = sentCookies.slice();
        for (const cookie of setCookies) {
            const index = cookies.findIndex(existing => existing.name === cookie.name);
            if (index >= 0) cookies.splice(index, 1);
            if (!expired(cookie)) cookies.push(cookie);
        }
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
                    const stored = JSON.parse(jar("set", String(target), header(cookie)));
                    return stored === null ? null : fromJar(stored);
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
        fields: input.body.mode === "urlencoded" ? fields : null,
        parts: input.body.mode === "formdata" ? parts.map(part => part.type === "file"
            ? {name: part.key, value: part.src, file: true}
            : {name: part.key, value: part.value}) : null,
    });
})
