use std::path::Path;

pub fn generate_from_idl(source: &str) -> Result<String, String> {
    let mut module: Option<String> = None;
    let mut declarations: Vec<String> = Vec::new();
    for (line_index, raw_line) in source.lines().enumerate() {
        let line = raw_line.split('#').next().map_or("", str::trim);
        if line.is_empty() {
            continue;
        }
        if let Some(name) = line.strip_prefix("module ") {
            if module.is_some() {
                return Err(format!("line {}: duplicate module declaration", line_index + 1));
            }
            validate_pascal(name.trim(), line_index + 1)?;
            module = Some(name.trim().to_string());
        } else if let Some(declaration) = line.strip_prefix("type ") {
            declarations.push(render_type(declaration.trim(), line_index + 1)?);
        } else if let Some(declaration) = line.strip_prefix("fun ") {
            declarations.push(render_function(declaration.trim(), line_index + 1)?);
        } else {
            return Err(format!("line {}: expected module, type, or fun declaration", line_index + 1));
        }
    }
    let module_name = match module {
        Some(name) => name,
        None => return Err("native IDL requires one module declaration".to_string()),
    };
    if declarations.is_empty() {
        return Err("native IDL requires at least one type or function".to_string());
    }
    let mut output = format!("#! Generated native surface. Ownership and implementation are supplied by the host.\npub mod {}\n", module_name);
    for declaration in declarations {
        output.push_str("  ");
        output.push_str(&declaration);
        output.push('\n');
    }
    output.push_str("end\n");
    Ok(output)
}

pub fn generate_file(input: &Path, output: &Path) -> Result<(), String> {
    let source = std::fs::read_to_string(input)
        .map_err(|read_error| format!("cannot read native IDL '{}': {}", input.display(), read_error))?;
    let generated = generate_from_idl(&source)?;
    std::fs::write(output, generated)
        .map_err(|write_error| format!("cannot write generated native surface '{}': {}", output.display(), write_error))
}

fn render_type(declaration: &str, line: usize) -> Result<String, String> {
    let (name, parameters) = split_generic_name(declaration, line)?;
    validate_pascal(name, line)?;
    validate_type_parameters(parameters, line)?;
    let constructor_parameters = if parameters.is_empty() {
        String::new()
    } else {
        let body = parameters
            .strip_prefix('<')
            .and_then(|text| text.strip_suffix('>'))
            .ok_or_else(|| format!("line {}: malformed generic parameter list", line))?;
        format!("({})", body)
    };
    Ok(format!("pub nat type {}{}", name, constructor_parameters))
}

fn render_function(declaration: &str, line: usize) -> Result<String, String> {
    let open = declaration.find('(').ok_or_else(|| format!("line {}: function requires parameter list", line))?;
    let close = matching_close(declaration, open).ok_or_else(|| format!("line {}: function requires closing ')'", line))?;
    let head = declaration[..open].trim();
    let (name, generics) = split_generic_name(head, line)?;
    validate_snake(name, line)?;
    validate_type_parameters(generics, line)?;
    let params = declaration[open + 1..close].trim();
    validate_parameters(params, line)?;
    let suffix = declaration[close + 1..].trim();
    let (return_type, impure) = match suffix.strip_suffix(" impure") {
        Some(return_text) => (return_text.trim(), true),
        None => (suffix, false),
    };
    validate_type_expression(return_type, line)?;
    let effect = if impure { " impure" } else { "" };
    Ok(format!("pub nat fun {}{}({}){} {}", name, generics, params, effect, return_type))
}

fn matching_close(text: &str, open: usize) -> Option<usize> {
    let mut depth = 0i64;
    for (relative, character) in text[open..].char_indices() {
        if character == '(' {
            depth += 1;
        } else if character == ')' {
            depth -= 1;
            if depth == 0 {
                return Some(open + relative);
            }
        }
    }
    None
}

fn split_generic_name(text: &str, line: usize) -> Result<(&str, &str), String> {
    if let Some(open) = text.find('<') {
        if !text.ends_with('>') {
            return Err(format!("line {}: generic parameters require closing '>'", line));
        }
        Ok((text[..open].trim(), &text[open..]))
    } else {
        Ok((text.trim(), ""))
    }
}

