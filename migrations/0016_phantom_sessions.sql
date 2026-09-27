-- Conversations recorded under another conversation's NAME, which hold nothing.
--
-- A stray `--session recall` recorded a session whose id is `recall`, and the
-- CLI's resolver let an exact id beat a name, so every `--to recall` went to
-- that phantom instead of the conversation named recall. `sessions::touch` now
-- refuses such an id and the resolver refuses the collision; this removes the
-- rows already made. Only those holding no task: deleting a held one would set
-- its holder to NULL (`fk_tasks_session`). A focus cascades, and a history line
-- keeps the id as its actor.
DELETE FROM sessions
WHERE id IN (
    SELECT id FROM (
        SELECT p.id FROM sessions p
        JOIN sessions n ON n.name = p.id AND n.id <> p.id
        WHERE NOT EXISTS (SELECT 1 FROM tasks t WHERE t.assignee_session = p.id)
    ) AS phantom
);
