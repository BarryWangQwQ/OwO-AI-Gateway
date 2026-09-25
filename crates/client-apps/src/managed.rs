//! Reversible edits of client configuration files.
//!
//! An integration owns *fragments*: values at fixed paths inside a JSON, YAML, or TOML
//! document (`provider.owo`, `rules[providerId=owo]`, `mcp_servers.x`, ...), or one marked
//! text block in a TOML file. Enabling records each fragment's previous value and backs the
//! file up once; restoring puts the file back byte-for-byte when nobody touched it since, and
//! otherwise puts back only OwO AI Gateway's fragments. A fragment changed by someone else is a conflict, never silently
//! overwritten or removed (`force` overrides).

use std::path::{Path, PathBuf};

use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use sha2::{Digest, Sha256};
use toml_edit::DocumentMut;

/// One step of a fragment path.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Seg {
    /// Object member.
    Key(String),
    /// Array element whose string fields equal all of these.
    Item(Vec<(String, String)>),
}

impl Seg {
    pub fn key(k: &str) -> Self {
        Seg::Key(k.to_string())
    }

    pub fn item(fields: &[(&str, &str)]) -> Self {
        Seg::Item(fields.iter().map(|(f, v)| (f.to_string(), v.to_string())).collect())
    }

    fn matches(&self, v: &Value) -> bool {
        match self {
            Seg::Item(fields) => fields.iter().all(|(f, want)| v.get(f).and_then(Value::as_str) == Some(want.as_str())),
            Seg::Key(_) => false,
        }
    }
}

pub fn display_path(path: &[Seg]) -> String {
    path.iter()
        .map(|s| match s {
            Seg::Key(k) => k.clone(),
            Seg::Item(f) => format!("[{}]", f.iter().map(|(k, v)| format!("{k}={v}")).collect::<Vec<_>>().join(",")),
        })
        .collect::<Vec<_>>()
        .join(".")
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Format {
    Json,
    Yaml,
    /// A text block between OwO AI Gateway's markers, appended to a TOML file.
    TomlBlock,
    /// Tables and values at key paths of a TOML file, edited in place (comments and layout
    /// kept). An OwO AI Gateway text block in the same file stays as it is, at the end.
    Toml,
}

#[derive(Debug, Clone)]
pub struct Fragment {
    pub path: Vec<Seg>,
    pub value: Value,
}

/// What an integration wants in one file.
#[derive(Debug, Clone)]
pub struct FileEdit {
    pub path: PathBuf,
    pub format: Format,
    pub fragments: Vec<Fragment>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FragmentState {
    pub path: Vec<Seg>,
    pub original: Option<Value>,
    /// What OwO AI Gateway wrote; `null` when only `written_digest` is kept.
    pub written: Value,
    /// Digest of what was written, instead of the value itself, for fragments that may
    /// carry secrets (see [`Options::redact`]).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub written_digest: Option<String>,
    /// Leading path steps that already existed; emptied ancestors below are pruned on restore.
    pub existing_depth: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileState {
    pub path: PathBuf,
    pub format: Format,
    pub existed: bool,
    pub backup: Option<PathBuf>,
    pub written_sha256: String,
    /// Only OwO AI Gateway has changed the file since `backup` was taken, so restoring the backup
    /// undoes exactly OwO AI Gateway's changes. Cleared once someone else writes the file between two
    /// of OwO AI Gateway's writes, or when an existing entry is adopted.
    #[serde(default = "yes")]
    pub pristine: bool,
    pub fragments: Vec<FragmentState>,
}

fn yes() -> bool {
    true
}

/// Whether `value` is what OwO AI Gateway recorded writing for `state`.
pub fn is_written(state: &FragmentState, value: &Value) -> bool {
    match &state.written_digest {
        Some(d) => digest(value) == *d,
        None => *value == state.written,
    }
}

/// SHA-256 of `value` with object keys sorted, so key order does not matter.
pub fn digest(value: &Value) -> String {
    fn canonical(v: &Value) -> Value {
        match v {
            Value::Object(o) => {
                let mut keys: Vec<&String> = o.keys().collect();
                keys.sort();
                Value::Object(keys.into_iter().map(|k| (k.clone(), canonical(&o[k]))).collect())
            }
            Value::Array(a) => Value::Array(a.iter().map(canonical).collect()),
            other => other.clone(),
        }
    }
    sha256(canonical(value).to_string().as_bytes())
}

fn fragment_state(path: &[Seg], original: Option<Value>, written: &Value, existing_depth: usize, redact: bool) -> FragmentState {
    let (written, written_digest) = if redact { (Value::Null, Some(digest(written))) } else { (written.clone(), None) };
    FragmentState { path: path.to_vec(), original, written, written_digest, existing_depth }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClientState {
    pub client: String,
    pub owo_version: String,
    pub updated_at_unix: u64,
    pub files: Vec<FileState>,
}

pub struct Store {
    pub state_dir: PathBuf,
    pub backups_dir: PathBuf,
}

impl Store {
    fn state_path(&self, client: &str) -> PathBuf {
        self.state_dir.join(client).join("state.json")
    }

    pub fn load(&self, client: &str) -> Result<Option<ClientState>> {
        match std::fs::read_to_string(self.state_path(client)) {
            Ok(t) => Ok(Some(serde_json::from_str(&t).with_context(|| format!("{client} integration state is corrupt"))?)),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(e) => Err(e.into()),
        }
    }

    fn save(&self, state: &ClientState) -> Result<()> {
        write_atomic(&self.state_path(&state.client), serde_json::to_string_pretty(state)?.as_bytes())
    }

    fn clear(&self, client: &str) -> Result<()> {
        match std::fs::remove_file(self.state_path(client)) {
            Err(e) if e.kind() != std::io::ErrorKind::NotFound => Err(e.into()),
            _ => Ok(()),
        }
    }
}

pub fn sha256(bytes: &[u8]) -> String {
    Sha256::digest(bytes).iter().map(|b| format!("{b:02x}")).collect()
}

pub fn now_unix() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0)
}

pub fn write_atomic(path: &Path, contents: &[u8]) -> Result<()> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).with_context(|| format!("cannot create {}", dir.display()))?;
    }
    let name = path.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
    let tmp = path.with_file_name(format!(".{name}.owo-{}.tmp", std::process::id()));
    std::fs::write(&tmp, contents).with_context(|| format!("cannot write {}", tmp.display()))?;
    if let Err(e) = std::fs::rename(&tmp, path) {
        let _ = std::fs::remove_file(&tmp);
        return Err(e).with_context(|| format!("cannot replace {}", path.display()));
    }
    Ok(())
}

