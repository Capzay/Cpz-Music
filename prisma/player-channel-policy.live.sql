-- End-to-end check of the player-channel policies against realtime.messages
-- itself, rather than against a copy of their predicates.
--
--   psql "$DATABASE_URL" -f prisma/player-channel-policy.live.sql
--
-- Applies the policies, impersonates the `authenticated` role with a set of
-- forged JWT claims, attempts a real INSERT, and rolls the whole thing back.
-- Nothing survives the transaction. Every row must say PASS.
begin;

\i prisma/migrations/20260801000000_owner_only_player_channel/migration.sql

create or replace function pg_temp.try_broadcast(claims text)
returns text language plpgsql as $$
begin
  -- realtime.topic() reads this setting, not the row being inserted. Realtime
  -- sets it per channel; the test has to stand in for that.
  set local "realtime.topic" = 'player';
  set local role authenticated;
  execute format('set local request.jwt.claims = %L', claims);
  insert into realtime.messages (topic, extension, payload, inserted_at)
  values ('player', 'broadcast', '{"probe": true}'::jsonb, now());
  reset role;
  return 'ALLOWED';
exception
  when others then
    reset role;
    return 'DENIED: ' || sqlerrm;
end;
$$;

select label,
       left(got, 52) as broadcast,
       expected,
       case when got like expected || '%' then 'PASS' else 'FAIL' end as result
from (values
  -- The owner's own devices: the only client allowed to drive playback. The
  -- flag is written by the auth callback using the service role.
  ('owner',
   '{"role":"authenticated","app_metadata":{"owner":true}}',
   'ALLOWED'),

  -- An admitted guest listens for the now-playing bar but never writes.
  ('admitted jam guest',
   '{"role":"authenticated","app_metadata":{"jam":{"pid":"p1","jamId":"j1"}}}',
   'DENIED'),

  -- The hole this migration closes: anyone can mint one of these with the
  -- public anon key, which ships in the browser bundle.
  ('plain anonymous signup',
   '{"role":"authenticated","is_anonymous":true,"app_metadata":{}}',
   'DENIED'),

  -- The other hole: a different GitHub account that completed the OAuth flow.
  ('non-owner github account',
   '{"role":"authenticated","app_metadata":{},"user_metadata":{"provider_id":"116733336"}}',
   'DENIED'),

  -- A former owner whose flag was set false after OWNER_GITHUB_ID changed.
  ('owner flag explicitly false',
   '{"role":"authenticated","app_metadata":{"owner":false}}',
   'DENIED'),

  -- user_metadata is writable by the account itself, so a claim planted there
  -- must count for nothing.
  ('forged owner flag in user_metadata',
   '{"role":"authenticated","app_metadata":{},"user_metadata":{"owner":true}}',
   'DENIED')
) as t(label, claims, expected),
lateral (select pg_temp.try_broadcast(claims)) as r(got);

rollback;
