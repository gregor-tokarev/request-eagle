/** Request Eagle's supported script APIs. The editor supplies the current phase's `pm` type. */
declare namespace RequestEagle {
    type HttpMethod = "GET" | "POST" | "PUT" | "PATCH" | "DELETE" | "HEAD" | "OPTIONS";

    /** Common HTTP header names. Any other header name is also accepted. */
    type CommonHeaderName =
        | "Accept"
        | "Accept-Encoding"
        | "Accept-Language"
        | "Access-Control-Allow-Origin"
        | "Authorization"
        | "Cache-Control"
        | "Connection"
        | "Content-Disposition"
        | "Content-Encoding"
        | "Content-Length"
        | "Content-Type"
        | "Cookie"
        | "Date"
        | "ETag"
        | "Expires"
        | "If-Match"
        | "If-Modified-Since"
        | "If-None-Match"
        | "Last-Modified"
        | "Location"
        | "Origin"
        | "Referer"
        | "Retry-After"
        | "Server"
        | "Set-Cookie"
        | "User-Agent"
        | "Vary"
        | "WWW-Authenticate"
        | "X-Api-Key"
        | "X-Request-Id"
        | "X-Requested-With";

    /** `string & {}` keeps the common names as suggestions without restricting other names. */
    type HeaderName = CommonHeaderName | (string & {});

    interface KeyValue<Name extends string = string> {
        key: Name;
        value: string;
    }

    /** Header names are case-insensitive; query parameter names are case-sensitive. */
    interface Entries<Name extends string = string> {
        /** Return the first matching value, or undefined when absent. */
        get(name: Name): string | undefined;
        has(name: Name): boolean;
        /** Append an entry, preserving existing entries with the same name. */
        add(entry: KeyValue<Name>): void;
        /** Remove all entries with this name. */
        remove(name: Name): void;
        /** Replace all matching entries with this one. */
        upsert(entry: KeyValue<Name>): void;
        clear(): void;
        /** Copy the entries, including repeated names. */
        toJSON(): KeyValue<Name>[];
    }

    type Headers = Entries<HeaderName>;

    interface RequestURL {
        /** Query parameters from both the URL and the Params editor. */
        readonly query: Entries;
        /** Replace the URL and clear the previous query parameters. */
        update(url: string): void;
        /** Return the URL with query parameters; omit the fragment. */
        toString(): string;
        toJSON(): string;
    }

    interface RequestBody {
        readonly mode: "raw";
        /** Decoded body text, or null when absent. Assign text to replace the body. */
        raw: string | null;
        /** Replace the outgoing body with text. GET and HEAD omit request bodies. */
        update(text: string): void;
    }

    /** The execution snapshot. Edits never modify the request draft or saved file. */
    interface Request {
        method: HttpMethod;
        /** Read the URL object, or assign a string to replace the URL. */
        get url(): RequestURL;
        set url(value: string | RequestURL);
        readonly headers: Headers;
        readonly body: RequestBody;
    }

    /** Header names mapped to values. Common names are suggested; any name is accepted. */
    type HeaderObject = {[Name in CommonHeaderName]?: string} & Record<string, string>;

    type RequestHeaders = HeaderObject | [HeaderName, string][] | KeyValue<HeaderName>[];

    interface RawBody {
        mode: "raw";
        raw: string;
    }

    interface RequestOptions {
        /** Absolute HTTP(S) URL. {{variables}} resolve when the call starts. */
        url: string;
        /** HTTP method; defaults to GET. */
        method?: HttpMethod;
        /** Header object, [name, value] pairs, or {key, value} entries. */
        headers?: RequestHeaders;
        /** Alias for headers; ignored when headers is supplied. */
        header?: RequestHeaders;
        /** Text or a raw body object. GET and HEAD omit the body. */
        body?: string | RawBody | null;
    }

