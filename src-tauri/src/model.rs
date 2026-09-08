//! The domain: six records, the vocabularies both surfaces speak, and the serialisable
//! dataset underneath them.
//!
//! **Every enum a surface shows lives here.** If the board draws a "Negotiation" column,
//! then `crm move` takes that word and `crm -h` names it — one list, in the core, and a
//! test pins all three against it. A vocabulary that lives in the window is a vocabulary
//! the first agent on the CLI has to learn from a screenshot.
//!
//! Nothing in this file reads a clock, a file or an environment variable. Time arrives as
//! a value ([`Now`]), which is what lets a test place a task in the past without waiting.

// The vocabulary lands here in M1; the verbs that consume it land in M2. Until then some
// of these parsers have only their tests as callers, and a warning per word would train
// everyone to ignore the warnings that matter.
#![allow(dead_code)]

use serde::{Deserialize, Serialize};

/// A record's identity: a slug derived from its name and uniquified — `acme`, `acme-2`.
///
/// Typable on purpose. `crm show acme` is then an exact match, and the ambiguity
/// machinery only runs when it genuinely has to. Stable across a rename: the label
/// changes, the id does not, so nothing that points at a record is ever orphaned by an
/// edit.
pub type Id = String;

/// Milliseconds since the Unix epoch. A plain integer because the core does not do
/// calendar arithmetic on it — [`Date`] does, and it arrives already computed.
pub type Timestamp = i64;

// MARK: - Time, as a value

/// The clock, handed to the core rather than read by it.
///
/// `today` is **local** and cannot be derived from `at` without a timezone, which is
/// platform state a pure core has no business reading. The caller computes both and
/// passes them in — which is also why a test can put a task three days overdue without
/// sleeping.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Now {
    pub at: Timestamp,
    pub today: Date,
}

/// A civil date — what a due date actually is. Not an instant: "due Tuesday" does not
/// become a different day because someone opened the app from another timezone.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct Date {
    pub y: i32,
    pub m: u32,
    pub d: u32,
}

impl Date {
    pub fn new(y: i32, m: u32, d: u32) -> Date {
        Date { y, m, d }
    }

    /// `YYYY-MM-DD`, and nothing else. A due date an agent types has one spelling, so
    /// there is one thing to document and one thing to get wrong.
    pub fn parse(s: &str) -> Option<Date> {
        // ASCII first: the slices below are byte ranges, and a multi-byte character
        // inside a ten-byte string would make one of them fall mid-character and panic.
        // A date is digits and hyphens, so this rejects nothing that was ever valid.
        if !s.is_ascii() {
            return None;
        }
        let b = s.as_bytes();
        if b.len() != 10 || b[4] != b'-' || b[7] != b'-' {
            return None;
        }
        let y: i32 = s[0..4].parse().ok()?;
        let m: u32 = s[5..7].parse().ok()?;
        let d: u32 = s[8..10].parse().ok()?;
        let date = Date { y, m, d };
        // Reject 2026-02-30 rather than silently rolling it into March.
        (m >= 1 && m <= 12 && d >= 1 && d <= days_in_month(y, m)).then_some(date)
    }

    /// Days between two dates — negative when `self` is earlier. The only arithmetic the
    /// due buckets need.
    pub fn days_until(&self, other: Date) -> i64 {
        other.to_days() - self.to_days()
    }

    /// Days since 1970-01-01, by Howard Hinnant's `days_from_civil`. Fifteen lines and no
    /// dependency; a calendar crate would be a larger surface than the problem.
    fn to_days(&self) -> i64 {
        let y = if self.m <= 2 { self.y - 1 } else { self.y } as i64;
        let era = if y >= 0 { y } else { y - 399 } / 400;
        let yoe = y - era * 400;
        let m = self.m as i64;
        let d = self.d as i64;
        let doy = (153 * (if m > 2 { m - 3 } else { m + 9 }) + 2) / 5 + d - 1;
        let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
        era * 146_097 + doe - 719_468
    }

    pub fn to_string_iso(&self) -> String {
        format!("{:04}-{:02}-{:02}", self.y, self.m, self.d)
    }
}

