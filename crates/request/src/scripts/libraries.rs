use rquickjs::{Ctx, Function, Value, context::EvalOptions};

/// Unmodified browser builds of the libraries Postman scripts commonly
/// require; see libraries/SOURCES.md.
const LIBRARIES: &[(&str, &str)] = &[
    ("crypto-js", include_str!("libraries/crypto-js.js")),
    ("lodash", include_str!("libraries/lodash.js")),
    ("moment", include_str!("libraries/moment.js")),
];

/// Returns a library's CommonJS module function, which takes `module`,
/// `exports` and the `window` whose `crypto` its build looks for, or
/// `undefined` for an unknown name. Compiling only when a script requires a
/// library keeps other scripts fast.
pub(super) fn binding<'js>(cx: Ctx<'js>) -> rquickjs::Result<Function<'js>> {
    Function::new(cx, |cx: Ctx<'js>, name: String| {
        let Some((_, source)) = LIBRARIES.iter().find(|(library, _)| *library == name) else {
            return Ok(Value::new_undefined(cx));
        };

        let mut options = EvalOptions::default();
        // Like Node, run each library in the mode it declares.
        options.strict = false;
        options.filename = Some(format!("{name}.js"));

        cx.eval_with_options(
            format!("(function (module, exports, window) {{\n{source}\n}})"),
            options,
        )
    })
}
