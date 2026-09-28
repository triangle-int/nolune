CREATE TABLE `memory_change` (
	`id` integer PRIMARY KEY AUTOINCREMENT NOT NULL,
	`profile_id` text NOT NULL,
	`conversation_id` text,
	`after_message_id` integer NOT NULL,
	`op` text NOT NULL,
	`note` text NOT NULL,
	`line` text NOT NULL,
	`before` text,
	`created_note` integer DEFAULT false NOT NULL,
	`created_at` integer DEFAULT (cast(unixepoch('subsecond') * 1000 as integer)) NOT NULL,
	`undone_at` integer,
	`undone_by` text,
	FOREIGN KEY (`profile_id`) REFERENCES `profile`(`id`) ON UPDATE no action ON DELETE cascade,
	FOREIGN KEY (`conversation_id`) REFERENCES `conversation`(`id`) ON UPDATE no action ON DELETE set null
);
--> statement-breakpoint
CREATE INDEX `memory_change_conversationId_idx` ON `memory_change` (`conversation_id`);--> statement-breakpoint
CREATE INDEX `memory_change_profileId_idx` ON `memory_change` (`profile_id`,`created_at`);