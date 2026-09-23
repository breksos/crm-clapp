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

/// A record's true identity: a [`Ulid`], rendered. Globally unique and creation-ordered.
///
/// **Not derived from the name**, and that is the whole point. A name-derived id does not
/// survive more than one machine: two installs both create "Acme Corp", both mint `acme`,
/// and on sync nothing can tell whether that is one company or two. Unresolvable after the
/// fact, so it is decided before there is any data to migrate.
///
/// Every reference stores this — `company_id`, `contact_ids`, `links`, `focus`, `dealIds`.
/// What a person or an agent *types* is the [`Company::handle`], resolved at the edge.
pub type Id = String;

/// The typable name for a record — the old slug: `acme`, then `acme-2`.
///
/// Unique within this workspace, stable across a rename, and never a reference: nothing
/// stores a handle, so a workspace that merges with another can re-handle a collision
/// without orphaning a single pointer.
pub type Handle = String;

/// Which install wrote a version of a record. A UUID minted once on first run.
///
/// Without it there is no conflict resolution later, only guessing which of two edits came
/// first by a clock that two machines do not share. Nothing reads it in v1 — it exists so
/// that building sync is a feature rather than a migration.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct InstanceId(pub String);

impl InstanceId {
    /// A v4 UUID from 16 random bytes, with the version and variant bits set.
    ///
    /// Takes its randomness rather than reading any, so it is pure and a test can pin the
    /// exact string those bytes produce.
    pub fn from_bytes(mut b: [u8; 16]) -> InstanceId {
        b[6] = (b[6] & 0x0f) | 0x40; // version 4
        b[8] = (b[8] & 0x3f) | 0x80; // variant 10xx
        let h = |r: &[u8]| r.iter().map(|x| format!("{x:02x}")).collect::<String>();
        InstanceId(format!(
            "{}-{}-{}-{}-{}",
            h(&b[0..4]),
            h(&b[4..6]),
            h(&b[6..8]),
            h(&b[8..10]),
            h(&b[10..16])
        ))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// Milliseconds since the Unix epoch. A plain integer because the core does not do
/// calendar arithmetic on it — [`Date`] does, and it arrives already computed.
pub type Timestamp = i64;

// MARK: - ULID

/// 48 bits of millisecond timestamp then 80 bits of randomness, as 26 Crockford base32
/// characters. Sorts lexicographically in creation order, which is what makes a log of
/// them replayable.
///
/// **It mints nothing by itself.** The timestamp and the randomness are handed in, exactly
/// like [`Now`], because the core reads no clock and no entropy source. That is also what
/// lets a test assert the precise id a given input produces.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct Ulid(u128);

/// Crockford base32: no `I`, `L`, `O` or `U`, so a handwritten id cannot be misread.
const CROCKFORD: &[u8; 32] = b"0123456789ABCDEFGHJKMNPQRSTVWXYZ";

impl Ulid {
    /// The timestamp in the high 48 bits, the entropy in the low 80.
    pub fn from_parts(at: Timestamp, entropy: [u8; 10]) -> Ulid {
        // A negative instant is before 1970 and cannot be a record's creation time; it is
        // a broken clock, and clamping keeps the ordering property intact.
        let ms = (at.max(0) as u128) & 0xFFFF_FFFF_FFFF;
        let mut rand: u128 = 0;
        for byte in entropy {
            rand = (rand << 8) | byte as u128;
        }
        Ulid((ms << 80) | rand)
    }

    /// The next id after `prev`, within the same millisecond.
    ///
    /// ULID's own monotonic rule: when the clock has not moved, increment the random
    /// component rather than drawing fresh randomness. Two records created in one
    /// millisecond then still sort in the order they were made — and two records created
    /// in one *command*, which share one draw of entropy, cannot collide.
    pub fn next_after(prev: Ulid, at: Timestamp, entropy: [u8; 10]) -> Ulid {
        let fresh = Ulid::from_parts(at, entropy);
        if fresh.timestamp_ms() != prev.timestamp_ms() || fresh > prev {
            return fresh;
        }
        // Carrying out of the 80 random bits would corrupt the timestamp. It takes 2^80
        // ids in one millisecond to get there, so the fallback is unreachable rather than
        // load-bearing — but a silently wrong timestamp is not a thing to leave to luck.
        match prev.0.checked_add(1).filter(|next| next >> 80 == prev.0 >> 80) {
            Some(next) => Ulid(next),
            None => fresh,
        }
    }

    pub fn timestamp_ms(&self) -> u64 {
        (self.0 >> 80) as u64
    }

    pub fn parse(s: &str) -> Option<Ulid> {
        if s.len() != 26 {
            return None;
        }
        let mut v: u128 = 0;
        for ch in s.bytes() {
            let up = ch.to_ascii_uppercase();
            let digit = CROCKFORD.iter().position(|c| *c == up)?;
            v = v.checked_mul(32)?.checked_add(digit as u128)?;
        }
        Some(Ulid(v))
    }
}

impl std::fmt::Display for Ulid {
    /// 26 characters, most significant first. The leading character carries only the top
    /// three bits, because 26 × 5 is 130 and a ULID is 128.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let mut out = [0u8; 26];
        for (i, slot) in out.iter_mut().enumerate() {
            let shift = 5 * (25 - i);
            *slot = CROCKFORD[((self.0 >> shift) & 0x1F) as usize];
        }
        f.write_str(std::str::from_utf8(&out).expect("Crockford base32 is ASCII"))
    }
}

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

/// Everything impure that one command needs, gathered at the edge and handed in.
///
/// The core reads no clock, no entropy source and no environment. It is *given* the
/// instant, the day, a draw of randomness to mint ids from, and the identity of this
/// install — and every rule below is then a pure function of its inputs, which is what
/// makes them testable without a window server and reproducible when one of them is wrong.
#[derive(Clone, Debug)]
pub struct Ctx {
    pub now: Now,
    /// 80 bits, drawn once per command. Two records made in one command do not collide:
    /// [`Ulid::next_after`] increments rather than redrawing.
    pub entropy: [u8; 10],
    /// Which install is writing. Stamped onto every mutable record it touches.
    pub origin: InstanceId,
    /// Seconds east of UTC, in effect at `now.at`. The same number `main.rs` used to
    /// derive `now.today` — carried alongside it so the core can place a **different**
    /// civil date (`crm log … --at 2026-09-10`) on the calendar the same way, without
    /// reading a clock or a zone of its own. See [`Date::to_timestamp_ms`].
    pub offset_secs: i32,
}

impl Ctx {
    pub fn at(&self) -> Timestamp {
        self.now.at
    }
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
        ((1..=12).contains(&m) && d >= 1 && d <= days_in_month(y, m)).then_some(date)
    }

