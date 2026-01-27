CREATE TABLE `analysis_results` (
	`local_id` text PRIMARY KEY NOT NULL,
	`data` text NOT NULL,
	`created_at` integer NOT NULL
);
--> statement-breakpoint
CREATE TABLE `screenshot_map` (
	`local_id` text PRIMARY KEY NOT NULL,
	`host_id` text NOT NULL
);