fn days_in_month(y: i32, m: u32) -> u32 {
    match m {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 if (y % 4 == 0 && y % 100 != 0) || y % 400 == 0 => 29,
        2 => 28,
        _ => 0,
    }
}

// MARK: - Who wrote it

/// Who did a thing: the person at the window, or a named agent.
///
/// **Keyed on the agent's `id`, never its display name.** The id is immutable for the
/// agent's whole lifetime; the name is unique but re-pointable, so a rename would silently
/// re-attribute every line the agent ever wrote. The window looks the name up to draw it.
///
/// Attribution is the feature: a shared log where you cannot tell who said what is a log
/// two parties stop trusting.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum Actor {
    Human,
    Agent { id: String },
}

impl Actor {
    /// The caller id `clappkit::app::spawn_ipc` hands the handler: present for an agent,
    /// absent for the person. This is the only place that decision is made.
    pub fn from_caller(caller: Option<&str>) -> Actor {
        match caller.filter(|id| !id.is_empty()) {
            Some(id) => Actor::Agent { id: id.to_string() },
            None => Actor::Human,
        }
    }

    pub fn is_human(&self) -> bool {
        matches!(self, Actor::Human)
    }
}

// MARK: - The pipeline vocabulary

/// The four **open** stages, in order.
///
/// Fixed in v1: editable stages would make `crm -h` lie the moment someone renamed a
/// column. When they do become editable, `crm move` must fail by printing the *current*
/// list rather than a compiled-in one.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Stage {
    Lead,
    Qualified,
    Proposal,
    Negotiation,
}

impl Stage {
    /// The vocabulary, in pipeline order. **This list is the source of truth**: the
    /// snapshot carries it, `crm -h` names it, `crm move` accepts it, and a test pins all
    /// three against this one array.
    pub const ALL: [Stage; 4] = [Stage::Lead, Stage::Qualified, Stage::Proposal, Stage::Negotiation];

    /// The word an agent types and the snapshot carries.
    pub fn word(&self) -> &'static str {
        match self {
            Stage::Lead => "lead",
            Stage::Qualified => "qualified",
            Stage::Proposal => "proposal",
            Stage::Negotiation => "negotiation",
        }
    }

    /// The word a person reads on a column header.
    pub fn label(&self) -> &'static str {
        match self {
            Stage::Lead => "Lead",
            Stage::Qualified => "Qualified",
            Stage::Proposal => "Proposal",
            Stage::Negotiation => "Negotiation",
        }
    }

    /// Case-folded, because parsers meet data rather than specs — an agent that types
    /// `Proposal` means the stage, and refusing it teaches nothing.
    pub fn parse(word: &str) -> Option<Stage> {
        let w = word.trim().to_ascii_lowercase();
        Stage::ALL.into_iter().find(|s| s.word() == w)
    }
}

/// Open, or closed one of two ways.
///
/// **Status is a different axis from [`Stage`], and this is the part most likely to be got
/// wrong.** Won and Lost are statuses, not stages: closing a deal leaves `stage` exactly
/// where it was, which is what makes conversion reporting possible later — "how many deals
/// we won out of Negotiation" is unanswerable if winning erased the stage.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Status {
    Open,
    Won,
    Lost,
}

impl Status {
    pub fn word(&self) -> &'static str {
        match self {
            Status::Open => "open",
            Status::Won => "won",
            Status::Lost => "lost",
        }
    }

    pub fn is_open(&self) -> bool {
        matches!(self, Status::Open)
    }
}

/// What `crm move <deal> <word>` was asked to do. One word, two axes — which is exactly
/// the confusion this type exists to make impossible to write by accident.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MoveTarget {
    /// A stage. On a closed deal this also **reopens** it.
    To(Stage),
    /// Won or Lost. Sets the status and **leaves the stage alone**.
    Close(Status),
}