    interface VariableScope {
        /** Return the value, or undefined when the name is absent. */
        get(name: string): string | undefined;
        has(name: string): boolean;
        /** Set a value after converting it to a string. */
        set(name: string, value: unknown): void;
        unset(name: string): void;
        clear(): void;
        /** Copy the values currently visible in this scope. */
        toObject(): Record<string, string>;
        /** Substitute {{name}} references once. Unknown names remain unchanged. */
        replaceIn(text: string): string;
    }

    interface LocalVariables extends VariableScope {
        /** Read the local override first, then the environment. */
        get(name: string): string | undefined;
        /** Set an override for this execution, including its post-response phase. */
        set(name: string, value: unknown): void;
        /** Remove a local override, revealing its environment value if present. */
        unset(name: string): void;
        /** Remove all local overrides, leaving environment values visible. */
        clear(): void;
    }

    interface EnvironmentVariables extends VariableScope {
        /**
         * Save a session value for other requests in this environment without changing its file.
         * @param name Nonempty variable name that does not start with $.
         */
        set(name: string, value: unknown): void;
        /** Hide the value for this session, including any value in the environment file. */
        unset(name: string): void;
        /** Hide all environment values visible to this script. */
        clear(): void;
    }

    interface Crypto {
        /** SHA-256 of UTF-8 text, returned as lowercase hexadecimal. Input limit: 1 MiB. */
        sha256(text: string): string;
        /**
         * HMAC-SHA256, returned as lowercase hexadecimal. Each input is limited to 1 MiB.
         * @param secret UTF-8 signing key.
         * @param text UTF-8 message to sign.
         */
        hmacSha256(secret: string, text: string): string;
        /**
         * Return secure random bytes as hexadecimal, with two characters per byte.
         * @param count Integer byte count from 0 to 65536.
         */
        randomBytes(count: number): string;
    }

    interface Encoding {
        /** Encode UTF-8 text as standard padded Base64. Input limit: 1 MiB. */
        base64Encode(text: string): string;
        /** Decode standard Base64 to UTF-8 text. Invalid Base64 or UTF-8 throws. */
        base64Decode(text: string): string;
        /** Encode UTF-8 text as URL-safe Base64 without padding. Input limit: 1 MiB. */
        base64UrlEncode(text: string): string;
        /** Decode padded or unpadded URL-safe Base64 to UTF-8 text. */
        base64UrlDecode(text: string): string;
    }

    type SchemaType = "null" | "boolean" | "object" | "array" | "number" | "integer" | "string";
    type JSONSchema = boolean | Schema;
    type SchemaFormat =
        | "date"
        | "date-time"
        | "duration"
        | "email"
        | "hostname"
        | "idn-email"
        | "idn-hostname"
        | "ipv4"
        | "ipv6"
        | "iri"
        | "iri-reference"
        | "json-pointer"
        | "regex"
        | "relative-json-pointer"
        | "time"
        | "uri"
        | "uri-reference"
        | "uri-template"
        | "uuid"
        | (string & {});

