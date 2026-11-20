//! CSV import from other password managers, and the generic layout this crate
//! also exports.
//!
//! # Supported layouts
//!
//! Layouts are taken from each vendor's public documentation; vendors can
//! change them, so unknown columns are ignored and a missing optional column is
//! tolerated for the built-in formats.
//!
//! * **Bitwarden**, header
//!   `folder,favorite,type,name,notes,fields,reprompt,login_uri,login_username,login_password,login_totp`
//!   ([Condition a Bitwarden import](https://bitwarden.com/help/condition-bitwarden-import/)).
//!   `type` is `login` or `note`; any other value is skipped. The folder
//!   becomes a tag.
//! * **LastPass**, header `url,username,password,totp,extra,name,grouping,fav`
//!   (older exports lack `totp`); a row whose `url` is `http://sn` is a secure
//!   note and its text is in `extra`
//!   ([Bitwarden: import from LastPass](https://bitwarden.com/help/import-from-lastpass/),
//!   [LastPass: generic CSV import](https://community.lastpass.com/kb/articles/56-import-data-into-lastpass-using-a-generic-csv-file%EF%BB%BF)).
//!   The group becomes a tag.
//! * **1Password**, header `Title,Url,Username,Password,OTPAuth,Favorite,Archived,Tags,Notes`
//!   (the website column is `Url` or `Website`); logins only
//!   ([1Password: export your data](https://support.1password.com/export/)).
//!   Tags are comma separated.
//! * **Generic**: name the columns yourself with [`ColumnMapping`]. This
//!   crate's own export uses [`ColumnMapping::vault_export`].
//!
//! Columns this crate has no field for (one-time-password seeds, custom
//! fields) are appended to the notes as `column: value` lines so nothing is
//! lost silently. Favourites and archive flags are dropped.
//!
//! # Safety of the data
//!
//! The reader works on borrowed text and wipes each record after use; reports
//! hold row numbers and fixed reasons only, never cell contents. Callers should
//! wipe or delete their copy of the file. Exported CSV opened in a spreadsheet
//! can interpret cells starting with `=`, `+`, `-` or `@` as formulas; the
//! export does not alter values.

use zeroize::Zeroizing;

use crate::csv::CsvReader;
use crate::error::VaultError;
use crate::item::{ItemBody, ItemDraft};

const LASTPASS_NOTE_URL: &str = "http://sn";

/// Which CSV layout to read.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CsvFormat {
    /// Header-mapped import with caller-chosen column names.
    Generic(ColumnMapping),
    /// Bitwarden CSV export.
    Bitwarden,
    /// 1Password CSV export.
    OnePassword,
    /// LastPass CSV export.
    LastPass,
}

/// Column names (matched case-insensitively) for the generic import. Every
/// named column must exist in the file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ColumnMapping {
    /// Title column; required.
    pub title: String,
    /// Login username.
    pub username: Option<String>,
    /// Login password, or the key of an API key row.
    pub password: Option<String>,
    /// Login URLs, one per line.
    pub url: Option<String>,
    /// Notes, or the text of a secure note row.
    pub notes: Option<String>,
    /// Tags separated by commas or semicolons.
    pub tags: Option<String>,
    /// Item kind: empty or `login`, `note`, or `apikey`.
    pub kind: Option<String>,
}

impl ColumnMapping {
    /// The layout written by `Vault::export_csv`.
    pub fn vault_export() -> Self {
        let name = |s: &str| Some(s.to_owned());
        Self {
            title: "title".into(),
            username: name("username"),
            password: name("password"),
            url: name("url"),
            notes: name("notes"),
            tags: name("tags"),
            kind: name("type"),
        }
    }
}

/// Why the CSV file as a whole was rejected.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ImportError {
    /// The file is not UTF-8 (UTF-16 exports must be re-saved as UTF-8).
    #[error("the file is not UTF-8 text")]
    UnsupportedEncoding,
    /// The file has no header row.
    #[error("the file has no header row")]
    NoHeader,
    /// A required column is absent from the header.
    #[error("the header has no column named {0:?}")]
    MissingColumn(String),
}

/// Why one row was not imported. Never includes cell contents.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum SkipReason {
    /// Every mapped cell is empty.
    #[error("the row is empty")]
    Empty,
    /// No title, and no URL to use instead.
    #[error("the row has no title")]
    MissingTitle,
    /// A kind this vault does not store (for example a card or identity).
    #[error("unsupported item type")]
    UnsupportedType,
    /// A quoted cell is never closed.
    #[error("the row is malformed (unclosed quote)")]
    Malformed,
    /// The row violates an item limit.
    #[error("invalid item: {0}")]
    Invalid(&'static str),
}

/// One row that was not imported.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SkippedRow {
    /// 1-based position among the data rows, blank lines not counted.
    pub row: usize,
    /// Why it was skipped.
    pub reason: SkipReason,
}

