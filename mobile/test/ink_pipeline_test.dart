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
    // eased mid is still 0.5 for classic smoothstep → same width
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

  test('finalize thins and smooths a noisy trail', () {
    final raw = [
      for (var i = 0; i < 20; i++)
        InkPoint(
          x: i * 0.001,
          y: (i.isEven ? 0.0 : 0.002),
          pressure: 0.3 + i * 0.02,
        ),
    ];
    final done = finalizeInkPoints(raw);
    expect(done.length, greaterThanOrEqualTo(2));
    expect(done.length, lessThanOrEqualTo(raw.length));
    expect(done.first.x, 0);
  });

  test('segment width averages neighboring pressure', () {
    const a = InkPoint(x: 0, y: 0, pressure: 0);
    const b = InkPoint(x: 1, y: 0, pressure: 1);
    expect(
      segmentWidth(a, b, minWidth: 2, maxWidth: 4),
      closeTo(widthFromPressure(0.5, minWidth: 2, maxWidth: 4), 1e-9),
    );
  });

  test('tilt width factor thins flat stylus and keeps upright full', () {
    expect(tiltWidthFactor(0), 1.0);
    expect(tiltWidthFactor(math.pi / 2), closeTo(0.1, 1e-9));
    expect(tiltWidthFactor(math.pi / 4), lessThan(1.0));
    expect(tiltWidthFactor(math.pi / 4), greaterThan(0.1));
  });

  test('segment width applies tilt thinning on top of pressure', () {
    const upright = InkPoint(x: 0, y: 0, pressure: 1, tilt: 0);
    const flat = InkPoint(x: 1, y: 0, pressure: 1, tilt: math.pi / 2);
    final full = segmentWidth(upright, upright, minWidth: 2, maxWidth: 4);
    final thin = segmentWidth(flat, flat, minWidth: 2, maxWidth: 4);
    expect(full, closeTo(4.0, 1e-9));
    expect(thin, closeTo(4.0 * 0.1, 1e-9));
  });
}
