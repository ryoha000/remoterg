import { Effect, Queue, Schedule, Duration } from "effect";

export const createDataChannel = (pc: RTCPeerConnection, label: string = "input") =>
  Effect.acquireRelease(
    Effect.sync(() => pc.createDataChannel(label, { ordered: true })),
    (dc) => Effect.sync(() => dc.close()),
  );

export const runDataChannel = (
  dc: RTCDataChannel,
  keyQ: Queue.Queue<{ key: string; down: boolean }>,
  screenQ: Queue.Queue<void>,
  onOpen: () => void,
) =>
  Effect.gen(function* () {
    const waitForOpen = Effect.async<void>((resume) => {
      if (dc.readyState === "open") resume(Effect.void);
      else {
        const handler = () => {
          dc.removeEventListener("open", handler);
          resume(Effect.void);
        };
        dc.addEventListener("open", handler);
      }
    });

    const waitForCloseInitial = Effect.async<void>((resume) => {
      if (dc.readyState === "closed") resume(Effect.void);
      else {
        const handler = () => {
          dc.removeEventListener("close", handler);
          resume(Effect.void);
        };
        dc.addEventListener("close", handler);
      }
    });

    // Race to ensure we don't hang if it closes while waiting to open
    yield* Effect.race(waitForOpen, waitForCloseInitial);

    if (dc.readyState !== "open") {
      return;
    }

    onOpen();

    const keepAlive = Effect.repeat(
      Effect.sync(() => {
        if (dc.readyState === "open") {
          dc.send(JSON.stringify({ Ping: { timestamp: Date.now() } }));
        }
      }),
      Schedule.spaced(Duration.seconds(3)),
    );

    const processKeys = Queue.take(keyQ).pipe(
      Effect.tap((item) =>
        Effect.sync(() => {
          if (dc.readyState === "open") {
            dc.send(JSON.stringify({ Key: { key: item.key, down: item.down } }));
          }
        }),
      ),
      Effect.forever,
    );

    const processScreens = Queue.take(screenQ).pipe(
      Effect.tap(() =>
        Effect.sync(() => {
          if (dc.readyState === "open") {
            dc.send(JSON.stringify({ ScreenshotRequest: null }));
          }
        }),
      ),
      Effect.forever,
    );

    const waitForClose = Effect.async<void>((resume) => {
      const onClose = () => {
        dc.removeEventListener("close", onClose);
        resume(Effect.void);
      };
      // Close and error both terminate the loop
      dc.addEventListener("close", onClose);
      dc.addEventListener("error", onClose);

      return Effect.sync(() => {
        dc.removeEventListener("close", onClose);
        dc.removeEventListener("error", onClose);
      });
    });

    yield* Effect.race(
      Effect.all([keepAlive, processKeys, processScreens], { concurrency: "unbounded" }),
      waitForClose,
    );
  });
