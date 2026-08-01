import assert from "node:assert/strict";
import test from "node:test";
// dist/ is CommonJS, so named imports have to come off the default export.
import navigation from "./dist/navigation.cjs";

const { isInternalUrl } = navigation;
const SERVER = "https://music.capzay.uk";

test("keeps our own origin in the window", () => {
  assert.equal(isInternalUrl(`${SERVER}/library`, SERVER), true);
});

test("keeps the sign-in hops in the window", () => {
  assert.equal(isInternalUrl("https://abc123.supabase.co/auth/v1/authorize?x=1", SERVER), true);
  assert.equal(isInternalUrl("https://github.com/login/oauth/authorize", SERVER), true);
});

test("sends everything else to the browser", () => {
  assert.equal(isInternalUrl("https://example.com/", SERVER), false);
  // A prefix of the server URL, but somebody else's site.
  assert.equal(isInternalUrl("https://music.capzay.uk.example.com/", SERVER), false);
  // A suffix of an auth host, but not under it.
  assert.equal(isInternalUrl("https://evilgithub.com/", SERVER), false);
  assert.equal(isInternalUrl("http://github.com/", SERVER), false);
  assert.equal(isInternalUrl("not a url", SERVER), false);
});

test("allows a self-hosted server on plain http", () => {
  assert.equal(isInternalUrl("http://localhost:3000/queue", "http://localhost:3000"), true);
});