fn validate_type_parameters(parameters: &str, line: usize) -> Result<(), String> {
    if parameters.is_empty() {
        return Ok(());
    }
    let body = parameters
        .strip_prefix('<')
        .and_then(|rest| rest.strip_suffix('>'))
        .ok_or_else(|| format!("line {}: malformed generic parameter list", line))?;
    for parameter in body.split(',') {
        validate_pascal(parameter.trim(), line)?;
    }
    Ok(())
}

fn validate_parameters(parameters: &str, line: usize) -> Result<(), String> {
    if parameters.is_empty() {
        return Ok(());
    }
    for parameter in split_top_level(parameters, ',')? {
        let (name, ty) = parameter
            .split_once(':')
            .ok_or_else(|| format!("line {}: parameter '{}' requires ':'", line, parameter.trim()))?;
        validate_snake(name.trim(), line)?;
        validate_type_expression(ty.trim(), line)?;
    }
    Ok(())
}

fn split_top_level(text: &str, separator: char) -> Result<Vec<&str>, String> {
    let mut parts = Vec::new();
    let mut depth = 0i64;
    let mut start = 0usize;
    for (index, character) in text.char_indices() {
        if character == '(' || character == '[' || character == '<' {
            depth += 1;
        } else if character == ')' || character == ']' || character == '>' {
            depth -= 1;
            if depth < 0 {
                return Err("unbalanced delimiters in native IDL".to_string());
            }
        } else if character == separator && depth == 0 {
            parts.push(text[start..index].trim());
            start = index + character.len_utf8();
        }
    }
    if depth != 0 {
        return Err("unbalanced delimiters in native IDL".to_string());
    }
    parts.push(text[start..].trim());
    Ok(parts)
}

fn validate_type_expression(text: &str, line: usize) -> Result<(), String> {
    if text.is_empty() {
        return Err(format!("line {}: type cannot be empty", line));
    }
    let mut depth = 0i64;
    for character in text.chars() {
        if character == '(' || character == '[' || character == '<' {
            depth += 1;
        } else if character == ')' || character == ']' || character == '>' {
            depth -= 1;
        } else if character.is_ascii_alphanumeric()
            || character == '_'
            || character == '&'
            || character == ';'
            || character == ','
            || character == ' '
            || character == '.'
        {
        } else {
            return Err(format!("line {}: unsupported character '{}' in type", line, character));
        }
        if depth < 0 {
            return Err(format!("line {}: unbalanced type delimiters", line));
        }
    }
    if depth != 0 {
        return Err(format!("line {}: unbalanced type delimiters", line));
    }
    Ok(())
}

fn validate_pascal(name: &str, line: usize) -> Result<(), String> {
    if is_pascal(name) {
        Ok(())
    } else {
        Err(format!("line {}: '{}' must be PascalCase", line, name))
    }
}

fn validate_snake(name: &str, line: usize) -> Result<(), String> {
    if is_snake(name) {
        Ok(())
    } else {
        Err(format!("line {}: '{}' must be snake_case", line, name))
    }
}

fn is_pascal(name: &str) -> bool {
    let mut chars = name.chars();
    match chars.next() {
        Some(first) => first.is_ascii_uppercase() && chars.all(|character| character.is_ascii_alphanumeric()),
        None => false,
    }
}

fn is_snake(name: &str) -> bool {
    let mut chars = name.chars();
    match chars.next() {
        Some(first) => {
            (first.is_ascii_lowercase())
                && chars.all(|character| character.is_ascii_lowercase() || character.is_ascii_digit() || character == '_')
        }
        None => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn renders_typed_native_surface() {
        let idl = "module Device\ntype Handle\ntype Buffer<T>\ntype Error\nfun open(port: U16) Handle impure\nfun read<T>(handle: &Handle, out: &mut [T]) Result(Usize, Device.Error) impure\n";
        let generated = match generate_from_idl(idl) {
            Ok(value) => value,
            Err(message) => {
                assert!(message.is_empty(), "{}", message);
                return;
            }
        };
        assert!(generated.contains("pub mod Device"));
        assert!(generated.contains("pub nat type Buffer(T)"));
        assert!(generated.contains("pub nat fun read<T>(handle: &Handle, out: &mut [T]) impure Result(Usize, Device.Error)"));
    }

    #[test]
    fn rejects_names_that_violate_language_casing() {
        assert!(generate_from_idl("module bad_module\ntype Handle\n").is_err());
        assert!(generate_from_idl("module Device\nfun Bad() Unit\n").is_err());
    }
}
