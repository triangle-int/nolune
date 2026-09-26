CREATE TABLE `background_command` (
	`tool_use_id` text PRIMARY KEY NOT NULL,
	`conversation_id` text NOT NULL,
	`summary` text,
	`command` text NOT NULL,
	`started_at` integer DEFAULT (cast(unixepoch('subsecond') * 1000 as integer)) NOT NULL,
	FOREIGN KEY (`conversation_id`) REFERENCES `conversation`(`id`) ON UPDATE no action ON DELETE cascade
);
--> statement-breakpoint
CREATE INDEX `background_command_conversationId_idx` ON `background_command` (`conversation_id`);--> statement-breakpoint
CREATE TABLE `subagent` (
	`id` text PRIMARY KEY NOT NULL,
	`parent_id` text NOT NULL,
	`name` text NOT NULL,
	`conversation_id` text NOT NULL,
	`status` text NOT NULL,
	`error` text,
	`created_at` integer DEFAULT (cast(unixepoch('subsecond') * 1000 as integer)) NOT NULL,
	`updated_at` integer DEFAULT (cast(unixepoch('subsecond') * 1000 as integer)) NOT NULL,
	FOREIGN KEY (`parent_id`) REFERENCES `conversation`(`id`) ON UPDATE no action ON DELETE cascade,
	FOREIGN KEY (`conversation_id`) REFERENCES `conversation`(`id`) ON UPDATE no action ON DELETE cascade
);
--> statement-breakpoint
CREATE UNIQUE INDEX `subagent_parent_name_idx` ON `subagent` (`parent_id`,`name`);--> statement-breakpoint
CREATE INDEX `subagent_conversationId_idx` ON `subagent` (`conversation_id`);--> statement-breakpoint
CREATE INDEX `subagent_status_idx` ON `subagent` (`status`);--> statement-breakpoint
ALTER TABLE `conversation` ADD `tools` text;--> statement-breakpoint
ALTER TABLE `conversation` ADD `cache_ttl` text DEFAULT '1h' NOT NULL;