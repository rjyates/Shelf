import 'package:flutter/gestures.dart';
import 'package:flutter/material.dart';

import 'eink_refresh.dart';
import 'ink_pipeline.dart';
import 'ink_stroke.dart';

/// Stylus capture / paint over the e-ink Readium view.
///
/// On accelerated Nomad: native [InkHostView] owns the live trail; this overlay
/// only paints **committed** strokes (and skips the global stylus route).
/// On noop / LCD: stylus is captured here via a global pointer route.
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

  bool get _nativeOwnsPen =>
      widget.refresh != null && widget.refresh!.isAccelerated;

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
    GestureBinding.instance.pointerRouter.addGlobalRoute(_onGlobalPointer);
  }

  @override
  void didUpdateWidget(covariant InkOverlay oldWidget) {
    super.didUpdateWidget(oldWidget);
    if (widget.strokes.length > oldWidget.strokes.length && _live.isNotEmpty) {
      _live.clear();
    }
    if (!widget.enabled && _live.isNotEmpty) {
      _live.clear();
    }
  }

  @override
  void dispose() {
    GestureBinding.instance.pointerRouter.removeGlobalRoute(_onGlobalPointer);
    super.dispose();
  }

  void _onGlobalPointer(PointerEvent event) {
    if (!widget.enabled || !mounted) return;
    // Accelerated: native consumes + paints live; do not double-handle.
    if (_nativeOwnsPen) return;
    if (!_isStylus(event)) {
      if (!(widget.absorbTouch && _pointer == event.pointer)) return;
    }
    final box = context.findRenderObject() as RenderBox?;
    if (box == null || !box.hasSize) return;
    final local = box.globalToLocal(event.position);
    final size = box.size;

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
    }
    setState(() {});
  }

  void _end(PointerEvent event) {
    if (_pointer != event.pointer) return;
    final points = finalizeInkPoints(_live);
    _pointer = null;
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
    widget.onStrokeComplete(InkStroke(points: points, tool: _tool));
  }

  @override
  Widget build(BuildContext context) {
    return IgnorePointer(
      child: CustomPaint(
        painter: _InkPainter(
          strokes: widget.strokes,
          // Native path owns the live trail on InkHostView.
          live: _nativeOwnsPen ? const [] : List<InkPoint>.of(_live),
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
        // Match native live host: linear pressure width, no densify fattening.
        stroke.points,
        stroke.tool,
        stroke.width,
        speed,
        matchNativeLive: true,
      );
    }
    if (live.length >= 2) {
      speed.reset();
      _paintStroke(
        canvas,
        size,
        densifyInkPoints(live, maxGap: 0.003),
        liveTool,
        2,
        speed,
        matchNativeLive: false,
      );
    }
  }

  void _paintStroke(
    Canvas canvas,
    Size size,
    List<InkPoint> points,
    String tool,
    double width,
    InkSpeedTracker speed, {
    required bool matchNativeLive,
  }) {
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
      final w = matchNativeLive
          // Same formula as EinkRefreshPlugin: 1.2 + p·3.8
          ? (1.2 + ((a.pressure + b.pressure) * 0.5).clamp(0.0, 1.0) * 3.8)
                .clamp(1.2, 5.0)
          : segmentWidth(
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
  bool shouldRepaint(covariant _InkPainter oldDelegate) =>
      !identical(strokes, oldDelegate.strokes) ||
      !identical(live, oldDelegate.live) ||
      liveTool != oldDelegate.liveTool;
}