    /** JSON Schema. External, cyclic, dynamic, and recursive references are not supported. */
    interface Schema {
        /** Dialect URI. Defaults to JSON Schema Draft 2020-12. */
        $schema?: string;
        /** Schema identifier; supported only at the root. */
        $id?: string;
        /** Legacy schema identifier; supported only at the root. */
        id?: string;
        /** Local acyclic JSON pointer, such as #/$defs/user. */
        $ref?: string;
        $defs?: Record<string, JSONSchema>;
        definitions?: Record<string, JSONSchema>;
        title?: string;
        description?: string;
        $comment?: string;
        type?: SchemaType | SchemaType[];
        enum?: unknown[];
        const?: unknown;
        default?: unknown;
        examples?: unknown[];
        readOnly?: boolean;
        writeOnly?: boolean;
        deprecated?: boolean;
        multipleOf?: number;
        minimum?: number;
        maximum?: number;
        exclusiveMinimum?: number | boolean;
        exclusiveMaximum?: number | boolean;
        minLength?: number;
        maxLength?: number;
        /** Regular expression without lookaround or backreferences. */
        pattern?: string;
        /** Supported JSON Schema formats are validated. */
        format?: SchemaFormat;
        properties?: Record<string, JSONSchema>;
        patternProperties?: Record<string, JSONSchema>;
        additionalProperties?: JSONSchema;
        required?: string[];
        minProperties?: number;
        maxProperties?: number;
        propertyNames?: JSONSchema;
        dependentRequired?: Record<string, string[]>;
        dependentSchemas?: Record<string, JSONSchema>;
        dependencies?: Record<string, JSONSchema | string[]>;
        items?: JSONSchema | JSONSchema[];
        additionalItems?: JSONSchema;
        prefixItems?: JSONSchema[];
        contains?: JSONSchema;
        minContains?: number;
        maxContains?: number;
        minItems?: number;
        maxItems?: number;
        uniqueItems?: boolean;
        allOf?: JSONSchema[];
        anyOf?: JSONSchema[];
        oneOf?: JSONSchema[];
        not?: JSONSchema;
        if?: JSONSchema;
        then?: JSONSchema;
        else?: JSONSchema;
        contentEncoding?: string;
        contentMediaType?: string;
        contentSchema?: JSONSchema;
    }

    interface SchemaError {
        /** JSON pointer to the failing value. An empty string denotes the root. */
        instancePath: string;
        /** JSON pointer to the failing schema keyword. */
        schemaPath: string;
        message: string;
    }

    interface SchemaValidation {
        valid: boolean;
        /** Up to 20 validation errors. */
        errors: SchemaError[];
        /** Whether further validation errors were omitted. */
        truncated: boolean;
    }

    interface SchemaAPI {
        /**
         * Validate JSON data. Invalid or unsupported schemas throw; mismatches return valid: false.
         * @param data JSON-serializable data, limited to 1 MiB.
         * @param schema JSON Schema, limited to 64 KiB. Local acyclic references only.
         */
        validate(data: unknown, schema: JSONSchema): SchemaValidation;
    }

    interface ResponseHaveAssertions {
        /** Assert the status code or canonical status text. */
        status(codeOrReason: number | string): void;
        /** Assert a nonempty body, exact text, a regular expression, or a parsed JSON object. */
        body(content?: string | RegExp | Record<string, unknown>): void;
        /** Assert valid JSON; optionally check a nested property and its value. */
        jsonBody(path?: string, value?: unknown): void;
        /** Assert that the parsed JSON response matches a schema. */
        jsonSchema(schema: JSONSchema): void;
        /** Assert that a header exists; optionally compare its first value. */
        header(name: HeaderName, value?: string): void;
    }

    interface ResponseBeAssertions {
        /** Assert status 200. */
        readonly ok: void;
        /** Assert a status from 200 through 299. */
        readonly success: void;
        /** Assert a status from 400 through 599. */
        readonly error: void;
        /** Assert a status from 400 through 499. */
        readonly clientError: void;
        /** Assert a status from 500 through 599. */
        readonly serverError: void;
        /** Assert that the response body is valid JSON. */
        readonly json: void;
    }

    interface Response {
        readonly code: number;
        /** Canonical HTTP status text. */
        readonly status: string;
        /** Elapsed response time in milliseconds. */
        readonly responseTime: number;
        readonly headers: Headers;
        text(): string;
        /** Parse the response body as JSON. Invalid JSON throws. */
        json(): any;
        readonly to: {
            readonly have: ResponseHaveAssertions;
            readonly be: ResponseBeAssertions;
        };
    }

    type PropertyName = string | number | symbol;

