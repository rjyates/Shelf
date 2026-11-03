import 'dart:collection';
import 'dart:math' as math;

import 'ink_stroke.dart';

/// Portable stroke shaping inspired by Ratta drawpath math (not their code/libs).
///
/// Derived ideas (REA on `librecgnition.so`):
/// - `smoothstep`: Hermite `t²(3−2t)` after clamping
/// - `pressureMapping2Width`: **linear** map pressure∈[min,max] → width (+floor)
/// - `procreateStyleWidth`: thin by `sin(tilt)` via smoothstep (Lamy LUT skipped)
/// - `calculateSmoothedSpeedFactor`: 5-sample deque of `dist*0.25/0.0025`
/// - `getSpeedMapWidth`: approach target width by at most 1px/sample (EMA stand-in)
/// - azimuth × brush direction for chisel tips
/// - densify large MotionEvent gaps (fast circle→box)
///
/// Skipped (OEM-only): EMR thread, OpenCV painter, Wacom type LUTs.

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
double widthFromPressure(
  double pressure, {
  double minWidth = 1.2,
  double maxWidth = 6.0,
  bool ease = false,
}) {
  var p = pressure.clamp(0.0, 1.0);
  if (ease) p = inkSmoothstep(0, 1, p);
  final w = minWidth + p * (maxWidth - minWidth);
  return w < minWidth ? minWidth : w;
}

/// Tilt thinning from REA `procreateStyleWidth` (without nested Lamy LUT).
double tiltWidthFactor(double tiltRadians, {double strength = 0.55}) {
  var s = math.sin(tiltRadians);
  if (s < 0) s = 0;
  if (s > 1) s = 1;
  var factor = 1.0 - strength * inkSmoothstep(0, 1, s);
  if (factor > 1) factor = 1;
  if (factor < 0.35) factor = 0.35;
  return factor;
}

/// Chisel / calligraphy factor from stylus azimuth vs stroke direction.
///
/// Thickest when the flat of the nib is across the stroke (angle near π/2),
/// thinnest when aligned with travel. REA logged this; width return unused it.
double azimuthBrushFactor(InkPoint a, InkPoint b, {double strength = 0.35}) {
  final az = (a.orientation + b.orientation) * 0.5;
  // No chisel without a real azimuth reading (keeps upright pen feel neutral).
  if (az.abs() < 1e-6) return 1.0;
  final dx = b.x - a.x;
  final dy = b.y - a.y;
  if (dx * dx + dy * dy < 1e-12) return 1.0;
  final travel = math.atan2(dy, dx);
  var diff = (az - travel).abs() % math.pi;
  if (diff > math.pi / 2) diff = math.pi - diff;
  // 0 = aligned (thin), π/2 = across (thick); centered on 1.0
  final across = (diff / (math.pi / 2)).clamp(0.0, 1.0);
  return 1.0 + strength * (across - 0.5);
}

/// Instantaneous Nomad-style speed sample: `(distPx * 0.25) / dtSec`.
///
/// Their code hard-coded `dt = 0.0025`; with timestamps we use real Δt floored
/// at 2.5ms so a stalled sample does not explode.
double inkSpeedSample(InkPoint a, InkPoint b, {double refPixels = 1400}) {
  final distPx = inkPointDistance(a, b) * refPixels;
  var dtSec = 0.0025;
  final ta = a.timeMs;
  final tb = b.timeMs;
  if (ta != null && tb != null && tb > ta) {
    dtSec = ((tb - ta) / 1000.0).clamp(0.0025, 0.05);
  }
  return (distPx * 0.25) / dtSec;
}

/// Rolling 5-sample speed → width factor (`calculateSmoothedSpeedFactor`).
class InkSpeedTracker {
  final ListQueue<double> _samples = ListQueue<double>();
  static const int window = 5;

  void reset() => _samples.clear();

  /// Push the segment [a]→[b] and return the smoothed width factor ∈ [0.22, 1].
  double push(InkPoint a, InkPoint b, {double refPixels = 1400}) {
    _samples.addLast(inkSpeedSample(a, b, refPixels: refPixels));
    while (_samples.length > window) {
      _samples.removeFirst();
    }
    return factorFromMean(_mean);
  }

  double get _mean {
    if (_samples.isEmpty) return 50.0;
    var sum = 0.0;
    for (final s in _samples) {
      sum += s;
    }
    return sum / _samples.length;
  }

  /// REA curve: clamp mean to [50, 6500], `1 − 0.78·ln(1+7t)/ln(8)`.
  static double factorFromMean(double mean) {
    final clamped = mean.clamp(50.0, 6500.0);
    final t = ((clamped - 50.0) / 6450.0).clamp(0.0, 1.0);
    final logTerm = math.log(t * 7.0 + 1.0) / 2.0794415416798357; // ln(8)
    final factor = 1.0 - 0.78 * logTerm;
    if (factor < 0.22) return 0.22;
    if (factor > 1) return 1;
    return factor;
  }
}

