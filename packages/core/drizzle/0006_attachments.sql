CREATE TABLE `provider_file` (
	`provider` text NOT NULL,
	`account` text NOT NULL,
	`sha256` text NOT NULL,
	`file_id` text NOT NULL,
	`used_at` integer DEFAULT (cast(unixepoch('subsecond') * 1000 as integer)) NOT NULL,
	PRIMARY KEY(`provider`, `account`, `sha256`)
);
--> statement-breakpoint
CREATE INDEX `provider_file_fileId_idx` ON `provider_file` (`file_id`);--> statement-breakpoint
CREATE TABLE `upload` (
	`id` text PRIMARY KEY NOT NULL,
	`profile_id` text NOT NULL,
	`user_id` text NOT NULL,
	`name` text NOT NULL,
	`sha256` text NOT NULL,
	`mime` text NOT NULL,
	`bytes` integer NOT NULL,
	`created_at` integer DEFAULT (cast(unixepoch('subsecond') * 1000 as integer)) NOT NULL,
	FOREIGN KEY (`profile_id`) REFERENCES `profile`(`id`) ON UPDATE no action ON DELETE cascade,
	FOREIGN KEY (`user_id`) REFERENCES `user`(`id`) ON UPDATE no action ON DELETE cascade
);
--> statement-breakpoint
CREATE INDEX `upload_profileId_idx` ON `upload` (`profile_id`);--> statement-breakpoint
ALTER TABLE `message` ADD `attachments` text;