impl MoveTarget {
    /// Every word `crm move` accepts: the four stages, then the two closings. The manifest
    /// promises "change a deal's stage, or close it won or lost", and this is that
    /// sentence as code.
    pub fn vocabulary() -> Vec<&'static str> {
        let mut words: Vec<&'static str> = Stage::ALL.iter().map(|s| s.word()).collect();
        words.push(Status::Won.word());
        words.push(Status::Lost.word());
        words
    }

    pub fn parse(word: &str) -> Option<MoveTarget> {
        let w = word.trim().to_ascii_lowercase();
        if let Some(stage) = Stage::parse(&w) {
            return Some(MoveTarget::To(stage));
        }
        match w.as_str() {
            "won" => Some(MoveTarget::Close(Status::Won)),
            "lost" => Some(MoveTarget::Close(Status::Lost)),
            _ => None,
        }
    }
}

/// A board column: the four stages, then the two closing ones. Six columns over four
/// stages — the shape that falls out of stage and status being different axes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Column {
    Stage(Stage),
    Closed(Status),
}

impl Column {
    pub const ALL: [Column; 6] = [
        Column::Stage(Stage::Lead),
        Column::Stage(Stage::Qualified),
        Column::Stage(Stage::Proposal),
        Column::Stage(Stage::Negotiation),
        Column::Closed(Status::Won),
        Column::Closed(Status::Lost),
    ];

    pub fn key(&self) -> &'static str {
        match self {
            Column::Stage(s) => s.word(),
            Column::Closed(st) => st.word(),
        }
    }

    pub fn label(&self) -> &'static str {
        match self {
            Column::Stage(s) => s.label(),
            Column::Closed(Status::Won) => "Won",
            Column::Closed(Status::Lost) => "Lost",
            Column::Closed(Status::Open) => "Open",
        }
    }

    /// Whether a deal belongs in this column. An open deal is drawn under its stage; a
    /// closed one under how it closed, whatever stage it stopped at.
    pub fn holds(&self, deal: &Deal) -> bool {
        match self {
            Column::Stage(s) => deal.status.is_open() && deal.stage == *s,
            Column::Closed(st) => deal.status == *st,
        }
    }
}

/// What a logged line is.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ActivityKind {
    Call,
    Email,
    Meeting,
    Note,
}

impl ActivityKind {
    pub const ALL: [ActivityKind; 4] =
        [ActivityKind::Call, ActivityKind::Email, ActivityKind::Meeting, ActivityKind::Note];

    pub fn word(&self) -> &'static str {
        match self {
            ActivityKind::Call => "call",
            ActivityKind::Email => "email",
            ActivityKind::Meeting => "meeting",
            ActivityKind::Note => "note",
        }
    }

    pub fn parse(word: &str) -> Option<ActivityKind> {
        let w = word.trim().to_ascii_lowercase();
        ActivityKind::ALL.into_iter().find(|k| k.word() == w)
    }
}

/// How the shared list is ordered. **Sort is state, not a caller's request**: a control
/// that only reorders the page you happen to hold is a lie about the data underneath it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Sort {
    /// Most recently touched first. The default, because it is the one order that is
    /// useful before you know what you are looking for.
    Updated,
    Name,
    Value,
}

impl Sort {
    pub const ALL: [Sort; 3] = [Sort::Updated, Sort::Name, Sort::Value];

    pub fn word(&self) -> &'static str {
        match self {
            Sort::Updated => "updated",
            Sort::Name => "name",
            Sort::Value => "value",
        }
    }

    pub fn parse(word: &str) -> Option<Sort> {
        let w = word.trim().to_ascii_lowercase();
        Sort::ALL.into_iter().find(|s| s.word() == w)
    }
}

/// Which of the three record types something is. The `kind` an agent types, and the one
/// `focus` carries.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Kind {
    Company,
    Contact,
    Deal,
}

impl Kind {
    pub const ALL: [Kind; 3] = [Kind::Company, Kind::Contact, Kind::Deal];

    pub fn word(&self) -> &'static str {
        match self {
            Kind::Company => "company",
            Kind::Contact => "contact",
            Kind::Deal => "deal",
        }
    }

    pub fn parse(word: &str) -> Option<Kind> {
        let w = word.trim().to_ascii_lowercase();
        Kind::ALL.into_iter().find(|k| k.word() == w)
    }
}

