ALTER TABLE `profile` ADD `avatar` text DEFAULT 'probe' NOT NULL;--> statement-breakpoint
-- Existing profiles get the avatar defaultAvatar() (packages/core/src/avatars.ts) picks from the
-- slug: a 32-bit FNV-1a hash of its characters, then h ^ (h >> 13) times the FNV prime, whose bits
-- 16 and up, modulo 8, index AVATARS. SQLite has no XOR, so a ^ b is written (a | b) - (a & b).
WITH RECURSIVE `hash`(`id`, `slug`, `i`, `h`) AS (
	SELECT `id`, `slug`, 1, 2166136261 FROM `profile`
	UNION ALL
	SELECT `id`, `slug`, `i` + 1,
		(((`h` | unicode(substr(`slug`, `i`, 1))) - (`h` & unicode(substr(`slug`, `i`, 1)))) * 16777619)
			& 4294967295
	FROM `hash` WHERE `i` <= length(`slug`)
)
UPDATE `profile` SET `avatar` = CASE (
	SELECT (((((`h` | (`h` >> 13)) - (`h` & (`h` >> 13))) * 16777619) & 4294967295) >> 16) % 8
	FROM `hash`
	WHERE `hash`.`id` = `profile`.`id` AND `hash`.`i` > length(`profile`.`slug`)
)
	WHEN 0 THEN 'probe'
	WHEN 1 THEN 'campfire'
	WHEN 2 THEN 'lantern'
	WHEN 3 THEN 'planet'
	WHEN 4 THEN 'quantum'
	WHEN 5 THEN 'comet'
	WHEN 6 THEN 'moon'
	WHEN 7 THEN 'satellite'
END;
