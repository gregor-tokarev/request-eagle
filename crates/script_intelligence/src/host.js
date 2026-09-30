(function (libraryJson, declarations) {
    "use strict";

    // QuickJS has no Intl. The compiler only needs Collator for presentation
    // sorting; deterministic code-point order is sufficient for this menu.
    globalThis.Intl = {Collator: function () {
        this.compare = (left, right) => left < right ? -1 : left > right ? 1 : 0;
    }};

    const files = Object.assign(Object.create(null), JSON.parse(libraryJson));
    const libraries = Object.keys(files);
    const snapshots = new Map();
    let source = "", phase = "", version = 0, declarationVersion = 0;
    let lineStarts = [0];
    let isCancelled = () => false;
    const cancellationToken = {
        isCancellationRequested: () => isCancelled(),
        throwIfCancellationRequested() {
            if (isCancelled()) throw new ts.OperationCanceledException();
        },
    };
    const filename = "script.js";
    const options = {
        allowJs: true,
        checkJs: true,
        strict: true,
        noEmit: true,
        skipLibCheck: true,
        target: ts.ScriptTarget.ES2023,
        module: ts.ModuleKind.ESNext,
        moduleResolution: ts.ModuleResolutionKind.Classic,
        lib: libraries,
        types: [],
    };
    const host = {
        getCompilationSettings: () => options,
        getCancellationToken: () => cancellationToken,
        getScriptFileNames: () => [filename, "pm.d.ts"],
        getScriptVersion: name => String(name === filename ? version : name === "pm.d.ts" ? declarationVersion : 0),
        getScriptSnapshot(name) {
            if (files[name] === undefined) return undefined;
            if (!snapshots.has(name)) snapshots.set(name, ts.ScriptSnapshot.fromString(files[name]));
            return snapshots.get(name);
        },
        getCurrentDirectory: () => "",
        getDefaultLibFileName: () => "lib.es5.d.ts",
        fileExists: name => Object.hasOwn(files, name),
        readFile: name => files[name],
        readDirectory: () => [],
        getNewLine: () => "\n",
        useCaseSensitiveFileNames: () => true,
    };
    const service = ts.createLanguageService(host);
    const display = parts => ts.displayPartsToString(parts || []).slice(0, 8192);
    const markdown = text => text ? {kind: "markdown", value: text.slice(0, 8192)} : undefined;

    const lineBreakEscapes = {"\r": "\\r", "\n": "\\n", "\u2028": "\\u2028", "\u2029": "\\u2029"};
    const escapeLineBreaks = text => text.replace(/[\r\n\u2028\u2029]/g, ch => lineBreakEscapes[ch]);

    function completionDetail(parts, name, kind) {
        // A variable's inferred type can contain a property with the same name.
        // Only strip the owner prefix of an actual member/function completion.
        const member = ["property", "method", "getter", "setter", "function", "local function"].includes(kind)
            ? parts.findIndex(part => part.text === name &&
                ["propertyName", "methodName", "functionName"].includes(part.kind)) : -1;
        const property = member >= 0 && parts[member].kind === "propertyName";
        const preview = member < 0 ? parts : parts.slice(member + (property ? 1 : 0));
        let text = "", space = false;
        for (let index = 0; index < preview.length; index++) {
            // Omit our internal namespace, preserving string literal types.
            if (preview[index].kind === "moduleName" && preview[index].text === "RequestEagle"
                && preview[index + 1]?.text === ".") {
                index++;
                continue;
            }
            const part = preview[index];
            if (part.kind === "space" || part.kind === "lineBreak") {
                space = true;
                continue;
            }
            if (space && text) text += " ";
            text += escapeLineBreaks(part.text);
            space = false;
        }

        // Native completion rows have a uniform height. Anonymous object types
        // contain hard line breaks, which would overlap neighboring rows.
        // Collapse formatting whitespace only; spaces inside literal types and
        // quoted property names are meaningful and must remain unchanged.
        return (property ? text.replace(/^\??:\s*/, "") : text).slice(0, 8192);
    }

    const kinds = {
        method: 2, function: 3, constructor: 4, property: 10, field: 5,
        var: 6, let: 6, const: 21, class: 7, interface: 8, module: 9,
        enum: 13, "enum member": 20, keyword: 14, type: 25, parameter: 6,
        "local var": 6, "local function": 3, string: 12,
    };

    function positionAt(offset) {
        // The editor splits lines on LF. TypeScript also treats bare CR,
        // U+2028 and U+2029 as line breaks, including inside string literals.
        // Keep TS's UTF-16 offsets but map lines against the actual document.
        let low = 0, high = lineStarts.length;
        while (low < high) {
            const middle = (low + high) >>> 1;
            if (lineStarts[middle] <= offset) low = middle + 1;
            else high = middle;
        }
        const line = low - 1;
        return {line, character: offset - lineStarts[line]};
    }

    function range(span) {
        return {start: positionAt(span.start), end: positionAt(span.start + span.length)};
    }

    function complete(position) {
        const result = service.getCompletionsAtPosition(filename, position, {
            includeCompletionsForModuleExports: false,
            includeCompletionsWithInsertText: true,
            includeCompletionsWithSnippetText: false,
            includeCompletionsWithClassMemberSnippets: false,
            includeCompletionsWithObjectLiteralMethodSnippets: false,
        });
        if (!result) return [];

        const before = source.slice(0, position).match(/[$\p{ID_Continue}\u200c\u200d]*$/u)[0];
        const after = source.slice(position).match(/^[$\p{ID_Continue}\u200c\u200d]*/u)[0];
        const fallback = {start: position - before.length, length: before.length + after.length};
        const file = service.getProgram().getSourceFile(filename);
        if (result.isGlobalCompletion && !before) return [];
        const token = ts.getTokenAtPosition(file, Math.max(0, position - 1));
        const parent = token.parent;
        if (parent && parent.name === token && (
            ts.isParameter(parent) || ts.isVariableDeclaration(parent) ||
            ts.isFunctionDeclaration(parent) || ts.isFunctionExpression(parent) ||
            ts.isClassDeclaration(parent)
        )) return [];

        // A string value such as 'Content-T' has a prefix that includes
        // characters outside identifiers. Match it from the opening quote.
        const tokenStart = token.getStart(file);
        const typed = ts.isStringLiteralLike(token) && tokenStart < position &&
            (position < token.end || token.isUnterminated)
            ? source.slice(tokenStart + 1, position) : before;

        // Property names that are not identifiers, such as "Content-Type",
        // arrive quoted. Match and order them by the name the user types.
        const unquoted = name => name.replace(/^(["'])(.*)\1$/s, "$2");

        return result.entries
            // Without a typed prefix, only offer suggestions from the context.
            // Every global and keyword is valid after `{` or `(`, but listing
            // them hides the fields and values the API expects.
            .filter(entry => !entry.isSnippet && unquoted(entry.name).startsWith(typed) &&
                (typed || entry.sortText < ts.Completions.SortText.GlobalsOrKeywords))
            .sort((left, right) => left.sortText.localeCompare(right.sortText) ||
                unquoted(left.name).localeCompare(unquoted(right.name)))
            .slice(0, 100)
            .map(entry => {
                cancellationToken.throwIfCancellationRequested();
                const detail = service.getCompletionEntryDetails(filename, position, entry.name, {}, entry.source, {}, entry.data);
                const span = entry.replacementSpan || result.optionalReplacementSpan || fallback;
                return {
                    label: escapeLineBreaks(entry.name),
                    kind: kinds[entry.kind] || 1,
                    detail: detail ? completionDetail(detail.displayParts || [], entry.name, entry.kind) : undefined,
                    documentation: detail ? markdown(display(detail.documentation)) : undefined,
                    sortText: entry.sortText,
                    // GPUI's menu uses this as the highlighted prefix and does
                    // no filtering itself. Filtering is performed above.
                    filterText: typed,
                    textEdit: {range: range(span), newText: entry.insertText || entry.name},
                };
            });
    }

    function signature(position) {
        const result = service.getSignatureHelpItems(filename, position, undefined);
        if (!result) return null;

        const selected = result.items[result.selectedItemIndex];
        const items = result.items.length > 20 ? [selected] : result.items;

        return {
            activeSignature: result.items.length > 20 ? 0 : result.selectedItemIndex,
            activeParameter: Math.min(result.argumentIndex, Math.max(0, selected.parameters.length - 1)),
            signatures: items.map(item => {
                let label = display(item.prefixDisplayParts);
                const separator = display(item.separatorDisplayParts);
                const parameters = item.parameters.map((parameter, index) => {
                    if (index) label += separator;
                    const start = label.length;
                    label += display(parameter.displayParts);
                    return {label: [start, label.length], documentation: markdown(display(parameter.documentation))};
                });
                label += display(item.suffixDisplayParts);

                return {label, parameters, documentation: markdown(display(item.documentation))};
            }),
        };
    }

    function hover(position) {
        const result = service.getQuickInfoAtPosition(filename, position);
        if (!result) return null;

        const documentation = display(result.documentation);
        return {
            contents: markdown("```typescript\n" + display(result.displayParts) + "\n```" + (documentation ? "\n\n" + documentation : "")),
            range: range(result.textSpan),
        };
    }

    function query(nextSource, position, nextPhase, kind) {
        if (source !== nextSource) {
            source = files[filename] = nextSource;
            lineStarts = [0];
            for (let index = 0; index < source.length; index++) {
                if (source.charCodeAt(index) === 10) lineStarts.push(index + 1);
            }
            snapshots.delete(filename);
            version++;
        }
        if (phase !== nextPhase) {
            phase = nextPhase;
            files["pm.d.ts"] = declarations + "\ndeclare const pm: RequestEagle." +
                (phase === "post" ? "PostResponseAPI" : "PreRequestAPI") + ";\n";
            snapshots.delete("pm.d.ts");
            declarationVersion++;
        }

        const result = kind === "completions" ? complete(position)
            : kind === "signature" ? signature(position)
            : hover(position);
        return JSON.stringify(result);
    }

    return function cancellableQuery(nextSource, position, nextPhase, kind, cancellation) {
        isCancelled = cancellation || (() => false);
        try {
            cancellationToken.throwIfCancellationRequested();
            const result = query(nextSource, position, nextPhase, kind);
            cancellationToken.throwIfCancellationRequested();
            return result;
        } catch (error) {
            // This is TypeScript's supported cancellation path. Preserve its
            // reusable service/cache; unexpected exceptions still reach Rust.
            if (error instanceof ts.OperationCanceledException) return null;
            throw error;
        } finally {
            isCancelled = () => false;
        }
    };
})
