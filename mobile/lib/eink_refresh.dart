import 'package:flutter/foundation.dart';
import 'package:flutter/services.dart';

/// Portable e-ink partial-refresh contract (M8).
///
/// Backends: `noop` (default / iOS / LCD), `ratta_pw` (SuperNote framework
/// reflection), `onyx_epd` when Onyx classes resolve. OEM ioctls are not used.
///
/// On Nomad, live ink uses a host [Bitmap] + `postRectForPw` (Notes path).
/// Call [captureHost] after [beginInk], then [drawSegment] per stylus move.
class EinkRefresh {
  EinkRefresh({MethodChannel? channel})
    : _channel = channel ?? const MethodChannel('shelf.eink/refresh');

  final MethodChannel _channel;
  var _backend = 'noop';
  var _ready = false;
  var hostLive = false;

  String get backend => _backend;

  bool get isAccelerated => _backend != 'noop';

  /// Last native diagnose map from [diagnose] (Nomad O1 smoke).
  Map<String, Object?> lastDiagnose = const {};

  /// Probe once; safe on every platform (returns `noop` when unavailable).
  Future<String> ensureReady() async {
    if (_ready) return _backend;
    _ready = true;
    if (kIsWeb || defaultTargetPlatform != TargetPlatform.android) {
      _backend = 'noop';
      return _backend;
    }
    try {
      final name = await _channel.invokeMethod<String>('probe');
      _backend = (name == null || name.isEmpty) ? 'noop' : name;
    } catch (_) {
      _backend = 'noop';
    }
    return _backend;
  }

  /// Run native capability checks (logs to logcat tag `ShelfEink`).
  Future<Map<String, Object?>> diagnose() async {
    if (kIsWeb || defaultTargetPlatform != TargetPlatform.android) {
      lastDiagnose = const {'backend': 'noop'};
      return lastDiagnose;
    }
    try {
      final raw = await _channel.invokeMethod<dynamic>('diagnose');
      if (raw is Map) {
        lastDiagnose = {for (final e in raw.entries) e.key.toString(): e.value};
      } else {
        lastDiagnose = const {};
      }
    } catch (_) {
      lastDiagnose = const {};
    }
    return lastDiagnose;
  }

  Future<void> beginInk() async {
    await ensureReady();
    hostLive = false;
    if (_backend == 'noop') return;
    try {
      await _channel.invokeMethod<void>('beginInk');
      // PixelCopy the page into the PW host bmp (Nomad Notes path).
      final ok = await _channel.invokeMethod<bool>('captureHost');
      hostLive = ok == true;
    } catch (_) {
      // shortcut: OEM APIs are best-effort; keep drawing on Canvas failure.
      hostLive = false;
    }
  }

  /// Draw one segment into the host bitmap and post a PW rect (physical px).
  Future<bool> drawSegment({
    required double x0,
    required double y0,
    required double x1,
    required double y1,
    required double width,
    bool eraser = false,
  }) async {
    if (_backend == 'noop') return false;
    try {
      final ok = await _channel.invokeMethod<bool>('drawSegment', {
        'x0': x0,
        'y0': y0,
        'x1': x1,
        'y1': y1,
        'width': width,
        'eraser': eraser,
      });
      if (ok == true) hostLive = true;
      return ok == true;
    } catch (_) {
      return false;
    }
  }

  DateTime? _lastInvalidate;

  /// Ask the panel to show the latest Flutter frame. Throttled when host PW
  /// is not live; host-bitmap segments already post their own rects.
  Future<void> invalidate({
    required double left,
    required double top,
    required double right,
    required double bottom,
    Duration minInterval = const Duration(milliseconds: 16),
  }) async {
    if (_backend == 'noop') return;
    if (hostLive) return; // drawSegment already posted the PW rect
    final now = DateTime.now();
    final last = _lastInvalidate;
    if (last != null && now.difference(last) < minInterval) return;
    _lastInvalidate = now;
    try {
      await _channel.invokeMethod<void>('invalidate', {
        'left': left.round(),
        'top': top.round(),
        'right': right.round(),
        'bottom': bottom.round(),
      });
    } catch (_) {}
  }

  Future<void> endInk() async {
    hostLive = false;
    if (_backend == 'noop') return;
    try {
      await _channel.invokeMethod<void>('endInk');
    } catch (_) {}
  }

  Future<void> fullRefresh() async {
    await ensureReady();
    if (_backend == 'noop') return;
    try {
      await _channel.invokeMethod<void>('fullRefresh');
    } catch (_) {}
  }
}
