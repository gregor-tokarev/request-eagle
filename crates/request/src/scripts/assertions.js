(function () {
    "use strict";

    const words = new Set("to be been is that which and has have with at of same but does still also".split(" "));
    const aliases = {
        equals: "equal", eq: "equal", includes: "include", contain: "include", contains: "include",
        an: "a", greaterThan: "above", lessThan: "below", gte: "least", lte: "most",
        length: "lengthOf", exists: "exist",
    };
    const methods = new Set("equal eql include property keys members oneOf a above below least most within closeTo lengthOf match".split(" "));
    const chainable = new Set(["include", "a", "lengthOf"]);
    const type = value => Object.prototype.toString.call(value).slice(8, -1).toLowerCase();
    const keys = value => Reflect.ownKeys(value).filter(key => Object.prototype.propertyIsEnumerable.call(value, key));
    const plain = value => value !== null && typeof value === "object" && [null, Object.prototype].includes(Object.getPrototypeOf(value));

    function require(condition, message) {
        if (!condition) throw new Error(message);
    }

    function display(value) {
        try {
            return (typeof value === "object" && value !== null ? JSON.stringify(value) : String(value)).slice(0, 200);
        } catch (_) {
            return Object.prototype.toString.call(value);
        }
    }

    // Compare response-shaped data; unsupported objects must not silently compare equal.
    function deepEqual(left, right, seenLeft = new Set(), seenRight = new Set()) {
        if (Object.is(left, right)) return true;
        if (left === null || right === null || typeof left !== "object" || typeof right !== "object") return false;

        const supported = value => plain(value) || Array.isArray(value) || value instanceof Date || value instanceof RegExp;
        require(supported(left) && supported(right), "Deep assertions support plain objects, arrays, dates, and regular expressions");
        if (type(left) !== type(right)) return false;
        if (left instanceof Date) return Object.is(left.getTime(), right.getTime());
        if (left instanceof RegExp) return left.source === right.source && left.flags === right.flags;
        require(!seenLeft.has(left) && !seenRight.has(right), "Deep assertions do not support cyclic values");
        if (Array.isArray(left) && left.length !== right.length) return false;

        const array = Array.isArray(left);
        const leftKeys = array ? Array.from(left.keys()) : keys(left);
        if (!array && leftKeys.length !== keys(right).length) return false;
        seenLeft.add(left);
        seenRight.add(right);
        const equal = leftKeys.every(key => (array || Object.hasOwn(right, key)) && deepEqual(left[key], right[key], seenLeft, seenRight));
        seenLeft.delete(left);
        seenRight.delete(right);
        return equal;
    }

    function pathKeys(path) {
        require(typeof path === "string" && path.length > 0, "Nested property paths must be nonempty strings");
        const token = /(?:\\.|[^.\[\]\\])+|\[\d+\]/y;
        const result = [];
        let offset = 0;

        while (offset < path.length) {
            token.lastIndex = offset;
            const match = token.exec(path);
            require(match !== null, `Unsupported nested property path: ${path}`);
            const part = match[0];
            result.push(part.startsWith("[") ? String(Number(part.slice(1, -1))) : part.replace(/\\(.)/g, "$1"));
            offset = token.lastIndex;
            if (offset === path.length) break;
            if (path[offset] === ".") offset++;
            else require(path[offset] === "[", `Unsupported nested property path: ${path}`);
            require(offset < path.length, `Unsupported nested property path: ${path}`);
        }
        return result;
    }

    return function expect(actual, message) {
        const flags = {};
        const compare = (left, right) => flags.deep ? deepEqual(left, right) : left === right;

        function check(passed, description, detail) {
            if (detail !== undefined) message = detail;
            if (flags.not ? passed : !passed) {
                throw new Error(`${message === undefined ? "" : `${message}: `}expected ${display(actual)} ${flags.not ? "not " : ""}${description}`);
            }
            return chain();
        }

        function property(name) {
            require(["string", "number", "symbol"].includes(typeof name), "Property names must be strings, numbers, or symbols");
            const parts = flags.nested ? pathKeys(name) : [name];
            let value = actual;
            for (const key of parts) {
                const exists = value != null && (flags.own ? Object.hasOwn(Object(value), key) : key in Object(value));
                if (!exists) return {exists: false, value: undefined};
                value = value[key];
            }
            return {exists: true, value};
        }

        function includes(expected) {
            if (typeof actual === "string") {
                require(typeof expected === "string", "String inclusion requires a string");
                return actual.includes(expected);
            }
            if (Array.isArray(actual)) return actual.some(value => compare(value, expected));
            require(plain(actual) && plain(expected), "Inclusion supports strings, arrays, or plain object subsets");
            const expectedKeys = keys(expected);
            require(expectedKeys.length > 0, "Object inclusion requires a nonempty subset");
            return expectedKeys.every(key => {
                const found = property(key);
                return found.exists && compare(found.value, expected[key]);
            });
        }

        function length() {
            require(actual != null && typeof actual.length === "number", "Length assertions require a length property");
            return actual.length;
        }

        function number(value) {
            require(typeof value === "number" && !Number.isNaN(value), "Numeric assertions require numbers");
            return value;
        }

        function invoke(name, args) {
            const [expected, second, third] = args;
            switch (name) {
                case "equal": return check(compare(actual, expected), `to equal ${display(expected)}`, second);
                case "eql": return check(deepEqual(actual, expected), `to deeply equal ${display(expected)}`, second);
                case "a":
                    require(typeof expected === "string", "Type assertions require a type name");
                    return check(type(actual) === expected.toLowerCase(), `to be ${expected}`, second);
                case "include": return check(includes(expected), `to include ${display(expected)}`, second);
                case "property": {
                    const found = property(expected);
                    const passed = found.exists && (args.length < 2 || compare(found.value, second));
                    const description = `to have property ${String(expected)}${args.length < 2 ? "" : ` equal to ${display(second)}`}`;
                    const result = check(passed, description, third);
                    actual = found.value;
                    return result;
                }
                case "keys": {
                    require(plain(actual) || Array.isArray(actual), "Key assertions require a plain object or array");
                    const wanted = args.length === 1 && Array.isArray(expected) ? expected : args;
                    require(wanted.length > 0 && wanted.every(key => ["string", "number", "symbol"].includes(typeof key)), "Key assertions require one or more property names");
                    const names = [...new Set(wanted.map(key => typeof key === "symbol" ? key : String(key)))];
                    require(names.length === wanted.length, "Key assertions require distinct property names");
                    const actualKeys = keys(actual);
                    const matches = key => actualKeys.includes(key);
                    const passed = flags.any ? names.some(matches) : names.every(matches) && (flags.include || names.length === actualKeys.length);
                    return check(passed, `to have ${flags.any ? "any" : "all"} keys ${names.map(String).join(", ")}`);
                }
                case "members": {
                    require(Array.isArray(actual) && Array.isArray(expected), "Member assertions require arrays");
                    const remaining = [...actual];
                    const passed = (flags.include || actual.length === expected.length) && Array.from(expected).every((value, index) => {
                        if (flags.ordered) return index < actual.length && compare(actual[index], value);
                        const found = remaining.findIndex(item => compare(item, value));
                        if (found < 0) return false;
                        if (!flags.include) remaining.splice(found, 1);
                        return true;
                    });
                    return check(passed, `to have ${flags.include ? "at least " : ""}${flags.ordered ? "ordered " : ""}members ${display(expected)}`, second);
                }
                case "oneOf":
                    require(Array.isArray(expected), "oneOf requires an array");
                    return check(expected.some(value => flags.include ? includes(value) : compare(actual, value)), `to be one of ${display(expected)}`, second);
                case "lengthOf": return check(length() === number(expected), `to have length ${expected}`, second);
                case "match":
                    require(typeof actual === "string" && expected instanceof RegExp, "match requires a string and a regular expression");
                    return check(expected.test(actual), `to match ${expected}`, second);
                default: {
                    const value = number(flags.length ? length() : actual);
                    const bound = number(expected);
                    switch (name) {
                        case "above": return check(value > bound, `to be above ${bound}`, second);
                        case "below": return check(value < bound, `to be below ${bound}`, second);
                        case "least": return check(value >= bound, `to be at least ${bound}`, second);
                        case "most": return check(value <= bound, `to be at most ${bound}`, second);
                        case "within": {
                            const upper = number(second);
                            return check(value >= bound && value <= upper, `to be within ${bound}..${upper}`, third);
                        }
                        case "closeTo":
                            require(number(second) >= 0, "closeTo requires a nonnegative delta");
                            return check(Math.abs(value - bound) <= second, `to be within ${second} of ${bound}`, third);
                    }
                }
            }
        }

        function chain(method) {
            return new Proxy(() => {}, {
                apply(_, __, args) {
                    require(method !== undefined, "Expected an assertion method");
                    return invoke(method, args);
                },
                get(_, name) {
                    if (name === "then" || typeof name === "symbol") return undefined;
                    require(method === undefined || chainable.has(method), `Assertion method ${method} must be called`);
                    name = Object.hasOwn(aliases, name) ? aliases[name] : name;
                    if (words.has(name)) return chain();
                    if (["not", "deep", "nested", "own", "ordered"].includes(name)) {
                        flags[name] = true;
                        require(!(flags.nested && flags.own), "nested and own cannot be combined");
                        return chain();
                    }
                    if (name === "any" || name === "all") {
                        flags.any = name === "any";
                        return chain();
                    }
                    if (methods.has(name)) {
                        if (name === "include") flags.include = true;
                        if (name === "lengthOf") flags.length = true;
                        return chain(name);
                    }
                    switch (name) {
                        case "true": return check(actual === true, "to be true");
                        case "false": return check(actual === false, "to be false");
                        case "null": return check(actual === null, "to be null");
                        case "undefined": return check(actual === undefined, "to be undefined");
                        case "ok": return check(Boolean(actual), "to be truthy");
                        case "exist": return check(actual != null, "to exist");
                        case "NaN": return check(Number.isNaN(actual), "to be NaN");
                        case "finite": return check(Number.isFinite(actual), "to be finite");
                        case "empty":
                            require(typeof actual === "string" || Array.isArray(actual) || plain(actual), "empty supports strings, arrays, or plain objects");
                            return check((plain(actual) ? keys(actual).length : actual.length) === 0, "to be empty");
                        default: throw new Error(`Unsupported assertion: ${name}`);
                    }
                },
            });
        }

        return chain();
    };
})()
