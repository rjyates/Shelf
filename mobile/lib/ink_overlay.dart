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
  var _tool = 'stylus';
  int? _pointer;

  bool _isStylus(PointerEvent event) =>
      event.kind == PointerDeviceKind.stylus ||
      event.kind == PointerDeviceKind.invertedStylus;

  void _begin(PointerDownEvent event, Size size) {
    if (!widget.enabled) return;
    if (!_isStylus(event) && !widget.absorbTouch) return;
    if (!_isStylus(event)) return;
    _pointer = event.pointer;
    _tool = event.kind == PointerDeviceKind.invertedStylus
        ? 'eraser'
        : 'stylus';
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
        ),
      );
    setState(() {});
  }

  void _move(PointerMoveEvent event, Size size) {
    if (_pointer != event.pointer) return;
    _live.add(
      InkStroke.pointFromLocal(
        dx: event.localPosition.dx,
        dy: event.localPosition.dy,
        width: size.width,
        height: size.height,
        pressure: event.pressure,
        tilt: event.tilt,
        orientation: event.orientation,
      ),
    );
    setState(() {});
    _requestPartialRefresh(size);
  }

  void _end(PointerEvent event) {
    if (_pointer != event.pointer) return;
    final size = context.size ?? Size.zero;
    final points = finalizeInkPoints(_live);
    _pointer = null;
    _live.clear();
    setState(() {});
    if (points.length < 2) return;
    _requestPartialRefresh(size, points: points);
    widget.onStrokeComplete(InkStroke(points: points, tool: _tool));
  }

  void _requestPartialRefresh(Size size, {List<InkPoint>? points}) {
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
              live: _live,
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
    for (final stroke in strokes) {
      _paintStroke(canvas, size, stroke.points, stroke.tool, stroke.width);
    }
    if (live.length >= 2) {
      _paintStroke(canvas, size, live, liveTool, 2);
    }
  }

  void _paintStroke(
    Canvas canvas,
    Size size,
    List<InkPoint> points,
    String tool,
    double width,
  ) {
    // shortcut: constant width fallback for empty pressure; upgrade when
    // storing per-segment widths in the annotation payload.
    final color = tool == 'eraser'
        ? const Color(0xffffffff)
        : const Color(0xff111111);
    final paint = Paint()
      ..color = color
      ..strokeCap = StrokeCap.round
      ..strokeJoin = StrokeJoin.round
      ..style = PaintingStyle.stroke
      ..isAntiAlias = false;
    for (var i = 1; i < points.length; i++) {
      final a = points[i - 1];
      final b = points[i];
      paint.strokeWidth = segmentWidth(
        a,
        b,
        minWidth: width * 0.6,
        maxWidth: width * 2.5,
      );
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
