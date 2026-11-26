CREATE TABLE "server_templates" (
  "uuid" uuid PRIMARY KEY DEFAULT gen_random_uuid() NOT NULL,
  "name" varchar(255) NOT NULL,
  "description" varchar(1024),
  "owner_uuid" uuid REFERENCES "users" ("uuid") ON DELETE CASCADE,
  "source_server_uuid" uuid REFERENCES "servers" ("uuid") ON DELETE SET NULL,
  "snapshot" jsonb NOT NULL,
  "created" timestamp NOT NULL DEFAULT now(),
  "updated" timestamp NOT NULL DEFAULT now()
);
CREATE UNIQUE INDEX "server_templates_owner_name_idx" ON "server_templates" ("owner_uuid", "name");
CREATE UNIQUE INDEX "server_templates_global_name_idx" ON "server_templates" ("name") WHERE "owner_uuid" IS NULL;
CREATE INDEX "server_templates_owner_idx" ON "server_templates" ("owner_uuid");
CREATE INDEX "server_templates_source_server_idx" ON "server_templates" ("source_server_uuid");
