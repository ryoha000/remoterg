PRAGMA foreign_keys=OFF;--> statement-breakpoint
CREATE TABLE `__new_screenshot_map` (
	`local_id` text PRIMARY KEY NOT NULL,
	`host_id` text NOT NULL,
	`window_title` text DEFAULT '' NOT NULL,
	`process_path` text DEFAULT '' NOT NULL,
	`process_name` text DEFAULT '' NOT NULL
);
--> statement-breakpoint
INSERT INTO `__new_screenshot_map`("local_id", "host_id", "window_title", "process_path", "process_name") SELECT "local_id", "host_id", "window_title", "process_path", "process_name" FROM `screenshot_map`;--> statement-breakpoint
DROP TABLE `screenshot_map`;--> statement-breakpoint
ALTER TABLE `__new_screenshot_map` RENAME TO `screenshot_map`;--> statement-breakpoint
PRAGMA foreign_keys=ON;