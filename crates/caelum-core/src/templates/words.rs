pub(super) fn split(input: &str) -> Option<Vec<String>> {
    let mut chars = input.chars().peekable();
    let mut args = Vec::new();
    while let Some(&first) = chars.peek() {
        if first.is_whitespace() {
            chars.next();
            continue;
        }
        let mut arg = String::new();
        let mut quote = None;
        while let Some(c) = chars.next() {
            match c {
                _ if quote == Some(c) => quote = None,
                '\'' | '"' if quote.is_none() => quote = Some(c),
                '\\' if quote != Some('\'') => arg.push(chars.next()?),
                _ if quote.is_none() && c.is_whitespace() => break,
                _ => arg.push(c),
            }
        }
        if quote.is_some() {
            return None;
        }
        args.push(arg);
    }
    Some(args)
}