fn read_bytes(path: &Path) -> Result<Option<Vec<u8>>> {
    match std::fs::read(path) {
        Ok(b) => Ok(Some(b)),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(e).with_context(|| format!("cannot read {}", path.display())),
    }
}

// ---------------------------------------------------------------------------
// Documents

fn parse(path: &Path, format: Format, bytes: Option<&[u8]>) -> Result<Value> {
    let Some(bytes) = bytes.filter(|b| !b.iter().all(u8::is_ascii_whitespace)) else {
        return Ok(Value::Object(Map::new()));
    };
    let v: Value = match format {
        Format::Json => serde_json::from_slice(bytes)
            .with_context(|| format!("{} is not plain JSON (comments are not supported); fix it or edit it by hand", path.display()))?,
        Format::Yaml => serde_yaml::from_slice(bytes).with_context(|| format!("{} is not YAML OwO AI Gateway can rewrite", path.display()))?,
        Format::TomlBlock | Format::Toml => unreachable!("TOML is not parsed into a value tree"),
    };
    match v {
        Value::Object(_) => Ok(v),
        Value::Null => Ok(Value::Object(Map::new())),
        _ => bail!("{} does not contain an object at the top level", path.display()),
    }
}

fn render(format: Format, doc: &Value) -> Result<Vec<u8>> {
    Ok(match format {
        Format::Json => {
            let mut t = serde_json::to_string_pretty(doc)?;
            t.push('\n');
            t.into_bytes()
        }
        Format::Yaml => serde_yaml::to_string(doc)?.into_bytes(),
        Format::TomlBlock | Format::Toml => unreachable!("TOML is not rendered from a value tree"),
    })
}

pub fn get<'a>(root: &'a Value, path: &[Seg]) -> Option<&'a Value> {
    let mut cur = root;
    for seg in path {
        cur = match seg {
            Seg::Key(k) => cur.as_object()?.get(k)?,
            Seg::Item(_) => cur.as_array()?.iter().find(|v| seg.matches(v))?,
        };
    }
    Some(cur)
}

fn container_for(next: &Seg) -> Value {
    match next {
        Seg::Key(_) => Value::Object(Map::new()),
        Seg::Item(_) => Value::Array(Vec::new()),
    }
}

fn set(root: &mut Value, path: &[Seg], value: Value) -> Result<()> {
    let (last, parents) = path.split_last().context("empty fragment path")?;
    let mut cur = root;
    for (i, seg) in parents.iter().enumerate() {
        let next = &path[i + 1];
        cur = match seg {
            Seg::Key(k) => {
                let obj = cur.as_object_mut().with_context(|| format!("`{}` is not an object", display_path(&path[..i])))?;
                obj.entry(k.clone()).or_insert_with(|| container_for(next))
            }
            Seg::Item(_) => {
                let arr = cur.as_array_mut().with_context(|| format!("`{}` is not a list", display_path(&path[..i])))?;
                let pos = match arr.iter().position(|v| seg.matches(v)) {
                    Some(p) => p,
                    None => {
                        arr.push(container_for(next));
                        arr.len() - 1
                    }
                };
                &mut arr[pos]
            }
        };
    }
    match last {
        Seg::Key(k) => {
            cur.as_object_mut().with_context(|| format!("`{}` is not an object", display_path(parents)))?.insert(k.clone(), value);
        }
        Seg::Item(_) => {
            let arr = cur.as_array_mut().with_context(|| format!("`{}` is not a list", display_path(parents)))?;
            match arr.iter().position(|v| last.matches(v)) {
                Some(p) => arr[p] = value,
                None => arr.push(value),
            }
        }
    }
    Ok(())
}

fn get_mut<'a>(root: &'a mut Value, path: &[Seg]) -> Option<&'a mut Value> {
    let mut cur = root;
    for seg in path {
        cur = match seg {
            Seg::Key(k) => cur.as_object_mut()?.get_mut(k)?,
            Seg::Item(_) => cur.as_array_mut()?.iter_mut().find(|v| seg.matches(v))?,
        };
    }
    Some(cur)
}

fn remove(root: &mut Value, path: &[Seg]) {
    let Some((last, parents)) = path.split_last() else { return };
    let Some(parent) = get_mut(root, parents) else { return };
    match last {
        Seg::Key(k) => {
            if let Some(o) = parent.as_object_mut() {
                o.shift_remove(k);
            }
        }
        Seg::Item(_) => {
            if let Some(a) = parent.as_array_mut() {
                a.retain(|v| !last.matches(v));
            }
        }
    }
}

/// A document fragments can be read from and written into.
trait Doc {
    fn value_at(&self, path: &[Seg]) -> Option<Value>;
    fn put(&mut self, path: &[Seg], value: Value) -> Result<()>;
    fn delete(&mut self, path: &[Seg]);

    fn depth(&self, path: &[Seg]) -> usize {
        (0..path.len()).take_while(|i| self.value_at(&path[..=*i]).is_some()).count()
    }

