import * as Sentry from "@sentry/tanstackstart-react";

const dsn =
  // When this file runs under Node (dev/server), Vite replacements are not available
  process.env.VITE_SENTRY_DSN ??
  process.env.SENTRY_DSN ??
  (typeof import.meta !== "undefined"
    ? import.meta.env?.VITE_SENTRY_DSN
    : undefined);

if (dsn) {
  Sentry.init({
    dsn,
    // Adds request headers and IP for users, for more info visit:
    // https://docs.sentry.io/platforms/javascript/guides/tanstackstart-react/configuration/options/#sendDefaultPii
    sendDefaultPii: true,
    tracesSampleRate: 1.0,
    replaysSessionSampleRate: 1.0,
    replaysOnErrorSampleRate: 1.0,
  });
} else {
  console.warn("[sentry] VITE_SENTRY_DSN is not set; Sentry is disabled.");
}
