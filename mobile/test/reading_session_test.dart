import 'package:flutter/widgets.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:shelf_mobile/reading_session.dart';

void main() {
  TestWidgetsFlutterBinding.ensureInitialized();

  test('discards sessions shorter than 10 seconds', () async {
    var posted = 0;
    var progress = 10.0;
    final clock = _Clock(DateTime.utc(2026, 1, 1, 12));
    final tracker = ReadingSessionTracker(
      bookId: 1,
      sessionType: 'read',
      source: 'android',
      getProgressPercent: () => progress,
      onSessionEnd: (_) async {
        posted++;
      },
      now: clock.now,
      idleDuration: const Duration(hours: 1),
    );
    tracker.onActivity();
    clock.advance(const Duration(seconds: 9));
    progress = 12.0;
    await tracker.endSession();
    expect(posted, 0);
    tracker.detach();
  });

  test('posts session with duration and progress delta after 10s', () async {
    ReadingSessionPayload? payload;
    var progress = 20.0;
    final clock = _Clock(DateTime.utc(2026, 1, 1, 12));
    final tracker = ReadingSessionTracker(
      bookId: 42,
      sessionType: 'read',
      source: 'android',
      getProgressPercent: () => progress,
      onSessionEnd: (p) async {
        payload = p;
      },
      now: clock.now,
      idleDuration: const Duration(hours: 1),
    );
    tracker.onActivity();
    clock.advance(const Duration(seconds: 15));
    progress = 27.5;
    await tracker.endSession();
    expect(payload, isNotNull);
    expect(payload!.bookId, 42);
    expect(payload!.durationSeconds, 15);
    expect(payload!.progressDelta, closeTo(7.5, 0.0001));
    expect(payload!.endProgress, 27.5);
    expect(payload!.startProgress, 20.0);
    expect(payload!.sessionType, 'read');
    expect(payload!.toJson()['sessionId'], isNotEmpty);
    tracker.detach();
  });

  test('pause while inactive excludes wall time from duration', () async {
    ReadingSessionPayload? payload;
    final clock = _Clock(DateTime.utc(2026, 1, 1, 12));
    final tracker = ReadingSessionTracker(
      bookId: 1,
      sessionType: 'listen',
      source: 'android',
      getProgressPercent: () => 50.0,
      onSessionEnd: (p) async {
        payload = p;
      },
      now: clock.now,
      idleDuration: const Duration(hours: 1),
    );
    tracker.onActivity();
    clock.advance(const Duration(seconds: 8));
    tracker.didChangeAppLifecycleState(AppLifecycleState.paused);
    clock.advance(const Duration(seconds: 60));
    tracker.didChangeAppLifecycleState(AppLifecycleState.resumed);
    clock.advance(const Duration(seconds: 7));
    await tracker.endSession();
    expect(payload, isNotNull);
    expect(payload!.durationSeconds, 15);
    tracker.detach();
  });

  test('setSessionType ends prior session and tags the next', () async {
    final payloads = <ReadingSessionPayload>[];
    final clock = _Clock(DateTime.utc(2026, 1, 1, 12));
    final tracker = ReadingSessionTracker(
      bookId: 1,
      sessionType: 'read',
      source: 'android',
      getProgressPercent: () => 10.0,
      onSessionEnd: (p) async {
        payloads.add(p);
      },
      now: clock.now,
      idleDuration: const Duration(hours: 1),
    );
    tracker.onActivity();
    clock.advance(const Duration(seconds: 12));
    await tracker.setSessionType('tts');
    expect(payloads, hasLength(1));
    expect(payloads.first.sessionType, 'read');
    tracker.onActivity();
    clock.advance(const Duration(seconds: 12));
    await tracker.endSession();
    expect(payloads, hasLength(2));
    expect(payloads.last.sessionType, 'tts');
    tracker.detach();
  });
}

class _Clock {
  _Clock(this._now);
  DateTime _now;
  DateTime now() => _now;
  void advance(Duration d) => _now = _now.add(d);
}