// MARK: - Money

/// An amount in **minor units** and its ISO 4217 currency.
///
/// Never `f64`. Summing an empty list of floats yields `-0.0`, so an empty pipeline prints
/// `$-0.00` — and nobody finds that bug twice. An integer has no negative zero, so the
/// failure is not fixed here, it is *absent*.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Money {
    /// Minor units: cents, fils, or whole yen. See [`Money::exponent`].
    pub amount: i64,
    /// ISO 4217, upper-cased on the way in.
    pub currency: String,
}

impl Money {
    pub fn new(amount: i64, currency: &str) -> Money {
        Money { amount, currency: currency.trim().to_ascii_uppercase() }
    }

    /// How many decimal places this currency has. Most have two; a handful have none and a
    /// handful have three, and formatting ¥4,500 as `¥45.00` is the kind of error that
    /// makes a person stop trusting the totals.
    pub fn exponent(currency: &str) -> u32 {
        match currency {
            "BIF" | "CLP" | "DJF" | "GNF" | "ISK" | "JPY" | "KMF" | "KRW" | "PYG" | "RWF"
            | "UGX" | "UYI" | "VND" | "VUV" | "XAF" | "XOF" | "XPF" => 0,
            "BHD" | "IQD" | "JOD" | "KWD" | "LYD" | "OMR" | "TND" => 3,
            _ => 2,
        }
    }

    /// The amount as a person reads it — `45000.00`, `4500`, `45.000` — without the
    /// symbol, which is the window's business. In the core because both surfaces show
    /// totals and two formatters is two answers.
    pub fn format(&self) -> String {
        let places = Money::exponent(&self.currency) as usize;
        if places == 0 {
            return self.amount.to_string();
        }
        let unit = 10i64.pow(places as u32);
        let sign = if self.amount < 0 { "-" } else { "" };
        // `unsigned_abs` rather than `abs`: i64::MIN has no positive counterpart, and a
        // panic in a totals row would be an odd way to find that out.
        let magnitude = self.amount.unsigned_abs();
        let major = magnitude / unit as u64;
        let minor = magnitude % unit as u64;
        format!("{sign}{major}.{minor:0places$}")
    }
}

/// Totals for a set of deals, **grouped by currency and never summed across them**.
///
/// We hold no rate source, and this app reaches nothing outside the machine. Inventing a
/// conversion would be worse than showing two numbers: a wrong total is believed, and two
/// honest ones are merely read twice.
pub fn total_by_currency<'a>(deals: impl Iterator<Item = &'a Deal>) -> Vec<Money> {
    let mut totals: Vec<Money> = Vec::new();
    for deal in deals {
        let Some(value) = &deal.value else { continue };
        match totals.iter_mut().find(|t| t.currency == value.currency) {
            Some(t) => t.amount = t.amount.saturating_add(value.amount),
            None => totals.push(value.clone()),
        }
    }
    // A stable order, so the board's columns do not reshuffle between snapshots.
    totals.sort_by(|a, b| a.currency.cmp(&b.currency));
    totals
}

// MARK: - The six records

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Company {
    pub id: Id,
    pub name: String,
    pub domain: Option<String>,
    #[serde(default)]
    pub tags: Vec<String>,
    pub notes: Option<String>,
    /// Set, never unset by a delete. Archiving is reversible; there is no hard delete,
    /// because an agent holding one and a bad fuzzy match is an unrecoverable afternoon.
    pub archived_at: Option<Timestamp>,
    pub updated_at: Timestamp,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Contact {
    pub id: Id,
    pub name: String,
    pub email: Option<String>,
    pub phone: Option<String>,
    pub title: Option<String>,
    pub company_id: Option<Id>,
    #[serde(default)]
    pub tags: Vec<String>,
    pub archived_at: Option<Timestamp>,
    pub updated_at: Timestamp,
}

/// The spine.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Deal {
    pub id: Id,
    pub title: String,
    pub company_id: Option<Id>,
    #[serde(default)]
    pub contact_ids: Vec<Id>,
    pub value: Option<Money>,
    /// One of the four **open** stages, whatever the status. A won deal remembers where
    /// it was won from.
    pub stage: Stage,
    pub status: Status,
    /// Carried from day one, read by nothing in v1. The data shape is multi-ready; the
    /// resolution logic never learns about it. A test pins that it survives the store,
    /// because an untested seam is usually a subtly wrong one.
    pub pipeline_id: Id,
    pub opened_at: Timestamp,
    pub closed_at: Option<Timestamp>,
    pub archived_at: Option<Timestamp>,
    pub updated_at: Timestamp,
}

