import { Effect, Stream } from "effect";

export const makeMediaStreamHandler = (
  trackStream: Stream.Stream<RTCTrackEvent>,
  onTrack: (stream: MediaStream) => void,
) =>
  Effect.gen(function* () {
    let remoteStream: MediaStream | null = null;

    yield* trackStream.pipe(
      Stream.runForEach((event) =>
        Effect.sync(() => {
          if (event.track) {
            if (!remoteStream) {
              remoteStream = new MediaStream();
            }
            remoteStream.addTrack(event.track);
            onTrack(remoteStream);
          }
        }),
      ),
    );
  });
