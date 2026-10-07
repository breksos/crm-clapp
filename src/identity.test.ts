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
