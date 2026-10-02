use serde_json::{Value, json};

use crate::{Bindings, Expression, evaluate};

/// The sample document of the JSONata documentation.
fn address() -> Value {
    json!({
        "FirstName": "Fred",
        "Surname": "Smith",
        "Age": 28,
        "Address": {"Street": "Hursley Park", "City": "Winchester", "Postcode": "SO21 2JN"},
        "Phone": [
            {"type": "home", "number": "0203 544 1234"},
            {"type": "office", "number": "01962 001234"},
            {"type": "office", "number": "01962 001235"},
            {"type": "mobile", "number": "077 7700 1234"}
        ],
        "Email": [
            {"type": "office", "address": ["fred.smith@my-work.com", "fsmith@my-work.com"]},
            {"type": "home", "address": ["freddy@my-social.com", "frederic.smith@very-serious.com"]}
        ],
        "Other": {
            "Over 18 ?": true,
            "Misc": null,
            "Alternative.Address": {"Street": "Brick Lane", "City": "London", "Postcode": "E1 6RF"}
        }
    })
}

fn account() -> Value {
    json!({
        "Account": {
            "Account Name": "Firefly",
            "Order": [
                {
                    "OrderID": "order103",
                    "Product": [
                        {"Product Name": "Bowler Hat", "ProductID": 858383, "SKU": "0406654608",
                         "Description": {"Colour": "Purple", "Width": 300, "Height": 200, "Depth": 210, "Weight": 0.75},
                         "Price": 34.45, "Quantity": 2},
                        {"Product Name": "Trilby hat", "ProductID": 858236, "SKU": "0406634348",
                         "Description": {"Colour": "Orange", "Width": 300, "Height": 200, "Depth": 210, "Weight": 0.6},
                         "Price": 21.67, "Quantity": 1}
                    ]
                },
                {
                    "OrderID": "order104",
                    "Product": [
                        {"Product Name": "Bowler Hat", "ProductID": 858383, "SKU": "040657863",
                         "Description": {"Colour": "Purple", "Width": 300, "Height": 200, "Depth": 210, "Weight": 0.75},
                         "Price": 34.45, "Quantity": 4},
                        {"ProductID": 345664, "SKU": "0406654603", "Product Name": "Cloak",
                         "Description": {"Colour": "Black", "Width": 30, "Height": 20, "Depth": 210, "Weight": 2},
                         "Price": 107.99, "Quantity": 1}
                    ]
                }
            ]
        }
    })
}

fn eval(expression: &str, input: &Value) -> Option<Value> {
    evaluate(expression, Some(input), &Bindings::default())
        .unwrap_or_else(|error| panic!("{expression}: {error}"))
}

fn check(expression: &str, input: &Value, expected: Value) {
    assert_eq!(eval(expression, input), Some(expected), "{expression}");
}

fn undefined(expression: &str, input: &Value) {
    assert_eq!(eval(expression, input), None, "{expression}");
}

fn code(expression: &str, input: &Value) -> &'static str {
    match evaluate(expression, Some(input), &Bindings::default()) {
        Ok(value) => panic!("{expression} gave {value:?}"),
        Err(error) => error.code,
    }
}

#[test]
fn navigates_objects_and_arrays() {
    let input = address();

    check("Surname", &input, json!("Smith"));
    check("Age", &input, json!(28));
    check("Address.City", &input, json!("Winchester"));
    check("Other.Misc", &input, json!(null));
    undefined("Other.Nothing", &input);
    check("Other.`Over 18 ?`", &input, json!(true));
    check(
        "Other.\"Alternative.Address\".City",
        &input,
        json!("London"),
    );

    check(
        "Phone[0]",
        &input,
        json!({"type": "home", "number": "0203 544 1234"}),
    );
    check("Phone[1].number", &input, json!("01962 001234"));
    check("Phone[-1].type", &input, json!("mobile"));
    check("Phone[-2].number", &input, json!("01962 001235"));
    undefined("Phone[8]", &input);
    check("Phone[0.7].type", &input, json!("home"));

    check(
        "Phone.number",
        &input,
        json!([
            "0203 544 1234",
            "01962 001234",
            "01962 001235",
            "077 7700 1234"
        ]),
    );
    // A filter applies to each step's items, unless the path is grouped.
    check(
        "Phone.number[0]",
        &input,
        json!([
            "0203 544 1234",
            "01962 001234",
            "01962 001235",
            "077 7700 1234"
        ]),
    );
    check("(Phone.number)[0]", &input, json!("0203 544 1234"));
    check("Phone[[0..1]].type", &input, json!(["home", "office"]));
    check("Phone[[0, 3]].type", &input, json!(["home", "mobile"]));
}