    /** Lowercase type names reported by Object.prototype.toString. Matching is case-insensitive. */
    type TypeName =
        | "array"
        | "bigint"
        | "boolean"
        | "date"
        | "error"
        | "function"
        | "map"
        | "null"
        | "number"
        | "object"
        | "promise"
        | "regexp"
        | "set"
        | "string"
        | "symbol"
        | "undefined"
        | (string & {});

    interface IncludeAssertion extends Assertion {
        /** Assert string/array inclusion or a nonempty plain-object subset. */
        (expected: unknown, message?: string): Assertion;
    }

    interface TypeAssertion extends Assertion {
        /** Assert the JavaScript type name, such as string, array, object, date, or null. */
        (type: TypeName, message?: string): Assertion;
    }

    interface LengthAssertion extends Assertion {
        /** Assert an exact length, or chain a numeric comparison against the length. */
        (length: number, message?: string): Assertion;
    }

    /** Fluent assertions supported by Request Eagle. Assertion failures throw an Error. */
    interface Assertion {
        readonly to: Assertion;
        readonly be: Assertion;
        readonly been: Assertion;
        readonly is: Assertion;
        readonly that: Assertion;
        readonly which: Assertion;
        readonly and: Assertion;
        readonly has: Assertion;
        readonly have: Assertion;
        readonly with: Assertion;
        readonly at: Assertion;
        readonly of: Assertion;
        readonly same: Assertion;
        readonly but: Assertion;
        readonly does: Assertion;
        readonly still: Assertion;
        readonly also: Assertion;
        readonly not: Assertion;
        /** Compare plain objects, arrays, dates, and regular expressions recursively; cycles are unsupported. */
        readonly deep: Assertion;
        /** Interpret property names as paths, such as user.items[0].id. Cannot combine with own. */
        readonly nested: Assertion;
        /** Require an own property. Cannot combine with nested. */
        readonly own: Assertion;
        /** Compare members in their original order. */
        readonly ordered: Assertion;
        readonly any: Assertion;
        readonly all: Assertion;
        readonly true: Assertion;
        readonly false: Assertion;
        readonly null: Assertion;
        readonly undefined: Assertion;
        /** Assert a truthy value. */
        readonly ok: Assertion;
        /** Assert a value other than null or undefined. */
        readonly exist: Assertion;
        readonly exists: Assertion;
        readonly NaN: Assertion;
        readonly finite: Assertion;
        /** Assert an empty string, array, or plain object. */
        readonly empty: Assertion;
        equal(expected: unknown, message?: string): Assertion;
        equals(expected: unknown, message?: string): Assertion;
        eq(expected: unknown, message?: string): Assertion;
        /** Compare deeply without requiring the deep modifier. */
        eql(expected: unknown, message?: string): Assertion;
        readonly include: IncludeAssertion;
        readonly includes: IncludeAssertion;
        readonly contain: IncludeAssertion;
        readonly contains: IncludeAssertion;
        /** Assert a property, then continue asserting against its value. */
        property(name: PropertyName, value?: unknown, message?: string): Assertion;
        /** Assert the enumerable own keys. Use include for a subset, or any for at least one. */
        keys(keys: PropertyName[]): Assertion;
        keys(...keys: [PropertyName, ...PropertyName[]]): Assertion;
        /** Assert array members. Supports deep, include, and ordered modifiers. */
        members(expected: unknown[], message?: string): Assertion;
        /** Assert that the value matches one of the candidates. */
        oneOf(candidates: unknown[], message?: string): Assertion;
        readonly a: TypeAssertion;
        readonly an: TypeAssertion;
        above(bound: number, message?: string): Assertion;
        greaterThan(bound: number, message?: string): Assertion;
        below(bound: number, message?: string): Assertion;
        lessThan(bound: number, message?: string): Assertion;
        least(bound: number, message?: string): Assertion;
        gte(bound: number, message?: string): Assertion;
        most(bound: number, message?: string): Assertion;
        lte(bound: number, message?: string): Assertion;
        /** Assert an inclusive numeric range. */
        within(lower: number, upper: number, message?: string): Assertion;
        /** Assert a numeric distance no greater than the nonnegative delta. */
        closeTo(expected: number, delta: number, message?: string): Assertion;
        readonly lengthOf: LengthAssertion;
        readonly length: LengthAssertion;
        match(pattern: RegExp, message?: string): Assertion;
    }

