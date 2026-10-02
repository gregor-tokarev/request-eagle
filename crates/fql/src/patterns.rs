//! Regular expressions are written in JavaScript's syntax. This turns the
//! parts that differ into the `regex` crate's syntax; lookaround and
//! backreferences remain unsupported, and building them reports why.

pub(crate) fn translate(pattern: &str) -> String {
    let mut output = String::with_capacity(pattern.len());
    let mut chars = pattern.chars().peekable();
    let mut in_class = false;

    while let Some(ch) = chars.next() {
        match ch {
            '\\' => {
                let Some(escaped) = chars.next() else {
                    output.push_str("\\\\");
                    break;
                };
                // JavaScript's classes are ASCII; the regex crate's are Unicode.
                let class = match escaped {
                    'd' => Some(("0-9", false)),
                    'D' => Some(("0-9", true)),
                    'w' => Some(("A-Za-z0-9_", false)),
                    'W' => Some(("A-Za-z0-9_", true)),
                    _ => None,
                };
                match class {
                    Some((range, _)) if in_class => {
                        // A negated class inside a class keeps the regex
                        // crate's meaning.
                        if escaped.is_ascii_uppercase() {
                            output.push('\\');
                            output.push(escaped);
                        } else {
                            output.push_str(range);
                        }
                    }
                    Some((range, negated)) => {
                        output.push('[');
                        if negated {
                            output.push('^');
                        }
                        output.push_str(range);
                        output.push(']');
                    }
                    None => match escaped {
                        '/' => output.push('/'),
                        'u' => {
                            let digits: String = (0..4).filter_map(|_| chars.next()).collect();
                            output.push_str(&format!("\\x{{{digits}}}"));
                        }
                        other => {
                            output.push('\\');
                            output.push(other);
                        }
                    },
                }
            }
            '[' if !in_class => {
                in_class = true;
                output.push('[');
                // `[^]` matches any character in JavaScript.
                if chars.peek() == Some(&'^') {
                    chars.next();
                    if chars.peek() == Some(&']') {
                        chars.next();
                        output.push_str("\\s\\S]");
                        in_class = false;
                        continue;
                    }
                    output.push('^');
                }
                // A leading `]` is literal in the regex crate but closes an
                // empty class in JavaScript, which matches nothing.
                if chars.peek() == Some(&']') {
                    chars.next();
                    output.push_str("^\\s\\S]");
                    in_class = false;
                }
            }
            ']' if in_class => {
                in_class = false;
                output.push(']');
            }
            // The regex crate reads `[[` as a nested class, and `&&` and `~~`
            // in a class as set operations.
            '[' => output.push_str("\\["),
            '&' | '~' if in_class => {
                output.push('\\');
                output.push(ch);
            }
            _ => output.push(ch),
        }
    }

    output
}