#[test]
fn filters_with_predicates() {
    let input = address();

    check(
        "Phone[type='mobile'].number",
        &input,
        json!("077 7700 1234"),
    );
    check(
        "Phone[type='office'].number",
        &input,
        json!(["01962 001234", "01962 001235"]),
    );
    check(
        "Phone[type='office' and $contains(number, '35')].number",
        &input,
        json!("01962 001235"),
    );
    undefined("Phone[type='fax']", &input);
    check(
        "Email[type='home'].address[1]",
        &input,
        json!("frederic.smith@very-serious.com"),
    );
    check(
        "Email.address",
        &input,
        json!([
            "fred.smith@my-work.com",
            "fsmith@my-work.com",
            "freddy@my-social.com",
            "frederic.smith@very-serious.com"
        ]),
    );
    check(
        "Phone[type='mobile'][0].number",
        &input,
        json!("077 7700 1234"),
    );
}

#[test]
fn keeps_singleton_arrays_with_empty_brackets() {
    let input = address();

    check(
        "Phone[type='mobile'].number[]",
        &input,
        json!(["077 7700 1234"]),
    );
    check("Address.City[]", &input, json!(["Winchester"]));
    check("$[0]", &json!([1, 2]), json!(1));
    check("[Address.City]", &input, json!(["Winchester"]));
}

#[test]
fn wildcards_and_descendants() {
    let input = address();

    check(
        "Address.*",
        &input,
        json!(["Hursley Park", "Winchester", "SO21 2JN"]),
    );
    check("*.Postcode", &input, json!("SO21 2JN"));
    check("**.Postcode", &input, json!(["SO21 2JN", "E1 6RF"]));
    check("$count(Phone.*)", &input, json!(8));
}

#[test]
fn computes_with_the_account_document() {
    let input = account();

    check(
        "Account.Order.Product.Price",
        &input,
        json!([34.45, 21.67, 34.45, 107.99]),
    );
    check(
        "Account.Order.OrderID",
        &input,
        json!(["order103", "order104"]),
    );
    check("$count(Account.Order.Product)", &input, json!(4));
    check(
        "$round($sum(Account.Order.Product.(Price * Quantity)), 2)",
        &input,
        json!(336.36),
    );
    check(
        "Account.Order.Product^(>Price).`Product Name`",
        &input,
        json!(["Cloak", "Bowler Hat", "Bowler Hat", "Trilby hat"]),
    );
    check(
        "Account.Order.Product^(`Product Name`, >Quantity).Quantity",
        &input,
        json!([4, 2, 1, 1]),
    );
    check(
        "Account.Order.Product{`Product Name`: Price}",
        &input,
        json!({"Bowler Hat": [34.45, 34.45], "Trilby hat": 21.67, "Cloak": 107.99}),
    );
    check(
        "Account.Order.Product{`Product Name`: $sum(Quantity)}",
        &input,
        json!({"Bowler Hat": 6, "Trilby hat": 1, "Cloak": 1}),
    );
    check(
        "$distinct(Account.Order.Product.Description.Colour)",
        &input,
        json!(["Purple", "Orange", "Black"]),
    );
    check(
        "Account.Order[0].Product.{'name': `Product Name`, 'total': Price * Quantity}",
        &input,
        json!([{"name": "Bowler Hat", "total": 68.9}, {"name": "Trilby hat", "total": 21.67}]),
    );
}

