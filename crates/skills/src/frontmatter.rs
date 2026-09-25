//! `SKILL.md` frontmatter: the YAML block between the first two `---` lines. Only `name`
//! and `description` matter to OwO AI Gateway; every other key is the apps' business.

use anyhow::{bail, Result};

pub const MAX_NAME: usize = 64;
/// OpenCode and the Agent Skills specification reject longer descriptions.
pub const MAX_DESCRIPTION: usize = 1024;

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Frontmatter {
    pub name: Option<String>,
    pub description: Option<String>,
}

/// Parses the frontmatter of a `SKILL.md`. YAML that does not parse (an unquoted `: ` in a
/// description is common) falls back to flat `key: value` lines, the way several apps read it.
pub fn parse(text: &str) -> Result<Frontmatter> {
    let text = text.strip_prefix('\u{feff}').unwrap_or(text);
    let mut lines = text.lines().skip_while(|l| l.trim().is_empty());
    if lines.next().map(str::trim_end) != Some("---") {
        bail!("SKILL.md does not start with a `---` frontmatter block");
    }
    let mut block = Vec::new();
    let mut closed = false;
    for line in lines {
        if line.trim_end() == "---" {
            closed = true;
            break;
        }
        block.push(line);
    }
    if !closed {
        bail!("the SKILL.md frontmatter block is not closed with `---`");
    }
    match serde_yaml::from_str::<serde_yaml::Value>(&block.join("\n")) {
        Ok(serde_yaml::Value::Mapping(map)) => {
            let field = |k: &str| map.get(k).and_then(scalar).map(|s| s.trim().to_string()).filter(|s| !s.is_empty());
            Ok(Frontmatter { name: field("name"), description: field("description") })
        }
        Ok(serde_yaml::Value::Null) => Ok(Frontmatter::default()),
        // `name:value` (no space) is a plain string to YAML; read such lines as flat keys.
        Ok(_) | Err(_) => Ok(flat(&block)),
    }
}

fn scalar(v: &serde_yaml::Value) -> Option<String> {
    match v {
        serde_yaml::Value::String(s) => Some(s.clone()),
        serde_yaml::Value::Number(n) => Some(n.to_string()),
        serde_yaml::Value::Bool(b) => Some(b.to_string()),
        _ => None,
    }
}

/// Top-level `key: value` lines; `>` and `|` values continue on the indented lines below.
fn flat(block: &[&str]) -> Frontmatter {
    let mut out = Frontmatter::default();
    let mut i = 0;
    while i < block.len() {
        let line = block[i];
        i += 1;
        if line.starts_with([' ', '\t', '#']) {
            continue;
        }
        let Some((key, value)) = line.split_once(':') else { continue };
        let mut value = value.trim().to_string();
        if matches!(value.as_str(), ">" | ">-" | "|" | "|-" | ">+" | "|+") {
            let folded = value.starts_with('>');
            let mut parts = Vec::new();
            while i < block.len() && (block[i].starts_with([' ', '\t']) || block[i].trim().is_empty()) {
                parts.push(block[i].trim());
                i += 1;
            }
            value = parts.join(if folded { " " } else { "\n" }).trim().to_string();
        } else if value.len() >= 2 && ((value.starts_with('"') && value.ends_with('"')) || (value.starts_with('\'') && value.ends_with('\''))) {
            value = value[1..value.len() - 1].to_string();
        } else if let Some(i) = value.find(" #") {
            // A comment after an unquoted value, as YAML reads it.
            value = value[..i].trim_end().to_string();
        }
        let value = Some(value).filter(|v| !v.is_empty());
        match key.trim() {
            "name" => out.name = value,
            "description" => out.description = value,
            _ => {}
        }
    }
    out
}

/// Lowercase letters, digits, and single hyphens, 1–64 characters (the Agent Skills rule
/// OpenCode, Cursor, and MiniMax enforce).
pub fn valid_name(name: &str) -> bool {
    !name.is_empty() && name.len() <= MAX_NAME && name.split('-').all(|p| !p.is_empty() && p.bytes().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit()))
}

/// What OwO AI Gateway needs to know about one skill.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SkillMeta {
    pub name: String,
    pub description: String,
    pub warnings: Vec<String>,
}