    /** Common gRPC metadata keys. Any other key is also accepted. */
    type MetadataKey =
        | "authorization"
        | "content-type"
        | "grpc-encoding"
        | "grpc-timeout"
        | "user-agent"
        | "x-api-key"
        | "x-request-id"
        | (string & {});

    /** Metadata keys are case-insensitive. */
    type Metadata = Entries<MetadataKey>;

    type GrpcStatusName =
        | "OK"
        | "CANCELLED"
        | "UNKNOWN"
        | "INVALID_ARGUMENT"
        | "DEADLINE_EXCEEDED"
        | "NOT_FOUND"
        | "ALREADY_EXISTS"
        | "PERMISSION_DENIED"
        | "RESOURCE_EXHAUSTED"
        | "FAILED_PRECONDITION"
        | "ABORTED"
        | "OUT_OF_RANGE"
        | "UNIMPLEMENTED"
        | "INTERNAL"
        | "UNAVAILABLE"
        | "DATA_LOSS"
        | "UNAUTHENTICATED";

    /** A message sent or received during a gRPC call. */
    interface GrpcMessage {
        /** The message as JSON, with lowerCamelCase field names. 64-bit integers are strings. */
        readonly data: any;
        /** When the message was sent or received. */
        readonly timestamp: Date;
    }

    interface GrpcMessageAssertions {
        /** Assert that a message has these fields. Nested objects match the fields they name. */
        include(fields: unknown): void;
        readonly not: {
            /** Assert that no message has these fields. */
            include(fields: unknown): void;
        };
        readonly have: {
            /** Assert that every message has a property path such as user.id, optionally with this value. */
            property(path: string, value?: unknown): void;
            /** Assert that every message matches the schema. */
            jsonSchema(schema: JSONSchema): void;
        };
    }

    /** Messages in the order they were sent or received: an array with list helpers. */
    interface GrpcMessageList extends Array<GrpcMessage> {
        /** Return the message at an index; negative indexes count from the end. */
        idx(index: number): GrpcMessage | undefined;
        count(): number;
        /** Copy the messages into a plain array. */
        all(): GrpcMessage[];
        each(callback: (message: GrpcMessage, index: number) => void): void;
        filter<S extends GrpcMessage>(predicate: (message: GrpcMessage, index: number, messages: GrpcMessage[]) => message is S): S[];
        filter(predicate: (message: GrpcMessage, index: number, messages: GrpcMessage[]) => unknown): GrpcMessage[];
        /** Return the messages that have these fields, such as {data: {type: "ping"}}. */
        filter(fields: Record<string, unknown>): GrpcMessage[];
        readonly to: GrpcMessageAssertions;
    }

    /** The call as it was invoked. */
    interface GrpcRequest {
        /** The server address. */
        readonly url: string;
        /** The method as package.Service/Method. */
        readonly methodPath: string;
        readonly metadata: Metadata;
        /** The composed message as JSON text. */
        readonly message: string;
    }

    /** The call about to be invoked. Edits never modify the request draft or saved file. */
    interface GrpcInvokeRequest extends GrpcRequest {
        /** Assign a server address, which may contain {{variables}}. */
        url: string;
        /** Assign text or an object to change the message unary and server streaming methods send. */
        get message(): string;
        set message(value: string | object);
    }

    interface GrpcSentRequest extends GrpcRequest {
        /** The messages sent during the call, up to the latest 8 MiB. */
        readonly messages: GrpcMessageList;
    }