/// The immutable log. Appended, never edited — which is also what makes the JSON store
/// survivable: the hot path is pushing onto a list, not rewriting a graph.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Activity {
    pub id: Id,
    pub kind: ActivityKind,
    pub body: String,
    pub at: Timestamp,
    /// The records this line is about — any mix of company, contact and deal ids.
    #[serde(default)]
    pub links: Vec<Id>,
    pub by: Actor,
}

/// The "next step". The only thing that can wake an agent.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Task {
    pub id: Id,
    pub what: String,
    pub due: Date,
    #[serde(default)]
    pub links: Vec<Id>,
    pub done_at: Option<Timestamp>,
    pub by: Actor,
}

/// A record, not a hardcoded enum — exactly one instance in v1.
///
/// Many pipelines would make stage names ambiguous for the agent and give the board a mode
/// an agent can yank out from under the person mid-drag. So: the cheap structural half,
/// and none of the behavioural half.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Pipeline {
    pub id: Id,
    pub name: String,
    pub stages: Vec<Stage>,
}

impl Pipeline {
    /// The one pipeline, seeded at first run.
    pub fn seed() -> Pipeline {
        Pipeline {
            id: "sales".to_string(),
            name: "Sales".to_string(),
            stages: Stage::ALL.to_vec(),
        }
    }
}

// MARK: - Ids

/// A typable id from a name: lower-cased, non-alphanumerics collapsed to single hyphens,
/// trimmed. `"Acme Corp."` → `"acme-corp"`.
///
/// Deliberately ASCII-folding nothing: a name with no ASCII alphanumerics at all yields an
/// empty slug, and [`unique_id`] gives it a stem instead of producing an id that is a bare
/// number and reads like an index.
pub fn slug(name: &str) -> String {
    let mut out = String::new();
    let mut pending_dash = false;
    for ch in name.chars() {
        if ch.is_ascii_alphanumeric() {
            if pending_dash && !out.is_empty() {
                out.push('-');
            }
            pending_dash = false;
            out.push(ch.to_ascii_lowercase());
        } else {
            pending_dash = true;
        }
    }
    out
}

/// [`slug`], uniquified against ids already taken: `acme`, then `acme-2`, `acme-3`.
///
/// The suffix starts at 2 because the first one is not "the first of several" until a
/// second arrives — and renaming it retroactively would break every id already written
/// down.
pub fn unique_id(name: &str, taken: &dyn Fn(&str) -> bool, fallback: &str) -> Id {
    let base = {
        let s = slug(name);
        if s.is_empty() {
            fallback.to_string()
        } else {
            s
        }
    };
    if !taken(&base) {
        return base;
    }
    for n in 2..=u32::MAX {
        let candidate = format!("{base}-{n}");
        if !taken(&candidate) {
            return candidate;
        }
    }
    unreachable!("u32::MAX ids with one stem is not a state this app can reach")
}

// MARK: - The dataset

/// Everything that is persisted: the records, the one pipeline, and the shared view state.
///
/// The view is in here on purpose. What is currently *being looked at* is as real as the
/// data and survives a restart — the board a person left open is the board they come back
/// to, and their agent can still see it.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Db {
    #[serde(default)]
    pub companies: Vec<Company>,
    #[serde(default)]
    pub contacts: Vec<Contact>,
    #[serde(default)]
    pub deals: Vec<Deal>,
    #[serde(default)]
    pub activities: Vec<Activity>,
    #[serde(default)]
    pub tasks: Vec<Task>,
    #[serde(default = "seed_pipelines")]
    pub pipelines: Vec<Pipeline>,
    #[serde(default)]
    pub view: crate::state::View,
}

