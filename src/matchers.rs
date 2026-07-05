use glob::Pattern;

pub fn scope_matches(scopes: &[String], target: &str) -> bool {
    if scopes.is_empty()
        || scopes
            .iter()
            .any(|s| matches!(s.as_str(), "**" | "*" | "**/*"))
    {
        return true;
    }

    let target = target.trim_start_matches("./");
    scopes.iter().any(|scope| {
        let scope = scope.trim();
        if scope.is_empty() {
            return false;
        }
        Pattern::new(scope)
            .map(|p| p.matches(target))
            .unwrap_or(false)
            || literal_scope_matches(scope, target)
    })
}

fn literal_scope_matches(scope: &str, target: &str) -> bool {
    if scope.contains('*') || scope.contains('?') || scope.contains('[') {
        return false;
    }
    let scope = scope.trim_end_matches('/');
    target == scope || target.starts_with(&format!("{scope}/"))
}

pub fn command_pattern_matches(command: &str, pattern: &str) -> bool {
    let command_tokens = shell_tokens(command);
    let pattern_tokens = shell_tokens(pattern);
    if command_tokens.is_empty() || pattern_tokens.is_empty() {
        return false;
    }
    if pattern_tokens.len() > command_tokens.len() {
        return false;
    }
    command_tokens.windows(pattern_tokens.len()).any(|window| {
        window
            .iter()
            .zip(pattern_tokens.iter())
            .all(|(token, pattern)| command_token_matches(token, pattern))
    })
}

fn command_token_matches(token: &str, pattern: &str) -> bool {
    if pattern.contains('*') || pattern.contains('?') || pattern.contains('[') {
        return Pattern::new(pattern)
            .map(|p| p.matches(token))
            .unwrap_or(false);
    }
    token == pattern
}

fn shell_tokens(input: &str) -> Vec<String> {
    let mut tokens = Vec::new();
    let mut current = String::new();
    let mut quote: Option<char> = None;
    let mut escaped = false;

    for ch in input.chars() {
        if escaped {
            current.push(ch);
            escaped = false;
            continue;
        }
        if ch == '\\' {
            escaped = true;
            continue;
        }
        if let Some(q) = quote {
            if ch == q {
                quote = None;
            } else {
                current.push(ch);
            }
            continue;
        }
        match ch {
            '\'' | '"' => quote = Some(ch),
            c if c.is_whitespace() => {
                if !current.is_empty() {
                    tokens.push(std::mem::take(&mut current));
                }
            }
            ';' | '|' | '&' | '(' | ')' => {
                if !current.is_empty() {
                    tokens.push(std::mem::take(&mut current));
                }
                tokens.push(ch.to_string());
            }
            _ => current.push(ch),
        }
    }

    if !current.is_empty() {
        tokens.push(current);
    }
    tokens
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn command_matching_is_token_aware() {
        assert!(command_pattern_matches(
            "npm install --ignore-scripts",
            "npm install"
        ));
        assert!(command_pattern_matches("env FOO=1 npm test", "npm test"));
        assert!(!command_pattern_matches("pnpm install", "npm install"));
        assert!(!command_pattern_matches("npm-check-updates", "npm"));
    }
}
