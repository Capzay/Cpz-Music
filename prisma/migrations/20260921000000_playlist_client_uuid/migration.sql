-- Stable client id for local-first playlist sync. Existing rows get a uuid so
-- devices can address them before they next edit. updatedAt stays a regular
-- timestamp: the client supplies it, last write wins.

ALTER TABLE "Playlist" ADD COLUMN "uuid" TEXT;

UPDATE "Playlist" SET "uuid" = gen_random_uuid()::text WHERE "uuid" IS NULL;

ALTER TABLE "Playlist" ALTER COLUMN "uuid" SET NOT NULL;
ALTER TABLE "Playlist" ALTER COLUMN "uuid" SET DEFAULT gen_random_uuid()::text;

CREATE UNIQUE INDEX "Playlist_uuid_key" ON "Playlist"("uuid");
