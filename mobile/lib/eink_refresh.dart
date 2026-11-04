import 'dart:async';

import 'package:flutter/foundation.dart';
import 'package:flutter/services.dart';

import 'ink_stroke.dart';

/// Portable e-ink ink / refresh contract (M8).
///
/// On Nomad (`ratta_pw`): Shelf's native translucent [InkHostView] draws live
/// ink. Dart receives completed strokes for persistence / post-exit paint.
/// Not the Notes HandWrite/PW EMR stack.
class EinkRefresh {
  EinkRefresh({MethodChannel? channel, EventChannel? strokes})
    : _channel = channel ?? const MethodChannel('shelf.eink/refresh'),
      _strokesChannel = strokes ?? const EventChannel('shelf.eink/strokes');

  final MethodChannel _channel;
  final EventChannel _strokesChannel;
  final _strokeController = StreamController<InkStroke>.broadcast();
  StreamSubscription<dynamic>? _strokeSub;
  var _backend = 'noop';
  var _ready = false;
  var hostLive = false;

  String get backend => _backend;

  bool get isAccelerated => _backend != 'noop';

  /// Native-completed strokes (accelerated path).
  Stream<InkStroke> get nativeStrokes => _strokeController.stream;

  Map<String, Object?> lastDiagnose = const {};

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
    _ensureStrokeListen();
    return _backend;
  }

  void _ensureStrokeListen() {
    if (_strokeSub != null) return;
    if (kIsWeb || defaultTargetPlatform != TargetPlatform.android) return;
    _strokeSub = _strokesChannel.receiveBroadcastStream().listen((raw) {
      if (raw is! Map) return;
      final stroke = InkStroke.fromJson(raw);
      if (stroke != null) _strokeController.add(stroke);
    }, onError: (_) {});
  }

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

  /// [content] is the ink overlay in **physical** screen pixels
  /// (`left`, `top`, `width`, `height`) so the native host matches Flutter.
  Future<void> beginInk({Map<String, double>? content}) async {
    await ensureReady();
    hostLive = false;
    if (_backend == 'noop') return;
    _ensureStrokeListen();
    try {
      await _channel.invokeMethod<void>('beginInk', content);
      hostLive = true;
    } catch (_) {
      hostLive = false;
    }
  }

  Future<bool> recaptureHost() async => hostLive;

  Future<void> clearInkLayer() async {
    if (_backend == 'noop') return;
    try {
      await _channel.invokeMethod<void>('clearInk');
    } catch (_) {}
  }

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
      return ok == true;
    } catch (_) {
      return false;
    }
  }

  Future<void> invalidate({
    required double left,
    required double top,
    required double right,
    required double bottom,
    Duration minInterval = const Duration(milliseconds: 16),
  }) async {
    // Accelerated: native InkHostView owns mid-stroke refresh.
    if (_backend == 'noop') return;
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

  Future<void> dispose() async {
    await _strokeSub?.cancel();
    _strokeSub = null;
    await _strokeController.close();
  }
}
