use serde_json::{Value, json};

use crate::{Bindings, evaluate, functions};

fn eval(expression: &str) -> Option<Value> {
    let input = json!({
        "name": "Hello World",
        "numbers": [3, 1, 2],
        "people": [{"name": "Ada", "age": 36}, {"name": "Alan", "age": 41}, {"name": "Bo", "age": 9}]
    });
    evaluate(expression, Some(&input), &Bindings::default())
        .unwrap_or_else(|error| panic!("{expression}: {error}"))
}

fn check(expression: &str, expected: Value) {
    assert_eq!(eval(expression), Some(expected), "{expression}");
}

fn undefined(expression: &str) {
    assert_eq!(eval(expression), None, "{expression}");
}

fn code(expression: &str) -> &'static str {
    match evaluate(expression, Some(&json!({})), &Bindings::default()) {
        Ok(value) => panic!("{expression} gave {value:?}"),
        Err(error) => error.code,
    }
}

#[test]
fn string_functions() {
    check("$string(5)", json!("5"));
    check("$string(0.1 + 0.2)", json!("0.3"));
    check("$string(1e21)", json!("1e+21"));
    check("$string(0.000001)", json!("0.000001"));
    check("$string(1e-7)", json!("1e-7"));
    check("$string(true)", json!("true"));
    check(
        "$string([1, 'two', {'a': null}])",
        json!("[1,\"two\",{\"a\":null}]"),
    );
    check(
        "$string({'a': [1]}, true)",
        json!("{\n  \"a\": [\n    1\n  ]\n}"),
    );
    check("$string($uppercase)", json!(""));
    undefined("$string(nothing)");
    check("name.$string()", json!("Hello World"));

    check("$length('ü€😀')", json!(3));
    check("$substring(name, 3)", json!("lo World"));
    check("$substring(name, 3, 5)", json!("lo Wo"));
    check("$substring(name, -4)", json!("orld"));
    check("$substring(name, -4, 2)", json!("or"));
    check("$substring(name, 0, 0)", json!(""));
    check("$substring('😀ab', 1)", json!("ab"));
    check("$substringBefore(name, ' ')", json!("Hello"));
    check("$substringAfter(name, ' ')", json!("World"));
    check("$substringAfter(name, 'x')", json!("Hello World"));
    check("$uppercase(name)", json!("HELLO WORLD"));
    check("$lowercase(name)", json!("hello world"));
    check("$trim('  a \n\t b  ')", json!("a b"));
    check("$pad('foo', 5)", json!("foo  "));
    check("$pad('5', -3, '0')", json!("005"));
    check("$pad('x', 4, 'ab')", json!("xaba"));
    check("$contains(name, 'World')", json!(true));
    check("$contains(name, /world/i)", json!(true));
    check("$split('a,b,c', ',')", json!(["a", "b", "c"]));
    check("$split('a, b,c', /,\\s*/)", json!(["a", "b", "c"]));
    check("$split('a,b,c', ',', 2)", json!(["a", "b"]));
    check("$split('abc', '')", json!(["a", "b", "c"]));
    check("$join(['a', 'b'], '-')", json!("a-b"));
    check("$join(people.name)", json!("AdaAlanBo"));
    assert_eq!(code("$join([1, 2])"), "T0412");
}

#[test]
fn regular_expression_functions() {
    check(
        "$match('ababbabbcc', /a(b+)/)",
        json!([
            {"match": "ab", "index": 0, "groups": ["b"]},
            {"match": "abb", "index": 2, "groups": ["bb"]},
            {"match": "abb", "index": 5, "groups": ["bb"]}
        ]),
    );
    check(
        "$match('abc123', /[0-9]+/)",
        json!({"match": "123", "index": 3, "groups": []}),
    );
    undefined("$match('abc', /[0-9]+/)");
    check(
        "$replace('John Smith', /(\\w+)\\s(\\w+)/, '$2, $1')",
        json!("Smith, John"),
    );
    check("$replace('265USD', /([0-9]+)USD/, '$$$1')", json!("$265"));
    check("$replace('abracadabra', 'a', 'o', 2)", json!("obrocadabra"));
    check(
        "$replace('temperature = 68F', /(-?\\d+(?:\\.\\d*)?)F\\b/, function($m) { $string($round(($number($m.groups[0]) - 32) * 5 / 9)) & 'C' })",
        json!("temperature = 20C"),
    );
    check("/^H/(name).match", json!("H"));
    assert_eq!(code("$replace('a', '', 'b')"), "D3010");
    assert_eq!(code("$match('a', /(?<=a)b/)"), "S0302");
}

