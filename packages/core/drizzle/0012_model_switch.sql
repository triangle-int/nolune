ALTER TABLE `message` ADD `provider` text;--> statement-breakpoint
ALTER TABLE `message` ADD `model` text;--> statement-breakpoint
UPDATE `message` SET `provider` = (SELECT `provider` FROM `conversation` WHERE `conversation`.`id` = `message`.`conversation_id`);--> statement-breakpoint
UPDATE `message` SET `model` = (SELECT `model` FROM `conversation` WHERE `conversation`.`id` = `message`.`conversation_id`) WHERE `role` = 'assistant';
