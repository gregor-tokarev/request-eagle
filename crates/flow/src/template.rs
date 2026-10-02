//! The Mustache subset Template blocks fill: `{{name}}`, dotted names,
//! `{{.}}`, sections, inverted sections and comments. Values are not
//! HTML-escaped, and objects and lists render as JSON.

use serde_json::Value;

enum Node {
    Text(String),
    Value(String),
    Section {
        name: String,
        inverted: bool,
        children: Vec<Node>,
    },
}

pub(crate) fn render(template: &str, data: &Value) -> Result<String, String> {
    let (nodes, rest) = parse(template, None)?;
    debug_assert!(rest.is_empty());

    let mut output = String::new();
    let mut stack = vec![data];
    render_nodes(&nodes, &mut stack, &mut output);

    Ok(output)
}

/// Parse until the end of the template, or the closing tag of `section`.
/// Returns the nodes and what follows the closing tag.
fn parse<'a>(mut source: &'a str, section: Option<&str>) -> Result<(Vec<Node>, &'a str), String> {
    let mut nodes = Vec::new();

    while let Some(start) = source.find("{{") {
        if start > 0 {
            nodes.push(Node::Text(source[..start].to_owned()));
        }

        let (tag, rest) = if let Some(tag) = source[start..].strip_prefix("{{{") {
            let end = tag.find("}}}").ok_or("A {{{ tag is not closed with }}}")?;
            (format!("&{}", &tag[..end]), &tag[end + 3..])
        } else {
            let tag = &source[start + 2..];
            let end = tag.find("}}").ok_or("A {{ tag is not closed with }}")?;
            (tag[..end].to_owned(), &tag[end + 2..])
        };
        source = rest;

        let tag = tag.trim();
        match tag.chars().next() {
            Some('!') => {}
            Some(sigil @ ('#' | '^')) => {
                let name = tag[1..].trim().to_owned();
                let (children, rest) = parse(source, Some(&name))?;
                source = rest;
                nodes.push(Node::Section {
                    name,
                    inverted: sigil == '^',
                    children,
                });
            }
            Some('/') => {
                let name = tag[1..].trim();
                return match section {
                    Some(open) if open == name => Ok((nodes, source)),
                    Some(open) => Err(format!(
                        "Section {{{{#{open}}}}} is closed by {{{{/{name}}}}}"
                    )),
                    None => Err(format!("{{{{/{name}}}}} closes no section")),
                };
            }
            Some('&') => nodes.push(Node::Value(tag[1..].trim().to_owned())),
            _ => nodes.push(Node::Value(tag.to_owned())),
        }
    }

    if let Some(open) = section {
        return Err(format!("Section {{{{#{open}}}}} is not closed"));
    }
    if !source.is_empty() {
        nodes.push(Node::Text(source.to_owned()));
    }

    Ok((nodes, ""))
}

fn render_nodes<'a>(nodes: &'a [Node], stack: &mut Vec<&'a Value>, output: &mut String) {
    for node in nodes {
        match node {
            Node::Text(text) => output.push_str(text),
            Node::Value(name) => {
                if let Some(value) = lookup(stack, name) {
                    push_value(value, output);
                }
            }
            Node::Section {
                name,
                inverted,
                children,
            } => {
                let value = lookup(stack, name);
                let truthy = value.is_some_and(|value| match value {
                    Value::Null | Value::Bool(false) => false,
                    Value::Array(items) => !items.is_empty(),
                    Value::String(text) => !text.is_empty(),
                    _ => true,
                });

                if *inverted {
                    if !truthy {
                        render_nodes(children, stack, output);
                    }
                    continue;
                }

                match value {
                    Some(Value::Array(items)) => {
                        for item in items {
                            stack.push(item);
                            render_nodes(children, stack, output);
                            stack.pop();
                        }
                    }
                    Some(value) if truthy => {
                        stack.push(value);
                        render_nodes(children, stack, output);
                        stack.pop();
                    }
                    _ => {}
                }
            }
        }
    }
}

/// A dotted name's first part is looked up from the innermost context out.
fn lookup<'a>(stack: &[&'a Value], name: &str) -> Option<&'a Value> {
    if name == "." {
        return stack.last().copied();
    }

    let mut parts = name.split('.');
    let first = parts.next()?;
    let mut value = stack
        .iter()
        .rev()
        .find_map(|context| child(context, first))?;
    for part in parts {
        value = child(value, part)?;
    }

    Some(value)
}

fn child<'a>(value: &'a Value, key: &str) -> Option<&'a Value> {
    match value {
        Value::Object(object) => object.get(key),
        Value::Array(items) => items.get(key.parse::<usize>().ok()?),
        _ => None,
    }
}

fn push_value(value: &Value, output: &mut String) {
    match value {
        Value::Null => {}
        Value::String(text) => output.push_str(text),
        value => output.push_str(&value.to_string()),
    }
}
