ALTER TABLE `conversation` ADD `learned_seq` integer;--> statement-breakpoint
ALTER TABLE `profile` ADD `learn_from_chats` integer DEFAULT true NOT NULL;