#[test]
fn number_functions() {
    check("$number('42')", json!(42));
    check("$number('-1.5e3')", json!(-1500));
    check("$number('0x10')", json!(16));
    check("$number('0b101')", json!(5));
    check("$number(true)", json!(1));
    assert_eq!(code("$number('12abc')"), "D3030");
    assert_eq!(code("$number('01')"), "D3030");

    check("$abs(-3)", json!(3));
    check("$floor(-1.5)", json!(-2));
    check("$ceil(1.2)", json!(2));
    check("$round(2.5)", json!(2));
    check("$round(3.5)", json!(4));
    check("$round(-2.5)", json!(-2));
    check("$round(1.2345, 2)", json!(1.23));
    check("$round(1.005, 2)", json!(1));
    check("$round(125, -1)", json!(120));
    check("$power(2, 10)", json!(1024));
    check("$sqrt(16)", json!(4));
    assert_eq!(code("$sqrt(-1)"), "D3060");
    assert_eq!(code("$power(10, 400)"), "D3061");

    check("$sum(numbers)", json!(6));
    check("$sum([])", json!(0));
    check("$sum(5)", json!(5));
    check("$max(numbers)", json!(3));
    check("$min(numbers)", json!(1));
    check("$average(numbers)", json!(2));
    undefined("$max([])");
    check("$count(numbers)", json!(3));
    check("$count(nothing)", json!(0));
    check("$count('x')", json!(1));
    assert_eq!(code("$sum(['a'])"), "T0412");

    let random = eval("$random()").unwrap().as_f64().unwrap();
    assert!((0. ..1.).contains(&random));
}

#[test]
fn number_formatting() {
    check("$formatNumber(12345.6, '#,###.00')", json!("12,345.60"));
    check(
        "$formatNumber(12345678.9, '9,999.99')",
        json!("12,345,678.90"),
    );
    check("$formatNumber(1234.5678, '00.000e0')", json!("12.346e2"));
    check("$formatNumber(0.14, '01%')", json!("14%"));
    check("$formatNumber(0.4857, '###.###‰')", json!("485.7‰"));
    check("$formatNumber(123.9, '9999')", json!("0124"));
    check("$formatNumber(-6.5, '0.0')", json!("-6.5"));
    check("$formatNumber(-6.5, '0.0;(0.0)')", json!("(6.5)"));
    check(
        "$formatNumber(1234.5, '#.##0,00', {'decimal-separator': ',', 'grouping-separator': '.'})",
        json!("1.234,50"),
    );
    check(
        "$formatNumber(0.14, '###pm', {'per-mille': 'pm'})",
        json!("140pm"),
    );

    check("$formatBase(100, 2)", json!("1100100"));
    check("$formatBase(255, 16)", json!("ff"));
    check("$formatBase(-10, 3)", json!("-101"));
    assert_eq!(code("$formatBase(10, 40)"), "D3100");

    check(
        "$formatInteger(2789, 'w')",
        json!("two thousand, seven hundred and eighty-nine"),
    );
    check("$formatInteger(12, 'W')", json!("TWELVE"));
    check("$formatInteger(21, 'Ww')", json!("Twenty-One"));
    check("$formatInteger(3, 'w;o')", json!("third"));
    check("$formatInteger(1999, 'I')", json!("MCMXCIX"));
    check("$formatInteger(14, 'i')", json!("xiv"));
    check("$formatInteger(12, '000')", json!("012"));
    check("$formatInteger(1234567, '#,##0')", json!("1,234,567"));
    check("$formatInteger(22, '1;o')", json!("22nd"));
    check("$formatInteger(28, 'a')", json!("ab"));
    check("$formatInteger(1, 'A')", json!("A"));

    check(
        "$parseInteger('twelve thousand, four hundred and seventy-six', 'w')",
        json!(12476),
    );
    check("$parseInteger('MCMXCIX', 'I')", json!(1999));
    check("$parseInteger('12,345', '#,##0')", json!(12345));
    check("$parseInteger('ab', 'a')", json!(28));
}

