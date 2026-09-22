/// Remove JSON comments using Postman's whitespace:false behavior. Copy slices
/// without parsing JSON so templates, number spelling, and string escapes stay
/// unchanged. In particular, a CRLF ending a line comment retains only its LF.
pub(super) fn strip(input: &str) -> String {
    let bytes = input.as_bytes();
    let mut output = String::with_capacity(input.len());
    let mut inside_string = false;
    let mut start = 0;
    let mut index = 0;

    while index < bytes.len() {
        if bytes[index] == b'"' {
            let mut escape_start = index;

            while escape_start > 0 && bytes[escape_start - 1] == b'\\' {
                escape_start -= 1;
            }

            if (index - escape_start) % 2 == 0 {
                inside_string = !inside_string;
            }
        }

        if !inside_string && bytes[index..].starts_with(b"//") {
            output.push_str(&input[start..index]);
            index += 2;

            while index < bytes.len() && bytes[index] != b'\n' {
                index += 1;
            }

            start = index;
        } else if !inside_string && bytes[index..].starts_with(b"/*") {
            output.push_str(&input[start..index]);
            index += 2;

            while index < bytes.len() && !bytes[index..].starts_with(b"*/") {
                index += 1;
            }

            index = (index + 2).min(bytes.len());
            start = index;
        } else {
            index += 1;
        }
    }

    output.push_str(&input[start..]);
    output
}

/// Postman's matching is case sensitive and accepts JSON media-type suffixes.
pub(super) fn is_json_content_type(value: &str) -> bool {
    let Some(subtype) = value.strip_prefix("application/") else {
        return false;
    };

    subtype.starts_with("json")
        || subtype
            .match_indices("+json")
            .any(|(index, _)| index > 0 && !subtype[..index].chars().any(char::is_whitespace))
}
