ALTER TABLE files ADD COLUMN url TEXT;

-- Backfill URL from linked queue entries
UPDATE files SET url = (SELECT q.url FROM queue q WHERE q.id = files.queue_id) WHERE queue_id IS NOT NULL;
