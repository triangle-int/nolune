-- btw was renamed nolune: its own format's rows and its built-in skill go by the new name.
UPDATE `message` SET `format` = 'nolune' WHERE `format` = 'btw';--> statement-breakpoint
UPDATE `profile` SET `disabled_skills` = replace(`disabled_skills`, '"btw-agent"', '"nolune"') WHERE `disabled_skills` LIKE '%"btw-agent"%';
