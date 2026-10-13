// p2: who "you" are, in words — and that local mode is left exactly as it was.
//
//     npm test

import { test } from "node:test";
import assert from "node:assert/strict";

import { humanInitial, humanName, initials, isMe, LOCAL, type Identity } from "./identity.ts";
import { sentence, SIGNED_OUT } from "./account.ts";

const ayse: Identity = { signedIn: true, me: { id: "usr_1", name: "Ayşe Demir" } };

test("signed out, the person is still 'You' with the neutral 'Y' disc — local mode is unchanged", () => {
  assert.equal(humanName({ kind: "human" }, LOCAL), "You");
  assert.equal(humanInitial({ kind: "human" }, LOCAL), "Y");
  // The core names the local person after the OS user; that is not an account, and is not said.
  const local: Identity = { signedIn: false, me: { id: "me", name: "berk" } };
  assert.equal(humanName({ kind: "human", id: "me" }, local), "You");
  assert.equal(humanInitial({ kind: "human", id: "me" }, local), "Y");
});

test("signed in, the person's own acts carry their real name and initials", () => {
  assert.equal(humanName({ kind: "human", id: "usr_1" }, ayse), "Ayşe Demir");
  assert.equal(humanInitial({ kind: "human", id: "usr_1" }, ayse), "AD");
});

test("a human with no id — a record from an older build — is the seat's own person", () => {
  assert.equal(isMe({ kind: "human" }, ayse), true);
  assert.equal(humanName({ kind: "human" }, ayse), "Ayşe Demir");
});

test("someone else's act is not claimed as yours, and not given a name the window was not told", () => {
  assert.equal(isMe({ kind: "human", id: "usr_2" }, ayse), false);
  assert.equal(humanName({ kind: "human", id: "usr_2" }, ayse), "A teammate");
  assert.equal(humanInitial({ kind: "human", id: "usr_2" }, ayse), "?");
});

test("an agent is never a person", () => {
  assert.equal(isMe({ kind: "agent", id: "usr_1" }, ayse), false);
  assert.equal(humanName({ kind: "agent", id: "x" }, ayse), "");
});

test("initials cope with one name, many names, and non-ASCII", () => {
  assert.equal(initials("berk"), "B");
  assert.equal(initials("Ayşe Demir"), "AD");
  assert.equal(initials("  Mehmet Ali  Yılmaz "), "MY");
  assert.equal(initials("şule"), "Ş");
  assert.equal(initials(""), "?");
});

test("a refusal reaches the person as the core's own sentence, never as a stack", () => {
  assert.equal(sentence("that email and password do not match"), "that email and password do not match");
  assert.equal(sentence(new Error("could not reach the server (Dns)")), "could not reach the server (Dns)");
  assert.equal(sentence(undefined), "that did not work — try again");
  assert.equal(sentence({ weird: true }), "that did not work — try again");
});

test("the signed-out status carries no server, no person and nothing to warn about", () => {
  assert.deepEqual(SIGNED_OUT, { signedIn: false, server: null, person: null, org: null, role: null, expiresAt: null, insecure: false });
});

// MARK: - p3: teammates

const team: Identity = {
  signedIn: true,
  me: { id: "me", name: "Berk Öge" },
  members: [{ id: "me", name: "Berk Öge" }, { id: "ana", name: "Ana Torres" }],
  teamAgents: [{ id: "ag_ana_scout", name: "Scout", ownerId: "ana" }, { id: "ag_orphan", name: "Relay", ownerId: "gone" }],
};

test("a teammate's work carries their name and initials, from the org's members", async () => {
  const { humanName, humanInitial } = await import("./identity.ts");
  assert.equal(humanName({ kind: "human", id: "ana" }, team), "Ana Torres");
  assert.equal(humanInitial({ kind: "human", id: "ana" }, team), "AT");
  assert.equal(humanName({ kind: "human", id: "stranger" }, team), "A teammate", "someone the window was not told about");
  assert.equal(humanName({ kind: "human", id: "me" }, team), "Berk Öge");
});

test("a teammate's agent is named for its owner — 'Ana's Scout' — and never invented", async () => {
  const { teamAgentName } = await import("./identity.ts");
  assert.equal(teamAgentName("ag_ana_scout", team), "Ana's Scout");
  assert.equal(teamAgentName("ag_orphan", team), "Relay", "an owner the window does not know: just the agent's name");
  assert.equal(teamAgentName("ag_unknown", team), null);
  assert.equal(teamAgentName("ag_ana_scout", LOCAL), null);
});

test("signed out, members are not used: local mode says 'You' and nothing about a team", () => {
  assert.equal(humanName({ kind: "human", id: "ana" }, { ...team, signedIn: false }), "You");
});

// MARK: - p2-sec: look before anything leaves the window

import { sendChecked, serverWarning, type ServerCheck } from "./account.ts";

