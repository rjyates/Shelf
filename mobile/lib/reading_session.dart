import 'dart:async';
import 'dart:math';

import 'package:flutter/widgets.dart';

/// Book Orbit–shaped reading session timer (capture only).
///
/// Rules mirrored from `useReadingSession.ts`:
/// - idle timeout 5 minutes → end session
/// - minimum 10 seconds active before POST
/// - pause while app is hidden / inactive
class ReadingSessionTracker with WidgetsBindingObserver {
  ReadingSessionTracker({
    required this.bookId,
    required this.sessionType,
    required this.source,
    required this.getProgressPercent,
    required this.onSessionEnd,
    DateTime Function()? now,
    this.idleDuration = idleTimeout,
  }) : _now = now ?? DateTime.now;

  final int bookId;
  String sessionType; // read | tts | listen
  final String source; // android | ios | web
  final double Function() getProgressPercent;
  final Future<void> Function(ReadingSessionPayload payload) onSessionEnd;

  static const idleTimeout = Duration(minutes: 5);
  static const minSession = Duration(seconds: 10);

  String _sessionId = _newId();
  DateTime? _startedAt;
  DateTime? _activeStart;
  var _activeMs = 0;
  double? _startProgress;
  var _ended = true;
  Timer? _idleTimer;
  var _observing = false;
  final DateTime Function() _now;
  final Duration idleDuration;

  void attach() {
    if (_observing) return;
    WidgetsBinding.instance.addObserver(this);
    _observing = true;
  }

  void detach() {
    if (!_observing) return;
    WidgetsBinding.instance.removeObserver(this);
    _observing = false;
  }

  void onActivity() {
    if (_ended || _startedAt == null) {
      _sessionId = _newId();
      _startSession();
      return;
    }
    if (_activeStart == null) _resumeTimer();
    _resetIdleTimer();
  }

  /// End the current session (if any) and tag subsequent activity with [type].
  Future<void> setSessionType(String type) async {
    if (sessionType == type) {
      onActivity();
      return;
    }
    await endSession();
    sessionType = type;
  }

  Future<void> endSession() async {
    if (_ended || _startedAt == null) {
      _clearIdleTimer();
      return;
    }
    _ended = true;
    _clearIdleTimer();
    if (_activeStart != null) {
      _activeMs += _now().difference(_activeStart!).inMilliseconds;
      _activeStart = null;
    }
    if (_activeMs < minSession.inMilliseconds) {
      _startedAt = null;
      _activeMs = 0;
      return;
    }
    final endedAt = _now().toUtc();
    final start = _startedAt!.toUtc();
    final endProgress = getProgressPercent().clamp(0.0, 100.0);
    final startProgress = (_startProgress ?? endProgress).clamp(0.0, 100.0);
    final payload = ReadingSessionPayload(
      sessionId: _sessionId,
      bookId: bookId,
      startedAt: start,
      endedAt: endedAt,
      durationSeconds: _activeMs ~/ 1000,
      progressDelta: endProgress - startProgress,
      endProgress: endProgress,
      startProgress: startProgress,
      sessionType: sessionType,
      source: source,
      timezoneOffsetMinutes: DateTime.now().timeZoneOffset.inMinutes * -1,
    );
    _startedAt = null;
    _activeMs = 0;
    await onSessionEnd(payload);
  }

  @override
  void didChangeAppLifecycleState(AppLifecycleState state) {
    if (state == AppLifecycleState.resumed) {
      if (_startedAt != null && !_ended) _resumeTimer();
    } else {
      _pauseTimer();
    }
  }

  void _startSession() {
    _startedAt = _now();
    _activeStart = _now();
    _activeMs = 0;
    _ended = false;
    _startProgress = getProgressPercent().clamp(0.0, 100.0);
    _resetIdleTimer();
  }

  void _pauseTimer() {
    if (_activeStart != null) {
      _activeMs += _now().difference(_activeStart!).inMilliseconds;
      _activeStart = null;
    }
    _clearIdleTimer();
  }

  void _resumeTimer() {
    if (_activeStart != null) return;
    _activeStart = _now();
    _resetIdleTimer();
  }

  void _resetIdleTimer() {
    _clearIdleTimer();
    _idleTimer = Timer(idleDuration, () {
      unawaited(endSession());
    });
  }

  void _clearIdleTimer() {
    _idleTimer?.cancel();
    _idleTimer = null;
  }

  static String _newId() {
    final rand = Random.secure();
    final bytes = List<int>.generate(16, (_) => rand.nextInt(256));
    bytes[6] = (bytes[6] & 0x0f) | 0x40;
    bytes[8] = (bytes[8] & 0x3f) | 0x80;
    String hex(int b) => b.toRadixString(16).padLeft(2, '0');
    final h = bytes.map(hex).join();
    return '${h.substring(0, 8)}-${h.substring(8, 12)}-${h.substring(12, 16)}-${h.substring(16, 20)}-${h.substring(20)}';
  }
}

class ReadingSessionPayload {
  const ReadingSessionPayload({
    required this.sessionId,
    required this.bookId,
    required this.startedAt,
    required this.endedAt,
    required this.durationSeconds,
    required this.progressDelta,
    required this.endProgress,
    required this.startProgress,
    required this.sessionType,
    required this.source,
    required this.timezoneOffsetMinutes,
  });

  final String sessionId;
  final int bookId;
  final DateTime startedAt;
  final DateTime endedAt;
  final int durationSeconds;
  final double progressDelta;
  final double endProgress;
  final double startProgress;
  final String sessionType;
  final String source;
  final int timezoneOffsetMinutes;

  Map<String, dynamic> toJson() => {
    'sessionId': sessionId,
    'startedAt': startedAt.toIso8601String(),
    'endedAt': endedAt.toIso8601String(),
    'durationSeconds': durationSeconds,
    'progressDelta': double.parse(progressDelta.toStringAsFixed(4)),
    'endProgress': endProgress,
    'startProgress': startProgress,
    'sessionType': sessionType,
    'source': source,
    'timezoneOffsetMinutes': timezoneOffsetMinutes,
  };
}