#[test]
fn boolean_functions() {
    check("$boolean('')", json!(false));
    check("$boolean('0')", json!(true));
    check("$boolean(0)", json!(false));
    check("$boolean([])", json!(false));
    check("$boolean([0])", json!(false));
    check("$boolean([0, 1])", json!(true));
    check("$boolean({})", json!(false));
    check("$boolean({'a': 1})", json!(true));
    check("$boolean(null)", json!(false));
    check("$boolean($uppercase)", json!(false));
    undefined("$boolean(nothing)");
    check("$not(true)", json!(false));
    check("$exists(name)", json!(true));
    check("$exists(nothing)", json!(false));
    check("$exists(null)", json!(true));
}

#[test]
fn array_functions() {
    check("$append([1, 2], [3])", json!([1, 2, 3]));
    check("$append(1, 2)", json!([1, 2]));
    check("$append(nothing, [1])", json!([1]));
    check("$sort(numbers)", json!([1, 2, 3]));
    check("$sort(['b', 'a', 'C'])", json!(["C", "a", "b"]));
    check(
        "$sort(people, function($a, $b) { $a.age > $b.age }).name",
        json!(["Bo", "Ada", "Alan"]),
    );
    assert_eq!(code("$sort([1, 'a'])"), "D3070");
    check("$reverse(numbers)", json!([2, 1, 3]));
    check("$count($shuffle(numbers))", json!(3));
    check("$sort($shuffle(numbers))", json!([1, 2, 3]));
    check(
        "$distinct([1, 2, 1, {'a': 1}, {'a': 1}])",
        json!([1, 2, {"a": 1}]),
    );
    check("$zip([1, 2, 3], [4, 5])", json!([[1, 4], [2, 5]]));

    check("$map(numbers, function($v) { $v * 2 })", json!([6, 2, 4]));
    check("$map([1], function($v) { $v * 2 })", json!(2));
    check("$map(numbers, function($v, $i) { $i })", json!([0, 1, 2]));
    check("$map(['a', 'b'], $uppercase)", json!(["A", "B"]));
    check("$map(numbers, $string)", json!(["3", "1", "2"]));
    check("$filter(numbers, function($v) { $v > 1 })", json!([3, 2]));
    check(
        "$filter(people, function($p) { $p.age > 18 }).name",
        json!(["Ada", "Alan"]),
    );
    check("$single(numbers, function($v) { $v = 2 })", json!(2));
    assert_eq!(code("$single([1, 2], function($v) { $v > 0 })"), "D3138");
    assert_eq!(code("$single([1, 2], function($v) { $v > 5 })"), "D3139");
    check(
        "$reduce([1, 2, 3, 4], function($a, $b) { $a + $b })",
        json!(10),
    );
    check(
        "$reduce([1, 2, 3], function($a, $b) { $a * $b }, 10)",
        json!(60),
    );
    undefined("$reduce([], function($a, $b) { $a + $b })");
    assert_eq!(code("$reduce([1], function($a) { $a })"), "D3050");
}

