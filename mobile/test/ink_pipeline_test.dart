import 'dart:math' as math;

import 'package:flutter_test/flutter_test.dart';
import 'package:shelf_mobile/ink_pipeline.dart';
import 'package:shelf_mobile/ink_stroke.dart';

void main() {
  test('smoothstep is 0/0.5/1 at edges and mid', () {
    expect(inkSmoothstep(0, 1, -1), 0);
    expect(inkSmoothstep(0, 1, 0), 0);
    expect(inkSmoothstep(0, 1, 0.5), closeTo(0.5, 1e-9));
    expect(inkSmoothstep(0, 1, 1), 1);
    expect(inkSmoothstep(0, 1, 2), 1);
  });

  test('pressure maps linearly through smoothstep to width', () {
    expect(widthFromPressure(0, ease: false), 1.2);
    expect(widthFromPressure(1, ease: false), 6.0);
    expect(widthFromPressure(0.5, ease: false), closeTo(3.6, 1e-9));
    expect(widthFromPressure(0.5, ease: true), closeTo(3.6, 1e-9));
    expect(
      widthFromPressure(0.25, ease: true),
      lessThan(widthFromPressure(0.25, ease: false)),
    );
  });

  test('thin drops near duplicates but keeps endpoints', () {
    final raw = [
      const InkPoint(x: 0, y: 0, pressure: 0.5),
      const InkPoint(x: 0.001, y: 0, pressure: 0.5),
      const InkPoint(x: 0.01, y: 0, pressure: 0.8),
      const InkPoint(x: 0.0105, y: 0, pressure: 0.8),
      const InkPoint(x: 0.05, y: 0, pressure: 1),
    ];
    final thinned = thinInkPoints(raw, minDistance: 0.005);
    expect(thinned.first.x, 0);
    expect(thinned.last.x, 0.05);
    expect(thinned.length, lessThan(raw.length));
    expect(thinned.length, greaterThanOrEqualTo(2));
  });

  test('finalize thins without reshaping endpoints', () {
    final raw = [
      for (var i = 0; i < 20; i++)
        InkPoint(
          x: i * 0.001,
          y: (i.isEven ? 0.0 : 0.002),
          pressure: 0.3 + i * 0.02,
          timeMs: i * 8,
        ),
    ];
    final done = finalizeInkPoints(raw);
    expect(done.length, greaterThanOrEqualTo(2));
    expect(done.length, lessThanOrEqualTo(raw.length));
    expect(done.first.x, 0);
    expect(done.last.x, raw.last.x);
    expect(done.last.y, raw.last.y);
  });

  test('segment width averages neighboring pressure', () {
    const a = InkPoint(x: 0, y: 0, pressure: 0, timeMs: 0);
    const b = InkPoint(x: 0.00005, y: 0, pressure: 1, timeMs: 8);
    expect(
      segmentWidth(a, b, minWidth: 2, maxWidth: 4),
      closeTo(widthFromPressure(0.5, minWidth: 2, maxWidth: 4), 1e-6),
    );
  });

  test('tilt width factor thins flat stylus and keeps upright full', () {
    expect(tiltWidthFactor(0), 1.0);
    expect(tiltWidthFactor(math.pi / 2), closeTo(0.45, 1e-9));
    expect(tiltWidthFactor(math.pi / 4), lessThan(1.0));
    expect(tiltWidthFactor(math.pi / 4), greaterThan(0.35));
  });

  test('segment width applies tilt thinning on top of pressure', () {
    const upright = InkPoint(x: 0, y: 0, pressure: 1, tilt: 0, timeMs: 0);
    const flat = InkPoint(
      x: 0.00005,
      y: 0,
      pressure: 1,
      tilt: math.pi / 2,
      timeMs: 8,
    );
    final full = segmentWidth(upright, upright, minWidth: 2, maxWidth: 4);
    final thin = segmentWidth(flat, flat, minWidth: 2, maxWidth: 4);
    expect(full, closeTo(4.0, 1e-9));
    expect(thin, closeTo(4.0 * tiltWidthFactor(math.pi / 2), 1e-9));
  });

  test('densify keeps a quick circle from collapsing to a box', () {
    final sparse = [
      for (var i = 0; i < 8; i++)
        InkPoint(
          x: 0.5 + 0.2 * math.cos(i * math.pi / 4),
          y: 0.5 + 0.2 * math.sin(i * math.pi / 4),
          pressure: 0.5,
          timeMs: i * 8,
        ),
    ];
    final done = finalizeInkPoints(sparse);
    expect(done.length, greaterThan(sparse.length));
    var maxGap = 0.0;
    for (var i = 1; i < done.length; i++) {
      maxGap = math.max(maxGap, inkPointDistance(done[i - 1], done[i]));
    }
    expect(maxGap, lessThan(0.004));
  });

  test('densify Catmull-Rom keeps curve samples off the chord', () {
    // Three points of a right angle — linear densify stays on the L;
    // Catmull through the corner bows outward.
    final corner = [
      const InkPoint(x: 0, y: 0, pressure: 0.5, timeMs: 0),
      const InkPoint(x: 0.2, y: 0, pressure: 0.5, timeMs: 10),
      const InkPoint(x: 0.2, y: 0.2, pressure: 0.5, timeMs: 20),
    ];
    final dense = densifyInkPoints(corner, maxGap: 0.05);
    expect(dense.length, greaterThan(corner.length));
  });

  test('speed factor thins long jumps (REA curve)', () {
    const a = InkPoint(x: 0, y: 0, pressure: 1, timeMs: 0);
    const near = InkPoint(x: 0.0001, y: 0, pressure: 1, timeMs: 8);
    const far = InkPoint(x: 0.05, y: 0, pressure: 1, timeMs: 8);
    expect(speedWidthFactor(a, near), greaterThan(speedWidthFactor(a, far)));
    expect(speedWidthFactor(a, far), lessThan(1.0));
    expect(speedWidthFactor(a, far), greaterThanOrEqualTo(0.22));
  });

  test('speed tracker keeps a 5-sample deque', () {
    final tracker = InkSpeedTracker();
    const a = InkPoint(x: 0, y: 0, pressure: 1, timeMs: 0);
    double? last;
    for (var i = 1; i <= 8; i++) {
      last = tracker.push(
        a,
        InkPoint(x: i * 0.01, y: 0, pressure: 1, timeMs: i * 8),
      );
    }
    expect(last, lessThan(1.0));
    expect(InkSpeedTracker.window, 5);
  });

  test('getSpeedMapWidth approaches target by at most 1px', () {
    expect(getSpeedMapWidth(6, 2), 3);
    expect(getSpeedMapWidth(2.5, 2), closeTo(2.5, 1e-9));
    expect(getSpeedMapWidth(2, 5), 4);
    expect(getSpeedMapWidth(1, 2), 2);
  });

  test('azimuth brush thickens when nib is across the stroke', () {
    // Travel +x; azimuth π/2 → across → thicker than aligned (0).
    const across = InkPoint(x: 0, y: 0, pressure: 1, orientation: math.pi / 2);
    const acrossEnd = InkPoint(
      x: 0.01,
      y: 0,
      pressure: 1,
      orientation: math.pi / 2,
    );
    const aligned = InkPoint(x: 0, y: 0, pressure: 1, orientation: 0);
    const alignedEnd = InkPoint(x: 0.01, y: 0, pressure: 1, orientation: 0);
    expect(
      azimuthBrushFactor(across, acrossEnd),
      greaterThan(azimuthBrushFactor(aligned, alignedEnd)),
    );
  });
}