/// What an import did.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ImportReport {
    /// Rows turned into items.
    pub imported: usize,
    /// Rows left out, with reasons.
    pub skipped: Vec<SkippedRow>,
}

/// Parsed rows: validated drafts ready to add, plus the report.
#[derive(Debug)]
pub struct ImportBatch {
    /// One draft per imported row, in file order.
    pub drafts: Vec<ItemDraft>,
    /// Counts and skipped rows.
    pub report: ImportReport,
}

/// Candidate header names per target; the first present one is used.
struct Layout {
    title: Vec<String>,
    username: Vec<String>,
    password: Vec<String>,
    url: Vec<String>,
    notes: Vec<String>,
    tags: Vec<String>,
    kind: Vec<String>,
    /// Columns appended to the notes as `column: value`.
    append: Vec<String>,
    lastpass_notes: bool,
    /// Named columns must exist (generic) or may be missing (built-in).
    strict: bool,
}

fn names(list: &[&str]) -> Vec<String> {
    list.iter().map(|s| (*s).to_owned()).collect()
}

fn one(name: &Option<String>) -> Vec<String> {
    name.iter().map(|n| n.trim().to_lowercase()).collect()
}

impl CsvFormat {
    fn layout(&self) -> Layout {
        let base = Layout {
            title: vec![],
            username: vec![],
            password: vec![],
            url: vec![],
            notes: vec![],
            tags: vec![],
            kind: vec![],
            append: vec![],
            lastpass_notes: false,
            strict: false,
        };
        match self {
            Self::Generic(m) => Layout {
                title: one(&Some(m.title.clone())),
                username: one(&m.username),
                password: one(&m.password),
                url: one(&m.url),
                notes: one(&m.notes),
                tags: one(&m.tags),
                kind: one(&m.kind),
                strict: true,
                ..base
            },
            Self::Bitwarden => Layout {
                title: names(&["name"]),
                username: names(&["login_username"]),
                password: names(&["login_password"]),
                url: names(&["login_uri"]),
                notes: names(&["notes"]),
                tags: names(&["folder"]),
                kind: names(&["type"]),
                append: names(&["fields", "login_totp"]),
                ..base
            },
            Self::OnePassword => Layout {
                title: names(&["title"]),
                username: names(&["username"]),
                password: names(&["password"]),
                url: names(&["url", "website"]),
                notes: names(&["notes"]),
                tags: names(&["tags"]),
                append: names(&["otpauth"]),
                ..base
            },
            Self::LastPass => Layout {
                title: names(&["name"]),
                username: names(&["username"]),
                password: names(&["password"]),
                url: names(&["url"]),
                notes: names(&["extra"]),
                tags: names(&["grouping"]),
                append: names(&["totp"]),
                lastpass_notes: true,
                ..base
            },
        }
    }
}

/// Column positions in the actual file.
struct Resolved {
    title: usize,
    username: Option<usize>,
    password: Option<usize>,
    url: Option<usize>,
    notes: Option<usize>,
    tags: Option<usize>,
    kind: Option<usize>,
    append: Vec<(usize, String)>,
}

fn find(headers: &[String], candidates: &[String]) -> Option<usize> {
    candidates
        .iter()
        .find_map(|c| headers.iter().position(|h| h.trim().to_lowercase() == *c))
}

fn optional(
    headers: &[String],
    candidates: &[String],
    strict: bool,
) -> Result<Option<usize>, ImportError> {
    match (find(headers, candidates), candidates.first()) {
        (None, Some(name)) if strict => Err(ImportError::MissingColumn(name.clone())),
        (found, _) => Ok(found),
    }
}

fn resolve(headers: &[String], layout: &Layout) -> Result<Resolved, ImportError> {
    let title = find(headers, &layout.title).ok_or_else(|| {
        ImportError::MissingColumn(layout.title.first().cloned().unwrap_or_default())
    })?;
    let append = layout
        .append
        .iter()
        .filter_map(|name| find(headers, std::slice::from_ref(name)))
        .map(|i| (i, headers[i].trim().to_owned()))
        .collect();
    Ok(Resolved {
        title,
        username: optional(headers, &layout.username, layout.strict)?,
        password: optional(headers, &layout.password, layout.strict)?,
        url: optional(headers, &layout.url, layout.strict)?,
        notes: optional(headers, &layout.notes, layout.strict)?,
        tags: optional(headers, &layout.tags, layout.strict)?,
        kind: optional(headers, &layout.kind, layout.strict)?,
        append,
    })
}

fn text(csv: &[u8]) -> Result<&str, ImportError> {
    if csv.starts_with(&[0xff, 0xfe]) || csv.starts_with(&[0xfe, 0xff]) {
        return Err(ImportError::UnsupportedEncoding);
    }
    std::str::from_utf8(csv).map_err(|_| ImportError::UnsupportedEncoding)
}