#[test]
fn object_functions() {
    check("$keys({'a': 1, 'b': 2})", json!(["a", "b"]));
    check("$keys(people)", json!(["name", "age"]));
    check("$keys({'only': 1})", json!("only"));
    check("$lookup({'a': 1}, 'a')", json!(1));
    check("$lookup(people, 'name')", json!(["Ada", "Alan", "Bo"]));
    check("$spread({'a': 1, 'b': 2})", json!([{"a": 1}, {"b": 2}]));
    check(
        "$merge([{'a': 1, 'b': 1}, {'b': 2}])",
        json!({"a": 1, "b": 2}),
    );
    check(
        "$sift({'a': 1, 'b': 2}, function($v) { $v > 1 })",
        json!({"b": 2}),
    );
    check(
        "{'a': 1, 'b': 2}.$sift(function($v, $k) { $k = 'a' })",
        json!({"a": 1}),
    );
    check(
        "$each({'a': 1, 'b': 2}, function($v, $k) { $k & '=' & $v })",
        json!(["a=1", "b=2"]),
    );
    check("$type([])", json!("array"));
    check("$type(null)", json!("null"));
    check("$type($type)", json!("function"));
    check("$type('x')", json!("string"));
    undefined("$type(nothing)");
    check("$clone({'a': [1]})", json!({"a": [1]}));

    let error = evaluate("$error('Stop here')", None, &Bindings::default()).unwrap_err();
    assert_eq!(error.message, "Stop here");
    assert_eq!(code("$assert(1 = 2, 'Not equal')"), "D3141");
    undefined("$assert(1 = 1)");
}

#[test]
fn encoding_functions() {
    check(
        "$base64encode('myuser:mypass')",
        json!("bXl1c2VyOm15cGFzcw=="),
    );
    check(
        "$base64decode('bXl1c2VyOm15cGFzcw==')",
        json!("myuser:mypass"),
    );
    check("$encodeUrlComponent('?x=test')", json!("%3Fx%3Dtest"));
    check(
        "$encodeUrl('https://mozilla.org/?x=шеллы')",
        json!("https://mozilla.org/?x=%D1%88%D0%B5%D0%BB%D0%BB%D1%8B"),
    );
    check("$decodeUrlComponent('%3Fx%3Dtest')", json!("?x=test"));
    check(
        "$decodeUrl('https://mozilla.org/?x=%D1%88%D0%B5%D0%BB%D0%BB%D1%8B%2F')",
        json!("https://mozilla.org/?x=шеллы%2F"),
    );
}

#[test]
fn evaluates_expressions_in_strings() {
    check("$eval('[1, 2, 3]')", json!([1, 2, 3]));
    check("$eval('$sum(numbers)')", json!(6));
    check("$eval('a + 1', {'a': 2})", json!(3));
    assert_eq!(code("$eval('1 +')"), "D3120");
}

#[test]
fn postman_functions() {
    check("$jsonParse('{\"a\": [1, 2]}').a[1]", json!(2));
    assert_eq!(code("$jsonParse('{')"), "D3150");
    check("$json({'a': 'x'})", json!("{\"a\":\"x\"}"));
    check("$json('text')", json!("\"text\""));
    check(
        "$partition([1, 2, 3, 4, 5], 2)",
        json!([[1, 2], [3, 4], [5]]),
    );
    check("$constant('pi') > 3.14", json!(true));
    check("$isFinite(1)", json!(true));
    check("$cbrt(27)", json!(3));
    check("$round($log10(1000))", json!(3));
    check("$log2(8)", json!(3));
    check(
        "$round($atan2(1, 1) * 4, 5) = $round($constant('π'), 5)",
        json!(true),
    );

    let uuid = eval("$uuid()").unwrap();
    let uuid = uuid.as_str().unwrap();
    assert_eq!(uuid.len(), 36);
    assert_eq!(&uuid[14..15], "4");
    assert_ne!(eval("$uuid()"), eval("$uuid()"));
}

#[test]
fn lists_every_function_with_documentation() {
    let functions = functions();

    assert!(functions.len() > 90);
    for name in [
        "sum",
        "jsonParse",
        "fromMillis",
        "formatNumber",
        "map",
        "datePlus",
    ] {
        let function = functions
            .iter()
            .find(|function| function.name == name)
            .unwrap();
        assert!(function.signature.starts_with(&format!("${name}(")));
        assert!(!function.description.is_empty());
    }
}
