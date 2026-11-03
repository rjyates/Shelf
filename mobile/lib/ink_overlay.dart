import 'dart:async';

import 'package:flutter/gestures.dart';
import 'package:flutter/material.dart';
import 'package:flutter/scheduler.dart';

import 'eink_refresh.dart';
import 'ink_pipeline.dart';
import 'ink_stroke.dart';

/// Transparent stylus capture layer over the e-ink Readium view.
///
/// Stylus is captured via a **global pointer route** so this overlay is never
/// in the hit-test path — finger swipes still reach Readium for page turns.
/// Live ink always paints on the Flutter canvas and also posts PW segments.
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
  Size _size = Size.zero;

  bool _isStylus(PointerEvent event) =>
      event.kind == PointerDeviceKind.stylus ||
      event.kind == PointerDeviceKind.invertedStylus;

  int _nowMs(PointerEvent event) {
    final micros = event.timeStamp.inMicroseconds;
    if (micros > 0) return micros ~/ 1000;
    return DateTime.now().millisecondsSinceEpoch;
  }

  @override
  void initState() {
    super.initState();
    // Global route receives stylus without winning the Stack hit test, so
    // finger page-turns still reach the Readium WebView underneath.
    GestureBinding.instance.pointerRouter.addGlobalRoute(_onGlobalPointer);
  }

  @override
  void dispose() {
    GestureBinding.instance.pointerRouter.removeGlobalRoute(_onGlobalPointer);
    super.dispose();
  }

  void _onGlobalPointer(PointerEvent event) {
    if (!widget.enabled || !mounted) return;
    // Default: stylus only. Fingers must reach Readium for page turns.
    if (!_isStylus(event)) {
      if (!(widget.absorbTouch && _pointer == event.pointer)) return;
    }
    final box = context.findRenderObject() as RenderBox?;
    if (box == null || !box.hasSize) return;
    final local = box.globalToLocal(event.position);
    final size = box.size;
    _size = size;

    if (event is PointerDownEvent) {
      if (!_isStylus(event)) return;
      if (!(Offset.zero & size).contains(local)) return;
      _begin(event, local, size);
    } else if (event is PointerMoveEvent) {
      if (_pointer != event.pointer) return;
      _move(event, local, size);
    } else if (event is PointerUpEvent || event is PointerCancelEvent) {
      if (_pointer != event.pointer) return;
      _end(event);
    }
  }

  void _begin(PointerDownEvent event, Offset local, Size size) {
    _pointer = event.pointer;
    _tool = event.kind == PointerDeviceKind.invertedStylus
        ? 'eraser'
        : 'stylus';
    _speed.reset();
    _prevWidth = null;
    _strokeOriginMs = _nowMs(event);
    _prevLocal = local;
    _live
      ..clear()
      ..add(
        InkStroke.pointFromLocal(
          dx: local.dx,
          dy: local.dy,
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

  void _move(PointerMoveEvent event, Offset local, Size size) {
    if (_pointer != event.pointer) return;
    final origin = _strokeOriginMs ?? _nowMs(event);
    final point = InkStroke.pointFromLocal(
      dx: local.dx,
      dy: local.dy,
      width: size.width,
      height: size.height,
      pressure: event.pressure,
      tilt: event.tilt,
      orientation: event.orientation,
      timeMs: _nowMs(event) - origin,
    );
    final prev = _live.isEmpty ? null : _live.last;
    _live.add(point);
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
        final a = _prevLocal ?? local;
        unawaited(
          refresh.drawSegment(
            x0: a.dx * dpr,
            y0: a.dy * dpr,
            x1: local.dx * dpr,
            y1: local.dy * dpr,
            width: w * dpr,
            eraser: _tool == 'eraser',
          ),
        );
      }
    }
    _prevLocal = local;
    // Always paint live on Flutter — host PW alone was invisible mid-stroke
    // (panel shows the Flutter surface; host bmp posts did not replace it).
    setState(() {});
    _requestPartialRefresh(size);
  }

  void _end(PointerEvent event) {
    if (_pointer != event.pointer) return;
    final size = _size == Size.zero ? (context.size ?? Size.zero) : _size;
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
    const pad = 16.0;
    // Schedule after the frame so View.invalidate sees the new Flutter pixels.
    SchedulerBinding.instance.addPostFrameCallback((_) {
      unawaited(
        refresh.invalidate(
          left: (minX * size.width - pad) * dpr,
          top: (minY * size.height - pad) * dpr,
          right: (maxX * size.width + pad) * dpr,
          bottom: (maxY * size.height + pad) * dpr,
          minInterval: force ? Duration.zero : const Duration(milliseconds: 16),
        ),
      );
    });
  }

  @override
  Widget build(BuildContext context) {
    // IgnorePointer: never win the Stack hit test (finger paging works).
    // Stylus arrives through the global pointer route above.
    return IgnorePointer(
      child: CustomPaint(
        painter: _InkPainter(
          strokes: widget.strokes,
          live: _live,
          liveTool: _tool,
        ),
        size: Size.infinite,
      ),
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
        densifyInkPoints(stroke.points),
        stroke.tool,
        stroke.width,
        speed,
      );
    }
    if (live.length >= 2) {
      speed.reset();
      // Tighter densify for live trail so fast curves stay round mid-stroke.
      _paintStroke(
        canvas,
        size,
        densifyInkPoints(live, maxGap: 0.003),
        liveTool,
        2,
        speed,
      );
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
