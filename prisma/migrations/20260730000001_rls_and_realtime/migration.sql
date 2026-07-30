-- Row Level Security and Realtime authorisation.
--
-- Prisma connects as the table owner and bypasses RLS, so none of this affects
-- the application's own queries. It matters because the Supabase anon key ships
-- inside the browser bundle and is public by design: without these policies,
-- anyone who read the JavaScript could query PostgREST directly or join the
-- player channel and watch what is playing.
--
-- The app exposes no PostgREST access at all, so the correct policy for every
-- application table is "deny everything". Enabling RLS with no permissive
-- policy does exactly that.

ALTER TABLE "Artist"          ENABLE ROW LEVEL SECURITY;
ALTER TABLE "Album"           ENABLE ROW LEVEL SECURITY;
ALTER TABLE "Track"           ENABLE ROW LEVEL SECURITY;
ALTER TABLE "DailyListenStat" ENABLE ROW LEVEL SECURITY;
ALTER TABLE "Playlist"        ENABLE ROW LEVEL SECURITY;
ALTER TABLE "PlaylistTrack"   ENABLE ROW LEVEL SECURITY;
ALTER TABLE "Jam"             ENABLE ROW LEVEL SECURITY;
ALTER TABLE "JamParticipant"  ENABLE ROW LEVEL SECURITY;
ALTER TABLE "JamQueueAdd"     ENABLE ROW LEVEL SECURITY;

-- Also revoke the grants Supabase hands to the API roles by default, so a future
-- policy added by accident cannot open a table up on its own.
REVOKE ALL ON ALL TABLES IN SCHEMA "public" FROM anon, authenticated;
REVOKE ALL ON ALL SEQUENCES IN SCHEMA "public" FROM anon, authenticated;
ALTER DEFAULT PRIVILEGES IN SCHEMA "public" REVOKE ALL ON TABLES FROM anon, authenticated;
ALTER DEFAULT PRIVILEGES IN SCHEMA "public" REVOKE ALL ON SEQUENCES FROM anon, authenticated;

-- ── Realtime authorisation ──────────────────────────────────────────────────
-- The player channel is declared private, which makes Supabase check these
-- policies on `realtime.messages` before letting a client join or send.

-- Reading (joining the channel and receiving broadcasts) is allowed for any
-- signed-in user, which covers the host's own devices and admitted jam guests.
CREATE POLICY "player channel: authenticated may listen"
  ON "realtime"."messages"
  FOR SELECT
  TO authenticated
  USING ( realtime.topic() = 'player' );

-- Writing is restricted to the owner. A jam guest holds a real session, so
-- without this they could broadcast a forged `state` payload or send commands
-- to the host's active device.
CREATE POLICY "player channel: owner may broadcast"
  ON "realtime"."messages"
  FOR INSERT
  TO authenticated
  WITH CHECK (
    realtime.topic() = 'player'
    AND coalesce(auth.jwt() -> 'app_metadata' ->> 'jam', '') = ''
  );
