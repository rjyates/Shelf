import 'dart:async';

import 'package:flutter/gestures.dart';
import 'package:flutter/material.dart';

import 'eink_refresh.dart';
import 'ink_pipeline.dart';
import 'ink_stroke.dart';

/// Transparent stylus capture layer over the e-ink Readium view.
///
/// Only [PointerDeviceKind.stylus] / [PointerDeviceKind.invertedStylus] draw;
/// finger touches pass through for page turns when [absorbTouch] is false.
///
/// On Nomad (`EinkRefresh.hostLive`), live ink is drawn into the PW host
/// bitmap (Notes path) so the panel follows within a frame; Flutter paint
/// remains the committed layer after lift.
class InkOverlay extends StatefulWidget {
  const InkOverlay({
    super.key,
    required this.enabled,
    required this.strokes,
    required this.onStrokeComplete,
    this.refresh,
    this.absorbTouch = false,
  });

  final bool enabled;
  final List<InkStroke> strokes;
  final ValueChanged<InkStroke> onStrokeComplete;
  final EinkRefresh? refresh;

  /// When true, also claim touch pointers (blocks Readium paging under the overlay).
  final bool absorbTouch;

  @override
  State<InkOverlay> createState() => _InkOverlayState();
}

class _InkOverlayState extends State<InkOverlay> {
  final _live = <InkPoint>[];
  final _speed = InkSpeedTracker();
  var _tool = 'stylus';
  int? _pointer;
  int? _strokeOriginMs;
  double? _prevWidth;
  Offset? _prevLocal;

  bool _isStylus(PointerEvent event) =>
      event.kind == PointerDeviceKind.stylus ||
      event.kind == PointerDeviceKind.invertedStylus;

  int _nowMs(PointerEvent event) {
    final micros = event.timeStamp.inMicroseconds;
    if (micros > 0) return micros ~/ 1000;
    return DateTime.now().millisecondsSinceEpoch;
  }

  void _begin(PointerDownEvent event, Size size) {
    if (!widget.enabled) return;
    if (!_isStylus(event) && !widget.absorbTouch) return;
    if (!_isStylus(event)) return;
    _pointer = event.pointer;
    _tool = event.kind == PointerDeviceKind.invertedStylus
        ? 'eraser'
        : 'stylus';
    _speed.reset();
    _prevWidth = null;
    _strokeOriginMs = _nowMs(event);
    _prevLocal = event.localPosition;
    _live
      ..clear()
      ..add(
        InkStroke.pointFromLocal(
          dx: event.localPosition.dx,
          dy: event.localPosition.dy,
          width: size.width,
          height: size.height,
          pressure: event.pressure,
          tilt: event.tilt,
          orientation: event.orientation,
          timeMs: 0,
        ),
      );
    setState(() {});
  }

  void _move(PointerMoveEvent event, Size size) {
    if (_pointer != event.pointer) return;
    final origin = _strokeOriginMs ?? _nowMs(event);
    final point = InkStroke.pointFromLocal(
      dx: event.localPosition.dx,
      dy: event.localPosition.dy,
      width: size.width,
      height: size.height,
      pressure: event.pressure,
      tilt: event.tilt,
      orientation: event.orientation,
      timeMs: _nowMs(event) - origin,
    );
    final prev = _live.isEmpty ? null : _live.last;
    _live.add(point);
    // Host-bmp PW path: draw the newest segment natively (Nomad Notes latency).
    if (prev != null) {
      final w = segmentWidth(
        prev,
        point,
        minWidth: 1.2,
        maxWidth: 5.0,
        speed: _speed,
        previousWidth: _prevWidth,
      );
      _prevWidth = w;
      final refresh = widget.refresh;
      final dpr = MediaQuery.devicePixelRatioOf(context);
      if (refresh != null && refresh.isAccelerated) {
        final a = _prevLocal ?? event.localPosition;
        final b = event.localPosition;
        unawaited(
          refresh.drawSegment(
            x0: a.dx * dpr,
            y0: a.dy * dpr,
            x1: b.dx * dpr,
            y1: b.dy * dpr,
            width: w * dpr,
            eraser: _tool == 'eraser',
          ),
        );
      }
    }
    _prevLocal = event.localPosition;
    // Flutter paint stays as committed/fallback; skip mid-stroke setState when
    // host PW is carrying the live trail (cuts the 0.5s Canvas lag).
    if (widget.refresh?.hostLive != true) {
      setState(() {});
      _requestPartialRefresh(size);
    }
  }