    /// Days between two dates — negative when `self` is earlier. The only arithmetic the
    /// due buckets need.
    pub fn days_until(&self, other: Date) -> i64 {
        other.to_days() - self.to_days()
    }

    /// Days since 1970-01-01, by Howard Hinnant's `days_from_civil`. Fifteen lines and no
    /// dependency; a calendar crate would be a larger surface than the problem.
    fn to_days(self) -> i64 {
        let y = if self.m <= 2 { self.y - 1 } else { self.y } as i64;
        let era = if y >= 0 { y } else { y - 399 } / 400;
        let yoe = y - era * 400;
        let m = self.m as i64;
        let d = self.d as i64;
        let doy = (153 * (if m > 2 { m - 3 } else { m + 9 }) + 2) / 5 + d - 1;
        let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
        era * 146_097 + doe - 719_468
    }

    pub fn to_string_iso(self) -> String {
        format!("{:04}-{:02}-{:02}", self.y, self.m, self.d)
    }

    /// This date's local midnight, as a UTC instant — the exact inverse of
    /// `main.rs::local_date(at_ms, offset_secs)`. Used to place a **backdated** activity
    /// (`crm log … --at 2026-09-10`) on the calendar: the core has no clock of its own,
    /// so it cannot ask "what instant is this date's midnight" without the zone offset
    /// [`Ctx`] carries in for exactly this.
    pub fn to_timestamp_ms(self, offset_secs: i32) -> Timestamp {
        (self.to_days() * 86_400 - offset_secs as i64) * 1000
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
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum Actor {
    /// The default, so a data file written before a field that carries an `Actor` existed
    /// still loads. The person is the only honest guess about a write nobody recorded.
    #[default]
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

    /// The word a person reads in a record's fields.
    pub fn label(&self) -> &'static str {
        match self {
            Status::Open => "Open",
            Status::Won => "Won",
            Status::Lost => "Lost",
        }
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

    /// The amount as a person reads it: `$45,000.00`, `¥4,500`, `KWD 4.500`.
    ///
    /// **The only money formatter in the app.** It rides the snapshot as `formatted` beside
    /// the raw amount, and the window renders that string rather than doing the arithmetic
    /// itself — two formatters is two answers, and they had already been kept in step by
    /// hand, exponent table and all.
    pub fn format(&self) -> String {
        let places = Money::exponent(&self.currency) as usize;
        let sign = if self.amount < 0 { "-" } else { "" };
        // `unsigned_abs` rather than `abs`: i64::MIN has no positive counterpart, and a
        // panic in a totals row would be an odd way to find that out.
        let magnitude = self.amount.unsigned_abs();
        let unit = 10u64.pow(places as u32);
        let major = group_thousands(magnitude / unit);
        let number = if places == 0 {
            major
        } else {
            format!("{major}.{:0places$}", magnitude % unit)
        };
        match Money::symbol(&self.currency) {
            Some(symbol) => format!("{sign}{symbol}{number}"),
            None => format!("{sign}{} {number}", self.currency),
        }
    }

    /// The un-symboled, ungrouped decimal — `"45000.00"`, `"4500"` for yen — the wire
    /// form. `format()` is what a person reads; this is what a CSV cell or a re-typed
    /// `--value` holds, so it round-trips through [`Money::parse_decimal`] exactly.
    pub fn decimal(&self) -> String {
        let places = Money::exponent(&self.currency) as usize;
        let sign = if self.amount < 0 { "-" } else { "" };
        let magnitude = self.amount.unsigned_abs();
        if places == 0 {
            return format!("{sign}{magnitude}");
        }
        let unit = 10u64.pow(places as u32);
        format!("{sign}{}.{:0places$}", magnitude / unit, magnitude % unit)
    }

    /// The inverse of [`Money::decimal`]: `"45000"` or `"45000.50"` (an optional leading
    /// `-`, digits, at most one `.`) into minor units for `currency`'s exponent.
    ///
    /// `--value` takes a decimal so an agent never has to know a currency's exponent to
    /// use it — `crm add deal … --value 45000.5` means the same fifty cents whether the
    /// currency turns out to have two decimal places or three. Extra fractional digits
    /// are refused rather than silently truncated: `--value 45000.567 --currency USD`
    /// losing the `7` would be a number quietly changing under the person who typed it.
    pub fn parse_decimal(input: &str, currency: &str) -> Option<i64> {
        let s = input.trim();
        if s.is_empty() || !s.is_ascii() {
            return None;
        }
        let (sign, s) = match s.strip_prefix('-') {
            Some(rest) => (-1i64, rest),
            None => (1i64, s),
        };
        let places = Money::exponent(currency) as usize;
        let unit = 10i64.pow(places as u32);
        let (whole, frac) = match s.split_once('.') {
            Some((w, f)) => (w, f),
            None => (s, ""),
        };
        if whole.is_empty() && frac.is_empty() {
            return None;
        }
        if !whole.chars().all(|c| c.is_ascii_digit()) || !frac.chars().all(|c| c.is_ascii_digit()) {
            return None;
        }
        if frac.len() > places {
            return None; // more precision than the currency has — refuse, never truncate
        }
        let whole: i64 = if whole.is_empty() { 0 } else { whole.parse().ok()? };
        let scaled_frac: i64 = if frac.is_empty() {
            0
        } else {
            let padded = format!("{frac:0<places$}");
            padded.parse().ok()?
        };
        whole.checked_mul(unit)?.checked_add(scaled_frac)?.checked_mul(sign)
    }

    /// A symbol, **only where it names exactly one currency.**
    ///
    /// The board shows several currencies side by side, and a bare `$` over two columns
    /// that are not both US dollars is the kind of wrong nobody catches. So the shared
    /// signs carry a prefix — `CA$`, `A$`, `CN¥` — the way CLDR writes them for English,
    /// and everything not listed prints its ISO code, which is never ambiguous.
    pub fn symbol(currency: &str) -> Option<&'static str> {
        Some(match currency {
            "USD" => "$",
            "EUR" => "€",
            "GBP" => "£",
            "JPY" => "¥",
            "INR" => "₹",
            "KRW" => "₩",
            "ILS" => "₪",
            "VND" => "₫",
            "TRY" => "₺",
            "CAD" => "CA$",
            "AUD" => "A$",
            "NZD" => "NZ$",
            "HKD" => "HK$",
            "MXN" => "MX$",
            "TWD" => "NT$",
            "BRL" => "R$",
            "CNY" => "CN¥",
            _ => return None,
        })
    }
}

/// `1234567` → `1,234,567`.
fn group_thousands(n: u64) -> String {
    let digits = n.to_string();
    let mut out = String::with_capacity(digits.len() + digits.len() / 3);
    for (i, ch) in digits.chars().enumerate() {
        if i > 0 && (digits.len() - i).is_multiple_of(3) {
            out.push(',');
        }
        out.push(ch);
    }
    out
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
    /// What a person or an agent types. Nothing references it — see [`Handle`].
    #[serde(default)]
    pub handle: Handle,
    pub name: String,
    pub domain: Option<String>,
    #[serde(default)]
    pub tags: Vec<String>,
    pub notes: Option<String>,
    /// Set, never unset by a delete. Archiving is reversible; there is no hard delete,
    /// because an agent holding one and a bad fuzzy match is an unrecoverable afternoon.
    pub archived_at: Option<Timestamp>,
    pub updated_at: Timestamp,
    /// Which install last wrote this record. Read by nothing in v1.
    #[serde(default)]
    pub origin: InstanceId,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Contact {
    pub id: Id,
    #[serde(default)]
    pub handle: Handle,
    pub name: String,
    pub email: Option<String>,
    pub phone: Option<String>,
    pub title: Option<String>,
    pub company_id: Option<Id>,
    #[serde(default)]
    pub tags: Vec<String>,
    pub archived_at: Option<Timestamp>,
    pub updated_at: Timestamp,
    #[serde(default)]
    pub origin: InstanceId,
}

/// The spine.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Deal {
    pub id: Id,
    #[serde(default)]
    pub handle: Handle,
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
    /// **Who last put this deal where it is**, and when — set on creation and on every
    /// move, and on nothing else.
    ///
    /// Not "the actor of the latest activity": a stage move is not an activity, so that
    /// derivation names whoever last logged a *call*, and the board's ring would tint the
    /// wrong agent. This is what the card's attribution disc draws.
    #[serde(default)]
    pub moved_by: Actor,
    #[serde(default)]
    pub moved_at: Timestamp,
    pub archived_at: Option<Timestamp>,
    pub updated_at: Timestamp,
    #[serde(default)]
    pub origin: InstanceId,
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
    /// What `crm done <task-handle>` types. Minted from `what`, uniquified in the same
    /// namespace as every other handle.
    #[serde(default)]
    pub handle: Handle,
    pub what: String,
    pub due: Date,
    #[serde(default)]
    pub links: Vec<Id>,
    pub done_at: Option<Timestamp>,
    pub by: Actor,
    /// A task is mutable — completing one rewrites it — so it carries the same pair the
    /// other mutable records do. `Activity` deliberately does not: it is append-only,
    /// which is already the right shape for log-based replication.
    #[serde(default)]
    pub updated_at: Timestamp,
    #[serde(default)]
    pub origin: InstanceId,
    /// When `task.due` was sent for this task — **the mark that makes it fire once, ever.**
    ///
    /// Persisted with the task, so it survives a restart: the sweep that runs at launch
    /// cannot tell "came due while closed" from "already told the agent" by any other
    /// means. It also stands for "nobody needs telling": a task an **agent** made already
    /// due is born marked (firing would wake it about its own write), and the tasks that
    /// were already overdue when reminders first began are marked once, silently, by
    /// [`crate::state::AppState::open`]. A task a **person** makes already due is *not*
    /// born marked — they are asking for it to be handled. **Not a user edit**, so it
    /// never touches `updated_at`.
    ///
    /// Never serialized into a snapshot — the window and the CLI read `reminders`.
    #[serde(default)]
    pub due_signalled_at: Option<Timestamp>,
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

// MARK: - Handles

/// A typable handle from a name: lower-cased, non-alphanumerics collapsed to single
/// hyphens, trimmed. `"Acme Corp."` → `"acme-corp"`.
///
/// Deliberately ASCII-folding nothing: a name with no ASCII alphanumerics at all yields an
/// empty slug, and [`unique_handle`] gives it a stem instead of a bare number that reads
/// like an index.
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

/// Longest a handle's stem may be, in characters. A task's `what` is a sentence, and
/// "call-back-about-the-revised-pricing-before-friday" is typable in the way a phone number
/// read aloud is memorable.
pub const HANDLE_MAX: usize = 32;

/// Cut a slug to [`HANDLE_MAX`] at a word boundary, so a handle never ends mid-word or on
/// a hyphen.
fn cap_handle(slug: &str) -> String {
    if slug.len() <= HANDLE_MAX {
        return slug.to_string();
    }
    let cut = &slug[..HANDLE_MAX];
    match cut.rfind('-') {
        Some(at) if at > 0 => cut[..at].to_string(),
        _ => cut.trim_end_matches('-').to_string(),
    }
}

/// [`slug`], uniquified against the handles already taken: `acme`, then `acme-2`,
/// `acme-3`.
///
/// The suffix starts at 2 because the first one is not "the first of several" until a
/// second arrives — and renaming it retroactively would break every handle a person has
/// already written down.
///
/// A handle is **not** an identity: it is what somebody types, and [`Id`] is what every
/// reference stores. Two installs may legitimately both hold an `acme`, and only the ids
/// can say whether that is one company or two.
pub fn unique_handle(name: &str, taken: &dyn Fn(&str) -> bool, fallback: &str) -> Handle {
    let base = {
        let s = cap_handle(&slug(name));
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
    unreachable!("u32::MAX handles with one stem is not a state this app can reach")
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
    #[serde(default)]
    pub reminders: Reminders,
}

/// What the timer has to remember across a restart. Deliberately small: which tasks were
/// told lives on the tasks themselves, and the last sweep and any refusal are about *this
/// run* and are not persisted — a refusal is re-derived by the launch sweep retrying.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Reminders {
    /// The last time a `task.due` actually left the app, and how many tasks it carried.
    /// Written only when a signal is sent — never on an idle sweep, so a quiet app does
    /// not rewrite the person's data every few minutes.
    #[serde(default)]
    pub last_signal_at: Option<Timestamp>,
    #[serde(default)]
    pub last_signal_count: usize,
    /// Whether the one-time migration has run on this dataset. Absent from a file written
    /// before the timer existed, which is exactly how it is recognised as one.
    #[serde(default)]
    pub armed: bool,
    /// The tasks that were already overdue when reminders began and were marked told
    /// **without anybody being told** — the backlog the window shows the person instead
    /// of waking their agent about work they may have been ignoring on purpose.
    #[serde(default)]
    pub backlog: Vec<Id>,
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
            reminders: Reminders::default(),
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

    /// Is this handle spoken for, in any record type?
    ///
    /// Across the whole dataset, not per type — `crm show acme` must not have to be told
    /// which kind of thing `acme` is.
    pub fn handle_taken(&self, handle: &str) -> bool {
        self.by_handle(handle).is_some() || self.task_by_handle(handle).is_some()
    }

    /// A task by its handle — what `crm done <task-handle>` resolves. Tasks share the one
    /// handle namespace so that no typed word can ever mean two different things, but
    /// they are not records a `show` opens, which is why they are looked up separately.
    pub fn task_by_handle(&self, handle: &str) -> Option<&Task> {
        let h = handle.trim().to_ascii_lowercase();
        if h.is_empty() {
            return None;
        }
        self.tasks.iter().find(|t| t.handle == h)
    }

    /// The record a typed handle names: its kind and its **id**. This is the whole of
    /// "resolution happens at the edge" — past this point nothing speaks handles.
    ///
    /// Case-folded, because an agent that types `Acme` means the same record.
    pub fn by_handle(&self, handle: &str) -> Option<(Kind, Id)> {
        let h = handle.trim().to_ascii_lowercase();
        if h.is_empty() {
            return None;
        }
        if let Some(c) = self.companies.iter().find(|c| c.handle == h) {
            return Some((Kind::Company, c.id.clone()));
        }
        if let Some(c) = self.contacts.iter().find(|c| c.handle == h) {
            return Some((Kind::Contact, c.id.clone()));
        }
        self.deals.iter().find(|d| d.handle == h).map(|d| (Kind::Deal, d.id.clone()))
    }

    /// Whether the record an id names is archived. `false` for an id nothing names — an
    /// unknown link is not an archived one, and a task must not go quiet because a link
    /// dangles.
    pub fn is_archived(&self, id: &str) -> bool {
        self.company(id).map(|c| c.archived_at.is_some()).unwrap_or(false)
            || self.contact(id).map(|c| c.archived_at.is_some()).unwrap_or(false)
            || self.deal(id).map(|d| d.archived_at.is_some()).unwrap_or(false)
    }

    /// Which kind of record an id names, or `None` if nothing does.
    pub fn kind_of(&self, id: &str) -> Option<Kind> {
        if self.company(id).is_some() {
            Some(Kind::Company)
        } else if self.contact(id).is_some() {
            Some(Kind::Contact)
        } else if self.deal(id).is_some() {
            Some(Kind::Deal)
        } else {
            None
        }
    }

    /// The handle for an id — what to print so the reader has something they can type
    /// back. Ids themselves are never shown to anybody.
    pub fn handle_of(&self, id: &str) -> Option<&str> {
        self.company(id).map(|c| c.handle.as_str())
            .or_else(|| self.contact(id).map(|c| c.handle.as_str()))
            .or_else(|| self.deal(id).map(|d| d.handle.as_str()))
    }

    /// Every **active** deal — the board, the counts and default `find` all exclude
    /// archived records.
    pub fn active_deals(&self) -> impl Iterator<Item = &Deal> {
        self.deals.iter().filter(|d| d.archived_at.is_none())
    }

    /// Every id this dataset stores as a **reference** — one record pointing at another.
    ///
    /// These are the pointers that must never hold a [`Handle`]: a handle is what somebody
    /// types, and a workspace that merges with another has to be free to re-handle a
    /// collision without orphaning anything. A test walks this list to pin that.
    pub fn references(&self) -> Vec<Id> {
        let mut out = Vec::new();
        out.extend(self.contacts.iter().filter_map(|c| c.company_id.clone()));
        for d in &self.deals {
            out.extend(d.company_id.clone());
            out.extend(d.contact_ids.iter().cloned());
        }
        for a in &self.activities {
            out.extend(a.links.iter().cloned());
        }
        for t in &self.tasks {
            out.extend(t.links.iter().cloned());
        }
        out.extend(self.view.focus.as_ref().map(|f| f.id.clone()));
        out.extend(self.view.list.results.iter().cloned());
        out
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

        // Round 3 made `format()` the display form (`$45,000.00`), so zero is `$0.00`.
        // What this test exists for is unchanged: no minus sign on nothing, in any
        // currency and at any exponent.
        assert_eq!(Money::new(0, "USD").format(), "$0.00");
        for currency in ["USD", "JPY", "KWD", "CHF"] {
            let zero = Money::new(0, currency).format();
            assert!(!zero.contains('-'), "{currency}: an integer has no negative zero: {zero}");
        }
    }

    #[test]
    fn totals_group_by_currency_and_are_never_summed_across_them() {
        let deals = [
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
        assert_eq!(Money::new(4_500_000, "USD").format(), "$45,000.00");
        assert_eq!(Money::new(4_500, "JPY").format(), "¥4,500", "yen has no minor unit");
        assert_eq!(Money::new(4_500, "KWD").format(), "KWD 4.500", "the dinar has three");
        assert_eq!(Money::new(5, "USD").format(), "$0.05", "cents must not lose their zero");
        assert_eq!(Money::new(-4_500, "USD").format(), "-$45.00");
    }

    #[test]
    fn thousands_are_grouped_at_every_size() {
        assert_eq!(Money::new(99, "EUR").format(), "€0.99");
        assert_eq!(Money::new(99_999, "EUR").format(), "€999.99");
        assert_eq!(Money::new(100_000, "EUR").format(), "€1,000.00");
        assert_eq!(Money::new(123_456_789_012, "EUR").format(), "€1,234,567,890.12");
        assert_eq!(Money::new(i64::MIN, "USD").format(), "-$92,233,720,368,547,758.08");
    }

    /// The board shows several currencies side by side. A bare `$` over two columns that
    /// are not both US dollars is the kind of wrong nobody catches.
    #[test]
    fn a_bare_symbol_is_used_only_where_it_names_one_currency() {
        assert_eq!(Money::new(100, "USD").format(), "$1.00");
        assert_eq!(Money::new(100, "CAD").format(), "CA$1.00");
        assert_eq!(Money::new(100, "AUD").format(), "A$1.00");
        assert_eq!(Money::new(100, "JPY").format(), "¥100");
        assert_eq!(Money::new(100, "CNY").format(), "CN¥1.00");
        assert_eq!(Money::new(100, "CHF").format(), "CHF 1.00", "no symbol → the ISO code");
        assert_eq!(Money::new(-100, "CHF").format(), "-CHF 1.00");

        // Every symbol in the table is distinct, or it is not doing its one job.
        let codes = ["USD", "EUR", "GBP", "JPY", "INR", "KRW", "ILS", "VND", "TRY", "CAD",
                     "AUD", "NZD", "HKD", "MXN", "TWD", "BRL", "CNY"];
        let mut symbols: Vec<&str> = codes.iter().filter_map(|c| Money::symbol(c)).collect();
        assert_eq!(symbols.len(), codes.len());
        symbols.sort_unstable();
        symbols.dedup();
        assert_eq!(symbols.len(), codes.len(), "two currencies share a symbol");
    }

    #[test]
    fn a_currency_is_stored_upper_cased_so_two_spellings_are_one_total() {
        let deals = [
            deal_worth(Some(Money::new(100, "usd"))),
            deal_worth(Some(Money::new(100, "USD"))),
        ];
        let totals = total_by_currency(deals.iter());
        assert_eq!(totals, vec![Money::new(200, "USD")]);
    }

    // MARK: - Handles

    #[test]
    fn a_handle_is_a_typable_slug_of_the_name() {
        assert_eq!(slug("Acme"), "acme");
        assert_eq!(slug("Acme Corp."), "acme-corp");
        assert_eq!(slug("  Hooli   Inc  "), "hooli-inc");
        assert_eq!(slug("A&B / C"), "a-b-c", "runs of punctuation collapse to one hyphen");
        assert_eq!(slug("Zürich AG"), "z-rich-ag");
    }

    #[test]
    fn handles_are_uniquified_from_two_onwards() {
        let mut taken: Vec<String> = Vec::new();
        let mut next = |name: &str| {
            let h = unique_handle(name, &|c| taken.iter().any(|t| t.as_str() == c), "record");
            taken.push(h.clone());
            h
        };
        assert_eq!(next("Acme"), "acme");
        assert_eq!(next("Acme"), "acme-2");
        assert_eq!(next("ACME"), "acme-3");
    }

    /// A task's `what` is a sentence. Its handle is the first few words, cut at a word
    /// boundary — never mid-word, never on a hyphen.
    #[test]
    fn a_long_name_yields_a_handle_cut_at_a_word() {
        let h = unique_handle(
            "Call back about the revised pricing before Friday",
            &|_| false,
            "task",
        );
        assert!(h.len() <= HANDLE_MAX, "{h} is {} long", h.len());
        assert_eq!(h, "call-back-about-the-revised");
        assert!(!h.ends_with('-'));

        // A single unbroken word longer than the cap is cut, not refused.
        let long = unique_handle(&"x".repeat(50), &|_| false, "task");
        assert_eq!(long.len(), HANDLE_MAX);

        // Uniquifying adds the suffix after the cut, so the stem stays readable.
        let taken = "call-back-about-the-revised";
        let second = unique_handle(
            "Call back about the revised pricing before Friday",
            &|c| c == taken,
            "task",
        );
        assert_eq!(second, "call-back-about-the-revised-2");
    }

    /// A name with nothing typable in it still needs a handle somebody could type.
    #[test]
    fn a_nameless_record_gets_a_stem_rather_than_a_bare_number() {
        assert_eq!(unique_handle("→→→", &|_| false, "deal"), "deal");
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
    fn handles_are_unique_across_every_record_type_not_just_within_one() {
        let mut db = Db::default();
        db.companies.push(company(&an_id(1), "acme", "Acme"));
        assert!(db.handle_taken("acme"));
        assert_eq!(unique_handle("Acme", &|c| db.handle_taken(c), "contact"), "acme-2");
    }

    /// Resolution happens at the edge: a typed handle becomes an id, and past that point
    /// nothing speaks handles.
    #[test]
    fn a_handle_resolves_to_an_id_and_is_case_folded() {
        let mut db = Db::default();
        let id = an_id(1);
        db.companies.push(company(&id, "acme", "Acme Corp"));
        assert_eq!(db.by_handle("ACME"), Some((Kind::Company, id.clone())));
        assert_eq!(db.handle_of(&id), Some("acme"));
        assert_eq!(db.by_handle("nobody"), None);
        assert_eq!(db.by_handle(""), None, "an empty handle names nothing");
    }

    // MARK: - helpers

    /// A real ULID, minted by the same code the app uses — see the note in `store.rs`.
    /// A hand-spelled `01J0DEAL0…` contains an `L`, which Crockford base32 does not have,
    /// so it is a string the minter could never produce.
    fn an_id(seed: u8) -> Id {
        Ulid::from_parts(1_700_000_000_000, [seed; 10]).to_string()
    }

    fn deal_worth(value: Option<Money>) -> Deal {
        Deal {
            id: an_id(9),
            handle: "d".into(),
            title: "d".into(),
            company_id: None,
            contact_ids: Vec::new(),
            value,
            stage: Stage::Lead,
            status: Status::Open,
            pipeline_id: "sales".into(),
            opened_at: 0,
            closed_at: None,
            moved_by: Actor::Human,
            moved_at: 0,
            archived_at: None,
            updated_at: 0,
            origin: InstanceId::default(),
        }
    }

    fn company(id: &str, handle: &str, name: &str) -> Company {
        Company {
            id: id.into(),
            handle: handle.into(),
            name: name.into(),
            domain: None,
            tags: Vec::new(),
            notes: None,
            archived_at: None,
            updated_at: 0,
            origin: InstanceId::default(),
        }
    }

    // MARK: - Identity

    /// A ULID is a timestamp and a draw of randomness, and nothing else — so the same
    /// inputs always produce the same id, which is what makes every test below possible.
    #[test]
    fn a_ulid_is_twenty_six_crockford_characters() {
        let u = Ulid::from_parts(1_788_861_600_000, [0; 10]);
        let s = u.to_string();
        assert_eq!(s.len(), 26);
        assert!(
            s.bytes().all(|c| CROCKFORD.contains(&c)),
            "`{s}` must not contain I, L, O or U — they are what a handwritten id gets wrong"
        );
        assert_eq!(Ulid::parse(&s), Some(u), "an id must survive being written down");
    }

    #[test]
    fn a_ulid_carries_the_instant_it_was_made() {
        let at = 1_788_861_600_000;
        assert_eq!(Ulid::from_parts(at, [0xFF; 10]).timestamp_ms(), at as u64);
        // A broken clock cannot push a record before the epoch and invert the ordering.
        assert_eq!(Ulid::from_parts(-5, [0; 10]).timestamp_ms(), 0);
    }

    /// Creation-ordered, which is what makes a log of these replayable: a later id always
    /// sorts after an earlier one, as text.
    #[test]
    fn ulids_sort_as_text_in_the_order_they_were_made() {
        let first = Ulid::from_parts(1_000, [0; 10]);
        let second = Ulid::from_parts(2_000, [0; 10]);
        assert!(first.to_string() < second.to_string());

        // …and that still holds when the clock has not moved between them.
        let a = Ulid::from_parts(1_000, [7; 10]);
        let b = Ulid::next_after(a, 1_000, [7; 10]);
        assert!(a < b, "the monotonic rule must break the tie");
        assert!(a.to_string() < b.to_string());
        assert_eq!(b.timestamp_ms(), 1_000, "incrementing must not reach the timestamp");
    }

    /// Two records made in one command share one draw of entropy. If that produced one id
    /// twice, the second record would overwrite the first.
    #[test]
    fn one_draw_of_entropy_still_yields_distinct_ids() {
        let mut seen = Vec::new();
        let mut last = Ulid::from_parts(1_000, [42; 10]);
        seen.push(last);
        for _ in 0..100 {
            last = Ulid::next_after(last, 1_000, [42; 10]);
            assert!(!seen.contains(&last), "{last} was minted twice");
            seen.push(last);
        }
    }

    #[test]
    fn a_moved_clock_draws_fresh_randomness_rather_than_counting() {
        let a = Ulid::from_parts(1_000, [1; 10]);
        let b = Ulid::next_after(a, 1_001, [9; 10]);
        assert_eq!(b, Ulid::from_parts(1_001, [9; 10]));
    }

    #[test]
    fn only_a_well_formed_ulid_parses() {
        for bad in ["", "acme", "0123456789012345678901234", "0123456789012345678901234567"] {
            assert!(Ulid::parse(bad).is_none(), "`{bad}` must not parse");
        }
        // I, L, O and U are not in the alphabet at all.
        assert!(Ulid::parse("IIIIIIIIIIIIIIIIIIIIIIIIII").is_none());
    }

    /// The origin says which install wrote a version of a record. It is a v4 UUID, and the
    /// bits that say so must actually be set — otherwise it is just a hex string.
    #[test]
    fn an_instance_id_is_a_v4_uuid() {
        let id = InstanceId::from_bytes([0; 16]);
        let s = id.as_str();
        assert_eq!(s.len(), 36);
        assert_eq!(s.split('-').map(str::len).collect::<Vec<_>>(), vec![8, 4, 4, 4, 12]);
        assert_eq!(s.as_bytes()[14], b'4', "the version nibble");
        assert!(matches!(s.as_bytes()[19], b'8' | b'9' | b'a' | b'b'), "the variant bits");

        // Different randomness, different install.
        assert_ne!(InstanceId::from_bytes([0; 16]), InstanceId::from_bytes([1; 16]));
    }
}