    /// Removes emptied containers that OwO AI Gateway created (the steps after the first `existing_depth`).
    fn prune(&mut self, path: &[Seg], existing_depth: usize) {
        for depth in (existing_depth + 1..path.len()).rev() {
            let empty = self.value_at(&path[..depth]).is_some_and(|v| match v {
                Value::Object(o) => o.is_empty(),
                Value::Array(a) => a.is_empty(),
                _ => false,
            });
            if empty {
                self.delete(&path[..depth]);
            }
        }
    }
}

impl Doc for Value {
    fn value_at(&self, path: &[Seg]) -> Option<Value> {
        get(self, path).cloned()
    }

    fn put(&mut self, path: &[Seg], value: Value) -> Result<()> {
        set(self, path, value)
    }

    fn delete(&mut self, path: &[Seg]) {
        remove(self, path)
    }
}

// ---------------------------------------------------------------------------
// TOML documents

struct TomlDoc(DocumentMut);

fn parse_toml(path: &Path, text: &str) -> Result<TomlDoc> {
    Ok(TomlDoc(text.parse().with_context(|| format!("{} is not TOML OwO AI Gateway can rewrite", path.display()))?))
}

fn toml_key(seg: &Seg) -> Result<&str> {
    match seg {
        Seg::Key(k) => Ok(k),
        Seg::Item(_) => bail!("list items cannot be addressed in a TOML file"),
    }
}

fn toml_table_json(t: &dyn toml_edit::TableLike) -> Value {
    Value::Object(t.iter().filter_map(|(k, item)| Some((k.to_string(), toml_item_json(item)?))).collect())
}

fn toml_item_json(item: &toml_edit::Item) -> Option<Value> {
    use toml_edit::Item;
    match item {
        Item::None => None,
        Item::Value(v) => Some(toml_value_json(v)),
        Item::Table(t) => Some(toml_table_json(t)),
        Item::ArrayOfTables(a) => Some(Value::Array(a.iter().map(|t| toml_table_json(t)).collect())),
    }
}

fn toml_value_json(v: &toml_edit::Value) -> Value {
    use toml_edit::Value as T;
    match v {
        T::String(s) => Value::String(s.value().clone()),
        T::Integer(i) => Value::from(*i.value()),
        T::Float(f) => serde_json::Number::from_f64(*f.value()).map_or(Value::Null, Value::Number),
        T::Boolean(b) => Value::Bool(*b.value()),
        T::Datetime(d) => Value::String(d.value().to_string()),
        T::Array(a) => Value::Array(a.iter().map(toml_value_json).collect()),
        T::InlineTable(t) => toml_table_json(t),
    }
}

fn json_toml_value(v: &Value) -> Result<toml_edit::Value> {
    Ok(match v {
        Value::Null => bail!("TOML has no null value"),
        Value::Bool(b) => (*b).into(),
        Value::Number(n) => match n.as_i64() {
            Some(i) => i.into(),
            None => n.as_f64().context("number out of range for TOML")?.into(),
        },
        Value::String(s) => s.as_str().into(),
        Value::Array(items) => {
            let mut a = toml_edit::Array::new();
            for item in items {
                a.push(json_toml_value(item)?);
            }
            a.into()
        }
        Value::Object(o) => {
            let mut t = toml_edit::InlineTable::new();
            for (k, item) in o {
                t.insert(k, json_toml_value(item)?);
            }
            t.into()
        }
    })
}

/// An object becomes a standard table whose nested objects are inline tables
/// (`[mcp_servers.x]` with `env = { … }`); anything else becomes a plain value.
fn json_toml_item(v: &Value) -> Result<toml_edit::Item> {
    match v {
        Value::Object(o) => {
            let mut t = toml_edit::Table::new();
            for (k, item) in o {
                t.insert(k, toml_edit::Item::Value(json_toml_value(item)?));
            }
            Ok(toml_edit::Item::Table(t))
        }
        _ => Ok(toml_edit::Item::Value(json_toml_value(v)?)),
    }
}

impl Doc for TomlDoc {
    fn value_at(&self, path: &[Seg]) -> Option<Value> {
        let mut cur = self.0.as_item();
        for seg in path {
            cur = cur.as_table_like()?.get(toml_key(seg).ok()?)?;
        }
        toml_item_json(cur)
    }

    fn put(&mut self, path: &[Seg], value: Value) -> Result<()> {
        let (last, parents) = path.split_last().context("empty fragment path")?;
        let mut cur = self.0.as_table_mut();
        for (i, seg) in parents.iter().enumerate() {
            let k = toml_key(seg)?;
            if !cur.contains_key(k) {
                let mut t = toml_edit::Table::new();
                t.set_implicit(true);
                cur.insert(k, toml_edit::Item::Table(t));
            }
            cur = cur
                .get_mut(k)
                .and_then(toml_edit::Item::as_table_mut)
                .with_context(|| format!("`{}` is not a standard TOML table; OwO AI Gateway will not rewrite it", display_path(&path[..=i])))?;
        }
        cur.insert(toml_key(last)?, json_toml_item(&value)?);
        Ok(())
    }

    fn delete(&mut self, path: &[Seg]) {
        let Some((last, parents)) = path.split_last() else { return };
        let mut cur = self.0.as_item_mut();
        for seg in parents {
            let Ok(k) = toml_key(seg) else { return };
            let Some(next) = cur.as_table_like_mut().and_then(|t| t.get_mut(k)) else { return };
            cur = next;
        }
        if let (Some(t), Ok(k)) = (cur.as_table_like_mut(), toml_key(last)) {
            t.remove(k);
        }
    }
}

/// Splits off OwO AI Gateway's text block (if any), so key edits never land inside it.
fn toml_without_block(path: &Path, bytes: Option<&[u8]>) -> Result<(TomlDoc, Option<String>)> {
    let text = match bytes {
        Some(b) => String::from_utf8(b.to_vec()).with_context(|| format!("{} is not UTF-8", path.display()))?,
        None => String::new(),
    };
    let (rest, block) = without_block(&text)?;
    Ok((parse_toml(path, &rest)?, block))
}

