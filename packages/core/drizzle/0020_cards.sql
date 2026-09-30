CREATE TABLE `card` (
	`user_id` text PRIMARY KEY NOT NULL,
	`name` text NOT NULL,
	`created_at` integer DEFAULT (cast(unixepoch('subsecond') * 1000 as integer)) NOT NULL,
	FOREIGN KEY (`user_id`) REFERENCES `user`(`id`) ON UPDATE no action ON DELETE cascade
);
--> statement-breakpoint
CREATE UNIQUE INDEX `card_name_unique` ON `card` (`name`);--> statement-breakpoint
ALTER TABLE `memory_change` ADD `source` text DEFAULT 'learning' NOT NULL;