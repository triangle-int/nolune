CREATE TABLE `folder` (
	`id` text PRIMARY KEY NOT NULL,
	`profile_id` text NOT NULL,
	`slug` text NOT NULL,
	`name` text NOT NULL,
	`instructions` text DEFAULT '' NOT NULL,
	`created_by` text,
	`created_at` integer DEFAULT (cast(unixepoch('subsecond') * 1000 as integer)) NOT NULL,
	FOREIGN KEY (`profile_id`) REFERENCES `profile`(`id`) ON UPDATE no action ON DELETE cascade,
	FOREIGN KEY (`created_by`) REFERENCES `user`(`id`) ON UPDATE no action ON DELETE set null
);
--> statement-breakpoint
CREATE UNIQUE INDEX `folder_profile_slug_idx` ON `folder` (`profile_id`,`slug`);--> statement-breakpoint
CREATE TABLE `folder_file` (
	`id` text PRIMARY KEY NOT NULL,
	`folder_id` text NOT NULL,
	`name` text NOT NULL,
	`path` text NOT NULL,
	`sha256` text NOT NULL,
	`mime` text NOT NULL,
	`bytes` integer NOT NULL,
	`width` integer,
	`height` integer,
	`preview_sha256` text,
	`created_by` text,
	`created_at` integer DEFAULT (cast(unixepoch('subsecond') * 1000 as integer)) NOT NULL,
	FOREIGN KEY (`folder_id`) REFERENCES `folder`(`id`) ON UPDATE no action ON DELETE cascade,
	FOREIGN KEY (`created_by`) REFERENCES `user`(`id`) ON UPDATE no action ON DELETE set null
);
--> statement-breakpoint
CREATE INDEX `folder_file_folderId_idx` ON `folder_file` (`folder_id`);--> statement-breakpoint
ALTER TABLE `conversation` ADD `folder_id` text REFERENCES folder(id) ON UPDATE no action ON DELETE set null;--> statement-breakpoint
ALTER TABLE `conversation` ADD `folder_context` text DEFAULT '' NOT NULL;--> statement-breakpoint
ALTER TABLE `conversation` ADD `prompt_changed_at_seq` integer;--> statement-breakpoint
CREATE INDEX `conversation_folderId_idx` ON `conversation` (`folder_id`);