#[test]
fn binds_positions_and_contexts_in_paths() {
    let input = account();

    check(
        "Account.Order#$o.Product.{'order': $o, 'name': `Product Name`}",
        &input,
        json!([
            {"order": 0, "name": "Bowler Hat"},
            {"order": 0, "name": "Trilby hat"},
            {"order": 1, "name": "Bowler Hat"},
            {"order": 1, "name": "Cloak"}
        ]),
    );
    // `@` keeps the context, which joins siblings.
    let library = json!({"library": {
        "loans": [{"isbn": 2, "customer": "Ada"}, {"isbn": 1, "customer": "Bo"}],
        "books": [{"isbn": 1, "title": "Flows"}, {"isbn": 2, "title": "Queries"}]
    }});
    check(
        "library.loans@$l.books@$b[$l.isbn = $b.isbn].{'title': $b.title, 'customer': $l.customer}",
        &library,
        json!([{"title": "Queries", "customer": "Ada"}, {"title": "Flows", "customer": "Bo"}]),
    );
    check(
        "library.loans@$l.books@$b[$l.isbn = $b.isbn]{$b.title: $l.customer}",
        &library,
        json!({"Queries": "Ada", "Flows": "Bo"}),
    );
    check(
        "Account.Order.Product[Price > 30]#$i.{'i': $i, 'price': Price}",
        &input,
        json!([{"i": 0, "price": 34.45}, {"i": 1, "price": 34.45}, {"i": 2, "price": 107.99}]),
    );
}

#[test]
fn constructs_arrays_and_objects() {
    let input = address();

    check("[1, 2, 3]", &input, json!([1, 2, 3]));
    check("[]", &input, json!([]));
    check("[[1, 2], [3]]", &input, json!([[1, 2], [3]]));
    check("[1..5]", &input, json!([1, 2, 3, 4, 5]));
    check("[1..3, 7]", &input, json!([1, 2, 3, 7]));
    check("[3..1]", &input, json!([]));
    check(
        "[Phone.type]",
        &input,
        json!(["home", "office", "office", "mobile"]),
    );
    check("[1, nothing, 2]", &input, json!([1, 2]));
    check("{'a': 1, 'b': [Age]}", &input, json!({"a": 1, "b": [28]}));
    check("{'a': nothing}", &input, json!({}));
    check(
        "Phone{type: number}",
        &input,
        json!({"home": "0203 544 1234", "office": ["01962 001234", "01962 001235"], "mobile": "077 7700 1234"}),
    );
    check(
        "Phone{type: number[]}",
        &input,
        json!({"home": ["0203 544 1234"], "office": ["01962 001234", "01962 001235"], "mobile": ["077 7700 1234"]}),
    );
    check(
        "(Phone.{type: number})[0]",
        &input,
        json!({"home": "0203 544 1234"}),
    );
    assert_eq!(code("{1: 2}", &input), "T1003");
    assert_eq!(code("Phone{type: 1, 'home': 2}", &input), "D1009");
    assert_eq!(code("[0..10000000]", &input), "D2014");
    assert_eq!(
        code("[-100000000000000000000..100000000000000000000]", &input),
        "D2014"
    );
}

