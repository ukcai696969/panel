-- Billing / credits (Feature 4, first version).
--
-- Money rule: 1 credit = 1 minor currency unit (1 cent for USD/GBP/EUR).
-- All money columns are BIGINT credits — never floats.
--
-- NOTE: Drizzle keeps a snapshot.json next to every migration.sql. Do NOT
-- hand-write it: regenerate it with the project's Drizzle tooling after
-- this migration is applied in development (see MANIFEST.md). This file
-- is the source of truth for the schema change.

CREATE TABLE "user_credits" (
    "uuid" uuid PRIMARY KEY DEFAULT gen_random_uuid() NOT NULL,
    "user_uuid" uuid NOT NULL,
    "balance_credits" bigint NOT NULL DEFAULT 0,
    "created" timestamp NOT NULL DEFAULT now(),
    "updated" timestamp NOT NULL DEFAULT now(),
    CONSTRAINT "user_credits_user_uuid_unique" UNIQUE ("user_uuid"),
    CONSTRAINT "user_credits_user_uuid_fk"
        FOREIGN KEY ("user_uuid") REFERENCES "users" ("uuid")
        ON DELETE CASCADE ON UPDATE CASCADE
);
--> statement-breakpoint
CREATE INDEX "user_credits_user_uuid_idx" ON "user_credits" ("user_uuid");
--> statement-breakpoint
CREATE TABLE "user_credit_transactions" (
    "uuid" uuid PRIMARY KEY DEFAULT gen_random_uuid() NOT NULL,
    "user_uuid" uuid NOT NULL,
    "kind" text NOT NULL,
    "amount_credits" bigint NOT NULL,
    "balance_after_credits" bigint NOT NULL,
    "server_uuid" uuid,
    "reference" varchar(255),
    "description" varchar(512),
    "created" timestamp NOT NULL DEFAULT now(),
    CONSTRAINT "user_credit_transactions_kind_check"
        CHECK ("kind" IN ('purchase', 'charge', 'refund', 'adjustment')),
    CONSTRAINT "user_credit_transactions_amount_check"
        CHECK ("amount_credits" <> 0),
    CONSTRAINT "user_credit_transactions_user_uuid_fk"
        FOREIGN KEY ("user_uuid") REFERENCES "users" ("uuid")
        ON DELETE CASCADE ON UPDATE CASCADE,
    CONSTRAINT "user_credit_transactions_server_uuid_fk"
        FOREIGN KEY ("server_uuid") REFERENCES "servers" ("uuid")
        ON DELETE SET NULL ON UPDATE CASCADE
);
--> statement-breakpoint
CREATE INDEX "user_credit_transactions_user_uuid_created_idx"
    ON "user_credit_transactions" ("user_uuid", "created" DESC, "uuid" DESC);
--> statement-breakpoint
CREATE INDEX "user_credit_transactions_reference_idx"
    ON "user_credit_transactions" ("reference")
    WHERE "reference" IS NOT NULL;
--> statement-breakpoint
CREATE TABLE "server_billing_states" (
    "uuid" uuid PRIMARY KEY DEFAULT gen_random_uuid() NOT NULL,
    "server_uuid" uuid NOT NULL,
    "last_charged_at" timestamp,
    "total_charged_credits" bigint NOT NULL DEFAULT 0,
    "created" timestamp NOT NULL DEFAULT now(),
    "updated" timestamp NOT NULL DEFAULT now(),
    CONSTRAINT "server_billing_states_server_uuid_unique" UNIQUE ("server_uuid"),
    CONSTRAINT "server_billing_states_server_uuid_fk"
        FOREIGN KEY ("server_uuid") REFERENCES "servers" ("uuid")
        ON DELETE CASCADE ON UPDATE CASCADE
);
--> statement-breakpoint
CREATE INDEX "server_billing_states_server_uuid_idx"
    ON "server_billing_states" ("server_uuid");