  void _end(PointerEvent event) {
    if (_pointer != event.pointer) return;
    final size = context.size ?? Size.zero;
    final points = finalizeInkPoints(_live);
    _pointer = null;
    _prevLocal = null;
    _speed.reset();
    _prevWidth = null;
    if (points.length < 2) {
      _live.clear();
      setState(() {});
      return;
    }
    // Keep the finished samples painted until the parent adds them to
    // [strokes] (avoids a blank flash on slow e-ink).
    _live
      ..clear()
      ..addAll(points);
    setState(() {});
    _requestPartialRefresh(size, points: points, force: true);
    widget.onStrokeComplete(InkStroke(points: points, tool: _tool));
  }

  @override
  void didUpdateWidget(covariant InkOverlay oldWidget) {
    super.didUpdateWidget(oldWidget);
    if (widget.strokes.length > oldWidget.strokes.length && _live.isNotEmpty) {
      _live.clear();
    }
  }

  void _requestPartialRefresh(
    Size size, {
    List<InkPoint>? points,
    bool force = false,
  }) {
    final refresh = widget.refresh;
    if (refresh == null || !refresh.isAccelerated || size.isEmpty) return;
    final pts = points ?? _live;
    if (pts.isEmpty) return;
    var minX = pts.first.x;
    var maxX = pts.first.x;
    var minY = pts.first.y;
    var maxY = pts.first.y;
    for (final p in pts) {
      if (p.x < minX) minX = p.x;
      if (p.x > maxX) maxX = p.x;
      if (p.y < minY) minY = p.y;
      if (p.y > maxY) maxY = p.y;
    }
    final dpr = MediaQuery.devicePixelRatioOf(context);
    const pad = 12.0;
    unawaited(
      refresh.invalidate(
        left: (minX * size.width - pad) * dpr,
        top: (minY * size.height - pad) * dpr,
        right: (maxX * size.width + pad) * dpr,
        bottom: (maxY * size.height + pad) * dpr,
        minInterval: force ? Duration.zero : const Duration(milliseconds: 16),
      ),
    );
  }

  @override
  Widget build(BuildContext context) {
    return LayoutBuilder(
      builder: (context, constraints) {
        final size = Size(constraints.maxWidth, constraints.maxHeight);
        return Listener(
          behavior: widget.enabled
              ? HitTestBehavior.translucent
              : HitTestBehavior.deferToChild,
          onPointerDown: (event) => _begin(event, size),
          onPointerMove: (event) => _move(event, size),
          onPointerUp: _end,
          onPointerCancel: _end,
          child: CustomPaint(
            painter: _InkPainter(
              strokes: widget.strokes,
              // When host PW is live, skip mid-stroke Flutter paint (native
              // bitmap already shows the trail); still paint after lift.
              live: widget.refresh?.hostLive == true && _pointer != null
                  ? const []
                  : _live,
              liveTool: _tool,
            ),
            size: Size.infinite,
          ),
        );
      },
    );
  }
}

class _InkPainter extends CustomPainter {
  _InkPainter({
    required this.strokes,
    required this.live,
    required this.liveTool,
  });

  final List<InkStroke> strokes;
  final List<InkPoint> live;
  final String liveTool;

  @override
  void paint(Canvas canvas, Size size) {
    final speed = InkSpeedTracker();
    for (final stroke in strokes) {
      speed.reset();
      _paintStroke(
        canvas,
        size,
        stroke.points,
        stroke.tool,
        stroke.width,
        speed,
      );
    }
    if (live.length >= 2) {
      speed.reset();
      _paintStroke(canvas, size, densifyInkPoints(live), liveTool, 2, speed);
    }
  }

  void _paintStroke(
    Canvas canvas,
    Size size,
    List<InkPoint> points,
    String tool,
    double width,
    InkSpeedTracker speed,
  ) {
    final color = tool == 'eraser'
        ? const Color(0xffffffff)
        : const Color(0xff111111);
    final paint = Paint()
      ..color = color
      ..strokeCap = StrokeCap.round
      ..strokeJoin = StrokeJoin.round
      ..style = PaintingStyle.stroke
      ..isAntiAlias = false;
    double? prevW;
    for (var i = 1; i < points.length; i++) {
      final a = points[i - 1];
      final b = points[i];
      final w = segmentWidth(
        a,
        b,
        minWidth: width * 0.6,
        maxWidth: width * 2.5,
        speed: speed,
        previousWidth: prevW,
      );
      prevW = w;
      paint.strokeWidth = w;
      canvas.drawLine(
        Offset(a.x * size.width, a.y * size.height),
        Offset(b.x * size.width, b.y * size.height),
        paint,
      );
    }
  }

  @override
  bool shouldRepaint(covariant _InkPainter oldDelegate) => true;
}