#[test]
fn applies_operators_with_precedence() {
    let input = json!({"a": 5, "b": "text", "list": [1, 2]});

    check("1 + 2 * 3", &input, json!(7));
    check("(1 + 2) * 3", &input, json!(9));
    check("10 / 4", &input, json!(2.5));
    check("-5 % 3", &input, json!(-2));
    check("-a", &input, json!(-5));
    check("2 * -a", &input, json!(-10));
    undefined("nothing + 1", &input);
    assert_eq!(code("b + 1", &input), "T2001");
    assert_eq!(code("1 + b", &input), "T2002");
    assert_eq!(code("1 / 0", &input), "D1001");
    assert_eq!(code("-b", &input), "D1002");

    check("1 < 2", &input, json!(true));
    check("'a' < 'b'", &input, json!(true));
    check("a >= 5 and a <= 5", &input, json!(true));
    undefined("nothing < 1", &input);
    assert_eq!(code("1 < '2'", &input), "T2009");
    assert_eq!(code("list < 2", &input), "T2010");

    check(
        "{'x': [1, {'y': 2}]} = {'x': [1, {'y': 2}]}",
        &input,
        json!(true),
    );
    check("[1, 2] != [2, 1]", &input, json!(true));
    check("nothing = nothing", &input, json!(false));
    check("nothing != 1", &input, json!(false));
    check("null = null", &input, json!(true));

    check("2 in list", &input, json!(true));
    check("5 in a", &input, json!(true));
    check("{'a': 1} in [{'a': 1}]", &input, json!(false));
    check("nothing in list", &input, json!(false));

    check("true and nothing", &input, json!(false));
    check("nothing or 1", &input, json!(true));
    check("false or false and true", &input, json!(false));

    check(
        "'a' & 1 & true & null & [1, 2] & {'k': 'v'}",
        &input,
        json!("a1truenull[1,2]{\"k\":\"v\"}"),
    );
    check("nothing & 'x'", &input, json!("x"));
    check("0.1 + 0.2 & ''", &input, json!("0.3"));

    check("a > 2 ? 'big' : 'small'", &input, json!("big"));
    undefined("false ? 1", &input);
    check("nothing ?? 'default'", &input, json!("default"));
    check("0 ?? 'default'", &input, json!(0));
    check("0 ?: 'default'", &input, json!("default"));
    check("a ?: 'default'", &input, json!(5));
}

#[test]
fn binds_variables_in_blocks() {
    let input = json!({"price": 10});

    check("($tax := 0.2; price * (1 + $tax))", &input, json!(12));
    check("($a := 1; ($a := 2); $a)", &input, json!(1));
    check("($a := $b := 3; $a + $b)", &input, json!(6));
    check("$", &input, json!({"price": 10}));
    check("$$.price", &input, json!(10));
    check("[1, 2, 3].($ * 2)", &input, json!([2, 4, 6]));
    undefined("()", &input);

    let bindings: Bindings = [("rate".to_owned(), json!(3))].into_iter().collect();
    assert_eq!(
        evaluate("price * $rate", Some(&input), &bindings).unwrap(),
        Some(json!(30))
    );
}

#[test]
fn defines_and_calls_functions() {
    let input = json!({"names": ["ada", "alan"]});

    check(
        "($double := function($x) { $x * 2 }; $double(21))",
        &input,
        json!(42),
    );
    check("($inc := λ($x) { $x + 1 }; $inc(1))", &input, json!(2));
    check("($inc := fn($x) { $x + 1 }; $inc(1))", &input, json!(2));
    check(
        "($add := function($x) { function($y) { $x + $y } }; $add(2)(3))",
        &input,
        json!(5),
    );
    check(
        "($fact := function($n) { $n <= 1 ? 1 : $n * $fact($n - 1) }; $fact(10))",
        &input,
        json!(3628800),
    );
    check("names.$uppercase()", &input, json!(["ADA", "ALAN"]));
    check(
        "($first := $substring(?, 0, 2); names.$first($))",
        &input,
        json!(["ad", "al"]),
    );
    check("'hello' ~> $uppercase()", &input, json!("HELLO"));
    check(
        "[3, 1, 2] ~> $sort() ~> $reverse()",
        &input,
        json!([3, 2, 1]),
    );
    check(
        "($shout := $trim ~> $uppercase; $shout('  hi '))",
        &input,
        json!("HI"),
    );
    check(
        "($map := function($f) { function($a) { $a.$f($) } }; $map($uppercase)(names))",
        &input,
        json!(["ADA", "ALAN"]),
    );
    check("function($x) { $x }(7)", &input, json!(7));
    undefined("($f := function($x, $y) { $y }; $f(1))", &input);
}

