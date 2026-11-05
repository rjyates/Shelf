import 'dart:async';

import 'package:flutter/foundation.dart';
import 'package:flutter/services.dart';

import 'ink_stroke.dart';

/// E-ink ink / refresh contract.
///
/// Accelerated live ink is **Nomad / `ratta_pw` only** today: native
/// [InkHostView] + SPEED/A2 (see `docs/research/nomad-stylus.md`). Elsewhere
/// `noop` → Flutter Canvas in [InkOverlay]. Planned: generic Android tablet
/// mid-stroke path, then golden-device OEM backends.
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

  /// True when native Shelf overlay owns mid-stroke paint.
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

  Future<void> clearInkLayer() async {
    if (_backend == 'noop') return;
    try {
      await _channel.invokeMethod<void>('clearInk');
    } catch (_) {}
  }

  Future<void> endInk() async {
    hostLive = false;
    if (_backend == 'noop') return;
    try {
      await _channel.invokeMethod<void>('endInk');
    } catch (_) {}
  }

  Future<void> dispose() async {
    await _strokeSub?.cancel();
    _strokeSub = null;
    await _strokeController.close();
  }
}