fn toml_render(doc: &TomlDoc, block: Option<&str>) -> Vec<u8> {
    let mut out = doc.0.to_string();
    if let Some(block) = block {
        if !out.is_empty() {
            if !out.ends_with('\n') {
                out.push('\n');
            }
            out.push('\n');
        }
        out.push_str(block);
    }
    out.into_bytes()
}

/// The value at `path` in a client file; `None` when the file or the entry is missing.
pub fn read_value(file: &Path, format: Format, path: &[Seg]) -> Result<Option<Value>> {
    let Some(bytes) = read_bytes(file)? else { return Ok(None) };
    Ok(match format {
        Format::TomlBlock => bail!("a text block has no addressable values"),
        Format::Toml => toml_without_block(file, Some(&bytes))?.0.value_at(path),
        _ => parse(file, format, Some(&bytes))?.value_at(path),
    })
}

// ---------------------------------------------------------------------------
// TOML text blocks

pub const BLOCK_BEGIN: &str = "# >>> OwO AI Gateway managed block — do not edit (removed by `owo disconnect …`) >>>";
pub const BLOCK_END: &str = "# <<< OwO AI Gateway managed block <<<";

/// Splits `text` into (before, block, after) around OwO AI Gateway's markers.
fn find_block(text: &str) -> Result<Option<(usize, usize)>> {
    match (text.find(BLOCK_BEGIN), text.find(BLOCK_END)) {
        (None, None) => Ok(None),
        (Some(b), Some(e)) if e > b => {
            let mut end = e + BLOCK_END.len();
            if text[end..].starts_with("\r\n") {
                end += 2;
            } else if text[end..].starts_with('\n') {
                end += 1;
            }
            Ok(Some((b, end)))
        }
        _ => bail!("the OwO AI Gateway block markers are damaged; remove the partial block by hand"),
    }
}

fn wrap_block(body: &str) -> String {
    format!("{BLOCK_BEGIN}\n{}\n{BLOCK_END}\n", body.trim_end())
}

fn without_block(text: &str) -> Result<(String, Option<String>)> {
    Ok(match find_block(text)? {
        Some((b, e)) => {
            let mut before = text[..b].to_string();
            // Drop the blank separator line OwO AI Gateway added before the block.
            if before.ends_with("\n\n") {
                before.pop();
            }
            (format!("{before}{}", &text[e..]), Some(text[b..e].to_string()))
        }
        None => (text.to_string(), None),
    })
}

// ---------------------------------------------------------------------------
// Enable / restore

#[derive(Debug, Default)]
pub struct Report {
    pub lines: Vec<String>,
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone, Copy, Default)]
pub struct Options {
    /// Replace entries OwO AI Gateway did not write, or that were changed after it wrote them.
    pub force: bool,
    /// Keep only a digest of what was written in the state file, for fragments that may
    /// carry secrets (resolved keys in an MCP server's `env` or `headers`).
    pub redact: bool,
}

/// Copies `bytes` (the current `path`) into the client's backup directory.
fn back_up(store: &Store, client: &str, path: &Path, bytes: &[u8]) -> Result<PathBuf> {
    let dir = store.backups_dir.join(client);
    std::fs::create_dir_all(&dir)?;
    let name = path.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
    let stamp = now_unix();
    let mut dest = dir.join(format!("{stamp}-{name}"));
    let mut n = 2;
    while dest.exists() {
        dest = dir.join(format!("{stamp}-{n}-{name}"));
        n += 1;
    }
    std::fs::write(&dest, bytes).with_context(|| format!("cannot back up {}", path.display()))?;
    Ok(dest)
}

/// Applies `edits` for `client`. Re-enabling refreshes OwO AI Gateway's fragments and keeps the
/// originals recorded the first time.
pub fn enable(store: &Store, client: &str, edits: &[FileEdit], force: bool) -> Result<Report> {
    enable_with(store, client, edits, Options { force, redact: false })
}

pub fn enable_with(store: &Store, client: &str, edits: &[FileEdit], opts: Options) -> Result<Report> {
    let force = opts.force;
    let mut report = Report::default();
    let prior = store.load(client)?;
    let mut files = Vec::new();
    // Plan everything before writing anything.
    let mut writes = Vec::new();
    for edit in edits {
        let prior_file = prior.as_ref().and_then(|p| p.files.iter().find(|f| f.path == edit.path));
        let bytes = read_bytes(&edit.path)?;
        let existed = prior_file.map_or(bytes.is_some(), |f| f.existed);
        let (out, fragments) = match edit.format {
            Format::TomlBlock => plan_block(edit, bytes.as_deref(), prior_file, force)?,
            Format::Toml => plan_toml(edit, bytes.as_deref(), prior_file, opts)?,
            Format::Json | Format::Yaml => plan_doc(edit, bytes.as_deref(), prior_file, opts)?,
        };
        let changed_since = prior_file.is_some_and(|f| bytes.as_deref().map(sha256).as_deref() != Some(f.written_sha256.as_str()));
        let backup = match (prior_file, &bytes) {
            (Some(f), Some(b)) => {
                // The first backup stays the one restore uses; this copy keeps the other
                // program's version of the file.
                if changed_since {
                    let dest = back_up(store, client, &edit.path, b)?;
                    report.lines.push(format!("backup:   {} (changed since OwO AI Gateway last wrote it)", dest.display()));
                }
                f.backup.clone()
            }
            (Some(f), None) => f.backup.clone(),
            (None, Some(b)) => {
                let dest = back_up(store, client, &edit.path, b)?;
                report.lines.push(format!("backup:   {}", dest.display()));
                Some(dest)
            }
            (None, None) => None,
        };
        files.push(FileState {
            path: edit.path.clone(),
            format: edit.format,
            existed,
            backup,
            written_sha256: sha256(&out),
            pristine: prior_file.is_none_or(|f| f.pristine && !changed_since),
            fragments,
        });
        let unchanged = bytes.as_deref() == Some(out.as_slice());
        writes.push((edit.path.clone(), out, unchanged));
    }
    for (path, out, unchanged) in &writes {
        if *unchanged {
            report.lines.push(format!("unchanged: {}", path.display()));
            continue;
        }
        write_atomic(path, out)?;
        report.lines.push(format!("wrote:    {}", path.display()));
    }
    // Files OwO AI Gateway managed before but no longer edits are put back.
    if let Some(p) = &prior {
        for old in p.files.iter().filter(|f| !edits.iter().any(|e| e.path == f.path)) {
            restore_file(old, force, &mut report)?;
        }
    }
    store.save(&ClientState { client: client.to_string(), owo_version: env!("CARGO_PKG_VERSION").to_string(), updated_at_unix: now_unix(), files })?;
    Ok(report)
}

