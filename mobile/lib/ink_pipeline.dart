import 'dart:math' as math;

import 'ink_stroke.dart';

/// Portable stroke shaping inspired by Ratta drawpath math (not their code/libs).
///
/// Derived ideas (REA on `librecgnition.so`):
/// - `smoothstep`: Hermite `t²(3−2t)` after clamping
/// - `pressureMapping2Width`: linear map pressure∈[min,max] → width, +1px floor
/// - `procreateStyleWidth`: thin by `sin(tilt)` via smoothstep (Lamy LUT skipped)
/// - sample thinning / contour smooth: keep a min spacing + light EMA (KISS
///   stand-in for `reduceSamplePoints` / `GaussianSmoothContour`)
///
/// Skipped (OEM-only): EMR thread, EPDC/HTEINK ioctls, OpenCV painter, Wacom
/// type LUTs — Android already normalizes pressure to ~0..1 for sideloaded apps.

/// Classic smoothstep used in their width shaping (`smoothstep(float,float,float)`).
double inkSmoothstep(double edge0, double edge1, double x) {
  final span = edge1 - edge0;
  if (span.abs() < 1e-9) return x >= edge1 ? 1.0 : 0.0;
  var t = (x - edge0) / span;
  if (t < 0) t = 0;
  if (t > 1) t = 1;
  return (3.0 - (t + t)) * t * t;
}

/// Map normalized pressure to stroke width (logical px).
///
/// Mirrors `pressureMapping2Width` for p∈[0,1]:
/// `round(range * clamp(p)) + floor` with optional smoothstep on pressure.
double widthFromPressure(
  double pressure, {
  double minWidth = 1.2,
  double maxWidth = 6.0,
  bool ease = true,
}) {
  var p = pressure.clamp(0.0, 1.0);
  if (ease) p = inkSmoothstep(0, 1, p);
  final w = minWidth + p * (maxWidth - minWidth);
  return w < minWidth ? minWidth : w;
}

/// Tilt thinning from REA `procreateStyleWidth` (without nested Lamy LUT).
///
/// `factor = clamp(1 − 0.98·smoothstep(sin(tilt)), 0.1, 1)` then multiply base
/// width. Flutter/Android tilt: 0 = upright, π/2 = flat on glass.
double tiltWidthFactor(double tiltRadians) {
  var s = math.sin(tiltRadians);
  if (s < 0) s = 0;
  if (s > 1) s = 1;
  var factor = 1.0 - 0.98 * inkSmoothstep(0, 1, s);
  if (factor > 1) factor = 1;
  if (factor < 0.1) factor = 0.1;
  return factor;
}

/// Drop near-duplicate samples (normalized coords). Distance ≈ Ratta “reduce”.
List<InkPoint> thinInkPoints(
  List<InkPoint> points, {
  double minDistance = 0.002,
}) {
  if (points.length < 2) return List<InkPoint>.from(points);
  final out = <InkPoint>[points.first];
  final min2 = minDistance * minDistance;
  for (var i = 1; i < points.length; i++) {
    final prev = out.last;
    final cur = points[i];
    final dx = cur.x - prev.x;
    final dy = cur.y - prev.y;
    if (dx * dx + dy * dy >= min2 || i == points.length - 1) {
      out.add(cur);
    }
  }
  return out;
}

/// One-pass EMA on x/y/pressure (light stand-in for Gaussian contour smooth).
List<InkPoint> smoothInkPoints(List<InkPoint> points, {double alpha = 0.35}) {
  if (points.length < 3) return List<InkPoint>.from(points);
  final a = alpha.clamp(0.05, 1.0);
  final out = <InkPoint>[points.first];
  var x = points.first.x;
  var y = points.first.y;
  var p = points.first.pressure;
  for (var i = 1; i < points.length; i++) {
    final cur = points[i];
    x = a * cur.x + (1 - a) * x;
    y = a * cur.y + (1 - a) * y;
    p = a * cur.pressure + (1 - a) * p;
    out.add(
      InkPoint(
        x: x.clamp(0.0, 1.0),
        y: y.clamp(0.0, 1.0),
        pressure: p.clamp(0.0, 1.0),
        tilt: cur.tilt,
        orientation: cur.orientation,
      ),
    );
  }
  return out;
}

/// Finish a live trail: thin → smooth → keep ≥2 points.
List<InkPoint> finalizeInkPoints(List<InkPoint> raw) {
  final thinned = thinInkPoints(raw);
  final smoothed = smoothInkPoints(thinned);
  return smoothed.length >= 2 ? smoothed : thinned;
}

/// Segment width from averaged pressure, then tilt thinning.
double segmentWidth(
  InkPoint a,
  InkPoint b, {
  double minWidth = 1.2,
  double maxWidth = 6.0,
}) {
  final base = widthFromPressure(
    (a.pressure + b.pressure) * 0.5,
    minWidth: minWidth,
    maxWidth: maxWidth,
  );
  return base * tiltWidthFactor((a.tilt + b.tilt) * 0.5);
}

/// Euclidean distance in normalized space (for tests / debugging).
double inkPointDistance(InkPoint a, InkPoint b) {
  final dx = a.x - b.x;
  final dy = a.y - b.y;
  return math.sqrt(dx * dx + dy * dy);
}