fn seed_pipelines() -> Vec<Pipeline> {
    vec![Pipeline::seed()]
}

impl Default for Db {
    fn default() -> Db {
        Db {
            companies: Vec::new(),
            contacts: Vec::new(),
            deals: Vec::new(),
            activities: Vec::new(),
            tasks: Vec::new(),
            pipelines: seed_pipelines(),
            view: crate::state::View::default(),
        }
    }
}

impl Db {
    /// The active pipeline. v1 has exactly one, seeded at first run; a `Db` that somehow
    /// carries none is repaired rather than refused, because losing the pipeline record
    /// must not cost anyone their deals.
    pub fn pipeline(&self) -> Pipeline {
        self.pipelines.first().cloned().unwrap_or_else(Pipeline::seed)
    }

    pub fn company(&self, id: &str) -> Option<&Company> {
        self.companies.iter().find(|c| c.id == id)
    }

    pub fn contact(&self, id: &str) -> Option<&Contact> {
        self.contacts.iter().find(|c| c.id == id)
    }

    pub fn deal(&self, id: &str) -> Option<&Deal> {
        self.deals.iter().find(|d| d.id == id)
    }

    /// Is this id spoken for, in any record type? Ids are unique across the whole dataset,
    /// not per type — `crm show acme` must not have to be told which kind of thing `acme`
    /// is.
    pub fn id_taken(&self, id: &str) -> bool {
        self.companies.iter().any(|c| c.id == id)
            || self.contacts.iter().any(|c| c.id == id)
            || self.deals.iter().any(|d| d.id == id)
    }