fn apply_fragments(doc: &mut impl Doc, edit: &FileEdit, prior: Option<&FileState>, opts: Options) -> Result<Vec<FragmentState>> {
    let mut states = Vec::new();
    for frag in &edit.fragments {
        let current = doc.value_at(&frag.path);
        let prior_frag = prior.and_then(|f| f.fragments.iter().find(|s| s.path == frag.path));
        let (original, depth) = match prior_frag {
            Some(s) => {
                if current.as_ref().is_some_and(|c| !is_written(s, c)) && !opts.force {
                    bail!("`{}` in {} was changed after OwO AI Gateway wrote it (re-run with --force to replace it)", display_path(&frag.path), edit.path.display());
                }
                (s.original.clone(), s.existing_depth)
            }
            None => {
                if current.is_some() && !opts.force {
                    bail!("{} already has `{}`, not written by OwO AI Gateway (re-run with --force to replace it; the file is backed up first)", edit.path.display(), display_path(&frag.path));
                }
                (current, doc.depth(&frag.path))
            }
        };
        doc.put(&frag.path, frag.value.clone())?;
        states.push(fragment_state(&frag.path, original, &frag.value, depth, opts.redact));
    }
    // Fragments OwO AI Gateway wrote before but no longer wants go back to their originals.
    if let Some(p) = prior {
        for old in p.fragments.iter().filter(|s| !edit.fragments.iter().any(|f| f.path == s.path)) {
            if doc.value_at(&old.path).is_some_and(|v| is_written(old, &v)) || opts.force {
                put_back(doc, old);
            }
        }
    }
    Ok(states)
}

fn plan_doc(edit: &FileEdit, bytes: Option<&[u8]>, prior: Option<&FileState>, opts: Options) -> Result<(Vec<u8>, Vec<FragmentState>)> {
    let mut doc = parse(&edit.path, edit.format, bytes)?;
    let states = apply_fragments(&mut doc, edit, prior, opts)?;
    Ok((render(edit.format, &doc)?, states))
}

fn plan_toml(edit: &FileEdit, bytes: Option<&[u8]>, prior: Option<&FileState>, opts: Options) -> Result<(Vec<u8>, Vec<FragmentState>)> {
    let (mut doc, block) = toml_without_block(&edit.path, bytes)?;
    let states = apply_fragments(&mut doc, edit, prior, opts)?;
    Ok((toml_render(&doc, block.as_deref()), states))
}

fn plan_block(edit: &FileEdit, bytes: Option<&[u8]>, prior: Option<&FileState>, force: bool) -> Result<(Vec<u8>, Vec<FragmentState>)> {
    let text = match bytes {
        Some(b) => String::from_utf8(b.to_vec()).with_context(|| format!("{} is not UTF-8", edit.path.display()))?,
        None => String::new(),
    };
    let body = edit.fragments.first().and_then(|f| f.value.as_str()).context("a text block edit needs one string fragment")?;
    let block = wrap_block(body);
    let (rest, existing) = without_block(&text)?;
    if let Some(existing) = &existing {
        let ours = prior.and_then(|f| f.fragments.first()).and_then(|s| s.written.as_str()) == Some(existing.as_str());
        if !ours && !force {
            bail!("{} has an OwO AI Gateway block OwO AI Gateway did not write or that was edited (re-run with --force to replace it)", edit.path.display());
        }
    }
    let mut out = rest;
    if !out.is_empty() {
        if !out.ends_with('\n') {
            out.push('\n');
        }
        out.push('\n');
    }
    out.push_str(&block);
    let state = FragmentState { path: Vec::new(), original: None, written: Value::String(block), written_digest: None, existing_depth: 0 };
    Ok((out.into_bytes(), vec![state]))
}

fn put_back(doc: &mut impl Doc, s: &FragmentState) {
    match &s.original {
        Some(v) => {
            let _ = doc.put(&s.path, v.clone());
        }
        None => {
            doc.delete(&s.path);
            doc.prune(&s.path, s.existing_depth);
        }
    }
}

/// Puts back every fragment of `f` in `doc`; changed ones only with `force`.
fn revert_fragments(doc: &mut impl Doc, f: &FileState, force: bool) -> Result<()> {
    let conflicts: Vec<String> = f
        .fragments
        .iter()
        .filter(|s| doc.value_at(&s.path).is_some_and(|v| !is_written(s, &v)))
        .map(|s| display_path(&s.path))
        .collect();
    if !conflicts.is_empty() && !force {
        bail!("these entries in {} changed after OwO AI Gateway wrote them; nothing was restored: {} (re-run with --force)", f.path.display(), conflicts.join(", "));
    }
    for s in &f.fragments {
        put_back(doc, s);
    }
    Ok(())
}

