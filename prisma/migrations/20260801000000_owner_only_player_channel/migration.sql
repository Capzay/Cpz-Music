-- Tightens who may use the player channel.
--
-- The previous pair of policies approximated identity: anyone signed in could
-- listen, and anyone signed in without jam claims could broadcast. Both were too
-- loose once anonymous sign-ins were switched on for jam guests. The anon key is
-- public by design (it ships in the browser bundle), so anybody could mint a
-- session and either watch what is playing or push forged `state` and `cmd`
-- payloads at the owner's active device. A second GitHub account that completed
-- the OAuth flow could do the same, even though the app itself denies it every
-- page and endpoint.
--
-- Both policies now key on `app_metadata`, which only the service role can
-- write, so neither claim can be forged from a browser. That is the same
-- mechanism the jam claims already rely on. The owner's flag is stamped by the
-- auth callback, which is the one place that has both the freshly signed-in user
-- and OWNER_GITHUB_ID to compare them against.
--
-- Postgres cannot read the app's environment, and Supabase does not grant the
-- `postgres` role permission to set a custom parameter on the database, so
-- reading OWNER_GITHUB_ID from here directly is not an option.

DROP POLICY IF EXISTS "player channel: authenticated may listen" ON "realtime"."messages";
DROP POLICY IF EXISTS "player channel: owner may broadcast"      ON "realtime"."messages";

-- Listening is the owner's devices plus jam guests, who need it for the
-- now-playing bar. A kicked guest has their claims cleared, which drops them.
CREATE POLICY "player channel: owner and jam guests may listen"
  ON "realtime"."messages"
  FOR SELECT
  TO authenticated
  USING (
    realtime.topic() = 'player'
    AND (
      coalesce(auth.jwt() -> 'app_metadata' ->> 'owner', '') = 'true'
      OR coalesce(auth.jwt() -> 'app_metadata' ->> 'jam', '') <> ''
    )
  );

-- Writing is the owner alone. A guest's queue addition is pushed onto the
-- channel by the server using the service role instead, which is what stops a
-- guest forging one.
CREATE POLICY "player channel: owner may broadcast"
  ON "realtime"."messages"
  FOR INSERT
  TO authenticated
  WITH CHECK (
    realtime.topic() = 'player'
    AND coalesce(auth.jwt() -> 'app_metadata' ->> 'owner', '') = 'true'
  );
