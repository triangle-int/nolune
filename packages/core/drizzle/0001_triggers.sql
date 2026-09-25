CREATE TABLE `notification` (
	`id` text PRIMARY KEY NOT NULL,
	`profile_id` text NOT NULL,
	`trigger_id` text,
	`conversation_id` text,
	`title` text NOT NULL,
	`body` text NOT NULL,
	`level` text DEFAULT 'info' NOT NULL,
	`created_at` integer DEFAULT (cast(unixepoch('subsecond') * 1000 as integer)) NOT NULL,
	FOREIGN KEY (`profile_id`) REFERENCES `profile`(`id`) ON UPDATE no action ON DELETE cascade,
	FOREIGN KEY (`trigger_id`) REFERENCES `trigger`(`id`) ON UPDATE no action ON DELETE set null,
	FOREIGN KEY (`conversation_id`) REFERENCES `conversation`(`id`) ON UPDATE no action ON DELETE set null
);
--> statement-breakpoint
CREATE INDEX `notification_profileId_idx` ON `notification` (`profile_id`,`created_at`);--> statement-breakpoint
CREATE TABLE `notification_dismissal` (
	`notification_id` text NOT NULL,
	`user_id` text NOT NULL,
	PRIMARY KEY(`notification_id`, `user_id`),
	FOREIGN KEY (`notification_id`) REFERENCES `notification`(`id`) ON UPDATE no action ON DELETE cascade,
	FOREIGN KEY (`user_id`) REFERENCES `user`(`id`) ON UPDATE no action ON DELETE cascade
);
--> statement-breakpoint
CREATE TABLE `notification_seen` (
	`user_id` text PRIMARY KEY NOT NULL,
	`seen_at` integer NOT NULL,
	FOREIGN KEY (`user_id`) REFERENCES `user`(`id`) ON UPDATE no action ON DELETE cascade
);
--> statement-breakpoint
CREATE TABLE `trigger` (
	`id` text PRIMARY KEY NOT NULL,
	`profile_id` text NOT NULL,
	`name` text NOT NULL,
	`kind` text NOT NULL,
	`cron` text,
	`run_at` integer,
	`webhook_token` text,
	`action` text NOT NULL,
	`prompt` text,
	`command` text,
	`preset_id` text,
	`effort` text DEFAULT 'medium' NOT NULL,
	`enabled` integer DEFAULT true NOT NULL,
	`next_run_at` integer,
	`last_run_at` integer,
	`created_by` text,
	`created_at` integer DEFAULT (cast(unixepoch('subsecond') * 1000 as integer)) NOT NULL,
	FOREIGN KEY (`profile_id`) REFERENCES `profile`(`id`) ON UPDATE no action ON DELETE cascade,
	FOREIGN KEY (`preset_id`) REFERENCES `model_preset`(`id`) ON UPDATE no action ON DELETE set null,
	FOREIGN KEY (`created_by`) REFERENCES `user`(`id`) ON UPDATE no action ON DELETE set null
);
--> statement-breakpoint
CREATE UNIQUE INDEX `trigger_webhook_token_unique` ON `trigger` (`webhook_token`);--> statement-breakpoint
CREATE INDEX `trigger_profileId_idx` ON `trigger` (`profile_id`);--> statement-breakpoint
CREATE INDEX `trigger_nextRunAt_idx` ON `trigger` (`next_run_at`);--> statement-breakpoint
CREATE TABLE `trigger_run` (
	`id` text PRIMARY KEY NOT NULL,
	`profile_id` text NOT NULL,
	`trigger_id` text,
	`title` text NOT NULL,
	`action` text NOT NULL,
	`source` text NOT NULL,
	`status` text NOT NULL,
	`prompt` text,
	`payload` text,
	`preset_id` text,
	`effort` text DEFAULT 'medium' NOT NULL,
	`conversation_id` text,
	`output` text,
	`created_at` integer DEFAULT (cast(unixepoch('subsecond') * 1000 as integer)) NOT NULL,
	`started_at` integer,
	`finished_at` integer,
	FOREIGN KEY (`profile_id`) REFERENCES `profile`(`id`) ON UPDATE no action ON DELETE cascade,
	FOREIGN KEY (`trigger_id`) REFERENCES `trigger`(`id`) ON UPDATE no action ON DELETE set null,
	FOREIGN KEY (`preset_id`) REFERENCES `model_preset`(`id`) ON UPDATE no action ON DELETE set null,
	FOREIGN KEY (`conversation_id`) REFERENCES `conversation`(`id`) ON UPDATE no action ON DELETE set null
);
--> statement-breakpoint
CREATE INDEX `trigger_run_triggerId_idx` ON `trigger_run` (`trigger_id`,`created_at`);--> statement-breakpoint
CREATE INDEX `trigger_run_status_idx` ON `trigger_run` (`status`);--> statement-breakpoint
CREATE INDEX `trigger_run_conversationId_idx` ON `trigger_run` (`conversation_id`);--> statement-breakpoint
ALTER TABLE `conversation` ADD `hidden` integer DEFAULT false NOT NULL;