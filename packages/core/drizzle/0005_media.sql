CREATE TABLE `media` (
	`id` text PRIMARY KEY NOT NULL,
	`conversation_id` text NOT NULL,
	`message_id` integer NOT NULL,
	`src` text NOT NULL,
	`status` text NOT NULL,
	`error` text,
	`name` text NOT NULL,
	`sha256` text,
	`mime` text,
	`bytes` integer,
	`width` integer,
	`height` integer,
	`preview_sha256` text,
	`created_at` integer DEFAULT (cast(unixepoch('subsecond') * 1000 as integer)) NOT NULL,
	FOREIGN KEY (`conversation_id`) REFERENCES `conversation`(`id`) ON UPDATE no action ON DELETE cascade,
	FOREIGN KEY (`message_id`) REFERENCES `message`(`id`) ON UPDATE no action ON DELETE cascade
);
--> statement-breakpoint
CREATE INDEX `media_messageId_idx` ON `media` (`message_id`);--> statement-breakpoint
CREATE INDEX `media_conversationId_idx` ON `media` (`conversation_id`);