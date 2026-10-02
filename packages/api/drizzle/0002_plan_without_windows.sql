ALTER TABLE "plan" ALTER COLUMN "window_limit" DROP NOT NULL;--> statement-breakpoint
ALTER TABLE "plan" ALTER COLUMN "week_limit" DROP NOT NULL;--> statement-breakpoint
ALTER TABLE "plan" ADD COLUMN "month_limit" bigint;--> statement-breakpoint
ALTER TABLE "plan" ADD COLUMN "background_limit" bigint DEFAULT 0 NOT NULL;--> statement-breakpoint
ALTER TABLE "plan" ADD COLUMN "background_opened_at" timestamp with time zone;--> statement-breakpoint
ALTER TABLE "plan" ADD COLUMN "background_spent" bigint DEFAULT 0 NOT NULL;--> statement-breakpoint
-- Plans going (a weekly limit above 0 marked one): the month is what's left of their credits, and
-- background work may spend a tenth of it a day. Plans no longer have 5-hour or weekly limits.
UPDATE "plan" SET "month_limit" = GREATEST(COALESCE((SELECT SUM("credit"."left") FROM "credit" WHERE "credit"."user_id" = "plan"."user_id" AND "credit"."kind" = 'plan'), 0), 0) WHERE "week_limit" > 0;--> statement-breakpoint
UPDATE "plan" SET "background_limit" = "month_limit" / 10 WHERE "month_limit" IS NOT NULL;--> statement-breakpoint
UPDATE "plan" SET "window_limit" = NULL, "week_limit" = NULL;