    interface GrpcResponseHaveAssertions {
        /** Assert the status code, such as 0 for OK. */
        statusCode(code: number): void;
        /** Assert the status code or name. */
        status(codeOrName: number | GrpcStatusName): void;
        /** Assert that the server sent this metadata; optionally compare its first value. */
        metadata(key: MetadataKey, value?: string): void;
        /** Assert that the server sent this trailer; optionally compare its first value. */
        trailer(key: MetadataKey, value?: string): void;
        /** Assert that a received message deeply equals this one. */
        message(expected: unknown): void;
    }

    interface GrpcResponseBeAssertions {
        /** Assert status 0 OK. */
        readonly ok: void;
        /** Assert a status other than OK. */
        readonly error: void;
    }

    interface GrpcResponse {
        /** The status code: 0 for OK. */
        readonly code: number;
        readonly status: GrpcStatusName;
        /** The server's status message, or an empty string. */
        readonly statusMessage: string;
        /** Milliseconds from invoking until the status arrived. */
        readonly responseTime: number;
        /** The server's initial metadata, its response headers. */
        readonly metadata: Metadata;
        readonly trailers: Metadata;
        /** The received messages, up to the latest 8 MiB. */
        readonly messages: GrpcMessageList;
        readonly to: {
            readonly have: GrpcResponseHaveAssertions;
            readonly be: GrpcResponseBeAssertions;
        };
    }

    /** The callback receives either an error or a response. Thrown errors reject sendRequest. */
    type RequestCallback = (error: Error | null, response: Response | null) => unknown;

    interface CommonAPI {
        readonly variables: LocalVariables;
        readonly environment: EnvironmentVariables;
        readonly crypto: Crypto;
        readonly encoding: Encoding;
        readonly schema: SchemaAPI;
        /**
         * Send an additional HTTP request using the current execution settings.
         * @param config URL for a GET, or request options. Variables resolve when the call starts.
         */
        sendRequest(config: string | RequestOptions): Promise<Response>;
        /**
         * Send an additional HTTP request and await the callback when it returns a promise.
         * @param config URL for a GET, or request options.
         * @param callback Receives (null, response) on success or (error, null) on failure.
         */
        sendRequest(config: string | RequestOptions, callback?: RequestCallback): Promise<Response | undefined>;
        /** Start a fluent assertion; an optional message prefixes assertion failures. */
        expect(actual: unknown, message?: string): Assertion;
        /** Run an async test. A rejected promise is reported as a failed test. */
        test(name: string, callback: () => PromiseLike<unknown>): Promise<void>;
        /** Run a synchronous test. Thrown assertions are reported without stopping the script. */
        test(name: string, callback: () => unknown): void;
    }

    interface PreRequestAPI extends CommonAPI {
        readonly request: Request;
        readonly execution: {
            /** Stop the pre-request script and skip sending the primary request with a visible reason. */
            skipRequest(reason?: string): never;
        };
    }

    interface PostResponseAPI extends CommonAPI {
        readonly request: Request;
        readonly response: Response;
    }

    interface GrpcBeforeInvokeAPI extends CommonAPI {
        readonly request: GrpcInvokeRequest;
        readonly execution: {
            /** Stop the script and do not invoke the method, with a visible reason. */
            skipRequest(reason?: string): never;
        };
    }

    interface GrpcOnMessageAPI extends CommonAPI {
        readonly request: GrpcRequest;
        /** The message the server just sent. */
        readonly message: GrpcMessage;
    }

    interface GrpcAfterResponseAPI extends CommonAPI {
        readonly request: GrpcSentRequest;
        readonly response: GrpcResponse;
    }

    interface Console {
        log(...values: unknown[]): void;
        info(...values: unknown[]): void;
        warn(...values: unknown[]): void;
        error(...values: unknown[]): void;
        debug(...values: unknown[]): void;
    }
}

/** Captured script output, shown in the response Console. */
declare const console: RequestEagle.Console;