function fakeApi(check: ServerCheck) {
  const calls: { name: string; args?: unknown }[] = [];
  const status = { signedIn: true } as never;
  return {
    calls,
    api: {
      checkServer: async (server: string) => (calls.push({ name: "check", args: server }), check),
      signIn: async (a: unknown) => (calls.push({ name: "signIn", args: a }), status),
      signUp: async (a: unknown) => (calls.push({ name: "signUp", args: a }), status),
      acceptInvite: async (a: unknown) => (calls.push({ name: "acceptInvite", args: a }), status),
    },
  };
}
const secure: ServerCheck = { valid: true, host: "crm.example.com", insecure: false, userinfo: false };
const plain: ServerCheck = { valid: true, host: "crm.example.com", insecure: true, userinfo: false };
const sneaky: ServerCheck = { valid: true, host: "evil.example", insecure: false, userinfo: true };
const signIn = (server: string) => ({ kind: "in" as const, args: { server, email: "a@b.co", password: "correct horse battery" } });

test("the address is checked first, every time — and a secure one is sent with no confirmation flag", async () => {
  const { api, calls } = fakeApi(secure);
  const r = await sendChecked(api, signIn("https://crm.example.com"), false);
  assert.equal(r.sent, true);
  assert.deepEqual(calls.map((c) => c.name), ["check", "signIn"]);
  assert.equal("insecureConfirmed" in (calls[1].args as object), false);
});

test("an insecure address sends nothing until the person says yes, and says which host", async () => {
  const { api, calls } = fakeApi(plain);
  const r = await sendChecked(api, signIn("http://crm.example.com"), false);
  assert.equal(r.sent, false);
  assert.ok(!r.sent && r.reason === "confirm" && r.host === "crm.example.com");
  assert.deepEqual(calls.map((c) => c.name), ["check"], "no password left the window");
  assert.match(!r.sent && r.reason === "confirm" ? r.reasons.join(" ") : "", /plain HTTP.*crm\.example\.com/s);
});

test("only after a yes is insecure_confirmed sent — true, and only to a risky address", async () => {
  const yes = fakeApi(plain);
  await sendChecked(yes.api, signIn("http://crm.example.com"), true);
  assert.deepEqual(yes.calls.map((c) => c.name), ["check", "signIn"]);
  assert.equal((yes.calls[1].args as { insecureConfirmed?: boolean }).insecureConfirmed, true);
  const fine = fakeApi(secure);
  await sendChecked(fine.api, signIn("https://crm.example.com"), true);
  assert.equal("insecureConfirmed" in (fine.calls[1].args as object), false, "a yes never widens what is sent to a safe address");
});

test("an address with a name and password in front names the host the request really goes to", async () => {
  const { api, calls } = fakeApi(sneaky);
  const r = await sendChecked(api, signIn("https://crm.example.com@evil.example"), false);
  assert.ok(!r.sent && r.reason === "confirm");
  assert.equal(r.host, "evil.example");
  assert.match(r.reasons.join(" "), /evil\.example/);
  assert.deepEqual(calls.map((c) => c.name), ["check"]);
});

test("an address that cannot be read sends nothing and says so in the person's words", async () => {
  const { api, calls } = fakeApi({ valid: false, host: null, insecure: true, userinfo: false });
  const r = await sendChecked(api, signIn("not a url"), true);
  assert.ok(!r.sent && r.reason === "invalid");
  assert.match(r.message, /does not look like a server address/);
  assert.deepEqual(calls.map((c) => c.name), ["check"], "even a yes cannot send to an address nobody can read");
});

test("sign-up and an invite are held exactly the same way", async () => {
  for (const req of [
    { kind: "up" as const, args: { server: "http://x.example", email: "a@b.co", password: "correct horse battery", name: "A", orgName: "O" } },
    { kind: "invite" as const, args: { server: "http://x.example", email: "a@b.co", password: "correct horse battery", name: "A", invite: "inv_1" } },
  ]) {
    const { api, calls } = fakeApi(plain);
    const r = await sendChecked(api, req, false);
    assert.ok(!r.sent && r.reason === "confirm");
    assert.deepEqual(calls.map((c) => c.name), ["check"]);
  }
});

test("the core's note to the window never reaches the person", () => {
  const raw = "not sent: evil.example is not a secure address — check it with account_check_server and ask the person first";
  assert.doesNotMatch(sentence(raw), /account_check_server|ask the person/);
  assert.match(sentence(raw), /Nothing was sent/);
  assert.doesNotMatch(sentence(new Error(raw)), /account_check_server/);
  assert.equal(sentence("that email and password do not match"), "that email and password do not match");
});

test("the warning names the host and says what is wrong in plain words", () => {
  const both = serverWarning({ valid: true, host: "evil.example", insecure: true, userinfo: true });
  assert.equal(both.host, "evil.example");
  assert.equal(both.reasons.length, 2);
  for (const r of both.reasons) assert.match(r, /evil\.example/);
  assert.equal(serverWarning(secure).reasons.length, 0);
});
