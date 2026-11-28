--> statement-breakpoint
CREATE TABLE "webhook_subscriptions" (
	"uuid" uuid PRIMARY KEY DEFAULT gen_random_uuid() NOT NULL,
	"name" varchar(128) NOT NULL,
	"url" varchar(2048) NOT NULL,
	"secret" bytea NOT NULL,
	"events" text[] DEFAULT '{}' NOT NULL,
	"enabled" boolean DEFAULT true NOT NULL,
	"created_by_uuid" uuid,
	"created" timestamp DEFAULT now() NOT NULL,
	"updated" timestamp DEFAULT now() NOT NULL,
	CONSTRAINT "webhook_subscriptions_created_by_uuid_users_uuid_fk" FOREIGN KEY ("created_by_uuid") REFERENCES "public"."users"("uuid") ON DELETE set null ON UPDATE no action
);
--> statement-breakpoint
CREATE INDEX "webhook_subscriptions_enabled_idx" ON "webhook_subscriptions" USING btree ("enabled");
--> statement-breakpoint
CREATE TABLE "webhook_deliveries" (
	"uuid" uuid PRIMARY KEY DEFAULT gen_random_uuid() NOT NULL,
	"subscription_uuid" uuid NOT NULL,
	"event" varchar(255) NOT NULL,
	"attempt" integer DEFAULT 1 NOT NULL,
	"status_code" integer,
	"success" boolean DEFAULT false NOT NULL,
	"error" text,
	"created" timestamp DEFAULT now() NOT NULL,
	CONSTRAINT "webhook_deliveries_subscription_uuid_webhook_subscriptions_uuid_fk" FOREIGN KEY ("subscription_uuid") REFERENCES "public"."webhook_subscriptions"("uuid") ON DELETE cascade ON UPDATE no action
);
--> statement-breakpoint
CREATE INDEX "webhook_deliveries_subscription_created_idx" ON "webhook_deliveries" USING btree ("subscription_uuid","created");
