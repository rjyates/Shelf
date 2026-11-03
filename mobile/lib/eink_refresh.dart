import 'package:flutter/foundation.dart';
import 'package:flutter/services.dart';

/// Portable e-ink partial-refresh contract (M8).
///
/// Backends: `noop` (default / iOS / LCD), `ratta_pw` (SuperNote framework
/// reflection), `onyx_epd` when Onyx classes resolve. OEM ioctls are not used.
class EinkRefresh {
  EinkRefresh({MethodChannel? channel})
    : _channel = channel ?? const MethodChannel('shelf.eink/refresh');

  final MethodChannel _channel;
  var _backend = 'noop';
  var _ready = false;

  String get backend => _backend;

  bool get isAccelerated => _backend != 'noop';

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

  Future<void> beginInk() async {
    await ensureReady();
    if (_backend == 'noop') return;
    try {
      await _channel.invokeMethod<void>('beginInk');
    } catch (_) {
      // shortcut: OEM APIs are best-effort; keep drawing on Canvas failure.
    }
  }

  Future<void> invalidate({
    required double left,
    required double top,
    required double right,
    required double bottom,
  }) async {
    if (_backend == 'noop') return;
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