fn restore_file(f: &FileState, force: bool, report: &mut Report) -> Result<()> {
    let bytes = read_bytes(&f.path)?;
    let Some(bytes) = bytes else {
        report.warnings.push(format!("{} no longer exists; nothing to restore", f.path.display()));
        return Ok(());
    };
    if f.pristine && sha256(&bytes) == f.written_sha256 {
        match &f.backup {
            Some(b) => {
                let original = std::fs::read(b).with_context(|| format!("backup {} is missing", b.display()))?;
                write_atomic(&f.path, &original)?;
                report.lines.push(format!("restored: {} (byte-for-byte from backup)", f.path.display()));
            }
            None => {
                std::fs::remove_file(&f.path)?;
                report.lines.push(format!("removed:  {} (OwO AI Gateway created it)", f.path.display()));
            }
        }
        return Ok(());
    }
    let out = match f.format {
        Format::TomlBlock => {
            let text = String::from_utf8(bytes).with_context(|| format!("{} is not UTF-8", f.path.display()))?;
            let (rest, existing) = without_block(&text)?;
            let written = f.fragments.first().and_then(|s| s.written.as_str());
            match existing {
                Some(e) if Some(e.as_str()) != written && !force => {
                    bail!("the OwO AI Gateway block in {} was edited; nothing was restored (re-run with --force)", f.path.display())
                }
                _ => rest.into_bytes(),
            }
        }
        Format::Toml => {
            let (mut doc, block) = toml_without_block(&f.path, Some(&bytes))?;
            revert_fragments(&mut doc, f, force)?;
            toml_render(&doc, block.as_deref())
        }
        Format::Json | Format::Yaml => {
            let mut doc = parse(&f.path, f.format, Some(&bytes))?;
            revert_fragments(&mut doc, f, force)?;
            render(f.format, &doc)?
        }
    };
    let empty = out.iter().all(u8::is_ascii_whitespace) || out == b"{}\n";
    if !f.existed && empty {
        std::fs::remove_file(&f.path)?;
        report.lines.push(format!("removed:  {} (OwO AI Gateway created it)", f.path.display()));
    } else {
        write_atomic(&f.path, &out)?;
        report.lines.push(format!("restored: {} (OwO AI Gateway entries removed, other edits kept)", f.path.display()));
    }
    Ok(())
}

pub fn restore(store: &Store, client: &str, force: bool) -> Result<Report> {
    let mut report = Report::default();
    let Some(state) = store.load(client)? else { bail!("the {client} integration is not enabled") };
    for f in &state.files {
        restore_file(f, force, &mut report)?;
    }
    store.clear(client)?;
    report.lines.push(format!("{client} integration removed."));
    Ok(report)
}