    /// Every **active** deal — the board, the counts and default `find` all exclude
    /// archived records.
    pub fn active_deals(&self) -> impl Iterator<Item = &Deal> {
        self.deals.iter().filter(|d| d.archived_at.is_none())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // MARK: - The vocabulary that the surfaces share

    #[test]
    fn the_stage_vocabulary_is_four_open_stages_in_pipeline_order() {
        let words: Vec<&str> = Stage::ALL.iter().map(|s| s.word()).collect();
        assert_eq!(words, ["lead", "qualified", "proposal", "negotiation"]);
    }

    /// Won and Lost are statuses. If either ever appears in `Stage`, the board grows a
    /// column that `crm move` treats as a stage and conversion reporting quietly dies.
    #[test]
    fn won_and_lost_are_not_stages() {
        for word in ["won", "lost", "open"] {
            assert!(Stage::parse(word).is_none(), "`{word}` must not parse as a stage");
        }
    }

    #[test]
    fn the_board_draws_six_columns_over_four_stages() {
        let keys: Vec<&str> = Column::ALL.iter().map(|c| c.key()).collect();
        assert_eq!(keys, ["lead", "qualified", "proposal", "negotiation", "won", "lost"]);
        let stage_words: Vec<&str> = Stage::ALL.iter().map(|s| s.word()).collect();
        assert_eq!(&keys[..4], &stage_words[..], "the first four columns are the stages, in order");
    }

    /// `crm move` accepts exactly the stages plus the two closings — the manifest's
    /// promise, "change a deal's stage, or close it won or lost", as a list.
    #[test]
    fn move_accepts_the_stages_and_the_two_closings_and_nothing_else() {
        assert_eq!(
            MoveTarget::vocabulary(),
            ["lead", "qualified", "proposal", "negotiation", "won", "lost"]
        );
        for word in MoveTarget::vocabulary() {
            assert!(MoveTarget::parse(word).is_some(), "`{word}` is advertised and must parse");
        }
        for word in ["open", "closed", "", "negotiate", "win"] {
            assert!(MoveTarget::parse(word).is_none(), "`{word}` must not parse");
        }
    }

    /// Parsers meet data, not specs. An agent that types `Proposal` means the stage.
    #[test]
    fn the_vocabularies_are_case_folded_and_trimmed() {
        assert_eq!(Stage::parse("  Proposal "), Some(Stage::Proposal));
        assert_eq!(MoveTarget::parse("WON"), Some(MoveTarget::Close(Status::Won)));
        assert_eq!(ActivityKind::parse("Email"), Some(ActivityKind::Email));
        assert_eq!(Sort::parse("Updated"), Some(Sort::Updated));
        assert_eq!(Kind::parse(" Deal"), Some(Kind::Deal));
    }

    /// A word that rides the snapshot must serialise as that same word, or the window and
    /// the CLI end up speaking two dialects of one vocabulary.
    #[test]
    fn every_vocabulary_serialises_as_the_word_agents_type() {
        for s in Stage::ALL {
            assert_eq!(serde_json::to_value(s).unwrap(), serde_json::json!(s.word()));
        }
        for st in [Status::Open, Status::Won, Status::Lost] {
            assert_eq!(serde_json::to_value(st).unwrap(), serde_json::json!(st.word()));
        }
        for k in ActivityKind::ALL {
            assert_eq!(serde_json::to_value(k).unwrap(), serde_json::json!(k.word()));
        }
        for s in Sort::ALL {
            assert_eq!(serde_json::to_value(s).unwrap(), serde_json::json!(s.word()));
        }
        for k in Kind::ALL {
            assert_eq!(serde_json::to_value(k).unwrap(), serde_json::json!(k.word()));
        }
    }

    // MARK: - Attribution

    #[test]
    fn a_caller_id_is_an_agent_and_its_absence_is_the_person() {
        assert_eq!(Actor::from_caller(Some("agent-7")), Actor::Agent { id: "agent-7".into() });
        assert_eq!(Actor::from_caller(None), Actor::Human);
        // An empty caller is the person, not an agent with no name: an id is either there
        // or it is not, and `Agent { id: "" }` would attribute a line to nobody.
        assert_eq!(Actor::from_caller(Some("")), Actor::Human);
    }

    // MARK: - Money

    /// The whole reason `amount` is an integer. An empty pipeline has no deals to sum, and
    /// the total it prints must be `0.00` — `-0.00` is what a float gives you.
    #[test]
    fn an_empty_pipeline_totals_nothing_rather_than_negative_zero() {
        let empty: Vec<Deal> = Vec::new();
        assert!(total_by_currency(empty.iter()).is_empty());

        let zero = Money::new(0, "USD");
        assert_eq!(zero.format(), "0.00");
        assert!(!zero.format().starts_with('-'), "an integer has no negative zero");
    }

    #[test]
    fn totals_group_by_currency_and_are_never_summed_across_them() {
        let deals = vec![
            deal_worth(Some(Money::new(4_500_000, "USD"))),
            deal_worth(Some(Money::new(1_000_000, "EUR"))),
            deal_worth(Some(Money::new(500_000, "USD"))),
            deal_worth(None),
        ];
        let totals = total_by_currency(deals.iter());
        assert_eq!(totals.len(), 2, "two currencies, two numbers — never one");
        // Sorted, so the board's columns do not reshuffle between snapshots.
        assert_eq!(totals[0], Money::new(1_000_000, "EUR"));
        assert_eq!(totals[1], Money::new(5_000_000, "USD"));
    }

    #[test]
    fn minor_units_are_placed_by_the_currency_not_by_habit() {
        assert_eq!(Money::new(4_500_000, "USD").format(), "45000.00");
        assert_eq!(Money::new(4_500, "JPY").format(), "4500", "yen has no minor unit");
        assert_eq!(Money::new(4_500, "KWD").format(), "4.500", "the dinar has three");
        assert_eq!(Money::new(5, "USD").format(), "0.05", "cents must not lose their zero");
        assert_eq!(Money::new(-4_500, "USD").format(), "-45.00");
    }

    #[test]
    fn a_currency_is_stored_upper_cased_so_two_spellings_are_one_total() {
        let deals = vec![
            deal_worth(Some(Money::new(100, "usd"))),
            deal_worth(Some(Money::new(100, "USD"))),
        ];
        let totals = total_by_currency(deals.iter());
        assert_eq!(totals, vec![Money::new(200, "USD")]);
    }

    // MARK: - Ids

    #[test]
    fn an_id_is_a_typable_slug_of_the_name() {
        assert_eq!(slug("Acme"), "acme");
        assert_eq!(slug("Acme Corp."), "acme-corp");
        assert_eq!(slug("  Hooli   Inc  "), "hooli-inc");
        assert_eq!(slug("A&B / C"), "a-b-c", "runs of punctuation collapse to one hyphen");
        assert_eq!(slug("Zürich AG"), "z-rich-ag");
    }

    #[test]
    fn ids_are_uniquified_from_two_onwards() {
        let mut taken: Vec<String> = Vec::new();
        let mut next = |name: &str| {
            let id = unique_id(name, &|c| taken.iter().any(|t| t.as_str() == c), "record");
            taken.push(id.clone());
            id
        };
        assert_eq!(next("Acme"), "acme");
        assert_eq!(next("Acme"), "acme-2");
        assert_eq!(next("ACME"), "acme-3");
    }

    /// A name with nothing typable in it still needs an id somebody could type.
    #[test]
    fn a_nameless_record_gets_a_stem_rather_than_a_bare_number() {
        let id = unique_id("→→→", &|_| false, "deal");
        assert_eq!(id, "deal");
    }

    // MARK: - Dates

    #[test]
    fn a_due_date_has_one_spelling() {
        assert_eq!(Date::parse("2026-09-08"), Some(Date::new(2026, 9, 8)));
        for bad in ["2026-9-8", "08-09-2026", "2026-13-01", "2026-02-30", "tomorrow", ""] {
            assert!(Date::parse(bad).is_none(), "`{bad}` must not parse");
        }
        assert_eq!(Date::parse("2028-02-29"), Some(Date::new(2028, 2, 29)), "leap years exist");
    }

    #[test]
    fn days_between_dates_are_signed_and_cross_every_boundary() {
        let today = Date::new(2026, 9, 8);
        assert_eq!(today.days_until(Date::new(2026, 9, 8)), 0);
        assert_eq!(today.days_until(Date::new(2026, 9, 15)), 7);
        assert_eq!(today.days_until(Date::new(2026, 9, 1)), -7, "overdue reads as negative");
        assert_eq!(Date::new(2026, 12, 31).days_until(Date::new(2027, 1, 1)), 1);
        assert_eq!(Date::new(2028, 2, 28).days_until(Date::new(2028, 3, 1)), 2, "leap day counts");
    }

    // MARK: - The dataset

    #[test]
    fn a_fresh_dataset_is_seeded_with_exactly_one_pipeline() {
        let db = Db::default();
        assert_eq!(db.pipelines.len(), 1, "v1 has one pipeline — structurally ready for more");
        assert_eq!(db.pipeline().id, "sales");
        assert_eq!(db.pipeline().stages, Stage::ALL.to_vec());
    }

    /// `crm show acme` must not have to be told which kind of thing `acme` is.
    #[test]
    fn ids_are_unique_across_every_record_type_not_just_within_one() {
        let mut db = Db::default();
        db.companies.push(company("acme", "Acme"));
        assert!(db.id_taken("acme"));
        let contact_id = unique_id("Acme", &|c| db.id_taken(c), "contact");
        assert_eq!(contact_id, "acme-2");
    }

    // MARK: - helpers

    fn deal_worth(value: Option<Money>) -> Deal {
        Deal {
            id: "d".into(),
            title: "d".into(),
            company_id: None,
            contact_ids: Vec::new(),
            value,
            stage: Stage::Lead,
            status: Status::Open,
            pipeline_id: "sales".into(),
            opened_at: 0,
            closed_at: None,
            archived_at: None,
            updated_at: 0,
        }
    }

    fn company(id: &str, name: &str) -> Company {
        Company {
            id: id.into(),
            name: name.into(),
            domain: None,
            tags: Vec::new(),
            notes: None,
            archived_at: None,
            updated_at: 0,
        }
    }
}