/// Checks a skill whose `SKILL.md` reads `text` and whose directory is called `dir_name`.
/// The installed directory is named after `name`, so a differently named source directory
/// only earns a warning.
pub fn inspect(dir_name: &str, text: &str) -> Result<SkillMeta> {
    let fm = parse(text)?;
    let mut warnings = Vec::new();
    let name = match fm.name {
        Some(n) => n,
        None => {
            warnings.push(format!("SKILL.md has no `name`; using the directory name `{dir_name}`"));
            dir_name.to_string()
        }
    };
    if !valid_name(&name) {
        bail!("skill name `{name}` is not valid: use 1–{MAX_NAME} lowercase letters, digits, and single hyphens");
    }
    if name != dir_name && !dir_name.is_empty() {
        warnings.push(format!("directory `{dir_name}` does not match the skill name; it is installed as `{name}`"));
    }
    let Some(description) = fm.description else { bail!("SKILL.md of `{name}` has no `description`") };
    if description.chars().count() > MAX_DESCRIPTION {
        warnings.push(format!("the description is longer than {MAX_DESCRIPTION} characters; OpenCode ignores such skills"));
    }
    Ok(SkillMeta { name, description, warnings })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plain_yaml() {
        let fm = parse("---\nname: pdf\ndescription: Read and write PDF files.\nlicense: MIT\n---\n# PDF\n").unwrap();
        assert_eq!(fm.name.as_deref(), Some("pdf"));
        assert_eq!(fm.description.as_deref(), Some("Read and write PDF files."));
    }

    #[test]
    fn bom_crlf_and_folded_description() {
        let text = "\u{feff}---\r\nname: my-skill\r\ndescription: >\r\n  First line\r\n  second line\r\n---\r\nbody\r\n";
        let fm = parse(text).unwrap();
        assert_eq!(fm.name.as_deref(), Some("my-skill"));
        assert_eq!(fm.description.as_deref(), Some("First line second line"));
    }

    #[test]
    fn invalid_yaml_falls_back_to_flat_lines() {
        let fm = parse("---\nname: deploy\ndescription: Deploy: staging or prod: pick one\n---\n").unwrap();
        assert_eq!(fm.name.as_deref(), Some("deploy"));
        assert_eq!(fm.description.as_deref(), Some("Deploy: staging or prod: pick one"));
    }

    #[test]
    fn loose_names_as_the_editor_reads_them() {
        let names = [
            "---\nname: foo # the id\ndescription: d\n---\n",
            "---\r\nname: \"foo\"\r\ndescription: d\r\n---\r\n",
            "---\nname:foo\ndescription: d\n---\n",
            "---\nname:foo\n---\n",
            "---\nname: 'foo'\ndescription: a: b\n---\n",
        ];
        for text in names {
            assert_eq!(parse(text).unwrap().name.as_deref(), Some("foo"), "{text:?}");
        }
        assert_eq!(parse("---\nname: foo # x\ndescription: d: e\n---\n").unwrap().name.as_deref(), Some("foo"), "the flat fallback drops comments too");
    }

    #[test]
    fn missing_or_open_block() {
        assert!(parse("# no frontmatter\n").is_err());
        assert!(parse("---\nname: x\n").is_err());
        assert_eq!(parse("---\n---\n").unwrap(), Frontmatter::default());
    }

    #[test]
    fn names() {
        for ok in ["pdf", "skill-creator", "a1-b2", "x"] {
            assert!(valid_name(ok), "{ok}");
        }
        for bad in ["", "-a", "a-", "a--b", "Upper", "under_score", "dot.name", "../x", &"a".repeat(65)] {
            assert!(!valid_name(bad), "{bad}");
        }
    }

    #[test]
    fn inspect_rules() {
        let meta = inspect("pdf-main", "---\nname: pdf\ndescription: d\n---\n").unwrap();
        assert_eq!(meta.name, "pdf");
        assert_eq!(meta.warnings.len(), 1);
        assert_eq!(inspect("tool", "---\ndescription: d\n---\n").unwrap().name, "tool");
        assert!(inspect("x", "---\nname: x\n---\n").is_err(), "a description is required");
        assert!(inspect("x", "---\nname: ../evil\ndescription: d\n---\n").is_err());
    }
}