/// Takes over an entry someone else wrote: from now on OwO AI Gateway treats the value at `path` as
/// its own fragment with no original, so a later enable may rewrite it and a restore removes
/// it. The file is backed up first (when OwO AI Gateway has no backup of it yet); it is not changed.
pub fn adopt(store: &Store, client: &str, file: &Path, format: Format, path: &[Seg], redact: bool) -> Result<Report> {
    let mut report = Report::default();
    let bytes = read_bytes(file)?.with_context(|| format!("{} does not exist", file.display()))?;
    let current = match format {
        Format::TomlBlock => bail!("a text block has no addressable values"),
        Format::Toml => toml_without_block(file, Some(&bytes))?.0.value_at(path),
        Format::Json | Format::Yaml => parse(file, format, Some(&bytes))?.value_at(path),
    }
    .with_context(|| format!("{} has no `{}`", file.display(), display_path(path)))?;
    let mut state = store.load(client)?.unwrap_or_else(|| ClientState {
        client: client.to_string(),
        owo_version: env!("CARGO_PKG_VERSION").to_string(),
        updated_at_unix: now_unix(),
        files: Vec::new(),
    });
    let index = match state.files.iter().position(|f| f.path == file) {
        Some(i) => i,
        None => {
            let backup = back_up(store, client, file, &bytes)?;
            report.lines.push(format!("backup:   {}", backup.display()));
            state.files.push(FileState {
                path: file.to_path_buf(),
                format,
                existed: true,
                backup: Some(backup),
                written_sha256: sha256(&bytes),
                pristine: false,
                fragments: Vec::new(),
            });
            state.files.len() - 1
        }
    };
    let f = &mut state.files[index];
    // The backup holds the adopted entry, so restoring it would not remove it.
    f.pristine = false;
    let adopted = fragment_state(path, None, &current, path.len().saturating_sub(1), redact);
    match f.fragments.iter_mut().find(|s| s.path == path) {
        Some(s) => *s = adopted,
        None => f.fragments.push(adopted),
    }
    state.updated_at_unix = now_unix();
    store.save(&state)?;
    report.lines.push(format!("adopted:  `{}` in {}", display_path(path), file.display()));
    Ok(report)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn temp(name: &str) -> (PathBuf, Store) {
        let root = std::env::temp_dir().join(format!("owo-managed-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        let store = Store { state_dir: root.join("state"), backups_dir: root.join("backups") };
        (root, store)
    }

    fn edit(path: &Path, value: Value) -> FileEdit {
        FileEdit { path: path.into(), format: Format::Json, fragments: vec![Fragment { path: vec![Seg::key("provider"), Seg::key("owo")], value }] }
    }

    const USER: &str = "{\n  \"theme\": \"dark\",\n  \"provider\": {\n    \"mine\": {}\n  }\n}\n";

    #[test]
    fn json_round_trip_and_later_edits() {
        let (root, store) = temp("json");
        let file = root.join("c.json");
        std::fs::write(&file, USER).unwrap();
        enable(&store, "x", &[edit(&file, json!({"a": 1}))], false).unwrap();
        enable(&store, "x", &[edit(&file, json!({"a": 2}))], false).unwrap();
        let doc: Value = serde_json::from_str(&std::fs::read_to_string(&file).unwrap()).unwrap();
        assert_eq!(doc["provider"]["owo"], json!({"a": 2}));
        assert_eq!(doc["provider"]["mine"], json!({}));
        restore(&store, "x", false).unwrap();
        assert_eq!(std::fs::read_to_string(&file).unwrap(), USER, "untouched file comes back byte-for-byte");

        enable(&store, "x", &[edit(&file, json!({"a": 1}))], false).unwrap();
        let mut doc: Value = serde_json::from_str(&std::fs::read_to_string(&file).unwrap()).unwrap();
        doc["other"] = json!(true);
        std::fs::write(&file, serde_json::to_string(&doc).unwrap()).unwrap();
        restore(&store, "x", false).unwrap();
        let doc: Value = serde_json::from_str(&std::fs::read_to_string(&file).unwrap()).unwrap();
        assert_eq!(doc["other"], true);
        assert!(doc["provider"].get("owo").is_none());
        assert!(doc["provider"].get("mine").is_some());
    }

    #[test]
    fn conflicts_need_force() {
        let (root, store) = temp("conflict");
        let file = root.join("c.json");
        std::fs::write(&file, "{\"provider\": {\"owo\": {\"theirs\": 1}}}").unwrap();
        assert!(enable(&store, "x", &[edit(&file, json!({"a": 1}))], false).is_err());
        enable(&store, "x", &[edit(&file, json!({"a": 1}))], true).unwrap();
        restore(&store, "x", false).unwrap();
        let doc: Value = serde_json::from_str(&std::fs::read_to_string(&file).unwrap()).unwrap();
        assert_eq!(doc["provider"]["owo"], json!({"theirs": 1}), "the replaced value comes back");
    }

    #[test]
    fn created_files_and_containers_are_removed() {
        let (root, store) = temp("created");
        let file = root.join("sub/c.json");
        enable(&store, "x", &[edit(&file, json!(1))], false).unwrap();
        let mut doc: Value = serde_json::from_str(&std::fs::read_to_string(&file).unwrap()).unwrap();
        doc["x"] = json!(1);
        std::fs::write(&file, serde_json::to_string(&doc).unwrap()).unwrap();
        restore(&store, "x", false).unwrap();
        let doc: Value = serde_json::from_str(&std::fs::read_to_string(&file).unwrap()).unwrap();
        assert_eq!(doc, json!({"x": 1}), "the `provider` object OwO AI Gateway created is pruned");
    }

    #[test]
    fn array_items_and_yaml() {
        let (root, store) = temp("items");
        let file = root.join("s.yaml");
        std::fs::write(&file, "rules:\n- id: mine\n  v: 1\n").unwrap();
        let e = FileEdit {
            path: file.clone(),
            format: Format::Yaml,
            fragments: vec![
                Fragment { path: vec![Seg::key("rules"), Seg::item(&[("id", "owo")])], value: json!({"id": "owo", "v": 2}) },
                Fragment { path: vec![Seg::key("models"), Seg::item(&[("p", "owo"), ("m", "a")])], value: json!({"p": "owo", "m": "a"}) },
            ],
        };
        enable(&store, "y", &[e], false).unwrap();
        let doc: Value = serde_yaml::from_str(&std::fs::read_to_string(&file).unwrap()).unwrap();
        assert_eq!(doc["rules"].as_array().unwrap().len(), 2);
        assert_eq!(doc["models"][0]["m"], "a");
        std::fs::write(&file, std::fs::read_to_string(&file).unwrap() + "extra: true\n").unwrap();
        restore(&store, "y", false).unwrap();
        let doc: Value = serde_yaml::from_str(&std::fs::read_to_string(&file).unwrap()).unwrap();
        assert_eq!(doc, json!({"rules": [{"id": "mine", "v": 1}], "extra": true}));
    }

    #[test]
    fn toml_blocks() {
        let (root, store) = temp("toml");
        let file = root.join("config.toml");
        let user = "[ui]\ntheme = \"dark\"\n";
        std::fs::write(&file, user).unwrap();
        let e = |body: &str| FileEdit { path: file.clone(), format: Format::TomlBlock, fragments: vec![Fragment { path: vec![], value: json!(body) }] };
        enable(&store, "g", &[e("[model_providers.owo]\nbase_url = \"x\"")], false).unwrap();
        enable(&store, "g", &[e("[model_providers.owo]\nbase_url = \"y\"")], false).unwrap();
        let text = std::fs::read_to_string(&file).unwrap();
        assert!(text.starts_with(user) && text.contains("base_url = \"y\"") && !text.contains("\"x\""), "{text}");
        std::fs::write(&file, format!("{text}\n[extra]\nk = 1\n")).unwrap();
        restore(&store, "g", false).unwrap();
        assert_eq!(std::fs::read_to_string(&file).unwrap(), format!("{user}\n[extra]\nk = 1\n"));
    }

    fn toml_edit_of(file: &Path, entries: &[(&str, Value)]) -> FileEdit {
        FileEdit {
            path: file.into(),
            format: Format::Toml,
            fragments: entries.iter().map(|(k, v)| Fragment { path: vec![Seg::key("mcp_servers"), Seg::key(k)], value: v.clone() }).collect(),
        }
    }

    #[test]
    fn toml_fragments_keep_comments_and_the_text_block() {
        let (root, store) = temp("tomlfrag");
        let file = root.join("config.toml");
        let user = "model = \"x\" # keep me\n\n[mcp_servers.mine]\ncommand = 'C:\\tools\\mine.exe'\n";
        std::fs::write(&file, user).unwrap();
        // Another integration's block at the end of the same file.
        let block = FileEdit { path: file.clone(), format: Format::TomlBlock, fragments: vec![Fragment { path: vec![], value: json!("[model_providers.owo]\nbase_url = \"b\"") }] };
        enable(&store, "block", &[block], false).unwrap();
        let with_block = std::fs::read_to_string(&file).unwrap();

        let a = json!({"command": "npx", "args": ["-y", "a"], "env": {"K": "v"}});
        let opts = Options { force: false, redact: true };
        enable_with(&store, "t", &[toml_edit_of(&file, &[("a", a.clone())])], opts).unwrap();
        let text = std::fs::read_to_string(&file).unwrap();
        let doc: toml::Table = text.parse().unwrap();
        assert_eq!(doc["mcp_servers"]["a"]["args"].as_array().unwrap().len(), 2);
        assert_eq!(doc["mcp_servers"]["a"]["env"]["K"].as_str(), Some("v"));
        assert!(text.contains("# keep me") && text.contains("[mcp_servers.mine]"), "{text}");
        assert!(text.trim_end().ends_with(BLOCK_END), "the block stays last:\n{text}");
        assert!(!serde_json::to_string(&store.load("t").unwrap()).unwrap().contains("\"v\""), "values are not recorded");

        // A second server, then the first one dropped again.
        let b = json!({"url": "https://e.example/mcp", "http_headers": {"X": "1"}});
        enable_with(&store, "t", &[toml_edit_of(&file, &[("a", a.clone()), ("b", b.clone())])], opts).unwrap();
        enable_with(&store, "t", &[toml_edit_of(&file, &[("b", b)])], opts).unwrap();
        let doc: toml::Table = std::fs::read_to_string(&file).unwrap().parse().unwrap();
        assert!(doc["mcp_servers"].get("a").is_none() && doc["mcp_servers"].get("b").is_some() && doc["mcp_servers"].get("mine").is_some());

        restore(&store, "t", false).unwrap();
        assert_eq!(std::fs::read_to_string(&file).unwrap(), with_block, "nothing but OwO AI Gateway's entries changed, so the file comes back byte-for-byte");
        restore(&store, "block", false).unwrap();
        assert_eq!(std::fs::read_to_string(&file).unwrap(), user);
    }

    #[test]
    fn toml_conflicts_and_created_tables() {
        let (root, store) = temp("tomlconf");
        let file = root.join("config.toml");
        std::fs::write(&file, "[ui]\nx = 1\n").unwrap();
        let a = json!({"command": "a"});
        enable(&store, "t", &[toml_edit_of(&file, &[("a", a.clone())])], false).unwrap();
        let edited = std::fs::read_to_string(&file).unwrap().replace("command = \"a\"", "command = \"theirs\"");
        std::fs::write(&file, &edited).unwrap();
        assert!(enable(&store, "t", &[toml_edit_of(&file, &[("a", a)])], false).is_err(), "a changed entry is a conflict");
        assert!(restore(&store, "t", false).is_err());
        restore(&store, "t", true).unwrap();
        assert_eq!(std::fs::read_to_string(&file).unwrap(), "[ui]\nx = 1\n", "the `mcp_servers` table OwO AI Gateway created is pruned");
    }

    #[test]
    fn restore_keeps_changes_made_between_two_enables() {
        let (root, store) = temp("between");
        let file = root.join("c.json");
        std::fs::write(&file, USER).unwrap();
        enable(&store, "x", &[edit(&file, json!(1))], false).unwrap();
        // Another program rewrites the file, then OwO AI Gateway writes again.
        let mut doc: Value = serde_json::from_str(&std::fs::read_to_string(&file).unwrap()).unwrap();
        doc["theirs"] = json!(true);
        std::fs::write(&file, serde_json::to_string(&doc).unwrap()).unwrap();
        let report = enable(&store, "x", &[edit(&file, json!(2))], false).unwrap();
        assert!(report.lines.iter().any(|l| l.contains("changed since")), "{:?}", report.lines);
        restore(&store, "x", false).unwrap();
        let doc: Value = serde_json::from_str(&std::fs::read_to_string(&file).unwrap()).unwrap();
        assert_eq!(doc["theirs"], true, "the first backup must not be restored over the other program's change");
        assert!(doc["provider"].get("owo").is_none());
    }

    #[test]
    fn existing_empty_containers_survive_restore() {
        let (root, store) = temp("emptycontainer");
        let file = root.join("c.json");
        std::fs::write(&file, "{\"provider\": {}}").unwrap();
        enable(&store, "x", &[edit(&file, json!(1))], false).unwrap();
        let mut doc: Value = serde_json::from_str(&std::fs::read_to_string(&file).unwrap()).unwrap();
        doc["other"] = json!(1);
        std::fs::write(&file, serde_json::to_string(&doc).unwrap()).unwrap();
        restore(&store, "x", false).unwrap();
        let doc: Value = serde_json::from_str(&std::fs::read_to_string(&file).unwrap()).unwrap();
        assert_eq!(doc, json!({"provider": {}, "other": 1}));
    }

    #[test]
    fn adopted_entries_become_owned() {
        let (root, store) = temp("adopt");
        let file = root.join("c.json");
        std::fs::write(&file, "{\"provider\": {\"owo\": {\"secret\": \"s3\"}, \"mine\": 1}}").unwrap();
        let path = vec![Seg::key("provider"), Seg::key("owo")];
        adopt(&store, "x", &file, Format::Json, &path, true).unwrap();
        assert!(!serde_json::to_string(&store.load("x").unwrap()).unwrap().contains("s3"));
        // No conflict: OwO AI Gateway owns the entry now and may rewrite it.
        enable_with(&store, "x", &[edit(&file, json!({"secret": "s3", "v": 2}))], Options { force: false, redact: true }).unwrap();
        restore(&store, "x", false).unwrap();
        let doc: Value = serde_json::from_str(&std::fs::read_to_string(&file).unwrap()).unwrap();
        assert_eq!(doc, json!({"provider": {"mine": 1}}), "restore removes the adopted entry instead of bringing the backup back");
        assert_eq!(read_value(&file, Format::Json, &[Seg::key("provider"), Seg::key("mine")]).unwrap(), Some(json!(1)));
    }
}
