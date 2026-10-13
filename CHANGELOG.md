# Changelog

## Unreleased

- **A Windows (x64) build is in progress.** It is built on a real Windows machine in our build
  system, and is not released: it has not yet been tried on a real Windows 11 PC, and it
  is not code-signed, so Windows may warn before running it.

## 0.2.0

You and your agent share one pipeline. This release makes that visible: you can now watch your
agent work, and nothing it does goes by unseen.

### Watch your agent work

- **Every step your agent takes shows up in the window**, as it happens and in order — what it
  looked up, added, logged, moved or finished. Each line names the agent and the record it touched,
  and clicking a line opens that record. If something was refused, you see the exact words your
  agent was given. Changes it makes light up briefly in its colour.
- **Follow agent.** Turn it on and the window goes where your agent goes: when it opens a deal,
  the deal opens in front of you, on the right page. Turn it off and the window stays where you
  put it. The choice is remembered.
- **Follow never costs you your work.** If you are in the middle of typing something, Follow waits.
  A small note tells you where your agent went, and you can jump there when you are ready.
  Nothing you typed is dropped. This also fixes an older problem: clicking another record while
  you had unsaved changes could carry them over, and now it asks first.
- **The record panel folds away.** Collapse it to a slim strip beside the list (or press ⌘\) to
  give the list the whole window. While it is folded, a dot appears if your agent changed that
  record, and it clears when you open the panel again. Your agent never opens it for you.

### Next steps, names and lists

- **Change a next step's due date or wording** — ask your agent, or run
  `crm set <next-step> due <date>` / `crm set <next-step> what <text>`. The next step keeps its
  name, so nothing that points at it breaks.
- **Better handles for names with accents.** "Erdoğan" becomes `erdogan` and "Müller & Söhne"
  becomes `muller-sohne`. Turkish, German and accented Latin letters are all folded to their plain
  letters, so a name gets a handle you can actually type.
- **The list is always current.** A contact or deal your agent just added appears at once, instead
  of waiting until you switch pages. When your agent opens a record, the list moves to the page
  that record is on. When a name matches several records, the list heading now says what it
  is showing.