#[test]
fn reports_calls_of_things_that_are_not_functions() {
    let input = json!({"a": 1});

    assert_eq!(code("$nothing(1)", &input), "T1006");
    assert_eq!(code("uppercase('a')", &input), "T1005");
    assert_eq!(code("a ~> 1", &input), "T2006");
}

#[test]
fn transforms_copies_of_objects() {
    let input =
        json!({"order": {"items": [{"id": 1, "price": 5}, {"id": 2, "price": 7}], "secret": "x"}});

    check(
        "$ ~> |order.items|{'price': price * 2}|",
        &input,
        json!({"order": {"items": [{"id": 1, "price": 10}, {"id": 2, "price": 14}], "secret": "x"}}),
    );
    check(
        "$ ~> |order|{}, ['secret']|",
        &input,
        json!({"order": {"items": [{"id": 1, "price": 5}, {"id": 2, "price": 7}]}}),
    );
    // The input itself is unchanged.
    check("order.items[0].price", &input, json!(5));
    assert_eq!(code("$ ~> |order|5|", &input), "T2011");
}

#[test]
fn reads_flow_variables_as_fields() {
    let input = json!({
        "value1": {"http": {"status": 200}, "body": {"users": [{"name": "Ada", "active": true}, {"name": "Bo", "active": false}]}},
        "search": "Ada"
    });

    check("value1.http.status = 200", &input, json!(true));
    check("value1.body.users[active].name", &input, json!("Ada"));
    check(
        "value1.body.users[name = $$.search].active",
        &input,
        json!(true),
    );
    check(
        "($s := search; value1.body.users[name = $s].active)",
        &input,
        json!(true),
    );
}

#[test]
fn evaluates_arrays_as_one_input() {
    let input = json!([{"a": 1}, {"a": 2}]);

    check("a", &input, json!([1, 2]));
    check("$[1].a", &input, json!(2));
    check("$count($)", &input, json!(2));
    check("$$[0]", &input, json!({"a": 1}));
}

#[test]
fn stops_runaway_evaluations_with_errors() {
    // A thread with a small stack proves deep recursion fails cleanly.
    let result = std::thread::Builder::new()
        .stack_size(2 * 1024 * 1024)
        .spawn(|| {
            let input = json!({});
            let deep = code(
                "($f := function($n) { $n = 0 ? 0 : 1 + $f($n - 1) }; $f(100000))",
                &input,
            );
            let endless = code("($f := function($x) { $f($x) }; $f(1))", &input);
            let nested = Expression::parse(&format!("{}1{}", "(".repeat(5000), ")".repeat(5000)))
                .unwrap_err()
                .code;
            (deep, endless, nested)
        })
        .unwrap()
        .join()
        .unwrap();

    assert_eq!(result, ("D1011", "D1012", "S0217"));
}

#[test]
fn tail_calls_recurse_without_nesting() {
    check(
        "($loop := function($n, $sum) { $n = 0 ? $sum : $loop($n - 1, $sum + $n) }; $loop(10000, 0))",
        &json!({}),
        json!(50005000),
    );
}

#[test]
fn expressions_are_reusable_and_sendable() {
    fn sendable<T: Send + Sync>(_: &T) {}

    let expression = Expression::parse("value * 2").unwrap();
    sendable(&expression);
    for value in [1, 2, 3] {
        assert_eq!(
            expression
                .evaluate(Some(&json!({"value": value})), &Bindings::default())
                .unwrap(),
            Some(json!(value * 2))
        );
    }
    assert_eq!(
        expression.evaluate(None, &Bindings::default()).unwrap(),
        None
    );
}

#[test]
fn transforms_many_records_quickly() {
    let items: Vec<Value> = (0..60_000).map(|id| json!({"id": id})).collect();
    let input = json!({ "items": items });
    let started = std::time::Instant::now();

    let result = eval("$ ~> |items|{'active': true}|", &input).unwrap();

    assert_eq!(
        result["items"][59_999],
        json!({"id": 59_999, "active": true})
    );
    assert!(started.elapsed().as_secs() < 10, "{:?}", started.elapsed());
}