/// Guesses the layout from the header row, `None` when it matches none of the
/// built-in formats (use [`CsvFormat::Generic`] then).
pub fn detect_format(csv: &[u8]) -> Option<CsvFormat> {
    let text = text(csv).ok()?;
    let header = CsvReader::new(text).find(|r| r.fields.iter().any(|f| !f.trim().is_empty()))?;
    let has = |name: &str| {
        header
            .fields
            .iter()
            .any(|h| h.trim().eq_ignore_ascii_case(name))
    };
    if has("login_uri") && has("login_username") && has("name") {
        Some(CsvFormat::Bitwarden)
    } else if has("grouping") && has("extra") && has("url") && has("name") {
        Some(CsvFormat::LastPass)
    } else if has("title") && has("password") && (has("url") || has("website")) {
        Some(CsvFormat::OnePassword)
    } else {
        None
    }
}

/// Reads `csv` in `format` into validated drafts and a report. Never fails on
/// a bad row, only on an unreadable file or a missing required column.
pub fn parse_csv(csv: &[u8], format: &CsvFormat) -> Result<ImportBatch, ImportError> {
    let layout = format.layout();
    let mut reader = CsvReader::new(text(csv)?)
        .filter(|r| !(r.fields.len() == 1 && r.fields[0].is_empty() && !r.unterminated));
    let header = reader.next().ok_or(ImportError::NoHeader)?;
    let cols = resolve(&header.fields, &layout)?;
    let mut batch = ImportBatch {
        drafts: Vec::new(),
        report: ImportReport::default(),
    };
    for (index, record) in reader.enumerate() {
        let row = index + 1;
        let outcome = if record.unterminated {
            Err(SkipReason::Malformed)
        } else {
            row_to_draft(&record.fields, &cols, &layout)
        };
        match outcome {
            Ok(draft) => batch.drafts.push(draft),
            Err(reason) => batch.report.skipped.push(SkippedRow { row, reason }),
        }
    }
    batch.report.imported = batch.drafts.len();
    Ok(batch)
}

fn row_to_draft(
    fields: &[String],
    cols: &Resolved,
    layout: &Layout,
) -> Result<ItemDraft, SkipReason> {
    let get = |col: Option<usize>| col.and_then(|i| fields.get(i)).map_or("", String::as_str);
    let title = fields.get(cols.title).map_or("", String::as_str);
    let (username, password, url) = (get(cols.username), get(cols.password), get(cols.url));
    let mut notes = Zeroizing::new(get(cols.notes).to_owned());
    for (index, header) in &cols.append {
        let value = fields.get(*index).map_or("", String::as_str);
        if !value.trim().is_empty() {
            if !notes.is_empty() {
                notes.push('\n');
            }
            notes.push_str(&format!("{header}: {value}"));
        }
    }
    let tags_cell = get(cols.tags);
    let all_empty = [title, username, password, url, notes.as_str(), tags_cell]
        .iter()
        .all(|c| c.trim().is_empty());
    if all_empty {
        return Err(SkipReason::Empty);
    }

    let kind = if layout.lastpass_notes && url.trim() == LASTPASS_NOTE_URL {
        Kind::Note
    } else {
        classify(get(cols.kind))?
    };
    let urls: Vec<String> = url.lines().map(str::to_owned).collect();
    let title = match (title.trim(), kind) {
        ("", Kind::Login) => urls
            .iter()
            .map(|u| u.trim())
            .find(|u| !u.is_empty())
            .ok_or(SkipReason::MissingTitle)?
            .to_owned(),
        ("", _) => return Err(SkipReason::MissingTitle),
        (title, _) => title.to_owned(),
    };
    let notes = std::mem::take(&mut *notes);
    let body = match kind {
        Kind::Login => ItemBody::Login {
            username: username.to_owned(),
            password: password.to_owned(),
            urls,
            notes,
        },
        Kind::Note => ItemBody::SecureNote { text: notes },
        Kind::ApiKey => ItemBody::ApiKey {
            value: password.to_owned(),
            notes,
        },
    };
    ItemDraft {
        title,
        tags: tags_cell.split([',', ';']).map(str::to_owned).collect(),
        body,
    }
    .normalized()
    .map_err(|err| match err {
        VaultError::InvalidItem(why) => SkipReason::Invalid(why),
        _ => SkipReason::Invalid("rejected"),
    })
}

#[derive(Clone, Copy)]
enum Kind {
    Login,
    Note,
    ApiKey,
}

fn classify(cell: &str) -> Result<Kind, SkipReason> {
    match cell.trim().to_lowercase().as_str() {
        "" | "login" => Ok(Kind::Login),
        "note" | "securenote" | "secure_note" | "secure note" => Ok(Kind::Note),
        "apikey" | "api_key" | "api key" | "token" => Ok(Kind::ApiKey),
        _ => Err(SkipReason::UnsupportedType),
    }
}
