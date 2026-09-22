/// Split a POSIX-style command without running a shell or expanding variables.
pub(super) fn words(input: &str) -> Result<Vec<String>, String> {
    let mut words = Vec::new();
    let mut word = String::new();
    let mut started = false;
    let mut quote = None;
    let mut chars = input.chars().peekable();

    while let Some(character) = chars.next() {
        match (quote, character) {
            (Some('\''), '\'') | (Some('"'), '"') => quote = None,
            (Some('\''), _) => word.push(character),
            (_, '\\') => {
                let escaped = chars
                    .next()
                    .ok_or("The cURL command ends with an incomplete escape.")?;

                if escaped == '\n' {
                    continue;
                }

                // In double quotes a backslash only escapes these shell characters.
                if quote == Some('"') && !matches!(escaped, '$' | '`' | '"' | '\\') {
                    word.push('\\');
                }

                word.push(escaped);
                started = true;
            }
            (_, '$' | '`') => {
                return Err(
                    "Shell expansion is not supported. Paste literal values in single quotes."
                        .into(),
                );
            }
            (Some('"'), _) => word.push(character),
            (None, '~') if !started => {
                return Err("Shell tilde expansion is not supported. Replace the home directory with its absolute path, or quote a literal tilde.".into());
            }
            (None, '\'' | '"') => {
                quote = Some(character);
                started = true;
            }
            (None, ';' | '|' | '&' | '<' | '>' | '(' | ')') => {
                return Err("Import accepts one cURL command, without shell operators. Quote URLs and values.".into());
            }
            (None, ' ' | '\t' | '\n') => {
                if started {
                    words.push(std::mem::take(&mut word));
                    started = false;
                }
            }
            _ => {
                word.push(character);
                started = true;
            }
        }
    }

    if quote.is_some() {
        return Err("The cURL command contains an unclosed quote.".into());
    }

    if started {
        words.push(word);
    }

    Ok(words)
}