/// Stateless speed factor (single segment) — prefer [InkSpeedTracker] live.
double speedWidthFactor(InkPoint a, InkPoint b, {double refPixels = 1400}) {
  return InkSpeedTracker.factorFromMean(
    inkSpeedSample(a, b, refPixels: refPixels),
  );
}

/// REA `getSpeedMapWidth`: walk previous width toward [target] by ≤1px/sample.
double getSpeedMapWidth(double target, double previous) {
  final goal = target < 2 ? 2.0 : target;
  final prev = previous < 2 ? 2.0 : previous;
  final delta = goal - prev;
  if (delta.abs() <= 1e-9) return prev;
  final step = delta.abs() < 1 ? delta : delta.sign;
  final next = prev + step;
  if (next < 2) return 2;
  return next;
}

/// Drop near-duplicate samples (normalized coords).
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

/// Catmull-Rom interpolate between [a]→[b] using neighbors [p0]/[p3].
InkPoint _catmull(InkPoint p0, InkPoint a, InkPoint b, InkPoint p3, double t) {
  final t2 = t * t;
  final t3 = t2 * t;
  double axis(double v0, double v1, double v2, double v3) =>
      0.5 *
      ((2 * v1) +
          (-v0 + v2) * t +
          (2 * v0 - 5 * v1 + 4 * v2 - v3) * t2 +
          (-v0 + 3 * v1 - 3 * v2 + v3) * t3);
  final ta = a.timeMs;
  final tb = b.timeMs;
  return InkPoint(
    x: axis(p0.x, a.x, b.x, p3.x).clamp(0.0, 1.0),
    y: axis(p0.y, a.y, b.y, p3.y).clamp(0.0, 1.0),
    pressure: (a.pressure + (b.pressure - a.pressure) * t).clamp(0.0, 1.0),
    tilt: a.tilt + (b.tilt - a.tilt) * t,
    orientation: a.orientation + (b.orientation - a.orientation) * t,
    timeMs: (ta != null && tb != null) ? (ta + ((tb - ta) * t).round()) : null,
  );
}

/// Insert samples when MotionEvent gaps are large (quick curves → chords).
///
/// Uses Catmull-Rom when neighbors exist so fast circles stay round instead of
/// boxy; falls back to linear on endpoints. Default [maxGap] tightened after
/// Nomad smoke (0.006 still left visible flats).
List<InkPoint> densifyInkPoints(
  List<InkPoint> points, {
  double maxGap = 0.003,
}) {
  if (points.length < 2) return List<InkPoint>.from(points);
  final out = <InkPoint>[points.first];
  for (var i = 1; i < points.length; i++) {
    final a = points[i - 1];
    final b = points[i];
    final d = inkPointDistance(a, b);
    if (d > maxGap) {
      final steps = (d / maxGap).ceil().clamp(2, 96);
      final p0 = i >= 2 ? points[i - 2] : a;
      final p3 = i + 1 < points.length ? points[i + 1] : b;
      for (var s = 1; s < steps; s++) {
        final t = s / steps;
        out.add(_catmull(p0, a, b, p3, t));
      }
    }
    out.add(b);
  }
  return out;
}

/// One-pass EMA on x/y/pressure (opt-in; not used on finalize — morphs shapes).
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
        timeMs: cur.timeMs,
      ),
    );
  }
  return out;
}

/// Finish a live trail: densify sparse gaps, drop only near-duplicates.
List<InkPoint> finalizeInkPoints(List<InkPoint> raw) {
  final dense = densifyInkPoints(raw);
  final thinned = thinInkPoints(dense, minDistance: 0.0004);
  return thinned.length >= 2 ? thinned : List<InkPoint>.from(raw);
}

/// Segment width from pressure + tilt + speed + azimuth, with width EMA.
double segmentWidth(
  InkPoint a,
  InkPoint b, {
  double minWidth = 1.2,
  double maxWidth = 6.0,
  InkSpeedTracker? speed,
  double? previousWidth,
}) {
  final base = widthFromPressure(
    (a.pressure + b.pressure) * 0.5,
    minWidth: minWidth,
    maxWidth: maxWidth,
  );
  final speedFactor = speed != null ? speed.push(a, b) : speedWidthFactor(a, b);
  final raw =
      base *
      tiltWidthFactor((a.tilt + b.tilt) * 0.5) *
      speedFactor *
      azimuthBrushFactor(a, b);
  if (previousWidth == null) return raw;
  return getSpeedMapWidth(raw, previousWidth);
}

/// Euclidean distance in normalized space (for tests / debugging).
double inkPointDistance(InkPoint a, InkPoint b) {
  final dx = a.x - b.x;
  final dy = a.y - b.y;
  return math.sqrt(dx * dx + dy * dy);
}
